use chrono::{DateTime, Duration, Utc};
use jsonrpsee::core::client::{Client, Error};
use serde::Deserialize;
use serde_with::{formats::Flexible, serde_as, DurationSeconds, TimestampSeconds};

use crate::devices::shelly::light::rpc::LightClient;
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

#[derive(Debug, Clone, PartialEq, Copy, Hash, Eq, Ord, PartialOrd)]
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
impl Component {
    pub fn client<'a>(&'a mut self, client: &'a Client) -> ComponentClient<'a> {
        ComponentClient {
            component: self,
            client,
        }
    }
}
struct ComponentClient<'a> {
    component: &'a mut Component,
    client: &'a Client,
}

impl<'a> ComponentClient<'a> {
    pub async fn toggle(&self) -> Result<(), Error> {
        self.client.toggle(self.component.key.id).await
    }
    pub async fn set_brightness(&self, brightness: u8) -> Result<(), Error> {
        self.client
            .set(self.component.key.id, None, Some(brightness), None, None)
            .await
    }
    pub async fn refresh(&mut self) -> Result<(), Error> {
        //self.component.config = self.client.get_config(self.component.key.id).await?;
        self.component.status = self.client.get_status(self.component.key.id).await?;
        Ok(())
    }
}

mod rpc {
    use crate::devices::shelly::light::{Configuration, Status};
    use chrono::Duration;
    use jsonrpsee::proc_macros::rpc;
    use serde::Serialize;
    use serde_with::{formats::Flexible, serde_as, DurationSeconds};

    #[rpc(client)]
    pub trait Light {
        #[method(name = "Light.Toggle", param_kind=map)]
        async fn toggle(&self, id: u16) -> Result<(), ErrorObjectOwned>;
        #[method(name = "Light.Set", param_kind=map)]
        async fn set(
            &self,
            id: u16,
            on: Option<bool>,
            brightness: Option<u8>,
            transition_duration: Option<SerializableDurationSeconds>,
            toggle_after: Option<SerializableDurationSeconds>,
        ) -> Result<(), ErrorObjectOwned>;
        #[method(name = "Light.GetConfig", param_kind=map)]
        async fn get_config(&self, id: u16) -> Result<Configuration, ErrorObjectOwned>;
        #[method(name = "Light.GetStatus", param_kind=map)]
        async fn get_status(&self, id: u16) -> Result<Status, ErrorObjectOwned>;
    }
    #[serde_as]
    #[derive(Serialize, Debug, Clone, PartialEq)]
    pub struct SerializableDurationSeconds(
        #[serde_as(as = "DurationSeconds<String, Flexible>")] Duration,
    );

    impl From<Duration> for SerializableDurationSeconds {
        fn from(delay: Duration) -> Self {
            Self(delay)
        }
    }
    impl From<SerializableDurationSeconds> for Duration {
        fn from(value: SerializableDurationSeconds) -> Self {
            value.0
        }
    }
}
