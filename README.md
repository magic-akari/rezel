# Rezel

> [!WARNING]
> Rezel is under active development. It is not ready for production use, and
> its APIs, generated artifacts, and language coverage may change.

Rezel is a Rust parser generator and runtime for programming-language tooling.
It is a rewrite based on Lezer's core grammar, LR/GLR, recovery, and compact
tree design, with selected ideas informed by tree-sitter. Rezel generates
language-independent Rust parser configuration and keeps language-specific
tokenization, source translation, validation, and projections in language
packages.

A language grammar and typed schema are compiled into Rust glue, named terms,
little- and big-endian parser-table blobs, and typed CST wrappers. Grammar
external modules resolve directly to same-named Rust modules and items. At
runtime, UTF-8 source is read as logical code points with original byte
boundaries, tokenized, parsed by the LR/GLR engine, and represented as a compact
CST. Language packages may additionally expose an owned AST and syntactic
highlighting.

The repository currently contains several language packages that exercise
different integration patterns. They are the present implementation set, not
the intended limit of the project. New languages normally begin with a
maintained Lezer grammar when one exists, then fix defects and update it to the
language version promised by the package. A grammar is derived from the
language specification when no suitable Lezer grammar exists.

## Documentation

- [Documentation map](docs/README.md)
- [Architecture](docs/architecture.md)
- [Project invariants](docs/invariants.md)
- [Language packages](docs/languages.md)
- [Development](docs/development.md)
- [Validation](docs/validation.md)
- [Adding a language](docs/adding-a-language/README.md)
- [Contributing](CONTRIBUTING.md)

## Development

Install the pinned tools and run the normal repository gate:

```sh
mise install
mise run verify
```

See [Development](docs/development.md) for focused commands, generation
workflows, references, and the extended verification suite.

## License

Rezel is available under either the Apache License 2.0 or the MIT license, at
your option.
