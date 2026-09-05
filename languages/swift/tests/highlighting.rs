#![cfg(feature = "highlight")]
#![forbid(unsafe_code)]

#[test]
fn swift_highlighting_projects_representative_syntax_roles() {
    let source = concat!(
        "import Foundation\n",
        "@MainActor public struct Greeter<T> {\n",
        "    let message: String = \"hello\"\n",
        "    func greet(argument: String) async -> String {\n",
        "        return argument\n",
        "    }\n",
        "}\n",
    );
    let tree = rezel_lang_swift::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let selected_names = [
        "import",
        "Foundation",
        "@MainActor",
        "public",
        "struct",
        "Greeter",
        "T",
        "let",
        "message",
        "\"hello\"",
        "func",
        "greet",
        "argument",
        "async",
        "->",
        "return",
    ];
    let mut spans = Vec::new();
    rezel_lang_swift::highlight_spans(&tree, None, |span| {
        let range = span.range;
        let text = &source[usize::from(range.start())..usize::from(range.end())];
        if !selected_names.contains(&text) {
            return;
        }
        let tags = span
            .tags
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        spans.push((text, tags));
    });

    assert_eq!(
        spans,
        [
            ("import", vec!["moduleKeyword".to_owned()]),
            ("Foundation", vec!["namespace".to_owned()]),
            ("@MainActor", vec!["annotation".to_owned()]),
            ("public", vec!["modifier".to_owned()]),
            ("struct", vec!["definitionKeyword".to_owned()]),
            ("Greeter", vec!["definition(typeName)".to_owned()]),
            ("T", vec!["definition(typeName)".to_owned()]),
            ("let", vec!["definitionKeyword".to_owned()]),
            ("message", vec!["definition(propertyName)".to_owned()]),
            ("\"hello\"", vec!["string".to_owned()]),
            ("func", vec!["definitionKeyword".to_owned()]),
            (
                "greet",
                vec!["function(definition(variableName))".to_owned()],
            ),
            ("argument", vec!["definition(variableName)".to_owned()]),
            ("async", vec!["modifier".to_owned()]),
            ("->", vec!["typeOperator".to_owned()]),
            ("return", vec!["controlKeyword".to_owned()]),
            ("argument", vec!["variableName".to_owned()]),
        ],
    );
}
