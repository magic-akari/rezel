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

## From a grammar declaration to runtime

An external declaration is a compile-time contract. It does not load the
module named by `from` at runtime. The complete path is:

```text
grammar declaration
  -> declaration kind plus exact source and symbol keys
  -> <language>.bindings.toml
  -> checked Rust path emitted into generated.rs
  -> statically linked Rust callback or property provider
  -> LR tokenization, specialization, context tracking, or NodeSet construction
```

The grammar owns when an external can participate and which terms or
properties it exposes. The binding manifest owns symbol resolution. The Rust
adapter owns the algorithm and runtime flags. Generated code joins those pieces
with direct static references in the language's tokenizer and specializer
arrays, context slot, and node-set constructor.

There is no string lookup after generation. A change to a declaration's kind,
`source`, or symbol name must be accompanied by a matching manifest change and
regeneration. A change to a Rust path is confined to the manifest as long as
the external contract remains the same.

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

Every supported grammar declaration maps as follows:

| Grammar declaration    | Binding `kind`         | `rust_path` target                | Generated use                                  |
| ---------------------- | ---------------------- | --------------------------------- | ---------------------------------------------- |
| `@external tokens`     | `external-tokenizer`   | `static ExternalTokenizer`        | `Tokenizer::External(&...)`                    |
| `@external specialize` | `external-specializer` | `fn(&str, &Stack) -> Option<u16>` | callback result replaces the scanned base term |
| `@external extend`     | `external-specializer` | `fn(&str, &Stack) -> Option<u16>` | callback result accompanies the base term      |
| `@context`             | `context-tracker`      | `static ContextTracker`           | `Language::context`                            |
| `@external prop`       | `node-property`        | `fn() -> NodeProp<T>`             | deserialize a grammar value onto a node type   |
| `@external propSource` | `property-source`      | `fn() -> NodePropSource`          | extend the generated `NodeSet`                 |

`@external specialize` and `@external extend` deliberately share one binding
kind and callback shape. The grammar declaration tells generated code whether
the returned term has replacement or extension semantics. This is unrelated
to `TokenizerFlags::extend`, which controls tokenizer ordering rather than
specialization.

`source` and `name` must exactly match the grammar declaration. `rust_path`
must parse as a Rust path. Generation rejects duplicate entries, missing
bindings, unused bindings, unknown fields, and invalid paths. The manifest is
therefore a checked interface, not a loose module-resolution hint.

For `@external tokens`, the declaration block is the complete set of terms the
callback is allowed to accept:

```lezer
@external tokens layout from "./tokens" {
  indent[@name=Indent,group=Layout],
  dedent[@name=Dedent,group=Layout]
}
```

The block declares one tokenizer named `layout`, not one callback per term.
Its Rust implementation accepts the generated numeric identity for either
declared term through `InputStream`, or declines. Term properties such as
`@name` and `group` are compiled into CST and node-type metadata; they do not
configure the callback or its `TokenizerFlags`. Accepting an undeclared or
unrelated term violates the adapter contract even if the numeric value happens
to be valid.

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

Generated and local token DFAs, and generated or external specialization
results, are filtered against term dialect metadata by the runtime. Raw
external-token acceptance is not. If a term in an `@external tokens` block has
`@dialect`, its callback must call `Stack::dialect_enabled` with the generated
dialect id and decline while that dialect is disabled.

Implement one lexical decision per tokenizer. Typical examples are indentation
changes, automatic separators, string modes, or identifiers backed by generated
Unicode tables.

An adapter may attach a conservative `ExternalTokenizerStart` filter when its
first-code-point domain is much smaller than all input. The filter is a runtime
optimization, not lexical precedence: every code point and end-of-input
position where the callback may accept a token **or return an error** must be
included. Only an input on which the callback is guaranteed to decline may be
skipped. Test a filtered decline, an accepted token, an error result, and EOF;
also cover selected ranges when the tokenizer is used by mixed parsing.

The grammar determines which parser states enable the tokenizer. Its position
among generated, local, and other external tokenizers determines lexical
precedence. At an enabled position, the runtime considers tokenizer entries in
declaration order:

1. a tokenizer that declines leaves later entries eligible;
2. an accepted non-extension term with an executable parser action wins and
   stops the search;
3. an accepted term with no executable action suppresses ordinary later
   entries, but a later `fallback` tokenizer may still run;
4. an `extend` tokenizer contributes distinct actions and, when no earlier main
   candidate exists, leaves all lower-precedence entries eligible; when it runs
   as a fallback after a main candidate, only later fallback entries remain
   eligible.

This order is part of the grammar contract. Do not move an external declaration
without testing every token family that can inspect the same prefix.

### Tokenizer flags

`ExternalTokenizer::new` takes `TokenizerFlags`. These flags belong to the Rust
adapter because they describe how the callback participates in runtime
selection and caching:

| Flag         | Runtime meaning                                                                                                                                                          | Required invariant                                                                                                                                                   |
| ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `contextual` | Recompute for each parser stack instead of reusing a cached result for an equivalent position, tokenizer mask, and context hash.                                         | Set it when the result reads stack details finer than those cache keys, including `Stack::can_shift`. A non-contextual callback must be reusable across such stacks. |
| `fallback`   | Run after an earlier tokenizer accepted a lexical term but produced no executable action in the current parser state. It does not override an earlier actionable result. | The result must be a genuine lower-priority interpretation, commonly a guarded zero-length insertion.                                                                |
| `extend`     | Keep distinct actions produced by this tokenizer and continue to the lower-precedence entries still eligible under the fallback rule.                                    | Multiple surviving terms must be intentional grammar alternatives with bounded branching.                                                                            |

A tokenizer that depends only on source text can normally leave all three
flags false. Depending on tracked context alone does not by itself require
`contextual`: the context hash is already part of the cache key. Set
`contextual` when equal cache keys can still produce different callback
results, most commonly because the callback queries the exact stack state.
If two context values can produce different answers in a non-contextual
tokenizer, their hashes must also differ; otherwise the tokenizer must be
contextual.

`fallback` is not a general priority inversion. It matters only after an
earlier lexical match cannot drive the current parse. Likewise, `extend` does
not replace the earlier term and does not mean “keep scanning characters”; it
keeps tokenizer alternatives alive while selection proceeds to eligible later
entries. Actions are deduplicated by their encoded parser action; if two terms
lead to the same action, a second token identity is not retained merely because
`extend` was set. Use either flag only with grammar witnesses that need its
exact behavior.

The callback must either accept one token with a validated endpoint or decline
without changing parser-visible state. Observe these constraints:

- emit a zero-length token only when shifting it changes parser state or
  context so the same action cannot repeat at the same position;
- check `Stack::can_shift` when term availability is the progress guard, and
  set `contextual` when that query makes otherwise cache-equivalent stacks
  differ;
- use `InputStream::mark` and `accept_token_to` instead of constructing
  unchecked endpoints;
- do not read past selected ranges;
- return a parse error for malformed lexical conditions rather than allowing
  recovery to hide them;
- keep scanning work proportional to the consumed or explicitly bounded input.

The runtime's recovery limits are a safety boundary, not a progress mechanism
for an adapter. A zero-length insertion needs tests that ask for a second token
at the same source position after its shift, at EOF, and on multiple GLR or
recovery stacks.

Use this minimum interaction matrix whenever more than one tokenizer can run
at a position:

| Case                 | Earlier tokenizer                   | Later tokenizer             | Assertion                                                                |
| -------------------- | ----------------------------------- | --------------------------- | ------------------------------------------------------------------------ |
| Ordinary precedence  | accepts an actionable term          | would also accept           | only the earlier result participates                                     |
| Decline              | declines                            | accepts                     | the later result participates                                            |
| Non-actionable match | accepts, but its term has no action | ordinary                    | the later tokenizer remains suppressed                                   |
| Fallback handoff     | accepts, but its term has no action | `fallback` accepts          | the fallback result participates                                         |
| Fallback suppression | accepts an actionable term          | `fallback` would accept     | the fallback tokenizer cannot displace it                                |
| Extension            | `extend` accepts                    | accepts                     | the union of their distinct parser actions remains available             |
| Fallback extension   | accepts, but its term has no action | `fallback + extend` accepts | its actions participate and still-lower fallback entries remain eligible |

For each relevant row, cover consuming and zero-length results where supported,
EOF, Unicode boundaries, enabled and disabled dialects, each tracked context,
and a callback decline. For `contextual`, include two stacks at the same input
position whose exact parser state requires different answers. For `extend`,
include an ambiguity witness and an adversarial repetition proving branching
remains bounded.

## Context trackers

A context tracker carries immutable, type-erased `ContextValue` instances
through parser shifts and optionally reductions. Its start function creates the
initial value; transition callbacks return the next value; the hash
participates in tokenizer caching. Equal logical contexts must have equal
hashes. Contexts that can change a non-contextual tokenizer's result must have
distinct hashes. The hash is not part of parser-stack identity.

`ContextValue` and its hash do not participate in any parser stack-equivalence
or pruning decision. Some deduplication paths compare source position and LR
state stack; equivalence pruning compares only LR states, and the long-running
branch bound may prune still more broadly. Context-dependent branches must
therefore converge before those policies can erase a behaviorally distinct
value. A context design that needs such branches requires a runtime change;
assigning different hashes alone does not preserve them.

`ContextTracker::new` accepts the start function, optional shift and reduce
callbacks that can inspect a positioned `InputStream`, and the hash function.
Use `with_shift_without_input` or `with_reduce_without_input` when a transition
does not read source, so the runtime need not reposition the stream. Use
`with_shift_input_terms` when only a declared subset of shifted terms needs
input; the callback still runs for other terms and must not inspect or advance
the stream for them. When a transition makes no logical change, return a clone
of the existing `ContextValue` so its identity and cached hash remain intact.

Rezel's current tracker is non-incremental. It has no callback for reused tree
fragments and no equivalent of Lezer's node-reuse `strict` option. Record those
upstream settings during an adaptation, but do not invent manifest fields for
them; revisit the decision if cross-edit fragment reuse is implemented.

Use context for finite parser-relevant history such as indentation stacks or a
lexical mode that cannot be reconstructed from the immediate source. Keep
values immutable and cheaply cloneable. A transition should depend only on its
previous context, term, parser state, and—when the chosen callback form allows
it—the positioned input stream.

Do not store an AST, symbol table, mutable global state, or unbounded source
history in the context. Add tests for the initial value, each transition,
hash stability, nested state, and recovery paths that revisit a position.

## External specializers

An external specializer receives the scanned token text and parser stack and
returns an optional specialized term. Use it when a finite declarative
`@specialize` or `@extend` table cannot express the classification. Its result
must depend only on syntactic context available at that point.

The result is cached with the base tokenizer's result. An external specializer
has no independent `contextual` flag. Therefore its use of `Stack` must be
invariant for the base tokenizer's cache key: source position, tokenizer mask,
and context hash. In particular, do not call `Stack::can_shift` when the base
is a generated token. Exact-stack dependence is safe only when the base comes
from an external tokenizer that is itself `contextual`, or after the runtime
gains a separate specializer cache policy.

The callback has the same Rust shape for specialization and extension. For
`@external specialize`, a returned term replaces the scanned base term. For
`@external extend`, both the returned and base terms are offered to parser
action selection, in that order. Actions are then deduplicated by encoded
`Action`, so terms that lead to the same action do not retain two observable
token identities. The callback should return only terms listed by its grammar
declaration and should decline for all other lexemes. Test a recognized lexeme,
an unrecognized lexeme, every context-dependent branch, and—when extending—a
position where the two terms produce distinct viable actions as well as one
where their actions collapse.

## Node properties and property sources

An `@external prop` imports one property definition. A grammar value such as
`Node[property=value]` is deserialized by that `NodeProp<T>` while generated
code constructs the node type. Use it when the grammar needs a typed metadata
key that is not one of the built-in properties. Test valid and invalid values,
the node types that carry it, and node types that must not. The returned
property must be type-level (`per_node: false`), because generated grammar
metadata cannot attach a distinct value to each syntax-node occurrence. The
provider must return the same `NodeProp` identity on every call—normally a key
stored in a static or `OnceLock`—and the key must define a deserializer.
Generated node-set construction calls the provider at each use site and
expects deserialization to succeed. A missing decoder or invalid maintained
grammar value currently panics during node-set initialization rather than
becoming a parse error, so validate the decoder and every grammar literal in
adapter tests.

An `@external propSource` imports a function that computes or supplies a set of
property assignments after all generated node types exist. Generated code
passes its `NodePropSource` to `NodeSet::extend`. Use it for selector-like or
cross-cutting assignments that would be repetitive or computed outside the
grammar. Every property key assigned by the source must likewise have stable
identity shared with the code that reads it.

Both mechanisms attach static metadata to node types. Highlight tags and
grouping metadata belong here. They do not attach a different value to each
syntax-node occurrence and they do not run semantic analysis. If a
classification requires name resolution, types, or program state, perform it
after parsing in a semantic layer.

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
generated code contains the intended static reference, its state, flags,
ordering, progress, and coordinate rules are explicit, focused tests exercise
normal and boundary behavior, malformed input remains observable, and no
language-specific branch was added to `rezel-common` or `rezel-lr` without a
genuinely reusable abstraction.
