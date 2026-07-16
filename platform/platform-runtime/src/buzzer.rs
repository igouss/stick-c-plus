//! The buzzer's one owner — every melody, chime or jingle, serialised through a single
//! thread so two never interleave or truncate one another.
//!
//! The board has exactly one buzzer. `spawn_buzzer` takes the real
//! [`Tone`](platform_core::Tone) adapter, owns it on one thread, and hands back a
//! [`Clone`] + [`Send`] [`BuzzerHandle`] that *is itself* a `Tone`: every caller — the input
//! thread's jingles, the power-watch thread's chimes — submits whole melodies through its
//! own clone, and the owner plays each one to completion before it looks at the next. A
//! caller's `play` blocks until the owner has attempted it, matching the synchronous jingle a
//! caller already expects from a direct `Tone`.

use std::fmt;
use std::io;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread::{self, JoinHandle};

use log::warn;
use platform_core::{Note, Tone};

/// The buzzer-owner thread's stack, in bytes (it becomes a FreeRTOS task stack on device).
/// The owner drives the real [`Tone`] adapter's PWM and, on a failed play, formats an error
/// through `warn!` — `core::fmt`'s formatting call tree is deep and stack-hungry, and under
/// FreeRTOS preemption an interrupt frame can land on top of it. 4 KiB leaves no margin for
/// that worst case; 8 KiB does, the same headroom the other hardware-touching platform
/// threads are sized to.
pub const BUZZER_STACK_SIZE: usize = 8 * 1024;

/// The buzzer's one owner thread is gone — it panicked, or every [`BuzzerHandle`] was
/// dropped without a play in flight. The only way [`BuzzerHandle::play`] can fail; a real
/// playback failure is logged by the owner instead (fail-visible, matching `spawn_display`'s
/// render errors), not surfaced here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnerGone;

impl fmt::Display for OwnerGone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("buzzer owner thread is gone")
    }
}

/// One submitted melody: the notes to play, and where to signal once the owner has attempted
/// them.
struct Request {
    notes: Vec<Note>,
    done: SyncSender<()>,
}

/// A `Clone` + `Send` handle to the buzzer's one owner thread. Implements
/// [`Tone`](platform_core::Tone) itself: `play` submits the notes and blocks until the owner
/// has played them, so two handles' calls can race to submit but never race to sound.
#[derive(Clone)]
pub struct BuzzerHandle {
    tx: Sender<Request>,
}

impl Tone for BuzzerHandle {
    type Error = OwnerGone;

    fn play(&mut self, notes: &[Note]) -> Result<(), OwnerGone> {
        // A one-slot rendezvous the owner signals once it has attempted this melody, so `play`
        // blocks until then — the synchronous playback a caller expects from a direct `Tone`.
        let (done, wait): (SyncSender<()>, Receiver<()>) = mpsc::sync_channel(1);
        let request: Request = Request {
            notes: notes.to_vec(),
            done,
        };
        // Either failure means the one owner thread is gone: the send finds a closed channel,
        // or it dropped our request without signalling. Both surface as `OwnerGone`.
        self.tx
            .send(request)
            .map_err(|_gone: mpsc::SendError<Request>| OwnerGone)?;
        wait.recv().map_err(|_gone: mpsc::RecvError| OwnerGone)
    }
}

/// A running buzzer-owner thread — a handle to join it.
///
/// There is no `stop`: the owner exits on its own once every [`BuzzerHandle`] has been
/// dropped and its request channel closes — the same shutdown a plain
/// [`mpsc::Sender`](std::sync::mpsc::Sender) gives for free, so no extra stop flag is needed.
pub struct BuzzerTask {
    handle: JoinHandle<()>,
}

impl BuzzerTask {
    /// Block until the buzzer-owner thread has exited, propagating a panic it carried.
    pub fn join(self) -> thread::Result<()> {
        self.handle.join()
    }
}

/// Spawn the buzzer's one owner thread around the real `tone`, returning the [`BuzzerTask`]
/// handle and the first [`BuzzerHandle`] — clone it for every caller that plays melodies.
///
/// `tone` moves into the thread, so it must be [`Send`] + `'static`; its error must be
/// [`Display`](fmt::Display) so a failed play can be logged. Returns the [`io::Error`] from
/// failing to spawn the OS/RTOS thread.
pub fn spawn_buzzer<T>(tone: T) -> io::Result<(BuzzerTask, BuzzerHandle)>
where
    T: Tone + Send + 'static,
    T::Error: fmt::Display,
{
    let (tx, rx): (Sender<Request>, Receiver<Request>) = mpsc::channel();
    let handle: JoinHandle<()> = thread::Builder::new()
        .name("platform-buzzer".to_string())
        .stack_size(BUZZER_STACK_SIZE)
        .spawn(move || buzzer_loop(tone, rx))?;
    Ok((BuzzerTask { handle }, BuzzerHandle { tx }))
}

/// The owner thread body: play each submitted melody to completion, in submission order,
/// before touching the next — the serialisation guarantee that keeps a chime and a jingle
/// from ever interleaving or truncating one another.
///
/// [`recv`](Receiver::recv) parks the thread with no busy-wait between melodies, and returns
/// `Err` once every [`BuzzerHandle`] is dropped and the channel closes — the owner's clean
/// exit, no stop flag needed. A play failure is logged, never fatal (fail-visible, like
/// `spawn_display`'s renders); the submitter is signalled regardless so its `play` returns.
fn buzzer_loop<T>(mut tone: T, requests: Receiver<Request>)
where
    T: Tone,
    T::Error: fmt::Display,
{
    while let Ok(request) = requests.recv() {
        if let Err(err) = tone.play(&request.notes) {
            warn!("platform-buzzer: play failed: {err}");
        }
        // A dropped submitter just means the caller stopped waiting; harmless.
        let _ = request.done.send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier, Mutex};
    use std::thread as std_thread;

    /// A buzzer that records every note it is asked to play, so a test can assert exactly
    /// what sounded and in what order.
    struct RecordingTone {
        log: Arc<Mutex<Vec<Note>>>,
    }

    impl RecordingTone {
        fn new() -> (Self, Arc<Mutex<Vec<Note>>>) {
            let log: Arc<Mutex<Vec<Note>>> = Arc::new(Mutex::new(Vec::new()));
            (
                RecordingTone {
                    log: Arc::clone(&log),
                },
                log,
            )
        }
    }

    impl Tone for RecordingTone {
        type Error = std::convert::Infallible;
        fn play(&mut self, notes: &[Note]) -> Result<(), Self::Error> {
            self.log
                .lock()
                .expect("play log lock")
                .extend_from_slice(notes);
            Ok(())
        }
    }

    /// A short, made-up melody — its content is irrelevant, only that two distinct melodies
    /// are told apart in the recorded log.
    const MELODY_A: [Note; 2] = [Note::new(2_500, 50), Note::new(3_000, 50)];
    const MELODY_B: [Note; 2] = [Note::new(4_000, 60), Note::new(4_500, 60)];

    /// Zero — no melody submitted: the owner starts, sees no requests, and exits cleanly
    /// once its handle is dropped, having played nothing.
    #[test]
    fn no_melody_submitted_plays_nothing() {
        let (base, log): (RecordingTone, _) = RecordingTone::new();
        let (task, handle): (BuzzerTask, BuzzerHandle) =
            spawn_buzzer(base).expect("spawn buzzer thread");

        drop(handle);
        task.join().expect("the buzzer thread must not panic");

        assert_eq!(
            log.lock().expect("play log lock").clone(),
            Vec::<Note>::new()
        );
    }

    /// One — a lone melody plays in full.
    #[test]
    fn a_lone_melody_plays_in_full() {
        let (base, log): (RecordingTone, _) = RecordingTone::new();
        let (task, mut handle): (BuzzerTask, BuzzerHandle) =
            spawn_buzzer(base).expect("spawn buzzer thread");

        handle.play(&MELODY_A).expect("play must reach the owner");

        drop(handle);
        task.join().expect("the buzzer thread must not panic");

        assert_eq!(log.lock().expect("play log lock").clone(), MELODY_A);
    }

    /// Many (R15) — two melodies submitted in sequence through the same handle play fully,
    /// unbroken, and in submission order: the single owner never interleaves one into the
    /// other, never drops one for the other.
    #[test]
    fn two_melodies_through_one_handle_play_fully_and_in_order() {
        let (base, log): (RecordingTone, _) = RecordingTone::new();
        let (task, mut handle): (BuzzerTask, BuzzerHandle) =
            spawn_buzzer(base).expect("spawn buzzer thread");

        handle.play(&MELODY_A).expect("play A must reach the owner");
        handle.play(&MELODY_B).expect("play B must reach the owner");

        drop(handle);
        task.join().expect("the buzzer thread must not panic");

        let expected: Vec<Note> = MELODY_A.iter().chain(MELODY_B.iter()).copied().collect();
        assert_eq!(log.lock().expect("play log lock").clone(), expected);
    }

    /// R15 (Send + Clone) — a handle cloned into another thread reaches the same owner: the
    /// plumbing a composition root relies on to hand the same buzzer to two callers.
    #[test]
    fn a_cloned_handle_from_another_thread_reaches_the_same_owner() {
        let (base, log): (RecordingTone, _) = RecordingTone::new();
        let (task, handle): (BuzzerTask, BuzzerHandle) =
            spawn_buzzer(base).expect("spawn buzzer thread");

        let mut clone: BuzzerHandle = handle.clone();
        let played_from_thread: std_thread::JoinHandle<()> = std_thread::spawn(move || {
            clone.play(&MELODY_A).expect("play must reach the owner");
        });
        played_from_thread
            .join()
            .expect("the submitting thread must not panic");

        drop(handle);
        task.join().expect("the buzzer thread must not panic");

        assert_eq!(log.lock().expect("play log lock").clone(), MELODY_A);
    }

    /// Many (R15, under contention) — two handles on two threads submit whole melodies at the
    /// same instant. The single owner drains each request to completion before it looks at the
    /// next, so the recorded log is one melody followed by the other — which order is the
    /// race's to decide — but never a note of one spliced into the other, and neither
    /// truncated.
    #[test]
    fn two_concurrent_handles_never_interleave() {
        let (base, log): (RecordingTone, _) = RecordingTone::new();
        let (task, handle): (BuzzerTask, BuzzerHandle) =
            spawn_buzzer(base).expect("spawn buzzer thread");

        // Release both submitters together, so they race to the owner as tightly as the OS
        // allows — the worst case for a would-be interleave.
        let gate: Arc<Barrier> = Arc::new(Barrier::new(2));

        let mut handle_a: BuzzerHandle = handle.clone();
        let gate_a: Arc<Barrier> = Arc::clone(&gate);
        let submit_a: std_thread::JoinHandle<()> = std_thread::spawn(move || {
            gate_a.wait();
            handle_a
                .play(&MELODY_A[..])
                .expect("play A must reach the owner");
        });

        let mut handle_b: BuzzerHandle = handle.clone();
        let gate_b: Arc<Barrier> = Arc::clone(&gate);
        let submit_b: std_thread::JoinHandle<()> = std_thread::spawn(move || {
            gate_b.wait();
            handle_b
                .play(&MELODY_B[..])
                .expect("play B must reach the owner");
        });

        submit_a.join().expect("submitter A must not panic");
        submit_b.join().expect("submitter B must not panic");

        drop(handle);
        task.join().expect("the buzzer thread must not panic");

        let played: Vec<Note> = log.lock().expect("play log lock").clone();
        let a_then_b: Vec<Note> = MELODY_A.iter().chain(MELODY_B.iter()).copied().collect();
        let b_then_a: Vec<Note> = MELODY_B.iter().chain(MELODY_A.iter()).copied().collect();
        assert!(
            played == a_then_b || played == b_then_a,
            "two melodies interleaved or were truncated under contention: {played:?}"
        );
    }
}
