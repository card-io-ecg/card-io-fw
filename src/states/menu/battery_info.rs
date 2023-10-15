use crate::{
    board::initialized::Board,
    human_readable::LeftPad,
    states::menu::{AppMenu, AppMenuBuilder, MenuScreen},
    uformat, AppState,
};
use embassy_time::Duration;
use embedded_menu::items::NavigationItem;
use gui::screens::create_menu;

#[derive(Clone, Copy)]
pub enum BatteryEvents {
    None,
    Back,
}

pub async fn battery_info_menu(board: &mut Board) -> AppState {
    BatteryInfoMenu
        .display(board)
        .await
        .unwrap_or(AppState::Shutdown)
}

struct BatteryInfoMenu;
impl MenuScreen for BatteryInfoMenu {
    type Event = BatteryEvents;
    type Result = AppState;

    const REFRESH_PERIOD: Option<Duration> = Some(Duration::from_secs(1));

    async fn menu(&mut self, board: &mut Board) -> impl AppMenuBuilder<Self::Event> {
        let mut items = heapless::Vec::<_, 6>::new();

        let mut list_item = |label| {
            unwrap!(items
                .push(NavigationItem::new(label, BatteryEvents::None))
                .ok())
        };

        let mut sensor = board.battery_monitor.sensor().await;

        if let Ok(voltage) = sensor.fg.read_vcell().await {
            list_item(uformat!(
                20,
                "Voltage {}mV",
                LeftPad(10, voltage as i32 / 1000)
            ));
        }

        if let Ok(current) = sensor.fg.read_current().await {
            list_item(uformat!(
                20,
                "Current {}mA",
                LeftPad(10, current as i32 / 1000)
            ));
        }

        if let Ok(cap) = sensor.fg.read_design_capacity().await {
            list_item(uformat!(20, "Nominal {}mAh", LeftPad(9, cap as i32 / 1000)));
        }

        if let Ok(cap) = sensor.fg.read_reported_capacity().await {
            list_item(uformat!(
                20,
                "Capacity {}mAh",
                LeftPad(8, cap as i32 / 1000)
            ));
        }

        if let Ok(age) = sensor.fg.read_cell_age().await {
            list_item(uformat!(20, "Cell age {}%", LeftPad(10, age as i32)));
        }

        if let Ok(cycles) = sensor.fg.read_charge_cycles().await {
            list_item(uformat!(20, "Charge cycles {}", LeftPad(6, cycles as i32)));
        }

        create_menu("Battery info")
            .add_items(items)
            .add_item(NavigationItem::new("Back", BatteryEvents::Back))
    }

    async fn handle_event(
        &mut self,
        event: Self::Event,
        _board: &mut Board,
    ) -> Option<Self::Result> {
        match event {
            BatteryEvents::None => None,
            BatteryEvents::Back => Some(AppState::Menu(AppMenu::DeviceInfo)),
        }
    }
}
