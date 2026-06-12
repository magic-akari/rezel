use std::collections::HashSet;
use std::path::Path;

use serde::Deserialize;

const SNAPSHOT_SCHEMA: &str = "rezel.rustc-rust-reference-snapshot.v1";
const SNAPSHOT: &str = include_str!("../../../../tools/references/rustc/snapshots/rust.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Snapshot {
    schema: String,
    rustc_version: String,
    edition: String,
    coordinates: String,
    oracle: String,
    accepted: Vec<ReferenceCase>,
    rejected: Vec<ReferenceCase>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReferenceCase {
    id: String,
    source_name: String,
    source: String,
}

#[test]
fn accepted_sources_match_rustc_1_95_edition_2024() {
    let snapshot = reference_data();
    for case in &snapshot.accepted {
        rezel_lang_rust::parser()
            .with_strict(true)
            .parse(&case.source)
            .unwrap_or_else(|error| {
                panic!(
                    "rustc accepted {} ({}) but Rezel returned {error}",
                    case.id, case.source_name
                )
            });
    }
}

#[test]
fn rejected_sources_match_rustc_1_95_edition_2024() {
    let snapshot = reference_data();
    for case in &snapshot.rejected {
        assert!(
            rezel_lang_rust::parser()
                .with_strict(true)
                .parse(&case.source)
                .is_err(),
            "rustc rejected {} ({}) but Rezel accepted it",
            case.id,
            case.source_name,
        );
    }
}

fn reference_data() -> Snapshot {
    let snapshot =
        serde_json::from_str::<Snapshot>(SNAPSHOT).expect("valid rustc reference snapshot");
    assert_eq!(snapshot.schema, SNAPSHOT_SCHEMA);
    assert_eq!(
        snapshot.rustc_version,
        "rustc 1.95.0 (59807616e 2026-04-14)"
    );
    assert_eq!(snapshot.edition, "2024");
    assert_eq!(snapshot.coordinates, "raw-utf8-bytes");
    assert_eq!(snapshot.oracle, "rustc --crate-type=lib --emit=metadata");
    assert!(!snapshot.accepted.is_empty());
    assert!(!snapshot.rejected.is_empty());

    let mut ids = HashSet::new();
    for case in snapshot.accepted.iter().chain(&snapshot.rejected) {
        assert!(!case.id.is_empty());
        assert!(
            ids.insert(&case.id),
            "duplicate Rust reference case {}",
            case.id
        );
        assert!(!case.source.is_empty());
        assert!(
            Path::new(&case.source_name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
        );
    }
    snapshot
}
