use std::{net::SocketAddr, path::PathBuf, str::FromStr};

use axum::extract::State;
use axum::{
    extract::ConnectInfo, headers, response::IntoResponse, routing::get, Router, Server,
    TypedHeader,
};

use http::StatusCode;
use tokio;
use tower_http::services::ServeDir;

mod common;
mod server_state;
pub mod sturdy_ws;
mod web;
mod ws_handler;

use crate::ws_handler::validate_and_handle_client;
use server_state::ServerState;
use sturdy_ws::WebSocketUpgrade;

#[tokio::main]
async fn main() {
    let server_state = ServerState::new();
    run_server(server_state).await;
}

async fn run_server(state: ServerState) {
    let statics_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("statics");

    let app = Router::new()
        .fallback_service(ServeDir::new(statics_dir).append_index_html_on_directories(true))
        .route("/ws", get(ws_handler))
        .with_state(state.clone())
        .merge(web::routes(state));

    let port = std::env::var("PORT").unwrap_or(String::from("8080"));
    let addr = SocketAddr::from_str(&format!("0.0.0.0:{}", port)).unwrap();
    println!("Listening on {}...", addr);

    Server::bind(&addr)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .unwrap();
}

async fn ws_handler(
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
    ws.on_upgrade(move |socket| async move {
        validate_and_handle_client(server.ws_state, socket, addr).await;
    })
}
