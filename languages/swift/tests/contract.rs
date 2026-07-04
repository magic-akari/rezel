#![forbid(unsafe_code)]

use std::sync::Arc;

use rezel_common::{ParseErrorKind, Parser};

#[test]
fn parser_exposes_common_strict_recovery_and_utf8_contracts() {
    let parser: Arc<dyn Parser> = Arc::new(rezel_lang_swift::parser());
    let source = "let greeting = \"你好 ☕️\"\n";
    let tree = parser.parse(source).unwrap();
    assert_eq!(usize::from(tree.len()), source.len());

    let broken = "func broken( {\n";
    let error = rezel_lang_swift::parser()
        .with_strict(true)
        .parse(broken)
        .unwrap_err();
    assert_eq!(error.kind(), ParseErrorKind::Syntax);

    let first = rezel_lang_swift::parser().parse(broken).unwrap();
    let second = rezel_lang_swift::parser().parse(broken).unwrap();
    assert_eq!(first.to_string(), second.to_string());
    assert!(first.to_string().contains('⚠'));
}

#[test]
fn member_if_configs_preserve_initializer_declaration_context() {
    let source = "struct S {\n#if FEATURE\ninit?(from thread: Thread) {}\n#endif\n}";
    let tree = rezel_lang_swift::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    assert_eq!(
        tree.to_string().matches("InitializerDeclaration").count(),
        1,
        "{tree}",
    );
}
