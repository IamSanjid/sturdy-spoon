//! Lightweight, flexible WebSockets for Rust.
#![deny(
    missing_docs,
    missing_copy_implementations,
    missing_debug_implementations,
    trivial_casts,
    trivial_numeric_casts,
    unstable_features,
    unused_must_use,
    unused_mut,
    unused_imports,
    unused_import_braces
)]
pub mod error;
pub mod protocol;
pub mod util;

pub use self::{
    error::{Error, Result},
    protocol::{Message, WebSocket},
};
