use core::marker::PhantomData;

use const_base::ArrayStr;
use const_fnv1a_hash::fnv1a_hash_32;
use httparse::Header;
use object_chain::{Chain, ChainElement, Link};

use crate::{
    connector::Connection, method::Method, request::Request, response::ResponseStatus, HandleError,
};

pub trait Handler {
    type Connection: Connection;

    /// Returns `true` if this handler can handle the given request.
    fn handles(&self, request: &Request<'_, '_, Self::Connection>) -> bool;

    /// Handles the given request.
    async fn handle(
        &self,
        request: Request<'_, '_, Self::Connection>,
    ) -> Result<(), HandleError<Self::Connection>>;
}

pub struct NoHandler<C: Connection>(pub(crate) PhantomData<C>);
impl<C: Connection> Handler for NoHandler<C> {
    type Connection = C;

    fn handles(&self, _request: &Request<'_, '_, C>) -> bool {
        false
    }

    async fn handle(&self, _request: Request<'_, '_, C>) -> Result<(), HandleError<C>> {
        Ok(())
    }
}

pub trait RequestHandler<C: Connection>: Sized {
    async fn handle(&self, request: Request<'_, '_, C>) -> Result<(), HandleError<C>>;

    fn new(method: Method, path: &str, handler: Self) -> RequestWithMatcher<'_, C, Self> {
        RequestWithMatcher::new(method, path, handler)
    }

    fn get(path: &str, handler: Self) -> RequestWithMatcher<'_, C, Self> {
        Self::new(Method::Get, path, handler)
    }

    fn post(path: &str, handler: Self) -> RequestWithMatcher<'_, C, Self> {
        Self::new(Method::Post, path, handler)
    }
}

const BASE64_HASH_LEN: usize = const_base::encoded_len(4, const_base::Config::B64);

pub struct StaticHandler<'a> {
    headers: &'a [Header<'a>],
    body: &'a [u8],
    hash: ArrayStr<BASE64_HASH_LEN>,
}

impl<'a> StaticHandler<'a> {
    pub const fn new(headers: &'a [Header<'a>], body: &'a [u8]) -> Self {
        let hash = fnv1a_hash_32(body, None);
        let hash = match const_base::encode(&hash.to_le_bytes(), const_base::Config::B64) {
            Ok(hash) => hash,
            Err(_err) => ::core::panic!("Failed to base64-encode hash"),
        };

        Self {
            headers,
            body,
            hash,
        }
    }
}

impl<C: Connection> RequestHandler<C> for StaticHandler<'_> {
    async fn handle(&self, request: Request<'_, '_, C>) -> Result<(), HandleError<C>> {
        let etag_header = Header {
            name: "ETag",
            value: self.hash.as_slice(),
        };

        let status = if let Some(etag) = request.raw_header("if-none-match") {
            if etag == self.hash.as_slice() {
                ResponseStatus::NotModified
            } else {
                ResponseStatus::Ok
            }
        } else {
            ResponseStatus::Ok
        };

        let mut response = request.start_response(status).await?;
        if status == ResponseStatus::NotModified {
            response.start_body().await.map(|_| ())
        } else {
            response
                .send_headers(self.headers)
                .await?
                .send_headers(&[etag_header])
                .await?;

            response.send_body(self.body).await
        }
    }
}

pub struct RequestWithMatcher<'a, C: Connection, H: RequestHandler<C>> {
    method: Method,
    path: &'a str,
    handler: H,
    _connection: PhantomData<C>,
}

impl<'a, C: Connection, H: RequestHandler<C>> RequestWithMatcher<'a, C, H> {
    fn new(method: Method, path: &'a str, handler: H) -> Self {
        Self {
            method,
            path,
            handler,
            _connection: PhantomData,
        }
    }
}

impl<'a, C, H> Handler for RequestWithMatcher<'a, C, H>
where
    C: Connection,
    H: RequestHandler<C>,
{
    type Connection = C;

    fn handles(&self, request: &Request<'_, '_, C>) -> bool {
        self.method == request.method && self.path == request.path
    }

    async fn handle(&self, request: Request<'_, '_, C>) -> Result<(), HandleError<C>> {
        self.handler.handle(request).await
    }
}

impl<H, C> Handler for Chain<H>
where
    H: Handler<Connection = C>,
    C: Connection,
{
    type Connection = C;

    fn handles(&self, request: &Request<'_, '_, C>) -> bool {
        self.object.handles(request)
    }

    async fn handle(&self, request: Request<'_, '_, C>) -> Result<(), HandleError<C>> {
        self.object.handle(request).await
    }
}

impl<V, CE, C> Handler for Link<V, CE>
where
    V: Handler<Connection = C>,
    CE: ChainElement + Handler<Connection = C>,
    C: Connection,
{
    type Connection = C;

    fn handles(&self, request: &Request<'_, '_, C>) -> bool {
        self.object.handles(request) || self.parent.handles(request)
    }

    async fn handle(&self, request: Request<'_, '_, C>) -> Result<(), HandleError<C>> {
        if self.object.handles(&request) {
            self.object.handle(request).await
        } else {
            self.parent.handle(request).await
        }
    }
}
