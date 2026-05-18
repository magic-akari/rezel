# rezel-lang-java

Java SE 26 parser and typed syntax for Rezel.

The default parser recovers from syntax errors and represents recovery points
with `⚠` nodes. Enable strict mode when invalid input must be rejected:

```rust
# fn main() -> Result<(), Box<dyn std::error::Error>> {
let source = "class Sample {}";
let tree = rezel_lang_java::parser().parse(source)?;
let strict_tree = rezel_lang_java::parser()
    .with_strict(true)
    .parse(source)?;
assert_eq!(tree.to_string(), strict_tree.to_string());
# Ok(())
# }
```

All ranges address the original UTF-8 source. The parser applies the Unicode
escape translation required by the Java Language Specification while
preserving those original byte ranges. A malformed eligible Unicode escape is
reported as an input error before syntactic recovery.

Generated typed syntax provides zero-copy direct-child access over the CST:

```rust
use rezel_common::TypedNode;
use rezel_lang_java::typed::JavaProgram;

# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let source = "class Sample {}";
# let tree = rezel_lang_java::parser().with_strict(true).parse(source)?;
let program = JavaProgram::downcast_from(tree.top_node())
    .expect("the Java parser returns a Program top node");
assert!(program.compilation_unit().is_some());
# Ok(())
# }
```

`JavaAst::lower` converts a strict tree into an owned arena whose node kinds,
fields, properties, and source positions follow the public JDK compiler-tree
API. Comments remain available in the CST and typed syntax but are not part of
the lowered AST.

The optional `highlight` Cargo feature exposes syntactic tag projection
through `highlight_spans`. It performs no binding, scope, or semantic analysis.
