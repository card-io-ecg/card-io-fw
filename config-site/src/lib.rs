#![no_std]
#![allow(stable_features)]
#![feature(async_fn_in_trait)]
#![feature(generic_const_exprs)]
#![allow(unknown_lints, async_fn_in_trait)]
#![allow(incomplete_features)] // generic_const_exprs

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

#[macro_use]
extern crate logger;

pub mod data;
pub mod handlers;

#[inline(always)]
pub fn create<'a, CON>(
    context: &'a SharedWebContext,
    fw_version: &'a str,
) -> BadServer<
    impl Handler<Connection = CON> + 'a + object_chain::ChainElement,
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
        .with_handler(RequestHandler::get("/kn", ListKnownNetworks { context }))
        .with_handler(RequestHandler::post("/nn", AddNewNetwork { context }))
        .with_handler(RequestHandler::post("/dn", DeleteNetwork { context }))
        .with_handler(RequestHandler::get("/bu", BackendUrl { context }))
        .with_handler(RequestHandler::post("/cbu", ChangeBackendUrl { context }))
}
