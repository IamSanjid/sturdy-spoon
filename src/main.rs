use std::ops::ControlFlow;
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

use server_state::ServerState;
use sturdy_ws::{Message, WebSocket, WebSocketUpgrade};

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
        let _ = check_join(socket, addr).await;
    })
}

async fn check_join(mut socket: WebSocket, who: SocketAddr) -> Result<(), impl IntoResponse> {
    if socket.send(Message::Ping(vec![1, 2, 3])).await.is_ok() {
        println!("Pinged {}...", who);
    } else {
        println!("Could not send ping {}!", who);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    if let Some(msg) = socket.recv().await {
        if let Ok(msg) = msg {
            if process_message(msg, who).is_break() {
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        } else {
            println!("client {who} abruptly disconnected");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    return Ok(());
}

fn process_message(msg: Message, who: SocketAddr) -> ControlFlow<(), ()> {
    match msg {
        Message::Text(t) => {
            println!(">>> {} sent str: {:?}", who, t);
        }
        Message::Binary(d) => {
            println!(">>> {} sent {} bytes: {:?}", who, d.len(), d);
        }
        Message::Close(c) => {
            if let Some(cf) = c {
                println!(
                    ">>> {} sent close with code {} and reason `{}`",
                    who, cf.code, cf.reason
                );
            } else {
                println!(">>> {} somehow sent close message without CloseFrame", who);
            }
            return ControlFlow::Break(());
        }

        Message::Pong(v) => {
            println!(">>> {} sent pong with {:?}", who, v);
        }
        Message::Ping(v) => {
            println!(">>> {} sent ping with {:?}", who, v);
        }
        _ => {}
    }
    ControlFlow::Continue(())
}
