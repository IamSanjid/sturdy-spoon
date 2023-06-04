use std::net::SocketAddr;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::{
    mpsc::{self, UnboundedSender},
    RwLock,
};
use uuid::Uuid;

use crate::sturdy_ws::{Message, WebSocket};

use super::{ws_state::WsState, Permission, VideoData};

#[derive(Clone)]
pub(super) struct UserState {
    pub id: Uuid,
    pub tx: UnboundedSender<Message>,
}

pub struct LocalUser {
    pub name: String,
    pub id: Uuid,
    pub room_id: Uuid,
    pub permission: Permission,
}

pub async fn user_handle(
    socket: WebSocket,
    who: SocketAddr,
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
        loop {
            let Some(Ok(msg)) = receiver.next().await else {
                break;
            };
            println!(">> {}: {:?}", who, msg);
            process_message(msg, &ws_state, &data, &mut local_data).await;
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

async fn process_message(
    msg: Message,
    ws_state: &'static WsState,
    data: &RwLock<VideoData>,
    local_data: &mut LocalUser,
) {
}
