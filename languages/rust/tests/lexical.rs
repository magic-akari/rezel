#![forbid(unsafe_code)]

use rezel_common::ParseErrorKind;

#[test]
fn raw_strings_enforce_the_255_hash_boundary() {
    let accepted = raw_string_source(255);
    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(&accepted)
        .expect("Rust raw strings allow 255 delimiter hashes");

    let rejected = raw_string_source(256);
    let error = rezel_lang_rust::parser()
        .with_strict(true)
        .parse(&rejected)
        .expect_err("Rust raw strings reject 256 delimiter hashes");
    assert_eq!(error.kind(), ParseErrorKind::Syntax);
}

#[test]
#[allow(clippy::needless_raw_string_hashes)] // One fewer collides with the nested raw literal.
fn reserved_guards_inside_literals_and_comments_remain_ordinary_content() {
    let source = r####"
fn lexical() {
    let _ = r###"## #"guarded"#"###;
    // ## #"guarded"#
    /* ## #"guarded"# */
}
"####;
    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("Edition 2024 reservations do not inspect comments or literal bodies");
}

#[test]
fn crlf_normalization_preserves_raw_source_coordinates() {
    let source = "fn lexical() {\r\n    let _ = \"left\r\nright\";\r\n}\r\n";
    let tree = rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("Rust normalizes CRLF pairs before tokenization");
    assert_eq!(usize::from(tree.len()), source.len());
}

#[test]
fn recovering_mode_keeps_a_tree_for_strict_literal_errors() {
    let source = r#"fn lexical() { let _ = c"\0"; }"#;
    let strict = rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect_err("strict Rust parsing rejects an interior C-string NUL");
    assert_eq!(strict.kind(), ParseErrorKind::Syntax);

    rezel_lang_rust::parser()
        .parse(source)
        .expect("recovering Rust parsing preserves an editor CST");
}

fn raw_string_source(hash_count: usize) -> String {
    let hashes = "#".repeat(hash_count);
    format!("fn lexical() {{ let _ = r{hashes}\"body\"{hashes}; }}")
}
