use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use http::StatusCode;
use scc::HashMap;
use tokio::sync::broadcast;
use tokio::sync::RwLock;

use internal_server_error::InternalServerError;
use thiserror::Error;

use crate::basic_auth::{Keys, OwnerAuth, CHECKED_AUTH_EXPIRATION};
use crate::common::utils::get_elapsed_milis;
use crate::common::{get_new_id, HashContainer, Id};
use crate::sturdy_ws::{CloseCode, CloseFrame, WebSocketMessage};

use super::user_state::UserState;
use super::{room_state::RoomState, VideoData};

pub const DEFAULT_WS: &str = "room/ws";
const MAX_USERS: u32 = 100;

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
    #[error("The spcified owner doesn't exist.")]
    #[code(StatusCode::BAD_REQUEST)]
    NoOwner,
}

pub struct WsState {
    pub(super) users: HashContainer<UserState>,
    pub(super) rooms: HashContainer<Arc<RoomState>>,
    pub(super) checked_auth_ids: HashMap<Id, (OwnerAuth, u128)>,
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
            checked_auth_ids: HashMap::with_capacity(10),
            keys,
        }
    }
}

impl WsState {
    pub async fn update_checked_auths(&self) {
        let current_elapsed_time = get_elapsed_milis();
        self.checked_auth_ids
            .retain_async(|_, (_, start_time)| *start_time > current_elapsed_time)
            .await;
    }

    pub async fn create_room(
        &'static self,
        data: VideoData,
        name: String,
        max_users: u32,
    ) -> Result<(Id, String), WebSocketStateError> {
        if max_users > MAX_USERS {
            return Err(WebSocketStateError::MaxUserExceeded);
        }
        let room_id = get_new_id();
        let (broadcast_tx, _broadcast_rx) = broadcast::channel(max_users as usize);
        let data = RwLock::new(data);
        let room = Arc::new(RoomState {
            id: room_id,
            name,
            broadcast_tx,
            data,
            remaining_users: AtomicU32::new(max_users),
            max_users,
        });

        let _ = self.rooms.insert_async(room_id, room).await;

        Ok((room_id, DEFAULT_WS.into()))
    }

    pub fn verify_room(&self, room_id: Id) -> Result<(Id, String, String), WebSocketStateError> {
        // TODO: Check DB and get the proper ID? also the WebSocket path can be different than usual for load balancing stuff?
        let Some((is_full, name)) = self
            .rooms
            .read(&room_id, |_, v| (v.is_full(), v.name.clone()))
        else {
            return Err(WebSocketStateError::NoRoom);
        };
        if is_full {
            return Err(WebSocketStateError::RoomFull);
        }
        Ok((room_id, name, DEFAULT_WS.into()))
    }

    pub fn join_room(&self, room_id: Id) -> Result<(Id, Arc<RoomState>), WebSocketStateError> {
        // TODO: Seperate socket identifier and user identifier? If we ever implement DB and stuff.
        let id = get_new_id();
        let room_state = self.get_room(room_id)?;
        if !room_state.user_join() {
            return Err(WebSocketStateError::RoomFull);
        }

        return Ok((id, room_state));
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

    #[inline]
    pub fn get_room(&self, room_id: Id) -> Result<Arc<RoomState>, WebSocketStateError> {
        let Some(data) = self.rooms.read(&room_id, |_, v| v.clone()) else {
            return Err(WebSocketStateError::NoRoom);
        };
        Ok(data)
    }

    pub async fn remove_checked_auth<F: FnOnce(&mut (OwnerAuth, u128)) -> bool>(
        &self,
        id: Id,
        condition: F,
    ) -> Result<OwnerAuth, WebSocketStateError> {
        let Some((_, (owner_auth, _))) =
            self.checked_auth_ids.remove_if_async(&id, condition).await
        else {
            return Err(WebSocketStateError::NoOwner);
        };

        Ok(owner_auth)
    }

    pub async fn add_checked_auth(&self, owner_auth: OwnerAuth) -> Id {
        let id = get_new_id();
        let _ = self
            .checked_auth_ids
            .insert_async(
                id,
                (owner_auth, get_elapsed_milis() + CHECKED_AUTH_EXPIRATION),
            )
            .await;
        id
    }
}
