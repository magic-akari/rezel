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
pub mod ast;
mod highlighting;
mod syntax;
mod tokens;
#[rustfmt::skip]
pub mod typed;

/// Named grammar terms emitted by `rezel-generator`.
pub mod terms;

pub use rezel_common::TypedNode;
pub use typed::*;

/// Return a cheap clone of the default recovering Go parser.
#[must_use]
pub fn parser() -> LRParser {
    default_parser().clone()
}

/// Project the Go CST into Lezer-compatible syntactic highlight tags.
///
/// This deliberately performs no name binding, package loading, or type
/// checking. Use a semantic provider such as gopls for those responsibilities.
#[cfg(feature = "highlight")]
pub fn highlight_spans(tree: &Tree, range: Option<TextRange>, put_span: impl FnMut(HighlightSpan)) {
    rezel_highlight::highlight_spans(tree, range, put_span);
}

fn default_parser() -> &'static LRParser {
    static PARSER: OnceLock<LRParser> = OnceLock::new();
    PARSER.get_or_init(|| LRParser::from_language(&generated::LANGUAGE))
}
