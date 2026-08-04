#![forbid(unsafe_code)]

use std::{
    any::Any,
    error::Error,
    ffi::OsStr,
    fs::{self, File},
    io::{self, BufReader, Read},
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    process::{Command, Output},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use sha2::{Digest, Sha256};

const KOTLIN_VERSION: &str = "2.4.10";
const KOTLIN_BUILD: &str = "2.4.10-release-377";
const KOTLIN_SOURCE_ARCHIVE_SHA256: &str =
    "5c4fa84ec945a63453e64bc0141c222ece806a2026c2961fe76b0be483896323";
const KOTLIN_SOURCE_COUNT: usize = 380;
const KOTLIN_SOURCE_BYTES: usize = 4_469_330;
// SHA-256 over sorted `./relative/path.kt\n` records.
const KOTLIN_PATH_INVENTORY_SHA256: &str =
    "77974d389fdb6b48cec8d6a075c0043fe46321fadf90ae5cb81062b2cde9a92f";
// SHA-256 over sorted `<source-sha256>  ./relative/path.kt\n` records.
const KOTLIN_CONTENT_INVENTORY_SHA256: &str =
    "461b2f323a2b1bf439a27ac74985495b1f68c5b9c2bf58d07accebaa8d89e705";
const STDLIB_REJECTION_MANIFEST: &str = include_str!("../stdlib-rejections.txt");
const DISTRIBUTION_SOURCE_COUNT: usize = 1_299;
const DISTRIBUTION_SOURCE_BYTES: usize = 11_547_367;
const DISTRIBUTION_PATH_INVENTORY_SHA256: &str =
    "de2f142e732d2fa2117aa7c14b15c9ad7f2fb8548e809c91c4b78bab11e6549d";
const DISTRIBUTION_CONTENT_INVENTORY_SHA256: &str =
    "6441d4f32974b2786671823a6176ba4753cdc80bf1a31f2df1edec0d853c0ad6";
const DISTRIBUTION_REJECTION_MANIFEST: &str = include_str!("../distribution-rejections.txt");
const WORKER_STACK_SIZE: usize = 64 * 1024 * 1024;

struct ArchiveSpec {
    file: &'static str,
    sha256: &'static str,
    archive_bytes: usize,
    source_count: usize,
    source_bytes: usize,
}

const DISTRIBUTION_ARCHIVES: &[ArchiveSpec] = &[
    ArchiveSpec {
        file: "kotlin-annotations-jvm-sources.jar",
        sha256: "bf594d0e3a35c21b1f07097d9e10b6ab524ae5bb2ffdb92ea9b1f79f28e8ab09",
        archive_bytes: 4_005,
        source_count: 0,
        source_bytes: 0,
    },
    ArchiveSpec {
        file: "kotlin-metadata-jvm-sources.jar",
        sha256: "5ddad3d64871b6dbfdab3b5a65adb5541dcc6f71f5ef67036dfab4f7796b2e67",
        archive_bytes: 261_832,
        source_count: 59,
        source_bytes: 309_352,
    },
    ArchiveSpec {
        file: "kotlin-reflect-sources.jar",
        sha256: "ff246d790aad39f55779f44e410b477228999cd3c352585c73b56ea5df36549e",
        archive_bytes: 815_995,
        source_count: 416,
        source_bytes: 1_892_777,
    },
    ArchiveSpec {
        file: "kotlin-script-runtime-sources.jar",
        sha256: "275d90c0c92e9680aff9c013394f3f395bc46dc23762107c4fdccd9887a58561",
        archive_bytes: 9_933,
        source_count: 10,
        source_bytes: 14_164,
    },
    ArchiveSpec {
        file: "kotlin-stdlib-jdk7-sources.jar",
        sha256: "2534c8908432e06de73177509903d405b55f423dd4c2f747e16b92a2162611e6",
        archive_bytes: 580,
        source_count: 0,
        source_bytes: 0,
    },
    ArchiveSpec {
        file: "kotlin-stdlib-jdk8-sources.jar",
        sha256: "3cb6895054a0985bba591c165503fe4dd63a215af53263b67a071ccdc242bf6e",
        archive_bytes: 556,
        source_count: 0,
        source_bytes: 0,
    },
    ArchiveSpec {
        file: "kotlin-stdlib-js-sources.jar",
        sha256: "94500d86fe275691acf2e4148de5c037ef3a466eff4439291eb5cb2f0b5631d9",
        archive_bytes: 751_361,
        source_count: 401,
        source_bytes: 4_728_395,
    },
    ArchiveSpec {
        file: "kotlin-stdlib-sources.jar",
        sha256: "5c4fa84ec945a63453e64bc0141c222ece806a2026c2961fe76b0be483896323",
        archive_bytes: 757_671,
        source_count: 380,
        source_bytes: 4_469_330,
    },
    ArchiveSpec {
        file: "kotlin-test-js-sources.jar",
        sha256: "4613e172db7bde9cdb284a1b5a833ea20291104227894e34d3e8599be34c602f",
        archive_bytes: 18_785,
        source_count: 16,
        source_bytes: 65_254,
    },
    ArchiveSpec {
        file: "kotlin-test-junit-sources.jar",
        sha256: "3fd39bbf3f8283cf7b7759e02abdd752ddde9964352f645467b779257bccef20",
        archive_bytes: 3_238,
        source_count: 3,
        source_bytes: 5_770,
    },
    ArchiveSpec {
        file: "kotlin-test-junit5-sources.jar",
        sha256: "5e0d4688277a3f859be086b874601b45f54b30b5b045800138b54e9418bdc95b",
        archive_bytes: 3_258,
        source_count: 3,
        source_bytes: 5_832,
    },
    ArchiveSpec {
        file: "kotlin-test-sources.jar",
        sha256: "5872d95764647025277d207c29e9e8e816763a509f77ca24a3bea3241afb392f",
        archive_bytes: 11_990,
        source_count: 8,
        source_bytes: 50_757,
    },
    ArchiveSpec {
        file: "kotlin-test-testng-sources.jar",
        sha256: "b88a8f12928480d14e29c6c879958c156afc946b9b8d333d1c2f4a428cdb1041",
        archive_bytes: 3_241,
        source_count: 3,
        source_bytes: 5_736,
    },
];

type AnyError = Box<dyn Error + Send + Sync>;
type Result<T> = std::result::Result<T, AnyError>;

fn main() {
    if let Err(error) = entry() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn entry() -> Result<()> {
    let mut arguments = std::env::args_os();
    let _program = arguments.next();
    let mode = match (arguments.next(), arguments.next()) {
        (None, None) => Corpus::StandardLibrary,
        (Some(argument), None) if argument == "--distribution" => Corpus::Distribution,
        _ => {
            return Err(failure(
                "usage: rezel-kotlin-stdlib-reference [--distribution]",
            ));
        }
    };

    let worker = thread::Builder::new()
        .name("rezel-kotlin-stdlib-reference".to_owned())
        .stack_size(WORKER_STACK_SIZE)
        .spawn(move || verify_kotlin(mode))?;
    worker.join().map_err(|panic| {
        failure(format!(
            "Kotlin standard-library verifier panicked: {}",
            panic_message(panic.as_ref())
        ))
    })?
}

#[derive(Clone, Copy)]
enum Corpus {
    StandardLibrary,
    Distribution,
}

fn verify_kotlin(corpus: Corpus) -> Result<()> {
    let kotlin_home = kotlin_home()?;
    verify_kotlin_build(&kotlin_home)?;

    match corpus {
        Corpus::StandardLibrary => verify_standard_library(&kotlin_home),
        Corpus::Distribution => verify_distribution(&kotlin_home),
    }
}

fn verify_standard_library(kotlin_home: &Path) -> Result<()> {
    let source_archive = kotlin_home.join("lib").join("kotlin-stdlib-sources.jar");
    if !source_archive.is_file() {
        return Err(failure(format!(
            "Kotlin standard-library source archive is missing: {}",
            source_archive.display()
        )));
    }
    require_digest(
        "Kotlin standard-library source archive",
        &sha256_file(&source_archive)?,
        KOTLIN_SOURCE_ARCHIVE_SHA256,
    )?;

    let extracted = TemporaryDirectory::new("rezel-kotlin-stdlib-source")?;
    extract_archive(&source_archive, extracted.path())?;

    let sources = load_sources(extracted.path())?;
    verify_source_inventory(&sources)?;
    let rejection_count = verify_pinned_acceptance(
        "standard-library",
        &sources,
        &rejection_manifest(STDLIB_REJECTION_MANIFEST, "standard-library")?,
    )?;
    let accepted_count = KOTLIN_SOURCE_COUNT
        .checked_sub(rejection_count)
        .ok_or_else(|| failure("Kotlin standard-library rejection count overflow"))?;

    eprintln!(
        "accepted {accepted_count}/{KOTLIN_SOURCE_COUNT} Kotlin {KOTLIN_VERSION} \
         ({KOTLIN_BUILD}) standard-library sources ({KOTLIN_SOURCE_BYTES} UTF-8 bytes; \
         {rejection_count} pinned strict rejections)"
    );
    Ok(())
}

fn verify_distribution(kotlin_home: &Path) -> Result<()> {
    let extracted = TemporaryDirectory::new("rezel-kotlin-distribution-source")?;
    let mut sources = Vec::with_capacity(DISTRIBUTION_SOURCE_COUNT);

    for archive in DISTRIBUTION_ARCHIVES {
        let archive_path = kotlin_home.join("lib").join(archive.file);
        if !archive_path.is_file() {
            return Err(failure(format!(
                "Kotlin distribution source archive is missing: {}",
                archive_path.display()
            )));
        }
        let archive_bytes = usize::try_from(fs::metadata(&archive_path)?.len())?;
        require_count(
            &format!("{} archive byte", archive.file),
            archive_bytes,
            archive.archive_bytes,
        )?;
        require_digest(
            &format!("{} archive", archive.file),
            &sha256_file(&archive_path)?,
            archive.sha256,
        )?;

        let archive_root = extracted.path().join(archive.file);
        fs::create_dir(&archive_root)?;
        extract_archive(&archive_path, &archive_root)?;
        let archive_name = archive
            .file
            .strip_suffix(".jar")
            .ok_or_else(|| failure("Kotlin distribution source archive must end in .jar"))?;
        let prefix = format!("./{archive_name}/");
        let mut archive_sources = load_sources_with_prefix(&archive_root, &prefix, true)?;
        require_count(
            &format!("{} source", archive.file),
            archive_sources.len(),
            archive.source_count,
        )?;
        require_count(
            &format!("{} source byte", archive.file),
            source_bytes(&archive_sources)?,
            archive.source_bytes,
        )?;
        sources.append(&mut archive_sources);
    }

    sources.sort_by(|left, right| left.relative.as_bytes().cmp(right.relative.as_bytes()));
    verify_inventory(
        "Kotlin distribution",
        &sources,
        DISTRIBUTION_SOURCE_COUNT,
        DISTRIBUTION_SOURCE_BYTES,
        DISTRIBUTION_PATH_INVENTORY_SHA256,
        DISTRIBUTION_CONTENT_INVENTORY_SHA256,
    )?;
    let rejection_count = verify_pinned_acceptance(
        "distribution",
        &sources,
        &rejection_manifest(DISTRIBUTION_REJECTION_MANIFEST, "distribution")?,
    )?;
    let accepted_count = DISTRIBUTION_SOURCE_COUNT
        .checked_sub(rejection_count)
        .ok_or_else(|| failure("Kotlin distribution rejection count overflow"))?;

    eprintln!(
        "accepted {accepted_count} of {DISTRIBUTION_SOURCE_COUNT} Kotlin \
         {KOTLIN_VERSION} ({KOTLIN_BUILD}) distribution sources \
         ({DISTRIBUTION_SOURCE_BYTES} UTF-8 bytes; {rejection_count} pinned strict rejections)"
    );
    Ok(())
}

fn kotlin_home() -> Result<PathBuf> {
    let mut command = Command::new("mise");
    command.args(["where", "kotlin"]);
    let output = checked_output(&mut command, "resolve the mise-pinned Kotlin toolchain")?;
    let stdout = String::from_utf8(output.stdout)?;
    let mut roots = stdout.lines().filter(|line| !line.is_empty());
    let root = roots
        .next()
        .ok_or_else(|| failure("mise returned no Kotlin toolchain root; run `mise install`"))?;
    if roots.next().is_some() {
        return Err(failure("mise returned multiple Kotlin toolchain roots"));
    }
    Ok(Path::new(root).join("kotlinc"))
}

fn verify_kotlin_build(kotlin_home: &Path) -> Result<()> {
    let build_path = kotlin_home.join("build.txt");
    let build = fs::read_to_string(&build_path).map_err(|error| {
        failure(format!(
            "could not read Kotlin build identity {}: {error}",
            build_path.display()
        ))
    })?;
    if build == KOTLIN_BUILD {
        Ok(())
    } else {
        Err(failure(format!(
            "Kotlin build {KOTLIN_BUILD} is required, found {build:?}"
        )))
    }
}

fn extract_archive(archive: &Path, destination: &Path) -> Result<()> {
    let mut command = Command::new("jar");
    command
        .args(["--extract", "--file"])
        .arg(archive)
        .current_dir(destination);
    checked_output(
        &mut command,
        "extract the pinned Kotlin standard-library source archive",
    )?;
    Ok(())
}

struct Source {
    relative: String,
    text: String,
    content_sha256: String,
}

fn load_sources(root: &Path) -> Result<Vec<Source>> {
    load_sources_with_prefix(root, "./", false)
}

fn load_sources_with_prefix(
    root: &Path,
    relative_prefix: &str,
    include_scripts: bool,
) -> Result<Vec<Source>> {
    let mut paths = Vec::new();
    collect_kotlin_sources(root, include_scripts, &mut paths)?;

    let mut sources = Vec::with_capacity(paths.len());
    for path in paths {
        let relative_path = path.strip_prefix(root)?;
        let relative = format!("{relative_prefix}{}", slash_path(relative_path));
        let bytes = fs::read(&path)?;
        let content_sha256 = sha256_bytes(&bytes);
        let text = String::from_utf8(bytes).map_err(|error| {
            failure(format!(
                "Kotlin standard-library source is not UTF-8: {relative}: {error}"
            ))
        })?;
        sources.push(Source {
            relative,
            text,
            content_sha256,
        });
    }
    sources.sort_by(|left, right| left.relative.as_bytes().cmp(right.relative.as_bytes()));
    Ok(sources)
}

fn collect_kotlin_sources(
    directory: &Path,
    include_scripts: bool,
    paths: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_kotlin_sources(&entry.path(), include_scripts, paths)?;
        } else if file_type.is_file() {
            let path = entry.path();
            let extension = path.extension();
            let is_source = extension == Some(OsStr::new("kt"))
                || include_scripts && extension == Some(OsStr::new("kts"));
            if is_source {
                paths.push(path);
            }
        }
    }
    Ok(())
}

fn verify_source_inventory(sources: &[Source]) -> Result<()> {
    verify_inventory(
        "Kotlin standard-library",
        sources,
        KOTLIN_SOURCE_COUNT,
        KOTLIN_SOURCE_BYTES,
        KOTLIN_PATH_INVENTORY_SHA256,
        KOTLIN_CONTENT_INVENTORY_SHA256,
    )
}

fn verify_inventory(
    label: &str,
    sources: &[Source],
    expected_count: usize,
    expected_bytes: usize,
    expected_path_digest: &str,
    expected_content_digest: &str,
) -> Result<()> {
    require_count(&format!("{label} source"), sources.len(), expected_count)?;

    let mut path_inventory = Sha256::new();
    let mut content_inventory = Sha256::new();
    for source in sources {
        path_inventory.update(source.relative.as_bytes());
        path_inventory.update(b"\n");

        content_inventory.update(source.content_sha256.as_bytes());
        content_inventory.update(b"  ");
        content_inventory.update(source.relative.as_bytes());
        content_inventory.update(b"\n");
    }

    require_count(
        &format!("{label} source byte"),
        source_bytes(sources)?,
        expected_bytes,
    )?;
    require_digest(
        &format!("{label} path inventory"),
        &digest_hex(path_inventory.finalize()),
        expected_path_digest,
    )?;
    require_digest(
        &format!("{label} content inventory"),
        &digest_hex(content_inventory.finalize()),
        expected_content_digest,
    )
}

fn source_bytes(sources: &[Source]) -> Result<usize> {
    sources.iter().try_fold(0_usize, |total, source| {
        total
            .checked_add(source.text.len())
            .ok_or_else(|| failure("Kotlin source byte count overflow"))
    })
}

#[derive(Debug, Eq, PartialEq)]
struct StrictRejection {
    relative: String,
    byte: Option<usize>,
    kind: String,
}

fn verify_pinned_acceptance(
    corpus: &str,
    sources: &[Source],
    expected: &[StrictRejection],
) -> Result<usize> {
    let parser = rezel_lang_kotlin::parser().with_strict(true);
    let mut rejections = Vec::new();
    let mut panics = Vec::new();
    let mut accepted = 0_usize;
    for source in sources {
        match catch_unwind(AssertUnwindSafe(|| parser.parse(&source.text))) {
            Ok(Ok(_)) => accepted += 1,
            Ok(Err(error)) => rejections.push(StrictRejection {
                relative: source.relative.clone(),
                byte: error.position().map(usize::from),
                kind: format!("{:?}", error.kind()),
            }),
            Err(panic) => panics.push(format!(
                "{}: parser panicked: {}",
                source.relative,
                panic_message(panic.as_ref())
            )),
        }
    }

    if !panics.is_empty() {
        return Err(format_failures(
            &format!("strict Kotlin parser panicked on the pinned {corpus} inventory"),
            &panics,
        ));
    }

    if rejections != expected {
        let actual_manifest = format_rejection_manifest(&rejections);
        return Err(failure(format!(
            "Kotlin {corpus} strict rejection inventory changed\n\
             expected {} entries, found {}\n\
             actual manifest:\n{actual_manifest}",
            expected.len(),
            rejections.len()
        )));
    }
    let expected_accepted = sources
        .len()
        .checked_sub(expected.len())
        .ok_or_else(|| failure(format!("Kotlin {corpus} rejection count overflow")))?;
    require_count(
        &format!("accepted Kotlin {corpus} source"),
        accepted,
        expected_accepted,
    )?;
    Ok(expected.len())
}

fn rejection_manifest(source: &str, corpus: &str) -> Result<Vec<StrictRejection>> {
    let mut rejections = Vec::new();
    for (index, line) in source.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split('\t');
        let relative = fields.next();
        let byte = fields.next();
        let kind = fields.next();
        if relative.is_none() || byte.is_none() || kind.is_none() || fields.next().is_some() {
            return Err(failure(format!(
                "invalid Kotlin {corpus} rejection manifest line {}",
                index + 1
            )));
        }
        let byte = byte
            .expect("manifest byte field was checked")
            .parse::<usize>()
            .map_err(|error| {
                failure(format!(
                    "invalid byte on Kotlin {corpus} rejection manifest line {}: {error}",
                    index + 1
                ))
            })?;
        rejections.push(StrictRejection {
            relative: relative
                .expect("manifest path field was checked")
                .to_owned(),
            byte: Some(byte),
            kind: kind.expect("manifest kind field was checked").to_owned(),
        });
    }
    Ok(rejections)
}

fn format_rejection_manifest(rejections: &[StrictRejection]) -> String {
    rejections
        .iter()
        .map(|rejection| {
            format!(
                "{}\t{}\t{}",
                rejection.relative,
                rejection
                    .byte
                    .map_or_else(|| "none".to_owned(), |byte| byte.to_string()),
                rejection.kind
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest_hex(digest.finalize()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    digest_hex(Sha256::digest(bytes))
}

fn digest_hex(digest: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = digest.as_ref();
    let mut hexadecimal = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hexadecimal.push(char::from(HEX[usize::from(byte >> 4)]));
        hexadecimal.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    hexadecimal
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

fn require_digest(label: &str, actual: &str, expected: &str) -> Result<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(failure(format!(
            "{label} digest changed: expected {expected}, found {actual}"
        )))
    }
}

fn format_failures(message: &str, failures: &[String]) -> AnyError {
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

fn panic_message(panic: &(dyn Any + Send)) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = panic.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_owned()
    }
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
