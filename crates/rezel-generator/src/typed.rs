use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::{Ident, TokenStream};
use quote::quote;
use serde::Deserialize;
use syn::Path;

use crate::source::format_generated_rust;
use crate::{CompiledGrammar, GeneratorError};

/// Generate zero-copy typed CST wrappers from an independent TOML schema.
///
/// # Errors
///
/// Returns an error when the schema is malformed, refers to unknown or
/// ambiguous grammar kinds, describes an impossible direct-child field, or
/// would emit invalid Rust.
pub fn emit_typed_syntax(
    grammar: &CompiledGrammar,
    schema_source: &str,
) -> Result<String, GeneratorError> {
    let schema = TypedSyntaxSchema::parse(schema_source)?;
    let checked = CheckedSchema::new(grammar, schema)?;
    checked.emit()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TypedSyntaxSchema {
    language: String,
    kind: String,
    language_path: String,
    #[serde(default)]
    coverage: SchemaCoverage,
    #[serde(default)]
    ignore: Vec<String>,
    #[serde(default)]
    kind_names: BTreeMap<String, String>,
    #[serde(default, rename = "node")]
    nodes: Vec<NodeSchema>,
    #[serde(default, rename = "union")]
    unions: Vec<UnionSchema>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum SchemaCoverage {
    #[default]
    Partial,
    Complete,
}

impl TypedSyntaxSchema {
    fn parse(source: &str) -> Result<Self, GeneratorError> {
        toml::from_str(source).map_err(|error| {
            GeneratorError::new(format!("Invalid typed syntax schema: {error}"), None)
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeSchema {
    name: String,
    kind: String,
    #[serde(default, rename = "field")]
    fields: Vec<FieldSchema>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UnionSchema {
    name: String,
    variants: Vec<VariantSchema>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VariantSchema {
    name: String,
    #[serde(rename = "type")]
    target: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum FieldCardinality {
    One,
    Optional,
    Many,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldSchema {
    name: String,
    #[serde(default, rename = "type")]
    target: Option<String>,
    cardinality: FieldCardinality,
    #[serde(default)]
    occurrence: usize,
    selector: SelectorSchema,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectorSchema {
    node: Option<String>,
    union: Option<String>,
    token: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Cardinality {
    min: usize,
    max: Option<usize>,
}

#[derive(Clone)]
struct CheckedNode {
    name: Ident,
    kind: Ident,
    kind_ids: BTreeSet<u16>,
    fields: Vec<CheckedField>,
}

#[derive(Clone)]
struct CheckedUnion {
    name: Ident,
    variants: Vec<CheckedVariant>,
    kinds: BTreeSet<u16>,
}

#[derive(Clone)]
struct CheckedVariant {
    name: Ident,
    target: Ident,
    kinds: Vec<Ident>,
}

#[derive(Clone)]
enum CheckedSelector {
    Typed { target: Ident },
    Token { kind: Ident },
}

#[derive(Clone)]
struct CheckedField {
    name: Ident,
    cardinality: FieldCardinality,
    occurrence: usize,
    selector: CheckedSelector,
}

struct CheckedSchema {
    language: Ident,
    kind: Ident,
    language_path: Path,
    kinds: Vec<GrammarKind>,
    nodes: Vec<CheckedNode>,
    unions: Vec<CheckedUnion>,
}

#[derive(Clone)]
struct GrammarKind {
    ids: BTreeSet<u16>,
    variant: Ident,
}

impl CheckedSchema {
    fn new(grammar: &CompiledGrammar, schema: TypedSyntaxSchema) -> Result<Self, GeneratorError> {
        let language = rust_ident(&schema.language, "language type")?;
        let kind = rust_ident(&schema.kind, "kind type")?;
        let language_path = parse_language_path(&schema.language_path)?;
        let grammar_kinds = GrammarKinds::new(grammar);
        let kinds = grammar_kinds.kind_variants(&schema.kind_names)?;
        grammar_kinds.check_coverage(schema.coverage, &schema.ignore, &schema.nodes)?;

        let mut type_names = BTreeSet::new();
        let checked_nodes = check_nodes(&grammar_kinds, &kinds, &schema.nodes, &mut type_names)?;
        let mut nodes = checked_nodes.values;
        let node_targets = checked_nodes.targets;
        let checked_unions = check_unions(&kinds, &schema.unions, &node_targets, &mut type_names)?;
        let unions = checked_unions.values;
        let union_targets = checked_unions.targets;
        let mut normalized = NormalizedGrammar::new(grammar)?;
        let field_types = FieldTypes {
            grammar: &grammar_kinds,
            variants: &kinds,
            nodes: &node_targets,
            unions: &union_targets,
        };
        for (checked, source) in nodes.iter_mut().zip(schema.nodes) {
            checked.fields = check_fields(
                &mut normalized,
                &field_types,
                &checked.kind_ids,
                source.fields,
            )?;
        }

        Ok(Self {
            language,
            kind,
            language_path,
            kinds,
            nodes,
            unions,
        })
    }

    fn emit(&self) -> Result<String, GeneratorError> {
        let language = &self.language;
        let kind = &self.kind;
        let language_path = &self.language_path;
        let kind_variants = self.kinds.iter().map(|kind| &kind.variant);
        let kind_arms = self.kinds.iter().flat_map(|grammar_kind| {
            let variant = &grammar_kind.variant;
            grammar_kind
                .ids
                .iter()
                .map(move |term| quote!(#term => #kind::#variant,))
        });
        let nodes = self
            .nodes
            .iter()
            .map(|node| emit_node(node, language, kind));
        let unions = self
            .unions
            .iter()
            .map(|union| emit_union(union, language, kind));
        let source = quote! {
            #![allow(clippy::all, clippy::pedantic, missing_docs)]

            #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
            pub enum #kind {
                #(#kind_variants,)*
            }

            #[derive(Clone, Copy, Debug)]
            pub enum #language {}

            impl rezel_common::SyntaxLanguage for #language {
                type Kind = #kind;

                fn kind(node: &rezel_common::SyntaxNode) -> Option<Self::Kind> {
                    let node_type = node.node_type();
                    let node_set = (#language_path.node_set)();
                    let expected = node_set.get(node_type.id())?;
                    if !node_type.is(expected) {
                        return None;
                    }
                    let kind = match node_type.id() {
                        #(#kind_arms)*
                        _ => return None,
                    };
                    Some(kind)
                }
            }

            #(#nodes)*
            #(#unions)*
        };
        format_generated_rust(
            &source.to_string(),
            "Generated typed syntax is invalid Rust",
        )
    }
}

struct FieldTypes<'a> {
    grammar: &'a GrammarKinds,
    variants: &'a [GrammarKind],
    nodes: &'a BTreeMap<String, BTreeSet<u16>>,
    unions: &'a BTreeMap<String, BTreeSet<u16>>,
}

struct CheckedUnions {
    values: Vec<CheckedUnion>,
    targets: BTreeMap<String, BTreeSet<u16>>,
}

struct CheckedNodes {
    values: Vec<CheckedNode>,
    targets: BTreeMap<String, BTreeSet<u16>>,
}

fn parse_language_path(source: &str) -> Result<Path, GeneratorError> {
    syn::parse_str::<Path>(source).map_err(|error| {
        GeneratorError::new(
            format!("Invalid language_path {source:?} in typed syntax schema: {error}"),
            None,
        )
    })
}

fn check_nodes(
    grammar_kinds: &GrammarKinds,
    kinds: &[GrammarKind],
    schemas: &[NodeSchema],
    type_names: &mut BTreeSet<String>,
) -> Result<CheckedNodes, GeneratorError> {
    let mut nodes = Vec::new();
    let mut targets = BTreeMap::new();
    for schema in schemas {
        if !type_names.insert(schema.name.clone()) {
            return Err(schema_error(format!(
                "Duplicate typed node {:?}",
                schema.name
            )));
        }
        let name = rust_ident(&schema.name, "typed node")?;
        let kind_ids = grammar_kinds.resolve(&schema.kind)?;
        let kind = grammar_kind_variant(&kind_ids, kinds)?.clone();
        targets.insert(schema.name.clone(), kind_ids.clone());
        nodes.push(CheckedNode {
            name,
            kind,
            kind_ids,
            fields: Vec::new(),
        });
    }
    Ok(CheckedNodes {
        values: nodes,
        targets,
    })
}

fn check_unions(
    kinds: &[GrammarKind],
    schemas: &[UnionSchema],
    node_targets: &BTreeMap<String, BTreeSet<u16>>,
    type_names: &mut BTreeSet<String>,
) -> Result<CheckedUnions, GeneratorError> {
    let mut sources = BTreeMap::new();
    for schema in schemas {
        if !type_names.insert(schema.name.clone()) {
            return Err(schema_error(format!(
                "Duplicate typed syntax type {:?}",
                schema.name
            )));
        }
        sources.insert(schema.name.clone(), schema);
    }
    let mut resolved = BTreeMap::new();
    let mut visiting = BTreeSet::new();
    for schema in schemas {
        resolve_union(
            kinds,
            &sources,
            node_targets,
            &schema.name,
            &mut resolved,
            &mut visiting,
        )?;
    }
    let unions = schemas
        .iter()
        .map(|schema| {
            resolved
                .get(&schema.name)
                .expect("every typed union was resolved")
                .clone()
        })
        .collect::<Vec<_>>();
    let targets = resolved
        .into_iter()
        .map(|(name, union)| (name, union.kinds))
        .collect();
    Ok(CheckedUnions {
        values: unions,
        targets,
    })
}

fn resolve_union(
    kinds: &[GrammarKind],
    schemas: &BTreeMap<String, &UnionSchema>,
    node_targets: &BTreeMap<String, BTreeSet<u16>>,
    name: &str,
    resolved: &mut BTreeMap<String, CheckedUnion>,
    visiting: &mut BTreeSet<String>,
) -> Result<(), GeneratorError> {
    if resolved.contains_key(name) {
        return Ok(());
    }
    if !visiting.insert(name.to_owned()) {
        return Err(schema_error(format!(
            "Typed union dependency cycle includes {name:?}"
        )));
    }
    let schema = schemas
        .get(name)
        .copied()
        .ok_or_else(|| schema_error(format!("Unknown typed union {name:?}")))?;
    if schema.variants.is_empty() {
        return Err(schema_error(format!(
            "Typed union {:?} has no variants",
            schema.name
        )));
    }
    let mut names = BTreeSet::new();
    let mut variants = Vec::new();
    let mut union_kinds = BTreeSet::new();
    for variant in &schema.variants {
        if !names.insert(variant.name.clone()) {
            return Err(schema_error(format!(
                "Duplicate variant {:?} in union {:?}",
                variant.name, schema.name
            )));
        }
        let kind_ids = if let Some(kind_ids) = node_targets.get(&variant.target) {
            kind_ids.clone()
        } else if schemas.contains_key(&variant.target) {
            resolve_union(
                kinds,
                schemas,
                node_targets,
                &variant.target,
                resolved,
                visiting,
            )?;
            resolved
                .get(&variant.target)
                .expect("nested union was resolved")
                .kinds
                .clone()
        } else {
            return Err(schema_error(format!(
                "Union {:?} variant {:?} refers to unknown typed node or union {:?}",
                schema.name, variant.name, variant.target
            )));
        };
        if !union_kinds.is_disjoint(&kind_ids) {
            return Err(schema_error(format!(
                "Union {:?} contains the same grammar kind more than once",
                schema.name
            )));
        }
        let kind_variants = grammar_kind_variants(&kind_ids, kinds);
        union_kinds.extend(kind_ids);
        variants.push(CheckedVariant {
            name: rust_ident(&variant.name, "union variant")?,
            target: rust_ident(&variant.target, "union target")?,
            kinds: kind_variants,
        });
    }
    visiting.remove(name);
    resolved.insert(
        name.to_owned(),
        CheckedUnion {
            name: rust_ident(&schema.name, "typed union")?,
            variants,
            kinds: union_kinds,
        },
    );
    Ok(())
}

fn grammar_kind_variants(kind_ids: &BTreeSet<u16>, kinds: &[GrammarKind]) -> Vec<Ident> {
    kinds
        .iter()
        .filter(|kind| !kind.ids.is_disjoint(kind_ids))
        .map(|kind| kind.variant.clone())
        .collect()
}

fn emit_node(node: &CheckedNode, language: &Ident, kind: &Ident) -> TokenStream {
    let name = &node.name;
    let node_kind = &node.kind;
    let fields = node
        .fields
        .iter()
        .map(|field| emit_field(field, language, kind));
    quote! {
        #[derive(Clone, Debug)]
        pub struct #name {
            syntax: rezel_common::SyntaxNode,
        }

        impl rezel_common::TypedNode for #name {
            type Language = #language;

            fn downcast_from(
                node: rezel_common::SyntaxNode,
            ) -> Result<Self, rezel_common::SyntaxNode> {
                let kind = <#language as rezel_common::SyntaxLanguage>::kind(&node);
                if kind == Some(#kind::#node_kind) {
                    Ok(Self { syntax: node })
                } else {
                    Err(node)
                }
            }

            fn syntax(&self) -> &rezel_common::SyntaxNode {
                &self.syntax
            }

            fn into_syntax(self) -> rezel_common::SyntaxNode {
                self.syntax
            }
        }

        impl #name {
            #(#fields)*
        }
    }
}

fn emit_union(union: &CheckedUnion, language: &Ident, kind: &Ident) -> TokenStream {
    let name = &union.name;
    let variants = union.variants.iter().map(|variant| {
        let variant_name = &variant.name;
        let target = &variant.target;
        quote!(#variant_name(#target),)
    });
    let downcast_arms = union.variants.iter().map(|variant| {
        let variant_name = &variant.name;
        let target = &variant.target;
        let variant_kinds = &variant.kinds;
        quote! {
            #(Some(#kind::#variant_kinds))|* => {
                let typed = <#target as rezel_common::TypedNode>::downcast_from(node)
                    .expect("kind was checked before generated downcast");
                Ok(Self::#variant_name(typed))
            }
        }
    });
    let syntax_arms = union.variants.iter().map(|variant| {
        let variant_name = &variant.name;
        quote!(Self::#variant_name(node) => rezel_common::TypedNode::syntax(node),)
    });
    let into_arms = union.variants.iter().map(|variant| {
        let variant_name = &variant.name;
        quote!(Self::#variant_name(node) => rezel_common::TypedNode::into_syntax(node),)
    });
    quote! {
        #[derive(Clone, Debug)]
        pub enum #name {
            #(#variants)*
        }

        impl rezel_common::TypedNode for #name {
            type Language = #language;

            fn downcast_from(
                node: rezel_common::SyntaxNode,
            ) -> Result<Self, rezel_common::SyntaxNode> {
                match <#language as rezel_common::SyntaxLanguage>::kind(&node) {
                    #(#downcast_arms,)*
                    _ => Err(node),
                }
            }

            fn syntax(&self) -> &rezel_common::SyntaxNode {
                match self {
                    #(#syntax_arms)*
                }
            }

            fn into_syntax(self) -> rezel_common::SyntaxNode {
                match self {
                    #(#into_arms)*
                }
            }
        }
    }
}

fn check_fields(
    grammar: &mut NormalizedGrammar,
    types: &FieldTypes<'_>,
    parents: &BTreeSet<u16>,
    fields: Vec<FieldSchema>,
) -> Result<Vec<CheckedField>, GeneratorError> {
    let mut names = BTreeSet::new();
    let mut occurrences = BTreeSet::new();
    let mut checked = Vec::new();
    for field in fields {
        if !names.insert(field.name.clone()) {
            return Err(schema_error(format!(
                "Duplicate field {:?} on typed node term {parent}",
                field.name,
                parent = parents.iter().next().copied().unwrap_or_default(),
            )));
        }
        checked.push(check_field(
            grammar,
            types,
            parents,
            &field,
            &mut occurrences,
        )?);
    }
    Ok(checked)
}

fn check_field(
    grammar: &mut NormalizedGrammar,
    types: &FieldTypes<'_>,
    parents: &BTreeSet<u16>,
    field: &FieldSchema,
    occurrences: &mut BTreeSet<(BTreeSet<u16>, usize)>,
) -> Result<CheckedField, GeneratorError> {
    require_one_selector(field)?;
    let (selector, target_kinds) = resolve_field_selector(types, field)?;
    validate_field_occurrence(field, &target_kinds, occurrences)?;
    let actual = grammar.direct_cardinality_for(parents, &target_kinds)?;
    validate_cardinality(field, actual)?;
    Ok(CheckedField {
        name: rust_ident(&field.name, "field")?,
        cardinality: field.cardinality,
        occurrence: field.occurrence,
        selector,
    })
}

fn require_one_selector(field: &FieldSchema) -> Result<(), GeneratorError> {
    let selectors = [
        field.selector.node.is_some(),
        field.selector.union.is_some(),
        field.selector.token.is_some(),
    ]
    .into_iter()
    .filter(|present| *present)
    .count();
    if selectors == 1 {
        Ok(())
    } else {
        Err(schema_error(format!(
            "Field {:?} must declare exactly one node, union, or token selector",
            field.name
        )))
    }
}

fn resolve_field_selector(
    types: &FieldTypes<'_>,
    field: &FieldSchema,
) -> Result<(CheckedSelector, BTreeSet<u16>), GeneratorError> {
    if let Some(node) = &field.selector.node {
        let kind_ids = types.grammar.resolve(node)?;
        let target = field
            .target
            .as_ref()
            .ok_or_else(|| schema_error(format!("Typed field {:?} requires a type", field.name)))?;
        let target_kinds = types.nodes.get(target).ok_or_else(|| {
            schema_error(format!(
                "Field {:?} refers to unknown concrete typed node {target:?}",
                field.name
            ))
        })?;
        if target_kinds != &kind_ids {
            return Err(schema_error(format!(
                "Field {:?} type {target:?} wraps a different grammar kind",
                field.name
            )));
        }
        return Ok((
            CheckedSelector::Typed {
                target: rust_ident(target, "field type")?,
            },
            kind_ids,
        ));
    }
    if let Some(union) = &field.selector.union {
        return resolve_union_selector(types, field, union);
    }
    resolve_token_selector(types, field)
}

fn resolve_union_selector(
    types: &FieldTypes<'_>,
    field: &FieldSchema,
    union: &str,
) -> Result<(CheckedSelector, BTreeSet<u16>), GeneratorError> {
    let target = field
        .target
        .as_ref()
        .ok_or_else(|| schema_error(format!("Typed field {:?} requires a type", field.name)))?;
    if target != union {
        return Err(schema_error(format!(
            "Field {:?} must use union {union:?} as its type",
            field.name
        )));
    }
    let target_kinds = types.unions.get(union).cloned().ok_or_else(|| {
        schema_error(format!(
            "Field {:?} refers to unknown typed union {union:?}",
            field.name
        ))
    })?;
    Ok((
        CheckedSelector::Typed {
            target: rust_ident(target, "field type")?,
        },
        target_kinds,
    ))
}

fn resolve_token_selector(
    types: &FieldTypes<'_>,
    field: &FieldSchema,
) -> Result<(CheckedSelector, BTreeSet<u16>), GeneratorError> {
    if field.target.is_some() {
        return Err(schema_error(format!(
            "Token field {:?} must not declare a typed target",
            field.name
        )));
    }
    if field.cardinality == FieldCardinality::Many {
        return Err(schema_error(format!(
            "Token field {:?} cannot use many cardinality",
            field.name
        )));
    }
    let token = field
        .selector
        .token
        .as_deref()
        .expect("one selector was checked");
    let kind_ids = types.grammar.resolve(token)?;
    let kind = grammar_kind_variant(&kind_ids, types.variants)?.clone();
    Ok((CheckedSelector::Token { kind }, kind_ids))
}

fn validate_field_occurrence(
    field: &FieldSchema,
    target_kinds: &BTreeSet<u16>,
    occurrences: &mut BTreeSet<(BTreeSet<u16>, usize)>,
) -> Result<(), GeneratorError> {
    if field.cardinality == FieldCardinality::Many && field.occurrence != 0 {
        return Err(schema_error(format!(
            "Repeated field {:?} cannot select one occurrence",
            field.name
        )));
    }
    let key = (target_kinds.clone(), field.occurrence);
    if field.cardinality != FieldCardinality::Many && !occurrences.insert(key) {
        return Err(schema_error(format!(
            "Field {:?} duplicates occurrence {} of the same selector",
            field.name, field.occurrence
        )));
    }
    Ok(())
}

fn validate_cardinality(field: &FieldSchema, actual: Cardinality) -> Result<(), GeneratorError> {
    let occurrence = field.occurrence;
    let reaches_occurrence = actual.max.is_none_or(|maximum| maximum > occurrence);
    match field.cardinality {
        FieldCardinality::One if actual.min <= occurrence => Err(schema_error(format!(
            "Required field {:?} occurrence {occurrence} is absent from some normalized production",
            field.name
        ))),
        FieldCardinality::Optional if actual.min > occurrence => Err(schema_error(format!(
            "Optional field {:?} occurrence {occurrence} is required by every normalized production",
            field.name
        ))),
        FieldCardinality::Many if actual.max.is_some_and(|maximum| maximum <= 1) => {
            Err(schema_error(format!(
                "Repeated field {:?} cannot occur more than once",
                field.name
            )))
        }
        _ if !reaches_occurrence => Err(schema_error(format!(
            "Field {:?} occurrence {occurrence} cannot occur in a normalized production",
            field.name
        ))),
        _ => Ok(()),
    }
}

struct NormalizedGrammar {
    visible: Vec<bool>,
    productions: Vec<Vec<Vec<u16>>>,
    // A normalized grammar is scoped to one typed-syntax emission, so these
    // derived summaries cannot outlive or cross-contaminate a language build.
    cardinality_summaries: BTreeMap<BTreeSet<u16>, Summaries>,
}

impl NormalizedGrammar {
    fn new(grammar: &CompiledGrammar) -> Result<Self, GeneratorError> {
        let term_count = usize::from(grammar.max_term) + 1;
        let mut visible = vec![false; term_count];
        for term in &grammar.syntax.visible_terms {
            let slot = visible.get_mut(usize::from(*term)).ok_or_else(|| {
                schema_error("Visible syntax term lies outside the generated term table")
            })?;
            *slot = true;
        }
        let mut productions = vec![Vec::new(); term_count];
        for production in &grammar.syntax.productions {
            let alternatives = productions
                .get_mut(usize::from(production.lhs))
                .ok_or_else(|| {
                    schema_error("Normalized production lhs lies outside the term table")
                })?;
            if production
                .rhs
                .iter()
                .any(|term| usize::from(*term) >= term_count)
            {
                return Err(schema_error(
                    "Normalized production rhs lies outside the term table",
                ));
            }
            alternatives.push(production.rhs.clone());
        }
        Ok(Self {
            visible,
            productions,
            cardinality_summaries: BTreeMap::new(),
        })
    }

    fn direct_cardinality(
        productions: &[Vec<Vec<u16>>],
        parent: u16,
        summaries: &Summaries,
    ) -> Result<Cardinality, GeneratorError> {
        let parent = usize::from(parent);
        let alternatives = productions.get(parent).ok_or_else(|| {
            schema_error(format!(
                "Typed parent term {parent} is outside the term table"
            ))
        })?;
        if alternatives.is_empty() {
            return Ok(Cardinality {
                min: 0,
                max: Some(0),
            });
        }
        let mut minimum = usize::MAX;
        let mut maximum = Some(0);
        for production in alternatives {
            let mut production_minimum = 0usize;
            let mut production_maximum = Some(0usize);
            for term in production {
                let index = usize::from(*term);
                let term_minimum = summaries.minimum[index];
                if term_minimum == usize::MAX {
                    return Err(schema_error(format!(
                        "Normalized term {term} has no finite derivation"
                    )));
                }
                production_minimum = production_minimum
                    .checked_add(term_minimum)
                    .ok_or_else(|| schema_error("Typed field minimum cardinality overflowed"))?;
                production_maximum = match (production_maximum, summaries.maximum[index]) {
                    (Some(left), Some(right)) => Some(
                        left.checked_add(right)
                            .ok_or_else(|| schema_error("Typed field cardinality overflowed"))?,
                    ),
                    _ => None,
                };
            }
            minimum = minimum.min(production_minimum);
            maximum = match (maximum, production_maximum) {
                (Some(left), Some(right)) => Some(left.max(right)),
                _ => None,
            };
        }
        Ok(Cardinality {
            min: minimum,
            max: maximum,
        })
    }

    fn direct_cardinality_for(
        &mut self,
        parents: &BTreeSet<u16>,
        targets: &BTreeSet<u16>,
    ) -> Result<Cardinality, GeneratorError> {
        let mut parents = parents.iter().copied();
        let first = parents
            .next()
            .ok_or_else(|| schema_error("Typed node has no grammar kinds"))?;
        self.ensure_cardinality_summaries(targets)?;
        let summaries = self
            .cardinality_summaries
            .get(targets)
            .expect("cardinality summaries were cached");
        let mut combined = Self::direct_cardinality(&self.productions, first, summaries)?;
        for parent in parents {
            let cardinality = Self::direct_cardinality(&self.productions, parent, summaries)?;
            combined.min = combined.min.min(cardinality.min);
            combined.max = match (combined.max, cardinality.max) {
                (Some(left), Some(right)) => Some(left.max(right)),
                _ => None,
            };
        }
        Ok(combined)
    }

    fn ensure_cardinality_summaries(
        &mut self,
        targets: &BTreeSet<u16>,
    ) -> Result<(), GeneratorError> {
        if !self.cardinality_summaries.contains_key(targets) {
            let summaries = self.compute_cardinality_summaries(targets)?;
            self.cardinality_summaries
                .insert(targets.clone(), summaries);
        }
        Ok(())
    }

    fn compute_cardinality_summaries(
        &self,
        targets: &BTreeSet<u16>,
    ) -> Result<Summaries, GeneratorError> {
        let term_count = self.productions.len();
        let mut minimum = vec![usize::MAX; term_count];
        for term in 0..term_count {
            if self.visible[term] {
                let term = u16::try_from(term)
                    .map_err(|_| schema_error("Generated term id does not fit u16"))?;
                minimum[usize::from(term)] = usize::from(targets.contains(&term));
            } else if self.productions[term].is_empty() {
                minimum[term] = 0;
            }
        }
        for _ in 0..=term_count {
            let mut changed = false;
            for term in 0..term_count {
                if self.visible[term] || self.productions[term].is_empty() {
                    continue;
                }
                let candidate = self.productions[term]
                    .iter()
                    .filter_map(|production| sum_known(production, &minimum))
                    .min();
                if let Some(candidate) = candidate
                    && candidate < minimum[term]
                {
                    minimum[term] = candidate;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        let cardinality_cap = term_count.saturating_add(1).max(2);
        let mut finite_maximum = vec![0usize; term_count];
        for (term, maximum) in finite_maximum.iter_mut().enumerate() {
            if self.visible[term] {
                let term = u16::try_from(term)
                    .map_err(|_| schema_error("Generated term id does not fit u16"))?;
                *maximum = usize::from(targets.contains(&term));
            }
        }
        for _ in 0..term_count {
            finite_maximum = self.next_maximum(&finite_maximum, cardinality_cap);
        }
        let next = self.next_maximum(&finite_maximum, cardinality_cap);
        let mut unbounded = next
            .iter()
            .zip(&finite_maximum)
            .map(|(next, current)| *current >= cardinality_cap || next > current)
            .collect::<Vec<_>>();
        loop {
            let mut changed = false;
            for term in 0..term_count {
                if self.visible[term] || unbounded[term] {
                    continue;
                }
                let derives_unbounded = self.productions[term]
                    .iter()
                    .any(|production| production.iter().any(|part| unbounded[usize::from(*part)]));
                if derives_unbounded {
                    unbounded[term] = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        let maximum = finite_maximum
            .into_iter()
            .zip(unbounded)
            .map(|(maximum, unbounded)| (!unbounded).then_some(maximum))
            .collect();
        Ok(Summaries { minimum, maximum })
    }

    fn next_maximum(&self, current: &[usize], cardinality_cap: usize) -> Vec<usize> {
        let mut next = current.to_vec();
        for (term, alternatives) in self.productions.iter().enumerate() {
            if self.visible[term] || alternatives.is_empty() {
                continue;
            }
            let maximum = alternatives
                .iter()
                .map(|production| {
                    production.iter().fold(0usize, |total, part| {
                        total
                            .saturating_add(current[usize::from(*part)])
                            .min(cardinality_cap)
                    })
                })
                .max()
                .unwrap_or(0);
            next[term] = maximum;
        }
        next
    }
}

struct Summaries {
    minimum: Vec<usize>,
    maximum: Vec<Option<usize>>,
}

fn sum_known(production: &[u16], values: &[usize]) -> Option<usize> {
    let mut total = 0usize;
    for term in production {
        let value = values[usize::from(*term)];
        if value == usize::MAX {
            return None;
        }
        total = total.checked_add(value)?;
    }
    Some(total)
}

fn emit_field(field: &CheckedField, language: &Ident, kind: &Ident) -> TokenStream {
    let name = &field.name;
    let occurrence = syn::Index::from(field.occurrence);
    match (&field.selector, field.cardinality) {
        (CheckedSelector::Typed { target }, FieldCardinality::Many) => {
            quote! {
                #[must_use]
                pub fn #name(&self) -> rezel_common::TypedChildren<#target> {
                    rezel_common::TypedChildren::new(self.syntax.children())
                }
            }
        }
        (CheckedSelector::Typed { target }, _) => {
            quote! {
                #[must_use]
                pub fn #name(&self) -> Option<#target> {
                    self.syntax
                        .children()
                        .filter_map(|node| {
                            <#target as rezel_common::TypedNode>::downcast_from(node).ok()
                        })
                        .nth(#occurrence)
                }
            }
        }
        (CheckedSelector::Token { kind: token_kind }, _) => {
            quote! {
                #[must_use]
                pub fn #name(&self) -> Option<rezel_common::SyntaxNode> {
                    self.syntax
                        .children()
                        .filter(|node| {
                            <#language as rezel_common::SyntaxLanguage>::kind(node)
                                == Some(#kind::#token_kind)
                        })
                        .nth(#occurrence)
                }
            }
        }
    }
}

struct GrammarKinds {
    by_name: BTreeMap<String, BTreeSet<u16>>,
}

impl GrammarKinds {
    fn new(grammar: &CompiledGrammar) -> Self {
        let visible = grammar
            .syntax
            .visible_terms
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let mut by_name = BTreeMap::new();
        for node in &grammar.node_types {
            if !visible.contains(&node.id) {
                continue;
            }
            by_name
                .entry(node.name.clone())
                .or_insert_with(BTreeSet::new)
                .insert(node.id);
        }
        Self { by_name }
    }

    fn resolve(&self, name: &str) -> Result<BTreeSet<u16>, GeneratorError> {
        self.by_name
            .get(name)
            .cloned()
            .ok_or_else(|| schema_error(format!("Unknown visible grammar kind {name:?}")))
    }

    fn kind_variants(
        &self,
        aliases: &BTreeMap<String, String>,
    ) -> Result<Vec<GrammarKind>, GeneratorError> {
        for name in aliases.keys() {
            if !self.by_name.contains_key(name) {
                return Err(schema_error(format!(
                    "Kind alias refers to unknown visible grammar kind {name:?}"
                )));
            }
        }
        let mut used = BTreeSet::new();
        let mut kinds = self
            .by_name
            .iter()
            .map(|(name, ids)| {
                let representative = ids
                    .iter()
                    .next()
                    .copied()
                    .expect("visible grammar kind has an internal term");
                let candidate = aliases
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| default_kind_name(name, representative));
                let identifier = rust_ident(&candidate, "kind variant")?;
                if !used.insert(identifier.to_string()) {
                    return Err(schema_error(format!(
                        "Grammar kind {name:?} maps to duplicate Rust variant {candidate:?}"
                    )));
                }
                Ok(GrammarKind {
                    ids: ids.clone(),
                    variant: identifier,
                })
            })
            .collect::<Result<Vec<_>, GeneratorError>>()?;
        kinds.sort_by_key(|kind| {
            kind.ids
                .iter()
                .next()
                .copied()
                .expect("visible grammar kind has an internal term")
        });
        Ok(kinds)
    }

    fn check_coverage(
        &self,
        coverage: SchemaCoverage,
        ignored: &[String],
        nodes: &[NodeSchema],
    ) -> Result<(), GeneratorError> {
        let mut accounted = BTreeSet::new();
        for name in ignored {
            if !self.by_name.contains_key(name) {
                return Err(schema_error(format!(
                    "Typed syntax ignore list refers to unknown visible grammar kind {name:?}"
                )));
            }
            if !accounted.insert(name.as_str()) {
                return Err(schema_error(format!(
                    "Typed syntax accounts for grammar kind {name:?} more than once"
                )));
            }
        }
        for node in nodes {
            if !self.by_name.contains_key(&node.kind) {
                continue;
            }
            if !accounted.insert(node.kind.as_str()) {
                return Err(schema_error(format!(
                    "Typed syntax accounts for grammar kind {:?} more than once",
                    node.kind
                )));
            }
        }
        if coverage == SchemaCoverage::Complete {
            let missing = self
                .by_name
                .keys()
                .filter(|name| !accounted.contains(name.as_str()))
                .cloned()
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                return Err(schema_error(format!(
                    "Complete typed syntax schema does not account for visible grammar kinds: {}",
                    missing
                        .iter()
                        .map(|name| format!("{name:?}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
        }
        Ok(())
    }
}

fn grammar_kind_variant<'a>(
    ids: &BTreeSet<u16>,
    variants: &'a [GrammarKind],
) -> Result<&'a Ident, GeneratorError> {
    variants
        .iter()
        .find_map(|candidate| (candidate.ids == *ids).then_some(&candidate.variant))
        .ok_or_else(|| {
            schema_error(format!(
                "Grammar terms {ids:?} have no generated semantic kind"
            ))
        })
}

fn default_kind_name(name: &str, term: u16) -> String {
    let mut output = String::new();
    let mut uppercase = true;
    for character in name.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            if uppercase {
                output.extend(character.to_uppercase());
                uppercase = false;
            } else {
                output.push(character);
            }
        } else {
            uppercase = true;
        }
    }
    if output.is_empty() {
        format!("Term{term}")
    } else {
        output
    }
}

fn rust_ident(name: &str, role: &str) -> Result<Ident, GeneratorError> {
    syn::parse_str::<Ident>(name)
        .map_err(|error| schema_error(format!("Invalid Rust {role} {name:?}: {error}")))
}

fn schema_error(message: impl Into<String>) -> GeneratorError {
    GeneratorError::new(message, None)
}
