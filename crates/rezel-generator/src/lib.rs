#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod automaton;
mod build;
mod emit;
mod error;
mod grammar;
mod node;
mod parse;
mod source;
mod token;

pub use build::{
    BuildOptions, CompiledGrammar, ContextMetadata, ExternalPropertyMetadata, NodeMetadata,
    PropertySourceMetadata, SpecializedTokenMetadata, SpecializerMetadata, TokenizerMetadata,
    TopRuleMetadata, compile_grammar,
};
pub use emit::{GeneratedRust, RustBindings, emit_rust, emit_terms};
pub use error::{GeneratorError, GeneratorWarning};
pub use node::*;
pub use parse::parse_grammar;
