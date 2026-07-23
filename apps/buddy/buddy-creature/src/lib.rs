#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # buddy-creature
//!
//! The reusable creature foundation — the seam between the persona FSM (buddy-core) and the
//! sprite pipeline (platform-display). Given a species and a [`PersonaState`], it answers
//! *which* animation plays and, over elapsed time, *which* sprite in that state's set is
//! current — so that adding a creature is adding a `const` data value, never a match arm.
//!
//! Pure `no_std`, no `alloc`, no drawing. A species' sprite sets are `&'static` slices of
//! `&'static Sprite`, and the registry is a `&'static` slice of `&'static Species`; the crate
//! *borrows* vendored art, it never rasterizes it. Time is the injected `elapsed_ms`
//! parameter — this crate never reads a clock.
//!
//! ## Hexagon
//! - Role **port-and-adapter**, context **buddy**: presentation policy over shared art.
//! - **Inward deps only** — `platform-display` (the [`Sprite`](platform_display::sprite::Sprite)
//!   type and the vendored preset library) and `buddy-core`
//!   ([`PersonaState`] + [`SpeciesIndex`]). No `embedded-graphics`, no framework, no I/O, no
//!   clock.
//! - `buddy-display` (a later bead) depends on THIS crate and asks [`current`] for what to
//!   composite, owning no `(species, state)` map itself.
//!
//! ## The two layers of selection, kept distinct
//! 1. A cross-animation **variety** rule picks which sprite in a state's set is active as
//!    `elapsed_ms` advances, so a multi-sprite state shows real variety and every member is
//!    reachable.
//! 2. Frame timing is **delegated** to the chosen sprite's own
//!    [`frame_at`](platform_display::sprite::Sprite::frame_at) — this crate does not
//!    reimplement the frame clock.

mod claudepix;
mod registry;
mod selection;
mod species;

pub use claudepix::CLAUDEPIX;
pub use registry::{resolve, REGISTRY};
pub use selection::{current, Selected};
pub use species::Species;

// Re-exported for callers that name the persona/index the seam speaks in without also
// depending on buddy-core directly.
pub use buddy_core::{PersonaState, SpeciesIndex};
