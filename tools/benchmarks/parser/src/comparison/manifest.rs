use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::datasets::Language;

use super::ComparisonError;

const SCHEMA: &str = "rezel.parser-backends.v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    Binary,
    Library,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendScope {
    Parse,
}

#[derive(Clone, Debug, Serialize)]
pub struct BackendSpec {
    pub id: String,
    pub label: String,
    pub bench: String,
    pub function: String,
    pub filter: String,
    pub kind: BackendKind,
    pub scope: BackendScope,
    pub languages: Vec<Language>,
}

impl BackendSpec {
    #[must_use]
    pub fn supports(&self, language: Language) -> bool {
        self.languages.contains(&language)
    }
}

#[derive(Debug)]
pub struct BackendManifest {
    pub primary: String,
    pub gungraun: String,
    pub backend: Vec<BackendSpec>,
}

#[derive(Debug, Deserialize)]
struct RawManifest {
    schema: String,
    primary: String,
    gungraun: String,
    backend: Vec<RawBackend>,
}

#[derive(Debug, Deserialize)]
struct RawBackend {
    id: String,
    label: String,
    bench: String,
    function: String,
    filter: String,
    kind: BackendKind,
    scope: BackendScope,
    languages: Vec<String>,
}

impl BackendManifest {
    /// Reads and validates a backend manifest.
    ///
    /// # Errors
    ///
    /// Returns an error when the manifest cannot be read, decoded, or
    /// validated.
    pub fn load_from(path: &Path) -> Result<Self, ComparisonError> {
        let source = fs::read_to_string(path).map_err(|error| {
            ComparisonError::new(format!(
                "failed to read backend manifest {}: {error}",
                path.display()
            ))
        })?;
        let raw = toml::from_str::<RawManifest>(&source).map_err(|error| {
            ComparisonError::new(format!(
                "failed to parse backend manifest {}: {error}",
                path.display()
            ))
        })?;
        Self::from_raw(raw)
    }

    /// Reads and validates the checked-in backend manifest.
    ///
    /// # Errors
    ///
    /// Returns an error when the manifest cannot be read, decoded, or
    /// validated.
    pub fn load() -> Result<Self, ComparisonError> {
        Self::load_from(&backends_manifest_path())
    }

    /// Returns the configured primary backend.
    ///
    /// # Errors
    ///
    /// Returns an error if a manifest bypassed validation.
    pub fn primary(&self) -> Result<&BackendSpec, ComparisonError> {
        self.backend
            .iter()
            .find(|backend| backend.id == self.primary)
            .ok_or_else(|| ComparisonError::new("validated manifest lost its primary backend"))
    }

    #[must_use]
    pub fn by_function(&self, function: &str) -> Option<&BackendSpec> {
        self.backend
            .iter()
            .find(|backend| backend.function == function)
    }

    fn from_raw(raw: RawManifest) -> Result<Self, ComparisonError> {
        if raw.schema != SCHEMA {
            return Err(ComparisonError::new(format!(
                "unsupported backend schema {:?}",
                raw.schema
            )));
        }
        if raw.backend.is_empty() {
            return Err(ComparisonError::new("backend manifest is empty"));
        }
        if raw.gungraun.is_empty()
            || !raw
                .gungraun
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.')
        {
            return Err(ComparisonError::new(format!(
                "invalid Gungraun version {:?}",
                raw.gungraun
            )));
        }

        let backends = validate_backends(raw.backend)?;

        let primary = backends
            .iter()
            .find(|backend| backend.id == raw.primary)
            .ok_or_else(|| {
                ComparisonError::new(format!("unknown primary backend {:?}", raw.primary))
            })?;
        if primary.languages != Language::ALL {
            return Err(ComparisonError::new(format!(
                "primary backend {:?} must support every language",
                primary.id
            )));
        }

        Ok(Self {
            primary: raw.primary,
            gungraun: raw.gungraun,
            backend: backends,
        })
    }
}

fn validate_backends(raw_backends: Vec<RawBackend>) -> Result<Vec<BackendSpec>, ComparisonError> {
    let mut ids = BTreeSet::new();
    let mut functions = BTreeSet::new();
    let mut backends = Vec::with_capacity(raw_backends.len());
    for raw in raw_backends {
        validate_name("backend id", &raw.id, true)?;
        validate_name("bench target", &raw.bench, true)?;
        validate_name("benchmark function", &raw.function, false)?;
        if raw.label.trim().is_empty() {
            return Err(ComparisonError::new(format!(
                "backend {:?} has an empty label",
                raw.id
            )));
        }
        if raw.filter.trim().is_empty() {
            return Err(ComparisonError::new(format!(
                "backend {:?} has an empty filter",
                raw.id
            )));
        }
        if !ids.insert(raw.id.clone()) {
            return Err(ComparisonError::new(format!(
                "duplicate backend id {:?}",
                raw.id
            )));
        }
        if !functions.insert(raw.function.clone()) {
            return Err(ComparisonError::new(format!(
                "duplicate benchmark function {:?}",
                raw.function
            )));
        }
        let languages = validate_languages(&raw)?;
        backends.push(BackendSpec {
            id: raw.id,
            label: raw.label,
            bench: raw.bench,
            function: raw.function,
            filter: raw.filter,
            kind: raw.kind,
            scope: raw.scope,
            languages,
        });
    }
    Ok(backends)
}

fn validate_languages(raw: &RawBackend) -> Result<Vec<Language>, ComparisonError> {
    let mut languages = Vec::with_capacity(raw.languages.len());
    for language in &raw.languages {
        let language = Language::from_name(language).ok_or_else(|| {
            ComparisonError::new(format!(
                "backend {:?} has unknown language {language:?}",
                raw.id
            ))
        })?;
        if languages.contains(&language) {
            return Err(ComparisonError::new(format!(
                "backend {:?} repeats language {language}",
                raw.id
            )));
        }
        languages.push(language);
    }
    if languages.is_empty() {
        return Err(ComparisonError::new(format!(
            "backend {:?} supports no languages",
            raw.id
        )));
    }
    languages.sort_unstable();
    Ok(languages)
}

#[must_use]
pub fn backends_manifest_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("backends.toml")
}

fn validate_name(kind: &str, name: &str, allow_hyphen: bool) -> Result<(), ComparisonError> {
    let valid = !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || byte == b'_'
                || (allow_hyphen && byte == b'-')
        });
    if valid {
        Ok(())
    } else {
        Err(ComparisonError::new(format!("invalid {kind} {name:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::BackendManifest;

    #[test]
    fn checked_in_manifest_is_valid() {
        BackendManifest::load().expect("checked-in backend manifest must be valid");
    }
}
