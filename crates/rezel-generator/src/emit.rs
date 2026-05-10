use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::{LitStr, Path};

use crate::source::format_generated_rust;
use crate::{CompiledGrammar, GeneratorError, SpecializerMetadata, TokenizerMetadata};

/// Static Rust symbols used for grammar-declared externals.
///
/// A binding key is the exact `(source, exported name)` pair from the grammar.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RustBindings {
    symbols: BTreeMap<(String, String), String>,
}

impl RustBindings {
    /// Add one external Rust item path.
    #[must_use]
    pub fn with(
        mut self,
        source: impl Into<String>,
        name: impl Into<String>,
        rust_path: impl Into<String>,
    ) -> Self {
        self.symbols
            .insert((source.into(), name.into()), rust_path.into());
        self
    }

    fn resolve(&self, source: &str, name: &str) -> Result<Path, GeneratorError> {
        let key = (source.to_owned(), name.to_owned());
        let Some(path) = self.symbols.get(&key) else {
            return Err(GeneratorError::new(
                format!("Missing Rust binding for external {name:?} from {source:?}"),
                None,
            ));
        };
        syn::parse_str(path).map_err(|error| {
            GeneratorError::new(
                format!("Invalid Rust path for external {name:?}: {error}"),
                None,
            )
        })
    }
}

/// Generated parser and term modules.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedRust {
    pub parser: String,
    pub terms: String,
}

/// Emit one static-table Rust parser.
///
/// # Errors
///
/// Returns missing/invalid external bindings or invalid generated Rust.
pub fn emit_rust(
    grammar: &CompiledGrammar,
    bindings: &RustBindings,
) -> Result<GeneratedRust, GeneratorError> {
    let terms = emit_terms(grammar)?;
    let parser = emit_parser(grammar, bindings)?;
    Ok(GeneratedRust { parser, terms })
}

/// Emit named term and dialect identifiers.
///
/// # Errors
///
/// Returns an error if generated identifiers cannot be parsed as Rust.
pub fn emit_terms(grammar: &CompiledGrammar) -> Result<String, GeneratorError> {
    let mut used = BTreeSet::new();
    let mut constants = TokenStream::new();
    for (name, value) in &grammar.terms {
        let identifier = rust_identifier(name, &mut used);
        constants.extend(quote!(pub const #identifier: u16 = #value;));
    }
    for (index, (name, _)) in grammar.dialects.iter().enumerate() {
        let identifier = rust_identifier(&format!("DIALECT_{name}"), &mut used);
        let index = u16::try_from(index)
            .map_err(|_| GeneratorError::new("Too many generated dialects", None))?;
        constants.extend(quote!(pub const #identifier: u16 = #index;));
    }
    let source = quote! {
        #![allow(non_upper_case_globals)]
        #constants
    };
    format_generated_rust(&source.to_string(), "Generated Rust is invalid")
}

fn emit_parser(
    grammar: &CompiledGrammar,
    bindings: &RustBindings,
) -> Result<String, GeneratorError> {
    let mut source =
        String::from("#![allow(clippy::all, clippy::pedantic, missing_docs, unused_mut)]\n");
    let local_names = emit_parser_arrays(&mut source, grammar);
    let tokenizer_values = emit_tokenizers(grammar, bindings, &local_names)?;
    let dialects = emit_dialects(&mut source, grammar);
    let structural = emit_language_definition(grammar, bindings, &tokenizer_values, &dialects)?;
    source.push_str(&structural.to_string());
    format_generated_rust(&source, "Generated parser is invalid Rust")
}

fn emit_parser_arrays(source: &mut String, grammar: &CompiledGrammar) -> BTreeMap<usize, String> {
    write_array(source, "STATES", "u32", &grammar.states);
    write_array(source, "STATE_DATA", "u16", &grammar.state_data);
    write_array(source, "GOTO", "u16", &grammar.goto);
    write_array(source, "TOKEN_DATA", "u16", &grammar.token_data);
    grammar
        .tokenizers
        .iter()
        .enumerate()
        .filter_map(|(index, tokenizer)| match tokenizer {
            TokenizerMetadata::Local { data, .. } => {
                let name = format!("LOCAL_TOKEN_DATA_{index}");
                write_array(source, &name, "u16", data);
                Some((index, name))
            }
            _ => None,
        })
        .collect()
}

fn emit_tokenizers(
    grammar: &CompiledGrammar,
    bindings: &RustBindings,
    local_names: &BTreeMap<usize, String>,
) -> Result<Vec<TokenStream>, GeneratorError> {
    grammar
        .tokenizers
        .iter()
        .enumerate()
        .map(
            |(index, tokenizer)| -> Result<TokenStream, GeneratorError> {
                Ok(match tokenizer {
                    TokenizerMetadata::Group { group_id } => {
                        quote! {
                            rezel_lr::Tokenizer::Group(
                                rezel_lr::TokenGroup::new(#group_id)
                            )
                        }
                    }
                    TokenizerMetadata::Local {
                        precedence_offset,
                        else_token,
                        ..
                    } => {
                        let data = Ident::new(
                            local_names
                                .get(&index)
                                .expect("local tokenizer has emitted data"),
                            Span::call_site(),
                        );
                        let fallback = option_u16(*else_token);
                        quote! {
                            rezel_lr::Tokenizer::Local(
                                rezel_lr::LocalTokenGroup::new(
                                    #data,
                                    #precedence_offset,
                                    #fallback,
                                )
                            )
                        }
                    }
                    TokenizerMetadata::External { binding, source } => {
                        let path = bindings.resolve(source, binding)?;
                        quote!(rezel_lr::Tokenizer::External(&#path))
                    }
                })
            },
        )
        .collect()
}

fn emit_dialects(source: &mut String, grammar: &CompiledGrammar) -> Vec<TokenStream> {
    let dialect_term_arrays = grammar
        .dialects
        .iter()
        .enumerate()
        .map(|(index, (_, terms))| {
            let name = format!("DIALECT_TERMS_{index}");
            write_array(source, &name, "u16", terms);
            Ident::new(&name, Span::call_site())
        })
        .collect::<Vec<_>>();
    grammar
        .dialects
        .iter()
        .zip(&dialect_term_arrays)
        .map(|((name, _), terms)| {
            let name = LitStr::new(name, Span::call_site());
            quote! {
                rezel_lr::DialectSpec {
                    name: #name,
                    terms: #terms,
                }
            }
        })
        .collect()
}

fn emit_language_definition(
    grammar: &CompiledGrammar,
    bindings: &RustBindings,
    tokenizer_values: &[TokenStream],
    dialects: &[TokenStream],
) -> Result<TokenStream, GeneratorError> {
    let top_rules = grammar.top_rules.iter().map(|top| {
        let name = LitStr::new(&top.name, Span::call_site());
        let state = top.state;
        let term = top.term;
        quote! {
            rezel_lr::TopRule {
                name: #name,
                state: #state,
                term: #term,
            }
        }
    });
    let dynamic_precedences = if grammar.dynamic_precedences.is_empty() {
        Vec::new()
    } else {
        let mut values = vec![0_i16; usize::from(grammar.max_term) + 1];
        for &(term, precedence) in &grammar.dynamic_precedences {
            values[usize::from(term)] = precedence;
        }
        values
    };
    let term_names = grammar.term_names.iter().map(|(term, name)| {
        let name = LitStr::new(name, Span::call_site());
        quote!((#term, #name))
    });

    let (specializer_functions, specializer_values) = emit_specializers(grammar, bindings)?;
    let node_set = emit_node_set(grammar, bindings)?;
    let context = if let Some(context) = &grammar.context {
        let path = bindings.resolve(&context.source, &context.binding)?;
        quote!(Some(&#path))
    } else {
        quote!(None)
    };
    let max_term = grammar.max_term;
    let min_repeat_term = grammar.min_repeat_term;
    let token_precedence = grammar.token_precedence;
    Ok(quote! {
        #specializer_functions
        #node_set

        static TOKENIZERS: &[rezel_lr::Tokenizer] = &[
            #(#tokenizer_values),*
        ];
        static TOP_RULES: &[rezel_lr::TopRule] = &[
            #(#top_rules),*
        ];
        static DIALECTS: &[rezel_lr::DialectSpec] = &[
            #(#dialects),*
        ];
        static DYNAMIC_PRECEDENCES: &[i16] = &[
            #(#dynamic_precedences),*
        ];
        static SPECIALIZERS: &[rezel_lr::SpecializerSpec] = &[
            #(#specializer_values),*
        ];
        static TERM_NAMES: &[(u16, &str)] = &[
            #(#term_names),*
        ];

        pub static LANGUAGE: rezel_lr::Language = rezel_lr::Language {
            states: STATES,
            state_data: STATE_DATA,
            goto: GOTO,
            token_data: TOKEN_DATA,
            tokenizers: TOKENIZERS,
            top_rules: TOP_RULES,
            max_term: #max_term,
            min_repeat_term: #min_repeat_term,
            token_precedence: #token_precedence,
            node_set,
            context: #context,
            dialects: DIALECTS,
            dynamic_precedences: DYNAMIC_PRECEDENCES,
            specializers: SPECIALIZERS,
            term_names: TERM_NAMES,
        };
    })
}

fn emit_specializers(
    grammar: &CompiledGrammar,
    bindings: &RustBindings,
) -> Result<(TokenStream, Vec<TokenStream>), GeneratorError> {
    let mut functions = TokenStream::new();
    let mut values = Vec::new();
    for (index, specializer) in grammar.specializers.iter().enumerate() {
        match specializer {
            SpecializerMetadata::Table { term, entries } => {
                let function = Ident::new(&format!("specialize_{index}"), Span::call_site());
                let arms = entries.iter().map(|(value, result)| {
                    let value = LitStr::new(value, Span::call_site());
                    let term = result.term;
                    let kind = specialize_kind(result.extend);
                    quote!(
                        #value => Some(rezel_lr::SpecializedToken::new(#term, #kind)),
                    )
                });
                functions.extend(quote! {
                    fn #function(
                        value: &str,
                        _stack: &rezel_lr::Stack,
                    ) -> Option<rezel_lr::SpecializedToken> {
                        match value {
                            #(#arms)*
                            _ => None,
                        }
                    }
                });
                values.push(quote! {
                    rezel_lr::SpecializerSpec {
                        term: #term,
                        get: #function,
                    }
                });
            }
            SpecializerMetadata::External {
                term,
                binding,
                source,
                extend,
            } => {
                let path = bindings.resolve(source, binding)?;
                let kind = specialize_kind(*extend);
                let function = Ident::new(&format!("specialize_{index}"), Span::call_site());
                functions.extend(quote! {
                    fn #function(
                        value: &str,
                        stack: &rezel_lr::Stack,
                    ) -> Option<rezel_lr::SpecializedToken> {
                        #path(value, stack).map(|term| {
                            rezel_lr::SpecializedToken::new(term, #kind)
                        })
                    }
                });
                values.push(quote! {
                    rezel_lr::SpecializerSpec {
                        term: #term,
                        get: #function,
                    }
                });
            }
        }
    }
    Ok((functions, values))
}

fn emit_node_set(
    grammar: &CompiledGrammar,
    bindings: &RustBindings,
) -> Result<TokenStream, GeneratorError> {
    let external_properties = grammar
        .external_properties
        .iter()
        .map(|property| {
            Ok((
                property.name.as_str(),
                bindings.resolve(&property.source, &property.binding)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, GeneratorError>>()?;
    let nodes = grammar.node_types.iter().map(|node| {
        let id = node.id;
        let name = LitStr::new(&node.name, Span::call_site());
        let flags = node_flags(node);
        let properties = node.properties.iter().filter_map(|(name, value)| {
            if matches!(name.as_str(), "repeated" | "error") {
                return None;
            }
            let value = LitStr::new(value, Span::call_site());
            let property = match name.as_str() {
                "closedBy" => quote!(rezel_common::closed_by_prop()),
                "openedBy" => quote!(rezel_common::opened_by_prop()),
                "group" => quote!(rezel_common::group_prop()),
                "isolate" => quote!(rezel_common::isolate_prop()),
                _ => {
                    let path = external_properties.get(name.as_str())?;
                    quote!(#path())
                }
            };
            Some(quote! {
                let property = #property;
                let value = property
                    .deserialize(#value)
                    .expect("generated grammar property must deserialize");
                node = node.with_prop(property, value);
            })
        });
        quote! {
            {
                let mut node = rezel_common::NodeType::new(#id, #name, #flags);
                #(#properties)*
                nodes.push(node);
            }
        }
    });
    let sources = grammar
        .property_sources
        .iter()
        .map(|source| bindings.resolve(&source.source, &source.binding))
        .collect::<Result<Vec<_>, _>>()?;
    let finish = if sources.is_empty() {
        quote!(rezel_common::NodeSet::new(nodes))
    } else {
        quote! {
            rezel_common::NodeSet::new(nodes).extend(&[
                #(#sources()),*
            ])
        }
    };
    Ok(quote! {
        fn node_set() -> &'static std::sync::Arc<rezel_common::NodeSet> {
            static NODE_SET: std::sync::OnceLock<std::sync::Arc<rezel_common::NodeSet>> =
                std::sync::OnceLock::new();
            NODE_SET.get_or_init(|| {
                let mut nodes = Vec::new();
                #(#nodes)*
                std::sync::Arc::new(#finish)
            })
        }
    })
}

fn node_flags(node: &crate::NodeMetadata) -> TokenStream {
    let mut flags = Vec::new();
    if node.flags.contains(rezel_common::NodeFlags::TOP) {
        flags.push(quote!(rezel_common::NodeFlags::TOP));
    }
    if node.flags.contains(rezel_common::NodeFlags::SKIPPED) {
        flags.push(quote!(rezel_common::NodeFlags::SKIPPED));
    }
    if node.flags.contains(rezel_common::NodeFlags::ERROR) {
        flags.push(quote!(rezel_common::NodeFlags::ERROR));
    }
    if node.flags.contains(rezel_common::NodeFlags::ANONYMOUS) {
        flags.push(quote!(rezel_common::NodeFlags::ANONYMOUS));
    }
    let Some(first) = flags.first().cloned() else {
        return quote!(rezel_common::NodeFlags::default());
    };
    flags
        .into_iter()
        .skip(1)
        .fold(first, |result, flag| quote!(#result | #flag))
}

fn specialize_kind(extend: bool) -> TokenStream {
    if extend {
        quote!(rezel_lr::Specialize::Extend)
    } else {
        quote!(rezel_lr::Specialize::Replace)
    }
}

fn option_u16(value: Option<u16>) -> TokenStream {
    value.map_or_else(|| quote!(None), |value| quote!(Some(#value)))
}

fn write_array<T>(source: &mut String, name: &str, ty: &str, values: &[T])
where
    T: std::fmt::Display,
{
    let _ = write!(source, "static {name}: &[{ty}] = &[");
    for value in values {
        let _ = write!(source, "{value},");
    }
    source.push_str("];\n");
}

fn rust_identifier(name: &str, used: &mut BTreeSet<String>) -> Ident {
    let mut normalized = String::new();
    for character in name.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            normalized.push(character);
        } else {
            normalized.push('_');
        }
    }
    if normalized.is_empty()
        || normalized == "_"
        || normalized
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
        || is_rust_keyword(&normalized)
    {
        normalized.insert(0, '_');
    }
    let base = normalized.clone();
    for suffix in 0.. {
        if suffix != 0 {
            normalized = format!("{base}_{suffix}");
        }
        if used.insert(normalized.clone()) {
            return Ident::new(&normalized, Span::call_site());
        }
    }
    unreachable!("unbounded suffix search finds a unique identifier")
}

fn is_rust_keyword(name: &str) -> bool {
    matches!(
        name,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "try"
    )
}
