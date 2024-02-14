use std::{
    borrow::Cow,
    fmt::Write,
    net::SocketAddr,
    ops::{ControlFlow, Deref},
    str::FromStr,
    sync::Arc,
};

use futures_util::StreamExt;
use serde_json::json;
use thiserror::Error;
use tokio::sync::{mpsc, RwLock};

use super::{
    room_state::RoomState, ws_state::WsState, Permission, PermissionType, StateType, VideoData,
    WSMsgSender, MAX_VIDEO_LEN, PERMISSION_ALL, PERMISSION_CONTROLLABLE, STATE_MAX,
};
use crate::{
    basic_auth::OwnerAuth,
    common::Id,
    sturdy_ws::{ws_stream::SplitStream, CloseFrame, Message, WebSocket, WebSocketMessage},
    ws_handler::{
        room_state::room_shutdown_gracefully, ws_state::WebSocketStateError, CLIENT_TIMEOUT,
        STATE_PAUSE, STATE_PLAY, SYNC_TIMEOUT,
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

struct LocalUserState {
    pub name: String,
    pub id: Id,
    pub room_state: Arc<RoomState>,
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

fn video_data_json(data: &VideoData, permission: PermissionType) -> String {
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
#[inline]
fn check_str_packet<'a>(input_str: &'a str) -> Option<(&'a str, &'a str)> {
    let Some(input_str) = input_str.strip_prefix("||-=-||") else {
        return None;
    };
    let mut full_data = input_str.split("-=-");
    let Some(data_type) = full_data.next() else {
        return None;
    };
    let Some(data) = full_data.next() else {
        return None;
    };
    Some((data_type, data))
}

async fn user_handle(
    socket: WebSocket,
    who: SocketAddr,
    local_data: LocalUserState,
    permission: Permission,
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
    let current_room_id = local_data.room_state.id;

    let msg: WebSocketMessage = StringPacket::new("joined")
        .arg(name.clone())
        .arg(id.to_string())
        .into();
    let _ = local_data
        .room_state
        .broadcast_tx
        .send(msg.into_server_shared_bytes());

    {
        let r_data = local_data.room_state.data.read().await;
        let data_str =
            StringPacket::new("video_data").arg(video_data_json(r_data.deref(), permission.into()));
        let _ = dm_tx.send(data_str.into());
        drop(r_data);
    }
    let mut broadcast_rx = local_data.room_state.broadcast_tx.subscribe();

    let (socket, receiver) = socket.sock_split();
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

    let mut recv_task = if permission.has_permission(PERMISSION_CONTROLLABLE) {
        tokio::spawn(recv_task_privileged(receiver, dm_tx, local_data))
    } else {
        tokio::spawn(recv_task_normal(receiver, dm_tx, local_data, permission))
    };

    tokio::select! {
        _ = (&mut recv_task) => send_task.abort(),
        _ = (&mut send_task) => recv_task.abort(),
    }

    let Some(room) = ws_state.rooms.read(&current_room_id, |_, v| v.clone()) else {
        return;
    };

    if !room.user_left() {
        room_shutdown_gracefully(current_room_id, ws_state).await;
    } else {
        let msg: WebSocketMessage = StringPacket::new("left")
            .arg(name)
            .arg(id.to_string())
            .into();

        let _ = room.broadcast_tx.send(msg.into_server_shared_bytes());
    }

    ws_state.users.remove_async(&id).await;
    println!("{} left! - id: {}", who, id);
}

async fn recv_task_privileged(
    mut receiver: SplitStream<WebSocket>,
    dm_tx: WSMsgSender,
    mut local_data: LocalUserState,
) {
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
        if let Ok(read_data) = local_data.room_state.data.try_read() {
            drop(read_data); // Must drop it otherwise dead lock...
            local_data.room_state.data.write().await.update_time();
        }
        if let Some(msg) = msg {
            match msg {
                Message::Text(input_str) => {
                    println!(">>> {} sent str: {:?}", local_data.name, input_str);
                    match process_privileged_message(input_str, &dm_tx, &mut local_data).await {
                        ControlFlow::Break(_) => {
                            // TODO: Print why we're breaking..
                        }
                        ControlFlow::Continue(processed) => {
                            if processed {
                                missed_pings = 0;
                            }
                            continue;
                        }
                    };
                }
                Message::Close(_) => {
                    // TODO: Print the close reason.
                    break;
                }
                _ => {}
            }
        }
        missed_pings += 1;
        if missed_pings >= MAX_MISSED_PING {
            break;
        }
    }
}

async fn recv_task_normal(
    mut receiver: SplitStream<WebSocket>,
    dm_tx: WSMsgSender,
    local_data: LocalUserState,
    permission: Permission,
) {
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
        if let Ok(read_data) = local_data.room_state.data.try_read() {
            drop(read_data); // Must drop it otherwise dead lock...
            local_data.room_state.data.write().await.update_time();
        }
        if let Some(msg) = msg {
            match msg {
                Message::Text(input_str) => {
                    println!(">>> {} sent str: {:?}", local_data.name, input_str);
                    match process_normal_message(
                        input_str,
                        &dm_tx,
                        &local_data.room_state.data,
                        permission,
                    )
                    .await
                    {
                        ControlFlow::Break(_) => {
                            // TODO: Print why we're breaking..
                        }
                        ControlFlow::Continue(processed) => {
                            if processed {
                                missed_pings = 0;
                            }
                            continue;
                        }
                    };
                }
                Message::Close(_) => {
                    // TODO: Print the close reason.
                    break;
                }
                _ => {}
            }
        }
        missed_pings += 1;
        if missed_pings >= MAX_MISSED_PING {
            break;
        }
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
fn parse_state<'a>(mut data: impl Iterator<Item = &'a str>) -> ControlFlow<bool, StateType> {
    let Some(state) = data.next() else {
        return ControlFlow::Break(false);
    };
    let Ok(state) = state.parse::<StateType>() else {
        return ControlFlow::Break(false);
    };
    if state > STATE_MAX {
        return ControlFlow::Break(true);
    }
    return ControlFlow::Continue(state);
}

async fn process_privileged_message(
    input_str: String,
    dm_tx: &WSMsgSender,
    local_data: &mut LocalUserState,
) -> ControlFlow<Option<String>, bool> {
    let Some((data_type, data)) = check_str_packet(&input_str) else {
        return ControlFlow::Continue(false);
    };

    let state_data = &local_data.room_state.data;
    let broadcast_tx = &local_data.room_state.broadcast_tx;

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
                let msg = StringPacket::new("state_ok");
                if let Err(err) = dm_tx.send(msg.into()) {
                    return ControlFlow::Break(Some(err.to_string()));
                }
            }
            return ControlFlow::Continue(true);
        }
        "seek" => {
            let time = match parse_time(&mut data.split("|.|")) {
                ControlFlow::Break(con) => return ControlFlow::Continue(!con),
                ControlFlow::Continue(time) => time,
            };

            {
                let mut state_data = state_data.write().await;
                state_data.set_time(time);
            }
            println!("{}: {} at {}ms", local_data.name, data_type, time);

            let msg: WebSocketMessage = StringPacket::new(data_type).arg(time.to_string()).into();
            if let Err(err) = broadcast_tx.send(msg.into_server_shared_bytes()) {
                return ControlFlow::Break(Some(err.to_string()));
            }
        }
        "play" => {
            let time = match parse_time(&mut data.split("|.|")) {
                ControlFlow::Break(con) => return ControlFlow::Continue(!con),
                ControlFlow::Continue(time) => time,
            };

            {
                let mut state_data = state_data.write().await;
                state_data.set_state(time, STATE_PLAY);
            }

            println!("{}: {} at {}ms", local_data.name, data_type, time);

            let msg: WebSocketMessage = StringPacket::new(data_type).arg(time.to_string()).into();
            if let Err(err) = broadcast_tx.send(msg.into_server_shared_bytes()) {
                return ControlFlow::Break(Some(err.to_string()));
            }
        }
        "pause" => {
            let time = match parse_time(&mut data.split("|.|")) {
                ControlFlow::Break(con) => return ControlFlow::Continue(!con),
                ControlFlow::Continue(time) => time,
            };

            {
                let mut state_data = state_data.write().await;
                state_data.set_state(time, STATE_PAUSE);
            }

            println!("{}: {} at {}ms", local_data.name, data_type, time);

            let msg: WebSocketMessage = StringPacket::new(data_type).arg(time.to_string()).into();
            if let Err(err) = broadcast_tx.send(msg.into_server_shared_bytes()) {
                return ControlFlow::Break(Some(err.to_string()));
            }
        }
        _ => {}
    }

    return ControlFlow::Continue(false);
}

async fn process_normal_message(
    input_str: String,
    dm_tx: &WSMsgSender,
    state_data: &RwLock<VideoData>,
    permission: Permission,
) -> ControlFlow<Option<String>, bool> {
    let Some((data_type, data)) = check_str_packet(&input_str) else {
        return ControlFlow::Continue(false);
    };

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
                let msg = StringPacket::new("state")
                    .arg(read_state_data.get_time().to_string())
                    .arg(read_state_data.get_state().to_string());
                if let Err(err) = dm_tx.send(msg.into()) {
                    return ControlFlow::Break(Some(err.to_string()));
                }
            } else {
                let msg = StringPacket::new("state_ok");
                if let Err(err) = dm_tx.send(msg.into()) {
                    return ControlFlow::Break(Some(err.to_string()));
                }
            }
            return ControlFlow::Continue(true);
        }
        "pause" | "play" | "seek" => {
            let data = state_data.read().await;
            let data_str = StringPacket::new("video_data")
                .arg(video_data_json(data.deref(), permission.into()));
            if let Err(err) = dm_tx.send(data_str.into()) {
                return ControlFlow::Break(Some(err.to_string()));
            };
            return ControlFlow::Continue(true);
        }
        _ => {}
    }
    return ControlFlow::Continue(false);
}

async fn verify_join_msg(
    msg: Message,
    ws_state: &'static WsState,
    who: &SocketAddr,
) -> Result<(LocalUserState, Permission), ValidationError> {
    match msg {
        Message::Text(t) => {
            println!(">>> {}: {}", who, &t);
            let Some((data_type, data)) = check_str_packet(&t) else {
                return Err(ValidationError::InvalidPacket);
            };
            match data_type {
                "join_room" => {
                    let mut data = data.split("|.|");
                    let Some(room_id) = data.next() else {
                        return Err(ValidationError::InvalidPacket);
                    };
                    let Some(name) = data.next() else {
                        return Err(ValidationError::InvalidPacket);
                    };

                    let Ok(room_id) = Id::from_str(room_id) else {
                        return Err(ValidationError::InvalidPacket);
                    };

                    println!("Trying to join room: {}", room_id);

                    let room = ws_state.get_room(room_id).map_err(ValidationError::from)?;
                    let room_data = room.data.read().await;
                    return ws_state
                        .join_room(room_id)
                        .map_err(ValidationError::from)
                        .map(|(id, room_state)| {
                            (
                                LocalUserState {
                                    name: name.to_owned(),
                                    id,
                                    room_state,
                                },
                                room_data.get_permission(),
                            )
                        });
                }
                _ => return Err(ValidationError::InvalidPacket),
            }
        }
        _ => Err(ValidationError::InvalidPacket),
    }
}

pub async fn validate_and_handle_client(
    ws_state: &'static WsState,
    mut socket: WebSocket,
    who: SocketAddr,
    owner: Option<OwnerAuth>,
) {
    let (local_user, permision) = if let Some(owner_auth) = owner {
        match ws_state.join_room(owner_auth.room_id) {
            Ok((id, room_state)) => (
                LocalUserState {
                    name: owner_auth.username,
                    id,
                    room_state,
                },
                PERMISSION_ALL.into(),
            ),
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

    user_handle(socket, who, local_user, permision, ws_state).await;
}
