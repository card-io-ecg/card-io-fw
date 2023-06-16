#![no_std]

use bad_server::{handler::StaticHandler, Header};

pub const INDEX_HANDLER: StaticHandler = StaticHandler::new(
    &[Header {
        name: "Content-Encoding",
        value: b"gzip",
    }],
    include_bytes!(concat!(env!("COMPRESS_OUT_DIR"), "/static/index.html.gz")),
);

pub const HEADER_FONT: StaticHandler = StaticHandler::new(
    &[Header {
        name: "Content-Encoding",
        value: b"gzip",
    }],
    include_bytes!(concat!(
        env!("COMPRESS_OUT_DIR"),
        "/static/Poppins-Regular.ttf.gz"
    )),
);
