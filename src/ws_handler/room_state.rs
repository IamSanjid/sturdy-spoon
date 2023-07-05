use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tokio;
use tokio::sync::Notify;
use tokio::sync::RwLock;

use crate::common::Id;

use super::ws_state::WsState;
use super::VideoData;
use super::{BMsgSender, CLIENT_TIMEOUT};

#[derive(Clone)]
pub struct RoomState {
    #[allow(unused)]
    pub(super) id: Id,
    pub(super) name: String,
    pub(super) broadcast_tx: BMsgSender,
    pub(super) exit_notify: Arc<Notify>,
    pub(super) data: Arc<RwLock<VideoData>>,
    pub(super) remaining_users: Arc<AtomicUsize>,
    pub(super) max_users: usize,
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

    pub(super) fn close(&self) {
        self.exit_notify.notify_waiters();
    }
}

pub(super) async fn room_handle(room_id: Id, exit_notif: Arc<Notify>, ws_state: &'static WsState) {
    loop {
        exit_notif.notified().await;
        tokio::time::sleep(std::time::Duration::from_millis(CLIENT_TIMEOUT)).await;
        let res = ws_state.rooms.read(&room_id, |_, v| v.is_empty());
        if matches!(res, None) || matches!(res, Some(true)) {
            break;
        }
    }

    println!("[ROOM UPDATE] Removing room id: {}", room_id);
    ws_state.rooms.remove_async(&room_id).await;
}
