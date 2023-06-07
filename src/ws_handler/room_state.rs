use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use futures_util::stream::FuturesUnordered;
use futures_util::StreamExt;
use tokio;
use tokio::sync::Mutex;
use tokio::sync::Notify;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::sturdy_ws::CloseFrame;
use crate::sturdy_ws::{Message, WebSocket};

use super::ws_state::WsState;
use super::VideoData;
use super::{SocketsData, WClientReciever, WClientSender, WSMsgReciever, WSMsgSender};
use crate::sturdy_ws::Frame;

#[allow(unused)]
#[derive(Clone)]
pub struct RoomState {
    pub(super) id: Uuid,
    pub(super) name: String,
    pub(super) client_tx: WClientSender,
    pub(super) broadcast_tx: WSMsgSender,
    pub(super) exit_notify: Arc<Notify>,
    pub(super) data: Arc<RwLock<VideoData>>,
    pub(super) remaining_users: Arc<AtomicUsize>,
    pub(super) max_users: usize,
}

impl RoomState {
    pub(super) fn increase_remaining_users(&self) -> bool {
        if self.remaining_users.fetch_add(1, Ordering::AcqRel) >= self.max_users {
            self.remaining_users
                .store(self.max_users, Ordering::Release);
            return false;
        }
        return true;
    }

    pub(super) fn decrease_remaining_users(&self) -> bool {
        if self.remaining_users.fetch_sub(1, Ordering::AcqRel) == 0 {
            self.remaining_users.store(0, Ordering::Release);
            return false;
        }
        return true;
    }

    pub(super) fn is_full(&self) -> bool {
        self.remaining_users.load(Ordering::Relaxed) == 0
    }

    pub(super) fn is_first_user(&self) -> bool {
        self.remaining_users.load(Ordering::Relaxed) == self.max_users
    }
}

pub async fn room_handle(
    room_id: Uuid,
    exit_notify: Arc<Notify>,
    ws_state: &'static WsState,
    mut client_joined_rx: WClientReciever,
    mut broadcast_rx: WSMsgReciever,
) {
    let mut local_sockets = HashMap::with_capacity(10);
    let mut last_exit_msg = None;
    loop {
        tokio::select! {
            msg = broadcast_rx.recv() => {
                let Some(msg) = msg else {
                    last_exit_msg = None;
                    break;
                };

                if let Some(exit_msg) = handle_broadcast_msg(msg, ws_state, &local_sockets).await {
                    last_exit_msg = Some(exit_msg);
                    break;
                }
            },
            client_joined = client_joined_rx.recv() => {
                let Some((id, data)) = client_joined else {
                    break;
                };
                local_sockets.insert(id, data);
                while let Ok((id, data)) = client_joined_rx.try_recv() {
                    local_sockets.insert(id, data);
                }
            }
        }
    }

    let close_msg = last_exit_msg.unwrap_or(Message::Close(Some(CloseFrame {
        code: crate::sturdy_ws::CloseCode::Protocol,
        reason: std::borrow::Cow::Borrowed("Forced Close."),
    })));
    let cloned_msg = close_msg.clone();

    let frame: Frame = close_msg.into();
    let bytes: Vec<u8> = frame.into();

    let futures = FuturesUnordered::default();
    for (id, socket) in &local_sockets {
        futures.push(async {
            send_raw(socket, id, &bytes, ws_state, &cloned_msg).await;
            ws_state.users.remove_async(id).await;
        });
    }

    let _: Vec<_> = futures.collect().await;
    local_sockets.clear();

    ws_state.rooms.remove_async(&room_id).await;
    exit_notify.notify_waiters();
}

#[inline(always)]
async fn handle_broadcast_msg(
    msg: Message,
    ws_state: &'static WsState,
    local_sockets: &HashMap<Uuid, SocketsData>,
) -> Option<Message> {
    let cloned_msg = msg.clone();
    if matches!(cloned_msg, Message::Close(_)) {
        return Some(msg);
    }

    let frame: Frame = msg.into();
    let bytes: Vec<u8> = frame.into();
    let futures = FuturesUnordered::default();

    for (id, socket) in local_sockets {
        futures.push(send_raw(&socket, &id, &bytes, ws_state, &cloned_msg));
    }

    let _: Vec<_> = futures.collect().await;
    return None;
}

async fn send_raw(
    socket: &Arc<Mutex<WebSocket>>,
    id: &Uuid,
    bytes: &[u8],
    ws_state: &'static WsState,
    cloned_msg: &Message,
) {
    if let Err(err) = socket.lock().await.send_raw(bytes) {
        match err {
            crate::sturdy_ws::Error::Io(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                ws_state
                    .users
                    .read(id, |_, v| v.tx.send(cloned_msg.clone()));
            }
            _ => {}
        }
    }
}
