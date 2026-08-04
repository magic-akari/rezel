#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};

const EXPECTED_COMPILER_ACCEPTED: usize = 61;
const EXPECTED_STRICT_ACCEPTED: usize = 61;
const KNOWN_STRICT_REJECTIONS: &[(&str, u32)] = &[];

#[derive(Debug, Eq, PartialEq)]
struct StrictRejection {
    path: String,
    byte: Option<u32>,
}

#[test]
fn compiler_accepted_inventory_matches_rezel_strict_support() {
    let fixtures = fixtures("parse-accepted");
    assert_eq!(
        fixtures.len(),
        EXPECTED_COMPILER_ACCEPTED,
        "Kotlin compiler-accepted fixture inventory"
    );

    let mut actual_rejections = Vec::new();
    for (relative_path, path) in fixtures {
        let source = read_fixture(&path);
        if let Err(error) = rezel_lang_kotlin::parser().with_strict(true).parse(&source) {
            actual_rejections.push(StrictRejection {
                path: relative_path,
                byte: error.position().map(u32::from),
            });
        }
    }

    let expected_rejections = KNOWN_STRICT_REJECTIONS
        .iter()
        .map(|&(path, byte)| StrictRejection {
            path: path.to_owned(),
            byte: Some(byte),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actual_rejections, expected_rejections,
        "Kotlin compiler-accepted strict rejection inventory (expected vs actual)"
    );
    assert_eq!(
        EXPECTED_COMPILER_ACCEPTED - actual_rejections.len(),
        EXPECTED_STRICT_ACCEPTED,
        "Kotlin compiler-accepted strict support inventory"
    );
}

fn fixtures(directory: &str) -> Vec<(String, PathBuf)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/references/kotlin/fixtures")
        .join(directory);
    let mut paths = Vec::new();
    collect_kotlin_fixtures(&root, &mut paths);
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let relative_path = path
                .strip_prefix(&root)
                .expect("fixture is rooted in the Kotlin oracle directory")
                .to_string_lossy()
                .replace('\\', "/");
            (relative_path, path)
        })
        .collect()
}

fn collect_kotlin_fixtures(root: &Path, paths: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).expect("Kotlin oracle directory is readable") {
        let path = entry
            .expect("Kotlin oracle directory entry is readable")
            .path();
        if path.is_dir() {
            collect_kotlin_fixtures(&path, paths);
        } else if matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("kt" | "kts")
        ) {
            paths.push(path);
        }
    }
}

fn read_fixture(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}
