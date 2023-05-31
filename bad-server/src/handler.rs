use core::{fmt::Write as _, marker::PhantomData};

use httparse::Header;
use object_chain::{Chain, ChainElement, Link};

use crate::{
    connector::Connection,
    method::Method,
    request::Request,
    response::{Response, ResponseStatus},
    HandleError,
};

pub trait Handler {
    type Connection: Connection;

    /// Returns `true` if this handler can handle the given request.
    fn handles(&self, request: &Request<'_, '_, Self::Connection>) -> bool;

    /// Handles the given request.
    async fn handle(
        &self,
        request: Request<'_, '_, Self::Connection>,
        response: Response<'_, Self::Connection>,
    ) -> Result<(), HandleError<Self::Connection>>;
}

pub struct NoHandler<C: Connection>(pub(crate) PhantomData<C>);
impl<C: Connection> Handler for NoHandler<C> {
    type Connection = C;

    fn handles(&self, _request: &Request<'_, '_, C>) -> bool {
        false
    }

    async fn handle(
        &self,
        _request: Request<'_, '_, C>,
        _response: Response<'_, C>,
    ) -> Result<(), HandleError<C>> {
        Ok(())
    }
}

pub trait RequestHandler<C: Connection>: Sized {
    async fn handle(
        &self,
        request: Request<'_, '_, C>,
        response: Response<'_, C>,
    ) -> Result<(), HandleError<C>>;

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

pub struct StaticHandler<'a>(pub &'a [Header<'a>], pub &'a [u8]);

impl<C: Connection> RequestHandler<C> for StaticHandler<'_> {
    async fn handle(
        &self,
        _request: Request<'_, '_, C>,
        response: Response<'_, C>,
    ) -> Result<(), HandleError<C>> {
        let mut response = response.send_status(ResponseStatus::Ok).await?;

        let mut length = heapless::String::<12>::new();
        write!(length, "{}", self.1.len()).unwrap();

        response
            .send_headers(self.0)
            .await?
            .send_header(Header {
                name: "Content-Length",
                value: length.as_bytes(),
            })
            .await?;

        response.start_body().await?.write_raw(self.1).await
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

    async fn handle(
        &self,
        request: Request<'_, '_, C>,
        response: Response<'_, C>,
    ) -> Result<(), HandleError<C>> {
        self.handler.handle(request, response).await
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

    async fn handle(
        &self,
        request: Request<'_, '_, C>,
        response: Response<'_, C>,
    ) -> Result<(), HandleError<C>> {
        self.object.handle(request, response).await
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

    async fn handle(
        &self,
        request: Request<'_, '_, C>,
        response: Response<'_, C>,
    ) -> Result<(), HandleError<C>> {
        if self.object.handles(&request) {
            self.object.handle(request, response).await
        } else {
            self.parent.handle(request, response).await
        }
    }
}
