#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod mix;
mod parse;
mod prop;
mod tree;
mod typed;

pub use text_size::{TextRange, TextSize};

pub use mix::{MixedParseSpec, NestedParse, Overlay, OverlayMatch, parse_mixed};
pub use parse::{
    Input, InputChunk, LogicalUnits, ParseError, ParseErrorKind, ParseRequest, ParseWrapper,
    Parser, PartialParse, StringInput,
};
pub use prop::{
    NodeProp, NodePropConfig, NodePropDeserializer, NodePropSource, PropertyError, PropertyValue,
    closed_by_prop, group_prop, isolate_prop, mounted_prop, opened_by_prop,
};
pub use tree::{
    DEFAULT_BUFFER_LENGTH, FlatPostfixCursor, IterMode, MountedTree, NodeFlags, NodeSet, NodeType,
    PostfixBuffer, PostfixCursor, SyntaxChildren, SyntaxNode, Tree, TreeBuffer, TreeBuild,
    TreeChild, TreeCursor,
};
pub use typed::{SyntaxLanguage, TypedChildren, TypedNode};
