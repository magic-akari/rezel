#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod action_index;
mod decode;
mod parse;
mod stack;
mod token;

/// Static-table encoding shared with generated parser code.
#[doc(hidden)]
#[path = "constants.rs"]
pub mod table;

pub use parse::{
    ContextTracker, ContextValue, Dialect, DialectSpec, ExternalSpecializer, LRParser, Language,
    ParseLimits, Specialize, SpecializedToken, SpecializerSpec, TopRule,
};
pub use stack::Stack;
pub use token::{
    ExternalTokenizer, InputStream, LocalTokenGroup, TokenGroup, Tokenizer, TokenizerFlags,
};
