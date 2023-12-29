use std::{collections::HashMap, error::Error, fmt::Debug, time::Duration};

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
    io16_v2_bricklet::Io16V2Bricklet,
    ip_connection::{async_io::AsyncIpConnection, EnumerateResponse, EnumerationType},
    lcd_128x64_bricklet::Lcd128x64Bricklet,
    motion_detector_v2_bricklet::MotionDetectorV2Bricklet,
};
use tokio::{join, net::ToSocketAddrs, pin, sync::mpsc, task, task::JoinHandle, time::sleep};
use tokio_stream::StreamExt;

use crate::io_handler::DualButtonSettings;
use crate::registry::{BrightnessKey, ClockKey, DualButtonKey, LightColorKey, TemperatureKey};
use crate::screen_data_renderer::ScreenSettings;
use crate::{
    display::Orientation, io_handler::handle_io16_v2, registry::EventRegistry,
    screen_data_renderer::start_screen_thread, settings::CONFIG,
};

mod register;

mod display;

mod icons;
mod io_handler;
mod registry;
mod screen_data_renderer;
mod settings;
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
) -> Result<(), TfBridgeError> {
    let ipcon = AsyncIpConnection::new(addr).await?;
    // Enumerate
    let stream = ipcon.clone().enumerate().await?;
    let mut running_threads: HashMap<u32, mpsc::Sender<()>> = HashMap::new();
    pin!(stream);
    while let Some(paket) = stream.next().await {
        //print_enumerate_response(&paket);
        match paket.uid.base58_to_u32() {
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
                                info!("Found LCD Device: {}", paket.uid);
                                register_handle(
                                    &mut running_threads,
                                    uid,
                                    start_screen_thread(
                                        Lcd128x64Bricklet::new(uid, ipcon.clone()),
                                        event_registry.clone(),
                                        ScreenSettings {
                                            orientation: Orientation::LeftDown,
                                            clock_key: Some(ClockKey::MinuteClock),
                                            current_temperature_key: Some(
                                                TemperatureKey::CurrentTemperature,
                                            ),
                                            adjust_temperature_key: Some(
                                                TemperatureKey::TargetTemperature,
                                            ),
                                            light_color_key: Some(LightColorKey::IlluminationColor),
                                            brightness_key: Some(
                                                BrightnessKey::IlluminationBrightness,
                                            ),
                                        },
                                    )
                                    .await,
                                )
                                .await;
                            }
                            DmxBricklet::DEVICE_IDENTIFIER => {
                                info!("Found DMX Bricklet: {}", paket.uid);
                            }
                            Io16V2Bricklet::DEVICE_IDENTIFIER => {
                                info!("Found IO 16 Bricklet: {}", paket.uid);
                                register_handle(
                                    &mut running_threads,
                                    uid,
                                    handle_io16_v2(
                                        Io16V2Bricklet::new(uid, ipcon.clone()),
                                        event_registry.clone(),
                                        &[DualButtonSettings {
                                            up_button: 7,
                                            down_button: 6,
                                            output: DualButtonKey::DualButton,
                                        }],
                                    )
                                    .await,
                                )
                                .await;
                            }
                            MotionDetectorV2Bricklet::DEVICE_IDENTIFIER => {
                                info!("Found Motion detector Bricklet: {}", paket.uid);
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
    running_threads: &mut HashMap<u32, mpsc::Sender<()>>,
    uid: u32,
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
) -> JoinHandle<()> {
    let connection = connection.clone();
    task::spawn(async move {
        let socket_str = format!("{connection:?}");
        loop {
            match run_enumeration_listener(connection.clone(), event_registry.clone()).await {
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

    let prometheus = PrometheusMetricsBuilder::new("")
        .endpoint("/metrics")
        .build()
        .unwrap();
    let mgmt_server = HttpServer::new(move || App::new().wrap(prometheus.clone()).service(health))
        .bind((*bind_addr, mgmt_port))?
        .workers(2)
        .run();

    let event_registry = EventRegistry::new();
    let mut debug_stream = event_registry
        .dual_button_stream(DualButtonKey::DualButton)
        .await;
    tokio::spawn(async move {
        while let Some(event) = debug_stream.next().await {
            info!("Event: {event:?}")
        }
    });
    for endpoint in tinkerforge.endpoints() {
        start_enumeration_listener(
            (endpoint.address(), endpoint.port()),
            event_registry.clone(),
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
