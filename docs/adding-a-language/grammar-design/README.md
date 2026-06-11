# Grammar design foundations

A grammar is an executable model of a programming language. It connects a
source-character model, token languages, context-free productions, parse
selection, CST representation, and error recovery. Each connection has its own
theory and its own failure modes.

| Design question                    | Foundation                                             | Result encoded by the package               |
| ---------------------------------- | ------------------------------------------------------ | ------------------------------------------- |
| Which source texts are recognized? | Formal languages, specifications, and conformance      | Versioned strict-language contract          |
| How do characters become tokens?   | Regular languages, finite automata, and lexical state  | Token and trivia model                      |
| How do tokens compose?             | Context-free grammars and EBNF                         | Productions and top rules                   |
| Which parse wins?                  | LR items, precedence, associativity, cuts, and GLR     | Explicit conflict decisions                 |
| Where can complexity multiply?     | LR automata, token DFAs, templates, and GLR branching  | Table and runtime resource budgets          |
| Which syntax remains observable?   | Concrete-syntax-tree design                            | Visible nodes, properties, and range policy |
| What happens to invalid source?    | Error recovery and resource bounds                     | Strict/recovery contract                    |
| Why should the model be trusted?   | Conformance, differential testing, and corpus analysis | Layer-specific evidence                     |

The recognized language and the produced CST are separate products. A grammar
refactoring can preserve the accepted strings while changing visible nodes.
Recovery can produce a tree for a string that strict parsing correctly rejects.
An AST can agree with an official model while hiding a CST defect. Design and
verification must keep these claims distinct.

## Reading path

1. [Model the supported language](01-language-model.md)
2. [Design lexing and contextual behavior](02-lexing-and-context.md)
3. [Reason about CFGs, LR states, conflicts, and ambiguity](03-cfg-lr-and-ambiguity.md)
4. [Design the CST and recovery](04-cst-and-recovery.md)
5. [Derive and validate the grammar](05-derivation-and-validation.md)

After the model is clear, use the
[Rezel grammar guide](../02-grammar-syntax.md) to encode it and the remaining
chapters in [Adding a language](../README.md) to package and verify it.

## Deeper Lezer references

Rezel retains Lezer's central grammar and LR/GLR design. The project
documentation is sufficient to add a language; the following official
material develops the original model in more depth:

- [System Guide](https://lezer.codemirror.net/docs/guide/) — parser algorithm,
  recovery, contextual tokenization, grammar notation, and trees;
- [Writing a Grammar](https://lezer.codemirror.net/docs/guide/#writing-a-grammar)
  — exact upstream terms, tokens, skips, precedence, externals, contexts,
  properties, and dialects;
- [Basic example](https://lezer.codemirror.net/examples/basic/) — a small
  grammar-to-CST walkthrough;
- [JavaScript example](https://lezer.codemirror.net/examples/javascript/) —
  precedence and contextual lexical decisions;
- [Indentation example](https://lezer.codemirror.net/examples/indent/) —
  external tokens and context for layout;
- [Testing example](https://lezer.codemirror.net/examples/test/) — source and
  expected-tree fixtures;
- [Reference Manual](https://lezer.codemirror.net/docs/ref/) — exact upstream
  JavaScript APIs.

The upstream API reference describes Lezer's JavaScript implementation. Rezel
uses Rust callbacks, original UTF-8 byte coordinates, checked-in native-endian
tables, and its own public APIs. `rezel-generator` diagnostics and tests define
which grammar constructs currently compile and how they behave in Rezel.
