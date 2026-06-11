# 1. Define the language contract

A parser does more than decide whether a source file is valid. It chooses an
input model, recognizes one or more languages, constructs a concrete syntax
tree, recovers from incomplete input, and exposes projections of that tree.
Those choices form the language contract.

Write the contract before adapting a large grammar. It gives grammar decisions
and tests a common authority when a specification, an existing Lezer grammar,
and an official implementation disagree.

## Supported language

Name the exact version or edition. If the language has dialects, preview
features, implementation extensions, or multiple parser modes, classify each
one as supported, rejected, or not yet implemented.

Define every parser entry point. A file, expression fragment, interactive
input, type, or pattern may each start from a different top rule. Entry points
are separate language-recognition claims; sharing productions does not make
them interchangeable.

Strict parsing and recovery have different meanings:

- strict parsing accepts only members of the supported language;
- recovering parsing produces a deterministic and useful CST for incomplete
  or malformed source;
- input errors report source conditions that cannot be represented faithfully;
- resource-limit errors stop work that exceeds an explicit bound.

Recovery must not silently expand the strict language.

## Source and coordinate model

All public Rezel ranges refer to byte offsets in the original UTF-8 source.
Record whether the language facade reads the source directly or first presents
a translated logical stream. A translated stream must preserve a mapping from
every logical boundary back to the original bytes, and malformed translation
must remain observable as an input error.

The grammar consumes Unicode code points through Rezel's lexical input model.
Language-specific notions such as escapes processed before tokenization,
indentation columns, or normalized line endings belong in an explicit input or
lexical adapter. They do not change public tree coordinates.

## Tree contract

Separate recognition from representation. Two grammars can accept the same
source and still produce incompatible CSTs.

For each syntax family, decide:

- which constructs have persistent named nodes;
- which grammar helpers remain invisible;
- whether punctuation and comments are named;
- how delimiters and lists are represented;
- which source text a node range includes;
- which error nodes appear during recovery;
- which visible nodes and tokens need typed accessors.

The CST is the lossless parser product. A typed CST is a zero-copy navigation
layer over it. An owned AST is a later, optional projection. Do not reshape the
CST solely to imitate an AST when a typed accessor or private syntax view can
express the same interpretation.

Syntax highlighting is another CST projection. Its contract should contain
only syntactic classification; name resolution and type-dependent
classification belong to semantic tooling.

## Evidence and authority

Use a source for the claim it can actually establish:

| Claim                                             | Suitable authority                                                       |
| ------------------------------------------------- | ------------------------------------------------------------------------ |
| Lexical and syntactic membership                  | Normative language specification and accepted/rejected conformance cases |
| Behavior of the targeted implementation           | Official parser or compiler for that version                             |
| CST compatibility with a maintained Lezer grammar | Pinned Lezer parser and tree snapshots                                   |
| Owned AST compatibility                           | Public AST model and differential projections                            |
| Recovery behavior                                 | Explicit malformed-source fixtures and repeated parses                   |
| Practical coverage                                | Standard library, conformance suite, or representative source corpus     |
| Reproducible parser artifacts                     | Regeneration tests over grammar, bindings, schemas, and binary tables    |

One authority rarely covers all rows. A Lezer snapshot may define the inherited
CST but not the newest language syntax. An official compiler may establish
acceptance while exposing no stable CST. Record the role and version of each
source.

An official parser is both observable behavior and inspectable source. Do not
reduce it to a black-box accept/reject or AST endpoint. For the targeted
revision, read its lexer, maintained grammar or parser control flow, syntax
diagnostics, tests, and tree construction when those layers inform the
contract. This distinguishes language rules from parser modes, compatibility
extensions, recovery policy, and checks deferred to later compiler phases.

Source code remains evidence rather than an automatic specification. Translate
implementation-specific mechanisms into explicit Rezel decisions and preserve
the relevant revision and source location with non-obvious choices.

## Start from an existing Lezer grammar

When a suitable grammar exists:

1. pin the upstream package and revision;
2. identify the language version and parser modes it implements;
3. inventory top rules, tokens, skips, precedences, ambiguity markers,
   dialects, node properties, external tokenizers, specializers, and contexts;
4. compile the declarative grammar with Rezel and port required external
   behavior through checked Rust bindings;
5. compare representative strict and recovering CSTs with the pinned parser;
6. isolate inherited defects and version gaps as minimal cases;
7. update the grammar according to the package's language contract;
8. retain regression evidence for every fix and intentional difference.

The long-term intent may be to contribute fixes upstream. The local package
must nevertheless remain correct and reviewable before that contribution is
accepted.

## Derive a grammar when none exists

Use a portfolio of sources: the normative grammar, lexical prose, the official
implementation, conformance fixtures, and grammars written for other parser
families. Record their versions and licenses.

Derive in layers:

1. source characters and any pre-lexical translation;
2. regular tokens, trivia, and lexical modes;
3. context-free productions and entry points;
4. precedence, associativity, and intentional ambiguity;
5. persistent CST nodes and ranges;
6. external lexical or validation behavior;
7. strict, recovery, and corpus evidence.

Other grammars are models, not drop-in implementations. A PEG's ordered choice,
a hand-written parser's control flow, or a tree-sitter external scanner must be
translated into explicit Rezel decisions.

## Establish a representative case set

Begin with small, stable cases:

- a minimal source for every top rule;
- one case for each top-level construct;
- every token boundary, escape form, comment form, and lexical mode;
- every precedence and associativity boundary;
- every contextual token or source translation;
- malformed input for each important recovery path;
- non-ASCII input with explicit byte-range assertions;
- every supported dialect and a rejected disabled-dialect case.

Give cases stable identities and reuse them across contract, CST, typed,
reference, and AST tests. Grow into broad corpora only after failures in the
small set are easy to classify.

The contract is ready when a reviewer can tell what the package recognizes,
what tree it exposes, how source positions behave, which optional projections
exist, and what evidence supports each promise.
