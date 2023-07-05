use std::fmt::Display;
use std::net::SocketAddr;

use axum::extract::ConnectInfo;
use axum::extract::Path;
use axum::extract::State;
use axum::headers;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::get;
use axum::routing::post;
use axum::Json;
use axum::Router;
use axum::TypedHeader;
use http::header;
use http::StatusCode;
use serde::Deserialize;
use serde::Serialize;

use base64::{
    alphabet,
    engine::{self, general_purpose},
    Engine as _,
};

use crate::basic_auth::OwnerAuth;
use crate::basic_auth::EXPIRATION;
use crate::common::utils::basic_pad;
use crate::common::utils::basic_unpad;
use crate::common::{utils, Id};
use crate::server_state::ServerState;
use crate::ws_handler::ws_state::WebSocketStateError;
use crate::ws_handler::VideoData;
use crate::ws_handler::PERMISSION_CONTROLLABLE;
use crate::ws_handler::PLAYER_MAX;

const URL_SAFE_B64: engine::GeneralPurpose =
    engine::GeneralPurpose::new(&alphabet::URL_SAFE, general_purpose::NO_PAD);

#[derive(Debug, Deserialize)]
struct CreateRoomPayload {
    name: String,
    video_url: String,
    cc_url: String,
    max_users: isize,
    global_control: bool,
    player_index: usize,
}

#[derive(Serialize)]
struct Room {
    id: String,
    ws_path: String,
    auth: String,
}

#[derive(Debug, Deserialize)]
struct JoinRoomPayload {
    room_id: Id,
}

#[derive(Serialize)]
struct JoinUser {
    room_id: Id,
    name: String,
    ws_path: String,
}

pub(super) fn routes(server_state: ServerState) -> Router {
    Router::new()
        .route("/create", post(create))
        .route("/join", post(join))
        .route("/:id", get(join_direct))
        .with_state(server_state)
}

/*
fn parse_uuid_from_base64(id: String) -> Result<Uuid, impl IntoResponse> {
    fn format_error<T: Display>(error: T) -> String {
        format!("Error: {}", error)
    }
    let decode_res = URL_SAFE_B64
        .decode(id)
        .as_deref()
        .map_err(format_error)
        .and_then(|id_bytes| std::str::from_utf8(id_bytes).map_err(format_error))
        .and_then(|id_str| Uuid::from_str(id_str).map_err(format_error));

    match decode_res {
        Err(e) => {
            println!("[[Parse Uuid B64 Error]] {}", e);
            return Err((StatusCode::BAD_REQUEST, "Bad Uuid was provided."));
        }
        Ok(ok) => Ok(ok),
    }
}
*/
fn parse_usize_from_base64(id: String) -> Result<usize, impl IntoResponse> {
    fn format_error<T: Display>(error: T) -> String {
        format!("Error: {}", error)
    }
    let decode_res = URL_SAFE_B64
        .decode(id)
        .as_deref()
        .map_err(format_error)
        .and_then(|id_bytes| std::str::from_utf8(basic_unpad(id_bytes)).map_err(format_error))
        .and_then(|id_str| id_str.parse::<usize>().map_err(format_error));

    match decode_res {
        Err(e) => {
            println!("[[Parse Usize B64 Error]] {}", e);
            return Err((StatusCode::BAD_REQUEST, "Bad Usize was provided."));
        }
        Ok(ok) => Ok(ok),
    }
}

async fn create(
    user_agent: Option<TypedHeader<headers::UserAgent>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<ServerState>,
    Json(create_room_payload): Json<CreateRoomPayload>,
) -> Result<Json<Room>, impl IntoResponse> {
    let user_agent = if let Some(TypedHeader(user_agent)) = user_agent {
        user_agent.to_string()
    } else {
        return Err((StatusCode::FORBIDDEN, "Unknown User agent").into_response());
    };

    println!("Creator: {}", user_agent);

    if create_room_payload.player_index > PLAYER_MAX {
        return Err((StatusCode::BAD_REQUEST, "Player Index out of bounds.").into_response());
    }
    let data = if create_room_payload.global_control {
        VideoData::new(
            create_room_payload.video_url,
            create_room_payload.cc_url,
            create_room_payload.player_index,
        )
        .with_permission(PERMISSION_CONTROLLABLE)
    } else {
        VideoData::new(
            create_room_payload.video_url,
            create_room_payload.cc_url,
            create_room_payload.player_index,
        )
    };
    let max_users = create_room_payload.max_users.abs() as usize;
    let (id, ws_path) = state
        .ws_state
        .create_room(data, create_room_payload.name, max_users)
        .await
        .map_err(|err| err.into_response())?;

    let expires = utils::get_elapsed_milis() + EXPIRATION;
    let auth = OwnerAuth::new(id, addr.ip(), user_agent, expires).encode(&state.ws_state.keys);

    let id = URL_SAFE_B64.encode(basic_pad(id.to_string(), 8));
    Ok(Json(Room { id, ws_path, auth }))
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
    let id = parse_usize_from_base64(id).map_err(|e| e.into_response())?;
    let (room_id, name, ws_path) = state
        .ws_state
        .verify_room(id)
        .await
        .map_err(|e| e.into_response())?;

    let mut file = match tokio::fs::read_to_string(state.get_dyn_dir().join("room-min.html")).await
    {
        Ok(file) => file,
        Err(err) => {
            println!("room-min.html: {}", err);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Unexpectedly some required web page caused an error.",
            )
                .into_response());
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
