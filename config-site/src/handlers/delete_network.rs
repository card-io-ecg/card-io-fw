use core::str::FromStr;

use edge_http::io::{server::Connection, Error};
use embedded_io_async::{Read, Write};

use crate::{
    data::SharedWebContext,
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

    let index = match usize::from_str(post_body) {
        Ok(index) => index,
        Err(_err) => {
            warn!("Invalid index in POST body: {:?}", post_body);
            return respond(conn, 400, "Network index is not a valid number").await;
        }
    };

    {
        // Scope-limit the lock guard
        let mut context = context.lock().await;
        if index < context.known_networks.len() {
            context.known_networks.swap_remove(index);
        }
    };

    respond(conn, 200, "").await
}
