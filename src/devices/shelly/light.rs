use std::fmt::Formatter;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Deserializer};
use serde::de::{Error, Visitor};
use serde_with::DurationSeconds;
use serde_with::formats::Flexible;
use serde_with::serde_as;
use serde_with::TimestampSeconds;

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Component {
    pub key: Key,
    pub status: Status,
    pub config: Configuration,
}
#[serde_as]
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Status {
    pub id: u16,
    pub source: LastCommandSource,
    pub output: bool,
    pub brightness: u8,
    #[serde_as(as = "Option<TimestampSeconds<String, Flexible>>")]
    pub timer_started_at: Option<DateTime<Utc>>,
    #[serde_as(as = "Option<DurationSeconds<String, Flexible>>")]
    pub timer_duration: Option<Duration>,
    pub transition: Option<StatusTransition>,
    pub temperature: Option<Temperature>,
    #[serde(rename = "aenergy")]
    pub active_energy: Option<ActiveEnergy>,
    #[serde(rename = "apower")]
    pub active_power: Option<f64>,
    pub voltage: Option<f64>,
    pub current: Option<f64>,
    pub calibration: Option<Calibration>,
    pub errors: Option<Box<[StatusError]>>,
    pub flags: Box<[StatusFlags]>,
}
#[serde_as]
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Configuration {
    pub id: u16,
    pub name: Option<Box<str>>,
    pub in_mode: Option<LightMode>,
    pub initial_state: InitialState,
    pub auto_on: bool,
    #[serde_as(as = "DurationSeconds<String, Flexible>")]
    pub auto_on_delay: Duration,
    pub auto_off: bool,
    #[serde_as(as = "DurationSeconds<String, Flexible>")]
    pub auto_off_delay: Duration,
    #[serde_as(as = "DurationSeconds<String, Flexible>")]
    pub transition_duration: Duration,
    pub min_brightness_on_toggle: u8,
    pub night_mode: NightMode,
    pub button_fade_rate: u8,
    pub button_presets: ButtonPresets,
    pub range_map: Option<[f32; 2]>,
    pub power_limit: Option<u16>,
    pub voltage_limit: Option<u16>,
    pub undervoltage_limit: Option<u16>,
    pub current_limit: Option<f32>,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum LastCommandSource {
    #[serde(rename = "init")]
    Init,
    #[serde(rename = "WS_in")]
    WsIn,
    #[serde(rename = "http")]
    Http,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct StatusTransition {}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Temperature {
    #[serde(rename = "tC")]
    pub temp_celsius: Option<f32>,
    #[serde(rename = "tF")]
    pub temp_fahrenheit: Option<f32>,
}
#[serde_as]
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct ActiveEnergy {
    pub total: f64,
    pub by_minute: Option<[f64; 3]>,
    #[serde_as(as = "TimestampSeconds<String, Flexible>")]
    pub minute_ts: DateTime<Utc>,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Calibration {
    pub progress: u8,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
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
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum StatusFlags {
    #[serde(rename = "uncalibrated")]
    Uncalibrated,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum LightMode {
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
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(rename = "snake_case")]
pub enum InitialState {
    #[serde(rename = "on")]
    On,
    #[serde(rename = "off")]
    Off,
    #[serde(rename = "restore_last")]
    RestoreLast,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct NightMode {
    pub enable: bool,
    pub brightness: u8,
    pub active_between: Box<[Box<str>]>,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct ButtonPresets {
    pub button_doublepush: Option<ButtonDoublePush>,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct ButtonDoublePush {
    pub brightness: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub id: u16,
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(KeyVisitor)
    }
}

struct KeyVisitor;

const KEY_PREFIX: &str = "light:";

impl<'de> Visitor<'de> for KeyVisitor {
    type Value = Key;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("a light key beginning with 'light:' followed by a u16")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: Error,
    {
        if v.starts_with(KEY_PREFIX) {
            let id: u16 = v[KEY_PREFIX.len()..]
                .parse()
                .map_err(|e| Error::custom(e))?;
            Ok(Key { id })
        } else {
            Err(Error::custom(format!("wrong prefix: {v}")))
        }
    }
}
