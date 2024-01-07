use std::future::Future;
use std::{collections::HashMap, error::Error, fmt::Debug, fs::File, sync::Arc, time::Duration};

use actix_web::{get, App, HttpServer};
use actix_web_prometheus::PrometheusMetricsBuilder;
use env_logger::{Env, TimestampPrecision};
use log::{error, info};
use thiserror::Error;
use tinkerforge_async::io16_bricklet::Io16Bricklet;
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
use tokio::signal::unix::{signal, SignalKind};
use tokio::{net::ToSocketAddrs, pin, select, sync::mpsc, task, task::JoinHandle, time::sleep};
use tokio_stream::StreamExt;

use crate::data::settings::Tinkerforge;
use crate::devices::io_handler::handle_io16;
use crate::devices::screen_data_renderer::show_debug_text;
use crate::terminator::{AbortHandleTerminator, DeviceThreadTerminator, JoinHandleTerminator};
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

async fn run_enumeration_listener<T: ToSocketAddrs + Debug + Send + 'static + Clone>(
    addr: T,
    event_registry: EventRegistry,
    tinkerforge_devices: Arc<TinkerforgeDevices>,
    registered_devices: &mut HashMap<Uid, DeviceThreadTerminator>,
) -> Result<(), TfBridgeError> {
    let ipcon = AsyncIpConnection::new(addr.clone()).await?;
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
                            } else {
                                info!("Found unused LCD Device {} on {addr:?}", uid);
                                if let Err(error) = show_debug_text(
                                    Lcd128x64Bricklet::new(uid.into(), ipcon.clone()),
                                    &format!("UID: {uid}"),
                                )
                                .await
                                {
                                    error!("Cannot access device {uid}: {error}");
                                }
                            }
                        }
                        DmxBricklet::DEVICE_IDENTIFIER => {
                            if let Some(settings) = tinkerforge_devices.dmx_bricklets.get(&uid) {
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
                            } else {
                                info!("Found unused DMX Bricklet {uid} on {addr:?}");
                            }
                        }
                        Io16Bricklet::DEVICE_IDENTIFIER => {
                            if let Some(settings) = tinkerforge_devices.io_bricklets.get(&uid) {
                                register_handle(
                                    registered_devices,
                                    uid,
                                    handle_io16(
                                        Io16Bricklet::new(uid.into(), ipcon.clone()),
                                        event_registry.clone(),
                                        &settings.entries,
                                    )
                                    .await,
                                )
                                .await;
                            } else {
                                info!("Found unused IO16 Device {uid} on {addr:?}");
                            }
                        }
                        Io16V2Bricklet::DEVICE_IDENTIFIER => {
                            if let Some(settings) = tinkerforge_devices.io_bricklets.get(&uid) {
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
                            } else {
                                info!("Found unused IO16 v2 Device {uid} on {addr:?}");
                            }
                        }
                        MotionDetectorV2Bricklet::DEVICE_IDENTIFIER => {
                            if let Some(settings) = tinkerforge_devices.motion_detectors.get(&uid) {
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
                            } else {
                                info!("Found unused Motion detector {uid} on {addr:?}");
                            }
                        }
                        TemperatureV2Bricklet::DEVICE_IDENTIFIER => {
                            if let Some(settings) =
                                tinkerforge_devices.temperature_sensors.get(&uid)
                            {
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
                            } else {
                                info!("Found unused Temperature Sensor {uid} on {addr:?}");
                            }
                        }
                        IndustrialQuadRelayV2Bricklet::DEVICE_IDENTIFIER => {
                            if let Some(settings) = tinkerforge_devices.relays.get(&uid) {
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
                            } else {
                                info!("Found unused Relay Bricklet {uid} on {addr:?}");
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
    running_threads: &mut HashMap<Uid, DeviceThreadTerminator>,
    uid: Uid,
    abort_handle: mpsc::Sender<()>,
) {
    running_threads.insert(uid, DeviceThreadTerminator::new(abort_handle));
}

mod terminator;

fn start_enumeration_listener<T: ToSocketAddrs + Clone + Debug + Send + Sync + 'static>(
    connection: T,
    event_registry: EventRegistry,
    tinkerforge_devices: Arc<TinkerforgeDevices>,
) -> JoinHandle<()> {
    let connection = connection.clone();
    task::spawn(async move {
        let mut running_threads: HashMap<Uid, DeviceThreadTerminator> = HashMap::new();

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

    let snapshot_storage_thread = start_snapshot_thread(&event_registry, state_file);
    //let (tx, mut rx) = mpsc::channel(1);
    //let mgmt_tx = tx.clone();
    /* tokio::spawn(async move {
        match mgmt_server.await {
            Ok(_) => {
                info!("Management server terminated normally");
            }
            Err(error) => {
                error!("Management Server terminated with error: {error}");
            }
        }
        report_send_error(mgmt_tx.send(()).await);
    });*/

    //let cfg_tx = tx.clone();
    /*
    let config_updater = tokio::spawn(async move {
        match config_update_loop(tinkerforge, setup_file, event_registry).await {
            Ok(_) => {}
            Err(error) => {
                error!("Config load failed: {error}")
            }
        };
        report_send_error(cfg_tx.send(()).await);
    });*/
    let mut terminate_signal = signal(SignalKind::terminate())?;
    let config_update_future = config_update_loop(tinkerforge, setup_file, event_registry);
    select! {
        _ = snapshot_storage_thread =>{info!("Snapshot storage thread terminated");}
        status =
             mgmt_server =>{ match status {
                     Ok(_) => {
                info!("Management server terminated normally");
            }
            Err(error) => {
                error!("Management Server terminated with error: {error}");
            }}
            }
        status = config_update_future =>{
                match status{
                          Ok(_) => {}
            Err(error) => {
                error!("Config load failed: {error}")
            }
                }
            }
        _ = terminate_signal.recv() =>{
            info!("Terminated by signal");
        }
    }
    Ok(())
}

async fn config_update_loop(
    tinkerforge: &Tinkerforge,
    setup_file: &str,
    event_registry: EventRegistry,
) -> Result<(), Box<dyn Error>> {
    let mut current_wiring = Wiring::default();
    let mut running_controllers = Vec::new();
    let mut running_connections = Vec::new();

    loop {
        let wiring = fetch_config(setup_file).await?;
        if wiring == current_wiring {
            info!("Configuration unchanged");
            sleep(Duration::from_secs(5 * 60)).await;
            continue;
        }
        if wiring.controllers != current_wiring.controllers {
            info!("Terminating running controllers");
            running_controllers.clear();
            let Controllers {
                dual_input_dimmers,
                dual_input_switches,
                motion_detectors,
                heat_controllers,
                ring_controllers,
            } = wiring.controllers.clone();
            for dimmer_cfg in dual_input_dimmers.iter() {
                running_controllers.push(AbortHandleTerminator::new(
                    dual_input_dimmer(
                        &event_registry,
                        dimmer_cfg.input.as_ref(),
                        dimmer_cfg.output,
                        dimmer_cfg.auto_switch_off_time,
                        dimmer_cfg.presence.as_ref(),
                    )
                    .await,
                ));
            }
            for switch_cfg in dual_input_switches.iter() {
                running_controllers.push(AbortHandleTerminator::new(
                    dual_input_switch(
                        &event_registry,
                        switch_cfg.input.as_ref(),
                        switch_cfg.output,
                        switch_cfg.auto_switch_off_time,
                        switch_cfg.presence.as_ref(),
                    )
                    .await,
                ));
            }
            for motion_detector_cfg in motion_detectors.iter() {
                running_controllers.push(AbortHandleTerminator::new(match motion_detector_cfg {
                    MotionDetector::Switch {
                        input,
                        output,
                        switch_off_time,
                    } => {
                        motion_detector(&event_registry, input.as_ref(), *output, *switch_off_time)
                            .await
                    }
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
                }));
            }
            for cfg in heat_controllers.iter() {
                running_controllers.push(AbortHandleTerminator::new(
                    heat_controller(
                        &event_registry,
                        cfg.current_value_input,
                        cfg.target_value_input,
                        cfg.output,
                    )
                    .await,
                ));
            }
            for cfg in ring_controllers.iter() {
                running_controllers.push(AbortHandleTerminator::new(
                    ring_controller(&event_registry, cfg.input, cfg.output).await,
                ));
            }
            info!("Controllers updated");
        }
        if wiring.tinkerforge_devices != current_wiring.tinkerforge_devices {
            info!("Tinkerforge devices changed");
            running_connections.clear();
            let tinkerforge_devices = Arc::new(wiring.tinkerforge_devices.clone());

            for handle in (if tinkerforge.endpoints().is_empty() {
                tinkerforge_devices
                    .endpoints
                    .iter()
                    .map(|ip| {
                        start_enumeration_listener(
                            (*ip, 4223),
                            event_registry.clone(),
                            tinkerforge_devices.clone(),
                        )
                    })
                    .collect::<Vec<_>>()
            } else {
                tinkerforge
                    .endpoints()
                    .iter()
                    .map(|ep| {
                        start_enumeration_listener(
                            (ep.address(), ep.port()),
                            event_registry.clone(),
                            tinkerforge_devices.clone(),
                        )
                    })
                    .collect()
            })
            .into_iter()
            {
                running_connections.push(JoinHandleTerminator::new(handle));
            }
        }

        current_wiring = wiring;
        info!("Reloaded new configuration");

        sleep(Duration::from_secs(10)).await;
    }
}

async fn fetch_config(setup_file: &str) -> Result<Wiring, Box<dyn Error>> {
    Ok(
        if let Some(google_data) = match read_sheet_data().await {
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
        },
    )
}

fn start_snapshot_thread(
    event_registry: &EventRegistry,
    state_file: &'static str,
) -> impl Future<Output = ()> {
    let event_registry = event_registry.clone();
    async move {
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
    }
}

mod snapshot;
