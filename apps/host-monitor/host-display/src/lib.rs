#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # host-display
//!
//! What the host monitor's glass says, and nothing about how it is wired.
//!
//! This crate turns a [`HostState`] — the last good [`Pulse`](host_core::Pulse) frame plus
//! the endpoint's [`Status`](host_core::Status) — into pixels on any
//! [`DrawTarget`](embedded_graphics::prelude::DrawTarget). It is the seam between the
//! host-monitor domain and the graphics port: `DrawTarget` is the port, the on-target ST7789
//! panel (`firmware` adapters) is one adapter, a host framebuffer is another. The
//! board-generic parts it draws with — the [`sparkline`] and text primitives, the render
//! error — live in [`platform_display`] and are re-exported here, so this crate holds only
//! the host-monitor-specific *picture*.
//!
//! - [`render`] — the monitor screen: one row per homelab host, each a name and two current
//!   percentages above two side-by-side scrolling graphs (CPU cyan, memory yellow).
//! - [`layout`] — where the three rows and their sparklines sit, with the geometry asserted
//!   at compile time.
//!
//! ## The frame outlives the reading
//!
//! Unlike the plant monitor — whose scalar goes *unavailable* the instant it is stale — this
//! crate keeps drawing the last good frame even when the endpoint has gone dark. A window of
//! what the hosts were doing is useful, not misleading; the host names tint and a status
//! token (`DOWN` / `BAD` / `OLD`) appears, but the bars stay on the glass. A single host the
//! endpoint reports as down keeps its row and shows "no data".
//!
//! ## No creature
//!
//! The single-host monitor had room beside its two graphs for an animated ClaudePix creature.
//! Three hosts fill the 240×135 panel edge to edge, so there is nowhere for one to live
//! without crowding a row — the creature is retired here, and the endpoint's health is shown
//! by the tinted names and the status token instead. The screen is therefore *still*: the
//! render loop repaints only when the frame or the status changes.
//!
//! ## What a host render can and cannot prove
//!
//! It **can** prove the layout, the wording, the alignment, the colour each state is drawn
//! in, that a receding graph erases the taller bars it replaces, and that nothing is clipped.
//! It **cannot** prove anything below [`DrawTarget`]: the panel's colour order, offset,
//! inversion, or backlight — see [`platform_display`].

mod glass;
pub mod layout;
mod screen;

pub use glass::Glass;
pub use layout::{cpu_graph, mem_graph, ROWS};
pub use screen::{render, PEGGED_AT};

// The domain state the display draws, re-exported so a consumer that already reaches for
// `host_display` (the ST7789 adapter closure, the screenshots example) need not also name
// `host_core` for the one type it hands `render`.
pub use host_core::HostState;

// The board-generic foundation, re-exported so consumers (the ST7789 adapter, the
// screenshots example) reach it through `host_display`.
pub use platform_display::{sparkline, text_line, RenderError, SCREEN_SIZE};
