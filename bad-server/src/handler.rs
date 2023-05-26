use core::future::Future;

use embassy_net::tcp::TcpSocket;
use object_chain::{Chain, ChainElement, Link};

use crate::method::Method;

pub struct Request<'req, 'socket> {
    method: Method,
    path: &'req str,
    body: &'req [u8],
    headers: &'req [httparse::Header<'req>],
    socket: &'req mut TcpSocket<'socket>,
}

impl<'req, 'socket> Request<'req, 'socket> {
    pub fn new(
        req: httparse::Request<'req, 'req>,
        body: &'req [u8],
        socket: &'req mut TcpSocket<'socket>,
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
            socket,
        })
    }
}

pub trait Handler {
    /// Returns `true` if this handler can handle the given request.
    fn handles(&self, request: &Request<'_, '_>) -> bool;

    /// Handles the given request.
    async fn handle(&self, request: Request<'_, '_>);
}

impl Handler for () {
    fn handles(&self, _request: &Request<'_, '_>) -> bool {
        false
    }

    async fn handle(&self, _request: Request<'_, '_>) {}
}

pub struct ClosureHandler<'a, F> {
    closure: F,
    method: Method,
    path: &'a str,
}

impl<'a, F, FUT> ClosureHandler<'a, F>
where
    F: Fn(Request) -> FUT,
    FUT: Future<Output = ()>,
{
    pub fn new(method: Method, path: &'a str, closure: F) -> Self {
        Self {
            closure,
            method,
            path,
        }
    }

    pub fn get(path: &'a str, closure: F) -> Self {
        Self::new(Method::Get, path, closure)
    }

    pub fn post(path: &'a str, closure: F) -> Self {
        Self::new(Method::Post, path, closure)
    }
}

impl<F, FUT> Handler for ClosureHandler<'_, F>
where
    F: Fn(Request) -> FUT,
    FUT: Future<Output = ()>,
{
    fn handles(&self, request: &Request<'_, '_>) -> bool {
        self.method == request.method && self.path == request.path
    }

    async fn handle(&self, request: Request<'_, '_>) {
        (self.closure)(request).await
    }
}

impl<H> Handler for Chain<H>
where
    H: Handler,
{
    fn handles(&self, request: &Request<'_, '_>) -> bool {
        self.object.handles(request)
    }

    async fn handle(&self, request: Request<'_, '_>) {
        self.object.handle(request).await
    }
}

impl<V, C> Handler for Link<V, C>
where
    V: Handler,
    C: ChainElement + Handler,
{
    fn handles(&self, request: &Request<'_, '_>) -> bool {
        self.object.handles(request) || self.parent.handles(request)
    }

    async fn handle(&self, request: Request<'_, '_>) {
        if self.object.handles(&request) {
            self.object.handle(request).await
        } else {
            self.parent.handle(request).await
        }
    }
}
