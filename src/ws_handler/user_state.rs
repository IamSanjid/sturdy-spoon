use std::{
    borrow::Cow,
    fmt::Write,
    net::SocketAddr,
    ops::{ControlFlow, Deref},
    str::FromStr,
};

use futures_util::StreamExt;
use serde_json::json;
use thiserror::Error;
use tokio::sync::{mpsc, RwLock};

use super::{
    ws_state::WsState, BMsgSender, Permission, VideoData, WSMsgSender, MAX_VIDEO_LEN,
    PERMISSION_ALL, PERMISSION_CONTROLLABLE, STATE_MAX,
};
use crate::{
    basic_auth::OwnerAuth,
    common::Id,
    sturdy_ws::{CloseFrame, Message, WebSocket, WebSocketMessage},
    ws_handler::{
        ws_state::WebSocketStateError, CLIENT_TIMEOUT, STATE_PAUSE, STATE_PLAY, SYNC_TIMEOUT,
    },
};

const MAX_MISSED_PING: usize = 2;

#[derive(Debug, Error)]
enum ValidationError {
    #[error("The specified room doesn't exist.")]
    NoRoom,
    #[error("The specified room is full.")]
    RoomFull,
    #[error("Invalid packet structure.")]
    InvalidPacket,
}

impl From<WebSocketStateError> for ValidationError {
    fn from(err: WebSocketStateError) -> Self {
        match err {
            WebSocketStateError::NoRoom => ValidationError::NoRoom,
            WebSocketStateError::RoomFull => ValidationError::RoomFull,
            _ => ValidationError::InvalidPacket,
        }
    }
}

#[derive(Clone)]
#[allow(unused)]
pub(super) struct UserState {
    pub id: Id,
    pub tx: WSMsgSender,
}

pub struct LocalUser {
    pub name: String,
    pub id: Id,
    pub room_id: Id,
    pub permission: Permission,
}

pub struct StringPacket {
    packet_type: String,
    args: String,
    header: Cow<'static, str>,
    type_sep: Cow<'static, str>,
    arg_sep: Cow<'static, str>,
}

impl StringPacket {
    pub fn new<S: Into<String>>(packet_type: S) -> Self {
        Self {
            packet_type: packet_type.into(),
            args: String::new(),
            header: Cow::Borrowed("||-=-||"),
            type_sep: Cow::Borrowed("-=-"),
            arg_sep: Cow::Borrowed("|.|"),
        }
    }

    #[allow(unused)]
    pub fn arg_sep<S: Into<String>>(mut self, sep: S) -> Self {
        self.arg_sep = Cow::Owned(sep.into());
        self
    }

    pub fn arg<S: AsRef<str>>(mut self, value: S) -> Self {
        if self.args.len() == 0 {
            self.args += value.as_ref();
        } else {
            let _ = write!(&mut self.args, "{}{}", self.arg_sep, value.as_ref());
        }
        self
    }
}

impl Into<String> for StringPacket {
    fn into(self) -> String {
        format!(
            "{}{}{}{}",
            self.header, self.packet_type, self.type_sep, self.args
        )
    }
}

impl Into<WebSocketMessage> for StringPacket {
    fn into(self) -> WebSocketMessage {
        WebSocketMessage::Text(self.into())
    }
}

fn video_data_json(data: &VideoData, permission: usize) -> String {
    json!({
        "url": data.get_url(),
        "cc_url": data.get_cc_url(),
        "time": data.get_time(),
        "state": data.get_state(),
        "current_player": data.get_current_player(),
        "permission": permission
    })
    .to_string()
}

// TODO: Implement something like `StringPacket`.
fn check_str_packet<'a>(input_str: &'a str) -> Option<(&'a str, &'a str)> {
    let Some(input_str) = input_str.strip_prefix("||-=-||") else { return None; };
    let mut full_data = input_str.split("-=-");
    let Some(data_type) = full_data.next() else { return None; };
    let Some(data) = full_data.next() else { return None; };
    Some((data_type, data))
}

async fn verify_join_msg(
    msg: Message,
    ws_state: &'static WsState,
    who: &SocketAddr,
) -> Result<LocalUser, ValidationError> {
    match msg {
        Message::Text(t) => {
            println!(">>> {}: {}", who, &t);
            let Some((data_type, data)) = check_str_packet(&t) else { return Err(ValidationError::InvalidPacket); };
            match data_type {
                "join_room" => {
                    let mut data = data.split("|.|");
                    let Some(room_id) = data.next() else { return Err(ValidationError::InvalidPacket); };
                    let Some(name) = data.next() else { return Err(ValidationError::InvalidPacket); };

                    let Ok(room_id) = Id::from_str(room_id) else { return Err(ValidationError::InvalidPacket); };

                    println!("Trying to join room: {}", room_id);

                    let room_data = ws_state
                        .get_room_data(room_id)
                        .map_err(ValidationError::from)?;
                    let room_data = room_data.read().await;
                    return ws_state
                        .join_room(room_id, name.to_owned(), room_data.permission)
                        .map_err(ValidationError::from);
                }
                _ => return Err(ValidationError::InvalidPacket),
            }
        }
        _ => Err(ValidationError::InvalidPacket),
    }
}

async fn user_handle(
    socket: WebSocket,
    who: SocketAddr,
    mut local_data: LocalUser,
    ws_state: &'static WsState,
) {
    let id = local_data.id;

    let (dm_tx, mut dm_rx) = mpsc::unbounded_channel();
    let user = UserState {
        id,
        tx: dm_tx.clone(),
    };
    let _ = ws_state.users.insert_async(id, user).await;

    let name = local_data.name.clone();
    let current_room_id = local_data.room_id;
    let Some((exit_noti, data, broadcast_tx)) = ws_state.rooms
        .read(&current_room_id, |_, v| {
            (v.exit_notify.clone(), v.data.clone(), v.broadcast_tx.clone())
        }) else {
            return;
        };

    let msg: WebSocketMessage = StringPacket::new("joined")
        .arg(name.clone())
        .arg(id.to_string())
        .into();
    let _ = broadcast_tx.send(msg.into_server_shared_bytes());

    let r_data = data.read().await;
    let data_str = StringPacket::new("video_data").arg(video_data_json(
        r_data.deref(),
        local_data.permission.into(),
    ));
    let _ = dm_tx.send(data_str.into());
    drop(r_data);

    // get the notified future before starting the tasks..
    let exit_notif = exit_noti.notified();
    let mut broadcast_rx = broadcast_tx.subscribe();

    let (socket, mut receiver) = socket.sock_split();
    let mut send_task = tokio::spawn(async move {
        loop {
            let msg = tokio::select! {
                msg = dm_rx.recv() => {
                    let Some(msg) = msg else {
                        break;
                    };
                    msg.into_server_shared_bytes()
                },
                msg = broadcast_rx.recv() => {
                    let Ok(msg) = msg else {
                        break;
                    };
                    msg
                }
            };
            let mut socket = socket.lock().await;
            unsafe {
                // We trust ourselves :")
                if let Err(err) = socket.send_raw(msg.as_ref()) {
                    match err {
                        crate::sturdy_ws::Error::Io(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock => {}
                        _ => break,
                    }
                }
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        let mut missed_pings = 0;
        loop {
            let msg = tokio::select! {
                msg = receiver.next() => {
                    let Some(Ok(msg)) = msg else {
                        break;
                    };
                    Some(msg)
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(CLIENT_TIMEOUT)) => None
            };
            if let Ok(read_data) = data.try_read() {
                drop(read_data); // Must drop it otherwise dead lock...
                data.write().await.update_time();
            }
            if let Some(msg) = msg {
                let ControlFlow::Continue(processed) = process_message(msg, (&broadcast_tx, &dm_tx), &data, &mut local_data).await else {
                    break;
                };
                if processed {
                    missed_pings = 0;
                }
                continue;
            }
            missed_pings += 1;
            if missed_pings >= MAX_MISSED_PING {
                break;
            }
        }
    });

    tokio::select! {
        _ = (&mut recv_task) => send_task.abort(),
        _ = (&mut send_task) => recv_task.abort(),
        _ = exit_notif => {
            send_task.abort();
            recv_task.abort();
        }
    }

    let Some(users_remained) = ws_state.rooms.read(&current_room_id, |_, v| v.user_left()) else {
        return;
    };

    if !users_remained {
        let _ = ws_state.close_room(current_room_id);
    } else {
        ws_state.rooms.read(&current_room_id, |_, v| {
            let msg: WebSocketMessage = StringPacket::new("left")
                .arg(name)
                .arg(id.to_string())
                .into();

            v.broadcast_tx.send(msg.into_server_shared_bytes())
        });
    }

    ws_state.users.remove_async(&id).await;
    println!("{} left! - id: {}", who, id);
}

#[inline(always)]
fn check_permission_or_send_current<'a>(
    dm_tx: &'a WSMsgSender,
    user: &'a LocalUser,
    state_data: &'a RwLock<VideoData>,
) -> impl std::future::Future<Output = ControlFlow<Option<String>, bool>> + 'a {
    async {
        if user.permission.has_permission(PERMISSION_CONTROLLABLE) {
            return ControlFlow::Continue(true);
        }
        let data = state_data.read().await;
        let data_str = StringPacket::new("video_data")
            .arg(video_data_json(data.deref(), user.permission.into()));
        if let Err(err) = dm_tx.send(data_str.into()) {
            return ControlFlow::Break(Some(err.to_string()));
        };
        return ControlFlow::Continue(false);
    }
}

#[inline(always)]
fn parse_time<'a>(mut data: impl Iterator<Item = &'a str>) -> ControlFlow<bool, usize> {
    let Some(time) = data.next() else {
        return ControlFlow::Break(false);
    };
    let Ok(time) = time.parse::<f32>() else {
        return ControlFlow::Break(false);
    };
    let time = (time * 1000f32).floor() as usize;
    if time > MAX_VIDEO_LEN {
        return ControlFlow::Break(true);
    }
    return ControlFlow::Continue(time);
}

#[inline(always)]
fn parse_state<'a>(mut data: impl Iterator<Item = &'a str>) -> ControlFlow<bool, usize> {
    let Some(state) = data.next() else {
        return ControlFlow::Break(false);
    };
    let Ok(state) = state.parse::<usize>() else {
        return ControlFlow::Break(false);
    };
    if state > STATE_MAX {
        return ControlFlow::Break(true);
    }
    return ControlFlow::Continue(state);
}

async fn process_message(
    msg: Message,
    (broadcast_tx, dm_tx): (&BMsgSender, &WSMsgSender),
    state_data: &RwLock<VideoData>,
    local_data: &mut LocalUser,
) -> ControlFlow<Option<String>, bool> {
    // TODO: make permission `settable` for every user individually...
    match msg {
        Message::Text(input_str) => {
            println!(">>> {} sent str: {:?}", local_data.name, input_str);
            let Some((data_type, data)) = check_str_packet(&input_str) else { return ControlFlow::Continue(false); };

            match data_type {
                "state" => {
                    let mut data = data.split("|.|");
                    let time = match parse_time(&mut data) {
                        ControlFlow::Break(con) => return ControlFlow::Continue(!con),
                        ControlFlow::Continue(time) => time,
                    };

                    let video_state = match parse_state(&mut data) {
                        ControlFlow::Break(con) => return ControlFlow::Continue(!con),
                        ControlFlow::Continue(state) => state,
                    };

                    let read_state_data = state_data.read().await;
                    let needs_update = video_state != read_state_data.get_state()
                        || ((time as isize).abs_diff(read_state_data.get_time() as isize) as u128)
                            > SYNC_TIMEOUT;
                    if needs_update {
                        if local_data
                            .permission
                            .has_permission(PERMISSION_CONTROLLABLE)
                        {
                            {
                                drop(read_state_data);
                                let mut state_data = state_data.write().await;
                                state_data.set_state(time, video_state);
                            }

                            let msg: WebSocketMessage = StringPacket::new("state")
                                .arg(time.to_string())
                                .arg(video_state.to_string())
                                .into();
                            if let Err(err) = broadcast_tx.send(msg.into_server_shared_bytes()) {
                                return ControlFlow::Break(Some(err.to_string()));
                            }
                        } else {
                            let msg = StringPacket::new("state")
                                .arg(read_state_data.get_time().to_string())
                                .arg(read_state_data.get_state().to_string());
                            if let Err(err) = dm_tx.send(msg.into()) {
                                return ControlFlow::Break(Some(err.to_string()));
                            }
                        }
                    } else {
                        let msg = StringPacket::new("state_ok");
                        if let Err(err) = dm_tx.send(msg.into()) {
                            return ControlFlow::Break(Some(err.to_string()));
                        }
                    }
                    return ControlFlow::Continue(true);
                }
                "seek" => {
                    if !check_permission_or_send_current(dm_tx, &local_data, state_data).await? {
                        return ControlFlow::Continue(true);
                    }

                    let time = match parse_time(&mut data.split("|.|")) {
                        ControlFlow::Break(con) => return ControlFlow::Continue(!con),
                        ControlFlow::Continue(time) => time,
                    };

                    {
                        let mut state_data = state_data.write().await;
                        state_data.set_time(time);
                    }
                    println!("{}: {} at {}ms", local_data.name, data_type, time);

                    let msg: WebSocketMessage =
                        StringPacket::new(data_type).arg(time.to_string()).into();
                    if let Err(err) = broadcast_tx.send(msg.into_server_shared_bytes()) {
                        return ControlFlow::Break(Some(err.to_string()));
                    }
                }
                "play" => {
                    if !check_permission_or_send_current(dm_tx, &local_data, state_data).await? {
                        return ControlFlow::Continue(true);
                    }

                    let time = match parse_time(&mut data.split("|.|")) {
                        ControlFlow::Break(con) => return ControlFlow::Continue(!con),
                        ControlFlow::Continue(time) => time,
                    };

                    {
                        let mut state_data = state_data.write().await;
                        state_data.set_state(time, STATE_PLAY);
                    }

                    println!("{}: {} at {}ms", local_data.name, data_type, time);

                    let msg: WebSocketMessage =
                        StringPacket::new(data_type).arg(time.to_string()).into();
                    if let Err(err) = broadcast_tx.send(msg.into_server_shared_bytes()) {
                        return ControlFlow::Break(Some(err.to_string()));
                    }
                }
                "pause" => {
                    if !check_permission_or_send_current(dm_tx, &local_data, state_data).await? {
                        return ControlFlow::Continue(true);
                    }

                    let time = match parse_time(&mut data.split("|.|")) {
                        ControlFlow::Break(con) => return ControlFlow::Continue(!con),
                        ControlFlow::Continue(time) => time,
                    };

                    {
                        let mut state_data = state_data.write().await;
                        state_data.set_state(time, STATE_PAUSE);
                    }

                    println!("{}: {} at {}ms", local_data.name, data_type, time);

                    let msg: WebSocketMessage =
                        StringPacket::new(data_type).arg(time.to_string()).into();
                    if let Err(err) = broadcast_tx.send(msg.into_server_shared_bytes()) {
                        return ControlFlow::Break(Some(err.to_string()));
                    }
                }
                _ => {}
            }
        }
        Message::Close(c) => {
            let reason = if let Some(cf) = c {
                Some(format!("code {}, reason `{}`", cf.code, cf.reason))
            } else {
                None
            };
            return ControlFlow::Break(reason);
        }
        _ => {}
    }
    ControlFlow::Continue(false)
}

pub async fn validate_and_handle_client(
    ws_state: &'static WsState,
    mut socket: WebSocket,
    who: SocketAddr,
    owner: Option<OwnerAuth>,
) {
    let local_user = if let Some(owner_auth) = owner {
        match ws_state.join_room(
            owner_auth.room_id,
            owner_auth.username,
            PERMISSION_ALL.into(),
        ) {
            Ok(local_user) => local_user,
            Err(_) => return,
        }
    } else {
        let msg = tokio::select! {
            msg = socket.recv() => {
                let Some(Ok(msg)) = msg else {
                    return;
                };
                msg
            },
            _ = tokio::time::sleep(std::time::Duration::from_millis(CLIENT_TIMEOUT)) => return
        };

        match verify_join_msg(msg, ws_state, &who).await {
            Ok(local_user) => local_user,
            Err(err) => {
                let _ = socket
                    .send(Message::Close(Some(CloseFrame {
                        code: crate::sturdy_ws::CloseCode::Error,
                        reason: std::borrow::Cow::Owned(format!("{}", err)),
                    })))
                    .await;
                let _ = socket.close().await;
                return;
            }
        }
    };

    user_handle(socket, who, local_user, ws_state).await;
}
