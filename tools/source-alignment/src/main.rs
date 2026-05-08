#![forbid(unsafe_code)]

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt::Write as FmtWrite;
use std::fs;
use std::io::Write as IoWrite;
use std::path::{Component as PathComponent, Path, PathBuf};
use std::process::{Command, Stdio};
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

use quote::ToTokens;
use serde::{Deserialize, Serialize};
use syn::{ForeignItem, ImplItem, Item, TraitItem, Type};

const SCHEMA_VERSION: u32 = 1;
const ALIGNMENT_DIRECTORY: &str = "alignment";
const SNAPSHOT_DIRECTORY: &str = "alignment/upstream";
const ENUMERATOR_PATH: &str = "tools/source-alignment/enumerate-upstream.ts";
const TYPESCRIPT_PATH: &str = "tools/source-alignment/node_modules/typescript";

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    component: Component,
    upstream: Option<Upstream>,
    #[serde(default)]
    snapshot: Vec<SnapshotSpec>,
    scope: Vec<Scope>,
    #[serde(default)]
    override_item: Vec<ItemOverride>,
    #[serde(default)]
    excluded: Vec<Excluded>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Component {
    name: String,
    source_root: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Upstream {
    repository: String,
    commit: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotSpec {
    source: String,
    path: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UpstreamSnapshot {
    schema_version: u32,
    repository: String,
    commit: String,
    source: String,
    blob_oid: String,
    symbols: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnumeratorOutput {
    symbols: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Scope {
    rust_file: PathBuf,
    classification: Classification,
    upstream_file: Option<String>,
    #[serde(default)]
    upstream_symbols: Vec<String>,
    invariant: String,
    representation: String,
    #[serde(default)]
    evidence_files: Vec<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemOverride {
    rust_item: String,
    classification: Classification,
    upstream_file: Option<String>,
    #[serde(default)]
    upstream_symbols: Vec<String>,
    invariant: String,
    representation: String,
    #[serde(default)]
    evidence_files: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum Classification {
    Port,
    Adapted,
    StandardAlgorithm,
    RezelOwned,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Excluded {
    upstream_file: String,
    upstream_symbol: String,
    reason: String,
}

struct LoadedManifest {
    path: PathBuf,
    manifest: Manifest,
}

#[derive(Clone, Copy)]
struct Trace<'a> {
    classification: Classification,
    upstream_file: Option<&'a str>,
    upstream_symbols: &'a [String],
    invariant: &'a str,
    representation: &'a str,
}

struct Report {
    files: usize,
    overrides: usize,
    excluded: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SnapshotMode {
    Check,
    Write,
}

struct SnapshotArguments {
    component: String,
    checkout: PathBuf,
    root: Option<PathBuf>,
    mode: SnapshotMode,
}

struct SnapshotCandidate {
    relative_path: PathBuf,
    output_path: PathBuf,
    bytes: Vec<u8>,
}

#[cfg(test)]
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn main() {
    if let Err(error) = run() {
        eprintln!("source-alignment: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    let Some(command) = arguments.first().and_then(|value| value.to_str()) else {
        print_help();
        return Ok(());
    };

    match command {
        "-h" | "--help" | "help" => {
            print_help();
            Ok(())
        }
        "-V" | "--version" => {
            println!("rezel-source-alignment {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "verify" => verify_command(&arguments[1..]),
        "snapshot" => snapshot_command(&arguments[1..]),
        unknown => Err(format!("unknown command {unknown:?}; run with --help").into()),
    }
}

fn print_help() {
    println!(
        "\
Source-alignment traceability for Rezel

Usage:
  source-alignment verify [--root PATH]
  source-alignment snapshot --component NAME --checkout PATH --check [--root PATH]
  source-alignment snapshot --component NAME --checkout PATH --write [--root PATH]

Commands:
  verify    Validate source ownership, references, and committed snapshots
  snapshot  Reproduce snapshots from an exact pinned Git checkout

verify is offline and read-only. snapshot --check is read-only, while --write
is the only command that updates snapshots. This tool validates traceability;
it does not execute tests or prove behavioral equivalence."
    );
}

fn verify_command(arguments: &[OsString]) -> Result<()> {
    if arguments.len() == 1
        && arguments[0]
            .to_str()
            .is_some_and(|argument| matches!(argument, "-h" | "--help"))
    {
        println!("Usage: source-alignment verify [--root PATH]");
        return Ok(());
    }

    let root = parse_optional_root(arguments)?;
    let repository = resolve_repository(root.as_deref())?;
    verify_repository(&repository)
}

fn parse_optional_root(arguments: &[OsString]) -> Result<Option<PathBuf>> {
    match arguments {
        [] => Ok(None),
        [flag, path] if flag == OsStr::new("--root") => Ok(Some(PathBuf::from(path))),
        _ => Err("usage: source-alignment verify [--root PATH]".into()),
    }
}

fn snapshot_command(arguments: &[OsString]) -> Result<()> {
    if arguments.len() == 1
        && arguments[0]
            .to_str()
            .is_some_and(|argument| matches!(argument, "-h" | "--help"))
    {
        print_snapshot_help();
        return Ok(());
    }

    let parsed = parse_snapshot_arguments(arguments)?;
    let repository = resolve_repository(parsed.root.as_deref())?;
    reproduce_snapshots(
        &repository,
        &parsed.component,
        &parsed.checkout,
        parsed.mode,
    )
}

fn print_snapshot_help() {
    println!(
        "\
Usage:
  source-alignment snapshot --component NAME --checkout PATH --check [--root PATH]
  source-alignment snapshot --component NAME --checkout PATH --write [--root PATH]

The checkout must be at the exact commit declared by the component's alignment
manifest. Install the pinned TypeScript parser first with:
  npm ci --prefix tools/source-alignment"
    );
}

fn parse_snapshot_arguments(arguments: &[OsString]) -> Result<SnapshotArguments> {
    let mut component = None;
    let mut checkout = None;
    let mut root = None;
    let mut mode = None;
    let mut index = 0;

    while index < arguments.len() {
        let flag = arguments[index]
            .to_str()
            .ok_or("snapshot options must be valid UTF-8")?;
        match flag {
            "--component" => {
                component = Some(take_utf8_value(arguments, &mut index, flag)?);
            }
            "--checkout" => {
                checkout = Some(PathBuf::from(take_value(arguments, &mut index, flag)?));
            }
            "--root" => {
                root = Some(PathBuf::from(take_value(arguments, &mut index, flag)?));
            }
            "--check" => set_snapshot_mode(&mut mode, SnapshotMode::Check)?,
            "--write" => set_snapshot_mode(&mut mode, SnapshotMode::Write)?,
            unknown => return Err(format!("unknown snapshot option {unknown:?}").into()),
        }
        index += 1;
    }

    Ok(SnapshotArguments {
        component: component.ok_or("snapshot requires --component NAME")?,
        checkout: checkout.ok_or("snapshot requires --checkout PATH")?,
        root,
        mode: mode.ok_or("snapshot requires exactly one of --check or --write")?,
    })
}

fn take_value<'a>(arguments: &'a [OsString], index: &mut usize, flag: &str) -> Result<&'a OsStr> {
    *index += 1;
    arguments
        .get(*index)
        .map(OsString::as_os_str)
        .ok_or_else(|| format!("{flag} requires a value").into())
}

fn take_utf8_value(arguments: &[OsString], index: &mut usize, flag: &str) -> Result<String> {
    take_value(arguments, index, flag)?
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{flag} value must be valid UTF-8").into())
}

fn set_snapshot_mode(current: &mut Option<SnapshotMode>, next: SnapshotMode) -> Result<()> {
    if current.replace(next).is_some() {
        return Err("snapshot requires exactly one of --check or --write".into());
    }
    Ok(())
}

fn resolve_repository(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        let repository = fs::canonicalize(path)?;
        if !is_repository_root(&repository) {
            return Err(
                format!("{} is not the Rezel repository root", repository.display()).into(),
            );
        }
        return Ok(repository);
    }

    discover_repository_from(&env::current_dir()?)
}

fn discover_repository_from(start: &Path) -> Result<PathBuf> {
    let start = fs::canonicalize(start)?;
    for candidate in start.ancestors() {
        if is_repository_root(candidate) {
            return Ok(candidate.to_path_buf());
        }
    }
    Err(format!(
        "could not find the Rezel repository root from {}",
        start.display()
    )
    .into())
}

fn is_repository_root(path: &Path) -> bool {
    path.join("Cargo.toml").is_file()
        && path.join(ALIGNMENT_DIRECTORY).is_dir()
        && path.join("tools/source-alignment/Cargo.toml").is_file()
}

fn verify_repository(repository: &Path) -> Result<()> {
    let repository = fs::canonicalize(repository)?;
    let manifests = load_manifests(&repository)?;
    validate_manifest_set(&manifests)?;

    let mut file_count = 0;
    let mut override_count = 0;
    let mut excluded_count = 0;
    for loaded in &manifests {
        let report = verify_manifest(&repository, loaded)?;
        file_count += report.files;
        override_count += report.overrides;
        excluded_count += report.excluded;
        println!(
            "{}: {} Rust files, {} item overrides, {} explicit exclusions",
            loaded.manifest.component.name, report.files, report.overrides, report.excluded
        );
    }
    println!(
        "source alignment verified: {file_count} Rust files, {override_count} item overrides, \
         {excluded_count} explicit exclusions"
    );
    Ok(())
}

fn load_manifests(repository: &Path) -> Result<Vec<LoadedManifest>> {
    let root = repository.join(ALIGNMENT_DIRECTORY);
    let mut paths = fs::read_dir(&root)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    paths.retain(|path| path.extension().and_then(|extension| extension.to_str()) == Some("toml"));
    paths.sort();
    if paths.is_empty() {
        return Err(format!("no alignment manifests found in {}", root.display()).into());
    }

    let mut manifests = Vec::with_capacity(paths.len());
    for path in paths {
        let text = fs::read_to_string(&path)?;
        let manifest = toml::from_str(&text)
            .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
        manifests.push(LoadedManifest { path, manifest });
    }
    Ok(manifests)
}

fn validate_manifest_set(manifests: &[LoadedManifest]) -> Result<()> {
    let mut names = BTreeSet::new();
    let mut source_roots = BTreeSet::new();
    let mut snapshot_paths = BTreeSet::new();
    let mut failures = Vec::new();

    for loaded in manifests {
        let manifest = &loaded.manifest;
        if !names.insert(manifest.component.name.as_str()) {
            failures.push(format!(
                "duplicate component name {}",
                manifest.component.name
            ));
        }
        if !source_roots.insert(&manifest.component.source_root) {
            failures.push(format!(
                "duplicate component source_root {}",
                manifest.component.source_root.display()
            ));
        }
        for snapshot in &manifest.snapshot {
            if !snapshot_paths.insert(&snapshot.path) {
                failures.push(format!(
                    "duplicate snapshot path {}",
                    snapshot.path.display()
                ));
            }
        }
    }

    finish_failures("alignment manifest set", failures)
}

fn verify_manifest(repository: &Path, loaded: &LoadedManifest) -> Result<Report> {
    let manifest = &loaded.manifest;
    let mut failures = Vec::new();
    let source_root = validate_manifest_header(repository, loaded, &mut failures);
    let snapshots = load_snapshots(repository, manifest, &mut failures);
    let mut used_upstream = BTreeSet::new();

    let mut source_files = BTreeSet::new();
    let mut declarations = BTreeMap::new();
    if let Some(source_root) = source_root.as_deref() {
        source_files = enumerate_source_files(source_root)?;
        declarations = enumerate_declarations(
            source_root,
            &source_files,
            &manifest.component.name.replace('-', "_"),
        )?;
    }

    validate_scopes(
        repository,
        manifest,
        &source_files,
        &snapshots,
        &mut used_upstream,
        &mut failures,
    );
    validate_overrides(
        repository,
        manifest,
        &declarations,
        &snapshots,
        &mut used_upstream,
        &mut failures,
    );
    validate_exclusions(manifest, &snapshots, &mut used_upstream, &mut failures);

    for snapshot in &manifest.snapshot {
        if !used_upstream.contains(snapshot.source.as_str()) {
            failures.push(format!(
                "snapshot source {} is not referenced by a scope, override, or exclusion",
                snapshot.source
            ));
        }
    }

    failures.sort();
    failures.dedup();
    if !failures.is_empty() {
        let mut message = format!(
            "source alignment verification failed for {}:\n",
            loaded.path.display()
        );
        for failure in failures {
            let _ = writeln!(message, "- {failure}");
        }
        return Err(message.into());
    }

    Ok(Report {
        files: source_files.len(),
        overrides: manifest.override_item.len(),
        excluded: manifest.excluded.len(),
    })
}

fn validate_manifest_header(
    repository: &Path,
    loaded: &LoadedManifest,
    failures: &mut Vec<String>,
) -> Option<PathBuf> {
    let manifest = &loaded.manifest;
    if manifest.schema_version != SCHEMA_VERSION {
        failures.push(format!(
            "schema_version is {}, expected {SCHEMA_VERSION}",
            manifest.schema_version
        ));
    }
    if manifest.component.name.trim().is_empty() {
        failures.push("component.name is empty".to_owned());
    }
    if manifest.scope.is_empty() {
        failures.push("manifest has no Rust file scopes".to_owned());
    }

    let source_root = match resolve_existing_directory(
        repository,
        &manifest.component.source_root,
        "component.source_root",
    ) {
        Ok(path) => Some(path),
        Err(error) => {
            failures.push(error.to_string());
            None
        }
    };

    if let Some(upstream) = &manifest.upstream {
        if upstream.repository.trim().is_empty() {
            failures.push("upstream.repository is empty".to_owned());
        }
        if !is_full_git_oid(&upstream.commit) {
            failures.push("upstream.commit must be a lowercase 40-digit Git object ID".to_owned());
        }
    } else if !manifest.snapshot.is_empty() {
        failures.push("snapshots require an upstream table".to_owned());
    }

    source_root
}

fn load_snapshots(
    repository: &Path,
    manifest: &Manifest,
    failures: &mut Vec<String>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut result = BTreeMap::new();
    let mut paths = BTreeSet::new();

    for specification in &manifest.snapshot {
        if !is_source_path(&specification.source) {
            failures.push(format!(
                "snapshot source {:?} must be a relative slash-separated path",
                specification.source
            ));
            continue;
        }
        if !is_snapshot_path(&specification.path) {
            failures.push(format!(
                "snapshot path {} must be below {SNAPSHOT_DIRECTORY}",
                specification.path.display()
            ));
            continue;
        }
        if !paths.insert(&specification.path) {
            failures.push(format!(
                "duplicate snapshot path {}",
                specification.path.display()
            ));
            continue;
        }

        let path = match resolve_existing_file(repository, &specification.path, "snapshot") {
            Ok(path) => path,
            Err(error) => {
                failures.push(error.to_string());
                continue;
            }
        };
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                failures.push(format!("failed to read {}: {error}", path.display()));
                continue;
            }
        };
        let snapshot: UpstreamSnapshot = match serde_json::from_str(&text) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                failures.push(format!(
                    "failed to parse {}: {error}",
                    specification.path.display()
                ));
                continue;
            }
        };
        validate_snapshot_metadata(manifest, specification, &snapshot, failures);

        let symbols = snapshot.symbols.into_iter().collect();
        if result
            .insert(specification.source.clone(), symbols)
            .is_some()
        {
            failures.push(format!(
                "duplicate snapshot source {}",
                specification.source
            ));
        }
    }

    result
}

fn validate_snapshot_metadata(
    manifest: &Manifest,
    specification: &SnapshotSpec,
    snapshot: &UpstreamSnapshot,
    failures: &mut Vec<String>,
) {
    if snapshot.schema_version != SCHEMA_VERSION {
        failures.push(format!(
            "snapshot {} has schema_version {}, expected {SCHEMA_VERSION}",
            specification.path.display(),
            snapshot.schema_version
        ));
    }
    if let Some(upstream) = &manifest.upstream {
        if snapshot.repository != upstream.repository {
            failures.push(format!(
                "snapshot {} records repository {:?}, expected {:?}",
                specification.path.display(),
                snapshot.repository,
                upstream.repository
            ));
        }
        if snapshot.commit != upstream.commit {
            failures.push(format!(
                "snapshot {} records commit {}, expected {}",
                specification.path.display(),
                snapshot.commit,
                upstream.commit
            ));
        }
    }
    if snapshot.source != specification.source {
        failures.push(format!(
            "snapshot {} records source {:?}, expected {:?}",
            specification.path.display(),
            snapshot.source,
            specification.source
        ));
    }
    if !is_full_git_oid(&snapshot.blob_oid) {
        failures.push(format!(
            "snapshot {} has an invalid blob_oid",
            specification.path.display()
        ));
    }
    if snapshot.symbols.is_empty() {
        failures.push(format!(
            "snapshot {} has no declarations",
            specification.path.display()
        ));
    }
    if !is_sorted_unique(&snapshot.symbols) {
        failures.push(format!(
            "snapshot {} symbols must be sorted and unique",
            specification.path.display()
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_scopes(
    repository: &Path,
    manifest: &Manifest,
    source_files: &BTreeSet<PathBuf>,
    snapshots: &BTreeMap<String, BTreeSet<String>>,
    used_upstream: &mut BTreeSet<String>,
    failures: &mut Vec<String>,
) {
    let mut mapped_files = BTreeSet::new();
    for scope in &manifest.scope {
        let label = format!("scope {}", scope.rust_file.display());
        if !is_repository_relative(&scope.rust_file) {
            failures.push(format!("{label} must be relative to component.source_root"));
        } else if !source_files.contains(&scope.rust_file) {
            failures.push(format!("{label} does not name a Rust source file"));
        }
        if !mapped_files.insert(&scope.rust_file) {
            failures.push(format!("duplicate {label}"));
        }
        validate_trace(
            &label,
            Trace {
                classification: scope.classification,
                upstream_file: scope.upstream_file.as_deref(),
                upstream_symbols: &scope.upstream_symbols,
                invariant: &scope.invariant,
                representation: &scope.representation,
            },
            manifest.upstream.as_ref(),
            snapshots,
            used_upstream,
            failures,
        );
        validate_evidence_files(repository, &label, &scope.evidence_files, failures);
    }

    for source_file in source_files {
        if !mapped_files.contains(source_file) {
            failures.push(format!(
                "Rust source file {} has no alignment scope",
                source_file.display()
            ));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_overrides(
    repository: &Path,
    manifest: &Manifest,
    declarations: &BTreeMap<String, PathBuf>,
    snapshots: &BTreeMap<String, BTreeSet<String>>,
    used_upstream: &mut BTreeSet<String>,
    failures: &mut Vec<String>,
) {
    let mut overrides = BTreeSet::new();
    for item in &manifest.override_item {
        let label = format!("override {}", item.rust_item);
        if !overrides.insert(item.rust_item.as_str()) {
            failures.push(format!("duplicate {label}"));
        }
        if !declarations.contains_key(&item.rust_item) {
            failures.push(format!("{label} does not name a Rust declaration"));
        }
        validate_trace(
            &label,
            Trace {
                classification: item.classification,
                upstream_file: item.upstream_file.as_deref(),
                upstream_symbols: &item.upstream_symbols,
                invariant: &item.invariant,
                representation: &item.representation,
            },
            manifest.upstream.as_ref(),
            snapshots,
            used_upstream,
            failures,
        );
        validate_evidence_files(repository, &label, &item.evidence_files, failures);
    }
}

fn validate_exclusions(
    manifest: &Manifest,
    snapshots: &BTreeMap<String, BTreeSet<String>>,
    used_upstream: &mut BTreeSet<String>,
    failures: &mut Vec<String>,
) {
    let mut exclusions = BTreeSet::new();
    for excluded in &manifest.excluded {
        let key = (
            excluded.upstream_file.as_str(),
            excluded.upstream_symbol.as_str(),
        );
        if !exclusions.insert(key) {
            failures.push(format!(
                "duplicate exclusion {}::{}",
                excluded.upstream_file, excluded.upstream_symbol
            ));
        }
        if excluded.reason.trim().is_empty() {
            failures.push(format!(
                "excluded {}::{} lacks a reason",
                excluded.upstream_file, excluded.upstream_symbol
            ));
        }
        used_upstream.insert(excluded.upstream_file.clone());
        validate_upstream_symbols(
            &format!("excluded {}", excluded.upstream_symbol),
            Some(&excluded.upstream_file),
            std::slice::from_ref(&excluded.upstream_symbol),
            snapshots,
            failures,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_trace(
    label: &str,
    trace: Trace<'_>,
    upstream: Option<&Upstream>,
    snapshots: &BTreeMap<String, BTreeSet<String>>,
    used_upstream: &mut BTreeSet<String>,
    failures: &mut Vec<String>,
) {
    let requires_upstream = matches!(
        trace.classification,
        Classification::Port | Classification::Adapted
    );
    if requires_upstream {
        if upstream.is_none() {
            failures.push(format!("{label} requires a component upstream"));
        }
        if trace.upstream_file.is_none_or(str::is_empty) || trace.upstream_symbols.is_empty() {
            failures.push(format!("{label} lacks an upstream source or symbol"));
        }
    } else if trace.upstream_file.is_some_and(|file| !file.is_empty())
        || !trace.upstream_symbols.is_empty()
    {
        failures.push(format!(
            "{label} is {:?} but declares upstream symbols",
            trace.classification
        ));
    }
    if trace.invariant.trim().is_empty() {
        failures.push(format!("{label} lacks a semantic invariant"));
    }
    if trace.representation.trim().is_empty() {
        failures.push(format!("{label} lacks a representation note"));
    }

    if let Some(file) = trace.upstream_file.filter(|file| !file.is_empty()) {
        used_upstream.insert(file.to_owned());
    }
    let mut unique_symbols = BTreeSet::new();
    for symbol in trace.upstream_symbols {
        if symbol.trim().is_empty() {
            failures.push(format!("{label} contains an empty upstream symbol"));
        } else if !unique_symbols.insert(symbol.as_str()) {
            failures.push(format!("{label} repeats upstream symbol {symbol}"));
        }
    }
    validate_upstream_symbols(
        label,
        trace.upstream_file,
        trace.upstream_symbols,
        snapshots,
        failures,
    );
}

fn validate_upstream_symbols(
    label: &str,
    upstream_file: Option<&str>,
    symbols: &[String],
    snapshots: &BTreeMap<String, BTreeSet<String>>,
    failures: &mut Vec<String>,
) {
    if symbols.is_empty() {
        return;
    }
    let Some(file) = upstream_file else {
        failures.push(format!(
            "{label} declares symbols without an upstream source"
        ));
        return;
    };
    if !is_source_path(file) {
        failures.push(format!(
            "{label} upstream source {file:?} must be a relative slash-separated path"
        ));
        return;
    }
    let Some(available) = snapshots.get(file) else {
        failures.push(format!(
            "{label} refers to {file}, which has no committed snapshot"
        ));
        return;
    };
    for symbol in symbols {
        if !available.contains(symbol) {
            failures.push(format!(
                "{label} refers to missing upstream symbol {file}::{symbol}"
            ));
        }
    }
}

fn validate_evidence_files(
    repository: &Path,
    label: &str,
    evidence_files: &[PathBuf],
    failures: &mut Vec<String>,
) {
    let mut unique = BTreeSet::new();
    for evidence in evidence_files {
        if !unique.insert(evidence) {
            failures.push(format!(
                "{label} repeats evidence file {}",
                evidence.display()
            ));
        }
        if let Err(error) = resolve_existing_file(repository, evidence, "evidence file") {
            failures.push(format!("{label}: {error}"));
        }
    }
}

fn enumerate_source_files(source_root: &Path) -> Result<BTreeSet<PathBuf>> {
    let mut result = BTreeSet::new();
    collect_source_files(source_root, source_root, &mut result)?;
    if result.is_empty() {
        return Err(format!("no Rust source files found below {}", source_root.display()).into());
    }
    Ok(result)
}

fn collect_source_files(
    source_root: &Path,
    directory: &Path,
    result: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(format!("source symlink {} is not allowed", path.display()).into());
        }
        if file_type.is_dir() {
            collect_source_files(source_root, &path, result)?;
        } else if file_type.is_file()
            && path.extension().and_then(|extension| extension.to_str()) == Some("rs")
        {
            result.insert(path.strip_prefix(source_root)?.to_path_buf());
        }
    }
    Ok(())
}

fn enumerate_declarations(
    source_root: &Path,
    source_files: &BTreeSet<PathBuf>,
    crate_name: &str,
) -> Result<BTreeMap<String, PathBuf>> {
    let mut result = BTreeMap::new();
    for relative in source_files {
        let path = source_root.join(relative);
        let source = fs::read_to_string(&path)?;
        let syntax = syn::parse_file(&source)
            .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
        let module = module_prefix(crate_name, relative);
        collect_items(&syntax.items, &module, relative, &mut result)?;
    }
    Ok(result)
}

fn module_prefix(crate_name: &str, relative: &Path) -> String {
    let mut components = relative
        .with_extension("")
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if components
        .last()
        .is_some_and(|component| matches!(component.as_str(), "lib" | "main" | "mod"))
    {
        components.pop();
    }
    if components.is_empty() {
        crate_name.to_owned()
    } else {
        format!("{crate_name}::{}", components.join("::"))
    }
}

fn collect_items(
    items: &[Item],
    module: &str,
    relative: &Path,
    declarations: &mut BTreeMap<String, PathBuf>,
) -> Result<()> {
    for item in items {
        match item {
            Item::Const(item) => {
                insert_declaration(declarations, format!("{module}::{}", item.ident), relative)?;
            }
            Item::Enum(item) => {
                let owner = format!("{module}::{}", item.ident);
                insert_declaration(declarations, owner.clone(), relative)?;
                for variant in &item.variants {
                    insert_declaration(
                        declarations,
                        format!("{owner}::{}", variant.ident),
                        relative,
                    )?;
                }
            }
            Item::ExternCrate(item) => {
                insert_declaration(declarations, format!("{module}::{}", item.ident), relative)?;
            }
            Item::Fn(item) => {
                insert_declaration(
                    declarations,
                    format!("{module}::{}", item.sig.ident),
                    relative,
                )?;
            }
            Item::ForeignMod(item) => {
                collect_foreign_items(&item.items, module, relative, declarations)?;
            }
            Item::Impl(item) => {
                collect_impl_items(item, module, relative, declarations)?;
            }
            Item::Macro(item) => {
                if let Some(identifier) = &item.ident {
                    insert_declaration(declarations, format!("{module}::{identifier}"), relative)?;
                }
            }
            Item::Mod(item) => {
                let nested = format!("{module}::{}", item.ident);
                insert_declaration(declarations, nested.clone(), relative)?;
                if let Some((_, items)) = &item.content {
                    collect_items(items, &nested, relative, declarations)?;
                }
            }
            Item::Static(item) => {
                insert_declaration(declarations, format!("{module}::{}", item.ident), relative)?;
            }
            Item::Struct(item) => {
                insert_declaration(declarations, format!("{module}::{}", item.ident), relative)?;
            }
            Item::Trait(item) => {
                let owner = format!("{module}::{}", item.ident);
                insert_declaration(declarations, owner.clone(), relative)?;
                collect_trait_items(&item.items, &owner, relative, declarations)?;
            }
            Item::TraitAlias(item) => {
                insert_declaration(declarations, format!("{module}::{}", item.ident), relative)?;
            }
            Item::Type(item) => {
                insert_declaration(declarations, format!("{module}::{}", item.ident), relative)?;
            }
            Item::Union(item) => {
                insert_declaration(declarations, format!("{module}::{}", item.ident), relative)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn collect_impl_items(
    implementation: &syn::ItemImpl,
    module: &str,
    relative: &Path,
    declarations: &mut BTreeMap<String, PathBuf>,
) -> Result<()> {
    let Some(type_name) = type_name(&implementation.self_ty) else {
        return Ok(());
    };
    let owner = implementation.trait_.as_ref().map_or_else(
        || format!("{module}::{type_name}"),
        |(_, trait_path, _)| format!("{module}::<{type_name} as {}>", syntax_name(trait_path)),
    );
    for item in &implementation.items {
        let identifier = match item {
            ImplItem::Const(item) => Some(&item.ident),
            ImplItem::Fn(item) => Some(&item.sig.ident),
            ImplItem::Type(item) => Some(&item.ident),
            _ => None,
        };
        if let Some(identifier) = identifier {
            insert_declaration(declarations, format!("{owner}::{identifier}"), relative)?;
        }
    }
    Ok(())
}

fn collect_trait_items(
    items: &[TraitItem],
    owner: &str,
    relative: &Path,
    declarations: &mut BTreeMap<String, PathBuf>,
) -> Result<()> {
    for item in items {
        let identifier = match item {
            TraitItem::Const(item) => Some(&item.ident),
            TraitItem::Fn(item) => Some(&item.sig.ident),
            TraitItem::Type(item) => Some(&item.ident),
            _ => None,
        };
        if let Some(identifier) = identifier {
            insert_declaration(declarations, format!("{owner}::{identifier}"), relative)?;
        }
    }
    Ok(())
}

fn collect_foreign_items(
    items: &[ForeignItem],
    module: &str,
    relative: &Path,
    declarations: &mut BTreeMap<String, PathBuf>,
) -> Result<()> {
    for item in items {
        let identifier = match item {
            ForeignItem::Fn(item) => Some(&item.sig.ident),
            ForeignItem::Static(item) => Some(&item.ident),
            ForeignItem::Type(item) => Some(&item.ident),
            _ => None,
        };
        if let Some(identifier) = identifier {
            insert_declaration(declarations, format!("{module}::{identifier}"), relative)?;
        }
    }
    Ok(())
}

fn insert_declaration(
    declarations: &mut BTreeMap<String, PathBuf>,
    declaration: String,
    relative: &Path,
) -> Result<()> {
    match declarations.entry(declaration) {
        Entry::Vacant(entry) => {
            entry.insert(relative.to_path_buf());
        }
        Entry::Occupied(entry) => {
            return Err(format!(
                "Rust declaration {} is ambiguous between {} and {}",
                entry.key(),
                entry.get().display(),
                relative.display()
            )
            .into());
        }
    }
    Ok(())
}

fn type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Group(group) => type_name(&group.elem),
        Type::Paren(parenthesized) => type_name(&parenthesized.elem),
        Type::Path(path) => Some(syntax_name(&path.path)),
        Type::Reference(reference) => type_name(&reference.elem),
        _ => None,
    }
}

fn syntax_name(syntax: &impl ToTokens) -> String {
    syntax
        .to_token_stream()
        .to_string()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn reproduce_snapshots(
    repository: &Path,
    component: &str,
    checkout: &Path,
    mode: SnapshotMode,
) -> Result<()> {
    let manifests = load_manifests(repository)?;
    validate_manifest_set(&manifests)?;
    let loaded = manifests
        .iter()
        .find(|loaded| loaded.manifest.component.name == component)
        .ok_or_else(|| format!("no alignment manifest for component {component:?}"))?;
    preflight_snapshot(repository, loaded)?;

    let checkout = fs::canonicalize(checkout)?;
    if !checkout.is_dir() {
        return Err(format!("checkout {} is not a directory", checkout.display()).into());
    }
    let upstream = loaded
        .manifest
        .upstream
        .as_ref()
        .ok_or("snapshot reproduction requires an upstream table")?;
    let head = git_stdout(&checkout, &["rev-parse", "HEAD"])?;
    let head = String::from_utf8(head)?.trim().to_owned();
    if head != upstream.commit {
        return Err(format!(
            "upstream checkout is at {head}, expected {}",
            upstream.commit
        )
        .into());
    }

    let candidates = generate_snapshot_candidates(repository, &checkout, &loaded.manifest)?;
    match mode {
        SnapshotMode::Check => check_snapshot_candidates(&candidates)?,
        SnapshotMode::Write => write_snapshot_candidates(&candidates)?,
    }
    verify_repository(repository)
}

fn preflight_snapshot(repository: &Path, loaded: &LoadedManifest) -> Result<()> {
    let mut failures = Vec::new();
    validate_manifest_header(repository, loaded, &mut failures);
    if loaded.manifest.upstream.is_none() {
        failures.push("snapshot reproduction requires an upstream table".to_owned());
    }
    if loaded.manifest.snapshot.is_empty() {
        failures.push("component has no snapshot specifications".to_owned());
    }

    let mut sources = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for snapshot in &loaded.manifest.snapshot {
        if !is_source_path(&snapshot.source) {
            failures.push(format!("invalid upstream source {:?}", snapshot.source));
        }
        if !sources.insert(snapshot.source.as_str()) {
            failures.push(format!("duplicate snapshot source {}", snapshot.source));
        }
        if !paths.insert(&snapshot.path) {
            failures.push(format!(
                "duplicate snapshot path {}",
                snapshot.path.display()
            ));
        }
        if let Err(error) = safe_snapshot_output(repository, &snapshot.path) {
            failures.push(error.to_string());
        }
    }

    let enumerator = repository.join(ENUMERATOR_PATH);
    if !enumerator.is_file() {
        failures.push(format!("enumerator {} is missing", enumerator.display()));
    }
    if !repository.join(TYPESCRIPT_PATH).is_dir() {
        failures.push(format!(
            "{TYPESCRIPT_PATH} is missing; run `npm ci --prefix tools/source-alignment`"
        ));
    }
    failures.sort();
    failures.dedup();
    finish_failures("snapshot preflight", failures)
}

fn generate_snapshot_candidates(
    repository: &Path,
    checkout: &Path,
    manifest: &Manifest,
) -> Result<Vec<SnapshotCandidate>> {
    let upstream = manifest
        .upstream
        .as_ref()
        .ok_or("snapshot reproduction requires an upstream table")?;
    let mut candidates = Vec::with_capacity(manifest.snapshot.len());

    for specification in &manifest.snapshot {
        let object = format!("{}:{}", upstream.commit, specification.source);
        let blob_oid = git_stdout(checkout, &["rev-parse", "--verify", &object])?;
        let blob_oid = String::from_utf8(blob_oid)?.trim().to_owned();
        if !is_full_git_oid(&blob_oid) {
            return Err(format!(
                "{} at {} did not resolve to a full Git object ID",
                specification.source, upstream.commit
            )
            .into());
        }
        let source = git_stdout(checkout, &["cat-file", "blob", &blob_oid])?;
        std::str::from_utf8(&source).map_err(|error| {
            format!(
                "{} at {} is not UTF-8: {error}",
                specification.source, upstream.commit
            )
        })?;
        let symbols = enumerate_typescript(repository, &specification.source, &source)?;
        let snapshot = UpstreamSnapshot {
            schema_version: SCHEMA_VERSION,
            repository: upstream.repository.clone(),
            commit: upstream.commit.clone(),
            source: specification.source.clone(),
            blob_oid,
            symbols,
        };
        let mut bytes = serde_json::to_vec_pretty(&snapshot)?;
        bytes.push(b'\n');
        candidates.push(SnapshotCandidate {
            relative_path: specification.path.clone(),
            output_path: safe_snapshot_output(repository, &specification.path)?,
            bytes,
        });
    }
    Ok(candidates)
}

fn git_stdout(checkout: &Path, arguments: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(checkout)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "git {} failed in {}: {}",
            arguments.join(" "),
            checkout.display(),
            stderr.trim()
        )
        .into());
    }
    Ok(output.stdout)
}

fn enumerate_typescript(repository: &Path, source: &str, text: &[u8]) -> Result<Vec<String>> {
    let enumerator = repository.join(ENUMERATOR_PATH);
    let mut child = Command::new("node")
        .arg(enumerator)
        .arg(source)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let write_result = child
        .stdin
        .take()
        .ok_or("failed to open enumerator stdin")?
        .write_all(text);
    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("failed to enumerate {source}: {}", stderr.trim()).into());
    }
    write_result?;
    let inventory: EnumeratorOutput = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("enumerator returned invalid JSON for {source}: {error}"))?;
    if inventory.symbols.is_empty() {
        return Err(format!("enumerator found no declarations in {source}").into());
    }
    if !is_sorted_unique(&inventory.symbols) {
        return Err(
            format!("enumerator declarations for {source} are not sorted and unique").into(),
        );
    }
    Ok(inventory.symbols)
}

fn check_snapshot_candidates(candidates: &[SnapshotCandidate]) -> Result<()> {
    let mut stale = Vec::new();
    for candidate in candidates {
        match fs::read(&candidate.output_path) {
            Ok(current) if current == candidate.bytes => {}
            Ok(_) => stale.push(candidate.relative_path.display().to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                stale.push(candidate.relative_path.display().to_string());
            }
            Err(error) => return Err(error.into()),
        }
    }
    if stale.is_empty() {
        println!("all source-alignment snapshots are reproducible");
        return Ok(());
    }
    Err(format!(
        "stale source-alignment snapshots: {}; rerun with --write",
        stale.join(", ")
    )
    .into())
}

fn write_snapshot_candidates(candidates: &[SnapshotCandidate]) -> Result<()> {
    for candidate in candidates {
        fs::write(&candidate.output_path, &candidate.bytes)?;
        println!("updated {}", candidate.relative_path.display());
    }
    Ok(())
}

fn safe_snapshot_output(repository: &Path, relative: &Path) -> Result<PathBuf> {
    if !is_snapshot_path(relative) {
        return Err(format!(
            "snapshot path {} must be below {SNAPSHOT_DIRECTORY}",
            relative.display()
        )
        .into());
    }
    let path = repository.join(relative);
    let parent = path
        .parent()
        .ok_or_else(|| format!("snapshot path {} has no parent", relative.display()))?;
    let parent = fs::canonicalize(parent).map_err(|error| {
        format!(
            "snapshot parent {} is unavailable: {error}",
            parent.display()
        )
    })?;
    if !parent.starts_with(repository) {
        return Err(format!(
            "snapshot path {} escapes the repository",
            relative.display()
        )
        .into());
    }
    if path
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(format!("snapshot path {} must not be a symlink", relative.display()).into());
    }
    Ok(repository.join(relative))
}

fn resolve_existing_directory(repository: &Path, relative: &Path, label: &str) -> Result<PathBuf> {
    resolve_existing_path(repository, relative, label, true)
}

fn resolve_existing_file(repository: &Path, relative: &Path, label: &str) -> Result<PathBuf> {
    resolve_existing_path(repository, relative, label, false)
}

fn resolve_existing_path(
    repository: &Path,
    relative: &Path,
    label: &str,
    directory: bool,
) -> Result<PathBuf> {
    if !is_repository_relative(relative) {
        return Err(format!("{label} {} must be repository-relative", relative.display()).into());
    }
    let path = fs::canonicalize(repository.join(relative))
        .map_err(|error| format!("{label} {} is unavailable: {error}", relative.display()))?;
    if !path.starts_with(repository) {
        return Err(format!("{label} {} escapes the repository", relative.display()).into());
    }
    if directory && !path.is_dir() {
        return Err(format!("{label} {} is not a directory", relative.display()).into());
    }
    if !directory && !path.is_file() {
        return Err(format!("{label} {} is not a file", relative.display()).into());
    }
    Ok(path)
}

fn finish_failures(context: &str, mut failures: Vec<String>) -> Result<()> {
    failures.sort();
    failures.dedup();
    if failures.is_empty() {
        return Ok(());
    }
    let mut message = format!("{context} failed:\n");
    for failure in failures {
        let _ = writeln!(message, "- {failure}");
    }
    Err(message.into())
}

fn is_repository_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, PathComponent::Normal(_)))
}

fn is_snapshot_path(path: &Path) -> bool {
    is_repository_relative(path) && path.starts_with(SNAPSHOT_DIRECTORY)
}

fn is_source_path(path: &str) -> bool {
    !path.is_empty() && !path.contains('\\') && is_repository_relative(Path::new(path))
}

fn is_full_git_oid(oid: &str) -> bool {
    oid.len() == 40
        && oid
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_sorted_unique(values: &[String]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "rezel-source-alignment-{label}-{}-{counter}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn rejects_unknown_manifest_fields() {
        let manifest = r#"
schema_version = 1
unknown = true

[component]
name = "example"
source_root = "src"

[[scope]]
rust_file = "lib.rs"
classification = "rezel-owned"
invariant = "stable"
representation = "native"
"#;
        assert!(toml::from_str::<Manifest>(manifest).is_err());
    }

    #[test]
    fn repository_paths_reject_escape_and_absolute_forms() {
        assert!(is_repository_relative(Path::new("alignment/example.toml")));
        assert!(!is_repository_relative(Path::new("../example.toml")));
        assert!(!is_repository_relative(Path::new("/tmp/example.toml")));
        assert!(!is_repository_relative(Path::new(".")));
    }

    #[test]
    fn discovers_repository_from_a_nested_directory() {
        let directory = TestDirectory::new("root");
        fs::write(directory.0.join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::create_dir(directory.0.join("alignment")).unwrap();
        fs::create_dir_all(directory.0.join("tools/source-alignment")).unwrap();
        fs::write(
            directory.0.join("tools/source-alignment/Cargo.toml"),
            "[package]\nname = \"tool\"\n",
        )
        .unwrap();
        let nested = directory.0.join("crates/example/src");
        fs::create_dir_all(&nested).unwrap();

        assert_eq!(
            discover_repository_from(&nested).unwrap(),
            fs::canonicalize(&directory.0).unwrap()
        );
    }

    #[test]
    fn declaration_inventory_includes_constants_types_and_methods() {
        let directory = TestDirectory::new("declarations");
        fs::write(
            directory.0.join("lib.rs"),
            r"
const LIMIT: usize = 1;
struct Parser;
impl Parser {
    fn parse(&self) {}
}
mod nested {
    pub type Offset = u32;
}
",
        )
        .unwrap();
        let files = BTreeSet::from([PathBuf::from("lib.rs")]);
        let declarations = enumerate_declarations(&directory.0, &files, "example").unwrap();

        assert!(declarations.contains_key("example::LIMIT"));
        assert!(declarations.contains_key("example::Parser"));
        assert!(declarations.contains_key("example::Parser::parse"));
        assert!(declarations.contains_key("example::nested::Offset"));
    }

    #[test]
    fn declaration_inventory_distinguishes_generic_trait_implementations() {
        let directory = TestDirectory::new("generic-trait-implementations");
        fs::write(
            directory.0.join("lib.rs"),
            r"
struct Value;
impl From<u8> for Value {
    fn from(_: u8) -> Self {
        Self
    }
}
impl From<Vec<u8>> for Value {
    fn from(_: Vec<u8>) -> Self {
        Self
    }
}
",
        )
        .unwrap();
        let files = BTreeSet::from([PathBuf::from("lib.rs")]);
        let declarations = enumerate_declarations(&directory.0, &files, "example").unwrap();

        assert!(declarations.contains_key("example::<Value as From<u8>>::from"));
        assert!(declarations.contains_key("example::<Value as From<Vec<u8>>>::from"));
    }

    #[test]
    fn validates_canonical_git_object_ids() {
        assert!(is_full_git_oid("de5f96276a2954c249de1475e8b03f79c20d9ce4"));
        assert!(!is_full_git_oid("DE5F96276A2954C249DE1475E8B03F79C20D9CE4"));
        assert!(!is_full_git_oid("de5f962"));
    }

    #[test]
    fn sorted_inventory_must_be_unique() {
        assert!(is_sorted_unique(&["A".to_owned(), "B".to_owned()]));
        assert!(!is_sorted_unique(&["A".to_owned(), "A".to_owned()]));
        assert!(!is_sorted_unique(&["B".to_owned(), "A".to_owned()]));
    }
}
