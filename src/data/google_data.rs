use std::{
    array,
    collections::{BTreeMap, HashMap, HashSet},
    fmt::{Debug, Display, Formatter, Write},
    io,
    net::IpAddr,
    str::FromStr,
    time::Duration,
    vec::IntoIter,
};

use chrono::{DateTime, Local};
use google_sheets4::{
    api::{
        BatchUpdateValuesRequest, CellData, GridData, Spreadsheet, SpreadsheetMethods, ValueRange,
    },
    hyper::{client::HttpConnector, Client},
    hyper_rustls::{self, HttpsConnector},
    oauth2::{authenticator::Authenticator, ServiceAccountAuthenticator},
    Sheets,
};
use log::error;
use serde::Deserialize;
use thiserror::Error;

use crate::{
    data::{
        registry::{
            BrightnessKey, ClockKey, DualButtonKey, LightColorKey, SingleButtonKey,
            SwitchOutputKey, TemperatureKey,
        },
        settings::{GoogleButtonData, GoogleButtonTemplate, GoogleError, GoogleSheet, CONFIG},
        state::{BrickletConnectionData, BrickletMetadata, State},
        wiring::{
            ButtonSetting, Controllers, DmxConfigEntry, DmxSettings, DualInputDimmer,
            DualInputSwitch, HeatController, IoSettings, MotionDetector, MotionDetectorSettings,
            Orientation, RelayChannelEntry, RelaySettings, RingController, ScreenSettings,
            TemperatureSettings, TinkerforgeDevices, Wiring,
        },
        DeviceInRoom, Room, SubDeviceInRoom, Uid,
    },
    util::kelvin_2_mireds,
};

#[derive(Error, Debug)]
pub enum GoogleDataError {
    #[error("Error accessing file")]
    Io(#[from] io::Error),
    #[error("Error from google api: {0}")]
    Google(#[from] GoogleError),
    #[error("Error from google sheet api: {0}")]
    Sheet(#[from] google_sheets4::Error),
    #[error("Error parsing light template header: {0}")]
    LightTemplateHeader(HeaderError),
    #[error("Error parsing light header: {0}")]
    LightHeader(HeaderError),
    #[error("Error parsing button template header: {0}")]
    ButtonTemplateHeader(HeaderError),
    #[error("Error parsing button header: {0}")]
    ButtonHeader(HeaderError),
    #[error("Error parsing motion detector header: {0}")]
    MotionDetectorHeader(HeaderError),
    #[error("No data found in spreadsheet")]
    NoDataFound,
    #[error("Table contains no header")]
    EmptyTable,
    #[error("Error parsing headers in {1}: {0}")]
    HeaderNotFound(HeaderError, Box<str>),
}

enum LightTemplateTypes {
    Switch,
    Dimm,
    DimmWhitebalance {
        warm_temperature: u16,
        cold_temperature: u16,
    },
}

enum ButtonStyle {
    Single,
    Dual,
}

struct ButtonTemplateTypes<'a> {
    style: ButtonStyle,
    sub_devices: Box<[&'a str]>,
}

pub async fn read_sheet_data(state: Option<&State>) -> Result<Option<Wiring>, GoogleDataError> {
    let x = if let Some(config) = &CONFIG.google_sheet {
        let secret = config.read_secret().await?;
        let auth: Authenticator<HttpsConnector<HttpConnector>> =
            ServiceAccountAuthenticator::builder(secret).build().await?;

        let connector_builder = hyper_rustls::HttpsConnectorBuilder::new();

        let client = Client::builder().build(
            connector_builder
                .with_native_roots()
                .https_or_http()
                .enable_http1()
                .enable_http2()
                .build(),
        );

        let hub = Sheets::new(client, auth);

        let endpoints_config = config.endpoints();
        let light_template_config = config.light_templates();
        let light_config = config.light();
        let button_template_config = config.button_templates();
        let button_config = config.buttons();
        let room_controller_config = config.room_controllers();
        let motion_detector_config = config.motion_detectors();
        let relays_config = config.relays();

        let spreadsheet_methods = hub.spreadsheets();
        let (_, sheet) = spreadsheet_methods
            .get(config.spreadsheet_id())
            .add_scope("https://www.googleapis.com/auth/spreadsheets")
            .include_grid_data(true)
            .add_ranges(&format!(
                "{}!{}",
                endpoints_config.sheet(),
                endpoints_config.range()
            ))
            .add_ranges(&format!(
                "{}!{}",
                room_controller_config.sheet(),
                room_controller_config.range()
            ))
            .add_ranges(&format!(
                "{}!{}",
                motion_detector_config.sheet(),
                motion_detector_config.range()
            ))
            .add_ranges(&format!(
                "{}!{}",
                light_config.sheet(),
                light_config.range()
            ))
            .add_ranges(&format!(
                "{}!{}",
                light_template_config.sheet(),
                light_template_config.range()
            ))
            .add_ranges(&format!(
                "{}!{}",
                button_config.sheet(),
                button_config.range()
            ))
            .add_ranges(&format!(
                "{}!{}",
                button_template_config.sheet(),
                button_template_config.range()
            ))
            .add_ranges(&format!(
                "{}!{}",
                relays_config.sheet(),
                relays_config.range()
            ))
            .doit()
            .await?;
        let mut io_bricklets = BTreeMap::<_, Vec<_>>::new();
        let mut dmx_bricklets = BTreeMap::<_, Vec<_>>::new();
        let mut lcd_screens = BTreeMap::new();
        let mut temperature_sensors = BTreeMap::new();
        let mut motion_detector_sensors = BTreeMap::new();
        let mut relays = BTreeMap::new();
        let mut endpoints = Vec::new();

        let mut dual_input_dimmers = Vec::new();
        let mut dual_input_switches = Vec::new();
        let mut motion_detectors = Vec::new();
        let mut heat_controllers = Vec::new();
        let mut ring_controllers = Vec::new();

        let mut single_button_adresses = HashMap::new();
        let mut dual_button_adresses = HashMap::new();
        let mut heat_outputs_addresses = HashMap::new();
        let mut touchscreen_whitebalance_addresses = HashMap::new();
        let mut touchscreen_brightness_addresses = HashMap::new();
        let mut motion_detector_adresses = HashMap::new();

        parse_endpoints(config, &spreadsheet_methods, state, &mut endpoints).await?;
        parse_buttons(
            config,
            button_template_config,
            button_config,
            &spreadsheet_methods,
            &sheet,
            state,
            &mut io_bricklets,
            &mut single_button_adresses,
            &mut dual_button_adresses,
        )
        .await?;
        parse_motion_detectors(
            config,
            &spreadsheet_methods,
            state,
            &mut motion_detector_sensors,
            &mut motion_detector_adresses,
        )
        .await?;
        parse_controllers(
            config,
            &spreadsheet_methods,
            state,
            &mut lcd_screens,
            &mut temperature_sensors,
            &mut heat_controllers,
            &mut heat_outputs_addresses,
            &mut touchscreen_whitebalance_addresses,
            &mut touchscreen_brightness_addresses,
        )
        .await?;
        parse_lights(
            config,
            &spreadsheet_methods,
            state,
            &mut dmx_bricklets,
            &mut dual_input_dimmers,
            &mut dual_input_switches,
            &mut motion_detectors,
            &mut dual_button_adresses,
            &mut touchscreen_whitebalance_addresses,
            &mut touchscreen_brightness_addresses,
            &mut motion_detector_adresses,
        )
        .await?;
        parse_relays(
            config,
            &spreadsheet_methods,
            state,
            &mut relays,
            &mut ring_controllers,
            &mut single_button_adresses,
            &mut heat_outputs_addresses,
        )
        .await?;
        dual_input_dimmers.sort();
        dual_input_switches.sort();
        motion_detectors.sort();
        heat_controllers.sort();
        ring_controllers.sort();
        endpoints.sort();
        Some(Wiring {
            controllers: Controllers {
                dual_input_dimmers: dual_input_dimmers.into_boxed_slice(),
                dual_input_switches: dual_input_switches.into_boxed_slice(),
                motion_detectors: motion_detectors.into_boxed_slice(),
                heat_controllers: heat_controllers.into_boxed_slice(),
                ring_controllers: ring_controllers.into_boxed_slice(),
            },
            tinkerforge_devices: TinkerforgeDevices {
                endpoints: endpoints.into_boxed_slice(),
                lcd_screens,
                dmx_bricklets: dmx_bricklets
                    .into_iter()
                    .map(|(uid, mut settings)| {
                        settings.sort();
                        (
                            uid,
                            DmxSettings {
                                entries: settings.into_boxed_slice(),
                            },
                        )
                    })
                    .collect(),
                io_bricklets: io_bricklets
                    .into_iter()
                    .map(|(uid, mut settings)| {
                        settings.sort();
                        (
                            uid,
                            IoSettings {
                                entries: settings.into_boxed_slice(),
                            },
                        )
                    })
                    .collect(),
                motion_detectors: motion_detector_sensors,
                relays,
                temperature_sensors,
            },
        })
    } else {
        None
    };
    Ok(x)
}

async fn parse_endpoints(
    config: &GoogleSheet,
    spreadsheet_methods: &SpreadsheetMethods<'_, HttpsConnector<HttpConnector>>,
    state: Option<&State>,
    endpoints: &mut Vec<IpAddr>,
) -> Result<(), GoogleDataError> {
    let endpoints_config = config.endpoints();
    let mut updates = Vec::new();
    for (address, state_cell) in GoogleTable::connect(
        spreadsheet_methods,
        [endpoints_config.address(), endpoints_config.state()],
        [],
        config.spreadsheet_id(),
        endpoints_config.sheet(),
        endpoints_config.range(),
    )
    .await?
    .filter_map(|([address, state], _)| {
        address
            .get_content()
            .map(IpAddr::from_str)
            .and_then(Result::ok)
            .map(|ip| (ip, state))
    }) {
        endpoints.push(address);
        if let Some(update) = state
            .and_then(|s| s.endpoint(&address))
            .map(|data| {
                format!(
                    "{} at {}",
                    data.state,
                    DateTime::<Local>::from(data.last_change)
                )
            })
            .and_then(|new_state| state_cell.create_content_update(&new_state))
        {
            updates.push(update);
        }
    }
    write_updates_to_sheet(config, spreadsheet_methods, updates).await?;
    Ok(())
}

async fn parse_relays<'a>(
    config: &'a GoogleSheet,
    spreadsheet_methods: &SpreadsheetMethods<'_, HttpsConnector<HttpConnector>>,
    state: Option<&State>,
    relays: &mut BTreeMap<Uid, RelaySettings>,
    ring_controllers: &mut Vec<RingController>,
    single_buttons: &HashMap<Box<str>, SingleButtonKey>,
    heating_outputs: &HashMap<Box<str>, SwitchOutputKey>,
) -> Result<(), GoogleDataError> {
    let relay_configs = config.relays();
    let mut relay_channels = HashMap::<_, Vec<_>>::new();
    let mut device_ids_of_rooms = HashMap::<_, Vec<_>>::new();
    let mut updates = Vec::new();

    for (room, idx, uid, channel, temperature, ring_button, old_state) in GoogleTable::connect(
        spreadsheet_methods,
        [
            relay_configs.room_id(),
            relay_configs.idx(),
            relay_configs.device_address(),
            relay_configs.device_channel(),
            relay_configs.temperature_sensor(),
            relay_configs.ring_button(),
            relay_configs.state(),
        ],
        [],
        config.spreadsheet_id(),
        relay_configs.sheet(),
        relay_configs.range(),
    )
    .await?
    .filter_map(
        |([room, idx, address, channel, sensor, button, state], _)| {
            if let (Some(room), Some(address), Some(channel)) = (
                room.get_content().map(Room::from_str).and_then(Result::ok),
                address
                    .get_content()
                    .map(Uid::from_str)
                    .and_then(Result::ok),
                channel.get_integer().map(|v| v as u8),
            ) {
                Some((
                    room,
                    DeviceIdxCell(idx),
                    address,
                    channel,
                    sensor
                        .get_content()
                        .and_then(|k| heating_outputs.get(k))
                        .copied(),
                    button
                        .get_content()
                        .and_then(|k| single_buttons.get(k))
                        .copied(),
                    state,
                ))
            } else {
                None
            }
        },
    ) {
        if let Some(temp_input) = temperature {
            relay_channels
                .entry(uid)
                .or_default()
                .push(RelayChannelEntry {
                    channel,
                    input: temp_input,
                });
        } else if let Some(ring_button) = ring_button {
            device_ids_of_rooms.entry(room).or_default().push(RingRow {
                uid,
                channel,
                idx,
                ring_button,
            });
        }
        update_state_new(|v| updates.push(v), state, &old_state, uid);
    }
    struct RingRow<'a> {
        idx: DeviceIdxCell<'a>,
        ring_button: SingleButtonKey,
        uid: Uid,
        channel: u8,
    }
    impl<'a> DeviceIdxAccessNew<'a> for RingRow<'a> {
        fn id_cell<'b>(&'b mut self) -> &'b mut DeviceIdxCell<'a> {
            &mut self.idx
        }
    }
    let ring_rows = fill_device_idx(|v| updates.push(v), device_ids_of_rooms);
    write_updates_to_sheet(config, spreadsheet_methods, updates).await?;
    for (device, row) in ring_rows {
        let index = SwitchOutputKey::Bell(device);
        ring_controllers.push(RingController {
            input: row.ring_button,
            output: index,
        });
        relay_channels
            .entry(row.uid)
            .or_default()
            .push(RelayChannelEntry {
                channel: row.channel,
                input: index,
            });
    }

    for (uid, channels) in relay_channels {
        relays.insert(
            uid,
            RelaySettings {
                entries: channels.into_boxed_slice(),
            },
        );
    }
    Ok(())
}

async fn parse_controllers<'a>(
    config: &GoogleSheet,
    spreadsheet_methods: &SpreadsheetMethods<'_, HttpsConnector<HttpConnector>>,
    state: Option<&State>,
    lcd_screens: &mut BTreeMap<Uid, ScreenSettings>,
    temperature_sensors: &mut BTreeMap<Uid, TemperatureSettings>,
    heat_controllers: &mut Vec<HeatController>,
    heat_outputs: &mut HashMap<Box<str>, SwitchOutputKey>,
    touchscreen_whitebalances: &mut HashMap<Box<str>, LightColorKey>,
    touchscreen_brightness: &mut HashMap<Box<str>, BrightnessKey>,
) -> Result<(), GoogleDataError> {
    struct ControllerRow<'a> {
        id: Box<str>,
        idx: DeviceIdxCell<'a>,
        orientation: Orientation,
        touchscreen: Option<Uid>,
        temp_sensor: Option<Uid>,
        enable_heatcontrol: bool,
        enable_whitebalance_control: bool,
        enable_brighness_control: bool,
    }
    impl<'a> DeviceIdxAccessNew<'a> for ControllerRow<'a> {
        fn id_cell<'b>(&'b mut self) -> &'b mut DeviceIdxCell<'a> {
            &mut self.idx
        }
    }
    let controllers = config.room_controllers();
    let mut device_ids_of_rooms = HashMap::<_, Vec<_>>::new();
    let mut updates = Vec::new();

    for (room, controller_id, idx, orientation, touchscreen, temp_sensor, enable_heatcontrol, enable_whitebalance_control, enable_brighness_control, touchscreen_state, temperature_state) in GoogleTable::connect(
        spreadsheet_methods,
        [
            controllers.room_id(),
            controllers.controller_id(),
            controllers.controller_idx(),
            controllers.orientation(),
            controllers.touchscreen_device_address(),
            controllers.temperature_device_address(),
            controllers.enable_heat_control(),
            controllers.enable_whitebalance_control(),
            controllers.enable_brightness_control(),
            controllers.touchscreen_state(),
            controllers.temperature_state(),
        ],[],
        config.spreadsheet_id(),
        controllers.sheet(),
        controllers.range(),
    ).await?.filter_map(|([room, id, idx, orientation, touchscreen, temperature,
                         heat_control, whitebalance, brightness, touchscreen_state, temperature_state],_)| {
        if let (Some(room), Some(controller_id), Some(orientation)) = (room.get_content().map(Room::from_str)
                                                                           .and_then(Result::ok), id.get_content(), orientation.get_content().map(|v| Orientation::deserialize(serde_yaml::Value::String(v.to_string())))
                                                                           .and_then(Result::ok)) {
            Some((room, controller_id.to_string().into_boxed_str(), DeviceIdxCell(idx), orientation,
                  touchscreen.get_content().map(Uid::from_str).and_then(Result::ok),
                  temperature.get_content().map(Uid::from_str).and_then(Result::ok),
                  heat_control.get_content().map(|v| !v.is_empty())
                      .unwrap_or(false),
                  whitebalance.get_content().map(|v| !v.is_empty())
                      .unwrap_or(false),
                  brightness.get_content().map(|v| !v.is_empty())
                      .unwrap_or(false),
                  touchscreen_state, temperature_state
            ))
        } else {
            None
        }
    }) {
        device_ids_of_rooms.entry(room).or_default().push(
            ControllerRow {
                id: controller_id,
                idx,
                orientation,
                touchscreen,
                temp_sensor,
                enable_heatcontrol,
                enable_whitebalance_control,
                enable_brighness_control,
            },
        );
        if let Some(uid) = touchscreen {
            update_state_new(
                |s|updates.push(s),state, & touchscreen_state,uid,            );
        }
        if let Some(uid)= temp_sensor {
            update_state_new(|s| updates.push(s),state,&temperature_state,uid)
        }
    }
    let controller_rows = fill_device_idx(|s| updates.push(s), device_ids_of_rooms);
    for (device_idx, row) in controller_rows {
        let current_temperature_key = if let Some(uid) = row.temp_sensor {
            let output = TemperatureKey::CurrentTemperature(device_idx);
            temperature_sensors.insert(uid, TemperatureSettings { output });
            Some(output)
        } else {
            None
        };
        let adjust_temperature_key = if row.enable_heatcontrol {
            if let Some(current_value_input) = current_temperature_key {
                let target_value_input = TemperatureKey::TargetTemperature(device_idx);
                let output = SwitchOutputKey::Heat(device_idx);
                heat_controllers.push(HeatController {
                    current_value_input,
                    target_value_input,
                    output,
                });
                heat_outputs.insert(row.id.clone(), output);
                Some(target_value_input)
            } else {
                None
            }
        } else {
            None
        };
        let light_color_key = if row.enable_whitebalance_control {
            let key = LightColorKey::TouchscreenController(device_idx);
            touchscreen_whitebalances.insert(row.id.clone(), key);
            Some(key)
        } else {
            None
        };
        let brightness_key = if row.enable_brighness_control {
            let key = BrightnessKey::TouchscreenController(device_idx);
            touchscreen_brightness.insert(row.id, key);
            Some(key)
        } else {
            None
        };
        if let Some(uid) = row.touchscreen {
            lcd_screens.insert(
                uid,
                ScreenSettings {
                    orientation: row.orientation,
                    clock_key: Some(ClockKey::MinuteClock),
                    current_temperature_key,
                    adjust_temperature_key,
                    light_color_key,
                    brightness_key,
                },
            );
        }
    }

    write_updates_to_sheet(config, spreadsheet_methods, updates).await?;
    Ok(())
}

async fn parse_motion_detectors<'a>(
    config: &GoogleSheet,
    spreadsheet_methods: &SpreadsheetMethods<'_, HttpsConnector<HttpConnector>>,
    state: Option<&State>,
    motion_detectors: &mut BTreeMap<Uid, MotionDetectorSettings>,
    single_button_adresses: &mut HashMap<Box<str>, SingleButtonKey>,
) -> Result<(), GoogleDataError> {
    struct MotionDetectorRow<'a> {
        id: Box<str>,
        device_address: Uid,
        idx: DeviceIdxCell<'a>,
    }
    impl<'a> DeviceIdxAccessNew<'a> for MotionDetectorRow<'a> {
        fn id_cell<'b>(&'b mut self) -> &'b mut DeviceIdxCell<'a> {
            &mut self.idx
        }
    }
    let mut updates = Vec::new();

    let md_config = config.motion_detectors();
    let mut device_ids_of_rooms = HashMap::<_, Vec<_>>::new();

    for (room, device_address, id, idx, state_cell) in GoogleTable::connect(
        spreadsheet_methods,
        [
            md_config.room_id(),
            md_config.device_address(),
            md_config.id(),
            md_config.idx(),
            md_config.state(),
        ],
        [],
        config.spreadsheet_id(),
        md_config.sheet(),
        md_config.range(),
    )
    .await?
    .filter_map(|([room, address, id, idx, state], _)| {
        if let (Some(room), Some(address), Some(id)) = (
            room.get_content().map(Room::from_str).and_then(Result::ok),
            address
                .get_content()
                .map(Uid::from_str)
                .and_then(Result::ok),
            id.get_content().map(Into::<Box<str>>::into),
        ) {
            Some((room, address, id, DeviceIdxCell(idx), state))
        } else {
            None
        }
    }) {
        device_ids_of_rooms
            .entry(room)
            .or_default()
            .push(MotionDetectorRow {
                id,
                device_address,
                idx,
            });
        update_state_new(|v| updates.push(v), state, &state_cell, device_address);
    }
    let motion_detector_rows = fill_device_idx(|v| updates.push(v), device_ids_of_rooms);
    write_updates_to_sheet(config, spreadsheet_methods, updates).await?;

    for (device_idx, row) in motion_detector_rows {
        let key = SingleButtonKey::MotionDetector(device_idx);
        motion_detectors.insert(row.device_address, MotionDetectorSettings { output: key });
        single_button_adresses.insert(row.id.into(), key);
    }
    Ok(())
}

async fn parse_lights<'a>(
    config: &GoogleSheet,
    spreadsheet_methods: &SpreadsheetMethods<'_, HttpsConnector<HttpConnector>>,
    state: Option<&State>,
    dmx_bricklets: &mut BTreeMap<Uid, Vec<DmxConfigEntry>>,
    dual_input_dimmers: &mut Vec<DualInputDimmer>,
    dual_input_switches: &mut Vec<DualInputSwitch>,
    motion_detector_controllers: &mut Vec<MotionDetector>,
    dual_button_adresses: &mut HashMap<Box<str>, DualButtonKey>,
    touchscreen_whitebalances: &mut HashMap<Box<str>, LightColorKey>,
    touchscreen_brightness: &mut HashMap<Box<str>, BrightnessKey>,
    motion_detector_adresses: &mut HashMap<Box<str>, SingleButtonKey>,
) -> Result<(), GoogleDataError> {
    let light_templates = config.light_templates();
    let mut light_template_map = HashMap::new();
    for ([name, discriminator, warm, cold], _) in GoogleTable::connect(
        spreadsheet_methods,
        [
            light_templates.name_column(),
            light_templates.discriminator_column(),
            light_templates.temperature_warm_column(),
            light_templates.temperature_cold_column(),
        ],
        [],
        config.spreadsheet_id(),
        light_templates.sheet(),
        light_templates.range(),
    )
    .await?
    {
        if let (Some(name), Some(discriminator)) = (
            name.get_content().map(Into::<Box<str>>::into),
            discriminator.get_content(),
        ) {
            if discriminator == "Switch" {
                light_template_map.insert(name, LightTemplateTypes::Switch);
            } else if discriminator == "Dimm" {
                light_template_map.insert(name, LightTemplateTypes::Dimm);
            } else if discriminator == "DimmWhitebalance" {
                if let (Some(warm_temperature), Some(cold_temperature)) =
                    (warm.get_integer(), cold.get_integer())
                {
                    light_template_map.insert(
                        name,
                        LightTemplateTypes::DimmWhitebalance {
                            warm_temperature: kelvin_2_mireds(warm_temperature as u16),
                            cold_temperature: kelvin_2_mireds(cold_temperature as u16),
                        },
                    );
                }
            }
        }
    }
    struct LightRowContent<'a> {
        light_template: &'a LightTemplateTypes,
        device_id_in_room: DeviceIdxCell<'a>,
        device_address: Uid,
        bus_start_address: u16,
        manual_buttons: Box<[Box<str>]>,
        presence_detectors: Box<[Box<str>]>,
        touchscreen_whitebalance: Option<Box<str>>,
        touchscreen_brightness: Option<Box<str>>,
    }
    impl<'a> DeviceIdxAccessNew<'a> for LightRowContent<'a> {
        fn id_cell<'b>(&'b mut self) -> &'b mut DeviceIdxCell<'a> {
            &mut self.device_id_in_room
        }
    }
    let light_config = config.light();
    let button_columns = light_config
        .manual_buttons()
        .iter()
        .map(|c| c.as_ref())
        .collect::<Vec<_>>();
    let presence_detector_columns = light_config
        .presence_detectors()
        .iter()
        .map(|c| c.as_ref())
        .collect::<Vec<_>>();
    let mut device_ids_of_rooms = HashMap::<_, Vec<_>>::new();
    let mut updates = Vec::new();

    for (
        [room, light_idx, template, address, start_channel, whitebalance, brightness, old_state],
        [buttons, presence_detectors],
    ) in GoogleTable::connect(
        spreadsheet_methods,
        [
            light_config.room_id(),
            light_config.light_idx(),
            light_config.template(),
            light_config.device_address(),
            light_config.bus_start_address(),
            light_config.touchscreen_whitebalance(),
            light_config.touchscreen_brightness(),
            light_config.state(),
        ],
        [&button_columns, &presence_detector_columns],
        config.spreadsheet_id(),
        light_config.sheet(),
        light_config.range(),
    )
    .await?
    {
        if let (
            Some(room),
            device_id_in_room,
            Some(light_template),
            Some(device_address),
            Some(bus_start_address),
            manual_buttons,
            presence_detectors,
            touchscreen_whitebalance,
            touchscreen_brightness,
            old_state,
        ) = (
            room.get_content().map(Room::from_str).and_then(Result::ok),
            DeviceIdxCell(light_idx),
            template
                .get_content()
                .and_then(|t| light_template_map.get(t)),
            address
                .get_content()
                .map(Uid::from_str)
                .and_then(Result::ok),
            start_channel.get_integer(),
            buttons
                .iter()
                .filter_map(|cell| GoogleCellData::get_content(cell))
                .filter(|s| !s.is_empty())
                .map(|s| s.into())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            presence_detectors
                .iter()
                .filter_map(|cell| cell.get_content())
                .filter(|s| !s.is_empty())
                .map(|s| s.into())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            whitebalance.get_content().map(|s| s.into()),
            brightness.get_content().map(|s| s.into()),
            old_state,
        ) {
            device_ids_of_rooms
                .entry(room)
                .or_default()
                .push(LightRowContent {
                    light_template,
                    device_id_in_room,
                    device_address,
                    bus_start_address: bus_start_address as u16,
                    manual_buttons,
                    presence_detectors,
                    touchscreen_whitebalance,
                    touchscreen_brightness,
                });
            update_state_new(|v| updates.push(v), state, &old_state, device_address);
            //info!("Room: {room:?}, idx: {coordinates}");
        }
    }
    let light_device_rows = fill_device_idx(|v| updates.push(v), device_ids_of_rooms);

    for (device_idx, light_row) in light_device_rows {
        let dmx_bricklet_settings = dmx_bricklets.entry(light_row.device_address).or_default();
        let template = light_row.light_template;

        let mut manual_buttons = light_row
            .manual_buttons
            .iter()
            .flat_map(|name| dual_button_adresses.get(name))
            .copied()
            .collect::<Vec<_>>()
            .into_boxed_slice();
        manual_buttons.sort();
        let mut presence_detectors = light_row
            .presence_detectors
            .iter()
            .flat_map(|name| motion_detector_adresses.get(name))
            .copied()
            .collect::<Vec<_>>()
            .into_boxed_slice();
        presence_detectors.sort();
        let auto_switch_off_time = if manual_buttons.is_empty() {
            Duration::from_secs(2 * 60)
        } else if presence_detectors.is_empty() {
            Duration::from_secs(2 * 3600)
        } else {
            Duration::from_secs(3600)
        };
        match template {
            LightTemplateTypes::Switch => {
                let register = SwitchOutputKey::Light(device_idx);
                dmx_bricklet_settings.push(DmxConfigEntry::Switch {
                    register,
                    channel: light_row.bus_start_address,
                });
                if manual_buttons.is_empty() {
                    if !presence_detectors.is_empty() {
                        motion_detector_controllers.push(MotionDetector::Switch {
                            input: presence_detectors,
                            output: register,
                            switch_off_time: auto_switch_off_time,
                        });
                    }
                } else {
                    dual_input_switches.push(DualInputSwitch {
                        input: manual_buttons,
                        output: register,
                        auto_switch_off_time,
                        presence: presence_detectors,
                    });
                }
            }
            LightTemplateTypes::Dimm => {
                let register = BrightnessKey::Light(device_idx);
                dmx_bricklet_settings.push(DmxConfigEntry::Dimm {
                    register,
                    channel: light_row.bus_start_address,
                });
                if manual_buttons.is_empty() {
                    if !presence_detectors.is_empty() {
                        motion_detector_controllers.push(MotionDetector::Dimmer {
                            input: presence_detectors,
                            output: register,
                            brightness: light_row
                                .touchscreen_brightness
                                .and_then(|k| touchscreen_brightness.get(&k))
                                .copied(),
                            switch_off_time: auto_switch_off_time,
                        });
                    }
                } else {
                    dual_input_dimmers.push(DualInputDimmer {
                        input: manual_buttons,
                        output: register,
                        auto_switch_off_time,
                        presence: presence_detectors,
                    })
                }
            }
            LightTemplateTypes::DimmWhitebalance {
                warm_temperature,
                cold_temperature,
            } => {
                let device_in_room = device_idx;

                let output_brightness_register = BrightnessKey::Light(device_in_room);
                let whitebalance_register = if let Some(wb) = light_row
                    .touchscreen_whitebalance
                    .and_then(|k| touchscreen_whitebalances.get(&k))
                {
                    *wb
                } else {
                    LightColorKey::Light(device_in_room)
                };
                dmx_bricklet_settings.push(DmxConfigEntry::DimmWhitebalance {
                    brightness_register: output_brightness_register,
                    whitebalance_register,
                    warm_channel: light_row.bus_start_address,
                    cold_channel: light_row.bus_start_address + 1,
                    warm_mireds: *warm_temperature,
                    cold_mireds: *cold_temperature,
                });
                if manual_buttons.is_empty() {
                    if !presence_detectors.is_empty() {
                        motion_detector_controllers.push(MotionDetector::Dimmer {
                            input: presence_detectors,
                            output: output_brightness_register,
                            brightness: light_row
                                .touchscreen_brightness
                                .and_then(|k| touchscreen_brightness.get(&k))
                                .copied(),
                            switch_off_time: auto_switch_off_time,
                        });
                    }
                } else {
                    dual_input_dimmers.push(DualInputDimmer {
                        input: manual_buttons,
                        output: output_brightness_register,
                        auto_switch_off_time,
                        presence: presence_detectors,
                    });
                }
            }
        }
    }
    write_updates_to_sheet(config, spreadsheet_methods, updates).await?;

    Ok(())
}

trait DeviceIdxAccess {
    fn existing_id(&self) -> Option<u16>;
    fn update_id(&mut self, id: u16);
}

trait DeviceIdxAccessNew<'a> {
    fn id_cell<'b>(&'b mut self) -> &'b mut DeviceIdxCell<'a>;
}

fn fill_device_idx<'a, R: DeviceIdxAccessNew<'a>, F: FnMut(ValueRange)>(
    mut updater: F,
    device_ids_of_rooms: HashMap<Room, Vec<R>>,
) -> Vec<(DeviceInRoom, R)> {
    let mut device_rows = Vec::new();
    for (room, devices) in device_ids_of_rooms {
        let mut occupied_ids = HashSet::new();
        let mut remaining_devices = Vec::with_capacity(devices.len());
        for mut row in devices.into_iter() {
            let option = row
                .id_cell()
                .existing_idx()
                .filter(|id| !occupied_ids.contains(id));
            if let Some(idx) = option {
                occupied_ids.insert(idx);
                device_rows.push((DeviceInRoom { room, idx }, row));
            } else {
                remaining_devices.push(row);
            };
        }
        let mut next_id = 0;
        for mut row in remaining_devices {
            while occupied_ids.contains(&next_id) {
                next_id += 1;
            }
            let (idx, update) = row.id_cell().get_or_create_idx(&mut || next_id);
            device_rows.push((DeviceInRoom { room, idx }, row));
            if let Some(update) = update {
                updater(update);
            }
            next_id += 1;
        }
    }

    device_rows
}

async fn adjust_device_idx<R: DeviceIdxAccess>(
    device_ids_of_rooms: HashMap<Room, Vec<(CellCoordinates, R)>>,
    sheet_name: &str,
    updates: &mut Vec<ValueRange>,
) -> Result<Vec<R>, GoogleDataError> {
    let mut device_rows = Vec::new();
    for devices in device_ids_of_rooms.into_values() {
        let mut occupied_ids = HashSet::new();
        let mut remaining_devices = Vec::with_capacity(devices.len());
        for (coordinates, row) in devices {
            if if let Some(id) = row.existing_id() {
                if occupied_ids.contains(&id) {
                    true
                } else {
                    occupied_ids.insert(id);
                    false
                }
            } else {
                true
            } {
                remaining_devices.push((coordinates, row));
            } else {
                device_rows.push(row);
            };
        }
        let mut next_id = 0;
        for (coordinates, mut row) in remaining_devices {
            while occupied_ids.contains(&next_id) {
                next_id += 1;
            }
            row.update_id(next_id);
            device_rows.push(row);
            updates.push(ValueRange {
                major_dimension: None,
                range: Some(format!("{}!{}", sheet_name, coordinates)),
                values: Some(vec![vec![next_id.into()]]),
            });
            next_id += 1;
        }
    }
    Ok(device_rows)
}

async fn parse_buttons<'a>(
    config: &GoogleSheet,
    button_templates: &GoogleButtonTemplate,
    button_config: &GoogleButtonData,
    spreadsheet_methods: &SpreadsheetMethods<'_, HttpsConnector<HttpConnector>>,
    sheet: &'a Spreadsheet,
    state: Option<&State>,
    io_bricklets: &mut BTreeMap<Uid, Vec<ButtonSetting>>,
    single_button_adresses: &mut HashMap<Box<str>, SingleButtonKey>,
    dual_button_adresses: &mut HashMap<Box<str>, DualButtonKey>,
) -> Result<(), GoogleDataError> {
    let mut button_template_map = HashMap::new();

    if let Some(button_templates_grid) = find_sheet_by_name(sheet, button_templates.sheet()) {
        let (_, _, mut rows) = get_grid_and_coordinates(button_templates_grid);
        if let Some((_, header)) = rows.next() {
            let [name_column, sub_device_column, discriminiator_column] = parse_headers(
                header,
                [
                    button_templates.name(),
                    button_templates.sub_devices(),
                    button_templates.discriminator(),
                ],
            )
            .map_err(GoogleDataError::ButtonTemplateHeader)?;
            for (_, row) in rows {
                if let (Some(name), Some(sub_devices), Some(discriminator)) = (
                    get_cell_content(row, name_column),
                    get_cell_content(row, sub_device_column),
                    get_cell_content(row, discriminiator_column),
                ) {
                    let sub_devices = sub_devices
                        .split(',')
                        .collect::<Vec<_>>()
                        .into_boxed_slice();
                    if let Some(style) = if discriminator == "Single" {
                        Some(ButtonStyle::Single)
                    } else if discriminator == "Dual" {
                        Some(ButtonStyle::Dual)
                    } else {
                        None
                    } {
                        button_template_map
                            .insert(name, ButtonTemplateTypes { style, sub_devices });
                    }
                }
            }
        }
    }
    if let Some(button_grid) = find_sheet_by_name(sheet, button_config.sheet()) {
        struct ButtonRowContent<'a> {
            room: Room,
            button_template: &'a ButtonTemplateTypes<'a>,
            button_id: &'a str,
            button_id_in_room: Option<u16>,
            device_address: Uid,
            first_input_idx: u8,
        }
        let (start_row, start_column, mut rows) = get_grid_and_coordinates(button_grid);
        if let Some((_, header)) = rows.next() {
            let [room_column, button_id_column, button_idx_column, type_column, device_address_column, first_input_idx_column, state_column] =
                parse_headers(
                    header,
                    [
                        button_config.room_id(),
                        button_config.button_id(),
                        button_config.button_idx(),
                        button_config.button_type(),
                        button_config.device_address(),
                        button_config.first_input_idx(),
                        button_config.state(),
                    ],
                )
                .map_err(GoogleDataError::ButtonHeader)?;
            let mut button_ids_of_rooms = HashMap::<_, Vec<_>>::new();
            let mut updates = Vec::new();
            for (row_idx, row) in rows {
                if let (
                    Some(room),
                    Some(button_id),
                    button_id_in_room,
                    Some(button_template),
                    Some(device_address),
                    Some(first_input_idx),
                    state_data,
                ) = (
                    get_cell_content(row, room_column)
                        .map(Room::from_str)
                        .and_then(Result::ok),
                    get_cell_content(row, button_id_column),
                    get_cell_integer(row, button_idx_column).map(|id| id as u16),
                    get_cell_content(row, type_column).and_then(|t| button_template_map.get(t)),
                    get_cell_content(row, device_address_column)
                        .map(Uid::from_str)
                        .and_then(Result::ok),
                    get_cell_integer(row, first_input_idx_column).map(|id| id as u8),
                    get_cell_content(row, state_column),
                ) {
                    let row = row_idx + start_row;
                    let col = button_idx_column + start_column;
                    let coordinates = CellCoordinates { row, col };
                    update_state(
                        &mut updates,
                        button_config.sheet(),
                        CellCoordinates {
                            row,
                            col: start_column + state_column,
                        },
                        device_address,
                        state,
                        state_data,
                    );
                    button_ids_of_rooms.entry(room).or_default().push((
                        coordinates,
                        ButtonRowContent {
                            room,
                            button_template,
                            button_id,
                            button_id_in_room,
                            device_address,
                            first_input_idx,
                        },
                    ));
                }
            }
            let mut button_device_rows = Vec::new();
            for devices in button_ids_of_rooms.into_values() {
                let mut occupied_ids = HashSet::new();
                let mut remaining_devices = Vec::with_capacity(devices.len());
                for (coordinates, button_row) in devices {
                    if if let Some(id) = button_row.button_id_in_room {
                        if occupied_ids.contains(&id) {
                            true
                        } else {
                            occupied_ids.insert(id);
                            false
                        }
                    } else {
                        true
                    } {
                        remaining_devices.push((coordinates, button_row));
                    } else {
                        button_device_rows.push(button_row);
                    };
                }
                let mut next_id = 0;
                for (coordinates, mut row) in remaining_devices {
                    while occupied_ids.contains(&next_id) {
                        next_id += 1;
                    }
                    row.button_id_in_room = Some(next_id);
                    button_device_rows.push(row);
                    updates.push(ValueRange {
                        major_dimension: None,
                        range: Some(format!("{}!{}", button_config.sheet(), coordinates)),
                        values: Some(vec![vec![next_id.into()]]),
                    });
                    next_id += 1;
                }
            }
            write_updates_to_sheet(config, spreadsheet_methods, updates).await?;
            for button_row in button_device_rows {
                let io_bricklet_settings =
                    io_bricklets.entry(button_row.device_address).or_default();
                let mut current_input_idx = button_row.first_input_idx;
                for (subdevice_id, subdevice_name) in
                    button_row.button_template.sub_devices.iter().enumerate()
                {
                    let device_key = format!("{}, {}", button_row.button_id, subdevice_name);

                    let sub_device_in_room = SubDeviceInRoom {
                        room: button_row.room,
                        device_idx: button_row.button_id_in_room.unwrap(),
                        sub_device_idx: subdevice_id as u16,
                    };
                    match button_row.button_template.style {
                        ButtonStyle::Single => {
                            let output = SingleButtonKey::Button(sub_device_in_room);
                            io_bricklet_settings.push(ButtonSetting::Single {
                                button: current_input_idx,
                                output,
                            });
                            single_button_adresses.insert(device_key.into(), output);
                            current_input_idx += 1;
                        }
                        ButtonStyle::Dual => {
                            let output = DualButtonKey(sub_device_in_room);
                            io_bricklet_settings.push(ButtonSetting::Dual {
                                up_button: current_input_idx + 1,
                                down_button: current_input_idx,
                                output,
                            });
                            dual_button_adresses.insert(device_key.into(), output);
                            current_input_idx += 2;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

async fn write_updates_to_sheet(
    config: &GoogleSheet,
    spreadsheet_methods: &SpreadsheetMethods<'_, HttpsConnector<HttpConnector>>,
    updates: Vec<ValueRange>,
) -> Result<(), GoogleDataError> {
    if !updates.is_empty() {
        let update = BatchUpdateValuesRequest {
            data: Some(updates),
            include_values_in_response: None,
            response_date_time_render_option: None,
            response_value_render_option: None,
            value_input_option: Some("RAW".to_string()),
        };
        spreadsheet_methods
            .values_batch_update(update, config.spreadsheet_id())
            .doit()
            .await?;
    }
    Ok(())
}

fn update_state_new<F: FnMut(ValueRange)>(
    mut updater: F,
    current_state: Option<&State>,
    state_cell: &GoogleCellData,
    uid: Uid,
) {
    if let Some(current_state) = current_state {
        let new_text = match current_state.bricklet(&uid) {
            None => "Not found".to_string(),
            Some(&BrickletConnectionData {
                ref state,
                last_change,
                endpoint,
                ref metadata,
            }) => {
                let timestamp = DateTime::<Local>::from(last_change);
                if let Some(BrickletMetadata {
                    connected_uid,
                    position,
                    hardware_version,
                    firmware_version,
                }) = metadata
                {
                    format!("{state}, {timestamp}, {endpoint}; {connected_uid}, {position}")
                } else {
                    format!("{state}, {timestamp}, {endpoint}")
                }
            }
        };
        if let Some(update) = state_cell.create_content_update(&new_text) {
            updater(update);
        }
    }
}

fn update_state(
    updates: &mut Vec<ValueRange>,
    sheet_name: &str,
    cell_coordinates: CellCoordinates,
    uid: Uid,
    current_state: Option<&State>,
    stored_state: Option<&str>,
) {
    if let Some(current_state) = current_state {
        let new_text = match current_state.bricklet(&uid) {
            None => "Not found".to_string(),
            Some(&BrickletConnectionData {
                ref state,
                last_change,
                endpoint,
                ref metadata,
            }) => {
                let timestamp = DateTime::<Local>::from(last_change);
                if let Some(BrickletMetadata {
                    connected_uid,
                    position,
                    hardware_version,
                    firmware_version,
                }) = metadata
                {
                    format!("{state}, {timestamp}, {endpoint}; {connected_uid}, {position}")
                } else {
                    format!("{state}, {timestamp}, {endpoint}")
                }
            }
        };
        if stored_state != Some(&new_text) {
            updates.push(ValueRange {
                major_dimension: None,
                range: Some(format!("{}!{}", sheet_name, cell_coordinates)),
                values: Some(vec![vec![new_text.into()]]),
            });
        }
    }
}

fn find_sheet_by_name<'a>(sheet: &'a Spreadsheet, name_of_sheet: &str) -> Option<&'a GridData> {
    sheet
        .sheets
        .iter()
        .flatten()
        .filter(|s| {
            s.properties
                .as_ref()
                .and_then(|p| p.title.as_deref())
                .map(|t| t == name_of_sheet)
                .unwrap_or_default()
        })
        .flat_map(|s| &s.data)
        .flatten()
        .next()
}

fn get_cell_content(row: &[CellData], idx: usize) -> Option<&str> {
    row.get(idx).and_then(|f| f.formatted_value.as_deref())
}

fn get_cell_number(row: &[CellData], idx: usize) -> Option<f64> {
    row.get(idx)
        .and_then(|f| f.user_entered_value.as_ref())
        .and_then(|v| v.number_value)
}

fn get_cell_integer(row: &[CellData], idx: usize) -> Option<i64> {
    get_cell_number(row, idx).map(|v| v.round() as i64)
}

struct GoogleTable<'a, const N: usize, const M: usize> {
    fixed_column_indizes: [usize; N],
    dynamic_column_indices: [Box<[usize]>; M],
    sheet_name: &'a str,
    rows: IntoIter<(usize, Vec<CellData>)>,
    start_row: usize,
    start_column: usize,
}

impl<'a, const N: usize, const M: usize> Iterator for GoogleTable<'a, N, M> {
    type Item = ([GoogleCellData<'a>; N], [Box<[GoogleCellData<'a>]>; M]);

    fn next(&mut self) -> Option<Self::Item> {
        self.rows.next().map(|(row_idx, row_data)| {
            let mut data = row_data.into_iter().map(Some).collect::<Vec<_>>();
            let row = self.start_row + row_idx;
            let dynamic_cols = self.dynamic_column_indices.clone().map(|dynamic_columns| {
                dynamic_columns
                    .iter()
                    .map(|idx| GoogleCellData {
                        data: data.get_mut(*idx).and_then(|e| e.take()),
                        sheet: self.sheet_name,
                        coordinates: CellCoordinates {
                            row,
                            col: self.start_column + idx,
                        },
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice()
            });

            let static_cols = self.fixed_column_indizes.map(|idx| GoogleCellData {
                data: data.get_mut(idx).and_then(|e| e.take()),
                sheet: self.sheet_name,
                coordinates: CellCoordinates {
                    row,
                    col: self.start_column + idx,
                },
            });
            (static_cols, dynamic_cols)
        })
    }
}

struct GoogleCellData<'a> {
    data: Option<CellData>,
    sheet: &'a str,
    coordinates: CellCoordinates,
}

impl GoogleCellData<'_> {
    fn get_content(&self) -> Option<&str> {
        self.data
            .as_ref()
            .and_then(|c| c.formatted_value.as_deref())
    }
    fn get_number(&self) -> Option<f64> {
        self.data
            .as_ref()
            .and_then(|f| f.user_entered_value.as_ref())
            .and_then(|v| v.number_value)
    }
    fn get_integer(&self) -> Option<i64> {
        self.get_number().map(|v| v.round() as i64)
    }
    fn create_content_update(&self, new_value: &str) -> Option<ValueRange> {
        if self
            .data
            .as_ref()
            .and_then(|data| data.formatted_value.as_ref())
            .map(|old_value| old_value.as_str() != new_value)
            .unwrap_or(true)
        {
            Some(self.override_cell(new_value))
        } else {
            None
        }
    }
    fn override_cell(&self, value: impl Into<serde_json::Value>) -> ValueRange {
        ValueRange {
            major_dimension: None,
            range: Some(format!("{}!{}", self.sheet, self.coordinates)),
            values: Some(vec![vec![value.into()]]),
        }
    }
}

struct DeviceIdxCell<'a>(GoogleCellData<'a>);

impl<'a> DeviceIdxCell<'a> {
    fn get_or_create_idx<F: FnMut() -> u16>(&self, supplier: &mut F) -> (u16, Option<ValueRange>) {
        if let Some(old_value) = self.existing_idx() {
            (old_value, None)
        } else {
            let new_value = supplier();
            (new_value, Some(self.0.override_cell(new_value)))
        }
    }

    fn existing_idx(&self) -> Option<u16> {
        self.0.get_integer().map(|v| v as u16)
    }
}

impl<'a, const N: usize, const M: usize> GoogleTable<'a, N, M> {
    async fn connect(
        spreadsheet_methods: &'a SpreadsheetMethods<'a, HttpsConnector<HttpConnector>>,
        column_names: [&'a str; N],
        dynamic_columns: [&[&'a str]; M],
        document_id: &'a str,
        sheet_name: &'a str,
        range: &'a str,
    ) -> Result<Self, GoogleDataError> {
        let (_, sheet) = spreadsheet_methods
            .get(document_id)
            .add_scope("https://www.googleapis.com/auth/spreadsheets")
            .include_grid_data(true)
            .add_ranges(&format!("{}!{}", sheet_name, range))
            .doit()
            .await?;
        let grid = sheet
            .sheets
            .into_iter()
            .flatten()
            .flat_map(|s| s.data)
            .flatten()
            .next()
            .ok_or(GoogleDataError::NoDataFound)?;

        let start_row = grid.start_row.unwrap_or_default() as usize;
        let start_column = grid.start_column.unwrap_or_default() as usize;
        let mut rows: IntoIter<(usize, Vec<CellData>)> = grid
            .row_data
            .into_iter()
            .flatten()
            .map(|r| r.values)
            .enumerate()
            .filter_map(|(idx, r)| r.map(|row| (idx, row)))
            .collect::<Vec<_>>()
            .into_iter();
        let (_, header_row) = rows.next().ok_or(GoogleDataError::EmptyTable)?;
        let fixed_column_indizes = parse_headers(&header_row, column_names).map_err(|error| {
            GoogleDataError::HeaderNotFound(
                error,
                format!("{}!{}", sheet_name, range).into_boxed_str(),
            )
        })?;
        let mut dynamic_column_indices: [Option<Box<[usize]>>; M] = array::from_fn(|_| None);
        for (idx, column_result) in dynamic_columns
            .map(|dynamic_column_names| {
                parse_dynamic_headers(&header_row, dynamic_column_names).map_err(|error| {
                    GoogleDataError::HeaderNotFound(
                        error,
                        format!("{}!{}", sheet_name, range).into_boxed_str(),
                    )
                })
            })
            .into_iter()
            .enumerate()
        {
            match column_result {
                Err(error) => {
                    return Err(error);
                }
                Ok(value) => dynamic_column_indices[idx] = Some(value),
            }
        }

        Ok(GoogleTable {
            fixed_column_indizes,
            dynamic_column_indices: dynamic_column_indices
                .map(|v| v.expect("This should not be possible")),
            sheet_name,
            rows,
            start_row,
            start_column,
        })
    }
}

#[derive(Error, Debug)]
#[error("Missing Headers: {0:?}")]
pub struct HeaderError(Box<[Box<str>]>);

fn parse_headers<const N: usize>(
    row: &[CellData],
    header_columns: [&str; N],
) -> Result<[usize; N], HeaderError> {
    let mut found_header_ids = [None; N];
    find_indizes_of_headers(row, &header_columns, &mut found_header_ids);
    let mut missing_headers = Vec::with_capacity(N);
    let mut ret = [0; N];
    for (header_idx, col_idx) in found_header_ids.into_iter().enumerate() {
        if let Some(idx) = col_idx {
            ret[header_idx] = idx;
        } else {
            missing_headers.push(header_columns[header_idx].to_string().into_boxed_str());
        }
    }
    if missing_headers.is_empty() {
        Ok(ret)
    } else {
        Err(HeaderError(missing_headers.into_boxed_slice()))
    }
}

fn find_indizes_of_headers(
    row: &[CellData],
    header_columns: &[&str],
    found_header_ids: &mut [Option<usize>],
) {
    assert_eq!(header_columns.len(), found_header_ids.len());
    for (col_idx, content) in row.iter().enumerate() {
        if let Some(text) = content.formatted_value.as_deref() {
            for (idx, title) in header_columns.iter().enumerate() {
                if *title == text {
                    found_header_ids[idx] = Some(col_idx);
                }
            }
        }
    }
}

fn parse_dynamic_headers(
    row: &[CellData],
    header_columns: &[&str],
) -> Result<Box<[usize]>, HeaderError> {
    let mut found_header_ids = vec![None; header_columns.len()].into_boxed_slice();
    find_indizes_of_headers(row, header_columns, &mut found_header_ids);
    let mut missing_headers = Vec::with_capacity(header_columns.len());
    let mut ret = Vec::with_capacity(header_columns.len());
    for (header_idx, col_idx) in found_header_ids.iter().enumerate() {
        if let Some(idx) = col_idx {
            ret.push(*idx);
        } else {
            missing_headers.push(header_columns[header_idx].to_string().into_boxed_str());
        }
    }
    if missing_headers.is_empty() {
        Ok(ret.into_boxed_slice())
    } else {
        Err(HeaderError(missing_headers.into_boxed_slice()))
    }
}

fn get_grid_and_coordinates(
    light_grid: &GridData,
) -> (usize, usize, impl Iterator<Item = (usize, &Vec<CellData>)>) {
    let start_row = light_grid.start_row.unwrap_or_default() as usize;
    let start_column = light_grid.start_column.unwrap_or_default() as usize;
    let rows = light_grid
        .row_data
        .iter()
        .flatten()
        .map(|r| &r.values)
        .enumerate()
        .filter_map(|(idx, r)| r.as_ref().map(|row| (idx, row)));
    (start_row, start_column, rows)
}

#[derive(Copy, Clone)]
struct CellCoordinates {
    row: usize,
    col: usize,
}

impl CellCoordinates {
    fn format(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if self.col < 26 {
            f.write_char(char::from_u32('A' as u32 + self.col as u32).unwrap())?;
        } else {
            f.write_char(char::from_u32('A' as u32 + self.col as u32 / 26 - 1).unwrap())?;
            f.write_char(char::from_u32('A' as u32 + self.col as u32 % 26).unwrap())?;
        }
        write!(f, "{}", self.row + 1)
    }
}

impl Debug for CellCoordinates {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.format(f)
    }
}

impl Display for CellCoordinates {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.format(f)
    }
}

#[cfg(test)]
mod test {
    use env_logger::Env;
    use log::{error, info};

    use crate::data::google_data::{read_sheet_data, CellCoordinates};

    #[test]
    fn format_coordinates() {
        assert_eq!(
            "A1",
            CellCoordinates { row: 0, col: 0 }.to_string().as_str()
        );
        assert_eq!(
            "Z2",
            CellCoordinates { row: 1, col: 25 }.to_string().as_str()
        );
        assert_eq!(
            "AA7",
            CellCoordinates { row: 6, col: 26 }.to_string().as_str()
        );
    }

    #[tokio::test]
    async fn test_read_sheet() {
        env_logger::init_from_env(Env::default().filter_or("LOG_LEVEL", "info"));
        let result = read_sheet_data(None).await;
        match result {
            Ok(Some(wiring)) => {
                info!("Loaded data: \n{}", serde_yaml::to_string(&wiring).unwrap());
            }
            Err(error) => {
                error!("Error loading data: {error}");
            }
            Ok(None) => {
                info!("No Error, No Data");
            }
        };
    }
}
