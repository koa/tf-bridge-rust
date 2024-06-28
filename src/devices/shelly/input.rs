use std::fmt::Formatter;

use serde::{
    de::{Error, Visitor},
    Deserialize, Deserializer,
};

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
pub struct Key {
    pub id: u16,
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(KeyVisitor)
    }
}

struct KeyVisitor;

const KEY_PREFIX: &str = "input:";

impl<'de> Visitor<'de> for KeyVisitor {
    type Value = Key;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("a input key beginning with 'input:' followed by a u16")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: Error,
    {
        if let Some(id_str) = v.strip_prefix(KEY_PREFIX) {
            let id: u16 = id_str.parse().map_err(|e| Error::custom(e))?;
            Ok(Key { id })
        } else {
            Err(Error::custom(format!("wrong prefix: {v}")))
        }
    }
}
