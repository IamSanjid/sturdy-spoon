use std::ops::ControlFlow;
use std::{net::SocketAddr, path::PathBuf, str::FromStr};

use axum::extract::State;
use axum::{
    extract::ConnectInfo, headers, response::IntoResponse, routing::get, Router, Server,
    TypedHeader,
};

use futures_util::stream::FuturesUnordered;
use futures_util::StreamExt;
use http::StatusCode;
use tokio;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tower_http::services::ServeDir;

mod server_state;
pub mod sturdy_ws;

use server_state::ServerState;
use sturdy_ws::{Message, WebSocket, WebSocketUpgrade};

use crate::sturdy_ws::Frame;

type WSender = UnboundedSender<(WebSocket, SocketAddr)>;
type WReciever = UnboundedReceiver<(WebSocket, SocketAddr)>;
type WSMsgReciever = UnboundedReceiver<Message>;

async fn handle(mut rx: WReciever, mut msg_rx: WSMsgReciever) {
    let mut senders = Vec::new();
    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some((mut socket, who)) => {
                        client_handler(&mut socket, who).await;
                        //let (sender, _) = socket.split::<&[u8]>();
                        let (sender, mut receiver) = socket.split();
                        senders.push(sender);
                        tokio::spawn(async move {
                            while let Some(Ok(msg)) = receiver.next().await {
                                println!("{}: {:?}", who, msg);
                            }
                        });
                    },
                    None => break,
                }
            },
            send_msg = msg_rx.recv() => {
                match send_msg {
                    Some(msg) => {
                        let frame: Frame = msg.into();
                        let bytes: Vec<u8> = frame.into();
                        let futures = FuturesUnordered::default();
                        for socket in &mut senders {
                            futures.push(async {
                                socket.lock().await.send_raw(&bytes)
                            });
                        }
                        let _: Vec<_> = futures.collect().await;
                    },
                    None => break,
                }
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let server_state = ServerState::new();
    let (tx, rx) = mpsc::unbounded_channel();
    let (msg_tx, msg_rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let _ = msg_tx.send(Message::Text("Ping after 5 sec".to_owned()));
        }
    });

    tokio::spawn(handle(rx, msg_rx));
    run_server(server_state, tx).await;
}

async fn run_server(_server_state: ServerState, tx: WSender) {
    let statics_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("statics");

    let app = Router::new()
        .fallback_service(ServeDir::new(statics_dir).append_index_html_on_directories(true))
        .route("/ws", get(ws_handler))
        .with_state(tx);
    //.with_state(server_state);

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
    State(tx): State<WSender>,
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
        let _ = tx.send((socket, addr));
    })
}

async fn client_handler(socket: &mut WebSocket, who: SocketAddr) {
    if socket.send(Message::Ping(vec![1, 2, 3])).await.is_ok() {
        println!("Pinged {}...", who);
    } else {
        println!("Could not send ping {}!", who);
        return;
    }

    if let Some(msg) = socket.recv().await {
        if let Ok(msg) = msg {
            if process_message(msg, who).is_break() {
                return;
            }
        } else {
            println!("client {who} abruptly disconnected");
            return;
        }
    }

    for i in 1..5 {
        if socket
            .send(Message::Text(format!("Hi {i} times!")))
            .await
            .is_err()
        {
            println!("client {who} abruptly disconnected");
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
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
