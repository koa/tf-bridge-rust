use std::num::ParseIntError;
use std::str::FromStr;

use thiserror::Error;

mod register;
pub mod registry;
pub mod settings;

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct Room {
    pub floor: i16,
    pub room: u16,
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

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct DeviceInRoom {
    pub room: Room,
    pub idx: u16,
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct SubDeviceInRoom {
    pub room: Room,
    pub device_idx: u16,
    pub sub_device_idx: u16,
}
