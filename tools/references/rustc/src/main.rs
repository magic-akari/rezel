#![forbid(unsafe_code)]

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde::Serialize;

const REQUIRED_RUSTC_RELEASE: &str = "1.95.0";
const EDITION: &str = "2024";
const SNAPSHOT_SCHEMA: &str = "rezel.rustc-rust-reference-snapshot.v1";
const ORACLE: &str = "rustc --crate-type=lib --emit=metadata";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    schema: &'static str,
    rustc_version: String,
    edition: &'static str,
    coordinates: &'static str,
    oracle: &'static str,
    accepted: Vec<ReferenceCase>,
    rejected: Vec<ReferenceCase>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReferenceCase {
    id: String,
    source_name: String,
    source: String,
}

struct Fixture {
    id: String,
    source_name: String,
    path: PathBuf,
    source: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let update = parse_arguments(env::args().skip(1))?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rustc_version = rustc_version()?;
    let snapshot = build_snapshot(root, rustc_version)?;
    let encoded = render_snapshot(&snapshot)?;
    let snapshot_path = root.join("snapshots/rust.json");

    if update {
        let parent = snapshot_path
            .parent()
            .ok_or_else(|| format!("{} has no parent directory", snapshot_path.display()))?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
        fs::write(&snapshot_path, encoded)
            .map_err(|error| format!("write {}: {error}", snapshot_path.display()))?;
        eprintln!("updated {}", snapshot_path.display());
        return Ok(());
    }

    let checked = fs::read(&snapshot_path)
        .map_err(|error| format!("read {}: {error}", snapshot_path.display()))?;
    if checked != encoded {
        return Err(
            "rustc reference snapshot is stale; run rezel-rustc-reference --update".to_owned(),
        );
    }
    Ok(())
}

fn parse_arguments(arguments: impl Iterator<Item = String>) -> Result<bool, String> {
    let arguments = arguments.collect::<Vec<_>>();
    match arguments.as_slice() {
        [argument] if argument == "--check" => Ok(false),
        [argument] if argument == "--update" => Ok(true),
        _ => Err("usage: rezel-rustc-reference --check|--update".to_owned()),
    }
}

fn rustc_version() -> Result<String, String> {
    let output = Command::new("rustc")
        .args(["--version", "--verbose"])
        .output()
        .map_err(|error| format!("run rustc --version --verbose: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "rustc --version --verbose failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let verbose = String::from_utf8(output.stdout)
        .map_err(|error| format!("rustc version output is not UTF-8: {error}"))?;
    let version = verbose
        .lines()
        .next()
        .ok_or_else(|| "rustc produced empty version output".to_owned())?;
    let release = verbose
        .lines()
        .find_map(|line| line.strip_prefix("release: "))
        .ok_or_else(|| "rustc version output has no release field".to_owned())?;
    if release != REQUIRED_RUSTC_RELEASE {
        return Err(format!(
            "Rust {REQUIRED_RUSTC_RELEASE} is required, found {release}"
        ));
    }
    Ok(version.to_owned())
}

fn build_snapshot(root: &Path, rustc_version: String) -> Result<Snapshot, String> {
    let accepted = reference_cases(root, "parse-accepted", true)?;
    let rejected = reference_cases(root, "parse-rejected", false)?;
    if accepted.is_empty() || rejected.is_empty() {
        return Err("both accepted and rejected Rust fixtures are required".to_owned());
    }

    Ok(Snapshot {
        schema: SNAPSHOT_SCHEMA,
        rustc_version,
        edition: EDITION,
        coordinates: "raw-utf8-bytes",
        oracle: ORACLE,
        accepted,
        rejected,
    })
}

fn reference_cases(
    root: &Path,
    directory: &str,
    expected_to_compile: bool,
) -> Result<Vec<ReferenceCase>, String> {
    let fixture_root = root.join("fixtures").join(directory);
    let fixtures = read_fixtures(&fixture_root)?;
    let mut cases = Vec::with_capacity(fixtures.len());

    for (index, fixture) in fixtures.into_iter().enumerate() {
        let output = compile_fixture(&fixture, index)?;
        if output.status.success() != expected_to_compile {
            let expectation = if expected_to_compile {
                "accepted"
            } else {
                "rejected"
            };
            return Err(format!(
                "{} must be {expectation} by rustc {REQUIRED_RUSTC_RELEASE} in Edition {EDITION}:\n{}",
                fixture.id,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        cases.push(ReferenceCase {
            id: fixture.id,
            source_name: fixture.source_name,
            source: fixture.source,
        });
    }
    Ok(cases)
}

fn compile_fixture(fixture: &Fixture, index: usize) -> Result<Output, String> {
    let metadata = env::temp_dir().join(format!(
        "rezel-rustc-reference-{}-{index}.rmeta",
        std::process::id()
    ));
    let emit = format!("metadata={}", metadata.display());
    let crate_name = format!("rezel_reference_{index}");
    let output = Command::new("rustc")
        .args([
            "--color=never",
            "--error-format=human",
            "--crate-type=lib",
            "--edition",
            EDITION,
            "--crate-name",
            &crate_name,
            "--emit",
            &emit,
        ])
        .arg(&fixture.path)
        .output()
        .map_err(|error| format!("compile {} with rustc: {error}", fixture.path.display()))?;

    match fs::remove_file(&metadata) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("remove {}: {error}", metadata.display())),
    }
    Ok(output)
}

fn read_fixtures(root: &Path) -> Result<Vec<Fixture>, String> {
    let mut pending = vec![root.to_owned()];
    let mut paths = Vec::new();
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory)
            .map_err(|error| format!("read {}: {error}", directory.display()))?;
        for entry in entries {
            let entry =
                entry.map_err(|error| format!("read entry in {}: {error}", directory.display()))?;
            let file_type = entry
                .file_type()
                .map_err(|error| format!("inspect {}: {error}", entry.path().display()))?;
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() && entry.path().extension() == Some(OsStr::new("rs")) {
                paths.push(entry.path());
            }
        }
    }
    paths.sort();

    paths
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| format!("relativize {}: {error}", path.display()))?;
            let source_name = relative.to_string_lossy().replace('\\', "/");
            let id = source_name
                .strip_suffix(".rs")
                .ok_or_else(|| format!("{source_name} has no .rs suffix"))?
                .to_owned();
            let source = fs::read_to_string(&path)
                .map_err(|error| format!("read {} as UTF-8: {error}", path.display()))?;
            Ok(Fixture {
                id,
                source_name,
                path,
                source,
            })
        })
        .collect()
}

fn render_snapshot(snapshot: &Snapshot) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"\t");
    let mut serializer = serde_json::Serializer::with_formatter(&mut output, formatter);
    snapshot
        .serialize(&mut serializer)
        .map_err(|error| format!("encode rustc snapshot: {error}"))?;
    output.push(b'\n');
    Ok(output)
}
