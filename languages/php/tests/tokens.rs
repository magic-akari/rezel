#![forbid(unsafe_code)]

fn parse_program(source: &str) -> String {
    rezel_lang_php::program_parser()
        .with_strict(true)
        .parse(source)
        .unwrap_or_else(|error| panic!("valid PHP token fixture failed: {error}\n{source}"))
        .to_string()
}

#[test]
fn keywords_are_case_insensitive_without_stealing_type_names() {
    let tree =
        parse_program("EcHo TRUE; function accepts_bool(bool $value): bool { return $value; }");
    assert!(tree.contains("EchoStatement"), "{tree}");
    assert!(tree.contains("Boolean"), "{tree}");
    assert_eq!(tree.matches("NamedType").count(), 2, "{tree}");
}

#[test]
fn external_expression_tokens_cover_casts_and_heredocs() {
    let cast = parse_program("$number = (integer) $value;");
    assert!(cast.contains("CastExpression"), "{cast}");

    let nowdoc = parse_program("echo <<<'TXT'\n你好 $literal\nTXT;\n");
    assert!(nowdoc.contains("HeredocString"), "{nowdoc}");

    let heredoc = parse_program("echo <<<TEXT\nhello world\nTEXT;\n");
    assert!(heredoc.contains("HeredocString"), "{heredoc}");
}

#[test]
fn interpolation_and_close_tags_preserve_php_boundaries() {
    let interpolation = parse_program("echo \"Hello $user->name and {$user->title}\";");
    assert!(interpolation.contains("Interpolation"), "{interpolation}");

    let source = "<h1><?= $title ?></h1>";
    let tree = rezel_lang_php::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let printed = tree.to_string();
    assert!(printed.contains("ExpressionStatement"), "{printed}");
    assert!(printed.contains("PhpClose"), "{printed}");
    assert_eq!(usize::from(tree.len()), source.len());
}
