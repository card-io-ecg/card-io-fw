use edge_http::io::{server::Connection, Error};
use embedded_io_async::{Read, Write};

use crate::{
    data::{
        network::{StationStatus, WifiNetwork},
        SharedWebContext,
    },
    handlers::{read_body, respond_lines, respond_result, station::Station, MAX_BODY_SIZE},
};

pub async fn list<T, const N: usize>(
    context: &SharedWebContext,
    conn: &mut Connection<'_, T, N>,
) -> Result<(), Error<T::Error>>
where
    T: Read + Write,
{
    let context = context.lock().await;

    respond_lines(
        conn,
        context.known_networks.iter().map(|network| &network.ssid),
    )
    .await
}

pub async fn add<T, const N: usize>(
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

    respond_result(conn, add_network(context, body).await).await
}

pub async fn forget<T, S, const N: usize>(
    context: &SharedWebContext,
    station: &S,
    conn: &mut Connection<'_, T, N>,
) -> Result<(), Error<T::Error>>
where
    T: Read + Write,
    S: Station,
{
    let mut buf = [0; MAX_BODY_SIZE];
    let Some(ssid) = read_body(conn, &mut buf).await? else {
        return Ok(());
    };

    respond_result(conn, forget_network(context, station, ssid).await).await
}

async fn add_network(context: &SharedWebContext, body: &str) -> Result<(), &'static str> {
    let network = parse_network(body).ok_or("invalid")?;

    let mut context = context.lock().await;
    if context
        .known_networks
        .iter()
        .any(|known| known.ssid == network.ssid)
    {
        return Err("duplicate");
    }
    context.known_networks.push(network).map_err(|_| "full")
}

/// Reads `{ssid}\n{password}`. A body without a password line has an empty password.
fn parse_network(body: &str) -> Option<WifiNetwork> {
    let (ssid, pass) = body.split_once('\n').unwrap_or((body, ""));
    let ssid = heapless::String::try_from(ssid.trim_end_matches('\r')).ok()?;
    let pass = heapless::String::try_from(pass).ok()?;

    if ssid.is_empty() {
        return None;
    }
    Some(WifiNetwork { ssid, pass })
}

/// Removes the network from the page's list, then drops the link if the station uses it.
async fn forget_network<S: Station>(
    context: &SharedWebContext,
    station: &S,
    ssid: &str,
) -> Result<(), &'static str> {
    {
        let mut context = context.lock().await;
        let index = context
            .known_networks
            .iter()
            .position(|known| known.ssid == ssid)
            .ok_or("unknown")?;
        context.known_networks.remove(index);
    }

    if let StationStatus::Joining(current) | StationStatus::Joined(current) = station.status().await
    {
        if current == ssid {
            station.drop_link(ssid).await;
        }
    }
    Ok(())
}
