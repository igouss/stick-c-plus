//! A `Copy`, fixed-capacity string — the only kind of text an [`Animated`] view can carry.
//!
//! [`Animated`](crate::Animated) is `Copy + Eq`, and the generic render loop additionally
//! requires the state be `Send + 'static`: it computes the view on one thread and remembers the
//! last one it painted, comparing the two to suppress a redundant repaint. That rules out both
//! ends of the usual spectrum — a borrow (`&str`) cannot be `'static`, and a heap or
//! `heapless::String` is not `Copy`. So a view whose picture contains *text that changes* —
//! a transcript line, a tool name, a clock face — needs a string that is a plain value.
//!
//! [`FixedStr`] is that: `N` bytes of inline storage plus a length, `Copy` because its fields
//! are, `Eq` on the *contents* so the suppression comparison means what it says, and `no_std`
//! with no allocator anywhere.
//!
//! ## Truncation is the contract, not a failure
//!
//! Text arriving from the wire is attacker-shaped: a tool name or a transcript line may be any
//! length at all, and the glass is 24 characters wide. [`FixedStr::new`] therefore **truncates**
//! rather than refusing, at the largest UTF-8 character boundary that fits, so the result is
//! always valid UTF-8 and always renders. A caller that needs to know it lost something asks
//! [`was_truncated`](FixedStr::was_truncated).
//!
//! Truncating at a *character* boundary rather than a byte is what keeps this honest: cutting a
//! multi-byte character in half would produce bytes that are not a string at all, and the panic
//! would land in the render loop on the device rather than in a test on the host.

use core::fmt;

/// A string of at most `N` bytes, stored inline.
///
/// `Copy + Eq + Ord`, so it composes into an [`Animated`](crate::Animated) view and compares by
/// content. Built with [`new`](Self::new), which truncates on a character boundary; read with
/// [`as_str`](Self::as_str).
///
/// `N` is a byte capacity, not a character count: with ASCII — which is all the board's 10×20
/// font can draw — the two coincide, and a layout sizes `N` to the field it fills.
#[derive(Clone, Copy)]
pub struct FixedStr<const N: usize> {
    /// The bytes; only `..len` are meaningful, and they are always valid UTF-8.
    bytes: [u8; N],
    /// How many of `bytes` are in use. `usize` rather than a packed `u8` so an `N` past 255 is
    /// not a silent trap for whoever picks the next capacity.
    len: usize,
    /// Whether [`new`](Self::new) dropped anything to make the value fit.
    truncated: bool,
}

impl<const N: usize> FixedStr<N> {
    /// The capacity, in bytes.
    pub const CAPACITY: usize = N;

    /// The empty string.
    pub const fn empty() -> Self {
        FixedStr {
            bytes: [0; N],
            len: 0,
            truncated: false,
        }
    }

    /// The first `N` bytes of `source` that end on a character boundary.
    ///
    /// Never fails and never panics: a value that fits is kept whole, and one that does not is
    /// cut at the last boundary at or below `N`. Because the cut lands on a boundary the
    /// retained bytes are still valid UTF-8, so [`as_str`](Self::as_str) is total.
    pub fn new(source: &str) -> Self {
        let mut end: usize = source.len().min(N);
        // `is_char_boundary` is true at 0 and at `source.len()`, so this walk terminates at
        // worst at the empty prefix — there is no input for which it runs off the front.
        while !source.is_char_boundary(end) {
            end -= 1;
        }
        let kept: &[u8] = &source.as_bytes()[..end];
        let mut bytes: [u8; N] = [0; N];
        bytes[..end].copy_from_slice(kept);
        FixedStr {
            bytes,
            len: end,
            truncated: end < source.len(),
        }
    }

    /// The stored text.
    pub fn as_str(&self) -> &str {
        // `new` only ever stores a character-boundary prefix of a `&str`, and `empty` stores
        // nothing, so the retained bytes are valid UTF-8 by construction. `unwrap_or` rather
        // than `expect` keeps this total even if that construction is ever broken: a render
        // path degrades to a blank field instead of panicking on the device.
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }

    /// How many bytes are stored.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether nothing is stored.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether [`new`](Self::new) dropped part of its input to make it fit.
    ///
    /// The renderer's cue to draw an ellipsis, so a truncated tool name reads as cut off rather
    /// than as a differently-named tool.
    pub const fn was_truncated(&self) -> bool {
        self.truncated
    }
}

impl<const N: usize> Default for FixedStr<N> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<const N: usize> From<&str> for FixedStr<N> {
    fn from(source: &str) -> Self {
        Self::new(source)
    }
}

/// Equality is on the **contents**, not on the backing bytes.
///
/// Load-bearing for the render loop: the padding past `len` is whatever the last longer value
/// left there, so a derived `PartialEq` over the whole array would report two identical strings
/// as different and repaint the glass forever. `truncated` is likewise excluded — it describes
/// where the value came from, not what is drawn.
impl<const N: usize> PartialEq for FixedStr<N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl<const N: usize> Eq for FixedStr<N> {}

impl<const N: usize> PartialOrd for FixedStr<N> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<const N: usize> Ord for FixedStr<N> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl<const N: usize> fmt::Display for FixedStr<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Shows the text, so a failing assertion names the string rather than dumping `N` bytes of
/// padding.
impl<const N: usize> fmt::Debug for FixedStr<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl<const N: usize> core::ops::Deref for FixedStr<N> {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// The capacity every unit test below speaks in — wide enough to hold a short word whole
    /// and narrow enough that a sentence must be cut.
    type Short = FixedStr<8>;

    /// Zero: the empty string holds nothing and lost nothing.
    #[test]
    fn the_empty_string_is_empty() {
        let empty: Short = Short::empty();
        assert_eq!(empty.as_str(), "");
        assert!(empty.is_empty());
        assert!(!empty.was_truncated());
    }

    /// One: a value that fits is kept whole.
    #[test]
    fn a_value_that_fits_is_kept_whole() {
        let fitted: Short = Short::new("Bash");
        assert_eq!(fitted.as_str(), "Bash");
        assert_eq!(fitted.len(), 4);
        assert!(!fitted.was_truncated());
    }

    /// A value exactly at capacity fits — the boundary is inclusive, so an eight-character
    /// field holds an eight-character word without reporting a loss.
    #[test]
    fn a_value_exactly_at_capacity_is_not_truncated() {
        let exact: Short = Short::new("12345678");
        assert_eq!(exact.as_str(), "12345678");
        assert!(!exact.was_truncated());
    }

    /// Many: a value past capacity is cut, and says so.
    #[test]
    fn a_value_past_capacity_is_cut_and_says_so() {
        let cut: Short = Short::new("WebFetch and more");
        assert_eq!(cut.as_str(), "WebFetch");
        assert!(cut.was_truncated());
    }

    /// The cut lands on a character boundary, not a byte: a three-byte character straddling the
    /// capacity is dropped whole rather than sliced into bytes that are not a string.
    ///
    /// `"aaaaaa€"` is six ASCII bytes then three — so a byte-wise cut at 8 would keep two thirds
    /// of the euro sign and the result would not be UTF-8 at all.
    #[test]
    fn a_multibyte_character_straddling_the_capacity_is_dropped_whole() {
        let cut: Short = Short::new("aaaaaa\u{20AC}");
        assert_eq!(cut.as_str(), "aaaaaa");
        assert_eq!(cut.len(), 6);
        assert!(cut.was_truncated());
    }

    /// Equality is on the contents: a short value built after a long one — which is exactly what
    /// happens when a field's text shrinks — compares equal to the same short value built fresh,
    /// however different the padding behind them. Without this the render loop would repaint an
    /// unchanged picture forever.
    #[test]
    fn equality_ignores_the_padding_behind_the_text() {
        let after_long: Short = Short::new("WebFetch and more");
        let fresh: Short = Short::new("WebFetch");
        assert_eq!(after_long, fresh);
    }

    /// Two different strings are different — the other half of the suppression rule, so the
    /// contents-only equality above cannot be satisfied by an equality that says yes to
    /// everything.
    #[test]
    fn two_different_values_are_not_equal() {
        assert_ne!(Short::new("Bash"), Short::new("Read"));
    }

    proptest! {
        /// TOTALITY: whatever the input, the stored text is a prefix of it that fits — so
        /// `as_str` never has to fall back, and no input can produce invalid UTF-8.
        #[test]
        fn the_stored_text_is_always_a_fitting_prefix(source in ".*") {
            let held: Short = Short::new(&source);
            prop_assert!(held.len() <= Short::CAPACITY);
            prop_assert!(source.starts_with(held.as_str()));
        }

        /// A value that fits round-trips exactly, and reports no loss — the truncation flag
        /// tracks the input rather than the capacity.
        #[test]
        fn a_fitting_value_round_trips(source in "\\PC{0,8}".prop_filter(
            "within capacity in BYTES, which is what the storage counts",
            |s: &String| s.len() <= 8,
        )) {
            let held: Short = Short::new(&source);
            prop_assert_eq!(held.as_str(), source.as_str());
            prop_assert!(!held.was_truncated());
        }

        /// The flag and the length agree: something was dropped exactly when the stored text is
        /// shorter than the input.
        #[test]
        fn the_truncation_flag_agrees_with_the_length(source in ".*") {
            let held: Short = Short::new(&source);
            prop_assert_eq!(held.was_truncated(), held.len() < source.len());
        }
    }
}
