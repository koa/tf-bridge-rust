use crate::devices::shelly::{
    ble, bthome, cloud, common::DeviceId, eth, input, knx, light, mqtt, switch, sys, ui, wifi, ws,
};
use jsonrpsee::{core::Serialize, proc_macros::rpc};
use serde::{de::value::CowStrDeserializer, Deserialize};
use serde_json::value::RawValue;
use std::{
    borrow::Cow,
    fmt::{Display, Formatter},
};
pub use std::{fmt::Debug, str::FromStr};

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
    pub name: Option<Box<str>>,
    pub id: DeviceId,
    pub model: Box<str>,
    pub gen: u8,
    pub fw_id: Box<str>,
    pub ver: Box<str>,
    pub app: Box<str>,
    pub profile: Option<Box<str>>,
    pub auth_en: bool,
    pub auth_domain: Option<Box<str>>,
    pub discoverable: Option<bool>,
    pub key: Option<Box<str>>,
    pub batch: Option<Box<str>>,
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

#[derive(Debug, Deserialize, Clone, PartialEq)]
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

#[derive(Serialize, Deserialize, Clone, PartialEq, Copy, Hash, Eq, Debug, PartialOrd, Ord)]
#[serde(untagged)]
pub enum SwitchingKey {
    Switch(switch::Key),
    Light(light::Key),
}

impl FromStr for SwitchingKey {
    type Err = serde::de::value::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::deserialize(CowStrDeserializer::new(Cow::Borrowed(s)))
    }
}

impl Display for SwitchingKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.serialize(f)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Copy, Hash, Eq, Ord, PartialOrd)]
pub enum ComponentKey {
    Input(input::Key),
    Ble(ble::Key),
    Cloud(cloud::Key),
    Eth(eth::Key),
    Light(light::Key),
    Mqtt(mqtt::Key),
    Switch(switch::Key),
    Sys(sys::Key),
    Ws(ws::Key),
    Wifi(wifi::Key),
    Ui(ui::Key),
    Bthome(bthome::Key),
    Knx(knx::Key),
}
#[derive(Debug, Clone, PartialEq, Copy, Hash, Eq, Ord, PartialOrd)]
pub struct ComponentAddress {
    pub device: DeviceId,
    pub key: ComponentKey,
}

impl Display for ComponentAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.device, self.key)
    }
}
impl Display for ComponentKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ComponentKey::Input(k) => std::fmt::Display::fmt(&k, f),
            ComponentKey::Ble(k) => k.fmt(f),
            ComponentKey::Cloud(k) => k.fmt(f),
            ComponentKey::Eth(k) => k.fmt(f),
            ComponentKey::Light(k) => std::fmt::Display::fmt(&k, f),
            ComponentKey::Mqtt(k) => k.fmt(f),
            ComponentKey::Switch(k) => std::fmt::Display::fmt(&k, f),
            ComponentKey::Sys(k) => k.fmt(f),
            ComponentKey::Ws(k) => k.fmt(f),
            ComponentKey::Wifi(k) => k.fmt(f),
            ComponentKey::Ui(k) => k.fmt(f),
            ComponentKey::Bthome(k) => k.fmt(f),
            ComponentKey::Knx(k) => k.fmt(f),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd)]
pub struct SwitchingKeyId {
    pub device: DeviceId,
    pub key: SwitchingKey,
}
impl ComponentEntry {
    pub fn key(&self) -> ComponentKey {
        match self {
            ComponentEntry::Input(c) => ComponentKey::Input(c.key),
            ComponentEntry::Ble(c) => ComponentKey::Ble(c.key),
            ComponentEntry::Cloud(c) => ComponentKey::Cloud(c.key),
            ComponentEntry::Eth(c) => ComponentKey::Eth(c.key),
            ComponentEntry::Light(c) => ComponentKey::Light(c.key),
            ComponentEntry::Mqtt(c) => ComponentKey::Mqtt(c.key),
            ComponentEntry::Switch(c) => ComponentKey::Switch(c.key),
            ComponentEntry::Sys(c) => ComponentKey::Sys(c.key),
            ComponentEntry::Ws(c) => ComponentKey::Ws(c.key),
            ComponentEntry::Wifi(c) => ComponentKey::Wifi(c.key),
            ComponentEntry::Ui(c) => ComponentKey::Ui(c.key),
            ComponentEntry::Bthome(c) => ComponentKey::Bthome(c.key),
            ComponentEntry::Knx(c) => ComponentKey::Knx(c.key),
        }
    }
    pub fn type_name(&self) -> &'static str {
        match self {
            ComponentEntry::Input(_) => "Input",
            ComponentEntry::Ble(_) => "Ble",
            ComponentEntry::Cloud(_) => "Cloud",
            ComponentEntry::Eth(_) => "Eth",
            ComponentEntry::Light(_) => "Light",
            ComponentEntry::Mqtt(_) => "Mqtt",
            ComponentEntry::Switch(_) => "Switch",
            ComponentEntry::Sys(_) => "Sys",
            ComponentEntry::Ws(_) => "Ws",
            ComponentEntry::Wifi(_) => "Wifi",
            ComponentEntry::Ui(_) => "Ui",
            ComponentEntry::Bthome(_) => "Bthome",
            ComponentEntry::Knx(_) => "Knx",
        }
    }
}

#[cfg(test)]
mod test;
