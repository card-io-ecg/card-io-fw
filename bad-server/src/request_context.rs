use httparse::Header;

use crate::{
    connector::Connection,
    method::Method,
    request::Request,
    request_body::{ReadResult, RequestBody},
    response::{Body, Headers, Initial, Response, ResponseState, ResponseStatus},
};

pub struct RequestContext<'req, C, RSP = Initial>
where
    C: Connection,
    RSP: ResponseState,
{
    connection: &'req mut C,
    response: Response<RSP>,
    request: Request<'req>,
}

impl<'req, C> RequestContext<'req, C, Initial>
where
    C: Connection,
{
    pub fn new(
        req: httparse::Request<'req, 'req>,
        body: RequestBody<'req>,
        connection: &'req mut C,
    ) -> Result<Self, ResponseStatus> {
        Request::new(req, body).map(|request| Self {
            connection,
            response: Response::new(),
            request,
        })
    }

    pub async fn send_status(
        self,
        status: ResponseStatus,
    ) -> Result<RequestContext<'req, C, Headers>, C::Error> {
        Ok(RequestContext {
            response: self.response.send_status(status, self.connection).await?,
            connection: self.connection,
            request: self.request,
        })
    }
}

impl<'req, C> RequestContext<'req, C, Headers>
where
    C: Connection,
{
    pub async fn send_header(&mut self, header: Header<'_>) -> Result<&mut Self, C::Error> {
        self.response.send_header(header, self.connection).await?;
        Ok(self)
    }

    pub async fn send_headers(&mut self, headers: &[Header<'_>]) -> Result<&mut Self, C::Error> {
        self.response.send_headers(headers, self.connection).await?;
        Ok(self)
    }

    pub async fn end_headers(self) -> Result<RequestContext<'req, C, Body>, C::Error> {
        Ok(RequestContext {
            response: self.response.start_body(self.connection).await?,
            connection: self.connection,
            request: self.request,
        })
    }
}

impl<'req, C> RequestContext<'req, C, Body>
where
    C: Connection,
{
    pub async fn write_string(&mut self, data: &str) -> Result<(), C::Error> {
        self.response.write_string(data, self.connection).await
    }

    pub async fn write_raw(&mut self, data: &[u8]) -> Result<(), C::Error> {
        self.response.write_raw(data, self.connection).await
    }
}

impl<'req, C, RSP> RequestContext<'req, C, RSP>
where
    C: Connection,
    RSP: ResponseState,
{
    pub fn method(&self) -> Method {
        self.request.method
    }

    pub fn path(&self) -> &str {
        self.request.path
    }

    pub fn is_request_complete(&self) -> bool {
        self.request.is_complete()
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> ReadResult<usize, C> {
        self.request.read(buf, self.connection).await
    }

    pub fn header(&self, header: &str) -> Option<&str> {
        self.request.header(header)
    }

    pub fn raw_header(&self, header: &str) -> Option<&[u8]> {
        self.request.raw_header(header)
    }
}
