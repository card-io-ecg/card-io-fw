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

    if !validate_url(post_body) {
        return respond(conn, 400, "Input is not a valid URL").await;
    }

    let result = {
        // Scope-limit the lock guard
        let mut context = context.lock().await;
        context.backend_url.clear();
        context.backend_url.push_str(post_body)
    };

    if result.is_err() {
        return respond(conn, 400, "URL is too long").await;
    }

    respond(conn, 200, "").await
}

fn validate_url(url: &str) -> bool {
    if url.is_empty() {
        return true;
    }

    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return false;
    }

    const VALID_CHARS: &[u8] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~:/?#[]@!$&'()*+,;=";

    if url.bytes().any(|b| !VALID_CHARS.contains(&b)) {
        return false;
    }

    true
}
