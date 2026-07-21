#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # platform-bench
//!
//! The pure arithmetic a bench instrument reads its own measurements with. The *measuring*
//! happens on the metal and cannot be host-tested; the *reading* is ordinary arithmetic over a
//! set of timings, and belongs where every rule in this tree belongs — inward, host-tested,
//! naming no hardware.
//!
//! ## Why a distribution and not a threshold
//!
//! The board already has a threshold instrument: the render loop times a paint and warns past
//! a budget. It is the right tool for raising an alarm and useless for answering *why*, because
//! it only ever fires on the failures. It cannot show that 99.2% of paints take a third of the
//! budget — and that reframing, from "the paint is slow" to "0.8% of paints are blocked", was
//! the whole breakthrough of the 2026-07-21 rotation study.
//!
//! So [`Summary`] reports min, median and max rather than a pass/fail, and
//! [`over_budget`](Summary::over_budget) counts the breaches separately instead of hiding the
//! rest.
//!
//! ## Why the split is the interesting part
//!
//! [`Split`] is the discriminator, and the reason this crate exists rather than a `min`/`max`
//! helper inlined into a bench bin. Mark each sample with whether the *suspect* was active
//! while it was taken, and one measurement answers three questions at once:
//!
//! - **only the `during` samples are slow** → the suspect blocks, and by how much;
//! - **both halves are slow** → something else blocks, and the suspect is a bystander;
//! - **neither half is slow** → the run did not reproduce the problem at all, and nothing
//!   about the suspect has been shown either way.
//!
//! That third reading matters as much as the first two. An instrument that cannot come back
//! saying "I failed to reproduce it" will instead come back agreeing with whoever built it.
//!
//! Nothing here allocates and nothing here is `std`: a bench tool fills a fixed array on the
//! measured path and reads it afterwards, because a `warn!` at 115200 baud is milliseconds of
//! blocking UART and would become the thing being measured.

mod sample;
mod split;
mod summary;

pub use sample::Sample;
pub use split::{Evidence, Split};
pub use summary::Summary;
