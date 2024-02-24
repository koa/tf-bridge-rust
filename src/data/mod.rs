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

pub(crate) mod google_data;
mod register;
pub mod registry;
pub mod settings;
pub mod state;
pub mod wiring;

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Ord, PartialOrd)]
pub struct Room {
    pub floor: i16,
    pub room: u16,
}

#[derive(
    Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize, Ord, PartialOrd,
)]
pub struct DeviceInRoom {
    pub room: Room,
    pub idx: u16,
}

#[derive(
    Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize, Ord, PartialOrd,
)]
pub struct SubDeviceInRoom {
    pub room: Room,
    pub device_idx: u16,
    pub sub_device_idx: u16,
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
