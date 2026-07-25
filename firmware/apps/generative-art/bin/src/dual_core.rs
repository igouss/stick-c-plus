//! The two-core frond evaluator: fill the plume's cloud on both ESP32 cores at once.
//!
//! The plume's dominant per-frame cost is evaluating ~6000 points of table trigonometry. The
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
//! ## Only the far half is buffered
//!
//! The near half never lands in a buffer: the render thread evaluates it point-by-point and plots
//! each as it goes ([`PlumeField::iter_range`]). Only the worker's far half needs a buffer, because
//! a worker on another thread cannot borrow the render thread's frame — it must own its output. So
//! the whole pipeline adds just that one half-frame buffer (~30 KiB) to the heap, not a second
//! whole cloud, which is what keeps it inside the ESP32's tight memory.
//!
//! ## Bit-identical to the serial path
//!
//! The split is [`PlumeField::compute_range`] over `[0, split)` and `[split, POINT_COUNT)`; a
//! `plume-core` property proves two such ranges reassemble the whole frame bit for bit. So this
//! evaluator produces the *identical* cloud [`SerialFrond`](art_display::SerialFrond) does — it is
//! only a faster route to it. The plot downstream cannot tell which core drew a point.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use art_display::{FieldPoint, FrondCompute};
use esp_idf_hal::cpu::Core;
use esp_idf_hal::task::thread::ThreadSpawnConfiguration;
use plume_core::{PlumeField, SinTable, POINT_COUNT};

/// The worker's stack, in bytes. It only blocks on a channel and fills a heap `Vec` through
/// [`compute_range`](PlumeField::compute_range) — a shallow loop of `f32` locals and table lookups,
/// no recursion and no `libm` (the `sqrtf` is paid at build, not per frame) — so 4 KiB is ample.
/// It is kept lean on purpose: the render thread needs a 16 KiB stack of its own, and on this heap
/// every kilobyte the worker does not take is one the render thread's stack can.
const WORKER_STACK: usize = 4 * 1024;

/// One frame's far-half job for the worker: the phase to evaluate at, and the buffer to fill.
///
/// The buffer travels *with* the job and is handed back when filled, so its ownership ping-pongs
/// between the two threads and is never shared — the property that keeps the whole pipeline safe.
struct FarJob {
    /// The animation phase to evaluate the far half at.
    t: f32,
    /// The far-half buffer to fill, sized `POINT_COUNT - split`. Returned on the result channel.
    far: Vec<FieldPoint>,
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
    from_worker: Receiver<Vec<FieldPoint>>,
    /// The far buffer, parked here between frames: taken when a frame starts, returned when the
    /// worker hands it back. `Option` so it can be moved out and back without cloning.
    far: Option<Vec<FieldPoint>>,
    /// The worker task, kept alive for the life of the frond — dropping it would close the job
    /// channel and end the worker's loop.
    _worker: JoinHandle<()>,
}

impl DualCoreFrond {
    /// Build the field and table, spawn the Core1 worker, and park the far buffer for frame one.
    pub fn new() -> Self {
        let split: usize = POINT_COUNT as usize / 2;
        let far_len: usize = POINT_COUNT as usize - split;

        // Park the far buffer *before* the 100 KiB field. On the ESP32's pool-fragmented heap a
        // 30 KiB contiguous run is easy to find while the pools are fresh, but scarce once the field
        // has been carved out — so the awkward middle-sized buffer is claimed first, and the field
        // (which needs a whole pool of its own) is allocated after.
        let far: Vec<FieldPoint> = vec![FieldPoint::default(); far_len];

        let table: Arc<SinTable> = Arc::new(SinTable::new());
        let field: Arc<PlumeField> = Arc::new(PlumeField::new(&table));

        let (to_worker, jobs): (Sender<FarJob>, Receiver<FarJob>) = channel();
        let (results, from_worker): (Sender<Vec<FieldPoint>>, Receiver<Vec<FieldPoint>>) =
            channel();

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

/// The worker's forever loop: block for a job, fill its far half on this core, hand the buffer
/// back. Ends only when the job channel closes — which happens only if the frond is dropped, and
/// it lives for the whole app.
fn worker_loop(
    split: usize,
    field: Arc<PlumeField>,
    table: Arc<SinTable>,
    jobs: Receiver<FarJob>,
    results: Sender<Vec<FieldPoint>>,
) {
    while let Ok(FarJob { t, mut far }) = jobs.recv() {
        field.compute_range(split, t, &table, &mut far);
        if results.send(far).is_err() {
            break; // the frond was dropped; the far buffer has nowhere to go.
        }
    }
}

impl FrondCompute for DualCoreFrond {
    /// Evaluate the whole cloud through `plot` using both cores: dispatch the far half to the
    /// worker, stream the near half here meanwhile, then join and plot the worker's far half.
    fn evaluate(&mut self, t: f32, plot: &mut dyn FnMut(FieldPoint)) {
        // Hand this frame's far half to the worker; it starts as soon as Core1 picks it up.
        let far: Vec<FieldPoint> = self.far.take().expect("far buffer parked between frames");
        self.to_worker
            .send(FarJob { t, far })
            .expect("the plume worker is alive");

        // Stream the near half on this core — straight into the plot, no buffer — while the worker
        // fills the far half on Core1.
        self.field
            .iter_range(0, self.split, t, &self.table)
            .for_each(|point: FieldPoint| plot(point));

        // The barrier: block until the worker returns the far half, plot it, and re-park it for the
        // next frame.
        let far: Vec<FieldPoint> = self
            .from_worker
            .recv()
            .expect("the plume worker returned its half");
        far.iter().for_each(|&point: &FieldPoint| plot(point));
        self.far = Some(far);
    }
}
