# Validation

Parser correctness is not a single property. Rezel checks generation,
acceptance, recovery, CST shape, typed navigation, owned AST projection, and
behavior over broad source corpora separately.

Each claim must be paired with evidence that observes the same layer.

## Claims and evidence

| Claim                                                                                       | Primary evidence                                                                                              |
| ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| Generated artifacts match their maintained inputs.                                          | Central `rezel-codegen` checks comparing parser and term source, typed wrappers, and both parser-table blobs. |
| Public parser modes and source ranges behave as documented.                                 | Language contract tests and generic parser tests.                                                             |
| CST shape and selected recovery behavior remain compatible with a maintained Lezer grammar. | Pinned Lezer cases and snapshots.                                                                             |
| Strict acceptance follows the supported language version.                                   | Language specification cases, official parser/compiler fixtures, and conformance tests.                       |
| Typed syntax exposes the intended CST surface.                                              | Central typed-schema generation checks, coverage checks, and navigation tests.                                |
| An owned AST follows its declared public model.                                             | Lowering tests and snapshots from the corresponding official AST implementation.                              |
| The implementation handles realistic syntax at scale.                                       | Standard-library or other broad-corpus runners.                                                               |
| Adapted implementation relationships remain recorded.                                       | Alignment manifests and their traceability check.                                                             |

Passing one row does not establish another. A standard library is mostly
positive input and cannot prove rejection behavior. An official AST says
nothing about CST shape. A matching generated file says nothing about whether
the grammar accepts the right language.

## Local tests

Tests under a crate or language package establish the contracts owned by that
package:

- generator tests check grammar compilation, diagnostics, emitted layouts, and
  deterministic output;
- contract tests check public parser configuration, strict/recovery behavior,
  errors, and source coordinates;
- parse or tree tests check concrete structure and recovery nodes;
- typed tests check generated downcasts, fields, unions, cardinality, and
  coverage;
- lowering tests check owned AST construction and recovery-tree rejection;
- highlighting tests check syntactic tag projection.

Small local cases should be the first evidence added for a behavior. They make
failures easier to diagnose than a large external snapshot or corpus.

Generated-artifact reproducibility is the repository-level exception to this
package-local pattern. `rezel-codegen` owns the comparisons, and the registered
`mise` code-generation tasks compose them into the repository verification
gate.

## Lezer references

Lezer is the primary reference for the parser and CST model that Rezel
retains. The runners under `tools/references/lezer` build pinned Lezer parsers
for selected cases and compare their output with committed snapshots.

Use these references for claims about:

- grammar interpretation;
- concrete tree shape;
- tokenization or contextual behavior retained from an upstream grammar;
- selected recovery behavior.

Do not use a Lezer snapshot as proof that an owned Rezel AST matches a language's
official AST, or that an old upstream grammar implements the latest language
specification.

## Official language implementations

When a language has an official parser, compiler, or public AST, it provides
stronger evidence for language-version behavior:

- Go uses `go/parser`, `go/token`, and `go/ast`;
- Java uses javac parsing and compiler-tree APIs;
- Python uses the CPython parser and public `ast` model.

These tools check accepted and rejected source and, where applicable, produce
AST snapshots. Their implementation-specific extensions and deferred semantic
diagnostics still have to be separated from the language contract.

Reference runners observe these implementations, but do not replace studying
their source. When a result affects tokenization, grammar structure, strict
acceptance, ranges, or AST shape, inspect the targeted revision's lexer, parser,
diagnostics, tests, and tree construction. A black-box mismatch establishes
only that outputs differ; source study helps locate the responsible phase and
separate a language rule from a parser mode, extension, recovery choice, or
later compiler check.

## Conformance suites and broad corpora

Conformance suites provide focused accepted, rejected, and edge cases. Broad
corpora such as standard libraries exercise combinations and scale that small
fixtures rarely cover.

The current standard-library runners check strict acceptance and lowering for
the languages that provide them. More expensive AST-oracle modes compare the
complete projection with the official implementation.

Broad validation finds gaps; it does not silently redefine the supported
language. Reduce each disagreement to a stable case, decide which source owns
the claim, and add focused evidence before changing the grammar or lowerer.

## Snapshots and updates

Normal reference tasks verify committed snapshots:

```sh
mise run reference:lezer
mise run reference:go
mise run reference:javac
mise run reference:cpython
```

The corresponding `:update` tasks regenerate them:

```sh
mise run reference:lezer:update
mise run reference:go:update
mise run reference:javac:update
mise run reference:cpython:update
```

An update is appropriate when the upstream pin, fixture, maintained grammar, or
intended behavior changes. The snapshot diff must be reviewed with the source
change; update commands are not a way to accept an unexplained failure.

The aggregate broad-corpus task is:

```sh
mise run reference:stdlib
```

Language-specific `reference:stdlib:*:ast` tasks enable the more expensive
official AST comparisons.

## Traceability

Files under `alignment/` record relationships between selected external
sources and local implementation units. `mise run alignment` verifies that
covered files and symbols remain accounted for.

This is a maintenance aid. It does not establish behavioral equivalence and
does not replace parser, tree, lowering, or reference tests.

## Resolving disagreement

When a specification, an official implementation, and an existing Lezer
grammar disagree:

1. reduce the disagreement to a minimal stable input;
2. identify the exact language versions and parser modes;
3. decide which behavior Rezel promises;
4. assign the decision to acceptance, CST, recovery, or AST;
5. add focused evidence for that layer;
6. retain an explanation when the unselected result is likely to surprise a
   maintainer.

This keeps grammar updates and upstream fixes reviewable without treating any
single external implementation as the authority for every representation.
