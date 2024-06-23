use std::{collections::BTreeMap, net::IpAddr, time::Duration};

use serde::{Deserialize, Serialize};
use strum_macros::EnumIter;
use tinkerforge_async::base58::Uid;

use crate::data::registry::{
    BrightnessKey, ClockKey, DualButtonKey, LightColorKey, SingleButtonKey, SwitchOutputKey,
    TemperatureKey,
};

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Wiring {
    pub controllers: Controllers,
    pub tinkerforge_devices: TinkerforgeDevices,
    pub shelly_devices: ShellyDevices,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Controllers {
    pub dual_input_dimmers: Box<[DualInputDimmer]>,
    pub dual_input_switches: Box<[DualInputSwitch]>,
    pub motion_detectors: Box<[MotionDetector]>,
    pub heat_controllers: Box<[HeatController]>,
    pub ring_controllers: Box<[RingController]>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Ord, PartialOrd)]
pub struct DualInputDimmer {
    pub input: Box<[DualButtonKey]>,
    pub output: BrightnessKey,
    pub auto_switch_off_time: Duration,
    pub presence: Box<[SingleButtonKey]>,
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Ord, PartialOrd)]
pub struct DualInputSwitch {
    pub input: Box<[DualButtonKey]>,
    pub output: SwitchOutputKey,
    pub auto_switch_off_time: Duration,
    pub presence: Box<[SingleButtonKey]>,
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Ord, PartialOrd)]
pub enum MotionDetector {
    Switch {
        input: Box<[SingleButtonKey]>,
        output: SwitchOutputKey,
        switch_off_time: Duration,
    },
    Dimmer {
        input: Box<[SingleButtonKey]>,
        output: BrightnessKey,
        brightness: Option<BrightnessKey>,
        switch_off_time: Duration,
    },
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Ord, PartialOrd)]
pub struct HeatController {
    pub current_value_input: TemperatureKey,
    pub target_value_input: TemperatureKey,
    pub output: SwitchOutputKey,
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Ord, PartialOrd)]
pub struct RingController {
    pub input: SingleButtonKey,
    pub output: SwitchOutputKey,
}
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq, Ord, PartialOrd)]
pub struct TinkerforgeDevices {
    pub endpoints: Box<[IpAddr]>,
    pub lcd_screens: BTreeMap<Uid, ScreenSettings>,
    pub dmx_bricklets: BTreeMap<Uid, DmxSettings>,
    pub io_bricklets: BTreeMap<Uid, IoSettings>,
    pub motion_detectors: BTreeMap<Uid, MotionDetectorSettings>,
    pub relays: BTreeMap<Uid, RelaySettings>,
    pub temperature_sensors: BTreeMap<Uid, TemperatureSettings>,
}
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq, Ord, PartialOrd)]
pub struct ShellyDevices {
    pub endpoints: Box<[IpAddr]>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Ord, PartialOrd)]
pub struct ScreenSettings {
    pub orientation: Orientation,
    pub clock_key: Option<ClockKey>,
    pub current_temperature_key: Option<TemperatureKey>,
    pub adjust_temperature_key: Option<TemperatureKey>,
    pub light_color_key: Option<LightColorKey>,
    pub brightness_key: Option<BrightnessKey>,
}
#[derive(Copy, Clone, Debug, Eq, PartialEq, EnumIter, Serialize, Deserialize, Ord, PartialOrd)]
pub enum Orientation {
    Straight,
    LeftDown,
    UpsideDown,
    RightDown,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Ord, PartialOrd)]
pub struct DmxSettings {
    pub entries: Box<[DmxConfigEntry]>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Ord, PartialOrd)]
pub enum DmxConfigEntry {
    Dimm {
        register: BrightnessKey,
        channel: u16,
    },
    DimmWhitebalance {
        brightness_register: BrightnessKey,
        whitebalance_register: LightColorKey,
        warm_channel: u16,
        cold_channel: u16,
        warm_mireds: u16,
        cold_mireds: u16,
    },
    Switch {
        register: SwitchOutputKey,
        channel: u16,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Ord, PartialOrd)]
pub struct IoSettings {
    pub entries: Box<[ButtonSetting]>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Ord, PartialOrd)]
pub enum ButtonSetting {
    Dual {
        up_button: u8,
        down_button: u8,
        output: DualButtonKey,
    },
    Single {
        button: u8,
        output: SingleButtonKey,
    },
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Ord, PartialOrd)]
pub struct MotionDetectorSettings {
    pub output: SingleButtonKey,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Ord, PartialOrd)]
pub struct RelaySettings {
    pub entries: Box<[RelayChannelEntry]>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Ord, PartialOrd)]
pub struct RelayChannelEntry {
    pub channel: u8,
    pub input: SwitchOutputKey,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Ord, PartialOrd)]
pub struct TemperatureSettings {
    pub output: TemperatureKey,
}

#[cfg(test)]
mod test {
    use std::{
        collections::BTreeMap,
        net::{IpAddr, Ipv4Addr},
        time::Duration,
    };

    use crate::data::wiring::ShellyDevices;
    use crate::{
        data::{
            registry::{BrightnessKey, DualButtonKey, LightColorKey},
            wiring::{
                Controllers, DmxConfigEntry, DmxSettings, DualInputDimmer, TinkerforgeDevices,
                Wiring,
            },
            DeviceInRoom,
        },
        util::kelvin_2_mireds,
    };

    #[test]
    fn test_serialize_tinkerforge() {
        let data = Wiring {
            controllers: Controllers {
                dual_input_dimmers: Box::new([DualInputDimmer {
                    input: Box::new([DualButtonKey(Default::default())]),
                    output: BrightnessKey::Light(Default::default()),
                    auto_switch_off_time: Duration::from_secs(2 * 3600),
                    presence: Box::new([]),
                }]),
                dual_input_switches: Box::new([]),
                motion_detectors: Box::new([]),
                heat_controllers: Box::new([]),
                ring_controllers: Box::new([]),
            },
            tinkerforge_devices: TinkerforgeDevices {
                endpoints: Box::new([IpAddr::V4(Ipv4Addr::LOCALHOST)]),
                lcd_screens: Default::default(),
                dmx_bricklets: BTreeMap::from([(
                    "EHc".parse().unwrap(),
                    DmxSettings {
                        entries: Box::new([
                            DmxConfigEntry::Dimm {
                                register: BrightnessKey::Light(DeviceInRoom {
                                    room: "1.4".parse().unwrap(),
                                    idx: 0,
                                }),
                                channel: 3,
                            },
                            DmxConfigEntry::DimmWhitebalance {
                                brightness_register: BrightnessKey::Light(DeviceInRoom {
                                    room: "1.4".parse().unwrap(),
                                    idx: 0,
                                }),
                                whitebalance_register: LightColorKey::Light(DeviceInRoom {
                                    room: "1.4".parse().unwrap(),
                                    idx: 0,
                                }),
                                warm_channel: 2,
                                cold_channel: 3,
                                warm_mireds: kelvin_2_mireds(2700),
                                cold_mireds: kelvin_2_mireds(7500),
                            },
                        ]),
                    },
                )]),
                io_bricklets: Default::default(),
                motion_detectors: Default::default(),
                relays: Default::default(),
                temperature_sensors: Default::default(),
            },
            shelly_devices: ShellyDevices {
                endpoints: Box::new(["10.192.5.6".parse().expect("Cannot parse ip address")]),
            },
        };
        let yaml_data = serde_yaml::to_string(&data).unwrap();
        println!("{}", yaml_data);
        assert_eq!(data, serde_yaml::from_str(&yaml_data).unwrap());
    }
}
