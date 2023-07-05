use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use http::StatusCode;
use tokio::sync::broadcast;
use tokio::sync::{Notify, RwLock};

use internal_server_error::InternalServerError;
use thiserror::Error;

use crate::basic_auth::Keys;
use crate::common::{Id, HashContainer, get_new_id};
use crate::sturdy_ws::{CloseCode, CloseFrame, WebSocketMessage};

use super::room_state::room_handle;
use super::user_state::{LocalUser, UserState};
use super::PERMISSION_ALL;
use super::{room_state::RoomState, VideoData};

const MAX_USERS: usize = 100;
const DEFAULT_WS: &str = "ws";

#[derive(Debug, Error, InternalServerError)]
pub enum WebSocketStateError {
    #[error("Max user exceeded!")]
    #[code(StatusCode::BAD_REQUEST)]
    MaxUserExceeded,
    #[error("The spcified room doesn't exist.")]
    #[code(StatusCode::BAD_REQUEST)]
    NoRoom,
    #[error("The spcified room is full.")]
    #[code(StatusCode::BAD_REQUEST)]
    RoomFull,
    #[error("The spcified user doesn't exist.")]
    #[code(StatusCode::BAD_REQUEST)]
    NoUser,
}

pub struct WsState {
    pub(super) users: HashContainer<UserState>,
    pub(super) rooms: HashContainer<RoomState>,
    pub keys: Keys,
}

impl Default for WsState {
    fn default() -> Self {
        let keys = Keys::new(
            std::env::var("JWT_KEY")
                .unwrap_or("ChudiGBoBDudu42096546".into())
                .as_bytes(),
        );
        Self {
            users: HashContainer::with_capacity(20),
            rooms: HashContainer::with_capacity(10),
            keys,
        }
    }
}

impl WsState {
    pub async fn create_room(
        &'static self,
        data: VideoData,
        name: String,
        max_users: usize,
    ) -> Result<(Id, String), WebSocketStateError> {
        if max_users > MAX_USERS {
            return Err(WebSocketStateError::MaxUserExceeded);
        }
        let room_id = get_new_id();
        let (broadcast_tx, _broadcast_rx) = broadcast::channel(MAX_USERS);
        let exit_notify = Arc::new(Notify::new());
        let data = Arc::new(RwLock::new(data));
        let room = RoomState {
            id: room_id,
            name,
            broadcast_tx,
            exit_notify: exit_notify.clone(),
            data: data.clone(),
            remaining_users: Arc::new(AtomicUsize::new(max_users)),
            max_users,
        };

        let _ = self.rooms.insert_async(room_id, room).await;

        tokio::spawn(room_handle(room_id, exit_notify, self));

        Ok((room_id, DEFAULT_WS.into()))
    }

    pub async fn verify_room(
        &self,
        room_id: Id,
    ) -> Result<(Id, String, String), WebSocketStateError> {
        // TODO: Check DB and get the proper ID? also the WebSocket path can be different than usual due to load balancing?
        let Some((is_full, name)) = self.rooms.read(&room_id, |_, v| (v.is_full(), v.name.clone())) else {
            return Err(WebSocketStateError::NoRoom);
        };
        if is_full {
            return Err(WebSocketStateError::RoomFull);
        }
        Ok((room_id, name, DEFAULT_WS.into()))
    }

    pub async fn join_room(
        &self,
        room_id: Id,
        name: String,
        is_owner: bool,
    ) -> Result<LocalUser, WebSocketStateError> {
        // TODO: Seperate socket identifier and user identifier... If we ever imlement DB and stuff.
        let id = get_new_id();
        let Some((join_able, data)) = self.rooms.read(&room_id, |_, v| {
            (v.user_join(), v.data.clone())
        }) else {
            return Err(WebSocketStateError::NoRoom);
        };
        if !join_able {
            return Err(WebSocketStateError::RoomFull);
        }

        let data = data.read().await;
        let permission = if is_owner {
            PERMISSION_ALL.into()
        } else {
            data.permission
        };
        let local_user = LocalUser {
            permission,
            name,
            id,
            room_id,
            is_owner,
        };

        return Ok(local_user);
    }

    pub fn kick_user(&self, id: Id) -> Result<(), WebSocketStateError> {
        if let None = self.users.read(&id, |_, v| {
            v.tx.send(WebSocketMessage::Close(Some(CloseFrame {
                code: CloseCode::Protocol,
                reason: std::borrow::Cow::Borrowed("Test"),
            })))
        }) {
            return Err(WebSocketStateError::NoUser);
        };

        Ok(())
    }

    pub async fn close_room(&self, room_id: Id) -> Result<(), WebSocketStateError> {
        if let None = self.rooms.read(&room_id, |_, v| v.close()) {
            return Err(WebSocketStateError::NoRoom);
        }
        Ok(())
    }
}
