#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

use std::sync::OnceLock;

use rezel_lr::LRParser;

#[rustfmt::skip]
mod generated;
#[cfg(test)]
mod syntax;
mod tokens;
#[rustfmt::skip]
mod typed;

/// Named grammar terms emitted by `rezel-generator`.
pub mod terms;

pub use rezel_common::TypedNode;
pub use typed::{
    SwiftCodeBlock, SwiftCodeBlockItem, SwiftDeclaration, SwiftDollarIdentifier,
    SwiftFunctionDeclaration, SwiftFunctionName, SwiftFunctionParameter,
    SwiftFunctionParameterClause, SwiftIdentifier, SwiftImportDeclaration, SwiftIntegerLiteral,
    SwiftKind, SwiftLanguage, SwiftMemberBlock, SwiftPatternBinding, SwiftReturnClause,
    SwiftSourceFile, SwiftStringLiteral, SwiftStructDeclaration, SwiftTypeName,
    SwiftVariableDeclaration,
};

/// Swift concrete-syntax parser.
pub type SwiftParser = LRParser;

/// Return a cheaply cloned recovering Swift parser.
#[must_use]
pub fn parser() -> SwiftParser {
    default_parser().clone()
}

fn default_parser() -> &'static LRParser {
    static PARSER: OnceLock<LRParser> = OnceLock::new();
    PARSER.get_or_init(|| LRParser::from_language(&generated::LANGUAGE))
}
