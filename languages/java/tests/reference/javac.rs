use std::collections::HashSet;
use std::path::Path;

use rezel_lang_java::ast::{AstNodeId, JavaAst, JavaAstProperty};
use serde::Deserialize;

const SNAPSHOT_SCHEMA: &str = "rezel.javac-java-reference-snapshot.v1";
const SNAPSHOT: &str = include_str!("../../../../tools/references/javac/snapshots/java.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Snapshot {
    schema: String,
    java_release: u32,
    java_runtime_version: String,
    coordinates: String,
    accepted: Vec<AcceptedCase>,
    rejected: Vec<RejectedCase>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AcceptedCase {
    id: String,
    source_name: String,
    source: String,
    tree: ReferenceNode,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RejectedCase {
    id: String,
    source_name: String,
    source: String,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct ReferenceNode {
    kind: String,
    start: Option<usize>,
    end: Option<usize>,
    name: Option<String>,
    modifiers: Vec<String>,
    properties: Vec<String>,
    children: Vec<ReferenceEdge>,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct ReferenceEdge {
    field: String,
    node: ReferenceNode,
}

#[test]
fn accepted_sources_match_the_javac_compiler_tree_projection() {
    let snapshot = reference_data();
    for case in &snapshot.accepted {
        let tree = rezel_lang_java::parser()
            .with_strict(true)
            .parse(&case.source)
            .unwrap_or_else(|error| {
                panic!("javac accepted {} but Rezel returned {error}", case.id)
            });
        let ast = JavaAst::lower_with_file_name(&tree, &case.source, &case.source_name)
            .unwrap_or_else(|error| panic!("could not lower javac-accepted {}: {error}", case.id));
        let mut visited = vec![false; ast.nodes().len()];
        let actual = project(&ast, ast.root_id(), &mut visited);
        assert!(
            visited.into_iter().all(|visited| visited),
            "{} has AST nodes unreachable from its public root",
            case.id,
        );
        assert_projection(&case.id, "$", &actual, &case.tree);
    }
}

#[test]
fn rejected_sources_match_javac_parse_acceptance() {
    let snapshot = reference_data();
    for case in &snapshot.rejected {
        assert!(
            rezel_lang_java::parser()
                .with_strict(true)
                .parse(&case.source)
                .is_err(),
            "javac rejected {} ({}) but Rezel accepted it",
            case.id,
            case.source_name,
        );
    }
}

fn reference_data() -> Snapshot {
    let snapshot = serde_json::from_str::<Snapshot>(SNAPSHOT).expect("valid javac snapshot");
    assert_eq!(snapshot.schema, SNAPSHOT_SCHEMA);
    assert_eq!(snapshot.java_release, 26);
    assert!(!snapshot.java_runtime_version.is_empty());
    assert_eq!(snapshot.coordinates, "raw-utf8-bytes");
    assert!(!snapshot.accepted.is_empty());
    assert!(!snapshot.rejected.is_empty());

    let mut ids = HashSet::new();
    for (id, source_name) in snapshot
        .accepted
        .iter()
        .map(|case| (&case.id, &case.source_name))
        .chain(
            snapshot
                .rejected
                .iter()
                .map(|case| (&case.id, &case.source_name)),
        )
    {
        assert!(!id.is_empty());
        assert!(ids.insert(id), "duplicate javac reference case {id}");
        assert!(
            Path::new(source_name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("java"))
        );
    }
    snapshot
}

fn project(ast: &JavaAst, id: AstNodeId, visited: &mut [bool]) -> ReferenceNode {
    assert!(
        !std::mem::replace(&mut visited[id.index()], true),
        "Java AST node is reachable more than once",
    );
    let node = ast.node(id).expect("public AST edge addresses a node");
    let range = node.source_range();
    let mut name = None;
    let mut modifiers = Vec::new();
    let mut properties = Vec::new();
    for property in ast.properties(id).expect("public AST node has properties") {
        match *property {
            JavaAstProperty::Name(value) => {
                name = Some(
                    ast.string(value)
                        .expect("public AST name addresses an interned string")
                        .to_owned(),
                );
            }
            JavaAstProperty::Modifier(value) => modifiers.push(value.javac_name().to_owned()),
            JavaAstProperty::ImportModule(value) => {
                properties.push(format!("module={value}"));
            }
            JavaAstProperty::ImportStatic(value)
            | JavaAstProperty::BlockStatic(value)
            | JavaAstProperty::RequiresStatic(value) => {
                properties.push(format!("static={value}"));
            }
            JavaAstProperty::CaseKind(value) => {
                properties.push(format!("caseKind={}", value.javac_name()));
            }
            JavaAstProperty::LambdaBodyKind(value) => {
                properties.push(format!("bodyKind={}", value.javac_name()));
            }
            JavaAstProperty::ReferenceMode(value) => {
                properties.push(format!("referenceMode={}", value.javac_name()));
            }
            JavaAstProperty::ModuleKind(value) => {
                properties.push(format!("moduleKind={}", value.javac_name()));
            }
            JavaAstProperty::RequiresTransitive(value) => {
                properties.push(format!("transitive={value}"));
            }
            JavaAstProperty::PrimitiveKind(value) => {
                properties.push(format!("primitiveKind={}", value.javac_name()));
            }
            _ => {}
        }
    }
    modifiers.sort_unstable();
    properties.sort_unstable();

    let children = ast
        .edges(id)
        .expect("public AST node has child edges")
        .iter()
        .map(|edge| ReferenceEdge {
            field: edge.field().javac_name().to_owned(),
            node: project(ast, edge.node(), visited),
        })
        .collect();
    ReferenceNode {
        kind: node.kind().javac_name().to_owned(),
        start: range.start().map(Into::into),
        end: range.end().map(Into::into),
        name,
        modifiers,
        properties,
        children,
    }
}

fn assert_projection(case: &str, path: &str, actual: &ReferenceNode, expected: &ReferenceNode) {
    assert_eq!(actual.kind, expected.kind, "{case} {path}: kind");
    assert_eq!(actual.start, expected.start, "{case} {path}: start");
    assert_eq!(actual.end, expected.end, "{case} {path}: end");
    assert_eq!(actual.name, expected.name, "{case} {path}: name");
    assert_eq!(
        actual.modifiers, expected.modifiers,
        "{case} {path}: modifiers"
    );
    assert_eq!(
        actual.properties, expected.properties,
        "{case} {path}: properties"
    );
    assert_eq!(
        actual.children.len(),
        expected.children.len(),
        "{case} {path}: child count"
    );
    for (index, (actual, expected)) in actual.children.iter().zip(&expected.children).enumerate() {
        assert_eq!(
            actual.field, expected.field,
            "{case} {path}: child field {index}"
        );
        let path = format!("{path}/{}[{index}]", expected.field);
        assert_projection(case, &path, &actual.node, &expected.node);
    }
}
