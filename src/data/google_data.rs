use crate::shelly::common::DeviceId;
use crate::shelly::shelly::ComponentEntry;
use chrono::{format::StrftimeItems, DateTime, Local};
use google_sheets4::{
    api::{BatchUpdateValuesRequest, CellData, SpreadsheetMethods, ValueRange},
    hyper::{client::HttpConnector, Client},
    hyper_rustls::{self, HttpsConnector},
    oauth2::ServiceAccountAuthenticator,
    Sheets,
};
use log::{debug, error, info};
use serde::Deserialize;
use serde_json::Value;
use std::iter::Map;
use std::{
    array,
    borrow::Cow,
    collections::{BTreeMap, HashMap, HashSet},
    fmt::{Debug, Display, Formatter, Write},
    io,
    net::IpAddr,
    str::FromStr,
    time::{Duration, SystemTime},
    vec::IntoIter,
};
use thiserror::Error;
use tinkerforge_async::{base58::Uid, ip_connection::Version, DeviceIdentifier};

use crate::devices::shelly::shelly::{ComponentAddress, ComponentKey};
use crate::{
    data::{
        registry::{
            BrightnessKey, ClockKey, DualButtonKey, LightColorKey, SingleButtonKey,
            SwitchOutputKey, TemperatureKey,
        },
        settings::{GoogleEndpointData, GoogleError, GoogleSheet, CONFIG},
        state::{
            BrickletConnectionData, BrickletMetadata, ConnectionState, SpitfpErrorCounters, State,
        },
        wiring::{
            ButtonSetting, Controllers, DmxConfigEntry, DmxSettings, DualInputDimmer,
            DualInputSwitch, HeatController, IoSettings, MotionDetector, MotionDetectorSettings,
            Orientation, RelayChannelEntry, RelaySettings, RingController, ScreenSettings,
            ShellyDevices, TemperatureSettings, TinkerforgeDevices, Wiring,
        },
        DeviceInRoom, Room, SubDeviceInRoom,
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

struct ButtonTemplateTypes {
    style: ButtonStyle,
    sub_devices: Box<[Box<str>]>,
}

struct ParserContext<'a> {
    config: &'a GoogleSheet,
    spreadsheet_methods: SpreadsheetMethods<'a, HttpsConnector<HttpConnector>>,
    state: Option<&'a State>,
    timestamp_format: StrftimeItems<'a>,
}

pub async fn read_sheet_data(state: Option<&State>) -> Result<Option<Wiring>, GoogleDataError> {
    Ok(if let Some(config) = &CONFIG.google_sheet {
        let secret = config.read_secret().await?;
        let auth = ServiceAccountAuthenticator::builder(secret).build().await?;

        let connector_builder = hyper_rustls::HttpsConnectorBuilder::new();

        let client = Client::builder().build(
            connector_builder
                .with_native_roots()
                .https_or_http()
                .enable_http1()
                //.enable_http2()
                .build(),
        );

        let hub = Sheets::new(client, auth);

        let spreadsheet_methods: SpreadsheetMethods<HttpsConnector<HttpConnector>> =
            hub.spreadsheets();
        let timestamp_format = StrftimeItems::new(config.timestamp_format());
        let context = ParserContext {
            config,
            spreadsheet_methods,
            state,
            timestamp_format,
        };

        let mut builder: GoogleSheetWireBuilder = Default::default();
        builder.parse_tinkerforge_endpoints(&context).await?;
        builder.parse_shelly_endpoints(&context).await?;
        builder.parse_buttons(&context).await?;
        builder.parse_motion_detectors(&context).await?;
        builder.parse_controllers(&context).await?;
        builder.parse_lights(&context).await?;
        builder.parse_relays(&context).await?;

        builder.update_available_bricklets(&context).await?;

        builder.write_updates_to_sheet(&context).await?;

        Some(builder.build_wiring())
    } else {
        None
    })
}

#[derive(Default)]
struct GoogleSheetWireBuilder {
    io_bricklets: BTreeMap<Uid, Vec<ButtonSetting>>,
    dmx_bricklets: BTreeMap<Uid, Vec<DmxConfigEntry>>,
    lcd_screens: BTreeMap<Uid, ScreenSettings>,
    temperature_sensors: BTreeMap<Uid, TemperatureSettings>,
    motion_detector_sensors: BTreeMap<Uid, MotionDetectorSettings>,
    relays: BTreeMap<Uid, RelaySettings>,
    tinkerforge_endpoints: Vec<IpAddr>,
    shelly_endpoints: Vec<IpAddr>,

    dual_input_dimmers: Vec<DualInputDimmer>,
    dual_input_switches: Vec<DualInputSwitch>,
    motion_detectors: Vec<MotionDetector>,
    heat_controllers: Vec<HeatController>,
    ring_controllers: Vec<RingController>,

    single_button_adresses: HashMap<Box<str>, SingleButtonKey>,
    dual_button_adresses: HashMap<Box<str>, DualButtonKey>,
    heat_outputs_addresses: HashMap<Box<str>, SwitchOutputKey>,
    touchscreen_whitebalance_addresses: HashMap<Box<str>, LightColorKey>,
    touchscreen_brightness_addresses: HashMap<Box<str>, BrightnessKey>,
    motion_detector_adresses: HashMap<Box<str>, SingleButtonKey>,

    endpoint_names: HashMap<IpAddr, Box<str>>,

    updates: Vec<ValueRange>,
}

enum Connection {
    Master { position: u8 },
    Isolator { parent: Uid, position: char },
}

impl GoogleSheetWireBuilder {
    async fn update_available_bricklets<'a>(
        &'a mut self,
        context: &'a ParserContext<'a>,
    ) -> Result<(), GoogleDataError> {
        if let Some(state) = context.state {
            {
                info!("Update available shelly components");
                let config = context.config.available_shelly_components();
                let mut shelly_devices: Box<[_]> = state.shelly_components().iter().collect();
                shelly_devices.sort_by_key(|(key, _)| *key);
                Self::override_table(
                    &mut self.updates,
                    GoogleTable::connect(
                        &context.spreadsheet_methods,
                        [config.device(), config.address(), config.component_type()],
                        [],
                        context.config.spreadsheet_id(),
                        config.sheet(),
                        config.range(),
                    )
                    .await?,
                    shelly_devices
                        .iter()
                        .flat_map(|(ip, (device_id, components))| {
                            let mut components: Box<[_]> = components.iter().collect();
                            components.sort_by_key(|e| e.key());
                            let rows: Vec<[Value; 3]> = components
                                .iter()
                                .map(|component| {
                                    [
                                        device_id.to_string().into(),
                                        ComponentAddress {
                                            device: *device_id,
                                            key: component.key(),
                                        }
                                        .to_string()
                                        .into(),
                                        component.type_name().into(),
                                    ]
                                })
                                .collect();
                            rows.into_iter()
                        }),
                );
            }
            {
                info!("Update available bricklets");
                let config = context.config.available_bricklets();
                let mut output_table = GoogleTable::connect(
                    &context.spreadsheet_methods,
                    [
                        config.endpoint(),
                        config.master_id(),
                        config.connector(),
                        config.uid(),
                        config.device_type(),
                        config.hardware_version(),
                        config.firmware_version(),
                        config.io_ports(),
                        config.temp_sensor(),
                        config.motion_detectors(),
                        config.display(),
                        config.dmx_channels(),
                        config.relays(),
                        config.connection_failed_counters(),
                        config.errors(),
                    ],
                    [],
                    context.config.spreadsheet_id(),
                    config.sheet(),
                    config.range(),
                )
                .await?;
                #[derive(Eq, PartialEq)]
                struct BrickletRow<'a> {
                    endpoint_addr: IpAddr,
                    master_idx: Option<u8>,
                    connector: char,
                    uid: Uid,
                    device_type: DeviceIdentifier,
                    hardware_version: Version,
                    firmware_version: Version,
                    state: ConnectionState,
                    last_change: SystemTime,
                    connection_failed_counter: u32,
                    error_counters: &'a HashMap<Option<char>, SpitfpErrorCounters>,
                }
                let master_positions = state
                    .bricklets()
                    .iter()
                    .filter_map(|(uid, BrickletConnectionData { metadata, .. })| {
                        metadata.as_ref().and_then(
                            |BrickletMetadata {
                                 position,
                                 device_identifier,
                                 connected_uid,
                                 ..
                             }| {
                                match *device_identifier {
                                    DeviceIdentifier::MasterBrick => Some((
                                        *uid,
                                        Connection::Master {
                                            position: *position as u8 - b'0',
                                        },
                                    )),
                                    DeviceIdentifier::IsolatorBricklet => Some((
                                        *uid,
                                        Connection::Isolator {
                                            parent: *connected_uid,
                                            position: *position,
                                        },
                                    )),
                                    _ => None,
                                }
                            },
                        )
                    })
                    .collect::<HashMap<_, _>>();

                let mut rows = state
                    .bricklets()
                    .iter()
                    .filter_map(
                        |(
                            uid,
                            BrickletConnectionData {
                                state,
                                last_change,
                                endpoint,
                                metadata,
                                connection_failed_counter,
                                error_counters,
                                session: _,
                            },
                        )| {
                            if state != &ConnectionState::Connected {
                                return None;
                            }
                            metadata.as_ref().map(
                                |BrickletMetadata {
                                     connected_uid,
                                     position,
                                     hardware_version,
                                     firmware_version,
                                     device_identifier,
                                 }| {
                                    let (master_idx, connector) = Self::find_connection(
                                        &master_positions,
                                        *connected_uid,
                                        *position,
                                    );
                                    BrickletRow {
                                        endpoint_addr: *endpoint,
                                        master_idx,
                                        connector,
                                        uid: *uid,
                                        device_type: *device_identifier,
                                        hardware_version: *hardware_version,
                                        firmware_version: *firmware_version,
                                        state: *state,
                                        last_change: *last_change,
                                        connection_failed_counter: *connection_failed_counter,
                                        error_counters,
                                    }
                                },
                            )
                        },
                    )
                    .collect::<Vec<_>>();
                rows.sort_by(|r1, r2| {
                    r1.endpoint_addr
                        .cmp(&r2.endpoint_addr)
                        .then(r1.master_idx.cmp(&r2.master_idx))
                        .then(r1.connector.cmp(&r2.connector))
                        .then(r1.device_type.cmp(&r2.device_type))
                        .then(r1.uid.cmp(&r2.uid))
                });

                let rows = rows.into_iter().map(|row| {
                    let io_count: Option<u16> = self.io_bricklets.get(&row.uid).map(|v| {
                        v.iter()
                            .map(|s| match s {
                                ButtonSetting::Dual { .. } => 2,
                                ButtonSetting::Single { .. } => 1,
                            })
                            .sum()
                    });
                    let temperature_sensor = self.temperature_sensors.contains_key(&row.uid);
                    let motion_detector = self.motion_detector_sensors.contains_key(&row.uid);
                    let lcd_screen = self.lcd_screens.contains_key(&row.uid);
                    let dmx_count: Option<u16> = self.dmx_bricklets.get(&row.uid).map(|v| {
                        v.iter()
                            .map(|s| match s {
                                DmxConfigEntry::Dimm { .. } => 1,
                                DmxConfigEntry::DimmWhitebalance { .. } => 2,
                                DmxConfigEntry::Switch { .. } => 1,
                            })
                            .sum()
                    });
                    let mut error_description = String::new();
                    for (connection, counters) in row.error_counters {
                        if counters.error_count_message_checksum > 0 {
                            Self::append_counter(
                                &mut error_description,
                                *connection,
                                "msg",
                                counters.error_count_message_checksum,
                            );
                        }
                        if counters.error_count_ack_checksum > 0 {
                            Self::append_counter(
                                &mut error_description,
                                *connection,
                                "ack",
                                counters.error_count_ack_checksum,
                            );
                        }
                        if counters.error_count_overflow > 0 {
                            Self::append_counter(
                                &mut error_description,
                                *connection,
                                "overflow",
                                counters.error_count_overflow,
                            );
                        }
                        if counters.error_count_frame > 0 {
                            Self::append_counter(
                                &mut error_description,
                                *connection,
                                "frame",
                                counters.error_count_frame,
                            );
                        }
                    }
                    let relay_count = self.relays.get(&row.uid).map(|rs| rs.entries.len());
                    [
                        (&**self
                            .endpoint_names
                            .get(&row.endpoint_addr)
                            .map(Cow::Borrowed)
                            .unwrap_or_else(|| {
                                Cow::Owned(row.endpoint_addr.to_string().into_boxed_str())
                            })
                            .as_ref())
                            .into(),
                        row.master_idx.unwrap_or_default().into(),
                        row.connector.to_string().into(),
                        row.uid.to_string().into(),
                        identify_device_type(row.device_type),
                        row.hardware_version.to_string().into(),
                        row.firmware_version.to_string().into(),
                        io_count.into(),
                        show_bool(temperature_sensor),
                        show_bool(motion_detector),
                        show_bool(lcd_screen),
                        dmx_count.into(),
                        relay_count.into(),
                        row.connection_failed_counter.into(),
                        error_description.into(),
                    ]
                });
                Self::override_table(&mut self.updates, output_table, rows);
            }
        }
        Ok(())
    }

    fn override_table<const N: usize, I>(
        updates: &mut Vec<ValueRange>,
        mut output_table: GoogleTable<N, 0>,
        rows: I,
    ) where
        I: Iterator<Item = [Value; N]>,
    {
        for data_row in rows {
            if let Some((table_row, _)) = output_table.next() {
                for (cell, value) in table_row.iter().zip(data_row.into_iter()) {
                    if Some(&value).filter(|value| !value.is_null() && value.as_str() != Some(""))
                        != cell.get_value().as_ref()
                    {
                        debug!(
                            "Update {}: {:?}->{:?}",
                            cell.coordinates,
                            cell.get_value().as_ref(),
                            value
                        );
                        updates.push(cell.override_cell(value));
                    }
                }
            } else {
                output_table.append_row((data_row, []), |v| updates.push(v));
            }
        }
        output_table.clean_remaining_rows(|v| updates.push(v));
    }

    fn append_counter(
        error_description: &mut String,
        connection: Option<char>,
        counter_name: &str,
        counter: u32,
    ) {
        if !error_description.is_empty() {
            error_description.push_str(", ")
        }
        error_description.push_str(counter_name);
        if let Some(ch) = connection {
            error_description.push('(');
            error_description.push(ch);
            error_description.push_str("): ");
        } else {
            error_description.push_str(": ")
        }
        error_description.push_str(&format!("{counter}"));
    }

    fn find_connection(
        master_positions: &HashMap<Uid, Connection>,
        connected_uid: Uid,
        position: char,
    ) -> (Option<u8>, char) {
        let (master_idx, connector) = match master_positions.get(&connected_uid) {
            None => (None, position),
            Some(Connection::Master {
                position: master_position,
            }) => (Some(*master_position), position),
            Some(Connection::Isolator { parent, position }) => {
                Self::find_connection(master_positions, *parent, *position)
            }
        };
        (master_idx, connector)
    }

    async fn write_updates_to_sheet<'a>(
        &'a mut self,
        context: &'a ParserContext<'a>,
    ) -> Result<(), GoogleDataError> {
        let updates: Vec<_> = self.updates.drain(..).collect();

        if !updates.is_empty() {
            let update = BatchUpdateValuesRequest {
                data: Some(updates),
                include_values_in_response: None,
                response_date_time_render_option: None,
                response_value_render_option: None,
                value_input_option: Some("RAW".to_string()),
            };
            context
                .spreadsheet_methods
                .values_batch_update(update, context.config.spreadsheet_id())
                .doit()
                .await?;
        }
        Ok(())
    }

    async fn parse_tinkerforge_endpoints<'a>(
        &'a mut self,
        context: &'a ParserContext<'a>,
    ) -> Result<(), GoogleDataError> {
        let endpoints_config = context.config.tinkerforge_endpoints();
        for endpoint in self
            .parse_ip_endpoints(context, endpoints_config)
            .await?
            .iter()
        {
            self.tinkerforge_endpoints.push(*endpoint);
        }
        Ok(())
    }
    async fn parse_shelly_endpoints<'a>(
        &'a mut self,
        context: &'a ParserContext<'a>,
    ) -> Result<(), GoogleDataError> {
        let endpoints_config = context.config.shelly_endpoints();
        for endpoint in self
            .parse_ip_endpoints(context, endpoints_config)
            .await?
            .iter()
        {
            self.shelly_endpoints.push(*endpoint);
        }
        Ok(())
    }

    async fn parse_ip_endpoints<'a>(
        &'a mut self,
        context: &'a ParserContext<'a>,
        endpoints_config: &GoogleEndpointData,
    ) -> Result<Box<[IpAddr]>, GoogleDataError> {
        let mut target_endpoints = Vec::new();
        for (address, state_cell, place) in GoogleTable::connect(
            &context.spreadsheet_methods,
            [
                endpoints_config.address(),
                endpoints_config.state(),
                endpoints_config.place(),
            ],
            [],
            context.config.spreadsheet_id(),
            endpoints_config.sheet(),
            endpoints_config.range(),
        )
        .await?
        .filter_map(|([address, state, place], _)| {
            address
                .get_content()
                .map(IpAddr::from_str)
                .and_then(Result::ok)
                .map(|ip| (ip, state, place.get_content().unwrap_or_default().into()))
        }) {
            target_endpoints.push(address);
            self.endpoint_names.insert(address, place);
            if let Some(update) = context
                .state
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
                self.updates.push(update);
            }
        }
        Ok(target_endpoints.into_boxed_slice())
    }

    fn build_wiring(self) -> Wiring {
        let GoogleSheetWireBuilder {
            io_bricklets,
            dmx_bricklets,
            lcd_screens,
            temperature_sensors,
            motion_detector_sensors,
            relays,
            mut tinkerforge_endpoints,
            mut shelly_endpoints,
            mut dual_input_dimmers,
            mut dual_input_switches,
            mut motion_detectors,
            mut heat_controllers,
            mut ring_controllers,
            ..
        } = self;

        dual_input_dimmers.sort();
        dual_input_switches.sort();
        motion_detectors.sort();
        heat_controllers.sort();
        ring_controllers.sort();
        tinkerforge_endpoints.sort();
        shelly_endpoints.sort();
        Wiring {
            controllers: Controllers {
                dual_input_dimmers: dual_input_dimmers.into_boxed_slice(),
                dual_input_switches: dual_input_switches.into_boxed_slice(),
                motion_detectors: motion_detectors.into_boxed_slice(),
                heat_controllers: heat_controllers.into_boxed_slice(),
                ring_controllers: ring_controllers.into_boxed_slice(),
            },
            tinkerforge_devices: TinkerforgeDevices {
                endpoints: tinkerforge_endpoints.into_boxed_slice(),
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
            shelly_devices: ShellyDevices {
                endpoints: shelly_endpoints.into_boxed_slice(),
            },
        }
    }

    async fn parse_relays<'a>(
        &mut self,
        context: &'a ParserContext<'a>,
    ) -> Result<(), GoogleDataError> {
        let relay_configs = context.config.relays();
        let mut relay_channels = HashMap::<_, Vec<_>>::new();
        let mut device_ids_of_rooms = HashMap::<_, Vec<_>>::new();

        for (room, idx, uid, channel, temperature, ring_button, old_state) in GoogleTable::connect(
            &context.spreadsheet_methods,
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
            context.config.spreadsheet_id(),
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
                            .and_then(|k| self.heat_outputs_addresses.get(k))
                            .copied(),
                        button
                            .get_content()
                            .and_then(|k| self.single_button_adresses.get(k))
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
            update_state_new(
                |v| self.updates.push(v),
                context.state,
                &old_state,
                uid,
                &context.timestamp_format,
            );
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
        let ring_rows = fill_device_idx(|v| self.updates.push(v), device_ids_of_rooms);
        for (device, row) in ring_rows {
            let index = SwitchOutputKey::Bell(device);
            self.ring_controllers.push(RingController {
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
            self.relays.insert(
                uid,
                RelaySettings {
                    entries: channels.into_boxed_slice(),
                },
            );
        }
        Ok(())
    }

    async fn parse_controllers<'a>(
        &mut self,
        context: &'a ParserContext<'a>,
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
        let controllers = context.config.room_controllers();
        let mut device_ids_of_rooms = HashMap::<_, Vec<_>>::new();

        for (room, controller_id, idx, orientation, touchscreen, temp_sensor, enable_heatcontrol, enable_whitebalance_control, enable_brighness_control, touchscreen_state, temperature_state) in GoogleTable::connect(
            &context.spreadsheet_methods,
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
            ], [],
            context.config.spreadsheet_id(),
            controllers.sheet(),
            controllers.range(),
        ).await?.filter_map(|([room, id, idx, orientation, touchscreen, temperature,
        heat_control, whitebalance, brightness, touchscreen_state, temperature_state], _)| {
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
                    |s| self.updates.push(s), context.state, &touchscreen_state, uid
                    , &context.timestamp_format);
            }
            if let Some(uid) = temp_sensor {
                update_state_new(|s| self.updates.push(s), context.state, &temperature_state, uid, &context.timestamp_format)
            }
        }
        let controller_rows = fill_device_idx(|s| self.updates.push(s), device_ids_of_rooms);
        for (device_idx, row) in controller_rows {
            let current_temperature_key = if let Some(uid) = row.temp_sensor {
                let output = TemperatureKey::CurrentTemperature(device_idx);
                self.temperature_sensors
                    .insert(uid, TemperatureSettings { output });
                Some(output)
            } else {
                None
            };
            let adjust_temperature_key = if row.enable_heatcontrol {
                if let Some(current_value_input) = current_temperature_key {
                    let target_value_input = TemperatureKey::TargetTemperature(device_idx);
                    let output = SwitchOutputKey::Heat(device_idx);
                    self.heat_controllers.push(HeatController {
                        current_value_input,
                        target_value_input,
                        output,
                    });
                    self.heat_outputs_addresses.insert(row.id.clone(), output);
                    Some(target_value_input)
                } else {
                    None
                }
            } else {
                None
            };
            let light_color_key = if row.enable_whitebalance_control {
                let key = LightColorKey::TouchscreenController(device_idx);
                self.touchscreen_whitebalance_addresses
                    .insert(row.id.clone(), key);
                Some(key)
            } else {
                None
            };
            let brightness_key = if row.enable_brighness_control {
                let key = BrightnessKey::TouchscreenController(device_idx);
                self.touchscreen_brightness_addresses.insert(row.id, key);
                Some(key)
            } else {
                None
            };
            if let Some(uid) = row.touchscreen {
                self.lcd_screens.insert(
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

        Ok(())
    }

    async fn parse_motion_detectors<'a>(
        &mut self,
        context: &'a ParserContext<'a>,
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

        let md_config = context.config.motion_detectors();
        let mut device_ids_of_rooms = HashMap::<_, Vec<_>>::new();

        for (room, device_address, id, idx, state_cell) in GoogleTable::connect(
            &context.spreadsheet_methods,
            [
                md_config.room_id(),
                md_config.device_address(),
                md_config.id(),
                md_config.idx(),
                md_config.state(),
            ],
            [],
            context.config.spreadsheet_id(),
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
            update_state_new(
                |v| self.updates.push(v),
                context.state,
                &state_cell,
                device_address,
                &context.timestamp_format,
            );
        }
        let motion_detector_rows = fill_device_idx(|v| self.updates.push(v), device_ids_of_rooms);

        for (device_idx, row) in motion_detector_rows {
            let key = SingleButtonKey::MotionDetector(device_idx);
            self.motion_detector_sensors
                .insert(row.device_address, MotionDetectorSettings { output: key });
            self.motion_detector_adresses.insert(row.id, key);
        }
        Ok(())
    }

    async fn parse_lights<'a>(
        &mut self,
        context: &'a ParserContext<'a>,
    ) -> Result<(), GoogleDataError> {
        let light_templates = context.config.light_templates();
        let mut light_template_map = HashMap::new();
        for ([name, discriminator, warm, cold], _) in GoogleTable::connect(
            &context.spreadsheet_methods,
            [
                light_templates.name_column(),
                light_templates.discriminator_column(),
                light_templates.temperature_warm_column(),
                light_templates.temperature_cold_column(),
            ],
            [],
            context.config.spreadsheet_id(),
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
        let mut device_ids_of_rooms = HashMap::<_, Vec<_>>::new();
        if let Some(light_config) = context.config.light_tinkerforge() {
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

            for (
                [room, light_idx, template, address, start_channel, whitebalance, brightness, old_state],
                [buttons, presence_detectors],
            ) in GoogleTable::connect(
                &context.spreadsheet_methods,
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
                context.config.spreadsheet_id(),
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
                        .map(<&str>::into)
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                    presence_detectors
                        .iter()
                        .filter_map(|cell| cell.get_content())
                        .filter(|s| !s.is_empty())
                        .map(<&str>::into)
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                    whitebalance.get_content().map(<&str>::into),
                    brightness.get_content().map(<&str>::into),
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
                    update_state_new(
                        |v| self.updates.push(v),
                        context.state,
                        &old_state,
                        device_address,
                        &context.timestamp_format,
                    );
                    //info!("Room: {room:?}, idx: {coordinates}");
                }
            }
        }
        let light_device_rows = fill_device_idx(|v| self.updates.push(v), device_ids_of_rooms);

        for (device_idx, light_row) in light_device_rows {
            let dmx_bricklet_settings = self
                .dmx_bricklets
                .entry(light_row.device_address)
                .or_default();
            let template = light_row.light_template;

            let mut manual_buttons = light_row
                .manual_buttons
                .iter()
                .flat_map(|name| self.dual_button_adresses.get(name))
                .copied()
                .collect::<Vec<_>>()
                .into_boxed_slice();
            manual_buttons.sort();
            let mut presence_detectors = light_row
                .presence_detectors
                .iter()
                .flat_map(|name| self.motion_detector_adresses.get(name))
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
                            self.motion_detectors.push(MotionDetector::Switch {
                                input: presence_detectors,
                                output: register,
                                switch_off_time: auto_switch_off_time,
                            });
                        }
                    } else {
                        self.dual_input_switches.push(DualInputSwitch {
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
                            self.motion_detectors.push(MotionDetector::Dimmer {
                                input: presence_detectors,
                                output: register,
                                brightness: light_row
                                    .touchscreen_brightness
                                    .and_then(|k| self.touchscreen_brightness_addresses.get(&k))
                                    .copied(),
                                switch_off_time: auto_switch_off_time,
                            });
                        }
                    } else {
                        self.dual_input_dimmers.push(DualInputDimmer {
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
                        .and_then(|k| self.touchscreen_whitebalance_addresses.get(&k))
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
                            self.motion_detectors.push(MotionDetector::Dimmer {
                                input: presence_detectors,
                                output: output_brightness_register,
                                brightness: light_row
                                    .touchscreen_brightness
                                    .and_then(|k| self.touchscreen_brightness_addresses.get(&k))
                                    .copied(),
                                switch_off_time: auto_switch_off_time,
                            });
                        }
                    } else {
                        self.dual_input_dimmers.push(DualInputDimmer {
                            input: manual_buttons,
                            output: output_brightness_register,
                            auto_switch_off_time,
                            presence: presence_detectors,
                        });
                    }
                }
            }
        }

        Ok(())
    }

    async fn parse_buttons<'a>(
        &mut self,
        context: &'a ParserContext<'a>,
    ) -> Result<(), GoogleDataError> {
        let mut button_template_map = HashMap::<Box<str>, _>::new();
        let button_templates = context.config.button_templates();
        for ([name, sub_device, discriminator], _) in GoogleTable::connect(
            &context.spreadsheet_methods,
            [
                button_templates.name(),
                button_templates.sub_devices(),
                button_templates.discriminator(),
            ],
            [],
            context.config.spreadsheet_id(),
            button_templates.sheet(),
            button_templates.range(),
        )
        .await?
        {
            if let (Some(name), Some(sub_devices), Some(discriminator)) = (
                name.get_content().map(<&str>::into),
                sub_device.get_content(),
                discriminator.get_content(),
            ) {
                let sub_devices = sub_devices
                    .split(',')
                    .map(<&str>::into)
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                if let Some(style) = if discriminator == "Single" {
                    Some(ButtonStyle::Single)
                } else if discriminator == "Dual" {
                    Some(ButtonStyle::Dual)
                } else {
                    None
                } {
                    button_template_map.insert(name, ButtonTemplateTypes { style, sub_devices });
                }
            }
        }

        let button_config = context.config.buttons();
        struct ButtonRowContent<'a> {
            device_idx: DeviceIdxCell<'a>,
            button_template: &'a ButtonTemplateTypes,
            button_id: Box<str>,
            device_address: Uid,
            first_input_idx: u8,
        }
        impl<'a> DeviceIdxAccessNew<'a> for ButtonRowContent<'a> {
            fn id_cell<'b>(&'b mut self) -> &'b mut DeviceIdxCell<'a> {
                &mut self.device_idx
            }
        }
        let mut button_ids_of_rooms = HashMap::<_, Vec<_>>::new();

        for (
            [room, button, button_idx, button_type, device_address, first_input_idx, old_state],
            _,
        ) in GoogleTable::connect(
            &context.spreadsheet_methods,
            [
                button_config.room_id(),
                button_config.button_id(),
                button_config.button_idx(),
                button_config.button_type(),
                button_config.device_address(),
                button_config.first_input_idx(),
                button_config.state(),
            ],
            [],
            context.config.spreadsheet_id(),
            button_config.sheet(),
            button_config.range(),
        )
        .await?
        {
            if let (
                Some(room),
                Some(button_id),
                device_idx,
                Some(button_template),
                Some(device_address),
                Some(first_input_idx),
                state_data,
            ) = (
                room.get_content().map(Room::from_str).and_then(Result::ok),
                button.get_content().map(<&str>::into),
                DeviceIdxCell(button_idx),
                button_type
                    .get_content()
                    .and_then(|t| button_template_map.get(t)),
                device_address
                    .get_content()
                    .map(Uid::from_str)
                    .and_then(Result::ok),
                first_input_idx.get_integer().map(|id| id as u8),
                old_state,
            ) {
                update_state_new(
                    |v| self.updates.push(v),
                    context.state,
                    &state_data,
                    device_address,
                    &context.timestamp_format,
                );
                button_ids_of_rooms
                    .entry(room)
                    .or_default()
                    .push(ButtonRowContent {
                        button_template,
                        button_id,
                        device_idx,
                        device_address,
                        first_input_idx,
                    });
            }
        }
        let button_device_rows = fill_device_idx(|v| self.updates.push(v), button_ids_of_rooms);
        for (idx, button_row) in button_device_rows {
            let io_bricklet_settings = self
                .io_bricklets
                .entry(button_row.device_address)
                .or_default();
            let mut current_input_idx = button_row.first_input_idx;
            for (subdevice_id, subdevice_name) in
                button_row.button_template.sub_devices.iter().enumerate()
            {
                let device_key = format!("{}, {}", button_row.button_id, subdevice_name);

                let sub_device_in_room = SubDeviceInRoom {
                    room: idx.room,
                    device_idx: idx.idx,
                    sub_device_idx: subdevice_id as u16,
                };
                match button_row.button_template.style {
                    ButtonStyle::Single => {
                        let output = SingleButtonKey::Button(sub_device_in_room);
                        io_bricklet_settings.push(ButtonSetting::Single {
                            button: current_input_idx,
                            output,
                        });
                        self.single_button_adresses
                            .insert(device_key.into(), output);
                        current_input_idx += 1;
                    }
                    ButtonStyle::Dual => {
                        let output = DualButtonKey(sub_device_in_room);
                        io_bricklet_settings.push(ButtonSetting::Dual {
                            up_button: current_input_idx + 1,
                            down_button: current_input_idx,
                            output,
                        });
                        self.dual_button_adresses.insert(device_key.into(), output);
                        current_input_idx += 2;
                    }
                }
            }
        }
        Ok(())
    }
}

fn show_bool(value: bool) -> serde_json::Value {
    if value {
        "x".into()
    } else {
        "".into()
    }
}

fn identify_device_type(device_type: DeviceIdentifier) -> serde_json::Value {
    device_type.name().into()
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

fn update_state_new<F: FnMut(ValueRange)>(
    mut updater: F,
    current_state: Option<&State>,
    state_cell: &GoogleCellData,
    uid: Uid,
    timestamp_format: &StrftimeItems,
) {
    if let Some(update) = if let Some(current_state) = current_state {
        let new_text = match current_state.bricklet(&uid) {
            None => "".to_string(),
            Some(&BrickletConnectionData {
                ref state,
                last_change,
                endpoint,
                ref metadata,
                connection_failed_counter,
                ref error_counters,
                session,
            }) => {
                let timestamp = DateTime::<Local>::from(last_change);
                let timestamp = timestamp.format_with_items(timestamp_format.clone().into_iter());
                if let Some(BrickletMetadata {
                    connected_uid,
                    position,
                    hardware_version,
                    firmware_version,
                    device_identifier: _,
                }) = metadata
                {
                    format!("{state}, {timestamp}, {endpoint}; {connected_uid}, {position}, hw: {}, fw: {}", hardware_version, firmware_version)
                } else {
                    format!("{state}, {timestamp}, {endpoint}")
                }
            }
        };
        state_cell.create_content_update(&new_text)
    } else {
        state_cell.create_content_update("")
    } {
        updater(update);
    }
}

#[derive(Ord, PartialOrd, Eq, PartialEq)]
struct TinkerforgeVersion([u8; 3]);

impl Display for TinkerforgeVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0[0], self.0[1], self.0[2])
    }
}

struct GoogleTable<'a, const N: usize, const M: usize> {
    fixed_column_indizes: [usize; N],
    dynamic_column_indices: [Box<[usize]>; M],
    sheet_name: &'a str,
    rows: IntoIter<(usize, Vec<CellData>)>,
    start_row: usize,
    start_column: usize,
    next_new_row: usize,
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
    pub(crate) fn get_value(&self) -> Option<serde_json::Value> {
        self.data
            .as_ref()
            .and_then(|d| d.effective_value.as_ref())
            .and_then(|v| {
                if let Some(bool) = v.bool_value {
                    return Some(serde_json::Value::Bool(bool));
                }
                if let Some(number) = v.number_value {
                    if number.is_sign_positive() {
                        if number <= u64::MAX as f64
                            && (number - (number as u64 as f64)).abs() < f64::EPSILON
                        {
                            return Some(serde_json::Value::Number((number as u64).into()));
                        }
                    } else if number >= i64::MIN as f64
                        && (number - (number as i64 as f64)).abs() < f64::EPSILON
                    {
                        return Some(serde_json::Value::Number((number as i64).into()));
                    }
                    return serde_json::Number::from_f64(number).map(serde_json::Value::Number);
                }
                if let Some(text) = v.string_value.clone() {
                    return Some(serde_json::Value::String(text));
                }
                None
            })
    }
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
        let mut json_value = value.into();
        if json_value.is_null() {
            json_value = serde_json::Value::String("".to_string());
        }
        ValueRange {
            major_dimension: None,
            range: Some(format!("{}!{}", self.sheet, self.coordinates)),
            values: Some(vec![vec![json_value]]),
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
        let end_row = start_row + grid.row_data.as_ref().map(Vec::len).unwrap_or_default();
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
            next_new_row: end_row,
        })
    }
    fn append_row<F: FnMut(ValueRange)>(
        &mut self,
        row: ([serde_json::Value; N], [Box<[serde_json::Value]>; M]),
        mut updater: F,
    ) {
        let (static_cols, dynamic_cols) = row;
        for (value, col_idx) in static_cols
            .into_iter()
            .zip(self.fixed_column_indizes.iter().copied())
        {
            let coordinates = CellCoordinates {
                row: self.next_new_row,
                col: col_idx + self.start_column,
            };
            updater(ValueRange {
                major_dimension: None,
                range: Some(format!("{}!{}", self.sheet_name, coordinates)),
                values: Some(vec![vec![value]]),
            });
        }
        for (values, indizes) in dynamic_cols
            .into_iter()
            .zip(self.dynamic_column_indices.iter())
        {
            for (value, col_index) in values.into_vec().into_iter().zip(indizes.iter().copied()) {
                let coordinates = CellCoordinates {
                    row: self.next_new_row,
                    col: col_index + self.start_column,
                };
                updater(ValueRange {
                    major_dimension: None,
                    range: Some(format!("{}!{}", self.sheet_name, coordinates)),
                    values: Some(vec![vec![value]]),
                });
            }
        }
        self.next_new_row += 1;
    }
    fn clean_remaining_rows<F: FnMut(ValueRange)>(&mut self, mut updater: F) {
        for (static_cols, dynamic_cols) in self.by_ref() {
            for cell in static_cols {
                if cell.get_content().filter(|f| !f.is_empty()).is_some() {
                    updater(cell.override_cell(""));
                }
            }
            for cells in dynamic_cols {
                for cell in cells.iter() {
                    if cell.get_content().filter(|f| !f.is_empty()).is_some() {
                        updater(cell.override_cell(""));
                    }
                }
            }
        }
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
