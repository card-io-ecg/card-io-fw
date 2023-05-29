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

struct Buffer<'buf> {
    buffer: &'buf mut [u8],
    bytes: usize,
}

impl Buffer<'_> {
    fn flush_loaded<'r>(&mut self, dst: &'r mut [u8]) -> &'r mut [u8] {
        let loaded = &self.buffer[0..self.bytes];

        let bytes = loaded.len().min(dst.len());
        dst[..bytes].copy_from_slice(&loaded[..bytes]);
        self.bytes -= bytes;

        &mut dst[bytes..]
    }

    async fn load(&mut self, count: usize, connection: &mut impl Connection) -> Result<usize, ()> {
        if count <= self.bytes {
            return Ok(self.bytes);
        }

        let end = self.buffer.len().min(count);
        let buffer_to_fill = &mut self.buffer[self.bytes..end];

        let read = connection.read(buffer_to_fill).await.map_err(|_| ())?;
        self.bytes += read;

        Ok(read)
    }

    async fn load_exact(
        &mut self,
        count: usize,
        connection: &mut impl Connection,
    ) -> Result<(), ()> {
        while self.bytes < count {
            let read = self.bytes;
            let new_read = self.load(count, connection).await?;
            if new_read == read {
                return Err(());
            }
        }

        Ok(())
    }
}

// Reader state specific to each body type
enum BodyReader {
    Chunked,
    ContentLength(u32),
    Unknown,
}

pub struct RequestBody<'buf> {
    buffer: Buffer<'buf>,
    reader: BodyReader,
}

impl<'buf> RequestBody<'buf> {
    pub(crate) fn from_preloaded(
        headers: &[Header],
        buffer: &'buf mut [u8],
        bytes: usize,
    ) -> Result<Self, RequestBodyError> {
        let request_type =
            RequestBodyType::from_headers(headers).map_err(RequestBodyError::BodyType)?;

        Ok(Self {
            buffer: Buffer { buffer, bytes },
            reader: match request_type {
                RequestBodyType::Chunked => BodyReader::Chunked,
                RequestBodyType::ContentLength(length) => BodyReader::ContentLength(length),
                RequestBodyType::Unknown => BodyReader::Unknown,
            },
        })
    }

    pub async fn read(
        &mut self,
        buf: &mut [u8],
        connection: &mut impl Connection,
    ) -> Result<usize, ()> {
        self.buffer.load_exact(buf.len(), connection).await?;

        let buf_len = buf.len();
        let buffer_to_fill = self.buffer.flush_loaded(buf);

        Ok(buf_len - buffer_to_fill.len())
    }
}
