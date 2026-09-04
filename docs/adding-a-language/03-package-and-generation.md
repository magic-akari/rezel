# 3. Create the package and generate artifacts

A language package owns the maintained inputs and the generated artifacts for
one language contract. The generic crates do not discover grammars at runtime:
the package compiles a grammar ahead of time and links immutable Rust
configuration and native-endian parser tables into the program.

## Register the package

Create `languages/<language>`. The root [`Cargo.toml`](../../Cargo.toml) includes
`languages/*`, so a package in that directory becomes a workspace member
without another registry entry. A parser package normally starts with:

```toml
[package]
name = "rezel-lang-<language>"
version.workspace = true
authors.workspace = true
edition.workspace = true
rust-version.workspace = true
homepage.workspace = true
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
  src/
    <language>.grammar
    <language>.typed.toml
    highlighting.rs
    lib.rs
    generated.rs
    generated.le.bin
    generated.be.bin
    terms.rs
    typed.rs
  tests/
    contract.rs
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

## Register centralized generation

Checked-in parser artifacts are owned by the repository-level
[`rezel-codegen`](../../tools/codegen/src/main.rs) workflow, not by a duplicate
test inside each language crate. The tool discovers every directory under
`languages/` and requires `Cargo.toml`, `src/<language>.grammar`, and
`src/<language>.typed.toml`. A missing maintained input fails discovery instead
of silently omitting the package from the repository gate.

The conventional scope keeps its maintained inputs and generated outputs under
`languages/<language>/src/` and performs the common five-artifact generation
without a language-name registry. If a package needs additional generated
files, extend `rezel-codegen` so check and update mode continue to share one
declaration of expected output.

No per-language `mise.toml` entry is required. The generic scope task keeps the
narrow check and update commands available, while the all-language task is the
one used by `mise run verify`:

```sh
mise run codegen:rezel:scope <language> --check
mise run codegen:rezel:scope <language> --update
mise run codegen:rezel
```

## Generate all parser artifacts

After creating a package that satisfies the discovery contract, generate its
artifacts through the update task:

```sh
mise run codegen:rezel:scope <language> --update
```

This writes:

| Output             | Role                                                                    |
| ------------------ | ----------------------------------------------------------------------- |
| `generated.rs`     | Static language structure, callback references, and parser-table loader |
| `generated.le.bin` | Parser tables encoded for little-endian targets                         |
| `generated.be.bin` | Parser tables encoded for big-endian targets                            |
| `terms.rs`         | Stable numeric constants for named or exported grammar terms            |
| `typed.rs`         | Typed CST kinds, wrappers, unions, and direct-child accessors           |

The standard language scope fixes this contract: it reads
`src/<language>.grammar` and `src/<language>.typed.toml`, compiles with term
names enabled, and always emits all five files under `src/` with the names
shown above. These are not optional CLI choices in the repository workflow.
Change the central scope explicitly if a future package needs a different
artifact contract.

The Rust library API exposes the same separation. `compile_grammar` produces a
checked grammar, `emit_rust` resolves relative external modules by convention
and returns parser source, terms, and both byte vectors, and
`emit_typed_syntax` validates and emits the typed schema.

Generated files are committed so ordinary users do not need the generator.
At compile time, `generated.rs` selects the target's native-endian blob and
uses `zerocopy` to validate its layout. Runtime parser construction borrows the
static tables; it does not decode a portable blob or allocate a second table
copy. Construction does build compact auxiliary lookup indexes, so cache the
immutable parser in the package facade as shown below.

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

pub type ExampleParser = LRParser;

#[must_use]
pub fn parser() -> ExampleParser {
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

## Check generation centrally

Run the paired check task after generation and before handoff:

```sh
mise run codegen:rezel:scope <language> --check
```

In check mode, `rezel-codegen` compiles the checked-in grammar, validates the
bindings and typed schema, reconstructs the declared artifacts, and compares
their bytes with the working tree. The language scope must account for parser
source, named terms, little- and big-endian table blobs, and typed source. The
check reports stale or missing files and generated Rust source that the scope
owns but no longer declares; update mode applies the same expected set.

For orphan detection, a language scope inspects direct children of its `src/`
directory. It owns an extra file only when it is an `.rs` file whose generated
marker and regeneration-command line name that scope's update task. Extra old
binary files are not inferred as orphans, so removing or renaming a declared
binary output requires an explicit reviewed deletion.

Do not add a package-local `generated` test or a language-crate development
dependency on `rezel-generator`. That would duplicate the central comparison
and allow the package test and repository gate to drift apart. Add or change an
output by teaching `rezel-codegen` about it; automatic discovery keeps the
all-language check in `mise run verify`.

This centralized check proves reproducible emission. Parser contract and
reference tests remain responsible for behavioral correctness.
