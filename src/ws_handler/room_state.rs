use std::sync::atomic::{AtomicU32, Ordering};

use tokio;
use tokio::sync::RwLock;

use crate::common::Id;

use super::ws_state::WsState;
use super::VideoData;
use super::{BMsgSender, CLIENT_TIMEOUT};

pub struct RoomState {
    pub(super) data: RwLock<VideoData>,
    pub(super) id: Id,
    pub(super) name: String,
    pub(super) broadcast_tx: BMsgSender,
    pub(super) remaining_users: AtomicU32,
    pub(super) max_users: u32,
}

impl RoomState {
    pub(super) fn user_left(&self) -> bool {
        if self.remaining_users.fetch_add(1, Ordering::AcqRel) >= self.max_users - 1 {
            self.remaining_users
                .store(self.max_users, Ordering::Release);
            return false;
        }
        return true;
    }

    pub(super) fn user_join(&self) -> bool {
        if self.remaining_users.fetch_sub(1, Ordering::AcqRel) == 0 {
            self.remaining_users.store(0, Ordering::Release);
            return false;
        }
        return true;
    }

    pub(super) fn is_full(&self) -> bool {
        self.remaining_users.load(Ordering::Relaxed) == 0
    }

    pub(super) fn is_empty(&self) -> bool {
        self.remaining_users.load(Ordering::Relaxed) == self.max_users
    }
}

pub(super) async fn room_shutdown_gracefully(room_id: Id, ws_state: &'static WsState) {
    tokio::time::sleep(std::time::Duration::from_millis(CLIENT_TIMEOUT)).await;
    let res = ws_state.rooms.read(&room_id, |_, v| v.is_empty());
    if matches!(res, Some(true)) {
        return;
    }

    println!("[ROOM UPDATE] Removing room id: {}", room_id);
    ws_state.rooms.remove_async(&room_id).await;
}
