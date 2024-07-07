use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_with::{DurationSeconds, formats::Flexible, serde_as, TimestampSeconds};

use crate::{
    devices::shelly::common::{
        ButtonPresets, InitialState, InputMode, LastCommandSource, StatusError, Temperature,
    },
    serde::{PrefixedKey, SerdeStringKey},
    shelly::common::ActiveEnergy,
};

pub type Key = SerdeStringKey<LightKey>;

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
    pub brightness: f32,
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
    pub in_mode: Option<InputMode>,
    pub initial_state: InitialState,
    pub auto_on: bool,
    #[serde_as(as = "DurationSeconds<String, Flexible>")]
    pub auto_on_delay: Duration,
    pub auto_off: bool,
    #[serde_as(as = "DurationSeconds<String, Flexible>")]
    pub auto_off_delay: Duration,
    #[serde_as(as = "DurationSeconds<String, Flexible>")]
    pub transition_duration: Duration,
    pub min_brightness_on_toggle: f32,
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
pub struct StatusTransition {}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Calibration {
    pub progress: u8,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum StatusFlags {
    #[serde(rename = "uncalibrated")]
    Uncalibrated,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct NightMode {
    pub enable: bool,
    pub brightness: f32,
    pub active_between: Box<[Box<str>]>,
}

#[derive(Debug, Clone, PartialEq, Copy)]
pub struct LightKey {
    pub id: u16,
}

impl PrefixedKey for LightKey {
    fn prefix() -> &'static str {
        "light:"
    }
}

impl From<u16> for LightKey {
    fn from(id: u16) -> Self {
        LightKey { id }
    }
}
impl From<LightKey> for u16 {
    fn from(value: LightKey) -> Self {
        value.id
    }
}
