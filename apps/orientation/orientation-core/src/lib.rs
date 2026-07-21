#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # orientation-core
//!
//! What the board's own accelerometer says about which way it is pointing — and nothing about
//! how that accelerometer is wired, addressed, or read.
//!
//! An accelerometer at rest measures one thing: the pull of the earth. That single vector is
//! enough to say how steeply the board is tilted and which face it is lying on, and this
//! crate is the pure transform from the one to the other. It names no framework and performs
//! no I/O, so it is verified entirely on the host and cross-compiles to Xtensa unchanged.
//!
//! ## Hexagon
//! - **Entities**: [`Attitude`] (the tilt, in whole degrees) and [`Facing`] (the resting
//!   pose, in words) — the two value objects the glass shows, with [`Orientation`] the
//!   aggregate that carries them beside the vector they were read from.
//! - **Control**: [`Orientation::of`] — the use case, a total pure function of one
//!   [`Acceleration`](platform_core::Acceleration).
//! - **Entities / policy**: [`Smoother`] — the exponential moving average that turns a noisy
//!   sample stream into a readout that can be read, tuned for responsiveness over stillness.
//! - **Entities / policy**: [`Signal`] — whether a reading is fresh enough to still be true,
//!   and [`Reading`], the pose paired with that verdict. Liveness is decided from an age in
//!   milliseconds handed in, never from a clock read here.
//!
//! The [`Imu`](platform_core::Imu) port itself lives in the shared kernel beside every other
//! driven port; this crate only decides what its readings *mean*.
//!
//! Which quarter turn the *picture* is drawn at is not here either — see
//! [`ScreenRotation`](platform_core::ScreenRotation). Every app draws on the same glass and
//! it turns the same way for all of them, so that question belongs to the board rather than
//! to this app. Naming the face the board rests on is what stays: a board lying flat has a
//! perfectly good [`Facing`] and no readable rotation at all.
//!
//! ## What an accelerometer cannot tell you
//!
//! Two of the three axes of rotation, and no more. Pitch and roll are observable because
//! gravity is a fixed reference to measure them against; **yaw — the compass heading — is
//! not**, because spinning the board about the vertical moves the gravity vector not at all.
//! The M5StickC Plus has no magnetometer, so nothing here can recover it. The readout
//! therefore reports the two angles it can actually stand behind, rather than a third that
//! would drift quietly away from the truth and never say so.

mod attitude;
mod facing;
mod orientation;
mod signal;
mod smooth;

pub use attitude::{attitude_of, Attitude, RIGHT_ANGLE_DEG, STRAIGHT_ANGLE_DEG};
pub use facing::{facing_of, Facing, FACE_THRESHOLD_MG};
pub use orientation::Orientation;
pub use signal::{Reading, Signal, SIGNAL_TIMEOUT_MS};
pub use smooth::{Smoother, RESPONSIVE_WEIGHT};
