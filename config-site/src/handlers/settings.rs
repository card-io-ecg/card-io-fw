use core::fmt::Write as _;

use edge_http::io::{server::Connection, Error};
use embedded_io_async::{Read, Write};
use network_services::url;

use crate::{
    data::{parse_ntp_server, parse_time_zone, SharedWebContext},
    handlers::{read_body, respond, respond_result, MAX_BODY_SIZE},
};

pub async fn get<T, const N: usize>(
    context: &SharedWebContext,
    conn: &mut Connection<'_, T, N>,
) -> Result<(), Error<T::Error>>
where
    T: Read + Write,
{
    let mut lines = heapless::String::<180>::new();
    {
        let context = context.lock().await;
        let settings = &context.settings;
        unwrap!(writeln!(lines, "{}", settings.backend_url));
        unwrap!(writeln!(lines, "{}", settings.ntp_server));
        unwrap!(writeln!(lines, "{}", settings.time_zone));
    }

    respond(conn, 200, &lines).await
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

/// The lines are the backend address, the NTP server and the time zone. A body of one line
/// changes only the address. An empty address means no server.
async fn store(context: &SharedWebContext, body: &str) -> Result<(), &'static str> {
    let mut lines = body.lines();

    let backend_url = heapless::String::<64>::try_from(lines.next().unwrap_or(""))
        .ok()
        .filter(|address| address.is_empty() || url::parse(address).is_some())
        .ok_or("host")?;

    let time = match lines.next() {
        Some(server) => {
            let server = parse_ntp_server(server)?;
            let zone = parse_time_zone(lines.next().ok_or("zone")?)?;
            Some((server, zone))
        }
        None => None,
    };

    let mut context = context.lock().await;
    let settings = &mut context.settings;
    settings.backend_url = backend_url;
    if let Some((server, zone)) = time {
        settings.ntp_server = server;
        settings.time_zone = zone;
    }
    Ok(())
}
