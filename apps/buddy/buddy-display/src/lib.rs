#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # buddy-display
//!
//! What the Claude desk pet's glass says, and nothing about how it is wired.
//!
//! This crate turns a [`BuddyView`] — a snapshot of the buddy domain and the merged wire state —
//! into pixels on any [`DrawTarget`](embedded_graphics::prelude::DrawTarget). The same code
//! drives the ST7789 panel over SPI and a host framebuffer under `cargo test`, so every screen
//! here is verified, screenshotted and held against a golden without a device in the room.
//!
//! - [`render`] — the compositor: the passkey takeover, else the screen, then the overlay.
//! - [`BuddyView`] — the snapshot the render loop shows. It implements
//!   [`Animated`](platform_core::Animated), so the buddy drives the *same* generic render loop
//!   the plant monitor and the pomodoro timer do.
//! - [`canvas_size`] — the canvas shape a rotation needs, for allocating a host target.
//!
//! ## What it owns, and what it asks for
//!
//! It owns the **screens** and the **compositing**. It does *not* own the
//! `(species, PersonaState) -> art` binding: that belongs to [`buddy_creature`], which this crate
//! asks for the sprite and frame to draw. Adding a creature is a `const` in the crate next door,
//! never a match arm here.
//!
//! ## The screens
//!
//! - the **home** screen — the creature, two status rows, and the transcript HUD in the bottom
//!   band: word-wrapped, newest bright and older dim, with a scroll indicator for what is behind
//!   it;
//! - the **approval** screen, which replaces the HUD whenever a permission prompt is pending:
//!   the elapsed seconds (hot after ten), the tool, a hint, and the A-allow / B-deny footer;
//! - the **pet** screen — stats and a how-to;
//! - the **info** screen — about, buttons, claude, device, bluetooth, credits;
//! - the **passkey takeover**, whenever a pairing passkey is active;
//! - the **charging clock**, portrait and landscape;
//! - the **menu**, **settings** and **reset** overlays, drawn on top, innermost first.
//!
//! ## Colour is reused, never re-derived
//!
//! Sprite palettes are quantized to `Rgb565` when the art is generated, and the panel adapter is
//! on `ColorOrder::Rgb`. There is no channel swap anywhere in this crate and there must never be
//! one — exactly one boundary reorders, and it is not this one. See the kb findings
//! `st7789-wants-rgb-colour-order` and `ws2812-grb-byte-order`.
//!
//! ## What a host render can and cannot prove
//!
//! It can prove the layout, the wording, the wrapping, the alignment, and the colour each state
//! is *asked* to be drawn in. It cannot prove anything below the `DrawTarget`: the panel's
//! MADCTL colour order, its CGRAM offset, its inversion, or whether the backlight is even
//! powered. A golden PNG is not a verdict on the glass — only the glass is.

#[cfg(test)]
extern crate alloc;

mod approval;
mod clock;
mod creature;
mod home;
mod hud;
mod info;
mod layout;
mod meter;
mod overlay;
mod page;
mod palette;
mod passkey;
mod pet;
mod screen;
mod status;
mod units;
mod view;
mod wrap;

pub use layout::{canvas_size, LANDSCAPE_CANVAS, PORTRAIT_CANVAS};
pub use passkey::PASSKEY_DIGITS;
pub use screen::render;
pub use status::persona_word;
pub use view::{
    BuddyView, ClockView, DeviceView, Entry, Field, Hint, InfoPage, Overlay, PetPage, PromptView,
    Screen, StatsView, Tool, Transcript, ENTRY_CAP, FIELD_CAP, HINT_CAP, HOT_AFTER_S, TOOL_CAP,
    TRANSCRIPT_SLOTS,
};

// The board-generic foundation, re-exported so the panel adapter and the screenshots example
// reach it through `buddy_display`.
pub use platform_display::{RenderError, SCREEN_SIZE};
