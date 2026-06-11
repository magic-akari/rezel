# Contributing to Rezel

Rezel is under active development. Changes should preserve the boundaries in
[Architecture](docs/architecture.md) and the contracts in
[Project invariants](docs/invariants.md). Language work should also follow the
[language package model](docs/languages.md).

## Set up the workspace

The repository pins dprint, Go, Java, Node.js, and Python in
[`mise.toml`](mise.toml), and Rust in
[`rust-toolchain.toml`](rust-toolchain.toml):

```sh
mise install
mise run verify
```

`mise install` also installs the Node.js dependencies used by reference and
traceability tools. `mise run verify` is the normal handoff gate. Run the
extended reference and corpus suite when the change affects those contracts:

```sh
mise run verify:full
```

See [Development](docs/development.md) for focused package commands, code
generation, snapshot updates, and the checks included in each gate.

## Make a change

1. Identify the owning layer. Generic input, tree, and parser mechanics belong
   in `crates/`; grammar behavior, source translation, external tokenization,
   strict validation, typed syntax, AST lowering, and language references
   belong to a language package or its tools.
2. Change the maintained input. Edit a grammar, binding manifest, typed schema,
   generation script, or handwritten implementation rather than a derived
   file.
3. Regenerate every affected artifact. Parser generation includes Rust glue,
   named terms, little- and big-endian table blobs, and typed CST source.
4. Run the smallest relevant tests and inspect generated or snapshot diffs.
   An update command is appropriate only after deciding that the new output is
   intended.
5. Run `mise run verify`. Add `mise run verify:full` for changes that require
   pinned parser references, conformance cases, or broad source corpora.
6. Update documentation when a public contract, architecture boundary,
   language version, command, or generated interface changes.

When adding a parser, follow [Adding a language](docs/adding-a-language/README.md).
It covers language contracts, grammar theory and notation, package generation,
adapters, typed CSTs, optional ASTs, and verification.

## Test the claim being changed

Use evidence at the layer that owns the behavior:

- runtime unit tests for generic parser mechanics;
- generator cases for grammar syntax, conflicts, and deterministic emission;
- language contract tests for strict acceptance, recovery, and coordinates;
- generated tests for all committed parser artifacts;
- typed and lowering tests for CST and AST projections;
- reference comparisons for behavior defined by a pinned parser or official
  implementation;
- corpus runners for breadth and resource-limit tests for adversarial input.

A generated diff does not prove language correctness, and a broad corpus does
not explain a failure. Reduce new failures to focused cases and keep distinct
claims separately reviewable.

## Preserve project boundaries

- Public source positions are byte offsets in the original UTF-8 input.
- The generic runtime remains independent of language-specific grammar
  behavior.
- Grammars, bindings, schemas, and maintained generation scripts are the
  inputs of generated code.
- CST, typed CST, private syntax views, owned ASTs, and highlighting retain
  their separate roles.
- Strict parsing and recovering parsing make different claims.
- Parser work over untrusted input remains explicitly bounded.
- Highlighting remains syntactic.
- Project code does not use `unsafe`.

If a changed file is covered by a manifest under `alignment/`, run
`mise run alignment` and update the mapping when its source relationship or
rationale changed. Traceability complements behavioral tests; it does not
replace them.

## Keep changes reviewable

Use focused commits when practical. Explain non-obvious grammar conflicts,
representation changes, snapshot updates, and reference disagreements with a
minimal source case. Preserve third-party licenses and attribution when
adapting grammar or implementation code.
