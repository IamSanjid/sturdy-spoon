use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use crate::sturdy_ws::{CloseCode, CloseFrame, Message};

use super::room_state::room_handle;
use super::user_state::{LocalUser, UserState};
use super::PERMISSION_ALL;
use super::{room_state::RoomState, VideoData};
use http::StatusCode;
use jsonwebtoken::{DecodingKey, EncodingKey};
use scc::HashIndex;
use tokio::sync::broadcast;
use tokio::sync::{Notify, RwLock};
use uuid::Uuid;

use internal_server_error::InternalServerError;
use thiserror::Error;

const MAX_USERS: usize = 100;
const DEFAULT_WS: &str = "ws";

pub(super) struct Keys {
    pub encoding: EncodingKey,
    pub decoding: DecodingKey,
}

impl Keys {
    fn new(secret: &[u8]) -> Self {
        Self {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
        }
    }
}

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
    pub(super) keys: Keys,
}

impl Default for WsState {
    fn default() -> Self {
        let keys = Keys::new(
            std::env::var("JWT_KEY")
                .unwrap_or("ChudiGBoBDudu42096546".into())
                .as_bytes(),
        );
        Self {
            users: HashIndex::with_capacity(20),
            rooms: HashIndex::with_capacity(10),
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
            owner_id: Arc::new(RwLock::new(None)),
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
        pref_id: Option<Uuid>,
    ) -> Result<LocalUser, WebSocketStateError> {
        let id = Uuid::new_v4();
        let mut is_new_owner = false;
        // TODO: Seperate socket identifier Id(s) from the actual user Id probably if we ever imlement DB.
        let Some((is_empty, decreased, data, owner_id)) = self.rooms.read(&room_id, |_, v| {
            (v.is_empty(), v.decrease_remaining_users(), v.data.clone(), v.owner_id.clone())
        }) else {
            return Err(WebSocketStateError::NoRoom);
        };
        if !decreased {
            return Err(WebSocketStateError::RoomFull);
        }

        let owner_id_read = owner_id.read().await;
        let is_owner = if is_empty && owner_id_read.is_none() {
            drop(owner_id_read);
            *owner_id.write_owned().await = Some(id);
            is_new_owner = true;
            true
        } else {
            owner_id_read.is_some_and(|oid| Some(oid) == pref_id)
        };

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
            is_new_owner,
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
        Ok(())
    }
}
