use serde::Deserialize;

use crate::serde::{PrefixedKey, SerdeStringKey};

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Component {
    pub key: Key,
    pub status: Status,
    pub config: Configuration,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Status {
    pub id: u16,
    pub state: Option<bool>,
    pub percent: Option<u8>,
    pub xpercent: Option<u8>,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Configuration {
    pub id: u16,
    pub name: Option<Box<str>>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct InputKey {
    pub id: u16,
}

impl From<u16> for InputKey {
    fn from(id: u16) -> Self {
        InputKey { id }
    }
}
impl From<InputKey> for u16 {
    fn from(value: InputKey) -> Self {
        value.id
    }
}

impl PrefixedKey for InputKey {
    fn prefix() -> &'static str {
        KEY_PREFIX
    }
}

pub type Key = SerdeStringKey<InputKey>;

const KEY_PREFIX: &str = "input:";
