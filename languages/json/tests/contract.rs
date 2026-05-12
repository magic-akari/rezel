use rezel_common::{ParseErrorKind, closed_by_prop, opened_by_prop};

#[test]
fn parser_exposes_utf8_ranges_and_delimiter_properties() {
    let source = r#" { "你好": [true, "值", null, -12.5e+2] } "#;
    let tree = rezel_lang_json::parser()
        .with_strict(true)
        .parse(source)
        .expect("representative JSON text");
    assert_eq!(usize::from(tree.len()), source.len());

    let object = tree
        .top_node()
        .child_by_name("Object")
        .expect("JSON object");
    let property = object.child_by_name("Property").expect("JSON property");
    let property_name = property
        .child_by_name("PropertyName")
        .expect("JSON property name");
    let range = property_name.range();
    assert_eq!(
        &source[usize::from(range.start())..usize::from(range.end())],
        r#""你好""#,
    );

    let left_brace = object.child_by_name("{").expect("opening brace");
    assert_eq!(
        left_brace.prop(closed_by_prop()),
        Some(vec!["}".to_owned()]),
    );
    let right_brace = object.child_by_name("}").expect("closing brace");
    assert_eq!(
        right_brace.prop(opened_by_prop()),
        Some(vec!["{".to_owned()]),
    );
}

#[test]
fn parser_defaults_to_recovery_and_supports_strict_mode() {
    let source = r#"{"你好":[true,]}"#;
    let strict_error = rezel_lang_json::parser()
        .with_strict(true)
        .parse(source)
        .expect_err("strict JSON rejects a trailing comma");
    assert_eq!(strict_error.kind(), ParseErrorKind::Syntax);

    let recovered = rezel_lang_json::parser()
        .parse(source)
        .expect("the default parser recovers");
    assert!(contains_error(&recovered.top_node()));
}

fn contains_error(node: &rezel_common::SyntaxNode) -> bool {
    node.node_type().is_error() || node.children().any(|child| contains_error(&child))
}
