#![cfg(feature = "highlight")]

#[test]
fn php_highlighting_projects_core_syntax_tags() {
    let source = "<?php final class Greeting { public function text(): string { return 'hi'; } }";
    let tree = rezel_lang_php::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let mut spans = Vec::new();
    rezel_lang_php::highlight_spans(&tree, None, |span| {
        let range = span.range;
        let text = &source[usize::from(range.start())..usize::from(range.end())];
        let tags = span
            .tags
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        spans.push((text.to_owned(), tags));
    });

    assert!(
        spans.iter().any(|(text, tags)| {
            text == "class" && tags.iter().any(|tag| tag == "definitionKeyword")
        }),
        "{spans:?}"
    );
    assert!(
        spans.iter().any(|(text, tags)| {
            text == "Greeting" && tags.iter().any(|tag| tag == "definition(className)")
        }),
        "{spans:?}"
    );
    assert!(
        spans
            .iter()
            .any(|(text, tags)| { text == "'hi'" && tags.iter().any(|tag| tag == "string") }),
        "{spans:?}"
    );
}
