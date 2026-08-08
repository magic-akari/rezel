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

| Form                                | Meaning                   |
| ----------------------------------- | ------------------------- |
| `a b`                               | sequence                  |
| `a \| b`                            | choice                    |
| `a?`                                | zero or one               |
| `a*`                                | zero or more              |
| `a+`                                | one or more               |
| `( ... )`                           | grouping                  |
| `"text"` or `'text'`                | literal token             |
| `rule<arg>`                         | template-rule application |
| `Name { ... }` inside an expression | local inline rule         |

Choice is not ordered. `a | b` and `b | a` recognize the same language; the
branch order of a hand-written parser must be translated into an explicit
token, production, precedence, cut, ambiguity, or validation decision when it
is semantically significant.

Postfix repetition binds most tightly, sequence binds next, and `|` choice
binds least tightly. Thus `a b | c` means `(a b) | c`, while `a (b | c)` needs
parentheses. An empty string literal denotes the empty sequence, or epsilon,
in a nonterminal expression.

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

Literal adjacency depends on the grammar region. In a nonterminal, `"a" "b"`
is a sequence of two terminal tokens. Inside a token rule it is one token
matching the concatenated text `"ab"`. Token rules may refer to other token
rules, but a reference cycle is valid only through tail-recursive positions;
an arbitrary recursive cycle would no longer describe a regular language.

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

`@conflict { A, B }` takes exactly two token references—named token-rule
instances or literal tokens—and makes them incompatible in one generated main
tokenizer group. The generator reports an error when one LR state or skip
context needs both. It does not choose a winner. Use it to make expected
parser-context separation checkable rather than relying on incidental LR
states.

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
the binding manifest. Multiple generated or external specializers may share a
base token. They run in declaration order and stop at the first result allowed
by the active dialect; declining or dialect-disabled results continue to the
next specializer.

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

The marker `!multiply` consumes no input and creates no CST node. When a
shift/reduce or reduce/reduce conflict compares positions carrying precedence
markers, their order in the top-level `@precedence` block selects the result.
`@left` and `@right` resolve self-association. A precedence without an
associativity annotation does not decide a conflict with itself.

`@cut` changes when that selection happens. Declare it on a precedence name,
then use the same ordinary `!name` marker in a production:

```lezer
@precedence { block @cut }

statement {
  Block { !block "{" statement* "}" } |
  expression ";"
}
```

There is no separate `@cut` expression at the use site. Crossing `!block`
commits to that interpretation before an ordinary LR conflict necessarily
appears and prunes competing interpretations that do not carry an equal or
stronger cut. When several cut levels meet, their order in `@precedence`
applies just as it does for ordinary levels: the earlier declaration is
stronger. A pruned path cannot become valid again after more input arrives,
which can also change error recovery. Use a cut only where the syntax itself
commits to the selected construct; it should not hide an incorrect language
model.

The available syntactic selection mechanisms are related but not
interchangeable:

| Mechanism                        | When it acts               | Purpose                                         |
| -------------------------------- | -------------------------- | ----------------------------------------------- |
| `!name` with ordinary precedence | while building LR actions  | resolve a statically known parser conflict      |
| `@left` or `@right`              | while building LR actions  | resolve equal-precedence self-association       |
| `!name` declared with `@cut`     | when the marker is crossed | commit early and prune other interpretations    |
| `~name`                          | at an allowed LR conflict  | retain multiple bounded GLR interpretations     |
| `[@dynamicPrecedence=integer]`   | when its rule reduces      | add a score used to rank competing parse stacks |

Start with an unambiguous CFG, use static precedence for operator-like
conflicts, and reserve cuts and GLR markers for decisions justified by a
minimal source witness.

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

`@dynamicPrecedence` adds to a parse stack's score whenever the annotated rule
reduces:

```lezer
Preferred[@dynamicPrecedence=1] { ambiguousForm ~choice }
Fallback[@dynamicPrecedence=-1] { ambiguousForm ~choice }
```

The runtime uses the cumulative score while merging equivalent stacks, pruning
stack limits, and choosing a final parse. The current generator accepts values
from -10 through 10. Dynamic precedence is a stable syntactic preference, not
a place for unbounded semantic scoring. It ranks ambiguity that already
exists; it does not create a GLR split. Every GLR site needs a minimal witness,
an explanation of the competing interpretations, and a test for the resulting
CST.

## Distinguish overloaded grammar symbols

Several spellings have unrelated parser and tokenizer meanings. Read them in
the grammar region where they occur:

| Form                                       | Meaning                                       |
| ------------------------------------------ | --------------------------------------------- |
| `expression !add "+" expression`           | parser precedence or cut marker               |
| `lineContent { ![\n]* }`                   | token character set excluding newline         |
| top-level `@precedence { add @left, ... }` | parser precedence, associativity, and cuts    |
| `@precedence { A, B }` inside `@tokens`    | lexical precedence between overlapping tokens |

Thus `!add` is a zero-width annotation on a CFG sequence, whereas `![\n]`
matches one code point from a negated token set. Top-level parser precedence
orders grammar decisions and is referenced with `!name`. Token precedence is
local to `@tokens` or `@local tokens`, orders overlapping token matches from
highest to lowest, and is not referenced by parser markers.

`@` itself is a namespace prefix used in several syntactic positions, not one
operation:

| Position                    | Examples                                                                                | Role                                            |
| --------------------------- | --------------------------------------------------------------------------------------- | ----------------------------------------------- |
| top level                   | `@top`, `@skip`, `@tokens`, `@local tokens`, `@dialects`, `@precedence`, `@detectDelim` | declare grammar-wide structure or parser policy |
| top-level external boundary | `@external`, `@context`                                                                 | name behavior supplied by a binding             |
| inside `@tokens`            | `@precedence`, `@conflict`                                                              | control overlap or assert token separation      |
| inside `@local tokens`      | `@precedence`, `@else`                                                                  | control overlap or declare local fallback       |
| inside a token expression   | `@asciiLetter`, `@digit`, `@whitespace`, `@eof`                                         | match a built-in character class or EOF         |
| inside a grammar expression | `@specialize`, `@extend`                                                                | reclassify a scanned base token                 |
| after a precedence name     | `@left`, `@right`, `@cut`                                                               | configure a parser precedence level             |
| inside square brackets      | `@name`, `@inline`, `@isGroup`                                                          | configure generation through a pseudo-prop      |

The surrounding region therefore determines the meaning. For example,
top-level `@external extend` imports a callback, while expression-level
`@extend<Base, "word">` uses a declarative specialization table.

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

A local inline rule gives a production a local node name or properties without
adding a reusable top-level declaration:

```lezer
value {
  Number |
  Boolean { "true" | "false" }
}
```

Here `Boolean { ... }` is a complete nested rule. `Boolean` is emitted as a CST
node, but no other production can refer to it. Local inline rules cannot take
template parameters. Use a named reusable rule when several productions depend
on the same syntax; use a local inline rule when the node identity is local to
one choice.

The `@inline` pseudo-property is a different feature:

```lezer
qualifiedPrefix[@inline] {
  "::" (Name "::")* |
  (Name "::")+
}

QualifiedName { qualifiedPrefix Name }
```

Before constructing the LR automaton, the generator expands the alternatives
of this eligible `qualifiedPrefix` into each caller. The expanded rule has no
CST node or independent reduction boundary. This can expose the caller's
surrounding tokens to LR conflict resolution, and Rezel also performs a similar
automatic optimization for some small hidden rules.

`[@inline]` takes no value and applies only to nonterminals. Combining it with
ordinary node properties or dynamic precedence is an error. `@name` cannot
create a node for the eliminated boundary. On a non-parameterized top-level
rule, `@export` is currently accepted and registers a term identity, but that
identity does not restore the eliminated reduction boundary; avoid this
misleading combination. Parameterized rules and nested local inline rules have
separate export restrictions. `@isGroup` remains meaningful: it assigns a
group to the named nodes produced by the helper's choices, not to the
eliminated helper itself. A dialect is token metadata and is not accepted on
any nonterminal.

Inlining is defined for finitely substitutable helpers. The current generator
does not reject every recursive case: a directly self-referential marked rule
stays as a hidden nonterminal, while an indirect recursive cycle may be
partially expanded through another rule. Do not apply `[@inline]` to any rule
in a direct or indirect recursive cycle.

Inlining copies productions rather than calling a runtime helper. A rule with
several alternatives, used several times or combined with other inlined
choices, can multiply productions and LR states. Use it for a demonstrated
grammar boundary problem or a small structural helper, then compare generator
statistics before and after the change.

## Node names, groups, and delimiters

Square brackets hold two distinct kinds of metadata. Names beginning with `@`
are generator pseudo-properties that change grammar construction:

| Pseudo-property      | Effect                                                  |
| -------------------- | ------------------------------------------------------- |
| `@name`              | override the emitted node or token name                 |
| `@export`            | give a generated term a stable exported identity        |
| `@inline`            | expand a nonterminal into its callers                   |
| `@isGroup`           | define a group from the named nodes produced by a rule  |
| `@dialect`           | restrict a token to one declared dialect                |
| `@dynamicPrecedence` | assign a GLR score from -10 through 10 to a nonterminal |

Names without `@` are values stored on generated node types. Rezel has four
built-in node properties:

| Node property | Effect                                                         |
| ------------- | -------------------------------------------------------------- |
| `group`       | add the node type to one or more named consumer groups         |
| `closedBy`    | list the closing-delimiter node names for an opening delimiter |
| `openedBy`    | list the opening-delimiter node names for a closing delimiter  |
| `isolate`     | request bidirectional-text isolation (`auto`, `ltr`, or `rtl`) |

Bare `[isolate]` means `auto`. This property is display metadata for Unicode
bidirectional text. It does not create a tokenizer mode, isolate a skip region,
or control embedded-language parsing.

Literal and external tokens can receive applicable properties in their token
sets. The top-level `@detectDelim` directive is not a property; it asks the
generator to infer `openedBy` and `closedBy` relationships for conventional
delimiter pairs. Declare those properties explicitly when inference is not the
intended CST contract. Additional property types must first be introduced with
`@external prop`.

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
@external tokens scanLayout from "./tokens" {
  virtualOpen,
  virtualClose,
  @conflict { Identifier }
}

@external specialize { Identifier } classifyWord from "./tokens" {
  ReservedWord
}

@external extend { Identifier } classifySoftWord from "./tokens" {
  ContextWord
}

@external prop rustProperty as grammarProperty from "./props"
@external propSource nodeMetadata from "./props"
@context trackLayout from "./tokens"
```

The complete set of external declaration forms is:

| Declaration                     | Bound Rust behavior                                       |
| ------------------------------- | --------------------------------------------------------- |
| `@external tokens`              | scan input and return one declared external token         |
| `@external specialize { Base }` | replace a recognized base token with a returned term      |
| `@external extend { Base }`     | retain both the base token and returned interpretation    |
| `@external prop`                | introduce a custom node-property type, optionally aliased |
| `@external propSource`          | compute properties for generated node types               |
| `@context`                      | maintain parse-stack context read by external callbacks   |

`@context` is not spelled `@external`, but it is an external binding just like
the five `@external` forms. The current grammar format accepts at most one
context tracker. A tracker belongs to parsing and tokenization; it is not a
place for semantic name or type state. Any adapter callback that receives a
`Stack`, including an external specializer, can read the tracked value.

An external tokenizer reads the current input position directly. An external
specializer does not: it runs after the expression inside `{ Base }` has
already produced a token. A specializing result replaces that base term,
whereas an extending result retains both terms and may create a GLR branch when
both are accepted by the current state.

`@external prop rustProperty as grammarProperty` binds the Rust property symbol
`rustProperty` and exposes it to grammar nodes as `grammarProperty`; without
`as`, both names are the same. This introduces a property type but does not
attach it to any node. A property source instead computes assignments over the
generated node types. Neither form scans input or changes the accepted CFG.

The source and symbol names are declarative keys rather than runtime
module-loading instructions. The
[binding manifest](04-language-adapters.md#binding-manifest) maps them to Rust
paths and rejects missing or unused entries.

The trailing set on `@external tokens`, `@external specialize`, and
`@external extend` is a closed contract: it enumerates every term that the Rust
callback may return. Entries can carry token properties such as `@name` or
`isolate`. The list does not rank the terms. An external tokenizer is relevant
only in LR states that can accept at least one term in its set.

An `@conflict { TokenName }` entry inside an external-token set asserts that no
term from that external group may be active in the same LR state as the named
token. The generator rejects a state that violates the assertion. It is an
expected-separation check, not a runtime preference. This differs from the
pairwise `@conflict { A, B }` declaration inside `@tokens`, though both protect
an intentional tokenizer-state separation. The external form currently checks
only names that resolve to another declared terminal and silently ignores an
unknown or nonterminal name, so verify its spelling while auditing the grammar.

The relative source order of `@external tokens` declarations and the main
`@tokens` block defines tokenizer priority. An earlier successful non-fallback
tokenizer can prevent a later tokenizer from running. Keep that order
intentional and test shared prefixes, fallback paths, context-dependent paths,
and zero-length tokens. Runtime tokenizer flags and Rust binding shapes are
specified in [language adapters](04-language-adapters.md).

The pinned
[external-tokenizer case study](case-studies/javascript-externals.md)
shows how to audit all declarations in one maintained upstream grammar. Its
coverage table explicitly lists the external forms that the example does not
exercise.

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
