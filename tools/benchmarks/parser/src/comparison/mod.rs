mod manifest;
mod model;
mod report;
mod summary;

use std::{error::Error, fmt};

pub use manifest::{
    BackendKind, BackendManifest, BackendScope, BackendSpec, backends_manifest_path,
};
pub use model::{ComparisonResults, Environment, Measurement, Revision, RunMetadata};
pub use report::render_report;
pub use summary::load_current_measurements;

#[derive(Debug)]
pub struct ComparisonError(String);

impl ComparisonError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ComparisonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ComparisonError {}
