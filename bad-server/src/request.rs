use httparse::Header;

use crate::{
    connector::Connection,
    method::Method,
    request_body::{ReadResult, RequestBody},
    response::ResponseStatus,
};

pub struct Request<'req> {
    pub method: Method,
    pub path: &'req str,
    body: RequestBody<'req>,
    headers: &'req [Header<'req>],
}

impl<'req> Request<'req> {
    pub(crate) fn new(
        req: httparse::Request<'req, 'req>,
        body: RequestBody<'req>,
    ) -> Result<Self, ResponseStatus> {
        let Some(path) = req.path else {
            log::warn!("Path not set");
            return Err(ResponseStatus::BadRequest);
        };

        let Some(method) = req.method.and_then(Method::new) else {
            log::warn!("Unknown method: {:?}", req.method);
            return Err(ResponseStatus::BadRequest);
        };

        log::info!("[{}] {path}", method.as_str());

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

    pub async fn read<C: Connection>(
        &mut self,
        buf: &mut [u8],
        socket: &mut C,
    ) -> ReadResult<usize, C> {
        self.body.read(buf, socket).await
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
}
