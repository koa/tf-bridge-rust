use std::net::{IpAddr, Ipv6Addr};
use serde::Deserialize;
use lazy_static::lazy_static;
use config::{Config, ConfigError, Environment, File};
#[derive(Deserialize)]
pub struct ServerSettings {
    port: Option<u16>,
    mgmt_port: Option<u16>,
    bind_address: Option<IpAddr>,
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
fn create_settings() -> Result<ServerSettings,ConfigError> {
    let cfg = Config::builder()
        .add_source(File::with_name("config.yaml"))
        .add_source(
            Environment::with_prefix("APP")
                .separator("-")
                .prefix_separator("_"),
        )
        .build()?;
    let server: ServerSettings = cfg.get("server")?;
    Ok(server)
}

lazy_static! {
    pub static ref CONFIG: ServerSettings = create_settings().expect("Cannot load config.yaml");
}