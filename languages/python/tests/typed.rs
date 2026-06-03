#![forbid(unsafe_code)]

use rezel_lang_python::{PythonExpressionNode, PythonStatementNode, PythonTop, TypedNode};

#[test]
fn public_typed_cst_projects_nested_declarations_and_expressions() {
    let source = "class Box:\n    def value(self):\n        return self.item\n";
    let tree = rezel_lang_python::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();

    let PythonTop::Module(module) = PythonTop::downcast_from(tree.top_node()).unwrap() else {
        panic!("the default parser must return a Module top node");
    };
    let PythonStatementNode::Class(class) = module.statements().next().unwrap() else {
        panic!("the module statement must be a class");
    };
    assert_eq!(class.name().unwrap().text(source), Some("Box"));

    let PythonStatementNode::Function(function) =
        class.body().unwrap().statements().next().unwrap()
    else {
        panic!("the class body statement must be a function");
    };
    assert_eq!(function.name().unwrap().text(source), Some("value"));
    assert_eq!(function.parameters().unwrap().text(source), Some("(self)"));

    let PythonStatementNode::Return(return_statement) =
        function.body().unwrap().statements().next().unwrap()
    else {
        panic!("the function body statement must be a return");
    };
    let PythonExpressionNode::Member(member) = return_statement.values().next().unwrap() else {
        panic!("the return value must be a member expression");
    };
    assert_eq!(member.text(source), Some("self.item"));
    let PythonExpressionNode::VariableName(base) = member.base().unwrap() else {
        panic!("the member base must be a variable name");
    };
    assert_eq!(base.text(source), Some("self"));
}
