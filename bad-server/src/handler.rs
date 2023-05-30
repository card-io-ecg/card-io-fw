use core::{future::Future, marker::PhantomData};

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

pub struct SimpleHandler<'a, F, FUT, C>
where
    F: Fn(RequestContext<'_, C>) -> FUT,
    FUT: Future<Output = Result<(), HandleError<C>>>,
    C: Connection,
{
    closure: F,
    method: Method,
    path: &'a str,
    _connection: PhantomData<C>,
}

impl<'a, F, FUT, C> SimpleHandler<'a, F, FUT, C>
where
    F: Fn(RequestContext<'_, C>) -> FUT,
    FUT: Future<Output = Result<(), HandleError<C>>>,
    C: Connection,
{
    pub fn new(method: Method, path: &'a str, closure: F) -> Self {
        Self {
            closure,
            method,
            path,
            _connection: PhantomData,
        }
    }

    pub fn get(path: &'a str, closure: F) -> Self {
        Self::new(Method::Get, path, closure)
    }

    pub fn post(path: &'a str, closure: F) -> Self {
        Self::new(Method::Post, path, closure)
    }
}

impl<F, FUT, C> Handler for SimpleHandler<'_, F, FUT, C>
where
    F: Fn(RequestContext<'_, C>) -> FUT,
    FUT: Future<Output = Result<(), HandleError<C>>>,
    C: Connection,
{
    type Connection = C;

    fn handles(&self, request: &RequestContext<'_, C>) -> bool {
        self.method == request.method() && self.path == request.path()
    }

    async fn handle(&self, request: RequestContext<'_, C>) -> Result<(), HandleError<C>> {
        (self.closure)(request).await
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
