use embassy_time::{Duration, Ticker};
use embedded_graphics::{pixelcolor::BinaryColor, prelude::DrawTarget, Drawable};
use embedded_menu::{
    builder::MenuBuilder,
    collection::MenuItemCollection,
    interaction::single_touch::{SingleTouch, SingleTouchAdapter},
    selection_indicator::{style::AnimatedTriangle, AnimatedPosition},
    Menu, MenuState,
};
use gui::embedded_layout::view_group::ViewGroup;

use crate::{
    board::initialized::Context,
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
    type Menu: AppMenuT<E>;

    fn build(self) -> Self::Menu;

    fn build_with_state(
        self,
        state: MenuState<SingleTouchAdapter<E>, AnimatedPosition, AnimatedTriangle>,
    ) -> Self::Menu;
}

impl<T, VG, E> AppMenuBuilder<E>
    for MenuBuilder<T, SingleTouch, VG, E, BinaryColor, AnimatedPosition, AnimatedTriangle>
where
    T: AsRef<str>,
    VG: ViewGroup + MenuItemCollection<E>,
{
    type Menu = impl AppMenuT<E>;

    fn build(self) -> Self::Menu {
        MenuBuilder::build(self)
    }

    fn build_with_state(
        self,
        state: MenuState<SingleTouchAdapter<E>, AnimatedPosition, AnimatedTriangle>,
    ) -> Self::Menu {
        MenuBuilder::build_with_state(self, state)
    }
}

pub trait AppMenuT<E>: Drawable<Color = BinaryColor, Output = ()> {
    fn interact(&mut self, touched: bool) -> Option<E>;
    fn update(&mut self, display: &impl DrawTarget<Color = BinaryColor>);
    fn state(&self) -> MenuState<SingleTouchAdapter<E>, AnimatedPosition, AnimatedTriangle>;
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

    fn state(&self) -> MenuState<SingleTouchAdapter<E>, AnimatedPosition, AnimatedTriangle> {
        Menu::state(self)
    }
}

pub trait MenuScreen {
    type Event;
    type Result;

    const REFRESH_PERIOD: Option<Duration> = None;

    async fn menu(&mut self, context: &mut Context) -> impl AppMenuBuilder<Self::Event>;

    async fn handle_event(
        &mut self,
        event: Self::Event,
        context: &mut Context,
    ) -> Option<Self::Result>;

    async fn display(&mut self, context: &mut Context) -> Option<Self::Result> {
        let mut menu_screen = self.menu(context).await.build();

        let mut exit_timer = Timeout::new(MENU_IDLE_DURATION);
        let mut ticker = Ticker::every(MIN_FRAME_TIME);
        let mut input = TouchInputShaper::new();

        let mut refresh = Self::REFRESH_PERIOD.map(Timeout::new);

        while !exit_timer.is_elapsed() && !context.battery_monitor.is_low() {
            input.update(&mut context.frontend);

            let is_touched = input.is_touched();
            if is_touched {
                exit_timer.reset();
            }

            if let Some(refresh) = refresh.as_mut() {
                if refresh.is_elapsed() {
                    refresh.reset();

                    let state = menu_screen.state();
                    menu_screen = self.menu(context).await.build_with_state(state);
                }
            }

            if let Some(event) = menu_screen.interact(is_touched) {
                if let Some(result) = self.handle_event(event, context).await {
                    return Some(result);
                }
            }

            context
                .with_status_bar(|display| {
                    menu_screen.update(display);
                    menu_screen.draw(display)
                })
                .await;

            ticker.next().await;
        }

        info!("Menu timeout");
        None
    }
}
