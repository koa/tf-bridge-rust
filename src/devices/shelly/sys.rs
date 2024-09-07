use crate::serde::SerializableMacAddress;
use chrono::{DateTime, Duration, Utc};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_with::{formats::Flexible, serde_as, DurationSeconds, TimestampSeconds};

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Component {
    pub key: Key,
    pub status: Status,
    pub config: Configuration,
}
#[serde_as]
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Status {
    pub mac: SerializableMacAddress,
    pub restart_required: bool,
    #[serde_as(as = "Option<TimestampSeconds<String, Flexible>>")]
    pub unixtime: Option<DateTime<Utc>>,
    #[serde_as(as = "Option<DurationSeconds<String, Flexible>>")]
    pub uptime: Option<Duration>,
    pub ram_size: u32,
    pub ram_free: u32,
    pub fs_size: u32,
    pub fs_free: u32,
    pub cfg_rev: u32,
    pub kvs_rev: u32,
    pub schedule_rev: Option<u32>,
    pub webhook_rev: Option<u32>,
    pub knx_rev: Option<u32>,
    pub available_updates: AvailableUpdates,
    pub wakeup_reason: Option<WakeupReason>,
    #[serde_as(as = "Option<DurationSeconds<String, Flexible>>")]
    pub wakeup_period: Option<Duration>,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct WakeupReason {
    boot: BootType,
    cause: BootCause,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum BootType {
    Poweron,
    SoftwareRestart,
    DeepsleepWake,
    Internal,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum BootCause {
    Button,
    Usb,
    Periodic,
    StatusUpdate,
    Alarm,
    AlarmTest,
    Undefined,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct AvailableUpdates {
    beta: Option<FirmwareVersion>,
    stable: Option<FirmwareVersion>,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct FirmwareVersion {
    pub version: Version,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Configuration {
    pub device: DeviceConfiguration,
    pub location: DeviceLocation,
    pub debug: DeviceDebug,
    pub rpc_udp: DeviceRpcUdp,
    pub sntp: DeviceSntpServer,
    pub cfg_rev: u32,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct DeviceSntpServer {
    pub server: Box<str>,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct DeviceLocation {
    pub tz: Option<Box<str>>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct DeviceConfiguration {
    pub name: Option<Box<str>>,
    pub eco_mode: Option<bool>,
    pub mac: SerializableMacAddress,
    pub fw_id: Box<str>,
    pub profile: Option<Box<str>>,
    pub discoverable: bool,
    pub addon_type: Option<DeviceAddonType>,
    pub sys_btn_toggle: Option<bool>,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct DeviceDebug {
    pub mqtt: DeviceDebugChannel,
    pub websocket: DeviceDebugChannel,
    pub udp: DeviceDebugUdp,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct DeviceRpcUdp {
    pub dst_addr: Option<Box<str>>,
    pub listen_port: Option<u16>,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct DeviceDebugUdp {
    pub addr: Option<Box<str>>,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct DeviceDebugChannel {
    pub enable: bool,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum DeviceAddonType {
    Sensor,
    Prooutput,
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Copy, Hash, Eq, Ord, PartialOrd)]
pub enum Key {
    #[serde(rename = "sys")]
    Sys,
}
