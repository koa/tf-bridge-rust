use chrono::{DateTime, Duration, Utc};
use jsonrpsee::core::client::{Client, Error};
use serde::{Deserialize, Serialize};
use serde_with::{formats::Flexible, serde_as, DurationSeconds, TimestampSeconds};

use crate::devices::shelly::switch::rpc::{SwitchClient as GeneratedSwitchClient, WasOnResponse};
use crate::{
    devices::shelly::common::{
        ActiveEnergy, InitialState, InputMode, LastCommandSource, StatusError, Temperature,
    },
    serde::{PrefixedKey, SerdeStringKey},
};

pub type Key = SerdeStringKey<SwitchKey>;

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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd)]
pub struct Configuration {
    pub id: u16,
    pub name: Option<Box<str>>,
    #[serde(flatten)]
    pub settings: Settings,
}
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd)]
pub struct Settings {
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

#[derive(Debug, Clone, PartialEq, Copy, Hash, Eq, Ord, PartialOrd)]
pub struct SwitchKey {
    pub id: u16,
}

impl Component {
    pub async fn toggle(&self, client: &Client) -> Result<WasOnResponse, Error> {
        client.toggle(self.key.id).await
    }
    pub async fn set(
        &self,
        client: &Client,
        on: bool,
        toggle_after: Option<Duration>,
    ) -> Result<WasOnResponse, Error> {
        client
            .set(self.key.id, on, toggle_after.map(|d| d.into()))
            .await
    }
}

impl From<u16> for SwitchKey {
    fn from(value: u16) -> Self {
        SwitchKey { id: value }
    }
}
impl From<SwitchKey> for u16 {
    fn from(value: SwitchKey) -> Self {
        value.id
    }
}

impl PrefixedKey for SwitchKey {
    fn prefix() -> &'static str {
        "switch:"
    }
}

mod rpc {
    use chrono::Duration;
    use jsonrpsee::proc_macros::rpc;
    use serde::{Deserialize, Serialize};
    use serde_with::{formats::Flexible, serde_as, DurationSeconds};

    #[rpc(client)]
    pub trait Switch {
        #[method(name = "Switch.Toggle", param_kind=map)]
        async fn toggle(&self, id: u16) -> Result<WasOnResponse, ErrorObjectOwned>;
        #[method(name = "Switch.Set", param_kind=map)]
        async fn set(
            &self,
            id: u16,
            on: bool,
            toggle_after: Option<ToggleAfter>,
        ) -> Result<WasOnResponse, ErrorObjectOwned>;
    }
    #[serde_as]
    #[derive(Serialize, Debug, Clone, PartialEq)]
    pub struct ToggleAfter(#[serde_as(as = "DurationSeconds<String, Flexible>")] Duration);

    impl From<Duration> for ToggleAfter {
        fn from(delay: Duration) -> Self {
            Self(delay)
        }
    }
    impl From<ToggleAfter> for Duration {
        fn from(value: ToggleAfter) -> Self {
            value.0
        }
    }

    #[derive(Deserialize, Debug, Clone, PartialEq)]
    pub struct WasOnResponse {
        pub was_on: bool,
    }
}
