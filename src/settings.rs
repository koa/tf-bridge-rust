use std::net::{IpAddr, Ipv6Addr};

use config::{Config, ConfigError, Environment, File};
use lazy_static::lazy_static;
use serde::Deserialize;

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

#[derive(Debug)]
pub struct Settings {
    pub server: ServerSettings,
    pub tinkerforge: Tinkerforge,
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
    })
}

lazy_static! {
    pub static ref CONFIG: Settings = create_settings().expect("Cannot load config.yaml");
}
