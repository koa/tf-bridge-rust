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
}

#[derive(Deserialize, Debug)]
pub struct Tinkerforge {
    endpoints: Box<[TinkerforgeEndpoint]>,
}

impl Tinkerforge {
    pub fn endpoints(&self) -> &[TinkerforgeEndpoint] {
        &self.endpoints
    }
}

#[derive(Deserialize, Debug)]
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
    key_file: Option<String>,
    key_data: Option<String>,
    spreadsheet_id: String,
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
            let result = read_service_account_key(filename).await;
            match result {
                Ok(key) => Ok(key),
                Err(error) => {
                    warn!("Cannot load file {filename}: {error}");
                    Err(error.into())
                }
            }
        } else if let Some(data) = &self.key_data {
            Ok(parse_service_account_key(data)?)
        } else {
            Err(GoogleError::ConfigContent {
                description: "neither key_file nor key_data filled in",
            })
        }
    }

    pub fn spreadsheet_id(&self) -> &str {
        &self.spreadsheet_id
    }
}

#[derive(Debug)]
pub struct Settings {
    pub server: ServerSettings,
    pub tinkerforge: Tinkerforge,
    pub google_sheet: GoogleSheet,
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
