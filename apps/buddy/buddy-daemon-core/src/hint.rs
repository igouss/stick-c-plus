//! The hint under a tool name: where the call is happening, in as few characters as possible.
//!
//! The glass has forty-eight characters for it and a working directory is routinely longer than
//! that, so the front of the path — which is the part every directory on the machine has in
//! common — is what pushes the part that identifies it off the end. Shortening the owner's home
//! to `~` costs nothing in meaning and buys back the whole prefix.
//!
//! Pure, and the home directory is **injected**: this crate reads no environment. The shell that
//! owns the process passes what it found, and a daemon that could not find one passes `None` and
//! gets the path unchanged.

/// The path an owner's home is shown as.
const HOME_MARK: &str = "~";

/// `cwd`, with `home` shortened to `~` when it is a prefix of it.
///
/// Only a whole path component matches: `/home/pat` shortens `/home/pat/code` but not
/// `/home/patricia`, which is a different person's directory and must not be dressed up as this
/// one's. An empty or absent home shortens nothing — a prefix that matches everything would turn
/// every path into `~`.
pub fn shorten(cwd: &str, home: Option<&str>) -> String {
    let home: &str = match home {
        Some(home) if !home.is_empty() => home.trim_end_matches('/'),
        _ => return cwd.to_string(),
    };
    if cwd == home {
        return HOME_MARK.to_string();
    }
    match cwd.strip_prefix(home) {
        Some(rest) if rest.starts_with('/') => format!("{HOME_MARK}{rest}"),
        _ => cwd.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Zero: no home to shorten by, so nothing is shortened. Stated for the empty string too,
    /// because an empty prefix matches every path and would turn all of them into `~`.
    #[test]
    fn a_path_with_no_home_to_shorten_by_is_unchanged() {
        assert_eq!(shorten("/home/pat/code", None), "/home/pat/code");
        assert_eq!(shorten("/home/pat/code", Some("")), "/home/pat/code");
    }

    /// One: the home itself is the mark alone.
    #[test]
    fn the_home_directory_itself_is_the_mark() {
        assert_eq!(shorten("/home/pat", Some("/home/pat")), "~");
        assert_eq!(shorten("/home/pat", Some("/home/pat/")), "~");
    }

    /// Many: a path under it keeps everything that identifies it, and loses only the prefix
    /// every directory on the machine shares.
    #[test]
    fn a_path_under_the_home_keeps_only_what_identifies_it() {
        assert_eq!(
            shorten("/home/pat/code/m5/stick-c-plus", Some("/home/pat")),
            "~/code/m5/stick-c-plus"
        );
    }

    /// THE ONE THAT MATTERS: a component boundary, not a string prefix. Another person's home
    /// that merely starts with these letters is not this owner's, and shortening it would show a
    /// path that does not exist.
    #[test]
    fn a_longer_name_that_merely_starts_the_same_is_not_the_home() {
        assert_eq!(
            shorten("/home/patricia/code", Some("/home/pat")),
            "/home/patricia/code"
        );
    }

    /// A path outside the home is left alone, root included.
    #[test]
    fn a_path_outside_the_home_is_left_alone() {
        assert_eq!(shorten("/etc/hosts", Some("/home/pat")), "/etc/hosts");
        assert_eq!(shorten("/", Some("/home/pat")), "/");
    }
}
