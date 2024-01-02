use std::time::Duration;
use std::{
    collections::{HashMap, HashSet},
    fmt::{Debug, Display, Formatter, Write},
    io,
    str::FromStr,
};

use futures::StreamExt;
use google_sheets4::{
    api::{BatchUpdateValuesRequest, CellData, GridData, Spreadsheet, ValueRange},
    hyper::{client::HttpConnector, Client},
    hyper_rustls::{self, HttpsConnector},
    oauth2::{authenticator::Authenticator, ServiceAccountAuthenticator},
    Sheets,
};
use log::error;
use prometheus::register;
use thiserror::Error;

use crate::data::registry::{
    BrightnessKey, DualButtonKey, LightColorKey, SingleButtonKey, SwitchOutputKey,
};
use crate::data::wiring::{
    ButtonSetting, Controllers, DmxConfigEntry, DmxSettings, DualInputDimmer, DualInputSwitch,
    IoSettings, MotionDetector, TinkerforgeDevices, Wiring,
};
use crate::data::{DeviceInRoom, SubDeviceInRoom, Uid};
use crate::{
    data::{
        settings::{GoogleError, CONFIG},
        Room,
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
pub async fn read_sheet_data() -> Result<Option<Wiring>, GoogleDataError> {
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

        let light_templates = config.light_templates();
        let light_config = config.light();
        let button_templates = config.button_templates();
        let button_config = config.buttons();

        let spreadsheet_methods = hub.spreadsheets();
        let (_, sheet) = spreadsheet_methods
            .get(config.spreadsheet_id())
            .add_scope("https://www.googleapis.com/auth/spreadsheets")
            .include_grid_data(true)
            .add_ranges(&format!(
                "{}!{}",
                light_config.sheet(),
                light_config.range()
            ))
            .add_ranges(&format!(
                "{}!{}",
                light_templates.sheet(),
                light_templates.range()
            ))
            .add_ranges(&format!(
                "{}!{}",
                button_config.sheet(),
                button_config.range()
            ))
            .add_ranges(&format!(
                "{}!{}",
                button_templates.sheet(),
                button_templates.range()
            ))
            .doit()
            .await?;
        let mut io_bricklets = HashMap::<_, Vec<_>>::new();
        let mut dmx_bricklets = HashMap::<_, Vec<_>>::new();
        let mut dual_input_dimmers = Vec::new();
        let mut dual_input_switches = Vec::new();
        let mut motion_detectors = Vec::new();

        let mut single_button_adresses = HashMap::new();
        let mut dual_button_adresses = HashMap::new();
        let mut button_template_map = HashMap::new();
        if let Some(button_templates_grid) = find_sheet_by_name(&sheet, button_templates.sheet()) {
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
        if let Some(button_grid) = find_sheet_by_name(&sheet, button_config.sheet()) {
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
                let [room_column, button_id_column, button_idx_column, type_column, device_address_column, first_input_idx_column] =
                    parse_headers(
                        header,
                        [
                            button_config.room_id(),
                            button_config.button_id(),
                            button_config.button_idx(),
                            button_config.button_type(),
                            button_config.device_address(),
                            button_config.first_input_idx(),
                        ],
                    )
                    .map_err(GoogleDataError::ButtonHeader)?;
                let mut button_ids_of_rooms = HashMap::<_, Vec<_>>::new();
                for (row_idx, row) in rows {
                    if let (
                        Some(room),
                        Some(button_id),
                        button_id_in_room,
                        Some(button_template),
                        Some(device_address),
                        Some(first_input_idx),
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
                    ) {
                        let row = row_idx + start_row;
                        let col = button_idx_column + start_column;
                        let coordinates = CellCoordinates { row, col };
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
                let mut updates = Vec::new();
                let mut button_device_rows = Vec::new();
                for (devices) in button_ids_of_rooms.into_values() {
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
                for button_row in button_device_rows {
                    let io_bricklet_settings =
                        io_bricklets.entry(button_row.device_address).or_default();
                    let mut current_input_idx = button_row.first_input_idx;
                    for (subdevice_id, subdevice_name) in
                        button_row.button_template.sub_devices.iter().enumerate()
                    {
                        let device_key = format!("{}, {}", button_row.button_id, subdevice_name)
                            .into_boxed_str();

                        let sub_device_in_room = SubDeviceInRoom {
                            room: button_row.room,
                            device_idx: button_row.button_id_in_room.unwrap(),
                            sub_device_idx: subdevice_id as u16,
                        };
                        match button_row.button_template.style {
                            ButtonStyle::Single => {
                                let output = SingleButtonKey(sub_device_in_room);
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
        let mut light_template_map = HashMap::new();
        if let Some(light_templates_grid) = find_sheet_by_name(&sheet, light_templates.sheet()) {
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
                        get_cell_content(&row, name_column),
                        get_cell_content(&row, discriminator_column),
                    ) {
                        if discriminator == "Switch" {
                            light_template_map.insert(name, LightTemplateTypes::Switch);
                        }
                        if discriminator == "Dimm" {
                            light_template_map.insert(name, LightTemplateTypes::Dimm);
                        }
                        if discriminator == "DimmWhitebalance" {
                            if let (Some(warm_temperature), Some(cold_temperature)) = (
                                get_cell_integer(&row, warm_column),
                                get_cell_integer(&row, cold_column),
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
        if let Some(light_grid) = find_sheet_by_name(&sheet, light_config.sheet()) {
            struct LightRowContent<'a> {
                room: Room,
                light_template: &'a LightTemplateTypes,
                device_id: &'a str,
                device_id_in_room: Option<u16>,
                device_address: Uid,
                bus_start_address: u16,
                manual_buttons: Box<[&'a str]>,
                presence_detectors: Box<[&'a str]>,
                touchscreen_whitebalance: Option<&'a str>,
                touchscreen_brightness: Option<&'a str>,
            }
            let (start_row, start_column, mut rows) = get_grid_and_coordinates(light_grid);
            if let Some((_, header)) = rows.next() {
                let [room_column, light_id_column, light_idx_column, template_column, device_address_column, bus_start_address_column, touchscreen_whitebalance_column, touchscreen_brightness_column] =
                    parse_headers(
                        header,
                        [
                            light_config.room_id(),
                            light_config.light_id(),
                            light_config.light_idx(),
                            light_config.template(),
                            light_config.device_address(),
                            light_config.bus_start_address(),
                            light_config.touchscreen_whitebalance(),
                            light_config.touchscreen_brightness(),
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
                for (row_idx, row) in rows {
                    if let (
                        Some(room),
                        device_id_in_room,
                        Some(device_id),
                        Some(light_template),
                        Some(device_address),
                        Some(bus_start_address),
                        manual_buttons,
                        presence_detectors,
                        touchscreen_whitebalance,
                        touchscreen_brightness,
                    ) = (
                        get_cell_content(row, room_column)
                            .map(Room::from_str)
                            .and_then(Result::ok),
                        get_cell_integer(row, light_idx_column).map(|id| id as u16),
                        get_cell_content(row, light_id_column),
                        get_cell_content(row, template_column)
                            .and_then(|t| light_template_map.get(t)),
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
                    ) {
                        let row = row_idx + start_row;
                        let col = light_idx_column + start_column;
                        let coordinates = CellCoordinates { row, col };
                        device_ids_of_rooms.entry(room).or_default().push((
                            coordinates,
                            LightRowContent {
                                room,
                                light_template,
                                device_id,
                                device_id_in_room,
                                device_address,
                                bus_start_address: bus_start_address as u16,
                                manual_buttons,
                                presence_detectors,
                                touchscreen_whitebalance,
                                touchscreen_brightness,
                            },
                        ));
                        //info!("Room: {room:?}, idx: {coordinates}");
                    }
                }
                let mut updates = Vec::new();
                let mut light_device_rows = Vec::new();
                for (devices) in device_ids_of_rooms.into_values() {
                    let mut occupied_ids = HashSet::new();
                    let mut remaining_devices = Vec::with_capacity(devices.len());
                    for (coordinates, light_row) in devices {
                        if if let Some(id) = light_row.device_id_in_room {
                            if occupied_ids.contains(&id) {
                                true
                            } else {
                                occupied_ids.insert(id);
                                false
                            }
                        } else {
                            true
                        } {
                            remaining_devices.push((coordinates, light_row));
                        } else {
                            light_device_rows.push(light_row);
                        };
                    }
                    let mut next_id = 0;
                    for (coordinates, mut row) in remaining_devices {
                        while occupied_ids.contains(&next_id) {
                            next_id += 1;
                        }
                        row.device_id_in_room = Some(next_id);
                        light_device_rows.push(row);
                        updates.push(ValueRange {
                            major_dimension: None,
                            range: Some(format!("{}!{}", light_config.sheet(), coordinates)),
                            values: Some(vec![vec![next_id.into()]]),
                        });
                        next_id += 1;
                    }
                }
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
                for light_row in light_device_rows {
                    let dmx_bricklet_settings =
                        dmx_bricklets.entry(light_row.device_address).or_default();
                    let template = light_row.light_template;

                    let manual_buttons = light_row
                        .manual_buttons
                        .iter()
                        .flat_map(|name| dual_button_adresses.get(*name))
                        .copied()
                        .collect::<Vec<_>>()
                        .into_boxed_slice();
                    let presence_detectors = light_row
                        .presence_detectors
                        .iter()
                        .flat_map(|name| single_button_adresses.get(*name))
                        .copied()
                        .collect::<Vec<_>>()
                        .into_boxed_slice();
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
                                    motion_detectors.push(MotionDetector::Switch {
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
                                    motion_detectors.push(MotionDetector::Dimmer {
                                        input: presence_detectors,
                                        output: register,
                                        brightness: None,
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
                            let brightness_register = BrightnessKey::Light(DeviceInRoom {
                                room: light_row.room,
                                idx: light_row.device_id_in_room.unwrap(),
                            });
                            let whitebalance_register = LightColorKey::Light(DeviceInRoom {
                                room: light_row.room,
                                idx: light_row.device_id_in_room.unwrap(),
                            });
                            dmx_bricklet_settings.push(DmxConfigEntry::DimmWhitebalance {
                                brightness_register,
                                whitebalance_register,
                                warm_channel: light_row.bus_start_address,
                                cold_channel: light_row.bus_start_address + 1,
                                warm_mireds: *warm_temperature,
                                cold_mireds: *cold_temperature,
                            });
                            if manual_buttons.is_empty() {
                                if !presence_detectors.is_empty() {
                                    motion_detectors.push(MotionDetector::Dimmer {
                                        input: presence_detectors,
                                        output: brightness_register,
                                        brightness: None,
                                        switch_off_time: auto_switch_off_time,
                                    });
                                }
                            } else {
                                dual_input_dimmers.push(DualInputDimmer {
                                    input: manual_buttons,
                                    output: brightness_register,
                                    auto_switch_off_time,
                                    presence: presence_detectors,
                                });
                            }
                        }
                    }
                }
            }
        }
        Some(Wiring {
            controllers: Controllers {
                dual_input_dimmers: dual_input_dimmers.into_boxed_slice(),
                dual_input_switches: dual_input_switches.into_boxed_slice(),
                motion_detectors: motion_detectors.into_boxed_slice(),
            },
            tinkerforge_devices: TinkerforgeDevices {
                lcd_screens: Default::default(),
                dmx_bricklets: dmx_bricklets
                    .into_iter()
                    .map(|(uid, settings)| {
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
                    .map(|(uid, settings)| {
                        (
                            uid,
                            IoSettings {
                                entries: settings.into_boxed_slice(),
                            },
                        )
                    })
                    .collect(),
                motion_detectors: Default::default(),
                relays: Default::default(),
                temperature_sensors: Default::default(),
            },
        })
    } else {
        None
    };
    Ok(x)
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
struct HeaderError(Box<[Box<str>]>);
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
    find_indizes_of_headers(row, &header_columns, &mut found_header_ids);
    let mut missing_headers = Vec::with_capacity(header_columns.len());
    let mut ret = Vec::with_capacity(header_columns.len());
    for (header_idx, col_idx) in found_header_ids.into_iter().enumerate() {
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
        let result = read_sheet_data().await;
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
