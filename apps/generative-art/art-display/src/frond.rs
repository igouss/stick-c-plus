//! The compute port for the plume's frond: turn an animation phase into projected panel pixels.
//!
//! The plume is two halves — *evaluate-and-project* the parametric field into a cloud of drawable
//! [`ScreenPoint`]s, then *plot* that cloud onto the panel. The plot is a bare pixel set
//! ([`plume::plot_screen`](crate::sketch::plume::plot_screen)); the evaluation-and-projection is the
//! frame's dominant cost, ~6000 points of table trigonometry and a scale-and-offset each. This port
//! is the seam under that work, so *how* the cloud is computed **and projected** is a decision the
//! composition root makes, not the renderer:
//!
//! - the host, the screenshots and the goldens compute it on one core with [`SerialFrond`], the
//!   pure default the [`Gallery`](crate::Gallery) is born with;
//! - the firmware injects a two-core implementation that fills two ranges of the same cloud on the
//!   ESP32's two cores at once — a strictly faster way to the *identical* points, proven bit for
//!   bit against the whole sweep in [`plume_core`].
//!
//! Keeping the split behind a port is what lets the domain stay single-threaded and framework-free:
//! `plume-core` and this crate know nothing of cores or threads, and the whole gallery is still
//! verified on the host. The port speaks in phase `t` and a plot callback, never in clocks or
//! threads, so an implementation is free to parallelise however the board allows.
//!
//! ## Why a plot callback, not a returned buffer
//!
//! The port hands each point to a caller-supplied `plot` closure rather than filling a shared
//! buffer, so the single-core path streams the whole cloud through the plot with **no intermediate
//! allocation at all**, and the two-core path buffers only the *far* half its worker must own —
//! never the whole frame twice. And because what it buffers is a projected [`ScreenPoint`] (six
//! bytes) rather than a canvas [`FieldPoint`] (twelve), that half costs half the heap it used to. On
//! a 520 KiB no-PSRAM ESP32, whose heap already holds a ~100 KiB field and two full-screen frame
//! buffers, that saving is part of what lets the frond grow. The callback is what keeps the pipeline
//! inside the heap.
//!
//! ## Why the port projects, not just evaluates
//!
//! Projection — the scale-and-offset from the field's 400×400 canvas onto the panel — sits *inside*
//! this port, not after it, so the two-core implementation can do it on its worker core. The render
//! core then only sets pixels; the ~3000 far-half projections move off its critical path onto the
//! core that would otherwise idle. That is why `evaluate` takes the canvas size and hands back
//! [`ScreenPoint`]s ready to draw rather than field points still to be projected.

use embedded_graphics::prelude::Size;
use plume_core::{FieldPoint, PlumeField, SinTable};

use crate::sketch::plume::{project, ScreenPoint};

/// Evaluate the frond at animation phase `t`, project it onto a `canvas`-shaped panel, and hand each
/// drawable pixel to `plot`.
///
/// One method, called once a frame: produce the frond's points at phase `t`, project each onto the
/// panel, and pass the drawable ones to `plot` as [`ScreenPoint`]s in the field's index order —
/// dropping the field's degenerate infinities, which are not pixels. An implementation may compute
/// and project the points however it likes — one core or many — but must present the same cloud the
/// field defines; the parallel firmware implementation is held to that bit for bit. Streaming
/// through the callback rather than a returned buffer is what lets a two-core implementation add
/// only its worker's half to the heap, not a whole second frame (see the module docs).
///
/// `Send` so the whole [`Gallery`](crate::Gallery) that holds one can move into the display thread.
/// `&mut self` because a parallel implementation carries a far-half buffer it hands to its worker
/// and reclaims each frame — state the frame mutates, not shared config.
pub trait FrondCompute: Send {
    /// Evaluate and project the frond at phase `t` onto `canvas`, passing each drawable
    /// [`ScreenPoint`] to `plot` in index order.
    fn evaluate(&mut self, t: f32, canvas: Size, plot: &mut dyn FnMut(ScreenPoint));
}

/// The single-core default: stream the whole cloud on the calling thread.
///
/// Owns the sine table and the precomputed field — the two pieces of capital a frond evaluation
/// needs — and sweeps the whole range on one core, projecting each point straight into the plot with
/// no buffer. This is the pure, host-testable path every renderer that is not the firmware uses; the
/// firmware swaps in a two-core implementation over the same [`PlumeField::iter_range`], which is why
/// the field's sweep is addressable by range at all.
pub struct SerialFrond {
    /// The startup-built trigonometry the field is evaluated through.
    table: SinTable,
    /// The frond's per-index invariants, precomputed once so a frame pays only its phase lookups.
    field: PlumeField,
}

impl SerialFrond {
    /// Build the frond's capital once: the sine table, then the field precomputed from it.
    pub fn new() -> Self {
        let table: SinTable = SinTable::new();
        let field: PlumeField = PlumeField::new(&table);
        Self { table, field }
    }
}

impl Default for SerialFrond {
    fn default() -> Self {
        Self::new()
    }
}

impl FrondCompute for SerialFrond {
    /// Stream the whole frond on this core: [`frame`](PlumeField::frame), project each point onto
    /// `canvas`, and hand the drawable ones straight into `plot` — no buffer.
    fn evaluate(&mut self, t: f32, canvas: Size, plot: &mut dyn FnMut(ScreenPoint)) {
        self.field
            .frame(t, &self.table)
            .for_each(|point: FieldPoint| {
                if let Some(screen) = project(point, canvas) {
                    plot(screen);
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// The 135×240 portrait canvas the frond is projected onto in these tests.
    const PORTRAIT: Size = Size::new(135, 240);

    /// Drain the frond at phase `t` into a `Vec` through the plot callback — every drawable
    /// [`ScreenPoint`], in the order the port presents it.
    fn evaluated(frond: &mut dyn FrondCompute, t: f32) -> Vec<ScreenPoint> {
        let mut cloud: Vec<ScreenPoint> = Vec::new();
        frond.evaluate(t, PORTRAIT, &mut |point: ScreenPoint| cloud.push(point));
        cloud
    }

    /// One: the serial frond presents exactly the field's cloud, projected — every drawable point,
    /// identical to projecting [`PlumeField::frame`], the reference sweep, one by one. The port is
    /// only ever a faster route to these exact pixels, so the default route must land on them
    /// exactly. (The degenerate infinities the field throws are dropped by projection, so the
    /// reference is filtered the same way.)
    #[test]
    fn the_serial_frond_is_the_reference_cloud() {
        let table: SinTable = SinTable::new();
        let field: PlumeField = PlumeField::new(&table);

        let mut frond: SerialFrond = SerialFrond::new();
        let t: f32 = 1.0;
        let cloud: Vec<ScreenPoint> = evaluated(&mut frond, t);

        let reference: Vec<ScreenPoint> = field
            .frame(t, &table)
            .filter_map(|point: FieldPoint| project(point, PORTRAIT))
            .collect();
        assert_eq!(
            cloud, reference,
            "the serial frond is the projected reference"
        );
    }

    /// The frond breathes: two phases present two different clouds, so a port that ignored `t`
    /// could never pass. (Compared as whole clouds — any moved point trips it.)
    #[test]
    fn a_different_phase_is_a_different_cloud() {
        let mut frond: SerialFrond = SerialFrond::new();
        assert_ne!(evaluated(&mut frond, 0.0), evaluated(&mut frond, 1.0));
    }
}
