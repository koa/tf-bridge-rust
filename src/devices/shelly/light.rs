use crate::{
    data::{
        registry::EventRegistry,
        wiring::{ShellyLightRegister, ShellyLightSettings},
    },
    devices::shelly::{
        common::{
            ButtonPresets, InitialState, InputMode, LastCommandSource, SetConfigResponse,
            StatusError, Temperature,
        },
        light::rpc::LightClient,
        ShellyError,
    },
    serde::{PrefixedKey, SerdeStringKey},
    shelly::common::ActiveEnergy,
};
use chrono::{DateTime, Duration, Utc};
use futures::StreamExt;
use jsonrpsee::core::{
    client::{Client, Error},
    Serialize,
};
use log::{error, info};
use serde::Deserialize;
use serde_with::{formats::Flexible, serde_as, DefaultOnNull, DurationSeconds, TimestampSeconds};
use std::{num::Saturating, sync::Arc, time};
use tokio::{sync::Mutex, task::JoinHandle, time::sleep};
use tokio_util::either::Either;

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
    #[serde(default)]
    pub flags: Box<[StatusFlags]>,
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
impl Default for Settings {
    fn default() -> Self {
        Self {
            name: Default::default(),
            in_mode: Some(InputMode::Dim),
            initial_state: InitialState::Off,
            auto_on: false,
            auto_on_delay: Duration::seconds(1),
            auto_off: true,
            auto_off_delay: Duration::hours(2),
            transition_duration: Default::default(),
            min_brightness_on_toggle: 0.0,
            night_mode: NightMode {
                enable: false,
                brightness: 0.0,
                active_between: Box::new([]),
            },
            button_fade_rate: 3,
            button_presets: ButtonPresets {
                button_doublepush: None,
            },
            range_map: None,
            power_limit: None,
            voltage_limit: None,
            undervoltage_limit: None,
            current_limit: None,
        }
    }
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd)]
pub struct NightMode {
    pub enable: bool,
    pub brightness: f32,
    pub active_between: Box<[Box<str>]>,
}

#[derive(Debug, Clone, PartialEq, Copy, Hash, Eq, Ord, PartialOrd, Serialize, Deserialize)]
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
    pub fn client(self, client: Arc<Mutex<Client>>) -> ComponentClient {
        ComponentClient {
            component: self,
            client,
        }
    }
    pub fn run(
        self,
        client: &Arc<Mutex<Client>>,
        settings: &ShellyLightSettings,
        event_registry: EventRegistry,
    ) -> JoinHandle<()> {
        let client = client.clone();
        let light = self.clone();
        let settings = settings.clone();
        tokio::spawn(async move {
            match light.run_loop(client, settings, event_registry).await {
                Ok(()) => {
                    info!("Light Terminated")
                }
                Err(e) => {
                    error!("Light {} failed: {}", self.key.id, e)
                }
            }
        })
    }
    async fn run_loop(
        self,
        client: Arc<Mutex<Client>>,
        settings: ShellyLightSettings,
        event_registry: EventRegistry,
    ) -> Result<(), ShellyError> {
        let new_settings = &settings.settings;
        let client = self.client(client.clone());
        if new_settings != &client.component.config.settings {
            client.set_config(new_settings.clone()).await?;
        };
        if client
            .component
            .status
            .flags
            .contains(&StatusFlags::Uncalibrated)
        {
            client.calibrate().await?;
        }
        let mut stream = match settings.register {
            ShellyLightRegister::Dimm(register) => Either::Left(
                event_registry
                    .brightness_stream(register)
                    .await
                    .map(LoopEvent::SetBrightness),
            ),
            ShellyLightRegister::Switch(register) => Either::Right(
                event_registry
                    .switch_stream(register)
                    .await
                    .map(LoopEvent::Switch),
            ),
        };
        while let Some(event) = stream.next().await {
            match event {
                LoopEvent::SetBrightness(event) => {
                    client
                        .set_brightness(((event.0 as u16 * 101) >> 8) as u8)
                        .await?
                }
                LoopEvent::Switch(on) => client.set_brightness(if on { 0 } else { 100 }).await?,
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
enum LoopEvent {
    SetBrightness(Saturating<u8>),
    Switch(bool),
}
pub struct ComponentClient {
    component: Component,
    client: Arc<Mutex<Client>>,
}

impl ComponentClient {
    pub async fn toggle(&self) -> Result<(), Error> {
        self.client.lock().await.toggle(self.component.key.id).await
    }
    pub async fn set_brightness(&self, brightness: u8) -> Result<(), Error> {
        let toggle_after = if brightness > 0 && self.component.config.settings.auto_off {
            Some(self.component.config.settings.auto_off_delay.into())
        } else {
            None
        };

        self.client
            .lock()
            .await
            .set(
                self.component.key.id,
                Some(brightness > 0),
                Some(brightness),
                None,
                toggle_after,
            )
            .await
    }
    pub async fn set_config(&self, settings: Settings) -> Result<SetConfigResponse, Error> {
        self.client
            .lock()
            .await
            .set_config(
                self.component.key.id,
                Configuration {
                    id: self.component.key.id,
                    settings,
                },
            )
            .await
    }
    pub async fn calibrate(&self) -> Result<(), Error> {
        let client = self.client.lock().await;
        client.calibrate(self.component.key.id).await?;
        while let Some(calibration) = client.get_status(self.component.key.id).await?.calibration {
            sleep(time::Duration::from_millis(500)).await;
        }
        Ok(())
    }
    pub async fn refresh(&mut self) -> Result<(), Error> {
        //self.component.config = self.client.get_config(self.component.key.id).await?;
        self.component.status = self
            .client
            .lock()
            .await
            .get_status(self.component.key.id)
            .await?;
        Ok(())
    }
}

mod rpc {
    use crate::devices::shelly::common::SetConfigResponse;
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
        #[method(name = "Light.SetConfig", param_kind=map)]
        async fn set_config(
            &self,
            id: u16,
            config: Configuration,
        ) -> Result<SetConfigResponse, ErrorObjectOwned>;
        #[method(name = "Light.GetConfig", param_kind=map)]
        async fn get_config(&self, id: u16) -> Result<Configuration, ErrorObjectOwned>;
        #[method(name = "Light.GetStatus", param_kind=map)]
        async fn get_status(&self, id: u16) -> Result<Status, ErrorObjectOwned>;
        #[method(name = "Light.Calibrate", param_kind=map)]
        async fn calibrate(&self, id: u16) -> Result<(), ErrorObjectOwned>;
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
