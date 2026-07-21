//! What the last reset was, and what it means.

/// Whether the board came up clean or fell over.
///
/// `Clean` covers `PowerOn`, `ExternalPin` and `Software` resets; mapping the raw reset
/// register into that collapse is the adapter's job (`boot-verdict-bz1`), not this crate's —
/// this crate only decides what a crash *means* to the verdict.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootVerdict {
    /// The board came up the ordinary way.
    Clean,
    /// The board came up because something made it restart.
    Crashed(CrashCause),
}

/// Why the board restarted, when it did not come up clean.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrashCause {
    /// A Rust panic unwound to the top and the runtime reset.
    Panic,
    /// The task watchdog decided a thread stopped feeding it.
    TaskWatchdog,
    /// The interrupt watchdog decided an ISR ran too long.
    InterruptWatchdog,
    /// The supply sagged below the brownout threshold.
    Brownout,
}
