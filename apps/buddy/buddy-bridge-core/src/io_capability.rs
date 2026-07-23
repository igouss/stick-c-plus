//! The pairing IO capability, as a type that cannot express the insecure choice.
//!
//! bluer infers the IO capability from *which* agent handlers are set: wiring only
//! `request_passkey` yields KeyboardOnly (Passkey Entry against the device's DisplayOnly);
//! wiring none yields NoInputNoOutput — a silent downgrade to Just Works with no MITM
//! protection. That downgrade is a false green: pairing still "succeeds", but the bond the
//! whole design exists to get is worthless.
//!
//! [`IoCapability`] has exactly one variant, so NoInputNoOutput is **unrepresentable** — there
//! is no value to pass, so no adapter can be handed the insecure posture by mistake. By
//! convention the driving-adapter's agent builder is the sole producer, minting a
//! [`IoCapability::KeyboardOnly`] in the same place it wires `request_passkey`, so "we set the
//! passkey handler" and "our capability is KeyboardOnly" travel together. The composition root
//! logs it at startup, turning the security posture into an asserted fact an operator sees.

/// The pairing IO capability the bridge uses. One variant by design: the insecure
/// NoInputNoOutput posture has no representation here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IoCapability {
    /// Central enters the 6-digit passkey the device displays — LE Secure Connections with
    /// MITM protection. Produced only alongside wiring the `request_passkey` agent handler.
    KeyboardOnly,
}

impl IoCapability {
    /// The BlueZ IO-capability name this posture pairs as — for the startup log line.
    pub const fn as_bluez(self) -> &'static str {
        match self {
            IoCapability::KeyboardOnly => "KeyboardOnly",
        }
    }
}

impl core::fmt::Display for IoCapability {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_bluez())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_only_pairs_as_keyboardonly() {
        assert_eq!(IoCapability::KeyboardOnly.as_bluez(), "KeyboardOnly");
    }

    #[test]
    fn it_displays_its_bluez_name() {
        assert_eq!(IoCapability::KeyboardOnly.to_string(), "KeyboardOnly");
    }
}
