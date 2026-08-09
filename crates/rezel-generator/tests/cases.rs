use std::fs;
use std::path::Path;
use std::sync::Arc;

#[path = "upstream/case_format.rs"]
mod case_format;
#[allow(clippy::all, clippy::pedantic, dead_code, missing_docs)]
#[path = "support/case_registry.rs"]
mod case_registry;

mod support;

mod external_tokens {
    pub(crate) use crate::support::externals::EXT1 as ext1;
}

mod script {
    pub(crate) use crate::support::externals::tag;
}

mod something {
    pub(crate) use crate::support::externals::spec1;
}

use rezel_common::{Input, ParseErrorKind, ParseRequest, Parser, StringInput, TextRange};
use rezel_generator::{BuildOptions, compile_grammar};
use rezel_lr::{
    LRParser,
    table::{GOTO_COMPRESSED_HEADER, GOTO_COMPRESSED_TAG},
};

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
fn external_tokenizer_start_respects_selected_ranges_and_visible_outcomes() {
    let parser =
        LRParser::from_language(generated_case("ExternalTokens").language).with_strict(true);
    let source = "x?Q{x}";
    let input: Arc<dyn Input> = Arc::new(StringInput::try_new(source).unwrap());
    let request = ParseRequest::ranges(
        input,
        vec![
            TextRange::new(0.into(), 1.into()),
            TextRange::new(2.into(), 2.into()),
            TextRange::new(3.into(), 6.into()),
        ],
    )
    .unwrap();
    let mut parse = parser.create_parse(request).unwrap();
    let tree = loop {
        if let Some(tree) = parse.advance().unwrap() {
            break tree;
        }
    };
    assert_eq!(tree.to_string(), "T(X,Braced(X))");

    let source = "x?Q!";
    let input: Arc<dyn Input> = Arc::new(StringInput::try_new(source).unwrap());
    let request = ParseRequest::ranges(
        input,
        vec![
            TextRange::new(0.into(), 0.into()),
            TextRange::new(2.into(), 2.into()),
            TextRange::new(4.into(), 4.into()),
        ],
    )
    .unwrap();
    let mut parse = parser.create_parse(request).unwrap();
    let tree = loop {
        if let Some(tree) = parse.advance().unwrap() {
            break tree;
        }
    };
    assert_eq!(tree.to_string(), "T");

    support::externals::reset_ext1_calls();
    let error = parser.parse("?").unwrap_err();
    assert_eq!(error.kind(), ParseErrorKind::Syntax);
    assert_eq!(support::externals::ext1_calls(), 0);

    support::externals::reset_ext1_calls();
    let error = parser.parse("!").unwrap_err();
    assert_eq!(error.kind(), ParseErrorKind::Input);
    assert_eq!(support::externals::ext1_calls(), 1);

    support::externals::reset_ext1_calls();
    parser.parse("").unwrap();
    assert!(support::externals::ext1_calls() > 0);
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
    let mut multiple_target_terms = 0;
    for (term, position) in goto_header_positions(table).into_iter().enumerate() {
        let Some(mut position) = position else {
            continue;
        };
        let mut group_count = 0;
        let mut largest_length = 0;
        loop {
            let group_tag = table[position];
            let last = group_tag & 1 != 0;
            let (group_length, end) = goto_group_length_and_end(table, position + 2, group_tag);
            group_count += 1;
            largest_length = largest_length.max(group_length);
            position = end;
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

fn goto_header_positions(table: &[u16]) -> Vec<Option<usize>> {
    if table[0] != GOTO_COMPRESSED_HEADER {
        let term_count = usize::from(table[0]);
        let header_length = term_count + 1;
        return table[1..header_length]
            .iter()
            .map(|position| {
                let position = usize::from(*position);
                (position >= header_length).then_some(position)
            })
            .collect();
    }

    let term_count = usize::from(table[1]);
    let header_words = usize::from(table[2]);
    let data_start = 3 + header_words;
    let mut positions = Vec::with_capacity(term_count);
    let mut previous = 0_i64;
    let mut byte_index = 0_usize;
    for _ in 0..term_count {
        let mut code = 0_u64;
        let mut shift = 0_u32;
        loop {
            let word = table[3 + byte_index / 2];
            let byte = if byte_index & 1 == 0 {
                word & 0xff
            } else {
                word >> 8
            };
            byte_index += 1;
            code |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
        }
        if code == 0 {
            positions.push(None);
            continue;
        }
        let zigzag = code - 1;
        let magnitude = i64::try_from(zigzag >> 1).unwrap();
        let sign = -i64::try_from(zigzag & 1).unwrap();
        previous += magnitude ^ sign;
        positions.push(Some(data_start + usize::try_from(previous).unwrap()));
    }
    positions
}

fn goto_group_length_and_end(table: &[u16], position: usize, group_tag: u16) -> (usize, usize) {
    let compressed = group_tag & !1 == GOTO_COMPRESSED_TAG;
    if !compressed {
        let raw_length = usize::from(group_tag >> 1);
        return (raw_length, position + raw_length);
    }

    let source_count = usize::from(table[position]);
    let sources = position + 1;
    let mut byte_index = 0_usize;
    for _ in 0..source_count {
        loop {
            let word = table[sources + byte_index / 2];
            let byte = if byte_index & 1 == 0 {
                word & 0xff
            } else {
                word >> 8
            };
            byte_index += 1;
            if byte & 0x80 == 0 {
                break;
            }
        }
    }
    (source_count, sources + byte_index.div_ceil(2))
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
