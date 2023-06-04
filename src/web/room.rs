use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use axum::Router;

use axum::routing::post;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

use crate::server_state::ServerState;

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

pub(super) fn routes(server_state: ServerState) -> Router {
    Router::new()
        .route("/create", post(create))
        .route("/join", post(join))
        .with_state(server_state)
}

async fn create(
    State(state): State<ServerState>,
    Json(create_room_payload): Json<CreateRoomPayload>,
) -> Result<Json<Room>, impl IntoResponse> {
    Err(StatusCode::NOT_IMPLEMENTED.into_response())
}

async fn join(
    State(state): State<ServerState>,
    Json(join_room_payload): Json<JoinRoomPayload>,
) -> Result<Json<Room>, impl IntoResponse> {
    Err(StatusCode::NOT_IMPLEMENTED.into_response())
}
