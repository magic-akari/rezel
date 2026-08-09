#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

use std::sync::{Arc, OnceLock};

use rezel_common::{
    Input, ParseError, ParseErrorKind, ParseRequest, PartialParse, TextRange, TextSize, Tree,
};
use rezel_lr::LRParser;

#[cfg(feature = "highlight")]
pub use rezel_highlight::HighlightSpan;

#[rustfmt::skip]
mod generated;
mod highlighting;
mod identifier;
mod input;
mod syntax;
mod tokens;
#[rustfmt::skip]
pub mod typed;

/// Named grammar terms emitted by `rezel-generator`.
pub mod terms;

pub use rezel_common::TypedNode;
pub use typed::*;

/// Unicode version used for Rust identifiers and lifetimes.
pub const UNICODE_VERSION: &str = identifier::UNICODE_VERSION;

/// Rust parser with source-prefix normalization and strict syntax validation.
pub type RustParser = LRParser;

struct RustValidatedParse {
    inner: Box<dyn PartialParse>,
    input: Option<Arc<dyn Input>>,
}

impl PartialParse for RustValidatedParse {
    fn advance(&mut self) -> Result<Option<Tree>, ParseError> {
        let Some(tree) = self.inner.advance()? else {
            return Ok(None);
        };
        let source = self.input.as_ref().map(|input| {
            let range = TextRange::new(TextSize::from(0), input.len());
            input.read(range)
        });
        syntax::validate_syntax(&tree, source.as_deref()).map_err(|error| {
            ParseError::new(
                ParseErrorKind::Syntax,
                Some(error.position()),
                error.message(),
            )
        })?;
        Ok(Some(tree))
    }

    fn parsed_position(&self) -> TextSize {
        self.inner.parsed_position()
    }

    fn stop_at(&mut self, position: TextSize) -> Result<(), ParseError> {
        self.inner.stop_at(position)
    }

    fn stopped_at(&self) -> Option<TextSize> {
        self.inner.stopped_at()
    }
}

/// Return a cheap clone of the default recovering Rust parser.
#[must_use]
pub fn parser() -> RustParser {
    default_parser().clone()
}

/// Project the pinned Lezer syntax tags into Rezel highlight spans.
#[cfg(feature = "highlight")]
pub fn highlight_spans(tree: &Tree, range: Option<TextRange>, put_span: impl FnMut(HighlightSpan)) {
    rezel_highlight::highlight_spans(tree, range, put_span);
}

fn default_parser() -> &'static LRParser {
    static PARSER: OnceLock<LRParser> = OnceLock::new();
    PARSER.get_or_init(|| {
        LRParser::from_language(&generated::LANGUAGE).with_create_parse(create_rust_parse)
    })
}

fn create_rust_parse(
    parser: &LRParser,
    request: ParseRequest,
) -> Result<Box<dyn PartialParse>, ParseError> {
    let request = request.into_validated()?;
    let full_source = matches!(
        request.selected_ranges(),
        [range]
            if range.start() == TextSize::from(0) && range.end() == request.input().len()
    );
    let strict = parser.is_strict();
    let input = (strict && full_source).then(|| Arc::clone(request.input()));
    let lexical_input = Arc::new(input::RustInput::new(Arc::clone(request.input())));
    let request = request.with_lexical_input(lexical_input)?;
    let inner = parser.create_lr_parse(request)?;
    if !strict {
        return Ok(inner);
    }
    Ok(Box::new(RustValidatedParse { inner, input }))
}
