use embedded_io::{blocking::ReadExactError, Io};
use httparse::Header;

use crate::{connector::Connection, response::ResponseStatus};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BodyTypeError {
    IncorrectEncoding,
    ConflictingHeaders,
    IncorrectTransferEncoding,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RequestBodyType {
    Chunked,
    ContentLength(u32),
    Unknown,
}

impl RequestBodyType {
    fn from_header(header: Header) -> Result<Self, BodyTypeError> {
        if header.name.eq_ignore_ascii_case("transfer-encoding") {
            let Ok(value) = core::str::from_utf8(header.value) else {
                return Err(BodyTypeError::IncorrectEncoding);
            };

            match value
                .rsplit(',')
                .map(|encoding| encoding.trim())
                .position(|enc| enc.eq_ignore_ascii_case("chunked"))
            {
                Some(0) => Ok(Self::Chunked),
                None => Ok(Self::Unknown),
                Some(_) => {
                    // If a Transfer-Encoding header field is present in a request and the chunked
                    // transfer coding is not the final encoding, the message body length cannot be
                    // determined reliably; the server MUST respond with the 400 (Bad Request)
                    // status code and then close the connection.
                    Err(BodyTypeError::IncorrectTransferEncoding)
                }
            }
        } else if header.name.eq_ignore_ascii_case("content-length") {
            // When a message does not have a Transfer-Encoding header field, a
            // Content-Length header field (Section 8.6 of [HTTP]) can provide the anticipated size
            let Ok(value) = core::str::from_utf8(header.value) else {
                return Err(BodyTypeError::IncorrectEncoding);
            };

            value
                .parse::<u32>()
                .map_err(|_| BodyTypeError::IncorrectEncoding)
                .map(Self::ContentLength)
        } else {
            Ok(Self::Unknown)
        }
    }

    fn from_headers(headers: &[Header]) -> Result<Self, BodyTypeError> {
        let mut result = Self::Unknown;

        // Transfer-Encoding is defined as overriding Content-Length
        // A server MAY reject a request that contains both Content-Length and Transfer-Encoding
        // or process such a request in accordance with the Transfer-Encoding alone.

        for header in headers {
            let header_type = Self::from_header(*header)?;

            if header_type != Self::Unknown {
                if result != Self::Unknown {
                    return Err(BodyTypeError::ConflictingHeaders);
                }
                result = header_type;
            }
        }

        Ok(result)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RequestBodyError {
    BodyType(BodyTypeError),
}

impl From<RequestBodyError> for ResponseStatus {
    fn from(value: RequestBodyError) -> Self {
        match value {
            RequestBodyError::BodyType(BodyTypeError::IncorrectEncoding) => {
                // A server that receives a request message with a transfer coding it does
                // not understand SHOULD respond with 501 (Not Implemented).

                // Note: this is a bit of a stretch, because this error is for incorrectly
                // encoded strings, but I think technically we are correct.
                ResponseStatus::NotImplemented
            }
            RequestBodyError::BodyType(BodyTypeError::ConflictingHeaders) => {
                ResponseStatus::BadRequest
            }
            RequestBodyError::BodyType(BodyTypeError::IncorrectTransferEncoding) => {
                // must return 400
                ResponseStatus::BadRequest
            }
        }
    }
}

/// A buffer around the socket that first returns pre-loaded data.
pub struct Buffer<'buf, 's, C: Connection> {
    buffer: &'buf [u8],
    socket: &'s mut C,
}

impl<'s, C: Connection> Buffer<'_, 's, C> {
    fn flush_loaded<'r>(&mut self, dst: &'r mut [u8]) -> &'r mut [u8] {
        let bytes = self.buffer.len().min(dst.len());

        let (buffer, remaining) = self.buffer.split_at(bytes);
        dst[..bytes].copy_from_slice(buffer);
        self.buffer = remaining;

        &mut dst[bytes..]
    }

    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, C::Error> {
        let buffer_to_fill = self.flush_loaded(buf);
        self.socket.read(buffer_to_fill).await
    }

    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), ReadExactError<C::Error>> {
        let buffer_to_fill = self.flush_loaded(buf);
        self.socket.read_exact(buffer_to_fill).await
    }

    async fn read_one(&mut self) -> Result<Option<u8>, C::Error> {
        let mut buffer = [0];
        match self.read_exact(&mut buffer).await {
            Ok(()) => Ok(Some(buffer[0])),
            Err(ReadExactError::UnexpectedEof) => Ok(None),
            Err(ReadExactError::Other(e)) => Err(e),
        }
    }

    fn take_socket(self) -> &'s mut C {
        self.socket
    }
}

pub struct ContentLengthReader<'buf, 's, C: Connection> {
    buffer: Buffer<'buf, 's, C>,
    length: u32,
}

impl<'buf, 's, C: Connection> ContentLengthReader<'buf, 's, C> {
    fn new(buffer: Buffer<'buf, 's, C>, length: u32) -> Self {
        Self { buffer, length }
    }

    fn is_complete(&self) -> bool {
        self.length == 0
    }

    async fn read(&mut self, buf: &mut [u8]) -> ReadResult<usize, C> {
        let len = self.length.min(buf.len() as u32) as usize;

        let read = self
            .buffer
            .read(&mut buf[0..len])
            .await
            .map_err(ReadError::Io)?;
        self.length -= read as u32;

        Ok(read)
    }

    fn take_socket(self) -> &'s mut C {
        self.buffer.take_socket()
    }
}

pub enum ReadError<C: Io> {
    Io(C::Error),
    Encoding,
    UnexpectedEof,
}

impl<C> core::fmt::Debug for ReadError<C>
where
    C: Io,
{
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            ReadError::Io(f0) => f.debug_tuple("Io").field(&f0).finish(),
            ReadError::Encoding => f.write_str("Encoding"),
            ReadError::UnexpectedEof => f.write_str("UnexpectedEof"),
        }
    }
}

pub type ReadResult<T, C> = Result<T, ReadError<C>>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChunkedReaderState {
    ReadChunkSize,
    Chunk(usize),
    Finished,
}

pub struct ChunkedReader<'buf, 's, C: Connection> {
    buffer: Buffer<'buf, 's, C>,
    state: ChunkedReaderState,
}

impl<'buf, 's, C: Connection> ChunkedReader<'buf, 's, C> {
    fn new(buffer: Buffer<'buf, 's, C>) -> Self {
        Self {
            buffer,
            state: ChunkedReaderState::ReadChunkSize,
        }
    }

    fn is_complete(&self) -> bool {
        self.state == ChunkedReaderState::Finished
    }

    async fn read_chunk_size(&mut self) -> ReadResult<usize, C> {
        let mut read = false;
        let mut number = 0;
        while let Some(byte) = self.buffer.read_one().await.map_err(ReadError::Io)? {
            read = true;
            let digit_value = match byte {
                byte @ b'0'..=b'9' => (byte - b'0') as usize,
                byte @ b'a'..=b'f' => (byte - b'a' + 10) as usize,
                byte @ b'A'..=b'F' => (byte - b'A' + 10) as usize,
                b'\r' => return self.consume(b"\n").await.map(|_| number),
                b' ' => return self.consume_until_newline().await.map(|_| number),
                _ => return Err(ReadError::Encoding),
            };
            number = number * 16 + digit_value;
        }

        if read {
            Err(ReadError::UnexpectedEof)
        } else {
            // EOF at the beginning of a chunk is allowed and indicates the end of the body
            // Note: this is spelled out in the RFC for responses, but not for requests.
            Ok(0)
        }
    }

    async fn consume_until_newline(&mut self) -> ReadResult<usize, C> {
        let mut consumed = 0;
        while let Some(byte) = self.buffer.read_one().await.map_err(ReadError::Io)? {
            if let b'\r' = byte {
                self.consume(b"\n").await?;
                return Ok(consumed);
            } else {
                consumed += 1;
            }
        }

        Err(ReadError::UnexpectedEof)
    }

    async fn consume(&mut self, expected: &[u8]) -> ReadResult<(), C> {
        for expected_byte in expected {
            let byte = self.buffer.read_one().await.map_err(ReadError::Io)?;

            if byte != Some(*expected_byte) {
                return Err(ReadError::Encoding);
            }
        }

        Ok(())
    }

    async fn consume_trailers(&mut self) -> ReadResult<(), C> {
        loop {
            let consumed = self.consume_until_newline().await?;
            if consumed == 0 {
                break;
            }
        }

        Ok(())
    }

    async fn read_one(&mut self) -> ReadResult<Option<u8>, C> {
        loop {
            match self.state {
                ChunkedReaderState::ReadChunkSize => {
                    let chunk_size = self.read_chunk_size().await?;

                    self.state = if chunk_size == 0 {
                        // A recipient that removes the chunked coding from a message MAY
                        // selectively retain or discard the received trailer fields.
                        self.consume_trailers().await?;
                        ChunkedReaderState::Finished
                    } else {
                        ChunkedReaderState::Chunk(chunk_size)
                    };
                }
                ChunkedReaderState::Chunk(ref mut remaining) => {
                    let read_result = self.buffer.read_one().await.map_err(ReadError::Io)?;
                    let Some(byte) = read_result else {
                        // unexpected eof
                        self.state = ChunkedReaderState::Finished;
                        return Err(ReadError::UnexpectedEof);
                    };

                    *remaining -= 1;
                    if *remaining == 0 {
                        self.consume(b"\r\n").await?;
                        self.state = ChunkedReaderState::ReadChunkSize;
                    }

                    return Ok(Some(byte));
                }
                ChunkedReaderState::Finished => return Ok(None),
            }
        }
    }

    async fn read(&mut self, buf: &mut [u8]) -> ReadResult<usize, C> {
        for (index, byte) in buf.iter_mut().enumerate() {
            *byte = match self.read_one().await? {
                Some(byte) => byte,
                None => return Ok(index),
            };
        }

        Ok(buf.len())
    }

    fn take_socket(self) -> &'s mut C {
        self.buffer.take_socket()
    }
}

pub enum RequestBody<'buf, 's, C: Connection> {
    Chunked(ChunkedReader<'buf, 's, C>),
    ContentLength(ContentLengthReader<'buf, 's, C>),
    Unknown(Buffer<'buf, 's, C>),
}

impl<'buf, 's, C> RequestBody<'buf, 's, C>
where
    C: Connection,
{
    pub(crate) fn new(
        headers: &[Header],
        pre_loaded: &'buf [u8],
        socket: &'s mut C,
    ) -> Result<Self, RequestBodyError> {
        let request_type =
            RequestBodyType::from_headers(headers).map_err(RequestBodyError::BodyType)?;

        let buffer = Buffer {
            buffer: pre_loaded,
            socket,
        };

        Ok(match request_type {
            RequestBodyType::Chunked => RequestBody::Chunked(ChunkedReader::new(buffer)),
            RequestBodyType::ContentLength(length) => {
                RequestBody::ContentLength(ContentLengthReader::new(buffer, length))
            }
            RequestBodyType::Unknown => RequestBody::Unknown(buffer),
        })
    }

    pub fn is_complete(&self) -> bool {
        match self {
            Self::Chunked(reader) => reader.is_complete(),
            Self::ContentLength(reader) => reader.is_complete(),
            Self::Unknown(_) => false,
        }
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> ReadResult<usize, C> {
        match self {
            Self::Chunked(reader) => reader.read(buf).await,
            Self::ContentLength(reader) => reader.read(buf).await,
            Self::Unknown(reader) => reader.read(buf).await.map_err(ReadError::Io),
        }
    }

    pub(crate) fn take_socket(self) -> &'s mut C {
        match self {
            RequestBody::Chunked(reader) => reader.take_socket(),
            RequestBody::ContentLength(reader) => reader.take_socket(),
            RequestBody::Unknown(reader) => reader.take_socket(),
        }
    }
}
