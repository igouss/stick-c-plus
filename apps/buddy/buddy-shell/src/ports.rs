//! The two driven ports the shell needs and refuses to implement.
//!
//! Both are one method wide, because that is genuinely all the shell asks of the outside world:
//! put a line on the wire, and forget the bond. The composition root implements them over
//! NimBLE; a test implements them over a `Vec`.

/// Somewhere to put a device→central line.
///
/// The transport does the framing and the MTU-sized fragmentation; the shell hands over one
/// complete line and is told whether it left. A refusal is *reported*, not retried here — the
/// hook host-side holds its own deadline and fails safe on its own, so a device that queued and
/// resent an answer would be racing a decision that has already been made.
pub trait Notifier {
    /// Send one line. `Err` means it did not go.
    fn notify(&self, line: &str) -> Result<(), NotifyError>;
}

/// Why a line did not leave the device.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NotifyError {
    /// What the transport said, for the log.
    pub reason: String,
}

impl NotifyError {
    /// A refusal carrying `reason`.
    pub fn new(reason: impl Into<String>) -> Self {
        NotifyError {
            reason: reason.into(),
        }
    }
}

impl core::fmt::Display for NotifyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "the line was not sent: {}", self.reason)
    }
}

impl std::error::Error for NotifyError {}

/// The BLE bond, as far as the shell is concerned: something that can be forgotten.
pub trait Bond {
    /// Forget every bonded central. The device is expected to become pairable again.
    fn forget(&self);
}

/// Somewhere to persist the selected species across a reboot.
///
/// Separate from [`Bond`] because they fail differently and at different times: a bond is
/// forgotten once, deliberately, by a person holding the device; a species is written whenever
/// the menu cycles.
pub trait SpeciesStore {
    /// Persist the selected registry index.
    fn store(&self, index: u8);
}
