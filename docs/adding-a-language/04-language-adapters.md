# 4. Implement language adapters

Regular tokens and a context-free grammar should express ordinary syntax.
Language adapters handle the remaining behavior at explicit boundaries:
translated source, stateful tokenization, contextual classification, computed
node properties, and strict conditions that are clearer outside the CFG.

Before adding an adapter, identify why the grammar model is insufficient. The
answer determines the correct interface.

| Requirement                                                        | Boundary                         |
| ------------------------------------------------------------------ | -------------------------------- |
| Present a logical character stream while retaining original ranges | Lexical input                    |
| Emit a token using parser state or nearby source                   | External tokenizer               |
| Carry immutable state across shifts or reductions                  | Context tracker                  |
| Classify a scanned base token with Rust code                       | External specializer             |
| Attach generated or computed tree metadata                         | Node property or property source |
| Reject malformed source before parsing                             | Facade preflight                 |
| Enforce a strict syntactic condition over the completed CST        | Strict post-validation           |

Adapters belong to the language package. A rule useful to only one language is
not a reason to teach the generic LR runtime that language.

## Binding manifest

External declarations use source and symbol names inherited from the grammar.
`<language>.bindings.toml` resolves each declaration to a statically linked
Rust path:

```toml
[[binding]]
kind = "external-tokenizer"
source = "./tokens"
name = "indentation"
rust_path = "crate::tokens::INDENTATION"

[[binding]]
kind = "context-tracker"
source = "./tokens"
name = "trackIndent"
rust_path = "crate::tokens::TRACK_INDENT"

[[binding]]
kind = "property-source"
source = "./highlight"
name = "languageHighlighting"
rust_path = "crate::language_highlighting"
```

The supported `kind` values are:

- `external-tokenizer`;
- `external-specializer`;
- `context-tracker`;
- `node-property`;
- `property-source`.

`source` and `name` must exactly match the grammar declaration. `rust_path`
must parse as a Rust path. Generation rejects duplicate entries, missing
bindings, unused bindings, unknown fields, and invalid paths. The manifest is
therefore a checked interface, not a loose module-resolution hint.

## Preserve raw source coordinates

`rezel_common::Input` stores immutable UTF-8 source and exposes lengths and
ranges in bytes. The ordinary lexical view reads Unicode scalar values directly
from that input. A language facade may instead attach a language-specific
lexical view to a validated `ParseRequest`.

Each logical character contains:

- the code point seen by tokenization;
- the original byte position where it begins;
- the original byte position where the next logical character begins.

This allows source translation without changing tree coordinates. A translated
view must satisfy:

1. every public position remains an original UTF-8 byte boundary;
2. forward and backward character reads agree on logical boundaries;
3. identity-mapped regions can expose shared chunks without copying;
4. translated scalar text is returned only when it can be represented as
   UTF-8;
5. malformed or incomplete translation is reported as
   `ParseErrorKind::Input`;
6. selected parse ranges begin and end at valid raw and logical boundaries.

Some language definitions may deliberately expose surrogate code points after
translation. That is an adapter decision; ordinary UTF-8 input produces Unicode
scalar values. Never convert public tree ranges into code-point, UTF-16, line,
or column coordinates inside the parser.

Test identity regions, translated regions, malformed forms, reverse reads,
non-ASCII text, and exact byte endpoints.

## External tokenizers

An `ExternalTokenizer` receives an `InputStream` positioned at the candidate
token start and the current `Stack`. The stream exposes code-point lookahead,
validated advancement, marks, source reads, and token acceptance. The stack can
report parser state, current context, enabled dialects, and whether a term can
be shifted.

Implement one lexical decision per tokenizer. Typical examples are indentation
changes, automatic separators, string modes, or identifiers backed by generated
Unicode tables.

The callback must either accept one token with a validated endpoint or decline
without changing parser-visible state. Observe these constraints:

- do not emit a zero-length token indefinitely;
- check `Stack::can_shift` when token availability prevents repeated
  insertion;
- use `InputStream::mark` and `accept_token_to` instead of constructing
  unchecked endpoints;
- do not read past selected ranges;
- return a parse error for malformed lexical conditions rather than allowing
  recovery to hide them;
- keep scanning work proportional to the consumed or explicitly bounded input.

Test every path where the tokenizer accepts, declines, reaches EOF, crosses
Unicode input, or observes a context/dialect boundary.

## Context trackers

A context tracker carries immutable, type-erased `ContextValue` instances
through parser shifts and optionally reductions. Its start function creates the
initial value; transition callbacks return the next value; the hash identifies
equivalent contexts to the parser.

Use context for finite parser-relevant history such as indentation stacks or a
lexical mode that cannot be reconstructed from the immediate source. Keep
values immutable and cheaply cloneable. Equal logical contexts must have equal
hashes. A transition should depend only on its previous context, term, parser
state, and—when the chosen callback form allows it—the positioned input stream.

Do not store an AST, symbol table, mutable global state, or unbounded source
history in the context. Add tests for the initial value, each transition,
hash stability, nested state, and recovery paths that revisit a position.

## Specializers and property sources

An external specializer receives the scanned token text and parser stack and
returns an optional specialized term. Use it when a finite declarative
`@specialize` or `@extend` table cannot express the classification. Its result
must depend only on syntactic context available at that point.

Node properties and property sources attach static metadata to node types.
Highlight tags and grouping metadata belong here. They describe syntax kinds,
not individual semantic occurrences. If a classification requires name
resolution or types, perform it after parsing in a semantic layer.

## Preflight and strict validation

Some invalid source is better rejected at the language facade:

- preflight validates conditions known before parser execution, such as a
  malformed source translation or an indentation invariant;
- strict post-validation inspects a completed strict CST when the condition is
  syntactic but awkward or dangerous to encode in the grammar;
- recovering parsing may omit strict post-validation when the contract requires
  a useful editor tree for the same input.

A wrapper should preserve the generic `Parser` lifecycle:

```text
raw Input
  -> validate ParseRequest
  -> preflight / attach lexical view
  -> LR parse
  -> optional strict CST validation
  -> Tree or ParseError
```

Return a precise `ParseErrorKind` and original byte position. Validation should
not silently repair the tree, perform semantic analysis, or make strict and
recovering modes indistinguishable.

## Adapter completion criteria

An adapter is ready when its grammar declaration and binding manifest agree,
its state and coordinate rules are explicit, focused tests exercise normal and
boundary behavior, malformed input remains observable, and no
language-specific branch was added to `rezel-common` or `rezel-lr` without a
genuinely reusable abstraction.
