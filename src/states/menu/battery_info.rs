use crate::{
    board::initialized::Board,
    states::menu::{AppMenu, AppMenuBuilder, MenuScreen},
    AppState,
};
use alloc::format;
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

        let voltage = unwrap!(sensor.fg.read_vcell().await.ok());
        list_item(format!("Voltage {:>10}mV", voltage / 1000));

        let current = unwrap!(sensor.fg.read_current().await.ok());
        list_item(format!("Current {:>10}mA", current / 1000));

        let capacity = unwrap!(sensor.fg.read_design_capacity().await.ok());
        list_item(format!("Nominal {:>9}mAh", capacity / 1000));

        let capacity = unwrap!(sensor.fg.read_reported_capacity().await.ok());
        list_item(format!("Capacity {:>8}mAh", capacity / 1000));

        let age = unwrap!(sensor.fg.read_cell_age().await.ok());
        list_item(format!("Cell age {:>10}%", age));

        let charge_cycles = unwrap!(sensor.fg.read_charge_cycles().await.ok());
        list_item(format!("Charge cycles {:>6}", charge_cycles));

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
