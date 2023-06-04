use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use super::room_state::room_handle;
use super::user_state::{LocalUser, UserState};
use super::PERMISSION_ALL;
use super::{room_state::RoomState, VideoData};
use http::StatusCode;
use scc::HashIndex;
use tokio::sync::mpsc;
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
        max_users: usize,
    ) -> Result<Uuid, WebSocketStateError> {
        if max_users > MAX_USERS {
            return Err(WebSocketStateError::MaxUserExceeded);
        }
        let room_id = Uuid::new_v4();
        let (client_tx, client_rx) = mpsc::unbounded_channel();
        let (broadcast_tx, broadcast_rx) = mpsc::unbounded_channel();
        let exit_notify = Arc::new(Notify::new());
        let data = Arc::new(RwLock::new(data));
        let room = RoomState {
            id: room_id,
            client_tx,
            broadcast_tx,
            exit_notify: exit_notify.clone(),
            data: data.clone(),
            remaining_users: Arc::new(AtomicUsize::new(max_users)),
            max_users,
        };

        let _ = self.rooms.insert_async(room_id, room).await;
        tokio::spawn(room_handle(
            room_id,
            exit_notify,
            self,
            client_rx,
            broadcast_rx,
        ));

        Ok(room_id)
    }

    pub async fn verify_room(&self, room_id: Uuid) -> Result<(Uuid, String), WebSocketStateError> {
        // TODO: Check DB and get the proper ID? also the WebSocket path can be different than usual due to load balancing?
        let Some(is_full) = self.rooms.read(&room_id, |_, v| v.is_full()) else {
            return Err(WebSocketStateError::NoRoom);
        };
        if is_full {
            return Err(WebSocketStateError::RoomFull);
        }
        Ok((room_id, DEFAULT_WS.into()))
    }

    pub async fn join_room(
        &'static self,
        room_id: Uuid,
        name: String,
    ) -> Result<LocalUser, WebSocketStateError> {
        let Some((is_owner, increased, data)) = self.rooms.read(&room_id, |_, v| {
            (v.is_first_user(), v.increase_remaining_users(), v.data.clone())
        }) else {
            return Err(WebSocketStateError::NoRoom);
        };
        if !increased {
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
}
