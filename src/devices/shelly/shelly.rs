use jsonrpsee::proc_macros::rpc;
use serde::Deserialize;

use crate::devices::shelly::{ble, cloud, eth, input, light, mqtt, switch};

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
}

#[derive(Deserialize, Debug)]
pub struct GetDeviceInfoResponse {
    name: Option<Box<str>>,
    id: Box<str>,
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

#[cfg(test)]
mod test {
    use std::net::{IpAddr, Ipv4Addr};

    use chrono::{DateTime, Duration};
    use serde_json::json;

    use crate::devices::shelly::{
        ble, cloud, eth, input, light, mqtt,
        shelly::{ComponentEntry, GetComponentsResponse},
        switch,
    };
    use crate::devices::shelly::light::{
        ActiveEnergy, ButtonDoublePush, ButtonPresets, InitialState, LightMode, NightMode,
        StatusFlags, Temperature,
    };
    use crate::shelly::light::LastCommandSource;

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
        .expect("Cannot parse Input Component");
        if let ComponentEntry::Light(light::Component {
            key,
            status,
            config,
        }) = entry
        {
            assert_eq!(key, light::Key { id: 0 });
            assert_eq!(
                status,
                light::Status {
                    id: 0,
                    source: LastCommandSource::WsIn,
                    output: false,
                    brightness: 100,
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
                    in_mode: Some(LightMode::Dim),
                    initial_state: InitialState::RestoreLast,
                    auto_on: false,
                    auto_on_delay: Duration::seconds(60),
                    auto_off: false,
                    auto_off_delay: Duration::seconds(60),
                    transition_duration: Duration::seconds(3),
                    min_brightness_on_toggle: 3,
                    night_mode: NightMode {
                        enable: false,
                        brightness: 50,
                        active_between: Box::new([]),
                    },
                    button_fade_rate: 3,
                    button_presets: ButtonPresets {
                        button_doublepush: Some(ButtonDoublePush { brightness: 100 })
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
            assert_eq!(key, switch::Key { id: 1 });
            assert_eq!(
                status,
                switch::Status {
                    id: 1,
                    source: switch::LastCommandSource::WsIn,
                    output: false,
                    timer_started_at: None,
                    timer_duration: None,
                    temperature: Some(switch::Temperature {
                        temp_celsius: Some(31.0),
                        temp_fahrenheit: Some(87.7),
                    }),
                    active_energy: Some(switch::ActiveEnergy {
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
                    returned_active_energy: Some(switch::ActiveEnergy {
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
                    in_mode: Some(switch::LightMode::Follow),
                    initial_state: switch::InitialState::MatchInput,
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
}
