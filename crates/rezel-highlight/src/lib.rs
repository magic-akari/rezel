#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod highlight;
mod style;
mod tag;

pub use highlight::{HighlightSpan, StyledSpan, highlight_code, highlight_spans, highlight_tree};
pub use style::{
    Highlighter, ScopePredicate, SelectorError, StyleMatch, StyleMode, TagHighlighter,
    TagHighlighterOptions, TagStyle, class_highlighter, get_style_tags, style_tags,
    tag_highlighter,
};
pub use tag::{Modifier, StandardTags, Tag, TagSet, tags};
