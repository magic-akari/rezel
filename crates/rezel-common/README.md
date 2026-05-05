# rezel-common

Shared syntax-tree and parser interfaces for Rezel runtimes, generated parsers,
and language crates.

## When to depend on this crate

Start with a `rezel-lang-*` crate when parsing a supported language. Add a
direct dependency on `rezel-common` when your code needs to:

- inspect generic `Tree`, `SyntaxNode`, `TreeCursor`, or `NodeType` values;
- implement an `Input`, `Parser`, or `PartialParse` adapter;
- define or consume node properties;
- integrate mounted trees, overlays, or mixed-language parsing.

Language crates use these types in their public APIs without re-exporting the
entire common API. Use the same `rezel-common` version as the language crate.

## Tree traversal

Parsers produce compact concrete syntax trees. A cursor provides iterative
navigation over visible nodes:

```rust
use rezel_common::{IterMode, Tree};

fn node_names(tree: &Tree) -> Vec<String> {
    let mut cursor = tree.cursor(IterMode::NONE);
    let mut names = Vec::new();

    loop {
        names.push(cursor.name().to_string());
        if !cursor.next(true) {
            break;
        }
    }

    names
}
```

`NodeProp` and `NodeSet` attach language-specific metadata without changing the
generic tree representation. `parse_mixed` supports parsers that mount nested
trees or parse selected overlay ranges with another language.

## Source positions

All source positions and ranges are UTF-8 byte offsets represented by the
re-exported [`TextSize`](https://docs.rs/text-size/1.1.1/text_size/struct.TextSize.html)
and [`TextRange`](https://docs.rs/text-size/1.1.1/text_size/struct.TextRange.html)
types. Their 32-bit coordinate space supports inputs smaller than 4 GiB;
larger inputs are rejected when constructing parser input.

## Current limits

This crate does not provide a language grammar by itself. Incremental fragments,
changed-range reporting, weak node maps, and cross-parse node reuse are not
currently supported.

## License

Licensed under either the Apache License 2.0 or the MIT license, at your option.
See `THIRD_PARTY_NOTICES.md` for upstream attribution.
