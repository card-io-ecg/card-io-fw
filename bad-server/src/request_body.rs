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

    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize, C::Error> {
        let len = self.length.min(buf.len() as u32) as usize;

        let read = self.buffer.read(&mut buf[0..len]).await?;
        self.length -= read as u32;

        Ok(read)
    }
}

struct ChunkedReader<'buf, C: Connection> {
    buffer: Buffer<'buf, C>,
}

impl<'buf, C: Connection> ChunkedReader<'buf, C> {
    fn new(buffer: Buffer<'buf, C>) -> Self {
        Self { buffer }
    }

    pub fn is_complete(&self) -> bool {
        todo!()
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize, C::Error> {
        todo!()
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

    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize, C::Error> {
        match self {
            Self::Chunked(reader) => reader.read(buf).await,
            Self::ContentLength(reader) => reader.read(buf).await,
            Self::Unknown(reader) => reader.read(buf).await,
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

    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        self.reader.read(buf).await.map_err(|_| ())
    }
}
