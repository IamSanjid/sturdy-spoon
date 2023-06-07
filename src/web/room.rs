use axum::extract::State;
use axum::routing::post;
use axum::Json;
use axum::Router;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

use crate::server_state::ServerState;
use crate::ws_handler::ws_state::WebSocketStateError;
use crate::ws_handler::VideoData;
use crate::ws_handler::PERMISSION_CONTROLLABLE;

#[derive(Debug, Deserialize)]
struct CreateRoomPayload {
    name: String,
    video_url: String,
    max_users: usize,
    global_control: bool,
}

#[derive(Serialize)]
struct Room {
    id: Uuid,
    ws_path: String,
}

#[derive(Debug, Deserialize)]
struct JoinRoomPayload {
    room_id: Uuid,
}

#[derive(Serialize)]
struct JoinUser {
    room_id: Uuid,
    ws_path: String,
}

// TODO: /room/:id = auto join to the spcific id!
pub(super) fn routes(server_state: ServerState) -> Router {
    Router::new()
        .route("/create", post(create))
        .route("/join", post(join))
        .with_state(server_state)
}

async fn create(
    State(state): State<ServerState>,
    Json(create_room_payload): Json<CreateRoomPayload>,
) -> Result<Json<Room>, WebSocketStateError> {
    let data = if create_room_payload.global_control {
        VideoData::new(create_room_payload.video_url).with_permission(PERMISSION_CONTROLLABLE)
    } else {
        VideoData::new(create_room_payload.video_url)
    };
    let (id, ws_path) = state
        .ws_state
        .create_room(
            data,
            create_room_payload.name,
            create_room_payload.max_users,
        )
        .await?;

    Ok(Json(Room { id, ws_path }))
}

async fn join(
    State(state): State<ServerState>,
    Json(join_room_payload): Json<JoinRoomPayload>,
) -> Result<Json<JoinUser>, WebSocketStateError> {
    let (room_id, ws_path) = state
        .ws_state
        .verify_room(join_room_payload.room_id)
        .await?;
    Ok(Json(JoinUser { room_id, ws_path }))
}
