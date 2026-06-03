//! Python parser, typed concrete syntax, and owned syntax AST.

#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

use std::sync::{Arc, OnceLock};

use rezel_common::{
    Input, NodePropSource, ParseError, ParseErrorKind, ParseRequest, PartialParse, TextRange,
    TextSize, Tree,
};
use rezel_lr::LRParser;

#[cfg(feature = "highlight")]
pub use rezel_highlight::HighlightSpan;
#[cfg(feature = "highlight")]
use rezel_highlight::TagSet;

#[rustfmt::skip]
mod generated;
mod identifier;
mod indentation;
mod tokens;
mod unicode_names;
#[rustfmt::skip]
mod typed;

pub mod ast;
mod syntax;
/// Named grammar terms emitted by `rezel-generator`.
pub mod terms;
pub use rezel_common::TypedNode;
pub use typed::*;

/// Unicode Character Database version used for Python identifiers.
pub const UNICODE_VERSION: &str = identifier::UNICODE_VERSION;

/// Python parser with strict indentation and syntax validation.
pub type PythonParser = LRParser;

struct PythonValidatedParse {
    inner: Box<dyn PartialParse>,
    input: Arc<dyn Input>,
}

impl PartialParse for PythonValidatedParse {
    fn advance(&mut self) -> Result<Option<Tree>, ParseError> {
        let Some(tree) = self.inner.advance()? else {
            return Ok(None);
        };
        let range = TextRange::new(TextSize::from(0), self.input.len());
        let source = self.input.read(range);
        syntax::validate_syntax(&tree, &source).map_err(|error| {
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

/// Return a cheap clone of the default recovering Python parser.
#[must_use]
pub fn parser() -> PythonParser {
    default_parser().clone()
}

/// Project the Python CST into Lezer-compatible syntactic highlight tags.
#[cfg(feature = "highlight")]
pub fn highlight_spans(tree: &Tree, range: Option<TextRange>, put_span: impl FnMut(HighlightSpan)) {
    rezel_highlight::highlight_spans(tree, range, put_span);
}

fn default_parser() -> &'static LRParser {
    static PARSER: OnceLock<LRParser> = OnceLock::new();
    PARSER.get_or_init(|| {
        LRParser::from_language(&generated::LANGUAGE).with_create_parse(create_python_parse)
    })
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "CreateParse callbacks receive an owned request"
)]
fn create_python_parse(
    parser: &LRParser,
    request: ParseRequest,
) -> Result<Box<dyn PartialParse>, ParseError> {
    let full_source = matches!(
        request.selected_ranges(),
        [range]
            if range.start() == TextSize::from(0) && range.end() == request.input().len()
    );
    let validate = parser.is_strict() && full_source;
    if validate {
        let range = TextRange::new(TextSize::from(0), request.input().len());
        let source = request.input().read(range);
        indentation::validate(&source)?;
    }

    let input = validate.then(|| Arc::clone(request.input()));
    let inner = parser.create_lr_parse(&request)?;
    let Some(input) = input else {
        return Ok(inner);
    };
    Ok(Box::new(PythonValidatedParse { inner, input }))
}

fn python_highlighting() -> NodePropSource {
    #[cfg(feature = "highlight")]
    {
        let tags = rezel_highlight::tags();
        rezel_highlight::style_tags([
            (
                "async \"*\" \"**\" FormatConversion FormatSpec",
                TagSet::from(tags.modifier),
            ),
            (
                "for while if elif else try except finally return raise break continue with pass assert await yield match case",
                TagSet::from(tags.control_keyword),
            ),
            ("in not and or is del", TagSet::from(tags.operator_keyword)),
            (
                "from def class global nonlocal lambda type",
                TagSet::from(tags.definition_keyword),
            ),
            ("import", TagSet::from(tags.module_keyword)),
            ("with as print", TagSet::from(tags.keyword)),
            ("Boolean", TagSet::from(tags.bool_)),
            ("None", TagSet::from(tags.null)),
            ("VariableName", TagSet::from(tags.variable_name)),
            (
                "FunctionDefinition/VariableName",
                TagSet::from(tags.function.apply(tags.definition.apply(tags.variable_name))),
            ),
            (
                "ClassDefinition/VariableName",
                TagSet::from(tags.definition.apply(tags.class_name)),
            ),
            ("PropertyName", TagSet::from(tags.property_name)),
            ("Comment", TagSet::from(tags.line_comment)),
            ("Number", TagSet::from(tags.number)),
            ("String", TagSet::from(tags.string)),
            (
                "FormatString TemplateString",
                TagSet::from(tags.special.apply(tags.string)),
            ),
            ("Escape", TagSet::from(tags.escape)),
            ("UpdateOp", TagSet::from(tags.update_operator)),
            ("ArithOp", TagSet::from(tags.arithmetic_operator)),
            ("BitOp", TagSet::from(tags.bitwise_operator)),
            ("CompareOp", TagSet::from(tags.compare_operator)),
            ("AssignOp", TagSet::from(tags.definition_operator)),
            ("Ellipsis", TagSet::from(tags.punctuation)),
            ("At", TagSet::from(tags.meta)),
            ("( )", TagSet::from(tags.paren)),
            ("[ ]", TagSet::from(tags.square_bracket)),
            ("{ }", TagSet::from(tags.brace)),
            (".", TagSet::from(tags.deref_operator)),
            (", ;", TagSet::from(tags.separator)),
        ])
        .expect("Python highlight selectors are valid")
    }
    #[cfg(not(feature = "highlight"))]
    {
        rezel_common::group_prop().source(|_| None)
    }
}
