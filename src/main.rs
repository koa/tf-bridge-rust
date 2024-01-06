use std::{collections::HashMap, error::Error, fmt::Debug, sync::Arc, time::Duration};

use actix_web::{get, App, HttpServer};
use actix_web_prometheus::PrometheusMetricsBuilder;
use env_logger::{Env, TimestampPrecision};
use log::{error, info};
use prometheus::{gather, Encoder, TextEncoder};
use thiserror::Error;
use tinkerforge_async::{
    base58::Base58,
    dmx_bricklet::DmxBricklet,
    error::TinkerforgeError,
    industrial_quad_relay_bricklet::IndustrialQuadRelayBricklet,
    io16_v2_bricklet::Io16V2Bricklet,
    ip_connection::{async_io::AsyncIpConnection, EnumerateResponse, EnumerationType},
    lcd_128x64_bricklet::Lcd128x64Bricklet,
    motion_detector_v2_bricklet::MotionDetectorV2Bricklet,
    temperature_v2_bricklet::TemperatureV2Bricklet,
};
use tokio::{join, net::ToSocketAddrs, pin, sync::mpsc, task, task::JoinHandle, time::sleep};
use tokio_stream::StreamExt;

use crate::{
    controller::light::{
        dual_input_dimmer, dual_input_switch, motion_detector, motion_detector_dimmer,
    },
    data::{
        google_data::read_sheet_data,
        registry::{BrightnessKey, DualButtonKey, EventRegistry, LightColorKey},
        settings::CONFIG,
        wiring::DmxConfigEntry,
        wiring::{
            Controllers, DmxSettings, DualInputDimmer, MotionDetector, TinkerforgeDevices, Wiring,
        },
        DeviceInRoom, Uid,
    },
    devices::{
        dmx_handler::handle_dmx, io_handler::handle_io16_v2,
        motion_detector::handle_motion_detector, relay::handle_quad_relay,
        screen_data_renderer::start_screen_thread, temperature::handle_temperature,
    },
    snapshot::{read_snapshot, write_snapshot},
    util::kelvin_2_mireds,
};

mod controller;
mod data;
mod devices;
mod icons;
mod util;

fn print_enumerate_response(response: &EnumerateResponse) {
    println!("UID:               {}", response.uid);
    println!("Enumeration Type:  {:?}", response.enumeration_type);

    if response.enumeration_type == EnumerationType::Disconnected {
        println!();
        return;
    }

    println!("Connected UID:     {}", response.connected_uid);
    println!("Position:          {}", response.position);
    println!(
        "Hardware Version:  {}.{}.{}",
        response.hardware_version[0], response.hardware_version[1], response.hardware_version[2]
    );
    println!(
        "Firmware Version:  {}.{}.{}",
        response.firmware_version[0], response.firmware_version[1], response.firmware_version[2]
    );
    println!("Device Identifier: {}", response.device_identifier);
    println!();
}
#[get("/health")]
async fn health() -> &'static str {
    "Ok"
}

#[derive(Error, Debug)]
enum TfBridgeError {
    #[error("Error communicating to device")]
    TinkerforgeError(#[from] TinkerforgeError),
}

async fn run_enumeration_listener<T: ToSocketAddrs>(
    addr: T,
    event_registry: EventRegistry,
    tinkerforge_devices: Arc<TinkerforgeDevices>,
) -> Result<(), TfBridgeError> {
    let ipcon = AsyncIpConnection::new(addr).await?;
    // Enumerate
    let stream = ipcon.clone().enumerate().await?;
    let mut running_threads: HashMap<_, mpsc::Sender<()>> = HashMap::new();
    pin!(stream);
    while let Some(paket) = stream.next().await {
        //print_enumerate_response(&paket);
        match paket.uid.base58_to_u32().map(Into::<Uid>::into) {
            Ok(uid) => {
                match paket.enumeration_type {
                    EnumerationType::Available | EnumerationType::Connected => {
                        match paket.device_identifier {
                            /*MasterBrick::DEVICE_IDENTIFIER => {
                                let mut brick = MasterBrick::new(&paket.uid, ipcon.clone());
                                let voltage = brick.get_stack_voltage().await? as f64 / 1000.0;
                                println!("Voltage: {voltage}V");
                                let current = brick.get_stack_current().await? as f64 / 1000.0;
                                println!("Current: {current}A");
                                let power = current * voltage;
                                println!("Power  : {power}W");
                                let extension_type = brick.get_extension_type(0).await?;
                                println!("Extension: {extension_type}");
                                let ethernet_config = brick.get_ethernet_configuration().await?;
                                println!("Eth Config: {ethernet_config:?}");
                                let ethernet_status = brick.get_ethernet_status().await?;
                                println!("Eth Status: {ethernet_status:?}");
                                let connection_type = brick.get_connection_type().await?;
                                println!("Conn Type: {connection_type}");
                                println!();
                            }*/
                            Lcd128x64Bricklet::DEVICE_IDENTIFIER => {
                                if let Some(screen_settings) =
                                    tinkerforge_devices.lcd_screens.get(&uid)
                                {
                                    info!("Found LCD Device: {}", paket.uid);
                                    register_handle(
                                        &mut running_threads,
                                        uid,
                                        start_screen_thread(
                                            Lcd128x64Bricklet::new(uid.into(), ipcon.clone()),
                                            event_registry.clone(),
                                            *screen_settings,
                                        )
                                        .await,
                                    )
                                    .await;
                                }
                            }
                            DmxBricklet::DEVICE_IDENTIFIER => {
                                if let Some(settings) = tinkerforge_devices.dmx_bricklets.get(&uid)
                                {
                                    info!("Found DMX Bricklet: {}", paket.uid);
                                    register_handle(
                                        &mut running_threads,
                                        uid,
                                        handle_dmx(
                                            DmxBricklet::new(uid.into(), ipcon.clone()),
                                            event_registry.clone(),
                                            &settings.entries,
                                        )
                                        .await,
                                    )
                                    .await;
                                }
                            }
                            Io16V2Bricklet::DEVICE_IDENTIFIER => {
                                if let Some(settings) = tinkerforge_devices.io_bricklets.get(&uid) {
                                    info!("Found IO 16 Bricklet: {}", paket.uid);
                                    register_handle(
                                        &mut running_threads,
                                        uid,
                                        handle_io16_v2(
                                            Io16V2Bricklet::new(uid.into(), ipcon.clone()),
                                            event_registry.clone(),
                                            &settings.entries,
                                        )
                                        .await,
                                    )
                                    .await;
                                }
                            }
                            MotionDetectorV2Bricklet::DEVICE_IDENTIFIER => {
                                if let Some(settings) =
                                    tinkerforge_devices.motion_detectors.get(&uid)
                                {
                                    info!("Found Motion detector Bricklet: {}", paket.uid);
                                    register_handle(
                                        &mut running_threads,
                                        uid,
                                        handle_motion_detector(
                                            MotionDetectorV2Bricklet::new(
                                                uid.into(),
                                                ipcon.clone(),
                                            ),
                                            event_registry.clone(),
                                            settings.output,
                                        ),
                                    )
                                    .await;
                                }
                            }
                            TemperatureV2Bricklet::DEVICE_IDENTIFIER => {
                                if let Some(settings) =
                                    tinkerforge_devices.temperature_sensors.get(&uid)
                                {
                                    info!(
                                        "Found Temperature Bricklet: {}\n{:#?}",
                                        paket.uid, settings
                                    );
                                    register_handle(
                                        &mut running_threads,
                                        uid,
                                        handle_temperature(
                                            TemperatureV2Bricklet::new(uid.into(), ipcon.clone()),
                                            event_registry.clone(),
                                            settings.output,
                                        ),
                                    )
                                    .await;
                                }
                            }
                            IndustrialQuadRelayBricklet::DEVICE_IDENTIFIER => {
                                if let Some(settings) = tinkerforge_devices.relays.get(&uid) {
                                    info!("Found Relay Bricklet: {}\n{:#?}", paket.uid, settings);
                                    register_handle(
                                        &mut running_threads,
                                        uid,
                                        handle_quad_relay(
                                            IndustrialQuadRelayBricklet::new(
                                                uid.into(),
                                                ipcon.clone(),
                                            ),
                                            &event_registry,
                                            &settings.entries,
                                        )
                                        .await,
                                    )
                                    .await;
                                }
                            }

                            _ => {}
                        }
                    }
                    EnumerationType::Disconnected => {
                        info!("Disconnected device: {}", paket.uid);
                    }
                    EnumerationType::Unknown => {
                        info!("Unknown Event: {:?}", paket);
                    }
                }
            }
            Err(error) => {
                error!("Cannot parse UID {}: {error}", paket.uid)
            }
        }
    }
    Ok(())
}

async fn register_handle(
    running_threads: &mut HashMap<Uid, mpsc::Sender<()>>,
    uid: Uid,
    abort_handle: mpsc::Sender<()>,
) {
    if let Some(old_handle) = running_threads.insert(uid, abort_handle) {
        //info!("Stop old thread");
        if let Err(error) = old_handle.send(()).await {
            error!("Cannot stop thread: {error}")
        }
    }
}

fn start_enumeration_listener<T: ToSocketAddrs + Clone + Debug + Send + Sync + 'static>(
    connection: T,
    event_registry: EventRegistry,
    tinkerforge_devices: Arc<TinkerforgeDevices>,
) -> JoinHandle<()> {
    let connection = connection.clone();
    task::spawn(async move {
        let socket_str = format!("{connection:?}");
        loop {
            match run_enumeration_listener(
                connection.clone(),
                event_registry.clone(),
                tinkerforge_devices.clone(),
            )
            .await
            {
                Ok(_) => {
                    info!("{socket_str}: Closed");
                    break;
                }
                Err(e) => {
                    error!("{socket_str}: Error: {e}");
                    sleep(Duration::from_secs(10)).await;
                }
            };
        }
    })
}

#[actix_web::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::builder()
        .parse_env(Env::default().filter_or("LOG_LEVEL", "info"))
        .format_timestamp(Some(TimestampPrecision::Millis))
        .init();
    //env_logger::init_from_env(Env::default().filter_or("LOG_LEVEL", "info"));

    let bind_addr = CONFIG.server.bind_address();
    let mgmt_port = CONFIG.server.mgmt_port();
    let tinkerforge = &CONFIG.tinkerforge;
    let state_file = CONFIG.server.state_file();

    let prometheus = PrometheusMetricsBuilder::new("")
        .endpoint("/metrics")
        .build()
        .unwrap();
    let mgmt_server = HttpServer::new(move || App::new().wrap(prometheus.clone()).service(health))
        .bind((*bind_addr, mgmt_port))?
        .workers(2)
        .run();

    let initial_snapshot = read_snapshot(state_file).await.unwrap_or_else(|error| {
        error!("Cannot load snapshot: {error}");
        None
    });

    let event_registry = EventRegistry::new(initial_snapshot);

    start_snapshot_thread(&event_registry, state_file);

    let wiring = Wiring {
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

    let wiring = read_sheet_data().await.unwrap().unwrap();
    //info!("Config: \n{}", serde_yaml::to_string(&wiring).unwrap());

    let mut debug_stream = event_registry
        .dual_button_stream(DualButtonKey(Default::default()))
        .await;
    tokio::spawn(async move {
        while let Some(event) = debug_stream.next().await {
            info!("Event: {event:?}")
        }
    });
    let Wiring {
        controllers:
            Controllers {
                dual_input_dimmers,
                dual_input_switches,
                motion_detectors,
                heat_controllers,
                ring_controllers,
            },
        tinkerforge_devices,
    } = wiring;

    for dimmer_cfg in dual_input_dimmers.iter() {
        dual_input_dimmer(
            &event_registry,
            dimmer_cfg.input.as_ref(),
            dimmer_cfg.output,
            dimmer_cfg.auto_switch_off_time,
            dimmer_cfg.presence.as_ref(),
        )
        .await;
    }
    for switch_cfg in dual_input_switches.iter() {
        dual_input_switch(
            &event_registry,
            switch_cfg.input.as_ref(),
            switch_cfg.output,
            switch_cfg.auto_switch_off_time,
            switch_cfg.presence.as_ref(),
        )
        .await;
    }
    for motion_detector_cfg in motion_detectors.iter() {
        match motion_detector_cfg {
            MotionDetector::Switch {
                input,
                output,
                switch_off_time,
            } => motion_detector(&event_registry, input.as_ref(), *output, *switch_off_time).await,
            MotionDetector::Dimmer {
                input,
                output,
                brightness,
                switch_off_time,
            } => {
                motion_detector_dimmer(
                    &event_registry,
                    input.as_ref(),
                    *brightness,
                    *output,
                    *switch_off_time,
                )
                .await
            }
        };
    }
    let tinkerforge_devices = Arc::new(tinkerforge_devices);

    for endpoint in tinkerforge.endpoints() {
        start_enumeration_listener(
            (endpoint.address(), endpoint.port()),
            event_registry.clone(),
            tinkerforge_devices.clone(),
        );
    }

    let mut buffer = vec![];
    let encoder = TextEncoder::new();
    let metrics = gather();
    encoder.encode(&metrics, &mut buffer).unwrap();

    // Output to the standard output.
    println!("{}", String::from_utf8(buffer).unwrap());

    join!(mgmt_server).0?;
    Ok(())
}

fn start_snapshot_thread(event_registry: &EventRegistry, state_file: &'static str) {
    let event_registry = event_registry.clone();
    tokio::spawn(async move {
        let mut last_snapshot = Default::default();
        loop {
            sleep(Duration::from_secs(10)).await;
            let snapshot = event_registry.take_snapshot().await;
            if snapshot == last_snapshot {
                continue;
            }
            match write_snapshot(&snapshot, state_file).await {
                Ok(_) => {}
                Err(error) => {
                    error!("Cannot write snapshot: {error}");
                }
            }
            last_snapshot = snapshot;
        }
    });
}

mod snapshot;

fn dither<const N: usize>(input: &[f32; N]) -> Box<[bool; N]> {
    let mut current_error = 0.0;
    input
        .iter()
        .map(move |value| {
            let current_value = value + current_error;
            if current_value > 0.3 {
                current_error = current_value - 1.0;
                true
            } else {
                current_error = current_value;
                false
            }
        })
        .collect::<Box<[bool]>>()
        .try_into()
        .unwrap()
}
