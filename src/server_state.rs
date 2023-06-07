use axum::extract::FromRef;

use crate::ws_handler::ws_state::WsState;

#[derive(Clone, FromRef)]
pub struct ServerState {
    pub ws_state: &'static WsState,
}

impl ServerState {
    pub fn new() -> Self {
        let ws_state = Box::leak(Box::new(WsState::default()));
        Self { ws_state }
    }
}
