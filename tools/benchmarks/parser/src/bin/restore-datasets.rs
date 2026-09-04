#![forbid(unsafe_code)]

use std::{
    env,
    error::Error,
    ffi::OsStr,
    fs,
    path::Path,
    process::{Command, Output},
};

use rezel_parser_benchmark::{
    backends::{rezel, tree_sitter},
    datasets::{Language, Manifest, Tier, active_git_cache_root, active_sources_root},
};

fn main() -> Result<(), Box<dyn Error>> {
    let languages = selected_languages()?;
    let manifest = Manifest::load()?;
    let paths_by_repository = manifest.paths_by_repository_for(languages.iter().copied())?;
    fs::create_dir_all(active_git_cache_root())?;
    fs::create_dir_all(active_sources_root())?;

    let mut restored = 0_usize;
    for repository in &manifest.repository {
        let Some(paths) = paths_by_repository.get(&repository.id) else {
            continue;
        };
        let cache = active_git_cache_root().join(&repository.id);
        ensure_bare_repository(&cache)?;
        ensure_origin(&cache, &repository.url)?;
        ensure_commit(&cache, &repository.commit)?;

        for path in paths {
            let object = format!("{}:{path}", repository.commit);
            let output = git(&cache, ["show", object.as_str()])?;
            let destination = active_sources_root().join(&repository.id).join(path);
            let parent = destination
                .parent()
                .expect("validated dataset path has a parent");
            fs::create_dir_all(parent)?;
            fs::write(&destination, output.stdout)?;
            restored += 1;
        }
        println!(
            "restored {} files from {} at {}",
            paths.len(),
            repository.id,
            &repository.commit[..12]
        );
    }

    println!("restored {restored} dataset files");
    validate_datasets(&languages);
    Ok(())
}

fn selected_languages() -> Result<Vec<Language>, Box<dyn Error>> {
    let mut arguments = env::args().skip(1);
    let Some(flag) = arguments.next() else {
        return Ok(Language::ALL.to_vec());
    };
    if flag != "--language" {
        return Err(format!("unknown argument {flag:?}; expected --language NAME").into());
    }
    let name = arguments
        .next()
        .ok_or("--language requires a language name")?;
    if arguments.next().is_some() {
        return Err("unexpected arguments after --language NAME".into());
    }
    let language =
        Language::from_name(&name).ok_or_else(|| format!("unknown benchmark language {name:?}"))?;
    Ok(vec![language])
}

fn validate_datasets(languages: &[Language]) {
    for &language in languages {
        for tier in Tier::ALL {
            drop(rezel::setup(language, tier));
            drop(tree_sitter::setup(language, tier));
        }
    }
    println!("validated selected datasets with Rezel and Tree-sitter");
}

fn ensure_bare_repository(cache: &Path) -> Result<(), Box<dyn Error>> {
    if cache.join("HEAD").is_file() {
        return Ok(());
    }
    if cache.exists() {
        return Err(format!(
            "dataset Git cache exists but is not a bare repository: {}",
            cache.display()
        )
        .into());
    }
    let output = Command::new("git")
        .args(["init", "--bare"])
        .arg(cache)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    require_success(
        output,
        &format!("initialize bare cache {}", cache.display()),
    )?;
    Ok(())
}

fn ensure_origin(cache: &Path, url: &str) -> Result<(), Box<dyn Error>> {
    let current = git_allow_failure(cache, ["remote", "get-url", "origin"])?;
    if current.status.success() {
        let current = String::from_utf8(current.stdout)?;
        if current.trim() != url {
            git(cache, ["remote", "set-url", "origin", url])?;
        }
    } else {
        git(cache, ["remote", "add", "origin", url])?;
    }
    git(cache, ["config", "remote.origin.promisor", "true"])?;
    git(
        cache,
        ["config", "remote.origin.partialclonefilter", "blob:none"],
    )?;
    Ok(())
}

fn ensure_commit(cache: &Path, commit: &str) -> Result<(), Box<dyn Error>> {
    let commit_object = format!("{commit}^{{commit}}");
    let present = git_allow_failure(cache, ["cat-file", "-e", commit_object.as_str()])?;
    if present.status.success() {
        return Ok(());
    }
    git(
        cache,
        [
            "fetch",
            "--no-tags",
            "--depth=1",
            "--filter=blob:none",
            "origin",
            commit,
        ],
    )?;
    Ok(())
}

fn git<I, S>(cache: &Path, arguments: I) -> Result<Output, Box<dyn Error>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = git_allow_failure(cache, arguments)?;
    require_success(output, &format!("run Git in {}", cache.display()))
}

fn git_allow_failure<I, S>(cache: &Path, arguments: I) -> Result<Output, Box<dyn Error>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Ok(Command::new("git")
        .arg("--git-dir")
        .arg(cache)
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?)
}

fn require_success(output: Output, operation: &str) -> Result<Output, Box<dyn Error>> {
    if output.status.success() {
        return Ok(output);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!("{operation} failed: {}", stderr.trim()).into())
}
