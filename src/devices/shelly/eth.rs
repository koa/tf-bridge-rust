use std::net::IpAddr;

use crate::devices::shelly::common::IPv4Mode;
use serde::{Deserialize, Serialize};

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
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Copy, Hash, Eq, Ord, PartialOrd)]
pub enum Key {
    #[serde(rename = "eth")]
    Eth,
}
