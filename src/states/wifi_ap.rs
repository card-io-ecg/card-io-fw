use embassy_executor::Spawner;
use embassy_net::{
    tcp::TcpSocket, Config, IpListenEndpoint, Ipv4Address, Ipv4Cidr, Stack, StackResources,
    StaticConfig,
};
use embassy_time::{Duration, Ticker, Timer};
use embedded_graphics::Drawable;
use embedded_io::asynch::Write;
use embedded_svc::wifi::{AccessPointConfiguration, Configuration, Wifi};
use esp_wifi::wifi::{WifiController, WifiDevice, WifiEvent, WifiMode, WifiState};
use gui::screens::wifi_ap::{ApMenuEvents, WifiApScreen};

use crate::{
    board::{initialized::Board, LOW_BATTERY_VOLTAGE},
    states::MIN_FRAME_TIME,
    AppState,
};

unsafe fn as_static_ref<T>(what: &T) -> &'static T {
    core::mem::transmute(what)
}

unsafe fn as_static_mut<T>(what: &mut T) -> &'static mut T {
    core::mem::transmute(what)
}

pub async fn wifi_ap(board: &mut Board) -> AppState {
    let (wifi_interface, controller) = esp_wifi::wifi::new_with_mode(
        unsafe {
            as_static_mut(
                board
                    .wifi
                    .driver_mut(&board.clocks, &mut board.peripheral_clock_control),
            )
        },
        WifiMode::Ap,
    );

    let config = Config::Static(StaticConfig {
        address: Ipv4Cidr::new(Ipv4Address::new(192, 168, 2, 1), 24),
        gateway: Some(Ipv4Address::from_bytes(&[192, 168, 2, 1])),
        dns_servers: Default::default(),
    });
    let mut stack_resources = StackResources::<3>::new();
    let stack = Stack::new(
        wifi_interface,
        config,
        unsafe { as_static_mut(&mut stack_resources) },
        1234,
    );

    let spawner = Spawner::for_current_executor().await;

    spawner.spawn(connection_task(controller)).ok();
    spawner
        .spawn(net_task(unsafe { as_static_ref(&stack) }))
        .ok();
    spawner
        .spawn(webserver_task(unsafe { as_static_ref(&stack) }))
        .ok();

    let mut screen = WifiApScreen::new(
        board.battery_monitor.battery_data().await,
        board.config.battery_style(),
    );

    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    loop {
        let battery_data = board.battery_monitor.battery_data().await;

        if let Some(battery) = battery_data {
            if battery.voltage < LOW_BATTERY_VOLTAGE {
                // Enabling wifi modifies ADC readings and board shuts down
                // return AppState::Shutdown;
            }
        }

        screen.battery_data = battery_data;

        if let Some(event) = screen.menu.interact(board.frontend.is_touched()) {
            match event {
                ApMenuEvents::Exit => break,
            };
        }

        board
            .display
            .frame(|display| screen.draw(display))
            .await
            .unwrap();

        ticker.next().await;
    }

    AppState::MainMenu
}

#[embassy_executor::task]
async fn connection_task(mut controller: WifiController<'static>) {
    log::debug!("start connection task");
    log::debug!("Device capabilities: {:?}", controller.get_capabilities());
    loop {
        match esp_wifi::wifi::get_wifi_state() {
            WifiState::ApStart => {
                // wait until we're no longer connected
                controller.wait_for_event(WifiEvent::ApStop).await;
                Timer::after(Duration::from_millis(5000)).await
            }
            _ => {}
        }
        if !matches!(controller.is_started(), Ok(true)) {
            let client_config = Configuration::AccessPoint(AccessPointConfiguration {
                ssid: "Card/IO".into(),
                ..Default::default()
            });
            controller.set_configuration(&client_config).unwrap();
            log::debug!("Starting wifi");
            controller.start().await.unwrap();
            log::debug!("Wifi started!");
        }
    }
}

#[embassy_executor::task]
async fn net_task(stack: &'static Stack<WifiDevice<'static>>) {
    stack.run().await
}

#[embassy_executor::task]
async fn webserver_task(stack: &'static Stack<WifiDevice<'static>>) {
    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];

    loop {
        if stack.is_link_up() {
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    let mut socket = TcpSocket::new(&stack, &mut rx_buffer, &mut tx_buffer);
    socket.set_timeout(Some(embassy_net::SmolDuration::from_secs(10)));
    loop {
        log::info!("Wait for connection...");
        let r = socket
            .accept(IpListenEndpoint {
                addr: None,
                port: 8080,
            })
            .await;
        log::info!("Connected...");

        if let Err(e) = r {
            log::warn!("connect error: {:?}", e);
            continue;
        }

        let mut buffer = [0u8; 1024];
        let mut pos = 0;
        loop {
            match socket.read(&mut buffer).await {
                Ok(0) => {
                    log::info!("read EOF");
                    break;
                }
                Ok(len) => {
                    let to_print =
                        unsafe { core::str::from_utf8_unchecked(&buffer[..(pos + len)]) };

                    if to_print.contains("\r\n\r\n") {
                        log::debug!("Received: {}", to_print);
                        break;
                    }

                    pos += len;
                }
                Err(e) => {
                    log::warn!("read error: {:?}", e);
                    break;
                }
            };
        }

        let r = socket
            .write_all(
                b"HTTP/1.0 200 OK\r\n\r\n\
            <html>\
                <body>\
                    <h1>Hello Rust! Hello esp-wifi!</h1>\
                </body>\
            </html>\r\n\
            ",
            )
            .await;
        if let Err(e) = r {
            log::warn!("write error: {:?}", e);
        }

        if let Err(e) = socket.flush().await {
            log::warn!("flush error: {:?}", e);
        }

        socket.close();
    }
}
