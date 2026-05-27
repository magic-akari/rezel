use std::fs;
use std::path::Path;

#[path = "upstream/case_format.rs"]
mod case_format;
#[allow(clippy::all, clippy::pedantic, dead_code, missing_docs)]
#[path = "support/case_registry.rs"]
mod case_registry;

mod support;

use rezel_generator::{BuildOptions, compile_grammar};
use rezel_lr::LRParser;

use case_format::{expected_diagnostic, split_case_file};
use support::file_tests::file_tests;

struct CaseFile {
    name: String,
    source: String,
}

fn case_files() -> Vec<CaseFile> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/upstream/cases");
    let entries = fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()));
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| {
            panic!(
                "failed to read an entry in {}: {error}",
                directory.display()
            )
        });
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "txt") {
            paths.push(path);
        }
    }
    paths.sort();

    let mut cases = Vec::with_capacity(paths.len());
    for path in paths {
        let name = path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or_else(|| panic!("case path is not UTF-8: {}", path.display()))
            .to_owned();
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        cases.push(CaseFile { name, source });
    }
    cases
}

#[test]
fn case_inventory_and_diagnostics() {
    let cases = case_files();
    assert_eq!(cases.len(), 63);

    let mut diagnostic_count = 0;
    let mut grammar_count = 0;
    let mut parse_case_count = 0;
    let mut configured_case_count = 0;
    for case in &cases {
        let (grammar_source, cases_source) = split_case_file(&case.source);
        let diagnostic = expected_diagnostic(grammar_source);
        if let Some(expected) = diagnostic {
            diagnostic_count += 1;
            assert!(
                cases_source.trim().is_empty(),
                "{} has both a diagnostic and parse cases",
                case.name
            );
            assert_expected_diagnostic(&case.name, grammar_source, expected);
            continue;
        }

        grammar_count += 1;
        assert!(
            !cases_source.trim().is_empty(),
            "{} has neither a diagnostic nor parse cases",
            case.name
        );
        let tests = file_tests(cases_source, &case.name)
            .unwrap_or_else(|error| panic!("{}: {error}", case.name));
        configured_case_count += tests
            .iter()
            .filter(|test| test.config_source.is_some())
            .count();
        parse_case_count += tests.len();
    }

    assert_eq!(diagnostic_count, 16);
    assert_eq!(grammar_count, 47);
    assert_eq!(parse_case_count, 82);
    assert_eq!(configured_case_count, 6);
    assert_eq!(case_registry::CASES.len(), grammar_count);
}

#[test]
fn parse_cases_match_expected_trees() {
    let mut executed = 0;
    let mut failures = Vec::new();
    for case in case_files() {
        let (grammar_source, cases_source) = split_case_file(&case.source);
        if expected_diagnostic(grammar_source).is_some() {
            continue;
        }
        let generated = generated_case(&case.name);
        let parser = LRParser::from_language(generated.language);
        let tests = file_tests(cases_source, &case.name)
            .unwrap_or_else(|error| panic!("{}: {error}", case.name));
        for test in tests {
            if let Err(error) = test.run(parser.clone()) {
                failures.push(format!("{}/{}: {error}", case.name, test.name));
            }
            executed += 1;
        }
    }
    assert_eq!(executed, 82);
    assert!(
        failures.is_empty(),
        "{} upstream parse cases failed:\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

#[test]
fn goto_defaults_cover_the_dominant_source_groups() {
    let mut multiple_target_terms = 0;
    for case in case_files() {
        let (grammar_source, _) = split_case_file(&case.source);
        if expected_diagnostic(grammar_source).is_some() {
            continue;
        }
        let compiled = compile_grammar(grammar_source, Some(&case.name), BuildOptions::default())
            .unwrap_or_else(|error| panic!("{} failed to compile: {error}", case.name));
        multiple_target_terms += assert_dominant_goto_defaults(&case.name, &compiled.goto);
    }
    assert!(
        multiple_target_terms > 0,
        "the upstream grammar inventory contains no multi-target goto terms"
    );
}

fn assert_dominant_goto_defaults(case: &str, table: &[u16]) -> usize {
    let term_count = usize::from(table[0]);
    let header_length = term_count + 1;
    let mut multiple_target_terms = 0;
    for term in 0..term_count {
        let mut position = usize::from(table[term + 1]);
        if position < header_length {
            continue;
        }
        let mut group_count = 0;
        let mut largest_length = 0;
        loop {
            let group_tag = table[position];
            let group_length = usize::from(group_tag >> 1);
            let last = group_tag & 1 != 0;
            group_count += 1;
            largest_length = largest_length.max(group_length);
            position += 2 + group_length;
            if last {
                assert_eq!(
                    group_length, largest_length,
                    "{case}: term {term} does not encode its dominant goto group as the default"
                );
                break;
            }
        }
        multiple_target_terms += usize::from(group_count > 1);
    }
    multiple_target_terms
}

fn generated_case(name: &str) -> &'static case_registry::GeneratedCase {
    case_registry::CASES
        .iter()
        .find(|case| case.name == name)
        .unwrap_or_else(|| panic!("missing generated parser for {name}"))
}

fn assert_expected_diagnostic(name: &str, grammar_source: &str, expected: &str) {
    let observed = match compile_grammar(grammar_source, Some(name), BuildOptions::default()) {
        Err(error) => error.to_string(),
        Ok(compiled) if !compiled.warnings.is_empty() => compiled
            .warnings
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
        Ok(_) => panic!("{name} unexpectedly compiled without a diagnostic"),
    };
    assert!(
        observed.to_lowercase().contains(&expected.to_lowercase()),
        "{name}: expected diagnostic containing {expected:?}, got {observed:?}"
    );
}
