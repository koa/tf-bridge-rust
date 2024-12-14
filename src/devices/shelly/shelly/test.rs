use std::net::{IpAddr, Ipv4Addr};

use chrono::{DateTime, Duration};
use serde_json::json;

use crate::{
    devices::shelly::{
        ble, cloud,
        common::{
            self, ButtonDoublePush, ButtonPresets, IPv4Mode, InitialState, InputMode, SslCa,
            Temperature,
        },
        eth, input, light,
        light::{NightMode, StatusFlags},
        mqtt,
        shelly::{ComponentEntry, GetComponentsResponse},
        switch, sys,
    },
    shelly::{
        common::{ActiveEnergy, LastCommandSource},
        wifi, ws,
    },
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
    let entry =
        serde_json::from_value::<ComponentEntry>(json!(                                        {
            "key": "cloud",
            "status": {
                "connected": false
            },
            "config": {
                "enable": false,
                "server": "iot.shelly.cloud:6012/jrpc"
            }
        }))
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
                ipv4mode: IPv4Mode::Dhcp,
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
                settings: light::Settings {
                    name: Default::default(),
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
                    current_limit: Some(1.22),
                },
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
                settings: switch::Settings {
                    name: Default::default(),
                    in_mode: common::InputMode::Follow,
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
                    current_limit: Some(16.0),
                },
            }
        );
    } else {
        panic!("Wrong component: {entry:?}");
    }
}
#[test]
fn test_sys() {
    let entry = serde_json::from_value::<sys::Component>(
        json!({"config":{"cfg_rev":5,"debug":{"file_level":null,"level":2,"mqtt":{"enable":false},"udp":{"addr":null},"websocket":{"enable":false}},"device":{"discoverable":true,"fw_id":"20240625-123141/1.3.3-gbdfd9b3","mac":"08F9E0E720C8","name":null},"location":{"lat":47.3682,"lon":8.5671,"tz":"Europe/Zurich"},"rpc_udp":{"dst_addr":null,"listen_port":null},"sntp":{"server":"time.google.com"},"ui_data":{}},"key":"sys","status":{"available_updates":{"stable":{"version":"1.4.2"}},"cfg_rev":5,"fs_free":192512,"fs_size":524288,"kvs_rev":0,"mac":"08F9E0E720C8","ram_free":120240,"ram_size":259828,"reset_reason":1,"restart_required":false,"schedule_rev":1,"time":"10:03","unixtime":1725091412,"uptime":700043,"webhook_rev":0}}),
    ).expect("Cannot parse sys");
    //println!("{:#?}", entry);
}
#[test]
fn test_wifi() {
    let entry = serde_json::from_value::<wifi::Component>(
        json!({"config":{"ap":{"enable":true,"is_open":true,"range_extender":{"enable":false},"ssid":"ShellyPro4PM-34987A47A1DC"},"roam":{"interval":60,"rssi_thr":-80},"sta":{"enable":false,"gw":null,"ip":null,"ipv4mode":"dhcp","is_open":true,"nameserver":null,"netmask":null,"ssid":null},"sta1":{"enable":false,"gw":null,"ip":null,"ipv4mode":"dhcp","is_open":true,"nameserver":null,"netmask":null,"ssid":null}},"key":"wifi","status":{"rssi":0,"ssid":null,"sta_ip":null,"status":"disconnected"}}),
    ).expect("Cannot parse wifi");
    //println!("{:#?}", entry);
}
#[test]
fn test_ws() {
    let entry = serde_json::from_value::<ws::Component>(
        json!({"config":{"enable":false,"server":null,"ssl_ca":"ca.pem"},"key":"ws","status":{"connected":false}}),
    ).expect("Cannot parse ws");
    assert_eq!(
        entry,
        ws::Component {
            key: ws::Key::Ws,
            status: ws::Status { connected: false },
            config: ws::Configuration {
                enable: false,
                server: None,
                ssl_ca: SslCa::BuiltIn,
            },
        }
    );
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
    let components_json = json!( {"cfg_rev":7,"components":[{"config":{"client_id":"shellyprodm2pm-08f9e0e720c8","enable":false,"enable_control":true,"enable_rpc":true,"rpc_ntf":true,"server":null,"ssl_ca":null,"status_ntf":false,"topic_prefix":"shellyprodm2pm-08f9e0e720c8","use_client_cert":false,"user":null},"key":"mqtt","status":{"connected":false}},{"config":{"cfg_rev":7,"debug":{"file_level":null,"level":2,"mqtt":{"enable":false},"udp":{"addr":null},"websocket":{"enable":false}},"device":{"discoverable":true,"fw_id":"20240819-074552/1.4.2-gc2639da","mac":"08F9E0E720C8","name":null},"location":{"lat":47.3682,"lon":8.5671,"tz":"Europe/Zurich"},"rpc_udp":{"dst_addr":null,"listen_port":null},"sntp":{"server":"time.google.com"},"ui_data":{}},"key":"sys","status":{"available_updates":{},"cfg_rev":7,"fs_free":184320,"fs_size":524288,"kvs_rev":0,"mac":"08F9E0E720C8","ram_free":87208,"ram_size":265584,"reset_reason":3,"restart_required":false,"schedule_rev":1,"time":"16:25","unixtime":1725114307,"uptime":2881,"webhook_rev":0}},{"config":{"ap":{"enable":false,"is_open":true,"range_extender":{"enable":false},"ssid":"ShellyProDM2PM-08F9E0E720C8"},"roam":{"interval":60,"rssi_thr":-80},"sta":{"enable":true,"gw":null,"ip":null,"ipv4mode":"dhcp","is_open":true,"nameserver":null,"netmask":null,"ssid":"#CHFreeWiFi"},"sta1":{"enable":false,"gw":null,"ip":null,"ipv4mode":"dhcp","is_open":true,"nameserver":null,"netmask":null,"ssid":null}},"key":"wifi","status":{"rssi":-49,"ssid":"#CHFreeWiFi","sta_ip":"10.192.1.198","status":"got ip"}},{"config":{"enable":false,"server":null,"ssl_ca":"ca.pem"},"key":"ws","status":{"connected":false}}],"offset":11,"total":15} );
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
    serde_json::from_value::<switch::Component>(json!(                {"config":{"auto_off":false,"auto_off_delay":60.0,"auto_on":false,"auto_on_delay":60.0,"autorecover_voltage_errors":false,"current_limit":16.0,"id":0,"in_mode":"follow","initial_state":"match_input","name":null,"power_limit":4480,"undervoltage_limit":0,"voltage_limit":280},"key":"switch:0","status":{"aenergy":{"by_minute":[0.0,0.0,0.0],"minute_ts":1725093540,"total":0.0},"apower":0.0,"current":0.0,"freq":50.0,"id":0,"output":false,"pf":0.0,"ret_aenergy":{"by_minute":[0.0,0.0,0.0],"minute_ts":1725093540,"total":0.0},"source":"timer","temperature":{"tC":57.3,"tF":135.1},"voltage":232.7}}))
    .expect("Cannot parse switch");
}
