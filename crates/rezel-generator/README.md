# rezel-generator

Grammar compiler and Rust source generator for Rezel parsers.

## When to depend on this crate

Most applications should depend on a published `rezel-lang-*` parser crate.
Use `rezel-generator` directly when you are:

- compiling a Lezer grammar into Rust during parser development;
- integrating grammar compilation into a build or release tool;
- inspecting grammar diagnostics or generated term identifiers.

Generated parser crates must use matching versions of `rezel-generator`,
`rezel-lr`, and `rezel-common`.

## Command-line interface

The `rezel` binary exposes three operations:

```console
rezel check grammar.grammar
rezel generate grammar.grammar --output parser.rs --terms terms.rs
rezel terms grammar.grammar --output terms.rs
```

`generate` emits deterministic Rust source containing static LR tables and a
`rezel_lr::Language`. Grammar-declared external tokenizers, specializers,
context trackers, and node properties are linked with repeated
`--binding SOURCE:NAME=RUST_PATH` options.

## Library interface

The same pipeline is available without spawning the CLI:

```rust
use rezel_generator::{BuildOptions, RustBindings, compile_grammar, emit_rust};

let grammar = compile_grammar(
    r#"@top Document { word* } @tokens { word { @asciiLetter+ } }"#,
    Some("document.grammar"),
    BuildOptions::default(),
)?;
let generated = emit_rust(&grammar, &RustBindings::default())?;

assert!(generated.parser.contains("LANGUAGE"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

Compilation constructs a canonical LR(1) automaton, conservatively merges
compatible states, compiles token automata, and produces the metadata consumed
by `rezel-lr`.

## Current limits

The generator emits static Rust source only. JavaScript parser images,
direct-code parsers, IELR tables, and incremental-reuse metadata are not
currently generated.

## License

Licensed under either the Apache License 2.0 or the MIT license, at your option.
See `THIRD_PARTY_NOTICES.md` for upstream attribution.
