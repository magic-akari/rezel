use rezel_lang_java::ast::{AstError, JavaAst, JavaAstField, JavaAstKind, JavaAstProperty};

#[test]
fn strict_cst_lowers_to_a_navigable_owned_ast() {
    let source = r"
        package example;
        import java.util.List;

        final class Sample {
            int value = 1;
            int size() { return value + 1; }
        }
    ";
    let tree = rezel_lang_java::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let ast = JavaAst::lower(&tree, source).unwrap();

    assert_eq!(ast.root().kind(), JavaAstKind::CompilationUnit);
    let root_fields = ast
        .edges(ast.root_id())
        .unwrap()
        .iter()
        .map(|edge| edge.field())
        .collect::<Vec<_>>();
    assert_eq!(
        root_fields,
        [
            JavaAstField::Package,
            JavaAstField::Imports,
            JavaAstField::TypeDecls,
        ]
    );

    for kind in [
        JavaAstKind::Class,
        JavaAstKind::Variable,
        JavaAstKind::Method,
        JavaAstKind::Return,
        JavaAstKind::Plus,
    ] {
        assert!(
            ast.nodes().iter().any(|node| node.kind() == kind),
            "missing {kind:?}"
        );
    }

    let class = ast
        .nodes()
        .iter()
        .position(|node| node.kind() == JavaAstKind::Class)
        .and_then(rezel_lang_java::ast::AstNodeId::from_index)
        .unwrap();
    let class_name = ast
        .properties(class)
        .unwrap()
        .iter()
        .find_map(|property| match property {
            JavaAstProperty::Name(name) => ast.string(*name),
            _ => None,
        });
    assert_eq!(class_name, Some("Sample"));

    for edge in ast.nodes().iter().enumerate().flat_map(|(index, _)| {
        let id = rezel_lang_java::ast::AstNodeId::from_index(index).unwrap();
        ast.edges(id).unwrap()
    }) {
        assert!(ast.node(edge.node()).is_some());
    }
}

#[test]
fn lowering_rejects_recovery_trees() {
    let source = "class Sample { int value = ; }";
    let tree = rezel_lang_java::parser().parse(source).unwrap();
    assert!(matches!(
        JavaAst::lower(&tree, source),
        Err(AstError::RecoveryTree)
    ));
}

#[test]
fn method_reference_lowering_uses_the_member_identifier() {
    let source = r"
        class References {
            java.util.function.Function<String, String> trim() {
                return String::trim;
            }
        }
    ";
    let tree = rezel_lang_java::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let ast = JavaAst::lower(&tree, source).unwrap();
    let reference = ast
        .nodes()
        .iter()
        .position(|node| node.kind() == JavaAstKind::MemberReference)
        .and_then(rezel_lang_java::ast::AstNodeId::from_index)
        .expect("the source contains one method reference");
    let name = ast
        .properties(reference)
        .unwrap()
        .iter()
        .find_map(|property| match property {
            JavaAstProperty::Name(name) => ast.string(*name),
            _ => None,
        });
    assert_eq!(name, Some("trim"));
}
