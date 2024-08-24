use std::{
    fmt::{Debug, Formatter},
    str::FromStr,
};

use crate::devices::shelly::{ble, cloud, eth, input, light, mqtt, switch};
use jsonrpsee::proc_macros::rpc;
use macaddr::MacAddr6;
use serde::{
    de::{Error, Visitor},
    Deserialize, Deserializer,
};
use serde_json::{value::RawValue, Value};

#[rpc(client)]
pub trait Shelly {
    #[method(name = "shelly.getdeviceinfo")]
    async fn get_deviceinfo(&self, ident: bool) -> Result<GetDeviceInfoResponse, ErrorObjectOwned>;
    #[method(name = "shelly.getcomponents")]
    async fn get_components(
        &self,
        offset: u16,
        dynamic_only: bool,
    ) -> Result<GetComponentsResponse, ErrorObjectOwned>;
    #[method(name = "shelly.getcomponents")]
    async fn get_components_string(
        &self,
        offset: u16,
        dynamic_only: bool,
    ) -> Result<Box<RawValue>, ErrorObjectOwned>;
}

#[derive(Deserialize, Debug)]
pub struct GetDeviceInfoResponse {
    name: Option<Box<str>>,
    id: DeviceId,
    model: Box<str>,
    gen: u8,
    fw_id: Box<str>,
    ver: Box<str>,
    app: Box<str>,
    profile: Option<Box<str>>,
    auth_en: bool,
    auth_domain: Option<Box<str>>,
    discoverable: Option<bool>,
    key: Option<Box<str>>,
    batch: Option<Box<str>>,
    fw_sbits: Option<Box<str>>,
}
#[derive(Deserialize, Debug)]
pub struct GetComponentsResponse {
    components: Box<[ComponentEntry]>,
    cfg_rev: u16,
    offset: u16,
    total: u16,
}

impl GetComponentsResponse {
    pub fn components(&self) -> &Box<[ComponentEntry]> {
        &self.components
    }
    pub fn cfg_rev(&self) -> u16 {
        self.cfg_rev
    }
    pub fn offset(&self) -> u16 {
        self.offset
    }
    pub fn total(&self) -> u16 {
        self.total
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum ComponentEntry {
    Input(input::Component),
    Ble(ble::Component),
    Cloud(cloud::Component),
    Eth(eth::Component),
    Light(light::Component),
    Mqtt(mqtt::Component),
    Switch(switch::Component),
}

#[derive(Clone, PartialEq, Copy)]
pub struct DeviceId {
    pub device_type: DeviceType,
    pub mac: MacAddr6,
}

#[derive(Clone, PartialEq, Copy)]
pub enum SwitchingKey {
    Switch(switch::Key),
    Light(light::Key),
}
#[derive(Clone, PartialEq, Copy)]
pub struct SwitchingKeyId {
    pub device: DeviceId,
    pub key: SwitchingKey,
}

impl Debug for DeviceId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.device_type.fmt(f)?;
        f.write_str("-")?;
        let bytes = self.mac.into_array();
        f.write_fmt(format_args!(
            "{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5],
        ))
    }
}

impl<'de> Deserialize<'de> for DeviceId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(DeviceIdVisitor)
    }
}

struct DeviceIdVisitor;
#[derive(Deserialize, Clone, PartialEq, Copy, Debug)]
pub enum DeviceType {
    #[serde(rename = "shellyprodm2pm")]
    ShellyProDm2Pm,
    #[serde(rename = "shellypro4pm")]
    ShellyPro4Pm,
}

impl<'de> Visitor<'de> for DeviceIdVisitor {
    type Value = DeviceId;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("parse shelly device id into a Copy-Struct")
    }

    fn visit_str<E>(self, v: &str) -> Result<DeviceId, E>
    where
        E: Error,
    {
        if let Some((type_str, mac_str)) = v.split_once("-") {
            println!("Type str: {type_str}");
            let device_type = serde_json::from_value(Value::from(type_str))
                .map_err(|e| Error::custom(format!("Cannot parse type value {type_str}: {e}")))?;
            let mac = MacAddr6::from_str(mac_str)
                .map_err(|e| Error::custom(format!("Cannot parse mac address {mac_str}: {e}")))?;
            Ok(DeviceId { device_type, mac })
        } else {
            Err(Error::custom(format!("Missing delimiter in {v}")))
        }
    }
}
#[cfg(test)]
mod test {
    use std::net::{IpAddr, Ipv4Addr};

    use chrono::{DateTime, Duration};
    use serde_json::json;

    use crate::{
        devices::shelly::{
            ble, cloud, common,
            common::{ButtonDoublePush, ButtonPresets, InitialState, InputMode, Temperature},
            eth, input, light,
            light::{NightMode, StatusFlags},
            mqtt,
            shelly::{ComponentEntry, GetComponentsResponse},
            switch,
        },
        shelly::common::{ActiveEnergy, LastCommandSource},
    };

    #[test]
    fn test_input() {
        let entry = serde_json::from_value::<ComponentEntry>(json!({
            "key": "input:1",
            "status": {
                "id": 1,
                "state": null
            },
            "config": {
                "id": 1,
                "name": null,
                "type": "button",
                "enable": true,
                "invert": false
            }
        }))
        .expect("Cannot parse Input Component");
        if let ComponentEntry::Input(input::Component {
            key,
            status,
            config,
        }) = entry
        {
            //assert_eq!(key.as_ref(), "input:1");
        } else {
            panic!("Wrong component: {entry:?}");
        }
    }
    #[test]
    fn test_ble() {
        let entry = serde_json::from_value::<ComponentEntry>(json!(                    {
            "key": "ble",
            "status": {},
            "config": {
                "enable": true,
                "rpc": {
                    "enable": true
                },
                "observer": {
                    "enable": false
                }
            }
        }))
        .expect("Cannot parse Input Component");
        if let ComponentEntry::Ble(ble::Component {
            key: _,
            status: _,
            config,
        }) = entry
        {
            let configuration = ble::Configuration {
                enable: true,
                rpc: ble::RpcConfiguration { enable: true },
                observer: ble::ObserverConfiguration { enable: false },
            };
            assert_eq!(config, configuration);
        } else {
            panic!("Wrong component: {entry:?}");
        }
    }
    #[test]
    fn test_cloud() {
        let entry = serde_json::from_value::<ComponentEntry>(
            json!(                                        {
                "key": "cloud",
                "status": {
                    "connected": false
                },
                "config": {
                    "enable": false,
                    "server": "iot.shelly.cloud:6012/jrpc"
                }
            }),
        )
        .expect("Cannot parse Input Component");
        if let ComponentEntry::Cloud(cloud::Component {
            key: _,
            status,
            config,
        }) = entry
        {
            assert_eq!(status, cloud::Status { connected: false });
            assert_eq!(
                config,
                cloud::Configuration {
                    enable: false,
                    server: "iot.shelly.cloud:6012/jrpc".into(),
                }
            );
        } else {
            panic!("Wrong component: {entry:?}");
        }
    }
    #[test]
    fn test_mqtt() {
        let entry = serde_json::from_value::<ComponentEntry>(json!(                    {
            "key": "mqtt",
            "status": {
                "connected": false
            },
            "config": {
                "enable": false,
                "server": null,
                "client_id": "shellypro4pm-34987a47a1dc",
                "user": null,
                "ssl_ca": null,
                "topic_prefix": "shellypro4pm-34987a47a1dc",
                "rpc_ntf": true,
                "status_ntf": false,
                "use_client_cert": false,
                "enable_rpc": true,
                "enable_control": true
            }
        }))
        .expect("Cannot parse Input Component");
        if let ComponentEntry::Mqtt(mqtt::Component {
            key: _,
            status,
            config,
        }) = entry
        {
            assert_eq!(status, mqtt::Status { connected: false });
            assert_eq!(
                config,
                mqtt::Configuration {
                    enable: false,
                    server: None,
                    client_id: Some("shellypro4pm-34987a47a1dc".into()),
                    user: None,
                    ssl_ca: None,
                    topic_prefix: Some("shellypro4pm-34987a47a1dc".into()),
                    rpc_ntf: true,
                    status_ntf: false,
                    use_client_cert: false,
                    enable_control: true,
                }
            );
        } else {
            panic!("Wrong component: {entry:?}");
        }
    }

    #[test]
    fn test_eth() {
        let entry = serde_json::from_value::<ComponentEntry>(
            json!(                                                            {
                "key": "eth",
                "status": {
                    "ip": "10.192.5.6"
                },
                "config": {
                    "enable": true,
                    "ipv4mode": "dhcp",
                    "ip": null,
                    "netmask": null,
                    "gw": null,
                    "nameserver": null
                }
            }),
        )
        .expect("Cannot parse Input Component");
        if let ComponentEntry::Eth(eth::Component {
            key: _,
            status,
            config,
        }) = entry
        {
            assert_eq!(
                status,
                eth::Status {
                    ip: Some(IpAddr::V4(Ipv4Addr::new(10, 192, 5, 6)))
                }
            );
            assert_eq!(
                config,
                eth::Configuration {
                    enable: true,
                    ipv4mode: eth::IPv4Mode::Dhcp,
                    ip: None,
                    netmask: None,
                    gw: None,
                    nameserver: None,
                }
            );
        } else {
            panic!("Wrong component: {entry:?}");
        }
    }
    #[test]
    fn test_light() {
        let entry = serde_json::from_value::<ComponentEntry>(json!(                    {
            "key": "light:0",
            "status": {
                "id": 0,
                "source": "WS_in",
                "output": false,
                "brightness": 100,
                "temperature": {
                    "tC": 30.5,
                    "tF": 86.8
                },
                "aenergy": {
                    "total": 0,
                    "by_minute": [
                        0,
                        0,
                        0
                    ],
                    "minute_ts": 1719028440
                },
                "apower": 0,
                "current": 0,
                "voltage": 232.7,
                "flags": [
                    "uncalibrated"
                ]
            },
            "config": {
                "id": 0,
                "name": null,
                "initial_state": "restore_last",
                "auto_on": false,
                "auto_on_delay": 60,
                "auto_off": false,
                "auto_off_delay": 60,
                "transition_duration": 3,
                "min_brightness_on_toggle": 3,
                "night_mode": {
                    "enable": false,
                    "brightness": 50,
                    "active_between": []
                },
                "button_fade_rate": 3,
                "button_presets": {
                    "button_doublepush": {
                        "brightness": 100
                    }
                },
                "in_mode": "dim",
                "current_limit": 1.22,
                "power_limit": 230,
                "undervoltage_limit": 200,
                "voltage_limit": 280
            }
        }))
        .expect("Cannot parse Light Component");
        if let ComponentEntry::Light(light::Component {
            key,
            status,
            config,
        }) = entry
        {
            assert_eq!(key, 0.into());
            assert_eq!(
                status,
                light::Status {
                    id: 0,
                    source: LastCommandSource::WsIn,
                    output: false,
                    brightness: 100.0,
                    timer_started_at: None,
                    timer_duration: None,
                    transition: None,
                    temperature: Some(Temperature {
                        temp_celsius: Some(30.5),
                        temp_fahrenheit: Some(86.8),
                    }),
                    active_energy: Some(ActiveEnergy {
                        total: 0.0,
                        by_minute: Some([0.0, 0.0, 0.0]),
                        minute_ts: DateTime::from_timestamp_nanos(1719028440 * 1000 * 1000 * 1000),
                    }),
                    active_power: Some(0.0),
                    voltage: Some(232.7),
                    current: Some(0.0),
                    calibration: None,
                    errors: None,
                    flags: Box::new([StatusFlags::Uncalibrated]),
                }
            );
            assert_eq!(
                config,
                light::Configuration {
                    id: 0,
                    name: None,
                    in_mode: Some(InputMode::Dim),
                    initial_state: InitialState::RestoreLast,
                    auto_on: false,
                    auto_on_delay: Duration::seconds(60),
                    auto_off: false,
                    auto_off_delay: Duration::seconds(60),
                    transition_duration: Duration::seconds(3),
                    min_brightness_on_toggle: 3.0,
                    night_mode: NightMode {
                        enable: false,
                        brightness: 50.0,
                        active_between: Box::new([]),
                    },
                    button_fade_rate: 3,
                    button_presets: ButtonPresets {
                        button_doublepush: Some(ButtonDoublePush { brightness: 100.0 })
                    },
                    range_map: None,
                    power_limit: Some(230),
                    voltage_limit: Some(280),
                    undervoltage_limit: Some(200),
                    current_limit: Some(1.22)
                }
            );
        } else {
            panic!("Wrong component: {entry:?}");
        }
    }

    #[test]
    fn test_switch() {
        let entry = serde_json::from_value::<ComponentEntry>(json!(                    {
            "key": "switch:1",
            "status": {
                "id": 1,
                "source": "WS_in",
                "output": false,
                "apower": 0,
                "voltage": 231.2,
                "freq": 50,
                "current": 0,
                "pf": 0,
                "aenergy": {
                    "total": 0,
                    "by_minute": [
                        0,
                        0,
                        0
                    ],
                    "minute_ts": 1719549120
                },
                "ret_aenergy": {
                    "total": 0,
                    "by_minute": [
                        0,
                        0,
                        0
                    ],
                    "minute_ts": 1719549120
                },
                "temperature": {
                    "tC": 31,
                    "tF": 87.7
                }
            },
            "config": {
                "id": 1,
                "name": null,
                "in_mode": "follow",
                "initial_state": "match_input",
                "auto_on": false,
                "auto_on_delay": 60,
                "auto_off": false,
                "auto_off_delay": 60,
                "power_limit": 4480,
                "voltage_limit": 280,
                "undervoltage_limit": 0,
                "autorecover_voltage_errors": false,
                "current_limit": 16
            }
        }))
        .expect("Cannot parse Switch Component");
        if let ComponentEntry::Switch(switch::Component {
            key,
            status,
            config,
        }) = entry
        {
            assert_eq!(key, 1.into());
            assert_eq!(
                status,
                switch::Status {
                    id: 1,
                    source: common::LastCommandSource::WsIn,
                    output: false,
                    timer_started_at: None,
                    timer_duration: None,
                    temperature: Some(common::Temperature {
                        temp_celsius: Some(31.0),
                        temp_fahrenheit: Some(87.7),
                    }),
                    active_energy: Some(ActiveEnergy {
                        total: 0.0,
                        by_minute: Some([0.0, 0.0, 0.0]),
                        minute_ts: DateTime::from_timestamp_nanos(1719549120 * 1000 * 1000 * 1000),
                    }),
                    active_power: Some(0.0),
                    voltage: Some(231.2),
                    current: Some(0.0),
                    power_factor: Some(0.0),
                    errors: None,
                    frequency: None,
                    returned_active_energy: Some(ActiveEnergy {
                        total: 0.0,
                        by_minute: Some([0.0, 0.0, 0.0]),
                        minute_ts: DateTime::from_timestamp_nanos(1719549120 * 1000 * 1000 * 1000),
                    }),
                }
            );
            assert_eq!(
                config,
                switch::Configuration {
                    id: 1,
                    name: None,
                    in_mode: Some(common::InputMode::Follow),
                    initial_state: common::InitialState::MatchInput,
                    auto_on: false,
                    auto_on_delay: Duration::seconds(60),
                    auto_off: false,
                    auto_off_delay: Duration::seconds(60),
                    autorecover_voltage_errors: Some(false),
                    input_id: None,
                    power_limit: Some(4480),
                    voltage_limit: Some(280),
                    undervoltage_limit: Some(0),
                    current_limit: Some(16.0)
                }
            );
        } else {
            panic!("Wrong component: {entry:?}");
        }
    }

    #[test]
    fn test_dimmer_2pm() {
        let result = serde_json::from_value::<GetComponentsResponse>(serde_json::json!({
                "components": [
                    {
                        "key": "ble",
                        "status": {},
                        "config": {
                            "enable": true,
                            "rpc": {
                                "enable": true
                            },
                            "observer": {
                                "enable": false
                            }
                        }
                    },
                    {
                        "key": "cloud",
                        "status": {
                            "connected": false
                        },
                        "config": {
                            "enable": false,
                            "server": "iot.shelly.cloud:6012/jrpc"
                        }
                    },
                    {
                        "key": "eth",
                        "status": {
                            "ip": "10.192.5.6"
                        },
                        "config": {
                            "enable": true,
                            "ipv4mode": "dhcp",
                            "ip": null,
                            "netmask": null,
                            "gw": null,
                            "nameserver": null
                        }
                    },
                    {
                        "key": "input:0",
                        "status": {
                            "id": 0,
                            "state": null
                        },
                        "config": {
                            "id": 0,
                            "name": null,
                            "type": "button",
                            "enable": true,
                            "invert": false
                        }
                    },
                    {
                        "key": "input:1",
                        "status": {
                            "id": 1,
                            "state": null
                        },
                        "config": {
                            "id": 1,
                            "name": null,
                            "type": "button",
                            "enable": true,
                            "invert": false
                        }
                    },
                    {
                        "key": "input:2",
                        "status": {
                            "id": 2,
                            "state": null
                        },
                        "config": {
                            "id": 2,
                            "name": null,
                            "type": "button",
                            "enable": true,
                            "invert": false
                        }
                    },
                    {
                        "key": "input:3",
                        "status": {
                            "id": 3,
                            "state": null
                        },
                        "config": {
                            "id": 3,
                            "name": null,
                            "type": "button",
                            "enable": true,
                            "invert": false
                        }
                    },
                    {
                        "key": "light:0",
                        "status": {
                            "id": 0,
                            "source": "WS_in",
                            "output": false,
                            "brightness": 100,
                            "temperature": {
                                "tC": 30.5,
                                "tF": 86.8
                            },
                            "aenergy": {
                                "total": 0,
                                "by_minute": [
                                    0,
                                    0,
                                    0
                                ],
                                "minute_ts": 1719028440
                            },
                            "apower": 0,
                            "current": 0,
                            "voltage": 232.7,
                            "flags": [
                                "uncalibrated"
                            ]
                        },
                        "config": {
                            "id": 0,
                            "name": null,
                            "initial_state": "restore_last",
                            "auto_on": false,
                            "auto_on_delay": 60,
                            "auto_off": false,
                            "auto_off_delay": 60,
                            "transition_duration": 3,
                            "min_brightness_on_toggle": 3,
                            "night_mode": {
                                "enable": false,
                                "brightness": 50,
                                "active_between": []
                            },
                            "button_fade_rate": 3,
                            "button_presets": {
                                "button_doublepush": {
                                    "brightness": 100
                                }
                            },
                            "in_mode": "dim",
                            "current_limit": 1.22,
                            "power_limit": 230,
                            "undervoltage_limit": 200,
                            "voltage_limit": 280
                        }
                    },
                    {
                        "key": "light:1",
                        "status": {
                            "id": 1,
                            "source": "init",
                            "output": false,
                            "brightness": 100,
                            "temperature": {
                                "tC": 34.9,
                                "tF": 94.9
                            },
                            "aenergy": {
                                "total": 0,
                                "by_minute": [
                                    0,
                                    0,
                                    0
                                ],
                                "minute_ts": 1719028440
                            },
                            "apower": 0,
                            "current": 0,
                            "voltage": 232.7,
                            "flags": [
                                "uncalibrated"
                            ]
                        },
                        "config": {
                            "id": 1,
                            "name": null,
                            "initial_state": "restore_last",
                            "auto_on": false,
                            "auto_on_delay": 60,
                            "auto_off": false,
                            "auto_off_delay": 60,
                            "transition_duration": 3,
                            "min_brightness_on_toggle": 3,
                            "night_mode": {
                                "enable": false,
                                "brightness": 50,
                                "active_between": []
                            },
                            "button_fade_rate": 3,
                            "button_presets": {
                                "button_doublepush": {
                                    "brightness": 100
                                }
                            },
                            "in_mode": "dim",
                            "current_limit": 1.22,
                            "power_limit": 230,
                            "undervoltage_limit": 200,
                            "voltage_limit": 280
                        }
                    }
                ],
                "cfg_rev": 4,
                "offset": 0,
                "total": 13
        }))
        .expect("Cannot parse 2pm components");
    }
    #[test]
    fn test_4pm() {
        let result = serde_json::from_value::<GetComponentsResponse>(serde_json::json!({

                "components": [
                    {
                        "key": "ble",
                        "status": {},
                        "config": {
                            "enable": true,
                            "rpc": {
                                "enable": true
                            },
                            "observer": {
                                "enable": false
                            }
                        }
                    },
                    {
                        "key": "cloud",
                        "status": {
                            "connected": false
                        },
                        "config": {
                            "enable": false,
                            "server": "iot.shelly.cloud:6012/jrpc"
                        }
                    },
                    {
                        "key": "eth",
                        "status": {
                            "ip": "10.192.5.8"
                        },
                        "config": {
                            "enable": true,
                            "ipv4mode": "dhcp",
                            "ip": null,
                            "netmask": null,
                            "gw": null,
                            "nameserver": null
                        }
                    },
                    {
                        "key": "input:0",
                        "status": {
                            "id": 0,
                            "state": false
                        },
                        "config": {
                            "id": 0,
                            "name": null,
                            "type": "switch",
                            "enable": true,
                            "invert": false
                        }
                    },
                    {
                        "key": "input:1",
                        "status": {
                            "id": 1,
                            "state": false
                        },
                        "config": {
                            "id": 1,
                            "name": null,
                            "type": "switch",
                            "enable": true,
                            "invert": false
                        }
                    },
                    {
                        "key": "input:2",
                        "status": {
                            "id": 2,
                            "state": false
                        },
                        "config": {
                            "id": 2,
                            "name": null,
                            "type": "switch",
                            "enable": true,
                            "invert": false
                        }
                    },
                    {
                        "key": "input:3",
                        "status": {
                            "id": 3,
                            "state": false
                        },
                        "config": {
                            "id": 3,
                            "name": null,
                            "type": "switch",
                            "enable": true,
                            "invert": false
                        }
                    },
                    {
                        "key": "mqtt",
                        "status": {
                            "connected": false
                        },
                        "config": {
                            "enable": false,
                            "server": null,
                            "client_id": "shellypro4pm-34987a47a1dc",
                            "user": null,
                            "ssl_ca": null,
                            "topic_prefix": "shellypro4pm-34987a47a1dc",
                            "rpc_ntf": true,
                            "status_ntf": false,
                            "use_client_cert": false,
                            "enable_rpc": true,
                            "enable_control": true
                        }
                    },
                    {
                        "key": "switch:0",
                        "status": {
                            "id": 0,
                            "source": "init",
                            "output": false,
                            "apower": 0,
                            "voltage": 231.3,
                            "freq": 50,
                            "current": 0,
                            "pf": 0,
                            "aenergy": {
                                "total": 0,
                                "by_minute": [
                                    0,
                                    0,
                                    0
                                ],
                                "minute_ts": 1719549120
                            },
                            "ret_aenergy": {
                                "total": 0,
                                "by_minute": [
                                    0,
                                    0,
                                    0
                                ],
                                "minute_ts": 1719549120
                            },
                            "temperature": {
                                "tC": 31,
                                "tF": 87.7
                            }
                        },
                        "config": {
                            "id": 0,
                            "name": null,
                            "in_mode": "follow",
                            "initial_state": "match_input",
                            "auto_on": false,
                            "auto_on_delay": 60,
                            "auto_off": false,
                            "auto_off_delay": 60,
                            "power_limit": 4480,
                            "voltage_limit": 280,
                            "undervoltage_limit": 0,
                            "autorecover_voltage_errors": false,
                            "current_limit": 16
                        }
                    },
                    {
                        "key": "switch:1",
                        "status": {
                            "id": 1,
                            "source": "WS_in",
                            "output": false,
                            "apower": 0,
                            "voltage": 231.2,
                            "freq": 50,
                            "current": 0,
                            "pf": 0,
                            "aenergy": {
                                "total": 0,
                                "by_minute": [
                                    0,
                                    0,
                                    0
                                ],
                                "minute_ts": 1719549120
                            },
                            "ret_aenergy": {
                                "total": 0,
                                "by_minute": [
                                    0,
                                    0,
                                    0
                                ],
                                "minute_ts": 1719549120
                            },
                            "temperature": {
                                "tC": 31,
                                "tF": 87.7
                            }
                        },
                        "config": {
                            "id": 1,
                            "name": null,
                            "in_mode": "follow",
                            "initial_state": "match_input",
                            "auto_on": false,
                            "auto_on_delay": 60,
                            "auto_off": false,
                            "auto_off_delay": 60,
                            "power_limit": 4480,
                            "voltage_limit": 280,
                            "undervoltage_limit": 0,
                            "autorecover_voltage_errors": false,
                            "current_limit": 16
                        }
                    }
                ],
                "cfg_rev": 3,
                "offset": 0,
                "total": 16
        }))
        .expect("Cannot parse 4pm components");
    }
    #[test]
    fn test_dimmer_2pm_1() {
        let result = serde_json::from_value::<GetComponentsResponse>(json!({"components":[{"key":"ble","status":{},"config":{"enable":true,"rpc":{"enable":true},"observer":{"enable":false}}},{"key":"cloud","status":{"connected":false},"config":{"enable":false,"server":"iot.shelly.cloud:6012/jrpc"}},{"key":"eth","status":{"ip":"10.192.5.6"},"config":{"enable":true,"ipv4mode":"dhcp","ip":null,"netmask":null,"gw":null,"nameserver":null}},{"key":"input:0","status":{"id":0,"state":null},"config":{"id":0,"name":null,"type":"button","enable":true,"invert":false}},{"key":"input:1","status":{"id":1,"state":null},"config":{"id":1,"name":null,"type":"button","enable":true,"invert":false}},{"key":"input:2","status":{"id":2,"state":null},"config":{"id":2,"name":null,"type":"button","enable":true,"invert":false}},{"key":"input:3","status":{"id":3,"state":null},"config":{"id":3,"name":null,"type":"button","enable":true,"invert":false}},{"key":"light:0","status":{"id":0,"source":"WS_in","output":false,"brightness":100.0,"temperature":{"tC":34.4, "tF":93.9},"aenergy":{"total":0.000,"by_minute":[0.000,0.000,0.000],"minute_ts":1719554760},"apower":0.0,"current":0.000,"voltage":231.6,"flags":["uncalibrated"]},"config":{"id":0, "name":null,"initial_state":"restore_last", "auto_on":false,"auto_on_delay":60.00, "auto_off":false,"auto_off_delay": 60.00,"transition_duration":3.00,"min_brightness_on_toggle":3.00,"night_mode":{"enable":false,"brightness":50.0,"active_between":[]},"button_fade_rate":3,"button_presets":{"button_doublepush":{"brightness":100.0}},"in_mode":"dim","current_limit":1.220,"power_limit":230,"undervoltage_limit":200,"voltage_limit":280}},{"key":"light:1","status":{"id":1,"source":"init","output":false,"brightness":100.0,"temperature":{"tC":38.6, "tF":101.5},"aenergy":{"total":0.000,"by_minute":[0.000,0.000,0.000],"minute_ts":1719554760},"apower":0.0,"current":0.000,"voltage":231.5,"flags":["uncalibrated"]},"config":{"id":1, "name":null,"initial_state":"restore_last", "auto_on":false,"auto_on_delay":60.00, "auto_off":false,"auto_off_delay": 60.00,"transition_duration":3.00,"min_brightness_on_toggle":3.00,"night_mode":{"enable":false,"brightness":50.0,"active_between":[]},"button_fade_rate":3,"button_presets":{"button_doublepush":{"brightness":100.0}},"in_mode":"dim","current_limit":1.220,"power_limit":230,"undervoltage_limit":200,"voltage_limit":280}}],"cfg_rev":4,"offset":0,"total":13}))
            .expect("Cannot parse dimmer 2pm components");
    }

    #[test]
    fn test_components() {
        let components_json = json!({
            "components": [{"config":{"enable":true,"observer":{"enable":false},"rpc":{"enable":true}},"key":"ble","status":{}},{"config":{"enable":false,"server":"iot.shelly.cloud:6012/jrpc"},"key":"cloud","status":{"connected":false}},{"config":{"enable":true,"gw":null,"ip":null,"ipv4mode":"dhcp","nameserver":null,"netmask":null},"key":"eth","status":{"ip":"10.192.5.6"}},{"config":{"enable":true,"id":0,"invert":false,"name":null,"type":"button"},"key":"input:0","status":{"id":0,"state":null}},{"config":{"enable":true,"id":1,"invert":false,"name":null,"type":"button"},"key":"input:1","status":{"id":1,"state":null}},{"config":{"enable":true,"id":2,"invert":false,"name":null,"type":"button"},"key":"input:2","status":{"id":2,"state":null}},{"config":{"enable":true,"id":3,"invert":false,"name":null,"type":"button"},"key":"input:3","status":{"id":3,"state":null}},{"config":{"auto_off":false,"auto_off_delay":60.0,"auto_on":false,"auto_on_delay":60.0,"button_fade_rate":3,"button_presets":{"button_doublepush":{"brightness":100.0}},"current_limit":1.22,"id":0,"in_mode":"dim","initial_state":"restore_last","min_brightness_on_toggle":3.0,"name":null,"night_mode":{"active_between":[],"brightness":50.0,"enable":false},"power_limit":230,"transition_duration":3.0,"undervoltage_limit":200,"voltage_limit":280},"key":"light:0","status":{"aenergy":{"by_minute":[0.0,0.0,0.0],"minute_ts":1724401200,"total":0.0},"apower":0.0,"brightness":1.0,"current":0.0,"flags":["uncalibrated"],"id":0,"output":false,"source":"","temperature":{"tC":31.6,"tF":88.9},"voltage":233.8}},{"config":{"auto_off":false,"auto_off_delay":60.0,"auto_on":false,"auto_on_delay":60.0,"button_fade_rate":3,"button_presets":{"button_doublepush":{"brightness":100.0}},"current_limit":1.22,"id":1,"in_mode":"dim","initial_state":"restore_last","min_brightness_on_toggle":3.0,"name":null,"night_mode":{"active_between":[],"brightness":50.0,"enable":false},"power_limit":230,"transition_duration":3.0,"undervoltage_limit":200,"voltage_limit":280},"key":"light:1","status":{"aenergy":{"by_minute":[0.0,0.0,0.0],"minute_ts":1724401200,"total":0.0},"apower":0.0,"brightness":1.0,"current":0.0,"flags":["uncalibrated"],"id":1,"output":false,"source":"","temperature":{"tC":35.5,"tF":95.9},"voltage":233.8}}]        });
        for component in components_json
            .get("components")
            .and_then(|v| v.as_array())
            .iter()
            .flat_map(|v| v.iter())
        {
            let component_id = component
                .get("key")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let parsed = serde_json::from_value::<ComponentEntry>(component.clone());
            if let Err(error) = parsed {
                panic!("Cannot parse component {component_id}: {component}: {error}");
            }
        }
    }
    #[test]
    fn test_parse_switch() {
        serde_json::from_value::<switch::Component>(json!(                {
            "key": "switch:0",
            "status": {
                "id": 0,
                "source": "UI",
                "output": false,
                "apower": 0,
                "voltage": 233.1,
                "freq": 50,
                "current": 0,
                "pf": 0,
                "aenergy": {
                    "total": 0,
                    "by_minute": [
                        0,
                        0,
                        0
                    ],
                    "minute_ts": 1720259700
                },
                "ret_aenergy": {
                    "total": 0,
                    "by_minute": [
                        0,
                        0,
                        0
                    ],
                    "minute_ts": 1720259700
                },
                "temperature": {
                    "tC": 39.8,
                    "tF": 103.6
                }
            },
            "config": {
                "id": 0,
                "name": null,
                "in_mode": "follow",
                "initial_state": "match_input",
                "auto_on": false,
                "auto_on_delay": 60,
                "auto_off": false,
                "auto_off_delay": 60,
                "power_limit": 4480,
                "voltage_limit": 280,
                "undervoltage_limit": 0,
                "autorecover_voltage_errors": false,
                "current_limit": 16
            }
        }))
        .expect("Cannot parse switch");
    }
}
