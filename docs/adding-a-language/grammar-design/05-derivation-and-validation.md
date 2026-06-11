# 5. Derive and validate the grammar

Grammar construction is a model-derivation process. Starting from scratch means
that no suitable Lezer grammar exists; it does not mean inventing syntax from a
few examples. In both the adaptation and new-grammar paths, decisions should be
traceable to a language source and testable at the layer they affect.

## Build a source portfolio

Collect complementary sources:

- normative lexical and syntactic specification;
- official parser or compiler behavior for the targeted version;
- an existing Lezer grammar and external support code, if available;
- grammars for LR, LL, PEG, ANTLR, tree-sitter, or other parser families;
- conformance suites with accepted and rejected cases;
- a standard library or representative real-world corpus;
- a public AST definition when an owned projection is planned.

Record revision, language mode, dialect, license, and intended role. Reusing
grammar text or callback code requires attribution. Observing behavior through
a differential test is a separate evidentiary relationship.

## Study the official parser

An official parser has two distinct roles. It is an executable implementation
whose results can be compared with Rezel, and it is an inspectable record of
how a language implementation turns specification prose into operational
decisions. Differential fixtures cover the first role; grammar derivation also
requires the second.

Study the exact revision targeted by the language contract. Follow the relevant
path through:

- source-character translation, decoding, and line normalization;
- token definitions, trivia, lexical modes, and contextual state;
- grammar productions or hand-written parser control flow;
- precedence, ambiguity resolution, lookahead, and contextual restrictions;
- the boundary between parser rejection, recovery, and later diagnostics;
- CST or AST construction, source ranges, and normalization;
- focused tests and version history for ambiguous or recently changed forms.

A public parser API may hide several of these phases. If parser code is
generated, find the maintained grammar and generator inputs; inspect generated
control flow only when it clarifies the resulting behavior. Record the source
revision and relevant files or symbols for decisions that would otherwise be
hard to reconstruct.

The goal is to recover the language decision, not to transcribe the mechanism.
A recursive-descent branch, PEG predicate, semantic action, or compiler
diagnostic may need a different representation in Rezel's lexical, LR/GLR,
CST, or strict-validation layers. Implementation study explains why an
observable difference exists; the specification and language contract still
decide which behavior Rezel promises.

For a hand-written recursive-descent parser, audit one feature in this order:

1. trace the accepted token sequence and the smallest branch-selecting witness;
2. mark which branches implement syntax, lookahead, diagnostics, semantics,
   AST construction, or performance;
3. collapse methods and flag combinations that recognize the same
   context-free language;
4. encode the remaining language decisions as one vertical Rezel slice;
5. compare strict behavior, CST, parser-state statistics, and token-table
   statistics before extending the slice.

Do not translate a parser method into a nonterminal merely because both have a
name. The [CFG and LR chapter](03-cfg-lr-and-ambiguity.md#translate-recursive-descent-by-meaning)
explains why copying caller flags and control-flow branches can multiply LR
states, token states, or runtime GLR stacks.

## Adapt an existing Lezer grammar

An existing grammar already embodies decisions about tokenization, conflicts,
CST shape, and recovery. Audit those decisions before modifying it:

1. pin the package version and exact source revision;
2. identify its targeted language version and top rules;
3. inventory visible nodes, tokens, skips, precedence, ambiguity markers,
   dialects, external tokenizers, contexts, specializers, and properties;
4. compile the declarative grammar with Rezel and isolate unsupported or
   differently implemented constructs;
5. bind external behavior to small Rust adapters;
6. compare representative strict and recovered CSTs with the pinned Lezer
   parser;
7. reduce known upstream defects and language-version gaps to minimal cases;
8. apply fixes and updates against the package's language contract;
9. retain tests for inherited behavior, fixes, and intentional differences.

Structural similarity to the upstream grammar is useful when it preserves a
well-tested design. It is not a reason to keep an incorrect production or an
obsolete language version. Fixes can later be proposed upstream without making
the local package depend on that process.

## Derive a new grammar

When no suitable grammar exists, proceed from language layers:

1. **Character model.** Define raw encoding, logical characters, translation,
   line endings, and byte-coordinate preservation.
2. **Lexical model.** Classify regular tokens, overlap, trivia, contextual
   words, local modes, and external state.
3. **Normalized syntax.** Translate specification notation and prose
   conditions into explicit CFG productions for every top rule.
4. **Selection model.** Tabulate precedence and associativity; classify every
   LR conflict and intentional GLR site.
5. **CST model.** Choose visible nodes, properties, delimiters, comments, and
   ranges independently from editorial nonterminals.
6. **Side-condition model.** Assign each non-CFG syntactic check to preflight or
   strict validation and leave semantic checks later.
7. **Evidence model.** Pair each membership, tree, recovery, and projection
   claim with an authority and test.

Other grammars help reveal recursion, ambiguity, and lexical modes. Translate
their language model, not their semantic actions, ordered-choice assumptions,
backtracking behavior, error productions, or AST construction.

## Grow through vertical slices

A vertical slice reaches from characters to one top rule and expected CST.
Build a small complete path before adding broad disconnected productions.

A practical sequence is:

1. minimal root and EOF behavior;
2. identifiers, literals, trivia, and delimiters;
3. one declaration or statement;
4. primary expressions;
5. postfix and prefix forms;
6. binary, conditional, and assignment hierarchy;
7. types, patterns, or another major subgrammar;
8. contextual layout, modes, and dialects;
9. malformed forms and recovery boundaries.

Every slice should contain:

- a strict positive case;
- a nearby strict negative case;
- the complete expected CST;
- a lexical or syntactic boundary;
- recovery behavior when the construct is incomplete;
- byte-range assertions where source mapping is nontrivial.

This shape localizes failures. A broad grammar added without tree cases makes
token, production, conflict, and representation defects difficult to
distinguish.

## Validate independent dimensions

At least three parser dimensions remain independent:

| Dimension         | Question                                                                    | Evidence                                                  |
| ----------------- | --------------------------------------------------------------------------- | --------------------------------------------------------- |
| Strict membership | Are exactly the intended source forms accepted?                             | Specification cases, official parser, conformance, corpus |
| CST mapping       | Does accepted source produce the intended visible tree?                     | Expected-tree fixtures and pinned Lezer comparison        |
| Recovery          | Does malformed source produce deterministic useful structure within limits? | Invalid fixtures, repeated parses, and adversarial tests  |

Typed navigation, AST projection, and highlighting add further dimensions.
Success in a later projection does not prove the underlying tree: normalization
may accidentally erase a mismatch.

Lezer's [Testing example](https://lezer.codemirror.net/examples/test/) shows a
compact source-plus-expected-tree format. Rezel language tests can use ordinary
Rust fixtures and snapshots as long as source, mode, and expected tree remain
reviewable together.

## Use differential and metamorphic evidence carefully

A differential test compares Rezel with another implementation. First define
the observable being compared:

- accept/reject result;
- top rule and mode;
- normalized CST;
- error position or category;
- AST projection.

Normalize only known representation differences. A broad equality check is
meaningless if the normalization silently discards the field under test.

Metamorphic cases check relations without requiring another parser. Examples
include adding allowed trivia, changing an identifier to another identifier
with the same lexical class, or wrapping an expression in parentheses where
the language promises equivalent membership. State the expected invariant:
membership may remain equal while CST and ranges intentionally change.

Neither technique replaces direct negative tests. Two implementations can
share a bug, and a transformation may leave the subset exercised by the
language.

## Resolve disagreements explicitly

When specification, official implementation, and inherited grammar disagree:

1. reduce the difference to a stable minimal source;
2. identify versions, modes, flags, and entry points;
3. classify the affected layer: membership, CST, recovery, or projection;
4. choose the behavior promised by Rezel's language contract;
5. preserve the selected behavior in a focused test;
6. retain the alternative result as context when it explains a compatibility
   decision.

Do not update a snapshot before making this decision. Do not accept an
extension merely because it occurs in a corpus, and do not reject specified
syntax merely because an inherited grammar predates it.

## Close the model at the Rezel boundary

Before treating the grammar as the maintained implementation:

- generator diagnostics contain no unexplained conflict or warning;
- parser-state and token-table growth is proportionate to explained language
  structure;
- hand-written reference-parser control flow has been classified rather than
  mirrored as a method-for-rule or flag-for-template matrix;
- every marker and external declaration has a reason;
- bindings resolve exactly to checked Rust symbols;
- generated source and both native-endian table blobs are reproducible;
- strict membership, CST, recovery, adapters, and resource bounds have focused
  cases;
- typed syntax and optional projections cover their declared contracts;
- reference and corpus tests are assigned only to claims they establish;
- language-specific logic remains in the language package.

Continue with the [grammar syntax](../02-grammar-syntax.md),
[package and generation](../03-package-and-generation.md), and
[verification](../07-verification.md) chapters to turn the model into the
repository implementation.
