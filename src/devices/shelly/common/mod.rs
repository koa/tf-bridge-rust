use chrono::{DateTime, Utc};
use jsonrpsee::core::Serialize;
use macaddr::MacAddr6;
use serde::{
    de::{Error, Visitor},
    Deserialize, Deserializer, Serializer,
};
use serde_json::Value;
use serde_with::{formats::Flexible, serde_as, TimestampSeconds};
use std::fmt::{Display, Write};
use std::{
    fmt::{Debug, Formatter},
    str::FromStr,
};
use thiserror::Error;
#[cfg(test)]
mod test;
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
    #[serde(rename = "calibration")]
    Calibration,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Ord, PartialOrd)]
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

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, Ord, PartialOrd)]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, PartialOrd)]
pub struct ButtonPresets {
    pub button_doublepush: Option<ButtonDoublePush>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, PartialOrd)]
pub struct ButtonDoublePush {
    pub brightness: f32,
}

#[derive(Copy, Clone, PartialEq, PartialOrd, Hash, Ord, Eq)]
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
        let device_type = serde_json::to_value(self.device_type).map_err(|e| std::fmt::Error)?;
        f.write_str(device_type.as_str().ok_or(std::fmt::Error)?)?;
        f.write_str("-")?;
        let bytes = self.mac.into_array();
        f.write_fmt(format_args!(
            "{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5],
        ))
    }
}

#[derive(Debug, Error)]
pub enum DeviceIdParseError {
    #[error("Missing delimiter \"-\"")]
    MissingDelimiterError,
    #[error("Cannot parse type {0}")]
    ErrorParseType(#[from] serde_json::Error),
    #[error("Cannot parse mac {0}")]
    ErrorParseMac(#[from] macaddr::ParseError),
}
impl FromStr for DeviceId {
    type Err = DeviceIdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some((type_str, mac_str)) = s.split_once("-") {
            let device_type = serde_json::from_value(Value::from(type_str))?;
            let mac = MacAddr6::from_str(mac_str)?;
            Ok(DeviceId { device_type, mac })
        } else {
            Err(DeviceIdParseError::MissingDelimiterError)
        }
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

impl Serialize for DeviceId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

struct DeviceIdVisitor;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Copy, Eq, Ord, PartialOrd, Hash)]
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
        DeviceId::from_str(v).map_err(|e| Error::custom(format!("Cannot parse device id {v}: {e}")))
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

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct SetConfigResponse {
    restart_required: bool,
}
