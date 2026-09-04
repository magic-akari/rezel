#![forbid(unsafe_code)]

use std::sync::Arc;

use rezel_common::{Input, ParseErrorKind, ParseRequest, Parser, StringInput, TextRange};

#[test]
fn template_and_program_entry_points_accept_valid_utf8_php() {
    let template_source = "<!doctype html>你好<?php function greet(string $name): string { return \"你好, $name\"; } ?>尾";
    let template = rezel_lang_php::parser()
        .with_strict(true)
        .parse(template_source)
        .unwrap();
    assert_eq!(template.top_node().name().as_ref(), "Template");
    assert_eq!(usize::from(template.len()), template_source.len());

    let program_source =
        "final class Greeting { public function text(): string { return 'hello'; } }";
    let program = rezel_lang_php::program_parser()
        .with_strict(true)
        .parse(program_source)
        .unwrap();
    assert_eq!(program.top_node().name().as_ref(), "Program");
    assert_eq!(usize::from(program.len()), program_source.len());
}

#[test]
fn php_parser_implements_the_common_parser_interface() {
    let parser: Arc<dyn Parser> = Arc::new(rezel_lang_php::program_parser());
    parser
        .parse("echo 'shared parser interface';")
        .expect("PHP remains composable through the common parser interface");
}

#[test]
fn invalid_input_uses_the_fast_strict_failure_path() {
    let error = rezel_lang_php::program_parser()
        .with_strict(true)
        .parse("function broken( {")
        .unwrap_err();
    assert_eq!(error.kind(), ParseErrorKind::Syntax);
}

#[test]
fn keyword_qualified_names_cross_input_boundaries() {
    let source = "namespace___\\local();";
    let input: Arc<dyn Input> = Arc::new(StringInput::try_new(source).unwrap());
    let request = ParseRequest::ranges(
        input,
        vec![
            TextRange::new(0.into(), 9.into()),
            TextRange::new(12.into(), source.len().try_into().unwrap()),
        ],
    )
    .expect("the selected ranges use valid UTF-8 boundaries");
    let mut parse = rezel_lang_php::program_parser()
        .with_strict(true)
        .create_parse(request)
        .expect("the selected-range PHP parse starts");
    loop {
        if parse
            .advance()
            .expect("the qualified name parses across input ranges")
            .is_some()
        {
            break;
        }
    }

    rezel_lang_php::program_parser()
        .with_strict(true)
        .parse("NAMESPACE\\名();")
        .expect("the qualified-name lookahead falls back for Unicode");
}
