struct CaseFile {
    name: &'static str,
    source: &'static str,
    cases: usize,
}

const CASE_FILES: &[CaseFile] = &[
    CaseFile {
        name: "arrays",
        source: include_str!("upstream/lezer-json/test/arrays.txt"),
        cases: 3,
    },
    CaseFile {
        name: "literals",
        source: include_str!("upstream/lezer-json/test/literals.txt"),
        cases: 3,
    },
    CaseFile {
        name: "numbers",
        source: include_str!("upstream/lezer-json/test/numbers.txt"),
        cases: 11,
    },
    CaseFile {
        name: "objects",
        source: include_str!("upstream/lezer-json/test/objects.txt"),
        cases: 3,
    },
    CaseFile {
        name: "strings",
        source: include_str!("upstream/lezer-json/test/strings.txt"),
        cases: 4,
    },
];

#[test]
fn parse_cases_match_expected_trees() {
    let parser = rezel_lang_json::parser();
    let mut executed = 0;

    for file in CASE_FILES {
        let cases = parse_case_file(file.source);
        assert_eq!(cases.len(), file.cases, "{} case inventory", file.name);

        for case in cases {
            let tree = parser
                .parse(case.source)
                .unwrap_or_else(|error| panic!("{}/{}: {error}", file.name, case.name));
            assert_eq!(
                render_tree(&tree),
                normalize_tree(case.expected),
                "{}/{}",
                file.name,
                case.name,
            );
            executed += 1;
        }
    }

    assert_eq!(executed, 24);
}

struct Case<'a> {
    name: &'a str,
    source: &'a str,
    expected: &'a str,
}

fn parse_case_file(source: &str) -> Vec<Case<'_>> {
    source
        .trim()
        .split("\n# ")
        .map(|section| {
            let section = section.strip_prefix("# ").unwrap_or(section);
            let (name, body) = section.split_once('\n').expect("upstream case has a name");
            let (source, expected) = body
                .split_once("\n==>\n")
                .expect("upstream case has an expected tree");
            Case {
                name,
                source: source.trim(),
                expected: expected.trim(),
            }
        })
        .collect()
}

fn normalize_tree(tree: &str) -> String {
    tree.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn render_tree(tree: &rezel_common::Tree) -> String {
    let rendered = render_node(&tree.top_node());
    assert_eq!(rendered.len(), 1, "a parse tree has one named top node");
    rendered.into_iter().next().expect("checked one top node")
}

fn render_node(node: &rezel_common::SyntaxNode) -> Vec<String> {
    let children = node
        .children()
        .flat_map(|child| render_node(&child))
        .collect();
    let node_type = node.node_type();
    let name = node_type.name();
    let is_named = !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_');
    if !is_named && !node_type.is_error() {
        return children;
    }

    if children.is_empty() {
        vec![name.to_owned()]
    } else {
        vec![format!("{name}({})", children.join(","))]
    }
}
