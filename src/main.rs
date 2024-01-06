use std::{collections::HashMap, error::Error, fmt::Debug, fs::File, sync::Arc, time::Duration};

use actix_web::{get, App, HttpServer};
use actix_web_prometheus::PrometheusMetricsBuilder;
use env_logger::{Env, TimestampPrecision};
use log::{error, info, warn};
use prometheus::{gather, Encoder, TextEncoder};
use thiserror::Error;
use tinkerforge_async::{
    base58::Base58,
    dmx_bricklet::DmxBricklet,
    error::TinkerforgeError,
    industrial_quad_relay_v2_bricklet::IndustrialQuadRelayV2Bricklet,
    io16_v2_bricklet::Io16V2Bricklet,
    ip_connection::{async_io::AsyncIpConnection, EnumerationType},
    lcd_128x64_bricklet::Lcd128x64Bricklet,
    motion_detector_v2_bricklet::MotionDetectorV2Bricklet,
    temperature_v2_bricklet::TemperatureV2Bricklet,
};
use tokio::{join, net::ToSocketAddrs, pin, sync::mpsc, task, task::JoinHandle, time::sleep};
use tokio_stream::StreamExt;

use crate::{
    controller::{
        action::ring_controller,
        heat::heat_controller,
        light::{dual_input_dimmer, dual_input_switch, motion_detector, motion_detector_dimmer},
    },
    data::{
        google_data::read_sheet_data,
        registry::EventRegistry,
        settings::CONFIG,
        wiring::{Controllers, MotionDetector, TinkerforgeDevices, Wiring},
        Uid,
    },
    devices::{
        dmx_handler::handle_dmx, io_handler::handle_io16_v2,
        motion_detector::handle_motion_detector, relay::handle_quad_relay,
        screen_data_renderer::start_screen_thread, temperature::handle_temperature,
    },
    snapshot::{read_snapshot, write_snapshot},
};

mod controller;
mod data;
mod devices;
mod icons;
mod util;

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
    registered_devices: &mut HashMap<Uid, mpsc::Sender<()>>,
) -> Result<(), TfBridgeError> {
    let ipcon = AsyncIpConnection::new(addr).await?;
    // Enumerate
    let stream = ipcon.clone().enumerate().await?;
    pin!(stream);
    while let Some(paket) = stream.next().await {
        match paket.uid.base58_to_u32().map(Into::<Uid>::into) {
            Ok(uid) => match paket.enumeration_type {
                EnumerationType::Available | EnumerationType::Connected => {
                    match paket.device_identifier {
                        Lcd128x64Bricklet::DEVICE_IDENTIFIER => {
                            if let Some(screen_settings) = tinkerforge_devices.lcd_screens.get(&uid)
                            {
                                info!("Found LCD Device: {}", paket.uid);
                                register_handle(
                                    registered_devices,
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
                            if let Some(settings) = tinkerforge_devices.dmx_bricklets.get(&uid) {
                                info!("Found DMX Bricklet: {}", paket.uid);
                                register_handle(
                                    registered_devices,
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
                                    registered_devices,
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
                            if let Some(settings) = tinkerforge_devices.motion_detectors.get(&uid) {
                                info!("Found Motion detector Bricklet: {}", paket.uid);
                                register_handle(
                                    registered_devices,
                                    uid,
                                    handle_motion_detector(
                                        MotionDetectorV2Bricklet::new(uid.into(), ipcon.clone()),
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
                                info!("Found Temperature Bricklet: {}", paket.uid);
                                register_handle(
                                    registered_devices,
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
                        IndustrialQuadRelayV2Bricklet::DEVICE_IDENTIFIER => {
                            if let Some(settings) = tinkerforge_devices.relays.get(&uid) {
                                info!("Found Relay Bricklet: {}", paket.uid);
                                register_handle(
                                    registered_devices,
                                    uid,
                                    handle_quad_relay(
                                        IndustrialQuadRelayV2Bricklet::new(
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
            },
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
        if let Err(error) = old_handle.send(()).await {
            warn!("Cannot terminate old thread: {error}")
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
        let mut running_threads: HashMap<Uid, mpsc::Sender<()>> = HashMap::new();

        let socket_str = format!("{connection:?}");
        loop {
            match run_enumeration_listener(
                connection.clone(),
                event_registry.clone(),
                tinkerforge_devices.clone(),
                &mut running_threads,
            )
            .await
            {
                Ok(_) => {
                    info!("{socket_str}: Closed");
                }
                Err(e) => {
                    error!("{socket_str}: Error: {e}");
                }
            };
            sleep(Duration::from_secs(10)).await;
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
    let setup_file = CONFIG.server.setup_file();

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

    let wiring = if let Some(google_data) = match read_sheet_data().await {
        Ok(data) => {
            serde_yaml::to_writer(File::create(setup_file)?, &data)?;
            data
        }
        Err(error) => {
            error!("Cannot read config from google: {error}");
            None
        }
    } {
        google_data
    } else {
        serde_yaml::from_reader(File::open(setup_file)?)?
    };

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
    for cfg in heat_controllers.iter() {
        heat_controller(
            &event_registry,
            cfg.current_value_input,
            cfg.target_value_input,
            cfg.output,
        )
        .await;
    }
    for cfg in ring_controllers.iter() {
        ring_controller(&event_registry, cfg.input, cfg.output).await;
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
