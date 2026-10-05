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
        sta::{CommandQueue, Sta, StaConnectionState, StaController},
        AP_ADDRESS,
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
use embassy_net::{Runner, Stack};
use esp_radio::wifi::{ap::AccessPointConfig, Interface, WifiController};
use macros as cardio;

pub(super) struct ApStaState {
    connection_task_control: TaskController<(), ApStaTaskResources>,
    ap_net_task_control: TaskController<()>,
    sta_net_task_control: TaskController<()>,
    dhcp_task_control: TaskController<()>,
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
    ) -> Self {
        info!("Configuring AP-STA");

        let ap_state = Rc::new(ApConnectionState::new());
        let sta_state = Rc::new(StaConnectionState::new());
        let networks = Rc::new(Mutex::new(heapless::Vec::new()));
        let known_networks = Rc::new(Mutex::new(Vec::new()));
        let command_queue = Rc::new(CommandQueue::new());
        let networks_changed = Rc::new(Signal::new());

        let connection_task_control =
            TaskController::from_resources(ApStaTaskResources { controller });
        let ap_net_task_control = TaskController::new();
        let sta_net_task_control = TaskController::new();
        let dhcp_task_control = TaskController::new();

        let ap_config = AccessPointConfig::default()
            .with_ssid(unwrap!("Card/IO".try_into()))
            .with_max_connections(1);

        info!("Starting AP-STA tasks");
        spawner.spawn(unwrap!(ap_sta_task(
            StaController::new(
                sta_state.clone(),
                networks.clone(),
                known_networks.clone(),
                sta_stack.clone(),
                command_queue.clone(),
                networks_changed.clone(),
                Some(ap_config),
            ),
            ApController::new(ap_state.clone()),
            connection_task_control.token(),
        )));

        spawner.spawn(unwrap!(net_task(ap_runner, ap_net_task_control.token())));
        spawner.spawn(unwrap!(net_task(sta_runner, sta_net_task_control.token())));
        spawner.spawn(unwrap!(dhcp_task(ap_stack, dhcp_task_control.token())));

        Self {
            connection_task_control,
            ap_net_task_control,
            sta_net_task_control,
            dhcp_task_control,

            ap_handle: Ap {
                ap_stack,
                state: ap_state,
            },
            sta_handle: Sta {
                sta_stack,
                networks,
                known_networks,
                state: sta_state,
                command_queue,
                networks_changed,
            },
        }
    }

    pub(super) async fn stop(self) {
        info!("Stopping AP-STA");
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

#[cardio::task]
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
            let options = ServerOptions::new(AP_ADDRESS, Some(&mut gateway));

            if let Err(e) =
                run_dhcp(&mut server, &options, &mut socket, &mut resources.packet).await
            {
                warn!("DHCP server error: {:?}", e);
            }
        })
        .await;
    info!("Stopped DHCP task");
}

#[cardio::task]
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
