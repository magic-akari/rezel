use std::{collections::BTreeSet, fs, path::Path};

use serde::Deserialize;

use crate::datasets::{Language, Tier};

use super::{BackendManifest, ComparisonError, Measurement, Revision};

const SUMMARY_SCHEMA: &str = "6";

#[derive(Debug, Deserialize)]
struct GungraunSummary {
    version: String,
    function_name: String,
    id: String,
    profiles: Vec<Profile>,
}

#[derive(Debug, Deserialize)]
struct Profile {
    tool: String,
    summaries: ProfileSummaries,
}

#[derive(Debug, Deserialize)]
struct ProfileSummaries {
    total: TotalSummary,
}

#[derive(Debug, Deserialize)]
struct TotalSummary {
    summary: CallgrindSummary,
}

#[derive(Debug, Deserialize)]
struct CallgrindSummary {
    #[serde(rename = "Callgrind")]
    callgrind: CallgrindMetrics,
}

#[derive(Debug, Deserialize)]
struct CallgrindMetrics {
    #[serde(rename = "Ir")]
    instructions: Metric,
}

#[derive(Debug, Deserialize)]
struct Metric {
    metrics: MetricValues,
}

#[derive(Debug, Deserialize)]
struct MetricValues {
    #[serde(rename = "Both")]
    both: Option<Vec<InstructionValue>>,
    #[serde(rename = "Left")]
    left: Option<InstructionValue>,
    #[serde(rename = "Right")]
    right: Option<InstructionValue>,
}

#[derive(Debug, Deserialize)]
struct InstructionValue {
    #[serde(rename = "Int")]
    value: u64,
}

/// Loads current measurements and the primary backend's comparison baseline.
///
/// # Errors
///
/// Returns an error when summaries are missing, duplicated, malformed, or do
/// not match the backend manifest.
pub fn load_current_measurements(
    root: &Path,
    manifest: &BackendManifest,
) -> Result<Vec<Measurement>, ComparisonError> {
    let summary_paths = find_summary_files(root)?;
    if summary_paths.is_empty() {
        return Err(ComparisonError::new(format!(
            "no Gungraun summary files found under {}",
            root.display()
        )));
    }

    let mut measurements = Vec::new();
    let mut keys = BTreeSet::new();
    for path in summary_paths {
        for measurement in parse_summary(&path, manifest)? {
            insert_measurement(
                &mut measurements,
                &mut keys,
                measurement.revision,
                &measurement.backend,
                measurement.language,
                measurement.tier,
                measurement.instructions,
            )?;
        }
    }

    validate_expected_measurements(&measurements, manifest)?;
    Ok(measurements)
}

fn parse_summary(
    path: &Path,
    manifest: &BackendManifest,
) -> Result<Vec<Measurement>, ComparisonError> {
    let source = fs::read_to_string(path).map_err(|error| {
        ComparisonError::new(format!(
            "failed to read Gungraun summary {}: {error}",
            path.display()
        ))
    })?;
    let summary = serde_json::from_str::<GungraunSummary>(&source).map_err(|error| {
        ComparisonError::new(format!(
            "failed to parse Gungraun summary {}: {error}",
            path.display()
        ))
    })?;
    if summary.version != SUMMARY_SCHEMA {
        return Err(ComparisonError::new(format!(
            "unsupported Gungraun summary schema {:?} in {}",
            summary.version,
            path.display()
        )));
    }
    let backend = manifest
        .by_function(&summary.function_name)
        .ok_or_else(|| {
            ComparisonError::new(format!(
                "unknown benchmark function {:?} in {}",
                summary.function_name,
                path.display()
            ))
        })?;
    let (language, tier) = parse_case_id(&summary.id).ok_or_else(|| {
        ComparisonError::new(format!(
            "invalid benchmark case id {:?} in {}",
            summary.id,
            path.display()
        ))
    })?;
    if !backend.supports(language) {
        return Err(ComparisonError::new(format!(
            "backend {:?} produced undeclared language {language}",
            backend.id
        )));
    }
    let profile = summary
        .profiles
        .iter()
        .find(|profile| profile.tool == "Callgrind")
        .ok_or_else(|| {
            ComparisonError::new(format!("missing Callgrind profile in {}", path.display()))
        })?;
    measurements_from_metric(
        &profile
            .summaries
            .total
            .summary
            .callgrind
            .instructions
            .metrics,
        manifest,
        &backend.id,
        language,
        tier,
    )
}

fn measurements_from_metric(
    values: &MetricValues,
    manifest: &BackendManifest,
    backend: &str,
    language: Language,
    tier: Tier,
) -> Result<Vec<Measurement>, ComparisonError> {
    if values.right.is_some() {
        return Err(ComparisonError::new(format!(
            "backend {backend:?} only exists in the baseline for {language} {tier}"
        )));
    }
    if backend == manifest.primary {
        let both = values.both.as_ref().ok_or_else(|| {
            ComparisonError::new(format!(
                "primary backend {backend:?} has no HEAD/base pair for {language} {tier}"
            ))
        })?;
        let [head, base] = both.as_slice() else {
            return Err(ComparisonError::new(format!(
                "primary backend {backend:?} has an invalid HEAD/base pair for {language} {tier}"
            )));
        };
        Ok(vec![
            Measurement {
                revision: Revision::Base,
                backend: backend.to_owned(),
                language,
                tier,
                instructions: base.value,
            },
            Measurement {
                revision: Revision::Head,
                backend: backend.to_owned(),
                language,
                tier,
                instructions: head.value,
            },
        ])
    } else {
        let current = values.left.as_ref().ok_or_else(|| {
            ComparisonError::new(format!(
                "current-only backend {backend:?} unexpectedly has a baseline for {language} {tier}"
            ))
        })?;
        Ok(vec![Measurement {
            revision: Revision::Head,
            backend: backend.to_owned(),
            language,
            tier,
            instructions: current.value,
        }])
    }
}

fn insert_measurement(
    measurements: &mut Vec<Measurement>,
    keys: &mut BTreeSet<(Revision, String, Language, Tier)>,
    revision: Revision,
    backend: &str,
    language: Language,
    tier: Tier,
    instructions: u64,
) -> Result<(), ComparisonError> {
    let key = (revision, backend.to_owned(), language, tier);
    if !keys.insert(key) {
        return Err(ComparisonError::new(format!(
            "duplicate {revision:?} measurement for {backend} {language} {tier}"
        )));
    }
    measurements.push(Measurement {
        revision,
        backend: backend.to_owned(),
        language,
        tier,
        instructions,
    });
    Ok(())
}

fn validate_expected_measurements(
    measurements: &[Measurement],
    manifest: &BackendManifest,
) -> Result<(), ComparisonError> {
    for backend in &manifest.backend {
        for &language in &backend.languages {
            for tier in Tier::ALL {
                require_measurement(measurements, Revision::Head, &backend.id, language, tier)?;
                if backend.id == manifest.primary {
                    require_measurement(measurements, Revision::Base, &backend.id, language, tier)?;
                }
            }
        }
    }
    Ok(())
}

fn require_measurement(
    measurements: &[Measurement],
    revision: Revision,
    backend: &str,
    language: Language,
    tier: Tier,
) -> Result<(), ComparisonError> {
    if measurements.iter().any(|measurement| {
        measurement.revision == revision
            && measurement.backend == backend
            && measurement.language == language
            && measurement.tier == tier
    }) {
        Ok(())
    } else {
        Err(ComparisonError::new(format!(
            "missing {revision:?} measurement for {backend} {language} {tier}"
        )))
    }
}

fn parse_case_id(id: &str) -> Option<(Language, Tier)> {
    let (language, tier) = id.rsplit_once('_')?;
    Some((Language::from_name(language)?, Tier::from_name(tier)?))
}

fn find_summary_files(root: &Path) -> Result<Vec<std::path::PathBuf>, ComparisonError> {
    let mut directories = vec![root.to_path_buf()];
    let mut summaries = Vec::new();
    while let Some(directory) = directories.pop() {
        let entries = fs::read_dir(&directory).map_err(|error| {
            ComparisonError::new(format!(
                "failed to enumerate {}: {error}",
                directory.display()
            ))
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                ComparisonError::new(format!(
                    "failed to enumerate {}: {error}",
                    directory.display()
                ))
            })?;
            let file_type = entry.file_type().map_err(|error| {
                ComparisonError::new(format!(
                    "failed to inspect {}: {error}",
                    entry.path().display()
                ))
            })?;
            if file_type.is_dir() {
                directories.push(entry.path());
            } else if file_type.is_file() && entry.file_name() == "summary.json" {
                summaries.push(entry.path());
            }
        }
    }
    summaries.sort();
    Ok(summaries)
}

#[cfg(test)]
mod tests {
    use super::parse_case_id;
    use crate::datasets::{Language, Tier};

    #[test]
    fn parses_case_ids() {
        assert_eq!(
            parse_case_id("kotlin_500kb"),
            Some((Language::Kotlin, Tier::Kb500))
        );
        assert_eq!(parse_case_id("unknown_10kb"), None);
        assert_eq!(parse_case_id("go_unknown"), None);
    }
}
