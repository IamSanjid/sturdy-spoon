use crate::common::utils::get_elapsed_milis;

pub mod room_state;
mod user_state;
pub mod ws_state;

pub(super) type WClientSender = tokio::sync::mpsc::UnboundedSender<(uuid::Uuid, SocketsData)>;
pub(super) type WClientReciever = tokio::sync::mpsc::UnboundedReceiver<(uuid::Uuid, SocketsData)>;
pub(super) type WSMsgSender = tokio::sync::mpsc::UnboundedSender<crate::sturdy_ws::Message>;
pub(super) type WSMsgReciever = tokio::sync::mpsc::UnboundedReceiver<crate::sturdy_ws::Message>;
pub(super) type SocketsData = std::sync::Arc<tokio::sync::Mutex<crate::sturdy_ws::WebSocket>>;

pub const STATE_PAUSE: usize = 0;
pub const STATE_PLAY: usize = 1;
pub const STATE_MAX: usize = STATE_PLAY;

pub const PERMISSION_RESTRICTED: usize = 0;
pub const PERMISSION_CONTROLLABLE: usize = 1;
pub const PERMISSION_ALL: usize = PERMISSION_RESTRICTED | PERMISSION_CONTROLLABLE;

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

pub struct VideoData {
    url: String,
    time: usize,            // in Miliseconds
    state: usize,           // 0: Pause, 1: Play
    permission: Permission, // 0: Restricted, 1: Can control video
    last_updated: u128,
}

impl VideoData {
    pub fn new(url: String) -> Self {
        Self {
            url,
            time: 0,
            state: 0,
            permission: Permission::default(),
            last_updated: get_elapsed_milis(),
        }
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
        self.update();
    }

    pub fn get_time(&self) -> usize {
        self.time
    }

    pub fn set_time(&mut self, time: usize) {
        self.last_updated = get_elapsed_milis();
        self.time = time;
    }

    pub fn update(&mut self) {
        let current_time = get_elapsed_milis();
        let diff = current_time - self.last_updated;
        self.last_updated = current_time;
        if self.state != STATE_PLAY {
            return;
        }
        self.time += diff as usize;
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
