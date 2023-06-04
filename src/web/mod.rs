use axum::Router;

use crate::server_state::ServerState;

mod room;

pub fn routes(state: ServerState) -> Router {
    Router::new().nest("/room", room::routes(state))
}
