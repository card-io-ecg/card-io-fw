use crate::{
    board::initialized::Board,
    states::{menu::AppMenu, TouchInputShaper, MENU_IDLE_DURATION, MIN_FRAME_TIME},
    timeout::Timeout,
    AppState,
};
use alloc::format;
use embassy_time::{Duration, Ticker};
use embedded_graphics::Drawable;
use embedded_menu::items::NavigationItem;
use gui::screens::{create_menu, screen::Screen};

#[derive(Clone, Copy)]
pub enum BatteryEvents {
    None,
    Back,
}

pub async fn battery_info_menu(board: &mut Board) -> AppState {
    let mut exit_timer = Timeout::new(MENU_IDLE_DURATION);
    let mut menu_state = Default::default();

    let list_item = |label| NavigationItem::new(label, BatteryEvents::None);

    let mut items = heapless::Vec::<_, 6>::new();

    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    let mut input = TouchInputShaper::new();

    let mut load_sensor_data = Timeout::new(Duration::from_secs(1));
    let mut first = true;

    while !exit_timer.is_elapsed() {
        input.update(&mut board.frontend);
        let is_touched = input.is_touched();
        if is_touched {
            exit_timer.reset();
        }

        if first || load_sensor_data.is_elapsed() {
            load_sensor_data.reset();
            first = false;

            items.clear();

            let mut sensor = board.battery_monitor.sensor().await;

            let voltage = unwrap!(sensor.fg.read_vcell().await.ok());
            unwrap!(items
                .push(list_item(format!("Voltage {:>10}mV", voltage / 1000)))
                .ok());

            let current = unwrap!(sensor.fg.read_current().await.ok());
            unwrap!(items
                .push(list_item(format!("Current {:>10}mA", current / 1000)))
                .ok());

            let capacity = unwrap!(sensor.fg.read_design_capacity().await.ok());
            unwrap!(items
                .push(list_item(format!("Nominal {:>9}mAh", capacity / 1000)))
                .ok());

            let capacity = unwrap!(sensor.fg.read_reported_capacity().await.ok());
            unwrap!(items
                .push(list_item(format!("Capacity {:>8}mAh", capacity / 1000)))
                .ok());

            let age = unwrap!(sensor.fg.read_cell_age().await.ok());
            unwrap!(items.push(list_item(format!("Cell age {:>10}%", age))).ok());

            let charge_cycles = unwrap!(sensor.fg.read_charge_cycles().await.ok());
            unwrap!(items
                .push(list_item(format!("Chg Cycles {:>9}", charge_cycles)))
                .ok());
        }

        let mut menu_screen = Screen {
            content: create_menu("Battery info")
                .add_items(&mut items[..])
                .add_item(NavigationItem::new("Back", BatteryEvents::Back))
                .build_with_state(menu_state),

            status_bar: board.status_bar(),
        };

        if let Some(event) = menu_screen.content.interact(is_touched) {
            match event {
                BatteryEvents::None => {}
                BatteryEvents::Back => return AppState::Menu(AppMenu::DeviceInfo),
            };
        }

        if board.battery_monitor.is_low() {
            return AppState::Shutdown;
        }

        board
            .display
            .frame(|display| {
                menu_screen.content.update(display);
                menu_screen.draw(display)
            })
            .await;

        menu_state = menu_screen.content.state();

        ticker.next().await;
    }

    AppState::Menu(AppMenu::Main)
}
