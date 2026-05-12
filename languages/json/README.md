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

The default feature set contains only parsing. Enable the optional `highlight`
feature to attach the upstream `jsonHighlighting` property source and use
`rezel-highlight` to project syntactic tags.
