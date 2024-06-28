use std::fmt::Formatter;

use chrono::{DateTime, Duration, Utc};
use serde::{
    de::{Error, Visitor},
    Deserialize, Deserializer,
};
use serde_with::{DurationSeconds, formats::Flexible, serde_as, TimestampSeconds};

use crate::devices::shelly::common::{
    ActiveEnergy, InitialState, InputMode, LastCommandSource, StatusError, Temperature,
};

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
    #[serde_as(as = "Option<TimestampSeconds<String, Flexible>>")]
    pub timer_started_at: Option<DateTime<Utc>>,
    #[serde_as(as = "Option<DurationSeconds<String, Flexible>>")]
    pub timer_duration: Option<Duration>,
    #[serde(rename = "apower")]
    pub active_power: Option<f64>,
    pub voltage: Option<f64>,
    pub current: Option<f64>,
    #[serde(rename = "pf")]
    pub power_factor: Option<f64>,
    pub frequency: Option<f64>,
    #[serde(rename = "aenergy")]
    pub active_energy: Option<ActiveEnergy>,
    #[serde(rename = "ret_aenergy")]
    pub returned_active_energy: Option<ActiveEnergy>,
    pub temperature: Option<Temperature>,
    pub errors: Option<Box<[StatusError]>>,
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
    pub autorecover_voltage_errors: Option<bool>,
    pub input_id: Option<u8>,
    pub power_limit: Option<u16>,
    pub voltage_limit: Option<u16>,
    pub undervoltage_limit: Option<u16>,
    pub current_limit: Option<f32>,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct StatusTransition {}

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

const KEY_PREFIX: &str = "switch:";

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
