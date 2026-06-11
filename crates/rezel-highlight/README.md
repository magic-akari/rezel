# rezel-highlight

Optional syntactic highlighting for Rezel concrete syntax trees.

The crate provides abstract tags, `styleTags`-compatible selectors, tag
highlighters, range filtering, and mount-aware tree traversal. Language
packages attach tag sets to node kinds through property sources and may expose
highlight projection behind an opt-in `highlight` Cargo feature.

Highlighting is a CST projection. It classifies syntactic roles such as
keywords, literals, comments, delimiters, and grammar-defined names. It does
not perform name resolution, scope analysis, type checking, or other semantic
classification. Downstream tools map the abstract tags to an editor theme or
token legend and may layer semantic results on top.

Parser crates do not require this crate. The implementation is based on the
non-semantic tree-highlighting model of `@lezer/highlight` and uses Rezel's
generic node properties and mounted-tree traversal.

## License

Licensed under either the Apache License 2.0 or the MIT license, at your option.
See `THIRD_PARTY_NOTICES.md` for upstream attribution.
