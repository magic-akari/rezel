use serde::Serialize;

use crate::datasets::{Language, Tier};

use super::BackendSpec;

pub const RESULTS_SCHEMA: &str = "rezel.parser-comparison-results.v1";
pub const METADATA_SCHEMA: &str = "rezel.parser-comparison-metadata.v1";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Revision {
    Base,
    Head,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Measurement {
    pub revision: Revision,
    pub backend: String,
    pub language: Language,
    pub tier: Tier,
    pub instructions: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Environment {
    pub system: String,
    pub rustc: String,
    pub cargo: String,
    pub gungraun_runner: String,
    pub valgrind: String,
}

#[derive(Debug, Serialize)]
pub struct ComparisonResults {
    pub schema: &'static str,
    pub head: String,
    pub base: String,
    pub harness: String,
    pub primary: String,
    pub measurements: Vec<Measurement>,
}

impl ComparisonResults {
    #[must_use]
    pub fn new(
        head: String,
        base: String,
        primary: String,
        mut measurements: Vec<Measurement>,
    ) -> Self {
        measurements.sort();
        Self {
            schema: RESULTS_SCHEMA,
            harness: head.clone(),
            head,
            base,
            primary,
            measurements,
        }
    }

    #[must_use]
    pub fn find(
        &self,
        revision: Revision,
        backend: &str,
        language: Language,
        tier: Tier,
    ) -> Option<&Measurement> {
        self.measurements.iter().find(|measurement| {
            measurement.revision == revision
                && measurement.backend == backend
                && measurement.language == language
                && measurement.tier == tier
        })
    }
}

#[derive(Debug, Serialize)]
pub struct RunMetadata<'a> {
    pub schema: &'static str,
    pub head: &'a str,
    pub base: &'a str,
    pub harness: &'a str,
    pub primary: &'a str,
    pub backends: &'a [BackendSpec],
    pub environment: &'a Environment,
}

impl<'a> RunMetadata<'a> {
    #[must_use]
    pub fn new(
        results: &'a ComparisonResults,
        backends: &'a [BackendSpec],
        environment: &'a Environment,
    ) -> Self {
        Self {
            schema: METADATA_SCHEMA,
            head: &results.head,
            base: &results.base,
            harness: &results.harness,
            primary: &results.primary,
            backends,
            environment,
        }
    }
}
