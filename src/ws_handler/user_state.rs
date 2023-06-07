use std::{
    borrow::Cow,
    fmt::Write,
    net::SocketAddr,
    ops::{ControlFlow, Deref},
    str::FromStr,
};

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::{
    mpsc::{self, UnboundedSender},
    RwLock,
};
use uuid::Uuid;

use super::{
    ws_state::WsState, Permission, VideoData, MAX_VIDEO_LEN, PERMISSION_CONTROLLABLE, STATE_MAX,
};
use crate::{
    sturdy_ws::{CloseFrame, Message, WebSocket},
    ws_handler::{STATE_PAUSE, STATE_PLAY, SYNC_TIMEOUT},
};

const TIMEOUT: u64 = 60 * 2 * 1000; // 2 Minutes
const MAX_MISSED_PING: usize = 2;

#[derive(Clone)]
pub(super) struct UserState {
    #[allow(unused)]
    pub id: Uuid,
    pub tx: UnboundedSender<Message>,
}

pub struct LocalUser {
    pub name: String,
    pub id: Uuid,
    pub room_id: Uuid,
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

impl Into<Message> for StringPacket {
    fn into(self) -> Message {
        Message::Text(self.into())
    }
}

async fn video_data_json(data: &VideoData, permission: usize) -> String {
    json!({
        "url": data.get_url(),
        "time": data.get_sync_time().get().await,
        "state": data.get_state(),
        "permission": permission
    })
    .to_string()
}

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
) -> Option<LocalUser> {
    match msg {
        Message::Text(t) => {
            println!(">>> {}: {}", who, &t);
            // TODO: Make the packet data splitter more generic way.
            let Some((data_type, data)) = check_str_packet(&t) else { return None; };
            match data_type {
                "join_room" => {
                    let mut data = data.split("|.|");
                    let Some(room_id) = data.next() else { return None; };
                    let Some(name) = data.next() else { return None; };

                    let Ok(room_id) = Uuid::from_str(room_id) else { return None; };

                    return ws_state.join_room(room_id, name.to_owned()).await.ok();
                }
                _ => return None,
            }
        }
        _ => None,
    }
}

async fn user_handle(
    socket: WebSocket,
    #[allow(unused)] who: SocketAddr,
    mut local_data: LocalUser,
    ws_state: &'static WsState,
) {
    let (dm_tx, mut dm_rx) = mpsc::unbounded_channel();
    let id = local_data.id;
    let user = UserState { id, tx: dm_tx };
    let (socket, mut receiver, mut sender) = socket.tri_split();
    let _ = ws_state.users.insert_async(id, user).await;

    let current_room_id = local_data.room_id;
    let Some((exit_noti, data)) = ws_state.rooms
        .read(&current_room_id, |_, v| (v.exit_notify.clone(), v.data.clone())) else {
            return;
        };
    // get the notified future before starting the tasks..
    let exit_noti = exit_noti.notified();

    let mut send_task = tokio::spawn(async move {
        loop {
            let Some(msg) = dm_rx.recv().await else {
                break;
            };
            if let Err(_) = sender.send(msg).await {
                break;
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
                _ = tokio::time::sleep(std::time::Duration::from_millis(TIMEOUT)) => None
            };
            if let Some(msg) = msg {
                let ControlFlow::Continue(processed) = process_message(msg, &ws_state, &data, &mut local_data).await else {
                    break;
                };
                if processed {
                    missed_pings = 0;
                }
            }
            if missed_pings >= MAX_MISSED_PING {
                break;
            }
        }
    });

    match ws_state
        .rooms
        .read(&current_room_id, |_, v| v.client_tx.send((id, socket)))
    {
        Some(Ok(())) => {}
        _ => {
            send_task.abort();
            recv_task.abort();
            return;
        }
    }

    tokio::select! {
        _ = (&mut recv_task) => send_task.abort(),
        _ = (&mut send_task) => recv_task.abort(),
        _ = exit_noti => {
            send_task.abort();
            recv_task.abort();
        }
    }

    let Some(users_remained) = ws_state.rooms.read(&current_room_id, |_, v| v.decrease_remaining_users()) else {
        return;
    };

    if !users_remained {
        // This should also trigger the cleanup...
        ws_state.rooms.remove_async(&current_room_id).await;
    }
}

#[inline(always)]
async fn check_permission_or_send_current(
    ws_state: &'static WsState,
    user: &LocalUser,
    state_data: &RwLock<VideoData>,
) -> ControlFlow<Option<String>, bool> {
    if user.permission.has_permission(PERMISSION_CONTROLLABLE) {
        return ControlFlow::Continue(true);
    }
    let data = state_data.read().await;
    let data_str = StringPacket::new("video_data")
        .arg(video_data_json(data.deref(), user.permission.into()).await);
    if let None = ws_state
        .users
        .read(&user.id, |_, v| v.tx.send(data_str.into()))
    {
        return ControlFlow::Break(Some("Failed to send video data".into()));
    };
    return ControlFlow::Continue(false);
}

#[inline(always)]
async fn parse_time(data: &mut impl Iterator<Item = &str>) -> ControlFlow<bool, usize> {
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
async fn parse_state(data: &mut impl Iterator<Item = &str>) -> ControlFlow<bool, usize> {
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
    ws_state: &'static WsState,
    state_data: &RwLock<VideoData>,
    local_data: &mut LocalUser,
) -> ControlFlow<Option<String>, bool> {
    // TODO: make permission `settable` for every user individually...
    state_data.read().await.update_time_async().await;
    match msg {
        Message::Text(input_str) => {
            println!(">>> {} sent str: {:?}", local_data.name, input_str);
            let Some((data_type, data)) = check_str_packet(&input_str) else { return ControlFlow::Continue(false); };

            match data_type {
                "state" => {
                    let mut data = data.split("|.|");
                    let time = match parse_time(&mut data).await {
                        ControlFlow::Break(con) => return ControlFlow::Continue(!con),
                        ControlFlow::Continue(time) => time,
                    };

                    let video_state = match parse_state(&mut data).await {
                        ControlFlow::Break(con) => return ControlFlow::Continue(!con),
                        ControlFlow::Continue(state) => state,
                    };

                    let read_state_data = state_data.read().await;
                    let state_time = read_state_data.get_sync_time().get().await;
                    let needs_update = video_state != read_state_data.get_state()
                        || ((time as isize).abs_diff(state_time as isize) as u128) > SYNC_TIMEOUT;
                    if needs_update {
                        if local_data
                            .permission
                            .has_permission(PERMISSION_CONTROLLABLE)
                        {
                            drop(read_state_data);
                            let mut write_state_data = state_data.write().await;
                            write_state_data.set_time(time);
                            write_state_data.set_state(video_state);
                            return ControlFlow::Continue(true);
                        }
                        let data_str = StringPacket::new("state")
                            .arg(state_time.to_string())
                            .arg(read_state_data.get_state().to_string());
                        if let None = ws_state
                            .users
                            .read(&local_data.id, |_, v| v.tx.send(data_str.into()))
                        {
                            return ControlFlow::Break(Some("Failed to send status data".into()));
                        };
                    }
                    return ControlFlow::Continue(true);
                }
                "seek" => {
                    if !check_permission_or_send_current(ws_state, &local_data, state_data).await? {
                        return ControlFlow::Continue(true);
                    }

                    let time = match parse_time(&mut data.split("|.|")).await {
                        ControlFlow::Break(con) => return ControlFlow::Continue(!con),
                        ControlFlow::Continue(time) => time,
                    };

                    let mut state_data = state_data.write().await;
                    state_data.set_time(time);

                    println!("{}: {} at {}ms", local_data.name, data_type, time);

                    let msg = StringPacket::new(data_type).arg(time.to_string());
                    if let None = ws_state
                        .rooms
                        .read(&local_data.room_id, |_, v| v.broadcast_tx.send(msg.into()))
                    {
                        return ControlFlow::Break(Some("Failed to send broadcast msg".into()));
                    };
                }
                "play" => {
                    if !check_permission_or_send_current(ws_state, &local_data, state_data).await? {
                        return ControlFlow::Continue(true);
                    }

                    let time = match parse_time(&mut data.split("|.|")).await {
                        ControlFlow::Break(con) => return ControlFlow::Continue(!con),
                        ControlFlow::Continue(time) => time,
                    };

                    let mut state_data = state_data.write().await;
                    state_data.set_time(time);
                    state_data.set_state(STATE_PLAY);

                    println!("{}: {} at {}ms", local_data.name, data_type, time);

                    let msg = StringPacket::new(data_type).arg(time.to_string());
                    if let None = ws_state
                        .rooms
                        .read(&local_data.room_id, |_, v| v.broadcast_tx.send(msg.into()))
                    {
                        return ControlFlow::Break(Some("Failed to send broadcast msg".into()));
                    };
                }
                "pause" => {
                    if !check_permission_or_send_current(ws_state, &local_data, state_data).await? {
                        return ControlFlow::Continue(true);
                    }

                    let time = match parse_time(&mut data.split("|.|")).await {
                        ControlFlow::Break(con) => return ControlFlow::Continue(!con),
                        ControlFlow::Continue(time) => time,
                    };

                    let mut state_data = state_data.write().await;
                    state_data.set_time(time);
                    state_data.set_state(STATE_PAUSE);

                    println!("{}: {} at {}ms", local_data.name, data_type, time);

                    let msg = StringPacket::new(data_type).arg(time.to_string());
                    if let None = ws_state
                        .rooms
                        .read(&local_data.room_id, |_, v| v.broadcast_tx.send(msg.into()))
                    {
                        return ControlFlow::Break(Some("Failed to send broadcast msg".into()));
                    };
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
) {
    let msg = tokio::select! {
        msg = socket.recv() => {
            let Some(Ok(msg)) = msg else {
                return;
            };
            msg
        },
        _ = tokio::time::sleep(std::time::Duration::from_millis(TIMEOUT)) => return
    };

    let Some(local_user) = verify_join_msg(msg, ws_state, &who).await else {
        let _ = socket.send(Message::Close(Some(CloseFrame {
            code: crate::sturdy_ws::CloseCode::Protocol,
            reason: std::borrow::Cow::Borrowed("Unexpected message was sent. Expected: `join_msg`.")
        }))).await;
        let _ = socket.close().await;
        return;
    };

    user_handle(socket, who, local_user, ws_state).await;
}
