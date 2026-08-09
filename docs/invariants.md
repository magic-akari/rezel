# Project invariants

Rezel is still evolving, and its public APIs and generated formats may change.
The following contracts must remain true unless the project deliberately
changes its underlying model.

## Original source coordinates

Every public `TextSize` and `TextRange` addresses the original UTF-8 input in
bytes. CST nodes, typed syntax, errors, highlight spans, and owned AST ranges
must use that coordinate space.

A language-specific lexical translation may expose different logical
characters to tokenization, but it must preserve a mapping to original byte
boundaries. Translation must never silently redefine the coordinate system of
the generic tree APIs.

## Language-independent core

`rezel-common`, `rezel-lr`, and `rezel-generator` provide reusable parsing
machinery. Language-specific rules—Unicode translation, indentation, automatic
separator insertion, contextual words, strict language constraints, and AST
shape—belong in the language package.

The generic core may gain a reusable capability needed by several languages.
It must not gain a branch that identifies or special-cases one language.

## Maintained inputs and generated artifacts

Grammar files, typed schemas, generated-data source models, and handwritten
language code are maintained inputs. Rust parser glue, named
terms, typed wrappers, Unicode tables, AST schemas, and parser-table blobs are
derived when their corresponding generator owns them.

Generated artifacts must be deterministic and protected by the repository's
centralized regeneration checks. They are changed by modifying their maintained
input or generator, never by editing the generated result to make a check pass.

## Syntax layers have different roles

The CST preserves concrete syntax, parser structure, error nodes, properties,
and source ranges. Typed syntax provides zero-copy language-specific navigation
over that CST. A private syntax view may normalize grammar shape for semantic
consumers. An owned AST represents a separate semantic or official public
syntax model.

These layers must not be collapsed for convenience. In particular, a grammar
must not acquire persistent nodes solely to make an AST lowerer easier, and an
AST lowerer must not treat raw generic child positions or node-name strings as
its semantic interface.

## Recovery and strict parsing are distinct

Recovering parsing may produce a useful CST for malformed or incomplete input.
Strict parsing makes a language-membership claim and must reject syntax errors
owned by the parser contract.

An owned AST lowerer must reject a recovery tree rather than silently turning
parser errors into a successful semantic representation.

## Parser work is bounded

Parser actions, active GLR stacks, stack depth, buffered records, and recovery
work are bounded by `ParseLimits` or an equivalent explicit budget. Refactors
and optimizations must preserve both successful behavior and resource-limit
failure behavior.

## Highlighting remains syntactic

`rezel-highlight` may inspect syntax kinds, properties, selectors, and mounted
trees. It does not perform name binding, scope analysis, type checking,
language-server queries, or theme policy.

## No unsafe implementation

The workspace forbids `unsafe` code. Generated layouts, table loading, input
fast paths, and parser optimizations must remain memory-safe without bypassing
that rule.
