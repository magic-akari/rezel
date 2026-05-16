use rezel_common::SyntaxNode;
use rezel_lr::LRParser;
use serde::Deserialize;

const CASE_SCHEMA: &str = "rezel.lezer-json-reference-cases.v1";
const SNAPSHOT_SCHEMA: &str = "rezel.lezer-json-reference-snapshot.v1";
const CASES: &str = include_str!("../../../tools/references/lezer/cases/json.json");
const SNAPSHOT: &str = include_str!("../../../tools/references/lezer/snapshots/json.json");

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
    source: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    schema: String,
    reference: ReferenceIdentity,
    strict: Vec<ExpectedCase>,
    recovering: Vec<ExpectedCase>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceIdentity {
    name: String,
    version: String,
    integrity: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedCase {
    id: String,
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
fn strict_json_csts_match_the_pinned_lezer_reference() {
    let (cases, snapshot) = reference_data();
    compare_cases(
        &cases.strict,
        &snapshot.strict,
        &rezel_lang_json::parser().with_strict(true),
    );
}

#[test]
fn recovering_json_csts_match_the_pinned_lezer_reference() {
    let (cases, snapshot) = reference_data();
    compare_cases(
        &cases.recovering,
        &snapshot.recovering,
        &rezel_lang_json::parser(),
    );
}

fn reference_data() -> (CaseManifest, Snapshot) {
    let cases = serde_json::from_str::<CaseManifest>(CASES).expect("valid reference cases");
    let snapshot = serde_json::from_str::<Snapshot>(SNAPSHOT).expect("valid reference snapshot");

    assert_eq!(cases.schema, CASE_SCHEMA);
    assert_eq!(snapshot.schema, SNAPSHOT_SCHEMA);
    assert_eq!(snapshot.reference.name, "@lezer/json");
    assert_eq!(snapshot.reference.version, "1.0.3");
    assert!(snapshot.reference.integrity.starts_with("sha512-"));

    (cases, snapshot)
}

fn compare_cases(cases: &[ReferenceCase], expected: &[ExpectedCase], parser: &LRParser) {
    assert_eq!(cases.len(), expected.len(), "reference case inventory");
    for (case, expected) in cases.iter().zip(expected) {
        assert_eq!(case.id, expected.id, "reference case order");
        let tree = parser
            .parse(&case.source)
            .unwrap_or_else(|error| panic!("reference case {} failed: {error}", case.id));
        assert_eq!(
            project(&tree.top_node()),
            expected.tree,
            "reference case {}",
            case.id,
        );
    }
}

fn project(node: &SyntaxNode) -> ReferenceNode {
    ReferenceNode {
        name: node.name().to_string(),
        from: usize::from(node.from()),
        to: usize::from(node.to()),
        children: node.children().map(|child| project(&child)).collect(),
    }
}
