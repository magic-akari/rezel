#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    ffi::OsStr,
    fs, io,
    path::{Component, Path, PathBuf},
};

use rezel_lr::ParseLimits;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const PHP_VERSION: &str = "8.5.9";
const PHP_SRC_COMMIT: &str = "dd6e76cce27aaa0ed9f7520648ed1081dfb6af36";
const CORPUS_FILES: usize = 5_308;
const CORPUS_BYTES: usize = 3_474_668;
const CORPUS_SHA256: &str = "3a1b988ac956a2763540d505a603362e7711c250e2e894811b083d11dbb876f8";
const SECTION_COUNT: usize = 5_301;
const SECTION_BYTES: usize = 1_839_044;
const NON_UTF8_FILES: usize = 5;
const ORACLE_ACCEPTED: usize = 5_162;
const ORACLE_REJECTED: usize = 139;
const MAX_FAILURE_DETAILS: usize = 100;
const KNOWN_REJECTIONS: &str = include_str!("../known-rejections.txt");

type AnyError = Box<dyn Error + Send + Sync>;
type Result<T> = std::result::Result<T, AnyError>;

#[derive(Deserialize)]
struct Oracle {
    php_version: String,
    sections: usize,
    bytes: usize,
    non_utf8: usize,
    accepted: usize,
    rejected: usize,
    records: Vec<OracleRecord>,
}

#[derive(Deserialize)]
struct OracleRecord {
    path: String,
    status: OracleStatus,
    bytes: usize,
    sha256: String,
    error: Option<String>,
}

#[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum OracleStatus {
    Accepted,
    Rejected,
}

struct SourceSection {
    source: String,
    bytes: usize,
    sha256: String,
}

fn main() {
    if let Err(error) = entry() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn entry() -> Result<()> {
    let repository = repository_root()?;
    let corpus = repository
        .join("target/reference-sources/php-src")
        .join(format!("php-src-{PHP_SRC_COMMIT}"));
    let tests = corpus.join("Zend/tests");
    let paths = collect_phpt_files(&tests)?;
    verify_corpus_inventory(&tests, &paths)?;

    let sections = load_source_sections(&tests, &paths)?;
    let oracle_path = repository
        .join("target/reference-sources/php-src")
        .join(format!("oracle-{PHP_VERSION}.json"));
    let oracle = load_oracle(&oracle_path)?;
    verify_oracle_header(&oracle)?;
    verify_oracle_records(&oracle, &sections)?;
    verify_rezel_acceptance(&oracle, &sections)
}

fn repository_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .map(Path::to_path_buf)
        .ok_or_else(|| failure("PHP reference package is not nested under tools/references"))
}

fn collect_phpt_files(root: &Path) -> Result<Vec<PathBuf>> {
    if !root.is_dir() {
        return Err(failure(format!(
            "pinned php-src corpus is missing: {}; run `mise run reference:php-src:prepare`",
            root.display()
        )));
    }

    let mut pending = vec![root.to_path_buf()];
    let mut paths = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() && entry.path().extension() == Some(OsStr::new("phpt")) {
                paths.push(entry.path());
            }
        }
    }
    paths.sort();
    Ok(paths)
}

fn verify_corpus_inventory(root: &Path, paths: &[PathBuf]) -> Result<()> {
    require_count("php-src .phpt file", paths.len(), CORPUS_FILES)?;

    let mut digest = Sha256::new();
    let mut bytes = 0usize;
    for path in paths {
        let data = fs::read(path)?;
        let relative = slash_path(path.strip_prefix(root)?)?;
        let data_bytes = u64::try_from(data.len())?.to_be_bytes();
        digest.update(relative.as_bytes());
        digest.update([0]);
        digest.update(data_bytes);
        digest.update(&data);
        bytes = bytes
            .checked_add(data.len())
            .ok_or_else(|| failure("php-src corpus byte count overflow"))?;
    }

    require_count("php-src corpus byte", bytes, CORPUS_BYTES)?;
    let digest = digest.finalize();
    require_text(
        "php-src corpus SHA-256",
        &format!("{digest:x}"),
        CORPUS_SHA256,
    )
}

fn load_source_sections(
    tests: &Path,
    paths: &[PathBuf],
) -> Result<BTreeMap<String, SourceSection>> {
    let mut sections = BTreeMap::new();
    let mut section_bytes = 0usize;
    let mut non_utf8 = 0usize;

    for path in paths {
        let data = fs::read(path)?;
        let Ok(test) = std::str::from_utf8(&data) else {
            non_utf8 += 1;
            continue;
        };
        let Some(source) = file_section(test) else {
            continue;
        };

        let relative = slash_path(path.strip_prefix(tests)?)?;
        let oracle_path = format!("Zend/tests/{relative}");
        let bytes = source.len();
        section_bytes = section_bytes
            .checked_add(bytes)
            .ok_or_else(|| failure("php-src section byte count overflow"))?;
        let previous = sections.insert(
            oracle_path.clone(),
            SourceSection {
                source: source.to_owned(),
                bytes,
                sha256: sha256(source.as_bytes()),
            },
        );
        if previous.is_some() {
            return Err(failure(format!(
                "duplicate php-src section path: {oracle_path}"
            )));
        }
    }

    require_count("UTF-8 php-src section", sections.len(), SECTION_COUNT)?;
    require_count("php-src section byte", section_bytes, SECTION_BYTES)?;
    require_count("non-UTF-8 php-src file", non_utf8, NON_UTF8_FILES)?;
    Ok(sections)
}

fn file_section(test: &str) -> Option<&str> {
    for marker in ["\n--FILE--\n", "\n--FILEEOF--\n"] {
        let Some(marker_start) = test.find(marker) else {
            continue;
        };
        let rest = &test[marker_start + marker.len()..];
        let end = rest.find("\n--").unwrap_or(rest.len());
        return Some(&rest[..end]);
    }
    None
}

fn load_oracle(path: &Path) -> Result<Oracle> {
    let data = fs::read(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "read PHP oracle {}: {error}; run `mise run reference:php-src:prepare`",
                path.display()
            ),
        )
    })?;
    Ok(serde_json::from_slice(&data)?)
}

fn verify_oracle_header(oracle: &Oracle) -> Result<()> {
    require_text("PHP oracle version", &oracle.php_version, PHP_VERSION)?;
    require_count("PHP oracle section", oracle.sections, SECTION_COUNT)?;
    require_count("PHP oracle byte", oracle.bytes, SECTION_BYTES)?;
    require_count("PHP oracle non-UTF-8 file", oracle.non_utf8, NON_UTF8_FILES)?;
    require_count(
        "PHP oracle accepted section",
        oracle.accepted,
        ORACLE_ACCEPTED,
    )?;
    require_count(
        "PHP oracle rejected section",
        oracle.rejected,
        ORACLE_REJECTED,
    )?;
    require_count("PHP oracle record", oracle.records.len(), SECTION_COUNT)
}

fn verify_oracle_records(
    oracle: &Oracle,
    sections: &BTreeMap<String, SourceSection>,
) -> Result<()> {
    let mut unseen = sections.keys().cloned().collect::<BTreeSet<_>>();
    let mut accepted = 0usize;
    let mut rejected = 0usize;

    for record in &oracle.records {
        validate_oracle_path(&record.path)?;
        if !unseen.remove(&record.path) {
            return Err(failure(format!(
                "PHP oracle contains a duplicate or unknown path: {}",
                record.path
            )));
        }
        let section = sections
            .get(&record.path)
            .ok_or_else(|| failure(format!("PHP oracle path is absent: {}", record.path)))?;
        require_count(
            &format!("{} oracle byte", record.path),
            record.bytes,
            section.bytes,
        )?;
        require_text(
            &format!("{} oracle SHA-256", record.path),
            &record.sha256,
            &section.sha256,
        )?;

        match record.status {
            OracleStatus::Accepted => {
                accepted += 1;
                if record.error.is_some() {
                    return Err(failure(format!(
                        "accepted PHP oracle record has an error: {}",
                        record.path
                    )));
                }
            }
            OracleStatus::Rejected => {
                rejected += 1;
                if record.error.as_deref().is_none_or(str::is_empty) {
                    return Err(failure(format!(
                        "rejected PHP oracle record has no error: {}",
                        record.path
                    )));
                }
            }
        }
    }

    if let Some(path) = unseen.first() {
        return Err(failure(format!("PHP oracle omits source section: {path}")));
    }
    require_count("accepted PHP oracle record", accepted, ORACLE_ACCEPTED)?;
    require_count("rejected PHP oracle record", rejected, ORACLE_REJECTED)
}

fn verify_rezel_acceptance(
    oracle: &Oracle,
    sections: &BTreeMap<String, SourceSection>,
) -> Result<()> {
    let expected_rejections = rejection_manifest()?;
    let parser = rezel_lang_php::parser()
        .with_strict(true)
        .with_limits(ParseLimits {
            max_actions: 5_000_000,
            max_stacks: 2,
            max_recovery_actions: 0,
            ..ParseLimits::default()
        });
    let mut actual_rejections = BTreeSet::new();
    let mut details = Vec::new();
    let mut accepted_bytes = 0usize;

    for record in &oracle.records {
        if record.status == OracleStatus::Rejected {
            continue;
        }
        let section = &sections[&record.path];
        match parser.parse(&section.source) {
            Ok(_) => {
                accepted_bytes = accepted_bytes
                    .checked_add(section.bytes)
                    .ok_or_else(|| failure("accepted PHP byte count overflow"))?;
            }
            Err(error) => {
                actual_rejections.insert(record.path.clone());
                if details.len() < MAX_FAILURE_DETAILS {
                    details.push(format!("{}: {error}", record.path));
                }
            }
        }
    }

    if actual_rejections != expected_rejections {
        let new = actual_rejections
            .difference(&expected_rejections)
            .cloned()
            .collect::<Vec<_>>();
        let stale = expected_rejections
            .difference(&actual_rejections)
            .cloned()
            .collect::<Vec<_>>();
        let mut message = String::from("strict PHP parser rejection inventory changed");
        append_paths(&mut message, "new rejections", &new);
        append_paths(&mut message, "stale known rejections", &stale);
        if !details.is_empty() {
            message.push_str("\nparse errors:\n");
            message.push_str(&details.join("\n"));
        }
        return Err(failure(message));
    }

    let accepted = ORACLE_ACCEPTED
        .checked_sub(actual_rejections.len())
        .ok_or_else(|| failure("Rezel PHP accepted count underflow"))?;
    eprintln!(
        "accepted {accepted}/{ORACLE_ACCEPTED} PHP {PHP_VERSION} Zend parser-positive sections \
         ({accepted_bytes} UTF-8 bytes; {} pinned strict rejections)",
        actual_rejections.len()
    );
    Ok(())
}

fn rejection_manifest() -> Result<BTreeSet<String>> {
    let mut paths = BTreeSet::new();
    for line in KNOWN_REJECTIONS.lines() {
        let path = line.trim();
        if path.is_empty() || path.starts_with('#') {
            continue;
        }
        validate_oracle_path(path)?;
        if !paths.insert(path.to_owned()) {
            return Err(failure(format!(
                "duplicate PHP known-rejection path: {path}"
            )));
        }
    }
    Ok(paths)
}

fn validate_oracle_path(path: &str) -> Result<()> {
    let relative = Path::new(path);
    let safe = !path.contains('\\')
        && relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && relative.starts_with("Zend/tests")
        && relative.extension() == Some(OsStr::new("phpt"));
    if !safe {
        return Err(failure(format!("unsafe PHP oracle path: {path}")));
    }
    Ok(())
}

fn append_paths(message: &mut String, label: &str, paths: &[String]) {
    if paths.is_empty() {
        return;
    }
    message.push('\n');
    message.push_str(label);
    message.push_str(":\n");
    message.push_str(&paths.join("\n"));
}

fn slash_path(path: &Path) -> Result<String> {
    let path = path
        .to_str()
        .ok_or_else(|| failure(format!("path is not UTF-8: {}", path.display())))?;
    Ok(path.replace('\\', "/"))
}

fn sha256(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

fn require_count(label: &str, actual: usize, expected: usize) -> Result<()> {
    if actual != expected {
        return Err(failure(format!(
            "{label} inventory changed: expected {expected}, found {actual}"
        )));
    }
    Ok(())
}

fn require_text(label: &str, actual: &str, expected: &str) -> Result<()> {
    if actual != expected {
        return Err(failure(format!(
            "{label} changed: expected {expected}, found {actual}"
        )));
    }
    Ok(())
}

fn failure(message: impl Into<String>) -> AnyError {
    io::Error::other(message.into()).into()
}
