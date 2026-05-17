use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::Deserialize;
use syn::Path;

use crate::{CompiledGrammar, GeneratorError, SpecializerMetadata, TokenizerMetadata};

/// The semantic role of one grammar-declared Rust external.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
pub enum RustBindingKind {
    ExternalTokenizer,
    ExternalSpecializer,
    ContextTracker,
    NodeProperty,
    PropertySource,
}

impl fmt::Display for RustBindingKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ExternalTokenizer => "external-tokenizer",
            Self::ExternalSpecializer => "external-specializer",
            Self::ContextTracker => "context-tracker",
            Self::NodeProperty => "node-property",
            Self::PropertySource => "property-source",
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct RustBindingDeclaration {
    kind: RustBindingKind,
    source: String,
    name: String,
    rust_path: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct RustBindingManifest {
    #[serde(default)]
    binding: Vec<RustBindingDeclaration>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RustBindingKey {
    kind: RustBindingKind,
    source: String,
    name: String,
}

impl RustBindingKey {
    fn new(kind: RustBindingKind, source: &str, name: &str) -> Self {
        Self {
            kind,
            source: source.to_owned(),
            name: name.to_owned(),
        }
    }
}

impl fmt::Display for RustBindingKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} {:?} from {:?}",
            self.kind, self.name, self.source
        )
    }
}

/// Statically linked Rust symbols for grammar-declared externals.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RustBindings {
    symbols: BTreeMap<RustBindingKey, String>,
}

impl RustBindings {
    /// Parse a deterministic external-binding manifest.
    ///
    /// # Errors
    ///
    /// Returns malformed TOML, duplicate binding keys, or invalid Rust paths.
    pub fn from_toml_str(source: &str) -> Result<Self, GeneratorError> {
        let manifest = toml::from_str::<RustBindingManifest>(source).map_err(|error| {
            GeneratorError::new(format!("Invalid Rust binding manifest: {error}"), None)
        })?;
        let mut bindings = Self::default();
        for declaration in manifest.binding {
            bindings.insert(declaration)?;
        }
        Ok(bindings)
    }

    pub(crate) fn validate(&self, grammar: &CompiledGrammar) -> Result<(), GeneratorError> {
        let required = required_bindings(grammar);
        let provided = self.symbols.keys().cloned().collect::<BTreeSet<_>>();
        let missing = required.difference(&provided).collect::<Vec<_>>();
        let unused = provided.difference(&required).collect::<Vec<_>>();
        if missing.is_empty() && unused.is_empty() {
            return Ok(());
        }

        let mut problems = Vec::new();
        if !missing.is_empty() {
            problems.push(format!("missing Rust bindings: {}", join_keys(&missing)));
        }
        if !unused.is_empty() {
            problems.push(format!("unused Rust bindings: {}", join_keys(&unused)));
        }
        Err(GeneratorError::new(problems.join("; "), None))
    }

    pub(crate) fn resolve(
        &self,
        kind: RustBindingKind,
        source: &str,
        name: &str,
    ) -> Result<Path, GeneratorError> {
        let key = RustBindingKey::new(kind, source, name);
        let path = self
            .symbols
            .get(&key)
            .ok_or_else(|| GeneratorError::new(format!("Missing Rust binding for {key}"), None))?;
        syn::parse_str(path).map_err(|error| {
            GeneratorError::new(format!("Invalid Rust path for {key}: {error}"), None)
        })
    }

    fn insert(&mut self, declaration: RustBindingDeclaration) -> Result<(), GeneratorError> {
        let key = RustBindingKey {
            kind: declaration.kind,
            source: declaration.source,
            name: declaration.name,
        };
        syn::parse_str::<Path>(&declaration.rust_path).map_err(|error| {
            GeneratorError::new(format!("Invalid Rust path for {key}: {error}"), None)
        })?;
        if self.symbols.contains_key(&key) {
            return Err(GeneratorError::new(
                format!("Duplicate Rust binding for {key}"),
                None,
            ));
        }
        self.symbols.insert(key, declaration.rust_path);
        Ok(())
    }
}

fn required_bindings(grammar: &CompiledGrammar) -> BTreeSet<RustBindingKey> {
    let mut required = BTreeSet::new();
    for tokenizer in &grammar.tokenizers {
        if let TokenizerMetadata::External { binding, source } = tokenizer {
            required.insert(RustBindingKey::new(
                RustBindingKind::ExternalTokenizer,
                source,
                binding,
            ));
        }
    }
    for specializer in &grammar.specializers {
        if let SpecializerMetadata::External {
            binding, source, ..
        } = specializer
        {
            required.insert(RustBindingKey::new(
                RustBindingKind::ExternalSpecializer,
                source,
                binding,
            ));
        }
    }
    if let Some(context) = &grammar.context {
        required.insert(RustBindingKey::new(
            RustBindingKind::ContextTracker,
            &context.source,
            &context.binding,
        ));
    }
    for property in &grammar.external_properties {
        required.insert(RustBindingKey::new(
            RustBindingKind::NodeProperty,
            &property.source,
            &property.binding,
        ));
    }
    for source in &grammar.property_sources {
        required.insert(RustBindingKey::new(
            RustBindingKind::PropertySource,
            &source.source,
            &source.binding,
        ));
    }
    required
}

fn join_keys(keys: &[&RustBindingKey]) -> String {
    keys.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BuildOptions, compile_grammar};

    const GRAMMAR: &str = r#"
@top T { word }
@external tokens words from "./tokens" { word }
@context scope from "./context"
@external propSource tags from "./highlight"
"#;

    const BINDINGS: &str = r#"
[[binding]]
kind = "external-tokenizer"
source = "./tokens"
name = "words"
rust_path = "crate::WORDS"

[[binding]]
kind = "context-tracker"
source = "./context"
name = "scope"
rust_path = "crate::SCOPE"

[[binding]]
kind = "property-source"
source = "./highlight"
name = "tags"
rust_path = "crate::tags"
"#;

    #[test]
    fn validates_manifest_against_the_grammar_contract() {
        let grammar = compile_grammar(GRAMMAR, None, BuildOptions::default()).unwrap();
        let bindings = RustBindings::from_toml_str(BINDINGS).unwrap();
        bindings.validate(&grammar).unwrap();

        let mismatched = BINDINGS.replace("external-tokenizer", "external-specializer");
        let bindings = RustBindings::from_toml_str(&mismatched).unwrap();
        let error = bindings.validate(&grammar).unwrap_err().to_string();
        assert!(error.contains("missing Rust bindings"));
        assert!(error.contains("unused Rust bindings"));
        assert!(error.contains("external-tokenizer"));
        assert!(error.contains("external-specializer"));
    }

    #[test]
    fn rejects_invalid_manifest_entries() {
        for (manifest, expected) in [
            (
                BINDINGS.replace("rust_path", "rust-path"),
                "unknown field `rust-path`",
            ),
            (
                format!("{BINDINGS}\n{}", BINDINGS.split("\n\n").next().unwrap()),
                "Duplicate Rust binding",
            ),
            (
                BINDINGS.replace("crate::WORDS", "crate::"),
                "Invalid Rust path",
            ),
        ] {
            let error = RustBindings::from_toml_str(&manifest)
                .unwrap_err()
                .to_string();
            assert!(
                error.contains(expected),
                "expected {expected:?}, got {error:?}"
            );
        }
    }
}
