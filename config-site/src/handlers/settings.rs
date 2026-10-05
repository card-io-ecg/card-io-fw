use core::fmt::Write as _;

use edge_http::io::{server::Connection, Error};
use embedded_io_async::{Read, Write};
use network_services::url;

use crate::{
    data::SharedWebContext,
    handlers::{read_body, respond, respond_result, MAX_BODY_SIZE},
};

pub async fn get<T, const N: usize>(
    context: &SharedWebContext,
    conn: &mut Connection<'_, T, N>,
) -> Result<(), Error<T::Error>>
where
    T: Read + Write,
{
    let mut line = heapless::String::<65>::new();
    unwrap!(writeln!(line, "{}", context.lock().await.backend_url));

    respond(conn, 200, &line).await
}

pub async fn set<T, const N: usize>(
    context: &SharedWebContext,
    conn: &mut Connection<'_, T, N>,
) -> Result<(), Error<T::Error>>
where
    T: Read + Write,
{
    let mut buf = [0; MAX_BODY_SIZE];
    let Some(body) = read_body(conn, &mut buf).await? else {
        return Ok(());
    };

    respond_result(conn, store(context, body).await).await
}

pub async fn discard<T, const N: usize>(
    context: &SharedWebContext,
    conn: &mut Connection<'_, T, N>,
) -> Result<(), Error<T::Error>>
where
    T: Read + Write,
{
    context.lock().await.discard_settings();

    respond(conn, 200, "").await
}

/// An empty address means no server.
async fn store(context: &SharedWebContext, body: &str) -> Result<(), &'static str> {
    let backend_url = heapless::String::<64>::try_from(body)
        .ok()
        .filter(|address| address.is_empty() || url::parse(address).is_some())
        .ok_or("host")?;

    context.lock().await.backend_url = backend_url;
    Ok(())
}
