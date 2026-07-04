//! Cross-oracle test for [`object_id_key`]: the Rust FNV-1a hash must match an
//! independent implementation.
//!
//! `fixtures/entity_keys.tsv` was produced by `tools/fnv1a_oracle.py` — a second
//! implementation of FNV-1a, in a different language. Agreement across the two
//! proves the hash is faithful to the algorithm, not merely self-consistent, the
//! same discipline `golden_ids.rs` applies to the message-id table. Because the
//! hash is seedless, the fixture is reproduced by every fresh test process, which
//! is exactly the reboot-stability the entity key relies on.

use esphome_api::object_id_key;

const FIXTURE: &str = include_str!("fixtures/entity_keys.tsv");

#[test]
fn object_id_key_matches_the_python_oracle() {
    let mut rows: usize = 0;
    for raw in FIXTURE.lines() {
        // Skip comments and blank lines. The empty-object_id row survives: once
        // trimmed it reads as its key hex, so it is neither blank nor a comment;
        // the split below still runs on the raw line, giving an empty object_id.
        let trimmed: &str = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let (object_id, key_hex): (&str, &str) = raw
            .split_once('\t')
            .expect("each row is object_id<TAB>key_hex");
        let expected: u32 =
            u32::from_str_radix(key_hex.trim(), 16).expect("key_hex is 8 hex digits");
        assert_eq!(
            object_id_key(object_id),
            expected,
            "FNV-1a mismatch for object_id {object_id:?}"
        );
        rows += 1;
    }
    // Guard against a silently-empty fixture passing vacuously.
    assert!(rows >= 5, "expected several oracle rows, parsed {rows}");
}
