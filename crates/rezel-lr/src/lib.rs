#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod action_index;
mod decode;
mod goto_index;
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
    ExternalTokenizer, ExternalTokenizerStart, InputMark, InputStream, LocalTokenGroup,
    TokenAccept, TokenEdge, TokenEof, TokenGroup, TokenState, TokenTable, Tokenizer,
    TokenizerFlags,
};

/// Implementation details referenced by generated parser glue.
#[doc(hidden)]
pub mod __private {
    pub use zerocopy::include_value;
}
