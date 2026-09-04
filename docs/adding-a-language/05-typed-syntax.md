# 5. Define the typed CST

The generic CST preserves syntax without assigning a Rust type to each grammar
kind. A typed schema adds that mapping. The generated API remains a zero-copy
view: wrappers hold `SyntaxNode` handles, fields search direct children, and
unions downcast existing nodes.

The schema is checked against the normalized grammar before Rust is emitted.
It therefore documents and enforces tree shape rather than merely renaming
nodes after generation.

## Schema header

Create `grammar/<language>.typed.toml`:

```toml
language = "ExampleLanguage"
kind = "ExampleKind"
language_path = "crate::generated::LANGUAGE"
coverage = "complete"
ignore = ["⚠", "{", "}", ",", ":"]

[kind_names]
"⚠" = "Error"
"{" = "LeftBrace"
"}" = "RightBrace"
":" = "Colon"
"," = "Comma"
```

`language` names the generated zero-sized type implementing
`SyntaxLanguage`. `kind` names the generated enum for visible grammar kinds.
`language_path` points to the generated static language definition.

`kind_names` maps grammar names that are not valid or useful Rust variants to
stable Rust identifiers. It is commonly used for punctuation and the error
term.

`coverage` is `partial` by default. With `complete`, every visible grammar kind
must appear in exactly one concrete typed node or in `ignore`. Complete
coverage catches CST changes that would otherwise leave the public typed API
silently incomplete. Ignored kinds remain available as generic syntax nodes
and kind enum variants.

## Concrete nodes and fields

Declare one wrapper for each concrete node exposed by the API:

```toml
[[node]]
name = "ExampleObject"
kind = "Object"

[[node.field]]
name = "left_brace_token"
cardinality = "one"
selector = { token = "{" }

[[node.field]]
name = "properties"
type = "ExampleProperty"
cardinality = "many"
selector = { node = "Property" }

[[node.field]]
name = "right_brace_token"
cardinality = "one"
selector = { token = "}" }
```

`name` is the Rust wrapper type. `kind` is the visible grammar kind it wraps.
A field selects direct children only. It declares exactly one selector:

- `{ node = "Kind" }` selects a concrete grammar kind and requires `type` to
  name its typed wrapper;
- `{ union = "ExampleValue" }` selects any member of a typed union and requires
  the same union as `type`;
- `{ token = "{" }` selects a visible token and does not use `type`.

The generator rejects unknown or ambiguous grammar names and checks that a
declared concrete type wraps exactly the selected kind.

## Cardinality and occurrence

Cardinality describes the selected child in every normalized production of
the parent:

| Value      | Schema meaning                                                           | Generated accessor                  |
| ---------- | ------------------------------------------------------------------------ | ----------------------------------- |
| `one`      | The selected occurrence is required by every valid production            | `Option<T>` or `Option<SyntaxNode>` |
| `optional` | The occurrence is present in some valid productions and absent in others | `Option<T>` or `Option<SyntaxNode>` |
| `many`     | The typed node or union may occur repeatedly                             | `TypedChildren<T>`                  |

Even a `one` accessor returns `Option`: recovering trees can omit required
syntax. The cardinality is still valuable because the generator proves the
strict grammar shape.

`occurrence` is a zero-based selector for several direct children with the same
kind:

```toml
[[node.field]]
name = "left"
type = "ExampleExpression"
cardinality = "one"
occurrence = 0
selector = { union = "ExampleExpression" }

[[node.field]]
name = "right"
type = "ExampleExpression"
cardinality = "one"
occurrence = 1
selector = { union = "ExampleExpression" }
```

It defaults to zero. A `many` field cannot specify another occurrence, and a
token field cannot use `many`.

The generator expands hidden rules and repetitions while checking these
claims. It rejects a required field missing from any production, an optional
field that is actually always required, an impossible occurrence, or a
repeated field whose target can never repeat.

## Unions

Use unions for grammar alternatives that callers should handle as one role:

```toml
[[union]]
name = "ExampleValue"
variants = [
	{ name = "Object", type = "ExampleObject" },
	{ name = "Array", type = "ExampleArray" },
	{ name = "String", type = "ExampleString" },
]
```

Each variant has a public Rust variant name and a target concrete node or
another union. Targets must cover disjoint, known grammar kinds. Union
dependencies must be acyclic.

Prefer a union over a generic `SyntaxNode` when the grammar defines a closed set
of syntactic roles. Do not use a union to collapse semantically unrelated
constructs merely because they occur under the same parent.

## Design fields around grammar roles

Typed fields should expose stable grammatical relationships:

- declaration name, modifiers, parameters, and body;
- expression receiver, operator, operands, and arguments;
- statement condition and branches;
- list elements and meaningful delimiter tokens.

They should not perform text decoding, constant evaluation, name resolution,
or AST normalization. If a role requires searching arbitrary descendants or
combining several CST shapes, put that interpretation in a private `syntax/`
view. Direct-child fields remain predictable and cheap.

Changing a visible grammar node can change schema coverage, cardinality, or
union membership. Treat the grammar and schema as two views of one CST
contract and review them together.

## Generate and test

The language's central code-generation scope validates the typed schema and
emits `typed.rs` in the same operation as the other parser artifacts:

```sh
mise run codegen:rezel:scope <language> --update
mise run codegen:rezel:scope <language> --check
```

The update task writes the complete expected set; the check task reconstructs
it and compares `typed.rs` with the committed file. Add package-local
behavioral tests that:

1. strictly parse representative source;
2. downcast the top node through `TypedNode::downcast_from`;
3. navigate required, optional, repeated, token, and union fields;
4. assert source ranges for selected nodes and tokens;
5. verify that wrong-kind downcasts fail;
6. exercise missing children on a recovery tree without panicking.

Re-export the intended wrappers and `TypedNode` from the language facade. Do
not hand-edit `typed.rs`; change the grammar or schema and regenerate.
