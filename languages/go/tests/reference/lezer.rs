use rezel_common::{IterMode, ParseError, Tree, TreeCursor};
use serde::Deserialize;

const CASE_SCHEMA: &str = "rezel.lezer-go-reference-cases.v1";
const SNAPSHOT_SCHEMA: &str = "rezel.lezer-go-reference-snapshot.v1";
const CASES: &str = include_str!("../../../../tools/references/lezer/cases/go.json");
const SNAPSHOT: &str = include_str!("../../../../tools/references/lezer/snapshots/go.json");

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
    coordinates: String,
    strict: Vec<ExpectedCase>,
    recovering: Vec<ExpectedCase>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceIdentity {
    packages: ReferencePackages,
    artifacts: ReferenceArtifacts,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceArtifacts {
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
struct ReferencePackages {
    #[serde(rename = "@lezer/common")]
    common: PackageIdentity,
    #[serde(rename = "@lezer/generator")]
    generator: PackageIdentity,
    #[serde(rename = "@lezer/lr")]
    lr: PackageIdentity,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageIdentity {
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
fn valid_go_csts_match_strict_and_recovering_lezer_parses() {
    let (cases, snapshot) = reference_data();
    assert_eq!(cases.strict.len(), snapshot.strict.len());
    for (case, expected) in cases.strict.iter().zip(&snapshot.strict) {
        assert_eq!(case.id, expected.id, "reference case order");
        let strict = parse_case(case, true)
            .unwrap_or_else(|error| panic!("strict reference case {} failed: {error}", case.id));
        let recovering = parse_case(case, false).unwrap_or_else(|error| {
            panic!("recovering reference case {} failed: {error}", case.id)
        });
        assert_eq!(
            project(&strict),
            project(&recovering),
            "reference case {} recovered despite being valid",
            case.id,
        );
        assert_eq!(
            project(&strict),
            expected.tree,
            "reference case {}",
            case.id,
        );
    }
}

#[test]
fn invalid_go_csts_match_deterministic_lezer_recovery() {
    let (cases, snapshot) = reference_data();
    assert_eq!(cases.recovering.len(), snapshot.recovering.len());
    for (case, expected) in cases.recovering.iter().zip(&snapshot.recovering) {
        assert_eq!(case.id, expected.id, "reference case order");
        assert!(
            parse_case(case, true).is_err(),
            "reference case {} unexpectedly passed strict parsing",
            case.id,
        );
        let first = parse_case(case, false)
            .unwrap_or_else(|error| panic!("recovery case {} failed: {error}", case.id));
        let second = parse_case(case, false)
            .unwrap_or_else(|error| panic!("second recovery case {} failed: {error}", case.id));
        assert_eq!(
            project(&first),
            project(&second),
            "reference case {} recovered non-deterministically",
            case.id,
        );
        assert_eq!(project(&first), expected.tree, "reference case {}", case.id);
    }
}

fn reference_data() -> (CaseManifest, Snapshot) {
    let cases = serde_json::from_str::<CaseManifest>(CASES).expect("valid reference cases");
    let snapshot = serde_json::from_str::<Snapshot>(SNAPSHOT).expect("valid reference snapshot");

    assert_eq!(cases.schema, CASE_SCHEMA);
    assert_eq!(snapshot.schema, SNAPSHOT_SCHEMA);
    assert_eq!(snapshot.coordinates, "raw-utf8-bytes");
    for package in [
        &snapshot.reference.packages.common,
        &snapshot.reference.packages.generator,
        &snapshot.reference.packages.lr,
    ] {
        assert!(!package.version.is_empty());
        assert!(package.integrity.starts_with("sha512-"));
    }
    assert_eq!(
        snapshot.reference.artifacts.grammar.path,
        "languages/go/grammar/go.grammar"
    );
    assert_eq!(snapshot.reference.artifacts.grammar.sha256.len(), 64);
    (cases, snapshot)
}

fn parse_case(case: &ReferenceCase, strict: bool) -> Result<Tree, ParseError> {
    rezel_lang_go::parser()
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
