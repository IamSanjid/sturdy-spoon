use crate::common::utils::get_elapsed_milis;

pub mod room_state;
mod user_state;
pub mod ws_state;

pub use user_state::validate_and_handle_client;

pub(super) type WSMsgSender =
    tokio::sync::mpsc::UnboundedSender<crate::sturdy_ws::WebSocketMessage>;
pub(super) type BMsgSender = tokio::sync::broadcast::Sender<std::sync::Arc<[u8]>>;

// TODO: Refactor these parts?
pub type StateType = u8;
pub type PermissionType = u8;
pub type PlayerType = u8;

pub const STATE_PAUSE: StateType = 0;
pub const STATE_PLAY: StateType = 1;
pub const STATE_MAX: StateType = STATE_PLAY;

pub const PERMISSION_RESTRICTED: PermissionType = 0b000;
pub const PERMISSION_CONTROLLABLE: PermissionType = 0b001;
pub const PERMISSION_CHANGER: PermissionType = 0b010;
pub const PERMISSION_ALL: PermissionType =
    PERMISSION_RESTRICTED | PERMISSION_CONTROLLABLE | PERMISSION_CHANGER;

#[allow(unused)]
pub const PLAYER_JW: PlayerType = 0;
pub const PLAYER_NORMAL: PlayerType = 1;
pub const PLAYER_MAX: PlayerType = PLAYER_NORMAL;

pub const CLIENT_TIMEOUT: u64 = 60 * 2 * 1000; // 2 Minutes
pub const SYNC_TIMEOUT: u128 = 5 * 1000; // 5 seconds
pub const MAX_VIDEO_LEN: usize = 4 * 3600 * 1000; // 4 hours

#[derive(Clone, Copy)]
// TODO: Remove permission from User
pub struct Permission(PermissionType);

impl Permission {
    pub fn has_permission(&self, permission: PermissionType) -> bool {
        (self.0 & permission) == permission
    }

    pub fn set_permission(&mut self, permission: PermissionType) {
        self.0 |= permission;
    }

    pub fn clear_permission(&mut self, permission: PermissionType) {
        self.0 = self.0 & !permission
    }
}

impl Default for Permission {
    fn default() -> Self {
        Self(PERMISSION_RESTRICTED)
    }
}

impl From<Permission> for PermissionType {
    fn from(value: Permission) -> Self {
        value.0
    }
}

impl From<&Permission> for Permission {
    fn from(value: &Permission) -> Self {
        Self(value.0)
    }
}

impl From<PermissionType> for Permission {
    fn from(value: PermissionType) -> Self {
        Self(value)
    }
}

pub struct VideoData {
    last_time_updated: u128,
    time: usize, // in Miliseconds
    url: String,
    cc_url: String,
    state: StateType,       // 0: Pause, 1: Play
    permission: Permission, // 0: Restricted, 1: Can control video
    current_player: PlayerType,
    // TODO: Wasted 5 bytes...
}

impl VideoData {
    pub fn new(url: String, cc_url: String, current_player: PlayerType) -> Self {
        Self {
            last_time_updated: get_elapsed_milis(),
            time: 0,
            url,
            cc_url,
            state: STATE_PAUSE,
            permission: Permission::default(),
            current_player,
        }
    }

    #[inline]
    pub fn get_url(&self) -> String {
        self.url.clone()
    }

    #[inline]
    pub fn update_url(&mut self, url: String) {
        self.url = url;
    }

    #[inline]
    pub fn get_cc_url(&self) -> String {
        self.cc_url.clone()
    }

    #[inline]
    pub fn update_cc_url(&mut self, cc_url: String) {
        self.cc_url = cc_url;
    }

    #[inline]
    pub fn get_state(&self) -> StateType {
        self.state
    }

    #[inline]
    pub fn set_state(&mut self, time: usize, state: StateType) {
        self.state = state;
        self.time = time;
        self.last_time_updated = get_elapsed_milis();
    }

    #[inline]
    pub fn set_time(&mut self, time: usize) {
        self.time = time;
        self.last_time_updated = get_elapsed_milis();
    }

    #[inline]
    pub fn get_time(&self) -> usize {
        self.time
    }

    pub fn update_time(&mut self) {
        let current_time = get_elapsed_milis();
        let diff = current_time - self.last_time_updated;
        self.last_time_updated = current_time;
        if self.state == STATE_PLAY {
            self.time += diff as usize;
        }
    }

    #[inline]
    pub fn get_current_player(&self) -> PlayerType {
        self.current_player
    }

    #[inline]
    pub fn set_current_player(&mut self, current_player: PlayerType) {
        self.current_player = current_player;
    }

    #[inline]
    pub fn get_permission(&self) -> Permission {
        self.permission
    }

    #[inline]
    pub fn has_permission(&self, permission: PermissionType) -> bool {
        self.permission.has_permission(permission)
    }

    #[inline]
    pub fn set_permission(&mut self, permission: PermissionType) {
        self.permission.set_permission(permission);
    }

    #[inline]
    pub fn clear_permission(&mut self, permission: PermissionType) {
        self.permission.clear_permission(permission);
    }
}
