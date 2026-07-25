//! The compute port for the plume's frond: turn an animation phase into the frond's point cloud.
//!
//! The plume is two halves — *evaluate* the parametric field into a cloud of points, then *plot*
//! that cloud onto the panel. The plot is cheap and pure ([`plume::plot`](crate::sketch::plume));
//! the evaluation is the frame's dominant cost, ~5000 points of table trigonometry. This port is
//! the seam under the evaluation, so *how* the cloud is computed is a decision the composition root
//! makes, not the renderer:
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
//! never the whole frame twice. On a 520 KiB no-PSRAM ESP32, whose heap already holds a 100 KiB
//! field and two full-screen frame buffers, a second whole-cloud buffer is the difference between
//! booting and an allocator abort. The callback is what keeps the pipeline inside the heap.

use plume_core::{FieldPoint, PlumeField, SinTable};

/// Evaluate the frond's point cloud at animation phase `t`, handing each point to `plot`.
///
/// One method, called once a frame: produce the frond's [`FieldPoint`]s at phase `t` and pass each
/// to `plot`, in the field's index order. An implementation may compute the points however it likes
/// — one core or many — but must present the same cloud the field defines; the parallel firmware
/// implementation is held to that bit for bit. Streaming through the callback rather than a returned
/// buffer is what lets a two-core implementation add only its worker's half to the heap, not a whole
/// second frame (see the module docs).
///
/// `Send` so the whole [`Gallery`](crate::Gallery) that holds one can move into the display thread.
/// `&mut self` because a parallel implementation carries a far-half buffer it hands to its worker
/// and reclaims each frame — state the frame mutates, not shared config.
pub trait FrondCompute: Send {
    /// Evaluate the frond at phase `t`, passing each point to `plot` in index order.
    fn evaluate(&mut self, t: f32, plot: &mut dyn FnMut(FieldPoint));
}

/// The single-core default: stream the whole cloud on the calling thread.
///
/// Owns the sine table and the precomputed field — the two pieces of capital a frond evaluation
/// needs — and sweeps the whole range on one core, straight into the plot with no buffer. This is
/// the pure, host-testable path every renderer that is not the firmware uses; the firmware swaps in
/// a two-core implementation over the same [`PlumeField::iter_range`]/[`compute_range`], which is
/// why the field's sweep is addressable by range at all.
///
/// [`PlumeField::compute_range`]: plume_core::PlumeField::compute_range
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
    /// Stream the whole frond on this core: [`frame`](PlumeField::frame) straight into `plot`, no
    /// buffer.
    fn evaluate(&mut self, t: f32, plot: &mut dyn FnMut(FieldPoint)) {
        self.field
            .frame(t, &self.table)
            .for_each(|point: FieldPoint| plot(point));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use plume_core::POINT_COUNT;

    /// Drain the frond at phase `t` into a `Vec` through the plot callback — the whole cloud, in
    /// the order the port presents it.
    fn evaluated(frond: &mut dyn FrondCompute, t: f32) -> Vec<FieldPoint> {
        let mut cloud: Vec<FieldPoint> = Vec::new();
        frond.evaluate(t, &mut |point: FieldPoint| cloud.push(point));
        cloud
    }

    /// One: the serial frond presents the whole field's cloud — every point, bit-identical to
    /// [`PlumeField::frame`], the reference sweep. The port is only ever a faster route to these
    /// exact points, so the default route must land on them exactly.
    #[test]
    fn the_serial_frond_is_the_reference_cloud() {
        let table: SinTable = SinTable::new();
        let field: PlumeField = PlumeField::new(&table);

        let mut frond: SerialFrond = SerialFrond::new();
        let t: f32 = 1.0;
        let cloud: Vec<FieldPoint> = evaluated(&mut frond, t);

        assert_eq!(
            cloud.len(),
            POINT_COUNT as usize,
            "the whole frond is presented"
        );
        for (reference, computed) in field.frame(t, &table).zip(cloud.iter()) {
            assert_eq!(reference.x.to_bits(), computed.x.to_bits());
            assert_eq!(reference.y.to_bits(), computed.y.to_bits());
            assert_eq!(reference.wide, computed.wide);
        }
    }

    /// The frond breathes: two phases present two different clouds, so a port that ignored `t`
    /// could never pass. (Compared as whole clouds — any moved point trips it.)
    #[test]
    fn a_different_phase_is_a_different_cloud() {
        let mut frond: SerialFrond = SerialFrond::new();
        assert_ne!(evaluated(&mut frond, 0.0), evaluated(&mut frond, 1.0));
    }
}
