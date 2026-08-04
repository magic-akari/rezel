#![forbid(unsafe_code)]

use rezel_common::TypedNode;
use rezel_lang_kotlin::{
    KotlinBinaryOperator, KotlinClassBodyValue, KotlinControlBody, KotlinDeclaration,
    KotlinExpression, KotlinFile, KotlinForVariable, KotlinFunctionBodyValue,
    KotlinFunctionDeclaration, KotlinFunctionTypeParameterValue, KotlinLambdaParameterTarget,
    KotlinLiteralValue, KotlinLoop, KotlinModifierValue, KotlinNullableBaseType, KotlinStatement,
    KotlinStringPart, KotlinTrailingLambda, KotlinType, KotlinWhenConditionValue,
};

fn text<'a>(node: &impl TypedNode, source: &'a str) -> Option<&'a str> {
    node.text(source)
}

fn syntax_text<'a>(node: &rezel_common::SyntaxNode, source: &'a str) -> Option<&'a str> {
    let range = node.range();
    source.get(usize::from(range.start())..usize::from(range.end()))
}

#[test]
fn file_headers_and_imports_are_navigable_from_the_root() {
    let source = r#"@file:Suppress("unused")
package sample.api

import kotlin.collections.List
import kotlin.io.*
import kotlin.math.max as maximum

fun sample() = Unit
"#;
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("valid Kotlin file");
    let file = KotlinFile::downcast_from(tree.top_node()).expect("KotlinFile root");

    let annotations = file.file_annotations().collect::<Vec<_>>();
    assert_eq!(annotations.len(), 1);
    assert_eq!(
        text(
            &annotations[0].annotation().expect("file annotation"),
            source
        ),
        Some("Suppress(\"unused\")")
    );

    let package = file.package_header().expect("package header");
    assert_eq!(
        syntax_text(&package.package_token().unwrap(), source),
        Some("package")
    );
    let package_name = package.name().expect("qualified package name");
    let package_segments = package_name
        .segments()
        .map(|segment| text(&segment, source).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(package_segments, ["sample", "api"]);

    let imports = file
        .import_list()
        .expect("import list")
        .headers()
        .collect::<Vec<_>>();
    assert_eq!(imports.len(), 3);
    let first_path = imports[0].path().expect("first import path");
    assert_eq!(text(&first_path, source), Some("kotlin.collections.List"));
    assert_eq!(first_path.segments().count(), 3);
    assert!(imports[0].alias().is_none());

    let wildcard_path = imports[1].path().expect("wildcard import path");
    assert_eq!(text(&wildcard_path, source), Some("kotlin.io.*"));
    assert_eq!(wildcard_path.segments().count(), 2);

    let alias = imports[2].alias().expect("import alias");
    assert_eq!(syntax_text(&alias.as_token().unwrap(), source), Some("as"));
    assert_eq!(
        text(&alias.name().expect("alias name"), source),
        Some("maximum")
    );

    assert_eq!(file.declarations().count(), 1);
}

#[test]
fn function_signature_and_body_preserve_cst_ownership() {
    let source = "public fun <T> choose(value: T = fallback): T where T : Any = value";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("valid generic function");
    let file = KotlinFile::downcast_from(tree.top_node()).expect("KotlinFile root");
    let declaration = file.declarations().next().expect("function declaration");
    let KotlinDeclaration::Function(function) = declaration else {
        panic!("expected a function declaration");
    };

    assert_eq!(
        syntax_text(&function.fun_token().unwrap(), source),
        Some("fun")
    );
    assert_eq!(text(&function.modifiers().unwrap(), source), Some("public"));
    assert_eq!(
        text(&function.direct_name().expect("function name"), source),
        Some("choose")
    );
    assert!(function.qualified_receiver().is_none());

    let type_parameters = function.type_parameters().expect("type parameters");
    let type_parameter = type_parameters.parameters().next().expect("type parameter");
    assert_eq!(text(&type_parameter.name().unwrap(), source), Some("T"));
    assert!(type_parameter.bound().is_none());

    let parameters = function.parameters().expect("value parameters");
    let value_parameter = parameters.parameters().next().expect("value parameter");
    let parameter = value_parameter.parameter().expect("parameter payload");
    assert_eq!(text(&parameter.name().unwrap(), source), Some("value"));
    assert!(matches!(
        parameter.type_annotation().unwrap().ty(),
        Some(KotlinType::User(_))
    ));
    let initializer = value_parameter.initializer().expect("default initializer");
    assert!(matches!(
        initializer.value(),
        Some(KotlinExpression::Name(_))
    ));

    let return_type = function.return_type().expect("return type");
    assert!(matches!(return_type.ty(), Some(KotlinType::User(_))));
    let constraints = function.type_constraints().expect("type constraints");
    let constraint = constraints.constraints().next().expect("type constraint");
    assert_eq!(text(&constraint.name().unwrap(), source), Some("T"));
    assert!(matches!(constraint.ty(), Some(KotlinType::User(_))));

    let body = function.body().expect("function body");
    assert!(matches!(
        body.value(),
        Some(KotlinFunctionBodyValue::Expression(KotlinExpression::Name(
            _
        )))
    ));
}

#[test]
fn typed_ranges_follow_the_source_and_wrong_kinds_are_rejected() {
    let source = "package sample.deep\nfun answer(): Int = 42\n";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("valid Kotlin file");
    let file = KotlinFile::downcast_from(tree.top_node()).expect("KotlinFile root");
    let package_name = file.package_header().unwrap().name().unwrap();
    assert_eq!(text(&package_name, source), Some("sample.deep"));
    assert_eq!(usize::from(package_name.syntax().range().start()), 8);
    assert_eq!(usize::from(package_name.syntax().range().end()), 19);
    assert!(KotlinFunctionDeclaration::downcast_from(file.into_syntax()).is_err());
}

#[test]
fn required_typed_children_remain_optional_in_recovery_trees() {
    let tree = rezel_lang_kotlin::parser()
        .parse("fun missing")
        .expect("recovery tree");
    let file = KotlinFile::downcast_from(tree.top_node()).expect("KotlinFile root");
    let KotlinDeclaration::Function(function) = file.declarations().next().unwrap() else {
        panic!("expected recovered function declaration");
    };
    assert_eq!(
        function.direct_name().unwrap().syntax().to_string(),
        "Definition"
    );
    assert!(function.parameters().is_none());
    assert!(function.body().is_none());
}

#[test]
fn class_object_and_type_alias_roles_are_directly_navigable() {
    let source = r"data class Box<T> public constructor(val value: T = fallback) : Base() where T : Any {}
object Registry : Base() {}
typealias Alias<T> = List<T>
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("valid declaration family");
    let file = KotlinFile::downcast_from(tree.top_node()).expect("KotlinFile root");
    let declarations = file.declarations().collect::<Vec<_>>();
    assert_eq!(declarations.len(), 3);

    let KotlinDeclaration::Class(class) = &declarations[0] else {
        panic!("expected class declaration");
    };
    assert_eq!(
        syntax_text(&class.class_token().unwrap(), source),
        Some("class")
    );
    assert_eq!(text(&class.name().unwrap(), source), Some("Box"));
    let modifier = class.modifiers().unwrap().entries().next().unwrap();
    assert!(matches!(
        modifier.value(),
        Some(KotlinModifierValue::Modifier(_))
    ));
    assert_eq!(class.type_parameters().unwrap().parameters().count(), 1);

    let constructor = class.primary_constructor().expect("primary constructor");
    assert_eq!(
        text(&constructor.modifiers().unwrap(), source),
        Some("public")
    );
    let class_parameter = constructor
        .parameters()
        .unwrap()
        .parameters()
        .next()
        .unwrap();
    assert_eq!(
        syntax_text(&class_parameter.val_token().unwrap(), source),
        Some("val")
    );
    let variable = class_parameter
        .variable()
        .expect("class parameter variable");
    assert_eq!(text(&variable.name().unwrap(), source), Some("value"));
    assert!(matches!(
        variable.type_annotation().unwrap().ty(),
        Some(KotlinType::User(_))
    ));
    assert!(matches!(
        class_parameter.initializer().unwrap().value(),
        Some(KotlinExpression::Name(_))
    ));
    assert_eq!(
        class.delegation_specifiers().unwrap().specifiers().count(),
        1
    );
    assert_eq!(class.type_constraints().unwrap().constraints().count(), 1);
    let Some(KotlinClassBodyValue::Class(body)) = class.body() else {
        panic!("expected a regular class body");
    };
    assert!(body.members().is_some());

    let KotlinDeclaration::Object(object) = &declarations[1] else {
        panic!("expected object declaration");
    };
    assert_eq!(
        syntax_text(&object.object_token().unwrap(), source),
        Some("object")
    );
    assert_eq!(text(&object.name().unwrap(), source), Some("Registry"));
    assert_eq!(
        object.delegation_specifiers().unwrap().specifiers().count(),
        1
    );
    assert!(object.body().is_some());

    let KotlinDeclaration::TypeAlias(alias) = &declarations[2] else {
        panic!("expected type alias declaration");
    };
    assert_eq!(
        syntax_text(&alias.typealias_token().unwrap(), source),
        Some("typealias")
    );
    assert_eq!(text(&alias.name().unwrap(), source), Some("Alias"));
    assert_eq!(alias.type_parameters().unwrap().parameters().count(), 1);
    assert!(matches!(alias.aliased_type(), Some(KotlinType::User(_))));
}

#[test]
fn property_initializer_delegate_and_accessors_keep_distinct_roles() {
    let source = r#"private val answer: Int = 42
val deferred: String by lazy { "ready" }
var count: Int = 0
    get() = field
    set(value) { field = value }
"#;
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("valid property declarations");
    let file = KotlinFile::downcast_from(tree.top_node()).expect("KotlinFile root");
    let declarations = file.declarations().collect::<Vec<_>>();
    assert_eq!(declarations.len(), 3);

    let KotlinDeclaration::Property(answer) = &declarations[0] else {
        panic!("expected property declaration");
    };
    assert_eq!(
        syntax_text(&answer.val_token().unwrap(), source),
        Some("val")
    );
    assert_eq!(answer.modifiers().unwrap().entries().count(), 1);
    assert_eq!(
        text(&answer.variable().unwrap().name().unwrap(), source),
        Some("answer")
    );
    assert!(matches!(
        answer.initializer().unwrap().value(),
        Some(KotlinExpression::Literal(_))
    ));
    assert!(answer.delegate().is_none());

    let KotlinDeclaration::Property(deferred) = &declarations[1] else {
        panic!("expected delegated property");
    };
    assert!(deferred.initializer().is_none());
    assert!(matches!(
        deferred.delegate().unwrap().expression(),
        Some(KotlinExpression::Call(_))
    ));

    let KotlinDeclaration::Property(count) = &declarations[2] else {
        panic!("expected accessor property");
    };
    assert_eq!(
        syntax_text(&count.var_token().unwrap(), source),
        Some("var")
    );
    assert!(count.getter().is_some());
    assert!(count.setter().is_some());
}

#[test]
fn blocks_expose_statement_and_loop_wrapper_layers() {
    let source = r"fun flow(items: List<Int>) {
    var total = 0
    for (item in items) { total += item }
    while (total > 0) total -= 1
    do total += 1 while (total < 0)
    total
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("valid statement family");
    let file = KotlinFile::downcast_from(tree.top_node()).expect("KotlinFile root");
    let KotlinDeclaration::Function(function) = file.declarations().next().unwrap() else {
        panic!("expected function declaration");
    };
    let Some(KotlinFunctionBodyValue::Block(block)) = function.body().unwrap().value() else {
        panic!("expected block body");
    };
    let statements = block.statements().unwrap().items().collect::<Vec<_>>();
    assert_eq!(statements.len(), 5);
    assert!(matches!(statements[0], KotlinStatement::Property(_)));

    let KotlinStatement::Loop(for_wrapper) = &statements[1] else {
        panic!("expected for loop wrapper");
    };
    let Some(KotlinLoop::For(for_statement)) = for_wrapper.value() else {
        panic!("expected for statement");
    };
    assert!(matches!(
        for_statement.variable(),
        Some(KotlinForVariable::Definition(_))
    ));
    assert!(matches!(
        for_statement.iterable(),
        Some(KotlinExpression::Name(_))
    ));
    let Some(KotlinControlBody::Block(for_body)) = for_statement.body().unwrap().value() else {
        panic!("expected block for body");
    };
    let nested = for_body.statements().unwrap().items().next().unwrap();
    let KotlinStatement::Assignment(assignment) = nested else {
        panic!("expected assignment in for body");
    };
    assert_eq!(text(&assignment.operator().unwrap(), source), Some("+="));
    assert!(matches!(
        assignment.target(),
        Some(KotlinExpression::Name(_))
    ));
    assert!(matches!(
        assignment.value(),
        Some(KotlinExpression::Name(_))
    ));

    let KotlinStatement::Loop(while_wrapper) = &statements[2] else {
        panic!("expected while loop wrapper");
    };
    let Some(KotlinLoop::While(while_statement)) = while_wrapper.value() else {
        panic!("expected while statement");
    };
    assert!(matches!(
        while_statement.condition(),
        Some(KotlinExpression::Binary(_))
    ));
    assert!(matches!(
        while_statement.body().unwrap().value(),
        Some(KotlinControlBody::Assignment(_))
    ));

    let KotlinStatement::Loop(do_while_wrapper) = &statements[3] else {
        panic!("expected do-while loop wrapper");
    };
    let Some(KotlinLoop::DoWhile(do_while)) = do_while_wrapper.value() else {
        panic!("expected do-while statement");
    };
    assert!(matches!(
        do_while.body().unwrap().value(),
        Some(KotlinControlBody::Assignment(_))
    ));
    assert!(matches!(
        do_while.condition(),
        Some(KotlinExpression::Binary(_))
    ));
    assert!(matches!(statements[4], KotlinStatement::Expression(_)));
}

fn assert_when_navigation(function: &KotlinFunctionDeclaration, source: &str) {
    let Some(KotlinFunctionBodyValue::Expression(KotlinExpression::When(when_expression))) =
        function.body().unwrap().value()
    else {
        panic!("expected when expression body");
    };
    let subject = when_expression.subject().expect("when subject");
    assert_eq!(
        text(&subject.variable().unwrap().name().unwrap(), source),
        Some("value")
    );
    assert!(matches!(
        subject.expression(),
        Some(KotlinExpression::Name(_))
    ));
    let entries = when_expression
        .entries()
        .unwrap()
        .items()
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 3);
    let first_conditions = entries[0].conditions().unwrap();
    assert!(matches!(
        first_conditions.items().next().unwrap().value(),
        Some(KotlinWhenConditionValue::Type(_))
    ));
    assert!(first_conditions.guard().is_some());
    assert!(matches!(
        entries[1]
            .conditions()
            .unwrap()
            .items()
            .next()
            .unwrap()
            .value(),
        Some(KotlinWhenConditionValue::Range(_))
    ));
    assert!(entries[2].conditions().is_none());
}

#[test]
fn control_bodies_expose_named_function_declarations() {
    let source = "fun host(flag:Boolean) { if(flag) fun local() {} else Unit }";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("valid named function control body");
    let file = KotlinFile::downcast_from(tree.top_node()).expect("KotlinFile root");
    let KotlinDeclaration::Function(host) = file.declarations().next().unwrap() else {
        panic!("expected host function");
    };
    let Some(KotlinFunctionBodyValue::Block(block)) = host.body().unwrap().value() else {
        panic!("expected host block");
    };
    let KotlinStatement::Expression(statement) =
        block.statements().unwrap().items().next().unwrap()
    else {
        panic!("expected if expression statement");
    };
    let Some(KotlinExpression::If(if_expression)) = statement.expression() else {
        panic!("expected if expression");
    };
    let Some(KotlinControlBody::Function(local)) = if_expression.then_body().unwrap().value()
    else {
        panic!("expected named function control body");
    };
    assert_eq!(
        text(&local.direct_name().expect("local function name"), source),
        Some("local")
    );
}

#[test]
fn control_bodies_expose_type_alias_declarations() {
    let source = "fun host(flag:Boolean) { if(flag) typealias Local = String else Unit }";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("valid type-alias control body");
    let file = KotlinFile::downcast_from(tree.top_node()).expect("KotlinFile root");
    let KotlinDeclaration::Function(host) = file.declarations().next().unwrap() else {
        panic!("expected host function");
    };
    let Some(KotlinFunctionBodyValue::Block(block)) = host.body().unwrap().value() else {
        panic!("expected host block");
    };
    let KotlinStatement::Expression(statement) =
        block.statements().unwrap().items().next().unwrap()
    else {
        panic!("expected if expression statement");
    };
    let Some(KotlinExpression::If(if_expression)) = statement.expression() else {
        panic!("expected if expression");
    };
    let Some(KotlinControlBody::TypeAlias(alias)) = if_expression.then_body().unwrap().value()
    else {
        panic!("expected type-alias control body");
    };
    assert_eq!(
        text(&alias.name().expect("type-alias name"), source),
        Some("Local")
    );
}

#[test]
fn empty_if_branches_preserve_typed_branch_roles() {
    let source = "fun host(flag:Boolean) { if(flag); if(flag) else; }";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("valid empty if branches");
    let file = KotlinFile::downcast_from(tree.top_node()).expect("KotlinFile root");
    let KotlinDeclaration::Function(host) = file.declarations().next().unwrap() else {
        panic!("expected host function");
    };
    let Some(KotlinFunctionBodyValue::Block(block)) = host.body().unwrap().value() else {
        panic!("expected host block");
    };
    let statements = block.statements().unwrap().items().collect::<Vec<_>>();
    assert_eq!(statements.len(), 2);

    let KotlinStatement::Expression(first) = &statements[0] else {
        panic!("expected first if statement");
    };
    let Some(KotlinExpression::If(first)) = first.expression() else {
        panic!("expected first if expression");
    };
    assert!(first.then_body().unwrap().value().is_none());
    assert!(first.else_body().is_none());

    let KotlinStatement::Expression(second) = &statements[1] else {
        panic!("expected second if statement");
    };
    let Some(KotlinExpression::If(second)) = second.expression() else {
        panic!("expected second if expression");
    };
    assert!(second.then_body().unwrap().value().is_none());
    assert!(second.else_body().unwrap().value().is_none());
}

#[test]
fn control_expressions_expose_branches_conditions_and_handlers() {
    let source = r"annotation class Mark
fun classify(input: Any): Int = when (val value = input) {
    is String if value.length > 0 -> 1
    in 1..10 -> 2
    else -> 0
}
fun choose(flag: Boolean): Int =
    if (flag) try { 1 } catch (@Mark error: Exception) { -1 } finally { cleanup() }
    else 0
fun exits(flag: Boolean) {
    if (flag) return
    throw Failure()
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("valid control expressions");
    let file = KotlinFile::downcast_from(tree.top_node()).expect("KotlinFile root");
    let declarations = file.declarations().collect::<Vec<_>>();
    assert_eq!(declarations.len(), 4);

    let KotlinDeclaration::Function(classify) = &declarations[1] else {
        panic!("expected classify function");
    };
    assert_when_navigation(classify, source);

    let KotlinDeclaration::Function(choose) = &declarations[2] else {
        panic!("expected choose function");
    };
    let Some(KotlinFunctionBodyValue::Expression(KotlinExpression::If(if_expression))) =
        choose.body().unwrap().value()
    else {
        panic!("expected if expression body");
    };
    assert!(matches!(
        if_expression.condition(),
        Some(KotlinExpression::Name(_))
    ));
    let Some(KotlinControlBody::Expression(then_statement)) =
        if_expression.then_body().unwrap().value()
    else {
        panic!("expected expression then branch");
    };
    let Some(KotlinExpression::Try(try_expression)) = then_statement.expression() else {
        panic!("expected try expression");
    };
    assert_eq!(
        try_expression
            .block()
            .unwrap()
            .statements()
            .unwrap()
            .items()
            .count(),
        1
    );
    let catch = try_expression.catches().next().expect("catch clause");
    assert_eq!(catch.annotations().count(), 1);
    assert_eq!(text(&catch.parameter().unwrap(), source), Some("error"));
    assert!(matches!(catch.ty(), Some(KotlinType::User(_))));
    assert_eq!(
        catch.block().unwrap().statements().unwrap().items().count(),
        1
    );
    assert!(try_expression.finally_clause().unwrap().block().is_some());
    assert!(matches!(
        if_expression.else_body().unwrap().value(),
        Some(KotlinControlBody::Expression(_))
    ));

    let KotlinDeclaration::Function(exits) = &declarations[3] else {
        panic!("expected exits function");
    };
    let Some(KotlinFunctionBodyValue::Block(block)) = exits.body().unwrap().value() else {
        panic!("expected block body");
    };
    let statements = block.statements().unwrap().items().collect::<Vec<_>>();
    let KotlinStatement::Expression(if_statement) = &statements[0] else {
        panic!("expected if statement");
    };
    let Some(KotlinExpression::If(if_expression)) = if_statement.expression() else {
        panic!("expected if expression");
    };
    let Some(KotlinControlBody::Expression(return_statement)) =
        if_expression.then_body().unwrap().value()
    else {
        panic!("expected return branch");
    };
    let Some(KotlinExpression::Return(return_expression)) = return_statement.expression() else {
        panic!("expected return expression");
    };
    assert!(return_expression.value().is_none());
    let KotlinStatement::Expression(throw_statement) = &statements[1] else {
        panic!("expected throw statement");
    };
    let Some(KotlinExpression::Throw(throw_expression)) = throw_statement.expression() else {
        panic!("expected throw expression");
    };
    assert!(matches!(
        throw_expression.value(),
        Some(KotlinExpression::Call(_))
    ));
}

#[test]
fn postfix_and_operator_expressions_preserve_operand_roles() {
    let source = r"fun expressions(value: Any, items: List<Int>) {
    val call = target.invoke(name = value) { arg -> arg }
    val member = call.value
    val reference = call::toString
    val checked = value is String
    val contained = 1 in items
    val cast = value as String
    val range = 1..10
    val infix = 1 to 2
    val binary = 1 + 2
    call!!
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("valid expression family");
    let file = KotlinFile::downcast_from(tree.top_node()).unwrap();
    let KotlinDeclaration::Function(function) = file.declarations().next().unwrap() else {
        panic!("expected function");
    };
    let Some(KotlinFunctionBodyValue::Block(block)) = function.body().unwrap().value() else {
        panic!("expected block");
    };
    let statements = block.statements().unwrap().items().collect::<Vec<_>>();
    let initializers = statements[..9]
        .iter()
        .map(|statement| {
            let KotlinStatement::Property(property) = statement else {
                panic!("expected local property");
            };
            property.initializer().unwrap().value().unwrap()
        })
        .collect::<Vec<_>>();

    let KotlinExpression::Call(call) = &initializers[0] else {
        panic!("expected call");
    };
    let outer_suffix = call.suffix().unwrap();
    let Some(KotlinTrailingLambda::Lambda(lambda)) = outer_suffix.trailing_lambda() else {
        panic!("expected trailing lambda");
    };
    let Some(KotlinExpression::Call(inner_call)) = call.receiver() else {
        panic!("expected nested argument call");
    };
    assert!(matches!(
        inner_call.receiver(),
        Some(KotlinExpression::Member(_))
    ));
    let argument = inner_call
        .suffix()
        .unwrap()
        .arguments()
        .unwrap()
        .arguments()
        .next()
        .unwrap();
    assert_eq!(text(&argument.name().unwrap(), source), Some("name"));
    assert!(matches!(
        argument.expression(),
        Some(KotlinExpression::Name(_))
    ));
    let parameter = lambda.parameters().unwrap().parameters().next().unwrap();
    assert!(matches!(
        parameter.target(),
        Some(KotlinLambdaParameterTarget::Variable(_))
    ));
    assert_eq!(lambda.statements().unwrap().items().count(), 1);

    let KotlinExpression::Member(member) = &initializers[1] else {
        panic!("expected member");
    };
    assert!(matches!(member.receiver(), Some(KotlinExpression::Name(_))));
    assert_eq!(text(&member.member().unwrap(), source), Some("value"));
    let KotlinExpression::CallableReference(reference) = &initializers[2] else {
        panic!("expected callable reference");
    };
    assert!(reference.receiver().is_some());
    assert_eq!(text(&reference.member().unwrap(), source), Some("toString"));
    assert!(matches!(initializers[3], KotlinExpression::TypeCheck(_)));
    assert!(matches!(initializers[4], KotlinExpression::Containment(_)));
    assert!(matches!(initializers[5], KotlinExpression::Cast(_)));
    assert!(matches!(initializers[6], KotlinExpression::Range(_)));
    assert!(matches!(initializers[7], KotlinExpression::Infix(_)));
    let KotlinExpression::Binary(binary) = &initializers[8] else {
        panic!("expected binary");
    };
    assert!(matches!(
        binary.operator(),
        Some(KotlinBinaryOperator::Arithmetic(_))
    ));
    assert!(matches!(binary.left(), Some(KotlinExpression::Literal(_))));
    let KotlinStatement::Expression(last) = &statements[9] else {
        panic!("expected expression statement");
    };
    assert!(matches!(
        last.expression(),
        Some(KotlinExpression::NotNull(_))
    ));
}

#[test]
fn type_and_string_wrappers_preserve_nested_payloads() {
    let source = r#"annotation class Ann
typealias Complex<T> = context(String) (@Ann List<out T>?, (T)) -> T & Any
fun text(name: String) = "hello $name ${name.length}"
"#;
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("valid nested types and interpolation");
    let file = KotlinFile::downcast_from(tree.top_node()).unwrap();
    let declarations = file.declarations().collect::<Vec<_>>();

    let KotlinDeclaration::TypeAlias(alias) = &declarations[1] else {
        panic!("expected type alias");
    };
    let Some(KotlinType::Function(function_type)) = alias.aliased_type() else {
        panic!("expected function type");
    };
    assert_eq!(
        function_type.context_parameters().unwrap().types().count(),
        1
    );
    let parameters = function_type
        .parameters()
        .unwrap()
        .parameters()
        .collect::<Vec<_>>();
    assert_eq!(parameters.len(), 2);
    let Some(KotlinFunctionTypeParameterValue::Type(KotlinType::Annotated(annotated))) =
        parameters[0].value()
    else {
        panic!("expected annotated parameter type");
    };
    assert_eq!(annotated.modifiers().unwrap().annotations().count(), 1);
    let Some(KotlinType::Nullable(nullable)) = annotated.ty() else {
        panic!("expected nullable type");
    };
    let Some(KotlinNullableBaseType::User(user)) = nullable.ty() else {
        panic!("expected nullable user type");
    };
    let segment = user.segments().next().unwrap();
    let projection = segment.arguments().unwrap().projections().next().unwrap();
    assert!(projection.variance().is_some());
    assert!(matches!(projection.ty(), Some(KotlinType::User(_))));

    let KotlinDeclaration::Function(text_function) = &declarations[2] else {
        panic!("expected text function");
    };
    let Some(KotlinFunctionBodyValue::Expression(KotlinExpression::Literal(literal))) =
        text_function.body().unwrap().value()
    else {
        panic!("expected string literal body");
    };
    let Some(KotlinLiteralValue::String(string)) = literal.value() else {
        panic!("expected string payload");
    };
    let parts = string.parts().collect::<Vec<_>>();
    assert!(
        parts
            .iter()
            .any(|part| matches!(part, KotlinStringPart::Content(_)))
    );
    assert!(
        parts
            .iter()
            .any(|part| matches!(part, KotlinStringPart::Identifier(_)))
    );
    let expression = parts.iter().find_map(|part| {
        let KotlinStringPart::Expression(expression) = part else {
            return None;
        };
        expression.expression()
    });
    assert!(matches!(expression, Some(KotlinExpression::Member(_))));
}
