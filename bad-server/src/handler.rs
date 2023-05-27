use core::{future::Future, marker::PhantomData};

use object_chain::{Chain, ChainElement, Link};

use crate::{connector::Connection, method::Method, request_body::RequestBody};

pub struct Request<'req, C: Connection> {
    method: Method,
    path: &'req str,
    body: RequestBody<'req>,
    headers: &'req [httparse::Header<'req>],
    connection: &'req mut C,
}

impl<'req, C: Connection> Request<'req, C> {
    pub(crate) fn new(
        req: httparse::Request<'req, 'req>,
        body: RequestBody<'req>,
        connection: &'req mut C,
    ) -> Result<Self, ()> {
        let Some(path) = req.path else {
            log::warn!("Path not set");
            return Err(());
        };

        let Some(method) = req.method.and_then(Method::new) else {
            log::warn!("Unknown method: {:?}", req.method);
            return Err(());
        };

        Ok(Self {
            method,
            path,
            body,
            headers: req.headers,
            connection,
        })
    }

    pub async fn read_body(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        self.body.read(buf, self.connection).await
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

pub trait Handler {
    type Connection: Connection;

    /// Returns `true` if this handler can handle the given request.
    fn handles(&self, request: &Request<'_, Self::Connection>) -> bool;

    /// Handles the given request.
    async fn handle(&self, request: Request<'_, Self::Connection>);
}

pub struct NoHandler<C: Connection>(pub(crate) PhantomData<C>);
impl<C: Connection> Handler for NoHandler<C> {
    type Connection = C;

    fn handles(&self, _request: &Request<'_, Self::Connection>) -> bool {
        false
    }

    async fn handle(&self, _request: Request<'_, Self::Connection>) {}
}

pub struct ClosureHandler<'a, F, C> {
    closure: F,
    method: Method,
    path: &'a str,
    _connection: PhantomData<C>,
}

impl<'a, F, FUT, C> ClosureHandler<'a, F, C>
where
    F: Fn(Request<'_, C>) -> FUT,
    FUT: Future<Output = ()>,
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

impl<F, FUT, C> Handler for ClosureHandler<'_, F, C>
where
    F: Fn(Request<'_, C>) -> FUT,
    FUT: Future<Output = ()>,
    C: Connection,
{
    type Connection = C;

    fn handles(&self, request: &Request<'_, C>) -> bool {
        self.method == request.method && self.path == request.path
    }

    async fn handle(&self, request: Request<'_, C>) {
        (self.closure)(request).await
    }
}

impl<H, C> Handler for Chain<H>
where
    H: Handler<Connection = C>,
    C: Connection,
{
    type Connection = C;
    fn handles(&self, request: &Request<'_, C>) -> bool {
        self.object.handles(request)
    }

    async fn handle(&self, request: Request<'_, C>) {
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

    fn handles(&self, request: &Request<'_, C>) -> bool {
        self.object.handles(request) || self.parent.handles(request)
    }

    async fn handle(&self, request: Request<'_, C>) {
        if self.object.handles(&request) {
            self.object.handle(request).await
        } else {
            self.parent.handle(request).await
        }
    }
}
