use embassy_net::{Config, Ipv4Address, Ipv4Cidr, Stack, StackResources, StaticConfig};
use embassy_time::Ticker;
use embedded_graphics::Drawable;
use esp_wifi::wifi::WifiMode;
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
    let (mut wifi_interface, controller) = esp_wifi::wifi::new_with_mode(
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
        unsafe { as_static_mut(&mut wifi_interface) },
        config,
        unsafe { as_static_mut(&mut stack_resources) },
        1234,
    );

    let mut screen = WifiApScreen::new(
        board.battery_monitor.battery_data().await,
        board.config.battery_style(),
    );

    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    loop {
        let battery_data = board.battery_monitor.battery_data().await;

        if let Some(battery) = battery_data {
            if battery.voltage < LOW_BATTERY_VOLTAGE {
                return AppState::Shutdown;
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
