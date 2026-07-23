//! The hardware random number generator, read safely.
//!
//! # Why this crate exists
//!
//! Every other crate in both workspaces opens with `#![forbid(unsafe_code)]`, and that stays
//! true. `esp_random` exists only as a C function in ESP-IDF, with no safe wrapper in
//! `esp-idf-svc` or `esp-idf-hal`, so the one `unsafe` lives here — the same containment
//! `esp-metrics` gives the heap counters.
//!
//! # Why the firmware needs it
//!
//! A BLE pairing passkey that is a *constant* defeats the whole point of the bonding it guards.
//! LE Secure Connections with MITM protection assumes the passkey is a secret shared out of band
//! for one pairing only — shown on the device's own glass, typed on the peer. An attacker who
//! knows the constant (because it is in a public repository, say) can complete the pairing
//! themselves. The spike this firmware grew out of used `123456` so pairing was reproducible
//! across flashes; the product must not.
//!
//! # What the ESP32's generator actually is
//!
//! `esp_random` is a true hardware RNG *only while an RF subsystem is enabled* — the entropy
//! comes from thermal noise in the Wi-Fi/Bluetooth receiver. With the radio off it degrades to a
//! pseudo-random sequence. That is exactly why [`passkey`] is documented as being for the
//! **pairing** path: the Bluetooth controller is running by definition when a passkey is being
//! requested, which is the condition under which the reading is a real one.

#![no_std]

use esp_idf_sys::esp_random;

/// The number of distinct BLE passkeys — the Bluetooth spec fixes the range at `000000..=999999`.
pub const PASSKEY_RANGE: u32 = 1_000_000;

/// A uniformly distributed 32-bit random word.
pub fn random_u32() -> u32 {
    // SAFETY: reads the RNG register and returns the word by value. Takes no arguments, touches
    // no memory the caller owns, and is safe to call from any task at any time.
    unsafe { esp_random() }
}

/// A fresh BLE passkey in `000000..=999999`, drawn without modulo bias.
///
/// A plain `random_u32() % 1_000_000` would be *biased*: 2^32 is not a multiple of a million, so
/// the low residues come up slightly more often. The bias is tiny — about one part in four
/// thousand — and it is still a real reduction in the entropy of a secret whose entire job is to
/// be unguessable, for the cost of a loop that almost never runs twice.
///
/// Call this **while the Bluetooth controller is up** (which the pairing callback always is) —
/// see the crate docs on where the entropy comes from.
pub fn passkey() -> u32 {
    // The largest multiple of the range that fits in a u32; draws at or above it are rejected.
    // 2^32 - (2^32 mod range), computed without overflowing: u32::MAX is 2^32 - 1.
    let limit: u32 = u32::MAX - (u32::MAX % PASSKEY_RANGE) - (PASSKEY_RANGE - 1);
    loop {
        let draw: u32 = random_u32();
        if draw < limit {
            return draw % PASSKEY_RANGE;
        }
    }
}
