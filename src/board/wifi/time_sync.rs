use alloc::rc::Rc;
use embassy_net::{
    dns::DnsQueryType,
    udp::{PacketMetadata, UdpSocket},
    IpEndpoint, Stack,
};
use embassy_time::{with_timeout, Duration, Timer};
use esp_hal::rng::Rng;
use macros as cardio;
use network_services::sntp::{self, SntpError};

use crate::{
    board::{
        clock::Clock,
        wifi::power::{Awake, Power},
    },
    task_control::TaskControlToken,
};

const ATTEMPTS: usize = 3;
const REPLY_TIMEOUT: Duration = Duration::from_secs(2);
const RETRY_DELAY: Duration = Duration::from_secs(1);

/// What the station needs to set the clock.
pub struct TimeSync {
    pub clock: &'static Clock,
    pub server: heapless::String<64>,
}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
enum SyncError {
    Dns,
    NoAddress,
    Bind,
    Send,
    Timeout,
    Reply(SntpError),
}

/// Sets the clock once in each boot, the first time the station has an address.
#[cardio::task]
pub(super) async fn time_sync_task(
    stack: Stack<'static>,
    sync: TimeSync,
    power: Rc<Power>,
    mut task_control: TaskControlToken<()>,
) {
    task_control
        .run_cancellable(|_| async {
            loop {
                stack.wait_config_up().await;
                if sync.clock.synced() {
                    return;
                }

                for _ in 0..ATTEMPTS {
                    let awake = Awake::new(power.clone()).await;
                    let result = query(stack, &sync.server).await;
                    core::mem::drop(awake);

                    match result {
                        Ok(unix_us) => {
                            match sync.clock.now_us() {
                                Some(old) => info!(
                                    "Clock corrected by {} ms",
                                    (unix_us as i64 - old as i64) / 1000
                                ),
                                None => info!("Clock set"),
                            }
                            sync.clock.set_us(unix_us);
                            return;
                        }
                        Err(e) => warn!("Time sync failed: {:?}", e),
                    }

                    Timer::after(RETRY_DELAY).await;
                }

                stack.wait_config_down().await;
            }
        })
        .await;
}

async fn query(stack: Stack<'static>, server: &str) -> Result<u64, SyncError> {
    let addresses = stack
        .dns_query(server, DnsQueryType::A)
        .await
        .map_err(|_| SyncError::Dns)?;
    let address = *addresses.first().ok_or(SyncError::NoAddress)?;
    let server = IpEndpoint::new(address, sntp::PORT);

    let mut rx_meta = [PacketMetadata::EMPTY; 1];
    let mut rx_buffer = [0; sntp::PACKET_LEN];
    let mut tx_meta = [PacketMetadata::EMPTY; 1];
    let mut tx_buffer = [0; sntp::PACKET_LEN];
    let mut socket = UdpSocket::new(
        stack,
        &mut rx_meta,
        &mut rx_buffer,
        &mut tx_meta,
        &mut tx_buffer,
    );
    socket.bind(0).map_err(|_| SyncError::Bind)?;

    let rng = Rng::new();
    let nonce = (u64::from(rng.random()) << 32) | u64::from(rng.random());
    socket
        .send_to(&sntp::request(nonce), server)
        .await
        .map_err(|_| SyncError::Send)?;

    let reply = with_timeout(REPLY_TIMEOUT, async {
        loop {
            let reply = socket
                .recv_from_with(|data, meta| {
                    (meta.endpoint == server).then(|| sntp::parse(data, nonce))
                })
                .await;
            if let Some(reply) = reply {
                break reply;
            }
        }
    })
    .await
    .map_err(|_| SyncError::Timeout)?;

    reply.map_err(SyncError::Reply)
}
