use std::net::SocketAddr;
use std::str::FromStr;

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

/*use base64::{
    alphabet,
    engine::{self, general_purpose},
    Engine as _,
};*/
use tower_cookies::cookie::time::Duration;
use tower_cookies::cookie::time::OffsetDateTime;
use tower_cookies::Cookie;
use tower_cookies::Cookies;

use crate::basic_auth::OwnerAuth;
use crate::basic_auth::CHECKED_AUTH_EXPIRATION;
use crate::basic_auth::EXPIRATION;
use crate::basic_auth::OWNER_AUTH_CHECKED_COOKIE;
use crate::basic_auth::OWNER_AUTH_COOKIE;
use crate::common::{utils, Id};
use crate::server_state::ServerState;
use crate::ws_handler::PlayerType;
use crate::ws_handler::VideoData;
use crate::ws_handler::PERMISSION_CONTROLLABLE;
use crate::ws_handler::PLAYER_MAX;

// const URL_SAFE_B64: engine::GeneralPurpose =
//     engine::GeneralPurpose::new(&alphabet::URL_SAFE, general_purpose::NO_PAD);

#[derive(Debug, Deserialize)]
struct CreateRoomPayload {
    name: String,
    creator_name: String,
    video_url: String,
    cc_url: String,
    max_users: i32,
    global_control: bool,
    player_index: PlayerType,
}

#[derive(Serialize)]
struct Room {
    id: String,
    ws_path: String,
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
    auto_connect: bool,
}

pub(super) fn routes(server_state: ServerState) -> Router {
    Router::new()
        .route("/create", post(create))
        .route("/join", post(join))
        .route("/:id", get(join_direct))
        .with_state(server_state)
}

/*
fn parse_uuid_from_base64(id: String) -> Result<Id, impl IntoResponse> {
    fn format_error<T: Display>(error: T) -> String {
        format!("Error: {}", error)
    }
    let decode_res = URL_SAFE_B64
.decode(id)
        .as_deref()
        .map_err(format_error)
        .and_then(|id_bytes| std::str::from_utf8(id_bytes).map_err(format_error))
        .and_then(|id_str| Id::from_str(id_str).map_err(format_error));

    match decode_res {
        Err(e) => {
            println!("[[Parse Uuid B64 Error]] {}", e);
            return Err((StatusCode::BAD_REQUEST, "Bad Uuid was provided."));
        }
        Ok(ok) => Ok(ok),
    }
}
*/

/*fn parse_usize_from_base64(id: String) -> Result<usize, impl IntoResponse> {
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
}*/

async fn validate_cookie(
    cookies: Cookies,
    user_agent: String,
    room_id: &Id,
    state: &ServerState,
    addr: &SocketAddr,
) -> bool {
    let auth = match cookies.get(OWNER_AUTH_COOKIE) {
        Some(cookie) => match OwnerAuth::from_token(cookie.value(), &state.ws_state.keys) {
            Err(e) => {
                println!("OwnerAuth Error: {}", e);
                None
            }
            Ok(auth) => {
                if !auth.is_valid_room_id(addr.ip(), &user_agent, room_id) {
                    None
                } else {
                    Some(auth)
                }
            }
        },
        None => None,
    };

    if let Some(auth) = auth {
        let checked_id = state.ws_state.add_checked_auth(auth).await;
        let mut checked_auth_cookie =
            Cookie::new(OWNER_AUTH_CHECKED_COOKIE, checked_id.to_string());
        checked_auth_cookie.set_expires(
            OffsetDateTime::now_utc() + Duration::milliseconds(CHECKED_AUTH_EXPIRATION as i64),
        );
        checked_auth_cookie.set_http_only(true);
        cookies.add(checked_auth_cookie);
        true
    } else {
        cookies.remove(Cookie::new(OWNER_AUTH_COOKIE, ""));
        false
    }
}

async fn create(
    cookies: Cookies,
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

    if create_room_payload.player_index > PLAYER_MAX {
        return Err((StatusCode::BAD_REQUEST, "Player Index out of bounds.").into_response());
    }
    let mut data = VideoData::new(
        create_room_payload.video_url,
        create_room_payload.cc_url,
        create_room_payload.player_index,
    );

    if create_room_payload.global_control {
        data.set_permission(PERMISSION_CONTROLLABLE)
    }

    let max_users = create_room_payload.max_users.abs() as u32;
    let (id, ws_path) = state
        .ws_state
        .create_room(data, create_room_payload.name, max_users)
        .await
        .map_err(|err| err.into_response())?;

    let expires = utils::get_elapsed_milis() + EXPIRATION;
    let auth = OwnerAuth::new(
        create_room_payload.creator_name,
        id,
        addr.ip(),
        user_agent,
        expires,
    )
    .encode(&state.ws_state.keys);

    let mut owner_auth_cookie = Cookie::new(OWNER_AUTH_COOKIE, auth);
    owner_auth_cookie
        .set_expires(OffsetDateTime::now_utc() + Duration::milliseconds(EXPIRATION as i64));
    owner_auth_cookie.set_http_only(true);
    cookies.add(owner_auth_cookie);

    let id = id.to_string();
    Ok(Json(Room { id, ws_path }))
}

async fn join(
    cookies: Cookies,
    State(state): State<ServerState>,
    user_agent: Option<TypedHeader<headers::UserAgent>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(join_room_payload): Json<JoinRoomPayload>,
) -> Result<Json<JoinUser>, impl IntoResponse> {
    let user_agent = if let Some(TypedHeader(user_agent)) = user_agent {
        user_agent.to_string()
    } else {
        return Err((StatusCode::FORBIDDEN, "Unknown User agent").into_response());
    };
    let (room_id, name, ws_path) = state
        .ws_state
        .verify_room(join_room_payload.room_id)
        .map_err(|e| e.into_response())?;
    let auto_connect = validate_cookie(cookies, user_agent, &room_id, &state, &addr).await;
    Ok(Json(JoinUser {
        room_id,
        name,
        ws_path,
        auto_connect,
    }))
}

async fn join_direct(
    cookies: Cookies,
    State(state): State<ServerState>,
    user_agent: Option<TypedHeader<headers::UserAgent>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, impl IntoResponse> {
    let user_agent = if let Some(TypedHeader(user_agent)) = user_agent {
        user_agent.to_string()
    } else {
        return Err((StatusCode::FORBIDDEN, "Unknown User agent").into_response());
    };

    let id = Id::from_str(id.as_str())
        .map_err(|_| (StatusCode::BAD_REQUEST, "Bad Room Id was provided.").into_response())?;
    let (room_id, name, ws_path) = state
        .ws_state
        .verify_room(id)
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

    let auto_connect = validate_cookie(cookies, user_agent, &room_id, &state, &addr).await;

    // this is peak server side html generation ;')
    file = file
        .replace(
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
        )
        .replace(
            "let autoConnect = false;",
            &format!("let autoConnect = {};", auto_connect),
        );
    let header = Response::builder()
        .header(header::CONTENT_TYPE, "text/html")
        .body(file);
    Ok(header.unwrap())
}
