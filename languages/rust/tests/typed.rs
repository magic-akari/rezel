#![forbid(unsafe_code)]

use rezel_lang_rust::{
    RustAssignmentOperand, RustBinaryOperator, RustBoundedTypeElement, RustCondition,
    RustDeclaration, RustDeclarationListItem, RustDeclarationStatement, RustDelimitedTokenTree,
    RustExpression, RustFieldList, RustFunctionItem, RustFunctionName, RustFunctionParameter,
    RustFunctionType, RustGenericArgument, RustLiteral, RustPath, RustPathComponent, RustPattern,
    RustPrefixOperator, RustSourceFile, RustStatement, RustTokenTreeElement, RustTraitBound,
    RustType, RustTypeBound, RustTypeParameter, RustUseTree, TypedNode,
};

fn syntax_text<'source>(node: &rezel_common::SyntaxNode, source: &'source str) -> &'source str {
    &source[usize::from(node.from())..usize::from(node.to())]
}

fn parse_function(source: &str) -> RustFunctionItem {
    let tree = rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let file = RustSourceFile::downcast_from(tree.top_node()).unwrap();
    file.statements()
        .find_map(|statement| {
            let RustStatement::Declaration(RustDeclarationStatement::Item(
                RustDeclaration::Function(function),
            )) = statement
            else {
                return None;
            };
            Some(function)
        })
        .expect("expected a function declaration")
}

fn parse_alias_type(source: &str, strict: bool) -> RustType {
    let tree = rezel_lang_rust::parser()
        .with_strict(strict)
        .parse(source)
        .unwrap();
    let file = RustSourceFile::downcast_from(tree.top_node()).unwrap();
    let Some(RustStatement::Declaration(RustDeclarationStatement::Item(RustDeclaration::Type(
        alias,
    )))) = file.statements().next()
    else {
        panic!("expected a type alias");
    };
    alias.ty().unwrap()
}

fn parse_function_type(source: &str, strict: bool) -> RustFunctionType {
    match parse_alias_type(source, strict) {
        RustType::Function(function) => function,
        RustType::Dynamic(dynamic) => {
            let Some(RustTraitBound::Function(function)) = dynamic.bound() else {
                panic!("expected a function trait");
            };
            function
        }
        _ => panic!("expected a function pointer or trait object"),
    }
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
fn attributed_foreign_items_are_navigable_as_single_items() {
    let source = "unsafe extern \"C\" { #[link_name = \"symbol\"] fn renamed(x: i32, _: ...); }";
    let tree = rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let file = RustSourceFile::downcast_from(tree.top_node()).unwrap();
    let Some(RustStatement::Declaration(RustDeclarationStatement::Item(
        RustDeclaration::ForeignModule(foreign),
    ))) = file.statements().next()
    else {
        panic!("expected a foreign module");
    };
    let body = foreign.body().unwrap();
    let mut items = body.items();
    let Some(RustDeclarationListItem::Attributed(attributed)) = items.next() else {
        panic!("expected one attributed foreign item");
    };
    assert_eq!(attributed.attributes().count(), 1);
    let Some(RustDeclarationStatement::Item(RustDeclaration::Function(function))) =
        attributed.declaration()
    else {
        panic!("expected a foreign function");
    };
    let Some(RustFunctionParameter::Parameter(variadic)) =
        function.parameters().unwrap().parameters().nth(1)
    else {
        panic!("expected a named variadic parameter");
    };
    assert!(matches!(variadic.pattern(), Some(RustPattern::Wildcard(_))));
    assert!(variadic.ty().is_none());
    assert_eq!(
        syntax_text(&variadic.ellipsis_token().unwrap(), source),
        "..."
    );
    assert!(items.next().is_none());
}

#[test]
fn named_variadics_share_function_and_pointer_parameter_accessors() {
    for name in ["args", "_"] {
        let source = format!(
            "unsafe extern \"C\" fn f(callback: unsafe extern \"C\" fn(i32, {name}: ...), {name}: ...) {{}}"
        );
        let function = parse_function(&source);
        let parameters = function.parameters().unwrap();
        let Some(RustFunctionParameter::Parameter(callback)) = parameters.parameters().next()
        else {
            panic!("expected a callback parameter");
        };
        assert!(callback.ellipsis_token().is_none());
        let Some(RustType::Function(pointer)) = callback.ty() else {
            panic!("expected a function pointer");
        };
        for list in [parameters, pointer.parameters().unwrap()] {
            let Some(RustFunctionParameter::Parameter(variadic)) = list.parameters().nth(1) else {
                panic!("expected a named variadic parameter");
            };
            assert_eq!(variadic.pattern().unwrap().text(&source), Some(name));
            assert!(variadic.ty().is_none());
            assert_eq!(
                syntax_text(&variadic.ellipsis_token().unwrap(), &source),
                "..."
            );
        }
    }

    let function = parse_function("unsafe extern \"C\" fn f(x: i32, ...) {}");
    let Some(RustFunctionParameter::Variadic(variadic)) =
        function.parameters().unwrap().parameters().nth(1)
    else {
        panic!("expected an unnamed variadic parameter");
    };
    assert!(variadic.ellipsis_token().is_some());
}

#[test]
fn unsafe_attribute_meta_exposes_its_token_and_arguments() {
    let source = "#[unsafe(no_mangle)] pub extern \"C\" fn exported() {}";
    let tree = rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let file = RustSourceFile::downcast_from(tree.top_node()).unwrap();
    let Some(RustStatement::AttributedItem(item)) = file.statements().next() else {
        panic!("expected an attributed item");
    };
    let meta = item.attributes().next().unwrap().meta().unwrap();
    assert!(meta.unsafe_token().is_some());
    assert!(matches!(
        meta.tokens(),
        Some(RustDelimitedTokenTree::Parenthesized(_))
    ));
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
fn typed_syntax_navigates_items_generics_uses_and_macros() {
    let source = r"
pub struct Pair<T: Copy, U = T>
where
    T: Send,
{
    pub left: T,
    right: U,
}

use crate::module::{Thing as AliasThing, *};

macro_rules! choose {
    ($value:expr) => { $value }
}

pub macro identity($value:expr) { $value }
";
    let tree = rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let file = RustSourceFile::downcast_from(tree.top_node()).unwrap();
    let declarations = file
        .statements()
        .map(|statement| {
            let RustStatement::Declaration(RustDeclarationStatement::Item(declaration)) = statement
            else {
                panic!("expected an item declaration");
            };
            declaration
        })
        .collect::<Vec<_>>();
    assert_eq!(declarations.len(), 4);

    let RustDeclaration::Struct(structure) = &declarations[0] else {
        panic!("expected a struct");
    };
    assert!(structure.visibility().is_some());
    assert_eq!(structure.name().unwrap().text(source), Some("Pair"));
    let parameters = structure
        .type_parameters()
        .unwrap()
        .parameters()
        .collect::<Vec<_>>();
    assert!(matches!(
        parameters.as_slice(),
        [
            RustTypeParameter::Constrained(_),
            RustTypeParameter::Optional(_)
        ]
    ));
    assert_eq!(structure.where_clause().unwrap().predicates().count(), 1);
    let RustFieldList::Named(fields) = structure.fields().unwrap() else {
        panic!("expected named struct fields");
    };
    let fields = fields.fields().collect::<Vec<_>>();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].name().unwrap().text(source), Some("left"));
    assert!(fields[0].visibility().is_some());
    assert_eq!(fields[1].name().unwrap().text(source), Some("right"));
    assert!(fields[1].visibility().is_none());

    let RustDeclaration::Use(use_declaration) = &declarations[1] else {
        panic!("expected a use declaration");
    };
    let RustUseTree::ScopedList(scoped) = use_declaration.tree().unwrap() else {
        panic!("expected a scoped use list");
    };
    assert_eq!(scoped.segments().count(), 2);
    let use_trees = scoped.list().unwrap().trees().collect::<Vec<_>>();
    assert!(matches!(
        use_trees.as_slice(),
        [RustUseTree::Alias(_), RustUseTree::Wildcard(_)]
    ));

    let RustDeclaration::MacroDefinition(definition) = &declarations[2] else {
        panic!("expected a macro_rules definition");
    };
    assert_eq!(definition.name().unwrap().text(source), Some("choose"));
    let rules = definition.rules().collect::<Vec<_>>();
    assert_eq!(rules.len(), 1);
    let delimited_tokens = rules[0].delimited_tokens().collect::<Vec<_>>();
    assert_eq!(delimited_tokens.len(), 2);
    let RustDelimitedTokenTree::Parenthesized(pattern) = &delimited_tokens[0] else {
        panic!("expected a parenthesized macro pattern");
    };
    let elements = pattern.elements().collect::<Vec<_>>();
    let [RustTokenTreeElement::Binding(binding)] = elements.as_slice() else {
        panic!("expected one macro fragment binding");
    };
    assert_eq!(binding.metavariable().unwrap().text(source), Some("$value"));
    assert_eq!(binding.fragment().unwrap().text(source), Some("expr"));

    let RustDeclaration::DeclarativeMacro(declaration) = &declarations[3] else {
        panic!("expected a declarative macro item");
    };
    assert!(declaration.visibility().is_some());
    assert_eq!(declaration.name().unwrap().text(source), Some("identity"));
    assert!(declaration.arguments().is_some());
    assert_eq!(declaration.body().unwrap().text(source), Some("{ $value }"));
}

#[test]
fn function_trait_parameters_are_navigable_as_parameters() {
    for (signature, expected) in [
        ("Fn()", vec![]),
        ("FnMut(u8, u16) -> bool", vec!["u8", "u16"]),
        ("FnOnce((u8, u16))", vec!["(u8, u16)"]),
    ] {
        let source = format!("type F = dyn {signature};");
        let function = parse_function_type(&source, true);
        let actual = function
            .parameters()
            .unwrap()
            .parameters()
            .map(|parameter| {
                let RustFunctionParameter::Parameter(parameter) = parameter else {
                    panic!("expected a type-only parameter");
                };
                assert!(parameter.pattern().is_none());
                assert!(parameter.ellipsis_token().is_none());
                parameter.ty().unwrap().text(&source).unwrap().to_owned()
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "{signature}");
    }
}

#[test]
fn function_type_fields_follow_signature_roles() {
    for (signature, path, output) in [
        ("dyn Fn()", Some("Fn"), None),
        ("dyn Fn(u8) -> bool", Some("Fn"), Some("bool")),
        (
            "dyn ops::FnMut((u8, u16)) -> ()",
            Some("ops::FnMut"),
            Some("()"),
        ),
        ("dyn Fn(fn(u8) -> bool)", Some("Fn"), None),
        ("fn()", None, None),
        ("fn() -> Result", None, Some("Result")),
        (
            "unsafe extern \"C\" fn() -> *const u8",
            None,
            Some("*const u8"),
        ),
        ("fn() -> fn(u8) -> bool", None, Some("fn(u8) -> bool")),
    ] {
        let source = format!("type F = {signature};");
        let function = parse_function_type(&source, true);
        assert_eq!(
            function.trait_path().and_then(|path| path.text(&source)),
            path,
            "{signature}"
        );
        assert_eq!(
            function.return_type().and_then(|ty| ty.text(&source)),
            output,
            "{signature}"
        );
    }
}

#[test]
fn function_type_fields_tolerate_missing_return_types() {
    for (source, path) in [
        ("type F = fn() -> ;", None),
        ("type F = dyn Fn() -> ;", Some("Fn")),
    ] {
        let function = parse_function_type(source, false);
        assert_eq!(
            function.trait_path().and_then(|path| path.text(source)),
            path
        );
        assert!(function.return_type().is_none());
    }
}

#[test]
fn parenthesized_bounds_preserve_trait_and_modifier_roles() {
    let source = "fn f<T: (Copy) + (?Sized)>() {}";
    let function = parse_function(source);
    let Some(RustTypeParameter::Constrained(parameter)) =
        function.type_parameters().unwrap().parameters().next()
    else {
        panic!("expected a constrained parameter")
    };
    let bounds = parameter.bounds().unwrap();
    let mut bounds = bounds.bounds();
    let Some(RustTypeBound::Trait(RustTraitBound::Parenthesized(first))) = bounds.next() else {
        panic!("expected a parenthesized trait bound")
    };
    let Some(RustTraitBound::Path(path)) = first.bound() else {
        panic!("expected the Copy path")
    };
    assert_eq!(path.text(source), Some("Copy"));
    let Some(RustTypeBound::Trait(RustTraitBound::Parenthesized(second))) = bounds.next() else {
        panic!("expected a parenthesized relaxed bound")
    };
    let Some(RustTraitBound::Removed(removed)) = second.bound() else {
        panic!("expected the question-mark modifier")
    };
    let Some(RustTraitBound::Path(path)) = removed.bound() else {
        panic!("expected the Sized path")
    };
    assert_eq!(path.text(source), Some("Sized"));
    assert!(bounds.next().is_none());
}

#[test]
fn bounded_types_expose_parenthesized_following_bounds() {
    for prefix in ["dyn", "impl"] {
        let source = format!("type F = {prefix} (Send) + (Sync);");
        let RustType::Bounded(bounded) = parse_alias_type(&source, true) else {
            panic!("expected a bounded type")
        };
        let mut elements = bounded.elements();
        let bound = match elements.next().unwrap() {
            RustBoundedTypeElement::Type(RustType::Dynamic(ty)) => ty.bound(),
            RustBoundedTypeElement::Type(RustType::Abstract(ty)) => ty.bound(),
            _ => panic!("expected the initial dyn or impl type"),
        };
        let Some(RustTraitBound::Parenthesized(first)) = bound else {
            panic!("expected the first parenthesized trait bound")
        };
        assert_eq!(first.bound().unwrap().text(&source), Some("Send"));
        let Some(RustBoundedTypeElement::Parenthesized(second)) = elements.next() else {
            panic!("expected the following parenthesized trait bound")
        };
        assert_eq!(second.bound().unwrap().text(&source), Some("Sync"));
        assert!(elements.next().is_none());
    }
}

#[test]
fn typed_syntax_navigates_composite_types() {
    let source = r"
type Layout<'a, T> = &'a ([T; 3], *const T);

fn inspect<T>(items: &[T], fallback: T) {}
";
    let tree = rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let file = RustSourceFile::downcast_from(tree.top_node()).unwrap();
    let declarations = file
        .statements()
        .map(|statement| {
            let RustStatement::Declaration(RustDeclarationStatement::Item(declaration)) = statement
            else {
                panic!("expected an item declaration");
            };
            declaration
        })
        .collect::<Vec<_>>();
    assert_eq!(declarations.len(), 2);

    let RustDeclaration::Type(layout) = &declarations[0] else {
        panic!("expected a type alias");
    };
    let RustType::Reference(reference) = layout.ty().unwrap() else {
        panic!("expected a reference type");
    };
    assert_eq!(reference.lifetime().unwrap().text(source), Some("'a"));
    let RustType::Tuple(tuple) = reference.ty().unwrap() else {
        panic!("expected a tuple type");
    };
    let elements = tuple.elements().collect::<Vec<_>>();
    let [RustType::Array(array), RustType::Pointer(pointer)] = elements.as_slice() else {
        panic!("expected array and pointer tuple elements");
    };
    assert!(matches!(array.element_type(), Some(RustType::Path(_))));
    assert_eq!(array.length().unwrap().text(source), Some("3"));
    assert!(matches!(pointer.ty(), Some(RustType::Path(_))));

    let RustDeclaration::Function(function) = &declarations[1] else {
        panic!("expected a function");
    };
    let parameters = function
        .parameters()
        .unwrap()
        .parameters()
        .collect::<Vec<_>>();
    let [
        RustFunctionParameter::Parameter(items),
        RustFunctionParameter::Parameter(fallback),
    ] = parameters.as_slice()
    else {
        panic!("expected two ordinary parameters");
    };
    let RustType::Reference(items_reference) = items.ty().unwrap() else {
        panic!("expected a reference parameter");
    };
    let RustType::Array(slice) = items_reference.ty().unwrap() else {
        panic!("expected the slice-shaped array type");
    };
    assert!(slice.length().is_none());
    assert!(matches!(fallback.ty(), Some(RustType::Path(_))));
}

#[test]
fn typed_syntax_navigates_recursive_paths() {
    let source = r"
fn inspect<T>(
    value: crate::module::Container<super::Item>,
) -> crate::module::Trait<T>::Assoc {
    crate::module::Record { value }
}
";
    let function = parse_function(source);
    let parameters = function
        .parameters()
        .unwrap()
        .parameters()
        .collect::<Vec<_>>();
    let [RustFunctionParameter::Parameter(value)] = parameters.as_slice() else {
        panic!("expected one ordinary parameter");
    };
    let RustType::Generic(container) = value.ty().unwrap() else {
        panic!("expected a generic container type");
    };
    let container_path = container.path().unwrap();
    assert_eq!(
        container_path.segment().unwrap().text(source),
        Some("Container")
    );
    let module_path = container_path.prefix().unwrap();
    assert_eq!(module_path.segment().unwrap().text(source), Some("module"));
    assert_eq!(
        module_path
            .prefix()
            .unwrap()
            .segment()
            .unwrap()
            .text(source),
        Some("crate")
    );

    let arguments = container
        .arguments()
        .unwrap()
        .arguments()
        .collect::<Vec<_>>();
    let [RustGenericArgument::Type(RustType::Path(item_path))] = arguments.as_slice() else {
        panic!("expected one path type argument");
    };
    assert_eq!(item_path.segment().unwrap().text(source), Some("Item"));
    assert_eq!(
        item_path.prefix().unwrap().segment().unwrap().text(source),
        Some("super")
    );

    let RustType::Path(associated_type) = function.return_type().unwrap() else {
        panic!("expected an associated type path");
    };
    assert_eq!(
        associated_type.segment().unwrap().text(source),
        Some("Assoc")
    );
    assert_eq!(
        associated_type.type_arguments().unwrap().text(source),
        Some("<T>")
    );
    assert_eq!(
        associated_type
            .prefix()
            .unwrap()
            .segment()
            .unwrap()
            .text(source),
        Some("Trait")
    );

    let Some(RustStatement::Expression(tail)) = function.body().unwrap().statements().next() else {
        panic!("expected a struct expression");
    };
    let RustExpression::Struct(record) = tail.expression().unwrap() else {
        panic!("expected a struct expression");
    };
    let RustPath::Scoped(record_path) = record.path().unwrap() else {
        panic!("expected a scoped struct path");
    };
    let components = record_path.components().collect::<Vec<_>>();
    let [
        RustPathComponent::Scoped(prefix),
        RustPathComponent::Identifier(record_name),
    ] = components.as_slice()
    else {
        panic!("expected a recursive prefix and final record name");
    };
    assert_eq!(record_name.text(source), Some("Record"));
    let prefix_components = prefix.components().collect::<Vec<_>>();
    let [
        RustPathComponent::CrateKeyword(crate_keyword),
        RustPathComponent::Identifier(module_name),
    ] = prefix_components.as_slice()
    else {
        panic!("expected crate and module path components");
    };
    assert_eq!(crate_keyword.text(source), Some("crate"));
    assert_eq!(module_name.text(source), Some("module"));
}

#[test]
fn typed_syntax_navigates_patterns_literals_and_assignments() {
    let source = r#"
fn bind(items: &[u8], fallback: u8) {
    let [first, rest @ ..] = items else { return; };
    "line\n";
    _ = fallback;
}
"#;
    let function = parse_function(source);
    let mut statements = function.body().unwrap().statements();
    let Some(RustStatement::Declaration(RustDeclarationStatement::Let(binding))) =
        statements.next()
    else {
        panic!("expected a let-else declaration");
    };
    let RustPattern::Slice(slice_pattern) = binding.pattern().unwrap() else {
        panic!("expected a slice pattern");
    };
    let patterns = slice_pattern.patterns().collect::<Vec<_>>();
    let [RustPattern::Binding(first), RustPattern::Captured(rest)] = patterns.as_slice() else {
        panic!("expected a binding followed by a captured rest pattern");
    };
    assert_eq!(first.text(source), Some("first"));
    assert_eq!(rest.binding().unwrap().text(source), Some("rest"));
    let RustPattern::Rest(rest_pattern) = rest.pattern().unwrap() else {
        panic!("expected a rest subpattern");
    };
    assert_eq!(rest_pattern.text(source), Some(".."));

    let Some(RustStatement::Expression(string_statement)) = statements.next() else {
        panic!("expected a string expression statement");
    };
    let RustExpression::Literal(RustLiteral::String(string)) =
        string_statement.expression().unwrap()
    else {
        panic!("expected a string literal");
    };
    assert_eq!(string.escapes().count(), 1);

    let Some(RustStatement::Expression(assignment_statement)) = statements.next() else {
        panic!("expected an assignment statement");
    };
    let RustExpression::Assignment(assignment) = assignment_statement.expression().unwrap() else {
        panic!("expected an underscore assignment");
    };
    let operands = assignment.operands().collect::<Vec<_>>();
    assert!(matches!(
        operands.as_slice(),
        [
            RustAssignmentOperand::Discard(_),
            RustAssignmentOperand::Expression(_)
        ]
    ));
    assert!(assignment.operator().is_none());
    assert!(statements.next().is_none());
}

#[test]
fn typed_syntax_navigates_control_flow_and_operators() {
    let source = r"
fn choose(first: &i32, fallback: i32) -> Option<i32> {
    match if *first > fallback { *first } else { fallback } {
        value if value >= fallback => Some(value),
        _ => None,
    }
}
";
    let function = parse_function(source);
    let mut statements = function.body().unwrap().statements();
    let Some(RustStatement::Expression(tail)) = statements.next() else {
        panic!("expected a tail expression");
    };
    let RustExpression::Match(match_expression) = tail.expression().unwrap() else {
        panic!("expected a match expression");
    };
    let RustExpression::If(if_expression) = match_expression.scrutinee().unwrap() else {
        panic!("expected an if scrutinee");
    };
    let RustCondition::Expression(RustExpression::Binary(condition)) =
        if_expression.condition().unwrap()
    else {
        panic!("expected a binary if condition");
    };
    assert!(matches!(
        condition.operator(),
        Some(RustBinaryOperator::Compare(_))
    ));
    let RustExpression::Unary(dereference) = condition.left().unwrap() else {
        panic!("expected a dereference on the left");
    };
    assert!(matches!(
        dereference.operator(),
        Some(RustPrefixOperator::Dereference(_))
    ));
    assert!(matches!(
        dereference.operand(),
        Some(RustExpression::Path(_))
    ));
    assert!(matches!(condition.right(), Some(RustExpression::Path(_))));
    assert_eq!(if_expression.blocks().count(), 2);

    let arms = match_expression.body().unwrap().arms().collect::<Vec<_>>();
    assert_eq!(arms.len(), 2);
    assert!(matches!(arms[0].pattern(), Some(RustPattern::Binding(_))));
    let RustCondition::Expression(RustExpression::Binary(guard_condition)) =
        arms[0].guard().unwrap().condition().unwrap()
    else {
        panic!("expected a binary match guard");
    };
    assert!(matches!(
        guard_condition.operator(),
        Some(RustBinaryOperator::Compare(_))
    ));
    let RustExpression::Call(call) = arms[0].expression().unwrap() else {
        panic!("expected a call expression");
    };
    assert!(matches!(call.callee(), Some(RustExpression::Path(_))));
    assert_eq!(call.arguments().unwrap().arguments().count(), 1);
    assert!(matches!(arms[1].pattern(), Some(RustPattern::Wildcard(_))));
    assert!(matches!(
        arms[1].expression(),
        Some(RustExpression::Path(_))
    ));
    assert!(statements.next().is_none());
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
