use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fmt::{Display, Formatter, Write};
use std::net::IpAddr;
use std::time::SystemTime;

use tinkerforge_async::base58::Base58Error;
use tinkerforge_async::{dmx_bricklet, io16_bricklet, io16_v2_bricklet};

use crate::data::Uid;

#[derive(Default, Debug)]
pub struct State {
    endpoints: HashMap<IpAddr, ConnectionData>,
    bricklets: HashMap<Uid, BrickletConnectionData>,
}

#[derive(Debug)]
pub enum StateUpdateMessage {
    EndpointConnected(IpAddr),
    EndpointDisconnected(IpAddr),
    BrickletConnected {
        uid: Uid,
        endpoint: IpAddr,
        metadata: BrickletMetadata,
    },
    BrickletDisconnected {
        uid: Uid,
        endpoint: IpAddr,
    },
}

impl TryFrom<(IpAddr, io16_bricklet::Identity)> for StateUpdateMessage {
    type Error = Base58Error;

    fn try_from((endpoint, id): (IpAddr, io16_bricklet::Identity)) -> Result<Self, Self::Error> {
        Ok(StateUpdateMessage::BrickletConnected {
            uid: id.uid.parse()?,
            endpoint,
            metadata: BrickletMetadata {
                connected_uid: id.connected_uid.parse()?,
                position: id.position,
                hardware_version: id.hardware_version,
                firmware_version: id.firmware_version,
            },
        })
    }
}
impl TryFrom<(IpAddr, io16_v2_bricklet::Identity)> for StateUpdateMessage {
    type Error = Base58Error;

    fn try_from((endpoint, id): (IpAddr, io16_v2_bricklet::Identity)) -> Result<Self, Self::Error> {
        Ok(StateUpdateMessage::BrickletConnected {
            uid: id.uid.parse()?,
            endpoint,
            metadata: BrickletMetadata {
                connected_uid: id.connected_uid.parse()?,
                position: id.position,
                hardware_version: id.hardware_version,
                firmware_version: id.firmware_version,
            },
        })
    }
}

impl TryFrom<(IpAddr, dmx_bricklet::Identity)> for StateUpdateMessage {
    type Error = Base58Error;

    fn try_from((endpoint, id): (IpAddr, dmx_bricklet::Identity)) -> Result<Self, Self::Error> {
        Ok(StateUpdateMessage::BrickletConnected {
            uid: id.uid.parse()?,
            endpoint,
            metadata: BrickletMetadata {
                connected_uid: id.connected_uid.parse()?,
                position: id.position,
                hardware_version: id.hardware_version,
                firmware_version: id.firmware_version,
            },
        })
    }
}

#[derive(Debug)]
pub struct ConnectionStateMsg<ID> {
    pub state: ConnectionState,
    pub id: ID,
}
#[derive(Debug)]
pub struct ConnectionData {
    pub state: ConnectionState,
    pub last_change: SystemTime,
}
#[derive(Debug)]
pub struct BrickletConnectionData {
    pub state: ConnectionState,
    pub last_change: SystemTime,
    pub endpoint: IpAddr,
    pub metadata: Option<BrickletMetadata>,
}
#[derive(Debug, Copy, Clone)]
pub struct BrickletMetadata {
    pub connected_uid: Uid,
    pub position: char,
    pub hardware_version: [u8; 3],
    pub firmware_version: [u8; 3],
}

#[derive(Debug, Eq, PartialEq)]
pub enum ConnectionState {
    Connected,
    Disconnected,
}

impl State {
    pub fn process_msg(&mut self, msg: StateUpdateMessage) {
        match msg {
            StateUpdateMessage::EndpointConnected(ip) => match self.endpoints.entry(ip) {
                Entry::Occupied(mut entry) => {
                    if entry.get().state != ConnectionState::Connected {
                        entry.insert(ConnectionData {
                            state: ConnectionState::Connected,
                            last_change: SystemTime::now(),
                        });
                    }
                }
                Entry::Vacant(new_entry) => {
                    new_entry.insert(ConnectionData {
                        state: ConnectionState::Connected,
                        last_change: SystemTime::now(),
                    });
                }
            },
            StateUpdateMessage::EndpointDisconnected(ip) => {
                if let Some(mut entry) = self.endpoints.get_mut(&ip) {
                    if entry.state != ConnectionState::Disconnected {
                        entry.state = ConnectionState::Disconnected;
                        entry.last_change = SystemTime::now();
                    }
                }
            }
            StateUpdateMessage::BrickletConnected {
                uid,
                endpoint,
                metadata,
            } => match self.bricklets.entry(uid) {
                Entry::Occupied(mut existing_entry) => {
                    if existing_entry.get().state != ConnectionState::Connected {
                        existing_entry.insert(BrickletConnectionData {
                            state: ConnectionState::Connected,
                            last_change: SystemTime::now(),
                            endpoint,
                            metadata: Some(metadata),
                        });
                    }
                }
                Entry::Vacant(new_entry) => {
                    new_entry.insert(BrickletConnectionData {
                        state: ConnectionState::Connected,
                        last_change: SystemTime::now(),
                        endpoint,
                        metadata: Some(metadata),
                    });
                }
            },
            StateUpdateMessage::BrickletDisconnected { uid, endpoint } => {
                if let Some(mut entry_data) = self.bricklets.get_mut(&uid) {
                    if entry_data.state != ConnectionState::Disconnected
                        && entry_data.endpoint == endpoint
                    {
                        entry_data.last_change = SystemTime::now();
                        entry_data.state = ConnectionState::Disconnected;
                    }
                }
            }
        }
    }
    pub fn endpoint(&self, ip: &IpAddr) -> Option<&ConnectionData> {
        self.endpoints.get(ip)
    }
    pub fn bricklet(&self, uid: &Uid) -> Option<&BrickletConnectionData> {
        self.bricklets.get(uid)
    }
}
impl Display for ConnectionState {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionState::Connected => f.write_str("Connected"),
            ConnectionState::Disconnected => f.write_str("Not Connected"),
        }
    }
}
