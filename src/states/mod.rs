mod init;
mod measure;
mod menu;

use embassy_time::Duration;

pub use init::initialize;
pub use measure::measure;
pub use menu::main_menu;

const MIN_FRAME_TIME: Duration = Duration::from_millis(10);
