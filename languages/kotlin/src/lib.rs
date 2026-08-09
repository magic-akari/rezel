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
mod syntax;
mod tokens;
#[rustfmt::skip]
pub mod typed;

/// Named grammar terms emitted by `rezel-generator`.
pub mod terms;

pub use rezel_common::TypedNode;
pub use typed::*;

/// Kotlin parser with strict identifier validation.
pub type KotlinParser = LRParser;

struct KotlinValidatedParse {
    inner: Box<dyn PartialParse>,
    input: Option<Arc<dyn Input>>,
}

impl PartialParse for KotlinValidatedParse {
    fn advance(&mut self) -> Result<Option<Tree>, ParseError> {
        let Some(tree) = self.inner.advance()? else {
            return Ok(None);
        };
        if let Some(input) = self.input.as_ref() {
            let range = TextRange::new(TextSize::from(0), input.len());
            let source = input.read(range);
            syntax::validate_identifiers(&tree, &source).map_err(|error| {
                ParseError::new(
                    ParseErrorKind::Syntax,
                    Some(error.position()),
                    error.message(),
                )
            })?;
        }
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

/// Return a cheap clone of the default recovering Kotlin parser.
#[must_use]
pub fn parser() -> KotlinParser {
    default_parser().clone()
}

/// Project the Kotlin CST into Lezer-compatible syntactic highlight tags.
#[cfg(feature = "highlight")]
pub fn highlight_spans(tree: &Tree, range: Option<TextRange>, put_span: impl FnMut(HighlightSpan)) {
    rezel_highlight::highlight_spans(tree, range, put_span);
}

fn default_parser() -> &'static LRParser {
    static PARSER: OnceLock<LRParser> = OnceLock::new();
    PARSER.get_or_init(|| {
        LRParser::from_language(&generated::LANGUAGE).with_create_parse(create_kotlin_parse)
    })
}

fn create_kotlin_parse(
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
    let inner = parser.create_lr_parse(request)?;
    if !strict {
        return Ok(inner);
    }
    Ok(Box::new(KotlinValidatedParse { inner, input }))
}
