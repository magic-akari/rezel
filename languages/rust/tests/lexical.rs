#![forbid(unsafe_code)]

use rezel_common::{IterMode, ParseErrorKind, TextRange};

#[test]
fn source_file_prefix_and_pattern_whitespace_preserve_raw_coordinates() {
    let whitespace =
        "\u{0009}\u{000a}\u{000b}\u{000c}\u{000d}\u{0020}\u{0085}\u{200e}\u{200f}\u{2028}\u{2029}";
    let source = format!("\u{feff}#!/usr/bin/env rustx\r\nfn{whitespace}source_input() {{}}\r\n");
    let tree = rezel_lang_rust::parser()
        .with_strict(true)
        .parse(&source)
        .expect("Rust removes a leading BOM and shebang and recognizes Pattern_White_Space");

    assert_eq!(usize::from(tree.len()), source.len());
    assert!(tree.to_string().contains("FunctionItem"));

    let expected = source.find("\nfn").unwrap() + 1;
    let mut function_keyword = None;
    tree.iterate(
        TextRange::new(0.into(), tree.len()),
        IterMode::INCLUDE_ANONYMOUS,
        |node| {
            if node.name().as_ref() == "fn" {
                function_keyword = Some(node.range());
                return false;
            }
            true
        },
        |_| {},
    );
    let function_keyword = function_keyword.expect("the source contains a function keyword");
    assert_eq!(usize::from(function_keyword.start()), expected);
    assert_eq!(
        &source[usize::from(function_keyword.start())..usize::from(function_keyword.end())],
        "fn"
    );
}

#[test]
fn inner_attributes_are_not_misclassified_as_shebangs() {
    for source in [
        "#! /* outer /* inner */ tail */ \u{200e} [allow(dead_code)]\nfn retained() {}",
        "#! // ordinary comment\n \u{2028}[allow(dead_code)]\nfn retained() {}",
        "#! /// documentation comment\n [allow(dead_code)]\nfn retained() {}",
    ] {
        let tree = rezel_lang_rust::parser()
            .with_strict(true)
            .parse(source)
            .unwrap_or_else(|error| panic!("inner attribute source failed: {source:?}: {error}"));
        let shape = tree.to_string();
        assert!(shape.contains("InnerAttribute"), "{source:?}: {shape}");
        assert!(shape.contains("FunctionItem"), "{source:?}: {shape}");
    }
}

#[test]
fn source_prefix_transformations_only_apply_at_the_absolute_start() {
    for source in [
        " #!/usr/bin/env rustx\nfn misplaced() {}",
        "\n#!/usr/bin/env rustx\nfn misplaced() {}",
        "\u{feff}\u{feff}fn misplaced() {}",
    ] {
        let error = rezel_lang_rust::parser()
            .with_strict(true)
            .parse(source)
            .expect_err("a misplaced source prefix must remain syntax");
        assert_eq!(error.kind(), ParseErrorKind::Syntax, "{source:?}");
    }

    let shebang = "#!/usr/bin/env rustx";
    let tree = rezel_lang_rust::parser()
        .with_strict(true)
        .parse(shebang)
        .expect("a shebang may extend through EOF");
    assert_eq!(usize::from(tree.len()), shebang.len());
}

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
fn float_width_suffixes_follow_rust_1_95_lexing() {
    let source = r"
fn floats() {
    let _: f16 = 1f16;
    let _: f16 = 10000.0_f16;
    let _: f32 = 1f32;
    let _: f64 = 1f64;
    let _: f128 = 1f128;
    let _: f128 = 3.14159265358979323846264338327950288419716939937510_f128;
}
";
    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("Rust 1.95 lexes f16 and f128 suffixes as floating-point literals");
}

#[test]
fn identifier_runs_preserve_ascii_unicode_and_tokenizer_boundaries() {
    let source = r"
macro_rules! ascii_rules {
    ($ascii東42:ident) => { fn $ascii東42() {} };
}

fn ascii東42<'life_ascii>(r#raw_ascii42: usize) -> usize {
    r#raw_ascii42
}
";
    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("Rust identifiers retain ASCII runs around Unicode and tokenizer prefixes");
}

fn raw_string_source(hash_count: usize) -> String {
    let hashes = "#".repeat(hash_count);
    format!("fn lexical() {{ let _ = r{hashes}\"body\"{hashes}; }}")
}
