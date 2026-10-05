use core::str::FromStr;

use edge_http::io::{server::Connection, Error};
use embedded_io_async::{Read, Write};

use crate::{
    data::{network::WifiNetwork, SharedWebContext},
    handlers::{read_body, respond, MAX_BODY_SIZE},
};

pub async fn handle<T, const N: usize>(
    context: &SharedWebContext,
    conn: &mut Connection<'_, T, N>,
) -> Result<(), Error<T::Error>>
where
    T: Read + Write,
{
    let mut buf = [0; MAX_BODY_SIZE];
    let Some(post_body) = read_body(conn, &mut buf).await? else {
        return Ok(());
    };

    let (ssid, pass) = post_body.split_once('\n').unwrap_or((post_body, ""));

    if ssid.is_empty() {
        return respond(conn, 400, "SSID is empty").await;
    }

    let Ok(ssid) = heapless::String::<32>::from_str(ssid.trim()) else {
        return respond(conn, 400, "SSID too long").await;
    };

    let Ok(pass) = heapless::String::<64>::from_str(pass.trim()) else {
        return respond(conn, 400, "Password too long").await;
    };

    let result = {
        // Scope-limit the lock guard
        let mut context = context.lock().await;
        context.known_networks.push(WifiNetwork { ssid, pass })
    };

    if result.is_err() {
        return respond(conn, 400, "Too many networks").await;
    }

    respond(conn, 200, "").await
}
