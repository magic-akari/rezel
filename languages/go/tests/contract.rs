#![forbid(unsafe_code)]

use rezel_lang_go::ast::{AstError, GoAst};

#[test]
fn recovery_trees_are_deterministic_and_rejected_by_ast_lowering() {
    let source = "package p\nfunc broken( {\n";
    assert!(
        rezel_lang_go::parser()
            .with_strict(true)
            .parse(source)
            .is_err()
    );
    let first = rezel_lang_go::parser().parse(source).unwrap();
    let second = rezel_lang_go::parser().parse(source).unwrap();
    assert_eq!(first.to_string(), second.to_string());
    assert!(first.to_string().contains('⚠'));
    assert_eq!(
        GoAst::lower(&first, source).unwrap_err(),
        AstError::RecoveryTree
    );
}

#[test]
fn accepts_go_1_26_syntax_beyond_the_pinned_lezer_grammar() {
    let source = r"package p

var _64bit = 64 /* one line */
var after = 1
var instantiatedArray = generic[[16]byte]
var instantiatedPair = generic[int32, int64]

var (
	grouped = 2 /* before close */
)

type names struct {
	true int
	false int
}

func visit(true bool) {
	var nil = true
	var other = false
	for range 3 {
	}
	_64bit = 32 /* contains
	a line ending */
	after = 2
	_ = nil
}
";
    let tree = rezel_lang_go::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let rendered = tree.to_string();

    assert!(rendered.contains("IndexListExpr"));
    assert!(rendered.contains("RangeClause"));
    assert!(!rendered.contains('⚠'));
}
