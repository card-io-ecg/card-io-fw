pub mod add_new_network;
pub mod backend_url;
pub mod change_backend_url;
pub mod delete_network;
pub mod list_known_networks;

#[cfg(feature = "compress")]
mod statics {
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
}

#[cfg(not(feature = "compress"))]
mod statics {
    use bad_server::handler::StaticHandler;

    pub const INDEX_HANDLER: StaticHandler =
        StaticHandler::new(&[], include_bytes!("../../static/index.html"));

    pub const HEADER_FONT: StaticHandler =
        StaticHandler::new(&[], include_bytes!("../../static/Poppins-Regular.ttf"));
}

pub use statics::*;
