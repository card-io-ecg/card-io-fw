use embedded_io::blocking::ReadExactError;
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
    pub fn from_header(header: Header) -> Result<Self, BodyTypeError> {
        if header.name.eq_ignore_ascii_case("transfer-encoding") {
            let Ok(value) = core::str::from_utf8(header.value) else {
                return Err(BodyTypeError::IncorrectEncoding);
            };

            match value
                .rsplit(',')
                .map(|encoding| encoding.trim())
                .position(|enc| enc.eq_ignore_ascii_case("chunked"))
            {
                Some(0) => return Ok(Self::Chunked),
                Some(_) => {
                    // If a Transfer-Encoding header field is present in a request and the chunked
                    // transfer coding is not the final encoding, the message body length cannot be
                    // determined reliably; the server MUST respond with the 400 (Bad Request)
                    // status code and then close the connection.
                    return Err(BodyTypeError::IncorrectTransferEncoding);
                }
                None => {}
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

// A buffer around the socket that first returns pre-loaded data.
struct Buffer<'buf> {
    buffer: &'buf [u8],
}

impl Buffer<'_> {
    fn flush_loaded<'r>(&mut self, dst: &'r mut [u8]) -> &'r mut [u8] {
        let bytes = self.buffer.len().min(dst.len());

        let (buffer, remaining) = self.buffer.split_at(bytes);
        dst[..bytes].copy_from_slice(buffer);
        self.buffer = remaining;

        &mut dst[bytes..]
    }

    pub async fn read<C: Connection>(
        &mut self,
        buf: &mut [u8],
        socket: &mut C,
    ) -> Result<usize, C::Error> {
        let buffer_to_fill = self.flush_loaded(buf);
        socket.read(buffer_to_fill).await
    }

    pub async fn read_exact<C: Connection>(
        &mut self,
        buf: &mut [u8],
        socket: &mut C,
    ) -> Result<(), ReadExactError<C::Error>> {
        let buffer_to_fill = self.flush_loaded(buf);
        socket.read_exact(buffer_to_fill).await
    }

    pub async fn read_one<C: Connection>(
        &mut self,
        socket: &mut C,
    ) -> Result<Option<u8>, C::Error> {
        let mut buffer = [0];
        match self.read_exact(&mut buffer, socket).await {
            Ok(()) => Ok(Some(buffer[0])),
            Err(ReadExactError::UnexpectedEof) => Ok(None),
            Err(ReadExactError::Other(e)) => Err(e),
        }
    }
}

struct ContentLengthReader<'buf> {
    buffer: Buffer<'buf>,
    length: u32,
}

impl<'buf> ContentLengthReader<'buf> {
    fn new(buffer: Buffer<'buf>, length: u32) -> Self {
        Self { buffer, length }
    }

    pub fn is_complete(&self) -> bool {
        self.length == 0
    }

    pub async fn read<C: Connection>(
        &mut self,
        buf: &mut [u8],
        socket: &mut C,
    ) -> ReadResult<usize, C> {
        let len = self.length.min(buf.len() as u32) as usize;

        let read = self
            .buffer
            .read(&mut buf[0..len], socket)
            .await
            .map_err(ReadError::Io)?;
        self.length -= read as u32;

        Ok(read)
    }
}

pub enum ReadError<C: Connection> {
    Io(C::Error),
    Encoding,
    UnexpectedEof,
}

impl<C> core::fmt::Debug for ReadError<C>
where
    C: Connection,
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

struct ChunkedReader<'buf> {
    buffer: Buffer<'buf>,
    state: ChunkedReaderState,
}

impl<'buf> ChunkedReader<'buf> {
    fn new(buffer: Buffer<'buf>) -> Self {
        Self {
            buffer,
            state: ChunkedReaderState::ReadChunkSize,
        }
    }

    pub fn is_complete(&self) -> bool {
        self.state == ChunkedReaderState::Finished
    }

    pub async fn read_chunk_size<C: Connection>(&mut self, socket: &mut C) -> ReadResult<usize, C> {
        let mut number;

        if let Some(digit) = self.buffer.read_one(socket).await.map_err(ReadError::Io)? {
            number = digit as usize;
        } else {
            // EOF at the beginning of a chunk is allowed and indicates the end of the body
            // Note: this is spelled out in the RFC for responses, but not for requests.
            return Ok(0);
        }

        while let Some(byte) = self.buffer.read_one(socket).await.map_err(ReadError::Io)? {
            let digit_value = match byte {
                byte @ b'0'..=b'9' => (byte - b'0') as usize,
                byte @ b'a'..=b'f' => (byte - b'a' + 10) as usize,
                byte @ b'A'..=b'F' => (byte - b'A' + 10) as usize,
                b'\r' => return self.consume(b"\n", socket).await.map(|_| number),
                b' ' => return self.consume_until_newline(socket).await.map(|_| number),
                _ => return Err(ReadError::Encoding),
            };
            number = number * 16 + digit_value;
        }

        Err(ReadError::UnexpectedEof)
    }

    pub async fn consume_until_newline<C: Connection>(
        &mut self,
        socket: &mut C,
    ) -> ReadResult<usize, C> {
        let mut consumed = 0;
        while let Some(byte) = self.buffer.read_one(socket).await.map_err(ReadError::Io)? {
            if let b'\r' = byte {
                self.consume(b"\n", socket).await?;
                return Ok(consumed);
            } else {
                consumed += 1;
            }
        }

        Err(ReadError::UnexpectedEof)
    }

    pub async fn consume<C: Connection>(
        &mut self,
        expected: &[u8],
        socket: &mut C,
    ) -> ReadResult<(), C> {
        for expected_byte in expected {
            let byte = self.buffer.read_one(socket).await.map_err(ReadError::Io)?;

            if byte != Some(*expected_byte) {
                return Err(ReadError::Encoding);
            }
        }

        Ok(())
    }

    pub async fn consume_trailers<C: Connection>(&mut self, socket: &mut C) -> ReadResult<(), C> {
        loop {
            let consumed = self.consume_until_newline(socket).await?;
            if consumed == 0 {
                break;
            }
        }

        Ok(())
    }

    pub async fn read_one<C: Connection>(&mut self, socket: &mut C) -> ReadResult<Option<u8>, C> {
        loop {
            match self.state {
                ChunkedReaderState::ReadChunkSize => {
                    let chunk_size = self.read_chunk_size(socket).await?;

                    self.state = if chunk_size == 0 {
                        // A recipient that removes the chunked coding from a message MAY
                        // selectively retain or discard the received trailer fields.
                        self.consume_trailers(socket).await?;
                        ChunkedReaderState::Finished
                    } else {
                        ChunkedReaderState::Chunk(chunk_size)
                    };
                }
                ChunkedReaderState::Chunk(ref mut remaining) => {
                    let read_result = self.buffer.read_one(socket).await.map_err(ReadError::Io)?;
                    let Some(byte) = read_result else {
                        // unexpected eof
                        self.state = ChunkedReaderState::Finished;
                        return Err(ReadError::UnexpectedEof);
                    };

                    *remaining -= 1;
                    if *remaining == 0 {
                        self.consume(b"\r\n", socket).await?;
                        self.state = ChunkedReaderState::ReadChunkSize;
                    }

                    return Ok(Some(byte));
                }
                ChunkedReaderState::Finished => return Ok(None),
            }
        }
    }

    pub async fn read<C: Connection>(
        &mut self,
        buf: &mut [u8],
        socket: &mut C,
    ) -> ReadResult<usize, C> {
        for (index, byte) in buf.iter_mut().enumerate() {
            *byte = match self.read_one(socket).await? {
                Some(byte) => byte,
                None => return Ok(index),
            };
        }

        Ok(buf.len())
    }
}

// Reader state specific to each body type
enum BodyReader<'buf> {
    Chunked(ChunkedReader<'buf>),
    ContentLength(ContentLengthReader<'buf>),
    Unknown(Buffer<'buf>),
}

impl<'buf> BodyReader<'buf> {
    pub fn is_complete(&self) -> bool {
        match self {
            Self::Chunked(reader) => reader.is_complete(),
            Self::ContentLength(reader) => reader.is_complete(),
            Self::Unknown(_) => false,
        }
    }

    pub async fn read<C: Connection>(
        &mut self,
        buf: &mut [u8],
        socket: &mut C,
    ) -> ReadResult<usize, C> {
        match self {
            Self::Chunked(reader) => reader.read(buf, socket).await,
            Self::ContentLength(reader) => reader.read(buf, socket).await,
            Self::Unknown(reader) => reader.read(buf, socket).await.map_err(ReadError::Io),
        }
    }
}

pub struct RequestBody<'buf> {
    reader: BodyReader<'buf>,
}

impl<'buf> RequestBody<'buf> {
    pub(crate) fn new(
        headers: &[Header],
        pre_loaded: &'buf [u8],
    ) -> Result<Self, RequestBodyError> {
        let request_type =
            RequestBodyType::from_headers(headers).map_err(RequestBodyError::BodyType)?;

        let buffer = Buffer { buffer: pre_loaded };

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

    pub async fn read<C>(&mut self, buf: &mut [u8], socket: &mut C) -> ReadResult<usize, C>
    where
        C: Connection,
    {
        self.reader.read(buf, socket).await
    }
}
