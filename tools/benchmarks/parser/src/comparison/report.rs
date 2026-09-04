use std::fmt::Write;

use crate::datasets::{Language, Tier};

use super::{
    BackendManifest, ComparisonError, ComparisonResults, Environment, Measurement, Revision,
};

#[derive(Clone, Copy)]
struct MeasurementSide<'a> {
    revision: Revision,
    backend: &'a str,
}

#[derive(Clone, Copy)]
struct MeasurementComparison<'a> {
    subject: MeasurementSide<'a>,
    reference: MeasurementSide<'a>,
}

/// Renders the normalized measurements as a compact Markdown report.
///
/// # Errors
///
/// Returns an error when an expected normalized measurement is missing.
pub fn render_report(
    results: &ComparisonResults,
    manifest: &BackendManifest,
    environment: &Environment,
) -> Result<String, ComparisonError> {
    let mut report = String::new();
    writeln!(report, "# Parser benchmark").expect("write to string");
    writeln!(report).expect("write to string");
    writeln!(
        report,
        "Callgrind instructions (`Ir`); lower is better. ↓ fewer, ↑ more, — no change."
    )
    .expect("write to string");
    writeln!(
        report,
        "Overall differences use summed instructions, not averaged percentages."
    )
    .expect("write to string");

    render_overview(&mut report, results, manifest)?;
    render_horizontal(&mut report, results, manifest)?;
    render_vertical(&mut report, results, manifest)?;
    render_run_details(&mut report, results, manifest, environment);
    Ok(report)
}

fn render_overview(
    report: &mut String,
    results: &ComparisonResults,
    manifest: &BackendManifest,
) -> Result<(), ComparisonError> {
    let primary = manifest.primary()?;
    writeln!(report).expect("write to string");
    writeln!(report, "## At a glance").expect("write to string");
    writeln!(report).expect("write to string");
    writeln!(
        report,
        "| Comparison | HEAD instructions | Reference instructions | Difference |"
    )
    .expect("write to string");
    writeln!(report, "| --- | ---: | ---: | ---: |").expect("write to string");

    let vertical = MeasurementComparison {
        subject: MeasurementSide {
            revision: Revision::Head,
            backend: &primary.id,
        },
        reference: MeasurementSide {
            revision: Revision::Base,
            backend: &primary.id,
        },
    };
    let (head, base) = sum_backend(results, vertical, &primary.languages)?;
    writeln!(
        report,
        "| {} HEAD vs {} base | {} | {} | {} |",
        primary.label,
        primary.label,
        format_integer(head),
        format_integer(base),
        format_difference(head, base)
    )
    .expect("write to string");

    for backend in &manifest.backend {
        if backend.id == primary.id {
            continue;
        }
        let horizontal = MeasurementComparison {
            subject: MeasurementSide {
                revision: Revision::Head,
                backend: &primary.id,
            },
            reference: MeasurementSide {
                revision: Revision::Head,
                backend: &backend.id,
            },
        };
        let (primary_total, backend_total) = sum_backend(results, horizontal, &backend.languages)?;
        writeln!(
            report,
            "| {} vs {} at HEAD | {} | {} | {} |",
            escape_table_cell(&primary.label),
            escape_table_cell(&backend.label),
            format_integer(primary_total),
            format_integer(backend_total),
            format_difference(primary_total, backend_total)
        )
        .expect("write to string");
    }
    Ok(())
}

fn render_horizontal(
    report: &mut String,
    results: &ComparisonResults,
    manifest: &BackendManifest,
) -> Result<(), ComparisonError> {
    let primary = manifest.primary()?;
    let alternatives = manifest
        .backend
        .iter()
        .filter(|backend| backend.id != primary.id)
        .collect::<Vec<_>>();
    if alternatives.is_empty() {
        return Ok(());
    }

    writeln!(report).expect("write to string");
    writeln!(report, "## Implementations at HEAD").expect("write to string");
    for backend in alternatives {
        writeln!(report).expect("write to string");
        writeln!(report, "### {} vs {}", primary.label, backend.label).expect("write to string");
        let comparison = MeasurementComparison {
            subject: MeasurementSide {
                revision: Revision::Head,
                backend: &primary.id,
            },
            reference: MeasurementSide {
                revision: Revision::Head,
                backend: &backend.id,
            },
        };
        render_matrix(report, results, comparison, &backend.languages)?;
    }
    Ok(())
}

fn render_vertical(
    report: &mut String,
    results: &ComparisonResults,
    manifest: &BackendManifest,
) -> Result<(), ComparisonError> {
    let primary = manifest.primary()?;
    writeln!(report).expect("write to string");
    writeln!(report, "## Change from base").expect("write to string");
    writeln!(report).expect("write to string");
    writeln!(
        report,
        "### {} HEAD vs base ({})",
        primary.label,
        short_revision(&results.base)
    )
    .expect("write to string");
    let comparison = MeasurementComparison {
        subject: MeasurementSide {
            revision: Revision::Head,
            backend: &primary.id,
        },
        reference: MeasurementSide {
            revision: Revision::Base,
            backend: &primary.id,
        },
    };
    render_matrix(report, results, comparison, &primary.languages)
}

fn render_matrix(
    report: &mut String,
    results: &ComparisonResults,
    comparison: MeasurementComparison<'_>,
    languages: &[Language],
) -> Result<(), ComparisonError> {
    writeln!(report).expect("write to string");
    writeln!(
        report,
        "| Language | 10kb | 50kb | 100kb | 500kb | Overall |"
    )
    .expect("write to string");
    writeln!(report, "| --- | ---: | ---: | ---: | ---: | ---: |").expect("write to string");

    for &language in languages {
        let mut row = String::new();
        let mut language_subject = 0_u64;
        let mut language_reference = 0_u64;
        for tier in Tier::ALL {
            let subject = require(
                results,
                comparison.subject.revision,
                comparison.subject.backend,
                language,
                tier,
            )?
            .instructions;
            let reference = require(
                results,
                comparison.reference.revision,
                comparison.reference.backend,
                language,
                tier,
            )?
            .instructions;
            language_subject += subject;
            language_reference += reference;
            write!(row, " | {}", format_difference(subject, reference)).expect("write to string");
        }
        writeln!(
            report,
            "| {language}{row} | **{}** |",
            format_difference(language_subject, language_reference)
        )
        .expect("write to string");
    }
    Ok(())
}

fn render_run_details(
    report: &mut String,
    results: &ComparisonResults,
    manifest: &BackendManifest,
    environment: &Environment,
) {
    let backends = manifest
        .backend
        .iter()
        .map(|backend| backend.label.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(report).expect("write to string");
    writeln!(report, "<details>").expect("write to string");
    writeln!(report, "<summary>Run details</summary>").expect("write to string");
    writeln!(report).expect("write to string");
    writeln!(report, "| Field | Value |").expect("write to string");
    writeln!(report, "| --- | --- |").expect("write to string");
    writeln!(report, "| HEAD | `{}` |", results.head).expect("write to string");
    writeln!(report, "| Base | `{}` |", results.base).expect("write to string");
    writeln!(report, "| Harness | `{}` |", results.harness).expect("write to string");
    writeln!(report, "| Backends | {} |", escape_table_cell(&backends)).expect("write to string");
    writeln!(
        report,
        "| System | {} |",
        escape_table_cell(&environment.system)
    )
    .expect("write to string");
    writeln!(
        report,
        "| Rust | {} |",
        escape_table_cell(&environment.rustc)
    )
    .expect("write to string");
    writeln!(
        report,
        "| Cargo | {} |",
        escape_table_cell(&environment.cargo)
    )
    .expect("write to string");
    writeln!(
        report,
        "| Gungraun | {} |",
        escape_table_cell(&environment.gungraun_runner)
    )
    .expect("write to string");
    writeln!(
        report,
        "| Valgrind | {} |",
        escape_table_cell(&environment.valgrind)
    )
    .expect("write to string");
    writeln!(report).expect("write to string");
    writeln!(
        report,
        "Exact measurements are available in `results.json`; run configuration is in `metadata.json`."
    )
    .expect("write to string");
    writeln!(report).expect("write to string");
    writeln!(report, "</details>").expect("write to string");
}

fn sum_backend(
    results: &ComparisonResults,
    comparison: MeasurementComparison<'_>,
    languages: &[Language],
) -> Result<(u64, u64), ComparisonError> {
    let mut subject_total = 0_u64;
    let mut reference_total = 0_u64;
    for &language in languages {
        for tier in Tier::ALL {
            subject_total += require(
                results,
                comparison.subject.revision,
                comparison.subject.backend,
                language,
                tier,
            )?
            .instructions;
            reference_total += require(
                results,
                comparison.reference.revision,
                comparison.reference.backend,
                language,
                tier,
            )?
            .instructions;
        }
    }
    Ok((subject_total, reference_total))
}

fn require<'a>(
    results: &'a ComparisonResults,
    revision: Revision,
    backend: &str,
    language: Language,
    tier: Tier,
) -> Result<&'a Measurement, ComparisonError> {
    results
        .find(revision, backend, language, tier)
        .ok_or_else(|| {
            ComparisonError::new(format!(
                "missing {revision:?} measurement for {backend} {language} {tier}"
            ))
        })
}

fn format_difference(value: u64, reference: u64) -> String {
    if value == reference {
        return "—".to_owned();
    }
    if reference == 0 {
        return "N/A".to_owned();
    }
    let numerator = (i128::from(value) - i128::from(reference)) * 10_000;
    let denominator = i128::from(reference);
    let rounded = if numerator < 0 {
        (numerator - denominator / 2) / denominator
    } else {
        (numerator + denominator / 2) / denominator
    };
    let arrow = if rounded < 0 { '↓' } else { '↑' };
    let magnitude = rounded.unsigned_abs();
    format!("{arrow} {}.{:02}%", magnitude / 100, magnitude % 100)
}

fn format_integer(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(character);
    }
    formatted
}

fn short_revision(revision: &str) -> String {
    revision.chars().take(12).collect()
}

fn escape_table_cell(value: &str) -> String {
    value.replace('|', "\\|").replace(['\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::{format_difference, format_integer, render_report, short_revision};
    use crate::{
        comparison::{
            BackendKind, BackendManifest, BackendScope, BackendSpec, ComparisonResults,
            Environment, Measurement, Revision,
        },
        datasets::{Language, Tier},
    };

    #[test]
    fn formats_integer_groups() {
        assert_eq!(format_integer(0), "0");
        assert_eq!(format_integer(999), "999");
        assert_eq!(format_integer(1_234_567), "1,234,567");
    }

    #[test]
    fn formats_instruction_differences() {
        assert_eq!(format_difference(80, 100), "↓ 20.00%");
        assert_eq!(format_difference(110, 100), "↑ 10.00%");
        assert_eq!(format_difference(100, 100), "—");
        assert_eq!(format_difference(1, 0), "N/A");
    }

    #[test]
    fn shortens_revisions_safely() {
        assert_eq!(short_revision("1234567890abcdef"), "1234567890ab");
        assert_eq!(short_revision("HEAD"), "HEAD");
    }

    #[test]
    fn renders_compact_difference_matrices() {
        let mut measurements = Vec::new();
        for language in Language::ALL {
            for tier in Tier::ALL {
                measurements.push(Measurement {
                    revision: Revision::Base,
                    backend: "rezel".to_owned(),
                    language,
                    tier,
                    instructions: 100,
                });
                measurements.push(Measurement {
                    revision: Revision::Head,
                    backend: "rezel".to_owned(),
                    language,
                    tier,
                    instructions: 100,
                });
            }
        }
        for tier in Tier::ALL {
            measurements.push(Measurement {
                revision: Revision::Head,
                backend: "tree-sitter".to_owned(),
                language: Language::Go,
                tier,
                instructions: 200,
            });
        }

        let results = ComparisonResults::new(
            "1111111111111111111111111111111111111111".to_owned(),
            "0000000000000000000000000000000000000000".to_owned(),
            "rezel".to_owned(),
            measurements,
        );
        let primary = BackendSpec {
            id: "rezel".to_owned(),
            label: "Rezel".to_owned(),
            bench: "gungraun".to_owned(),
            function: "parse_rezel".to_owned(),
            filter: "rezel".to_owned(),
            kind: BackendKind::Library,
            scope: BackendScope::Parse,
            languages: Language::ALL.to_vec(),
        };
        let alternative = BackendSpec {
            id: "tree-sitter".to_owned(),
            label: "Tree-sitter".to_owned(),
            bench: "gungraun".to_owned(),
            function: "parse_tree_sitter".to_owned(),
            filter: "tree-sitter".to_owned(),
            kind: BackendKind::Library,
            scope: BackendScope::Parse,
            languages: vec![Language::Go],
        };
        let manifest = BackendManifest {
            primary: "rezel".to_owned(),
            gungraun: "0.19.4".to_owned(),
            backend: vec![primary, alternative],
        };
        let environment = Environment {
            system: "Linux arm64".to_owned(),
            rustc: "rustc".to_owned(),
            cargo: "cargo".to_owned(),
            gungraun_runner: "gungraun-runner".to_owned(),
            valgrind: "valgrind".to_owned(),
        };

        let report = render_report(&results, &manifest, &environment).expect("render report");

        assert!(report.contains("| Rezel HEAD vs Rezel base | 3,200 | 3,200 | — |"));
        assert!(report.contains("| Rezel vs Tree-sitter at HEAD | 400 | 800 | ↓ 50.00% |"));
        assert!(report.contains("| go | ↓ 50.00% | ↓ 50.00% | ↓ 50.00% | ↓ 50.00% |"));
        assert!(report.contains("<summary>Run details</summary>"));
        assert!(!report.contains("| Language | Tier |"));
        assert!(report.lines().count() < 70);
    }
}
