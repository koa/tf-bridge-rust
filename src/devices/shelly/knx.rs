use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Component {
    pub key: Key,
    pub status: Status,
    pub config: Configuration,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Status {}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Configuration {
    pub enable: bool,
    pub ia: Box<str>,
    pub routing: KnxRouting,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct KnxRouting {
    pub addr: SocketAddr,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Copy, Hash, Eq, Ord, PartialOrd)]
pub enum Key {
    #[serde(rename = "knx")]
    Knx,
}
