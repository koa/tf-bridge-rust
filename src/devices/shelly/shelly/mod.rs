use std::{fmt::Debug, str::FromStr};

use crate::devices::shelly::common::DeviceId;
use crate::devices::shelly::{
    ble, bthome, cloud, eth, input, knx, light, mqtt, switch, sys, ui, wifi, ws,
};
use jsonrpsee::proc_macros::rpc;
use serde::{
    de::{Error, Visitor},
    Deserialize, Deserializer,
};
use serde_json::value::RawValue;

#[rpc(client)]
pub trait Shelly {
    #[method(name = "shelly.getdeviceinfo", param_kind=map)]
    async fn get_deviceinfo(&self, ident: bool) -> Result<GetDeviceInfoResponse, ErrorObjectOwned>;
    #[method(name = "shelly.getcomponents", param_kind=map)]
    async fn get_components(
        &self,
        offset: u16,
        dynamic_only: bool,
    ) -> Result<GetComponentsResponse, ErrorObjectOwned>;
    #[method(name = "shelly.getcomponents",param_kind=map)]
    async fn get_components_string(
        &self,
        offset: u16,
        dynamic_only: bool,
    ) -> Result<Box<RawValue>, ErrorObjectOwned>;
    #[method(name = "shelly.reboot",param_kind=map)]
    async fn reboot(&self, delay_ms: Option<u16>) -> Result<(), ErrorObjectOwned>;
}

#[derive(Deserialize, Debug)]
pub struct GetDeviceInfoResponse {
    name: Option<Box<str>>,
    id: DeviceId,
    model: Box<str>,
    gen: u8,
    fw_id: Box<str>,
    ver: Box<str>,
    app: Box<str>,
    profile: Option<Box<str>>,
    auth_en: bool,
    auth_domain: Option<Box<str>>,
    discoverable: Option<bool>,
    key: Option<Box<str>>,
    batch: Option<Box<str>>,
    fw_sbits: Option<Box<str>>,
}
#[derive(Deserialize, Debug)]
pub struct GetComponentsResponse {
    components: Box<[ComponentEntry]>,
    cfg_rev: u16,
    offset: u16,
    total: u16,
}

impl GetComponentsResponse {
    pub fn components(&self) -> &[ComponentEntry] {
        &self.components
    }
    pub fn cfg_rev(&self) -> u16 {
        self.cfg_rev
    }
    pub fn offset(&self) -> u16 {
        self.offset
    }
    pub fn total(&self) -> u16 {
        self.total
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum ComponentEntry {
    Input(input::Component),
    Ble(ble::Component),
    Cloud(cloud::Component),
    Eth(eth::Component),
    Light(light::Component),
    Mqtt(mqtt::Component),
    Switch(switch::Component),
    Sys(sys::Component),
    Ws(ws::Component),
    Wifi(wifi::Component),
    Ui(ui::Component),
    Bthome(bthome::Component),
    Knx(knx::Component),
}

#[derive(Clone, PartialEq, Copy)]
pub enum SwitchingKey {
    Switch(switch::Key),
    Light(light::Key),
}
#[derive(Clone, PartialEq, Copy)]
pub struct SwitchingKeyId {
    pub device: DeviceId,
    pub key: SwitchingKey,
}

#[cfg(test)]
mod test;
