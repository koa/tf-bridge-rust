use std::net::IpAddr;

use serde::Deserialize;

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Component {
    pub key: Key,
    pub status: Status,
    pub config: Configuration,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Status {
    pub ip: Option<IpAddr>,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Configuration {
    pub enable: bool,
    pub ipv4mode: IPv4Mode,
    pub ip: Option<IpAddr>,
    pub netmask: Option<IpAddr>,
    pub gw: Option<IpAddr>,
    pub nameserver: Option<IpAddr>,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum Key {
    #[serde(rename = "eth")]
    Eth,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum IPv4Mode {
    #[serde(rename = "dhcp")]
    Dhcp,
    #[serde(rename = "static")]
    Static,
}
