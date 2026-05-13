use std::sync::Arc;

use rezel_common::{
    NodeFlags, NodeSet, NodeType, SyntaxLanguage, SyntaxNode, TextSize, Tree, TypedNode,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TestKind {
    Root,
}

enum TestLanguage {}

impl SyntaxLanguage for TestLanguage {
    type Kind = TestKind;

    fn kind(node: &SyntaxNode) -> Option<Self::Kind> {
        node.node_type().is_name("Root").then_some(TestKind::Root)
    }
}

#[derive(Clone, Debug)]
struct TestRoot(SyntaxNode);

impl TypedNode for TestRoot {
    type Language = TestLanguage;

    fn downcast_from(node: SyntaxNode) -> Result<Self, SyntaxNode> {
        if TestLanguage::kind(&node) == Some(TestKind::Root) {
            Ok(Self(node))
        } else {
            Err(node)
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }

    fn into_syntax(self) -> SyntaxNode {
        self.0
    }
}

#[test]
fn typed_node_text_uses_utf8_byte_ranges_and_downcast_preserves_failure() {
    let node_set = Arc::new(NodeSet::new(vec![NodeType::new(
        0,
        "Other",
        NodeFlags::TOP,
    )]));
    let node = Tree::new(
        node_set.get(0).unwrap().clone(),
        Vec::new(),
        Vec::new(),
        TextSize::try_from("值".len()).unwrap(),
    )
    .top_node();
    let original = node.clone();
    let failed = TestRoot::downcast_from(node).unwrap_err();

    assert!(failed.node_type().is(&original.node_type()));
    assert_eq!(failed.range(), original.range());

    let root_set = Arc::new(NodeSet::new(vec![NodeType::new(0, "Root", NodeFlags::TOP)]));
    let root = Tree::new(
        root_set.get(0).unwrap().clone(),
        Vec::new(),
        Vec::new(),
        TextSize::try_from("值".len()).unwrap(),
    )
    .top_node();
    let root = TestRoot::downcast_from(root).unwrap();
    assert_eq!(root.text("值"), Some("值"));
}
