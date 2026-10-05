use edge_http::io::{server::Connection, Error};
use embedded_io_async::{Read, Write};
use network_services::pairing::Refusal;

use crate::handlers::{read_body, respond, MAX_BODY_SIZE};

// The web server and the example run each handler on one thread, so the futures need no `Send`.
#[allow(async_fn_in_trait)]
pub trait PairingControl {
    async fn status_line(&self, out: &mut impl core::fmt::Write);
    async fn pair(&self, typed: &str) -> Result<(), Refusal>;
    async fn unpair(&self) -> Result<(), Refusal>;
}

pub async fn status<T, P, const N: usize>(
    pairing: &P,
    conn: &mut Connection<'_, T, N>,
) -> Result<(), Error<T::Error>>
where
    T: Read + Write,
    P: PairingControl,
{
    let mut line = heapless::String::<40>::new();
    pairing.status_line(&mut line).await;

    respond(conn, 200, &line).await
}

pub async fn pair<T, P, const N: usize>(
    pairing: &P,
    conn: &mut Connection<'_, T, N>,
) -> Result<(), Error<T::Error>>
where
    T: Read + Write,
    P: PairingControl,
{
    let mut buf = [0; MAX_BODY_SIZE];
    let Some(typed) = read_body(conn, &mut buf).await? else {
        return Ok(());
    };

    answer(conn, pairing.pair(typed).await).await
}

pub async fn unpair<T, P, const N: usize>(
    pairing: &P,
    conn: &mut Connection<'_, T, N>,
) -> Result<(), Error<T::Error>>
where
    T: Read + Write,
    P: PairingControl,
{
    answer(conn, pairing.unpair().await).await
}

async fn answer<T, const N: usize>(
    conn: &mut Connection<'_, T, N>,
    result: Result<(), Refusal>,
) -> Result<(), Error<T::Error>>
where
    T: Read + Write,
{
    let (status, body) = match result {
        Ok(()) => (200, ""),
        Err(refusal) => (400, refusal.as_str()),
    };

    respond(conn, status, body).await
}
