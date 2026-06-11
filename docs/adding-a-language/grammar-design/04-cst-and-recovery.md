# 4. Design the CST and recovery

Recognition answers whether a source belongs to the strict language.
Representation answers what syntax identity consumers observe. Recovery
answers what structure remains useful when recognition fails. A grammar
controls all three, but they should be designed as separate contracts.

## Select persistent syntax

A complete derivation tree contains every nonterminal introduced to express
the grammar. A practical CST is a projection of that derivation: visible rules
remain nodes, while hidden helpers contribute structure without adding a named
layer.

Keep a node when at least one durable consumer needs its identity:

- it denotes a named syntactic construct;
- its source range is meaningful;
- it distinguishes alternatives that cannot be reconstructed reliably;
- it is a stable boundary for typed navigation or highlighting;
- it carries delimiter, group, mount, or other syntax metadata.

Hide a rule when it only:

- factors a shared LR prefix;
- implements precedence or repetition mechanically;
- aliases one child without adding syntax identity;
- exists to make one lowerer avoid a small private adapter;
- exposes a temporary conflict-resolution structure.

Capitalization in Lezer notation implements this choice: uppercase rules are
visible, lowercase rules are not. `@name`, groups, and node properties refine
the visible identity. The
[Writing a Grammar](https://lezer.codemirror.net/docs/guide/#writing-a-grammar)
and [Node Props](https://lezer.codemirror.net/docs/guide/#node-props) sections
describe the original model.

## Keep CST and AST pressures separate

The CST should preserve syntactic distinctions useful to several consumers.
The typed CST names direct-child roles without copying. A private syntax view
can normalize grammar-oriented shapes. An owned AST can omit punctuation,
decode values, and reorganize syntax for a particular public model.

This direction matters:

```text
CST -> typed view -> private normalization -> optional owned AST
```

Changing a persistent node to simplify one AST lowerer can break highlighting,
tree snapshots, ranges, and other consumers. First express the lowerer's need
in the typed schema or a private view.

## Specify punctuation, trivia, and ranges

For each syntax family, decide:

- which punctuation receives a named term;
- which delimiters have `openedBy` and `closedBy` relationships;
- whether separators are children of the list, element, or surrounding node;
- whether comments are skipped, visible, or recovered separately;
- whether modifiers and terminators belong to a node's range;
- whether trailing trivia is inside or outside that range;
- how empty and synthetic recovery nodes are positioned.

All positions remain original UTF-8 byte offsets. A translated lexical view may
change the logical characters seen by tokenization, but it does not change the
coordinate space of the CST.

Use the same rule for every member of a syntax family. Consumers should not
need language-specific exceptions to discover whether a closing delimiter is
inside its parent.

## Treat recovery as a search problem

When the next token has no ordinary LR action, a recovering parser explores
ways to regain a viable parse: inserting missing structure, skipping unexpected
input, or continuing from another stack. Candidates accumulate recovery costs,
and error nodes preserve where the source did not follow the strict grammar.

The selected tree therefore depends on:

- the grammar's available shifts and reductions;
- visible node boundaries;
- lexical token granularity;
- ambiguity and stack merging;
- recovery costs and resource limits.

Lezer's default mode is designed to return a tree even for badly malformed
input. A completed recovering parse is therefore not evidence that the source
belongs to the language. Every negative membership case must run in strict
mode, while a separate assertion checks the recovered tree.

Recovery quality is not simply “accept more.” Loosening the strict grammar can
remove useful error structure and turn an invalid program into an accepted
one. Keep strict membership fixed, then evaluate the recovery tree.

Recovery itself explores alternatives with GLR machinery. Explicit ambiguity,
extending specializers, zero-length tokens, and broad recovery opportunities
can interact and multiply work on malformed input even when valid input is
fast. Put malformed and truncated cases next to every such feature and assert
the relevant stack, action, and recovery limits.

Lezer's
[Error Recovery](https://lezer.codemirror.net/docs/guide/#error-recovery)
section gives the original algorithmic model.

## Define invalid-input families

Recovery should be tested by fault shape, not only by a miscellaneous list of
broken files:

| Fault                      | Expected structural question                           |
| -------------------------- | ------------------------------------------------------ |
| Missing token              | Where is an empty error or insertion boundary placed?  |
| Unexpected token           | Is the skipped source preserved under an error node?   |
| Truncated construct        | Which open construct remains visible at EOF?           |
| Wrong delimiter            | Does recovery stay within the nearest useful boundary? |
| Broken list                | Are unaffected neighbors still separate and ordered?   |
| Invalid lexical form       | Is it a token/recovery error or a fatal input error?   |
| Repeated malformed nesting | Is work bounded and the tree deterministic?            |

For each important family, assert:

- strict rejection and error category;
- the complete or relevant recovered CST;
- original byte ranges around the fault;
- the same result over repeated runs;
- bounded actions, stacks, recovery steps, and token scanning.

AST lowering should accept strict trees only. A recovery-aware editor feature
can operate on typed wrappers because their required accessors still return
`Option`.

## Preserve local structure

Even without cross-edit reuse, local tree structure improves diagnostics,
navigation, and bounded recovery:

- avoid one huge token when the language has meaningful internal boundaries;
- keep repeated syntax in compact generated repetition structures;
- make lexical context changes as local as the language permits;
- choose nodes around constructs that recover independently;
- avoid GLR branches that remain unresolved across large source regions.

These properties also preserve design room for future incremental parsing.
Lezer can reuse unchanged tree fragments, as described in
[Incremental Parsing](https://lezer.codemirror.net/docs/guide/#incremental-parsing).
Rezel does not currently implement changed-range or fragment reuse. That is a
current capability gap, not a permanent exclusion. Language documentation must
not claim incremental parsing unless the runtime later supplies and verifies
it.

The CST and recovery model is coherent when visible nodes follow a stable
consumer policy, punctuation and ranges are consistent, malformed-input
families have deliberate trees, strict acceptance remains separate, and
resource bounds cover the parser's recovery search.
