# rezel-lang-python

Python 3.14 parser, typed syntax, and owned AST for Rezel.

The default parser recovers from syntax errors and represents recovery points
with `⚠` nodes. Enable strict mode when invalid input must be rejected:

```rust
# fn main() -> Result<(), Box<dyn std::error::Error>> {
let source = "answer = 42\n";
let tree = rezel_lang_python::parser().parse(source)?;
let strict_tree = rezel_lang_python::parser()
    .with_strict(true)
    .parse(source)?;
assert_eq!(tree.to_string(), strict_tree.to_string());
# Ok(())
# }
```

The default top rule is `Module`. The named `Expression`, `Interactive`, and
`FunctionType` entry points correspond to the other public `ast.parse` modes.

Parsing accepts UTF-8 Rust strings and reports source coordinates as raw UTF-8
byte offsets. It does not decode byte streams or execute encoding cookies.

Generated typed syntax provides zero-copy direct-child access over the CST:

```rust
use rezel_lang_python::{PythonModule, TypedNode};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let source = "answer = 42\n";
# let tree = rezel_lang_python::parser().with_strict(true).parse(source)?;
let module = PythonModule::downcast_from(tree.top_node())
    .expect("the Python parser returns a Module top node");
assert_eq!(module.statements().count(), 1);
# Ok(())
# }
```

`PythonAst::lower` converts a strict tree into an owned arena whose node kinds,
fields, constants, and source ranges follow Python 3.14's public `ast` model:

```rust
use rezel_lang_python::ast::{PythonAst, PythonAstKind};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let source = "answer = 42\n";
# let tree = rezel_lang_python::parser().with_strict(true).parse(source)?;
let ast = PythonAst::lower(&tree, source)?;
assert_eq!(ast.root().kind(), PythonAstKind::Module);
# Ok(())
# }
```

The optional `highlight` Cargo feature exposes syntactic tag projection
through `highlight_spans`. It performs no binding, scope, type, or semantic
analysis.
