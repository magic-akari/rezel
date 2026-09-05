#![cfg(feature = "highlight")]

#[test]
fn json_highlighting_uses_the_official_abstract_tags() {
    let source = r#"{"name":"rezel","items":[true,null,12]}"#;
    let tree = rezel_lang_json::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let mut spans = Vec::new();
    rezel_lang_json::highlight_spans(&tree, None, |span| {
        let range = span.range;
        let text = &source[usize::from(range.start())..usize::from(range.end())];
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
            ("{", vec!["brace".to_owned()]),
            (r#""name""#, vec!["propertyName".to_owned()]),
            (":", vec!["separator".to_owned()]),
            (r#""rezel""#, vec!["string".to_owned()]),
            (",", vec!["separator".to_owned()]),
            (r#""items""#, vec!["propertyName".to_owned()]),
            (":", vec!["separator".to_owned()]),
            ("[", vec!["squareBracket".to_owned()]),
            ("true", vec!["bool".to_owned()]),
            (",", vec!["separator".to_owned()]),
            ("null", vec!["null".to_owned()]),
            (",", vec!["separator".to_owned()]),
            ("12", vec!["number".to_owned()]),
            ("]", vec!["squareBracket".to_owned()]),
            ("}", vec!["brace".to_owned()]),
        ],
    );
}
