use std::{net::SocketAddr, str::FromStr};

use axum::extract::State;
use axum::{
    extract::ConnectInfo, headers, response::IntoResponse, routing::get, Router, Server,
    TypedHeader,
};

use http::StatusCode;
use tokio;
use tower_cookies::{Cookie, CookieManagerLayer, Cookies};
use tower_http::services::ServeDir;
use ws_handler::ws_state::DEFAULT_WS;

mod basic_auth;
mod common;
mod server_state;
pub mod sturdy_ws;
mod web;
mod ws_handler;

use crate::basic_auth::{OWNER_AUTH_CHECKED_COOKIE, OWNER_AUTH_COOKIE};
use crate::common::Id;
use crate::ws_handler::validate_and_handle_client;
use server_state::ServerState;
use sturdy_ws::WebSocketUpgrade;

#[tokio::main]
async fn main() {
    let server_state = ServerState::new();
    run_server(server_state).await;
}

async fn run_server(state: ServerState) {
    let app = Router::new()
        .fallback_service(
            ServeDir::new(state.get_static_dir()).append_index_html_on_directories(true),
        )
        .nest_service("/js", ServeDir::new(state.get_js_dir()))
        .merge(ws_route(state.clone()))
        .merge(web::routes(state))
        .layer(CookieManagerLayer::new());

    let port = std::env::var("PORT").unwrap_or(String::from("8080"));
    let addr = SocketAddr::from_str(&format!("0.0.0.0:{}", port)).unwrap();
    println!("Listening on {}...", addr);

    Server::bind(&addr)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .unwrap();
}

fn ws_route(state: ServerState) -> Router {
    let ws_path: &str = &format!("/{}", DEFAULT_WS);
    Router::new()
        .route(ws_path, get(ws_handler))
        .with_state(state)
}

// TODO: Move this logic into `ws_handler` or something else?
async fn ws_handler(
    cookies: Cookies,
    ws: WebSocketUpgrade,
    State(server): State<ServerState>,
    user_agent: Option<TypedHeader<headers::UserAgent>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    let user_agent = if let Some(TypedHeader(user_agent)) = user_agent {
        user_agent.to_string()
    } else {
        return (StatusCode::FORBIDDEN, "Unknown User agent").into_response();
    };

    println!("`{user_agent}` at {addr} connected.");

    let owner = match cookies.get(OWNER_AUTH_CHECKED_COOKIE) {
        Some(cookie) => match Id::from_str(cookie.value()) {
            Err(_) => None,
            Ok(id) => {
                match server
                    .ws_state
                    .remove_checked_auth(id, |(v, _)| v.is_valid(addr.ip(), &user_agent))
                    .await
                    .ok()
                {
                    None => {
                        cookies.remove(Cookie::new(OWNER_AUTH_COOKIE, ""));
                        None
                    }
                    v => v,
                }
            }
        },
        None => None,
    };
    cookies.remove(Cookie::new(OWNER_AUTH_CHECKED_COOKIE, ""));

    ws.on_upgrade(move |socket| async move {
        validate_and_handle_client(server.ws_state, socket, addr, owner).await;
    })
}
