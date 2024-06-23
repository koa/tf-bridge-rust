use serde::Deserialize;

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
    pub rpc: RpcConfiguration,
    pub observer: ObserverConfiguration,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct RpcConfiguration {
    pub enable: bool,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct ObserverConfiguration {
    pub enable: bool,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum Key {
    #[serde(rename = "ble")]
    BLE,
}
