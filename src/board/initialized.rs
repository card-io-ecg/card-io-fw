use crate::{
    board::{
        config::Config,
        drivers::battery_monitor::BatteryMonitor,
        hal::{clock::Clocks, system::PeripheralClockControl},
        storage::FileSystem,
        wifi::{ap::Ap, sta::Sta, WifiDriver},
        ChargerStatus, EcgFrontend, PoweredDisplay, VbusDetect,
    },
    saved_measurement_exists,
};
use embassy_executor::SendSpawner;
use embassy_net::{Config as NetConfig, Ipv4Address, Ipv4Cidr, StaticConfigV4};
use norfs::OnCollision;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StaMode {
    Enable,
    OnDemand,
}

pub struct Board {
    pub display: PoweredDisplay,
    pub frontend: EcgFrontend,
    pub clocks: Clocks<'static>,
    pub peripheral_clock_control: PeripheralClockControl,
    pub high_prio_spawner: SendSpawner,
    pub battery_monitor: BatteryMonitor<VbusDetect, ChargerStatus>,
    pub wifi: &'static mut WifiDriver,
    pub config: &'static mut Config,
    pub config_changed: bool,
    pub storage: Option<FileSystem>,
    pub sta_work_available: Option<bool>,
}

impl Board {
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

    pub async fn enable_wifi_sta(&mut self, mode: StaMode) -> Option<Sta> {
        let can_enable = self.can_enable_wifi()
            && !self.config.known_networks.is_empty()
            && match mode {
                StaMode::Enable => true,
                StaMode::OnDemand => self.sta_has_work().await,
            };

        if !can_enable {
            self.wifi.stop_if().await;
            return None;
        }

        // Let's assume that when we disable STA, we don't have any work to do. This might be wrong,
        // especially when we disconnect from an AP, but in that case it's fine to just not try
        // again.
        self.sta_work_available = Some(false);

        self.wifi.initialize(&self.clocks);

        let sta = self
            .wifi
            .configure_sta(NetConfig::dhcpv4(Default::default()))
            .await;

        sta.update_known_networks(&self.config.known_networks).await;

        Some(sta)
    }

    pub async fn enable_wifi_ap(&mut self) -> Option<Ap> {
        if !self.can_enable_wifi() {
            self.wifi.stop_if().await;
            return None;
        }

        self.wifi.initialize(&self.clocks);

        let ap = self
            .wifi
            .configure_ap(NetConfig::ipv4_static(StaticConfigV4 {
                address: Ipv4Cidr::new(Ipv4Address::new(192, 168, 2, 1), 24),
                gateway: Some(Ipv4Address::from_bytes(&[192, 168, 2, 1])),
                dns_servers: Default::default(),
            }))
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
            .map(|battery| battery.percentage > 50 || battery.is_charging)
            .unwrap_or(false)
    }

    async fn sta_has_work(&mut self) -> bool {
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

    pub fn signal_sta_work_available(&mut self) {
        self.sta_work_available = Some(true);
    }
}
