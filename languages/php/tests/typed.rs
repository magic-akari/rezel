#![forbid(unsafe_code)]

use rezel_lang_php::{PhpProgram, PhpTemplate, TypedNode};

#[test]
fn typed_roots_match_both_public_entry_points() {
    let template = rezel_lang_php::parser()
        .with_strict(true)
        .parse("text <?php echo 1; ?>")
        .unwrap();
    assert!(PhpTemplate::downcast_from(template.top_node()).is_ok());

    let program = rezel_lang_php::program_parser()
        .with_strict(true)
        .parse("echo 1;")
        .unwrap();
    assert!(PhpProgram::downcast_from(program.top_node()).is_ok());
}
