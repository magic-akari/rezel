use std::error::Error;
use std::fmt;

/// One grammar compilation error with an optional UTF-8 byte position.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratorError {
    message: String,
    position: Option<usize>,
}

impl GeneratorError {
    #[must_use]
    pub fn new(message: impl Into<String>, position: Option<usize>) -> Self {
        Self {
            message: message.into(),
            position,
        }
    }

    #[must_use]
    pub const fn position(&self) -> Option<usize> {
        self.position
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for GeneratorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for GeneratorError {}

/// One non-fatal grammar diagnostic with a UTF-8 byte position.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratorWarning {
    message: String,
    position: usize,
}

impl GeneratorWarning {
    #[must_use]
    pub fn new(message: impl Into<String>, position: usize) -> Self {
        Self {
            message: message.into(),
            position,
        }
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub const fn position(&self) -> usize {
        self.position
    }
}

impl fmt::Display for GeneratorWarning {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} at byte {}", self.message, self.position)
    }
}
