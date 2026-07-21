#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # platform-core
//!
//! The board-generic pure kernel, shared by every app on the M5StickC Plus. It names no
//! framework and performs no I/O: just the value types and the driven-side ports the apps
//! speak in, so it is verified entirely on the host and cross-compiles to Xtensa unchanged.
//!
//! ## Ports (the driven side of the hexagon)
//! - [`Clock`] — a monotonic millisecond time base. The firmware's `Monotonic` implements
//!   it; the domain and shells take `now: `[`Tick`] so time is injected, never read.
//! - [`Screen`] — a driven display that renders a view of some app state `S`. The ST7789
//!   panel adapter implements it; the app supplies the picture.
//! - [`Button`] — a momentary button's raw pressed level, debounced by the pure
//!   [`Debounce`] into [`ButtonEvent`]s (a tap, or a long hold); an app that wants a
//!   double-tap routes that stream through the pure [`TapCadence`].
//! - [`Tone`] — a buzzer that plays a sequence of [`Note`]s. The app owns the melody; the
//!   adapter only sounds it.
//! - [`AudioIn`] — a microphone yielding mono PCM samples. The adapter only transfers samples;
//!   the pure `platform-audio` crate decides what they mean (e.g. whether a tone is present).
//! - [`PowerSource`] — whether the board is on USB or battery, debounced by the pure
//!   [`PowerDebounce`] into a settled level; [`edge`] then decides the [`PowerChime`] (if
//!   any) a transition plays.
//! - [`Imu`] — a three-axis accelerometer reporting an [`Acceleration`] in milli-g. The
//!   adapter only reads the registers; what a vector *means* — which way is up, how steeply
//!   the board is tilted — is decided inward by an app's own domain. [`is_at_rest`] is the
//!   one reading of a vector that is *not* an app's business: whether it is gravity alone,
//!   which is the precondition of every other question.
//!
//! ## Which way up the glass is
//! - [`ScreenRotation`] — the quarter turn the picture is drawn at, with [`rotation_for`]
//!   deciding it from a gravity vector and [`RotationSettler`] holding it steady so the
//!   picture does not spin while the board is being turned. This lives in the kernel rather
//!   than in an app because every app draws on the same glass, which turns the same way for
//!   all of them; naming the *face* the board rests on stays an app's own business.
//!
//! ## The animation contract
//! - [`Animated`] — an app state that knows its own animation policy: a coarse
//!   [`anchor`](Animated::anchor) that resets the creature's clock only on a real
//!   transition, whether it [`is_animated`](Animated::is_animated), and which
//!   [`frame_index`](Animated::frame_index) shows after a given elapsed time. The generic
//!   render loop (in `platform-runtime`) reads exactly this.

mod animated;
mod audio;
mod chime;
mod clock;
mod imu;
mod input;
mod power;
mod power_debounce;
mod rotation;
mod screen;
mod sound;
mod tick;

pub use animated::Animated;
pub use audio::AudioIn;
pub use chime::{edge, PowerChime};
pub use clock::Clock;
pub use imu::{is_at_rest, Acceleration, Imu, ONE_G_MG, REST_TOLERANCE_MG};
pub use input::{Button, ButtonEvent, Debounce, TapCadence, DEBOUNCE_MS, DOUBLE_TAP_MS, HOLD_MS};
pub use power::PowerSource;
pub use power_debounce::{PowerDebounce, POWER_DEBOUNCE_MS};
pub use rotation::{
    rotation_for, RotationSettler, ScreenRotation, IN_PLANE_MINIMUM_MG, QUADRANT_MARGIN_MG,
    ROTATION_SETTLE_MS,
};
pub use screen::Screen;
pub use sound::{Note, Tone};
pub use tick::Tick;
