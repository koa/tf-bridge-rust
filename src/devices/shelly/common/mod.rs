use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_with::{formats::Flexible, serde_as, TimestampSeconds};

use crate::devices::shelly::{light, switch};
use crate::devices::shelly::shelly::DeviceId;

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum LastCommandSource {
    #[serde(rename = "init")]
    Init,
    #[serde(rename = "WS_in")]
    WsIn,
    #[serde(rename = "http")]
    Http,
    #[serde(rename = "UI")]
    UI,
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
pub struct Temperature {
    #[serde(rename = "tC")]
    pub temp_celsius: Option<f32>,
    #[serde(rename = "tF")]
    pub temp_fahrenheit: Option<f32>,
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

#[derive(Deserialize, Debug, Clone, PartialEq)]
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
