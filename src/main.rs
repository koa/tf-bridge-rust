use std::{error::Error, fmt::Debug, fs::File, future::Future, time::Duration};

use actix_web::{App, get, HttpServer};
use actix_web_prometheus::PrometheusMetricsBuilder;
use env_logger::{Env, TimestampPrecision};
use log::{error, info};
use tokio::{
    select,
    signal::unix::{signal, SignalKind},
    sync::mpsc::{self, Sender},
    time::sleep,
};
use tokio_stream::{once, StreamExt, wrappers::ReceiverStream};

use crate::{
    controller::{
        action::ring_controller,
        heat::heat_controller,
        light::{dual_input_dimmer, dual_input_switch, motion_detector, motion_detector_dimmer},
    },
    data::{
        google_data::read_sheet_data,
        registry::EventRegistry,
        settings::{CONFIG, Shelly, Tinkerforge},
        state::{State, StateUpdateMessage},
        wiring::{Controllers, MotionDetector, Wiring},
    },
    devices::{shelly, tinkerforge},
    snapshot::{read_snapshot, write_snapshot},
    terminator::{AbortHandleTerminator, JoinHandleTerminator},
};

mod controller;
mod data;
mod devices;
mod icons;
mod metrics;
mod serde;
mod snapshot;
mod terminator;
mod util;

#[get("/health")]
async fn health() -> &'static str {
    "Ok"
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
    let shelly = &CONFIG.shelly;
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
    let mut terminate_signal = signal(SignalKind::terminate())?;
    let config_update_future = config_update_loop(tinkerforge, shelly, setup_file, event_registry);
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

#[derive(Debug)]
enum MainLoopEvent {
    FetchConfig,
    StatusUpdateMessage(StateUpdateMessage),
}

async fn config_update_loop(
    tinkerforge: &Tinkerforge,
    shelly: &Shelly,
    setup_file: &str,
    event_registry: EventRegistry,
) -> Result<(), Box<dyn Error>> {
    let mut current_wiring = Wiring::default();
    let mut running_controllers = Vec::new();
    let mut running_tinkerforge_connections = Vec::new();
    let mut running_shelly_connections = Vec::new();
    let (tx, rx) = mpsc::channel(100);
    let (main_tx, main_rx) = mpsc::channel(3);
    let mut stream = once(MainLoopEvent::FetchConfig)
        .merge(ReceiverStream::new(rx).map(MainLoopEvent::StatusUpdateMessage))
        .merge(ReceiverStream::new(main_rx));
    let mut known_state = State::default();
    let mut state_received = false;
    let mut config_timer = None;

    while let Some(message) = stream.next().await {
        match message {
            MainLoopEvent::StatusUpdateMessage(update) => {
                state_received = true;
                if known_state.process_msg(update) {
                    match update {
                        StateUpdateMessage::EndpointConnected(ip) => {
                            info!("Endpoint {ip} connected");
                        }
                        StateUpdateMessage::EndpointDisconnected(ip) => {
                            info!("Endpoint {ip} disconnected");
                        }
                        StateUpdateMessage::BrickletConnected { uid, .. } => {
                            info!("Bricklet {uid} connected");
                        }
                        StateUpdateMessage::BrickletDisconnected { uid, .. } => {
                            info!("Bricklet {uid} disconnected");
                        }
                        StateUpdateMessage::SpitfpMetrics { uid, .. } => {
                            info!("Bricklet {uid} updated metrics");
                        }
                        StateUpdateMessage::CommunicationFailed { uid, .. } => {
                            info!("Bricklet {uid} failed communication");
                        }
                    }
                    fech_next_in(main_tx.clone(), &mut config_timer, Duration::from_secs(2));
                }
            }
            MainLoopEvent::FetchConfig => {
                let wiring = fetch_config(
                    setup_file,
                    if state_received {
                        Some(&known_state)
                    } else {
                        None
                    },
                )
                .await?;
                if wiring == current_wiring {
                    info!("Configuration unchanged");
                    fech_next_in(
                        main_tx.clone(),
                        &mut config_timer,
                        Duration::from_secs(5 * 60),
                    );
                    continue;
                }
                if wiring.controllers != current_wiring.controllers {
                    activate_controllers(
                        &event_registry,
                        &mut running_controllers,
                        wiring.controllers.clone(),
                    )
                    .await;
                }
                if wiring.tinkerforge_devices != current_wiring.tinkerforge_devices {
                    tinkerforge::activate_devices(
                        tinkerforge,
                        &event_registry,
                        &mut running_tinkerforge_connections,
                        wiring.tinkerforge_devices.clone(),
                        tx.clone(),
                    );
                }
                if wiring.shelly_devices != current_wiring.shelly_devices {
                    shelly::activate_devices(
                        shelly,
                        &event_registry,
                        &mut running_shelly_connections,
                        wiring.shelly_devices.clone(),
                        tx.clone(),
                    );
                }

                current_wiring = wiring;
                info!("Reloaded new configuration");
                fech_next_in(main_tx.clone(), &mut config_timer, Duration::from_secs(10));
            }
        }
    }
    Ok(())
}

fn fech_next_in(
    main_tx: Sender<MainLoopEvent>,
    config_timer: &mut Option<JoinHandleTerminator<()>>,
    duration: Duration,
) {
    config_timer.replace(JoinHandleTerminator::new(tokio::spawn(async move {
        sleep(duration).await;
        if let Err(error) = main_tx.send(MainLoopEvent::FetchConfig).await {
            error!("Cannot send message: {error}");
        }
    })));
}

async fn activate_controllers(
    event_registry: &EventRegistry,
    running_controllers: &mut Vec<AbortHandleTerminator>,
    controllers: Controllers,
) {
    info!("Terminating running controllers");
    running_controllers.clear();
    let Controllers {
        dual_input_dimmers,
        dual_input_switches,
        motion_detectors,
        heat_controllers,
        ring_controllers,
    } = controllers;
    for dimmer_cfg in dual_input_dimmers.iter() {
        running_controllers.push(AbortHandleTerminator::new(
            dual_input_dimmer(
                event_registry,
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
                event_registry,
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
            } => motion_detector(event_registry, input.as_ref(), *output, *switch_off_time).await,
            MotionDetector::Dimmer {
                input,
                output,
                brightness,
                switch_off_time,
            } => {
                motion_detector_dimmer(
                    event_registry,
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
                event_registry,
                cfg.current_value_input,
                cfg.target_value_input,
                cfg.output,
            )
            .await,
        ));
    }
    for cfg in ring_controllers.iter() {
        running_controllers.push(AbortHandleTerminator::new(
            ring_controller(event_registry, cfg.input, cfg.output).await,
        ));
    }
    info!("Controllers updated");
}

async fn fetch_config(
    setup_file: &str,
    current_state: Option<&State>,
) -> Result<Wiring, Box<dyn Error>> {
    Ok(
        if let Some(google_data) = match read_sheet_data(current_state).await {
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
