//! The `LinkPeer` port — the seam between the live BLE link and whatever consumes it.
//!
//! [`DriveLoop`](crate::drive_loop::DriveLoop) knows how to bring a link up, bond, subscribe, and
//! reconnect; it does not know what the bytes *mean*. Once a link is running, `on_run` pumps the TX
//! notifications up and the RX writes down — and this port is where those cross into the permission
//! coordinator (or, under test, a fake). Keeping it a port means the whole run loop stays
//! host-testable: a fake peer records the lines it received and offers the lines to send, with no
//! daemon and no BLE.
//!
//! The methods are deliberately fire-and-forget and synchronous: the production implementor is the
//! daemon actor's handle, whose sends never block. `on_up` hands back the receiver of lines to write
//! this session — the implementor reads the clock and owner itself, so the loop never learns of
//! either.

use tokio::sync::mpsc::UnboundedReceiver;

/// The consumer on the far side of a running BLE link: it takes each whole line the device sent, and
/// supplies the lines to write back, for one link session at a time.
pub trait LinkPeer: Send {
    /// The link is up and subscribed. Begin a session and return the receiver of serialized lines to
    /// write down the link until it drops. The implementor injects its own time/owner; the loop only
    /// pumps what this receiver yields.
    fn on_up(&self) -> UnboundedReceiver<String>;

    /// One whole reassembled line arrived from the device.
    fn on_line(&self, line: Vec<u8>);

    /// The link dropped; the session is over.
    fn on_down(&self);
}
