use std::sync::Arc;

use super::sturdy_tungstenite::{
    protocol::frame::{
        coding::{Data, OpCode},
        CloseFrame, Frame,
    },
    Message,
};

pub enum WebSocketMessage {
    Text(String),
    Binary(Vec<u8>),
    Close(Option<CloseFrame<'static>>),
}

impl WebSocketMessage {
    pub fn into_server_shared_bytes(self) -> Arc<[u8]> {
        let frame: Frame = self.into();
        let bytes: Vec<u8> = frame.into();

        Arc::from(bytes.into_boxed_slice())
    }

    pub fn into_client_shared_bytes(self) -> Arc<[u8]> {
        let mut frame: Frame = self.into();
        frame.set_random_mask();
        let bytes: Vec<u8> = frame.into();

        Arc::from(bytes.into_boxed_slice())
    }
}

impl From<WebSocketMessage> for Frame {
    fn from(value: WebSocketMessage) -> Self {
        match value {
            WebSocketMessage::Close(close_frame) => Frame::close(close_frame),
            WebSocketMessage::Text(data) => {
                Frame::message(data.into(), OpCode::Data(Data::Text), true)
            }
            WebSocketMessage::Binary(data) => {
                Frame::message(data, OpCode::Data(Data::Binary), true)
            }
        }
    }
}

impl From<WebSocketMessage> for Message {
    fn from(value: WebSocketMessage) -> Self {
        match value {
            WebSocketMessage::Close(close_frame) => Message::Close(close_frame),
            WebSocketMessage::Text(data) => Message::Text(data),
            WebSocketMessage::Binary(data) => Message::Binary(data),
        }
    }
}

impl From<Message> for Frame {
    fn from(value: Message) -> Self {
        match value {
            Message::Text(data) => Frame::message(data.into(), OpCode::Data(Data::Text), true),
            Message::Binary(data) => Frame::message(data, OpCode::Data(Data::Binary), true),
            Message::Ping(data) => Frame::ping(data),
            Message::Pong(data) => Frame::pong(data),
            Message::Close(code) => Frame::close(code),
            Message::Frame(f) => f,
        }
    }
}

impl From<Frame> for Vec<u8> {
    fn from(value: Frame) -> Self {
        let mut output = Vec::with_capacity(value.len());
        value
            .format(&mut output)
            .expect("Bug can't write to vector.");
        output
    }
}
