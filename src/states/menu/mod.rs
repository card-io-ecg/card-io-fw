pub mod about;
pub mod display;
pub mod main;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AppMenu {
    Main,
    Display,
    About,
}
