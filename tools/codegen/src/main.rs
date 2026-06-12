#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use rezel_generator::{
    BuildOptions, CompiledGrammar, RustBindings, compile_grammar, emit_rust_with_data_paths,
    emit_typed_syntax,
};

const USAGE: &str =
    "Usage: rezel-codegen (json | java | go | python | rust | fixtures) (--check | --update)";

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn main() {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if let Err(error) = run(&arguments) {
        eprintln!("rezel-codegen: {error}");
        std::process::exit(1);
    }
}

fn run(arguments: &[String]) -> Result<()> {
    let [scope, mode] = arguments else {
        return Err(USAGE.into());
    };
    let scope = Scope::parse(scope).ok_or(USAGE)?;
    let mode = Mode::parse(mode).ok_or(USAGE)?;
    let regeneration_command = scope.regeneration_command();
    let root = workspace_root()?;
    let mut outputs = Outputs::new(&root, mode, &regeneration_command);
    match scope {
        Scope::Language(language) => {
            generate_language(&root, language, &regeneration_command, &mut outputs)?;
        }
        Scope::Fixtures => {
            generate_fixtures(&root, &regeneration_command, &mut outputs)?;
        }
    }
    outputs.finish()
}

fn generate_language(
    root: &Path,
    language: &str,
    regeneration_command: &str,
    outputs: &mut Outputs<'_>,
) -> Result<()> {
    let language_root = root.join("languages").join(language);
    let grammar_path = language_root
        .join("grammar")
        .join(format!("{language}.grammar"));
    let bindings_path = language_root
        .join("grammar")
        .join(format!("{language}.bindings.toml"));
    let typed_path = language_root
        .join("grammar")
        .join(format!("{language}.typed.toml"));
    let grammar_source = fs::read_to_string(&grammar_path)?;
    let source_name = relative_name(root, &grammar_path);
    let grammar = compile_grammar(
        &grammar_source,
        Some(&source_name),
        BuildOptions {
            include_names: true,
        },
    )?;
    reject_warnings(language, &grammar)?;
    let bindings_source = fs::read_to_string(bindings_path)?;
    let bindings = RustBindings::from_toml_str(&bindings_source)?;
    let generated =
        emit_rust_with_data_paths(&grammar, &bindings, "generated.le.bin", "generated.be.bin")?;
    let source_root = language_root.join("src");
    outputs.manage_scoped_source_directory(&source_root, regeneration_command);
    let parser = annotated(&generated.parser, regeneration_command)?;
    outputs.emit(&source_root.join("generated.rs"), &parser)?;
    let terms = annotated(&generated.terms, regeneration_command)?;
    outputs.emit(&source_root.join("terms.rs"), &terms)?;
    outputs.emit_bytes(
        &source_root.join("generated.le.bin"),
        &generated.little_endian_data,
    )?;
    outputs.emit_bytes(
        &source_root.join("generated.be.bin"),
        &generated.big_endian_data,
    )?;

    let typed_schema = fs::read_to_string(typed_path)?;
    let typed = emit_typed_syntax(&grammar, &typed_schema)?;
    let typed = annotated(&typed, regeneration_command)?;
    outputs.emit(&source_root.join("typed.rs"), &typed)?;
    Ok(())
}

fn workspace_root() -> Result<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .ok_or("codegen tool must live under the workspace tools directory")?;
    Ok(root.to_path_buf())
}

fn generate_fixtures(
    root: &Path,
    regeneration_command: &str,
    outputs: &mut Outputs<'_>,
) -> Result<()> {
    generate_upstream_cases(root, regeneration_command, outputs)?;
    generate_test_parse_fixtures(root, regeneration_command, outputs)
}

fn generate_upstream_cases(
    root: &Path,
    regeneration_command: &str,
    outputs: &mut Outputs<'_>,
) -> Result<()> {
    let input_root = root.join("crates/rezel-generator/tests/upstream/cases");
    let output_root = root.join("crates/rezel-generator/tests/generated/cases");
    outputs.manage_generated_directory(&output_root);
    let mut cases = Vec::new();
    for path in files_with_extension(&input_root, "txt")? {
        let name = utf8_file_stem(&path)?;
        let source = fs::read_to_string(&path)?;
        let grammar_source = split_case_file(&source).0;
        if expected_diagnostic(grammar_source).is_some() {
            continue;
        }
        let grammar = compile_grammar(grammar_source, Some(name), BuildOptions::default())?;
        reject_warnings(name, &grammar)?;
        let stem = snake_case(name);
        let little_endian_name = format!("{stem}.le.bin");
        let big_endian_name = format!("{stem}.be.bin");
        let bindings = case_bindings(&path)?;
        let generated =
            emit_rust_with_data_paths(&grammar, &bindings, &little_endian_name, &big_endian_name)?;
        let parser = annotated(&generated.parser, regeneration_command)?;
        outputs.emit(&output_root.join(format!("{stem}.rs")), &parser)?;
        let terms = annotated(&generated.terms, regeneration_command)?;
        outputs.emit(&output_root.join(format!("{stem}_terms.rs")), &terms)?;
        outputs.emit_bytes(
            &output_root.join(little_endian_name),
            &generated.little_endian_data,
        )?;
        outputs.emit_bytes(
            &output_root.join(big_endian_name),
            &generated.big_endian_data,
        )?;
        cases.push((name.to_owned(), stem));
    }
    let registry = case_registry_source(&cases, regeneration_command)?;
    let registry_path = root.join("crates/rezel-generator/tests/support/case_registry.rs");
    outputs.emit(&registry_path, &registry)?;
    Ok(())
}

fn split_case_file(source: &str) -> (&str, &str) {
    source
        .find("\n# ")
        .map_or((source, ""), |boundary| source.split_at(boundary))
}

fn expected_diagnostic(grammar: &str) -> Option<&str> {
    grammar.lines().find_map(|line| {
        let (_, message) = line.split_once("//! ")?;
        Some(message.trim())
    })
}

fn generate_test_parse_fixtures(
    root: &Path,
    regeneration_command: &str,
    outputs: &mut Outputs<'_>,
) -> Result<()> {
    let input_root = root.join("crates/rezel-generator/tests/fixtures/test_parse");
    let output_root = root.join("crates/rezel-generator/tests/generated/test_parse");
    outputs.manage_generated_directory(&output_root);
    for path in files_with_extension(&input_root, "grammar")? {
        let name = utf8_file_stem(&path)?;
        let source = fs::read_to_string(&path)?;
        let source_name = format!("test/test-parse.ts::{name}");
        let grammar = compile_grammar(&source, Some(&source_name), BuildOptions::default())?;
        reject_warnings(name, &grammar)?;
        let little_endian_name = format!("{name}.le.bin");
        let big_endian_name = format!("{name}.be.bin");
        let generated = emit_rust_with_data_paths(
            &grammar,
            &RustBindings::default(),
            &little_endian_name,
            &big_endian_name,
        )?;
        let parser = annotated(&generated.parser, regeneration_command)?;
        outputs.emit(&output_root.join(format!("{name}.rs")), &parser)?;
        outputs.emit_bytes(
            &output_root.join(little_endian_name),
            &generated.little_endian_data,
        )?;
        outputs.emit_bytes(
            &output_root.join(big_endian_name),
            &generated.big_endian_data,
        )?;
    }
    Ok(())
}

fn annotated(source: &str, regeneration_command: &str) -> Result<String> {
    let (marker, body) = source
        .split_once('\n')
        .ok_or("rezel-generator output has no generated marker")?;
    if !marker.starts_with("// @generated by rezel-generator.") {
        return Err("rezel-generator output has an invalid generated marker".into());
    }

    let command = format!("// Regenerate with: {regeneration_command}\n");
    let capacity = marker.len() + 1 + command.len() + body.len();
    let mut annotated = String::with_capacity(capacity);
    annotated.push_str(marker);
    annotated.push('\n');
    annotated.push_str(&command);
    annotated.push_str(body);
    Ok(annotated)
}

fn codegen_source(body: &str, regeneration_command: &str) -> String {
    let marker = "// @generated by rezel-codegen. Do not edit manually.\n";
    let command = format!("// Regenerate with: {regeneration_command}\n");
    let mut source = String::with_capacity(marker.len() + command.len() + body.len());
    source.push_str(marker);
    source.push_str(&command);
    source.push_str(body);
    source
}

fn reject_warnings(name: &str, grammar: &CompiledGrammar) -> Result<()> {
    if grammar.warnings.is_empty() {
        return Ok(());
    }
    let warnings = grammar
        .warnings
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    Err(format!("{name} generated warnings:\n{warnings}").into())
}

fn files_with_extension(directory: &Path, extension: &str) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_file() && path.extension().is_some_and(|value| value == extension)
        {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn utf8_file_stem(path: &Path) -> Result<&str> {
    path.file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("path has no UTF-8 file stem: {}", path.display()).into())
}

fn relative_name(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn snake_case(name: &str) -> String {
    let mut output = String::new();
    for character in name.chars() {
        if character.is_ascii_uppercase() {
            if !output.is_empty() {
                output.push('_');
            }
            output.push(character.to_ascii_lowercase());
        } else {
            output.push(character);
        }
    }
    output
}

fn case_bindings(case_path: &Path) -> Result<RustBindings> {
    let bindings_path = case_path.with_extension("bindings.toml");
    let manifest = match fs::read_to_string(&bindings_path) {
        Ok(manifest) => manifest,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RustBindings::default());
        }
        Err(error) => return Err(error.into()),
    };
    Ok(RustBindings::from_toml_str(&manifest)?)
}

fn case_registry_source(cases: &[(String, String)], regeneration_command: &str) -> Result<String> {
    let mut body = String::from(
        r"pub struct GeneratedCase {
    pub name: &'static str,
    pub language: &'static rezel_lr::Language,
}

macro_rules! generated_cases {
    ($($name:literal => $module:ident @ $path:literal;)*) => {
        $(
            #[path = $path]
            #[rustfmt::skip]
            pub mod $module;
        )*

        pub static CASES: &[GeneratedCase] = &[
            $(
                GeneratedCase {
                    name: $name,
                    language: &$module::LANGUAGE,
                },
            )*
        ];
    };
}

generated_cases! {
",
    );
    for (name, stem) in cases {
        writeln!(
            body,
            "    {name:?} => {stem} @ \"../generated/cases/{stem}.rs\";"
        )?;
    }
    body.push_str("}\n");
    Ok(codegen_source(&body, regeneration_command))
}

#[derive(Clone, Copy)]
enum Scope {
    Language(&'static str),
    Fixtures,
}

impl Scope {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "json" => Some(Self::Language("json")),
            "java" => Some(Self::Language("java")),
            "go" => Some(Self::Language("go")),
            "python" => Some(Self::Language("python")),
            "rust" => Some(Self::Language("rust")),
            "fixtures" => Some(Self::Fixtures),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Language(language) => language,
            Self::Fixtures => "fixtures",
        }
    }

    fn regeneration_command(self) -> String {
        format!("mise run codegen:rezel:{}:update", self.name())
    }
}

#[derive(Clone, Copy)]
enum Mode {
    Check,
    Update,
}

impl Mode {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "--check" => Some(Self::Check),
            "--update" => Some(Self::Update),
            _ => None,
        }
    }
}

struct Outputs<'a> {
    root: &'a Path,
    mode: Mode,
    regeneration_command: &'a str,
    expected: BTreeMap<PathBuf, Vec<u8>>,
    managed_directories: BTreeSet<PathBuf>,
    scoped_source_directories: BTreeMap<PathBuf, String>,
}

impl<'a> Outputs<'a> {
    fn new(root: &'a Path, mode: Mode, regeneration_command: &'a str) -> Self {
        Self {
            root,
            mode,
            regeneration_command,
            expected: BTreeMap::new(),
            managed_directories: BTreeSet::new(),
            scoped_source_directories: BTreeMap::new(),
        }
    }

    fn manage_generated_directory(&mut self, path: &Path) {
        self.managed_directories.insert(path.to_path_buf());
    }

    fn manage_scoped_source_directory(&mut self, path: &Path, regeneration_command: &str) {
        self.scoped_source_directories
            .insert(path.to_path_buf(), regeneration_command.to_owned());
    }

    fn emit(&mut self, path: &Path, source: &str) -> Result<()> {
        self.emit_bytes(path, source.as_bytes())
    }

    fn emit_bytes(&mut self, path: &Path, expected: &[u8]) -> Result<()> {
        if let Some(previous) = self.expected.insert(path.to_path_buf(), expected.to_vec())
            && previous != expected
        {
            return Err(format!("conflicting generated output for {}", path.display()).into());
        }
        Ok(())
    }

    fn changed_outputs(&self) -> Result<Vec<PathBuf>> {
        let mut changed = Vec::new();
        for (path, expected) in &self.expected {
            let current = match fs::read(path) {
                Ok(current) => Some(current),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            };
            if current.as_deref() != Some(expected) {
                changed.push(path.clone());
            }
        }
        Ok(changed)
    }

    fn orphaned_outputs(&self) -> Result<Vec<PathBuf>> {
        let mut orphans = BTreeSet::new();
        for directory in &self.managed_directories {
            self.collect_orphans(directory, &mut orphans, |_| Ok(true))?;
        }
        for (directory, command) in &self.scoped_source_directories {
            self.collect_orphans(directory, &mut orphans, |path| {
                source_belongs_to_command(path, command)
            })?;
        }
        Ok(orphans.into_iter().collect())
    }

    fn collect_orphans(
        &self,
        directory: &Path,
        orphans: &mut BTreeSet<PathBuf>,
        belongs_to_scope: impl Fn(&Path) -> Result<bool>,
    ) -> Result<()> {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            if self.expected.contains_key(&path) || !belongs_to_scope(&path)? {
                continue;
            }
            orphans.insert(path);
        }
        Ok(())
    }

    fn stale_error(&self, changed: &[PathBuf], orphans: &[PathBuf]) -> Box<dyn Error> {
        let paths = changed
            .iter()
            .chain(orphans)
            .map(|path| self.relative(path))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|path| format!("  {}", path.display()))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "generated outputs are stale:\n{paths}\nrun `{}`",
            self.regeneration_command
        )
        .into()
    }

    fn apply_updates(&self, changed: &[PathBuf], orphans: &[PathBuf]) -> Result<()> {
        let mut staged = Vec::with_capacity(changed.len());
        for (index, path) in changed.iter().enumerate() {
            let expected = self
                .expected
                .get(path)
                .ok_or_else(|| format!("missing generated candidate for {}", path.display()))?;
            staged.push(StagedOutput::new(path, expected, index)?);
        }

        for output in staged {
            let path = output.target().to_path_buf();
            output.commit()?;
            println!("updated {}", self.relative(&path).display());
        }
        for path in orphans {
            fs::remove_file(path)?;
            println!("removed {}", self.relative(path).display());
        }
        Ok(())
    }

    fn relative(&self, path: &Path) -> PathBuf {
        path.strip_prefix(self.root).unwrap_or(path).to_path_buf()
    }

    fn finish(self) -> Result<()> {
        let changed = self.changed_outputs()?;
        let orphans = self.orphaned_outputs()?;
        if changed.is_empty() && orphans.is_empty() {
            match self.mode {
                Mode::Check => println!("all generated Rezel sources are current"),
                Mode::Update => println!("updated 0 generated outputs"),
            }
            return Ok(());
        }

        match self.mode {
            Mode::Check => Err(self.stale_error(&changed, &orphans)),
            Mode::Update => {
                let updated = changed.len() + orphans.len();
                self.apply_updates(&changed, &orphans)?;
                println!("updated {updated} generated outputs");
                Ok(())
            }
        }
    }
}

fn source_belongs_to_command(path: &Path, regeneration_command: &str) -> Result<bool> {
    if path.extension().is_none_or(|extension| extension != "rs") {
        return Ok(false);
    }
    let source = fs::read_to_string(path)?;
    let mut lines = source.lines();
    let marker = lines.next().unwrap_or_default();
    let command = lines.next().unwrap_or_default();
    let expected_command = format!("// Regenerate with: {regeneration_command}");
    Ok(marker.starts_with("// @generated by ") && command == expected_command)
}

struct StagedOutput {
    target: PathBuf,
    temporary: PathBuf,
    committed: bool,
}

impl StagedOutput {
    fn new(target: &Path, contents: &[u8], index: usize) -> Result<Self> {
        let parent = target
            .parent()
            .ok_or_else(|| format!("generated output has no parent: {}", target.display()))?;
        fs::create_dir_all(parent)?;
        let name = target
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("generated output has no UTF-8 name: {}", target.display()))?;
        for attempt in 0..100 {
            let temporary = parent.join(format!(
                ".{name}.rezel-codegen-{}-{index}-{attempt}.tmp",
                std::process::id()
            ));
            let mut file = match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            };
            if let Err(error) = file.write_all(contents).and_then(|()| file.sync_all()) {
                drop(file);
                let _ = fs::remove_file(&temporary);
                return Err(error.into());
            }
            return Ok(Self {
                target: target.to_path_buf(),
                temporary,
                committed: false,
            });
        }
        Err(format!(
            "could not allocate a temporary file beside {}",
            target.display()
        )
        .into())
    }

    fn target(&self) -> &Path {
        &self.target
    }

    fn commit(mut self) -> Result<()> {
        fs::rename(&self.temporary, &self.target)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for StagedOutput {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        if let Err(error) = fs::remove_file(&self.temporary)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "failed to remove temporary generated output {}: {error}",
                self.temporary.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    struct TemporaryDirectory(PathBuf);

    impl TemporaryDirectory {
        fn new() -> Self {
            let suffix = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let name = format!("rezel-codegen-{}-{suffix}", std::process::id());
            let path = env::temp_dir().join(name);
            fs::create_dir(&path).expect("temporary test directory can be created");
            Self(path)
        }
    }

    impl Drop for TemporaryDirectory {
        fn drop(&mut self) {
            if let Err(error) = fs::remove_dir_all(&self.0) {
                eprintln!(
                    "failed to remove temporary directory {}: {error}",
                    self.0.display()
                );
            }
        }
    }

    #[test]
    fn adds_the_repository_command_after_the_generator_marker() {
        let source = "// @generated by rezel-generator. Do not edit manually.\nfn generated() {}\n";
        let command = "mise run codegen:rezel:fixtures:update";
        let annotated = annotated(source, command).unwrap();
        assert_eq!(
            annotated,
            concat!(
                "// @generated by rezel-generator. Do not edit manually.\n",
                "// Regenerate with: mise run codegen:rezel:fixtures:update\n",
                "fn generated() {}\n",
            )
        );
    }

    #[test]
    fn reports_and_removes_orphans_in_managed_directories() {
        let temporary = TemporaryDirectory::new();
        let generated = temporary.0.join("generated");
        let orphan = generated.join("orphan.rs");
        fs::create_dir(&generated).unwrap();
        fs::write(&orphan, "stale").unwrap();
        let command = "mise run codegen:rezel:fixtures:update";

        let mut check = Outputs::new(&temporary.0, Mode::Check, command);
        check.manage_generated_directory(&generated);
        let error = check.finish().unwrap_err();
        assert!(error.to_string().contains("generated/orphan.rs"));
        assert!(error.to_string().contains(command));
        assert!(orphan.exists());

        let mut update = Outputs::new(&temporary.0, Mode::Update, command);
        update.manage_generated_directory(&generated);
        update.finish().unwrap();
        assert!(!orphan.exists());
    }

    #[test]
    fn defers_repository_writes_until_all_outputs_are_ready() {
        let temporary = TemporaryDirectory::new();
        let output = temporary.0.join("generated.rs");
        fs::write(&output, "old").unwrap();
        let command = "mise run codegen:rezel:fixtures:update";

        let mut abandoned = Outputs::new(&temporary.0, Mode::Update, command);
        abandoned.emit(&output, "abandoned").unwrap();
        assert_eq!(fs::read_to_string(&output).unwrap(), "old");
        drop(abandoned);
        assert_eq!(fs::read_to_string(&output).unwrap(), "old");

        let mut committed = Outputs::new(&temporary.0, Mode::Update, command);
        committed.emit(&output, "new").unwrap();
        assert_eq!(fs::read_to_string(&output).unwrap(), "old");
        committed.finish().unwrap();
        assert_eq!(fs::read_to_string(&output).unwrap(), "new");
    }

    #[test]
    fn scoped_directories_reconcile_only_their_own_generated_sources() {
        let temporary = TemporaryDirectory::new();
        let source_root = temporary.0.join("src");
        fs::create_dir(&source_root).unwrap();
        let command = "mise run codegen:rezel:json:update";
        let owned = source_root.join("obsolete.rs");
        let foreign = source_root.join("foreign.rs");
        let handwritten = source_root.join("handwritten.rs");
        fs::write(
            &owned,
            format!(
                "// @generated by rezel-generator. Do not edit manually.\n\
                 // Regenerate with: {command}\n"
            ),
        )
        .unwrap();
        fs::write(
            &foreign,
            "// @generated by another-generator. Do not edit manually.\n\
             // Regenerate with: mise run codegen:another:update\n",
        )
        .unwrap();
        fs::write(&handwritten, "fn handwritten() {}\n").unwrap();

        let mut check = Outputs::new(&temporary.0, Mode::Check, command);
        check.manage_scoped_source_directory(&source_root, command);
        let error = check.finish().unwrap_err().to_string();
        assert!(error.contains("src/obsolete.rs"));
        assert!(!error.contains("src/foreign.rs"));
        assert!(!error.contains("src/handwritten.rs"));

        let mut update = Outputs::new(&temporary.0, Mode::Update, command);
        update.manage_scoped_source_directory(&source_root, command);
        update.finish().unwrap();
        assert!(!owned.exists());
        assert!(foreign.exists());
        assert!(handwritten.exists());
    }
}
