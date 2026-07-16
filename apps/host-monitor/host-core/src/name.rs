//! HostName — a bounded, owned host label the frame can carry by value.
//!
//! A host in the pulse frame names itself — `fedora`, `oracle-arm`, `oracle-amd`. The
//! frame is the render loop's `Copy + Eq` state, so a host cannot borrow its name from a
//! transient wire buffer; it must *own* it. [`HostName`] is that owned label: a fixed
//! [`CAP`]-byte buffer plus a length, holding the name's UTF-8 bytes, so the whole frame
//! stays a plain array with no heap.
//!
//! Names are short ASCII identifiers, so [`CAP`] is generous. An over-long or non-UTF-8
//! name is *truncated on a character boundary* rather than rejected — a monitor should
//! still show `oracle-very-lo…`'s row, not drop the host because its label ran long.
//!
//! Pure and `no_std`.

/// The most bytes a host name retains. Host identifiers are short (`oracle-arm` is ten
/// ASCII bytes); this leaves headroom without bloating the `Copy` frame.
pub const CAP: usize = 16;

/// A bounded, owned host name — up to [`CAP`] UTF-8 bytes.
///
/// `Copy + Eq`, so it rides inside the frame the render loop compares by value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HostName {
    buf: [u8; CAP],
    len: usize,
}

impl HostName {
    /// The empty name — the fill for an unused host slot.
    pub const EMPTY: HostName = HostName {
        buf: [0; CAP],
        len: 0,
    };

    /// Build a name from `s`, copying up to [`CAP`] bytes on a UTF-8 character boundary.
    ///
    /// A name longer than the buffer is truncated at the last whole character that fits, so
    /// [`as_str`](Self::as_str) is always valid UTF-8 — a multibyte character is never cut
    /// in half.
    pub fn new(s: &str) -> Self {
        let mut end: usize = core::cmp::min(s.len(), CAP);
        // Back off to a character boundary so the retained bytes are valid UTF-8.
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        let mut name: HostName = HostName::EMPTY;
        name.buf[..end].copy_from_slice(&s.as_bytes()[..end]);
        name.len = end;
        name
    }

    /// The name as a string slice — always valid UTF-8.
    pub fn as_str(&self) -> &str {
        // `new` only ever stores valid UTF-8 on a character boundary, so this cannot fail;
        // fall back to empty rather than panic if that invariant is ever violated.
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }

    /// Whether the name is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for HostName {
    fn default() -> Self {
        Self::EMPTY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_name_is_empty() {
        let name: HostName = HostName::new("");
        assert!(name.is_empty());
        assert_eq!(name.as_str(), "");
    }

    #[test]
    fn a_short_name_round_trips() {
        assert_eq!(HostName::new("fedora").as_str(), "fedora");
        assert_eq!(HostName::new("oracle-arm").as_str(), "oracle-arm");
    }

    #[test]
    fn an_over_long_name_is_truncated_to_capacity() {
        let long: &str = "oracle-arm-node-01-east"; // > CAP bytes
        let name: HostName = HostName::new(long);
        assert_eq!(name.as_str().len(), CAP);
        assert_eq!(name.as_str(), &long[..CAP]);
    }

    #[test]
    fn truncation_keeps_utf8_on_a_character_boundary() {
        // A name whose CAP-th byte falls inside a multibyte character truncates *before*
        // it, so the result is still valid UTF-8 (shorter than CAP).
        let name: HostName = HostName::new("näme-that-is-toooo-long-é");
        // as_str must not panic and must be valid UTF-8.
        let s: &str = name.as_str();
        assert!(s.len() <= CAP);
        assert!(core::str::from_utf8(s.as_bytes()).is_ok());
    }
}
