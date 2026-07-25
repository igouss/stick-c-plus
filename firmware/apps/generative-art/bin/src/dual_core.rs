//! The two-core frond evaluator: fill the plume's cloud on both ESP32 cores at once.
//!
//! The plume's dominant per-frame cost is evaluating ~7500 points of table trigonometry. The
//! render loop runs on one core; the other (APP/Core1) would otherwise idle. This adapter is the
//! firmware's implementation of [`FrondCompute`](art_display::FrondCompute): it splits the cloud in
//! two and evaluates the halves on the two cores concurrently, so a frame's evaluation costs about
//! half the wall-clock a single core spends.
//!
//! ## Why a persistent worker, and why a ping-pong
//!
//! Spawning a thread per frame would pay a FreeRTOS task create/delete every ~25 ms — dwarfing the
//! work. So the worker is spawned **once**, pinned to [`Core::Core1`], and lives for the app,
//! blocking on a channel between frames. Each frame the main thread hands it the *far half* buffer
//! and the phase, then streams the *near half* straight into the plot on its own core meanwhile,
//! and finally blocks reclaiming the far half — the barrier — and plots it. The far buffer is
//! **ping-ponged** by value through the channels, so at every instant exactly one thread owns it:
//! no shared mutable state, no lock, no `unsafe`. The two cores only ever read the shared field and
//! table (both immutable [`Arc`]s).
//!
//! ## Only the far half is buffered, and only as projected pixels
//!
//! The near half never lands in a buffer: the render thread evaluates it point-by-point, projects
//! each, and plots it as it goes ([`PlumeField::iter_range`] + [`project`]). Only the worker's far
//! half needs a buffer, because a worker on another thread cannot borrow the render thread's frame —
//! it must own its output. And what it owns is already *projected*: an `Option<ScreenPoint>` a point,
//! six bytes each, not the twelve a canvas [`FieldPoint`] costs. So the whole pipeline adds only that
//! one half-buffer, at half the width it once was — a saving that goes straight into the frond's
//! point budget — and the far-half projections run on Core1 rather than the render core.
//!
//! ## Identical to the serial path
//!
//! Each half is [`PlumeField::iter_range`] followed by [`project`], the same two steps
//! [`SerialFrond`](art_display::SerialFrond) runs in one sweep; a `plume-core` property proves the
//! two index ranges reassemble the whole field bit for bit, and both halves project through the same
//! function, so the projected cloud is identical to the one-core path's. The plot downstream cannot
//! tell which core drew a pixel.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use art_display::{project, FieldPoint, FrondCompute, ScreenPoint, Size};
use esp_idf_hal::cpu::Core;
use esp_idf_hal::task::thread::ThreadSpawnConfiguration;
use plume_core::{PlumeField, SinTable, POINT_COUNT};

/// The worker's stack, in bytes. It only blocks on a channel and fills a heap `Vec` through
/// [`project_far`] — a shallow loop of `f32` locals, table lookups and a projection, no recursion
/// and no `libm` (the `sqrtf` is paid at build, not per frame) — so 4 KiB is ample. It is kept lean
/// on purpose: the render thread needs its own stack, and on this heap every kilobyte the worker
/// does not take is one the render thread's stack can.
const WORKER_STACK: usize = 4 * 1024;

/// One frame's far-half job for the worker: the phase to evaluate at, the canvas to project onto,
/// and the buffer to fill.
///
/// The buffer travels *with* the job and is handed back when filled, so its ownership ping-pongs
/// between the two threads and is never shared — the property that keeps the whole pipeline safe.
struct FarJob {
    /// The animation phase to evaluate the far half at.
    t: f32,
    /// The panel the far half is projected onto — constant across frames, but carried in the job so
    /// the worker projects with exactly the canvas the render thread was handed this frame.
    canvas: Size,
    /// The far-half buffer to fill, sized `POINT_COUNT - split`, one slot per point: `Some` for a
    /// drawable pixel, `None` where the field's degenerate index projected to nothing. Returned on
    /// the result channel. Holding already-projected [`ScreenPoint`]s (six bytes each) rather than
    /// canvas [`FieldPoint`]s (twelve) halves this buffer, and doing the projection here spends it on
    /// Core1 instead of the render core.
    far: Vec<Option<ScreenPoint>>,
}

/// The firmware's [`FrondCompute`]: a render-thread half and a pinned Core1 worker, joined per
/// frame through a pair of channels.
pub struct DualCoreFrond {
    /// The near/far boundary. The render thread fills `[0, split)`; the worker fills the rest.
    split: usize,
    /// The precomputed field, shared read-only with the worker (both cores only read it).
    field: Arc<PlumeField>,
    /// The sine table, shared read-only with the worker.
    table: Arc<SinTable>,
    /// Hand a [`FarJob`] to the worker for this frame.
    to_worker: Sender<FarJob>,
    /// Reclaim the filled far half — receiving on this is the per-frame barrier.
    from_worker: Receiver<Vec<Option<ScreenPoint>>>,
    /// The far buffer, parked here between frames: taken when a frame starts, returned when the
    /// worker hands it back. `Option` so it can be moved out and back without cloning.
    far: Option<Vec<Option<ScreenPoint>>>,
    /// The worker task, kept alive for the life of the frond — dropping it would close the job
    /// channel and end the worker's loop.
    _worker: JoinHandle<()>,
}

impl DualCoreFrond {
    /// Build the field and table, spawn the Core1 worker, and park the far buffer for frame one.
    pub fn new() -> Self {
        let split: usize = POINT_COUNT as usize / 2;
        let far_len: usize = POINT_COUNT as usize - split;

        // Park the far buffer *before* the field's two halves. On the ESP32's pool-fragmented heap a
        // mid-sized contiguous run is easy to find while the pools are fresh, but scarce once the
        // field has been carved out — so the awkward buffer is claimed first, and the field is
        // allocated after. One `Option<ScreenPoint>` per far-half point, all `None` until a frame
        // fills it.
        let far: Vec<Option<ScreenPoint>> = vec![None; far_len];

        let table: Arc<SinTable> = Arc::new(SinTable::new());
        let field: Arc<PlumeField> = Arc::new(PlumeField::new(&table));

        let (to_worker, jobs): (Sender<FarJob>, Receiver<FarJob>) = channel();
        let (results, from_worker): (
            Sender<Vec<Option<ScreenPoint>>>,
            Receiver<Vec<Option<ScreenPoint>>>,
        ) = channel();

        // Pin the worker to Core1 so it runs on the core the render loop does not. `set` configures
        // the *next* `std::thread` spawn on this thread; the config is restored to default right
        // after so nothing else inherits the affinity or the name.
        ThreadSpawnConfiguration {
            name: Some(c"plume-worker"),
            stack_size: WORKER_STACK,
            pin_to_core: Some(Core::Core1),
            ..Default::default()
        }
        .set()
        .expect("pin the plume worker to Core1");

        let _worker: JoinHandle<()> = {
            let field: Arc<PlumeField> = Arc::clone(&field);
            let table: Arc<SinTable> = Arc::clone(&table);
            thread::spawn(move || worker_loop(split, field, table, jobs, results))
        };

        ThreadSpawnConfiguration::default()
            .set()
            .expect("restore the default spawn config");

        Self {
            split,
            field,
            table,
            to_worker,
            from_worker,
            far: Some(far),
            _worker,
        }
    }
}

impl Default for DualCoreFrond {
    fn default() -> Self {
        Self::new()
    }
}

/// The worker's forever loop: block for a job, evaluate-and-project its far half on this core, hand
/// the buffer back. Ends only when the job channel closes — which happens only if the frond is
/// dropped, and it lives for the whole app.
fn worker_loop(
    split: usize,
    field: Arc<PlumeField>,
    table: Arc<SinTable>,
    jobs: Receiver<FarJob>,
    results: Sender<Vec<Option<ScreenPoint>>>,
) {
    while let Ok(FarJob { t, canvas, mut far }) = jobs.recv() {
        project_far(&field, split, t, &table, canvas, &mut far);
        if results.send(far).is_err() {
            break; // the frond was dropped; the far buffer has nowhere to go.
        }
    }
}

/// Evaluate the far half `[split, split + out.len())` at phase `t` and project each point onto
/// `canvas`, filling `out` — `Some` for a drawable pixel, `None` where the field's degenerate index
/// projected to nothing. This is the work Core1 does hidden under the render thread's near half; it
/// is exactly `iter_range` followed by [`project`], the same two steps the serial path runs, so the
/// far pixels are identical to a one-core sweep's.
fn project_far(
    field: &PlumeField,
    split: usize,
    t: f32,
    table: &SinTable,
    canvas: Size,
    out: &mut [Option<ScreenPoint>],
) {
    let len: usize = out.len();
    out.iter_mut()
        .zip(field.iter_range(split, len, t, table))
        .for_each(|(slot, point): (&mut Option<ScreenPoint>, FieldPoint)| {
            *slot = project(point, canvas);
        });
}

impl FrondCompute for DualCoreFrond {
    /// Evaluate and project the whole cloud through `plot` using both cores: dispatch the far half to
    /// the worker, stream the near half here meanwhile, then join and plot the worker's far half.
    fn evaluate(&mut self, t: f32, canvas: Size, plot: &mut dyn FnMut(ScreenPoint)) {
        // Hand this frame's far half to the worker; it starts as soon as Core1 picks it up. It
        // evaluates *and projects* its half, so the ~3000 far projections land on Core1, not here.
        let far: Vec<Option<ScreenPoint>> =
            self.far.take().expect("far buffer parked between frames");
        self.to_worker
            .send(FarJob { t, canvas, far })
            .expect("the plume worker is alive");

        // Stream the near half on this core — evaluate, project, and plot straight through, no buffer
        // — while the worker fills the far half on Core1.
        self.field
            .iter_range(0, self.split, t, &self.table)
            .for_each(|point: FieldPoint| {
                if let Some(screen) = project(point, canvas) {
                    plot(screen);
                }
            });

        // The barrier: block until the worker returns the far half, plot its drawable pixels, and
        // re-park it for the next frame.
        let far: Vec<Option<ScreenPoint>> = self
            .from_worker
            .recv()
            .expect("the plume worker returned its half");
        far.iter().for_each(|slot: &Option<ScreenPoint>| {
            if let Some(screen) = *slot {
                plot(screen);
            }
        });
        self.far = Some(far);
    }
}
