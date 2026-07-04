#![forbid(unsafe_code)]

use rezel_lang_swift::{SwiftSourceFile, TypedNode};

#[test]
fn typed_syntax_exposes_source_items_and_function_shape() {
    let source = "import Foundation\nfunc greet(name: String) -> String {\nreturn name\n}\n";
    let tree = rezel_lang_swift::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let file = SwiftSourceFile::downcast_from(tree.top_node()).unwrap();
    assert_eq!(file.items().count(), 2);

    let function = file
        .syntax()
        .children()
        .flat_map(|item| item.children())
        .find_map(|node| rezel_lang_swift::SwiftFunctionDeclaration::downcast_from(node).ok())
        .unwrap();
    assert_eq!(function.name().unwrap().text(source), Some("greet"));
    assert_eq!(function.parameters().unwrap().parameters().count(), 1);
    assert!(function.return_clause().is_some());
    assert!(function.body().is_some());
}
