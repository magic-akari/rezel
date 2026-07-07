# 2. Design lexing and contextual behavior

Tokenization maps a logical character stream to terminal symbols and source
ranges. In a classical pipeline this mapping is fixed before parsing. Rezel,
following Lezer, makes tokenization parser-aware: the current LR state limits
which token groups are relevant, and explicit callbacks may inspect syntactic
context.

This flexibility does not remove the need for a lexical model. It makes token
selection part of the grammar contract.

## Start with regular token languages

A regular language can be recognized with finite state. Rezel compiles ordinary
token expressions into deterministic automata over code points. Typical
regular families include:

- identifiers defined by fixed character classes;
- numeric literals with finite prefix, digit, separator, and suffix states;
- quoted strings whose escapes and termination do not nest recursively;
- line comments;
- punctuation and operators.

For each family, record:

- its character alphabet and Unicode version;
- case sensitivity and normalization;
- prefixes shared with other tokens;
- termination, escape, and malformed forms;
- whether it can cross a line or selected input range;
- the raw source range of one accepted token.

A finite automaton cannot remember an unbounded indentation stack, match
arbitrarily nested delimiters, or apply name/type information. Those
requirements need context, syntax, or a semantic layer even when a language
specification places them under a “lexical” heading.

## Preserve the character boundary

The tokenizer observes logical `CodePoint` values. Ordinary UTF-8 source yields
Unicode scalar values. A language-specific lexical view may translate source
spellings and may deliberately expose another code-point sequence, while every
position remains an original UTF-8 byte boundary.

Treat source translation as a function with boundary information:

```text
logical character at raw byte p -> (code point, next raw byte q)
```

The mapping must be deterministic in both directions used by tokenization.
Malformed translation is an input error, not an ordinary unmatched token.
Identity-mapped segments should remain shareable so the exceptional translation
does not impose copying on the whole file.

Specify translation before token rules. Otherwise the grammar may appear to
match source characters while actually matching a hidden transformed alphabet.

## Decide token overlap explicitly

“Take the longest token” is not a complete lexical policy. Consider:

- `/` and a comment beginning with `/`;
- `>` and `>>`;
- an identifier and a keyword;
- a regular-expression literal and a division operator;
- string content and an interpolation opener.

Rezel has several distinct selection mechanisms:

1. **Parser context.** Tokens that never occur together may overlap because the
   current LR state makes only one relevant.
2. **Token precedence.** When overlapping generated tokens can occur together,
   a declared order selects the winner.
3. **Specialization.** A base token is reclassified from its matched text.
4. **Local token group.** One grammar region uses a different finite lexical
   vocabulary.
5. **External tokenizer.** Rust code makes one bounded decision using input,
   stack, dialect, or context.

Choose the lowest mechanism that states the language rule. Add a witness where
both candidates share the longest common prefix and another where the shorter
candidate must win.

Parser-context separation is a property of the generated LR automaton, not a
permanent property of two token declarations. A grammar refactor can make
previously separate tokens valid in the same state and expose an overlap.
Preserve intended separation with an explicit token conflict declaration and
rerun lexical overlap tests after structural grammar changes.

Lezer's [Tokens](https://lezer.codemirror.net/docs/guide/#tokens),
[Token Precedence](https://lezer.codemirror.net/docs/guide/#token-precedence),
and
[Contextual Tokenization](https://lezer.codemirror.net/docs/guide/#contextual-tokenization)
sections develop these cases.

## Separate words from roles

Identifiers and keywords usually share one regular language. Scanning a common
identifier first avoids duplicating character logic and handles keyword
prefixes correctly.

Then decide the syntactic relationship:

- replacement specialization means the word ceases to be an identifier where
  the specialized term is selected;
- extension keeps both interpretations available;
- external specialization computes the result when a finite text table is
  insufficient.

A contextual word still needs grammar positions that explain when its keyword
role is legal. Specialization is token classification, not semantic name
resolution.

Putting every keyword literal directly into the token DFA duplicates identifier
prefixes, increases the lexical automaton, and can tokenize an identifier such
as `newest` as a keyword prefix plus a remainder when precedence is used
carelessly. Scan the common identifier language first and specialize its
complete text. Use extension only when both roles must remain alive—extension
can introduce the same runtime branching pressure as an explicit GLR site.

Pin the identifier character data to a named Unicode version when the language
does. Generate tables from maintained authoritative data and test the boundary
between accepted and rejected code points.

## Model trivia and lexical regions

Whitespace and comments affect syntax even when they are skipped. Define:

- the globally skipped set;
- regions with a different skip set;
- whether line endings are trivia or terminals;
- whether comments remain visible for CST, highlighting, or AST consumers;
- whether a repeated or multiline token should be split at stable boundaries.

A scoped skip expression changes the grammar in that region. A local token
group changes the available lexical language. Neither is merely a generator
workaround.

Local groups work well for finite sublanguages such as string content with a
small set of delimiters. Use explicit context when the mode has persistent
history that cannot be represented by the local DFA.

A parse state can have only one skip expression. A rule that enters a scoped
skip context must therefore have clear boundaries when called from another
context; an optional or repeated suffix at that boundary leaves the parser
without one unambiguous skip set. A local token group is stricter still: its
states cannot also use ordinary, literal, or skip tokens. Define its delimiters
inside the group when necessary and isolate the region with the intended skip
scope.

## Introduce context only for parser-relevant history

Let `K` be the set of lexical contexts. A stateful token decision can be viewed
as:

```text
token : (K, parser state, input suffix) -> token or no token
transition : (K, shifted/reduced term) -> K
```

The context must contain only information that can affect future syntactic
tokenization. Indentation levels, delimiter-sensitive separator rules, and
finite lexical modes are typical. An AST, symbol table, or package environment
is not.

Good context has these properties:

- immutable values and deterministic transitions;
- a stable hash for logically equal values;
- bounded growth relative to syntactic nesting;
- explicit behavior under recovery and EOF;
- no dependence on mutable global state.

An external tokenizer should implement one decision. It may inspect whether the
stack can shift a term, but it must not turn arbitrary parser state numbers
into an undocumented second grammar.

Lezer's [External Tokens](https://lezer.codemirror.net/docs/guide/#external-tokens),
[Context](https://lezer.codemirror.net/docs/guide/#context), and
[Indentation example](https://lezer.codemirror.net/examples/indent/) show the
original callback model. Rezel binds equivalent responsibilities to checked
Rust symbols.

## Design implicit separators as fallback decisions

An implicit separator is still a grammar terminal. Its spelling happens to
consume no source text, and a contextual tokenizer decides where the terminal
is available. Do not model it as discarded whitespace or as an unconditional
rewrite of every line boundary—the same boundary may either separate two
constructs or occur inside one construct.

Three terms are important in this decision:

- a **recognized token** is a candidate accepted by a tokenizer at the current
  source position;
- an **actionable token** has at least one parser or skip action for the current
  stack; `Stack::can_shift` is the adapter-facing check for whether a specific
  grammar term can shift after zero or more reductions;
- a **fallback tokenizer** is allowed to run after an earlier tokenizer
  recognized a main candidate but that candidate produced no actionable parse
  path.

Fallback does not mean “lower lexical precedence, always try both.” An
actionable ordinary token wins before a later fallback tokenizer runs. The
fallback flag matters precisely when the earlier tokenizer recognized input
but the current grammar state cannot use its term. A tokenizer marked `extend`
deliberately keeps eligible lower-precedence tokenizers in play after a match.
If it is itself running after a main candidate as `fallback + extend`, only
still-lower fallback tokenizers are eligible. Extension is a separate form of
branching, not part of the usual separator design.

Put an implicit-separator tokenizer after the ordinary tokenizers whose
candidates must get the first opportunity. At a source position `p`, use this
decision flow:

1. Run enabled tokenizers in declaration order. A tokenizer that declines
   leaves the decision to the next one.
2. If an ordinary tokenizer recognizes an actionable token, use its parser
   action. Do not insert a separator merely because trivia contains a line
   boundary.
3. If it recognizes a token with no action in the current stack, record it as
   the current main candidate and skip later non-fallback tokenizers. Later
   fallback tokenizers may still inspect the position and may replace that
   candidate.
4. When the separator fallback runs, test the declared boundary conditions:
   relevant trivia or delimiter state, end of input when allowed, and any
   finite parser context carried by the tracker.
5. Decline unless the separator term can shift in this stack. Otherwise accept
   the separator at `[p, p)` and let the parser take its action before
   tokenizing `p` again.
6. If neither the ordinary candidate nor the separator yields an action, leave
   the position to ordinary EOF handling or recovery. The tokenizer must not
   invent a separator solely to avoid an error.

This ordering preserves continuation. For example, if the token after a line
boundary can continue the current expression, that actionable token wins. If
the same token is not legal after a completed construct and the boundary rule
allows separation, the fallback can expose the inserted terminal instead.

A zero-length token has no input advance to prove progress. Its design must
provide another finite, strict progress measure at the same byte position:

- shifting the separator moves to a parser state where that separator can no
  longer shift;
- an indentation-like token shrinks or otherwise advances a finite contextual
  obligation;
- an end-of-input closer consumes one item from a finite set of pending
  closures.

After every accepted `[p, p)` token, each successor parse stack must advance
along that finite measure so that the same decision cannot repeat indefinitely.
Merely changing state and later returning to the same opportunity is not
progress. A grammar such as an unrestricted repetition of an insertable
zero-length terminal violates this invariant even when each individual
tokenizer call is deterministic. Prove the measure in the adapter notes and
test repeated opportunities at one position, including recovery and end of
input.

## Keep lexical work deterministic and bounded

For the same raw input, selected ranges, parser state, dialect, and context, a
tokenizer must return the same result. Its lookahead and stored state must have
a stated bound or advance through source proportionally.

Main and external tokenizers are tried in grammar declaration order. Treat
ordering, `fallback`, and `extend` behavior as part of the lexical contract,
and test positions where more than one tokenizer could inspect the same
prefix. As concrete examples of the general model, Lezer's
[JavaScript example](https://lezer.codemirror.net/examples/javascript/)
demonstrates fallback ordering for an automatic separator, while its
[Indentation example](https://lezer.codemirror.net/examples/indent/) shows
zero-length guards based on shiftability and shrinking context.
The project-local
[pinned external-tokenizer study](../case-studies/javascript-externals.md)
audits the maintained upstream grammar behind the same broad separator pattern,
from declarations through adapter obligations, and also records the mechanisms
it does not cover.

Test:

- every accepted token at its shortest and longest relevant prefixes;
- every overlap in both winning directions;
- Unicode and invalid character boundaries;
- EOF at every partial prefix;
- malformed escape or translation forms;
- skipped and non-skipped regions;
- each context transition and nested state;
- an actionable ordinary token and a non-actionable ordinary token at the same
  potential separator boundary;
- separator fallback when the boundary is absent, the term cannot shift, and
  the term can shift;
- repeated zero-length opportunities;
- zero-length insertion immediately before EOF and during recovery;
- long-prefix and adversarial cases that establish the tokenizer's stated scan
  bound.

The lexical model is complete when every terminal has a source language and
range policy, and every departure from finite-state tokenization has a narrow,
testable owner.
