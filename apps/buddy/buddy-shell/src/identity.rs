//! Who this stick is: the advertised name, the firmware version, the address, the owner label.
//!
//! Set once by the composition root, which is the only party that knows any of it — the name
//! comes from the BT controller's address, the version from the build. The owner label is the
//! one field the *host* may change, over the wire.

use buddy_display::{DeviceView, Field};

/// The board's own facts, as the info screen reports them.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Identity {
    /// The advertised name, e.g. `Claude-4F2A`.
    pub name: String,
    /// The firmware version.
    pub firmware: String,
    /// The Bluetooth address.
    pub address: String,
    /// The owner label, as set from the host.
    pub owner: String,
}

impl Identity {
    /// The identity the composition root knows at boot. The owner label starts empty — nobody
    /// has claimed the stick until a host says so.
    pub fn new(name: &str, firmware: &str, address: &str) -> Self {
        Identity {
            name: name.to_string(),
            firmware: firmware.to_string(),
            address: address.to_string(),
            owner: String::new(),
        }
    }

    /// The view the info screen draws, with the two live link facts folded in.
    ///
    /// Bonded and linked are separate arguments rather than fields because they are not facts
    /// about *identity* — they change while the stick is running, and the state that tracks them
    /// is the one that should own them.
    pub fn view(&self, bonded: bool, linked: bool) -> DeviceView {
        DeviceView {
            name: Field::new(&self.name),
            firmware: Field::new(&self.firmware),
            address: Field::new(&self.address),
            owner: Field::new(&self.owner),
            bonded,
            linked,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> Identity {
        Identity::new("Claude-4F2A", "0.1.0", "A0:B7:65:4F:2A:11")
    }

    /// Zero: a fresh stick has no owner — nobody has claimed it.
    #[test]
    fn a_fresh_stick_has_no_owner() {
        assert!(identity().owner.is_empty());
    }

    /// One: the identity reaches the view.
    #[test]
    fn the_identity_reaches_the_view() {
        let view: DeviceView = identity().view(true, true);
        assert_eq!(view.name.as_str(), "Claude-4F2A");
        assert_eq!(view.address.as_str(), "A0:B7:65:4F:2A:11");
    }

    /// Many: bonded and linked are carried through independently, because a bonded stick with no
    /// bridge running is a different thing from an unpaired one.
    #[test]
    fn bonded_and_linked_are_carried_independently() {
        assert!(identity().view(true, false).bonded);
        assert!(!identity().view(true, false).linked);
        assert!(!identity().view(false, true).bonded);
        assert!(identity().view(false, true).linked);
    }
}
