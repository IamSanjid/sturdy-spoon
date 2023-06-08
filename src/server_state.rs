use std::{collections::HashMap, path::PathBuf};

use axum::extract::FromRef;

use crate::ws_handler::ws_state::WsState;

#[derive(Hash, PartialEq, Eq)]
pub enum WebPageFileType {
    Static,
    Dynamic,
    JS,
}

#[derive(Clone, FromRef)]
pub struct ServerState {
    pub ws_state: &'static WsState,
    pub web_dirs: &'static HashMap<WebPageFileType, PathBuf>,
}

impl ServerState {
    pub fn new() -> Self {
        let statics_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("web_pages")
            .join("statics");
        let dyn_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("web_pages")
            .join("dynamic");
        let js_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("web_pages")
            .join("js");
        let web_dirs = Box::leak(Box::new(HashMap::from_iter([
            (WebPageFileType::Static, statics_dir),
            (WebPageFileType::Dynamic, dyn_dir),
            (WebPageFileType::JS, js_dir),
        ])));

        let ws_state = Box::leak(Box::new(WsState::default()));
        Self { ws_state, web_dirs }
    }

    pub fn get_static_dir(&self) -> &PathBuf {
        &self.web_dirs[&WebPageFileType::Static]
    }

    pub fn get_dyn_dir(&self) -> &PathBuf {
        &self.web_dirs[&WebPageFileType::Dynamic]
    }

    pub fn get_js_dir(&self) -> &PathBuf {
        &self.web_dirs[&WebPageFileType::JS]
    }
}
