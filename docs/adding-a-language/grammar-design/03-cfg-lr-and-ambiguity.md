# 3. Reason about CFGs, LR states, conflicts, and ambiguity

Tokens provide a terminal alphabet. A context-free grammar describes how those
terminals form recursive syntax:

```text
G = (N, T, P, S)
```

`N` is the set of nonterminals, `T` the terminal tokens, `P` the productions,
and `S` a start symbol. Each Rezel top rule supplies a start condition over the
shared grammar.

A production has one nonterminal on its left:

```text
A -> α
```

Choice, optionality, and repetition in the grammar notation are EBNF
conveniences that the generator lowers to productions. Template rules perform
generation-time substitution; they do not increase the recognized language
class.

## Normalize a specification into parser productions

Specifications are written for readers, not for a particular parser. Before
encoding them:

- expand parameterized prose conditions into explicit alternatives;
- make version guards and dialect choices visible;
- separate lexical definitions from syntactic productions;
- remove semantic actions and assign their checks to another layer;
- preserve useful left recursion for LR parsing;
- distinguish editorial nonterminals from persistent CST nodes.

Do not mechanically port transformations required by another algorithm. An LL
grammar may remove left recursion; a PEG may rely on ordered choice; a
tree-sitter grammar may attach dynamic precedence or scanner behavior with
different semantics. Recover the intended language first.

## Translate recursive descent by meaning

A hand-written recursive-descent parser often combines several responsibilities
inside one procedure:

- recognizing a context-free production;
- using lookahead to choose an implementation path;
- carrying parser-mode or caller-state flags;
- reporting a targeted diagnostic;
- enforcing a condition that is only syntactic by convention;
- constructing or normalizing an AST.

Its call graph is therefore not a grammar. In particular, CFG choice is
commutative: `a | b` recognizes the same language as `b | a`. The order of
`if` or `switch` branches in a recursive-descent parser has no effect after a
mechanical translation unless the language decision is represented explicitly
through tokens, productions, precedence, a cut, bounded ambiguity, or
validation. Lezer's
[Operators](https://lezer.codemirror.net/docs/guide/#operators) section makes
this property of grammar choice explicit.

Parameterized parser methods are a common source of accidental grammar
growth:

```text
parseExpression(allowIn, allowYield, stopAtColon)
```

A hand-written parser can reuse one function and branch on three booleans.
Encoding every combination as a distinct nonterminal can produce up to eight
grammar variants before caller contexts and lookahead are considered. Lezer
template rules do not provide runtime parameters—they are copied for every
distinct argument set—so a template matrix preserves this multiplication
rather than removing it. See Lezer's
[Template Rules](https://lezer.codemirror.net/docs/guide/#template-rules).

Classify each implementation branch before translating it:

| Recursive-descent construct                 | Language question                                | Rezel representation                                      |
| ------------------------------------------- | ------------------------------------------------ | --------------------------------------------------------- |
| Branch selected by the next syntactic token | Which production begins here?                    | Token and CFG choice                                      |
| Ordered fallback between overlapping forms  | Is there a real lexical or syntactic priority?   | Token precedence, specialization, precedence, or cut      |
| Caller flag that changes accepted syntax    | Is it an entry point, dialect, or local context? | Top rule, finite dialect, contextual token, or production |
| Predicate over already recognized structure | Is the condition context-free?                   | Refactored CFG or strict validation                       |
| AST allocation or normalization             | Which syntax identity must remain observable?    | CST design, typed view, or lowering                       |
| Targeted error branch                       | Does it change membership or only diagnostics?   | Strict rejection and recovery tests                       |
| Cached lookahead or control-flow shortcut   | Is it only an implementation optimization?       | No grammar representation                                 |

The official parser remains evidence for every row, but Rezel should encode the
language decision rather than reproduce its procedural mechanism.

## Read LR states as viable prefixes

An LR item records progress through one production:

```text
A -> α · β
```

The parser has recognized `α` and expects `β`. If a terminal follows the dot,
the table may shift that terminal and enter another state. If the dot is at the
end, the table may reduce the recognized right-hand side to `A`. LR(1)
lookahead records which next terminals permit the reduction.

A parser state represents a set of such items—a set of syntactic possibilities
consistent with the prefix read so far. Conflicts arise when one state and
lookahead permit more than one action:

- **shift/reduce**: continue the current possibility or complete a production;
- **reduce/reduce**: complete either of two productions.

The competing items reveal the actual decision. The source line where the
generator reports a conflict is only where that decision became unavoidable.

Rezel constructs a canonical LR(1) automaton and then applies conservative
compatible-state merging. Lezer's
[Parser Algorithm](https://lezer.codemirror.net/docs/guide/#parser-algorithm)
explains the wider LR/GLR model.

## Control static and dynamic growth

Grammar growth appears in three different places and should be measured
separately:

| Budget                     | Typical source                                                                              | Observable evidence                                       |
| -------------------------- | ------------------------------------------------------------------------------------------- | --------------------------------------------------------- |
| LR parser states and table | Copied productions, template or inline expansion, lookahead distinctions, and skip contexts | `rezel check` parser-state count and generated table size |
| Token DFA states and edges | Large literal vocabularies, duplicated character logic, and complex token overlap           | `rezel check` token-state and token-edge counts           |
| Runtime parse stacks       | Explicit ambiguity, extending specializers, and recovery search                             | Stack/action limits, adversarial tests, and profiles      |

A conflict-free grammar can still be unnecessarily large. Each production
position contributes LR items; copied context variants duplicate those
positions, and different lookahead or skip contexts can prevent otherwise
similar states from merging. Expanding a helper with several alternatives at
several call sites can multiply productions before LR states are constructed.
Lezer also permits only one skip expression for a parse state, so the boundary
of a scoped skip region is part of the automaton design.

Prefer:

- one shared nonterminal for one shared syntactic language;
- useful left recursion instead of LL-style elimination carried over from a
  recursive-descent implementation;
- `*` and `+` for genuine repetition instead of hand-unrolled list lengths;
- specialization for keyword classification instead of putting every keyword
  path into the token DFA;
- narrow contextual tokenization or strict validation instead of a
  cross-product of grammar flags;
- small, clearly delimited scoped-skip regions;
- local GLR sites whose alternatives converge quickly.

There is no universal correct state count. Record the output of `rezel check`
while adding vertical slices and investigate changes that are disproportionate
to the syntax added. A lower count is not automatically better—factoring can
obscure the CST or move complexity into tokenizers—but unexplained
multiplication is a design defect.

## Distinguish conflict from ambiguity

A grammar is ambiguous when at least one terminal string has more than one
valid parse tree. A table conflict means the selected LR construction cannot
choose one action for a state and lookahead without more information.

These are related but not equivalent:

- an ambiguous grammar requires a selection policy;
- an unambiguous grammar can still produce an LR conflict for a particular
  parser construction;
- refactoring can remove a conflict without changing the language;
- precedence can intentionally select one tree from an ambiguous expression
  grammar;
- a lexical overlap can masquerade as a syntactic conflict.

Always reduce the conflict to a minimal terminal or source witness before
choosing a mechanism.

## Distinguish selection mechanisms from grammar transformation

Precedence, cuts, ambiguity markers, dynamic precedence, and `[@inline]` are
sometimes discussed together because all can change a generator's conflict
report. They act at different stages and make different promises:

| Mechanism                                               | When it acts                                                               | What it does                                                                                  | Appropriate use                                                                        |
| ------------------------------------------------------- | -------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `!level`, optionally with `@left` or `@right`           | Parse-table construction                                                   | Selects among conflicting LR actions using a declared static order and optional associativity | A local operator hierarchy or another stable syntactic priority                        |
| A precedence level declared `@cut` and used as `!level` | At the marked production position, before an ordinary conflict is required | Commits to the marked interpretation and removes competing interpretations                    | A prefix after which the syntax has unconditionally selected one construct             |
| Matching `~group` markers                               | Parse-table construction and the corresponding runtime conflict            | Declares a conflict intentional so GLR may retain the competing stacks                        | A shared prefix whose later, nearby syntax makes the decision                          |
| `[@dynamicPrecedence=n]`                                | On each reduction of the annotated rule                                    | Adds a bounded score used to rank competing live and finished stacks                          | A documented syntactic preference among intentional ambiguous interpretations          |
| `[@inline]`                                             | Before LR states are constructed                                           | Substitutes an eligible helper's productions at its call sites                                | Removing an invisible helper reduction boundary or exposing caller-local CFG structure |

The first four mechanisms govern selection or delayed selection. `[@inline]`
does neither. It is a generation-time CFG transformation. For a non-recursive
helper such as:

```lezer
qualifiedTail[@inline] { "::" Name | "." Name }
Reference { Name qualifiedTail? }
```

the generator expands the helper alternatives into the productions that call
it. This may expose lookahead or precedence positions that were hidden behind
the helper reduction. It may also duplicate alternatives across call sites and
therefore increase—or occasionally decrease—the LR state count and table size.
It is not a runtime optimization, an ordered choice, or a way to prefer one
parse. Recursive helpers cannot generally be eliminated by finite
substitution. The current generator filters direct self-reference but does not
compute recursive strongly connected components: a directly recursive marked
rule remains a hidden reduction boundary, while an indirect cycle may be
partially expanded through another rule. Avoid `[@inline]` throughout a
recursive cycle.

Inlining must preserve the externally observable contract: the same strict
source language and the same visible CST nodes, ranges, groups, and properties.
Use it only for an invisible grammar helper, then compare strict accept/reject
cases and CST snapshots before and after the change. Recovery paths can still
change because the LR states have changed, so keep recovery witnesses as well
and record any material table-size movement reported by `rezel check`.

Choose among the actual decision mechanisms by their semantic timing:

1. Use static precedence when the language defines a context-local hierarchy
   that is known at the conflict point.
2. Use a cut only when crossing the marker makes the competing interpretation
   invalid, not merely less desirable. Test malformed input after the cut
   because the discarded path cannot reappear during recovery.
3. Use an ambiguity marker when later syntax must decide. Bound the number of
   stacks and the distance to convergence.
4. Add dynamic precedence only when ambiguity is already intentional. Its
   cumulative score participates in merging, stack-limit pruning, and final
   selection, so keep it small and derive the preference from the CST contract;
   the score does not create the GLR split.
5. Use `[@inline]` only to reshape the generated CFG. If the desired result is
   “choose this interpretation,” use or repair one of the preceding mechanisms
   instead.

## Resolve the language decision before the table

Classify each conflict in this order:

1. **Wrong membership model.** Both alternatives should not be accepted; fix
   tokens or productions.
2. **Hidden lexical distinction.** A contextual token, specialization, or
   lexical mode should separate the alternatives.
3. **Shared prefix.** Both are valid and clearer factoring exposes where they
   diverge.
4. **Operator hierarchy.** Precedence and associativity define the selected
   expression tree.
5. **Syntactic commitment.** A cut expresses a point where the language has
   committed to one construct.
6. **Local delayed decision.** A bounded GLR split keeps alternatives until
   nearby syntax distinguishes them.
7. **Non-CFG condition.** Strict validation or a semantic layer owns the
   decision.

A precedence marker is not evidence that the selected parse is correct. Its
name and location should correspond to a rule explainable from the language
contract.

## Derive precedence as data

For each operator family, build a table independent of generator diagnostics:

| Property      | Questions                                                                      |
| ------------- | ------------------------------------------------------------------------------ |
| Level         | Which operators bind more tightly?                                             |
| Associativity | Does equal precedence group left, right, or not chain?                         |
| Fixity        | Prefix, infix, postfix, or mixfix?                                             |
| Domain        | Expression, type, pattern, or another subgrammar?                              |
| Boundaries    | How does it interact with assignment, conditionals, casts, calls, or indexing? |
| Authority     | Which specification rule and minimal cases establish it?                       |

Grammar precedence markers implement this table. `@left` and `@right` resolve
equal-level association. A precedence level without associativity leaves
self-conflicts unresolved when both groupings remain possible.

A cut is different: it commits to a production before an ordinary conflict
must appear. Dynamic precedence is also different: each annotated reduction
adds to the score of its live parse stack. Neither should encode an opaque
semantic preference.

See Lezer's
[Precedence](https://lezer.codemirror.net/docs/guide/#precedence) section for
the original notation.

## Use GLR for bounded delayed choice

GLR keeps more than one LR stack when an explicitly annotated ambiguity is
reached. It is appropriate when a shared prefix has several syntactic
interpretations and later nearby terminals select the valid one.

A sound GLR site has:

- a named family of competing interpretations;
- a small, bounded branching factor;
- a reason deterministic LR cannot decide at that position;
- a nearby convergence or rejection point;
- a stable preference if multiple parses remain complete;
- resource tests for nesting and repetition.

If alternatives remain valid for an arbitrarily long suffix, their cost may
multiply. If several parses remain valid at EOF, the language or CST contract
must say which tree wins. Do not depend on incidental stack order.

Dynamic precedence can rank competing live and completed parses within the
documented small range. It cannot replace type information or an unbounded
global score.
Lezer's
[Allowing Ambiguity](https://lezer.codemirror.net/docs/guide/#allowing-ambiguity)
section gives the original ambiguity-marker model.

## Keep a conflict record

For every non-obvious conflict, preserve:

| Field                    | Content                                                           |
| ------------------------ | ----------------------------------------------------------------- |
| Minimal source           | Shortest stable witness                                           |
| LR choice                | Shift/reduce or competing reductions                              |
| Language interpretations | Meaning of each branch                                            |
| Resolution               | Refactoring, precedence, cut, GLR, lexical context, or validation |
| Authority                | Specification, reference parser, or explicit CST policy           |
| Evidence                 | Strict, tree, recovery, and resource cases                        |

Temporary analysis can live outside the final documentation. The grammar should
retain meaningful marker names, and stable surprising decisions should remain
near the language package so later updates do not “simplify” them back into a
bug.

The syntactic model is ready to encode when each top rule reaches a production
graph, operator hierarchy is explicit, conflicts have language explanations,
GLR sites are local and bounded, and every contextual or semantic decision has
an owner outside the CFG.
