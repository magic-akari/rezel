use std::{
    collections::{BTreeMap, HashSet},
    env,
    error::Error,
    fmt, fs,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use serde::{Deserialize, Serialize};

const SCHEMA: &str = "rezel.parser-datasets.v1";
pub const GIT_CACHE_ENV: &str = "REZEL_PARSER_BENCHMARK_GIT_CACHE";
pub const SOURCES_ENV: &str = "REZEL_PARSER_BENCHMARK_SOURCES";

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Go,
    Java,
    Json,
    Kotlin,
    Python,
    Rust,
    Swift,
}

impl Language {
    pub const ALL: [Self; 7] = [
        Self::Go,
        Self::Java,
        Self::Json,
        Self::Kotlin,
        Self::Python,
        Self::Rust,
        Self::Swift,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Go => "go",
            Self::Java => "java",
            Self::Json => "json",
            Self::Kotlin => "kotlin",
            Self::Python => "python",
            Self::Rust => "rust",
            Self::Swift => "swift",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|language| language.name() == name)
    }
}

impl fmt::Display for Language {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Tier {
    #[serde(rename = "10kb")]
    Kb10,
    #[serde(rename = "50kb")]
    Kb50,
    #[serde(rename = "100kb")]
    Kb100,
    #[serde(rename = "500kb")]
    Kb500,
}

impl Tier {
    pub const ALL: [Self; 4] = [Self::Kb10, Self::Kb50, Self::Kb100, Self::Kb500];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Kb10 => "10kb",
            Self::Kb50 => "50kb",
            Self::Kb100 => "100kb",
            Self::Kb500 => "500kb",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tier| tier.name() == name)
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

#[derive(Debug, Deserialize)]
pub struct Manifest {
    schema: String,
    pub repository: Vec<Repository>,
    datasets: BTreeMap<String, DatasetGroup>,
}

#[derive(Debug, Deserialize)]
pub struct Repository {
    pub id: String,
    pub url: String,
    pub commit: String,
}

#[derive(Debug, Deserialize)]
struct DatasetGroup {
    pub repository: String,
    #[serde(flatten)]
    pub tiers: BTreeMap<String, Vec<String>>,
}

#[derive(Debug)]
pub(crate) struct SourceDataset {
    pub(crate) files: Vec<SourceFile>,
}

#[derive(Debug)]
pub(crate) struct SourceFile {
    pub(crate) path: String,
    pub(crate) source: Arc<str>,
}

#[derive(Debug)]
pub struct DatasetError(String);

impl DatasetError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for DatasetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for DatasetError {}

impl Manifest {
    /// Reads and validates the checked-in dataset manifest.
    ///
    /// # Errors
    ///
    /// Returns an error when the manifest cannot be read, decoded, or
    /// validated.
    pub fn load() -> Result<Self, DatasetError> {
        let path = manifest_path();
        Self::load_from(&path)
    }

    /// Reads and validates a dataset manifest at an explicit path.
    ///
    /// # Errors
    ///
    /// Returns an error when the manifest cannot be read, decoded, or
    /// validated.
    pub fn load_from(path: &Path) -> Result<Self, DatasetError> {
        let source = fs::read_to_string(path).map_err(|error| {
            DatasetError::new(format!(
                "failed to read dataset manifest {}: {error}",
                path.display()
            ))
        })?;
        let manifest = toml::from_str::<Self>(&source).map_err(|error| {
            DatasetError::new(format!(
                "failed to parse dataset manifest {}: {error}",
                path.display()
            ))
        })?;
        manifest.validate()?;
        Ok(manifest)
    }

    fn repository(&self, id: &str) -> Result<&Repository, DatasetError> {
        self.repository
            .iter()
            .find(|repository| repository.id == id)
            .ok_or_else(|| DatasetError::new(format!("unknown repository {id:?}")))
    }

    fn group(&self, language: Language) -> Result<&DatasetGroup, DatasetError> {
        self.datasets
            .get(language.name())
            .ok_or_else(|| DatasetError::new(format!("missing {language} datasets")))
    }

    /// Groups the fixed upstream paths by repository id.
    #[must_use]
    pub fn paths_by_repository(&self) -> BTreeMap<String, Vec<String>> {
        let mut paths = BTreeMap::<String, Vec<String>>::new();
        for group in self.datasets.values() {
            let repository_paths = paths.entry(group.repository.clone()).or_default();
            for tier_paths in group.tiers.values() {
                repository_paths.extend(tier_paths.iter().cloned());
            }
        }
        for repository_paths in paths.values_mut() {
            repository_paths.sort();
            repository_paths.dedup();
        }
        paths
    }

    fn validate(&self) -> Result<(), DatasetError> {
        if self.schema != SCHEMA {
            return Err(DatasetError::new(format!(
                "unsupported dataset schema {:?}",
                self.schema
            )));
        }

        let mut repository_ids = HashSet::new();
        for repository in &self.repository {
            validate_repository(repository)?;
            if !repository_ids.insert(repository.id.as_str()) {
                return Err(DatasetError::new(format!(
                    "duplicate repository id {:?}",
                    repository.id
                )));
            }
        }

        for language in Language::ALL {
            let group = self.group(language)?;
            self.repository(&group.repository)?;
            for tier in Tier::ALL {
                let paths = group.tiers.get(tier.name()).ok_or_else(|| {
                    DatasetError::new(format!("missing {language} {tier} dataset"))
                })?;
                if paths.is_empty() {
                    return Err(DatasetError::new(format!(
                        "{language} {tier} dataset is empty"
                    )));
                }
                let mut unique_paths = HashSet::new();
                for path in paths {
                    validate_source_path(path)?;
                    if !unique_paths.insert(path) {
                        return Err(DatasetError::new(format!(
                            "duplicate path in {language} {tier} dataset: {path}"
                        )));
                    }
                }
            }
            for tier in group.tiers.keys() {
                if !Tier::ALL.iter().any(|known| known.name() == tier) {
                    return Err(DatasetError::new(format!(
                        "unknown {language} dataset tier {tier:?}"
                    )));
                }
            }
        }
        for language in self.datasets.keys() {
            if !Language::ALL.iter().any(|known| known.name() == language) {
                return Err(DatasetError::new(format!(
                    "unknown dataset language {language:?}"
                )));
            }
        }
        Ok(())
    }
}

impl SourceDataset {
    pub(crate) fn load(language: Language, tier: Tier) -> Result<Self, DatasetError> {
        let manifest = Manifest::load()?;
        let group = manifest.group(language)?;
        let paths = group.tiers.get(tier.name()).ok_or_else(|| {
            DatasetError::new(format!("validated manifest lost {language} {tier}"))
        })?;
        let root = active_sources_root().join(&group.repository);
        let mut files = Vec::with_capacity(paths.len());
        for path in paths {
            let source_path = root.join(path);
            let source = fs::read_to_string(&source_path).map_err(|error| {
                DatasetError::new(format!(
                    "failed to read dataset source {}: {error}; run \
                     `mise run benchmark:datasets` first",
                    source_path.display()
                ))
            })?;
            files.push(SourceFile {
                path: path.clone(),
                source: Arc::from(source),
            });
        }
        Ok(Self { files })
    }
}

#[must_use]
pub fn manifest_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("datasets.toml")
}

#[must_use]
pub fn datasets_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("datasets")
}

#[must_use]
pub fn sources_root() -> PathBuf {
    datasets_root().join("sources")
}

#[must_use]
pub fn git_cache_root() -> PathBuf {
    datasets_root().join("git-cache")
}

#[must_use]
pub fn active_sources_root() -> PathBuf {
    env::var_os(SOURCES_ENV).map_or_else(sources_root, PathBuf::from)
}

#[must_use]
pub fn active_git_cache_root() -> PathBuf {
    env::var_os(GIT_CACHE_ENV).map_or_else(git_cache_root, PathBuf::from)
}

fn validate_repository(repository: &Repository) -> Result<(), DatasetError> {
    if repository.id.is_empty()
        || !repository
            .id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(DatasetError::new(format!(
            "invalid repository id {:?}",
            repository.id
        )));
    }
    if !repository.url.starts_with("https://") {
        return Err(DatasetError::new(format!(
            "repository {:?} must use an HTTPS URL",
            repository.id
        )));
    }
    if repository.commit.len() != 40
        || !repository
            .commit
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(DatasetError::new(format!(
            "repository {:?} has an invalid commit",
            repository.id
        )));
    }
    Ok(())
}

fn validate_source_path(path: &str) -> Result<(), DatasetError> {
    let path = Path::new(path);
    let valid = !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    if !valid {
        return Err(DatasetError::new(format!(
            "dataset source path must be relative and normalized: {}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Manifest;

    #[test]
    fn checked_in_manifest_is_valid() {
        Manifest::load().expect("checked-in dataset manifest must be valid");
    }
}
