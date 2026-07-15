//! Drawing a sprite frame into a [`DrawTarget`].

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};

use crate::error::RenderError;
use crate::sprite::{Frame, Sprite, SPRITE_SIZE};

/// Draw `frame` of `sprite` at `origin`, each cell scaled to a `scale`×`scale` square,
/// **compositing** over whatever is already on the glass: transparent cells are skipped.
///
/// A `scale` of 0 draws nothing.
///
/// Use this to lay a creature over an existing background *once*. For a creature that
/// animates in place, use [`draw_onto`] — successive frames of a transparent sprite
/// accumulate, leaving the union of every limb it has ever raised.
pub fn draw<D>(
    target: &mut D,
    sprite: &Sprite,
    frame: &Frame,
    origin: Point,
    scale: u32,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    paint(target, sprite, frame, origin, scale, None)
}

/// Draw `frame` of `sprite` at `origin`, painting `background` where the sprite is
/// transparent — so the frame **overwrites its own box** in place.
///
/// This is what lets a creature animate without a clear between frames, and therefore
/// without a flash: exactly the trick the text rows use. A transparent draw would instead
/// smear, because frame N never erases the pixels frame N−1 lit and N does not.
///
/// ## One window, not four hundred
///
/// The box is filled with a single [`fill_contiguous`](DrawTarget::fill_contiguous) over
/// the whole area, streaming pixels row-major. The obvious implementation — one
/// `Rectangle` fill per cell — makes the panel driver set a fresh address window
/// (`CASET`/`RASET`/`RAMWR`) 400 times a frame. Measured on the board, that cost **85 ms**
/// per paint against a 50 ms animation tick: the creature silently dropped frames, and
/// nothing on the glass said so. A framebuffer cannot reveal this; only the hardware can.
/// The `plant-shell` render loop now warns whenever a paint outruns its own tick.
pub fn draw_onto<D>(
    target: &mut D,
    sprite: &Sprite,
    frame: &Frame,
    origin: Point,
    scale: u32,
    background: Rgb565,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    if scale == 0 {
        return Ok(());
    }
    // Copied into the iterator: both are `Copy`, and the closure must outlive the borrow.
    let sprite: Sprite = *sprite;
    let frame: Frame = *frame;
    let extent: u32 = SPRITE_SIZE as u32 * scale;
    let area: Rectangle = Rectangle::new(origin, Size::new(extent, extent));

    // Row-major, matching `Rectangle::points()`, so pixel N of this iterator lands on
    // point N of the area. Every cell yields a colour — transparent becomes `background` —
    // which is what makes the fill contiguous and the erase free.
    let colours = (0..extent).flat_map(move |row: u32| {
        (0..extent).map(move |col: u32| {
            let index: u8 = frame.index_at((col / scale) as usize, (row / scale) as usize);
            sprite.colour(index).unwrap_or(background)
        })
    });

    target
        .fill_contiguous(&area, colours)
        .map_err(RenderError::Draw)
}

/// One `Rectangle` fill per **opaque** cell — the compositing path, where transparent cells
/// must be skipped and a contiguous fill is therefore impossible.
///
/// Up to 400 fills, each its own address window on a real panel. That is why [`draw_onto`],
/// which animates, does not use this. A one-shot composite over a static background can
/// afford it.
fn paint<D>(
    target: &mut D,
    sprite: &Sprite,
    frame: &Frame,
    origin: Point,
    scale: u32,
    background: Option<Rgb565>,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    if scale == 0 {
        return Ok(());
    }
    let cell: Size = Size::new(scale, scale);

    // `try_for_each` over the cells: the first draw error short-circuits rather than
    // leaving half a creature on the panel.
    (0..SPRITE_SIZE).try_for_each(|y: usize| {
        (0..SPRITE_SIZE).try_for_each(|x: usize| {
            let colour: Option<Rgb565> = sprite.colour(frame.index_at(x, y)).or(background);
            let Some(colour): Option<Rgb565> = colour else {
                return Ok(()); // transparent, and no background asked for
            };
            let at: Point = Point::new(
                origin.x + (x as u32 * scale) as i32,
                origin.y + (y as u32 * scale) as i32,
            );
            Rectangle::new(at, cell)
                .into_styled(PrimitiveStyle::with_fill(colour))
                .draw(target)
                .map_err(RenderError::Draw)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sprite::ALL;
    use crate::testing::Framebuffer;

    /// The first sprite of the library — any would do; these tests are about the
    /// renderer, not about a particular creature.
    fn a_sprite() -> &'static Sprite {
        ALL[0]
    }

    fn painted(origin: Point, scale: u32) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::new();
        let sprite: &Sprite = a_sprite();
        draw(&mut fb, sprite, &sprite.frames()[0], origin, scale)
            .expect("a framebuffer render cannot fail");
        fb
    }

    /// Zero: a zero scale paints nothing at all.
    #[test]
    fn a_zero_scale_paints_nothing() {
        assert_eq!(painted(Point::new(0, 0), 0).lit_pixels(), 0);
    }

    /// One: at scale 1 the creature paints, and paints inside the canvas.
    #[test]
    fn a_sprite_paints_ink_and_stays_on_the_canvas() {
        let fb: Framebuffer = painted(Point::new(0, 0), 1);
        assert!(fb.lit_pixels() > 0);
        assert_eq!(fb.escaped(), 0);
    }

    /// Many: scaling multiplies the painted area by exactly scale².
    #[test]
    fn scaling_squares_the_painted_area() {
        let one: usize = painted(Point::new(0, 0), 1).lit_pixels();
        let three: usize = painted(Point::new(0, 0), 3).lit_pixels();
        assert_eq!(three, one * 9);
    }

    /// The sprite is transparent, not opaque: it does not paint its own background, so
    /// the lit-pixel count is well under the 400 cells of the grid.
    #[test]
    fn transparent_cells_are_left_alone() {
        let fb: Framebuffer = painted(Point::new(0, 0), 1);
        assert!(
            fb.lit_pixels() < crate::sprite::CELLS_PER_FRAME,
            "the sprite painted every cell — transparency was ignored"
        );
    }

    /// A sprite drawn past the right edge escapes rather than silently clipping — the
    /// caller, not this function, is responsible for placing it on the canvas.
    #[test]
    fn a_sprite_drawn_off_the_canvas_escapes() {
        let fb: Framebuffer = painted(Point::new(crate::SCREEN_SIZE.width as i32, 0), 1);
        assert!(fb.escaped() > 0);
        assert_eq!(fb.lit_pixels(), 0);
    }

    /// `draw` and `draw_onto` are now two different algorithms — per-cell rectangles, and
    /// one contiguous row-major stream. Over a background that already matches, they must
    /// paint **identical pixels**. This is the only thing standing between them and a
    /// silent divergence, and it is also what pins `fill_contiguous`'s row-major order to
    /// the per-cell `(x, y)` mapping: transpose one and this fails immediately.
    #[test]
    fn the_opaque_and_compositing_paths_paint_the_same_picture() {
        let sprite: &Sprite = a_sprite();
        let frame: &Frame = &sprite.frames()[0];
        let origin: Point = Point::new(3, 7);

        let mut composited: Framebuffer = Framebuffer::new(); // already black
        draw(&mut composited, sprite, frame, origin, 3).expect("composite over black");

        let mut opaque: Framebuffer = Framebuffer::new();
        draw_onto(&mut opaque, sprite, frame, origin, 3, Rgb565::BLACK).expect("opaque paint");

        assert_eq!(composited.pixels(), opaque.pixels());
    }

    /// The opaque path fills its whole box, which is what erases the previous frame. A
    /// non-black background makes the claim testable: every cell of the box is painted.
    #[test]
    fn the_opaque_path_fills_its_entire_box() {
        let sprite: &Sprite = a_sprite();
        let scale: u32 = 3;
        let mut fb: Framebuffer = Framebuffer::new();
        draw_onto(
            &mut fb,
            sprite,
            &sprite.frames()[0],
            Point::new(0, 0),
            scale,
            Rgb565::WHITE,
        )
        .expect("opaque paint");

        let extent: usize = SPRITE_SIZE * scale as usize;
        assert_eq!(fb.escaped(), 0);
        assert_eq!(
            fb.lit_pixels(),
            extent * extent,
            "the opaque path left holes: it cannot erase the frame before it"
        );
    }

    /// A zero scale still draws nothing on the opaque path.
    #[test]
    fn a_zero_scale_paints_nothing_on_the_opaque_path() {
        let sprite: &Sprite = a_sprite();
        let mut fb: Framebuffer = Framebuffer::new();
        draw_onto(
            &mut fb,
            sprite,
            &sprite.frames()[0],
            Point::new(0, 0),
            0,
            Rgb565::WHITE,
        )
        .expect("nothing to paint");
        assert_eq!(fb.lit_pixels(), 0);
    }

    /// Moving the origin translates the picture, it does not change it.
    #[test]
    fn the_origin_translates_the_sprite() {
        let a: Framebuffer = painted(Point::new(0, 0), 2);
        let b: Framebuffer = painted(Point::new(40, 20), 2);
        assert_eq!(a.lit_pixels(), b.lit_pixels());
        assert_ne!(a.pixels(), b.pixels());
    }
}
