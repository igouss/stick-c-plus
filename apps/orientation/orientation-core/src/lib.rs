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
//!
//! The [`Imu`](platform_core::Imu) port itself lives in the shared kernel beside every other
//! driven port; this crate only decides what its readings *mean*.
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
mod smooth;

pub use attitude::{attitude_of, Attitude, RIGHT_ANGLE_DEG, STRAIGHT_ANGLE_DEG};
pub use facing::{facing_of, Facing, FACE_THRESHOLD_MG, REST_TOLERANCE_MG};
pub use orientation::Orientation;
pub use smooth::{Smoother, RESPONSIVE_WEIGHT};
