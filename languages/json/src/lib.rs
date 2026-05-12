#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

use std::sync::OnceLock;

use rezel_common::NodePropSource;
use rezel_lr::LRParser;

#[rustfmt::skip]
mod generated;

/// Named grammar terms emitted by `rezel-generator`.
pub mod terms;

/// Return a cheaply cloned recovering JSON parser.
#[must_use]
pub fn parser() -> LRParser {
    default_parser().clone()
}

fn default_parser() -> &'static LRParser {
    static PARSER: OnceLock<LRParser> = OnceLock::new();
    PARSER.get_or_init(|| LRParser::from_language(&generated::LANGUAGE))
}

fn json_highlighting() -> NodePropSource {
    #[cfg(feature = "highlight")]
    {
        let tags = rezel_highlight::tags();
        rezel_highlight::style_tags([
            ("String", rezel_highlight::TagSet::from(tags.string)),
            ("Number", rezel_highlight::TagSet::from(tags.number)),
            ("True False", rezel_highlight::TagSet::from(tags.bool_)),
            (
                "PropertyName",
                rezel_highlight::TagSet::from(tags.property_name),
            ),
            ("Null", rezel_highlight::TagSet::from(tags.null)),
            (", :", rezel_highlight::TagSet::from(tags.separator)),
            ("[ ]", rezel_highlight::TagSet::from(tags.square_bracket)),
            ("{ }", rezel_highlight::TagSet::from(tags.brace)),
        ])
        .expect("the pinned JSON highlight selectors are valid")
    }

    #[cfg(not(feature = "highlight"))]
    {
        rezel_common::group_prop().source(|_| None)
    }
}
