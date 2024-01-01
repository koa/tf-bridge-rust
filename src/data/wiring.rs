use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use strum_macros::EnumIter;

use crate::data::registry::{
    BrightnessKey, ClockKey, DualButtonKey, LightColorKey, SingleButtonKey, SwitchOutputKey,
    TemperatureKey,
};
use crate::data::Uid;

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Wiring {
    pub controllers: Controllers,
    pub tinkerforge_devices: TinkerforgeDevices,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Controllers {
    pub dual_input_dimmers: Box<[DualInputDimmer]>,
    pub dual_input_switches: Box<[DualInputSwitch]>,
    pub motion_detectors: Box<[MotionDetector]>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DualInputDimmer {
    pub input: DualButtonKey,
    pub output: BrightnessKey,
    pub auto_switch_off_time: Duration,
    pub presence: Option<SingleButtonKey>,
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DualInputSwitch {
    input: DualButtonKey,
    output: SwitchOutputKey,
    auto_switch_off_time: Duration,
    presence: Option<SingleButtonKey>,
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct MotionDetector {
    input: SingleButtonKey,
    output: SwitchOutputKey,
    switch_off_time: Duration,
}
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct TinkerforgeDevices {
    pub lcd_screens: HashMap<Uid, ScreenSettings>,
    pub dmx_bricklets: HashMap<Uid, DmxSettings>,
    pub io_bricklets: HashMap<Uid, IoSettings>,
    pub motion_detectors: HashMap<Uid, MotionDetectorSettings>,
    pub relays: HashMap<Uid, RelaySettings>,
    pub temperature_sensors: HashMap<Uid, TemperatureSettings>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ScreenSettings {
    pub orientation: Orientation,
    pub clock_key: Option<ClockKey>,
    pub current_temperature_key: Option<TemperatureKey>,
    pub adjust_temperature_key: Option<TemperatureKey>,
    pub light_color_key: Option<LightColorKey>,
    pub brightness_key: Option<BrightnessKey>,
}
#[derive(Copy, Clone, Debug, Eq, PartialEq, EnumIter, Serialize, Deserialize)]
pub enum Orientation {
    Straight,
    LeftDown,
    UpsideDown,
    RightDown,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DmxSettings {
    pub entries: Box<[DmxConfigEntry]>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IoSettings {
    entries: Box<[ButtonSetting]>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ButtonSetting {
    Dual {
        up_button: u8,
        down_button: u8,
        output: DualButtonKey,
    },
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct MotionDetectorSettings {
    output: SingleButtonKey,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RelaySettings {
    entries: Box<[RelayChannelEntry]>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct RelayChannelEntry {
    channel: u8,
    input: SwitchOutputKey,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct TemperatureSettings {
    output: TemperatureKey,
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use crate::data::registry::{BrightnessKey, LightColorKey};
    use crate::data::wiring::{DmxConfigEntry, DmxSettings, TinkerforgeDevices, Wiring};
    use crate::data::DeviceInRoom;
    use crate::util::kelvin_2_mireds;

    #[test]
    fn test_serialize_tinkerforge() {
        let data = Wiring {
            controllers: Default::default(),
            tinkerforge_devices: TinkerforgeDevices {
                lcd_screens: Default::default(),
                dmx_bricklets: HashMap::from([(
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
        };
        let yaml_data = serde_yaml::to_string(&data).unwrap();
        println!("{}", yaml_data);
        assert_eq!(data, serde_yaml::from_str(&yaml_data).unwrap());
    }
}
