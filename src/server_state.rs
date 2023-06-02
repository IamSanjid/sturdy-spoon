use axum::extract::FromRef;

#[derive(Clone, FromRef)]
pub struct ServerState {}

impl ServerState {
    pub fn new() -> Self {
        Self {}
    }
}
