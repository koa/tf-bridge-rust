use crate::devices::shelly::common::IPv4Mode;
use chrono::Duration;
use serde::Deserialize;
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
#[serde(rename_all = "camelCase")]
pub enum WifiStatus {
    Disconnected,
    Connecting,
    Connected,
    GotIp,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Configuration {
    pub ap: ApConfiguration,
    pub sta: StaConfiguration,
    pub sta1: StaConfiguration,
    pub roam: RoamConfiguration,
}
#[serde_as]
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct RoamConfiguration {
    pub rssi_thr: f32,
    #[serde_as(as = "Option<DurationSeconds<String, Flexible>>")]
    pub interval: Option<Duration>,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
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
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct ApConfiguration {
    pub ssid: Box<str>,
    pub pass: Option<Box<str>>,
    pub is_open: bool,
    pub enable: bool,
    pub range_extender: ApRangeExtender,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct ApRangeExtender {
    pub enable: bool,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum Key {
    #[serde(rename = "wifi")]
    Wifi,
}
