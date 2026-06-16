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
fn block_comments_keep_searching_for_the_end_after_repeated_slashes() {
    let source = r"
fn comments() {
    /*
     * https://example.com/reference
     * integer division // inside quoted source
     */
    consume();
}
";
    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("slashes inside a block comment do not hide its closing delimiter");
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

#[test]
fn unstable_float_width_suffixes_follow_rust_1_95_lexing() {
    let source = r"
fn floats() {
    let _: f16 = 1f16;
    let _: f16 = 10000.0_f16;
    let _: f128 = 1f128;
    let _: f128 = 3.14159265358979323846264338327950288419716939937510_f128;
}
";
    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("Rust 1.95 lexes f16 and f128 suffixes as floating-point literals");
}

fn raw_string_source(hash_count: usize) -> String {
    let hashes = "#".repeat(hash_count);
    format!("fn lexical() {{ let _ = r{hashes}\"body\"{hashes}; }}")
}
