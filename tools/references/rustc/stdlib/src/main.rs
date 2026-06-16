#![forbid(unsafe_code)]

use std::{
    error::Error,
    ffi::OsStr,
    fs, io,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    process::{Command, Output},
    thread,
};

const RUST_RELEASE: &str = "1.95.0";
const RUST_COMMIT: &str = "59807616e1fa2540724bfbac14d7976d7e4a3860";
const RUST_SOURCE_COUNT: usize = 1_964;
const WORKER_STACK_SIZE: usize = 64 * 1024 * 1024;

const RUST_KNOWN_REJECTIONS: &[(&str, usize)] = &[];

type AnyError = Box<dyn Error + Send + Sync>;
type Result<T> = std::result::Result<T, AnyError>;

fn main() {
    if let Err(error) = entry() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn entry() -> Result<()> {
    if std::env::args_os().len() != 1 {
        return Err(failure("usage: rezel-rust-stdlib-reference"));
    }

    let worker = thread::Builder::new()
        .name("rezel-rust-stdlib-reference".to_owned())
        .stack_size(WORKER_STACK_SIZE)
        .spawn(verify_rust)?;
    worker
        .join()
        .map_err(|_| failure("Rust standard-library verifier panicked"))?
}

fn verify_rust() -> Result<()> {
    let source_root = rust_source_root()?;
    let sources = collect_rust_sources(&source_root)?;
    require_count(
        "Rust standard-library source",
        sources.len(),
        RUST_SOURCE_COUNT,
    )?;

    let parser = rezel_lang_rust::parser().with_strict(true);
    let mut panics = Vec::new();
    let mut rejected = Vec::new();
    let mut rejection_details = Vec::new();
    for (index, path) in sources.iter().enumerate() {
        let source = fs::read_to_string(path)?;
        let relative = slash_path(path.strip_prefix(&source_root)?);
        match catch_unwind(AssertUnwindSafe(|| parser.parse(&source))) {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => {
                let line = error
                    .position()
                    .map(|position| line_number(&source, usize::from(position)))
                    .transpose()?;
                rejection_details.push(format!("{relative}:{}: {error}", line.unwrap_or(0)));
                rejected.push((relative, line));
            }
            Err(_) => panics.push(relative),
        }
        if (index + 1) % 250 == 0 {
            eprintln!(
                "Rezel Rust standard-library sources: {}/{}",
                index + 1,
                sources.len()
            );
        }
    }

    if !panics.is_empty() {
        return Err(failure(format!(
            "strict Rust parser panicked on standard-library sources:\n{}",
            panics.join("\n")
        )));
    }

    let expected = RUST_KNOWN_REJECTIONS
        .iter()
        .map(|(path, line)| ((*path).to_owned(), Some(*line)))
        .collect::<Vec<_>>();
    if rejected != expected {
        return Err(failure(format!(
            "strict Rust parser rejection inventory changed\nexpected ({}):\n{}\nactual ({}):\n{}",
            expected.len(),
            format_rejections(&expected),
            rejected.len(),
            rejection_details.join("\n")
        )));
    }

    let accepted = RUST_SOURCE_COUNT - rejected.len();
    eprintln!(
        "accepted {accepted} Rust {RUST_RELEASE} standard-library sources; {} known CST limitations remain",
        rejected.len()
    );
    Ok(())
}

fn rust_source_root() -> Result<PathBuf> {
    let mut version_command = Command::new("rustc");
    version_command.args(["--version", "--verbose"]);
    let output = checked_output(&mut version_command, "inspect the pinned Rust toolchain")?;
    let version = String::from_utf8(output.stdout)?;
    let release = version
        .lines()
        .find_map(|line| line.strip_prefix("release: "))
        .ok_or_else(|| failure("rustc version output has no release"))?;
    let commit = version
        .lines()
        .find_map(|line| line.strip_prefix("commit-hash: "))
        .ok_or_else(|| failure("rustc version output has no commit hash"))?;
    if release != RUST_RELEASE || commit != RUST_COMMIT {
        return Err(failure(format!(
            "Rust {RUST_RELEASE} ({RUST_COMMIT}) is required, found {release} ({commit})"
        )));
    }

    let mut sysroot_command = Command::new("rustc");
    sysroot_command.args(["--print", "sysroot"]);
    let output = checked_output(&mut sysroot_command, "resolve the pinned Rust sysroot")?;
    let sysroot = String::from_utf8(output.stdout)?;
    let source_root = Path::new(sysroot.trim())
        .join("lib")
        .join("rustlib")
        .join("src")
        .join("rust")
        .join("library");
    if !source_root.is_dir() {
        return Err(failure(format!(
            "Rust source component is missing: {}; install rust-src for {RUST_RELEASE}",
            source_root.display()
        )));
    }
    Ok(source_root)
}

fn collect_rust_sources(root: &Path) -> Result<Vec<PathBuf>> {
    let mut pending = vec![root.to_owned()];
    let mut sources = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() && entry.path().extension() == Some(OsStr::new("rs")) {
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
        .map(|(path, line)| format!("{path}:{}", line.unwrap_or(0)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn failure(message: impl Into<String>) -> AnyError {
    io::Error::other(message.into()).into()
}
