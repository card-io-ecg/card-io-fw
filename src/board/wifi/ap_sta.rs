use core::{
    future::pending,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

use alloc::{boxed::Box, rc::Rc, vec::Vec};
use embassy_sync::{mutex::Mutex, signal::Signal};
use embassy_time::{Instant, Timer};

use crate::{
    board::wifi::{
        ap::{Ap, ApConnectionState, ApController},
        net_task,
        power::Power,
        sta::{CommandQueue, Sta, StaConnectionState, StaController},
        time_sync::{time_sync_task, TimeSync},
        AP_ADDRESS, CAPTIVE_URL,
    },
    task_control::{TaskControlToken, TaskController},
};
use edge_dhcp::{
    io::server::run as run_dhcp,
    server::{Server as DhcpServer, ServerOptions},
};
use edge_nal::UdpBind;
use edge_nal_embassy::{Udp, UdpBuffers};
use embassy_executor::Spawner;
use embassy_futures::{
    join::join4,
    select::{select4, Either4},
};
use embassy_net::{
    udp::{PacketMetadata, UdpSocket},
    Runner, Stack,
};
use esp_radio::wifi::{ap::AccessPointConfig, Interface, PowerSaveMode, WifiController};
use macros as cardio;
use network_services::dns;

pub(super) struct ApStaState {
    connection_task_control: TaskController<(), ApStaTaskResources>,
    ap_net_task_control: TaskController<()>,
    sta_net_task_control: TaskController<()>,
    dhcp_task_control: TaskController<()>,
    dns_task_control: TaskController<()>,
    time_sync_task_control: TaskController<()>,
    ap_handle: Ap,
    sta_handle: Sta,
}

impl ApStaState {
    pub(super) fn init(
        controller: WifiController<'static>,
        ap_stack: Stack<'static>,
        ap_runner: Runner<'static, Interface>,
        sta_stack: Stack<'static>,
        sta_runner: Runner<'static, Interface>,
        spawner: Spawner,
        time_sync: TimeSync,
    ) -> Self {
        info!("Configuring AP-STA");

        let ap_state = Rc::new(ApConnectionState::new());
        let sta_handle = Sta {
            sta_stack,
            networks: Rc::new(Mutex::new(heapless::Vec::new())),
            known_networks: Rc::new(Mutex::new(Vec::new())),
            state: Rc::new(StaConnectionState::new()),
            command_queue: Rc::new(CommandQueue::new()),
            networks_changed: Rc::new(Signal::new()),
            // Modem sleep takes the radio away from the access point.
            power: Rc::new(Power::new(PowerSaveMode::None)),
        };

        let connection_task_control =
            TaskController::from_resources(ApStaTaskResources { controller });
        let ap_net_task_control = TaskController::new();
        let sta_net_task_control = TaskController::new();
        let dhcp_task_control = TaskController::new();
        let dns_task_control = TaskController::new();
        let time_sync_task_control = TaskController::new();

        let ap_config = AccessPointConfig::default()
            .with_ssid(unwrap!("Card/IO".try_into()))
            .with_max_connections(1);

        info!("Starting AP-STA tasks");
        spawner.spawn(unwrap!(ap_sta_task(
            StaController::new(&sta_handle, Some(ap_config)),
            ApController::new(ap_state.clone()),
            connection_task_control.token(),
        )));

        spawner.spawn(unwrap!(net_task(ap_runner, ap_net_task_control.token())));
        spawner.spawn(unwrap!(net_task(sta_runner, sta_net_task_control.token())));
        spawner.spawn(unwrap!(dhcp_task(ap_stack, dhcp_task_control.token())));
        spawner.spawn(unwrap!(dns_task(ap_stack, dns_task_control.token())));
        spawner.spawn(unwrap!(time_sync_task(
            sta_stack,
            time_sync,
            sta_handle.power.clone(),
            time_sync_task_control.token(),
        )));

        Self {
            connection_task_control,
            ap_net_task_control,
            sta_net_task_control,
            dhcp_task_control,
            dns_task_control,
            time_sync_task_control,

            ap_handle: Ap {
                ap_stack,
                state: ap_state,
            },
            sta_handle,
        }
    }

    pub(super) async fn stop(self) {
        info!("Stopping AP-STA");
        let _ = self.time_sync_task_control.stop().await;
        let _ = self.dns_task_control.stop().await;
        let _ = join4(
            self.connection_task_control.stop(),
            self.ap_net_task_control.stop(),
            self.sta_net_task_control.stop(),
            self.dhcp_task_control.stop(),
        )
        .await;

        info!("Stopped AP-STA");
    }

    pub(crate) fn handles(&self) -> (&Ap, &Sta) {
        (&self.ap_handle, &self.sta_handle)
    }
}

struct ApStaTaskResources {
    controller: WifiController<'static>,
}
unsafe impl Send for ApStaTaskResources {}

const DHCP_SERVER_PORT: u16 = 67;
const DHCP_LEASES: usize = 8;
const DHCP_PACKET_LEN: usize = 1500;

struct DhcpResources {
    buffers: UdpBuffers<1>,
    packet: [u8; DHCP_PACKET_LEN],
}

// One spare slot: see `net_task`.
#[cardio::task(pool_size = 2)]
async fn dhcp_task(stack: Stack<'static>, mut task_control: TaskControlToken<()>) {
    info!("Started DHCP task");
    task_control
        .run_cancellable(|_| async {
            let mut resources = Box::new(DhcpResources {
                buffers: UdpBuffers::new(),
                packet: [0; DHCP_PACKET_LEN],
            });

            let udp = Udp::new(stack, &resources.buffers);
            let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), DHCP_SERVER_PORT);
            let mut socket = match udp.bind(address).await {
                Ok(socket) => socket,
                Err(e) => {
                    warn!("Failed to bind DHCP socket: {:?}", e);
                    return;
                }
            };

            let mut server =
                DhcpServer::<_, DHCP_LEASES>::new(|| Instant::now().as_secs(), AP_ADDRESS);
            let mut gateway = [AP_ADDRESS];
            let dns = [AP_ADDRESS];
            let mut options = ServerOptions::new(AP_ADDRESS, Some(&mut gateway));
            // The access point answers every name, and a phone opens the setup page as the
            // captive portal.
            options.dns = &dns;
            options.captive_url = Some(CAPTIVE_URL);

            if let Err(e) =
                run_dhcp(&mut server, &options, &mut socket, &mut resources.packet).await
            {
                warn!("DHCP server error: {:?}", e);
            }
        })
        .await;
    info!("Stopped DHCP task");
}

const DNS_PORT: u16 = 53;
/// The largest DNS message over UDP without EDNS.
const DNS_MESSAGE_LEN: usize = 512;

// One spare slot: see `net_task`.
#[cardio::task(pool_size = 2)]
async fn dns_task(stack: Stack<'static>, mut task_control: TaskControlToken<()>) {
    task_control
        .run_cancellable(|_| async {
            let mut rx_meta = [PacketMetadata::EMPTY; 2];
            let mut rx_buffer = [0; 2 * DNS_MESSAGE_LEN];
            let mut tx_meta = [PacketMetadata::EMPTY; 2];
            let mut tx_buffer = [0; 2 * DNS_MESSAGE_LEN];
            let mut socket = UdpSocket::new(
                stack,
                &mut rx_meta,
                &mut rx_buffer,
                &mut tx_meta,
                &mut tx_buffer,
            );
            if let Err(e) = socket.bind(DNS_PORT) {
                warn!("Failed to bind the DNS socket: {:?}", e);
                return;
            }

            let mut query = [0; DNS_MESSAGE_LEN];
            let mut reply = [0; DNS_MESSAGE_LEN];
            loop {
                let Ok((len, meta)) = socket.recv_from(&mut query).await else {
                    continue;
                };
                let Some(reply_len) = dns::answer(&query[..len], AP_ADDRESS, &mut reply) else {
                    debug!("Ignored a DNS message");
                    continue;
                };
                debug!("Answered a DNS query from {}", meta.endpoint);
                if let Err(e) = socket.send_to(&reply[..reply_len], meta.endpoint).await {
                    warn!("Failed to send a DNS answer: {:?}", e);
                }
            }
        })
        .await;
}

// One spare slot: see `net_task`.
#[cardio::task(pool_size = 2)]
async fn ap_sta_task(
    mut sta_controller: StaController,
    mut ap_controller: ApController,
    mut task_control: TaskControlToken<(), ApStaTaskResources>,
) {
    task_control
        .run_cancellable(|resources| async {
            sta_controller.setup(&mut resources.controller).await;

            loop {
                let next_step = sta_controller.update(&mut resources.controller).await;

                let poll_result = select4(
                    async {
                        if sta_controller.controller_state.is_connected() {
                            _ = resources.controller.wait_for_disconnect_async().await;
                            true
                        } else if let Some(next_step) = next_step {
                            Timer::at(next_step).await;
                            false
                        } else {
                            pending().await
                        }
                    },
                    resources
                        .controller
                        .wait_for_access_point_connected_event_async(),
                    sta_controller.wait_for_command(),
                    sta_controller.wait_for_networks_changed(),
                )
                .await;

                match poll_result {
                    Either4::First(disconnected) if disconnected => {
                        sta_controller.on_disconnected();
                    }
                    Either4::Second(Ok(ap_event)) => {
                        ap_controller.handle_event(ap_event);
                    }
                    Either4::Third(command) => {
                        sta_controller
                            .handle_command(command, &mut resources.controller)
                            .await;
                    }
                    Either4::Fourth(()) => sta_controller.on_networks_changed(),

                    _ => {}
                }
            }
        })
        .await;
}
