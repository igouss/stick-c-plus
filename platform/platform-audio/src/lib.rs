#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # platform-audio
//!
//! Pure, framework-free acoustic analysis for the M5StickC Plus — the half of the chime
//! self-test that has no hardware in it.
//!
//! The board can check its own buzzer by ear-in-silicon: record the silent noise floor, then
//! play a note while capturing the mic through the [`AudioIn`](platform_core::AudioIn) port, and
//! ask *did the sound get louder?* This crate answers that as a pure function of the samples, so
//! the decision is host-tested against synthetic signals and the firmware only supplies the
//! capture.
//!
//! - [`ac_rms`] — the acoustic level of a PCM block: its DC-removed RMS amplitude, near zero for
//!   silence and rising with loudness.
//! - [`present`] — the verdict: a measured level clears a threshold.
//!
//! The level, not the pitch, is the measurement that fits the hardware: the M5StickC Plus buzzer
//! is a tiny resonant transducer that does not radiate a clean tone at the frequency it is
//! driven, so "is the chime audible?" is the answerable question, and it is a question of loudness
//! above the floor. Both functions are pure and `no_std`; the only dependency is `libm`, for the
//! one `sqrt` the RMS needs.

mod level;

pub use level::{ac_rms, present};
