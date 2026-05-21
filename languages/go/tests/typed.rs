#![forbid(unsafe_code)]

use rezel_lang_go::{GoDeclaration, GoSourceFile, TypedNode};

#[test]
fn typed_syntax_exposes_core_file_and_declaration_children() {
    let source = r#"package sample

import "fmt"

type Item[T any] struct {
	Value T
}

var current Item[int]

func Build[T any](value T) (result T) {
	fmt.Println(value)
	return value
}
"#;
    let tree = rezel_lang_go::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let file = GoSourceFile::downcast_from(tree.top_node()).unwrap();
    let package = file.package_clause().unwrap();
    assert_eq!(package.name().unwrap().text(source), Some("sample"));
    assert_eq!(file.imports().count(), 1);

    let declarations = file.declarations().collect::<Vec<_>>();
    assert_eq!(declarations.len(), 3);

    let GoDeclaration::Type(type_decl) = &declarations[0] else {
        panic!("expected type declaration");
    };
    let type_spec = type_decl.spec().unwrap();
    assert_eq!(type_spec.name().unwrap().text(source), Some("Item"));
    assert!(type_spec.type_parameters().is_some());

    let GoDeclaration::Var(var_decl) = &declarations[1] else {
        panic!("expected var declaration");
    };
    assert_eq!(
        var_decl
            .spec()
            .unwrap()
            .names()
            .next()
            .unwrap()
            .text(source),
        Some("current")
    );

    let GoDeclaration::Function(function) = &declarations[2] else {
        panic!("expected function declaration");
    };
    assert_eq!(function.name().unwrap().text(source), Some("Build"));
    assert!(function.type_parameters().is_some());
    assert!(function.parameters().is_some());
    assert!(function.result_parameters().is_some());
    assert!(function.body().is_some());
}
