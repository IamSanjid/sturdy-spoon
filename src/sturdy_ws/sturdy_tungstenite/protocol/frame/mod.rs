//! Utilities to work with raw WebSocket frames.

pub mod coding;

#[allow(clippy::module_inception)]
mod frame;
mod mask;

use std::io::{Error as IoError, ErrorKind as IoErrorKind, Read, Write};

use log::*;

pub use self::frame::{CloseFrame, Frame, FrameHeader};
use super::error::{self, CapacityError, Error, Result};

const READ_BUFFER_CHUNK_SIZE: usize = 4096;
type ReadBuffer = tokio_tungstenite::tungstenite::buffer::ReadBuffer<READ_BUFFER_CHUNK_SIZE>;

/// A reader and writer for WebSocket frames.
#[derive(Debug)]
pub struct FrameSocket<Stream> {
    /// The underlying network stream.
    stream: Stream,
    /// Codec for reading/writing frames.
    codec: FrameCodec,
}

impl<Stream> FrameSocket<Stream> {
    /// Create a new frame socket.
    pub fn new(stream: Stream) -> Self {
        FrameSocket {
            stream,
            codec: FrameCodec::new(),
        }
    }

    /// Create a new frame socket from partially read data.
    pub fn from_partially_read(stream: Stream, part: Vec<u8>) -> Self {
        FrameSocket {
            stream,
            codec: FrameCodec::from_partially_read(part),
        }
    }

    /// Extract a stream from the socket.
    pub fn into_inner(self) -> (Stream, Vec<u8>) {
        (self.stream, self.codec.in_buffer.into_vec())
    }

    /// Returns a shared reference to the inner stream.
    pub fn get_ref(&self) -> &Stream {
        &self.stream
    }

    /// Returns a mutable reference to the inner stream.
    pub fn get_mut(&mut self) -> &mut Stream {
        &mut self.stream
    }
}

impl<Stream> FrameSocket<Stream>
where
    Stream: Read,
{
    /// Read a frame from stream.
    pub fn read_frame(&mut self, max_size: Option<usize>) -> Result<Option<Frame>> {
        self.codec.read_frame(&mut self.stream, max_size)
    }
}

impl<Stream> FrameSocket<Stream>
where
    Stream: Write,
{
    /// Write a frame to stream.
    ///
    /// This function guarantees that the frame is queued regardless of any errors.
    /// There is no need to resend the frame. In order to handle WouldBlock or Incomplete,
    /// call write_pending() afterwards.
    pub fn write_frame(&mut self, frame: Frame) -> Result<()> {
        self.codec.write_frame(&mut self.stream, frame)
    }

    /// Complete pending write, if any.
    pub fn write_pending(&mut self) -> Result<()> {
        self.codec.write_pending(&mut self.stream)
    }
}

/// A codec for WebSocket frames.
#[derive(Debug)]
pub(super) struct FrameCodec {
    /// Buffer to read data from the stream.
    in_buffer: ReadBuffer,
    /// Buffer to send packets to the network.
    out_buffer: Vec<u8>,
    /// Header and remaining size of the incoming packet being processed.
    header: Option<(FrameHeader, u64)>,
}

impl FrameCodec {
    /// Create a new frame codec.
    pub(super) fn new() -> Self {
        Self {
            in_buffer: ReadBuffer::new(),
            out_buffer: Vec::new(),
            header: None,
        }
    }

    /// Create a new frame codec from partially read data.
    pub(super) fn from_partially_read(part: Vec<u8>) -> Self {
        Self {
            in_buffer: ReadBuffer::from_partially_read(part),
            out_buffer: Vec::new(),
            header: None,
        }
    }

    /// Read a frame from the provided stream.
    pub(super) fn read_frame<Stream>(
        &mut self,
        stream: &mut Stream,
        max_size: Option<usize>,
    ) -> Result<Option<Frame>>
    where
        Stream: Read,
    {
        let max_size = max_size.unwrap_or_else(usize::max_value);

        let payload = loop {
            {
                let cursor = self.in_buffer.as_cursor_mut();

                if self.header.is_none() {
                    self.header = FrameHeader::parse(cursor)?;
                }

                if let Some((_, ref length)) = self.header {
                    let length = *length;

                    // Enforce frame size limit early and make sure `length`
                    // is not too big (fits into `usize`).
                    if length > max_size as u64 {
                        return Err(Error::Capacity(CapacityError::MessageTooLong {
                            size: length as usize,
                            max_size,
                        }));
                    }

                    let input_size = cursor.get_ref().len() as u64 - cursor.position();
                    if length <= input_size {
                        // No truncation here since `length` is checked above
                        let mut payload = Vec::with_capacity(length as usize);
                        if length > 0 {
                            cursor.take(length).read_to_end(&mut payload)?;
                        }
                        break payload;
                    }
                }
            }

            // Not enough data in buffer.
            let size = self.in_buffer.read_from(stream)?;
            if size == 0 {
                trace!("no frame received");
                return Ok(None);
            }
        };

        let (header, length) = self.header.take().expect("Bug: no frame header");
        debug_assert_eq!(payload.len() as u64, length);
        let frame = Frame::from_payload(header, payload);
        trace!("received frame {}", frame);
        Ok(Some(frame))
    }

    /// Write a frame to the provided stream.
    pub(super) fn write_frame<Stream>(&mut self, stream: &mut Stream, frame: Frame) -> Result<()>
    where
        Stream: Write,
    {
        trace!("writing frame {}", frame);
        self.out_buffer.reserve(frame.len());
        frame
            .format(&mut self.out_buffer)
            .expect("Bug: can't write to vector");
        self.write_pending(stream)
    }

    pub(super) fn write_raw<Stream>(&mut self, stream: &mut Stream, bytes: &[u8]) -> Result<()>
    where
        Stream: Write,
    {
        trace!("writing bytes {}", bytes.len());
        self.out_buffer.reserve(bytes.len());
        self.out_buffer
            .write_all(bytes)
            .expect("Bug: can't write to vector");
        self.write_pending(stream)
    }

    /// Complete pending write, if any.
    pub(super) fn write_pending<Stream>(&mut self, stream: &mut Stream) -> Result<()>
    where
        Stream: Write,
    {
        while !self.out_buffer.is_empty() {
            let len = stream.write(&self.out_buffer)?;
            if len == 0 {
                // This is the same as "Connection reset by peer"
                return Err(IoError::new(
                    IoErrorKind::ConnectionReset,
                    "Connection reset while sending",
                )
                .into());
            }
            self.out_buffer.drain(0..len);
        }
        stream.flush()?;
        Ok(())
    }
}
