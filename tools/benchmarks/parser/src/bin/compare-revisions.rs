#![forbid(unsafe_code)]

use std::{
    env,
    error::Error,
    ffi::OsString,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use rezel_parser_benchmark::{
    comparison::{
        BackendManifest, BackendSpec, ComparisonResults, Environment, RunMetadata,
        load_current_measurements, render_report,
    },
    datasets::{GIT_CACHE_ENV, SOURCES_ENV, active_git_cache_root},
};

const BENCHMARK_PATH: &str = "tools/benchmarks/parser";

type AnyError = Box<dyn Error>;

#[derive(Debug)]
struct Args {
    head: String,
    base: String,
    output_dir: Option<PathBuf>,
}

fn main() -> Result<(), AnyError> {
    let args = parse_args()?;
    if env::consts::OS != "linux" {
        return Err(failure(
            "commit comparison requires Linux, Valgrind, and gungraun-runner",
        ));
    }

    let repository = repository_root()?;
    let head = resolve_revision(&repository, &args.head)?;
    let base = resolve_revision(&repository, &args.base)?;
    let scratch = tempfile::Builder::new()
        .prefix("rezel-parser-comparison-")
        .tempdir()?;
    let head_snapshot = scratch.path().join("head");
    let base_snapshot = scratch.path().join("base");

    println!("preparing HEAD {}", short_sha(&head));
    archive_revision(&repository, &head, &head_snapshot, scratch.path(), "head")?;
    println!("preparing base {}", short_sha(&base));
    archive_revision(&repository, &base, &base_snapshot, scratch.path(), "base")?;
    install_head_harness(&repository, &head, &base_snapshot, scratch.path())?;

    let head_benchmark = head_snapshot.join(BENCHMARK_PATH);
    let backend_manifest = BackendManifest::load_from(&head_benchmark.join("backends.toml"))?;
    let environment = inspect_environment(&backend_manifest.gungraun)?;

    let output_root = args.output_dir.unwrap_or_else(default_output_root);
    let output = create_output_directory(&output_root, &head, &base)?;
    let sources = scratch.path().join("sources");
    run_restore_datasets(
        &head_snapshot,
        &scratch.path().join("target-restore"),
        &sources,
        &active_git_cache_root(),
        &output.join("datasets.log"),
    )?;
    let sources = fs::canonicalize(sources)?;
    let gungraun_home = output.join("gungraun");
    fs::create_dir_all(&gungraun_home)?;
    let baseline = baseline_name(&base);

    let primary = backend_manifest.primary()?;
    println!("measuring base {} with {}", short_sha(&base), primary.label);
    run_backend(RunBackend {
        snapshot: &base_snapshot,
        target: &scratch.path().join("target-base"),
        gungraun_home: &gungraun_home,
        sources: &sources,
        backend: primary,
        mode: RunMode::SaveBaseline(&baseline),
        log: &output.join(format!("base-{}.log", primary.id)),
    })?;

    for backend in &backend_manifest.backend {
        println!("measuring HEAD {} with {}", short_sha(&head), backend.label);
        run_backend(RunBackend {
            snapshot: &head_snapshot,
            target: &scratch.path().join("target-head"),
            gungraun_home: &gungraun_home,
            sources: &sources,
            backend,
            mode: RunMode::Compare(&baseline),
            log: &output.join(format!("head-{}.log", backend.id)),
        })?;
    }

    let measurements = load_current_measurements(&gungraun_home, &backend_manifest)?;
    let results =
        ComparisonResults::new(head, base, backend_manifest.primary.clone(), measurements);
    write_json(&output.join("results.json"), &results)?;
    write_json(
        &output.join("metadata.json"),
        &RunMetadata::new(&results, &backend_manifest.backend, &environment),
    )?;
    let report = render_report(&results, &backend_manifest, &environment)?;
    fs::write(output.join("report.md"), report)?;

    println!(
        "comparison complete: {} normalized measurements",
        results.measurements.len()
    );
    println!("report: {}", output.join("report.md").display());
    Ok(())
}

#[derive(Clone, Copy)]
enum RunMode<'a> {
    Compare(&'a str),
    SaveBaseline(&'a str),
}

#[derive(Clone, Copy)]
struct RunBackend<'a> {
    snapshot: &'a Path,
    target: &'a Path,
    gungraun_home: &'a Path,
    sources: &'a Path,
    backend: &'a BackendSpec,
    mode: RunMode<'a>,
    log: &'a Path,
}

fn run_backend(run: RunBackend<'_>) -> Result<(), AnyError> {
    let manifest = run.snapshot.join(BENCHMARK_PATH).join("Cargo.toml");
    let home_argument = format!("--home={}", utf8_path(run.gungraun_home)?);
    let sources_argument = format!("--envs={SOURCES_ENV}={}", utf8_path(run.sources)?);
    let mode_argument = match run.mode {
        RunMode::Compare(baseline) => format!("--baseline={baseline}"),
        RunMode::SaveBaseline(baseline) => format!("--save-baseline={baseline}"),
    };

    let mut command = Command::new("cargo");
    command
        .current_dir(run.snapshot)
        .args(["bench", "--quiet", "--locked", "--manifest-path"])
        .arg(manifest)
        .args(["--bench", &run.backend.bench, "--"])
        .args([home_argument, mode_argument]);
    if matches!(run.mode, RunMode::Compare(_)) {
        command.arg("--save-summary=json");
    }
    command
        .arg(sources_argument)
        .arg(&run.backend.filter)
        .env("CARGO_TARGET_DIR", run.target)
        .env("GUNGRAUN_COLOR", "never");
    run_logged(&mut command, run.log)
}

fn run_restore_datasets(
    snapshot: &Path,
    target: &Path,
    sources: &Path,
    git_cache: &Path,
    log: &Path,
) -> Result<(), AnyError> {
    let manifest = snapshot.join(BENCHMARK_PATH).join("Cargo.toml");
    let mut command = Command::new("cargo");
    command
        .current_dir(snapshot)
        .args(["run", "--quiet", "--locked", "--manifest-path"])
        .arg(manifest)
        .args(["--bin", "restore-datasets"])
        .env("CARGO_TARGET_DIR", target)
        .env(SOURCES_ENV, sources)
        .env(GIT_CACHE_ENV, git_cache);
    run_logged(&mut command, log)
}

fn parse_args() -> Result<Args, AnyError> {
    let mut head = "HEAD".to_owned();
    let mut base = "HEAD^".to_owned();
    let mut output_dir = None;
    let mut arguments = env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--head") => head = next_utf8(&mut arguments, "--head")?,
            Some("--base") => base = next_utf8(&mut arguments, "--base")?,
            Some("--output-dir") => {
                output_dir = Some(PathBuf::from(next_os(&mut arguments, "--output-dir")?));
            }
            Some("-h" | "--help") => {
                println!("Usage: compare-revisions [--head REF] [--base REF] [--output-dir PATH]");
                std::process::exit(0);
            }
            _ => {
                return Err(failure(format!(
                    "unknown argument {}",
                    argument.to_string_lossy()
                )));
            }
        }
    }
    Ok(Args {
        head,
        base,
        output_dir,
    })
}

fn next_utf8(
    arguments: &mut impl Iterator<Item = OsString>,
    option: &str,
) -> Result<String, AnyError> {
    let value = next_os(arguments, option)?;
    value
        .into_string()
        .map_err(|_| failure(format!("{option} requires UTF-8")))
}

fn next_os(
    arguments: &mut impl Iterator<Item = OsString>,
    option: &str,
) -> Result<OsString, AnyError> {
    arguments
        .next()
        .ok_or_else(|| failure(format!("{option} requires a value")))
}

fn repository_root() -> Result<PathBuf, AnyError> {
    let output = command_output(
        Command::new("git")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(["rev-parse", "--show-toplevel"]),
        "locate repository root",
    )?;
    Ok(PathBuf::from(output))
}

fn resolve_revision(repository: &Path, reference: &str) -> Result<String, AnyError> {
    let commit = format!("{reference}^{{commit}}");
    let output = command_output(
        Command::new("git")
            .current_dir(repository)
            .args(["rev-parse", "--verify", &commit]),
        &format!("resolve revision {reference:?}"),
    )?;
    if output.len() != 40 || !output.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(failure(format!(
            "Git returned an invalid commit for {reference:?}: {output:?}"
        )));
    }
    Ok(output)
}

fn archive_revision(
    repository: &Path,
    revision: &str,
    destination: &Path,
    scratch: &Path,
    name: &str,
) -> Result<(), AnyError> {
    fs::create_dir_all(destination)?;
    let archive = scratch.join(format!("{name}.tar"));
    let mut git = Command::new("git");
    git.current_dir(repository)
        .args(["archive", "--format=tar", "--output"])
        .arg(&archive)
        .arg(revision);
    require_success(&mut git, &format!("archive revision {revision}"))?;
    extract_archive(&archive, destination)
}

fn install_head_harness(
    repository: &Path,
    head: &str,
    base_snapshot: &Path,
    scratch: &Path,
) -> Result<(), AnyError> {
    let destination = base_snapshot.join(BENCHMARK_PATH);
    if destination.exists() {
        fs::remove_dir_all(&destination)?;
    }
    let archive = scratch.join("harness.tar");
    let mut git = Command::new("git");
    git.current_dir(repository)
        .args(["archive", "--format=tar", "--output"])
        .arg(&archive)
        .arg(head)
        .arg(BENCHMARK_PATH);
    require_success(&mut git, "archive HEAD benchmark harness")?;
    extract_archive(&archive, base_snapshot)
}

fn extract_archive(archive: &Path, destination: &Path) -> Result<(), AnyError> {
    let mut tar = Command::new("tar");
    tar.args(["-xf"]).arg(archive).args(["-C"]).arg(destination);
    require_success(&mut tar, &format!("extract archive {}", archive.display()))
}

fn inspect_environment(expected_gungraun: &str) -> Result<Environment, AnyError> {
    let runner =
        env::var_os("GUNGRAUN_RUNNER").unwrap_or_else(|| OsString::from("gungraun-runner"));
    let gungraun_runner = command_output(
        Command::new(&runner).arg("--version"),
        "inspect gungraun-runner",
    )?;
    if !gungraun_runner
        .split_ascii_whitespace()
        .any(|part| part == expected_gungraun)
    {
        return Err(failure(format!(
            "gungraun-runner {expected_gungraun} is required, found {gungraun_runner:?}"
        )));
    }
    Ok(Environment {
        system: command_output(Command::new("uname").args(["-s", "-m"]), "inspect system")?,
        rustc: command_output(Command::new("rustc").arg("--version"), "inspect rustc")?,
        cargo: command_output(Command::new("cargo").arg("--version"), "inspect cargo")?,
        gungraun_runner,
        valgrind: command_output(
            Command::new("valgrind").arg("--version"),
            "inspect Valgrind",
        )?,
    })
}

fn create_output_directory(root: &Path, head: &str, base: &str) -> Result<PathBuf, AnyError> {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let name = format!(
        "{}-vs-{}-{timestamp}-{}",
        short_sha(head),
        short_sha(base),
        std::process::id()
    );
    let output = root.join(name);
    fs::create_dir_all(&output)?;
    Ok(fs::canonicalize(output)?)
}

fn default_output_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("comparisons")
}

fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<(), AnyError> {
    let mut json = serde_json::to_string_pretty(value)?;
    json.push('\n');
    fs::write(path, json)?;
    Ok(())
}

fn command_output(command: &mut Command, operation: &str) -> Result<String, AnyError> {
    let output = command.output()?;
    if !output.status.success() {
        return Err(failure(format!(
            "{operation} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn require_success(command: &mut Command, operation: &str) -> Result<(), AnyError> {
    let output = command.output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(failure(format!(
            "{operation} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn run_logged(command: &mut Command, log_path: &Path) -> Result<(), AnyError> {
    println!("running {command:?}");
    let log = Arc::new(Mutex::new(File::create(log_path)?));
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().expect("configured child stdout pipe");
    let stderr = child.stderr.take().expect("configured child stderr pipe");
    let stdout_thread = copy_stream(stdout, false, Arc::clone(&log));
    let stderr_thread = copy_stream(stderr, true, log);
    let status = child.wait()?;
    join_copy(stdout_thread)?;
    join_copy(stderr_thread)?;
    if status.success() {
        Ok(())
    } else {
        Err(failure(format!(
            "benchmark command failed with status {status}; see {}",
            log_path.display()
        )))
    }
}

fn copy_stream<R: Read + Send + 'static>(
    mut stream: R,
    stderr: bool,
    log: Arc<Mutex<File>>,
) -> thread::JoinHandle<io::Result<()>> {
    thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            let count = stream.read(&mut buffer)?;
            if count == 0 {
                return Ok(());
            }
            {
                let mut log = log
                    .lock()
                    .map_err(|_| io::Error::other("benchmark log lock was poisoned"))?;
                log.write_all(&buffer[..count])?;
            }
            if stderr {
                io::stderr().write_all(&buffer[..count])?;
                io::stderr().flush()?;
            } else {
                io::stdout().write_all(&buffer[..count])?;
                io::stdout().flush()?;
            }
        }
    })
}

fn join_copy(handle: thread::JoinHandle<io::Result<()>>) -> Result<(), AnyError> {
    handle
        .join()
        .map_err(|_| failure("benchmark output thread panicked"))??;
    Ok(())
}

fn utf8_path(path: &Path) -> Result<&str, AnyError> {
    path.to_str()
        .ok_or_else(|| failure(format!("path is not UTF-8: {}", path.display())))
}

fn short_sha(sha: &str) -> &str {
    &sha[..12]
}

fn baseline_name(base: &str) -> String {
    format!("parent_{}", short_sha(base))
}

fn failure(message: impl Into<String>) -> AnyError {
    io::Error::other(message.into()).into()
}

#[cfg(test)]
mod tests {
    use super::baseline_name;

    #[test]
    fn creates_a_valid_gungraun_baseline_name() {
        let name = baseline_name("194a5ae02f6e0af543fa0f12b10fbd8f754f451d");

        assert_eq!(name, "parent_194a5ae02f6e");
        assert!(
            name.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        );
    }
}
