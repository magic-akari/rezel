use rezel_common::{IterMode, TextRange, TypedNode};
use rezel_lang_java::typed::{
    JavaExpression, JavaMemberDeclaration, JavaProgram, JavaTopLevelTypeDeclaration,
    JavaVariableInitializer,
};

#[test]
fn typed_syntax_projects_representative_direct_children() {
    let source = r"
        package example;

        final class Sample {
            int[] values = new int[] { 1, 2 };

            int size() {
                return values.length;
            }
        }
    ";
    let tree = rezel_lang_java::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let program = JavaProgram::downcast_from(tree.top_node()).unwrap();
    let unit = program.compilation_unit().unwrap();
    assert!(unit.package().is_some());
    assert_eq!(unit.imports().count(), 0);

    let declaration = unit.declarations().next().unwrap();
    let JavaTopLevelTypeDeclaration::Class(class) = declaration.declaration().unwrap() else {
        panic!("expected class declaration");
    };
    assert!(class.modifiers().is_some());
    let core = class.declaration().unwrap();
    assert_eq!(core.name().unwrap().text(source), Some("Sample"));
    let body = core.body().unwrap();
    let left_brace = body.left_brace_token().unwrap();
    assert_eq!(
        &source[usize::from(left_brace.from())..usize::from(left_brace.to())],
        "{"
    );
    let right_brace = body.right_brace_token().unwrap();
    assert_eq!(
        &source[usize::from(right_brace.from())..usize::from(right_brace.to())],
        "}"
    );

    let mut members = body.members();
    let JavaMemberDeclaration::Field(field) = members.next().unwrap() else {
        panic!("expected field declaration");
    };
    let variable = field.declarators().next().unwrap();
    let JavaVariableInitializer::Expression(JavaExpression::ArrayCreation(initializer)) =
        variable.initializer().unwrap()
    else {
        panic!("expected array creation initializer");
    };
    assert_eq!(initializer.dimension_expressions().count(), 0);
    assert!(initializer.initializer().is_some());

    let JavaMemberDeclaration::Method(method) = members.next().unwrap() else {
        panic!("expected method declaration");
    };
    assert_eq!(method.name().unwrap().text(source), Some("size"));
    assert!(method.body().is_some());
    assert!(members.next().is_none());
}

#[test]
fn comments_and_optional_children_remain_visible() {
    let source = "/* doc */ package example; class Sample { // line\n int value; }";
    let tree = rezel_lang_java::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let program = JavaProgram::downcast_from(tree.top_node()).unwrap();
    let unit = program.compilation_unit().unwrap();
    let declaration = unit.declarations().next().unwrap();
    let JavaTopLevelTypeDeclaration::Class(class) = declaration.declaration().unwrap() else {
        panic!("expected class declaration");
    };
    let body = class.declaration().unwrap().body().unwrap();
    let JavaMemberDeclaration::Field(field) = body.members().next().unwrap() else {
        panic!("expected field declaration");
    };
    let variable = field.declarators().next().unwrap();
    assert!(variable.initializer().is_none());

    let mut comments = Vec::new();
    tree.iterate(
        TextRange::new(0.into(), tree.len()),
        IterMode::default(),
        |node| {
            if let Ok(comment) = rezel_lang_java::typed::JavaComment::downcast_from(node.clone()) {
                comments.push(comment.text(source).unwrap().to_owned());
            }
            true
        },
        |_| {},
    );
    assert_eq!(comments, ["/* doc */", "// line"]);
}
