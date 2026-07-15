//! The failure of a render, over any [`DrawTarget`](embedded_graphics::prelude::DrawTarget).

use core::fmt;

/// A render failed: either the target refused a draw, or a line did not fit.
///
/// Generic in the target's own error so this crate names no concrete panel — the
/// ST7789's SPI error and a host framebuffer's [`Infallible`](core::convert::Infallible)
/// both flow through unchanged, and each adapter maps the result into its own error.
///
/// [`LineOverflow`](Self::LineOverflow) is unreachable for the content this crate
/// actually formats — [`fits_the_line_buffer`] proves it over every `u16` raw count
/// and every percent — but the buffer is finite, so the type says so rather than
/// truncating a value silently. A screen that quietly drops a digit is exactly the
/// kind of convenient lie this project refuses.
///
/// [`fits_the_line_buffer`]: crate::layout
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenderError<E> {
    /// The draw target rejected a pixel write.
    Draw(E),
    /// A formatted line exceeded the stack line buffer. See the type docs: the
    /// content this crate renders cannot reach this, and a test proves it.
    LineOverflow,
}

impl<E: fmt::Display> fmt::Display for RenderError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::Draw(err) => write!(f, "draw target rejected the write: {err}"),
            RenderError::LineOverflow => f.write_str("a rendered line overflowed its buffer"),
        }
    }
}
