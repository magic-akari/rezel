#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

use std::sync::OnceLock;

use rezel_common::{
    NodePropSource, ParseError, ParseErrorKind, ParseRequest, PartialParse, TextSize, Tree,
};
use rezel_lr::LRParser;

#[cfg(feature = "highlight")]
use rezel_common::TextRange;
#[cfg(feature = "highlight")]
pub use rezel_highlight::HighlightSpan;
#[cfg(feature = "highlight")]
use rezel_highlight::TagSet;

#[rustfmt::skip]
mod generated;
mod syntax;
mod tokens;
#[rustfmt::skip]
pub mod typed;

/// Named grammar terms emitted by `rezel-generator`.
pub mod terms;

pub use rezel_common::TypedNode;
pub use typed::*;

/// Rust parser with strict syntax validation.
pub type RustParser = LRParser;

struct RustValidatedParse {
    inner: Box<dyn PartialParse>,
}

impl PartialParse for RustValidatedParse {
    fn advance(&mut self) -> Result<Option<Tree>, ParseError> {
        let Some(tree) = self.inner.advance()? else {
            return Ok(None);
        };
        syntax::validate_syntax(&tree).map_err(|error| {
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
    let strict = parser.is_strict();
    let inner = parser.create_lr_parse(request)?;
    if !strict {
        return Ok(inner);
    }
    Ok(Box::new(RustValidatedParse { inner }))
}

fn rust_highlighting() -> NodePropSource {
    #[cfg(feature = "highlight")]
    {
        let tags = rezel_highlight::tags();
        rezel_highlight::style_tags([
            (
                "const macro_rules struct union enum type fn impl trait let static",
                TagSet::from(tags.definition_keyword),
            ),
            ("mod use crate", TagSet::from(tags.module_keyword)),
            (
                "pub unsafe async mut extern default move",
                TagSet::from(tags.modifier),
            ),
            (
                "for if else loop while match continue break return await",
                TagSet::from(tags.control_keyword),
            ),
            ("as in ref", TagSet::from(tags.operator_keyword)),
            ("where _ crate super dyn", TagSet::from(tags.keyword)),
            ("self", TagSet::from(tags.self_)),
            ("String", TagSet::from(tags.string)),
            ("Char", TagSet::from(tags.character)),
            ("RawString", TagSet::from(tags.special.apply(tags.string))),
            ("Boolean", TagSet::from(tags.bool_)),
            ("Identifier", TagSet::from(tags.variable_name)),
            (
                "CallExpression/Identifier",
                TagSet::from(tags.function.apply(tags.variable_name)),
            ),
            (
                "BoundIdentifier",
                TagSet::from(tags.definition.apply(tags.variable_name)),
            ),
            (
                "FunctionItem/BoundIdentifier",
                TagSet::from(
                    tags.function
                        .apply(tags.definition.apply(tags.variable_name)),
                ),
            ),
            ("LoopLabel", TagSet::from(tags.label_name)),
            ("FieldIdentifier", TagSet::from(tags.property_name)),
            (
                "CallExpression/FieldExpression/FieldIdentifier",
                TagSet::from(tags.function.apply(tags.property_name)),
            ),
            (
                "Lifetime",
                TagSet::from(tags.special.apply(tags.variable_name)),
            ),
            ("ScopeIdentifier", TagSet::from(tags.namespace)),
            ("TypeIdentifier", TagSet::from(tags.type_name)),
            (
                "MacroInvocation/Identifier MacroInvocation/ScopedIdentifier/Identifier",
                TagSet::from(tags.macro_name),
            ),
            (
                "MacroInvocation/TypeIdentifier MacroInvocation/ScopedIdentifier/TypeIdentifier",
                TagSet::from(tags.macro_name),
            ),
            ("\"!\"", TagSet::from(tags.macro_name)),
            ("UpdateOp", TagSet::from(tags.update_operator)),
            ("LineComment", TagSet::from(tags.line_comment)),
            ("BlockComment", TagSet::from(tags.block_comment)),
            ("Integer", TagSet::from(tags.integer)),
            ("Float", TagSet::from(tags.float)),
            ("ArithOp", TagSet::from(tags.arithmetic_operator)),
            ("LogicOp", TagSet::from(tags.logic_operator)),
            ("BitOp", TagSet::from(tags.bitwise_operator)),
            ("CompareOp", TagSet::from(tags.compare_operator)),
            ("=", TagSet::from(tags.definition_operator)),
            (".. ... => ->", TagSet::from(tags.punctuation)),
            ("( )", TagSet::from(tags.paren)),
            ("[ ]", TagSet::from(tags.square_bracket)),
            ("{ }", TagSet::from(tags.brace)),
            (". DerefOp", TagSet::from(tags.deref_operator)),
            ("&", TagSet::from(tags.operator)),
            (", ; ::", TagSet::from(tags.separator)),
            ("Attribute/...", TagSet::from(tags.meta)),
        ])
        .expect("the pinned Rust highlight selectors are valid")
    }

    #[cfg(not(feature = "highlight"))]
    {
        rezel_common::group_prop().source(|_| None)
    }
}
