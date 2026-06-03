#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    error::Error,
    ffi::{OsStr, OsString},
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Output},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use rezel_lang_python::ast::{AstNodeId, PythonAst, PythonAstValue, PythonConstant};
use serde::Deserialize;
use serde_json::{Value, json};

const PYTHON_VERSION: &str = "3.14.5";
const PYTHON_UNICODE_VERSION: &str = "16.0.0";
const PYTHON_SOURCE_COUNT: usize = 655;
const CPYTHON_FINGERPRINT_SCHEMA: &str = "rezel.cpython-stdlib-ast-fingerprints.v1";
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const WORKER_STACK_SIZE: usize = 64 * 1024 * 1024;

const PYTHON_KNOWN_REJECTIONS: &[(&str, usize)] = &[];

type AnyError = Box<dyn Error + Send + Sync>;
type Result<T> = std::result::Result<T, AnyError>;

#[derive(Clone, Copy)]
enum VerificationMode {
    Acceptance,
    AstOracle,
}

impl VerificationMode {
    fn parse(arguments: impl Iterator<Item = OsString>) -> Result<Self> {
        let arguments = arguments.collect::<Vec<_>>();
        match arguments.as_slice() {
            [] => Ok(Self::Acceptance),
            [argument] if argument == "--ast-oracle" => Ok(Self::AstOracle),
            _ => Err(failure(
                "usage: rezel-python-stdlib-reference [--ast-oracle]",
            )),
        }
    }
}

fn main() {
    if let Err(error) = entry() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn entry() -> Result<()> {
    let mode = VerificationMode::parse(std::env::args_os().skip(1))?;
    let worker = thread::Builder::new()
        .name("rezel-python-stdlib-reference".to_owned())
        .stack_size(WORKER_STACK_SIZE)
        .spawn(move || verify_python(mode))?;
    worker
        .join()
        .map_err(|_| failure("Python standard-library verifier panicked"))?
}

fn verify_python(mode: VerificationMode) -> Result<()> {
    let source_root = python_source_root()?;
    let sources = collect_python_sources(&source_root)?;
    require_count(
        "Python standard-library source",
        sources.len(),
        PYTHON_SOURCE_COUNT,
    )?;

    match mode {
        VerificationMode::Acceptance => verify_acceptance(&source_root, &sources),
        VerificationMode::AstOracle => verify_ast_oracle(&source_root, &sources),
    }
}

fn python_source_root() -> Result<PathBuf> {
    let mut command = Command::new("python");
    command.args([
        "-c",
        "import sys,sysconfig; print('.'.join(map(str, sys.version_info[:3]))); print(sysconfig.get_path('stdlib'))",
    ]);
    let output = checked_output(&mut command, "resolve the pinned Python toolchain")?;
    let stdout = String::from_utf8(output.stdout)?;
    let mut lines = stdout.lines();
    let version = lines
        .next()
        .ok_or_else(|| failure("Python returned no version"))?;
    let root = lines
        .next()
        .filter(|line| !line.is_empty())
        .ok_or_else(|| failure("Python returned no standard-library root"))?;
    if version != PYTHON_VERSION {
        return Err(failure(format!(
            "Python {PYTHON_VERSION} is required, found {version}"
        )));
    }
    if lines.next().is_some() {
        return Err(failure("Python returned unexpected extra output"));
    }
    Ok(PathBuf::from(root))
}

fn verify_acceptance(source_root: &Path, sources: &[PathBuf]) -> Result<()> {
    let mut rejected = Vec::new();
    let mut rejection_details = Vec::new();
    let mut lowering_failures = Vec::new();
    for path in sources {
        let source = fs::read_to_string(path)?;
        let relative = slash_path(path.strip_prefix(source_root)?);
        match rezel_lang_python::parser().with_strict(true).parse(&source) {
            Ok(tree) => {
                if let Err(error) = PythonAst::lower(&tree, &source) {
                    lowering_failures.push(format!("{relative}: {error}"));
                }
            }
            Err(error) => {
                let line = error
                    .position()
                    .map(|position| line_number(&source, usize::from(position)))
                    .transpose()?;
                rejection_details.push(format!("{relative}:{}: {error}", line.unwrap_or(0)));
                rejected.push((relative, line));
            }
        }
    }

    if !lowering_failures.is_empty() {
        return Err(failures(
            "Python AST lowering rejected accepted standard-library sources",
            &lowering_failures,
        ));
    }
    let expected = PYTHON_KNOWN_REJECTIONS
        .iter()
        .map(|(path, line)| ((*path).to_owned(), Some(*line)))
        .collect::<Vec<_>>();
    if rejected != expected {
        return Err(failure(format!(
            "strict Python parser rejection inventory changed\nexpected:\n{}\nactual:\n{}",
            format_rejections(&expected),
            rejection_details.join("\n")
        )));
    }

    let accepted = PYTHON_SOURCE_COUNT - rejected.len();
    eprintln!(
        "accepted and lowered {accepted} Python {PYTHON_VERSION} standard-library sources; {} known CST limitations remain",
        rejected.len()
    );
    Ok(())
}

fn verify_ast_oracle(root: &Path, sources: &[PathBuf]) -> Result<()> {
    let oracle = load_cpython_oracle(root, sources.len())?;
    let mut expected_sources = index_cpython_sources(oracle.sources)?;
    let parser = rezel_lang_python::parser().with_strict(true);
    let mut oracle_rejections = Vec::new();
    let mut rezel_rejections = Vec::new();
    let mut lowering_failures = Vec::new();
    let mut mismatches = Vec::new();

    for (index, path) in sources.iter().enumerate() {
        let relative = slash_path(path.strip_prefix(root)?);
        let expected = expected_sources
            .remove(&relative)
            .ok_or_else(|| failure(format!("CPython AST fingerprints omit {relative}")))?;
        if !expected.accepted {
            oracle_rejections.push(format_cpython_rejection(&relative, &expected));
            continue;
        }
        let expected_fingerprint = expected.fingerprint.as_deref().ok_or_else(|| {
            failure(format!(
                "{relative}: accepted CPython record has no fingerprint"
            ))
        })?;
        if expected.nodes == 0 {
            return Err(failure(format!(
                "{relative}: accepted CPython record has no AST nodes"
            )));
        }

        let source = fs::read_to_string(path)?;
        let tree = match parser.parse(&source) {
            Ok(tree) => tree,
            Err(error) => {
                let line = error
                    .position()
                    .map(|position| line_number(&source, usize::from(position)))
                    .transpose()?;
                let suffix = line.map_or_else(String::new, |line| format!(":{line}"));
                rezel_rejections.push(format!("{relative}{suffix}: {error}"));
                continue;
            }
        };
        let ast = match PythonAst::lower(&tree, &source) {
            Ok(ast) => ast,
            Err(error) => {
                lowering_failures.push(format!("{relative}: {error}"));
                continue;
            }
        };
        let actual = AstFingerprint::from_ast(&ast)?;
        let actual_fingerprint = actual.hexadecimal();
        if actual.nodes != expected.nodes || actual_fingerprint != expected_fingerprint {
            let diagnostic = if mismatches.len() < 20 {
                format!(
                    "\n  first structural difference: {}",
                    diagnose_ast_mismatch(path, &relative, &ast)?
                )
            } else {
                String::new()
            };
            mismatches.push(format!(
                "{relative}: CPython nodes={} fingerprint={expected_fingerprint}; \
                 Rezel nodes={} fingerprint={actual_fingerprint}{diagnostic}",
                expected.nodes, actual.nodes,
            ));
        }
        if (index + 1) % 100 == 0 {
            eprintln!("Rezel AST fingerprints: {}/{}", index + 1, sources.len());
        }
    }

    if let Some(path) = expected_sources.keys().next() {
        return Err(failure(format!(
            "CPython AST fingerprints contain unknown source {path}"
        )));
    }
    if !oracle_rejections.is_empty() {
        return Err(failures(
            "CPython rejected official Python standard-library sources",
            &oracle_rejections,
        ));
    }
    if !rezel_rejections.is_empty() {
        return Err(failures(
            "strict Python parser rejected CPython-accepted standard-library sources",
            &rezel_rejections,
        ));
    }
    if !lowering_failures.is_empty() {
        return Err(failures(
            "Python AST lowering rejected CPython-accepted standard-library sources",
            &lowering_failures,
        ));
    }
    if !mismatches.is_empty() {
        return Err(failures(
            "Rezel Python AST differs from CPython over standard-library sources",
            &mismatches,
        ));
    }

    eprintln!(
        "matched {PYTHON_SOURCE_COUNT} Python {PYTHON_VERSION} standard-library AST fingerprints against CPython"
    );
    Ok(())
}

fn load_cpython_oracle(root: &Path, source_count: usize) -> Result<CpythonOracle> {
    let output = TemporaryDirectory::new("rezel-cpython-ast-oracle")?;
    let oracle_path = output.path().join("fingerprints.json");
    run_cpython_oracle(root, &oracle_path)?;
    let oracle = serde_json::from_str::<CpythonOracle>(&fs::read_to_string(&oracle_path)?)?;
    if oracle.schema != CPYTHON_FINGERPRINT_SCHEMA {
        return Err(failure(format!(
            "unexpected CPython AST fingerprint schema: {}",
            oracle.schema
        )));
    }
    if oracle.python_version != PYTHON_VERSION {
        return Err(failure(format!(
            "CPython AST fingerprints use Python {}, expected {PYTHON_VERSION}",
            oracle.python_version
        )));
    }
    if oracle.unicode_version != PYTHON_UNICODE_VERSION {
        return Err(failure(format!(
            "CPython AST fingerprints use Unicode {}, expected {PYTHON_UNICODE_VERSION}",
            oracle.unicode_version
        )));
    }
    if oracle.coordinates != "raw-utf8-bytes" {
        return Err(failure(format!(
            "CPython AST fingerprints use unsupported coordinates: {}",
            oracle.coordinates
        )));
    }
    if oracle.mode != "exec" {
        return Err(failure(format!(
            "CPython AST fingerprints use unexpected parse mode: {}",
            oracle.mode
        )));
    }
    if oracle.type_comments {
        return Err(failure(
            "CPython AST fingerprints unexpectedly enable type comments",
        ));
    }
    require_count(
        "CPython AST fingerprint",
        oracle.sources.len(),
        source_count,
    )?;
    Ok(oracle)
}

fn run_cpython_oracle(root: &Path, output: &Path) -> Result<()> {
    let status = Command::new("python")
        .arg(cpython_reference()?)
        .arg("--stdlib-fingerprints")
        .arg(root)
        .arg(output)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(failure(format!(
            "CPython AST fingerprint oracle exited with {status}"
        )))
    }
}

fn cpython_reference() -> Result<PathBuf> {
    let reference = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| failure("CPython reference package has no parent directory"))?
        .join("reference.py");
    Ok(reference)
}

fn diagnose_ast_mismatch(path: &Path, relative: &str, ast: &PythonAst) -> Result<String> {
    let output = TemporaryDirectory::new("rezel-cpython-ast-diagnostic")?;
    let projection_path = output.path().join("projection.json");
    let status = Command::new("python")
        .arg(cpython_reference()?)
        .arg("--project-file")
        .arg(path)
        .arg(relative)
        .arg(&projection_path)
        .status()?;
    if !status.success() {
        return Err(failure(format!(
            "CPython AST projection oracle exited with {status}"
        )));
    }
    let expected = serde_json::from_str::<Value>(&fs::read_to_string(projection_path)?)?;
    let actual = project_ast(ast)?;
    first_json_difference(&expected, &actual, "$")
        .ok_or_else(|| failure("AST fingerprints differ but full projections match"))
}

fn project_ast(ast: &PythonAst) -> Result<Value> {
    let mut visited = vec![false; ast.nodes().len()];
    let projection = project_ast_node(ast, ast.root_id(), &mut visited)?;
    if visited.iter().any(|visited| !visited) {
        return Err(failure(
            "Python AST projection found nodes unreachable from the public root",
        ));
    }
    Ok(projection)
}

fn project_ast_node(ast: &PythonAst, id: AstNodeId, visited: &mut [bool]) -> Result<Value> {
    let was_visited = visited
        .get_mut(id.index())
        .ok_or_else(|| failure("Python AST field addresses a missing node"))?;
    if std::mem::replace(was_visited, true) {
        return Err(failure(
            "Python AST node is reachable more than once from the public root",
        ));
    }
    let node = ast
        .node(id)
        .ok_or_else(|| failure("Python AST node identity is invalid"))?;
    let range = node.source_range();
    let fields = node
        .fields()
        .iter()
        .map(|field| {
            Ok(json!({
                "field": field.field().python_name(),
                "value": project_ast_value(ast, field.value(), visited)?,
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({
        "kind": node.kind().python_name(),
        "start": range.start().map(usize::from),
        "end": range.end().map(usize::from),
        "fields": fields,
    }))
}

fn project_ast_value(
    ast: &PythonAst,
    value: &PythonAstValue,
    visited: &mut [bool],
) -> Result<Value> {
    let value = match value {
        PythonAstValue::None => Value::Null,
        PythonAstValue::Bool(value) => Value::Bool(*value),
        PythonAstValue::Integer(value) => json!({
            "integer": ast
                .string(*value)
                .ok_or_else(|| failure("Python AST field addresses a missing integer"))?,
        }),
        PythonAstValue::String(value) => Value::String(
            ast.string(*value)
                .ok_or_else(|| failure("Python AST field addresses a missing string"))?
                .to_owned(),
        ),
        PythonAstValue::Strings(values) => Value::Array(
            values
                .iter()
                .map(|value| {
                    ast.string(*value)
                        .map(|value| Value::String(value.to_owned()))
                        .ok_or_else(|| failure("Python AST string list addresses a missing string"))
                })
                .collect::<Result<Vec<_>>>()?,
        ),
        PythonAstValue::Constant(value) => project_python_constant(ast, value)?,
        PythonAstValue::Node(node) => project_ast_node(ast, *node, visited)?,
        PythonAstValue::Nodes(nodes) => Value::Array(
            nodes
                .iter()
                .map(|node| project_ast_node(ast, *node, visited))
                .collect::<Result<Vec<_>>>()?,
        ),
        PythonAstValue::OptionalNodes(nodes) => Value::Array(
            nodes
                .iter()
                .map(|node| {
                    node.map_or_else(
                        || Ok(Value::Null),
                        |node| project_ast_node(ast, node, visited),
                    )
                })
                .collect::<Result<Vec<_>>>()?,
        ),
        _ => {
            return Err(failure(format!(
                "Python AST projection does not support value {value:?}"
            )));
        }
    };
    Ok(value)
}

fn project_python_constant(ast: &PythonAst, value: &PythonConstant) -> Result<Value> {
    let value = match value {
        PythonConstant::None => Value::Null,
        PythonConstant::Bool(value) => Value::Bool(*value),
        PythonConstant::Integer(value) => json!({
            "integer": ast
                .string(*value)
                .ok_or_else(|| failure("Python AST constant addresses a missing integer"))?,
        }),
        PythonConstant::Float(value) => json!({"floatBits": value}),
        PythonConstant::Complex { real, imaginary } => {
            json!({"complexBits": {"real": real, "imaginary": imaginary}})
        }
        PythonConstant::String(value) => json!({
            "stringCodePoints": ast
                .python_string(*value)
                .ok_or_else(|| failure("Python AST constant addresses a missing string"))?,
        }),
        PythonConstant::Bytes(value) => json!({
            "bytes": ast
                .bytes(*value)
                .ok_or_else(|| failure("Python AST constant addresses missing bytes"))?,
        }),
        PythonConstant::Ellipsis => json!({"ellipsis": true}),
        _ => {
            return Err(failure(format!(
                "Python AST projection does not support constant {value:?}"
            )));
        }
    };
    Ok(value)
}

fn first_json_difference(expected: &Value, actual: &Value, path: &str) -> Option<String> {
    if expected == actual {
        return None;
    }
    if ast_node_kind(expected).is_some() && ast_node_kind(actual).is_some() {
        return first_ast_node_difference(expected, actual, path);
    }
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => {
            for (key, expected_value) in expected {
                let Some(actual_value) = actual.get(key) else {
                    return Some(format!("{path}.{key} is missing from Rezel"));
                };
                if let Some(difference) =
                    first_json_difference(expected_value, actual_value, &format!("{path}.{key}"))
                {
                    return Some(difference);
                }
            }
            actual
                .keys()
                .find(|key| !expected.contains_key(*key))
                .map(|key| format!("{path}.{key} is only present in Rezel"))
        }
        (Value::Array(expected), Value::Array(actual)) => {
            for (index, (expected_value, actual_value)) in expected.iter().zip(actual).enumerate() {
                if let Some(difference) =
                    first_json_difference(expected_value, actual_value, &format!("{path}[{index}]"))
                {
                    return Some(difference);
                }
            }
            Some(format!(
                "{path}.length: CPython {}; Rezel {}",
                expected.len(),
                actual.len()
            ))
        }
        _ => Some(format!(
            "{path}: CPython {}; Rezel {}",
            display_json_value(expected),
            display_json_value(actual)
        )),
    }
}

fn ast_node_kind(value: &Value) -> Option<&str> {
    let node = value.as_object()?;
    if node.len() != 4
        || !node.contains_key("start")
        || !node.contains_key("end")
        || !node.contains_key("fields")
    {
        return None;
    }
    node.get("kind")?.as_str()
}

fn first_ast_node_difference(expected: &Value, actual: &Value, path: &str) -> Option<String> {
    let expected_kind = ast_node_kind(expected)?;
    let actual_kind = ast_node_kind(actual)?;
    let node_path = format!("{path}.{expected_kind}");
    if expected_kind != actual_kind {
        return Some(format!(
            "{node_path}.kind: CPython {expected_kind:?}; Rezel {actual_kind:?}"
        ));
    }
    for property in ["start", "end"] {
        if let Some(difference) = first_json_difference(
            &expected[property],
            &actual[property],
            &format!("{node_path}.{property}"),
        ) {
            return Some(difference);
        }
    }

    let expected_fields = expected["fields"].as_array()?;
    let actual_fields = actual["fields"].as_array()?;
    for (index, (expected_field, actual_field)) in
        expected_fields.iter().zip(actual_fields).enumerate()
    {
        let expected_name = expected_field["field"].as_str()?;
        let actual_name = actual_field["field"].as_str()?;
        if expected_name != actual_name {
            return Some(format!(
                "{node_path}.fields[{index}]: CPython {expected_name:?}; Rezel {actual_name:?}"
            ));
        }
        if let Some(difference) = first_json_difference(
            &expected_field["value"],
            &actual_field["value"],
            &format!("{node_path}.{expected_name}"),
        ) {
            return Some(difference);
        }
    }
    Some(format!(
        "{node_path}.fields.length: CPython {}; Rezel {}",
        expected_fields.len(),
        actual_fields.len()
    ))
}

fn display_json_value(value: &Value) -> String {
    const LIMIT: usize = 160;
    let rendered = value.to_string();
    let mut characters = rendered.chars();
    let prefix = characters.by_ref().take(LIMIT).collect::<String>();
    if characters.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

fn index_cpython_sources(
    sources: Vec<CpythonFingerprint>,
) -> Result<BTreeMap<String, CpythonFingerprint>> {
    let mut indexed = BTreeMap::new();
    for source in sources {
        let path = source.path.clone();
        if indexed.insert(path.clone(), source).is_some() {
            return Err(failure(format!(
                "CPython AST fingerprints contain duplicate source {path}"
            )));
        }
    }
    Ok(indexed)
}

fn format_cpython_rejection(relative: &str, record: &CpythonFingerprint) -> String {
    let kind = record.error_kind.as_deref().unwrap_or("<unknown error>");
    match (record.error_line, record.error_offset) {
        (Some(line), Some(offset)) => format!("{relative}:{line}:{offset}: {kind}"),
        (Some(line), None) => format!("{relative}:{line}: {kind}"),
        _ => format!("{relative}: {kind}"),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CpythonOracle {
    schema: String,
    python_version: String,
    unicode_version: String,
    coordinates: String,
    mode: String,
    type_comments: bool,
    sources: Vec<CpythonFingerprint>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CpythonFingerprint {
    path: String,
    accepted: bool,
    error_kind: Option<String>,
    error_line: Option<usize>,
    error_offset: Option<usize>,
    nodes: usize,
    fingerprint: Option<String>,
}

struct AstFingerprint {
    state: u64,
    nodes: usize,
}

impl AstFingerprint {
    fn from_ast(ast: &PythonAst) -> Result<Self> {
        let mut fingerprint = Self { state: 0, nodes: 0 };
        let mut visited = vec![false; ast.nodes().len()];
        fingerprint.add_node(ast, ast.root_id(), &mut visited)?;
        if visited.iter().any(|visited| !visited) {
            return Err(failure(
                "Python AST fingerprint found nodes unreachable from the public root",
            ));
        }
        Ok(fingerprint)
    }

    fn add_node(&mut self, ast: &PythonAst, id: AstNodeId, visited: &mut [bool]) -> Result<()> {
        let was_visited = visited
            .get_mut(id.index())
            .ok_or_else(|| failure("Python AST field addresses a missing node"))?;
        if std::mem::replace(was_visited, true) {
            return Err(failure(
                "Python AST node is reachable more than once from the public root",
            ));
        }
        let node = ast
            .node(id)
            .ok_or_else(|| failure("Python AST node identity is invalid"))?;
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or_else(|| failure("Python AST node count overflow"))?;
        self.add_string(node.kind().python_name())?;
        let range = node.source_range();
        self.add_optional_position(range.start());
        self.add_optional_position(range.end());

        self.add_usize(node.fields().len())?;
        for field in node.fields() {
            self.add_string(field.field().python_name())?;
            self.add_value(ast, field.value(), visited)?;
        }
        Ok(())
    }

    fn add_value(
        &mut self,
        ast: &PythonAst,
        value: &PythonAstValue,
        visited: &mut [bool],
    ) -> Result<()> {
        match value {
            PythonAstValue::None => self.add_long(0),
            PythonAstValue::String(value) => {
                self.add_long(1);
                let value = ast
                    .string(*value)
                    .ok_or_else(|| failure("Python AST field addresses a missing string"))?;
                self.add_string(value)?;
            }
            PythonAstValue::Bool(value) => {
                self.add_long(2);
                self.add_long(u64::from(*value));
            }
            PythonAstValue::Integer(value) => {
                self.add_long(3);
                let value = ast
                    .string(*value)
                    .ok_or_else(|| failure("Python AST field addresses a missing integer"))?;
                self.add_string(value)?;
            }
            PythonAstValue::Constant(value) => {
                self.add_constant(ast, value)?;
            }
            PythonAstValue::Node(node) => {
                self.add_long(8);
                self.add_node(ast, *node, visited)?;
            }
            PythonAstValue::Strings(values) => {
                self.add_long(9);
                self.add_usize(values.len())?;
                for value in values {
                    self.add_long(1);
                    let value = ast.string(*value).ok_or_else(|| {
                        failure("Python AST string list addresses a missing string")
                    })?;
                    self.add_string(value)?;
                }
            }
            PythonAstValue::Nodes(nodes) => {
                self.add_long(9);
                self.add_usize(nodes.len())?;
                for node in nodes {
                    self.add_long(8);
                    self.add_node(ast, *node, visited)?;
                }
            }
            PythonAstValue::OptionalNodes(nodes) => {
                self.add_long(9);
                self.add_usize(nodes.len())?;
                for node in nodes {
                    if let Some(node) = node {
                        self.add_long(8);
                        self.add_node(ast, *node, visited)?;
                    } else {
                        self.add_long(0);
                    }
                }
            }
            _ => {
                return Err(failure(format!(
                    "Python AST fingerprint does not support value {value:?}"
                )));
            }
        }
        Ok(())
    }

    fn add_constant(&mut self, ast: &PythonAst, value: &PythonConstant) -> Result<()> {
        match value {
            PythonConstant::None => self.add_long(0),
            PythonConstant::Bool(value) => {
                self.add_long(2);
                self.add_long(u64::from(*value));
            }
            PythonConstant::Integer(value) => {
                self.add_long(3);
                let value = ast
                    .string(*value)
                    .ok_or_else(|| failure("Python AST constant addresses a missing integer"))?;
                self.add_string(value)?;
            }
            PythonConstant::Float(value) => {
                self.add_long(4);
                self.add_long(*value);
            }
            PythonConstant::Complex { real, imaginary } => {
                self.add_long(5);
                self.add_long(*real);
                self.add_long(*imaginary);
            }
            PythonConstant::Bytes(value) => {
                self.add_long(6);
                let value = ast
                    .bytes(*value)
                    .ok_or_else(|| failure("Python AST constant addresses missing bytes"))?;
                self.add_usize(value.len())?;
                for byte in value {
                    self.add_long(u64::from(*byte));
                }
            }
            PythonConstant::Ellipsis => self.add_long(7),
            PythonConstant::String(value) => {
                self.add_long(10);
                let value = ast
                    .python_string(*value)
                    .ok_or_else(|| failure("Python AST constant addresses a missing string"))?;
                self.add_usize(value.len())?;
                for code_point in value {
                    self.add_long(u64::from(*code_point));
                }
            }
            _ => {
                return Err(failure(format!(
                    "Python AST fingerprint does not support constant {value:?}"
                )));
            }
        }
        Ok(())
    }

    fn add_optional_position<T: Into<u32>>(&mut self, value: Option<T>) {
        match value {
            Some(value) => {
                self.add_long(1);
                self.add_long(u64::from(value.into()));
            }
            None => self.add_long(0),
        }
    }

    fn add_string(&mut self, value: &str) -> Result<()> {
        self.add_usize(value.len())?;
        for value_byte in value.bytes() {
            self.add_long(u64::from(value_byte));
        }
        Ok(())
    }

    fn add_usize(&mut self, value: usize) -> Result<()> {
        let value =
            u64::try_from(value).map_err(|_| failure("Python AST fingerprint length overflow"))?;
        self.add_long(value);
        Ok(())
    }

    fn add_long(&mut self, value: u64) {
        self.state = (self.state ^ value).wrapping_mul(FNV_PRIME);
    }

    fn hexadecimal(&self) -> String {
        format!("{:016x}", self.state)
    }
}

fn collect_python_sources(root: &Path) -> Result<Vec<PathBuf>> {
    let mut pending = vec![root.to_owned()];
    let mut sources = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                if !matches!(
                    entry.file_name().to_str(),
                    Some("site-packages" | "__pycache__")
                ) {
                    pending.push(entry.path());
                }
            } else if file_type.is_file() && entry.path().extension() == Some(OsStr::new("py")) {
                sources.push(entry.path());
            }
        }
    }
    sources.sort();
    Ok(sources)
}

fn checked_output(command: &mut Command, description: &str) -> Result<Output> {
    let output = command.output()?;
    if output.status.success() {
        return Ok(output);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(failure(format!(
        "failed to {description}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    )))
}

fn require_count(label: &str, actual: usize, expected: usize) -> Result<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(failure(format!(
            "{label} inventory changed: expected {expected}, found {actual}"
        )))
    }
}

fn line_number(source: &str, position: usize) -> Result<usize> {
    let position = position.min(source.len());
    let prefix = source
        .get(..position)
        .ok_or_else(|| failure("parser error position is not a UTF-8 boundary"))?;
    Ok(prefix.bytes().filter(|byte| *byte == b'\n').count() + 1)
}

fn format_rejections(rejections: &[(String, Option<usize>)]) -> String {
    rejections
        .iter()
        .map(|(path, line)| match line {
            Some(line) => format!("{path}:{line}"),
            None => format!("{path}:<no position>"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn failures(message: &str, failures: &[String]) -> AnyError {
    const LIMIT: usize = 20;
    let details = failures
        .iter()
        .take(LIMIT)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    let omitted = failures.len().saturating_sub(LIMIT);
    let suffix = if omitted == 0 {
        String::new()
    } else {
        format!("\n... and {omitted} more")
    };
    failure(format!(
        "{message} ({}):\n{details}{suffix}",
        failures.len()
    ))
}

fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn failure(message: impl Into<String>) -> AnyError {
    io::Error::other(message.into()).into()
}

struct TemporaryDirectory {
    path: PathBuf,
}

impl TemporaryDirectory {
    fn new(prefix: &str) -> Result<Self> {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = std::env::temp_dir();
        for attempt in 0..100_u8 {
            let path = root.join(format!(
                "{prefix}-{}-{timestamp}-{attempt}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        Err(failure("could not create a unique temporary directory"))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
