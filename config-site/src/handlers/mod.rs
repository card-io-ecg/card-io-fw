use core::fmt::{Debug, Display, Write as _};

use edge_http::{
    io::{
        server::{Connection, Handler},
        Error,
    },
    Method,
};
use embedded_io_async::{Read, Write};

use crate::data::SharedWebContext;

mod add_new_network;
mod backend_url;
mod change_backend_url;
mod delete_network;
mod list_known_networks;

#[cfg(feature = "compress")]
mod statics {
    pub const CONTENT_ENCODING: Option<&str> = Some("gzip");

    pub const INDEX: &[u8] =
        include_bytes!(concat!(env!("COMPRESS_OUT_DIR"), "/static/index.html.gz"));

    pub const HEADER_FONT: &[u8] = include_bytes!(concat!(
        env!("COMPRESS_OUT_DIR"),
        "/static/Poppins-Regular.ttf.gz"
    ));
}

#[cfg(not(feature = "compress"))]
mod statics {
    pub const CONTENT_ENCODING: Option<&str> = None;

    pub const INDEX: &[u8] = include_bytes!("../../static/index.html");

    pub const HEADER_FONT: &[u8] = include_bytes!("../../static/Poppins-Regular.ttf");
}

use statics::*;

/// Request bodies larger than this are rejected.
const MAX_BODY_SIZE: usize = 100;

/// Serves the configuration website.
pub struct ConfigSite<'a> {
    context: &'a SharedWebContext,
    fw_version: &'a str,
}

impl<'a> ConfigSite<'a> {
    pub fn new(context: &'a SharedWebContext, fw_version: &'a str) -> Self {
        Self {
            context,
            fw_version,
        }
    }
}

impl Handler for ConfigSite<'_> {
    type Error<E>
        = Error<E>
    where
        E: Debug;

    async fn handle<T, const N: usize>(
        &self,
        _task_id: impl Display + Copy,
        conn: &mut Connection<'_, T, N>,
    ) -> Result<(), Self::Error<T::Error>>
    where
        T: Read + Write,
    {
        let headers = conn.headers()?;
        let method = headers.method;
        let path = headers.path.split('?').next().unwrap_or_default();

        match (method, path) {
            (Method::Get, "/") => send_static(conn, "text/html; charset=utf-8", INDEX).await,
            (Method::Get, "/font") => send_static(conn, "font/ttf", HEADER_FONT).await,
            (Method::Get, "/si") => respond(conn, 200, self.fw_version).await,
            (Method::Get, "/kn") => list_known_networks::handle(self.context, conn).await,
            (Method::Post, "/nn") => add_new_network::handle(self.context, conn).await,
            (Method::Post, "/dn") => delete_network::handle(self.context, conn).await,
            (Method::Get, "/bu") => backend_url::handle(self.context, conn).await,
            (Method::Post, "/cbu") => change_backend_url::handle(self.context, conn).await,
            _ => respond(conn, 404, "Not found").await,
        }
    }
}

async fn send_static<T, const N: usize>(
    conn: &mut Connection<'_, T, N>,
    content_type: &str,
    body: &[u8],
) -> Result<(), Error<T::Error>>
where
    T: Read + Write,
{
    send(conn, 200, content_type, CONTENT_ENCODING, body).await
}

/// Sends a plain text response.
pub(crate) async fn respond<T, const N: usize>(
    conn: &mut Connection<'_, T, N>,
    status: u16,
    body: &str,
) -> Result<(), Error<T::Error>>
where
    T: Read + Write,
{
    send(
        conn,
        status,
        "text/plain; charset=utf-8",
        None,
        body.as_bytes(),
    )
    .await
}

async fn send<T, const N: usize>(
    conn: &mut Connection<'_, T, N>,
    status: u16,
    content_type: &str,
    content_encoding: Option<&str>,
    body: &[u8],
) -> Result<(), Error<T::Error>>
where
    T: Read + Write,
{
    let mut len = heapless::String::<20>::new();
    unwrap!(write!(len, "{}", body.len()));

    let headers = [
        ("Content-Type", content_type),
        ("Content-Length", len.as_str()),
        ("Content-Encoding", content_encoding.unwrap_or_default()),
    ];
    let headers = if content_encoding.is_some() {
        &headers[..]
    } else {
        &headers[..2]
    };

    conn.initiate_response(status, None, headers).await?;
    conn.write_all(body).await
}

/// Starts a plain text response whose length is not known in advance.
pub(crate) async fn start_text_response<T, const N: usize>(
    conn: &mut Connection<'_, T, N>,
) -> Result<(), Error<T::Error>>
where
    T: Read + Write,
{
    conn.initiate_response(200, None, &[("Content-Type", "text/plain; charset=utf-8")])
        .await
}

/// Reads the request body as text.
///
/// Returns `None` after sending an error response if the body is too large or not valid UTF-8.
pub(crate) async fn read_body<'b, T, const N: usize>(
    conn: &mut Connection<'_, T, N>,
    buf: &'b mut [u8; MAX_BODY_SIZE],
) -> Result<Option<&'b str>, Error<T::Error>>
where
    T: Read + Write,
{
    debug!("Reading POST data");

    let mut len = 0;
    loop {
        if len == buf.len() {
            if conn.read(&mut [0]).await? != 0 {
                respond(conn, 413, "POST body too large").await?;
                return Ok(None);
            }
            break;
        }

        let read = conn.read(&mut buf[len..]).await?;
        if read == 0 {
            break;
        }
        len += read;
    }

    let post_data = &buf[..len];
    match core::str::from_utf8(post_data) {
        Ok(body) => {
            debug!("POST body: {:?}", body);
            Ok(Some(body))
        }
        Err(_err) => {
            warn!("Invalid UTF-8 in POST body: {:?}", post_data);
            respond(conn, 400, "Input is not valid text").await?;
            Ok(None)
        }
    }
}
