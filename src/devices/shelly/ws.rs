use crate::devices::shelly::common::SslCa;
use serde::Deserialize;

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

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum Key {
    #[serde(rename = "ws")]
    Ws,
}
