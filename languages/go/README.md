# rezel-lang-go

Go 1.26 parser, typed syntax, and owned AST for Rezel.

The grammar is derived from a pinned Lezer grammar and updated to the package's
Go language contract. Strict acceptance and the owned AST are compared with
Go's standard parser over focused cases and standard-library source. External
Rust tokenization implements automatic semicolon insertion and its parser
context.

## Parsing

`SourceFile` is the package entry point. The default parser recovers from
syntax errors and records recovery points with `⚠` nodes. Enable strict mode
when invalid input must be rejected:

```rust
# fn main() -> Result<(), Box<dyn std::error::Error>> {
let source = "package sample\n";
let tree = rezel_lang_go::parser().parse(source)?;
let strict_tree = rezel_lang_go::parser()
    .with_strict(true)
    .parse(source)?;
assert_eq!(tree.to_string(), strict_tree.to_string());
# Ok(())
# }
```

All source ranges are original UTF-8 byte offsets. Parser clones share the
immutable generated language and native-endian tables.
Recovering identifiers use a broad scalar candidate range; strict parsing
validates the selected base token with the `unicode-ident` XID profile before
its LR action. Validation remains keyed to the base identifier even when the
parser-visible token is a specialized keyword.

## Typed CST

Generated typed syntax provides zero-copy direct-child access:

```rust
use rezel_lang_go::{GoSourceFile, TypedNode};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let source = "package sample\n";
# let tree = rezel_lang_go::parser().with_strict(true).parse(source)?;
let file = GoSourceFile::downcast_from(tree.top_node())
    .expect("the Go parser returns a SourceFile top node");
assert_eq!(
    file.package_clause()
        .and_then(|package| package.name())
        .and_then(|name| name.text(source)),
    Some("sample"),
);
# Ok(())
# }
```

The typed API remains backed by the CST and can also be used with recovery
trees through optional accessors.

## Owned AST

`GoAst::lower` converts a strict tree into a compact owned arena. Its public
node kinds, fields, token values, strings, and ranges follow Go's `go/ast` and
`go/token` syntax model:

```rust
use rezel_lang_go::ast::{GoAst, GoAstKind};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let source = "package sample\nvar answer = 42\n";
# let tree = rezel_lang_go::parser().with_strict(true).parse(source)?;
let ast = GoAst::lower(&tree, source)?;
assert_eq!(ast.root().kind(), GoAstKind::File);
# Ok(())
# }
```

Lowering rejects recovery trees. The AST is a projection rather than a
replacement for the concrete tree.

## Highlighting

The optional `highlight` Cargo feature exposes `highlight_spans`. It projects
syntactic tags and performs no binding, type checking, or semantic analysis.
