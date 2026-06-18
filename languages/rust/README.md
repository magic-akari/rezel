# rezel-lang-rust

Rust 1.95.0 parser and typed concrete syntax for Rezel.

The package starts from the pinned `@lezer/rust` 1.0.2 grammar and preserves
its CST and recovery behavior as a bootstrap baseline. The first maintained
Rust 1.95.0 / Edition 2024 delta adds let-else statements, let chains,
`if let` match guards, async closures, and inline const blocks.
The maintained lexical layer uses Rust 1.95's Unicode 17 identifier profile
and covers raw identifiers and lifetimes, C and raw C strings, literal escape
and radix validation, Edition 2024 reserved guards and prefixes, and the full
Rust `Pattern_White_Space` set. The source-file input view removes an optional
leading UTF-8 byte order mark and shebang before tokenization while retaining
original UTF-8 byte coordinates.
The maintained grammar also covers Edition 2024 unsafe extern blocks and
foreign-item safety qualifiers, raw borrow expressions, and precise capturing
`use<...>` bounds.

The parser does not yet claim complete Rust 1.95.0 language coverage. The
remaining work is a Reference-driven grammar and negative-conformance audit;
the standard-library corpus is broad positive evidence, not a substitute for
that audit.

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
`SourceFile` applies Rust's leading BOM and shebang rules before tokenization.
Recovering mode preserves editor CSTs for invalid literal contents; strict
mode additionally enforces Rust's literal and reserved-token rules.

The Rust Reference defines language membership. Small maintained cases cover
the local grammar contract, selected cases are compared with the pinned Rust
1.95.0 compiler in Edition 2024 mode, and concrete syntax trees are compared
separately with the pinned Lezer implementation. The rustc runner emits
metadata because stable rustc does not expose a supported parse-only AST API;
its result is implementation evidence rather than the parser's sole
specification.

Non-obvious implementation behavior is cross-checked against
`rust-lang/rust` revision
`59807616e1fa2540724bfbac14d7976d7e4a3860` (tag `1.95.0`).
Relevant entry points are `rustc_parse::parser::item::parse_item_kind` and
`is_macro_rules_item`, `parse_foreign_item`,
`rustc_parse::parser::expr::parse_borrow_modifiers`,
`rustc_parse::parser::ty::parse_use_bound`, and
`rustc_ast_passes::ast_validation::AstValidator::walk_ty`. Rezel normalizes
those decisions into LR productions and strict CST validation; it does not
transcribe rustc's recursive-descent control flow or recovery diagnostics.
In particular, `macro_rules` uses contextual tokenization with finite
token-level lookahead for `!` and the definition name. That keeps the LR
grammar deterministic at this boundary while preserving `macro_rules! {}`
as an ordinary same-named macro invocation.

## Standard-library corpus

`mise run reference:stdlib:rust` recursively parses every `.rs` file under the
`rust-src` standard-library `library` tree from Rust 1.95.0 revision
`59807616e1fa2540724bfbac14d7976d7e4a3860`. The pinned inventory contains
1,964 files, has no exclusions, uses the complete `SourceFile` entry point in
strict mode with the parser's default resource limits, and expects every file
to be accepted with no recovery or known-rejection allowance.

The runner verifies both the toolchain release and commit before collecting
source, then verifies the exact file count. This makes corpus drift explicit.
It establishes practical coverage over the Rust 1.95 standard library,
including large macro token trees, but it does not establish rejection
behavior, semantic validity, Edition-specific name resolution, or syntax that
the standard library does not exercise.

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
