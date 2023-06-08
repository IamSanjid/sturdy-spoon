use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tokio;
use tokio::sync::Notify;
use tokio::sync::RwLock;
use uuid::Uuid;

use super::BMsgSender;
use super::VideoData;

#[derive(Clone)]
pub struct RoomState {
    #[allow(unused)]
    pub(super) id: Uuid,
    pub(super) name: String,
    pub(super) broadcast_tx: BMsgSender,
    pub(super) exit_notify: Arc<Notify>,
    pub(super) data: Arc<RwLock<VideoData>>,
    pub(super) remaining_users: Arc<AtomicUsize>,
    pub(super) max_users: usize,
}

impl RoomState {
    pub(super) fn increase_remaining_users(&self) -> bool {
        if self.remaining_users.fetch_add(1, Ordering::AcqRel) >= self.max_users - 1 {
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

    pub(super) fn close(&self) {
        self.exit_notify.notify_waiters();
    }
}
