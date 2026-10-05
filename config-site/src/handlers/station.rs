use core::fmt::Write as _;

use edge_http::io::{server::Connection, Error};
use embedded_io_async::{Read, Write};

use crate::{
    data::network::{StationStatus, VisibleNetwork, MAX_VISIBLE_NETWORKS},
    handlers::{respond, respond_lines},
};

#[allow(async_fn_in_trait)]
pub trait Station {
    async fn status(&self) -> StationStatus;
    /// Adds the networks of the last scan. It does not scan.
    async fn visible_networks(&self, out: &mut heapless::Vec<VisibleNetwork, MAX_VISIBLE_NETWORKS>);
    async fn scan(&self);
    /// Called only when `ssid` is the joined or joining network.
    async fn drop_link(&self, ssid: &str);
}

pub async fn networks<T, S, const N: usize>(
    station: &S,
    conn: &mut Connection<'_, T, N>,
) -> Result<(), Error<T::Error>>
where
    T: Read + Write,
    S: Station,
{
    let mut networks = heapless::Vec::new();
    station.visible_networks(&mut networks).await;

    respond_lines(conn, networks.iter()).await
}

pub async fn status<T, S, const N: usize>(
    station: &S,
    conn: &mut Connection<'_, T, N>,
) -> Result<(), Error<T::Error>>
where
    T: Read + Write,
    S: Station,
{
    let mut line = heapless::String::<48>::new();
    unwrap!(writeln!(line, "{}", station.status().await));

    respond(conn, 200, &line).await
}

pub async fn scan<T, S, const N: usize>(
    station: &S,
    conn: &mut Connection<'_, T, N>,
) -> Result<(), Error<T::Error>>
where
    T: Read + Write,
    S: Station,
{
    station.scan().await;

    respond(conn, 200, "").await
}
