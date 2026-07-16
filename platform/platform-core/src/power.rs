//! The power-source port: whether the board is drawing from USB or running on battery.

/// Whether the board is currently drawing power from USB.
///
/// The driven port for "is VBUS present?" — a thin read of one status bit, sibling to
/// [`Button`](crate::Button) and [`Tone`](crate::Tone). The adapter owns its own failure
/// type; this port names no hardware, no register, no bus.
pub trait PowerSource {
    /// The adapter's own read-failure type.
    type Error;

    /// `true` while USB (VBUS) is present; `false` while running on battery.
    fn on_usb(&mut self) -> Result<bool, Self::Error>;
}
