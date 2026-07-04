//! Golden id test: the crate's `MESSAGE_IDS` must match TWO independent
//! representations of the pinned aioesphomeapi 1.14 commit —
//!
//!   1. the `option (id) = N` annotations in the vendored `proto/api.proto`,
//!      re-parsed here (guards `ids.rs` against drift or a hand edit), and
//!   2. aioesphomeapi's own `MESSAGE_TYPE_TO_PROTO` table, captured from
//!      `core.py` into `fixtures/message_ids.tsv` — a separate, hand-maintained
//!      oracle in a different language.
//!
//! Agreement across both proves the registry is faithful to the source, not
//! merely self-consistent. A round-trip against a single source could not.

use esphome_api::MESSAGE_IDS;
use regex::Regex;

const PROTO: &str = include_str!("../proto/api.proto");
const CORE_FIXTURE: &str = include_str!("fixtures/message_ids.tsv");

/// Re-derive `(id, name)` from the proto, attributing each `option (id)` to the
/// message whose body actually contains it — the search is bounded by the next
/// `message` declaration, so a message without an id never borrows a later
/// message's annotation.
fn ids_from_proto() -> Vec<(u32, String)> {
    let decl: Regex = Regex::new(r"message\s+(\w+)\s*\{").unwrap();
    let id_opt: Regex = Regex::new(r"option\s*\(id\)\s*=\s*(\d+)\s*;").unwrap();

    let decls: Vec<(usize, String)> = decl
        .captures_iter(PROTO)
        .map(|c: regex::Captures| {
            let end: usize = c.get(0).unwrap().end();
            (end, c[1].to_string())
        })
        .collect();

    let mut out: Vec<(u32, String)> = Vec::new();
    for (i, (body_start, name)) in decls.iter().enumerate() {
        let span_end: usize = decls.get(i + 1).map(|d: &(usize, String)| d.0).unwrap_or(PROTO.len());
        if let Some(c) = id_opt.captures(&PROTO[*body_start..span_end]) {
            out.push((c[1].parse::<u32>().unwrap(), name.clone()));
        }
    }
    out.sort();
    out
}

/// The independent oracle: aioesphomeapi's `MESSAGE_TYPE_TO_PROTO`, captured to
/// a TSV fixture.
fn ids_from_fixture() -> Vec<(u32, String)> {
    let mut out: Vec<(u32, String)> = CORE_FIXTURE
        .lines()
        .filter(|line: &&str| {
            let t: &str = line.trim();
            !t.is_empty() && !t.starts_with('#')
        })
        .map(|line: &str| {
            let (id, name): (&str, &str) = line.split_once('\t').expect("fixture rows are id<TAB>name");
            (id.trim().parse::<u32>().unwrap(), name.trim().to_string())
        })
        .collect();
    out.sort();
    out
}

/// The crate's registry, normalised to owned pairs and sorted for comparison.
fn registry() -> Vec<(u32, String)> {
    let mut out: Vec<(u32, String)> = MESSAGE_IDS
        .iter()
        .map(|(id, name): &(u32, &str)| (*id, name.to_string()))
        .collect();
    out.sort();
    out
}

#[test]
fn registry_matches_the_vendored_proto() {
    assert_eq!(
        registry(),
        ids_from_proto(),
        "MESSAGE_IDS drifted from the option(id) annotations in proto/api.proto"
    );
}

#[test]
fn registry_matches_aioesphomeapi_core_table() {
    assert_eq!(
        registry(),
        ids_from_fixture(),
        "MESSAGE_IDS disagrees with aioesphomeapi core.py MESSAGE_TYPE_TO_PROTO"
    );
}

#[test]
fn the_registry_is_sorted_and_unique_by_id() {
    let ids: Vec<u32> = MESSAGE_IDS.iter().map(|(id, _): &(u32, &str)| *id).collect();

    let mut ordered: Vec<u32> = ids.clone();
    ordered.sort();
    assert_eq!(ids, ordered, "MESSAGE_IDS must stay sorted by id — message_name() binary-searches it");

    ordered.dedup();
    assert_eq!(ordered.len(), MESSAGE_IDS.len(), "MESSAGE_IDS has duplicate ids");
}
