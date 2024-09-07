use macaddr::MacAddr6;
use serde::de::{Error, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::cmp::Ordering;
use std::fmt::{Display, Formatter, Write};
use std::marker::PhantomData;
use std::num::ParseIntError;
use std::ops::Deref;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Clone, Hash)]
pub struct SerdeStringKey<K: PrefixedKey>(K);

#[derive(Debug, Clone, Copy, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct SerializableMacAddress(MacAddr6);

impl<K: PrefixedKey + PartialOrd> PartialOrd for SerdeStringKey<K> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

impl<K: PrefixedKey + Eq> Eq for SerdeStringKey<K> {}

impl<K: PrefixedKey + Ord> Ord for SerdeStringKey<K> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl<K: Copy + PrefixedKey> Copy for SerdeStringKey<K> {}
impl<K: PartialEq + PrefixedKey> PartialEq for SerdeStringKey<K> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}
impl<K: PrefixedKey> Deref for SerdeStringKey<K> {
    type Target = K;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub trait PrefixedKey: From<u16> + Into<u16> + Clone {
    fn prefix() -> &'static str;
}

#[derive(Debug, Error)]
pub enum ParseKeyError {
    #[error("Invalid prefix on key")]
    InvalidPrefix,
    #[error("Cannot parse number of key {0}")]
    CannotParseNumber(#[from] ParseIntError),
}
pub fn parse_key(prefix: &str, key: &str) -> Result<u16, ParseKeyError> {
    if let Some(suffix) = key.strip_prefix(prefix) {
        Ok(suffix.parse()?)
    } else {
        Err(ParseKeyError::InvalidPrefix)
    }
}

impl<K: PrefixedKey> FromStr for SerdeStringKey<K> {
    type Err = ParseKeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(SerdeStringKey(parse_key(K::prefix(), s)?.into()))
    }
}
impl<K: PrefixedKey> From<u16> for SerdeStringKey<K> {
    fn from(value: u16) -> Self {
        SerdeStringKey(value.into())
    }
}
impl<K: PrefixedKey> From<SerdeStringKey<K>> for u16 {
    fn from(value: SerdeStringKey<K>) -> Self {
        value.0.into()
    }
}

impl<K: PrefixedKey> Display for SerdeStringKey<K> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(K::prefix())?;
        self.0.clone().into().fmt(f)
    }
}

impl<'de, K: PrefixedKey> Deserialize<'de> for SerdeStringKey<K> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(KeyVisitor(PhantomData::<K>::default()))
    }
}
impl<K: PrefixedKey> Serialize for SerdeStringKey<K> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

struct KeyVisitor<K: PrefixedKey>(PhantomData<K>);

impl<'de, K: PrefixedKey> Visitor<'de> for KeyVisitor<K> {
    type Value = SerdeStringKey<K>;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("a key beginning with '")?;
        formatter.write_str(K::prefix())?;
        formatter.write_str("' followed by a u16")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: Error,
    {
        Ok(SerdeStringKey(
            parse_key(K::prefix(), v)
                .map_err(|e| Error::custom(e))?
                .into(),
        ))
    }
}

impl<'de> Deserialize<'de> for SerializableMacAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(MacVisitor)
    }
}

struct MacVisitor;
impl Visitor<'_> for MacVisitor {
    type Value = SerializableMacAddress;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("a MAC address")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: Error,
    {
        Ok(SerializableMacAddress(
            MacAddr6::from_str(v).map_err(|e| Error::custom(e))?,
        ))
    }
}
