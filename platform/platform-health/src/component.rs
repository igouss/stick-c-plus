//! Naming the thing a fault is about.

/// The name a fault points at: which registered component this is about.
///
/// A `&'static str`, because the banner has to read "the display thread has been silent for
/// 12 s" — an opaque index cannot. This is a label, not a runtime identity: the crate still
/// has no idea what a thread *is* (that stays outside this crate entirely). `Copy`,
/// allocation-free, `no_std`-clean.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Component(pub &'static str);
