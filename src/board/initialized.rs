use core::ops::{Deref, DerefMut};

use crate::{
    board::{
        config::Config,
        drivers::battery_monitor::BatteryMonitor,
        hal::clock::Clocks,
        storage::FileSystem,
        wifi::{ap::Ap, sta::Sta, GenericConnectionState, WifiDriver},
        ChargerStatus, EcgFrontend, PoweredDisplay, VbusDetect,
    },
    saved_measurement_exists,
    states::MESSAGE_MIN_DURATION,
};
use display_interface::DisplayError;
use embassy_executor::SendSpawner;
use embassy_net::{Config as NetConfig, Ipv4Address, Ipv4Cidr, StaticConfigV4};
use embassy_time::{Duration, Instant, Timer};
use embedded_graphics::Drawable;
use gui::{
    screens::message::MessageScreen,
    widgets::{
        battery_small::Battery,
        status_bar::StatusBar,
        wifi::{WifiState, WifiStateView},
    },
};
use norfs::OnCollision;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StaMode {
    Enable,
    OnDemand,
}

pub struct Inner {
    pub clocks: Clocks<'static>,
    pub high_prio_spawner: SendSpawner,
    pub battery_monitor: BatteryMonitor<VbusDetect, ChargerStatus>,
    pub wifi: &'static mut WifiDriver,
    pub config: &'static mut Config,
    pub config_changed: bool,
    pub storage: Option<FileSystem>,
    pub sta_work_available: Option<bool>,
    pub message_displayed_at: Option<Instant>,
}

pub struct Board {
    pub display: PoweredDisplay,
    pub frontend: EcgFrontend,
    pub inner: Inner,
}

impl Deref for Board {
    type Target = Inner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for Board {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl Board {
    pub async fn display_message(&mut self, message: &str) {
        self.inner.wait_for_message(MESSAGE_MIN_DURATION).await;
        self.inner.message_displayed_at = Some(Instant::now());

        info!("Displaying message: {}", message);
        self.with_status_bar(|display| MessageScreen { message }.draw(display))
            .await;
    }

    pub async fn with_status_bar(
        &mut self,
        draw: impl FnOnce(&mut PoweredDisplay) -> Result<(), DisplayError>,
    ) {
        self.display
            .frame(|display| {
                let status_bar = self.inner.status_bar();
                status_bar.draw(display)?;
                draw(display)?;

                Ok(())
            })
            .await;
    }

    pub async fn apply_hw_config_changes(&mut self) {
        let _ = self
            .display
            .update_brightness_async(self.inner.config.display_brightness())
            .await;
    }
}

impl Inner {
    pub async fn with_status_bar(
        &mut self,
        display: &mut PoweredDisplay,
        draw: impl FnOnce(&mut PoweredDisplay) -> Result<(), DisplayError>,
    ) {
        display
            .frame(|display| {
                let status_bar = self.status_bar();
                status_bar.draw(display)?;
                draw(display)?;

                Ok(())
            })
            .await;
    }

    pub async fn wait_for_message(&mut self, duration: Duration) {
        if let Some(message_at) = self.message_displayed_at.take() {
            Timer::at(message_at + duration).await;
        }
    }

    pub async fn save_config(&mut self) {
        if !self.config_changed {
            return;
        }

        info!("Saving config");
        self.config_changed = false;

        if let Some(storage) = self.storage.as_mut() {
            if let Err(e) = storage
                .store_writer("config", self.config, OnCollision::Overwrite)
                .await
            {
                error!("Failed to save config: {:?}", e);
            }
        } else {
            warn!("Storage unavailable");
        }
    }

    async fn enable_sta(&mut self, can_enable: bool) -> Option<Sta> {
        if !can_enable {
            warn!("Not enabling STA");
            self.wifi.stop_if().await;
            return None;
        }

        let sta = self
            .wifi
            .configure_sta(NetConfig::dhcpv4(Default::default()), &self.clocks)
            .await;

        sta.update_known_networks(&self.config.known_networks).await;

        Some(sta)
    }

    pub async fn enable_wifi_sta(&mut self, mode: StaMode) -> Option<Sta> {
        debug!("Enabling STA");
        let can_enable = self.can_enable_wifi()
            && !self.config.known_networks.is_empty()
            && match mode {
                StaMode::Enable => true,
                StaMode::OnDemand => self.sta_has_work().await,
            };

        self.enable_sta(can_enable).await
    }

    pub async fn enable_wifi_sta_for_scan(&mut self) -> Option<Sta> {
        debug!("Enabling STA for scan");
        let can_enable = self.can_enable_wifi();

        self.enable_sta(can_enable).await
    }

    pub async fn enable_wifi_ap(&mut self) -> Option<Ap> {
        if !self.can_enable_wifi() {
            self.wifi.stop_if().await;
            return None;
        }

        let ap = self
            .wifi
            .configure_ap(
                NetConfig::ipv4_static(StaticConfigV4 {
                    address: Ipv4Cidr::new(Ipv4Address::new(192, 168, 2, 1), 24),
                    gateway: Some(Ipv4Address::from_bytes(&[192, 168, 2, 1])),
                    dns_servers: Default::default(),
                }),
                &self.clocks,
            )
            .await;

        Some(ap)
    }

    /// Note: make sure Sta/Ap is released before calling this.
    pub async fn disable_wifi(&mut self) {
        self.wifi.stop_if().await
    }

    pub fn can_enable_wifi(&mut self) -> bool {
        self.battery_monitor
            .battery_data()
            .map(|battery| battery.percentage > 50 || battery.is_charging())
            .unwrap_or(false)
    }

    pub async fn sta_has_work(&mut self) -> bool {
        // TODO: we can do a flag that is true on boot, so that entering the menu will always
        // connect and look for update, etc. We can also use a flag to see if we have ongoing
        // communication, so we can keep wifi on. Question is: when/how do we disable wifi if
        // it is in on-demand mode?

        if self.sta_work_available.is_none() {
            if let Some(storage) = self.storage.as_mut() {
                if saved_measurement_exists(storage).await {
                    self.sta_work_available = Some(true);
                }
            }
        }

        self.sta_work_available.unwrap_or(false)
    }

    pub fn signal_sta_work_available(&mut self, available: bool) {
        self.sta_work_available = Some(available);
    }

    pub fn update_config(&mut self, cb: impl FnOnce(&mut Config)) {
        struct ConfigWriter<'a> {
            config: &'a mut Config,
            changed: bool,
        }
        impl core::ops::Deref for ConfigWriter<'_> {
            type Target = Config;

            fn deref(&self) -> &Self::Target {
                &self.config
            }
        }

        impl core::ops::DerefMut for ConfigWriter<'_> {
            fn deref_mut(&mut self) -> &mut Self::Target {
                self.changed = true;
                &mut self.config
            }
        }

        let mut wrapper = ConfigWriter {
            config: self.config,
            changed: false,
        };

        cb(&mut wrapper);

        self.config_changed |= wrapper.changed;
    }

    pub fn status_bar(&mut self) -> StatusBar {
        let battery_data = self.battery_monitor.battery_data();
        let connection_state = match self.wifi.connection_state() {
            GenericConnectionState::Sta(state) => Some(WifiState::from(state)),
            GenericConnectionState::Ap(state) => Some(WifiState::from(state)),
            GenericConnectionState::Disabled => None,
        };

        StatusBar {
            battery: Battery::with_style(battery_data, self.config.battery_style()),
            wifi: WifiStateView::new(connection_state),
        }
    }
}
