# 2. Write or adapt the grammar

A Rezel grammar describes three related products:

1. regular languages that turn code points into tokens;
2. a context-free language that turns tokens into syntax;
3. the visible node structure of the resulting CST.

External tokenizers and validation fill the deliberate gaps outside those
models. Keeping the boundaries explicit makes a grammar explainable and keeps
language-specific behavior out of the generic runtime.

## Terms, rules, and visible nodes

A minimal grammar has a top rule, nonterminal rules, and tokens:

```lezer
@top Document { expression }

expression { Name | Number | BinaryExpression }

BinaryExpression {
  "(" expression ("+" | "-") expression ")"
}

@tokens {
  Name { @asciiLetter+ }
  Number { @digit+ }
}
```

`@top` declares an entry point that must cover the complete input. A grammar
may have multiple top rules.

Rules beginning with an uppercase letter produce named CST nodes. Lowercase
rules organize the grammar without adding a persistent node. This convention
is a representation decision, not a distinction between terminals and
nonterminals: tokens are declared inside `@tokens`, and both visible and hidden
rules can refer to them.

Grammar expressions use:

| Form                           | Meaning                   |
| ------------------------------ | ------------------------- |
| `a b`                          | sequence                  |
| `a \| b`                       | choice                    |
| `a?`                           | zero or one               |
| `a*`                           | zero or more              |
| `a+`                           | one or more               |
| `( ... )`                      | grouping                  |
| `"text"` or `'text'`           | literal token             |
| `rule<arg>`                    | template-rule application |
| `{ ... }` inside an expression | inline rule               |

Choice is not ordered. `a | b` and `b | a` recognize the same language; the
branch order of a hand-written parser must be translated into an explicit
token, production, precedence, cut, ambiguity, or validation decision when it
is semantically significant.

Factor repeated structures into lowercase helpers or template rules. Keep a
rule visible only when its identity and range are useful to downstream syntax
consumers.

## Tokens consume Unicode code points

Rezel presents the grammar tokenizer with Unicode code points while retaining
raw UTF-8 byte boundaries. A token must describe a regular language. It can use
literals, character sets such as `$[a-zA-Z_]`, negated sets such as `![\n]`,
named classes, grouping, choice, and repetition, but it cannot use arbitrary
recursive context-free structure.

The built-in classes are:

- `@asciiLetter`, `@asciiLowercase`, and `@asciiUppercase`;
- `@digit`;
- `@whitespace`;
- `@eof`;
- `_` where any single code point is required.

Use explicit generated tables or an external tokenizer when a language's
identifier or literal rules depend on a particular Unicode version and cannot
be stated precisely with the built-ins.

Skipped input is declared separately:

```lezer
@skip { space | LineComment | BlockComment }

@tokens {
  space { @whitespace+ }
  LineComment { "//" ![\n]* }
}
```

Top-level `@skip` applies generally. A scoped `@skip { ... } { rule }`
declaration changes trivia inside a particular grammar region. Comments should
be skipped only if the CST contract does not require them as visible nodes.
One parser state can carry only one skip expression, so a scoped region used
from another skip context must have clear entry and exit delimiters. Do not
leave its boundary behind an optional or repeated suffix that makes either
skip set possible in the same state.

## Contextual tokenization and overlap

The parser asks only tokenizers relevant to the current LR state. Tokens may
therefore overlap when they cannot both occur at the same parse position. When
overlapping tokens are valid together, state the lexical choice explicitly.
This separation can change when productions are refactored, so use
`@conflict` and overlap tests for token pairs that must never share a state.

Inside `@tokens`, precedence runs from highest to lowest:

```lezer
@tokens {
  @precedence { BlockComment, Divide }

  Divide { "/" }
  BlockComment { "/*" blockContent* "*/" }
  blockContent { ![*] | "*" ![/] }
}
```

`@conflict { A, B }` declares that the listed token groups must remain
separate. Use it to make an expected non-overlap checkable rather than relying
on incidental LR states.

A local token group replaces the normal token vocabulary in a grammar region.
It is suitable for string content or an embedded lexical mode:

```lezer
String {
  stringStart (Interpolation | stringCharacter)* stringEnd
}

@local tokens {
  stringEnd { '"' }
  interpolationStart { "${" }
  @else stringCharacter
}
```

`@else` consumes content not matched by another local token. Local groups must
be isolated from ordinary, literal, and skip tokens in the states where they
apply. A scoped `@skip {}` region and locally defined delimiters are commonly
required. Do not use a local group as an informal substitute for an unbounded
state machine; if the next token depends on explicit history, use a context
tracker and external tokenizer.

## Keywords and specialization

Keywords commonly share their spelling rules with identifiers. Token
specialization first scans the base token and then changes its term when the
matched text belongs to a declared set:

```lezer
kw<word> { @specialize<Identifier, word> }
softKw<word> { @extend<Identifier, word> }

@tokens {
  Identifier { @asciiLetter (@asciiLetter | @digit)* }
}
```

`@specialize` replaces the base token, so the specialized word is no longer an
identifier where both would otherwise be accepted. `@extend` keeps both
interpretations available and lets later syntax choose. Properties such as
`@name` and `@export` may be attached to the specialization when the generated
term needs a stable name or external visibility.

Prefer specialization over giving keyword prefixes token precedence over an
identifier. Precedence can split `newest` into `"new"` and `est`, and a large
literal keyword vocabulary needlessly expands the token DFA. Use `@extend`
only when both interpretations are required, because it may create runtime
parse branches.

Use an external specializer when classification needs Rust code rather than a
finite declarative word set. It must be declared in the grammar and resolved by
the binding manifest.

## Precedence, associativity, and cuts

An ambiguous expression grammar needs a parse-selection rule. Declare
precedence from highest to lowest and place a marker at the decision point:

```lezer
@precedence {
  multiply @left,
  add @left,
  assign @right
}

expression { Atom | BinaryExpression | AssignmentExpression }

BinaryExpression {
  expression !multiply ("*" | "/") expression |
  expression !add ("+" | "-") expression
}

AssignmentExpression {
  expression !assign "=" expression
}
```

`@left` and `@right` resolve self-association. A precedence without an
associativity annotation does not decide a conflict with itself.

`@cut` is a commitment marker. It gives a production priority before a normal
conflict necessarily appears:

```lezer
@precedence { block @cut }

statement {
  Block { !block "{" statement* "}" } |
  expression ";"
}
```

Use a cut only where the language commits to the selected construct. It should
not hide an incorrect language model.

## Intentional ambiguity and GLR

Some prefixes cannot be classified until later tokens arrive. Rezel can split
the LR stack only at matching ambiguity markers:

```lezer
ParenExpression { "(" Expression ")" }
ArrowExpression { "(" ParameterName ")" "=>" Expression }

Expression { Identifier ~paren | ParenExpression | ArrowExpression }
ParameterName { Identifier ~paren }
```

The name after `~` identifies a family of allowed conflicts; it does not itself
choose a winner. Alternatives should converge quickly and remain bounded.

`@dynamicPrecedence` can rank completed competing parses:

```lezer
Preferred[@dynamicPrecedence=1] { ambiguousForm ~choice }
Fallback[@dynamicPrecedence=-1] { ambiguousForm ~choice }
```

The current generator accepts values from -10 through 10. Dynamic precedence
is a stable syntactic preference, not a place for unbounded semantic scoring.
Every GLR site needs a minimal witness, an explanation of the competing
interpretations, and a test for the resulting CST.

## Template and inline rules

Template rules capture repeated grammar patterns:

```lezer
commaSep<content> { (content ("," content)*)? }

Arguments { "(" commaSep<Expression> ")" }
Parameters { "(" commaSep<Parameter> ")" }
```

Arguments are grammar expressions substituted by the generator. They do not
create runtime polymorphism. Each distinct argument set creates a concrete
grammar instance. Do not translate boolean parameters from a recursive-descent
parser into a cross-product of template arguments without first proving that
the resulting languages are genuinely distinct.

An inline rule gives a local production a node name or properties without
adding a reusable declaration:

```lezer
value {
  Number |
  Boolean { "true" | "false" }
}
```

Use named reusable rules when several productions depend on the same syntax;
use inline rules when the identity is local to one choice.

## Node names, groups, and delimiters

Rule and token properties refine generated terms:

- `@name` assigns the emitted node name;
- `@export` gives a term an exported identity;
- `@isGroup` adds a group used by typed or tree consumers;
- `@dialect` restricts a term to a declared dialect;
- `@dynamicPrecedence` ranks GLR results.

Literal tokens can also receive properties inside `@tokens`. `@detectDelim`
asks the generator to infer matching delimiter properties. Explicit
`openedBy` and `closedBy` node properties are available when inference is not
the intended representation.

Properties affect the CST contract. Renaming or regrouping a term can break
typed schemas, highlighters, and snapshots even when accepted source does not
change.

## Dialects

Declare optional syntax dimensions once:

```lezer
@dialects { preview, legacy }

@tokens {
  "preview"[@dialect=preview,@name=PreviewKeyword]
}
```

The language facade enables a dialect when constructing the parser. Dialects
are appropriate for explicit, finite grammar variants. A language version with
many interacting changes is usually clearer as a maintained grammar update
than as an ever-growing set of flags.

## External declarations

Grammar declarations name behavior implemented in Rust:

```lezer
@external tokens indentation from "./tokens" {
  indent,
  dedent
}

@context trackIndent from "./tokens"
@external propSource languageHighlighting from "./highlight"
```

Rezel supports external tokenizers, external specializers and extenders,
context trackers, node properties, and property sources. The source and symbol
names are declarative keys; the
[binding manifest](04-language-adapters.md#binding-manifest) maps them to Rust
paths and rejects missing or unused entries.

Generated and external tokenizers are ordered by their declarations. An
earlier successful non-fallback tokenizer can prevent later tokenizers from
running. Keep the order intentional and test shared prefixes, fallback paths,
and zero-length tokens.

Use an external declaration only for behavior outside the regular-token or CFG
model: indentation, automatic separators, lexical modes with explicit state,
versioned Unicode classification, or computed node metadata. Semantic analysis
does not belong in a tokenizer.

## Check the grammar while it grows

Use the repository-local generator:

```sh
cargo run --locked -p rezel-generator -- check \
  languages/<language>/grammar/<language>.grammar

cargo run --locked -p rezel-generator -- terms \
  languages/<language>/grammar/<language>.grammar
```

`check` compiles the grammar and reports table statistics, warnings, lexical
overlap, and unresolved LR conflicts. Add one vertical slice at a time: tokens,
a path to a top rule, the intended CST, and boundary cases. A successful table
build proves internal consistency; strict, recovery, and reference tests prove
the language claims.

Read its statistics as separate budgets:

- parser states measure the generated LR automaton;
- token states and token edges measure the generated lexical DFA;
- runtime stack and action limits, exercised by tests, measure GLR and recovery
  branching that static table counts do not show.

A grammar can be conflict-free and still be much larger than its language
feature requires. Compare these values before and after each slice. Investigate
template combinations, duplicated caller contexts, scoped skip regions,
keyword token expansion, and ambiguity when growth is disproportionate.

The [Lezer System Guide](https://lezer.codemirror.net/docs/guide/#writing-a-grammar)
provides the original, more detailed grammar manual. Pay particular attention
to [skip expressions](https://lezer.codemirror.net/docs/guide/#skip-expressions),
[template rules](https://lezer.codemirror.net/docs/guide/#template-rules),
[token specialization](https://lezer.codemirror.net/docs/guide/#token-specialization),
[local token groups](https://lezer.codemirror.net/docs/guide/#local-token-groups),
[external tokens](https://lezer.codemirror.net/docs/guide/#external-tokens),
[precedence](https://lezer.codemirror.net/docs/guide/#precedence),
[ambiguity](https://lezer.codemirror.net/docs/guide/#allowing-ambiguity), and
[contextual tokenization](https://lezer.codemirror.net/docs/guide/#contextual-tokenization).
Rezel's generator tests and diagnostics are the authority for the currently
implemented subset and its Rust emission.
