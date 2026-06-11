# rezel-lang-json

JSON parser and zero-copy typed syntax for Rezel.

The grammar is derived from the pinned `@lezer/json` grammar identified in
`THIRD_PARTY_NOTICES.md`. Strict acceptance and resource behavior are also
tested against `JSONTestSuite`. The package has one `JsonText` entry point.

## Parsing

The default parser recovers from syntax errors and records recovery points with
`⚠` nodes. Enable strict mode when invalid JSON must be rejected:

```rust
# fn main() -> Result<(), Box<dyn std::error::Error>> {
let source = r#"{"answer": 42}"#;
let recovered = rezel_lang_json::parser().parse(source)?;
let strict = rezel_lang_json::parser()
    .with_strict(true)
    .parse(source)?;
assert_eq!(recovered.to_string(), strict.to_string());
# Ok(())
# }
```

All positions are byte offsets in the original UTF-8 Rust string. Parser
configuration is cheap to clone and shares immutable generated tables.

## Typed CST

The generic CST is always available. Generated wrappers such as `JsonRoot`,
`JsonObject`, `JsonArray`, and `JsonValue` provide zero-copy direct-child
navigation over the same tree. Import the re-exported `TypedNode` trait for
downcasting and source-backed node access.

Typed syntax does not decode an owned JSON value. Consumers that need a value
model can project one from a strict tree while retaining the CST for concrete
syntax and ranges.

## Highlighting

The optional `highlight` Cargo feature attaches JSON syntactic tags for use
with `rezel-highlight`. It classifies strings, numbers, property names,
literals, separators, and delimiters without semantic analysis.
