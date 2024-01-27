use std::collections::BTreeMap;
use std::net::IpAddr;
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    fmt::{Debug, Display, Formatter, Write},
    io,
    str::FromStr,
    time::Duration,
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
use log::{error, info};
use serde::Deserialize;
use serde_yaml::Value;
use thiserror::Error;

use crate::data::state::{BrickletConnectionData, BrickletMetadata, State};
use crate::{
    data::{
        registry::{
            BrightnessKey, ClockKey, DualButtonKey, LightColorKey, SingleButtonKey,
            SwitchOutputKey, TemperatureKey,
        },
        settings::{GoogleButtonData, GoogleButtonTemplate, GoogleError, GoogleSheet, CONFIG},
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
    #[error("Error parsing controller header: {0}")]
    ControllerHeader(HeaderError),
    #[error("Error parsing motion detector header: {0}")]
    MotionDetectorHeader(HeaderError),
    #[error("Error parsing relays header: {0}")]
    RelayHeader(HeaderError),
    #[error("Error parsing endpoints header: {0}")]
    EndpointsHeader(HeaderError),
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

        parse_endpoints(config, &spreadsheet_methods, &sheet, state, &mut endpoints).await?;
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
            &sheet,
            state,
            &mut motion_detector_sensors,
            &mut motion_detector_adresses,
        )
        .await?;
        parse_controllers(
            config,
            &spreadsheet_methods,
            &sheet,
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
            &sheet,
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
            &sheet,
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
    sheet: &Spreadsheet,
    state: Option<&State>,
    endpoints: &mut Vec<IpAddr>,
) -> Result<(), GoogleDataError> {
    let endpoints_config = config.endpoints();
    if let Some(endpoint_grid) = find_sheet_by_name(sheet, endpoints_config.sheet()) {
        let (start_row, start_column, mut rows) = get_grid_and_coordinates(endpoint_grid);
        if let Some((_, header)) = rows.next() {
            let [address_column, state_column] = parse_headers(
                header,
                [endpoints_config.address(), endpoints_config.state()],
            )
            .map_err(GoogleDataError::EndpointsHeader)?;
            let mut updates = Vec::new();
            while let Some((row_idx, row)) = rows.next() {
                if let (Some(address), current_state) = (
                    get_cell_content(row, address_column)
                        .map(IpAddr::from_str)
                        .and_then(Result::ok),
                    get_cell_content(row, state_column),
                ) {
                    endpoints.push(address);
                    if let Some(new_state) = state.and_then(|s| s.endpoint(&address)).map(|data| {
                        format!(
                            "{} at {}",
                            data.state,
                            DateTime::<Local>::from(data.last_change)
                        )
                    }) {
                        info!("New State: {new_state:?}");
                        if current_state != Some(&new_state) {
                            let row = row_idx + start_row;
                            let col = state_column + start_column;
                            let coordinates = CellCoordinates { row, col };
                            updates.push(ValueRange {
                                major_dimension: None,
                                range: Some(format!(
                                    "{}!{}",
                                    endpoints_config.sheet(),
                                    coordinates
                                )),
                                values: Some(vec![vec![new_state.into()]]),
                            })
                        }
                    }
                }
            }
            write_updates_to_sheet(config, spreadsheet_methods, updates).await?;
        }
    }
    Ok(())
}

async fn parse_relays<'a>(
    config: &'a GoogleSheet,
    spreadsheet_methods: &SpreadsheetMethods<'_, HttpsConnector<HttpConnector>>,
    sheet: &Spreadsheet,
    state: Option<&State>,
    relays: &mut BTreeMap<Uid, RelaySettings>,
    ring_controllers: &mut Vec<RingController>,
    single_buttons: &mut HashMap<Cow<'a, str>, SingleButtonKey>,
    heating_outputs: &mut HashMap<&str, SwitchOutputKey>,
) -> Result<(), GoogleDataError> {
    struct RingRow {
        room: Room,
        idx: Option<u16>,
        ring_button: SingleButtonKey,
        uid: Uid,
        channel: u8,
    }
    impl DeviceIdxAccess for RingRow {
        fn existing_id(&self) -> Option<u16> {
            self.idx
        }

        fn update_id(&mut self, id: u16) {
            self.idx = Some(id);
        }
    }
    let mut relay_channels = HashMap::<_, Vec<_>>::new();
    let relay_configs = config.relays();
    if let Some(relay_grid) = find_sheet_by_name(sheet, relay_configs.sheet()) {
        let (start_row, start_column, mut rows) = get_grid_and_coordinates(relay_grid);
        if let Some((_, header)) = rows.next() {
            let [room_id_column, /*id_column,*/ idx_column, device_address_column, device_channel_column, temperature_column, ring_button_column,state_column] =
                parse_headers(
                    header,
                    [
                        relay_configs.room_id(),
                        //relay_configs.id(),
                        relay_configs.idx(),
                        relay_configs.device_address(),
                        relay_configs.device_channel(),
                        relay_configs.temperature_sensor(),
                        relay_configs.ring_button(),
                        relay_configs.state()
                    ],
                )
                .map_err(GoogleDataError::RelayHeader)?;
            let mut device_ids_of_rooms = HashMap::<_, Vec<_>>::new();
            let mut updates = Vec::new();
            for (row_idx, row) in rows {
                if let (
                    Some(room),
                    //Some(id),
                    idx,
                    Some(uid),
                    Some(channel),
                    temperature,
                    ring_button,
                    old_state,
                ) = (
                    get_cell_content(row, room_id_column)
                        .map(Room::from_str)
                        .and_then(Result::ok),
                    //get_cell_content(row, id_column),
                    get_cell_integer(row, idx_column).map(|v| v as u16),
                    get_cell_content(row, device_address_column)
                        .map(Uid::from_str)
                        .and_then(Result::ok),
                    get_cell_integer(row, device_channel_column).map(|v| v as u8),
                    get_cell_content(row, temperature_column)
                        .and_then(|k| heating_outputs.get(k))
                        .copied(),
                    get_cell_content(row, ring_button_column)
                        .and_then(|k| single_buttons.get(k))
                        .copied(),
                    get_cell_content(row, state_column),
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
                        let row = row_idx + start_row;
                        let col = idx_column + start_column;

                        device_ids_of_rooms.entry(room).or_default().push((
                            CellCoordinates { row, col },
                            RingRow {
                                uid,
                                channel,
                                room,
                                idx,
                                ring_button,
                            },
                        ));
                    }
                    update_state(
                        &mut updates,
                        relay_configs.sheet(),
                        CellCoordinates {
                            row: row_idx + start_row,
                            col: state_column + start_column,
                        },
                        uid,
                        state,
                        old_state,
                    );
                }
            }
            let ring_rows =
                adjust_device_idx(device_ids_of_rooms, relay_configs.sheet(), &mut updates).await?;
            write_updates_to_sheet(config, spreadsheet_methods, updates).await?;
            for row in ring_rows {
                let index = SwitchOutputKey::Bell(DeviceInRoom {
                    room: row.room,
                    idx: row.idx.unwrap(),
                });
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
        }
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
    sheet: &'a Spreadsheet,
    state: Option<&State>,
    lcd_screens: &mut BTreeMap<Uid, ScreenSettings>,
    temperature_sensors: &mut BTreeMap<Uid, TemperatureSettings>,
    heat_controllers: &mut Vec<HeatController>,
    heat_outputs: &mut HashMap<&'a str, SwitchOutputKey>,
    touchscreen_whitebalances: &mut HashMap<&'a str, LightColorKey>,
    touchscreen_brightness: &mut HashMap<&'a str, BrightnessKey>,
) -> Result<(), GoogleDataError> {
    struct ControllerRow<'a> {
        room: Room,
        id: &'a str,
        idx: Option<u16>,
        orientation: Orientation,
        touchscreen: Option<Uid>,
        temp_sensor: Option<Uid>,
        enable_heatcontrol: bool,
        enable_whitebalance_control: bool,
        enable_brighness_control: bool,
    }
    impl<'a> DeviceIdxAccess for ControllerRow<'a> {
        fn existing_id(&self) -> Option<u16> {
            self.idx
        }

        fn update_id(&mut self, id: u16) {
            self.idx = Some(id)
        }
    }
    let controllers = config.room_controllers();

    if let Some(controller_grid) = find_sheet_by_name(sheet, controllers.sheet()) {
        let (start_row, start_column, mut rows) = get_grid_and_coordinates(controller_grid);
        if let Some((_, header)) = rows.next() {
            let [room_column, id_column, index_column, orientation_column, touchscreen_column, temperature_column, heat_control_column, whitebalance_control_column, brightness_control_column, touchscreen_state_column, temperature_state_column] =
                parse_headers(
                    header,
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
                    ],
                )
                .map_err(GoogleDataError::ControllerHeader)?;

            let mut device_ids_of_rooms = HashMap::<_, Vec<_>>::new();
            let mut updates = Vec::new();

            for (row_idx, row) in rows {
                if let (
                    Some(room),
                    Some(controller_id),
                    idx,
                    Some(orientation),
                    touchscreen,
                    temp_sensor,
                    enable_heatcontrol,
                    enable_whitebalance_control,
                    enable_brighness_control,
                    touchscreen_state,
                    tempareature_state,
                ) = (
                    get_cell_content(row, room_column)
                        .map(Room::from_str)
                        .and_then(Result::ok),
                    get_cell_content(row, id_column),
                    get_cell_integer(row, index_column).map(|v| v as u16),
                    get_cell_content(row, orientation_column)
                        .map(|v| Orientation::deserialize(Value::String(v.to_string())))
                        .and_then(Result::ok),
                    get_cell_content(row, touchscreen_column)
                        .map(Uid::from_str)
                        .and_then(Result::ok),
                    get_cell_content(row, temperature_column)
                        .map(Uid::from_str)
                        .and_then(Result::ok),
                    get_cell_content(row, heat_control_column)
                        .map(|v| !v.is_empty())
                        .unwrap_or(false),
                    get_cell_content(row, whitebalance_control_column)
                        .map(|v| !v.is_empty())
                        .unwrap_or(false),
                    get_cell_content(row, brightness_control_column)
                        .map(|v| !v.is_empty())
                        .unwrap_or(false),
                    get_cell_content(row, touchscreen_state_column),
                    get_cell_content(row, temperature_state_column),
                ) {
                    let row = row_idx + start_row;
                    let col = index_column + start_column;
                    device_ids_of_rooms.entry(room).or_default().push((
                        CellCoordinates { row, col },
                        ControllerRow {
                            room,
                            id: controller_id,
                            idx,
                            orientation,
                            touchscreen,
                            temp_sensor,
                            enable_heatcontrol,
                            enable_whitebalance_control,
                            enable_brighness_control,
                        },
                    ));
                    if let Some(uid) = touchscreen {
                        update_state(
                            &mut updates,
                            controllers.sheet(),
                            CellCoordinates {
                                row,
                                col: touchscreen_state_column + start_column,
                            },
                            uid,
                            state,
                            touchscreen_state,
                        );
                    }
                    if let Some(uid) = temp_sensor {
                        update_state(
                            &mut updates,
                            controllers.sheet(),
                            CellCoordinates {
                                row,
                                col: temperature_state_column + start_column,
                            },
                            uid,
                            state,
                            tempareature_state,
                        );
                    }
                }
            }

            let controller_rows =
                adjust_device_idx(device_ids_of_rooms, controllers.sheet(), &mut updates).await?;
            write_updates_to_sheet(config, spreadsheet_methods, updates).await?;

            for row in controller_rows {
                let device_idx = DeviceInRoom {
                    room: row.room,
                    idx: row.idx.unwrap(),
                };
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
                        heat_outputs.insert(row.id, output);
                        Some(target_value_input)
                    } else {
                        None
                    }
                } else {
                    None
                };
                let light_color_key = if row.enable_whitebalance_control {
                    let key = LightColorKey::TouchscreenController(device_idx);
                    touchscreen_whitebalances.insert(row.id, key);
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
        }
    }
    Ok(())
}
async fn parse_motion_detectors<'a>(
    config: &GoogleSheet,
    spreadsheet_methods: &SpreadsheetMethods<'_, HttpsConnector<HttpConnector>>,
    sheet: &'a Spreadsheet,
    state: Option<&State>,
    motion_detectors: &mut BTreeMap<Uid, MotionDetectorSettings>,
    single_button_adresses: &mut HashMap<&'a str, SingleButtonKey>,
) -> Result<(), GoogleDataError> {
    struct MotionDetectorRow<'a> {
        id: &'a str,
        room: Room,
        device_address: Uid,
        idx: Option<u16>,
    }
    impl<'a> DeviceIdxAccess for MotionDetectorRow<'a> {
        fn existing_id(&self) -> Option<u16> {
            self.idx
        }

        fn update_id(&mut self, id: u16) {
            self.idx = Some(id);
        }
    }
    let md_config = config.motion_detectors();
    if let Some(motion_detector_grid) = find_sheet_by_name(sheet, md_config.sheet()) {
        let (start_row, start_column, mut rows) = get_grid_and_coordinates(motion_detector_grid);
        if let Some((_, header)) = rows.next() {
            let [room_column, address_column, id_column, idx_column, state_column] = parse_headers(
                header,
                [
                    md_config.room_id(),
                    md_config.device_address(),
                    md_config.id(),
                    md_config.idx(),
                    md_config.state(),
                ],
            )
            .map_err(GoogleDataError::MotionDetectorHeader)?;
            let mut device_ids_of_rooms = HashMap::<_, Vec<_>>::new();
            let mut updates = Vec::new();
            for (row_idx, row) in rows {
                if let (Some(room), Some(device_address), Some(id), idx, old_state) = (
                    get_cell_content(row, room_column)
                        .map(Room::from_str)
                        .and_then(Result::ok),
                    get_cell_content(row, address_column)
                        .map(Uid::from_str)
                        .and_then(Result::ok),
                    get_cell_content(row, id_column),
                    get_cell_integer(row, idx_column).map(|v| v as u16),
                    get_cell_content(row, state_column),
                ) {
                    let row = row_idx + start_row;
                    let col = idx_column + start_column;
                    device_ids_of_rooms.entry(room).or_default().push((
                        CellCoordinates { row, col },
                        MotionDetectorRow {
                            id,
                            room,
                            device_address,
                            idx,
                        },
                    ));
                    update_state(
                        &mut updates,
                        md_config.sheet(),
                        CellCoordinates {
                            row,
                            col: state_column + start_column,
                        },
                        device_address,
                        state,
                        old_state,
                    );
                }
            }

            let motion_detector_rows =
                adjust_device_idx(device_ids_of_rooms, md_config.sheet(), &mut updates).await?;
            write_updates_to_sheet(config, spreadsheet_methods, updates).await?;

            for row in motion_detector_rows {
                let key = SingleButtonKey::MotionDetector(DeviceInRoom {
                    room: row.room,
                    idx: row.idx.unwrap(),
                });
                motion_detectors.insert(row.device_address, MotionDetectorSettings { output: key });
                single_button_adresses.insert(row.id, key);
            }
        }
    }
    Ok(())
}
async fn parse_lights<'a>(
    config: &GoogleSheet,
    spreadsheet_methods: &SpreadsheetMethods<'_, HttpsConnector<HttpConnector>>,
    sheet: &'a Spreadsheet,
    state: Option<&State>,
    dmx_bricklets: &mut BTreeMap<Uid, Vec<DmxConfigEntry>>,
    dual_input_dimmers: &mut Vec<DualInputDimmer>,
    dual_input_switches: &mut Vec<DualInputSwitch>,
    motion_detector_controllers: &mut Vec<MotionDetector>,
    dual_button_adresses: &mut HashMap<Cow<'a, str>, DualButtonKey>,
    touchscreen_whitebalances: &mut HashMap<&'a str, LightColorKey>,
    touchscreen_brightness: &mut HashMap<&'a str, BrightnessKey>,
    motion_detector_adresses: &mut HashMap<&'a str, SingleButtonKey>,
) -> Result<(), GoogleDataError> {
    let light_templates = config.light_templates();
    let mut light_template_map = HashMap::new();
    if let Some(light_templates_grid) = find_sheet_by_name(sheet, light_templates.sheet()) {
        let (_, _, mut rows) = get_grid_and_coordinates(light_templates_grid);
        if let Some((_, header)) = rows.next() {
            let [name_column, discriminator_column, warm_column, cold_column] = parse_headers(
                header,
                [
                    light_templates.name_column(),
                    light_templates.discriminator_column(),
                    light_templates.temperature_warm_column(),
                    light_templates.temperature_cold_column(),
                ],
            )
            .map_err(GoogleDataError::LightTemplateHeader)?;
            for (_, row) in rows {
                if let (Some(name), Some(discriminator)) = (
                    get_cell_content(row, name_column),
                    get_cell_content(row, discriminator_column),
                ) {
                    if discriminator == "Switch" {
                        light_template_map.insert(name, LightTemplateTypes::Switch);
                    }
                    if discriminator == "Dimm" {
                        light_template_map.insert(name, LightTemplateTypes::Dimm);
                    }
                    if discriminator == "DimmWhitebalance" {
                        if let (Some(warm_temperature), Some(cold_temperature)) = (
                            get_cell_integer(row, warm_column),
                            get_cell_integer(row, cold_column),
                        ) {
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
        }
    }
    let light_config = config.light();
    if let Some(light_grid) = find_sheet_by_name(sheet, light_config.sheet()) {
        struct LightRowContent<'a> {
            room: Room,
            light_template: &'a LightTemplateTypes,
            //device_id: &'a str,
            device_id_in_room: Option<u16>,
            device_address: Uid,
            bus_start_address: u16,
            manual_buttons: Box<[&'a str]>,
            presence_detectors: Box<[&'a str]>,
            touchscreen_whitebalance: Option<&'a str>,
            touchscreen_brightness: Option<&'a str>,
        }
        impl<'a> DeviceIdxAccess for LightRowContent<'a> {
            fn existing_id(&self) -> Option<u16> {
                self.device_id_in_room
            }

            fn update_id(&mut self, id: u16) {
                self.device_id_in_room = Some(id);
            }
        }

        let (start_row, start_column, mut rows) = get_grid_and_coordinates(light_grid);
        if let Some((_, header)) = rows.next() {
            let [room_column, /*light_id_column,*/ light_idx_column, template_column, device_address_column, bus_start_address_column, touchscreen_whitebalance_column, touchscreen_brightness_column, state_column] =
                parse_headers(
                    header,
                    [
                        light_config.room_id(),
                        //light_config.light_id(),
                        light_config.light_idx(),
                        light_config.template(),
                        light_config.device_address(),
                        light_config.bus_start_address(),
                        light_config.touchscreen_whitebalance(),
                        light_config.touchscreen_brightness(),
                        light_config.state()
                    ],
                )
                .map_err(GoogleDataError::LightHeader)?;
            let manual_button_columns = parse_dynamic_headers(
                header,
                &light_config
                    .manual_buttons()
                    .iter()
                    .map(|c| c.as_ref())
                    .collect::<Vec<_>>(),
            )
            .map_err(GoogleDataError::LightHeader)?;
            let presence_detector_columns = parse_dynamic_headers(
                header,
                &light_config
                    .presence_detectors()
                    .iter()
                    .map(|c| c.as_ref())
                    .collect::<Vec<_>>(),
            )
            .map_err(GoogleDataError::LightHeader)?;
            let mut device_ids_of_rooms = HashMap::<_, Vec<_>>::new();
            let mut updates = Vec::new();

            for (row_idx, row) in rows {
                if let (
                    Some(room),
                    device_id_in_room,
                    //Some(device_id),
                    Some(light_template),
                    Some(device_address),
                    Some(bus_start_address),
                    manual_buttons,
                    presence_detectors,
                    touchscreen_whitebalance,
                    touchscreen_brightness,
                    old_state,
                ) = (
                    get_cell_content(row, room_column)
                        .map(Room::from_str)
                        .and_then(Result::ok),
                    get_cell_integer(row, light_idx_column).map(|id| id as u16),
                    //get_cell_content(row, light_id_column),
                    get_cell_content(row, template_column).and_then(|t| light_template_map.get(t)),
                    get_cell_content(row, device_address_column)
                        .map(Uid::from_str)
                        .and_then(Result::ok),
                    get_cell_integer(row, bus_start_address_column),
                    manual_button_columns
                        .iter()
                        .filter_map(|id| get_cell_content(row, *id))
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                    presence_detector_columns
                        .iter()
                        .filter_map(|id| get_cell_content(row, *id))
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                    get_cell_content(row, touchscreen_whitebalance_column),
                    get_cell_content(row, touchscreen_brightness_column),
                    get_cell_content(row, state_column),
                ) {
                    let row = row_idx + start_row;
                    let col = light_idx_column + start_column;
                    let coordinates = CellCoordinates { row, col };
                    device_ids_of_rooms.entry(room).or_default().push((
                        coordinates,
                        LightRowContent {
                            room,
                            light_template,
                            //device_id,
                            device_id_in_room,
                            device_address,
                            bus_start_address: bus_start_address as u16,
                            manual_buttons,
                            presence_detectors,
                            touchscreen_whitebalance,
                            touchscreen_brightness,
                        },
                    ));
                    update_state(
                        &mut updates,
                        light_config.sheet(),
                        CellCoordinates {
                            row: row_idx + start_row,
                            col: state_column + start_column,
                        },
                        device_address,
                        state,
                        old_state,
                    );
                    //info!("Room: {room:?}, idx: {coordinates}");
                }
            }
            let sheet_name = light_config.sheet();
            let light_device_rows =
                adjust_device_idx(device_ids_of_rooms, sheet_name, &mut updates).await?;
            write_updates_to_sheet(config, spreadsheet_methods, updates).await?;

            for light_row in light_device_rows {
                let dmx_bricklet_settings =
                    dmx_bricklets.entry(light_row.device_address).or_default();
                let template = light_row.light_template;

                let mut manual_buttons = light_row
                    .manual_buttons
                    .iter()
                    .flat_map(|name| dual_button_adresses.get(*name))
                    .copied()
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                manual_buttons.sort();
                let mut presence_detectors = light_row
                    .presence_detectors
                    .iter()
                    .flat_map(|name| motion_detector_adresses.get(*name))
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
                        let register = SwitchOutputKey::Light(DeviceInRoom {
                            room: light_row.room,
                            idx: light_row.device_id_in_room.unwrap(),
                        });
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
                        let register = BrightnessKey::Light(DeviceInRoom {
                            room: light_row.room,
                            idx: light_row.device_id_in_room.unwrap(),
                        });
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
                                        .and_then(|k| touchscreen_brightness.get(k))
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
                        let device_in_room = DeviceInRoom {
                            room: light_row.room,
                            idx: light_row.device_id_in_room.unwrap(),
                        };

                        let output_brightness_register = BrightnessKey::Light(device_in_room);
                        let whitebalance_register = if let Some(wb) = light_row
                            .touchscreen_whitebalance
                            .and_then(|k| touchscreen_whitebalances.get(k))
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
                                        .and_then(|k| touchscreen_brightness.get(k))
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
        }
    }
    Ok(())
}
trait DeviceIdxAccess {
    fn existing_id(&self) -> Option<u16>;
    fn update_id(&mut self, id: u16);
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
    single_button_adresses: &mut HashMap<Cow<'a, str>, SingleButtonKey>,
    dual_button_adresses: &mut HashMap<Cow<'a, str>, DualButtonKey>,
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
                    let device_key =
                        Cow::Owned(format!("{}, {}", button_row.button_id, subdevice_name));

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
                            single_button_adresses.insert(device_key, output);
                            current_input_idx += 1;
                        }
                        ButtonStyle::Dual => {
                            let output = DualButtonKey(sub_device_in_room);
                            io_bricklet_settings.push(ButtonSetting::Dual {
                                up_button: current_input_idx + 1,
                                down_button: current_input_idx,
                                output,
                            });
                            dual_button_adresses.insert(device_key, output);
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
