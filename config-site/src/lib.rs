#![no_std]
#![feature(async_fn_in_trait)]
#![feature(generic_const_exprs)]
#![allow(incomplete_features)]

use bad_server::{
    connector::Connection,
    error_handler::ErrorHandler,
    handler::{Handler, RequestHandler, StaticHandler},
    BadServer,
};

use crate::{
    data::SharedWebContext,
    handlers::{
        add_new_network::AddNewNetwork, backend_url::BackendUrl,
        change_backend_url::ChangeBackendUrl, delete_network::DeleteNetwork,
        list_known_networks::ListKnownNetworks, HEADER_FONT, INDEX_HANDLER,
    },
};

pub mod data;
pub mod handlers;

#[inline(always)]
pub fn create<'a, CON>(
    context: &'a SharedWebContext,
    fw_version: &'a str,
) -> BadServer<
    impl Handler<Connection = CON> + 'a,
    impl ErrorHandler<Connection = CON>,
    [u8; 1024],
    32,
>
where
    CON: Connection + 'a,
{
    BadServer::new()
        .with_handler(RequestHandler::get("/", INDEX_HANDLER))
        .with_handler(RequestHandler::get("/font", HEADER_FONT))
        .with_handler(RequestHandler::get(
            "/si",
            StaticHandler::new(&[], fw_version.as_bytes()),
        ))
        .with_handler(RequestHandler::get(
            "/kn",
            ListKnownNetworks { context: &context },
        ))
        .with_handler(RequestHandler::post(
            "/nn",
            AddNewNetwork { context: &context },
        ))
        .with_handler(RequestHandler::post(
            "/dn",
            DeleteNetwork { context: &context },
        ))
        .with_handler(RequestHandler::get("/bu", BackendUrl { context: &context }))
        .with_handler(RequestHandler::post(
            "/cbu",
            ChangeBackendUrl { context: &context },
        ))
}
