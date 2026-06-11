# 1. Model the supported language

The first design object is the language to recognize, not the grammar file.
For one parser entry point, the strict language can be viewed as a set:

```text
L_top ⊆ C*
```

`C` is the logical character alphabet presented to tokenization, and `C*` is
the set of finite character sequences. Each top rule, language version, and
dialect selection may define a different `L_top`.

This mathematical view does not say how to recognize the set or what tree to
build. It provides a stable question for every later decision: does this rule
change membership, representation, recovery, or only a downstream projection?

## Assign each language rule to a layer

Programming-language specifications often mix rules with different
computational structure:

| Layer                     | Examples                                                                   | Rezel owner                            |
| ------------------------- | -------------------------------------------------------------------------- | -------------------------------------- |
| Source characters         | Encoding, line endings, escape translation, normalization                  | Raw `Input` and optional lexical view  |
| Regular lexical language  | Identifiers, numeric literals, punctuation, comments                       | Generated token DFA                    |
| Contextual lexing         | Indentation, automatic separators, lexical modes, parser-dependent tokens  | External tokenizer and context tracker |
| Context-free syntax       | Expressions, declarations, statements, types                               | Grammar productions and LR/GLR tables  |
| Syntactic side conditions | Assignment-target shape, versioned restrictions, whole-source layout rules | Preflight or strict CST validation     |
| Semantic constraints      | Binding, types, imports, constant rules, execution                         | AST or later semantic tooling          |

The classification is not always dictated by the specification. It is an
implementation boundary with a proof obligation. Moving a syntactic side
condition out of the CFG does not remove it from the parser contract; it gives
the condition another named owner and another test surface.

Use the weakest model that states a rule clearly:

- regular rules belong in tokens;
- recursive nesting belongs in the CFG;
- finite parser-relevant history belongs in lexical context;
- whole-tree syntactic predicates belong in strict validation when a CFG
  encoding would be obscure or unsafe;
- name- or type-dependent rules remain semantic.

This separation keeps the grammar understandable without claiming that every
language rule is context-free.

## Define strict acceptance

Strict parsing is a membership decision for the parser-owned syntax. Specify:

- language release, edition, or grammar revision;
- complete-file and fragment entry points;
- enabled dialects and implementation extensions;
- preview or legacy syntax and its default state;
- textual side conditions enforced outside the CFG;
- valid source forms intentionally deferred to a later release.

Two complementary properties guide review:

- **soundness**: every strictly accepted source satisfies the documented
  parser-owned syntax;
- **completeness**: every source satisfying that syntax is accepted.

Neither is established by a few examples. The terms clarify why rejected-case
tests, official parser comparisons, and broad corpora are all needed.

Strict syntax is not complete semantic validity. A source may parse while
containing an unresolved name or type error because those rules belong to a
later layer. Conversely, a rule classified as strict syntactic validation must
still run before strict parsing reports success.

## Version the language dimensions

Language evolution is rarely one linear switch. Separate:

- lexical additions, such as new literal forms;
- production additions and removals;
- contextual-word changes;
- precedence or associativity changes;
- static restrictions;
- CST-only compatibility decisions.

Use a dialect when the variant is explicit, finite, and meant to be selected
at parser construction. Update the maintained grammar when the package moves
its primary language version. Avoid accumulating flags that permit combinations
no real language version defines.

Every version boundary needs at least one positive and one negative witness.
Broad source acceptance must not silently enable an implementation extension
outside the contract.

## Establish authority by claim

Different sources answer different questions:

| Source                            | Strong evidence for                                    | Typical limitation                                  |
| --------------------------------- | ------------------------------------------------------ | --------------------------------------------------- |
| Normative specification           | Intended lexical, syntactic, and static rules          | Editorial notation, prose gaps, or errors           |
| Official compiler or parser       | Observable behavior of one version and mode            | Extensions, deferred diagnostics, unstable trees    |
| Maintained Lezer grammar          | Practical tokenization, CST, and recovery design       | Older language version or editor-oriented choices   |
| Grammar for another parser family | Production structure, lexical modes, known ambiguities | Different choice semantics, actions, and tree model |
| Conformance suite                 | Focused accepted and rejected boundaries               | Incomplete language coverage                        |
| Standard library or source corpus | Real combinations and scale                            | Mostly positive, uneven feature distribution        |

Record each source's version, mode, license, and role. When sources disagree,
reduce the disagreement to a minimal input and decide which contract Rezel
intends to implement. The easiest source to copy is not automatically the
authority.

## Define the tree mapping separately

For strict parsing, the package also defines a mapping:

```text
parse_top : L_top -> CST
```

In practice, explicit GLR preferences and dynamic precedence make the selected
tree deterministic for the supported grammar. The mapping still requires its
own contract:

- visible syntax nodes and groups;
- invisible grammar helpers;
- named punctuation and comments;
- delimiter and list structure;
- original UTF-8 byte ranges;
- differences from an inherited Lezer CST.

Inlining a lowercase helper may leave this mapping unchanged. Renaming an
uppercase rule may change it without changing `L_top`. Typed syntax,
highlighting, and AST lowering depend on the mapping, so tree changes deserve
separate review.

Recovering parsing defines a broader operation:

```text
recover_top : C* -> CST | fatal resource/input error
```

It aims for deterministic useful structure, not membership. Recovery errors,
inserted or skipped syntax, and bounded work are part of this second contract.

## Turn the model into cases

For every top rule, collect:

- a minimal member of `L_top`;
- a boundary member for each lexical and syntactic family;
- a minimally invalid near-member;
- version and dialect pairs that differ by one feature;
- malformed source for each important recovery strategy;
- non-ASCII source with expected byte ranges.

Keep each case small enough to identify the owning layer. Larger conformance
and corpus inputs then test coverage rather than serving as the only
explanation of behavior.

The model is sufficiently defined when strict membership, source coordinates,
CST identity, recovery behavior, layer ownership, and evidentiary authority can
all be stated without referring to an unfinished grammar implementation.
