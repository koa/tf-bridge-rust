use std::future::Future;
use std::{
    marker::PhantomData,
    num::Saturating,
    ops::{Add, Sub},
    time::SystemTime,
};

use chrono::{DateTime, Local, Timelike};
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    image::{Image, ImageRaw},
    mono_font::{iso_8859_1::FONT_6X12, MonoTextStyle},
    pixelcolor::BinaryColor,
    prelude::PixelColor,
    primitives::Rectangle,
    text::Text,
};
use log::{error, info};
use simple_layout::prelude::{
    bordered, center, expand, horizontal_layout, optional_placement, owned_text, padding, scale,
    vertical_layout, DashedLine, Layoutable, RoundedLine,
};
use thiserror::Error;
use tinkerforge_async::{error::TinkerforgeError, lcd_128x64_bricklet::TouchPositionEvent};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::{
    join,
    sync::mpsc::{self, error::SendError},
    task::JoinHandle,
    time::sleep,
};
use tokio_stream::{empty, wrappers::ReceiverStream, StreamExt, StreamNotifyClose};
use tokio_util::either::Either;

use crate::registry::{BrightnessKey, ClockKey, LightColorKey, TemperatureKey};
use crate::{display::Lcd128x64BrickletDisplay, icons, registry::EventRegistry, util};

const TEXT_STYLE: MonoTextStyle<BinaryColor> = MonoTextStyle::new(&FONT_6X12, BinaryColor::On);

pub struct ScreenData<
    LT: Layoutable<BinaryColor>,
    LWB: Layoutable<BinaryColor>,
    LBR: Layoutable<BinaryColor>,
> {
    current_time: DateTime<Local>,
    measured_temperature: Option<f32>,
    configured_temperature: Option<AdjustableValue<f32, LT, BinaryColor>>,
    whitebalance: Option<AdjustableValue<Saturating<u16>, LWB, BinaryColor>>,
    brightness: Option<AdjustableValue<Saturating<u8>, LBR, BinaryColor>>,
}

impl<LT: Layoutable<BinaryColor>, LWB: Layoutable<BinaryColor>, LBR: Layoutable<BinaryColor>>
    ScreenData<LT, LWB, LBR>
{
    pub fn set_current_time(&mut self, time: DateTime<Local>) {
        self.current_time = time;
    }
    pub fn set_current_tempterature(&mut self, value: f32) {
        self.measured_temperature = Some(value);
    }
    pub fn set_configured_temperature(&mut self, value: f32) {
        if let Some(a) = self.configured_temperature.as_mut() {
            a.current_value = value;
        }
    }
    pub fn set_whitebalance(&mut self, value: u16) {
        if let Some(a) = self.whitebalance.as_mut() {
            a.current_value = Saturating(value);
        }
    }
    pub fn set_brightness(&mut self, value: u8) {
        if let Some(a) = self.brightness.as_mut() {
            a.current_value = Saturating(value);
        }
    }
    pub fn draw<DrawError>(
        &mut self,
        target: &mut impl DrawTarget<Color = BinaryColor, Error = DrawError>,
    ) -> Result<(), DrawError> {
        let rectangle = target.bounding_box();
        let clock = if self.current_time.second() % 2 == 0 {
            self.current_time.format("%H:%M")
        } else {
            self.current_time.format("%H %M")
        }
        .to_string();
        //let clock = self.current_time.format("%H:%M").to_string();
        let clock_text = Text::new(&clock, Point::zero(), TEXT_STYLE);
        let temperature_element = self
            .configured_temperature
            .as_mut()
            .map(AdjustableValue::element);

        let measured_temperature = self.measured_temperature.map(|temp_value| {
            expand(bordered(
                center(owned_text(format!("{temp_value:.1}°C"), TEXT_STYLE)),
                DashedLine::new(2, 2, BinaryColor::On),
            ))
        });
        let whitebalance = self.whitebalance.as_mut().map(AdjustableValue::element);
        let brightness = self.brightness.as_mut().map(AdjustableValue::element);
        if rectangle.size.width > rectangle.size.height {
            horizontal_layout(
                padding(
                    vertical_layout(center(clock_text), 0)
                        .append(measured_temperature, 2)
                        .append(temperature_element, 1),
                    0,
                    1,
                    0,
                    0,
                ),
                1,
            )
            .append(vertical_layout(whitebalance, 1).append(brightness, 1), 1)
            .draw_placed(target, rectangle)?;
        } else {
            vertical_layout(center(clock_text), 0)
                .append(measured_temperature, 2)
                .append(whitebalance, 1)
                .append(brightness, 1)
                .append(temperature_element, 1)
                .draw_placed(target, rectangle)?;
        }
        Ok(())
    }
    pub fn process_touch(&self, p: Point) -> Option<AdjustEvent> {
        self.configured_temperature
            .as_ref()
            .and_then(|e| e.detect_adjustment(p))
            .map(AdjustEvent::Temperature)
            .or_else(|| {
                self.whitebalance
                    .as_ref()
                    .and_then(|e| e.detect_adjustment(p))
                    .map(AdjustEvent::Whitebalance)
            })
            .or_else(|| {
                self.brightness
                    .as_ref()
                    .and_then(|e| e.detect_adjustment(p))
                    .map(AdjustEvent::Brightness)
            })
    }
}
#[derive(Debug, Clone, Copy)]
pub enum AdjustEvent {
    Whitebalance(Adjustment<Saturating<u16>>),
    Brightness(Adjustment<Saturating<u8>>),
    Temperature(Adjustment<f32>),
}
struct AdjustableValue<V, L, C>
where
    V: Copy + Add<Output = V> + Sub<Output = V> + PartialOrd,
    L: Layoutable<C> + ?Sized,
    C: PixelColor,
{
    current_value: V,
    max_value: V,
    min_value: V,
    step_size: V,
    renderer: fn(V) -> L,
    plus_button: Option<Rectangle>,
    minus_button: Option<Rectangle>,
    p: PhantomData<C>,
}

impl<V, L> AdjustableValue<V, L, BinaryColor>
where
    V: Copy + Add<Output = V> + Sub<Output = V> + PartialOrd,
    L: Layoutable<BinaryColor>,
{
    pub fn element(&mut self) -> impl Layoutable<BinaryColor> + '_ {
        show_adjustable_value(
            &mut self.minus_button,
            &mut self.plus_button,
            (self.renderer)(self.current_value),
        )
    }
    pub fn detect_adjustment(&self, p: Point) -> Option<Adjustment<V>> {
        if self.minus_button.map(|r| r.contains(p)).unwrap_or(false) {
            let stepped_value = self.current_value - self.step_size;
            let new_value = if stepped_value > self.min_value {
                stepped_value
            } else {
                self.min_value
            };
            Some(Adjustment {
                old_value: self.current_value,
                new_value,
            })
        } else if self.plus_button.map(|r| r.contains(p)).unwrap_or(false) {
            let stepped_value = self.current_value + self.step_size;
            let new_value = if stepped_value < self.max_value {
                stepped_value
            } else {
                self.max_value
            };
            Some(Adjustment {
                old_value: self.current_value,
                new_value,
            })
        } else {
            None
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub struct Adjustment<V> {
    old_value: V,
    new_value: V,
}

impl<V> Adjustment<V> {
    pub fn old_value(&self) -> &V {
        &self.old_value
    }
    pub fn new_value(&self) -> &V {
        &self.new_value
    }
}

pub fn screen_data(
    show_change_temp: bool,
    show_color: bool,
    show_brightness: bool,
) -> ScreenData<
    impl Layoutable<BinaryColor> + Sized,
    impl Layoutable<BinaryColor> + Sized,
    impl Layoutable<BinaryColor> + Sized,
> {
    let configured_temperature = if show_change_temp {
        Some(AdjustableValue {
            current_value: 21.0,
            max_value: 30.0,
            min_value: 18.0,
            step_size: 0.5,
            renderer: |value| expand(center(owned_text(format!("{value:.1}"), TEXT_STYLE))),
            plus_button: None,
            minus_button: None,
            p: Default::default(),
        })
    } else {
        None
    };
    let whitebalance = if show_color {
        Some(AdjustableValue {
            current_value: Saturating(200),
            max_value: Saturating(370),
            min_value: Saturating(133),
            step_size: Saturating(30),
            renderer: |value: Saturating<u16>| {
                let value = (value - Saturating(133)).0 as f32 / (370 - 133) as f32;
                vertical_layout(
                    padding(center(Image::new(&icons::COLOR, Point::zero())), 1, 1, 1, 1),
                    1,
                )
                .append(scale(value, BinaryColor::On), 0)
            },
            plus_button: None,
            minus_button: None,
            p: Default::default(),
        })
    } else {
        None
    };
    let brightness = if show_brightness {
        Some(AdjustableValue {
            current_value: Saturating(128),
            max_value: Saturating(255),
            min_value: Saturating(0),
            step_size: Saturating(30),
            renderer: |value| {
                let value = value.0 as f32 / 255.0;
                vertical_layout(
                    padding(
                        center(Image::new(&icons::BRIGHTNESS, Point::zero())),
                        1,
                        1,
                        1,
                        1,
                    ),
                    1,
                )
                .append(scale(value, BinaryColor::On), 0)
            },
            plus_button: None,
            minus_button: None,
            p: Default::default(),
        })
    } else {
        None
    };
    ScreenData {
        current_time: Local::now(),
        measured_temperature: None,
        configured_temperature,
        whitebalance,
        brightness,
    }
}

fn show_icon_scale<'a>(
    value: f32,
    minus_button: &'a mut Option<Rectangle>,
    plus_button: &'a mut Option<Rectangle>,
    icon: &'a ImageRaw<BinaryColor>,
) -> impl Layoutable<BinaryColor> + Sized + 'a {
    show_adjustable_value(
        minus_button,
        plus_button,
        vertical_layout(
            padding(center(Image::new(icon, Point::zero())), 1, 1, 1, 1),
            1,
        )
        .append(scale(value, BinaryColor::On), 0),
    )
}

fn show_adjustable_value<'a, L: Layoutable<BinaryColor> + 'a>(
    minus_button: &'a mut Option<Rectangle>,
    plus_button: &'a mut Option<Rectangle>,
    data_visualization: L,
) -> impl Layoutable<BinaryColor> + Sized + 'a {
    expand(center(
        horizontal_layout(
            center(optional_placement(
                minus_button,
                bordered(
                    padding(Text::new("-", Point::zero(), TEXT_STYLE), -2, 1, -1, 1),
                    RoundedLine::new(BinaryColor::On),
                ),
            )),
            0,
        )
        .append(data_visualization, 1)
        .append(
            center(optional_placement(
                plus_button,
                bordered(
                    padding(Text::new("+", Point::zero(), TEXT_STYLE), -2, 1, -1, 1),
                    RoundedLine::new(BinaryColor::On),
                ),
            )),
            0,
        ),
    ))
}

pub fn start_screen_thread(
    display: Lcd128x64BrickletDisplay,
    event_registry: EventRegistry,
) -> Sender<()> {
    let (tx, rx) = mpsc::channel(1);
    tokio::spawn(async move {
        match screen_thread_loop(
            display,
            event_registry,
            rx,
            Some(ClockKey::MinuteClock),
            Some(TemperatureKey::CurrentTemperature),
            Some(TemperatureKey::TargetTemperature),
            Some(LightColorKey::IlluminationColor),
            Some(BrightnessKey::IlluminationBrightness),
        )
        .await
        {
            Ok(()) => {
                info!("Screen thread ended");
            }
            Err(e) => {
                error!("Broke screen thread: {e}")
            }
        }
    });
    tx
}

#[derive(Debug, Error)]
pub enum ScreenDataError {
    #[error("Cannot communicate to device: {0}")]
    Communication(#[from] TinkerforgeError),
    #[error("Cannot update temperature {0}")]
    UpdateTemperature(SendError<f32>),
    #[error("Cannot update whitebalance {0}")]
    UpdateWhitebalance(SendError<Saturating<u16>>),
    #[error("Cannot update brightness {0}")]
    UpdateBrightness(SendError<Saturating<u8>>),
}
enum ScreenMessage {
    Touched(Point),
    LocalTime(DateTime<Local>),
    Closed,
    Dimm,
    SetCurrentTemperature(f32),
    UpdateTemperature(f32),
    UpdateLightColor(Saturating<u16>),
    UpdateBrightness(Saturating<u8>),
}

async fn screen_thread_loop(
    mut display: Lcd128x64BrickletDisplay,
    event_registry: EventRegistry,
    termination_receiver: Receiver<()>,
    clock_key: Option<ClockKey>,
    current_temperature_key: Option<TemperatureKey>,
    adjust_temperature_key: Option<TemperatureKey>,
    light_color_key: Option<LightColorKey>,
    brightness_key: Option<BrightnessKey>,
) -> Result<(), ScreenDataError> {
    display.set_backlight(0).await?;
    let (rx, tx) = mpsc::channel(2);

    let er = event_registry.clone();
    let clock_stream_future = util::optional_stream(
        clock_key.map(|clock| async move { er.clock(clock).await.map(ScreenMessage::LocalTime) }),
    );
    let er = event_registry.clone();
    let current_temperature_stream_future =
        util::optional_stream(current_temperature_key.map(|temp_key| async move {
            er.temperature_stream(temp_key)
                .await
                .map(ScreenMessage::SetCurrentTemperature)
        }));

    let (adjust_temperature_stream, update_temperature_sender) =
        if let Some(adjust_temperature_key) = adjust_temperature_key {
            let current_value_stream = event_registry
                .temperature_stream(adjust_temperature_key)
                .await;
            let value_update_sender = event_registry
                .temperature_sender(adjust_temperature_key)
                .await;
            (
                Either::Left(current_value_stream.map(ScreenMessage::UpdateTemperature)),
                Some(value_update_sender),
            )
        } else {
            (Either::Right(empty::<ScreenMessage>()), None)
        };
    let (update_color_stream, update_color_sender) = if let Some(light_color_key) = light_color_key
    {
        let current_value_stream = event_registry.light_color_stream(light_color_key).await;
        let value_update_sender = event_registry.light_color_sender(light_color_key).await;
        (
            Either::Left(current_value_stream.map(ScreenMessage::UpdateLightColor)),
            Some(value_update_sender),
        )
    } else {
        (Either::Right(empty::<ScreenMessage>()), None)
    };
    let (update_brightness_stream, update_brightness_sender) =
        if let Some(brightness_key) = brightness_key {
            let current_value_stream = event_registry.brightness_stream(brightness_key).await;
            let value_update_sender = event_registry.brightness_sender(brightness_key).await;
            (
                Either::Left(current_value_stream.map(ScreenMessage::UpdateBrightness)),
                Some(value_update_sender),
            )
        } else {
            (Either::Right(empty::<ScreenMessage>()), None)
        };

    let (clock_stream, current_temperature_stream) =
        join!(clock_stream_future, current_temperature_stream_future);

    let mut message_stream = StreamNotifyClose::new(display.input_stream().await?)
        .map(|event| match event {
            Some(TouchPositionEvent {
                pressure: _pressure,
                x,
                y,
                age: _age,
            }) => ScreenMessage::Touched(Point {
                x: x as i32,
                y: y as i32,
            }),
            None => ScreenMessage::Closed,
        })
        .merge(clock_stream)
        .merge(current_temperature_stream)
        .merge(adjust_temperature_stream)
        .merge(update_color_stream)
        .merge(update_brightness_stream)
        .merge(ReceiverStream::new(tx))
        .merge(ReceiverStream::new(termination_receiver).map(|_| ScreenMessage::Closed));

    let mut dimm_timer_handle = None::<JoinHandle<()>>;
    let mut screen = screen_data(
        update_temperature_sender.is_some(),
        update_color_sender.is_some(),
        true,
    );
    screen.draw(&mut display).expect("Infallible");
    display.draw().await?;

    while let Some(message) = message_stream.next().await {
        //let start_time = SystemTime::now();
        match message {
            ScreenMessage::Touched(touch_point) => {
                match screen.process_touch(touch_point) {
                    None => {}
                    Some(AdjustEvent::Whitebalance(adjustment)) => {
                        if let Some(sender) = &update_color_sender {
                            sender
                                .send(*adjustment.new_value())
                                .await
                                .map_err(ScreenDataError::UpdateWhitebalance)?;
                        }
                    }
                    Some(AdjustEvent::Brightness(adjustment)) => {
                        if let Some(sender) = &update_brightness_sender {
                            sender
                                .send(*adjustment.new_value())
                                .await
                                .map_err(ScreenDataError::UpdateBrightness)?;
                        }
                    }
                    Some(AdjustEvent::Temperature(adjustment)) => {
                        if let Some(sender) = &update_temperature_sender {
                            sender
                                .send(*adjustment.new_value())
                                .await
                                .map_err(ScreenDataError::UpdateTemperature)?;
                        }
                    }
                };
                display.set_backlight(100).await?;
                let receiver = rx.clone();
                if let Some(running) = dimm_timer_handle.replace(tokio::spawn(async move {
                    sleep(core::time::Duration::from_secs(10)).await;
                    if let Err(error) = receiver.send(ScreenMessage::Dimm).await {
                        error!("Cannot send message: {error}");
                    }
                })) {
                    running.abort();
                }
            }
            ScreenMessage::LocalTime(now) => {
                screen.set_current_time(now);
            }
            ScreenMessage::Dimm => {
                display.set_backlight(0).await?;
            }
            ScreenMessage::Closed => break,
            ScreenMessage::SetCurrentTemperature(temp) => {
                screen.set_current_tempterature(temp);
            }
            ScreenMessage::UpdateTemperature(temp) => {
                screen.set_configured_temperature(temp);
            }
            ScreenMessage::UpdateLightColor(color) => screen.set_whitebalance(color.0),
            ScreenMessage::UpdateBrightness(brightness) => screen.set_brightness(brightness.0),
        };
        display.clear();
        screen.draw(&mut display).expect("will not happen");
        /*if let Ok(duration) = start_time.elapsed() {
            info!("Prepare time: {:?}", duration);
        }
        let start_time = SystemTime::now();*/
        display.draw().await?;
        /*if let Ok(duration) = start_time.elapsed() {
            info!("Write time: {:?}", duration);
        }*/
    }
    Ok(())
}
