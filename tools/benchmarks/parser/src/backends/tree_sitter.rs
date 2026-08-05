use std::{hint::black_box, sync::Arc};

use crate::datasets::{Language, SourceDataset, Tier};

pub struct ParseCase {
    parser: tree_sitter::Parser,
    sources: Vec<Arc<str>>,
}

#[must_use]
/// Loads and strictly validates one Tree-sitter benchmark dataset.
///
/// # Panics
///
/// Panics when the dataset is unavailable or Tree-sitter rejects an input.
pub fn setup(language: Language, tier: Tier) -> ParseCase {
    let dataset = SourceDataset::load(language, tier)
        .unwrap_or_else(|error| panic!("failed to load {language} {tier}: {error}"));
    let mut validator = parser(language);
    for file in &dataset.files {
        let tree = validator
            .parse(file.source.as_bytes(), None)
            .expect("Tree-sitter must return a validation tree");
        assert!(
            !tree.root_node().has_error(),
            "Tree-sitter rejected {language} {tier} source {}",
            file.path
        );
    }
    ParseCase {
        parser: parser(language),
        sources: dataset.files.into_iter().map(|file| file.source).collect(),
    }
}

/// Parses every file in a prepared Tree-sitter dataset.
///
/// # Panics
///
/// Panics if an input accepted during setup later fails to parse.
pub fn parse(mut case: ParseCase) {
    for source in case.sources {
        let tree = case
            .parser
            .parse(source.as_bytes(), None)
            .expect("setup validates every Tree-sitter input");
        black_box(tree);
    }
}

fn parser(language: Language) -> tree_sitter::Parser {
    let language = match language {
        Language::Go => tree_sitter_go::LANGUAGE.into(),
        Language::Java => tree_sitter_java::LANGUAGE.into(),
        Language::Json => tree_sitter_json::LANGUAGE.into(),
        Language::Kotlin => tree_sitter_kotlin::LANGUAGE.into(),
        Language::Python => tree_sitter_python::LANGUAGE.into(),
        Language::Rust => tree_sitter_rust::LANGUAGE.into(),
        Language::Swift => tree_sitter_swift::LANGUAGE.into(),
    };
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language)
        .expect("pinned Tree-sitter grammar must be ABI-compatible");
    parser
}
