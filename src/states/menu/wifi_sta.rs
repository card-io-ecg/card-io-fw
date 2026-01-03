use core::cell::Cell;

use alloc::{string::String, vec};
use embassy_futures::select::{select, Either};
use embassy_time::{Duration, Instant, Ticker, Timer};
use embedded_graphics::Drawable;
use embedded_menu::items::menu_item::MenuItem;
use gui::screens::create_menu;

use crate::{
    board::{initialized::Context, wifi::sta::StaCommand},
    states::{TouchInputShaper, MIN_FRAME_TIME},
    timeout::Timeout,
    AppMenu, AppState,
};

#[derive(Clone, Copy)]
pub enum WifiStaMenuEvents {
    None,
    Back,
}

pub async fn wifi_sta(context: &mut Context) -> AppState {
    let Some(sta) = context.enable_wifi_sta_for_scan().await else {
        // FIXME: Show error screen
        return AppState::Menu(AppMenu::Main);
    };

    const MENU_IDLE_DURATION: Duration = Duration::from_secs(30);
    const SCAN_IDLE_DURATION: Duration = Duration::from_secs(5);
    const TOUCH_REFRESH_DEBOUNCE: Duration = Duration::from_secs(1);

    let scan_done = Cell::new(false);

    let ui = async {
        let mut ticker = Ticker::every(MIN_FRAME_TIME);
        let mut menu_state = Default::default();

        let list_item = |label: &str| {
            MenuItem::new(String::from(label), "").with_value_converter(|_| WifiStaMenuEvents::None)
        };

        // Initial placeholder
        let mut ssids = vec![list_item("Scanning...")];

        let mut exit_timer = Timeout::new(MENU_IDLE_DURATION);
        let mut input = TouchInputShaper::new();
        let mut last_touched = Instant::now();
        loop {
            if exit_timer.is_elapsed() {
                break AppState::Shutdown;
            }
            input.update(&mut context.frontend);
            let is_touched = input.is_touched();

            if is_touched {
                last_touched = Instant::now();
                exit_timer.reset();
            }

            if last_touched.elapsed() > TOUCH_REFRESH_DEBOUNCE && scan_done.take() {
                let networks = sta.visible_networks().await;

                if !networks.is_empty() {
                    ssids.clear();
                    ssids.extend(networks.iter().map(|n| list_item(&n.ssid)));
                }
            }

            if context.battery_monitor.is_low() {
                break AppState::Shutdown;
            }

            let mut menu_screen = create_menu("Access points")
                .add_menu_items(&mut ssids)
                .add_item("Back", "<-", |_| WifiStaMenuEvents::Back)
                .build_with_state(menu_state);

            if let Some(WifiStaMenuEvents::Back) = menu_screen.interact(is_touched) {
                break AppState::Menu(AppMenu::Main);
            }

            context
                .with_status_bar(|display| {
                    menu_screen.update(display);
                    menu_screen.draw(display)
                })
                .await;

            menu_state = menu_screen.state();

            ticker.next().await;
        }
    };

    let scan = async {
        loop {
            sta.send_command(StaCommand::ScanOnce).await;
            scan_done.set(true);
            Timer::after(SCAN_IDLE_DURATION).await;
        }
    };

    match select(ui, scan).await {
        Either::First(state) => state,
        Either::Second(_) => unreachable!(),
    }
}
