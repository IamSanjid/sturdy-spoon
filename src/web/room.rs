use std::str::FromStr;

use axum::extract::Path;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::get;
use axum::routing::post;
use axum::Json;
use axum::Router;
use http::header;
use http::StatusCode;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

use base64::{
    alphabet,
    engine::{self, general_purpose},
    Engine as _,
};

use crate::server_state::ServerState;
use crate::ws_handler::ws_state::WebSocketStateError;
use crate::ws_handler::VideoData;
use crate::ws_handler::PERMISSION_CONTROLLABLE;

const URL_SAFE_ENGINE: engine::GeneralPurpose =
    engine::GeneralPurpose::new(&alphabet::URL_SAFE, general_purpose::NO_PAD);

#[derive(Debug, Deserialize)]
struct CreateRoomPayload {
    name: String,
    video_url: String,
    max_users: isize,
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
    name: String,
    ws_path: String,
}

// TODO: /room/:id = auto join to the spcific id!
pub(super) fn routes(server_state: ServerState) -> Router {
    Router::new()
        .route("/create", post(create))
        .route("/join", post(join))
        .route("/:id", get(join_direct))
        .with_state(server_state)
}

fn parse_uuid_from_base64(id: String) -> Result<Uuid, impl IntoResponse> {
    // URL_SAFE_ENGINE
    //     .decode(id)
    //     .as_deref()
    //     .map_err(|err| (StatusCode::BAD_REQUEST, format!("Error: {}", err)))
    //     .and_then(|v| std::str::from_utf8(v).map_err(|err| (StatusCode::BAD_REQUEST, format!("Error: {}", err))))
    //     .and_then(|v| Uuid::from_str(v).map_err(|err| (StatusCode::BAD_REQUEST, format!("Error: {}", err))))
    match URL_SAFE_ENGINE.decode(id).as_deref() {
        Ok(vec) => match std::str::from_utf8(vec) {
            Ok(id) => match Uuid::from_str(id) {
                Ok(id) => return Ok(id),
                Err(err) => return Err((StatusCode::BAD_REQUEST, format!("Error: {}", err))),
            },
            Err(err) => return Err((StatusCode::BAD_REQUEST, format!("Error: {}", err))),
        },
        Err(err) => return Err((StatusCode::BAD_REQUEST, format!("Error: {}", err))),
    }
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
    let max_users = create_room_payload.max_users.abs() as usize;
    let (id, ws_path) = state
        .ws_state
        .create_room(data, create_room_payload.name, max_users)
        .await?;

    Ok(Json(Room { id, ws_path }))
}

async fn join(
    State(state): State<ServerState>,
    Json(join_room_payload): Json<JoinRoomPayload>,
) -> Result<Json<JoinUser>, WebSocketStateError> {
    let (room_id, name, ws_path) = state
        .ws_state
        .verify_room(join_room_payload.room_id)
        .await?;
    Ok(Json(JoinUser {
        room_id,
        name,
        ws_path,
    }))
}

async fn join_direct(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, impl IntoResponse> {
    let id = parse_uuid_from_base64(id).map_err(|e| e.into_response())?;
    let (room_id, name, ws_path) = state
        .ws_state
        .verify_room(id)
        .await
        .map_err(|e| e.into_response())?;

    let mut file = match tokio::fs::read_to_string(state.get_dyn_dir().join("room-min.html")).await
    {
        Ok(file) => file,
        Err(err) => {
            return Err((StatusCode::BAD_REQUEST, format!("Error: {}", err)).into_response())
        }
    };

    file = file.replace(
        "let room_data = null;",
        &format!(
            r#"let room_data = {{
                room_id: '{}',
                name: '{}',
                ws_path: '{}'
            }};
            "#,
            room_id, name, ws_path
        ),
    );
    let header = Response::builder()
        .header(header::CONTENT_TYPE, "text/html")
        .body(file);
    Ok(header.unwrap())
}
