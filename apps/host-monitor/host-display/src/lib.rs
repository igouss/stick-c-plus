#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # host-display
//!
//! What the host monitor's glass says, and nothing about how it is wired.
//!
//! This crate turns a [`HostState`] — the rolling [`History`](host_core::History) of
//! CPU/memory samples plus the current [`Status`](host_core::Status) — into pixels on
//! any [`DrawTarget`](embedded_graphics::prelude::DrawTarget). It is the seam between
//! the host-monitor domain and the graphics port: `DrawTarget` is the port, the
//! on-target ST7789 panel (`firmware` adapters) is one adapter, a host framebuffer is
//! another. The board-generic parts it draws with — the [`sparkline`] and text
//! primitives, the ClaudePix sprite library, the render error — live in
//! [`platform_display`] and are re-exported here, so this crate holds only the
//! host-monitor-specific *picture*.
//!
//! - [`render`] — the monitor screen: two stacked scrolling graphs (CPU on top,
//!   memory below), each with a live percentage, beside the creature that stands for
//!   the host's load.
//! - [`scene`] — the animation policy: which creature, and whether it moves. A calm or
//!   merely busy host maps to a *motionless* creature, so the render loop repaints
//!   nothing extra and the device may rest; only a pegged host or a fault is allowed to
//!   move.
//!
//! ## The graph outlives the reading
//!
//! Unlike the plant monitor — whose scalar goes *unavailable* the instant it is stale,
//! because a frozen number is a lie — this crate keeps drawing the retained history
//! even when the host has gone dark. A rolling window of what the host was doing before
//! it stopped answering is useful, not misleading; the *label* switches to `--` and the
//! creature falls asleep, but the trailing bars stay on the glass.
//!
//! ## What a host render can and cannot prove
//!
//! It **can** prove the layout, the wording, the alignment, the colour each state is
//! drawn in, that a receding graph erases the taller bars it replaces, and that nothing
//! is clipped. It **cannot** prove anything below [`DrawTarget`]: the panel's colour
//! order, offset, inversion, or backlight — see [`platform_display`].

mod glass;
mod layout;
pub mod scene;
mod screen;

pub use glass::Glass;
pub use layout::{CPU_GRAPH, MEM_GRAPH, SPRITE_ORIGIN, SPRITE_SCALE};
pub use scene::{frame_index, is_animated, scene, LoadBand, Motion, Scene, BUSY_AT, PEGGED_AT};
pub use screen::render;

// The domain state the display draws, re-exported so a consumer that already reaches
// for `host_display` (the ST7789 adapter closure, the screenshots example) need not also
// name `host_core` for the one type it hands `render`.
pub use host_core::HostState;

// The board-generic foundation, re-exported so consumers (the ST7789 adapter, the
// screenshots example) reach it through `host_display`.
pub use platform_display::{sparkline, sprite, text_line, RenderError, SCREEN_SIZE};
