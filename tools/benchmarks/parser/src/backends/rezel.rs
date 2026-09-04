use std::{hint::black_box, sync::Arc};

use rezel_lr::LRParser;

use crate::datasets::{Language, SourceDataset, Tier};

pub struct ParseCase {
    parser: LRParser,
    sources: Vec<Arc<str>>,
}

#[must_use]
/// Loads and pre-parses one Rezel benchmark dataset.
///
/// # Panics
///
/// Panics when the dataset is unavailable or Rezel rejects an input.
pub fn setup(language: Language, tier: Tier) -> ParseCase {
    let dataset = SourceDataset::load(language, tier)
        .unwrap_or_else(|error| panic!("failed to load {language} {tier}: {error}"));
    let parser = parser(language);
    let validator = parser.clone().with_strict(true);
    for file in &dataset.files {
        validator
            .parse(file.source.as_ref())
            .unwrap_or_else(|error| {
                panic!(
                    "Rezel rejected {language} {tier} source {}: {error}",
                    file.path
                )
            });
    }
    ParseCase {
        parser,
        sources: dataset.files.into_iter().map(|file| file.source).collect(),
    }
}

/// Parses every file in a prepared Rezel dataset.
///
/// # Panics
///
/// Panics if an input parsed during setup later fails to parse.
pub fn parse(case: ParseCase) {
    for source in case.sources {
        let tree = case
            .parser
            .parse(source.as_ref())
            .expect("setup parses every Rezel input");
        black_box(tree);
    }
}

fn parser(language: Language) -> LRParser {
    match language {
        Language::Go => rezel_lang_go::parser(),
        Language::Java => rezel_lang_java::parser(),
        Language::Json => rezel_lang_json::parser(),
        Language::Kotlin => rezel_lang_kotlin::parser(),
        Language::Php => rezel_lang_php::parser(),
        Language::Python => rezel_lang_python::parser(),
        Language::Rust => rezel_lang_rust::parser(),
        Language::Swift => rezel_lang_swift::parser(),
    }
}
