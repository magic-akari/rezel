# rezel-common

Shared input, parser, tree, typed-syntax, and node-property interfaces for
Rezel runtimes and language packages.

## Role in the system

`rezel-common` defines the language-independent boundaries around parsing:

- immutable UTF-8 `Input` and original byte coordinates;
- `Parser`, `ParseRequest`, `PartialParse`, errors, and wrappers;
- compact `Tree`, `SyntaxNode`, `TreeCursor`, and `NodeType` APIs;
- typed-CST traits and child iterators;
- node properties, mounted trees, overlays, and mixed-language parsing.

Applications normally begin with a `rezel-lang-*` package. Depend on this crate
directly when implementing a parser/input adapter or consuming generic syntax
trees. Use the same `rezel-common` version as the language package and runtime.

## Source model

`Input` stores immutable UTF-8 text. `TextSize` and `TextRange` always measure
bytes in the original source. Their 32-bit coordinate space supports inputs
smaller than 4 GiB; larger inputs are rejected during input construction.

The parser's internal lexical view reads logical `CodePoint` values together
with their next original byte boundary. Ordinary input yields Unicode scalar
values. A language-owned translation layer may present another logical stream
while preserving the raw coordinate contract.

Line and column coordinates are downstream projections. They are not stored in
the CST and must not replace original byte ranges in a language adapter.

## Trees and typed syntax

Parsers produce compact concrete syntax trees. Cursors traverse visible
structure without constructing another tree:

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

Generated typed wrappers implement `SyntaxLanguage` and `TypedNode` over the
same `SyntaxNode` handles. `NodeProp` and `NodeSet` attach metadata without
making the generic tree language-specific.

`parse_mixed` can parse mounted sublanguages and selected overlay ranges. The
host and mounted trees retain their own language node sets while sharing the
common tree interface.

## Parse lifecycle

`Parser` creates a `PartialParse`; repeated `advance` calls resume one parse
until it returns a tree or error. Wrappers can add language-owned validation
around that lifecycle.

Reusing unchanged tree fragments across separate source edits is not currently
implemented. The resumable single-parse interface does not imply cross-edit
incremental parsing.

## License

Licensed under either the Apache License 2.0 or the MIT license, at your option.
See `THIRD_PARTY_NOTICES.md` for upstream attribution.
