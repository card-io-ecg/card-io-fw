use alloc::{boxed::Box, format, string::String};
use core::alloc::AllocError;

use embassy_net::{dns::DnsQueryType, tcp::TcpSocket, IpAddress, Stack};
use embassy_time::Duration;
use embedded_io_async::{Error as _, ErrorKind, ErrorType, Read, Write};
use embedded_tls::{Aes128GcmSha256, TlsConfig, TlsConnection, TlsContext, UnsecureProvider};
use rand_core::CryptoRngCore;

use crate::{
    http::{self, Request, Response},
    url::{BaseUrl, Scheme},
};

const INACTIVITY_TIMEOUT: Duration = Duration::from_secs(10);
// An upload holds a measurement of up to 90 KB next to these buffers, so the TCP windows stay
// small; download speed pays for it.
const TCP_TX_LEN: usize = 4096;
const TCP_RX_LEN: usize = 4096;
// A TLS 1.3 record holds 16,384 bytes plus 256 of overhead, and some servers ignore the max
// fragment length extension, so the read buffer takes a full record.
const TLS_READ_LEN: usize = 16_640;
const TLS_WRITE_LEN: usize = 4096;
const HEAD_LEN: usize = 2048;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ClientError {
    Dns,
    Connect,
    Tls,
    Http,
}

pub struct Client {
    tcp_tx: Box<[u8; TCP_TX_LEN]>,
    tcp_rx: Box<[u8; TCP_RX_LEN]>,
    tls_read: Box<[u8; TLS_READ_LEN]>,
    tls_write: Box<[u8; TLS_WRITE_LEN]>,
    head: Box<[u8; HEAD_LEN]>,
}

impl Client {
    pub fn new() -> Result<Self, AllocError> {
        Ok(Self {
            tcp_tx: Box::try_new([0; TCP_TX_LEN])?,
            tcp_rx: Box::try_new([0; TCP_RX_LEN])?,
            tls_read: Box::try_new([0; TLS_READ_LEN])?,
            tls_write: Box::try_new([0; TLS_WRITE_LEN])?,
            head: Box::try_new([0; HEAD_LEN])?,
        })
    }

    pub async fn connect<'c>(
        &'c mut self,
        stack: Stack<'c>,
        rng: impl CryptoRngCore,
        base: &BaseUrl<'_>,
    ) -> Result<Connection<'c>, ClientError> {
        let address = resolve(stack, base.host).await?;

        let mut socket = TcpSocket::new(stack, &mut *self.tcp_rx, &mut *self.tcp_tx);
        socket.set_timeout(Some(INACTIVITY_TIMEOUT));
        socket
            .connect((address, base.port()))
            .await
            .map_err(|error| {
                warn!("connect: {}", error);
                ClientError::Connect
            })?;

        let stream = match base.scheme {
            Scheme::Http => Stream::Plain(socket),
            Scheme::Https => {
                let mut tls = TlsConnection::new(socket, &mut *self.tls_read, &mut *self.tls_write);
                let config = match base.ipv4() {
                    Some(_) => TlsConfig::new(),
                    None => TlsConfig::new().with_server_name(base.host),
                };
                let provider = UnsecureProvider::new::<Aes128GcmSha256>(rng);
                tls.open(TlsContext::new(&config, provider))
                    .await
                    .map_err(|error| {
                        warn!("TLS: {}", error);
                        ClientError::Tls
                    })?;
                Stream::Tls(tls)
            }
        };

        Ok(Connection {
            stream,
            host: match base.explicit_port {
                Some(port) => format!("{}:{port}", base.host),
                None => base.host.into(),
            },
            head: &mut *self.head,
        })
    }
}

async fn resolve(stack: Stack<'_>, host: &str) -> Result<IpAddress, ClientError> {
    let addresses = stack
        .dns_query(host, DnsQueryType::A)
        .await
        .map_err(|error| {
            warn!("DNS for {=str}: {}", host, error);
            ClientError::Dns
        })?;
    match addresses.first() {
        Some(address) => Ok(*address),
        None => {
            warn!("DNS for {=str}: no A record", host);
            Err(ClientError::Dns)
        }
    }
}

pub struct Connection<'c> {
    stream: Stream<'c>,
    /// The `Host` header: the host as written, with the port only when the base URL names one.
    host: String,
    head: &'c mut [u8],
}

impl<'c> Connection<'c> {
    // `use<'c, 's>`: the stream is behind `&mut`, so `'c` is invariant and the reader must name it.
    pub async fn send<'s>(
        &'s mut self,
        request: &Request<'_>,
    ) -> Result<Response<impl Read + use<'c, 's>>, ClientError> {
        http::send(&mut self.stream, &self.host, request, &mut *self.head)
            .await
            .map_err(|error| {
                warn!("HTTP: {}", error);
                ClientError::Http
            })
    }
}

// Unboxed on purpose: `https` is the route in use, and a box would add an allocation that can fail.
#[allow(clippy::large_enum_variant)]
enum Stream<'c> {
    Plain(TcpSocket<'c>),
    Tls(TlsConnection<'c, TcpSocket<'c>, Aes128GcmSha256>),
}

impl ErrorType for Stream<'_> {
    type Error = ErrorKind;
}

impl Read for Stream<'_> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ErrorKind> {
        match self {
            Stream::Plain(socket) => socket.read(buf).await.map_err(|error| error.kind()),
            Stream::Tls(tls) => tls.read(buf).await.map_err(|error| error.kind()),
        }
    }
}

impl Write for Stream<'_> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, ErrorKind> {
        match self {
            Stream::Plain(socket) => socket.write(buf).await.map_err(|error| error.kind()),
            Stream::Tls(tls) => tls.write(buf).await.map_err(|error| error.kind()),
        }
    }

    async fn flush(&mut self) -> Result<(), ErrorKind> {
        match self {
            Stream::Plain(socket) => socket.flush().await.map_err(|error| error.kind()),
            Stream::Tls(tls) => tls.flush().await.map_err(|error| error.kind()),
        }
    }
}
