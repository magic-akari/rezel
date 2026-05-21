#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

use std::sync::OnceLock;

use rezel_common::NodePropSource;
use rezel_lr::LRParser;

#[cfg(feature = "highlight")]
use rezel_common::{TextRange, Tree};
#[cfg(feature = "highlight")]
pub use rezel_highlight::HighlightSpan;
#[cfg(feature = "highlight")]
use rezel_highlight::TagSet;

#[rustfmt::skip]
mod generated;
pub mod ast;
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

fn go_highlighting() -> NodePropSource {
    #[cfg(feature = "highlight")]
    {
        let tags = rezel_highlight::tags();
        rezel_highlight::style_tags([
            (
                "func interface struct chan map const type var",
                TagSet::from(tags.definition_keyword),
            ),
            ("import package", TagSet::from(tags.module_keyword)),
            (
                "switch for go select return break continue goto fallthrough case if else defer",
                TagSet::from(tags.control_keyword),
            ),
            ("range", TagSet::from(tags.keyword)),
            ("Bool", TagSet::from(tags.bool_)),
            ("String", TagSet::from(tags.string)),
            ("Rune", TagSet::from(tags.character)),
            ("Number", TagSet::from(tags.number)),
            ("Nil", TagSet::from(tags.null)),
            ("VariableName", TagSet::from(tags.variable_name)),
            (
                "DefName",
                TagSet::from(tags.definition.apply(tags.variable_name)),
            ),
            ("TypeName", TagSet::from(tags.type_name)),
            ("LabelName", TagSet::from(tags.label_name)),
            ("FieldName", TagSet::from(tags.property_name)),
            (
                "FunctionDecl/DefName",
                TagSet::from(
                    tags.function
                        .apply(tags.definition.apply(tags.variable_name)),
                ),
            ),
            (
                "TypeSpec/DefName",
                TagSet::from(tags.definition.apply(tags.type_name)),
            ),
            (
                "CallExpr/VariableName",
                TagSet::from(tags.function.apply(tags.variable_name)),
            ),
            ("LineComment", TagSet::from(tags.line_comment)),
            ("BlockComment", TagSet::from(tags.block_comment)),
            ("LogicOp", TagSet::from(tags.logic_operator)),
            ("ArithOp", TagSet::from(tags.arithmetic_operator)),
            ("BitOp", TagSet::from(tags.bitwise_operator)),
            ("DerefOp .", TagSet::from(tags.deref_operator)),
            ("UpdateOp IncDecOp", TagSet::from(tags.update_operator)),
            ("CompareOp", TagSet::from(tags.compare_operator)),
            ("= :=", TagSet::from(tags.definition_operator)),
            ("<-", TagSet::from(tags.operator)),
            ("~ \"*\"", TagSet::from(tags.modifier)),
            ("; ,", TagSet::from(tags.separator)),
            ("... :", TagSet::from(tags.punctuation)),
            ("( )", TagSet::from(tags.paren)),
            ("[ ]", TagSet::from(tags.square_bracket)),
            ("{ }", TagSet::from(tags.brace)),
        ])
        .expect("the pinned Go highlight selectors are valid")
    }

    #[cfg(not(feature = "highlight"))]
    {
        rezel_common::group_prop().source(|_| None)
    }
}
