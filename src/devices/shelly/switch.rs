use crate::data::registry::EventRegistry;
use crate::data::wiring::ShellySwitchSettings;
use crate::devices::shelly::common::SetConfigResponse;
use crate::devices::shelly::ShellyError;
use crate::{
    devices::shelly::{
        common::{
            ActiveEnergy, InitialState, InputMode, LastCommandSource, StatusError, Temperature,
        },
        switch::rpc::{SwitchClient as GeneratedSwitchClient, WasOnResponse},
    },
    serde::{PrefixedKey, SerdeStringKey},
};
use chrono::{DateTime, Duration, Utc};
use config::Case::Toggle;
use futures::StreamExt;
use jsonrpsee::core::client::{Client, Error};
use log::{error, info};
use serde::{Deserialize, Serialize};
use serde_with::{formats::Flexible, serde_as, DefaultOnNull, DurationSeconds, TimestampSeconds};
use std::num::Saturating;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

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
    #[serde(flatten)]
    pub settings: Settings,
}
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd)]
pub struct Settings {
    pub name: Option<Box<str>>,
    pub in_mode: InputMode,
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

impl Default for Settings {
    fn default() -> Self {
        Settings {
            name: Default::default(),
            in_mode: InputMode::Detached,
            initial_state: InitialState::Off,
            auto_on: false,
            auto_on_delay: Duration::hours(2),
            auto_off: false,
            auto_off_delay: Duration::hours(2),
            autorecover_voltage_errors: None,
            input_id: None,
            power_limit: None,
            voltage_limit: None,
            undervoltage_limit: None,
            current_limit: None,
        }
    }
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct StatusTransition {}

#[derive(Debug, Clone, PartialEq, Copy, Hash, Eq, Ord, PartialOrd)]
pub struct SwitchKey {
    pub id: u16,
}
pub struct ComponentClient {
    component: Component,
    client: Arc<Mutex<Client>>,
}

impl Component {
    pub fn client(self, client: Arc<Mutex<Client>>) -> ComponentClient {
        ComponentClient {
            component: self,
            client,
        }
    }
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
    pub fn run(
        self,
        client: &Arc<Mutex<Client>>,
        settings: &ShellySwitchSettings,
        event_registry: EventRegistry,
    ) -> JoinHandle<()> {
        let client = client.clone();
        let settings = settings.clone();
        let id = self.key.id;
        tokio::spawn(async move {
            match self.run_loop(client, settings, event_registry).await {
                Ok(()) => {
                    info!("Switch Terminated")
                }
                Err(e) => {
                    error!("Switch {id} failed: {}", e)
                }
            }
        })
    }
    async fn run_loop(
        self,
        client: Arc<Mutex<Client>>,
        settings: ShellySwitchSettings,
        event_registry: EventRegistry,
    ) -> Result<(), ShellyError> {
        let new_settings = &settings.settings;
        let client = self.client(client.clone());
        if new_settings != &client.component.config.settings {
            info!("Switching to {new_settings:#?}");
            client.set_config(new_settings.clone()).await?;
        };
        let mut stream = event_registry
            .switch_stream(settings.register)
            .await
            .map(LoopEvent::Switch);
        while let Some(event) = stream.next().await {
            match event {
                LoopEvent::Switch(on) => {
                    client.set_on(on).await?;
                }
            }
        }
        Ok(())
    }
}
impl ComponentClient {
    pub async fn set_config(&self, settings: Settings) -> Result<SetConfigResponse, Error> {
        self.client
            .lock()
            .await
            .set_switch_config(
                self.component.key.id,
                Configuration {
                    id: self.component.key.id,
                    settings,
                },
            )
            .await
    }
    pub async fn set_on(&self, on: bool) -> Result<(), Error> {
        let toggle = if on && self.component.config.settings.auto_off {
            Some(self.component.config.settings.auto_off_delay.into())
        } else if !on && self.component.config.settings.auto_on {
            Some(self.component.config.settings.auto_on_delay.into())
        } else {
            None
        };
        self.client
            .lock()
            .await
            .set(self.component.key.id, on, toggle)
            .await
            .map(|_| ())
    }
}

#[derive(Debug, Clone)]
enum LoopEvent {
    Switch(bool),
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
    use super::*;
    use crate::devices::shelly::common::SetConfigResponse;
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
        #[method(name = "Switch.SetConfig", param_kind=map)]
        async fn set_switch_config(
            &self,
            id: u16,
            config: Configuration,
        ) -> Result<SetConfigResponse, ErrorObjectOwned>;
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
