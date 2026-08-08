use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::LitStr;

use crate::binary::BinaryTables;
use crate::source::format_generated_rust;
use crate::token::EncodedTokenTable;
use crate::{
    CompiledGrammar, GeneratorError, RustBindingKind, RustBindings, SpecializerMetadata,
    TokenizerMetadata,
};

/// Generated parser and term modules.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedRust {
    /// Thin Rust module containing the typed loader and executable parser glue.
    pub parser: String,
    /// Rust module containing exported term and dialect constants.
    pub terms: String,
    /// Native-layout parser data for little-endian targets.
    pub little_endian_data: Vec<u8>,
    /// Native-layout parser data for big-endian targets.
    pub big_endian_data: Vec<u8>,
}

/// Emit one static-table Rust parser.
///
/// The parser source expects sibling `generated.le.bin` and
/// `generated.be.bin` files containing the returned data.
///
/// # Errors
///
/// Returns missing/invalid external bindings or invalid generated Rust.
pub fn emit_rust(
    grammar: &CompiledGrammar,
    bindings: &RustBindings,
) -> Result<GeneratedRust, GeneratorError> {
    emit_rust_with_data_paths(grammar, bindings, "generated.le.bin", "generated.be.bin")
}

/// Emit one static-table Rust parser with explicit data paths.
///
/// The paths are embedded in the generated parser and are resolved relative to
/// that Rust source file.
///
/// # Errors
///
/// Returns missing/invalid external bindings or invalid generated Rust.
pub fn emit_rust_with_data_paths(
    grammar: &CompiledGrammar,
    bindings: &RustBindings,
    little_endian_path: &str,
    big_endian_path: &str,
) -> Result<GeneratedRust, GeneratorError> {
    bindings.validate(grammar)?;
    let terms = emit_terms(grammar)?;
    let emitted = emit_parser(grammar, bindings, little_endian_path, big_endian_path)?;
    Ok(GeneratedRust {
        parser: emitted.source,
        terms,
        little_endian_data: emitted.little_endian_data,
        big_endian_data: emitted.big_endian_data,
    })
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
    little_endian_path: &str,
    big_endian_path: &str,
) -> Result<EmittedParser, GeneratorError> {
    let mut source =
        String::from("#![allow(clippy::all, clippy::pedantic, missing_docs, unused_mut)]\n");
    let mut glue = String::new();
    let mut tables = BinaryTables::default();
    let local_names = emit_parser_arrays(&mut glue, &mut tables, grammar);
    let tokenizer_values = emit_tokenizers(grammar, bindings, &local_names)?;
    let dialects = emit_dialects(&mut tables, grammar);
    tables.push_dynamic_precedences("dynamic_precedences", &grammar.dynamic_precedences);
    let binary = tables.finish(little_endian_path, big_endian_path);
    source.push_str(&binary.declaration.to_string());
    source.push_str(&glue);
    let structural = emit_language_definition(grammar, bindings, &tokenizer_values, &dialects)?;
    source.push_str(&structural.to_string());
    let source = format_generated_rust(&source, "Generated parser is invalid Rust")?;
    Ok(EmittedParser {
        source,
        little_endian_data: binary.little_endian,
        big_endian_data: binary.big_endian,
    })
}

struct EmittedParser {
    source: String,
    little_endian_data: Vec<u8>,
    big_endian_data: Vec<u8>,
}

struct LocalTableNames {
    table: String,
    precedence: Ident,
}

fn emit_parser_arrays(
    source: &mut String,
    tables: &mut BinaryTables,
    grammar: &CompiledGrammar,
) -> BTreeMap<usize, LocalTableNames> {
    tables.push_u32("states", &grammar.states);
    tables.push_u16("state_data", &grammar.state_data);
    tables.push_u16("goto", &grammar.goto);
    write_token_table(source, tables, "TOKEN", "token", &grammar.token_table);
    grammar
        .tokenizers
        .iter()
        .enumerate()
        .filter_map(|(index, tokenizer)| match tokenizer {
            TokenizerMetadata::Local {
                table, precedence, ..
            } => {
                let prefix = format!("LOCAL_TOKEN_{index}");
                let field_prefix = format!("local_token_{index}");
                let table_name = write_token_table(source, tables, &prefix, &field_prefix, table);
                let precedence_name =
                    tables.push_u16(&format!("{field_prefix}_precedence"), precedence);
                Some((
                    index,
                    LocalTableNames {
                        table: table_name,
                        precedence: precedence_name,
                    },
                ))
            }
            _ => None,
        })
        .collect()
}

fn emit_tokenizers(
    grammar: &CompiledGrammar,
    bindings: &RustBindings,
    local_names: &BTreeMap<usize, LocalTableNames>,
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
                    TokenizerMetadata::Local { else_token, .. } => {
                        let names = local_names
                            .get(&index)
                            .expect("local tokenizer has emitted data");
                        let table = Ident::new(&names.table, Span::call_site());
                        let precedence = &names.precedence;
                        let fallback = option_u16(*else_token);
                        quote! {
                            rezel_lr::Tokenizer::Local(
                                rezel_lr::LocalTokenGroup::new(
                                    &#table,
                                    &TABLES.#precedence,
                                    #fallback,
                                )
                            )
                        }
                    }
                    TokenizerMetadata::External { binding, source } => {
                        let path = bindings.resolve(
                            RustBindingKind::ExternalTokenizer,
                            source,
                            binding,
                        )?;
                        quote!(rezel_lr::Tokenizer::External(&#path))
                    }
                })
            },
        )
        .collect()
}

fn emit_dialects(tables: &mut BinaryTables, grammar: &CompiledGrammar) -> Vec<TokenStream> {
    let dialect_term_arrays = grammar
        .dialects
        .iter()
        .enumerate()
        .map(|(index, (_, terms))| tables.push_u16(&format!("dialect_terms_{index}"), terms))
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
                    terms: &TABLES.#terms,
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
    let term_names = grammar.term_names.iter().map(|(term, name)| {
        let name = LitStr::new(name, Span::call_site());
        quote!((#term, #name))
    });

    let (specializer_functions, specializer_values) = emit_specializers(grammar, bindings)?;
    let node_set = emit_node_set(grammar, bindings)?;
    let context = if let Some(context) = &grammar.context {
        let path = bindings.resolve(
            RustBindingKind::ContextTracker,
            &context.source,
            &context.binding,
        )?;
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
        static SPECIALIZERS: &[rezel_lr::SpecializerSpec] = &[
            #(#specializer_values),*
        ];
        static TERM_NAMES: &[(u16, &str)] = &[
            #(#term_names),*
        ];

        pub static LANGUAGE: rezel_lr::Language = rezel_lr::Language {
            states: &TABLES.states,
            state_data: &TABLES.state_data,
            goto: &TABLES.goto,
            token_table: &TOKEN_TABLE,
            tokenizers: TOKENIZERS,
            top_rules: TOP_RULES,
            max_term: #max_term,
            min_repeat_term: #min_repeat_term,
            token_precedence: #token_precedence,
            node_set,
            context: #context,
            dialects: DIALECTS,
            dynamic_precedences: &TABLES.dynamic_precedences,
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
    let mut emitted = Vec::new();
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
                emitted.push((*term, function));
            }
            SpecializerMetadata::External {
                term,
                binding,
                source,
                extend,
            } => {
                let path =
                    bindings.resolve(RustBindingKind::ExternalSpecializer, source, binding)?;
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
                emitted.push((*term, function));
            }
        }
    }
    let values = if grammar.dialects.is_empty() {
        combine_specializers(&mut functions, emitted)
    } else {
        emitted
            .into_iter()
            .map(|(term, function)| specializer_spec(term, &function))
            .collect()
    };
    Ok((functions, values))
}

fn combine_specializers(
    functions: &mut TokenStream,
    emitted: Vec<(u16, Ident)>,
) -> Vec<TokenStream> {
    let mut groups: Vec<(u16, Vec<Ident>)> = Vec::new();
    for (term, function) in emitted {
        if let Some((_, members)) = groups
            .iter_mut()
            .find(|(known_term, _)| *known_term == term)
        {
            members.push(function);
        } else {
            groups.push((term, vec![function]));
        }
    }

    groups
        .into_iter()
        .enumerate()
        .map(|(index, (term, members))| {
            let function = if members.len() == 1 {
                members[0].clone()
            } else {
                let combined =
                    Ident::new(&format!("specialize_combined_{index}"), Span::call_site());
                let attempts = members.iter().map(|member| {
                    quote! {
                        if let Some(result) = #member(value, stack) {
                            return Some(result);
                        }
                    }
                });
                functions.extend(quote! {
                    fn #combined(
                        value: &str,
                        stack: &rezel_lr::Stack,
                    ) -> Option<rezel_lr::SpecializedToken> {
                        #(#attempts)*
                        None
                    }
                });
                combined
            };
            specializer_spec(term, &function)
        })
        .collect()
}

fn specializer_spec(term: u16, function: &Ident) -> TokenStream {
    quote! {
        rezel_lr::SpecializerSpec {
            term: #term,
            get: #function,
        }
    }
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
                bindings.resolve(
                    RustBindingKind::NodeProperty,
                    &property.source,
                    &property.binding,
                )?,
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
        .map(|source| {
            bindings.resolve(
                RustBindingKind::PropertySource,
                &source.source,
                &source.binding,
            )
        })
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

fn write_token_table(
    source: &mut String,
    tables: &mut BinaryTables,
    prefix: &str,
    field_prefix: &str,
    table: &EncodedTokenTable,
) -> String {
    let fields = tables.push_token_table(field_prefix, table);
    let table_name = format!("{prefix}_TABLE");
    let table_name_ident = Ident::new(&table_name, Span::call_site());
    let states = fields.states;
    let accepts = fields.accepts;
    let edges = fields.edges;
    let eof = fields.eof;
    source.push_str(
        &quote! {
            static #table_name_ident: rezel_lr::TokenTable = rezel_lr::TokenTable::new(
                &TABLES.#states,
                &TABLES.#accepts,
                &TABLES.#edges,
                &TABLES.#eof,
            );
        }
        .to_string(),
    );
    table_name
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
            | "gen"
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BuildOptions, compile_grammar};

    #[test]
    fn rust_2024_keywords_are_escaped_in_generated_identifiers() {
        let mut used = BTreeSet::new();

        assert_eq!(rust_identifier("gen", &mut used).to_string(), "_gen");
        assert_eq!(rust_identifier("_gen", &mut used).to_string(), "_gen_1");
    }

    #[test]
    fn same_base_specializers_are_combined_in_source_order_without_dialects() {
        let grammar = compile_grammar(
            r#"
@top T { (Contextual | kw<"word"> | Name)+ }
kw<word> { @specialize[@name={word}]<Name, word> }
@external specialize { Name } contextual from "./tokens" {
  Contextual[@name=contextual]
}
@tokens { Name { @asciiLetter+ } }
"#,
            None,
            BuildOptions::default(),
        )
        .unwrap();
        let bindings = RustBindings::from_toml_str(
            r#"
[[binding]]
kind = "external-specializer"
source = "./tokens"
name = "contextual"
rust_path = "crate::contextual"
"#,
        )
        .unwrap();

        let source = emit_rust(&grammar, &bindings).unwrap().parser;
        let external = source.find("crate::contextual(value, stack)").unwrap();
        let table = source.find("fn specialize_1(").unwrap();
        let first = source.find("specialize_0(value, stack)").unwrap();
        let second = source.find("specialize_1(value, stack)").unwrap();

        assert!(external < table);
        assert!(first < second);
        assert_eq!(source.matches("SpecializerSpec {").count(), 1);
        assert!(source.contains("get: specialize_combined_0"));
    }
}
