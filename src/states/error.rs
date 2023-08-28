use crate::{board::initialized::Board, states::MIN_FRAME_TIME, AppError, AppState};
use embassy_time::Ticker;
use embedded_graphics::Drawable;
use gui::{
    screens::{message::MessageScreen, screen::Screen},
    widgets::{battery_small::Battery, status_bar::StatusBar, wifi::WifiStateView},
};

pub async fn app_error(board: &mut Board, error: AppError) -> AppState {
    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    while board.frontend.is_touched() {
        let battery_data = board.battery_monitor.battery_data();

        if let Some(battery) = battery_data {
            if battery.is_low {
                return AppState::Shutdown;
            }
        }

        board
            .display
            .frame(|display| {
                Screen {
                    content: MessageScreen {
                        message: match error {
                            AppError::Adc => "ADC is not working",
                        },
                    },

                    status_bar: StatusBar {
                        battery: Battery::with_style(
                            board.battery_monitor.battery_data(),
                            board.config.battery_style(),
                        ),
                        wifi: WifiStateView::disabled(),
                    },
                }
                .draw(display)
            })
            .await
            .unwrap();

        ticker.next().await;
    }

    AppState::Shutdown
}
