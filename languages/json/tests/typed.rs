use rezel_common::{NodeFlags, NodeType, SyntaxNode, Tree};
use rezel_lang_json::{JsonArray, JsonRoot, JsonValue, TypedNode};

#[test]
fn typed_json_projects_direct_children_and_tokens() {
    let source = r#"{"a":[true,null,1],"b":"x"}"#;
    let tree = rezel_lang_json::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let root = JsonRoot::downcast_from(tree.top_node()).unwrap();
    let JsonValue::Object(object) = root.value().unwrap() else {
        panic!("root value must be an object");
    };

    assert_eq!(object.left_brace_token().unwrap().range().start(), 0.into());
    assert_eq!(
        usize::from(object.right_brace_token().unwrap().range().end()),
        source.len(),
    );
    let properties = object.properties().collect::<Vec<_>>();
    assert_eq!(properties.len(), 2);
    assert_eq!(properties[0].name().unwrap().text(source), Some(r#""a""#));
    assert_eq!(
        node_text(&properties[0].colon_token().unwrap(), source),
        ":",
    );

    let JsonValue::Array(array) = properties[0].value().unwrap() else {
        panic!("first property must hold an array");
    };
    assert_eq!(node_text(&array.left_bracket_token().unwrap(), source), "[");
    assert_eq!(
        node_text(&array.right_bracket_token().unwrap(), source),
        "]",
    );
    assert_eq!(array.values().count(), 3);
    assert_eq!(properties[1].name().unwrap().text(source), Some(r#""b""#));
}

#[test]
fn recovery_keeps_required_accessors_optional() {
    let source = r#"{"name":}"#;
    let tree = rezel_lang_json::parser().parse(source).unwrap();
    let root = JsonRoot::downcast_from(tree.top_node()).unwrap();
    let JsonValue::Object(object) = root.value().unwrap() else {
        panic!("recovered root must remain an object");
    };
    let property = object.properties().next().unwrap();

    assert_eq!(property.name().unwrap().text(source), Some(r#""name""#));
    assert!(property.value().is_none());
}

#[test]
fn failed_downcasts_return_the_original_node_and_reject_foreign_identity() {
    let tree = rezel_lang_json::parser()
        .with_strict(true)
        .parse("{}")
        .unwrap();
    let object = tree.top_node().first_child().unwrap();
    let original = object.clone();
    let failed = JsonArray::downcast_from(object).unwrap_err();
    assert!(failed.node_type().is(&original.node_type()));
    assert_eq!(failed.range(), original.range());

    let foreign = Tree::new(
        NodeType::new(1, "JsonText", NodeFlags::TOP),
        Vec::new(),
        Vec::new(),
        0.into(),
    )
    .top_node();
    let original = foreign.clone();
    let failed = JsonRoot::downcast_from(foreign).unwrap_err();
    assert!(failed.node_type().is(&original.node_type()));
    assert_eq!(failed.range(), original.range());
}

fn node_text<'source>(node: &SyntaxNode, source: &'source str) -> &'source str {
    let range = node.range();
    source
        .get(usize::from(range.start())..usize::from(range.end()))
        .unwrap()
}
