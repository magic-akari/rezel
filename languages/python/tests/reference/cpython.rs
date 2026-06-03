use rezel_lang_python::ast::{
    AstNodeId, PythonAst, PythonAstOptions, PythonAstValue, PythonConstant,
};
use serde::Deserialize;
use serde_json::{Value, json};

const SNAPSHOT_SCHEMA: &str = "rezel.cpython-python-reference-snapshot.v1";
const SNAPSHOT: &str = include_str!("../../../../tools/references/cpython/snapshots/python.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    schema: String,
    python_version: String,
    unicode_version: String,
    coordinates: String,
    accepted: Vec<Case>,
    rejected: Vec<Case>,
    extensions: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    id: String,
    mode: String,
    source: String,
    #[serde(default)]
    type_comments: bool,
    #[serde(default)]
    lowering: bool,
    #[serde(default)]
    ast: Option<Value>,
}

#[test]
fn current_cpython_sources_match_strict_acceptance() {
    let snapshot = reference_data();
    for case in &snapshot.accepted {
        parser_for(case)
            .with_strict(true)
            .parse(&case.source)
            .unwrap_or_else(|error| {
                panic!("CPython accepted {} but Rezel returned {error}", case.id)
            });
    }
}

#[test]
fn current_cpython_rejections_remain_rejected() {
    let snapshot = reference_data();
    let mut accepted = Vec::new();
    for case in &snapshot.rejected {
        if parser_for(case)
            .with_strict(true)
            .parse(&case.source)
            .is_ok()
        {
            accepted.push(case.id.as_str());
        }
    }
    assert!(
        accepted.is_empty(),
        "CPython-rejected cases accepted by Rezel: {accepted:?}"
    );
}

#[test]
fn forward_cst_extensions_have_no_invented_cpython_projection() {
    let snapshot = reference_data();
    for case in &snapshot.extensions {
        let tree = parser_for(case)
            .with_strict(true)
            .parse(&case.source)
            .unwrap_or_else(|error| {
                panic!(
                    "maintained forward CST extension {} did not parse: {error}",
                    case.id
                )
            });
        assert!(
            matches!(
                PythonAst::lower(&tree, &case.source),
                Err(rezel_lang_python::ast::AstError::UnsupportedSyntax { .. })
            ),
            "forward CST extension {} acquired an unverified CPython projection",
            case.id
        );
    }
}

#[test]
fn implemented_ast_surface_matches_cpython_projection() {
    let snapshot = reference_data();
    let omitted = snapshot
        .accepted
        .iter()
        .filter(|case| !case.lowering)
        .map(|case| case.id.as_str())
        .collect::<Vec<_>>();
    assert!(
        omitted.is_empty(),
        "accepted CPython cases without exact AST lowering: {omitted:?}"
    );
    let mut mismatches = Vec::new();
    for case in &snapshot.accepted {
        let tree = parser_for(case)
            .with_strict(true)
            .parse(&case.source)
            .unwrap_or_else(|error| panic!("could not parse {} for lowering: {error}", case.id));
        let ast = PythonAst::lower_with_options(
            &tree,
            &case.source,
            PythonAstOptions {
                type_comments: case.type_comments,
            },
        )
        .unwrap_or_else(|error| panic!("could not lower {}: {error}", case.id));
        let mut visited = vec![false; ast.nodes().len()];
        let actual = project_node(&ast, ast.root_id(), &mut visited);
        assert!(visited.into_iter().all(|visited| visited));
        if actual != *case.ast.as_ref().expect("lowering case has CPython AST") {
            mismatches.push(case.id.as_str());
        }
    }
    assert!(
        mismatches.is_empty(),
        "CPython AST projection mismatches: {mismatches:?}"
    );
}

fn parser_for(case: &Case) -> rezel_lang_python::PythonParser {
    let top = match case.mode.as_str() {
        "exec" => "Module",
        "eval" => "Expression",
        "single" => "Interactive",
        "func_type" => "FunctionType",
        mode => panic!("unknown CPython mode {mode}"),
    };
    rezel_lang_python::parser().with_top(top).unwrap()
}

fn reference_data() -> Snapshot {
    let snapshot: Snapshot = serde_json::from_str(SNAPSHOT).expect("valid CPython snapshot");
    assert_eq!(snapshot.schema, SNAPSHOT_SCHEMA);
    assert_eq!(snapshot.python_version, "3.14.5");
    assert_eq!(snapshot.unicode_version, "16.0.0");
    assert_eq!(snapshot.coordinates, "raw-utf8-bytes");
    assert!(!snapshot.accepted.is_empty());
    assert!(!snapshot.rejected.is_empty());
    assert!(!snapshot.extensions.is_empty());
    snapshot
}

fn project_node(ast: &PythonAst, id: AstNodeId, visited: &mut [bool]) -> Value {
    visited[id.index()] = true;
    let node = ast.node(id).expect("valid AST node ID");
    let range = node.source_range();
    json!({
        "kind": node.kind().python_name(),
        "start": range.start().map(usize::from),
        "end": range.end().map(usize::from),
        "fields": node.fields().iter().map(|field| json!({
            "field": field.field().python_name(),
            "value": project_value(ast, field.value(), visited),
        })).collect::<Vec<_>>(),
    })
}

fn project_value(ast: &PythonAst, value: &PythonAstValue, visited: &mut [bool]) -> Value {
    match value {
        PythonAstValue::None => Value::Null,
        PythonAstValue::Bool(value) => Value::Bool(*value),
        PythonAstValue::Integer(id) => json!({"integer": ast.string(*id).unwrap()}),
        PythonAstValue::String(id) => Value::String(ast.string(*id).unwrap().to_owned()),
        PythonAstValue::Strings(ids) => Value::Array(
            ids.iter()
                .map(|id| Value::String(ast.string(*id).unwrap().to_owned()))
                .collect(),
        ),
        PythonAstValue::Node(id) => project_node(ast, *id, visited),
        PythonAstValue::Nodes(ids) => Value::Array(
            ids.iter()
                .map(|id| project_node(ast, *id, visited))
                .collect(),
        ),
        PythonAstValue::OptionalNodes(ids) => Value::Array(
            ids.iter()
                .map(|id| id.map_or(Value::Null, |id| project_node(ast, id, visited)))
                .collect(),
        ),
        PythonAstValue::Constant(constant) => match constant {
            PythonConstant::None => Value::Null,
            PythonConstant::Bool(value) => Value::Bool(*value),
            PythonConstant::Integer(id) => json!({"integer": ast.string(*id).unwrap()}),
            PythonConstant::Float(bits) => json!({"floatBits": bits}),
            PythonConstant::Complex { real, imaginary } => {
                json!({"complexBits": {"real": real, "imaginary": imaginary}})
            }
            PythonConstant::String(id) => {
                json!({"stringCodePoints": ast.python_string(*id).unwrap()})
            }
            PythonConstant::Bytes(id) => json!({"bytes": ast.bytes(*id).unwrap()}),
            PythonConstant::Ellipsis => json!({"ellipsis": true}),
            _ => panic!("unprojected Python constant"),
        },
        _ => panic!("unprojected Python AST value"),
    }
}
