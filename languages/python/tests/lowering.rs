use rezel_lang_python::ast::{
    AstError, PythonAst, PythonAstField, PythonAstKind, PythonAstOptions, PythonAstValue,
};

#[test]
fn lowers_assignments_names_constants_and_binary_operators() {
    let source = "answer = 40 + 2\n";
    let tree = rezel_lang_python::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let ast = PythonAst::lower(&tree, source).unwrap();
    assert_eq!(ast.root().kind(), PythonAstKind::Module);
    assert!(
        ast.nodes()
            .iter()
            .any(|node| node.kind() == PythonAstKind::Assign)
    );
    assert!(
        ast.nodes()
            .iter()
            .any(|node| node.kind() == PythonAstKind::Name)
    );
    assert!(
        ast.nodes()
            .iter()
            .any(|node| node.kind() == PythonAstKind::BinOp)
    );
    assert!(
        ast.nodes()
            .iter()
            .any(|node| node.kind() == PythonAstKind::Add)
    );
    let body = ast
        .root()
        .fields()
        .iter()
        .find(|field| field.field() == PythonAstField::Body)
        .expect("Module.body");
    assert!(matches!(body.value(), PythonAstValue::Nodes(nodes) if nodes.len() == 1));
}

#[test]
fn type_comment_mode_populates_assignments_and_type_ignores() {
    let source = "value = 1  # type: Result\n# type: ignore[attr-defined]\n";
    let tree = rezel_lang_python::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let ast = PythonAst::lower_with_options(
        &tree,
        source,
        PythonAstOptions {
            type_comments: true,
        },
    )
    .unwrap();
    assert!(
        ast.nodes()
            .iter()
            .any(|node| node.kind() == PythonAstKind::TypeIgnore)
    );
    assert!(ast.nodes().iter().any(|node| {
        node.kind() == PythonAstKind::Assign
            && node.fields().iter().any(|field| {
                field.field() == PythonAstField::TypeComment
                    && matches!(field.value(), PythonAstValue::String(_))
            })
    }));
}

#[test]
fn recovery_trees_are_not_lowered() {
    let source = "answer = (\n";
    let tree = rezel_lang_python::parser().parse(source).unwrap();
    assert!(matches!(
        PythonAst::lower(&tree, source),
        Err(AstError::RecoveryTree)
    ));
}

#[test]
fn ast_lowerer_cannot_bypass_the_syntax_shape_boundary() {
    let lowerer = include_str!("../src/ast/lower.rs");
    assert!(
        !lowerer.contains(".children()"),
        "raw CST child traversal belongs in typed syntax or syntax views"
    );
    assert!(
        !lowerer.contains(".name().as_ref()"),
        "raw CST kind inspection belongs in typed syntax or syntax views"
    );
}
