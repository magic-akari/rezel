# Rezel

**Parsing infrastructure for language tools in Rust.**

Rezel provides compact syntax trees, error recovery, original UTF-8 byte ranges,
and typed syntax views. Use an existing language package or generate a Rust
parser from a Lezer-style grammar.

Under active development; not ready for production use. APIs and language
coverage may change. Cross-edit incremental parsing is not yet implemented.

## Languages

[Go](languages/go/README.md) · [Java](languages/java/README.md) ·
[JSON](languages/json/README.md) · [Kotlin](languages/kotlin/README.md) ·
[PHP](languages/php/README.md) · [Python](languages/python/README.md) ·
[Rust](languages/rust/README.md) · [Swift](languages/swift/README.md)

Typed syntax coverage and owned AST availability vary by package. Each offers
optional syntactic highlighting. See [language capabilities](docs/languages.md).

## Learn more

- [Build your own parser](docs/adding-a-language/README.md)
- [Architecture](docs/architecture.md) — based on Lezer, with selected ideas from
  tree-sitter.
- [Documentation](docs/README.md)
- [Contributing](CONTRIBUTING.md) — setup, development, and verification.

## License

MIT or Apache-2.0, at your option.
