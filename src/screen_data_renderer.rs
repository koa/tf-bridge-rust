use std::num::Saturating;
use std::{
    marker::PhantomData,
    ops::{Add, Sub},
};

use chrono::{DateTime, Local};
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::{
    geometry::Point,
    image::{Image, ImageRaw},
    mono_font::{iso_8859_1::FONT_6X12, MonoTextStyle},
    pixelcolor::BinaryColor,
    prelude::PixelColor,
    primitives::Rectangle,
    text::Text,
};
use simple_layout::prelude::{
    bordered, center, expand, horizontal_layout, optional_placement, owned_text, padding, scale,
    vertical_layout, DashedLine, Layoutable, RoundedLine,
};
use tokio_stream::StreamExt;

use crate::icons;

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
        let clock = Local::now().format("%H:%M").to_string();
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
