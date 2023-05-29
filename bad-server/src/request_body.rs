use embedded_io::blocking::ReadExactError;
use httparse::Header;

use crate::connector::Connection;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BodyTypeError {
    IncorrectEncoding,
    ConflictingHeaders,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RequestBodyType {
    Chunked,
    ContentLength(u32),
    Unknown,
}

impl RequestBodyType {
    pub fn from_header(header: Header) -> Result<Self, BodyTypeError> {
        if header.name.eq_ignore_ascii_case("transfer-encoding") {
            let Ok(value) = core::str::from_utf8(header.value) else {
                return Err(BodyTypeError::IncorrectEncoding);
            };

            if value
                .split(',')
                .map(|encoding| encoding.trim())
                .any(|enc| enc.eq_ignore_ascii_case("chunked"))
            {
                return Ok(Self::Chunked);
            }
        } else if header.name.eq_ignore_ascii_case("content-length") {
            // When a message does not have a Transfer-Encoding header field,
            // a Content-Length header field (Section 8.6 of [HTTP]) can provide the anticipated size
            let Ok(value) = core::str::from_utf8(header.value) else {
                return Err(BodyTypeError::IncorrectEncoding);
            };

            let length = value
                .parse::<u32>()
                .map_err(|_| BodyTypeError::IncorrectEncoding)?;

            return Ok(Self::ContentLength(length));
        }

        Ok(Self::Unknown)
    }

    pub fn from_headers(headers: &[Header]) -> Result<Self, BodyTypeError> {
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

// A buffer around the socket that first returns pre-loaded data.
struct Buffer<'buf, C: Connection> {
    buffer: &'buf [u8],
    connection: &'buf mut C,
}

impl<C: Connection> Buffer<'_, C> {
    fn flush_loaded<'r>(&mut self, dst: &'r mut [u8]) -> &'r mut [u8] {
        let bytes = self.buffer.len().min(dst.len());

        let (buffer, remaining) = self.buffer.split_at(bytes);
        dst[..bytes].copy_from_slice(buffer);
        self.buffer = remaining;

        &mut dst[bytes..]
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize, C::Error> {
        let buffer_to_fill = self.flush_loaded(buf);
        self.connection.read(buffer_to_fill).await
    }

    pub async fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), ReadExactError<C::Error>> {
        let buffer_to_fill = self.flush_loaded(buf);
        self.connection.read_exact(buffer_to_fill).await
    }

    pub async fn read_one(&mut self) -> Result<Option<u8>, C::Error> {
        let mut buffer = [0];
        match self.read_exact(&mut buffer).await {
            Ok(()) => Ok(Some(buffer[0])),
            Err(ReadExactError::UnexpectedEof) => Ok(None),
            Err(ReadExactError::Other(e)) => Err(e),
        }
    }
}

struct ContentLengthReader<'buf, C: Connection> {
    buffer: Buffer<'buf, C>,
    length: u32,
}

impl<'buf, C: Connection> ContentLengthReader<'buf, C> {
    fn new(buffer: Buffer<'buf, C>, length: u32) -> Self {
        Self { buffer, length }
    }

    pub fn is_complete(&self) -> bool {
        self.length == 0
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> ReadResult<usize, C> {
        let len = self.length.min(buf.len() as u32) as usize;

        let read = self
            .buffer
            .read(&mut buf[0..len])
            .await
            .map_err(ReadError::Io)?;
        self.length -= read as u32;

        Ok(read)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ReadError<C: Connection> {
    Io(C::Error),
    Encoding,
    UnexpectedEof,
}

pub type ReadResult<T, C> = Result<T, ReadError<C>>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChunkedReaderState {
    ReadChunkSize,
    Chunk(usize),
    Finished,
}

struct ChunkedReader<'buf, C: Connection> {
    buffer: Buffer<'buf, C>,
    state: ChunkedReaderState,
}

impl<'buf, C: Connection> ChunkedReader<'buf, C> {
    fn new(buffer: Buffer<'buf, C>) -> Self {
        Self {
            buffer,
            state: ChunkedReaderState::ReadChunkSize,
        }
    }

    pub fn is_complete(&self) -> bool {
        self.state == ChunkedReaderState::Finished
    }

    pub async fn read_chunk_size(&mut self) -> ReadResult<usize, C> {
        let mut number = 0;

        while let Some(byte) = self.buffer.read_one().await.map_err(ReadError::Io)? {
            match byte {
                byte @ b'0'..=b'9' => number = number * 10 + byte as usize,
                b'\r' => return self.consume(b"\n").await.map(|_| number),
                b' ' => return self.consume_until_newline().await.map(|_| number),
                _ => return Err(ReadError::Encoding),
            }
        }

        Err(ReadError::UnexpectedEof)
    }

    pub async fn consume_until_newline(&mut self) -> ReadResult<(), C> {
        while let Some(byte) = self.buffer.read_one().await.map_err(ReadError::Io)? {
            if let b'\r' = byte {
                return self.consume(b"\n").await;
            }
        }

        Err(ReadError::UnexpectedEof)
    }

    pub async fn consume(&mut self, expected: &[u8]) -> ReadResult<(), C> {
        for expected_byte in expected {
            let byte = self.buffer.read_one().await.map_err(ReadError::Io)?;

            if byte != Some(*expected_byte) {
                return Err(ReadError::Encoding);
            }
        }

        Ok(())
    }

    pub async fn read_one(&mut self) -> ReadResult<Option<u8>, C> {
        loop {
            match self.state {
                ChunkedReaderState::ReadChunkSize => {
                    let chunk_size = self.read_chunk_size().await?;

                    self.state = if chunk_size == 0 {
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

    pub async fn read(&mut self, buf: &mut [u8]) -> ReadResult<usize, C> {
        for (index, byte) in buf.iter_mut().enumerate() {
            *byte = match self.read_one().await? {
                Some(byte) => byte,
                None => return Ok(index),
            };
        }

        Ok(buf.len())
    }
}

// Reader state specific to each body type
enum BodyReader<'buf, C: Connection> {
    Chunked(ChunkedReader<'buf, C>),
    ContentLength(ContentLengthReader<'buf, C>),
    Unknown(Buffer<'buf, C>),
}

impl<'buf, C: Connection> BodyReader<'buf, C> {
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
}

pub struct RequestBody<'buf, C: Connection> {
    reader: BodyReader<'buf, C>,
}

impl<'buf, C: Connection> RequestBody<'buf, C> {
    pub(crate) fn new(
        headers: &[Header],
        pre_loaded: &'buf [u8],
        connection: &'buf mut C,
    ) -> Result<Self, RequestBodyError> {
        let request_type =
            RequestBodyType::from_headers(headers).map_err(RequestBodyError::BodyType)?;

        let buffer = Buffer {
            buffer: pre_loaded,
            connection,
        };

        Ok(Self {
            reader: match request_type {
                RequestBodyType::Chunked => BodyReader::Chunked(ChunkedReader::new(buffer)),
                RequestBodyType::ContentLength(length) => {
                    BodyReader::ContentLength(ContentLengthReader::new(buffer, length))
                }
                RequestBodyType::Unknown => BodyReader::Unknown(buffer),
            },
        })
    }

    pub fn is_complete(&self) -> bool {
        self.reader.is_complete()
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> ReadResult<usize, C> {
        self.reader.read(buf).await
    }
}
