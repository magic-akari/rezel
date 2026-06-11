# Rezel documentation

## Project model

- [Architecture](architecture.md) describes the build-time and run-time flows,
  syntax products, module boundaries, and validation boundary.
- [Project invariants](invariants.md) defines the contracts that implementation
  changes and language packages must preserve.
- [Language packages](languages.md) explains the common package model and the
  representative integration patterns in the current repository.

## Development and evidence

- [Development](development.md) covers setup, focused feedback loops,
  generation, tests, references, traceability, and performance work.
- [Validation](validation.md) assigns specifications, pinned parsers, official
  implementations, snapshots, and corpora to the claims they can establish.

## Adding a language

[Adding a language](adding-a-language/README.md) follows the complete path from
a versioned language contract to a verified Rust package:

1. define the language contract;
2. model or adapt the grammar;
3. generate parser artifacts;
4. implement language adapters;
5. define typed syntax;
6. optionally lower an owned AST;
7. verify the promised behavior.

The nested
[Grammar design foundations](adding-a-language/grammar-design/README.md)
develops the theory of language modeling, regular lexing, contextual tokens,
CFGs, LR conflicts, GLR, CST design, recovery, derivation, and differential
validation. The adjacent
[grammar syntax guide](adding-a-language/02-grammar-syntax.md) gives the
self-contained Rezel notation and links to the deeper Lezer documentation.

Contributor setup begins in [CONTRIBUTING.md](../CONTRIBUTING.md).
