# rezel-lang-go

Go 1.26 parser, typed syntax, and owned AST for Rezel.

The default parser recovers from syntax errors and represents recovery points
with `⚠` nodes. Enable strict mode when invalid input must be rejected:

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

Generated typed syntax provides zero-copy direct-child access over the CST:

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

`GoAst::lower` converts a strict tree into a compact owned arena whose public
node kinds, fields, tokens, strings, and source positions follow Go's
`go/ast` and `go/token` syntax model:

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

The optional `highlight` Cargo feature exposes syntactic tag projection
through `highlight_spans`. It performs no binding, type checking, or semantic
analysis.
