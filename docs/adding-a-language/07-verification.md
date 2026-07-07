# 7. Verify the language

Verification connects each language claim to evidence at the layer where the
claim is decided. No single reference or test suite proves the entire package.

## Verification map

| Claim                                           | Primary evidence                                               |
| ----------------------------------------------- | -------------------------------------------------------------- |
| The grammar is internally valid                 | Generator compilation, conflict cases, and warnings            |
| Generated artifacts match maintained inputs     | Parser, terms, both endian blobs, and typed-source comparisons |
| Strict parsing recognizes the intended language | Acceptance and rejection contract tests                        |
| Recovery produces a stable useful CST           | Malformed fixtures, repeated trees, and range assertions       |
| Input and external token behavior is correct    | Focused adapter tests                                          |
| Typed accessors describe the grammar            | Schema validation and typed navigation tests                   |
| An owned AST matches its contract               | Lowering tests, snapshots, and an AST oracle                   |
| Highlighting is syntactically stable            | Selector and span tests                                        |
| Behavior agrees with a selected implementation  | Pinned differential reference                                  |
| The package handles realistic breadth           | Conformance suite or broad source corpus                       |
| Work remains bounded                            | Adversarial inputs and configured limit tests                  |

Choose the relevant rows from the language contract. A parser-only package need
not invent AST evidence; a package that promises an AST cannot stop at CST
snapshots.

## Compile and regenerate

Start with grammar diagnostics:

```sh
cargo run --locked -p rezel-generator -- check \
  languages/<language>/grammar/<language>.grammar
```

Every remaining precedence marker, cut, ambiguity site, external declaration,
and warning should have an explanation and a minimal test.

Generate all artifacts, inspect the source and binary changes, and run:

```sh
mise run codegen:rezel:<language>
```

The registered repository-level `rezel-codegen` scope must compare
`generated.rs`, `terms.rs`, `generated.le.bin`, `generated.be.bin`, and
`typed.rs`. The standard language scope currently requires all five. Its check
task must be a dependency of `mise run verify`; do not duplicate the comparison
in a package-local `generated` test.

## Test the public parser contract

Cover every top rule and supported dialect in strict and recovering modes as
appropriate. Include:

- minimal and representative valid source;
- exact rejection cases for version boundaries and disabled features;
- malformed source that recovers into an expected tree;
- deterministic recovery over repeated parses;
- non-ASCII source with original UTF-8 byte ranges;
- empty input, EOF boundaries, comments, and trailing trivia;
- deeply nested, repeated, or adversarial syntax;
- configured action, active-stack, stack-depth, buffered-record, and recovery
  limits;
- arbitrary immutable `Input` implementations if the facade exposes them.

Strict tests make language-membership claims. Recovery tests make tree and
safety claims. Keep their expected outcomes separate.

`PartialParse` allows one parse to advance in bounded steps or stop at a
position. Test that lifecycle when the language facade customizes it. Reuse of
unchanged fragments across source edits is not currently implemented and
should not be claimed by the language package.

## Test adapters and projections

For every external tokenizer, context tracker, specializer, or lexical input,
test both the action and the point where it must decline. Include malformed
translation, selected ranges, EOF, Unicode, nested context, and recovery where
relevant.

Typed syntax tests should downcast roots and unions, traverse every field
cardinality, and verify token and node ranges. Complete schema coverage is a
generation-time check; navigation tests confirm the intended public roles.

If present:

- AST tests lower strict trees, reject recovery trees, and compare every
  supported projection with its model;
- highlighting tests compare syntactic tags and ranges without requiring
  semantic classification.

## Compare the right references

When adapting a Lezer grammar, pin the upstream revision and compare selected
strict and recovering CSTs. Keep minimal cases for:

- syntax retained without change;
- upstream grammar bugs fixed in Rezel;
- language-version updates;
- intentional tree or recovery differences;
- Rust ports of external callback behavior.

Use the normative specification and official implementation for current
language membership. Use a public AST model for AST compatibility. A
disagreement is a decision to isolate, version, and test—not a snapshot to
overwrite without explanation.

Reference tooling should have separate check and update modes. Check committed
snapshots in normal verification; update them only when the intended authority
or behavior changes.

## Run a broad corpus

After representative cases are stable, parse a standard library, conformance
suite, or real-world source set that matches the supported version. Record:

- corpus identity and revision;
- file selection and exclusions;
- parser entry point, dialect, and strictness;
- expected acceptance policy;
- failure classification;
- resource limits.

Broad parsing finds combinations omitted by small tests. It does not explain a
failure. Reduce every new failure to a focused regression case before changing
the grammar.

If the package has an AST oracle, compare the broad corpus in a separate mode
so parser acceptance and projection equality remain distinguishable.

## Integrate with the workspace

Before relying on the aggregate gate, register the language scope in
`rezel-codegen`, add its `codegen:rezel:<language>` and
`codegen:rezel:<language>:update` tasks to `mise.toml`, and make the check task
a dependency of `tasks.verify`. The update task writes generated files; only
the non-writing check belongs in the gate.

Run focused checks while iterating:

```sh
mise run codegen:rezel:<language>
cargo test --locked -p rezel-lang-<language>
cargo clippy --locked -p rezel-lang-<language> --all-targets --all-features -- -D warnings
cargo doc --locked -p rezel-lang-<language> --all-features --no-deps
```

Then run the normal repository gate:

```sh
mise run verify
```

Add new reference or corpus tasks to `mise.toml` when they are part of the
maintained language contract, and include them in `verify:full` when their cost
is unsuitable for the normal gate:

```sh
mise run verify:full
```

## Final review

Before presenting the language as supported, confirm that:

- the README names the version, entry points, strict/recovery behavior, typed
  API, optional AST/highlighting, and known limitations;
- grammar, bindings, typed schema, generated Rust, and both binary tables are
  synchronized;
- every external declaration has exactly one checked Rust binding;
- all public positions use original UTF-8 byte offsets;
- language-specific behavior remains inside the language package;
- every non-obvious conflict and recovery decision has a minimal test;
- references are pinned and assigned only to claims they can establish;
- the central generation check plus package-local contract, adapter, typed,
  optional projection, and corpus checks pass at the levels promised by the
  package.

Verification is complete when the evidence supports the stated contract, not
when every available test category has been copied from an existing language.
