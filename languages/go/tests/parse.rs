#![forbid(unsafe_code)]

use rezel_common::SyntaxNode;

const SUITES: &[(&str, &str)] = &[
    (
        "declarations",
        include_str!("upstream/lezer-go/test/declarations.txt"),
    ),
    (
        "expressions",
        include_str!("upstream/lezer-go/test/expressions.txt"),
    ),
    (
        "literals",
        include_str!("upstream/lezer-go/test/literals.txt"),
    ),
    (
        "source_files",
        include_str!("upstream/lezer-go/test/source_files.txt"),
    ),
    (
        "statements",
        include_str!("upstream/lezer-go/test/statements.txt"),
    ),
    ("types", include_str!("upstream/lezer-go/test/types.txt")),
];

struct IntentionalCstDifference {
    suite: &'static str,
    case: &'static str,
    upstream: &'static str,
    corrected: &'static str,
}

const INTENTIONAL_CST_DIFFERENCES: &[IntentionalCstDifference] = &[
    IntentionalCstDifference {
        suite: "expressions",
        case: "Unary expressions",
        upstream: "SourceFile(FunctionDecl(func,DefName,Parameters,Block(Assignment(VariableName,UnaryExp(LogicOp,UnaryExp(\"<-\",VariableName))),Assignment(VariableName,CallExpr(UnaryExp(DerefOp,VariableName),Arguments)))))",
        corrected: "SourceFile(FunctionDecl(func,DefName,Parameters,Block(Assignment(VariableName,UnaryExp(LogicOp,UnaryExp(\"<-\",VariableName))),Assignment(VariableName,UnaryExp(DerefOp,CallExpr(VariableName,Arguments))))))",
    },
    IntentionalCstDifference {
        suite: "statements",
        case: "Select statements",
        upstream: "SourceFile(FunctionDecl(func,DefName,Parameters,Block(SelectStatement(select,SelectBlock(Case(case,ReceiveStatement(DefName,UnaryExp(\"<-\",VariableName))),ExprStatement(CallExpr(VariableName,Arguments(VariableName))),Case(case,SendStatement(VariableName,\"<-\",VariableName)),ExprStatement(CallExpr(VariableName,Arguments(Number))),Case(case,ReceiveStatement(CallExpr(SelectorExpr(UnaryExp(\"<-\",VariableName),FieldName),Arguments(Number)))),ExprStatement(CallExpr(VariableName,Arguments(Number))),Case(default),ReturnStatement(return))))))",
        corrected: "SourceFile(FunctionDecl(func,DefName,Parameters,Block(SelectStatement(select,SelectBlock(Case(case,ReceiveStatement(DefName,UnaryExp(\"<-\",VariableName))),ExprStatement(CallExpr(VariableName,Arguments(VariableName))),Case(case,SendStatement(VariableName,\"<-\",VariableName)),ExprStatement(CallExpr(VariableName,Arguments(Number))),Case(case,ReceiveStatement(UnaryExp(\"<-\",CallExpr(SelectorExpr(VariableName,FieldName),Arguments(Number))))),ExprStatement(CallExpr(VariableName,Arguments(Number))),Case(default),ReturnStatement(return))))))",
    },
];

#[test]
fn recovering_cst_matches_upstream_except_postfix_precedence() {
    let mut count = 0;
    let mut intentional_differences = 0;
    for &(suite, source) in SUITES {
        for case in parse_cases(source) {
            count += 1;
            let tree = rezel_lang_go::parser()
                .parse(case.source)
                .unwrap_or_else(|error| panic!("{suite}/{}: recovery failed: {error}", case.name));
            let mut expected = case.tree.as_str();
            if let Some(difference) = INTENTIONAL_CST_DIFFERENCES
                .iter()
                .find(|difference| difference.suite == suite && difference.case == case.name)
            {
                assert_eq!(expected, difference.upstream);
                expected = difference.corrected;
                intentional_differences += 1;
            }
            let actual = project_tree(&tree.top_node(), expected);
            assert_eq!(actual, expected, "{suite}/{}: CST differs", case.name);
        }
    }
    assert_eq!(count, 66, "the pinned upstream fixture count changed");
    assert_eq!(
        intentional_differences,
        INTENTIONAL_CST_DIFFERENCES.len(),
        "every operator/postfix precedence correction must remain explicit"
    );
}

#[test]
fn strict_mode_accepts_exactly_the_error_free_upstream_fixtures() {
    for &(suite, source) in SUITES {
        for case in parse_cases(source) {
            let result = rezel_lang_go::parser().with_strict(true).parse(case.source);
            let expected = !case.tree.contains('⚠');
            assert_eq!(
                result.is_ok(),
                expected,
                "{suite}/{}: strict acceptance differs",
                case.name
            );
        }
    }
}

struct Case<'a> {
    name: &'a str,
    source: &'a str,
    tree: String,
}

fn parse_cases(source: &str) -> Vec<Case<'_>> {
    let source = source
        .strip_prefix("# ")
        .expect("upstream fixture starts with a case heading");
    source
        .split("\n# ")
        .map(|section| {
            let (name, body) = section
                .split_once('\n')
                .expect("case heading is followed by a body");
            let (input, expected) = body
                .split_once("\n==>\n")
                .expect("case contains one expected-tree separator");
            Case {
                name,
                source: input.trim(),
                tree: compact_tree(expected),
            }
        })
        .collect()
}

fn compact_tree(source: &str) -> String {
    let mut compact = String::with_capacity(source.len());
    let mut quoted = false;
    let mut escaped = false;
    for character in source.trim().chars() {
        if quoted {
            compact.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = false;
            }
        } else if character == '"' {
            quoted = true;
            compact.push(character);
        } else if !character.is_whitespace() {
            compact.push(character);
        }
    }
    assert!(!quoted, "unterminated quoted node name");
    compact
}

fn project_tree(root: &SyntaxNode, expected: &str) -> String {
    let roots = project_node(root, expected);
    assert_eq!(roots.len(), 1, "one visible top node");
    roots.into_iter().next().unwrap()
}

fn project_node(node: &SyntaxNode, expected: &str) -> Vec<String> {
    let name = node.name();
    if should_ignore(name.as_ref(), expected) {
        return Vec::new();
    }

    let mut children = Vec::new();
    let mut child = node.first_child();
    while let Some(current) = child {
        children.extend(project_node(&current, expected));
        child = current.next_sibling();
    }
    if name.is_empty() {
        return children;
    }
    let rendered_name = if name.as_ref() == "⚠"
        || name
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric())
    {
        name.to_string()
    } else {
        format!("{name:?}")
    };
    if children.is_empty() {
        vec![rendered_name]
    } else {
        vec![format!("{rendered_name}({})", children.join(","))]
    }
}

fn should_ignore(name: &str, expected: &str) -> bool {
    let has_non_word = name
        .chars()
        .any(|character| character != '_' && !character.is_ascii_alphanumeric());
    has_non_word && name != "⚠" && !expected.contains(&format!("{name:?}"))
}
