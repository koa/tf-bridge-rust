use crate::devices::shelly::{light, switch};
use chrono::{DateTime, Utc};
use jsonrpsee::core::Serialize;
use macaddr::MacAddr6;
use serde::{
    de::{Error, Visitor},
    Deserialize, Deserializer,
};
use serde_json::Value;
use serde_with::{formats::Flexible, serde_as, TimestampSeconds};
use std::fmt::Display;
use std::{
    fmt::{Debug, Formatter},
    str::FromStr,
};

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub enum LastCommandSource {
    #[serde(rename = "init")]
    Init,
    #[serde(rename = "WS_in")]
    WsIn,
    #[serde(rename = "http")]
    Http,
    #[serde(rename = "UI")]
    UI,
    #[serde(rename = "timer")]
    Timer,
    #[serde(rename = "")]
    None,
}

#[serde_as]
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct ActiveEnergy {
    pub total: f64,
    pub by_minute: Option<[f64; 3]>,
    #[serde_as(as = "TimestampSeconds<String, Flexible>")]
    pub minute_ts: DateTime<Utc>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct Temperature {
    #[serde(rename = "tC")]
    pub temp_celsius: Option<f32>,
    #[serde(rename = "tF")]
    pub temp_fahrenheit: Option<f32>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub enum StatusError {
    #[serde(rename = "overtemp")]
    Overtemp,
    #[serde(rename = "overpower")]
    Overpower,
    #[serde(rename = "overvoltage")]
    Overvoltage,
    #[serde(rename = "undervoltage")]
    Undervoltage,
    #[serde(rename = "overcurrent")]
    Overcurrent,
    #[serde(rename = "unsupported_load")]
    UnsupportedLoad,
    #[serde(rename = "cal_abort:interrupted")]
    CalibrationAbortInterrupted,
    #[serde(rename = "cal_abort:power_read")]
    CalibrationAbortPowerRead,
    #[serde(rename = "cal_abort:no_load")]
    CalibrationAbortNoLoad,
    #[serde(rename = "cal_abort:non_dimmable")]
    CalibrationAbortNonDimmable,
    #[serde(rename = "cal_abort:overpower")]
    CalibrationAbortOverpower,
    #[serde(rename = "cal_abort:unsupported_load")]
    CalibrationAbortUnsupportedLoad,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub enum InputMode {
    #[serde(rename = "follow")]
    Follow,
    #[serde(rename = "flip")]
    Flip,
    #[serde(rename = "activate")]
    Activate,
    #[serde(rename = "detached")]
    Detached,
    #[serde(rename = "dim")]
    Dim,
    #[serde(rename = "dual_dim")]
    DualDim,
    #[serde(rename = "momentary")]
    Momentary,
    #[serde(rename = "cycle")]
    Cycle,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(rename = "snake_case")]
pub enum InitialState {
    #[serde(rename = "on")]
    On,
    #[serde(rename = "off")]
    Off,
    #[serde(rename = "restore_last")]
    RestoreLast,
    #[serde(rename = "match_input")]
    MatchInput,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct ButtonPresets {
    pub button_doublepush: Option<ButtonDoublePush>,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct ButtonDoublePush {
    pub brightness: f32,
}
#[derive(Deserialize, Debug, Clone, PartialEq, Copy)]
pub enum ActorKey {
    Light(light::Key),
    Switch(switch::Key),
}

#[derive(Deserialize, Debug, Clone, PartialEq, Copy)]
pub struct ActorId {
    device: DeviceId,
    actor: ActorKey,
}

#[derive(Clone, PartialEq, Copy, Eq, Ord, PartialOrd, Hash)]
pub struct DeviceId {
    pub device_type: DeviceType,
    pub mac: MacAddr6,
}

impl Debug for DeviceId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.device_type.fmt(f)?;
        f.write_str("-")?;
        let bytes = self.mac.into_array();
        f.write_fmt(format_args!(
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5],
        ))
    }
}
impl Display for DeviceId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.device_type.fmt(f)?;
        f.write_str("-")?;
        let bytes = self.mac.into_array();
        f.write_fmt(format_args!(
            "{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5],
        ))
    }
}

impl<'de> Deserialize<'de> for DeviceId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(DeviceIdVisitor)
    }
}

struct DeviceIdVisitor;

#[derive(Deserialize, Clone, PartialEq, Copy, Debug, Eq, Ord, PartialOrd, Hash)]
pub enum DeviceType {
    #[serde(rename = "shellyprodm2pm")]
    ShellyProDm2Pm,
    #[serde(rename = "shellypro4pm")]
    ShellyPro4Pm,
}

impl<'de> Visitor<'de> for DeviceIdVisitor {
    type Value = DeviceId;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("parse shelly device id into a Copy-Struct")
    }

    fn visit_str<E>(self, v: &str) -> Result<DeviceId, E>
    where
        E: Error,
    {
        if let Some((type_str, mac_str)) = v.split_once("-") {
            println!("Type str: {type_str}");
            let device_type = serde_json::from_value(Value::from(type_str))
                .map_err(|e| Error::custom(format!("Cannot parse type value {type_str}: {e}")))?;
            let mac = MacAddr6::from_str(mac_str)
                .map_err(|e| Error::custom(format!("Cannot parse mac address {mac_str}: {e}")))?;
            Ok(DeviceId { device_type, mac })
        } else {
            Err(Error::custom(format!("Missing delimiter in {v}")))
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Copy)]
pub enum SslCa {
    #[serde(rename = "*")]
    Disabled,
    #[serde(rename = "user_ca.pem")]
    User,
    #[serde(rename = "ca.pem")]
    BuiltIn,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub enum IPv4Mode {
    #[serde(rename = "dhcp")]
    Dhcp,
    #[serde(rename = "static")]
    Static,
}
