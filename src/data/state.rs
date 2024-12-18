use crate::shelly::shelly::ComponentEntry;
use std::{
    collections::{hash_map::Entry, HashMap},
    fmt::{Display, Formatter},
    net::IpAddr,
    time::SystemTime,
};

use crate::devices::shelly::common::DeviceId;
use tinkerforge_async::DeviceIdentifier;
use tinkerforge_async::{base58::Uid, ip_connection::Version};

#[derive(Default, Debug)]
pub struct State {
    endpoints: HashMap<IpAddr, ConnectionData>,
    bricklets: HashMap<Uid, BrickletConnectionData>,
    shelly_components: HashMap<IpAddr, (DeviceId, Vec<ComponentEntry>)>,
}

#[derive(Debug, Clone)]
pub enum StateUpdateMessage {
    EndpointConnected {
        address: IpAddr,
        hostname: Option<Box<str>>,
    },
    EndpointDisconnected(IpAddr),
    BrickletConnected {
        uid: Uid,
        endpoint: IpAddr,
        metadata: BrickletMetadata,
        session: u32,
    },
    BrickletDisconnected {
        uid: Uid,
        endpoint: IpAddr,
        session: u32,
    },
    SpitfpMetrics {
        uid: Uid,
        port: Option<char>,
        counters: SpitfpErrorCounters,
    },
    BrickletCommunicationFailed {
        uid: Uid,
    },
    ShellyComponentFound {
        id: DeviceId,
        endpoint: IpAddr,
        component: ComponentEntry,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpitfpErrorCounters {
    pub error_count_ack_checksum: u32,
    pub error_count_message_checksum: u32,
    pub error_count_frame: u32,
    pub error_count_overflow: u32,
}

#[derive(Debug)]
pub struct ConnectionStateMsg<ID> {
    pub state: ConnectionState,
    pub id: ID,
}

#[derive(Debug)]
pub struct ConnectionData {
    pub state: ConnectionState,
    pub hostname: Option<Box<str>>,
    pub last_change: SystemTime,
}

#[derive(Debug)]
pub struct BrickletConnectionData {
    pub state: ConnectionState,
    pub last_change: SystemTime,
    pub endpoint: IpAddr,
    pub metadata: Option<BrickletMetadata>,
    pub connection_failed_counter: u32,
    pub error_counters: HashMap<Option<char>, SpitfpErrorCounters>,
    pub session: u32,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BrickletMetadata {
    pub connected_uid: Uid,
    pub position: char,
    pub hardware_version: Version,
    pub firmware_version: Version,
    pub device_identifier: DeviceIdentifier,
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, Ord, PartialOrd)]
pub enum ConnectionState {
    Connected,
    Disconnected,
}

impl State {
    pub fn process_msg(&mut self, msg: StateUpdateMessage) -> bool {
        match msg {
            StateUpdateMessage::EndpointConnected { address, hostname } => {
                match self.endpoints.entry(address) {
                    Entry::Occupied(mut entry) => {
                        if entry.get().state != ConnectionState::Connected {
                            entry.insert(ConnectionData {
                                state: ConnectionState::Connected,
                                hostname,
                                last_change: SystemTime::now(),
                            });
                            true
                        } else {
                            false
                        }
                    }
                    Entry::Vacant(new_entry) => {
                        new_entry.insert(ConnectionData {
                            state: ConnectionState::Connected,
                            hostname,
                            last_change: SystemTime::now(),
                        });
                        true
                    }
                }
            }
            StateUpdateMessage::EndpointDisconnected(ip) => {
                //self.shelly_components.remove(&ip).is_some() ||
                if let Some(entry) = self.endpoints.get_mut(&ip) {
                    if entry.state != ConnectionState::Disconnected {
                        entry.state = ConnectionState::Disconnected;
                        entry.last_change = SystemTime::now();
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            StateUpdateMessage::BrickletConnected {
                uid,
                endpoint,
                metadata,
                session,
            } => match self.bricklets.entry(uid) {
                Entry::Occupied(mut existing_entry) => {
                    let entry_ref = existing_entry.get();
                    if entry_ref.state != ConnectionState::Connected
                        || entry_ref.metadata.as_ref() != Some(&metadata)
                        || entry_ref.endpoint != endpoint
                        || entry_ref.session != session
                    {
                        existing_entry.insert(BrickletConnectionData {
                            state: ConnectionState::Connected,
                            last_change: SystemTime::now(),
                            endpoint,
                            metadata: Some(metadata),
                            connection_failed_counter: entry_ref.connection_failed_counter + 1,
                            error_counters: Default::default(),
                            session,
                        });
                        true
                    } else {
                        false
                    }
                }
                Entry::Vacant(new_entry) => {
                    new_entry.insert(BrickletConnectionData {
                        state: ConnectionState::Connected,
                        last_change: SystemTime::now(),
                        endpoint,
                        metadata: Some(metadata),
                        connection_failed_counter: 0,
                        error_counters: Default::default(),
                        session,
                    });
                    true
                }
            },
            StateUpdateMessage::BrickletDisconnected {
                uid,
                endpoint,
                session,
            } => {
                if let Some(entry_data) = self.bricklets.get_mut(&uid) {
                    if entry_data.state != ConnectionState::Disconnected
                        && entry_data.endpoint == endpoint
                        && entry_data.session == session
                    {
                        entry_data.last_change = SystemTime::now();
                        entry_data.state = ConnectionState::Disconnected;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            StateUpdateMessage::SpitfpMetrics {
                uid,
                port,
                counters,
            } => {
                if let Some(entry_data) = self.bricklets.get_mut(&uid) {
                    if entry_data.error_counters.get(&port) == Some(&counters) {
                        false
                    } else {
                        entry_data.error_counters.insert(port, counters);
                        true
                    }
                } else {
                    false
                }
            }
            StateUpdateMessage::BrickletCommunicationFailed { uid } => {
                if let Some(entry_data) = self.bricklets.get_mut(&uid) {
                    entry_data.connection_failed_counter += 1;
                    true
                } else {
                    false
                }
            }
            StateUpdateMessage::ShellyComponentFound {
                id,
                endpoint,
                component,
            } => match self.shelly_components.entry(endpoint) {
                Entry::Occupied(mut entry) => {
                    let (entry_id, components) = entry.get_mut();
                    let key_modified = if *entry_id == id {
                        false
                    } else {
                        *entry_id = id;
                        true
                    };
                    key_modified
                        || if components.contains(&component) {
                            false
                        } else {
                            components.push(component);
                            true
                        }
                }
                Entry::Vacant(new_entry) => {
                    new_entry.insert((id, vec![component]));
                    true
                }
            },
        }
    }
    pub fn endpoint(&self, ip: &IpAddr) -> Option<&ConnectionData> {
        self.endpoints.get(ip)
    }
    pub fn bricklet(&self, uid: &Uid) -> Option<&BrickletConnectionData> {
        self.bricklets.get(uid)
    }
    pub fn bricklets(&self) -> &HashMap<Uid, BrickletConnectionData> {
        &self.bricklets
    }

    pub fn shelly_components(&self) -> &HashMap<IpAddr, (DeviceId, Vec<ComponentEntry>)> {
        &self.shelly_components
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
