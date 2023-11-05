use std::error::Error;
use std::fmt::Debug;
use std::future::Future;
use std::time::{Duration, SystemTime};

use actix_web::{get, App, HttpServer};
use actix_web_prometheus::PrometheusMetricsBuilder;
use bitmap_font::tamzen::FONT_6x12;
use bitmap_font::TextStyle;
use embedded_graphics::geometry::Dimensions;
use embedded_graphics::image::Image;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::Primitive;
use embedded_graphics::prelude::{DrawTarget, Point};
use embedded_graphics::primitives::{Circle, PrimitiveStyle};
use embedded_graphics::text::Text;
use embedded_graphics::Drawable;
use env_logger::Env;
use log::{error, info};
use prometheus::{gather, Encoder, TextEncoder};
use tinkerforge::error::TinkerforgeError;
use tinkerforge::ip_connection::async_io::AsyncIpConnection;
use tinkerforge::ip_connection::{EnumerateResponse, EnumerationType};
use tinkerforge::lcd_128x64_bricklet::{Lcd128x64Bricklet, TouchPositionEvent};
use tinkerforge::master_brick::MasterBrick;
use tokio::net::ToSocketAddrs;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tokio::{join, pin};
use tokio_stream::StreamExt;

use crate::display::{Lcd128x64BrickletDisplay, Orientation};
use crate::settings::CONFIG;

mod display;

mod icons;
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

async fn run_enumeration_listener<T: ToSocketAddrs>(addr: T) -> Result<(), TinkerforgeError> {
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
                        let mut display = Lcd128x64BrickletDisplay::new(
                            &paket.uid,
                            ipcon.clone(),
                            Orientation::UpsideDown,
                        )
                        .await?;
                        let text = Text::new(
                            "Hello World!\n",
                            Point::zero(),
                            TextStyle::new(&FONT_6x12, BinaryColor::On),
                        );
                        let p = text.draw(&mut display).expect("No error possible");
                        Image::new(&icons::COLOR, Point { x: 30, y: 20 })
                            .draw(&mut display)
                            .unwrap();
                        Image::new(&icons::BRIGHTNESS, Point { x: 45, y: 20 })
                            .draw(&mut display)
                            .unwrap();
                        display.draw().await?;

                        tokio::spawn(async move {
                            let mut stream = display.input_stream().await.expect("Cannot config");
                            let mut touch_count: u32 = 0;
                            let mut last_touch_time = SystemTime::now();
                            while let Some(TouchPositionEvent {
                                pressure,
                                x,
                                y,
                                age,
                            }) = stream.next().await
                            {
                                let now = SystemTime::now();
                                let elapsed = now.duration_since(last_touch_time);
                                if let Ok(elapsed) = elapsed {
                                    if elapsed.as_millis() < 200 || pressure < 20 {
                                        println!("Skipped");
                                        continue;
                                    }
                                    println!("Elapsed: {elapsed:?}, age: {age}");
                                }
                                last_touch_time = now;
                                touch_count += 1;
                                let start_time = SystemTime::now();
                                display.clear();
                                let cursor_pos = text.draw(&mut display).unwrap();
                                Text::new(
                                    format!("p: {pressure}, x: {x}, y: {y}\n{touch_count}")
                                        .as_str(),
                                    cursor_pos,
                                    TextStyle::new(&FONT_6x12, BinaryColor::On),
                                )
                                .draw(&mut display)
                                .expect("will not happen");
                                let d = pressure / 6;
                                Circle::new(
                                    Point::new((x - d / 2) as i32, (y - d / 2) as i32),
                                    d as u32,
                                )
                                .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
                                .draw(&mut display)
                                .expect("No error");
                                display.draw().await.expect("will not happen");
                                //println!("Time: {:?}", start_time.elapsed());
                            }
                            println!("Thread done");
                        });
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
) -> JoinHandle<()> {
    let connection = connection.clone();
    tokio::spawn(async move {
        let socket_str = format!("{connection:?}");
        loop {
            match run_enumeration_listener(connection.clone()).await {
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

    start_enumeration_listener((HOST, PORT));
    let mut buffer = vec![];
    let encoder = TextEncoder::new();
    let metrics = gather();
    encoder.encode(&metrics, &mut buffer).unwrap();

    // Output to the standard output.
    println!("{}", String::from_utf8(buffer).unwrap());

    join!(mgmt_server);
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
