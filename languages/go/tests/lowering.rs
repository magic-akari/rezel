#![forbid(unsafe_code)]

use rezel_common::TextSize;
use rezel_lang_go::ast::{GoAst, GoAstField, GoAstKind, GoAstNode, GoAstValue};

#[test]
fn strict_syntax_lowers_to_the_public_owned_ast() {
    let source = "package sample\nvar answer = 42\n";
    let tree = rezel_lang_go::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let ast = GoAst::lower(&tree, source).unwrap();
    let kinds = ast.nodes().iter().map(GoAstNode::kind).collect::<Vec<_>>();
    let source_end = TextSize::try_from(source.len()).unwrap();
    let root_range = ast.root().source_range();

    assert_eq!(ast.root().kind(), GoAstKind::File);
    assert_eq!(root_range.start(), Some(0.into()));
    assert!(root_range.end().is_some_and(|end| end <= source_end));
    assert!(kinds.contains(&GoAstKind::GenDecl));
    assert!(kinds.contains(&GoAstKind::ValueSpec));
    assert!(kinds.contains(&GoAstKind::BasicLit));
}

#[test]
fn long_expression_chains_parse_and_lower() {
    let source = left_associative_expression(512);
    let tree = rezel_lang_go::parser()
        .with_strict(true)
        .parse(&source)
        .unwrap();
    let ast = GoAst::lower(&tree, &source).unwrap();

    assert!(ast.nodes().len() > 1_000);
}

#[test]
fn statement_edges_match_the_public_go_ast_shape() {
    let source = "package p\nfunc f(value any) {\n\tswitch typed := value.(type) { default: _ = typed }\nexplicit:\n\t;\nimplicit:\n}\n";
    let tree = rezel_lang_go::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let ast = GoAst::lower(&tree, source).unwrap();

    let labeled = ast
        .nodes()
        .iter()
        .enumerate()
        .filter(|(_, node)| node.kind() == GoAstKind::LabeledStmt)
        .filter_map(|(index, _)| rezel_lang_go::ast::AstNodeId::from_index(index))
        .collect::<Vec<_>>();
    assert_eq!(labeled.len(), 2);
    let statements = labeled
        .iter()
        .map(|labeled| {
            ast.fields(*labeled)
                .unwrap()
                .iter()
                .find_map(|field| match (field.field(), field.value()) {
                    (GoAstField::Stmt, GoAstValue::Node(statement)) => statement,
                    _ => None,
                })
                .expect("the label has a statement")
        })
        .collect::<Vec<_>>();
    assert!(
        statements
            .iter()
            .all(|statement| ast.node(*statement).unwrap().kind() == GoAstKind::EmptyStmt)
    );
    assert_eq!(
        ast.nodes()
            .iter()
            .filter(|node| node.kind() == GoAstKind::EmptyStmt)
            .count(),
        2
    );
    assert_eq!(
        statements
            .iter()
            .map(|statement| {
                ast.fields(*statement)
                    .unwrap()
                    .iter()
                    .find_map(|field| match (field.field(), field.value()) {
                        (GoAstField::Implicit, GoAstValue::Bool(implicit)) => Some(implicit),
                        _ => None,
                    })
                    .expect("the empty statement records whether it was implicit")
            })
            .collect::<Vec<_>>(),
        [false, true]
    );
    let closing_brace = TextSize::try_from(source.rfind('}').unwrap()).unwrap();
    assert_eq!(
        ast.node(statements[1]).unwrap().source_range().start(),
        Some(closing_brace)
    );
    assert_eq!(
        ast.node(labeled[1]).unwrap().source_range().end(),
        Some(closing_brace)
    );

    let type_switch = ast
        .nodes()
        .iter()
        .position(|node| node.kind() == GoAstKind::TypeSwitchStmt)
        .and_then(rezel_lang_go::ast::AstNodeId::from_index)
        .expect("one type switch statement");
    let assignment = ast
        .fields(type_switch)
        .unwrap()
        .iter()
        .find_map(|field| match (field.field(), field.value()) {
            (GoAstField::Assign, GoAstValue::Node(assignment)) => assignment,
            _ => None,
        })
        .expect("the type switch has an assignment");
    let range = ast
        .node(assignment)
        .unwrap()
        .source_range()
        .byte_range()
        .expect("the assignment has a complete range");
    assert_eq!(
        &source[usize::from(range.start())..usize::from(range.end())],
        "typed := value.(type)"
    );
}

#[test]
fn ast_lowerer_cannot_bypass_the_syntax_view_boundary() {
    let lowerer = include_str!("../src/ast/lower.rs");
    for forbidden in [
        ".children()",
        "children_with_tokens",
        "fn child(",
        "fn children(",
        "fn is_expr(",
        "fn is_type(",
        "fn is_statement_kind(",
    ] {
        assert!(
            !lowerer.contains(forbidden),
            "{forbidden:?} exposes raw syntax shape inside AST lowering"
        );
    }
}

fn left_associative_expression(operator_count: usize) -> String {
    let mut source = String::from("package p\nvar _ = 0");
    for _ in 0..operator_count {
        source.push_str(" + 0");
    }
    source.push('\n');
    source
}
