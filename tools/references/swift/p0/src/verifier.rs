#![forbid(unsafe_code)]

use std::{
    collections::BTreeSet,
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    thread,
};

const SWIFT_COMMIT: &str = "064859e41d68596f486c5d724401cb370f260409";
const SWIFT_SYNTAX_COMMIT: &str = "60e8eb850721b5a6eebbd973b39f450a16553bd9";
const WORKER_STACK_SIZE: usize = 64 * 1024 * 1024;
const FAILURE_LIMIT: usize = 100;

type AnyError = Box<dyn Error + Send + Sync>;
type Result<T> = std::result::Result<T, AnyError>;

struct SourceCorpus {
    label: &'static str,
    root: PathBuf,
    expected_files: usize,
    expected_bytes: u64,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Expectation {
    Strict,
    Recovery,
}

struct ParserCase {
    filename: String,
    expectation: Expectation,
    origin: String,
    bytes: usize,
}

#[derive(Default)]
struct FailureLog {
    details: Vec<String>,
    total: usize,
}

impl FailureLog {
    fn push(&mut self, detail: String) {
        self.total += 1;
        if self.details.len() < FAILURE_LIMIT {
            self.details.push(detail);
        }
    }

    fn into_result(self) -> Result<()> {
        if self.total == 0 {
            return Ok(());
        }
        let omitted = self.total.saturating_sub(self.details.len());
        let suffix = if omitted == 0 {
            String::new()
        } else {
            format!("\n... {omitted} additional failures omitted")
        };
        Err(failure(format!(
            "Swift P0 corpus verification failed ({} failures)\n{}{suffix}",
            self.total,
            self.details.join("\n")
        )))
    }
}

pub(crate) fn run() {
    if let Err(error) = entry() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn entry() -> Result<()> {
    let worker = thread::Builder::new()
        .name("rezel-swift-p0-reference".to_owned())
        .stack_size(WORKER_STACK_SIZE)
        .spawn(verify_p0)?;
    worker
        .join()
        .map_err(|_| failure("Swift P0 corpus verifier panicked"))?
}

fn verify_p0() -> Result<()> {
    let repository = repository_root()?;
    let source_root = repository.join("target/reference-sources/swift");
    let swift_root = source_root.join(format!("swift-{SWIFT_COMMIT}"));
    let swift_syntax_root = source_root.join(format!("swift-syntax-{SWIFT_SYNTAX_COMMIT}"));
    let case_root = source_root.join(format!("swift-syntax-cases-{SWIFT_SYNTAX_COMMIT}"));

    let corpora = [
        SourceCorpus {
            label: "Swift stdlib/public",
            root: swift_root.join("stdlib/public"),
            expected_files: 399,
            expected_bytes: 6_080_507,
        },
        SourceCorpus {
            label: "SwiftSyntax Sources",
            root: swift_syntax_root.join("Sources"),
            expected_files: 318,
            expected_bytes: 6_673_159,
        },
    ];

    let strict_parser = rezel_lang_swift::parser().with_strict(true);
    let recovery_parser = rezel_lang_swift::parser();
    let mut failures = FailureLog::default();
    let mut accepted_files = 0_usize;
    let mut accepted_bytes = 0_u64;

    for corpus in &corpora {
        let paths = collect_swift_files(&corpus.root)?;
        require_inventory(corpus.label, paths.len(), corpus.expected_files, "files")?;
        let mut corpus_bytes = 0_u64;
        for path in &paths {
            let source = fs::read_to_string(path)?;
            let source_bytes = u64::try_from(source.len())?;
            corpus_bytes = checked_add(corpus_bytes, source_bytes, corpus.label)?;
            if let Err(error) = strict_parser.parse(&source) {
                failures.push(format_parse_failure(
                    corpus.label,
                    &slash_path(path.strip_prefix(&corpus.root)?),
                    &source,
                    &error,
                )?);
            }
        }
        require_inventory(
            corpus.label,
            corpus_bytes,
            corpus.expected_bytes,
            "UTF-8 bytes",
        )?;
        accepted_files += paths.len();
        accepted_bytes = checked_add(accepted_bytes, corpus_bytes, "accepted source corpora")?;
    }

    let cases = read_case_manifest(&case_root)?;
    require_inventory("SwiftSyntax parser case", cases.len(), 3_301, "cases")?;
    let strict_cases = cases
        .iter()
        .filter(|case| case.expectation == Expectation::Strict)
        .count();
    require_inventory(
        "SwiftSyntax strict parser case",
        strict_cases,
        2_056,
        "cases",
    )?;

    let mut case_bytes = 0_u64;
    for case in &cases {
        let path = case_root.join(&case.filename);
        let source = fs::read_to_string(&path)?;
        require_inventory(
            &case.origin,
            source.len(),
            case.bytes,
            "materialized UTF-8 bytes",
        )?;
        case_bytes = checked_add(case_bytes, u64::try_from(source.len())?, "parser cases")?;

        if let Err(error) = recovery_parser.parse(&source) {
            failures.push(format_parse_failure(
                "SwiftSyntax recovery",
                &case.origin,
                &source,
                &error,
            )?);
            continue;
        }
        if case.expectation == Expectation::Strict
            && let Err(error) = strict_parser.parse(&source)
        {
            failures.push(format_parse_failure(
                "SwiftSyntax strict",
                &case.origin,
                &source,
                &error,
            )?);
        }
    }
    require_inventory(
        "SwiftSyntax parser case",
        case_bytes,
        186_014,
        "UTF-8 bytes",
    )?;

    failures.into_result()?;
    eprintln!(
        "accepted {accepted_files} pinned Swift source files ({accepted_bytes} UTF-8 bytes); \
         recovered {} SwiftSyntax parser cases ({strict_cases} also strict, {case_bytes} UTF-8 bytes)",
        cases.len()
    );
    Ok(())
}

fn repository_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .map(Path::to_owned)
        .ok_or_else(|| failure("could not resolve the repository root"))
}

fn collect_swift_files(root: &Path) -> Result<Vec<PathBuf>> {
    if !root.is_dir() {
        return Err(failure(format!(
            "Swift P0 corpus is missing: {}\nrun `mise run reference:swift:p0:prepare`",
            root.display()
        )));
    }
    let mut paths = Vec::new();
    collect_swift_files_in(root, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn collect_swift_files_in(directory: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<io::Result<Vec<_>>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_swift_files_in(&path, paths)?;
        } else if file_type.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "swift")
        {
            paths.push(path);
        }
    }
    Ok(())
}

fn read_case_manifest(root: &Path) -> Result<Vec<ParserCase>> {
    let manifest = root.join("manifest.tsv");
    let source = fs::read_to_string(&manifest).map_err(|error| {
        failure(format!(
            "SwiftSyntax parser cases are missing: {}\n\
             run `mise run reference:swift:p0:prepare`: {error}",
            manifest.display()
        ))
    })?;
    let mut filenames = BTreeSet::new();
    let mut cases = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let fields = line.split('\t').collect::<Vec<_>>();
        let [filename, mode, origin, bytes] = fields.as_slice() else {
            return Err(failure(format!(
                "{}:{}: expected four tab-separated fields",
                manifest.display(),
                index + 1
            )));
        };
        let path = Path::new(filename);
        if path.components().count() != 1 || path.file_name().is_none() {
            return Err(failure(format!(
                "{}:{}: invalid case filename {filename:?}",
                manifest.display(),
                index + 1
            )));
        }
        if !filenames.insert((*filename).to_owned()) {
            return Err(failure(format!(
                "{}:{}: duplicate case filename {filename:?}",
                manifest.display(),
                index + 1
            )));
        }
        let expectation = match *mode {
            "strict" => Expectation::Strict,
            "recovery" => Expectation::Recovery,
            _ => {
                return Err(failure(format!(
                    "{}:{}: invalid case mode {mode:?}",
                    manifest.display(),
                    index + 1
                )));
            }
        };
        cases.push(ParserCase {
            filename: (*filename).to_owned(),
            expectation,
            origin: (*origin).to_owned(),
            bytes: bytes.parse()?,
        });
    }
    Ok(cases)
}

fn format_parse_failure(
    corpus: &str,
    path: &str,
    source: &str,
    error: &rezel_common::ParseError,
) -> Result<String> {
    let position = error
        .position()
        .map(|position| source_location(source, usize::from(position)))
        .transpose()?;
    let location = position.map_or_else(
        || "0:0".to_owned(),
        |(line, column)| format!("{line}:{column}"),
    );
    let excerpt = position
        .and_then(|(line, _)| source.lines().nth(line - 1))
        .map(str::trim)
        .unwrap_or_default();
    Ok(format!("{corpus}/{path}:{location}: {error}: {excerpt}"))
}

fn source_location(source: &str, position: usize) -> Result<(usize, usize)> {
    let prefix = source
        .get(..position)
        .ok_or_else(|| failure(format!("parser returned invalid UTF-8 position {position}")))?;
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
    let column = position - line_start + 1;
    Ok((line, column))
}

fn checked_add(left: u64, right: u64, label: &str) -> Result<u64> {
    left.checked_add(right)
        .ok_or_else(|| failure(format!("{label} byte count overflowed")))
}

fn slash_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn require_inventory<T>(label: &str, actual: T, expected: T, unit: &str) -> Result<()>
where
    T: Copy + Eq + std::fmt::Display,
{
    if actual != expected {
        return Err(failure(format!(
            "{label} inventory changed: expected {expected} {unit}, found {actual}"
        )));
    }
    Ok(())
}

fn failure(message: impl Into<String>) -> AnyError {
    message.into().into()
}
