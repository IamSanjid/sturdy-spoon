use crate::common::utils::get_elapsed_milis;

pub mod room_state;
mod user_state;
pub mod ws_state;

pub use user_state::validate_and_handle_client;

pub(super) type WSMsgSender = tokio::sync::mpsc::UnboundedSender<crate::sturdy_ws::Message>;
pub(super) type BMsgSender = tokio::sync::broadcast::Sender<std::sync::Arc<Vec<u8>>>;

pub const STATE_PAUSE: usize = 0;
pub const STATE_PLAY: usize = 1;
pub const STATE_MAX: usize = STATE_PLAY;

pub const PERMISSION_RESTRICTED: usize = 0b000;
pub const PERMISSION_CONTROLLABLE: usize = 0b001;
pub const PERMISSION_CHANGER: usize = 0b010;
pub const PERMISSION_ALL: usize =
    PERMISSION_RESTRICTED | PERMISSION_CONTROLLABLE | PERMISSION_CHANGER;

pub const CLIENT_TIMEOUT: u64 = 60 * 2 * 1000; // 2 Minutes
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
    last_time_updated: u128,
}

impl VideoData {
    pub fn new(url: String) -> Self {
        Self {
            url,
            time: 0,
            state: 0,
            permission: Permission::default(),
            last_time_updated: get_elapsed_milis(),
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
        self.last_time_updated = get_elapsed_milis();
    }

    pub fn get_time(&self) -> usize {
        self.time
    }

    pub fn set_time(&mut self, time: usize) {
        self.time = time;
        self.last_time_updated = get_elapsed_milis();
    }

    pub fn update_time(&mut self) {
        let current_time = get_elapsed_milis();
        let diff = current_time - self.last_time_updated;
        self.last_time_updated = current_time;
        if self.state == STATE_PLAY {
            self.time += diff as usize;
        }
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
