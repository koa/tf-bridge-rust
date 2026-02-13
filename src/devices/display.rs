use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{Dimensions, DrawTarget, Point, Size},
    primitives::Rectangle,
    Pixel,
};
use strum_macros::EnumIter;
use tinkerforge_async::{
    error::TinkerforgeError,
    lcd_128_x_64::{
        Lcd128X64Bricklet, SetDisplayConfigurationRequest,
        SetTouchPositionCallbackConfigurationRequest, TouchLedConfig, TouchPositionCallback,
        WritePixelsRequest,
    },
};
use tokio_stream::{Stream, StreamExt};

use crate::data::wiring::Orientation;

const PIXEL_PER_PAKET: u16 = 448;
const DISPLAY_WIDTH: usize = 128;
const DISPLAY_HEIGHT: usize = 64;
const TOTAL_PIXEL_COUNT: usize = DISPLAY_WIDTH * DISPLAY_HEIGHT;

pub struct Lcd128x64BrickletDisplay {
    bricklet: Lcd128X64Bricklet,
    current_image: [bool; TOTAL_PIXEL_COUNT],
    pending_image: BooleanImage<DISPLAY_WIDTH, TOTAL_PIXEL_COUNT>,
    orientation: Orientation,
    contrast: u8,
    backlight: u8,
}

impl Dimensions for Lcd128x64BrickletDisplay {
    fn bounding_box(&self) -> Rectangle {
        translate_bbox(&self.orientation, self.pending_image.bounding_box())
    }
}

impl DrawTarget for Lcd128x64BrickletDisplay {
    type Color = BinaryColor;
    type Error = ();

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        self.pending_image.draw_iter(
            pixels
                .into_iter()
                .map(|Pixel(p, c)| Pixel(translate_point(&self.orientation, p), c)),
        )
    }
}

impl Lcd128x64BrickletDisplay {
    pub async fn new(
        mut bricklet: Lcd128X64Bricklet,
        orientation: Orientation,
    ) -> Result<Self, TinkerforgeError> {
        bricklet.clear_display().await?;
        bricklet.set_touch_led_config(TouchLedConfig::Off).await?;
        let contrast = 14;
        let backlight = 100;
        bricklet
            .set_display_configuration(SetDisplayConfigurationRequest {
                contrast,
                backlight,
                invert: false,
                automatic_draw: false,
            })
            .await?;
        Ok(Self {
            bricklet,
            current_image: [false; TOTAL_PIXEL_COUNT],
            pending_image: Default::default(),
            orientation,
            contrast,
            backlight,
        })
    }
    pub async fn draw(&mut self) -> Result<(), TinkerforgeError> {
        self.bricklet
            .set_touch_led_config(TouchLedConfig::Off)
            .await?;
        //let mut current_pos = 0;
        //let pixel_count = self.current_image.len();
        let mut y_start = None;
        let mut y_end = None;
        for y in 0u8..64 {
            let start_pos = y as usize * 128;
            let end_pos = (y as usize + 1) * 128;
            if self.current_image[start_pos..end_pos] != self.pending_image.data[start_pos..end_pos]
            {
                if y_start.is_none() {
                    y_start = Some(y);
                }
                y_end = Some(y);
            };
        }
        if let (Some(y_start), Some(y_end)) = (y_start, y_end) {
            //info!("Writing pixels from y={} to y={}", y_start, y_end);
            self.bricklet
                .write_pixels(WritePixelsRequest {
                    x_start: 0,
                    y_start,
                    x_end: 127,
                    y_end,
                    data: &self.pending_image.data
                        [y_start as usize * 128..(y_end as usize + 1) * 128],
                })
                .await?;
            self.current_image = self.pending_image.data;
        }
        self.bricklet.draw_buffered_frame(false).await?;
        self.bricklet
            .set_touch_led_config(TouchLedConfig::Off)
            .await?;

        Ok(())
    }
    pub fn clear(&mut self) {
        self.pending_image.clear(BinaryColor::Off).unwrap();
    }
    pub async fn input_stream(
        &mut self,
    ) -> Result<impl Stream<Item = TouchPositionCallback>, TinkerforgeError> {
        self.bricklet
            .set_touch_position_callback_configuration(
                SetTouchPositionCallbackConfigurationRequest {
                    period: 200,
                    value_has_to_change: true,
                },
            )
            .await?;
        let orientation = self.orientation;

        Ok(self.bricklet.touch_position_stream().await.map(
            move |TouchPositionCallback {
                      pressure,
                      x,
                      y,
                      age,
                  }| {
                let Point { x, y } = translate_reverse(
                    &orientation,
                    Point {
                        x: x as i32,
                        y: y as i32,
                    },
                );
                TouchPositionCallback {
                    pressure,
                    x: x as u16,
                    y: y as u16,
                    age,
                }
            },
        ))
    }
    pub async fn set_backlight(&mut self, value: u8) -> Result<(), TinkerforgeError> {
        if self.backlight == value {
            return Ok(());
        }
        self.bricklet
            .set_display_configuration(SetDisplayConfigurationRequest {
                contrast: self.contrast,
                backlight: value,
                invert: false,
                automatic_draw: false,
            })
            .await?;
        self.backlight = value;
        Ok(())
    }

    pub fn bricklet_mut(&mut self) -> &mut Lcd128X64Bricklet {
        &mut self.bricklet
    }
}

pub struct BooleanImage<const W: usize, const L: usize> {
    data: [bool; L],
}

impl<const W: usize, const L: usize> Default for BooleanImage<W, L> {
    fn default() -> Self {
        Self { data: [false; L] }
    }
}
/*
impl<const W: usize, const L: usize> BooleanImage<W, L> {
    pub fn new() -> Self {
        Self { data: [false; L] }
    }

    pub fn data(&self) -> [bool; L] {
        self.data
    }
}*/

impl<const W: usize, const L: usize> Dimensions for BooleanImage<W, L> {
    fn bounding_box(&self) -> Rectangle {
        Rectangle {
            top_left: Point { x: 0, y: 0 },
            size: Size {
                width: W as u32,
                height: (L / W) as u32,
            },
        }
    }
}

impl<const W: usize, const L: usize> DrawTarget for BooleanImage<W, L> {
    type Color = BinaryColor;
    type Error = ();

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(Point { x, y }, color) in pixels {
            if x >= 0 && x < W as i32 {
                let offset = y * W as i32 + x;
                if offset >= 0 && offset < L as i32 {
                    self.data[offset as usize] = color == BinaryColor::On;
                }
            }
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, EnumIter)]
pub enum OrientationFormat {
    Portrait,
    Landscape,
}

#[inline]
fn translate_point(orientation: &Orientation, p: Point) -> Point {
    match orientation {
        Orientation::Straight => p,
        Orientation::LeftDown => Point {
            x: DISPLAY_WIDTH as i32 - p.y,
            y: p.x,
        },
        Orientation::UpsideDown => Point {
            x: DISPLAY_WIDTH as i32 - p.x - 1,
            y: DISPLAY_HEIGHT as i32 - p.y - 1,
        },
        Orientation::RightDown => Point {
            x: p.y,
            y: DISPLAY_HEIGHT as i32 - p.x - 1,
        },
    }
}

fn translate_reverse(orientation: &Orientation, p: Point) -> Point {
    match orientation {
        Orientation::Straight => p,
        Orientation::LeftDown => Point {
            x: p.y,
            y: DISPLAY_WIDTH as i32 - p.x,
        },
        Orientation::UpsideDown => Point {
            x: DISPLAY_WIDTH as i32 - p.x - 1,
            y: DISPLAY_HEIGHT as i32 - p.y - 1,
        },
        Orientation::RightDown => Point {
            x: DISPLAY_HEIGHT as i32 - p.y - 1,
            y: p.x,
        },
    }
}

#[inline]
pub fn format(orientation: &Orientation) -> OrientationFormat {
    match orientation {
        Orientation::Straight | Orientation::UpsideDown => OrientationFormat::Landscape,
        Orientation::LeftDown | Orientation::RightDown => OrientationFormat::Portrait,
    }
}

fn translate_bbox(orientation: &Orientation, bbox: Rectangle) -> Rectangle {
    match format(orientation) {
        OrientationFormat::Landscape => bbox,
        OrientationFormat::Portrait => Rectangle {
            top_left: Default::default(),
            size: Size {
                width: bbox.size.height,
                height: bbox.size.width,
            },
        },
    }
}

impl Orientation {}

#[cfg(test)]
mod test {
    use embedded_graphics::prelude::Point;
    use strum::IntoEnumIterator;

    use crate::devices::display::{translate_point, translate_reverse, Orientation};

    #[test]
    fn test_translate_and_reverse() {
        for o in Orientation::iter() {
            let p = Point { x: 7, y: 13 };
            let p1 = translate_point(&o, p);
            let p2 = translate_reverse(&o, p1);
            assert_eq!(p, p2, "Orientation {o:?}");
        }
    }
}
