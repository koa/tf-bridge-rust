use crate::devices::shelly::common::IPv4Mode;
use crate::devices::shelly::shelly::ShellyClient;
use crate::devices::shelly::wifi::rpc::WifiClient;
use chrono::Duration;
use jsonrpsee::core::client::{Client, Error};
use serde::{Deserialize, Serialize};
use serde_with::formats::Flexible;
use serde_with::serde_as;
use serde_with::DurationSeconds;
use std::net::Ipv4Addr;

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Component {
    pub key: Key,
    pub status: Status,
    pub config: Configuration,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Status {
    pub sta_ip: Option<Box<str>>,
    pub status: WifiStatus,
    pub ssid: Option<Box<str>>,
    pub rssi: f32,
    pub ap_client_count: Option<u16>,
}
#[derive(Deserialize, Debug, Clone, PartialEq, Copy)]
#[serde(rename_all = "lowercase")]
pub enum WifiStatus {
    Disconnected,
    Connecting,
    Connected,
    #[serde(rename = "got ip")]
    GotIp,
}
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct Configuration {
    pub ap: ApConfiguration,
    pub sta: StaConfiguration,
    pub sta1: StaConfiguration,
    pub roam: RoamConfiguration,
}
#[serde_as]
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct RoamConfiguration {
    pub rssi_thr: f32,
    #[serde_as(as = "Option<DurationSeconds<String, Flexible>>")]
    pub interval: Option<Duration>,
}
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct StaConfiguration {
    pub ssid: Option<Box<str>>,
    pub pass: Option<Box<str>>,
    pub is_open: bool,
    pub enable: bool,
    pub ipv4mode: IPv4Mode,
    pub ip: Option<Ipv4Addr>,
    pub netmask: Option<Ipv4Addr>,
    pub gw: Option<Ipv4Addr>,
    pub nameserver: Option<Ipv4Addr>,
}
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct ApConfiguration {
    pub ssid: Box<str>,
    pub pass: Option<Box<str>>,
    pub is_open: bool,
    pub enable: bool,
    pub range_extender: ApRangeExtender,
}
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct ApRangeExtender {
    pub enable: bool,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub enum Key {
    #[serde(rename = "wifi")]
    Wifi,
}
impl Component {
    pub async fn disable(&mut self, client: &Client) -> Result<(), Error> {
        let mut cfg = self.config.clone();
        let mut modified = false;
        if cfg.ap.enable {
            cfg.ap.enable = false;
            modified = true;
        }
        if cfg.sta.enable {
            cfg.sta.enable = false;
            modified = true;
        }
        if cfg.sta1.enable {
            cfg.sta1.enable = false;
            modified = true;
        }
        let restart_required = client.setConfig(&cfg).await?.restart_required;
        if restart_required {
            client.reboot(None).await?;
        }
        self.config = cfg;
        Ok(())
    }
}
mod rpc {
    use crate::devices::shelly::wifi::Configuration;
    use jsonrpsee::proc_macros::rpc;
    use serde::{Deserialize, Serialize};

    #[rpc(client)]
    pub trait Wifi {
        #[method(name = "Wifi.SetConfig", param_kind=map)]
        async fn setConfig(
            &self,
            config: &Configuration,
        ) -> Result<SetConfigResult, ErrorObjectOwned>;
    }
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    pub struct SetConfigResult {
        pub restart_required: bool,
    }
}
