use self::rejection::*;
use super::sturdy_tungstenite::{error::Error as WsError, Message};
use axum::async_trait;
use axum::extract::FromRequestParts;
use axum::{
    body::{self, Bytes},
    response::Response,
    Error,
};
use futures_util::{
    sink::{Sink, SinkExt},
    stream::{Stream, StreamExt},
};
use http::{
    header::{self, HeaderMap, HeaderName, HeaderValue},
    request::Parts,
    Method, StatusCode,
};
use hyper::upgrade::{OnUpgrade, Upgraded};
use sha1::{Digest, Sha1};
use std::sync::Arc;
use std::{
    borrow::Cow,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::sync::Mutex;

use super::sturdy_tungstenite::protocol::{self, WebSocketConfig};
use super::ws_stream::{self, SplitSink, SplitStream, WebSocketStream};

/// Extractor for establishing WebSocket connections.
///
/// Note: This extractor requires the request method to be `GET` so it should
/// always be used with [`get`](crate::routing::get). Requests with other methods will be
/// rejected.
///
/// See the [module docs](self) for an example.
pub struct WebSocketUpgrade<F = DefaultOnFailedUpdgrade> {
    config: WebSocketConfig,
    /// The chosen protocol sent in the `Sec-WebSocket-Protocol` header of the response.
    protocol: Option<HeaderValue>,
    sec_websocket_key: HeaderValue,
    on_upgrade: OnUpgrade,
    on_failed_upgrade: F,
    sec_websocket_protocol: Option<HeaderValue>,
}

impl<F> std::fmt::Debug for WebSocketUpgrade<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebSocketUpgrade")
            .field("config", &self.config)
            .field("protocol", &self.protocol)
            .field("sec_websocket_key", &self.sec_websocket_key)
            .field("sec_websocket_protocol", &self.sec_websocket_protocol)
            .finish_non_exhaustive()
    }
}

impl<F> WebSocketUpgrade<F> {
    /// Set the size of the internal message send queue.
    pub fn max_send_queue(mut self, max: usize) -> Self {
        self.config.max_send_queue = Some(max);
        self
    }

    /// Set the maximum message size (defaults to 64 megabytes)
    pub fn max_message_size(mut self, max: usize) -> Self {
        self.config.max_message_size = Some(max);
        self
    }

    /// Set the maximum frame size (defaults to 16 megabytes)
    pub fn max_frame_size(mut self, max: usize) -> Self {
        self.config.max_frame_size = Some(max);
        self
    }

    /// Allow server to accept unmasked frames (defaults to false)
    pub fn accept_unmasked_frames(mut self, accept: bool) -> Self {
        self.config.accept_unmasked_frames = accept;
        self
    }

    /// Set the known protocols.
    ///
    /// If the protocol name specified by `Sec-WebSocket-Protocol` header
    /// to match any of them, the upgrade response will include `Sec-WebSocket-Protocol` header and
    /// return the protocol name.
    ///
    /// The protocols should be listed in decreasing order of preference: if the client offers
    /// multiple protocols that the server could support, the server will pick the first one in
    /// this list.
    ///
    /// # Examples
    ///
    /// ```
    /// use axum::{
    ///     extract::ws::{WebSocketUpgrade, WebSocket},
    ///     routing::get,
    ///     response::{IntoResponse, Response},
    ///     Router,
    /// };
    ///
    /// let app = Router::new().route("/ws", get(handler));
    ///
    /// async fn handler(ws: WebSocketUpgrade) -> Response {
    ///     ws.protocols(["graphql-ws", "graphql-transport-ws"])
    ///         .on_upgrade(|socket| async {
    ///             // ...
    ///         })
    /// }
    /// # async {
    /// # axum::Server::bind(&"".parse().unwrap()).serve(app.into_make_service()).await.unwrap();
    /// # };
    /// ```
    pub fn protocols<I>(mut self, protocols: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<Cow<'static, str>>,
    {
        if let Some(req_protocols) = self
            .sec_websocket_protocol
            .as_ref()
            .and_then(|p| p.to_str().ok())
        {
            self.protocol = protocols
                .into_iter()
                // FIXME: This will often allocate a new `String` and so is less efficient than it
                // could be. But that can't be fixed without breaking changes to the public API.
                .map(Into::into)
                .find(|protocol| {
                    req_protocols
                        .split(',')
                        .any(|req_protocol| req_protocol.trim() == protocol)
                })
                .map(|protocol| match protocol {
                    Cow::Owned(s) => HeaderValue::from_str(&s).unwrap(),
                    Cow::Borrowed(s) => HeaderValue::from_static(s),
                });
        }

        self
    }

    /// Provide a callback to call if upgrading the connection fails.
    ///
    /// The connection upgrade is performed in a background task. If that fails this callback
    /// will be called.
    ///
    /// By default any errors will be silently ignored.
    ///
    /// # Example
    ///
    /// ```
    /// use axum::{
    ///     extract::{WebSocketUpgrade},
    ///     response::Response,
    /// };
    ///
    /// async fn handler(ws: WebSocketUpgrade) -> Response {
    ///     ws.on_failed_upgrade(|error| {
    ///         report_error(error);
    ///     })
    ///     .on_upgrade(|socket| async { /* ... */ })
    /// }
    /// #
    /// # fn report_error(_: axum::Error) {}
    /// ```
    pub fn on_failed_upgrade<C>(self, callback: C) -> WebSocketUpgrade<C>
    where
        C: OnFailedUpdgrade,
    {
        WebSocketUpgrade {
            config: self.config,
            protocol: self.protocol,
            sec_websocket_key: self.sec_websocket_key,
            on_upgrade: self.on_upgrade,
            on_failed_upgrade: callback,
            sec_websocket_protocol: self.sec_websocket_protocol,
        }
    }

    /// Finalize upgrading the connection and call the provided callback with
    /// the stream.
    #[must_use = "to setup the WebSocket connection, this response must be returned"]
    pub fn on_upgrade<C, Fut>(self, callback: C) -> Response
    where
        C: FnOnce(WebSocket) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
        F: OnFailedUpdgrade,
    {
        let on_upgrade = self.on_upgrade;
        let config = self.config;
        let on_failed_upgrade = self.on_failed_upgrade;

        let protocol = self.protocol.clone();

        tokio::spawn(async move {
            let upgraded = match on_upgrade.await {
                Ok(upgraded) => upgraded,
                Err(err) => {
                    on_failed_upgrade.call(Error::new(err));
                    return;
                }
            };

            let socket =
                WebSocketStream::from_raw_socket(upgraded, protocol::Role::Server, Some(config))
                    .await;
            let socket = WebSocket {
                inner: socket,
                protocol,
            };
            callback(socket).await;
        });

        #[allow(clippy::declare_interior_mutable_const)]
        const UPGRADE: HeaderValue = HeaderValue::from_static("upgrade");
        #[allow(clippy::declare_interior_mutable_const)]
        const WEBSOCKET: HeaderValue = HeaderValue::from_static("websocket");

        let mut builder = Response::builder()
            .status(StatusCode::SWITCHING_PROTOCOLS)
            .header(header::CONNECTION, UPGRADE)
            .header(header::UPGRADE, WEBSOCKET)
            .header(
                header::SEC_WEBSOCKET_ACCEPT,
                sign(self.sec_websocket_key.as_bytes()),
            );

        if let Some(protocol) = self.protocol {
            builder = builder.header(header::SEC_WEBSOCKET_PROTOCOL, protocol);
        }

        builder.body(body::boxed(body::Empty::new())).unwrap()
    }
}

/// What to do when a connection upgrade fails.
///
/// See [`WebSocketUpgrade::on_failed_upgrade`] for more details.
pub trait OnFailedUpdgrade: Send + 'static {
    /// Call the callback.
    fn call(self, error: Error);
}

impl<F> OnFailedUpdgrade for F
where
    F: FnOnce(Error) + Send + 'static,
{
    fn call(self, error: Error) {
        self(error)
    }
}

/// The default `OnFailedUpdgrade` used by `WebSocketUpgrade`.
///
/// It simply ignores the error.
#[non_exhaustive]
#[derive(Debug)]
pub struct DefaultOnFailedUpdgrade;

impl OnFailedUpdgrade for DefaultOnFailedUpdgrade {
    #[inline]
    fn call(self, _error: Error) {}
}

#[async_trait]
impl<S> FromRequestParts<S> for WebSocketUpgrade<DefaultOnFailedUpdgrade>
where
    S: Send + Sync,
{
    type Rejection = WebSocketUpgradeRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        if parts.method != Method::GET {
            return Err(MethodNotGet.into());
        }

        if !header_contains(&parts.headers, header::CONNECTION, "upgrade") {
            return Err(InvalidConnectionHeader.into());
        }

        if !header_eq(&parts.headers, header::UPGRADE, "websocket") {
            return Err(InvalidUpgradeHeader.into());
        }

        if !header_eq(&parts.headers, header::SEC_WEBSOCKET_VERSION, "13") {
            return Err(InvalidWebSocketVersionHeader.into());
        }

        let sec_websocket_key = parts
            .headers
            .get(header::SEC_WEBSOCKET_KEY)
            .ok_or(WebSocketKeyHeaderMissing)?
            .clone();

        let on_upgrade = parts
            .extensions
            .remove::<OnUpgrade>()
            .ok_or(ConnectionNotUpgradable)?;

        let sec_websocket_protocol = parts.headers.get(header::SEC_WEBSOCKET_PROTOCOL).cloned();

        Ok(Self {
            config: Default::default(),
            protocol: None,
            sec_websocket_key,
            on_upgrade,
            sec_websocket_protocol,
            on_failed_upgrade: DefaultOnFailedUpdgrade,
        })
    }
}

fn header_eq(headers: &HeaderMap, key: HeaderName, value: &'static str) -> bool {
    if let Some(header) = headers.get(&key) {
        header.as_bytes().eq_ignore_ascii_case(value.as_bytes())
    } else {
        false
    }
}

fn header_contains(headers: &HeaderMap, key: HeaderName, value: &'static str) -> bool {
    let header = if let Some(header) = headers.get(&key) {
        header
    } else {
        return false;
    };

    if let Ok(header) = std::str::from_utf8(header.as_bytes()) {
        header.to_ascii_lowercase().contains(value)
    } else {
        false
    }
}

/// A stream of WebSocket messages.
///
/// See [the module level documentation](self) for more details.
#[derive(Debug)]
pub struct WebSocket {
    inner: WebSocketStream<Upgraded>,
    protocol: Option<HeaderValue>,
}

impl WebSocket {
    /// Receive another message.
    ///
    /// Returns `None` if the stream has closed.
    pub async fn recv(&mut self) -> Option<Result<Message, Error>> {
        self.next().await
    }

    /// Send a message.
    pub async fn send(&mut self, msg: Message) -> Result<(), Error> {
        self.inner.send(msg).await.map_err(Error::new)
    }

    pub fn send_raw<B: AsRef<[u8]>>(&mut self, bytes: B) -> Result<(), WsError> {
        self.inner.write_raw(bytes)
    }

    /// Gracefully close this WebSocket.
    pub async fn close(mut self) -> Result<(), Error> {
        self.inner.close(None).await.map_err(Error::new)
    }

    /// Return the selected WebSocket subprotocol, if one has been chosen.
    pub fn protocol(&self) -> Option<&HeaderValue> {
        self.protocol.as_ref()
    }

    pub fn tri_split(
        self,
    ) -> (
        Arc<Mutex<WebSocket>>,
        SplitStream<WebSocket>,
        SplitSink<WebSocket, Message>,
    ) {
        let a = Arc::new(Mutex::new(self));
        let b = SplitStream(a.clone());
        let c = SplitSink(a.clone());
        (a, b, c)
    }
}

impl Stream for WebSocket {
    type Item = Result<Message, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match futures_util::ready!(self.inner.poll_next_unpin(cx)) {
                Some(Ok(msg)) => {
                    return Poll::Ready(Some(Ok(msg)));
                }
                Some(Err(err)) => return Poll::Ready(Some(Err(Error::new(err)))),
                None => return Poll::Ready(None),
            }
        }
    }
}

impl Sink<Message> for WebSocket {
    type Error = Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        <ws_stream::WebSocketStream<Upgraded> as futures_util::Sink<Message>>::poll_ready(
            Pin::new(&mut self.inner),
            cx,
        )
        .map_err(Error::new)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        <ws_stream::WebSocketStream<Upgraded> as futures_util::Sink<Message>>::start_send(
            Pin::new(&mut self.inner),
            item,
        )
        .map_err(Error::new)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        <ws_stream::WebSocketStream<Upgraded> as futures_util::Sink<Message>>::poll_flush(
            Pin::new(&mut self.inner),
            cx,
        )
        .map_err(Error::new)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        <ws_stream::WebSocketStream<Upgraded> as futures_util::Sink<Message>>::poll_close(
            Pin::new(&mut self.inner),
            cx,
        )
        .map_err(Error::new)
    }
}

fn sign(key: &[u8]) -> HeaderValue {
    use base64::engine::Engine as _;

    let mut sha1 = Sha1::default();
    sha1.update(key);
    sha1.update(&b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11"[..]);
    let b64 = Bytes::from(base64::engine::general_purpose::STANDARD.encode(sha1.finalize()));
    HeaderValue::from_maybe_shared(b64).expect("base64 is a valid value")
}

pub mod rejection {
    //! WebSocket specific rejections.
    macro_rules! __log_rejection {
        (
            rejection_type = $ty:ident,
            body_text = $body_text:expr,
            status = $status:expr,
        ) => {
            #[cfg(feature = "tracing")]
            {
                tracing::event!(
                    target: "axum::rejection",
                    tracing::Level::TRACE,
                    status = $status.as_u16(),
                    body = $body_text,
                    rejection_type = std::any::type_name::<$ty>(),
                    "rejecting request",
                );
            }
        };
    }

    macro_rules! define_rejection {
        (
            #[status = $status:ident]
            #[body = $body:expr]
            $(#[$m:meta])*
            pub struct $name:ident;
        ) => {
            $(#[$m])*
            #[derive(Debug)]
            #[non_exhaustive]
            pub struct $name;

            impl axum::response::IntoResponse for $name {
                fn into_response(self) -> axum::response::Response {
                    __log_rejection!(
                        rejection_type = $name,
                        body_text = $body,
                        status = http::StatusCode::$status,
                    );
                    (self.status(), $body).into_response()
                }
            }

            impl $name {
                /// Get the response body text used for this rejection.
                pub fn body_text(&self) -> String {
                    $body.into()
                }

                /// Get the status code used for this rejection.
                pub fn status(&self) -> http::StatusCode {
                    http::StatusCode::$status
                }
            }

            impl std::fmt::Display for $name {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "{}", $body)
                }
            }

            impl std::error::Error for $name {}

            impl Default for $name {
                fn default() -> Self {
                    Self
                }
            }
        };

        (
            #[status = $status:ident]
            #[body = $body:expr]
            $(#[$m:meta])*
            pub struct $name:ident (Error);
        ) => {
            $(#[$m])*
            #[derive(Debug)]
            pub struct $name(pub(crate) $crate::Error);

            impl $name {
                pub(crate) fn from_err<E>(err: E) -> Self
                where
                    E: Into<$crate::BoxError>,
                {
                    Self($crate::Error::new(err))
                }
            }

            impl $crate::response::IntoResponse for $name {
                fn into_response(self) -> $crate::response::Response {
                    $crate::__log_rejection!(
                        rejection_type = $name,
                        body_text = self.body_text(),
                        status = http::StatusCode::$status,
                    );
                    (self.status(), self.body_text()).into_response()
                }
            }

            impl $name {
                /// Get the response body text used for this rejection.
                pub fn body_text(&self) -> String {
                    format!(concat!($body, ": {}"), self.0).into()
                }

                /// Get the status code used for this rejection.
                pub fn status(&self) -> http::StatusCode {
                    http::StatusCode::$status
                }
            }

            impl std::fmt::Display for $name {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "{}", $body)
                }
            }

            impl std::error::Error for $name {
                fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                    Some(&self.0)
                }
            }
        };
    }

    macro_rules! composite_rejection {
        (
            $(#[$m:meta])*
            pub enum $name:ident {
                $($variant:ident),+
                $(,)?
            }
        ) => {
            $(#[$m])*
            #[derive(Debug)]
            #[non_exhaustive]
            pub enum $name {
                $(
                    #[allow(missing_docs)]
                    $variant($variant)
                ),+
            }

            impl axum::response::IntoResponse for $name {
                fn into_response(self) -> axum::response::Response {
                    match self {
                        $(
                            Self::$variant(inner) => inner.into_response(),
                        )+
                    }
                }
            }

            impl $name {
                /// Get the response body text used for this rejection.
                pub fn body_text(&self) -> String {
                    match self {
                        $(
                            Self::$variant(inner) => inner.body_text(),
                        )+
                    }
                }

                /// Get the status code used for this rejection.
                pub fn status(&self) -> http::StatusCode {
                    match self {
                        $(
                            Self::$variant(inner) => inner.status(),
                        )+
                    }
                }
            }

            $(
                impl From<$variant> for $name {
                    fn from(inner: $variant) -> Self {
                        Self::$variant(inner)
                    }
                }
            )+

            impl std::fmt::Display for $name {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    match self {
                        $(
                            Self::$variant(inner) => write!(f, "{}", inner),
                        )+
                    }
                }
            }

            impl std::error::Error for $name {
                fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                    match self {
                        $(
                            Self::$variant(inner) => Some(inner),
                        )+
                    }
                }
            }
        };
    }

    define_rejection! {
        #[status = METHOD_NOT_ALLOWED]
        #[body = "Request method must be `GET`"]
        /// Rejection type for [`WebSocketUpgrade`](super::WebSocketUpgrade).
        pub struct MethodNotGet;
    }

    define_rejection! {
        #[status = BAD_REQUEST]
        #[body = "Connection header did not include 'upgrade'"]
        /// Rejection type for [`WebSocketUpgrade`](super::WebSocketUpgrade).
        pub struct InvalidConnectionHeader;
    }

    define_rejection! {
        #[status = BAD_REQUEST]
        #[body = "`Upgrade` header did not include 'websocket'"]
        /// Rejection type for [`WebSocketUpgrade`](super::WebSocketUpgrade).
        pub struct InvalidUpgradeHeader;
    }

    define_rejection! {
        #[status = BAD_REQUEST]
        #[body = "`Sec-WebSocket-Version` header did not include '13'"]
        /// Rejection type for [`WebSocketUpgrade`](super::WebSocketUpgrade).
        pub struct InvalidWebSocketVersionHeader;
    }

    define_rejection! {
        #[status = BAD_REQUEST]
        #[body = "`Sec-WebSocket-Key` header missing"]
        /// Rejection type for [`WebSocketUpgrade`](super::WebSocketUpgrade).
        pub struct WebSocketKeyHeaderMissing;
    }

    define_rejection! {
        #[status = UPGRADE_REQUIRED]
        #[body = "WebSocket request couldn't be upgraded since no upgrade state was present"]
        /// Rejection type for [`WebSocketUpgrade`](super::WebSocketUpgrade).
        ///
        /// This rejection is returned if the connection cannot be upgraded for example if the
        /// request is HTTP/1.0.
        ///
        /// See [MDN] for more details about connection upgrades.
        ///
        /// [MDN]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Upgrade
        pub struct ConnectionNotUpgradable;
    }

    composite_rejection! {
        /// Rejection used for [`WebSocketUpgrade`](super::WebSocketUpgrade).
        ///
        /// Contains one variant for each way the [`WebSocketUpgrade`](super::WebSocketUpgrade)
        /// extractor can fail.
        pub enum WebSocketUpgradeRejection {
            MethodNotGet,
            InvalidConnectionHeader,
            InvalidUpgradeHeader,
            InvalidWebSocketVersionHeader,
            WebSocketKeyHeaderMissing,
            ConnectionNotUpgradable,
        }
    }
}
