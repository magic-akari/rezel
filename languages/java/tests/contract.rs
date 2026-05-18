use std::sync::Arc;

use rezel_common::{IterMode, ParseErrorKind, Parser, TextRange};

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
