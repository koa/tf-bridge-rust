use std::io;
use std::net::{IpAddr, Ipv6Addr};

use config::{Config, ConfigError, Environment, File};
use google_sheets4::oauth2::{
    parse_service_account_key, read_service_account_key, ServiceAccountKey,
};
use lazy_static::lazy_static;
use log::warn;
use serde::Deserialize;
use thiserror::Error;

#[derive(Deserialize, Debug)]
pub struct ServerSettings {
    port: Option<u16>,
    mgmt_port: Option<u16>,
    bind_address: Option<IpAddr>,
    setup_file: Option<Box<str>>,
    state_file: Option<Box<str>>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Tinkerforge {
    endpoints: Box<[TinkerforgeEndpoint]>,
}

impl Tinkerforge {
    pub fn endpoints(&self) -> &[TinkerforgeEndpoint] {
        &self.endpoints
    }
}

#[derive(Deserialize, Debug, Copy, Clone)]
pub struct TinkerforgeEndpoint {
    address: IpAddr,
    port: Option<u16>,
}

impl TinkerforgeEndpoint {
    pub fn address(&self) -> IpAddr {
        self.address
    }
    pub fn port(&self) -> u16 {
        self.port.unwrap_or(4223)
    }
}
#[derive(Deserialize, Debug)]
pub struct GoogleSheet {
    key_file: Option<Box<str>>,
    key_data: Option<Box<str>>,
    spreadsheet_id: Box<str>,
    endpoints: GoogleEndpointData,
    light: GoogleLightData,
    light_templates: GoogleLightTemplateData,
    buttons: GoogleButtonData,
    button_templates: GoogleButtonTemplate,
    room_controllers: GoogleRoomController,
    motion_detectors: GoogleMotionDetectors,
    relays: GoogleRelay,
}
#[derive(Deserialize, Debug)]
pub struct GoogleEndpointData {
    sheet: Box<str>,
    range: Box<str>,
    address: Box<str>,
    state: Box<str>,
}
#[derive(Deserialize, Debug)]
pub struct GoogleButtonData {
    sheet: Box<str>,
    range: Box<str>,
    room_id: Box<str>,
    button_id: Box<str>,
    button_idx: Box<str>,
    button_type: Box<str>,
    device_address: Box<str>,
    first_input_idx: Box<str>,
    state: Box<str>,
}

#[derive(Deserialize, Debug)]
pub struct GoogleButtonTemplate {
    sheet: Box<str>,
    range: Box<str>,
    name: Box<str>,
    discriminator: Box<str>,
    sub_devices: Box<str>,
}
#[derive(Deserialize, Debug)]
pub struct GoogleLightTemplateData {
    sheet: Box<str>,
    range: Box<str>,
    name_column: Box<str>,
    discriminator_column: Box<str>,
    temperature_warm_column: Box<str>,
    temperature_cold_column: Box<str>,
}
#[derive(Deserialize, Debug)]
pub struct GoogleLightData {
    sheet: Box<str>,
    range: Box<str>,
    room_id: Box<str>,
    //light_id: Box<str>,
    light_idx: Box<str>,
    template: Box<str>,
    device_address: Box<str>,
    bus_start_address: Box<str>,
    manual_buttons: Box<[Box<str>]>,
    presence_detectors: Box<[Box<str>]>,
    touchscreen_whitebalance: Box<str>,
    touchscreen_brightness: Box<str>,
    state: Box<str>,
}
#[derive(Deserialize, Debug)]
pub struct GoogleRoomController {
    sheet: Box<str>,
    range: Box<str>,
    room_id: Box<str>,
    controller_id: Box<str>,
    controller_idx: Box<str>,
    orientation: Box<str>,
    touchscreen_device_address: Box<str>,
    temperature_device_address: Box<str>,
    enable_heat_control: Box<str>,
    enable_whitebalance_control: Box<str>,
    enable_brightness_control: Box<str>,
    touchscreen_state: Box<str>,
    temperature_state: Box<str>,
}
#[derive(Deserialize, Debug)]
pub struct GoogleMotionDetectors {
    sheet: Box<str>,
    range: Box<str>,
    room_id: Box<str>,
    device_address: Box<str>,
    id: Box<str>,
    idx: Box<str>,
    state: Box<str>,
}
#[derive(Deserialize, Debug)]
pub struct GoogleRelay {
    sheet: Box<str>,
    range: Box<str>,
    room_id: Box<str>,
    //id: Box<str>,
    idx: Box<str>,
    device_address: Box<str>,
    device_channel: Box<str>,
    temperature_sensor: Box<str>,
    ring_button: Box<str>,
    state: Box<str>,
}
#[derive(Error, Debug)]
pub enum GoogleError {
    #[error("IO Error {0}")]
    Io(#[from] io::Error),
    #[error("error access configuration: {description}")]
    ConfigContent { description: &'static str },
}

impl GoogleSheet {
    pub async fn read_secret(&self) -> Result<ServiceAccountKey, GoogleError> {
        if let Some(filename) = &self.key_file {
            let result = read_service_account_key(filename.as_ref()).await;
            match result {
                Ok(key) => Ok(key),
                Err(error) => {
                    warn!("Cannot load file {filename}: {error}");
                    Err(error.into())
                }
            }
        } else if let Some(data) = &self.key_data {
            Ok(parse_service_account_key(data.as_ref())?)
        } else {
            Err(GoogleError::ConfigContent {
                description: "neither key_file nor key_data filled in",
            })
        }
    }

    pub fn spreadsheet_id(&self) -> &str {
        &self.spreadsheet_id
    }

    pub fn light(&self) -> &GoogleLightData {
        &self.light
    }

    pub fn light_templates(&self) -> &GoogleLightTemplateData {
        &self.light_templates
    }

    pub fn buttons(&self) -> &GoogleButtonData {
        &self.buttons
    }
    pub fn button_templates(&self) -> &GoogleButtonTemplate {
        &self.button_templates
    }

    pub fn room_controllers(&self) -> &GoogleRoomController {
        &self.room_controllers
    }
    pub fn motion_detectors(&self) -> &GoogleMotionDetectors {
        &self.motion_detectors
    }

    pub fn relays(&self) -> &GoogleRelay {
        &self.relays
    }

    pub fn endpoints(&self) -> &GoogleEndpointData {
        &self.endpoints
    }
}
impl GoogleEndpointData {
    pub fn sheet(&self) -> &str {
        &self.sheet
    }
    pub fn range(&self) -> &str {
        &self.range
    }
    pub fn address(&self) -> &str {
        &self.address
    }
    pub fn state(&self) -> &str {
        &self.state
    }
}
impl GoogleLightTemplateData {
    pub fn sheet(&self) -> &str {
        &self.sheet
    }
    pub fn range(&self) -> &str {
        &self.range
    }
    pub fn name_column(&self) -> &str {
        &self.name_column
    }
    pub fn discriminator_column(&self) -> &str {
        &self.discriminator_column
    }
    pub fn temperature_warm_column(&self) -> &str {
        &self.temperature_warm_column
    }
    pub fn temperature_cold_column(&self) -> &str {
        &self.temperature_cold_column
    }
}
impl GoogleLightData {
    pub fn range(&self) -> &str {
        &self.range
    }

    pub fn room_id(&self) -> &str {
        &self.room_id
    }
    /*pub fn light_id(&self) -> &str {
        &self.light_id
    }*/
    pub fn light_idx(&self) -> &str {
        &self.light_idx
    }
    pub fn sheet(&self) -> &str {
        &self.sheet
    }

    pub fn template(&self) -> &str {
        &self.template
    }
    pub fn device_address(&self) -> &str {
        &self.device_address
    }
    pub fn bus_start_address(&self) -> &str {
        &self.bus_start_address
    }
    pub fn manual_buttons(&self) -> &[Box<str>] {
        &self.manual_buttons
    }
    pub fn presence_detectors(&self) -> &[Box<str>] {
        &self.presence_detectors
    }
    pub fn touchscreen_whitebalance(&self) -> &str {
        &self.touchscreen_whitebalance
    }
    pub fn touchscreen_brightness(&self) -> &str {
        &self.touchscreen_brightness
    }
    pub fn state(&self) -> &str {
        &self.state
    }
}

impl GoogleButtonData {
    pub fn sheet(&self) -> &str {
        &self.sheet
    }
    pub fn range(&self) -> &str {
        &self.range
    }
    pub fn room_id(&self) -> &str {
        &self.room_id
    }
    pub fn button_id(&self) -> &str {
        &self.button_id
    }
    pub fn button_idx(&self) -> &str {
        &self.button_idx
    }
    pub fn button_type(&self) -> &str {
        &self.button_type
    }
    pub fn device_address(&self) -> &str {
        &self.device_address
    }
    pub fn first_input_idx(&self) -> &str {
        &self.first_input_idx
    }

    pub fn state(&self) -> &str {
        &self.state
    }
}
impl GoogleButtonTemplate {
    pub fn sheet(&self) -> &str {
        &self.sheet
    }
    pub fn range(&self) -> &str {
        &self.range
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn discriminator(&self) -> &str {
        &self.discriminator
    }
    pub fn sub_devices(&self) -> &str {
        &self.sub_devices
    }
}

impl GoogleRoomController {
    pub fn sheet(&self) -> &str {
        &self.sheet
    }
    pub fn range(&self) -> &str {
        &self.range
    }
    pub fn room_id(&self) -> &str {
        &self.room_id
    }
    pub fn controller_id(&self) -> &str {
        &self.controller_id
    }

    pub fn touchscreen_device_address(&self) -> &str {
        &self.touchscreen_device_address
    }
    pub fn temperature_device_address(&self) -> &str {
        &self.temperature_device_address
    }
    pub fn controller_idx(&self) -> &str {
        &self.controller_idx
    }

    pub fn orientation(&self) -> &str {
        &self.orientation
    }

    pub fn enable_heat_control(&self) -> &str {
        &self.enable_heat_control
    }
    pub fn enable_whitebalance_control(&self) -> &str {
        &self.enable_whitebalance_control
    }
    pub fn enable_brightness_control(&self) -> &str {
        &self.enable_brightness_control
    }

    pub fn touchscreen_state(&self) -> &str {
        &self.touchscreen_state
    }
    pub fn temperature_state(&self) -> &str {
        &self.temperature_state
    }
}
impl GoogleMotionDetectors {
    pub fn sheet(&self) -> &str {
        &self.sheet
    }
    pub fn range(&self) -> &str {
        &self.range
    }
    pub fn room_id(&self) -> &str {
        &self.room_id
    }
    pub fn device_address(&self) -> &str {
        &self.device_address
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn idx(&self) -> &str {
        &self.idx
    }
    pub fn state(&self) -> &str {
        &self.state
    }
}
impl GoogleRelay {
    pub fn sheet(&self) -> &str {
        &self.sheet
    }
    pub fn range(&self) -> &str {
        &self.range
    }
    pub fn room_id(&self) -> &str {
        &self.room_id
    }
    /*pub fn id(&self) -> &str {
        &self.id
    }*/
    pub fn device_address(&self) -> &str {
        &self.device_address
    }
    pub fn device_channel(&self) -> &str {
        &self.device_channel
    }
    pub fn temperature_sensor(&self) -> &str {
        &self.temperature_sensor
    }
    pub fn ring_button(&self) -> &str {
        &self.ring_button
    }

    pub fn idx(&self) -> &str {
        &self.idx
    }
    pub fn state(&self) -> &str {
        &self.state
    }
}

#[derive(Debug)]
pub struct Settings {
    pub server: ServerSettings,
    pub tinkerforge: Tinkerforge,
    pub google_sheet: Option<GoogleSheet>,
}

const DEFAULT_IP_ADDRESS: IpAddr = IpAddr::V6(Ipv6Addr::UNSPECIFIED);
impl ServerSettings {
    pub fn port(&self) -> u16 {
        self.port.unwrap_or(8080)
    }
    pub fn mgmt_port(&self) -> u16 {
        self.mgmt_port.unwrap_or_else(|| self.port() + 1000)
    }
    pub fn bind_address(&self) -> &IpAddr {
        self.bind_address.as_ref().unwrap_or(&DEFAULT_IP_ADDRESS)
    }
    pub fn setup_file(&self) -> &str {
        self.setup_file
            .as_ref()
            .map(Box::as_ref)
            .unwrap_or("setup.yaml")
    }
    pub fn state_file(&self) -> &str {
        self.state_file
            .as_ref()
            .map(Box::as_ref)
            .unwrap_or("state.ron")
    }
}
fn create_settings() -> Result<Settings, ConfigError> {
    let cfg = Config::builder()
        .add_source(File::with_name("config.yaml"))
        .add_source(
            Environment::with_prefix("APP")
                .separator("-")
                .prefix_separator("_"),
        )
        .build()?;
    Ok(Settings {
        server: cfg.get("server")?,
        tinkerforge: cfg.get("tinkerforge")?,
        google_sheet: cfg.get("google-sheet")?,
    })
}

lazy_static! {
    pub static ref CONFIG: Settings = create_settings().expect("Cannot load config.yaml");
}
