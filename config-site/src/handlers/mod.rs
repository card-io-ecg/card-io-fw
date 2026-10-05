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

mod pairing;
mod profiles;
mod settings;
mod station;

pub use pairing::PairingControl;
pub use station::Station;

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
const MAX_BODY_SIZE: usize = 512;

const TEXT: &str = "text/plain; charset=utf-8";

/// Serves the configuration website.
pub struct ConfigSite<'a, S, P> {
    context: &'a SharedWebContext,
    station: &'a S,
    pairing: &'a P,
    fw_version: &'a str,
}

impl<'a, S: Station, P: PairingControl> ConfigSite<'a, S, P> {
    pub fn new(
        context: &'a SharedWebContext,
        station: &'a S,
        pairing: &'a P,
        fw_version: &'a str,
    ) -> Self {
        Self {
            context,
            station,
            pairing,
            fw_version,
        }
    }
}

impl<S: Station, P: PairingControl> Handler for ConfigSite<'_, S, P> {
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
            (Method::Get, "/networks") => station::networks(self.station, conn).await,
            (Method::Get, "/profiles") => profiles::list(self.context, conn).await,
            (Method::Get, "/status") => station::status(self.station, conn).await,
            (Method::Get, "/settings") => settings::get(self.context, conn).await,
            (Method::Get, "/pairing") => pairing::status(self.pairing, conn).await,
            (Method::Post, "/profiles") => profiles::add(self.context, conn).await,
            (Method::Post, "/forget") => profiles::forget(self.context, self.station, conn).await,
            (Method::Post, "/settings") => settings::set(self.context, conn).await,
            (Method::Post, "/settings/discard") => settings::discard(self.context, conn).await,
            (Method::Post, "/scan") => station::scan(self.station, conn).await,
            (Method::Post, "/pair") => pairing::pair(self.pairing, conn).await,
            (Method::Post, "/unpair") => pairing::unpair(self.pairing, conn).await,
            _ => respond(conn, 404, "Not found").await,
        }
    }
}

fn length(len: usize) -> heapless::String<20> {
    let mut text = heapless::String::new();
    unwrap!(write!(text, "{len}"));
    text
}

async fn send_static<T, const N: usize>(
    conn: &mut Connection<'_, T, N>,
    content_type: &str,
    body: &[u8],
) -> Result<(), Error<T::Error>>
where
    T: Read + Write,
{
    let len = length(body.len());
    let headers = [
        ("Content-Type", content_type),
        ("Content-Length", len.as_str()),
        ("Content-Encoding", CONTENT_ENCODING.unwrap_or_default()),
    ];
    let headers = if CONTENT_ENCODING.is_some() {
        &headers[..]
    } else {
        &headers[..2]
    };

    conn.initiate_response(200, None, headers).await?;
    conn.write_all(body).await
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
    let len = length(body.len());
    let headers = [
        ("Content-Type", TEXT),
        ("Content-Length", len.as_str()),
        ("Cache-Control", "no-store"),
    ];

    conn.initiate_response(status, None, &headers).await?;
    conn.write_all(body.as_bytes()).await
}

/// Answers `200`, or `400` with the reason word.
pub(crate) async fn respond_result<T, const N: usize>(
    conn: &mut Connection<'_, T, N>,
    result: Result<(), &str>,
) -> Result<(), Error<T::Error>>
where
    T: Read + Write,
{
    match result {
        Ok(()) => respond(conn, 200, "").await,
        Err(reason) => respond(conn, 400, reason).await,
    }
}

/// Sends one text line for each item. The response length is not known in advance.
pub(crate) async fn respond_lines<T, const N: usize>(
    conn: &mut Connection<'_, T, N>,
    items: impl Iterator<Item = impl Display>,
) -> Result<(), Error<T::Error>>
where
    T: Read + Write,
{
    let headers = [("Content-Type", TEXT), ("Cache-Control", "no-store")];
    conn.initiate_response(200, None, &headers).await?;

    // The longest line is a visible network: `-128 1 ` and a 32 byte SSID.
    let mut line = heapless::String::<48>::new();
    for item in items {
        line.clear();
        unwrap!(writeln!(line, "{item}"));
        conn.write_all(line.as_bytes()).await?;
    }
    Ok(())
}

/// Reads the request body as text, without its trailing line break.
///
/// Returns `None` after sending `400 too large` if the body is over `MAX_BODY_SIZE` bytes or is
/// not valid UTF-8.
pub(crate) async fn read_body<'b, T, const N: usize>(
    conn: &mut Connection<'_, T, N>,
    buf: &'b mut [u8; MAX_BODY_SIZE],
) -> Result<Option<&'b str>, Error<T::Error>>
where
    T: Read + Write,
{
    let mut len = 0;
    loop {
        if len == buf.len() {
            if conn.read(&mut [0]).await? != 0 {
                warn!("POST body is too large");
                respond(conn, 400, "too large").await?;
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

    match core::str::from_utf8(&buf[..len]) {
        Ok(body) => Ok(Some(body.trim_end_matches(['\r', '\n']))),
        Err(_err) => {
            warn!("POST body is not valid UTF-8");
            respond(conn, 400, "too large").await?;
            Ok(None)
        }
    }
}
