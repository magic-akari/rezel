#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

use std::sync::OnceLock;

use rezel_lr::LRParser;

#[cfg(feature = "highlight")]
use rezel_common::{TextRange, Tree};
#[cfg(feature = "highlight")]
pub use rezel_highlight::HighlightSpan;

#[rustfmt::skip]
mod generated;
mod highlighting;
mod tokens;
#[rustfmt::skip]
pub mod typed;

/// Named grammar terms emitted by `rezel-generator`.
pub mod terms;

pub use rezel_common::TypedNode;
pub use typed::*;

/// PHP concrete-syntax parser.
pub type PhpParser = LRParser;

/// Return a cheap clone of the default recovering PHP template parser.
#[must_use]
pub fn parser() -> PhpParser {
    default_parser().clone()
}

/// Return a recovering parser for PHP code without opening or closing tags.
///
/// # Panics
///
/// Panics only if the generated language no longer exposes its declared
/// `Program` top rule, which is a build-time generation invariant.
#[must_use]
pub fn program_parser() -> PhpParser {
    default_parser()
        .clone()
        .with_top("Program")
        .expect("the generated PHP parser exposes Program")
}

/// Project the PHP CST into Lezer-compatible syntactic highlight tags.
#[cfg(feature = "highlight")]
pub fn highlight_spans(tree: &Tree, range: Option<TextRange>, put_span: impl FnMut(HighlightSpan)) {
    rezel_highlight::highlight_spans(tree, range, put_span);
}

fn default_parser() -> &'static LRParser {
    static PARSER: OnceLock<LRParser> = OnceLock::new();
    PARSER.get_or_init(|| LRParser::from_language(&generated::LANGUAGE))
}
