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

use rezel_lang_java::{
    JavaParser,
    ast::{AstNodeId, JavaAst, JavaAstProperty},
};
use serde::Deserialize;

const JAVA_RUNTIME_VERSION: &str = "26.0.1+8-34";
const JAVA_SOURCE_COUNT: usize = 15_412;
const JAVAC_FINGERPRINT_SCHEMA: &str = "rezel.javac-stdlib-ast-fingerprints.v1";
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const WORKER_STACK_SIZE: usize = 64 * 1024 * 1024;

const JAVA_KNOWN_REJECTIONS: &[(&str, usize)] = &[
    ("java.base/java/lang/CharacterData00.java", 1_594),
    ("java.base/java/util/LocaleISOData.java", 201),
    (
        "java.base/jdk/internal/util/regex/IndicConjunctBreak.java",
        524,
    ),
    ("java.base/sun/nio/cs/GB18030.java", 318),
    (
        "java.base/sun/util/locale/provider/CollationRules.java",
        253,
    ),
    ("java.desktop/sun/font/X11GB18030_0.java", 246),
    ("java.desktop/sun/font/X11GB18030_1.java", 245),
    ("java.desktop/sun/font/X11Johab.java", 245),
    (
        "java.xml/com/sun/org/apache/xalan/internal/xsltc/compiler/XPathParser.java",
        319,
    ),
    ("jdk.charsets/sun/nio/cs/ext/EUC_TWMapping.java", 221),
    ("jdk.charsets/sun/nio/cs/ext/IBM33722.java", 432),
    ("jdk.charsets/sun/nio/cs/ext/IBM964.java", 438),
    (
        "jdk.localedata/sun/text/resources/ext/CollationData_ja.java",
        264,
    ),
    (
        "jdk.localedata/sun/text/resources/ext/CollationData_ko.java",
        233,
    ),
    (
        "jdk.localedata/sun/text/resources/ext/CollationData_zh.java",
        234,
    ),
    (
        "jdk.localedata/sun/text/resources/ext/CollationData_zh_TW.java",
        234,
    ),
];

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
            _ => Err(failure("usage: rezel-java-stdlib-reference [--ast-oracle]")),
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
        .name("rezel-java-stdlib-reference".to_owned())
        .stack_size(WORKER_STACK_SIZE)
        .spawn(move || verify_java(mode))?;
    worker
        .join()
        .map_err(|_| failure("Java standard-library verifier panicked"))?
}

fn verify_java(mode: VerificationMode) -> Result<()> {
    let properties = java_properties()?;
    let runtime_version = property(&properties, "java.runtime.version")?;
    if runtime_version != JAVA_RUNTIME_VERSION {
        return Err(failure(format!(
            "Java runtime {JAVA_RUNTIME_VERSION} is required, found {runtime_version}"
        )));
    }
    let java_home = property(&properties, "java.home")?;
    let source_archive = Path::new(java_home).join("lib").join("src.zip");
    if !source_archive.is_file() {
        return Err(failure(format!(
            "JDK source archive is missing: {}",
            source_archive.display()
        )));
    }

    let extracted = TemporaryDirectory::new("rezel-jdk-source")?;
    let mut command = Command::new("jar");
    command
        .args(["--extract", "--file"])
        .arg(&source_archive)
        .current_dir(extracted.path());
    checked_output(&mut command, "extract the pinned JDK source archive")?;

    let sources = collect_java_sources(extracted.path())?;
    require_count("JDK source", sources.len(), JAVA_SOURCE_COUNT)?;

    match mode {
        VerificationMode::Acceptance => verify_acceptance(extracted.path(), &sources),
        VerificationMode::AstOracle => verify_ast_oracle(extracted.path(), &sources),
    }
}

fn verify_acceptance(root: &Path, sources: &[PathBuf]) -> Result<()> {
    let parser = rezel_lang_java::parser().with_strict(true);
    let mut rejected = Vec::new();
    let mut lowering_failures = Vec::new();
    for path in sources {
        let source = fs::read_to_string(path)?;
        let relative_path = path.strip_prefix(root)?;
        let relative = slash_path(relative_path);
        match parser.parse(&source) {
            Ok(tree) => {
                let file_name = path
                    .file_name()
                    .and_then(OsStr::to_str)
                    .ok_or_else(|| failure(format!("non-UTF-8 Java path: {relative}")))?;
                if let Err(error) = JavaAst::lower_with_file_name(&tree, &source, file_name) {
                    lowering_failures.push(format!("{relative}: {error}"));
                }
            }
            Err(error) => {
                let line = error
                    .position()
                    .map(|position| line_number(&source, usize::from(position)))
                    .transpose()?;
                rejected.push((relative, line));
            }
        }
    }

    if !lowering_failures.is_empty() {
        return Err(failures(
            "Java AST lowering rejected accepted JDK sources",
            &lowering_failures,
        ));
    }
    verify_rejection_inventory(&rejected)?;

    let accepted = JAVA_SOURCE_COUNT - rejected.len();
    eprintln!(
        "accepted and lowered {accepted} JDK {JAVA_RUNTIME_VERSION} sources; {} known CST limitations remain",
        rejected.len()
    );
    Ok(())
}

fn verify_ast_oracle(root: &Path, sources: &[PathBuf]) -> Result<()> {
    let oracle = load_javac_oracle(root, sources.len())?;
    let mut expected_sources = index_javac_sources(oracle.sources)?;

    let parser = rezel_lang_java::parser().with_strict(true);
    let mut rejected = Vec::new();
    let mut oracle_rejections = Vec::new();
    let mut lowering_failures = Vec::new();
    let mut mismatches = Vec::new();
    for (index, path) in sources.iter().enumerate() {
        let relative_path = path.strip_prefix(root)?;
        let relative = slash_path(relative_path);
        let expected = expected_sources
            .remove(&relative)
            .ok_or_else(|| failure(format!("javac AST fingerprints omit {relative}")))?;
        match compare_ast_source(&parser, path, &relative, &expected)? {
            AstSourceResult::Matched => {}
            AstSourceResult::RezelRejected(line) => rejected.push((relative, line)),
            AstSourceResult::JavacRejected(message) => oracle_rejections.push(message),
            AstSourceResult::LoweringFailed(message) => lowering_failures.push(message),
            AstSourceResult::Mismatched(message) => mismatches.push(message),
        }
        if (index + 1) % 1_000 == 0 {
            eprintln!("Rezel AST fingerprints: {}/{}", index + 1, sources.len());
        }
    }
    if let Some(path) = expected_sources.keys().next() {
        return Err(failure(format!(
            "javac AST fingerprints contain unknown source {path}"
        )));
    }

    if !oracle_rejections.is_empty() {
        return Err(failures(
            "javac rejected official JDK sources",
            &oracle_rejections,
        ));
    }
    if !lowering_failures.is_empty() {
        return Err(failures(
            "Java AST lowering rejected javac-accepted JDK sources",
            &lowering_failures,
        ));
    }
    if !mismatches.is_empty() {
        return Err(failures(
            "Rezel Java AST differs from javac over JDK sources",
            &mismatches,
        ));
    }
    verify_rejection_inventory(&rejected)?;

    let compared = JAVA_SOURCE_COUNT - rejected.len();
    eprintln!(
        "matched {compared} JDK {JAVA_RUNTIME_VERSION} AST fingerprints against javac; \
         {} known CST limitations remain",
        rejected.len()
    );
    Ok(())
}

fn index_javac_sources(
    sources: Vec<JavacFingerprint>,
) -> Result<BTreeMap<String, JavacFingerprint>> {
    let mut indexed = BTreeMap::new();
    for source in sources {
        let path = source.path.clone();
        if indexed.insert(path.clone(), source).is_some() {
            return Err(failure(format!(
                "javac AST fingerprints contain duplicate source {path}"
            )));
        }
    }
    Ok(indexed)
}

enum AstSourceResult {
    Matched,
    RezelRejected(Option<usize>),
    JavacRejected(String),
    LoweringFailed(String),
    Mismatched(String),
}

fn compare_ast_source(
    parser: &JavaParser,
    path: &Path,
    relative: &str,
    expected: &JavacFingerprint,
) -> Result<AstSourceResult> {
    if !expected.accepted {
        return Ok(AstSourceResult::JavacRejected(format!(
            "{relative}: {} javac parse error(s)",
            expected.error_count
        )));
    }
    let expected_fingerprint = expected.fingerprint.as_deref().ok_or_else(|| {
        failure(format!(
            "{relative}: accepted javac record has no fingerprint"
        ))
    })?;
    if expected.nodes == 0 {
        return Err(failure(format!(
            "{relative}: accepted javac record has no AST nodes"
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
            return Ok(AstSourceResult::RezelRejected(line));
        }
    };
    let file_name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| failure(format!("non-UTF-8 Java path: {relative}")))?;
    let ast = match JavaAst::lower_with_file_name(&tree, &source, file_name) {
        Ok(ast) => ast,
        Err(error) => {
            return Ok(AstSourceResult::LoweringFailed(format!(
                "{relative}: {error}"
            )));
        }
    };
    let actual = AstFingerprint::from_ast(&ast)?;
    let actual_fingerprint = actual.hexadecimal();
    if actual.nodes == expected.nodes && actual_fingerprint == expected_fingerprint {
        return Ok(AstSourceResult::Matched);
    }
    Ok(AstSourceResult::Mismatched(format!(
        "{relative}: javac nodes={} fingerprint={expected_fingerprint}; \
         Rezel nodes={} fingerprint={actual_fingerprint}",
        expected.nodes, actual.nodes
    )))
}

fn load_javac_oracle(root: &Path, source_count: usize) -> Result<JavacOracle> {
    let output = TemporaryDirectory::new("rezel-javac-ast-oracle")?;
    let oracle_path = output.path().join("fingerprints.json");
    run_javac_oracle(root, &oracle_path)?;
    let oracle = serde_json::from_str::<JavacOracle>(&fs::read_to_string(&oracle_path)?)?;
    if oracle.schema != JAVAC_FINGERPRINT_SCHEMA {
        return Err(failure(format!(
            "unexpected javac AST fingerprint schema: {}",
            oracle.schema
        )));
    }
    if oracle.java_runtime_version != JAVA_RUNTIME_VERSION {
        return Err(failure(format!(
            "javac AST fingerprints use runtime {}, expected {JAVA_RUNTIME_VERSION}",
            oracle.java_runtime_version
        )));
    }
    if oracle.coordinates != "raw-utf8-bytes" {
        return Err(failure(format!(
            "javac AST fingerprints use unsupported coordinates: {}",
            oracle.coordinates
        )));
    }
    require_count("javac AST fingerprint", oracle.sources.len(), source_count)?;
    Ok(oracle)
}

fn run_javac_oracle(root: &Path, output: &Path) -> Result<()> {
    let reference = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("java")
        .join("JavacReference.java");
    let status = Command::new("java")
        .arg(reference)
        .arg("--stdlib-fingerprints")
        .arg(root)
        .arg(output)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(failure(format!(
            "javac AST fingerprint oracle exited with {status}"
        )))
    }
}

fn verify_rejection_inventory(rejected: &[(String, Option<usize>)]) -> Result<()> {
    let expected = JAVA_KNOWN_REJECTIONS
        .iter()
        .map(|(path, line)| ((*path).to_owned(), Some(*line)))
        .collect::<Vec<_>>();
    if rejected == expected {
        return Ok(());
    }
    let expected = format_rejections(&expected);
    let actual = format_rejections(rejected);
    Err(failure(format!(
        "strict Java parser rejection inventory changed\nexpected:\n{expected}\nactual:\n{actual}"
    )))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JavacOracle {
    schema: String,
    java_runtime_version: String,
    coordinates: String,
    sources: Vec<JavacFingerprint>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JavacFingerprint {
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
    fn from_ast(ast: &JavaAst) -> Result<Self> {
        let mut fingerprint = Self { state: 0, nodes: 0 };
        let mut visited = vec![false; ast.nodes().len()];
        fingerprint.add_node(ast, ast.root_id(), &mut visited)?;
        if visited.iter().any(|visited| !visited) {
            return Err(failure(
                "Java AST fingerprint found nodes unreachable from the public root",
            ));
        }
        Ok(fingerprint)
    }

    fn add_node(&mut self, ast: &JavaAst, id: AstNodeId, visited: &mut [bool]) -> Result<()> {
        let was_visited = visited
            .get_mut(id.index())
            .ok_or_else(|| failure("Java AST edge addresses a missing node"))?;
        if std::mem::replace(was_visited, true) {
            return Err(failure(
                "Java AST node is reachable more than once from the public root",
            ));
        }
        let node = ast
            .node(id)
            .ok_or_else(|| failure("Java AST node identity is invalid"))?;
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or_else(|| failure("Java AST node count overflow"))?;
        self.add_string(node.kind().javac_name())?;
        let range = node.source_range();
        self.add_optional_position(range.start());
        self.add_optional_position(range.end());

        let mut name = None;
        let mut modifiers = Vec::new();
        let mut properties = Vec::new();
        let node_properties = ast
            .properties(id)
            .ok_or_else(|| failure("Java AST node properties are invalid"))?;
        for property in node_properties {
            match *property {
                JavaAstProperty::Name(value) => {
                    if name.is_some() {
                        return Err(failure("Java AST node has more than one public name"));
                    }
                    name = Some(
                        ast.string(value)
                            .ok_or_else(|| failure("Java AST name addresses a missing string"))?,
                    );
                }
                JavaAstProperty::NameRange(_) => {}
                JavaAstProperty::Modifier(value) => {
                    modifiers.push(value.javac_name());
                }
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
                _ => {
                    return Err(failure(format!(
                        "Java AST fingerprint does not support property {property:?}"
                    )));
                }
            }
        }
        modifiers.sort_unstable();
        properties.sort_unstable();
        self.add_optional_string(name)?;
        self.add_strings(&modifiers)?;
        self.add_strings(&properties)?;

        let edges = ast
            .edges(id)
            .ok_or_else(|| failure("Java AST node edges are invalid"))?;
        self.add_usize(edges.len())?;
        for edge in edges {
            self.add_string(edge.field().javac_name())?;
            self.add_node(ast, edge.node(), visited)?;
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

    fn add_optional_string(&mut self, value: Option<&str>) -> Result<()> {
        if let Some(value) = value {
            self.add_long(1);
            self.add_string(value)
        } else {
            self.add_long(0);
            Ok(())
        }
    }

    fn add_strings<T: AsRef<str>>(&mut self, values: &[T]) -> Result<()> {
        self.add_usize(values.len())?;
        for value in values {
            self.add_string(value.as_ref())?;
        }
        Ok(())
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
            u64::try_from(value).map_err(|_| failure("Java AST fingerprint length overflow"))?;
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

fn java_properties() -> Result<String> {
    let mut command = Command::new("java");
    command.args(["-XshowSettings:properties", "-version"]);
    let output = checked_output(&mut command, "resolve the pinned Java toolchain")?;
    Ok(String::from_utf8(output.stderr)?)
}

fn property<'a>(properties: &'a str, name: &str) -> Result<&'a str> {
    properties
        .lines()
        .filter_map(|line| line.trim().split_once('='))
        .find_map(|(key, value)| (key.trim() == name).then(|| value.trim()))
        .ok_or_else(|| failure(format!("Java properties omit {name}")))
}

fn collect_java_sources(root: &Path) -> Result<Vec<PathBuf>> {
    let mut pending = vec![root.to_owned()];
    let mut sources = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() && entry.path().extension() == Some(OsStr::new("java")) {
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
