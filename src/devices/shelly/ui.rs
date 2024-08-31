use serde::Deserialize;

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Component {
    pub key: Key,
    pub status: Status,
    pub config: Configuration,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Status {}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Configuration {
    pub idle_brightness: u8,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum Key {
    #[serde(rename = "ui")]
    Ui,
}
