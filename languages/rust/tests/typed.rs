#![forbid(unsafe_code)]

use rezel_lang_rust::{RustSourceFile, TypedNode};

#[test]
fn typed_syntax_exposes_the_source_file_root() {
    let source = "fn main() { let answer = 42; }\n";
    let tree = rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let file = RustSourceFile::downcast_from(tree.top_node()).unwrap();

    assert_eq!(file.text(source), Some(source));
    assert_eq!(file.syntax().name().as_ref(), "SourceFile");
}
