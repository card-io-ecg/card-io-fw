use httparse::Header;

use crate::{
    connector::Connection,
    method::Method,
    request_body::{ReadResult, RequestBody},
    response::{Headers, Response, ResponseStatus},
    HandleError,
};

pub struct Request<'req, 's, C: Connection> {
    pub method: Method,
    pub path: &'req str,
    body: RequestBody<'req, 's, C>,
    headers: &'req [Header<'req>],
}

impl<'req, 's, C: Connection> Request<'req, 's, C> {
    pub(crate) fn new(
        req: httparse::Request<'req, 'req>,
        body: RequestBody<'req, 's, C>,
    ) -> Result<Self, ResponseStatus> {
        let Some(path) = req.path else {
            warn!("Path not set");
            return Err(ResponseStatus::BadRequest);
        };

        let Some(method) = req.method.and_then(Method::new) else {
            warn!("Unknown method: {:?}", req.method);
            return Err(ResponseStatus::BadRequest);
        };

        info!("[{}] {}", method.as_str(), path);

        Ok(Self {
            method,
            path,
            body,
            headers: req.headers,
        })
    }

    pub fn is_complete(&self) -> bool {
        self.body.is_complete()
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> ReadResult<usize, C> {
        self.body.read(buf).await
    }

    pub async fn read_all<'b>(&mut self, buffer: &'b mut [u8]) -> ReadResult<&'b mut [u8], C> {
        let mut read = 0;

        while !self.is_complete() && !buffer.is_empty() {
            read += self.read(&mut buffer[read..]).await?;
        }
        debug!("Read {} bytes", read);

        Ok(&mut buffer[..read])
    }

    pub fn raw_header(&self, name: &str) -> Option<&[u8]> {
        self.headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(name))
            .map(|header| header.value)
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.raw_header(name)
            .and_then(|header| core::str::from_utf8(header).ok())
    }

    pub async fn start_response(
        self,
        status: ResponseStatus,
    ) -> Result<Response<'s, C, Headers>, HandleError<C>> {
        let socket = self.body.take_socket();

        Response::new(socket).send_status(status).await
    }

    async fn send_response_impl(
        self,
        status: ResponseStatus,
        body: impl AsRef<[u8]>,
    ) -> Result<(), HandleError<C>> {
        let socket = self.body.take_socket();

        Response::new(socket)
            .send_status(status)
            .await?
            .send_body(body)
            .await
    }

    pub async fn send_response(self, body: impl AsRef<[u8]>) -> Result<(), HandleError<C>> {
        self.send_response_impl(ResponseStatus::Ok, body).await
    }

    pub async fn send_error_response(
        self,
        status: ResponseStatus,
        message: &str,
    ) -> Result<(), HandleError<C>> {
        warn!("Request error: {:?}, {}", status, message);
        self.send_response_impl(status, message).await
    }
}
