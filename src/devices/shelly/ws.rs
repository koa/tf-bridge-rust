use crate::devices::shelly::common::SslCa;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Component {
    pub key: Key,
    pub status: Status,
    pub config: Configuration,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Status {
    pub connected: bool,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Configuration {
    pub enable: bool,
    pub server: Option<Box<str>>,
    pub ssl_ca: SslCa,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Copy, Hash, Eq, Ord, PartialOrd)]
pub enum Key {
    #[serde(rename = "ws")]
    Ws,
}
