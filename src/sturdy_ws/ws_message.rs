use std::{
    io::{Cursor, Read},
    sync::Arc,
};

use super::sturdy_tungstenite::{
    protocol::frame::{
        coding::{Data, OpCode},
        CloseFrame, Frame, FrameHeader,
    },
    Message,
};

pub enum WebSocketMessage {
    Text(String),
    Binary(Vec<u8>),
    Close(Option<CloseFrame<'static>>),
}

pub trait ArcRawBytes {
    fn into_raw_bytes(self) -> Arc<Vec<u8>>;
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

impl From<Vec<u8>> for Frame {
    fn from(value: Vec<u8>) -> Self {
        let mut raw: Cursor<Vec<u8>> = Cursor::new(value);
        let (header, _) = FrameHeader::parse(&mut raw).unwrap().unwrap();
        let mut payload = Vec::new();
        raw.read_to_end(&mut payload).unwrap();
        Frame::from_payload(header, payload)
    }
}

impl From<WebSocketMessage> for Vec<u8> {
    fn from(value: WebSocketMessage) -> Self {
        let frame: Frame = value.into();
        frame.into()
    }
}

impl ArcRawBytes for Message {
    fn into_raw_bytes(self) -> Arc<Vec<u8>> {
        let frame: Frame = self.into();
        let bytes: Vec<u8> = frame.into();

        Arc::new(bytes)
    }
}
