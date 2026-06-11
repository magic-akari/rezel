# 6. Lower an owned AST

An owned AST is optional. Add one when the language package promises a compact
semantic syntax model, compatibility with an official public AST, or an
ownership boundary independent of the CST. A parser and typed CST do not
require an AST to be complete.

## Keep the syntax layers distinct

Each layer answers a different question:

| Layer               | Purpose                                                                        |
| ------------------- | ------------------------------------------------------------------------------ |
| CST                 | Preserve recognized concrete syntax, tokens, recovery nodes, and source ranges |
| Typed CST           | Navigate stable grammar kinds and direct-child roles without copying           |
| Private syntax view | Normalize awkward CST shapes for one package                                   |
| Owned AST           | Store the package's public abstract model independently of the tree            |

The AST may omit punctuation, flatten helper structures, decode literals, and
group several CST kinds under one model. Those are projection decisions. They
must not be fed backward into the persistent CST unless syntax consumers also
need the changed tree.

## Define the AST authority

Write down:

- root and entry-point models;
- node, enum, and field identities;
- optional and repeated relationships;
- source-range rules;
- whether comments, attributes, directives, or parentheses survive;
- how identifiers, strings, numbers, and other constants are stored;
- which language extensions have no AST projection;
- the official or public model used as a differential oracle.

An official implementation may expose implementation details that do not
belong in Rezel. Preserve behavior needed by the language contract, not object
identity, internal compiler nodes, or undocumented mutation.

If the model is generated from an upstream schema, keep that schema or
generation script as the maintained input and add a deterministic generated
check.

## Add a private syntax view

AST lowering often needs relationships that are wider than a generated
direct-child accessor: normalized parameter lists, flattened operator chains,
synthetic grammar wrappers, or several CST alternatives with the same AST
meaning.

Put those interpretations under `src/syntax/`. A private syntax view may use
typed wrappers and short-lived generic node traversal to expose named roles to
the lowerer. It should:

- preserve the source-backed tree rather than allocate another persistent tree;
- centralize assumptions about child order and hidden grammar helpers;
- return explicit errors for unexpected recovery or unsupported shapes;
- have focused tests for every normalized shape.

The AST lowerer should consume these roles instead of repeatedly matching raw
node names or numeric child positions.

## Lower strict trees

The normal lifecycle is:

```rust
let tree = rezel_lang_<language>::parser()
    .with_strict(true)
    .parse(source)?;
let ast = LanguageAst::lower(&tree, source)?;
```

Lowering must reject a tree containing recovery structure. Strict parser
success establishes that the source belongs to the parser's language; the
lowerer then establishes that every accepted construct has an AST projection.

Keep failures explicit:

- `InvalidTree` or its equivalent for recovery or structurally inconsistent
  input;
- `UnsupportedSyntax` for a maintained CST extension that the AST contract does
  not yet cover;
- a source/value error for literal decoding that cannot be represented;
- a resource error if lowering has its own explicit bound.

Do not manufacture a semantically plausible node when the source shape is not
understood.

## Preserve source identity deliberately

AST ranges remain original UTF-8 byte ranges. Decide whether a range covers
modifiers, delimiters, terminators, or only the abstract construct, and test
that rule. Store source text only when the AST contract needs an owned value;
otherwise retain a range and let callers read the original source.

Decode escapes and normalize values at one named boundary. Keep both the raw
range and the decoded value when diagnostics or round-tripping need both.
Never infer byte ranges from decoded string lengths.

Owned arenas are useful for compact nodes and stable indexes, but they are an
implementation choice. Public IDs and lifetimes should follow the AST contract,
not the layout of the reference implementation.

## Validate the projection

Use three complementary test forms:

1. focused lowering tests for every node family, field, literal, and range rule;
2. serialized snapshots that make complete small ASTs reviewable;
3. differential projection against an official AST over representative and
   broad corpora.

Normalize only known representation differences before a differential
comparison. Record each normalization so a broad equality result does not hide
missing information.

Also test recovery-tree rejection, unsupported forward syntax, repeated
lowering of the same tree, deep nesting, and large lists. The AST is complete
when every strict construct promised by its contract either lowers
deterministically or returns a documented explicit error.
