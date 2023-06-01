//! Async WebSocket usage.
//!
//! This library is an implementation of WebSocket handshakes and streams. It
//! is based on the crate which implements all required WebSocket protocol
//! logic. So this crate basically just brings tokio support / tokio integration
//! to it.
//!
//! Each WebSocket stream implements the required `Stream` and `Sink` traits,
//! so the socket is just a stream of messages coming in and going out.

#![deny(missing_docs, unused_must_use, unused_mut, unused_imports, unused_import_braces)]
use std::io::{Read, Write};

use super::compat::{cvt, AllowStd, ContextWaker};
use futures_util::{
    sink::{Sink, SinkExt},
    stream::{FusedStream, Stream}, Future,
};
use log::*;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite};
use super::sturdy_tungstenite::{
    self as tungstenite,
    error::Error as WsError,
    protocol::{Message, Role, WebSocket, WebSocketConfig},
};

use super::sturdy_tungstenite::protocol::CloseFrame;

/// A wrapper around an underlying raw stream which implements the WebSocket
/// protocol.
///
/// A `WebSocketStream<S>` represents a handshake that has been completed
/// successfully and both the server and the client are ready for receiving
/// and sending data. Message from a `WebSocketStream<S>` are accessible
/// through the respective `Stream` and `Sink`. Check more information about
/// them in `futures-rs` crate documentation or have a look on the examples
/// and unit tests for this crate.
#[derive(Debug)]
pub struct WebSocketStream<S> {
    inner: WebSocket<AllowStd<S>>,
    closing: bool,
    ended: bool,
}

impl<S> WebSocketStream<S> {
    /// Convert a raw socket into a WebSocketStream without performing a
    /// handshake.
    pub async fn from_raw_socket(stream: S, role: Role, config: Option<WebSocketConfig>) -> Self
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        without_handshake(stream, move |allow_std| {
            WebSocket::from_raw_socket(allow_std, role, config)
        })
        .await
    }

    /// Convert a raw socket into a WebSocketStream without performing a
    /// handshake.
    pub async fn from_partially_read(
        stream: S,
        part: Vec<u8>,
        role: Role,
        config: Option<WebSocketConfig>,
    ) -> Self
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        without_handshake(stream, move |allow_std| {
            WebSocket::from_partially_read(allow_std, part, role, config)
        })
        .await
    }

    pub(crate) fn new(ws: WebSocket<AllowStd<S>>) -> Self {
        WebSocketStream { inner: ws, closing: false, ended: false }
    }

    fn with_context<F, R>(&mut self, ctx: Option<(ContextWaker, &mut Context<'_>)>, f: F) -> R
    where
        S: Unpin,
        F: FnOnce(&mut WebSocket<AllowStd<S>>) -> R,
        AllowStd<S>: Read + Write,
    {
        trace!("{}:{} WebSocketStream.with_context", file!(), line!());
        if let Some((kind, ctx)) = ctx {
            self.inner.get_mut().set_waker(kind, ctx.waker());
        }
        f(&mut self.inner)
    }

    /// Returns a shared reference to the inner stream.
    pub fn get_ref(&self) -> &S
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        self.inner.get_ref().get_ref()
    }

    /// Returns a mutable reference to the inner stream.
    pub fn get_mut(&mut self) -> &mut S
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        self.inner.get_mut().get_mut()
    }

    /// Returns a reference to the configuration of the tungstenite stream.
    pub fn get_config(&self) -> &WebSocketConfig {
        self.inner.get_config()
    }

    /// Close the underlying web socket
    pub async fn close(&mut self, msg: Option<CloseFrame<'_>>) -> Result<(), WsError>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let msg = msg.map(|msg| msg.into_owned());
        self.send(Message::Close(msg)).await
    }
}

impl<T> Stream for WebSocketStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    type Item = Result<Message, WsError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        trace!("{}:{} Stream.poll_next", file!(), line!());

        // The connection has been closed or a critical error has occurred.
        // We have already returned the error to the user, the `Stream` is unusable,
        // so we assume that the stream has been "fused".
        if self.ended {
            return Poll::Ready(None);
        }

        match futures_util::ready!(self.with_context(Some((ContextWaker::Read, cx)), |s| {
            trace!("{}:{} Stream.with_context poll_next -> read_message()", file!(), line!());
            cvt(s.read_message())
        })) {
            Ok(v) => Poll::Ready(Some(Ok(v))),
            Err(e) => {
                self.ended = true;
                if matches!(e, WsError::AlreadyClosed | WsError::ConnectionClosed) {
                    Poll::Ready(None)
                } else {
                    Poll::Ready(Some(Err(e)))
                }
            }
        }
    }
}

impl<T> FusedStream for WebSocketStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    fn is_terminated(&self) -> bool {
        self.ended
    }
}

impl<T> Sink<Message> for WebSocketStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    type Error = WsError;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        (*self).with_context(Some((ContextWaker::Write, cx)), |s| cvt(s.write_pending()))
    }

    fn start_send(mut self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        match (*self).with_context(None, |s| s.write_message(item)) {
            Ok(()) => Ok(()),
            Err(tungstenite::Error::Io(err)) if err.kind() == std::io::ErrorKind::WouldBlock => {
                // the message was accepted and queued
                // isn't an error.
                Ok(())
            }
            Err(e) => {
                debug!("websocket start_send error: {}", e);
                Err(e)
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        (*self).with_context(Some((ContextWaker::Write, cx)), |s| cvt(s.write_pending()))
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let res = if self.closing {
            // After queueing it, we call `write_pending` to drive the close handshake to completion.
            (*self).with_context(Some((ContextWaker::Write, cx)), |s| s.write_pending())
        } else {
            (*self).with_context(Some((ContextWaker::Write, cx)), |s| s.close(None))
        };

        match res {
            Ok(()) => Poll::Ready(Ok(())),
            Err(tungstenite::Error::ConnectionClosed) => Poll::Ready(Ok(())),
            Err(tungstenite::Error::Io(err)) if err.kind() == std::io::ErrorKind::WouldBlock => {
                trace!("WouldBlock");
                self.closing = true;
                Poll::Pending
            }
            Err(err) => {
                debug!("websocket close error: {}", err);
                Poll::Ready(Err(err))
            }
        }
    }
}

impl<T, B> Sink<B> for WebSocketStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
    B: AsRef<[u8]>
{
    type Error = WsError;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        (*self).with_context(Some((ContextWaker::Write, cx)), |s| cvt(s.write_pending()))
    }

    fn start_send(mut self: Pin<&mut Self>, item: B) -> Result<(), Self::Error> {
        match (*self).with_context(None, |s| s.write_raw(item)) {
            Ok(()) => Ok(()),
            Err(tungstenite::Error::Io(err)) if err.kind() == std::io::ErrorKind::WouldBlock => {
                // the message was accepted and queued
                // isn't an error.
                Ok(())
            }
            Err(e) => {
                debug!("websocket start_send error: {}", e);
                Err(e)
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        (*self).with_context(Some((ContextWaker::Write, cx)), |s| cvt(s.write_pending()))
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let res = if self.closing {
            // After queueing it, we call `write_pending` to drive the close handshake to completion.
            (*self).with_context(Some((ContextWaker::Write, cx)), |s| s.write_pending())
        } else {
            (*self).with_context(Some((ContextWaker::Write, cx)), |s| s.close(None))
        };

        match res {
            Ok(()) => Poll::Ready(Ok(())),
            Err(tungstenite::Error::ConnectionClosed) => Poll::Ready(Ok(())),
            Err(tungstenite::Error::Io(err)) if err.kind() == std::io::ErrorKind::WouldBlock => {
                trace!("WouldBlock");
                self.closing = true;
                Poll::Pending
            }
            Err(err) => {
                debug!("websocket close error: {}", err);
                Poll::Ready(Err(err))
            }
        }
    }
}

pub(crate) async fn without_handshake<F, S>(stream: S, f: F) -> WebSocketStream<S>
where
    F: FnOnce(AllowStd<S>) -> WebSocket<AllowStd<S>> + Unpin,
    S: AsyncRead + AsyncWrite + Unpin,
{
    let start = SkippedHandshakeFuture(Some(SkippedHandshakeFutureInner { f, stream }));

    let ws = start.await;

    WebSocketStream::new(ws)
}

struct SkippedHandshakeFuture<F, S>(Option<SkippedHandshakeFutureInner<F, S>>);
struct SkippedHandshakeFutureInner<F, S> {
    f: F,
    stream: S,
}

impl<F, S> Future for SkippedHandshakeFuture<F, S>
where
    F: FnOnce(AllowStd<S>) -> WebSocket<AllowStd<S>> + Unpin,
    S: Unpin,
    AllowStd<S>: Read + Write,
{
    type Output = WebSocket<AllowStd<S>>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let inner = self.get_mut().0.take().expect("future polled after completion");
        trace!("Setting context when skipping handshake");
        let stream = AllowStd::new(inner.stream, ctx.waker());

        Poll::Ready((inner.f)(stream))
    }
}