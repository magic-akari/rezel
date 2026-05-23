use std::collections::{BTreeSet, HashSet};
use std::path::Path;

use rezel_lang_go::ast::{AstNodeId, GoAst, GoAstValue};
use serde::{Deserialize, Serialize};

const SNAPSHOT_SCHEMA: &str = "rezel.go-parser-reference-snapshot.v1";
const SNAPSHOT: &str = include_str!("../../../../tools/references/go/snapshots/go.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Snapshot {
    schema: String,
    go_version: String,
    coordinates: String,
    parse_mode: Vec<String>,
    excluded_fields: Vec<String>,
    recovery_only_node_kinds: Vec<String>,
    out_of_scope_node_kinds: Vec<String>,
    node_kinds: Vec<String>,
    accepted: Vec<AcceptedCase>,
    rejected: Vec<RejectedCase>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AcceptedCase {
    id: String,
    source_name: String,
    source: String,
    ast: ReferenceNode,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RejectedCase {
    id: String,
    source_name: String,
    source: String,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ReferenceNode {
    kind: String,
    start: Option<usize>,
    end: Option<usize>,
    fields: Vec<ReferenceField>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ReferenceField {
    field: String,
    value: ReferenceValue,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
enum ReferenceValue {
    Node(Box<ReferenceNode>),
    List(Vec<ReferenceValue>),
    String(String),
    Bool(bool),
    Number(usize),
    Null,
}

#[test]
fn accepted_sources_match_go_parser_ast_projection() {
    let snapshot = reference_data();
    for case in &snapshot.accepted {
        let tree = rezel_lang_go::parser()
            .with_strict(true)
            .parse(&case.source)
            .unwrap_or_else(|error| {
                panic!("go/parser accepted {} but Rezel returned {error}", case.id)
            });
        let ast = GoAst::lower(&tree, &case.source).unwrap_or_else(|error| {
            panic!("could not lower go/parser-accepted {}: {error}", case.id)
        });
        let mut visited = vec![false; ast.nodes().len()];
        let actual = project_node(&ast, ast.root_id(), &mut visited);
        assert!(
            visited.into_iter().all(|visited| visited),
            "{} has AST nodes unreachable from its public root",
            case.id,
        );
        assert_projection(&case.id, &actual, &case.ast);
    }
}

#[test]
fn rejected_sources_match_go_parser_acceptance() {
    let snapshot = reference_data();
    for case in &snapshot.rejected {
        assert!(
            rezel_lang_go::parser()
                .with_strict(true)
                .parse(&case.source)
                .is_err(),
            "go/parser rejected {} ({}) but Rezel accepted it",
            case.id,
            case.source_name,
        );
    }
}

fn reference_data() -> Snapshot {
    let snapshot = serde_json::from_str::<Snapshot>(SNAPSHOT).expect("valid Go reference snapshot");
    assert_eq!(snapshot.schema, SNAPSHOT_SCHEMA);
    assert_eq!(snapshot.go_version, "go1.26.3");
    assert_eq!(snapshot.coordinates, "raw-utf8-bytes");
    assert_eq!(
        snapshot.parse_mode,
        ["AllErrors", "ParseComments", "SkipObjectResolution"]
    );
    assert_eq!(
        snapshot.excluded_fields,
        ["File.Scope", "File.Unresolved", "Ident.Obj"]
    );
    assert_eq!(
        snapshot.recovery_only_node_kinds,
        ["BadDecl", "BadExpr", "BadStmt"]
    );
    assert_eq!(snapshot.out_of_scope_node_kinds, ["Directive", "Package"]);

    let mut observed_node_kinds = BTreeSet::new();
    for case in &snapshot.accepted {
        collect_node_kinds(&case.ast, &mut observed_node_kinds);
    }
    let observed_node_kinds = observed_node_kinds.into_iter().collect::<Vec<_>>();
    assert_eq!(
        snapshot.node_kinds, observed_node_kinds,
        "nodeKinds must be sorted, unique, and exactly match the accepted projections"
    );
    assert!(!snapshot.accepted.is_empty());
    assert!(!snapshot.rejected.is_empty());

    let mut ids = HashSet::new();
    for (id, source_name, source) in snapshot
        .accepted
        .iter()
        .map(|case| (&case.id, &case.source_name, &case.source))
        .chain(
            snapshot
                .rejected
                .iter()
                .map(|case| (&case.id, &case.source_name, &case.source)),
        )
    {
        assert!(!id.is_empty());
        assert!(ids.insert(id), "duplicate Go reference case {id}");
        assert!(!source.is_empty());
        assert!(
            Path::new(source_name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("go"))
        );
    }
    snapshot
}

fn project_node(ast: &GoAst, id: AstNodeId, visited: &mut [bool]) -> ReferenceNode {
    *visited
        .get_mut(id.index())
        .expect("public AST edge addresses an arena node") = true;
    let node = ast.node(id).expect("public AST edge addresses a node");
    let range = node.source_range();
    let fields = ast
        .fields(id)
        .expect("public AST node has fields")
        .iter()
        .map(|field| ReferenceField {
            field: field.field().go_name().to_owned(),
            value: project_value(ast, field.value(), visited),
        })
        .collect();
    ReferenceNode {
        kind: node.kind().go_name().to_owned(),
        start: range.start().map(Into::into),
        end: range.end().map(Into::into),
        fields,
    }
}

fn collect_node_kinds(node: &ReferenceNode, kinds: &mut BTreeSet<String>) {
    kinds.insert(node.kind.clone());
    for field in &node.fields {
        collect_value_node_kinds(&field.value, kinds);
    }
}

fn collect_value_node_kinds(value: &ReferenceValue, kinds: &mut BTreeSet<String>) {
    match value {
        ReferenceValue::Node(node) => collect_node_kinds(node, kinds),
        ReferenceValue::List(values) => {
            for value in values {
                collect_value_node_kinds(value, kinds);
            }
        }
        ReferenceValue::String(_)
        | ReferenceValue::Bool(_)
        | ReferenceValue::Number(_)
        | ReferenceValue::Null => {}
    }
}

fn project_value(ast: &GoAst, value: GoAstValue, visited: &mut [bool]) -> ReferenceValue {
    match value {
        GoAstValue::Position(position) => position.map_or(ReferenceValue::Null, |position| {
            ReferenceValue::Number(position.into())
        }),
        GoAstValue::String(string) => ReferenceValue::String(
            ast.string(string)
                .expect("public AST string addresses an interned value")
                .to_owned(),
        ),
        GoAstValue::Token(token) => ReferenceValue::String(token.go_name().to_owned()),
        GoAstValue::Direction(direction) => ReferenceValue::String(direction.go_name().to_owned()),
        GoAstValue::Bool(value) => ReferenceValue::Bool(value),
        GoAstValue::Node(node) => node.map_or(ReferenceValue::Null, |node| {
            ReferenceValue::Node(Box::new(project_node(ast, node, visited)))
        }),
        GoAstValue::Nodes(nodes) => ReferenceValue::List(
            ast.node_list(nodes)
                .expect("public AST node list addresses nodes")
                .iter()
                .map(|node| ReferenceValue::Node(Box::new(project_node(ast, *node, visited))))
                .collect(),
        ),
        _ => panic!("unhandled future GoAstValue"),
    }
}

fn assert_projection(case: &str, actual: &ReferenceNode, expected: &ReferenceNode) {
    if actual == expected {
        return;
    }
    let actual = serde_json::to_value(actual).expect("serialize actual Go AST projection");
    let expected = serde_json::to_value(expected).expect("serialize expected Go AST projection");
    panic!(
        "{case}: owned AST differs from go/parser at {}",
        first_difference(&actual, &expected, "$")
    );
}

fn first_difference(
    actual: &serde_json::Value,
    expected: &serde_json::Value,
    path: &str,
) -> String {
    match (actual, expected) {
        (serde_json::Value::Array(actual), serde_json::Value::Array(expected)) => {
            if actual.len() != expected.len() {
                let actual_names = actual.iter().filter_map(sequence_label).collect::<Vec<_>>();
                let expected_names = expected
                    .iter()
                    .filter_map(sequence_label)
                    .collect::<Vec<_>>();
                return format!(
                    "{path}.length (actual {} {:?}, expected {} {:?})",
                    actual.len(),
                    actual_names,
                    expected.len(),
                    expected_names,
                );
            }
            for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
                if actual != expected {
                    let label = expected
                        .get("field")
                        .and_then(serde_json::Value::as_str)
                        .map_or_else(|| index.to_string(), ToOwned::to_owned);
                    return first_difference(actual, expected, &format!("{path}[{label}]"));
                }
            }
        }
        (serde_json::Value::Object(actual), serde_json::Value::Object(expected)) => {
            let path = expected
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .map_or_else(|| path.to_owned(), |kind| format!("{path}<{kind}>"));
            for (name, expected) in expected {
                let Some(actual) = actual.get(name) else {
                    return format!("{path}.{name} (missing)");
                };
                if actual != expected {
                    return first_difference(actual, expected, &format!("{path}.{name}"));
                }
            }
            if actual.len() != expected.len() {
                return format!(
                    "{path} field count (actual {}, expected {})",
                    actual.len(),
                    expected.len()
                );
            }
        }
        _ => {}
    }
    format!("{path} (actual {actual}, expected {expected})")
}

fn sequence_label(value: &serde_json::Value) -> Option<&str> {
    value
        .get("field")
        .or_else(|| value.get("kind"))
        .and_then(serde_json::Value::as_str)
}
