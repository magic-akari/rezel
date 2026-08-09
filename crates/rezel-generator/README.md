# rezel-generator

Grammar compiler and Rust artifact generator for Rezel parsers.

## Role in the system

The generator reads a Lezer-style grammar and produces the immutable inputs
consumed by `rezel-lr`. Use it directly when developing a language package,
integrating generation into a tool, or testing grammar diagnostics. Ordinary
parser users should depend on a generated `rezel-lang-*` package.

The generator, runtime, common crate, and generated parser package must use
compatible versions.

## Command-line interface

The `rezel` binary exposes three operations:

```console
rezel check grammar.grammar
rezel generate grammar.grammar --output generated.rs --terms terms.rs
rezel terms grammar.grammar --output terms.rs
```

### Workspace development profile

When repeatedly checking or generating a grammar in this workspace, prefer
the `generator-dev` aliases:

```console
cargo rezel check languages/json/src/json.grammar
cargo rezel-codegen json --check
cargo rezel-codegen json --update
```

The profile inherits the normal development settings and optimizes only
`rezel-generator`. This keeps grammar construction responsive without changing
the default profile used by tests and ordinary workspace builds. Continue to
use the normal `cargo test`, `cargo check`, and Clippy commands when developing
the generator itself, and use `--release` only for generator performance
measurements or release binaries.

The named profile is defined in the workspace `.cargo/config.toml`. Cargo reads
profiles from the workspace root and configuration files, not from a member
crate manifest, so it cannot live in `crates/rezel-generator/Cargo.toml`.

`generate` always writes two parser-table blobs next to its Rust output:
`generated.le.bin` and `generated.be.bin`. The Rust glue selects the
native-endian representation at compile time.

External grammar declarations resolve by module and symbol name:

```lezer
@external tokens TOKENS from "./tokens" { Word }
```

The Rust backend emits `crate::tokens::TOKENS`. Relative `.js`, `.mjs`, and
`.ts` suffixes are removed before resolving the crate module. Module and symbol
names must therefore be valid Rust identifiers and match the language adapter
exactly.

Zero-copy typed CST wrappers are generated from an independently checked TOML
schema:

```console
rezel generate language.grammar --output generated.rs \
  --typed language.typed.toml --typed-output typed.rs
```

Use `--include-names` when generated diagnostics and tree APIs need term names.
See [Adding a language](../../docs/adding-a-language/README.md) for the full
package command and maintained-output workflow.

## Compilation pipeline

Generation performs these operations:

1. parse and validate grammar declarations;
2. lower EBNF, inline rules, and normalize productions;
3. construct a canonical LR(1) automaton and conservatively merge compatible
   states;
4. resolve declared precedence, cuts, and explicit GLR ambiguity;
5. compile code-point token automata and lexical precedence;
6. resolve conventional Rust external paths and validate typed-schema coverage;
7. emit Rust glue, named terms, both endian table blobs, and typed wrappers.

The grammar notation follows Lezer's core design. The
[Rezel grammar guide](../../docs/adding-a-language/02-grammar-syntax.md) is the
self-contained project reference; the
[Lezer System Guide](https://lezer.codemirror.net/docs/guide/) provides deeper
background. Generator diagnostics and tests define the currently implemented
Rezel behavior.

## Library interface

The same pipeline is available without spawning the CLI:

```rust
use rezel_generator::{BuildOptions, compile_grammar, emit_rust};

let grammar = compile_grammar(
    r#"@top Document { word* } @tokens { word { @asciiLetter+ } }"#,
    Some("document.grammar"),
    BuildOptions::default(),
)?;
let generated = emit_rust(&grammar)?;

assert!(generated.parser.contains("LANGUAGE"));
assert!(!generated.little_endian_data.is_empty());
assert!(!generated.big_endian_data.is_empty());
# Ok::<(), Box<dyn std::error::Error>>(())
```

`emit_rust` returns parser source, terms source, and both byte vectors.
`emit_typed_syntax` validates a typed schema against the compiled grammar and
returns Rust source.

The generator does not currently emit JavaScript parser images, direct-code
parsers, IELR tables, or cross-edit reuse metadata.

## License

Licensed under either the Apache License 2.0 or the MIT license, at your option.
See `THIRD_PARTY_NOTICES.md` for upstream attribution.
