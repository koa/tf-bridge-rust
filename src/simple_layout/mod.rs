use std::borrow::Cow;
use std::ops::Range;

use embedded_graphics::geometry::{Dimensions, Size};
use embedded_graphics::pixelcolor::PixelColor;
use embedded_graphics::prelude::{DrawTarget, Point};
use embedded_graphics::primitives::Rectangle;
use embedded_graphics::text::renderer::TextRenderer;
use embedded_graphics::text::Text;
use embedded_graphics::{Drawable, Pixel};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ComponentSize {
    preferred_width: u32,
    preferred_height: u32,
    width_range: Range<u32>,
    height_range: Range<u32>,
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct Placement {
    pub position: Point,
    pub size: Size,
}

impl ComponentSize {
    pub fn fixed_size(width: u32, height: u32) -> ComponentSize {
        ComponentSize {
            preferred_width: width,
            preferred_height: height,
            width_range: width..width,
            height_range: height..height,
        }
    }
    pub fn new(
        preferred_width: u32,
        preferred_height: u32,
        width_range: Range<u32>,
        height_range: Range<u32>,
    ) -> Self {
        Self {
            preferred_width,
            preferred_height,
            width_range,
            height_range,
        }
    }
    fn along<O: Orientation>(&self) -> (u32, Range<u32>) {
        O::extract_data(Cow::Borrowed(self))
    }
}

trait Orientation {
    fn extract_data(size: Cow<'_, ComponentSize>) -> (u32, Range<u32>);
}
struct Horizontal {}

impl Orientation for Horizontal {
    fn extract_data(size: Cow<'_, ComponentSize>) -> (u32, Range<u32>) {
        (size.preferred_width, size.width_range.clone())
    }
}
struct Vertical {}
impl Orientation for Vertical {
    fn extract_data(size: Cow<'_, ComponentSize>) -> (u32, Range<u32>) {
        (size.preferred_height, size.height_range.clone())
    }
}

pub trait Layoutable<Color: PixelColor> {
    fn size(&self) -> Cow<'_, ComponentSize>;
    fn draw_placed<DrawError>(
        &self,
        target: &mut impl DrawTarget<Color = Color, Error = DrawError>,
        position: Placement,
    ) -> Result<Point, DrawError>;
}

impl<'a, S: TextRenderer<Color = Color>, Color: PixelColor> Layoutable<Color> for Text<'a, S> {
    fn size(&self) -> Cow<'_, ComponentSize> {
        let mut total_height = 0;
        let mut max_line_length = 0;
        for line in self.text.split('\n') {
            let metrics = self.character_style.measure_string(
                line,
                Point::default(),
                self.text_style.baseline,
            );
            let bbox = metrics.bounding_box;
            if bbox.size.width > max_line_length {
                max_line_length = bbox.size.width;
            }
            total_height += bbox.size.height;
        }
        Cow::Owned(ComponentSize::fixed_size(max_line_length, total_height))
    }

    fn draw_placed<DrawError>(
        &self,
        target: &mut impl DrawTarget<Color = Color, Error = DrawError>,
        position: Placement,
    ) -> Result<Point, DrawError> {
        let height = self.character_style.line_height();
        let offset = (position.position - self.position)
            + Point {
                x: 0,
                y: height as i32,
            };
        Drawable::draw(self, &mut OffsetDrawable { target, offset })
    }
}

struct OffsetDrawable<'a, Color, Error, Target>
where
    Target: DrawTarget<Color = Color, Error = Error>,
    Color: PixelColor,
{
    target: &'a mut Target,
    offset: Point,
}

impl<'a, Color, Error, Target> Dimensions for OffsetDrawable<'a, Color, Error, Target>
where
    Target: DrawTarget<Color = Color, Error = Error>,
    Color: PixelColor,
{
    fn bounding_box(&self) -> Rectangle {
        let bbox = self.target.bounding_box();
        Rectangle {
            top_left: bbox.top_left - self.offset,
            size: bbox.size,
        }
    }
}

impl<'a, Color, Error, Target> DrawTarget for OffsetDrawable<'a, Color, Error, Target>
where
    Target: DrawTarget<Color = Color, Error = Error>,
    Color: PixelColor,
{
    type Color = Color;
    type Error = Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let offset = self.offset;
        self.target.draw_iter(
            pixels
                .into_iter()
                .map(|Pixel::<Self::Color>(p, c)| Pixel(p + offset, c)),
        )
    }
}
