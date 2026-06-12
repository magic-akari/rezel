#![forbid(unsafe_code)]

use std::sync::Arc;

use rezel_common::{IterMode, ParseErrorKind, Parser, TextRange};

#[test]
fn rust_parser_implements_the_common_parser_interface() {
    let parser: Arc<dyn Parser> = Arc::new(rezel_lang_rust::parser());
    parser
        .parse("fn main() {}")
        .expect("Rust parser remains composable through the common interface");
}

#[test]
fn parser_exposes_strict_and_recovery_modes() {
    let source = "fn broken() { let value = 1 + ; }";
    let error = rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .unwrap_err();
    assert_eq!(error.kind(), ParseErrorKind::Syntax);

    let first = rezel_lang_rust::parser().parse(source).unwrap();
    let second = rezel_lang_rust::parser().parse(source).unwrap();
    assert_eq!(first.to_string(), second.to_string());
    assert!(first.to_string().contains('⚠'));
}

#[test]
fn external_tokens_preserve_utf8_ranges() {
    let source = r##"fn sample<T: Copy>(value: T) {
    let number = 1.5e+2f64;
    let raw = r#"你好 😀"#;
    let closure = |item: T| item;
}"##;
    let tree = rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("representative external-token syntax");
    assert_eq!(usize::from(tree.len()), source.len());

    let mut tokens = Vec::new();
    tree.iterate(
        TextRange::new(0.into(), tree.len()),
        IterMode::INCLUDE_ANONYMOUS,
        |node| {
            if matches!(
                node.name().as_ref(),
                "Float" | "RawString" | "<" | ">" | "|"
            ) {
                tokens.push((node.name().to_string(), node.range()));
            }
            true
        },
        |_| {},
    );
    let token_text = tokens
        .iter()
        .map(|(name, range)| {
            (
                name.as_str(),
                &source[usize::from(range.start())..usize::from(range.end())],
            )
        })
        .collect::<Vec<_>>();

    assert!(token_text.contains(&("Float", "1.5e+2f64")));
    assert!(token_text.contains(&("RawString", "r#\"你好 😀\"#")));
    assert!(token_text.contains(&("<", "<")));
    assert!(token_text.contains(&(">", ">")));
    assert_eq!(
        token_text
            .iter()
            .filter(|(name, text)| *name == "|" && *text == "|")
            .count(),
        2
    );
}
