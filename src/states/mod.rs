mod adc_setup;
mod charging;
mod error;
mod init;
mod measure;
mod menu;

use embassy_time::Duration;

pub use adc_setup::adc_setup;
pub use charging::charging;
pub use error::app_error;
pub use init::initialize;
pub use measure::measure;
pub use menu::{
    about::about_menu, display::display_menu, main::main_menu, wifi_ap::wifi_ap, wifi_sta::wifi_sta,
};

const TARGET_FPS: u32 = 100;
const MIN_FRAME_TIME: Duration = Duration::from_hz(TARGET_FPS as u64);

const MENU_IDLE_DURATION: Duration = Duration::from_secs(30);

// The max number of webserver tasks.
const WEBSERVER_TASKS: usize = 2;

pub use menu::AppMenu;
