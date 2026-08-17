use std::sync::Arc;

use rezel_common::{Input, IterMode, ParseErrorKind, ParseRequest, Parser, StringInput, TextRange};

#[test]
fn parser_preserves_original_ranges_across_unicode_translation() {
    let source = r"cl\u0061ss Café { \u002f\u002f note\u000a int value; }";
    let tree = rezel_lang_java::parser()
        .with_strict(true)
        .parse(source)
        .expect("translated Java source parses");
    assert_eq!(tree.len(), source.len().try_into().unwrap());

    let mut comments = Vec::new();
    tree.iterate(
        TextRange::new(0.into(), tree.len()),
        IterMode::default(),
        |node| {
            if node.name().as_ref() == "LineComment" {
                comments.push(node.range());
            }
            true
        },
        |_| {},
    );
    let expected = r"\u002f\u002f note";
    let start = source.find(expected).unwrap();
    let end = start + expected.len();
    assert_eq!(
        comments,
        [TextRange::new(
            start.try_into().unwrap(),
            end.try_into().unwrap()
        )]
    );
    assert_eq!(&source[start..end], expected);
}

#[test]
fn java_parser_implements_the_common_parser_interface() {
    let parser: Arc<dyn Parser> = Arc::new(rezel_lang_java::parser());
    parser
        .parse(r"cl\u0061ss Sample {}")
        .expect("Java parser remains composable through the common interface");
}

#[test]
fn parser_exposes_strict_and_recovery_modes() {
    let source = "class Sample { int value = ; }";
    let error = rezel_lang_java::parser()
        .with_strict(true)
        .parse(source)
        .unwrap_err();
    assert_eq!(error.kind(), ParseErrorKind::Syntax);

    let recovered = rezel_lang_java::parser().parse(source).unwrap();
    assert!(recovered.to_string().contains('⚠'));
}

#[test]
fn record_keyword_specialization_is_contextual() {
    let parser = rezel_lang_java::parser().with_strict(true);

    parser
        .parse("record Point(int x) {} record Line(int y) {}")
        .expect("record declarations specialize the contextual keyword");
    parser
        .parse(
            "class Names { int record; int record() { int record = 1; return record; } \
             int value() { return record(); } }",
        )
        .expect("record remains an identifier outside record declarations");
}

#[test]
fn malformed_unicode_escapes_are_strict_lexical_errors() {
    for source in [
        "class Sample { // \\u12xz\n}",
        r#"class Sample { String value = "\u12xz"; }"#,
    ] {
        let position = source.find(r"\u").unwrap().try_into().unwrap();
        rezel_lang_java::parser()
            .parse(source)
            .expect("recovering Java preserves malformed Unicode escape text");

        let error = rezel_lang_java::parser()
            .with_strict(true)
            .parse(source)
            .expect_err("strict Java rejects malformed eligible Unicode escapes");
        assert_eq!(error.kind(), ParseErrorKind::Syntax);
        assert_eq!(error.position(), Some(position));
    }
}

#[test]
fn character_literals_follow_java_utf16_code_unit_width() {
    let parser = rezel_lang_java::parser().with_strict(true);

    parser
        .parse(r"class Sample { char value = '\uD800'; }")
        .expect("javac accepts one isolated surrogate code unit");

    for source in [
        r"class Sample { char value = '\uD83D\uDE00'; }",
        "class Sample { char value = '😀'; }",
    ] {
        let error = parser
            .parse(source)
            .expect_err("a supplementary character needs two UTF-16 code units");
        assert_eq!(error.kind(), ParseErrorKind::Syntax);
    }
}

#[test]
fn text_blocks_preserve_escaped_delimiters_and_recover_at_eof() {
    let parser = rezel_lang_java::parser();
    let valid = r#"class Sample {
    String value = """
        before \""" after
        """;
}"#;
    parser
        .clone()
        .with_strict(true)
        .parse(valid)
        .expect("an escaped quote must not start the text-block closing delimiter");

    let unterminated = r#"class Sample { String value = """
        content"#;
    let error = parser
        .clone()
        .with_strict(true)
        .parse(unterminated)
        .expect_err("strict Java rejects an unterminated text block");
    assert_eq!(error.kind(), ParseErrorKind::Syntax);

    let recovered = parser
        .parse(unterminated)
        .expect("recovering Java retains an unterminated text block");
    let tree = recovered.to_string();
    assert!(tree.contains("TextBlock"));
    assert!(tree.contains('⚠'));
}

#[test]
fn selected_ranges_reject_java_translation_interiors() {
    for (source, endpoint) in [(r"\u0061", 3_u32), (r"\uD83D\uDE00", 6_u32)] {
        let input: Arc<dyn Input> = Arc::new(StringInput::try_new(source).unwrap());
        let request = ParseRequest::ranges(input, vec![TextRange::new(0.into(), endpoint.into())])
            .expect("the endpoint is a raw UTF-8 boundary");

        let Err(error) = rezel_lang_java::parser().create_parse(request) else {
            panic!("translation interior endpoint was accepted");
        };

        assert_eq!(error.kind(), ParseErrorKind::Input);
        assert_eq!(error.position(), Some(endpoint.into()));
        assert_eq!(
            error.message(),
            "parse range endpoint splits a translated character"
        );
    }
}

#[test]
fn identifiers_continue_across_selected_ranges() {
    let source = "class Ho---st {}";
    let input: Arc<dyn Input> = Arc::new(StringInput::try_new(source).unwrap());
    let request = ParseRequest::ranges(
        input,
        vec![
            TextRange::new(0.into(), 8.into()),
            TextRange::new(11.into(), source.len().try_into().unwrap()),
        ],
    )
    .expect("the selected ranges use valid UTF-8 boundaries");
    let mut parse = rezel_lang_java::parser()
        .with_strict(true)
        .create_parse(request)
        .expect("the selected-range Java parse starts");
    loop {
        if parse
            .advance()
            .expect("the selected identifier parses")
            .is_some()
        {
            break;
        }
    }
}
