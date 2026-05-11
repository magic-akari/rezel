# rezel-highlight

Optional syntactic highlighting for Rezel syntax trees.

This crate rewrites the non-semantic behavior of `@lezer/highlight` 1.2.3 in
Rust. It provides abstract tags, `styleTags`-compatible selectors, tag
highlighters, and mount-aware tree traversal. It does not perform name
resolution, semantic classification, or incremental highlight reuse.

Parser crates do not depend on this crate. Language crates may expose it
behind an opt-in `highlight` Cargo feature.
