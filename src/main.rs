use std::{error::Error, fmt::Debug, time::Duration};

use actix_web::{get, App, HttpServer};
use actix_web_prometheus::PrometheusMetricsBuilder;
use env_logger::Env;
use log::{error, info};
use prometheus::{gather, Encoder, TextEncoder};
use thiserror::Error;
use tinkerforge_async::{
    error::TinkerforgeError,
    ip_connection::async_io::AsyncIpConnection,
    ip_connection::{EnumerateResponse, EnumerationType},
    lcd_128x64_bricklet::Lcd128x64Bricklet,
    master_brick::MasterBrick,
};
use tokio::{join, net::ToSocketAddrs, pin, task, task::JoinHandle, time::sleep};
use tokio_stream::StreamExt;

use crate::{
    display::{Lcd128x64BrickletDisplay, Orientation},
    registry::EventRegistry,
    screen_data_renderer::start_screen_thread,
    settings::CONFIG,
};

mod register;

mod display;

mod icons;
mod registry;
mod screen_data_renderer;
mod settings;

const HOST: &str = "localhost";
const PORT: u16 = 4223;

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
    pin!(stream);
    while let Some(paket) = stream.next().await {
        //print_enumerate_response(&paket);
        match paket.enumeration_type {
            EnumerationType::Available | EnumerationType::Connected => {
                match paket.device_identifier {
                    MasterBrick::DEVICE_IDENTIFIER => {
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
                    }
                    Lcd128x64Bricklet::DEVICE_IDENTIFIER => {
                        let display = Lcd128x64BrickletDisplay::new(
                            &paket.uid,
                            ipcon.clone(),
                            Orientation::LeftDown,
                        )
                        .await?;
                        start_screen_thread(display, event_registry.clone());
                    }
                    _ => {}
                }
            }
            EnumerationType::Disconnected => {}
            EnumerationType::Unknown => {}
        };
    }
    Ok(())
}

fn start_enumeration_listener<T: ToSocketAddrs + Clone + Debug + Send + Sync + 'static>(
    connection: T,
    event_registry: EventRegistry,
) -> JoinHandle<()> {
    let connection = connection.clone();
    task::spawn(async move {
        let socket_str = format!("{connection:?}");
        loop {
            let clock_stream = event_registry.clone();
            match run_enumeration_listener(connection.clone(), clock_stream).await {
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
    env_logger::init_from_env(Env::default().filter_or("LOG_LEVEL", "info"));

    let bind_addr = CONFIG.bind_address();
    let mgmt_port = CONFIG.mgmt_port();

    let prometheus = PrometheusMetricsBuilder::new("")
        .endpoint("/metrics")
        .build()
        .unwrap();
    let mgmt_server = HttpServer::new(move || App::new().wrap(prometheus.clone()).service(health))
        .bind((*bind_addr, mgmt_port))?
        .workers(2)
        .run();

    let event_registry = EventRegistry::new();

    let handle = start_enumeration_listener((HOST, PORT), event_registry);
    let handle1 = handle.abort_handle();
    handle1.abort();
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
