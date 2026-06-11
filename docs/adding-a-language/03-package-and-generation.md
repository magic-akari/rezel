# 3. Create the package and generate artifacts

A language package owns the maintained inputs and the generated artifacts for
one language contract. The generic crates do not discover grammars at runtime:
the package compiles a grammar ahead of time and links immutable Rust
configuration and native-endian parser tables into the program.

## Register the package

Add `languages/<language>` to the workspace members in the root
[`Cargo.toml`](../../Cargo.toml). A parser package normally starts with:

```toml
[package]
name = "rezel-lang-<language>"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true

[features]
default = []
highlight = ["dep:rezel-highlight"]

[dependencies]
rezel-common.workspace = true
rezel-highlight = { workspace = true, optional = true }
rezel-lr.workspace = true
zerocopy.workspace = true

[dev-dependencies]
rezel-generator.workspace = true

[lints]
workspace = true
```

`zerocopy` is required by the generated parser-table loader. Keep highlighting
optional. Add serialization or language-specific dependencies only for
features and tests that use them.

Include the grammar, generated source, parser-table blobs, README, notices, and
licenses in the published package. If grammar or adapter code derives from
another project, preserve its license and attribution in
`THIRD_PARTY_NOTICES.md`.

## Package layout

A full parser and typed-CST package has this shape:

```text
languages/<language>/
  Cargo.toml
  README.md
  THIRD_PARTY_NOTICES.md
  grammar/
    <language>.grammar
    <language>.bindings.toml
    <language>.typed.toml
  src/
    lib.rs
    generated.rs
    generated.le.bin
    generated.be.bin
    terms.rs
    typed.rs
  tests/
    contract.rs
    generated.rs
    parse.rs
    typed.rs
```

Only add files required by the language model:

- `src/input.rs` for a translated lexical view;
- `src/tokens.rs` or `src/identifier.rs` for external tokenizers;
- `src/syntax/` for private CST interpretation and strict validation;
- `src/ast/` for an owned AST;
- highlight tests and reference runners for their corresponding contracts.

Current language packages are examples of these components, not templates that
must all be copied. Begin with the smallest package that satisfies the new
contract.

## Generate all parser artifacts

The repository-local `rezel` binary accepts the grammar and the two declarative
side inputs:

```sh
cargo run --locked -p rezel-generator -- generate \
  languages/<language>/grammar/<language>.grammar \
  --output languages/<language>/src/generated.rs \
  --terms languages/<language>/src/terms.rs \
  --include-names \
  --bindings languages/<language>/grammar/<language>.bindings.toml \
  --typed languages/<language>/grammar/<language>.typed.toml \
  --typed-output languages/<language>/src/typed.rs
```

This writes:

| Output             | Role                                                                    |
| ------------------ | ----------------------------------------------------------------------- |
| `generated.rs`     | Static language structure, callback references, and parser-table loader |
| `generated.le.bin` | Parser tables encoded for little-endian targets                         |
| `generated.be.bin` | Parser tables encoded for big-endian targets                            |
| `terms.rs`         | Stable numeric constants for named or exported grammar terms            |
| `typed.rs`         | Typed CST kinds, wrappers, unions, and direct-child accessors           |

The two binary file names are derived from `--output`; they are always emitted.
`--terms` is optional. `--typed` and `--typed-output` must appear together.
`--include-names` retains term names used by diagnostics and tree-facing APIs.

The Rust library API exposes the same separation. `compile_grammar` produces a
checked grammar, `RustBindings::from_toml_str` reads the binding manifest,
`emit_rust` returns parser source, terms, and both byte vectors, and
`emit_typed_syntax` validates and emits the typed schema.

Generated files are committed so ordinary users do not need the generator.
At compile time, `generated.rs` selects the target's native-endian blob and
uses `zerocopy` to validate its layout. Runtime parser construction borrows the
static tables; it does not decode a portable blob or allocate a second table
representation.

## Expose a language facade

For a direct language, cache the immutable `LRParser` and return cheap clones:

```rust
use std::sync::OnceLock;

use rezel_lr::LRParser;

#[rustfmt::skip]
mod generated;
#[rustfmt::skip]
mod typed;
pub mod terms;

pub use rezel_common::TypedNode;
pub use typed::*;

#[must_use]
pub fn parser() -> LRParser {
    static PARSER: OnceLock<LRParser> = OnceLock::new();
    PARSER
        .get_or_init(|| LRParser::from_language(&generated::LANGUAGE))
        .clone()
}
```

When parsing requires a translated lexical input, preflight checks, or strict
post-validation, give the cached `LRParser` one language-owned parse function.
That function prepares the `ParseRequest`, calls `create_lr_parse`, and wraps
the returned `PartialParse` only when post-validation is required. Keep this
control flow in the language crate. Every package still returns `LRParser`
directly and therefore exposes the same configuration and query surface.

Do not introduce a language-specific parser newtype solely to intercept
`create_parse`. Such a type must manually forward every current and future
`LRParser` method and will eventually narrow the public API.

Re-export the typed root, unions, node wrappers, and named terms needed by
consumers. Keep the raw generated module private unless exposing it is part of
an intentional low-level API.

## Make generation reproducible

Add a generated-output test with the first parser commit. It should compile the
checked-in grammar, load and validate the checked-in bindings, emit all
artifacts, and compare them byte-for-byte:

```rust
let grammar = compile_grammar(
    GRAMMAR,
    Some("grammar/<language>.grammar"),
    BuildOptions { include_names: true },
)?;
let bindings = RustBindings::from_toml_str(BINDINGS)?;
let generated = emit_rust(&grammar, &bindings)?;

assert_eq!(generated.parser, include_str!("../src/generated.rs"));
assert_eq!(generated.terms, include_str!("../src/terms.rs"));
assert_eq!(
    generated.little_endian_data,
    include_bytes!("../src/generated.le.bin")
);
assert_eq!(
    generated.big_endian_data,
    include_bytes!("../src/generated.be.bin")
);

let typed = emit_typed_syntax(&grammar, TYPED_SCHEMA)?;
assert_eq!(typed, include_str!("../src/typed.rs"));
```

Adapt error handling to the test, but keep all five comparisons. Run it with:

```sh
cargo test --locked -p rezel-lang-<language> --test generated
```

This test catches stale output and nondeterministic emission. Parser contract
and reference tests remain responsible for behavioral correctness.
