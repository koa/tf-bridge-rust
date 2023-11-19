use std::{
    error::Error,
    fmt::Debug,
    time::{Duration, SystemTime},
};

use actix_web::{get, App, HttpServer};
use actix_web_prometheus::PrometheusMetricsBuilder;
use chrono::Local;
use embedded_graphics::mono_font::iso_8859_1::FONT_6X12;
use embedded_graphics::primitives::Rectangle;
use embedded_graphics::{
    geometry::Dimensions,
    image::Image,
    mono_font::MonoTextStyle,
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point, Primitive},
    text::Text,
    Drawable,
};
use env_logger::Env;
use log::{error, info};
use prometheus::proto::LabelPair;
use prometheus::{gather, Encoder, TextEncoder};
use tinkerforge::{
    error::TinkerforgeError,
    ip_connection::async_io::AsyncIpConnection,
    ip_connection::{EnumerateResponse, EnumerationType},
    lcd_128x64_bricklet::{Lcd128x64Bricklet, TouchPositionEvent},
    master_brick::MasterBrick,
};
use tokio::{join, net::ToSocketAddrs, pin, task::JoinHandle, time::sleep};
use tokio_stream::StreamExt;

use crate::display::{Lcd128x64BrickletDisplay, Orientation};
use crate::settings::CONFIG;
use crate::simple_layout::{expand, Layoutable, LinearPair, Vertical};

mod display;
mod simple_layout;

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
                            Orientation::RightDown,
                        )
                        .await?;
                        let display_area = display.bounding_box();
                        let text_style = MonoTextStyle::new(&FONT_6X12, BinaryColor::On);
                        let text = Text::new("Hello World!\n", Point { x: 0, y: 10 }, text_style);
                        text.draw(&mut display).expect("No error possible");
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
                                    if elapsed.as_millis() < 100 || pressure < 20 {
                                        println!("Skipped");
                                        continue;
                                    }
                                    println!("Elapsed: {elapsed:?}, age: {age}");
                                }
                                last_touch_time = now;
                                touch_count += 1;
                                let clock = Local::now().format("%H:%M").to_string();
                                display.clear();
                                let clock_text = Text::new(&clock, Point::zero(), text_style);

                                let rectangle = display.bounding_box();
                                //clock_text.draw_placed(&mut display, rectangle);
                                let pressure_string = format!("p: {pressure}\nXYq");
                                let vertical_layout: LinearPair<_, _, _, Vertical> = (
                                    expand(clock_text),
                                    Text::new(pressure_string.as_str(), Point::zero(), text_style),
                                )
                                    .into();
                                vertical_layout.draw_placed(&mut display, rectangle);
                                /*
                                LinearLayout::vertical(
                                    Chain::new(Text::new(&clock, Point::zero(), text_style))
                                        .append(Text::new(
                                            format!("p: {pressure}").as_str(),
                                            Point::zero(),
                                            text_style,
                                        )),
                                )
                                .with_alignment(horizontal::Center)
                                .with_spacing(DistributeFill(display_area.size.height))
                                .arrange()
                                .align_to(&display_area, horizontal::Center, vertical::Center)
                                .draw(&mut display)
                                .unwrap();
                                let d = pressure / 6;
                                Circle::new(
                                    Point::new((x - d / 2) as i32, (y - d / 2) as i32),
                                    d as u32,
                                )
                                .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
                                .draw(&mut display)
                                .expect("No error");
                                 */
                                display.draw().await.expect("will not happen");
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
