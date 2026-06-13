# rezel-lang-rust

Rust 1.95.0 parser and typed concrete syntax for Rezel.

The package starts from the pinned `@lezer/rust` 1.0.2 grammar and preserves
its CST and recovery behavior as a bootstrap baseline. The first maintained
Rust 1.95.0 / Edition 2024 delta adds let-else statements, let chains,
`if let` match guards, async closures, and inline const blocks.
The maintained lexical layer uses Rust 1.95's Unicode 17 identifier profile
and covers raw identifiers and lifetimes, C and raw C strings, literal escape
and radix validation, and Edition 2024 reserved guards and prefixes.

The parser does not yet claim complete Rust 1.95.0 language coverage. In
particular, unsafe extern items, precise capturing bounds, raw borrows,
source-file BOM and shebang removal, and the remaining grammar audit still
need to be aligned.

## Parsing

`SourceFile` is the package entry point. The default parser recovers from
syntax errors and records recovery points with `⚠` nodes. Enable strict mode
when invalid input must be rejected:

```rust
# fn main() -> Result<(), Box<dyn std::error::Error>> {
let source = "fn main() { let answer = 42; }";
let tree = rezel_lang_rust::parser().parse(source)?;
let strict_tree = rezel_lang_rust::parser()
    .with_strict(true)
    .parse(source)?;
assert_eq!(tree.to_string(), strict_tree.to_string());
# Ok(())
# }
```

The parser accepts UTF-8 Rust strings and reports original UTF-8 byte offsets.
Identifiers and lifetimes follow Unicode 17 `XID_Start` / `XID_Continue`.
Recovering mode preserves editor CSTs for invalid literal contents; strict
mode additionally enforces Rust's literal and reserved-token rules.

Maintained fixtures are checked against the pinned Rust 1.95.0 compiler in
Edition 2024 mode. The rustc snapshot is an acceptance oracle because stable
rustc does not expose a supported parse-only AST API. Concrete syntax trees are
compared separately with the pinned Lezer implementation.

## Typed CST

The initial typed schema exposes the root while the grammar stabilizes:

```rust
use rezel_lang_rust::{RustSourceFile, TypedNode};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let tree = rezel_lang_rust::parser()
    .with_strict(true)
    .parse("fn main() {}")?;
let file = RustSourceFile::downcast_from(tree.top_node())
    .expect("the Rust parser returns a SourceFile top node");
assert_eq!(file.text("fn main() {}"), Some("fn main() {}"));
# Ok(())
# }
```

The typed API is intentionally partial during the bootstrap phase. It remains
a zero-copy CST view and does not construct an owned Rust AST.

## Highlighting

The optional `highlight` Cargo feature exposes `highlight_spans`. It projects
the pinned Lezer syntax tags and performs no name resolution or type analysis.
