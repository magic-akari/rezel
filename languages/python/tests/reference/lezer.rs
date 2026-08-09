use rezel_common::{IterMode, ParseError, Tree, TreeCursor};
use serde::Deserialize;

const CASE_SCHEMA: &str = "rezel.lezer-python-reference-cases.v1";
const SNAPSHOT_SCHEMA: &str = "rezel.lezer-python-reference-snapshot.v1";
const CASES: &str = include_str!("../../../../tools/references/lezer/cases/python.json");
const SNAPSHOT: &str = include_str!("../../../../tools/references/lezer/snapshots/python.json");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseManifest {
    schema: String,
    strict: Vec<ReferenceCase>,
    recovering: Vec<ReferenceCase>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceCase {
    id: String,
    top: String,
    source: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    schema: String,
    reference: ReferenceIdentity,
    coordinates: String,
    strict: Vec<ExpectedCase>,
    recovering: Vec<ExpectedCase>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceIdentity {
    grammar: ReferenceArtifact,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceArtifact {
    path: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedCase {
    id: String,
    top: String,
    tree: ReferenceNode,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct ReferenceNode {
    name: String,
    from: usize,
    to: usize,
    #[serde(default)]
    children: Vec<ReferenceNode>,
}

#[test]
fn strict_python_csts_match_the_maintained_lezer_grammar() {
    let (cases, snapshot) = reference_data();
    assert_eq!(cases.strict.len(), snapshot.strict.len());
    for (case, expected) in cases.strict.iter().zip(&snapshot.strict) {
        assert_eq!(case.id, expected.id);
        assert_eq!(case.top, expected.top);
        let strict = parse_case(case, true)
            .unwrap_or_else(|error| panic!("strict case {} failed: {error}", case.id));
        let recovering = parse_case(case, false)
            .unwrap_or_else(|error| panic!("recovering case {} failed: {error}", case.id));
        assert_eq!(project(&strict), project(&recovering));
        assert_eq!(
            project(&strict),
            expected.tree,
            "reference case {}",
            case.id
        );
    }
}

#[test]
fn invalid_python_csts_match_deterministic_lezer_recovery() {
    let (cases, snapshot) = reference_data();
    assert_eq!(cases.recovering.len(), snapshot.recovering.len());
    for (case, expected) in cases.recovering.iter().zip(&snapshot.recovering) {
        assert_eq!(case.id, expected.id);
        assert_eq!(case.top, expected.top);
        assert!(parse_case(case, true).is_err());
        let first = parse_case(case, false)
            .unwrap_or_else(|error| panic!("recovery case {} failed: {error}", case.id));
        let second = parse_case(case, false)
            .unwrap_or_else(|error| panic!("recovery case {} failed twice: {error}", case.id));
        assert_eq!(project(&first), project(&second));
        assert_eq!(project(&first), expected.tree, "recovery case {}", case.id);
    }
}

#[test]
fn lowering_adapters_do_not_expand_the_persistent_cst() {
    let (_, snapshot) = reference_data();
    let forbidden = [
        "Argument",
        "DictionaryItem",
        "PositionalOnlySeparator",
        "RelativeImportDot",
        "RelativeImportEllipsis",
    ];
    for case in &snapshot.strict {
        for name in forbidden {
            assert!(
                !contains_kind(&case.tree, name),
                "lowering-only node {name} leaked into case {}",
                case.id
            );
        }
    }

    for (id, expected_nodes) in [
        ("parameter-separators", 25),
        ("relative-import-level", 11),
        ("call-and-dictionary-items", 26),
    ] {
        let case = snapshot
            .strict
            .iter()
            .find(|case| case.id == id)
            .unwrap_or_else(|| panic!("missing CST cost case {id}"));
        assert_eq!(
            node_count(&case.tree),
            expected_nodes,
            "persistent CST node budget for {id}"
        );
    }
}

fn contains_kind(node: &ReferenceNode, name: &str) -> bool {
    node.name == name || node.children.iter().any(|child| contains_kind(child, name))
}

fn node_count(node: &ReferenceNode) -> usize {
    1 + node.children.iter().map(node_count).sum::<usize>()
}

fn reference_data() -> (CaseManifest, Snapshot) {
    let cases: CaseManifest = serde_json::from_str(CASES).expect("valid Python cases");
    let snapshot: Snapshot = serde_json::from_str(SNAPSHOT).expect("valid Python snapshot");
    assert_eq!(cases.schema, CASE_SCHEMA);
    assert_eq!(snapshot.schema, SNAPSHOT_SCHEMA);
    assert_eq!(snapshot.coordinates, "raw-utf8-bytes");
    assert_eq!(
        snapshot.reference.grammar.path,
        "languages/python/src/python.grammar"
    );
    assert_eq!(snapshot.reference.grammar.sha256.len(), 64);
    (cases, snapshot)
}

fn parse_case(case: &ReferenceCase, strict: bool) -> Result<Tree, ParseError> {
    rezel_lang_python::parser()
        .with_top(&case.top)?
        .with_strict(strict)
        .parse(&case.source)
}

fn project(tree: &Tree) -> ReferenceNode {
    project_cursor(&mut tree.cursor(IterMode::INCLUDE_ANONYMOUS))
}

fn project_cursor(cursor: &mut TreeCursor) -> ReferenceNode {
    let name = cursor.name().to_string();
    let from = cursor.from().into();
    let to = cursor.to().into();
    let mut children = Vec::new();
    if cursor.first_child() {
        loop {
            children.push(project_cursor(cursor));
            if !cursor.next_sibling() {
                break;
            }
        }
        assert!(cursor.parent());
    }
    ReferenceNode {
        name,
        from,
        to,
        children,
    }
}
