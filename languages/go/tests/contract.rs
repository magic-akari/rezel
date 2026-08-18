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

#[test]
fn index_type_lookahead_keeps_type_only_arguments_and_receive_indexes_distinct() {
    let source = r"package p

var _ = values[<-ch]
var _ = values[mapValue]
var _ = generic[[16]byte]
var _ = generic[map[string]int]
var _ = generic[chan int]
var _ = generic[<- /* direction */ chan int]
var _ = generic[func(int) string]
var _ = generic[interface{ M() }]
var _ = generic[struct{ X int }]
";
    let tree = rezel_lang_go::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let rendered = tree.to_string();

    assert_eq!(rendered.matches("IndexExpr").count(), 9);
    assert!(rendered.contains("UnaryExp"));
    assert!(rendered.contains("ArrayType"));
    assert!(rendered.contains("MapType"));
    assert!(rendered.contains("ChannelType"));
    assert!(rendered.contains("FunctionType"));
    assert!(rendered.contains("InterfaceType"));
    assert!(rendered.contains("StructType"));
    assert!(!rendered.contains('⚠'));
}

#[test]
fn semicolon_lookahead_falls_back_for_unicode_comment_content() {
    let source = "package p\nvar value = 1 /* café\n */\nvar next = value\n";
    let tree = rezel_lang_go::parser()
        .with_strict(true)
        .parse(source)
        .expect("Unicode comment content preserves the preceding semicolon boundary");
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn qualified_types_follow_the_spec_package_shape() {
    let source = "package p\nvar value pkg.Type\nvar _ = pkg.Type{}\n";
    let tree = rezel_lang_go::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let rendered = tree.to_string();

    assert_eq!(rendered.matches("QualifiedType").count(), 2);
    assert!(!rendered.contains('⚠'));

    let invalid = "package p\nvar value root.pkg.Type\n";
    assert!(
        rezel_lang_go::parser()
            .with_strict(true)
            .parse(invalid)
            .is_err()
    );
}

#[test]
fn variable_name_postfix_prefixes_keep_each_dot_role() {
    let source = r"package p

var _ = pkg.Value
var _ = pkg.Func(value)

func inspect(value any) {
	_ = value.(pkg.Type)
	switch typed := value.(type) { default: _ = typed }
}
";
    let tree = rezel_lang_go::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let rendered = tree.to_string();

    assert_eq!(rendered.matches("SelectorExpr").count(), 2);
    assert!(rendered.contains("CallExpr"));
    assert!(rendered.contains("TypeAssertion"));
    assert!(rendered.contains("TypeSwitchStatement"));
    assert!(!rendered.contains('⚠'));
}
