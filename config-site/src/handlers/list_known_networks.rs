use edge_http::io::{server::Connection, Error};
use embedded_io_async::{Read, Write};

use crate::{data::SharedWebContext, handlers::start_text_response};

pub async fn handle<T, const N: usize>(
    context: &SharedWebContext,
    conn: &mut Connection<'_, T, N>,
) -> Result<(), Error<T::Error>>
where
    T: Read + Write,
{
    start_text_response(conn).await?;

    let context = context.lock().await;
    for network in context.known_networks.iter() {
        conn.write_all(network.ssid.as_bytes()).await?;
        conn.write_all(b"\n").await?;
    }

    Ok(())
}
