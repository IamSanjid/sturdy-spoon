use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use crate::sturdy_ws::{CloseCode, CloseFrame, Message};

use super::user_state::{LocalUser, UserState};
use super::PERMISSION_ALL;
use super::{room_state::RoomState, VideoData};
use http::StatusCode;
use scc::HashIndex;
use tokio::sync::broadcast;
use tokio::sync::{Notify, RwLock};
use uuid::Uuid;

use internal_server_error::InternalServerError;
use thiserror::Error;

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
    pub(super) users: HashIndex<Uuid, UserState>,
    pub(super) rooms: HashIndex<Uuid, RoomState>,
}

impl Default for WsState {
    fn default() -> Self {
        Self {
            users: HashIndex::with_capacity(20),
            rooms: HashIndex::with_capacity(10),
        }
    }
}

impl WsState {
    pub async fn create_room(
        &'static self,
        data: VideoData,
        name: String,
        max_users: usize,
    ) -> Result<(Uuid, String), WebSocketStateError> {
        if max_users > MAX_USERS {
            return Err(WebSocketStateError::MaxUserExceeded);
        }
        let room_id = Uuid::new_v4();
        let (broadcast_tx, _broadcast_rx) = broadcast::channel(MAX_USERS);
        let exit_notify = Arc::new(Notify::new());
        let data = Arc::new(RwLock::new(data));
        let room = RoomState {
            id: room_id,
            name,
            broadcast_tx,
            exit_notify,
            data: data.clone(),
            remaining_users: Arc::new(AtomicUsize::new(max_users)),
            max_users,
        };

        let _ = self.rooms.insert_async(room_id, room).await;

        Ok((room_id, DEFAULT_WS.into()))
    }

    pub async fn verify_room(
        &self,
        room_id: Uuid,
    ) -> Result<(Uuid, String, String), WebSocketStateError> {
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
        room_id: Uuid,
        name: String,
    ) -> Result<LocalUser, WebSocketStateError> {
        let Some((is_owner, decreased, data)) = self.rooms.read(&room_id, |_, v| {
            (v.is_first_user(), v.decrease_remaining_users(), v.data.clone())
        }) else {
            return Err(WebSocketStateError::NoRoom);
        };
        if !decreased {
            return Err(WebSocketStateError::RoomFull);
        }
        let data = data.read().await;
        let id = Uuid::new_v4();
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
        };

        return Ok(local_user);
    }

    pub fn kick_user(&self, id: Uuid) -> Result<(), WebSocketStateError> {
        if let None = self.users.read(&id, |_, v| {
            v.tx.send(Message::Close(Some(CloseFrame {
                code: CloseCode::Protocol,
                reason: std::borrow::Cow::Borrowed("Test"),
            })))
        }) {
            return Err(WebSocketStateError::NoUser);
        };

        Ok(())
    }

    pub async fn close_room(&self, room_id: Uuid) -> Result<(), WebSocketStateError> {
        if let None = self.rooms.read(&room_id, |_, v| v.close()) {
            return Err(WebSocketStateError::NoRoom);
        }
        self.rooms.remove_async(&room_id).await;
        Ok(())
    }
}
