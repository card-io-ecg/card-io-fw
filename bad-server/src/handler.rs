use core::marker::PhantomData;

use object_chain::{Chain, ChainElement, Link};

use crate::{connector::Connection, method::Method, request_context::RequestContext, HandleError};

pub trait Handler {
    type Connection: Connection;

    /// Returns `true` if this handler can handle the given request.
    fn handles(&self, request: &RequestContext<'_, Self::Connection>) -> bool;

    /// Handles the given request.
    async fn handle(
        &self,
        request: RequestContext<'_, Self::Connection>,
    ) -> Result<(), HandleError<Self::Connection>>;
}

pub struct NoHandler<C: Connection>(pub(crate) PhantomData<C>);
impl<C: Connection> Handler for NoHandler<C> {
    type Connection = C;

    fn handles(&self, _request: &RequestContext<'_, C>) -> bool {
        false
    }

    async fn handle(&self, _request: RequestContext<'_, C>) -> Result<(), HandleError<C>> {
        Ok(())
    }
}

pub trait RequestHandler<C: Connection> {
    async fn handle(&self, request: RequestContext<'_, C>) -> Result<(), HandleError<C>>;

    fn new<'a>(method: Method, path: &'a str, handler: Self) -> RequestWithMatcher<'a, C, Self>
    where
        Self: Sized,
    {
        RequestWithMatcher::new(method, path, handler)
    }

    fn get<'a>(path: &'a str, handler: Self) -> RequestWithMatcher<'a, C, Self>
    where
        Self: Sized,
    {
        Self::new(Method::Get, path, handler)
    }

    fn post<'a>(path: &'a str, handler: Self) -> RequestWithMatcher<'a, C, Self>
    where
        Self: Sized,
    {
        Self::new(Method::Post, path, handler)
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

    fn handles(&self, request: &RequestContext<'_, C>) -> bool {
        self.method == request.method() && self.path == request.path()
    }

    async fn handle(&self, request: RequestContext<'_, C>) -> Result<(), HandleError<C>> {
        self.handler.handle(request).await
    }
}

impl<H, C> Handler for Chain<H>
where
    H: Handler<Connection = C>,
    C: Connection,
{
    type Connection = C;
    fn handles(&self, request: &RequestContext<'_, C>) -> bool {
        self.object.handles(request)
    }

    async fn handle(&self, request: RequestContext<'_, C>) -> Result<(), HandleError<C>> {
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

    fn handles(&self, request: &RequestContext<'_, C>) -> bool {
        self.object.handles(request) || self.parent.handles(request)
    }

    async fn handle(&self, request: RequestContext<'_, C>) -> Result<(), HandleError<C>> {
        if self.object.handles(&request) {
            self.object.handle(request).await
        } else {
            self.parent.handle(request).await
        }
    }
}
