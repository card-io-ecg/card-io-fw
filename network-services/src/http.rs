use edge_http::{
    io::{Body as ResponseBody, Error},
    BodyType, ConnectionType, Headers, Method, RequestHeaders, ResponseHeaders,
};
use embedded_io_async::{Read, Write};
use heapless::String;

const MAX_HEADERS: usize = 16;
// Host, Authorization, Connection, Content-Type, Content-Length.
const REQUEST_HEADERS: usize = 5;
const COUNTER_HEADER: &str = "X-Request-Counter";

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Body<'a> {
    pub content_type: &'a str,
    /// Written in order. `Content-Length` is the sum of their lengths.
    pub parts: &'a [&'a [u8]],
}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Request<'a> {
    pub method: Method,
    pub path: &'a str,
    pub authorization: Option<&'a str>,
    pub body: Option<Body<'a>>,
}

pub struct Response<R> {
    pub status: u16,
    pub counter: Option<u64>,
    pub content_len: Option<u64>,
    pub body: R,
}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum HttpError<E> {
    Io(E),
    BadResponse,
}

impl<E> From<Error<E>> for HttpError<E> {
    fn from(error: Error<E>) -> Self {
        match error {
            Error::Io(error) => HttpError::Io(error),
            _ => HttpError::BadResponse,
        }
    }
}

pub async fn send<'s, S: Read + Write>(
    stream: &'s mut S,
    host: &str,
    request: &Request<'_>,
    head: &'s mut [u8],
) -> Result<Response<impl Read + 's>, HttpError<S::Error>> {
    let mut length = String::<20>::new();
    let mut headers = RequestHeaders::<REQUEST_HEADERS>::new();
    headers.method = request.method;
    headers.path = request.path;
    headers.headers.set_host(host);
    if let Some(authorization) = request.authorization {
        headers.headers.set("Authorization", authorization);
    }
    headers.headers.set_connection_close();
    if let Some(body) = &request.body {
        let len = body.parts.iter().map(|part| part.len() as u64).sum();
        headers
            .headers
            .set_content_type(body.content_type)
            .set_content_len(len, &mut length);
    }
    headers.send(false, &mut *stream).await?;
    if let Some(body) = &request.body {
        for part in body.parts {
            stream.write_all(part).await.map_err(HttpError::Io)?;
        }
    }
    stream.flush().await.map_err(HttpError::Io)?;

    let mut response = ResponseHeaders::<MAX_HEADERS>::new();
    let (rest, read_len) = response.receive(head, &mut *stream, true).await?;
    let content_len = content_len(&response.headers)?;
    let (_, body_type) = response.resolve(ConnectionType::Close)?;
    let counter = response.headers.get(COUNTER_HEADER).and_then(counter);
    // 1xx, 204 and 304 carry no body (RFC 9110). The backend's 204 has no Content-Length, and
    // reading it until the close fails over TLS, where `embedded-tls` reports the close as an error.
    let body_type = if matches!(response.code, 100..=199 | 204 | 304) {
        BodyType::ContentLen(0)
    } else {
        body_type
    };
    Ok(Response {
        status: response.code,
        counter,
        content_len,
        body: ResponseBody::new(body_type, rest, read_len, stream),
    })
}

// `edge-http` panics on a `Content-Length` it cannot parse, so every one is checked here first.
// The last one wins, as it does when `edge-http` resolves the body.
fn content_len<E, const N: usize>(headers: &Headers<'_, N>) -> Result<Option<u64>, HttpError<E>> {
    let mut content_len = None;
    for (_, value) in headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("Content-Length"))
    {
        content_len = Some(value.parse().map_err(|_| HttpError::BadResponse)?);
    }
    Ok(content_len)
}

// The backend stores counters as `i64`.
fn counter(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value
        .parse()
        .ok()
        .filter(|&counter| counter <= i64::MAX as u64)
}

#[cfg(test)]
mod test {
    use std::{string::String, vec::Vec};

    use embassy_futures::block_on;
    use embedded_io_async::{ErrorKind, ErrorType};

    use super::*;

    /// Hands out the response a few bytes per read, like a socket.
    struct Stream {
        response: Vec<u8>,
        position: usize,
        sent: Vec<u8>,
        /// Fails a read past the end, as a TLS connection does when the server closes it.
        fails_at_end: bool,
    }

    impl Stream {
        fn new(response: &[u8]) -> Self {
            Self {
                response: response.to_vec(),
                position: 0,
                sent: Vec::new(),
                fails_at_end: false,
            }
        }
    }

    impl ErrorType for Stream {
        type Error = ErrorKind;
    }

    impl Read for Stream {
        async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ErrorKind> {
            if self.fails_at_end && self.position == self.response.len() {
                return Err(ErrorKind::ConnectionAborted);
            }
            let rest = &self.response[self.position..];
            let len = rest.len().min(buf.len()).min(7);
            buf[..len].copy_from_slice(&rest[..len]);
            self.position += len;
            Ok(len)
        }
    }

    impl Write for Stream {
        async fn write(&mut self, buf: &[u8]) -> Result<usize, ErrorKind> {
            self.sent.extend_from_slice(buf);
            Ok(buf.len())
        }

        async fn flush(&mut self) -> Result<(), ErrorKind> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct Answer {
        status: u16,
        counter: Option<u64>,
        content_len: Option<u64>,
        body: Vec<u8>,
    }

    /// Reads the body in chunks smaller than any body used here.
    fn read_answer(
        stream: &mut Stream,
        request: &Request<'_>,
    ) -> Result<Answer, HttpError<ErrorKind>> {
        let mut head = [0u8; 2048];
        block_on(async {
            let mut response = send(stream, "example.com:3001", request, &mut head).await?;
            let mut body = Vec::new();
            let mut chunk = [0u8; 3];
            loop {
                let read = response.body.read(&mut chunk).await.expect("body read");
                if read == 0 {
                    break;
                }
                body.extend_from_slice(&chunk[..read]);
            }
            Ok(Answer {
                status: response.status,
                counter: response.counter,
                content_len: response.content_len,
                body,
            })
        })
    }

    fn get() -> Request<'static> {
        Request {
            method: Method::Get,
            path: "/api/firmware",
            authorization: None,
            body: None,
        }
    }

    fn answer(response: &[u8]) -> Answer {
        let mut stream = Stream::new(response);
        read_answer(&mut stream, &get()).expect("a good response")
    }

    fn sent(request: &Request<'_>, response: &[u8]) -> String {
        let mut stream = Stream::new(response);
        read_answer(&mut stream, request).expect("a good response");
        String::from_utf8(stream.sent).expect("ASCII request")
    }

    fn is_bad(response: &[u8]) -> bool {
        let mut stream = Stream::new(response);
        matches!(
            read_answer(&mut stream, &get()),
            Err(HttpError::BadResponse)
        )
    }

    #[test]
    fn post_sends_the_parts_in_order_with_their_total_length() {
        let request = Request {
            method: Method::Post,
            path: "/api/upload_data/ABC",
            authorization: None,
            body: Some(Body {
                content_type: "application/octet-stream",
                parts: &[&[1, 0, 0, 0], b"measurement"],
            }),
        };

        let sent = sent(&request, b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");

        assert_eq!(
            sent.as_bytes(),
            b"POST /api/upload_data/ABC HTTP/1.1\r\n\
              Host: example.com:3001\r\n\
              Connection: Close\r\n\
              Content-Type: application/octet-stream\r\n\
              Content-Length: 15\r\n\
              \r\n\
              \x01\x00\x00\x00measurement"
        );
    }

    #[test]
    fn get_sends_the_authorization_before_the_connection() {
        let request = Request {
            method: Method::Get,
            path: "/api/items",
            authorization: Some("Bearer a.b.c"),
            body: None,
        };

        let sent = sent(&request, b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");

        assert!(sent.starts_with(
            "GET /api/items HTTP/1.1\r\n\
             Host: example.com:3001\r\n\
             Authorization: Bearer a.b.c\r\n\
             Connection: Close\r\n"
        ));
    }

    #[test]
    fn delete_without_a_body_sends_an_empty_content_length() {
        let request = Request {
            method: Method::Delete,
            path: "/api/devices/ABC",
            authorization: Some("Bearer a.b.c"),
            body: None,
        };

        let sent = sent(&request, b"HTTP/1.1 204 No Content\r\n\r\n");

        assert_eq!(
            sent,
            "DELETE /api/devices/ABC HTTP/1.1\r\n\
             Host: example.com:3001\r\n\
             Authorization: Bearer a.b.c\r\n\
             Connection: Close\r\n\
             Content-Length: 0\r\n\
             \r\n"
        );
    }

    #[test]
    fn reads_a_fixed_length_body_in_small_chunks() {
        let answer = answer(
            b"HTTP/1.1 200 OK\r\ncontent-length: 11\r\ncontent-type: text/plain\r\n\r\nhello world",
        );

        assert_eq!(answer.status, 200);
        assert_eq!(answer.content_len, Some(11));
        assert_eq!(answer.body, b"hello world");
    }

    #[test]
    fn reads_a_chunked_body_in_small_chunks() {
        let answer = answer(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n6\r\nhello \r\n5\r\nworld\r\n0\r\n\r\n",
        );

        assert_eq!(answer.content_len, None);
        assert_eq!(answer.body, b"hello world");
    }

    #[test]
    fn reads_a_body_until_close_in_small_chunks() {
        let answer = answer(b"HTTP/1.1 200 OK\r\n\r\nhello world");

        assert_eq!(answer.content_len, None);
        assert_eq!(answer.body, b"hello world");
    }

    #[test]
    fn keeps_the_status_of_a_not_found() {
        let answer = answer(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");

        assert_eq!(answer.status, 404);
        assert_eq!(answer.body, b"");
    }

    #[test]
    fn no_body_for_304_and_204_whatever_the_content_length() {
        for status in ["204 No Content", "304 Not Modified", "100 Continue"] {
            let response = std::format!("HTTP/1.1 {status}\r\nContent-Length: 5\r\n\r\nhello");

            let answer = answer(response.as_bytes());

            assert_eq!(answer.content_len, Some(5), "{status}");
            assert_eq!(answer.body, b"", "{status}");
        }
    }

    #[test]
    fn a_204_without_a_length_is_not_read_to_its_close() {
        let mut stream = Stream::new(b"HTTP/1.1 204 No Content\r\ndate: Sun, 04 Oct 2026\r\n\r\n");
        stream.fails_at_end = true;

        let answer = read_answer(&mut stream, &get()).expect("a 204 needs no read past its head");

        assert_eq!(answer.status, 204);
        assert_eq!(answer.body, b"");
    }

    #[test]
    fn an_unparsable_content_length_is_a_bad_response() {
        assert!(is_bad(
            b"HTTP/1.1 200 OK\r\nContent-Length: five\r\n\r\nhello"
        ));
        assert!(is_bad(
            b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\ncontent-length: -1\r\n\r\nhello"
        ));
        assert!(is_bad(
            b"HTTP/1.1 204 No Content\r\nContent-Length: five\r\n\r\n"
        ));
    }

    #[test]
    fn a_head_over_the_buffer_is_a_bad_response() {
        let mut response = b"HTTP/1.1 200 OK\r\nX-Padding: ".to_vec();
        response.extend(core::iter::repeat_n(b'a', 2048));
        response.extend_from_slice(b"\r\n\r\n");

        assert!(is_bad(&response));
    }

    #[test]
    fn counter_accepts_zero_and_the_largest_i64() {
        for (value, expected) in [("0", 0), ("9223372036854775807", i64::MAX as u64)] {
            let response = std::format!(
                "HTTP/1.1 401 Unauthorized\r\nx-request-counter: {value}\r\ncontent-length: 0\r\n\r\n"
            );

            assert_eq!(answer(response.as_bytes()).counter, Some(expected));
        }
    }

    #[test]
    fn counter_header_name_is_matched_in_any_case() {
        let answer = answer(
            b"HTTP/1.1 401 Unauthorized\r\nX-Request-Counter: 41\r\nContent-Length: 0\r\n\r\n",
        );

        assert_eq!(answer.status, 401);
        assert_eq!(answer.counter, Some(41));
    }

    #[test]
    fn counter_is_none_when_absent_or_malformed() {
        for header in [
            "",
            "x-request-counter: \r\n",
            "x-request-counter: -1\r\n",
            "x-request-counter: +1\r\n",
            "x-request-counter: 4 2\r\n",
            "x-request-counter: abc\r\n",
            "x-request-counter: 9223372036854775808\r\n",
            "x-request-counter: 18446744073709551616\r\n",
        ] {
            let response =
                std::format!("HTTP/1.1 401 Unauthorized\r\n{header}content-length: 0\r\n\r\n");

            assert_eq!(answer(response.as_bytes()).counter, None, "{header}");
        }
    }
}
