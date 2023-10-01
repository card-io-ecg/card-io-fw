use embassy_time::Ticker;
use embedded_graphics::{pixelcolor::BinaryColor, prelude::DrawTarget, Drawable};
use embedded_menu::{
    builder::MenuBuilder,
    collection::MenuItemCollection,
    interaction::single_touch::{SingleTouch, SingleTouchAdapter},
    selection_indicator::{style::AnimatedTriangle, AnimatedPosition},
    Menu, MenuState,
};
use gui::{embedded_layout::view_group::ViewGroup, screens::screen::Screen};

use crate::{
    board::initialized::Board,
    states::{TouchInputShaper, MENU_IDLE_DURATION, MIN_FRAME_TIME},
    timeout::Timeout,
};

pub mod about;
#[cfg(feature = "battery_max17055")]
pub mod battery_info;
pub mod display;
pub mod main;
pub mod storage;
pub mod wifi_ap;
pub mod wifi_sta;

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum AppMenu {
    Main,
    Display,
    Storage,
    DeviceInfo,
    #[cfg(feature = "battery_max17055")]
    BatteryInfo,
    WifiAP,
    WifiListVisible,
}

pub trait AppMenuBuilder<E> {
    fn build(self) -> impl AppMenuT<E>;

    fn build_with_state(
        self,
        state: MenuState<SingleTouchAdapter<E>, AnimatedPosition, AnimatedTriangle>,
    ) -> impl AppMenuT<E>;
}

impl<T, VG, E> AppMenuBuilder<E>
    for MenuBuilder<T, SingleTouch, VG, E, BinaryColor, AnimatedPosition, AnimatedTriangle>
where
    T: AsRef<str>,
    VG: ViewGroup + MenuItemCollection<E>,
{
    fn build(self) -> impl AppMenuT<E> {
        MenuBuilder::build(self)
    }

    fn build_with_state(
        self,
        state: MenuState<SingleTouchAdapter<E>, AnimatedPosition, AnimatedTriangle>,
    ) -> impl AppMenuT<E> {
        MenuBuilder::build_with_state(self, state)
    }
}

pub trait AppMenuT<E>: Drawable<Color = BinaryColor, Output = ()> {
    fn interact(&mut self, touched: bool) -> Option<E>;
    fn update(&mut self, display: &impl DrawTarget<Color = BinaryColor>);
}

impl<T, VG, E> AppMenuT<E>
    for Menu<T, SingleTouch, VG, E, BinaryColor, AnimatedPosition, AnimatedTriangle>
where
    T: AsRef<str>,
    VG: ViewGroup + MenuItemCollection<E>,
{
    fn interact(&mut self, touched: bool) -> Option<E> {
        Menu::interact(self, touched)
    }

    fn update(&mut self, display: &impl DrawTarget<Color = BinaryColor>) {
        Menu::update(self, display)
    }
}

pub trait MenuScreen {
    type Event;
    type Result;

    async fn menu(&mut self, board: &mut Board) -> impl AppMenuBuilder<Self::Event>;

    async fn handle_event(&mut self, event: Self::Event, board: &mut Board)
        -> Option<Self::Result>;

    async fn display(&mut self, board: &mut Board) -> Option<Self::Result> {
        let mut screen = Screen {
            content: self.menu(board).await.build(),
            status_bar: board.status_bar(),
        };

        let mut exit_timer = Timeout::new(MENU_IDLE_DURATION);
        let mut ticker = Ticker::every(MIN_FRAME_TIME);
        let mut input = TouchInputShaper::new();

        while !exit_timer.is_elapsed() && !board.battery_monitor.is_low() {
            input.update(&mut board.frontend);

            let is_touched = input.is_touched();
            if is_touched {
                exit_timer.reset();
            }

            if let Some(event) = screen.content.interact(is_touched) {
                if let Some(result) = self.handle_event(event, board).await {
                    return Some(result);
                }
            }

            screen.status_bar = board.status_bar();

            board
                .display
                .frame(|display| {
                    screen.content.update(display);
                    screen.draw(display)
                })
                .await;

            ticker.next().await;
        }

        None
    }
}
