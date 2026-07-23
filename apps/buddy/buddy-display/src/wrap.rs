//! Word wrapping, as an iterator over borrowed lines.
//!
//! The transcript, the prompt hint and the info pages all arrive as one run of text and have to
//! land on a panel that is twenty-four characters wide (thirteen, held upright). [`wrap`]
//! answers that and nothing else: it yields the next line that fits, borrowing from the input,
//! so wrapping allocates nothing and a caller draws each line straight through
//! [`text_field`](platform_display::text_field).
//!
//! Greedy and total. It breaks at the last space that fits; a single word longer than the
//! column count — a path, a URL, a base64 blob, all of which the wire really does deliver — has
//! no space to break at, so it is **hard split** at the column boundary rather than allowed to
//! run off the glass. Losing the end of a long word is a layout decision; drawing past the edge
//! is a bug.

/// A value cut to the field it is drawn in, as a [`Display`](core::fmt::Display) adapter.
///
/// The fixed-width text primitive refuses content past its line buffer rather than truncating,
/// which is right for a value that *should* fit and wrong for a wire-supplied tool name or a
/// Bluetooth address on a thirteen-column canvas — there, refusing means a blank row where a
/// reading belongs. Cutting at a character boundary keeps the result a string.
///
/// A `Display` adapter rather than a function returning one, because the render path formats
/// straight into a stack buffer and must not allocate.
pub struct Cut<'a>(
    /// The value.
    pub &'a str,
    /// The characters it may occupy.
    pub usize,
);

impl core::fmt::Display for Cut<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut end: usize = self.0.len().min(self.1);
        while !self.0.is_char_boundary(end) {
            end -= 1;
        }
        f.write_str(&self.0[..end])
    }
}

/// Wrap `text` into lines of at most `cols` characters.
///
/// The lines borrow from `text`. Leading whitespace is dropped at every break, so the wrap
/// point does not show up as an indent on the next line, and a run of blanks collapses.
///
/// A `cols` of `0` yields nothing: there is no line that fits in no columns, and returning an
/// empty iterator is the honest answer where dividing by the width would trap.
pub fn wrap(text: &str, cols: usize) -> Wrap<'_> {
    Wrap { rest: text, cols }
}

/// The iterator [`wrap`] returns — see it for the wrapping rules.
#[derive(Clone, Copy, Debug)]
pub struct Wrap<'a> {
    rest: &'a str,
    cols: usize,
}

impl<'a> Iterator for Wrap<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        if self.cols == 0 {
            return None;
        }
        let rest: &'a str = self.rest.trim_start();
        if rest.is_empty() {
            self.rest = "";
            return None;
        }

        // Walk to the character that would be the (cols + 1)-th, remembering the last space
        // before it. Counting CHARACTERS while slicing at BYTE indices is what keeps a
        // multi-byte character out of the split: `char_indices` only ever yields boundaries.
        let mut split: Option<usize> = None;
        let mut last_space: Option<usize> = None;
        for (seen, (at, ch)) in rest.char_indices().enumerate() {
            if seen == self.cols {
                split = Some(at);
                break;
            }
            if ch == ' ' {
                last_space = Some(at);
            }
        }

        // The whole remainder fits: one last line, and the iterator is done.
        let Some(split) = split else {
            self.rest = "";
            return Some(rest);
        };

        // Break at the last space that fits, or — for a word with no space in it at all — hard
        // split at the column boundary. A `last_space` of 0 is a leading space `trim_start`
        // already removed, so it cannot produce an empty line here.
        let (line_end, next_start): (usize, usize) = match last_space {
            Some(space) => (space, space + 1),
            None => (split, split),
        };
        self.rest = &rest[next_start..];
        // Trimmed, because a break at the last space that fits leaves every space BEFORE it on
        // the line — a run of blanks in the middle of an entry would otherwise be drawn as a
        // line of padding. The slice starts on a non-space (the input was `trim_start`ed), so
        // trimming the end can never empty it.
        Some(rest[..line_end].trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;
    use alloc::vec::Vec;
    use proptest::prelude::*;

    extern crate alloc;

    /// The panel's landscape width in characters — the column count every unit test wraps to,
    /// so the examples read as the real thing.
    const COLS: usize = 24;

    fn lines(text: &str, cols: usize) -> Vec<&str> {
        wrap(text, cols).collect()
    }

    /// Zero: empty text wraps to no lines at all — not to one empty line.
    #[test]
    fn empty_text_yields_no_lines() {
        assert!(lines("", COLS).is_empty());
    }

    /// Zero columns: nothing fits, so nothing is yielded. The guard that would otherwise be a
    /// division by the width.
    #[test]
    fn no_columns_yields_no_lines() {
        assert!(lines("anything at all", 0).is_empty());
    }

    /// One: text that fits comes back as one line, unchanged.
    #[test]
    fn text_that_fits_is_one_line() {
        assert_eq!(lines("Bash: ls -la", COLS), ["Bash: ls -la"]);
    }

    /// Text exactly at the column count still fits — the boundary is inclusive.
    #[test]
    fn text_exactly_at_the_column_count_is_one_line() {
        let exact: &str = "123456789012345678901234";
        assert_eq!(exact.len(), COLS);
        assert_eq!(lines(exact, COLS), [exact]);
    }

    /// Many: a sentence breaks at spaces, and the space itself is consumed by the break rather
    /// than indenting the next line.
    #[test]
    fn a_sentence_breaks_at_the_last_space_that_fits() {
        assert_eq!(
            lines("Write the design notes into the bead", COLS),
            ["Write the design notes", "into the bead"]
        );
    }

    /// A word with no space in it — a path, a URL — is hard split at the column boundary
    /// instead of running off the glass.
    #[test]
    fn a_word_longer_than_the_line_is_hard_split() {
        assert_eq!(
            lines("/home/elendal/code/m5/stick-c-plus/justfile", COLS),
            ["/home/elendal/code/m5/st", "ick-c-plus/justfile"]
        );
    }

    /// A run of blanks collapses at the break rather than producing an empty line.
    #[test]
    fn a_run_of_spaces_does_not_produce_an_empty_line() {
        assert_eq!(
            lines("alpha                         beta", COLS),
            ["alpha", "beta"]
        );
    }

    /// A value that fits is not cut; one that does not is, at a character boundary — so a
    /// multi-byte character straddling the field is dropped whole rather than sliced into bytes
    /// that are not a string.
    #[test]
    fn a_cut_value_fits_its_field_and_stays_a_string() {
        assert_eq!(alloc::format!("{}", Cut("Bash", 8)), "Bash");
        assert_eq!(
            alloc::format!("{}", Cut("WebFetch and more", 8)),
            "WebFetch"
        );
        assert_eq!(alloc::format!("{}", Cut("aaaaaa\u{20AC}", 8)), "aaaaaa");
        assert_eq!(alloc::format!("{}", Cut("Bash", 0)), "");
    }

    proptest! {
        /// TOTALITY: no yielded line is ever wider than the column count. This is the one
        /// property the glass cares about — a line that overran would be drawn off the edge.
        #[test]
        fn no_line_is_ever_wider_than_the_columns(
            text in ".{0,200}",
            cols in 1usize..40,
        ) {
            let widest: usize = wrap(&text, cols).map(|l: &str| l.chars().count()).max().unwrap_or(0);
            prop_assert!(widest <= cols);
        }

        /// The wrap LOSES nothing but whitespace: concatenating the lines back with a single
        /// space reproduces the input's own words, in order. A wrap that dropped a word — the
        /// failure a "no line is too wide" property alone would happily accept — fails here.
        #[test]
        fn wrapping_preserves_every_word_in_order(
            text in "[a-zA-Z0-9 ]{0,200}",
            cols in 4usize..40,
        ) {
            let wrapped: String = wrap(&text, cols).collect::<Vec<&str>>().join(" ");
            // Hard splits insert breaks INSIDE a word, so compare the character streams with
            // whitespace removed rather than word by word.
            let expected: String = text.chars().filter(|c: &char| *c != ' ').collect();
            let got: String = wrapped.chars().filter(|c: &char| *c != ' ').collect();
            prop_assert_eq!(got, expected);
        }

        /// The iterator always terminates and never yields an empty line — the two ways a
        /// greedy wrap goes wrong, both of which would hang or blank the render loop.
        #[test]
        fn every_yielded_line_is_non_empty(text in ".{0,200}", cols in 1usize..40) {
            let empties: usize = wrap(&text, cols).filter(|l: &&str| l.is_empty()).count();
            prop_assert_eq!(empties, 0);
        }
    }
}
