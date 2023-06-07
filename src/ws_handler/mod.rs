use std::sync::atomic::{AtomicBool, Ordering};

use crate::common::utils::get_elapsed_milis;

pub mod room_state;
mod user_state;
pub mod ws_state;

use tokio::sync::RwLock;
pub use user_state::validate_and_handle_client;

pub(super) type WClientSender = tokio::sync::mpsc::UnboundedSender<(uuid::Uuid, SocketsData)>;
pub(super) type WClientReciever = tokio::sync::mpsc::UnboundedReceiver<(uuid::Uuid, SocketsData)>;
pub(super) type WSMsgSender = tokio::sync::mpsc::UnboundedSender<crate::sturdy_ws::Message>;
pub(super) type WSMsgReciever = tokio::sync::mpsc::UnboundedReceiver<crate::sturdy_ws::Message>;
pub(super) type SocketsData = std::sync::Arc<tokio::sync::Mutex<crate::sturdy_ws::WebSocket>>;

pub const STATE_PAUSE: usize = 0;
pub const STATE_PLAY: usize = 1;
pub const STATE_MAX: usize = STATE_PLAY;

pub const PERMISSION_RESTRICTED: usize = 0b000;
pub const PERMISSION_CONTROLLABLE: usize = 0b001;
pub const PERMISSION_CHANGER: usize = 0b010;
pub const PERMISSION_ALL: usize =
    PERMISSION_RESTRICTED | PERMISSION_CONTROLLABLE | PERMISSION_CHANGER;

pub const SYNC_TIMEOUT: u128 = 5 * 1000; // 5 seconds
pub const MAX_VIDEO_LEN: usize = 4 * 3600 * 1000; // 4 hours

#[derive(Clone, Copy)]
pub struct Permission(usize);

impl Permission {
    pub fn has_permission(&self, permission: usize) -> bool {
        (self.0 & permission) == permission
    }

    pub fn set_permission(&mut self, permission: usize) {
        self.0 |= permission;
    }

    pub fn clear_permission(&mut self, permission: usize) {
        self.0 = self.0 & !permission
    }
}

impl Default for Permission {
    fn default() -> Self {
        Self(PERMISSION_RESTRICTED)
    }
}

impl From<Permission> for usize {
    fn from(value: Permission) -> Self {
        value.0
    }
}

impl From<&Permission> for usize {
    fn from(value: &Permission) -> Self {
        value.0
    }
}

impl From<usize> for Permission {
    fn from(value: usize) -> Self {
        Self(value)
    }
}

pub struct SyncTime {
    data: RwLock<(u128, usize)>,
    is_writing: AtomicBool,
}

impl SyncTime {
    fn new(last_updated: u128, time: usize) -> Self {
        Self {
            data: RwLock::new((last_updated, time)),
            is_writing: AtomicBool::new(false),
        }
    }

    fn update_last_updated(&mut self) {
        let data = self.data.get_mut();
        data.0 = get_elapsed_milis();
    }

    fn set_time(&mut self, time: usize) {
        self.is_writing.store(true, Ordering::Release);
        let data = self.data.get_mut();
        data.0 = get_elapsed_milis();
        data.1 = time;
        self.is_writing.store(false, Ordering::Release);
    }

    async fn should_update_time(&self, should: bool) {
        if self.is_writing() {
            return; // someone is already writing continue..
        }
        let mut data = self.data.write().await;
        self.is_writing.store(true, Ordering::Release);
        let current_time = get_elapsed_milis();
        let diff = current_time - data.0;
        data.0 = current_time;
        if should {
            data.1 += diff as usize;
        }
        self.is_writing.store(false, Ordering::Release);
    }

    pub fn is_writing(&self) -> bool {
        self.is_writing.load(Ordering::Acquire)
    }

    pub async fn get(&self) -> usize {
        let data = self.data.read().await;
        self.is_writing.store(false, Ordering::Release);
        let time = data.1;
        return time;
    }
}

pub struct VideoData {
    url: String,
    state: usize,           // 0: Pause, 1: Play
    permission: Permission, // 0: Restricted, 1: Can control video
    sync_time: SyncTime,
}

impl VideoData {
    pub fn new(url: String) -> Self {
        Self {
            url,
            state: 0,
            permission: Permission::default(),
            sync_time: SyncTime::new(get_elapsed_milis(), 0),
        }
    }

    pub fn with_permission(mut self, permission: usize) -> Self {
        self.permission = permission.into();
        self
    }

    pub fn get_url(&self) -> String {
        self.url.to_owned()
    }

    pub fn update_url(&mut self, url: String) {
        self.url = url;
    }

    pub fn get_state(&self) -> usize {
        self.state
    }

    pub fn set_state(&mut self, state: usize) {
        self.state = state;
        self.sync_time.update_last_updated();
    }

    pub fn get_sync_time(&self) -> &SyncTime {
        &self.sync_time
    }

    pub fn set_time(&mut self, time: usize) {
        self.sync_time.set_time(time);
    }

    pub async fn update_time_async(&self) {
        self.sync_time
            .should_update_time(self.state == STATE_PLAY)
            .await
    }

    pub fn get_permission(&self) -> Permission {
        self.permission
    }

    pub fn has_permission(&self, permission: usize) -> bool {
        self.permission.has_permission(permission)
    }

    pub fn set_permission(&mut self, permission: usize) {
        self.permission.set_permission(permission);
    }

    pub fn clear_permission(&mut self, permission: usize) {
        self.permission.clear_permission(permission);
    }
}
