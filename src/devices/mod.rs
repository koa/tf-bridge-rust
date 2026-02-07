use std::{collections::HashMap, fmt::Debug, net::IpAddr, sync::Arc, time::Duration};

use log::{error, info};
use thiserror::Error;
use tinkerforge_async::{
    base58::Uid,
    dmx::DmxBricklet,
    error::TinkerforgeError,
    industrial_quad_relay_v_2::IndustrialQuadRelayV2Bricklet,
    io_16::Io16Bricklet,
    io_16_v_2::Io16V2Bricklet,
    ip_connection::{async_io::AsyncIpConnection, EnumerateResponse, EnumerationType},
    lcd_128_x_64::Lcd128X64Bricklet,
    motion_detector_v_2::MotionDetectorV2Bricklet,
    temperature_v_2::TemperatureV2Bricklet,
    DeviceIdentifier,
};
use tokio::{
    pin,
    sync::mpsc,
    task,
    time::{interval, sleep},
};
use tokio_stream::{wrappers::IntervalStream, StreamExt};

use crate::{
    data::{
        registry::EventRegistry, settings::Tinkerforge, state::BrickletMetadata,
        state::StateUpdateMessage, wiring::TinkerforgeDevices,
    },
    devices::{
        dmx_handler::handle_dmx,
        io_handler::{handle_io16, handle_io16_v2},
        motion_detector::handle_motion_detector,
        relay::handle_quad_relay,
        screen_data_renderer::{show_debug_text, start_screen_thread},
        temperature::handle_temperature,
    },
    terminator::{LifeLineEnd, TestamentReceiver, TestamentSender},
};

pub mod display;
pub mod dmx_handler;
pub mod io_handler;
pub mod motion_detector;
pub mod relay;
pub mod screen_data_renderer;
pub mod temperature;

fn do_activate_devices(
    endpoints: impl Iterator<Item = (IpAddr, u16)>,
    event_registry: &EventRegistry,
    running_connections: &mut Vec<TestamentSender>,
    devices: Arc<TinkerforgeDevices>,
    status_updater: mpsc::Sender<StateUpdateMessage>,
) {
    info!("Tinkerforge devices changed");
    running_connections.clear();
    for ep in endpoints {
        running_connections.push(start_enumeration_listener(
            ep,
            event_registry.clone(),
            devices.clone(),
            status_updater.clone(),
        ));
    }
}

pub fn activate_devices(
    tinkerforge: &Tinkerforge,
    event_registry: &EventRegistry,
    running_connections: &mut Vec<TestamentSender>,
    devices: TinkerforgeDevices,
    status_updater: mpsc::Sender<StateUpdateMessage>,
) {
    let tinkerforge_devices = Arc::new(devices);
    if tinkerforge.endpoints().is_empty() {
        do_activate_devices(
            tinkerforge_devices
                .endpoints
                .clone()
                .iter()
                .map(|ip| (*ip, 4223)),
            event_registry,
            running_connections,
            tinkerforge_devices,
            status_updater,
        );
    } else {
        do_activate_devices(
            tinkerforge
                .endpoints()
                .iter()
                .map(|ep| (ep.address(), ep.port())),
            event_registry,
            running_connections,
            tinkerforge_devices,
            status_updater,
        );
    };
}

#[derive(Clone)]
enum EnumerationListenerEvent {
    Packet(EnumerateResponse),
    Terminate,
    TerminatedClient(Uid),
    Ping,
}

#[derive(Error, Debug)]
enum TfBridgeError {
    #[error("Error communicating to device: {0}")]
    TinkerforgeError(#[from] TinkerforgeError),
    #[error("Connection to endpoint lost")]
    ConnectionLost,
    #[error("Cannot update status message {0}")]
    StatusUpdateMessage(#[from] mpsc::error::SendError<StateUpdateMessage>),
}

async fn run_enumeration_listener(
    addr: (IpAddr, u16),
    event_registry: EventRegistry,
    tinkerforge_devices: Arc<TinkerforgeDevices>,
    termination: TestamentReceiver,
    status_updater: mpsc::Sender<StateUpdateMessage>,
) -> Result<(), TfBridgeError> {
    let mut registered_devices: HashMap<Uid, LifeLineEnd> = HashMap::new();

    let mut ipcon = AsyncIpConnection::new(addr).await?;
    //let endpoint_addr = addr.0.clone();
    // Enumerate
    let enumeration_stream = ipcon.clone().enumerate().await?;
    pin!(enumeration_stream);

    //let (terminated_tx, terminated_rx) = mpsc::channel(10);

    let mut stream = enumeration_stream
        .as_mut()
        .map(EnumerationListenerEvent::Packet)
        .merge(termination.send_on_terminate(EnumerationListenerEvent::Terminate))
        .merge(
            IntervalStream::new(interval(Duration::from_secs(10)))
                .map(|_| EnumerationListenerEvent::Ping),
        )
        //.merge(ReceiverStream::new(terminated_rx).map(EnumerationListenerEvent::TerminatedClient))
        ;
    status_updater
        .send(StateUpdateMessage::EndpointConnected(addr.0))
        .await?;
    let mut device_testaments = HashMap::new();
    while let Some(event) = stream.next().await {
        match event {
            EnumerationListenerEvent::Ping => {
                //info!("Ping: {}", addr.0);
                ipcon.disconnect_probe().await?;
            }
            EnumerationListenerEvent::Packet(paket) => {
                let uid = paket.uid;
                match paket.enumeration_type {
                    EnumerationType::Available | EnumerationType::Connected => {
                        if let Some(live_end) = registered_devices.get(&uid) {
                            if live_end.is_alive() {
                                info!("Repeat: {uid}, {:?}", paket.enumeration_type);
                                continue;
                            } else {
                                info!("Refresh dead {uid}");
                            }
                        }

                        info!("Registered: {uid}");
                        if let Ok(device_identifier) = paket.device_identifier.try_into() {
                            status_updater
                                .send(StateUpdateMessage::BrickletConnected {
                                    uid,
                                    endpoint: addr.0,
                                    metadata: BrickletMetadata {
                                        connected_uid: paket.connected_uid,
                                        position: paket.position,
                                        hardware_version: paket.hardware_version,
                                        firmware_version: paket.firmware_version,
                                        device_identifier,
                                    },
                                })
                                .await
                                .expect("Cannot send connection message");
                            match device_identifier {
                                DeviceIdentifier::Lcd128X64Bricklet => {
                                    if let Some(screen_settings) =
                                        tinkerforge_devices.lcd_screens.get(&uid)
                                    {
                                        let sender = start_screen_thread(
                                            Lcd128X64Bricklet::new(uid, ipcon.clone()),
                                            event_registry.clone(),
                                            *screen_settings,
                                        )
                                        .await;
                                        register_handle(&mut registered_devices, uid, sender).await;
                                    } else {
                                        info!("Found unused LCD Device {} on {addr:?}", uid);
                                        if let Err(error) = show_debug_text(
                                            Lcd128X64Bricklet::new(uid, ipcon.clone()),
                                            &format!("UID: {uid}"),
                                        )
                                        .await
                                        {
                                            error!("Cannot access device {uid}: {error}");
                                        }
                                    }
                                }
                                DeviceIdentifier::DmxBricklet => {
                                    if let Some(settings) =
                                        tinkerforge_devices.dmx_bricklets.get(&uid)
                                    {
                                        register_handle(
                                            &mut registered_devices,
                                            uid,
                                            handle_dmx(
                                                DmxBricklet::new(uid, ipcon.clone()),
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
                                DeviceIdentifier::Io16Bricklet => {
                                    if let Some(settings) =
                                        tinkerforge_devices.io_bricklets.get(&uid)
                                    {
                                        register_handle(
                                            &mut registered_devices,
                                            uid,
                                            handle_io16(
                                                Io16Bricklet::new(uid, ipcon.clone()),
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
                                DeviceIdentifier::Io16V2Bricklet => {
                                    if let Some(settings) =
                                        tinkerforge_devices.io_bricklets.get(&uid)
                                    {
                                        register_handle(
                                            &mut registered_devices,
                                            uid,
                                            handle_io16_v2(
                                                Io16V2Bricklet::new(uid, ipcon.clone()),
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
                                DeviceIdentifier::MotionDetectorV2Bricklet => {
                                    if let Some(settings) =
                                        tinkerforge_devices.motion_detectors.get(&uid)
                                    {
                                        register_handle(
                                            &mut registered_devices,
                                            uid,
                                            handle_motion_detector(
                                                MotionDetectorV2Bricklet::new(uid, ipcon.clone()),
                                                event_registry.clone(),
                                                settings.output,
                                            ),
                                        )
                                        .await;
                                    } else {
                                        info!("Found unused Motion detector {uid} on {addr:?}");
                                    }
                                }
                                DeviceIdentifier::TemperatureV2Bricklet => {
                                    if let Some(settings) =
                                        tinkerforge_devices.temperature_sensors.get(&uid)
                                    {
                                        register_handle(
                                            &mut registered_devices,
                                            uid,
                                            handle_temperature(
                                                TemperatureV2Bricklet::new(uid, ipcon.clone()),
                                                event_registry.clone(),
                                                settings.output,
                                            ),
                                        )
                                        .await;
                                    } else {
                                        info!("Found unused Temperature Sensor {uid} on {addr:?}");
                                    }
                                }
                                DeviceIdentifier::IndustrialQuadRelayV2Bricklet => {
                                    if let Some(settings) = tinkerforge_devices.relays.get(&uid) {
                                        register_handle(
                                            &mut registered_devices,
                                            uid,
                                            handle_quad_relay(
                                                IndustrialQuadRelayV2Bricklet::new(
                                                    uid,
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
                            if let Some(running_registration) = registered_devices.get(&uid) {
                                running_registration.update_on_terminate(
                                    StateUpdateMessage::BrickletDisconnected {
                                        uid,
                                        endpoint: addr.0,
                                    },
                                    status_updater.clone(),
                                );
                            } else {
                                let (testament, testament_stream) = TestamentSender::create();
                                testament_stream.update_on_terminate(
                                    StateUpdateMessage::BrickletDisconnected {
                                        uid,
                                        endpoint: addr.0,
                                    },
                                    status_updater.clone(),
                                );
                                device_testaments.insert(uid, testament);
                            }
                        }
                    }
                    EnumerationType::Disconnected => {
                        info!("Disconnected device: {}", uid);
                        device_testaments.remove(&uid);
                        registered_devices.remove(&uid);
                    }
                    EnumerationType::Unknown => {
                        info!("Unknown Event: {:?}", paket);
                    }
                }
            }
            EnumerationListenerEvent::Terminate => return Ok(()),
            EnumerationListenerEvent::TerminatedClient(uid) => {
                info!("Terminated {uid} on {}", addr.0);
                device_testaments.remove(&uid);
            }
        };
    }
    Err(TfBridgeError::ConnectionLost)
}

async fn register_callbacks(
    callbacks: LifeLineEnd,
    uid: Uid,
    registered_devices: &mut HashMap<Uid, LifeLineEnd>,
    // terminated_tx: Sender<Uid>,
) {
    register_handle(registered_devices, uid, callbacks).await;
}

fn start_enumeration_listener(
    connection: (IpAddr, u16),
    event_registry: EventRegistry,
    tinkerforge_devices: Arc<TinkerforgeDevices>,
    status_updater: mpsc::Sender<StateUpdateMessage>,
) -> TestamentSender {
    let (testament, testament_stream) = TestamentSender::create();
    task::spawn(async move {
        let socket_str = format!("{connection:?}");
        loop {
            info!("Connect to {socket_str}");
            match run_enumeration_listener(
                connection,
                event_registry.clone(),
                tinkerforge_devices.clone(),
                testament_stream.clone(),
                status_updater.clone(),
            )
            .await
            {
                Ok(_) => {
                    info!("{socket_str}: Finished");
                    break;
                }
                Err(e) => {
                    error!("{socket_str}: Error: {e}");
                    if let Err(error) = status_updater
                        .send(StateUpdateMessage::EndpointDisconnected(connection.0))
                        .await
                    {
                        error!(
                            "Cannot send status update on connection to {}: {error}",
                            connection.0
                        );
                    }
                }
            };
            sleep(Duration::from_secs(10)).await;
        }
        status_updater
            .send(StateUpdateMessage::EndpointDisconnected(connection.0))
            .await
            .expect("Cannot send status update");
    });
    testament
}

async fn register_handle(
    running_threads: &mut HashMap<Uid, LifeLineEnd>,
    uid: Uid,
    abort_handle: LifeLineEnd,
) {
    running_threads.insert(uid, abort_handle);
}
