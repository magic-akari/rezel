# Case study: audit external declarations in a JavaScript grammar

This study demonstrates how to audit external grammar behavior before porting
it to a Rezel language package. It examines `@lezer/javascript` 1.5.4 at
revision `d28558ab1d686fcf54ca14dfa2d3cb20b41be7ae`. The maintained upstream
inputs are
[`javascript.grammar`](https://code.haverbeke.berlin/lezer/javascript/src/commit/d28558ab1d686fcf54ca14dfa2d3cb20b41be7ae/src/javascript.grammar),
[`tokens.js`](https://code.haverbeke.berlin/lezer/javascript/src/commit/d28558ab1d686fcf54ca14dfa2d3cb20b41be7ae/src/tokens.js), and
[`highlight.js`](https://code.haverbeke.berlin/lezer/javascript/src/commit/d28558ab1d686fcf54ca14dfa2d3cb20b41be7ae/src/highlight.js).

The revision is evidence for the inherited grammar, CST, and callback behavior.
It is not, by itself, evidence that the grammar implements a newer language
version, and this study does not claim that Rezel currently ships a JavaScript
package.

## Understand the coverage boundary

This is the primary broad tokenizer case study, not an exhaustive example of
every grammar feature. It is useful because several external mechanisms
interact in one maintained grammar: five tokenizers, tracked line-break state,
zero-length guards and insertion, parser-state queries, a dialect decision,
lookahead beyond the accepted range, fallback ordering, and a property source.

The same grammar also leaves deliberate gaps:

| Mechanism              | Present in this revision | Where its general contract is documented                                             |
| ---------------------- | ------------------------ | ------------------------------------------------------------------------------------ |
| `@external tokens`     | yes, five declarations   | [Grammar syntax](../02-grammar-syntax.md#external-declarations)                      |
| `@context`             | yes                      | [Language adapters](../04-language-adapters.md#context-trackers)                     |
| `@external propSource` | yes                      | [Language adapters](../04-language-adapters.md#node-properties-and-property-sources) |
| tokenizer `contextual` | yes                      | [Tokenizer flags](../04-language-adapters.md#tokenizer-flags)                        |
| tokenizer `fallback`   | yes                      | [Tokenizer flags](../04-language-adapters.md#tokenizer-flags)                        |
| tokenizer `extend`     | no                       | [Tokenizer flags](../04-language-adapters.md#tokenizer-flags)                        |
| `@external specialize` | no                       | [External specializers](../04-language-adapters.md#external-specializers)            |
| `@external extend`     | no                       | [External specializers](../04-language-adapters.md#external-specializers)            |
| `@external prop`       | no                       | [Node properties](../04-language-adapters.md#node-properties-and-property-sources)   |
| `[@inline]`            | no                       | [Template and inline rules](../02-grammar-syntax.md#template-and-inline-rules)       |

Thus this study supplies depth for external tokenization. The generic grammar
and adapter chapters supply completeness across declaration kinds. Do not use
the absence of a mechanism here as evidence that Rezel does not support it.

## Audit the complete boundary

Start from declarations, not from filenames. This grammar contains six
declarations beginning with `@external` and one `@context` declaration that
also imports external behavior:

| Declaration kind       | Imported symbol   | Declared output or role                          | Upstream runtime flags   |
| ---------------------- | ----------------- | ------------------------------------------------ | ------------------------ |
| `@context`             | `trackNewline`    | Boolean line-break context                       | `strict: false`          |
| `@external tokens`     | `noSemicolon`     | `noSemi`                                         | `contextual`             |
| `@external tokens`     | `noSemicolonType` | `noSemiType`                                     | `contextual`             |
| `@external tokens`     | `operatorToken`   | `incdec`, `incdecPrefix`, and `questionDot`      | `contextual`             |
| `@external tokens`     | `jsx`             | `JSXStartTag`                                    | none                     |
| `@external tokens`     | `insertSemicolon` | `insertSemi`                                     | `contextual`, `fallback` |
| `@external propSource` | `jsHighlight`     | Syntactic highlighting properties for node types | not a tokenizer          |

Thus the porting inventory is five tokenizers, one context tracker, and one
property source. Counting only declarations spelled `@external` would omit the
context tracker and produce an incomplete adapter plan.

The grammar has no external specializer, extender, or individual node
property. Keywords remain declarative `@specialize` and `@extend` expressions.
Regular-expression and division tokens remain generated tokens because parser
states keep their uses separate. Template content and block-comment content
remain local token groups. These negative findings matter: an adapter audit
must record what should stay in the grammar as well as what must move to Rust.

For each imported symbol, follow this sequence:

1. find its declaration and exact `source` string;
2. list every term the declaration allows it to return;
3. find every grammar position that consumes those terms;
4. inspect which input, stack, dialect, and context observations the callback
   makes;
5. record whether it consumes source or emits a zero-length term;
6. preserve tokenizer order and flags;
7. bind the symbol to one Rust implementation and test both acceptance and
   refusal paths.

## Track line-break context

The grammar imports `trackNewline` through `@context`. Its value starts as
`false`. On each shifted term, the upstream tracker:

- preserves the previous value for spaces and line or block comments;
- changes the value to `true` for the grammar's newline token;
- changes the value to `false` for every other shifted term.

Four external tokenizers read this value through `stack.context`;
`jsx` instead reads input and the active dialect. A Rezel port would use an
immutable boolean `ContextValue` with a deterministic shift transition and
equal hashes for equal values. The transition needs generated term constants
for spaces, newlines, and comments, which is why the two lowercase whitespace
terms carry `@export` in the grammar.

The upstream tracker sets `strict: false`: this context participates in token
decisions but is not treated as a condition for reusing an upstream syntax
node. Rezel's current `ContextTracker` has no corresponding manifest flag and
does not expose incremental node reuse, so the port records this fact rather
than inventing a Rust option. If reuse is added later, this decision must be
revisited explicitly.

There is a cross-file edge in this revision. `BlockComment` is a nonterminal
parsed from local delimiter, `blockCommentContent`, and
`blockCommentNewline` terms. Those shifted terms are neither the global
`newline` nor one of the three trivia terms recognized by the tracker, so they
set the context to `false`; reducing the completed comment does not restore the
prior value. A block comment therefore both fails to record its internal line
breaks and clears a line break already seen before the comment. The pinned
parser can consequently attach the following expression to a restricted
production after either a preceding newline plus block comment or a multiline
block comment. Record this as inherited behavior and a potential upstream
grammar defect; a port must choose explicitly between differential
compatibility and a language-contract fix.

Tests must cover the initial value, consecutive whitespace, visible comments,
a newline followed by more trivia, the first non-trivia term after a newline,
single-line and multiline block comments, and independent contexts on split
parse stacks. Testing only the tokenizer that consumes the context would leave
transition errors difficult to locate.

## Guard restricted productions with zero-length terms

`noSemicolon` produces the zero-length `noSemi` term. The term occurs between a
restricted keyword and an optional expression or label. It succeeds only when
the current position is not an automatic-semicolon boundary.

The callback first declines for code points in its fixed `space` table or at a
comment opener. For the common whitespace shared with the grammar, that lets
the skip tokenizer consume trivia and update the context before the decision
is retried. After trivia, it accepts `noSemi` only when:

- the next character is not `}`, `;`, or end of input; and
- the tracked context does not report a line break.

The fixed table and grammar alphabet are not identical. The callback treats
form feed (U+000C) and next-line (U+0085) as space even though this grammar's
`spaces` and `newline` tokens do not consume them. Conversely, the grammar's
`spaces` token accepts U+FEFF but the callback table does not. Preserve these
facts in differential fixtures and decide whether a port reproduces them or
repairs the mismatch under its language contract.

`noSemi` consumes no input. Its purpose is to make permission explicit in the
grammar: the following optional production is reachable only after the
callback has established that the line may continue.

`noSemicolonType` applies the same pattern to type suffixes. It accepts the
zero-length `noSemiType` term only when the next character is `[` and no line
break was tracked. The grammar uses the term before array and indexed-type
suffixes, preventing a bracket on the next line from being claimed by the
preceding type.

A port should test both guards as predicates, not merely as successful token
scans. Each test matrix needs the same-line case, line-break case, comment and
whitespace boundary, explicit separator, closing delimiter, and end of input.
The grammar action following a successful zero-length token must change parser
state so that the same term cannot be emitted forever.

## Classify context-sensitive operators

`operatorToken` handles three external terms. For `++` and `--`, it consumes
the two input characters and chooses between two grammar identities:

- `incdec` when no line break was tracked and the current stack can shift that
  general term;
- `incdecPrefix` otherwise.

`incdec` is consumed by both postfix and unary productions;
`incdecPrefix` is prefix-only. Thus `Stack::can_shift(incdec)` asks whether the
general term is usable in the current state. Choosing the prefix-only identity
after a line break prevents the postfix production while retaining the unary
one. The source spelling alone is insufficient because the LR state determines
whether the general interpretation is currently useful.

For `?.`, the callback looks one character beyond the dot. It accepts
`questionDot` only when that following character is not a decimal digit. This
preserves the alternative tokenization of a conditional operator followed by a
fractional numeric literal.

The tokenizer is contextual because its result depends on `stack.context` and
`Stack::can_shift`. Tests need both increment spellings in prefix and postfix
positions, a line break before the operator, states that reject the general
`incdec` term, optional access, and a dot followed by a digit.

## Delay the JSX or type decision

The `jsx` tokenizer produces `JSXStartTag`. It first checks that the JSX dialect
is enabled and that the next character is `<`. It declines for a closing-tag
prefix. For an opening prefix it scans enough identifier and whitespace text to
recognize selected generic-parameter shapes. It declines after a scanned name
followed by a comma. When the suffix spells `extends`, it also declines unless
the next code point satisfies the callback's approximate identifier-start
test. Thus exact `extends`, whitespace or punctuation after it, and suffixes
such as `extends2` or `extends$` take the type path, while a following ASCII
letter, underscore, or code point at least U+00C0 leaves the JSX path possible.

The callback's helper does not exactly match the grammar's identifier token.
In particular it excludes `$` and U+00A1 through U+00BF from identifier starts
and excludes `$` from continuations. This is another pinned implementation
boundary, not a general JSX or identifier rule.

The scan can look beyond the token that it ultimately accepts. After inspecting
the suffix, the callback sets the `JSXStartTag` endpoint immediately after the
opening `<`. A Rezel port should retain the endpoint with an `InputStream` mark
rather than reconstructing a byte offset from the number of inspected
characters.

Tests need the disabled dialect, opening and closing tags, fragment-like
openers, a generic parameter followed by a comma, a constrained generic
parameter, `extends` followed by a letter, digit, dollar sign, and non-ASCII
identifier characters on both sides of U+00C0, plus selected-range boundaries.
Lookahead changes incremental and range
dependencies even when the accepted token is one character long. In this
port, that means preserving the upstream incremental dependency as design
evidence and testing Rezel's selected-range boundary directly.

## Insert a separator only as fallback

`insertSemicolon` emits the zero-length `insertSemi` term before `}`, at end of
input, or when the context reports a line break. Two additional facts are part
of its behavior:

- its declaration appears after the generated `@tokens` block, giving ordinary
  tokens higher tokenizer precedence;
- its tokenizer flags are `contextual: true` and `fallback: true`.

The ordering and fallback flag encode the essential selection rule. If an
ordinary token can continue the current parse, the parser uses it. If a
higher-precedence tokenizer finds input but that token has no action in the
current LR state, the fallback tokenizer may still insert the separator. Thus
a line break does not unconditionally terminate a production.

Verification must pair inputs whose next token can continue the preceding
production with otherwise similar inputs whose next token cannot. It must also
cover `}`, end of input, explicit separators, comments around the line break,
and repeated insertion opportunities. Testing only newline-plus-identifier
would not establish the fallback policy.

## Attach highlighting as a property source

`jsHighlight` is not a tokenizer. The external property source returned by
`styleTags` maps node types and node paths to syntactic highlighting tags for
keywords, names, literals, operators, punctuation, comments, types, and JSX
structure.

A Rezel port would bind this declaration as a `property-source`, usually to a
function in the language facade when highlighting is feature-gated. It must
audit every selector against the maintained CST rather than copy the table
blindly. It must not perform name resolution, scope analysis, or type-dependent
classification.

That audit finds a concrete mismatch: the grammar defines
`JSXNamespacedName`, while two highlight selectors spell
`JSXNameSpacedName`. Those selectors cannot target the maintained node name.
A port should normally correct the typo to preserve the apparent highlighting
intent, record the deliberate differential change, and test namespaced tag and
attribute names. Reproducing the ineffective selector instead is possible only
when exact pinned output is the explicit contract.

Property-source tests should compare tags and byte ranges for direct node
selectors, path-sensitive selectors, delimiters, comments, and dialect-only
nodes. Parser acceptance tests do not establish these mappings.

## Express the conventional Rust boundary

Preserve each external's source and export name, then expose the same item from
the corresponding Rust module. A prospective port would use declarations
shaped like these:

```lezer
@context TRACK_NEWLINE from "./tokens.js"
@external tokens OPERATORS from "./tokens" { operatorToken }
@external propSource js_highlighting from "./highlighting"
```

The Rust backend removes an optional `.js`, `.mjs`, or `.ts` suffix, so the
first two declarations resolve to `crate::tokens::TRACK_NEWLINE` and
`crate::tokens::OPERATORS`; the property source resolves to
`crate::highlighting::js_highlighting`. JavaScript adapters export those same
names from the source modules named by the grammar.

The remaining tokenizer declarations use the same binding kind with their own
names and Rust statics. The runtime flags belong to each Rust
`ExternalTokenizer`; they are not encoded in the grammar declaration.

## Retain the general lessons

This audit yields reusable conclusions without turning language-specific code
into generic runtime branches:

- inventory `@context` alongside declarations spelled `@external`;
- treat the external token list as a checked callback interface;
- distinguish a zero-length permission guard from a zero-length inserted
  separator;
- preserve declaration order and `contextual`, `fallback`, and `extend` flags;
- preserve an accepted endpoint separately from the farthest lookahead;
- keep keyword tables, context-separated generated tokens, and local lexical
  regions declarative when they already fit the grammar model;
- test property sources as syntax projections rather than token behavior;
- pin the source revision and keep current language membership as a separate
  contract.

The general syntax and implementation rules are documented in
[Write or adapt the grammar](../02-grammar-syntax.md),
[Design lexing and contextual behavior](../grammar-design/02-lexing-and-context.md),
and [Implement language adapters](../04-language-adapters.md).
