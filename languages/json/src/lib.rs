#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

use std::sync::OnceLock;

use rezel_lr::LRParser;

#[rustfmt::skip]
mod generated;
mod highlighting;
#[rustfmt::skip]
mod typed;

/// Named grammar terms emitted by `rezel-generator`.
pub mod terms;

pub use rezel_common::TypedNode;
pub use typed::{
    JsonArray, JsonBoolean, JsonFalse, JsonKind, JsonLanguage, JsonNull, JsonNumber, JsonObject,
    JsonProperty, JsonPropertyName, JsonRoot, JsonString, JsonTrue, JsonValue,
};

/// Return a cheaply cloned recovering JSON parser.
#[must_use]
pub fn parser() -> LRParser {
    default_parser().clone()
}

fn default_parser() -> &'static LRParser {
    static PARSER: OnceLock<LRParser> = OnceLock::new();
    PARSER.get_or_init(|| LRParser::from_language(&generated::LANGUAGE))
}
