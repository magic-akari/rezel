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

use rezel_lang_go::ast::{AstNodeId, GoAst, GoAstValue};
use serde::Deserialize;

const GO_VERSION: &str = "go1.26.3";
const GO_SOURCE_COUNT: usize = 6_494;
const GO_FINGERPRINT_SCHEMA: &str = "rezel.go-stdlib-ast-fingerprints.v1";
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const WORKER_STACK_SIZE: usize = 64 * 1024 * 1024;

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
            _ => Err(failure("usage: rezel-go-stdlib-reference [--ast-oracle]")),
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
        .name("rezel-go-stdlib-reference".to_owned())
        .stack_size(WORKER_STACK_SIZE)
        .spawn(move || verify_go(mode))?;
    worker
        .join()
        .map_err(|_| failure("Go standard-library verifier panicked"))?
}

fn verify_go(mode: VerificationMode) -> Result<()> {
    let source_root = go_source_root()?;
    let sources = collect_go_sources(&source_root)?;
    require_count("Go standard-library source", sources.len(), GO_SOURCE_COUNT)?;

    match mode {
        VerificationMode::Acceptance => verify_acceptance(&source_root, &sources),
        VerificationMode::AstOracle => verify_ast_oracle(&source_root, &sources),
    }
}

fn go_source_root() -> Result<PathBuf> {
    let mut command = Command::new("go");
    command
        .args(["env", "GOVERSION", "GOROOT"])
        .env("GOTOOLCHAIN", "local")
        .env("GOWORK", "off");
    let output = checked_output(&mut command, "resolve the pinned Go toolchain")?;
    let stdout = String::from_utf8(output.stdout)?;
    let mut lines = stdout.lines();
    let version = lines
        .next()
        .ok_or_else(|| failure("go env GOVERSION returned no version"))?;
    let root = lines
        .next()
        .filter(|line| !line.is_empty())
        .ok_or_else(|| failure("go env GOROOT returned no root"))?;
    if version != GO_VERSION {
        return Err(failure(format!(
            "{GO_VERSION} is required, found {version}"
        )));
    }
    if lines.next().is_some() {
        return Err(failure("go env returned unexpected extra output"));
    }
    Ok(Path::new(root).join("src"))
}

fn verify_acceptance(root: &Path, sources: &[PathBuf]) -> Result<()> {
    let parser = rezel_lang_go::parser().with_strict(true);
    let mut rejected = Vec::new();
    for path in sources {
        let source = fs::read_to_string(path)?;
        if let Err(error) = parser.parse(&source) {
            let relative = slash_path(path.strip_prefix(root)?);
            rejected.push(format!("{relative}: {error}"));
        }
    }
    if !rejected.is_empty() {
        return Err(failures(
            "strict Go parser rejected standard-library sources",
            &rejected,
        ));
    }

    eprintln!("accepted {GO_SOURCE_COUNT} Go {GO_VERSION} standard-library sources");
    Ok(())
}

fn verify_ast_oracle(root: &Path, sources: &[PathBuf]) -> Result<()> {
    let oracle = load_go_oracle(root, sources.len())?;
    let mut expected_sources = index_go_sources(oracle.sources)?;
    let parser = rezel_lang_go::parser().with_strict(true);
    let mut oracle_rejections = Vec::new();
    let mut rezel_rejections = Vec::new();
    let mut lowering_failures = Vec::new();
    let mut mismatches = Vec::new();

    for (index, path) in sources.iter().enumerate() {
        let relative = slash_path(path.strip_prefix(root)?);
        let expected = expected_sources
            .remove(&relative)
            .ok_or_else(|| failure(format!("go/parser AST fingerprints omit {relative}")))?;
        if !expected.accepted {
            oracle_rejections.push(format!(
                "{relative}: {} go/parser error(s)",
                expected.error_count
            ));
            continue;
        }
        let expected_fingerprint = expected.fingerprint.as_deref().ok_or_else(|| {
            failure(format!(
                "{relative}: accepted go/parser record has no fingerprint"
            ))
        })?;
        if expected.nodes == 0 {
            return Err(failure(format!(
                "{relative}: accepted go/parser record has no AST nodes"
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
        let ast = match GoAst::lower(&tree, &source) {
            Ok(ast) => ast,
            Err(error) => {
                lowering_failures.push(format!("{relative}: {error}"));
                continue;
            }
        };
        let actual = AstFingerprint::from_ast(&ast)?;
        let actual_fingerprint = actual.hexadecimal();
        if actual.nodes != expected.nodes || actual_fingerprint != expected_fingerprint {
            mismatches.push(format!(
                "{relative}: go/parser nodes={} fingerprint={expected_fingerprint}; \
                 Rezel nodes={} fingerprint={actual_fingerprint}",
                expected.nodes, actual.nodes
            ));
        }
        if (index + 1) % 1_000 == 0 {
            eprintln!("Rezel AST fingerprints: {}/{}", index + 1, sources.len());
        }
    }

    if let Some(path) = expected_sources.keys().next() {
        return Err(failure(format!(
            "go/parser AST fingerprints contain unknown source {path}"
        )));
    }
    if !oracle_rejections.is_empty() {
        return Err(failures(
            "go/parser rejected official Go standard-library sources",
            &oracle_rejections,
        ));
    }
    if !rezel_rejections.is_empty() {
        return Err(failures(
            "strict Go parser rejected go/parser-accepted standard-library sources",
            &rezel_rejections,
        ));
    }
    if !lowering_failures.is_empty() {
        return Err(failures(
            "Go AST lowering rejected go/parser-accepted standard-library sources",
            &lowering_failures,
        ));
    }
    if !mismatches.is_empty() {
        return Err(failures(
            "Rezel Go AST differs from go/parser over standard-library sources",
            &mismatches,
        ));
    }

    eprintln!(
        "matched {GO_SOURCE_COUNT} Go {GO_VERSION} standard-library AST fingerprints against go/parser"
    );
    Ok(())
}

fn load_go_oracle(root: &Path, source_count: usize) -> Result<GoOracle> {
    let output = TemporaryDirectory::new("rezel-go-ast-oracle")?;
    let oracle_path = output.path().join("fingerprints.json");
    run_go_oracle(root, &oracle_path)?;
    let oracle = serde_json::from_str::<GoOracle>(&fs::read_to_string(&oracle_path)?)?;
    if oracle.schema != GO_FINGERPRINT_SCHEMA {
        return Err(failure(format!(
            "unexpected go/parser AST fingerprint schema: {}",
            oracle.schema
        )));
    }
    if oracle.go_version != GO_VERSION {
        return Err(failure(format!(
            "go/parser AST fingerprints use {}, expected {GO_VERSION}",
            oracle.go_version
        )));
    }
    if oracle.coordinates != "raw-utf8-bytes" {
        return Err(failure(format!(
            "go/parser AST fingerprints use unsupported coordinates: {}",
            oracle.coordinates
        )));
    }
    if oracle.parse_mode != ["AllErrors", "ParseComments", "SkipObjectResolution"] {
        return Err(failure(format!(
            "go/parser AST fingerprints use unexpected parse mode: {:?}",
            oracle.parse_mode
        )));
    }
    if oracle.excluded_fields != ["File.Scope", "File.Unresolved", "Ident.Obj"] {
        return Err(failure(format!(
            "go/parser AST fingerprints use unexpected exclusions: {:?}",
            oracle.excluded_fields
        )));
    }
    require_count(
        "go/parser AST fingerprint",
        oracle.sources.len(),
        source_count,
    )?;
    Ok(oracle)
}

fn run_go_oracle(root: &Path, output: &Path) -> Result<()> {
    let reference = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| failure("Go reference package has no parent directory"))?;
    let status = Command::new("go")
        .arg("-C")
        .arg(reference)
        .args(["run", ".", "--stdlib-fingerprints"])
        .arg(root)
        .arg(output)
        .env("GOTOOLCHAIN", "local")
        .env("GOWORK", "off")
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(failure(format!(
            "go/parser AST fingerprint oracle exited with {status}"
        )))
    }
}

fn index_go_sources(sources: Vec<GoFingerprint>) -> Result<BTreeMap<String, GoFingerprint>> {
    let mut indexed = BTreeMap::new();
    for source in sources {
        let path = source.path.clone();
        if indexed.insert(path.clone(), source).is_some() {
            return Err(failure(format!(
                "go/parser AST fingerprints contain duplicate source {path}"
            )));
        }
    }
    Ok(indexed)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GoOracle {
    schema: String,
    go_version: String,
    coordinates: String,
    parse_mode: Vec<String>,
    excluded_fields: Vec<String>,
    sources: Vec<GoFingerprint>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GoFingerprint {
    path: String,
    accepted: bool,
    error_count: usize,
    nodes: usize,
    fingerprint: Option<String>,
}

struct AstFingerprint {
    state: u64,
    nodes: usize,
}

impl AstFingerprint {
    fn from_ast(ast: &GoAst) -> Result<Self> {
        let mut fingerprint = Self { state: 0, nodes: 0 };
        let mut visited = vec![false; ast.nodes().len()];
        fingerprint.add_node(ast, ast.root_id(), &mut visited)?;
        if visited.iter().any(|visited| !visited) {
            return Err(failure(
                "Go AST fingerprint found nodes unreachable from the public root",
            ));
        }
        Ok(fingerprint)
    }

    fn add_node(&mut self, ast: &GoAst, id: AstNodeId, visited: &mut [bool]) -> Result<()> {
        let node_visited = visited
            .get_mut(id.index())
            .ok_or_else(|| failure("Go AST field addresses a missing node"))?;
        *node_visited = true;
        let node = ast
            .node(id)
            .ok_or_else(|| failure("Go AST node identity is invalid"))?;
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or_else(|| failure("Go AST node count overflow"))?;
        self.add_string(node.kind().go_name())?;
        let range = node.source_range();
        self.add_optional_position(range.start());
        self.add_optional_position(range.end());

        let fields = ast
            .fields(id)
            .ok_or_else(|| failure("Go AST node fields are invalid"))?;
        self.add_usize(fields.len())?;
        for field in fields {
            self.add_string(field.field().go_name())?;
            self.add_value(ast, field.value(), visited)?;
        }
        Ok(())
    }

    fn add_value(&mut self, ast: &GoAst, value: GoAstValue, visited: &mut [bool]) -> Result<()> {
        match value {
            GoAstValue::Position(Some(position)) => {
                self.add_long(3);
                self.add_long(u64::from(u32::from(position)));
            }
            GoAstValue::Position(None) | GoAstValue::Node(None) => self.add_long(0),
            GoAstValue::String(value) => {
                self.add_long(1);
                let value = ast
                    .string(value)
                    .ok_or_else(|| failure("Go AST string field addresses a missing string"))?;
                self.add_string(value)?;
            }
            GoAstValue::Token(value) => {
                self.add_long(1);
                self.add_string(value.go_name())?;
            }
            GoAstValue::Direction(value) => {
                self.add_long(1);
                self.add_string(value.go_name())?;
            }
            GoAstValue::Bool(value) => {
                self.add_long(2);
                self.add_long(u64::from(value));
            }
            GoAstValue::Node(Some(node)) => {
                self.add_long(4);
                self.add_node(ast, node, visited)?;
            }
            GoAstValue::Nodes(nodes) => {
                self.add_long(5);
                let nodes = ast
                    .node_list(nodes)
                    .ok_or_else(|| failure("Go AST list field addresses a missing node list"))?;
                self.add_usize(nodes.len())?;
                for node in nodes {
                    self.add_long(4);
                    self.add_node(ast, *node, visited)?;
                }
            }
            _ => {
                return Err(failure(format!(
                    "Go AST fingerprint does not support value {value:?}"
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
            u64::try_from(value).map_err(|_| failure("Go AST fingerprint length overflow"))?;
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

fn collect_go_sources(root: &Path) -> Result<Vec<PathBuf>> {
    let mut pending = vec![root.to_owned()];
    let mut sources = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                if entry.file_name() != "testdata" {
                    pending.push(entry.path());
                }
            } else if file_type.is_file() && entry.path().extension() == Some(OsStr::new("go")) {
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
