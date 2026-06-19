#![forbid(unsafe_code)]

use rezel_lang_rust::{
    RustDeclaration, RustDeclarationStatement, RustExpression, RustFunctionItem, RustFunctionName,
    RustPath, RustSourceFile, RustStatement, RustType, TypedNode,
};

fn syntax_text<'source>(node: &rezel_common::SyntaxNode, source: &'source str) -> &'source str {
    &source[usize::from(node.from())..usize::from(node.to())]
}

#[test]
fn typed_syntax_navigates_core_rust_roles() {
    let source = r"#![allow(dead_code)]

#[inline]
pub async fn build<T>(value: T) -> Option<T>
where
    T: Copy,
{
    let result = if true { value } else { value };
    result
}

struct Marker;
";
    let tree = rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let file = RustSourceFile::downcast_from(tree.top_node()).unwrap();

    assert_eq!(file.text(source), Some(source));
    let inner_attributes = file.inner_attributes().collect::<Vec<_>>();
    assert_eq!(inner_attributes.len(), 1);
    assert_eq!(
        inner_attributes[0].meta().unwrap().text(source),
        Some("allow(dead_code)")
    );

    let mut statements = file.statements();
    let RustStatement::AttributedItem(attributed) = statements.next().unwrap() else {
        panic!("expected an attributed function");
    };
    assert_eq!(attributed.attributes().count(), 1);
    let RustDeclarationStatement::Item(RustDeclaration::Function(function)) =
        attributed.declaration().unwrap()
    else {
        panic!("expected a function declaration");
    };
    assert!(function.visibility().is_some());
    assert!(function.type_parameters().is_some());
    assert!(function.where_clause().is_some());
    assert_eq!(syntax_text(&function.fn_token().unwrap(), source), "fn");
    let RustFunctionName::Identifier(name) = function.name().unwrap() else {
        panic!("expected an identifier function name");
    };
    assert_eq!(name.text(source), Some("build"));
    assert_eq!(function.parameters().unwrap().parameters().count(), 1);
    assert!(matches!(function.return_type(), Some(RustType::Generic(_))));

    let body = function.body().unwrap();
    assert_eq!(syntax_text(&body.left_brace_token().unwrap(), source), "{");
    assert_eq!(syntax_text(&body.right_brace_token().unwrap(), source), "}");
    let mut body_statements = body.statements();
    assert!(matches!(
        body_statements.next(),
        Some(RustStatement::Declaration(RustDeclarationStatement::Let(_)))
    ));
    let Some(RustStatement::Expression(tail)) = body_statements.next() else {
        panic!("expected a tail expression");
    };
    let RustExpression::Path(RustPath::Identifier(tail_name)) = tail.expression().unwrap() else {
        panic!("expected an identifier tail expression");
    };
    assert_eq!(tail_name.text(source), Some("result"));
    assert!(body_statements.next().is_none());

    assert!(matches!(
        statements.next(),
        Some(RustStatement::Declaration(RustDeclarationStatement::Item(
            RustDeclaration::Struct(_)
        )))
    ));
    assert!(statements.next().is_none());
}

#[test]
fn typed_downcasts_reject_the_wrong_kind() {
    let source = "fn main() {}\n";
    let tree = rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let file = RustSourceFile::downcast_from(tree.top_node()).unwrap();

    assert!(RustFunctionItem::downcast_from(file.syntax().clone()).is_err());
}

#[test]
fn typed_accessors_tolerate_recovery_children() {
    let source = "fn () {}\n";
    let tree = rezel_lang_rust::parser().parse(source).unwrap();
    let file = RustSourceFile::downcast_from(tree.top_node()).unwrap();
    let Some(RustStatement::Declaration(RustDeclarationStatement::Item(
        RustDeclaration::Function(function),
    ))) = file.statements().next()
    else {
        panic!("expected a recovered function");
    };

    assert_eq!(syntax_text(&function.fn_token().unwrap(), source), "fn");
    assert!(function.name().is_none());
    assert!(function.parameters().is_some());
    assert!(function.body().is_some());
}
