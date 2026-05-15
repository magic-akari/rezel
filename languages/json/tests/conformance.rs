use std::fs;
use std::path::PathBuf;

use rezel_common::ParseErrorKind;
use rezel_lr::ParseLimits;

const CORPUS_PATH: &str = "tests/conformance/json-test-suite/test_parsing";
const EXTENDED_CASES: [&str; 2] = [
    "n_structure_100000_opening_arrays.json",
    "n_structure_open_array_object.json",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Expectation {
    Accept,
    Reject,
    ImplementationDefined,
}

#[derive(Debug, Default, Eq, PartialEq)]
struct Inventory {
    accept: usize,
    reject: usize,
    implementation_defined: usize,
    non_utf8: usize,
}

#[test]
fn json_test_suite_conformance() {
    let parser = rezel_lang_json::parser().with_strict(true);
    let mut inventory = Inventory::default();

    for path in corpus_cases() {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("JSONTestSuite case names are UTF-8");
        let expectation = expectation(name);
        inventory.record(expectation);

        let bytes = fs::read(&path).expect("read JSONTestSuite case");
        let Ok(source) = std::str::from_utf8(&bytes) else {
            inventory.non_utf8 += 1;
            assert_ne!(
                expectation,
                Expectation::Accept,
                "required-accept case {name} is not UTF-8"
            );
            continue;
        };

        if EXTENDED_CASES.contains(&name) {
            continue;
        }

        match expectation {
            Expectation::Accept => {
                parser
                    .parse(source)
                    .unwrap_or_else(|error| panic!("required-accept case {name} failed: {error}"));
            }
            Expectation::Reject => {
                let Err(error) = parser.parse(source) else {
                    panic!("required-reject case {name} was accepted");
                };
                assert_eq!(
                    error.kind(),
                    ParseErrorKind::Syntax,
                    "required-reject case {name} failed for a non-syntax reason: {error}"
                );
            }
            Expectation::ImplementationDefined => {
                let _ = parser.parse(source);
            }
        }
    }

    assert_eq!(
        inventory,
        Inventory {
            accept: 95,
            reject: 188,
            implementation_defined: 35,
            non_utf8: 25,
        }
    );
}

#[test]
#[ignore = "run by `mise run verify:full`"]
fn deep_json_test_suite_cases_are_bounded() {
    let limits = ParseLimits {
        max_actions: 50_000,
        max_stacks: 64,
        max_stack_depth: 512,
        max_buffer_records: 20_000,
        max_recovery_actions: 2_000,
    };
    let parser = rezel_lang_json::parser()
        .with_strict(true)
        .with_limits(limits);

    for name in EXTENDED_CASES {
        let source = fs::read_to_string(corpus_directory().join(name))
            .expect("extended JSONTestSuite case is UTF-8");
        let Err(error) = parser.parse(&source) else {
            panic!("deep required-reject case {name} was accepted");
        };
        assert_eq!(
            error.kind(),
            ParseErrorKind::ResourceLimit,
            "deep case {name} did not stop at the configured resource boundary: {error}"
        );
    }
}

fn corpus_cases() -> Vec<PathBuf> {
    let mut paths = fs::read_dir(corpus_directory())
        .expect("JSONTestSuite corpus directory")
        .map(|entry| entry.expect("JSONTestSuite directory entry").path())
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn corpus_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(CORPUS_PATH)
}

fn expectation(name: &str) -> Expectation {
    if name.starts_with("y_") {
        Expectation::Accept
    } else if name.starts_with("n_") {
        Expectation::Reject
    } else if name.starts_with("i_") {
        Expectation::ImplementationDefined
    } else {
        panic!("unexpected JSONTestSuite case name {name}");
    }
}

impl Inventory {
    fn record(&mut self, expectation: Expectation) {
        match expectation {
            Expectation::Accept => self.accept += 1,
            Expectation::Reject => self.reject += 1,
            Expectation::ImplementationDefined => self.implementation_defined += 1,
        }
    }
}
