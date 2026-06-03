#![cfg(feature = "highlight")]
#![forbid(unsafe_code)]

use rezel_common::{TextRange, TextSize, Tree};

#[test]
fn python_highlighting_exposes_representative_abstract_tags() {
    let source = concat!(
        "# module\n",
        "class Box:\n",
        "    def value(self):\n",
        "        return self.item + 42\n",
        "message = \"hello\"\n",
    );
    let tree = rezel_lang_python::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();

    let selected = rendered_spans(source, &tree, None)
        .into_iter()
        .filter(|(text, _)| {
            matches!(
                *text,
                "# module" | "class" | "Box" | "def" | "value" | "item" | "42" | "\"hello\""
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        selected,
        [
            ("# module", vec!["lineComment".to_owned()]),
            ("class", vec!["definitionKeyword".to_owned()]),
            ("Box", vec!["definition(className)".to_owned()]),
            ("def", vec!["definitionKeyword".to_owned()]),
            (
                "value",
                vec!["function(definition(variableName))".to_owned()]
            ),
            ("item", vec!["propertyName".to_owned()]),
            ("42", vec!["number".to_owned()]),
            ("\"hello\"", vec!["string".to_owned()]),
        ]
    );
}

#[test]
fn python_highlighting_clips_spans_to_the_requested_range() {
    let source = "answer = 42\n";
    let tree = rezel_lang_python::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let number_start = source.find("42").unwrap();
    let clipped_start = TextSize::try_from(number_start + 1).unwrap();
    let clipped_end = TextSize::try_from(number_start + 2).unwrap();

    assert_eq!(
        rendered_spans(
            source,
            &tree,
            Some(TextRange::new(clipped_start, clipped_end)),
        ),
        [("2", vec!["number".to_owned()])]
    );
}

fn rendered_spans<'source>(
    source: &'source str,
    tree: &Tree,
    range: Option<TextRange>,
) -> Vec<(&'source str, Vec<String>)> {
    let mut spans = Vec::new();
    rezel_lang_python::highlight_spans(tree, range, |span| {
        let range = span.range;
        let text = &source[usize::from(range.start())..usize::from(range.end())];
        let tags = span
            .tags
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        spans.push((text, tags));
    });
    spans
}
