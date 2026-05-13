#![forbid(unsafe_code)]

use rezel_generator::{BuildOptions, compile_grammar, emit_typed_syntax};

const GRAMMAR: &str = r#"
@top Root { "{" Item? "}" }
Item { "x" }

@tokens { "{" "}" "x" }
"#;

const SCHEMA: &str = r#"
language = "TestLanguage"
kind = "TestKind"
language_path = "crate::generated::LANGUAGE"

[kind_names]
"{" = "LeftBrace"
"}" = "RightBrace"
"⚠" = "Error"

[[node]]
name = "TestRoot"
kind = "Root"

[[node.field]]
name = "left_brace_token"
cardinality = "one"
selector = { token = "{" }

[[node.field]]
name = "item"
type = "TestItem"
cardinality = "optional"
selector = { node = "Item" }

[[node.field]]
name = "right_brace_token"
cardinality = "one"
selector = { token = "}" }

[[node]]
name = "TestItem"
kind = "Item"

[[union]]
name = "TestValue"
variants = [
  { name = "Item", type = "TestItem" },
]

[[union]]
name = "TestNestedValue"
variants = [
  { name = "Value", type = "TestValue" },
]
"#;

#[test]
fn emits_direct_child_wrappers_after_schema_validation() {
    let grammar = compile_grammar(GRAMMAR, None, BuildOptions::default()).unwrap();
    let generated = emit_typed_syntax(&grammar, SCHEMA).unwrap();

    syn::parse_file(&generated).unwrap();
    assert!(generated.contains("pub struct TestRoot"));
    assert!(generated.contains("pub enum TestValue"));
    assert!(generated.contains("pub enum TestNestedValue"));
    assert!(generated.contains("Value(TestValue)"));
    assert!(generated.contains("pub fn item(&self) -> Option<TestItem>"));
    assert!(generated.contains("pub fn left_brace_token"));
    assert!(!generated.contains("child_by_name"));
}

#[test]
fn rejects_recursive_union_dependencies() {
    let grammar = compile_grammar(GRAMMAR, None, BuildOptions::default()).unwrap();
    let recursive = SCHEMA.replace(
        "{ name = \"Value\", type = \"TestValue\" },",
        "{ name = \"Value\", type = \"TestNestedValue\" },",
    );
    assert!(
        emit_typed_syntax(&grammar, &recursive)
            .unwrap_err()
            .message()
            .contains("dependency cycle")
    );
}

#[test]
fn rejects_unknown_kinds_cardinality_drift_and_invalid_occurrences() {
    let grammar = compile_grammar(GRAMMAR, None, BuildOptions::default()).unwrap();

    let unknown = SCHEMA.replace("kind = \"Item\"", "kind = \"Missing\"");
    assert!(
        emit_typed_syntax(&grammar, &unknown)
            .unwrap_err()
            .message()
            .contains("Unknown visible grammar kind")
    );

    let required = SCHEMA.replace(
        "name = \"item\"\ntype = \"TestItem\"\ncardinality = \"optional\"",
        "name = \"item\"\ntype = \"TestItem\"\ncardinality = \"one\"",
    );
    assert!(
        emit_typed_syntax(&grammar, &required)
            .unwrap_err()
            .message()
            .contains("absent from some normalized production")
    );

    let occurrence = SCHEMA.replace(
        "name = \"item\"\ntype = \"TestItem\"\ncardinality = \"optional\"",
        "name = \"item\"\ntype = \"TestItem\"\ncardinality = \"optional\"\noccurrence = 1",
    );
    assert!(
        emit_typed_syntax(&grammar, &occurrence)
            .unwrap_err()
            .message()
            .contains("cannot occur")
    );
}

#[test]
fn complete_schemas_must_account_for_every_visible_semantic_kind() {
    let grammar = compile_grammar(GRAMMAR, None, BuildOptions::default()).unwrap();
    let complete = SCHEMA.replace(
        "language_path = \"crate::generated::LANGUAGE\"",
        "language_path = \"crate::generated::LANGUAGE\"\ncoverage = \"complete\"\nignore = [\"⚠\", \"{\", \"}\", \"x\"]",
    );
    emit_typed_syntax(&grammar, &complete).unwrap();

    let missing = complete.replace(", \"x\"", "");
    assert!(
        emit_typed_syntax(&grammar, &missing)
            .unwrap_err()
            .message()
            .contains("\"x\"")
    );

    let duplicate = complete.replace(
        "ignore = [\"⚠\", \"{\", \"}\", \"x\"]",
        "ignore = [\"⚠\", \"{\", \"}\", \"x\", \"Item\"]",
    );
    assert!(
        emit_typed_syntax(&grammar, &duplicate)
            .unwrap_err()
            .message()
            .contains("more than once")
    );
}

#[test]
fn recursive_hidden_rules_have_bounded_schema_analysis() {
    let grammar = compile_grammar(
        r#"
        @precedence { plus @left }
        @top Root { expression }
        expression { expression !plus "+" expression | Item }
        Item { "x" }
        @tokens { "+" "x" }
        "#,
        None,
        BuildOptions::default(),
    )
    .unwrap();
    let schema = r#"
        language = "RecursiveLanguage"
        kind = "RecursiveKind"
        language_path = "crate::generated::LANGUAGE"

        [[node]]
        name = "RecursiveRoot"
        kind = "Root"

        [[node.field]]
        name = "items"
        type = "RecursiveItem"
        cardinality = "many"
        selector = { node = "Item" }

        [[node]]
        name = "RecursiveItem"
        kind = "Item"
    "#;
    let generated = emit_typed_syntax(&grammar, schema).unwrap();
    assert!(generated.contains("pub fn items"));
}
