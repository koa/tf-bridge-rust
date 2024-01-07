use std::{
    fmt::{Display, Formatter},
    num::ParseIntError,
    str::FromStr,
};

use serde::{
    de::{Error, Visitor},
    Deserialize, Deserializer, Serialize, Serializer,
};
use thiserror::Error;
use tinkerforge_async::base58::{u32_to_base58, Base58, Base58Error};

pub(crate) mod google_data;
mod register;
pub mod registry;
pub mod settings;
pub mod wiring;

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Ord, PartialOrd)]
pub struct Room {
    pub floor: i16,
    pub room: u16,
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Ord, PartialOrd)]
pub struct Uid(u32);

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize, Ord, PartialOrd)]
pub struct DeviceInRoom {
    pub room: Room,
    pub idx: u16,
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize, Ord, PartialOrd)]
pub struct SubDeviceInRoom {
    pub room: Room,
    pub device_idx: u16,
    pub sub_device_idx: u16,
}

impl From<u32> for Uid {
    fn from(value: u32) -> Self {
        Uid(value)
    }
}

impl From<Uid> for u32 {
    fn from(value: Uid) -> Self {
        value.0
    }
}
impl FromStr for Uid {
    type Err = Base58Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.base58_to_u32()?))
    }
}
impl Display for Uid {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&u32_to_base58(self.0))
    }
}

struct UidVisitor;
impl<'de> Visitor<'de> for UidVisitor {
    type Value = Uid;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("a string a tinkerforge uid in base58 format")
    }
    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: Error,
    {
        Uid::from_str(v).map_err(|error| E::custom(format!("Cannot parse {v} as uid: {error}")))
    }
}

impl<'de> Deserialize<'de> for Uid {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(UidVisitor)
    }
}
impl Serialize for Uid {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl Display for Room {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.floor, self.room)
    }
}
struct RoomVisitor;
impl<'de> Visitor<'de> for RoomVisitor {
    type Value = Room;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("a string containing floor and room (delimited by \".\")")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: Error,
    {
        Room::from_str(v).map_err(|error| E::custom(format!("Cannot parse {v} as floor: {error}")))
    }
}
impl<'de> Deserialize<'de> for Room {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(RoomVisitor)
    }
}

impl Serialize for Room {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(Error, Debug)]
pub enum RoomParseError {
    #[error("Missing dot separator at {0}")]
    MissingDotSeparator(Box<str>),
    #[error("Cannot parse room: {0}")]
    RoomId(ParseIntError),
    #[error("Cannot parse floor: {0}")]
    FloorId(ParseIntError),
}

impl FromStr for Room {
    type Err = RoomParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let separator_pos = s
            .find('.')
            .ok_or_else(|| RoomParseError::MissingDotSeparator(s.into()))?;
        let floor = s[0..separator_pos]
            .parse()
            .map_err(RoomParseError::FloorId)?;
        let room = s[separator_pos + 1..]
            .parse()
            .map_err(RoomParseError::RoomId)?;
        Ok(Room { floor, room })
    }
}

#[cfg(test)]
mod test {
    use crate::data::{Room, SubDeviceInRoom};

    #[test]
    fn test_serialize_room() {
        let room: Room = "2.4".parse().unwrap();
        println!("Room: {room:?}");
        let device_in_room = SubDeviceInRoom {
            room,
            device_idx: 1,
            sub_device_idx: 2,
        };
        let yaml_string = serde_yaml::to_string(&device_in_room).unwrap();
        println!("{}", yaml_string);
        let parsed: SubDeviceInRoom = serde_yaml::from_str(&yaml_string).unwrap();
        println!("Parsed: {parsed:?}");
        assert_eq!(device_in_room, parsed);
    }
}
