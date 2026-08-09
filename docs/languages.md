# Language packages

Each programming language is implemented as a package under `languages/`.
Packages share the generated parser core but own every rule that cannot be
generalized without changing language behavior.

The set of packages will grow as the project develops. The packages present in
the repository are current implementations and examples of different
integration patterns, not the fixed scope of Rezel.

## Package model

A language package may contain the following layers:

| Layer                | Typical contents                                                                             | Required                               |
| -------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------- |
| Language definition  | Grammar, Rust bindings, typed schema, version and reference decisions.                       | Yes                                    |
| Generated parser     | Rust glue, named terms, endian-specific parser-table blobs.                                  | Yes                                    |
| Public facade        | `parser()`, strict/recovery configuration, top rules, dialects, input setup.                 | Yes                                    |
| Lexical adapters     | External tokenizers, specializers, context trackers, or source translation.                  | When the grammar needs them            |
| Typed syntax         | Generated zero-copy views over visible CST nodes.                                            | By package contract                    |
| Private syntax views | Temporary normalization used by AST lowering or other semantic consumers.                    | When raw typed shapes are insufficient |
| Owned AST            | Arena-backed or otherwise owned projection from a strict CST.                                | Optional                               |
| Highlighting         | Syntax-property configuration and a `highlight` feature.                                     | Optional                               |
| Validation           | Package-local contract/tree/projection tests and repository codegen/reference/corpus checks. | Yes, according to the claims made      |

The grammar and adapters together define tokenization and parsing. Typed syntax,
AST lowering, and highlighting are downstream projections; none of them should
change what the generic runtime means.

## Common package flow

Every package follows the same broad path:

```text
maintained language definition
  -> generated parser artifacts
  -> language facade and adapters
  -> source-preserving CST
  -> package-specific projections
```

The details remain language-owned. Every package returns a configured
`LRParser` directly. Strict token validators run after a permissive tokenizer
selects one base token and before its LR action; packages may still provide a
language-owned parse function for input translation, preflight checks, or
whole-tree syntactic predicates.

## Current packages

| Package             | Current language boundary                                                | Notable package surface                                                                                                                  |
| ------------------- | ------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `rezel-lang-json`   | JSON using the pinned `@lezer/json` grammar and JSON conformance corpus. | Generated parser, typed CST, and optional highlighting.                                                                                  |
| `rezel-lang-go`     | Go 1.26 syntax calibrated against Lezer and the Go parser/public AST.    | Contextual semicolon handling, typed CST, private syntax views, owned AST, and optional highlighting.                                    |
| `rezel-lang-java`   | Java SE 26 syntax calibrated against Lezer and javac.                    | Unicode-escape lexical translation, typed CST, private syntax views, compiler-tree-aligned owned AST, and optional highlighting.         |
| `rezel-lang-kotlin` | Kotlin 2.4.10 syntax calibrated against the formal grammar and compiler. | Contextual lexical adapters, complete visible-kind typed coverage, strict standard-library corpus validation, and optional highlighting. |
| `rezel-lang-python` | Python 3.14 syntax calibrated against Lezer and CPython.                 | Indentation and string tokenizers, strict syntax validation, complete typed CST, CPython-aligned owned AST, and optional highlighting.   |
| `rezel-lang-rust`   | Rust 1.95.0 Edition 2024 syntax calibrated against Lezer and rustc.      | Permissive recovery identifiers, strict XID/syntax validation, partial typed CST, and optional highlighting.                             |

Version details and package APIs belong in each language's README and generated
Rust documentation.

## Representative integration patterns

A small regular language can stay close to the declarative grammar and
generated parser. JSON is the current example of this minimal shape.

Some languages need parser-aware tokenization. Go's semicolon insertion and
Python's indentation state are implemented as statically bound external
tokenizers and immutable parser contexts rather than branches in `rezel-lr`.

A language may define source translation before tokenization. Java's Unicode
escapes are exposed through a lexical view that yields logical code points
while retaining original UTF-8 byte boundaries.

Some strict language rules are clearer outside the CFG. A language facade may
perform bounded input preflight or strict post-parse validation, provided that
the owner of each rule and its error behavior are explicit.

An owned AST is added only when the package promises such a model. Typed syntax
alone is a complete and useful package surface for languages whose consumers
need a source-backed CST rather than an owned semantic projection.

## Adding or changing a language

A language change may affect grammar acceptance, CST shape, external
tokenization, source translation, typed fields, AST lowering, highlighting, and
reference evidence. Keep the change in the narrowest owning layer and validate
each claim with the corresponding test.

Follow [Adding a programming-language parser](adding-a-language/README.md) for
the complete workflow.
