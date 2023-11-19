use std::borrow::Cow;
use std::cmp::Ordering;
use std::marker::PhantomData;
use std::ops::{AddAssign, Range};

use embedded_graphics::geometry::{Dimensions, Size};
use embedded_graphics::pixelcolor::PixelColor;
use embedded_graphics::prelude::{DrawTarget, Point};
use embedded_graphics::primitives::Rectangle;
use embedded_graphics::text::renderer::TextRenderer;
use embedded_graphics::text::Text;
use embedded_graphics::{Drawable, Pixel};

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct ComponentSize {
    width: ValueRange<u32>,
    height: ValueRange<u32>,
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
struct ValueRange<V> {
    preferred_value: V,
    min_value: V,
    max_value: V,
}

impl<V: PartialOrd + Clone> ValueRange<V> {
    fn expand(&mut self, rhs: &Self) {
        if self.preferred_value < rhs.preferred_value {
            self.preferred_value = rhs.preferred_value.clone();
        }
        if self.min_value < rhs.min_value {
            self.min_value = rhs.min_value.clone()
        }
        if self.max_value < rhs.max_value {
            self.max_value = rhs.max_value.clone()
        }
    }
}

impl<V: AddAssign> AddAssign for ValueRange<V> {
    fn add_assign(&mut self, rhs: Self) {
        self.preferred_value += rhs.preferred_value;
        self.min_value += rhs.min_value;
        self.max_value += rhs.max_value;
    }
}

impl<V: Clone> ValueRange<V> {
    fn fixed(value: V) -> Self {
        Self {
            preferred_value: value.clone(),
            min_value: value.clone(),
            max_value: value,
        }
    }
}
impl ValueRange<u32> {
    fn expand_max(&self) -> Self {
        Self {
            preferred_value: self.preferred_value,
            min_value: self.max_value,
            max_value: u32::MAX,
        }
    }
}

impl ComponentSize {
    pub fn fixed_size(width: u32, height: u32) -> ComponentSize {
        ComponentSize {
            width: ValueRange::fixed(width),
            height: ValueRange::fixed(height),
        }
    }
    pub fn new(
        preferred_width: u32,
        preferred_height: u32,
        width_range: Range<u32>,
        height_range: Range<u32>,
    ) -> Self {
        Self {
            width: ValueRange {
                preferred_value: preferred_width,
                min_value: width_range.start,
                max_value: width_range.end,
            },
            height: ValueRange {
                preferred_value: preferred_height,
                min_value: height_range.start,
                max_value: height_range.end,
            },
        }
    }
}

trait Orientation {
    fn split_component_size(size: Cow<ComponentSize>) -> (ValueRange<u32>, ValueRange<u32>);
    fn split_size(size: Size) -> (u32, u32);
    fn split_point(p: Point) -> (i32, i32);
    fn create_component_size(along: ValueRange<u32>, cross: ValueRange<u32>) -> ComponentSize;
    fn create_size(along: u32, across: u32) -> Size;
    fn create_point(along: i32, cross: i32) -> Point;
}
struct Horizontal {}

impl Orientation for Horizontal {
    #[inline]
    fn split_component_size(size: Cow<ComponentSize>) -> (ValueRange<u32>, ValueRange<u32>) {
        (size.width, size.height)
    }

    #[inline]
    fn split_size(size: Size) -> (u32, u32) {
        (size.width, size.height)
    }

    #[inline]
    fn create_component_size(along: ValueRange<u32>, cross: ValueRange<u32>) -> ComponentSize {
        ComponentSize {
            width: along,
            height: cross,
        }
    }

    #[inline]
    fn create_size(along: u32, across: u32) -> Size {
        Size {
            width: along,
            height: across,
        }
    }

    #[inline]
    fn split_point(p: Point) -> (i32, i32) {
        let Point { x, y } = p;
        (x, y)
    }

    #[inline]
    fn create_point(along: i32, cross: i32) -> Point {
        Point { x: along, y: cross }
    }
}
pub struct Vertical {}
impl Orientation for Vertical {
    fn split_component_size(size: Cow<ComponentSize>) -> (ValueRange<u32>, ValueRange<u32>) {
        (size.height, size.width)
    }

    fn split_size(size: Size) -> (u32, u32) {
        (size.height, size.width)
    }

    fn split_point(p: Point) -> (i32, i32) {
        (p.y, p.x)
    }

    fn create_component_size(along: ValueRange<u32>, cross: ValueRange<u32>) -> ComponentSize {
        ComponentSize {
            width: cross,
            height: along,
        }
    }

    fn create_size(along: u32, across: u32) -> Size {
        Size {
            width: across,
            height: along,
        }
    }

    fn create_point(along: i32, cross: i32) -> Point {
        Point { x: cross, y: along }
    }
}

pub trait Layoutable<Color: PixelColor> {
    fn size(&self) -> Cow<'_, ComponentSize>;
    fn draw_placed<DrawError>(
        &self,
        target: &mut impl DrawTarget<Color = Color, Error = DrawError>,
        position: Rectangle,
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
        position: Rectangle,
    ) -> Result<Point, DrawError> {
        let offset = if let Some(first_line) = self.text.split('\n').next() {
            self.character_style
                .measure_string(first_line, Point::default(), self.text_style.baseline)
                .bounding_box
                .top_left
        } else {
            Point::zero()
        };
        let offset = position.top_left - self.position - offset;
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
pub struct LinearPair<L1: Layoutable<C>, L2: Layoutable<C>, C: PixelColor, O: Orientation> {
    l1: L1,
    l2: L2,
    weights: [u32; 2],
    p1: PhantomData<C>,
    p2: PhantomData<O>,
}

impl<L1: Layoutable<C>, L2: Layoutable<C>, C: PixelColor, O: Orientation> Layoutable<C>
    for LinearPair<L1, L2, C, O>
{
    fn size(&self) -> Cow<'_, ComponentSize> {
        let mut total_along = ValueRange::default();
        let mut total_cross = ValueRange::default();
        for size in self.sizes() {
            let (along, cross) = O::split_component_size(size);
            total_along += along;
            total_cross.expand(&cross);
        }
        Cow::Owned(O::create_component_size(total_along, total_cross))
    }

    fn draw_placed<DrawError>(
        &self,
        target: &mut impl DrawTarget<Color = C, Error = DrawError>,
        position: Rectangle,
    ) -> Result<Point, DrawError> {
        let (along_target, cross_target) = O::split_size(position.size);
        let (mut along_offset, cross_offset) = O::split_point(position.top_left);
        let sizes = self.sizes().map(|s| O::split_component_size(s).0);
        let preferred_sizes = sizes.map(|s| s.preferred_value);
        let total_preferred: u32 = preferred_sizes.iter().sum();
        let places = match along_target.cmp(&total_preferred) {
            Ordering::Less => {
                todo!()
            }
            Ordering::Equal => preferred_sizes,
            Ordering::Greater => {
                let max_sizes = sizes.map(|s| s.max_value);
                let total_max = max_sizes.iter().map(|v| *v as u64).sum::<u64>();
                if total_max > along_target as u64 {
                    let mut remaining_budget = along_target - total_preferred;
                    let mut result_sizes = preferred_sizes;
                    let weights = self.weights;
                    while remaining_budget > 0 {
                        let remaining_budget_before = remaining_budget;
                        let entries_with_headroom = weights
                            .iter()
                            .zip(result_sizes.iter_mut())
                            .zip(sizes.iter())
                            .filter(|((weight, result_size), size)| {
                                **weight > 0 && **result_size < size.max_value
                            })
                            .collect::<Vec<_>>();
                        let mut remaining_weights: u32 = entries_with_headroom
                            .iter()
                            .map(|((weight, _), _)| **weight)
                            .sum();
                        if remaining_weights == 0 {
                            break;
                        }

                        for ((weight, result_size), size) in entries_with_headroom {
                            let theoretical_increase =
                                remaining_budget * *weight / remaining_weights;
                            let selected_increase =
                                (theoretical_increase).min(size.max_value - *result_size);
                            *result_size += selected_increase;
                            remaining_budget -= theoretical_increase;
                            remaining_weights -= weight;
                        }
                        if remaining_budget_before == remaining_budget {
                            // nothing more to distribute -> break
                            break;
                        }
                    }
                    result_sizes
                } else {
                    max_sizes
                }
            }
        }
        .map(|l| {
            let place = Rectangle {
                top_left: O::create_point(along_offset, cross_offset),
                size: O::create_size(l, cross_target),
            };
            along_offset += l as i32;
            place
        });
        self.draw_placed_components(target, places)
    }
}

impl<L1: Layoutable<C>, L2: Layoutable<C>, C: PixelColor, O: Orientation> LinearPair<L1, L2, C, O> {
    fn draw_placed_components<DrawError>(
        &self,
        target: &mut impl DrawTarget<Color = C, Error = DrawError>,
        places: [Rectangle; 2],
    ) -> Result<Point, DrawError> {
        self.l1.draw_placed(target, places[0])?;
        self.l2.draw_placed(target, places[1])
    }
    fn sizes(&self) -> [Cow<ComponentSize>; 2] {
        [self.l1.size(), self.l2.size()]
    }
    fn weights(&self) -> &[u32; 2] {
        &self.weights
    }
}

impl<L1: Layoutable<C>, L2: Layoutable<C>, C: PixelColor, O: Orientation> From<(L1, L2)>
    for LinearPair<L1, L2, C, O>
{
    fn from((l1, l2): (L1, L2)) -> Self {
        Self {
            l1,
            l2,
            weights: [1, 1],
            p1: PhantomData,
            p2: PhantomData,
        }
    }
}
struct ExpandLayoutable<L: Layoutable<C>, C: PixelColor> {
    layoutable: L,
    p: PhantomData<C>,
}

pub fn expand<L: Layoutable<C>, C: PixelColor>(input: L) -> impl Layoutable<C> {
    ExpandLayoutable {
        layoutable: input,
        p: Default::default(),
    }
}

impl<L: Layoutable<C>, C: PixelColor> Layoutable<C> for ExpandLayoutable<L, C> {
    fn size(&self) -> Cow<'_, ComponentSize> {
        let ComponentSize { width, height } = self.layoutable.size().into_owned();
        Cow::Owned(ComponentSize {
            width: width.expand_max(),
            height: height.expand_max(),
        })
    }

    fn draw_placed<DrawError>(
        &self,
        target: &mut impl DrawTarget<Color = C, Error = DrawError>,
        position: Rectangle,
    ) -> Result<Point, DrawError> {
        self.layoutable.draw_placed(target, position)
    }
}
