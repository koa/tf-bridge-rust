use chrono::{DateTime, Duration, Utc};
use google_sheets4::client::serde_with::serde_as;
use serde::{Deserialize, Serialize};
use serde_with::{formats::Flexible, DurationSeconds, TimestampSeconds};

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Component {
    pub key: Key,
    pub status: Status,
    pub config: Configuration,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Status {
    pub discovery: Option<BtHomeDiscovery>,
    pub errors: Box<[BtHomeError]>,
}
#[serde_as]
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct BtHomeDiscovery {
    #[serde_as(as = "Option<TimestampSeconds<String, Flexible>>")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde_as(as = "Option<DurationSeconds<String, Flexible>>")]
    pub duration: Option<Duration>,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BtHomeError {
    ObserverDisabled,
    BluetoothDisabled,
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Configuration {}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Copy, Hash, Eq, Ord, PartialOrd)]
pub enum Key {
    #[serde(rename = "bthome")]
    Bthome,
}
