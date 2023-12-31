use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{Dimensions, DrawTarget, Point, Size},
    primitives::Rectangle,
    Pixel,
};
use strum_macros::EnumIter;
use sub_array::SubArray;
use tinkerforge_async::{
    error::TinkerforgeError,
    lcd_128x64_bricklet::{
        Lcd128x64Bricklet, TouchPositionEvent, LCD_128X64_BRICKLET_STATUS_LED_CONFIG_OFF,
        LCD_128X64_BRICKLET_TOUCH_LED_CONFIG_OFF, LCD_128X64_BRICKLET_TOUCH_LED_CONFIG_ON,
    },
};
use tokio_stream::{Stream, StreamExt};

const PIXEL_PER_PAKET: u16 = 448;
const DISPLAY_WIDTH: usize = 128;
const DISPLAY_HEIGHT: usize = 64;
const TOTAL_PIXEL_COUNT: usize = DISPLAY_WIDTH * DISPLAY_HEIGHT;

pub struct Lcd128x64BrickletDisplay {
    bricklet: Lcd128x64Bricklet,
    current_image: [bool; TOTAL_PIXEL_COUNT],
    pending_image: BooleanImage<DISPLAY_WIDTH, TOTAL_PIXEL_COUNT>,
    orientation: Orientation,
    contrast: u8,
    backlight: u8,
}

impl Dimensions for Lcd128x64BrickletDisplay {
    fn bounding_box(&self) -> Rectangle {
        self.orientation
            .translate_bbox(self.pending_image.bounding_box())
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
                .map(|Pixel(p, c)| Pixel(self.orientation.translate_point(p), c)),
        )
    }
}

impl Lcd128x64BrickletDisplay {
    pub async fn new(
        mut bricklet: Lcd128x64Bricklet,
        orientation: Orientation,
    ) -> Result<Self, TinkerforgeError> {
        bricklet.clear_display().await?;
        bricklet
            .set_status_led_config(LCD_128X64_BRICKLET_STATUS_LED_CONFIG_OFF)
            .await?;
        let contrast = 14;
        let backlight = 100;
        bricklet
            .set_display_configuration(contrast, backlight, false, false)
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
            .set_touch_led_config(LCD_128X64_BRICKLET_TOUCH_LED_CONFIG_ON)
            .await?;
        let mut current_pos = 0;
        let pixel_count = self.current_image.len();
        while current_pos < pixel_count {
            //println!("Scan from {current_pos}");
            while current_pos < pixel_count
                && self.current_image[current_pos..current_pos + 64]
                    == self.pending_image.data[current_pos..current_pos + 64]
            {
                current_pos += 64;
            }
            if current_pos >= TOTAL_PIXEL_COUNT {
                break;
            }
            //println!("Paint from {current_pos}");
            let remaining_pixels = TOTAL_PIXEL_COUNT - current_pos;
            if remaining_pixels > PIXEL_PER_PAKET as usize {
                let until_offset = current_pos as u16 + PIXEL_PER_PAKET;
                let data_chunk = self.pending_image.data.sub_array_ref(current_pos);
                self.bricklet
                    .write_pixels_low_level(
                        0,
                        0,
                        127,
                        63,
                        until_offset,
                        current_pos as u16,
                        data_chunk,
                    )
                    .await?;
                self.current_image[current_pos..current_pos + PIXEL_PER_PAKET as usize]
                    .copy_from_slice(data_chunk);
            } else {
                let mut temp_array = [false; PIXEL_PER_PAKET as usize];
                let data_chunk = &self.pending_image.data[current_pos..TOTAL_PIXEL_COUNT];
                temp_array[0..remaining_pixels].copy_from_slice(data_chunk);
                self.bricklet
                    .write_pixels_low_level(
                        0,
                        0,
                        127,
                        63,
                        TOTAL_PIXEL_COUNT as u16,
                        current_pos as u16,
                        &temp_array,
                    )
                    .await?;
                self.current_image[current_pos..TOTAL_PIXEL_COUNT].copy_from_slice(data_chunk);
            }
            current_pos += PIXEL_PER_PAKET as usize;
        }
        self.bricklet.draw_buffered_frame(false).await?;
        self.bricklet
            .set_touch_led_config(LCD_128X64_BRICKLET_TOUCH_LED_CONFIG_OFF)
            .await?;

        Ok(())
    }
    pub fn clear(&mut self) {
        self.pending_image.clear(BinaryColor::Off).unwrap();
    }
    pub async fn input_stream(
        &mut self,
    ) -> Result<impl Stream<Item = TouchPositionEvent>, TinkerforgeError> {
        self.bricklet
            .set_touch_position_callback_configuration(200, true)
            .await?;
        let orientation = self.orientation.clone();

        Ok(self
            .bricklet
            .get_touch_position_callback_receiver()
            .await
            .map(
                move |TouchPositionEvent {
                          pressure,
                          x,
                          y,
                          age,
                      }| {
                    let Point { x, y } = orientation.translate_reverse(Point {
                        x: x as i32,
                        y: y as i32,
                    });
                    TouchPositionEvent {
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
            .set_display_configuration(self.contrast, value, false, false)
            .await?;
        self.backlight = value;
        Ok(())
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

impl<const W: usize, const L: usize> BooleanImage<W, L> {
    pub fn new() -> Self {
        Self { data: [false; L] }
    }

    pub fn data(&self) -> [bool; L] {
        self.data
    }
}

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
pub enum Orientation {
    Straight,
    LeftDown,
    UpsideDown,
    RightDown,
}
#[derive(Copy, Clone, Debug, Eq, PartialEq, EnumIter)]
pub enum OrientationFormat {
    Portrait,
    Landscape,
}

impl Orientation {
    #[inline]
    fn translate_point(&self, p: Point) -> Point {
        match self {
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
    fn translate_reverse(&self, p: Point) -> Point {
        match self {
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
    pub fn format(&self) -> OrientationFormat {
        match self {
            Orientation::Straight | Orientation::UpsideDown => OrientationFormat::Landscape,
            Orientation::LeftDown | Orientation::RightDown => OrientationFormat::Portrait,
        }
    }
    fn translate_bbox(&self, bbox: Rectangle) -> Rectangle {
        match self.format() {
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
}

#[cfg(test)]
mod test {
    use crate::devices::display::Orientation;
    use embedded_graphics::prelude::Point;
    use strum::IntoEnumIterator;

    #[test]
    fn test_translate_and_reverse() {
        for o in Orientation::iter() {
            let p = Point { x: 7, y: 13 };
            let p1 = o.translate_point(p);
            let p2 = o.translate_reverse(p1);
            assert_eq!(p, p2, "Orientation {o:?}");
        }
    }
}
