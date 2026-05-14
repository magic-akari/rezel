# rezel-lang-json

JSON reference language for Rezel, generated from the unchanged
`@lezer/json` 1.0.3 grammar.

The crate exposes one cheap-to-clone recovering parser over static Rust
tables. Configure a clone for strict parsing when required:

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

The generic Rezel CST is always available. The crate also exposes zero-copy
typed syntax wrappers such as `JsonRoot`, `JsonObject`, `JsonArray`, and
`JsonValue`. Import the re-exported `TypedNode` trait to use `downcast_from`,
`syntax`, `into_syntax`, and `text`.

Typed syntax is a view over the same CST rather than an owned semantic JSON
model. It does not depend on the optional `highlight` feature, which attaches
the upstream `jsonHighlighting` property source for use with `rezel-highlight`.
