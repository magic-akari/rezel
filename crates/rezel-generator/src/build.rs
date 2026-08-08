use std::collections::{BTreeMap, BTreeSet};

use rezel_common::NodeFlags;
use rezel_lr::table::{
    Action as RuntimeAction, GOTO_COMPRESSED_HEADER, GOTO_COMPRESSED_TAG, SequenceCode, StateField,
    StateFlag,
};

use crate::automaton::{
    Action, FirstSets, State, build_full_automaton, compute_first_sets, finish_automaton,
};
use crate::grammar::{Conflicts, FinishedTerms, Props, Rule, RuleId, TermId, TermSet};
use crate::node::{
    ConflictMarker, ConflictMarkerKind, Expression, ExpressionKind, ExternalSpecializeDeclaration,
    ExternalTokenDeclaration, GrammarDeclaration, Identifier, LiteralExpression, NameExpression,
    PrecKind, Prop, RepeatKind, RuleDeclaration, SpecializeExpression, SpecializeKind,
    TokenReference,
};
use crate::parse_grammar;
use crate::token::{EncodedTokenTable, TokenConflict, TokenDfa, TokenNfa};
use crate::{GeneratorError, GeneratorWarning};

const MIN_SHARED_ACTIONS: usize = 5;
const MAX_CODE_POINT: u32 = 0x10_ffff;

/// Options that affect generated static parser data.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BuildOptions {
    /// Retain names for non-node grammar terms.
    pub include_names: bool,
}

/// One generated node type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeMetadata {
    pub id: u16,
    pub name: String,
    pub flags: NodeFlags,
    pub properties: BTreeMap<String, String>,
}

/// One statically generated tokenizer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TokenizerMetadata {
    Group {
        group_id: u8,
    },
    Local {
        table: EncodedTokenTable,
        precedence: Vec<u16>,
        else_token: Option<u16>,
    },
    External {
        binding: String,
        source: String,
    },
}

/// One grammar-declared external property source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertySourceMetadata {
    pub binding: String,
    pub source: String,
}

/// One grammar-declared property key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalPropertyMetadata {
    pub name: String,
    pub binding: String,
    pub source: String,
}

/// One grammar-declared context tracker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextMetadata {
    pub binding: String,
    pub source: String,
}

/// One generated token specializer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpecializerMetadata {
    Table {
        term: u16,
        entries: BTreeMap<String, SpecializedTokenMetadata>,
    },
    External {
        term: u16,
        binding: String,
        source: String,
        extend: bool,
    },
}

/// One generated specialization-table result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpecializedTokenMetadata {
    pub term: u16,
    pub extend: bool,
}

/// One named grammar entry point.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TopRuleMetadata {
    pub name: String,
    pub state: u16,
    pub term: u16,
}

/// One normalized grammar production expressed in emitted term ids.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductionMetadata {
    pub lhs: u16,
    pub rhs: Vec<u16>,
}

/// Grammar structure retained only for typed-syntax schema validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxMetadata {
    /// Node terms that remain visible through the default CST traversal.
    pub visible_terms: Vec<u16>,
    /// Productions after EBNF lowering, inlining, and equivalent-rule merge.
    pub productions: Vec<ProductionMetadata>,
}

/// Static parser data produced from a grammar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledGrammar {
    pub states: Vec<u32>,
    pub state_data: Vec<u16>,
    pub goto: Vec<u16>,
    pub token_table: EncodedTokenTable,
    pub tokenizers: Vec<TokenizerMetadata>,
    pub top_rules: Vec<TopRuleMetadata>,
    pub max_term: u16,
    pub min_repeat_term: u16,
    pub token_precedence: usize,
    pub node_types: Vec<NodeMetadata>,
    pub external_properties: Vec<ExternalPropertyMetadata>,
    pub property_sources: Vec<PropertySourceMetadata>,
    pub context: Option<ContextMetadata>,
    pub dialects: Vec<(String, Vec<u16>)>,
    pub dynamic_precedences: Vec<(u16, i16)>,
    pub specializers: Vec<SpecializerMetadata>,
    pub term_names: Vec<(u16, String)>,
    pub terms: BTreeMap<String, u16>,
    pub syntax: SyntaxMetadata,
    pub warnings: Vec<GeneratorWarning>,
}

/// Compile grammar source into static parser data.
///
/// # Errors
///
/// Returns source, grammar, automaton, tokenizer, or table-size errors.
pub fn compile_grammar(
    source: &str,
    file_name: Option<&str>,
    options: BuildOptions,
) -> Result<CompiledGrammar, GeneratorError> {
    let declaration = parse_grammar(source, file_name)?;
    Builder::new(declaration, options)?.prepare()
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Parts {
    terms: Vec<TermId>,
    conflicts: Option<Vec<Conflicts>>,
}

impl Parts {
    const fn empty() -> Self {
        Self {
            terms: Vec::new(),
            conflicts: None,
        }
    }

    fn one(term: TermId) -> Self {
        Self {
            terms: vec![term],
            conflicts: None,
        }
    }

    fn concat(&self, other: &Self) -> Self {
        if self.terms.is_empty() && self.conflicts.is_none() {
            return other.clone();
        }
        if other.terms.is_empty() && other.conflicts.is_none() {
            return self.clone();
        }
        let conflicts = if self.conflicts.is_some() || other.conflicts.is_some() {
            let mut conflicts = self.ensure_conflicts();
            let other_conflicts = other.ensure_conflicts();
            let end = conflicts.len() - 1;
            conflicts[end] = conflicts[end].join(&other_conflicts[0]);
            conflicts.extend(other_conflicts.into_iter().skip(1));
            Some(conflicts)
        } else {
            None
        };
        let mut terms = self.terms.clone();
        terms.extend_from_slice(&other.terms);
        Self { terms, conflicts }
    }

    fn with_conflicts(&self, position: usize, added: &Conflicts) -> Self {
        if added == &Conflicts::default() {
            return self.clone();
        }
        let mut conflicts = self.ensure_conflicts();
        conflicts[position] = conflicts[position].join(added);
        Self {
            terms: self.terms.clone(),
            conflicts: Some(conflicts),
        }
    }

    fn ensure_conflicts(&self) -> Vec<Conflicts> {
        self.conflicts
            .clone()
            .unwrap_or_else(|| vec![Conflicts::default(); self.terms.len() + 1])
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BuiltRule {
    name: String,
    arguments: Vec<Expression>,
    term: TermId,
}

impl BuiltRule {
    fn matches(&self, expression: &NameExpression) -> bool {
        self.name == expression.id.name
            && expressions_structurally_eq(&self.arguments, &expression.arguments)
    }

    fn matches_repeat(&self, expression: &Expression) -> bool {
        self.name == "+"
            && self
                .arguments
                .first()
                .is_some_and(|known| known.structurally_eq(expression))
    }
}

fn expressions_structurally_eq(left: &[Expression], right: &[Expression]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.structurally_eq(right))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TokenSetId {
    Main,
    Local(usize),
}

#[derive(Clone, Debug)]
struct TokenArgument {
    name: String,
    expression: Expression,
    scope: Vec<TokenArgument>,
}

#[derive(Clone, Debug)]
struct BuildingTokenRule {
    name: String,
    start: usize,
    target: usize,
    arguments: Vec<Expression>,
}

#[derive(Clone, Debug)]
struct TokenPrecedence {
    term: TermId,
    after: Vec<TermId>,
}

#[derive(Clone, Debug)]
struct TokenSet {
    nfa: TokenNfa,
    built: Vec<BuiltRule>,
    building: Vec<BuildingTokenRule>,
    rules: Vec<RuleDeclaration>,
    dialect_terms: BTreeMap<usize, Vec<TermId>>,
    precedence: Vec<TokenPrecedence>,
    fallback: Option<TermId>,
}

impl TokenSet {
    fn new(rules: Vec<RuleDeclaration>) -> Self {
        Self {
            nfa: TokenNfa::new(),
            built: Vec::new(),
            building: Vec::new(),
            rules,
            dialect_terms: BTreeMap::new(),
            precedence: Vec::new(),
            fallback: None,
        }
    }
}

#[derive(Clone, Debug)]
struct ExternalTokenSet {
    declaration: ExternalTokenDeclaration,
    tokens: BTreeMap<String, TermId>,
}

#[derive(Clone, Debug)]
struct ExternalSpecializer {
    declaration: ExternalSpecializeDeclaration,
    tokens: BTreeMap<String, TermId>,
    term: Option<TermId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TokenOrigin {
    Specialized(TermId),
    External(usize),
    Local(usize),
    ExternalSpecializer { base: TermId, specializer: usize },
}

#[derive(Clone, Debug)]
struct Specialization {
    value: String,
    name: Option<String>,
    term: TermId,
    kind: SpecializeKind,
    dialect: Option<usize>,
}

#[derive(Clone, Debug)]
struct SkipInfo {
    skip: Vec<TermId>,
    rule: Option<TermId>,
    start_tokens: Vec<TermId>,
}

#[derive(Clone, Debug)]
struct TokenGroup {
    terms: Vec<TermId>,
    group_id: u8,
}

#[derive(Clone, Debug)]
struct TokenConflictSets {
    hard: Vec<TokenConflict>,
    soft: Vec<TokenConflict>,
}

type MainTokenTables = (Vec<TokenGroup>, Vec<u16>, EncodedTokenTable);

#[derive(Clone, Debug)]
struct BuiltTokenizer {
    metadata: TokenizerMetadata,
    group_id: Option<u8>,
    external: Option<usize>,
}

#[derive(Clone, Debug)]
struct SharedActions {
    actions: Vec<Action>,
    address: usize,
}

#[derive(Clone, Copy, Debug)]
struct StateFinishInput<'a> {
    skip_info: &'a [SkipInfo],
    skip_data: &'a [usize],
    tokenizers: &'a [BuiltTokenizer],
    forced_reductions: &'a [u32],
    non_skip_states: &'a BTreeSet<usize>,
}

#[derive(Clone, Copy, Debug)]
struct ActionStoreContext<'a> {
    rules: &'a [Rule],
    terms: &'a TermSet,
    skip_info: &'a [SkipInfo],
}

#[derive(Clone, Debug)]
struct GotoParents {
    parents: Vec<usize>,
    target: usize,
}

#[derive(Default)]
struct DataBuilder {
    data: Vec<u16>,
}

impl DataBuilder {
    fn store_array(&mut self, value: &[u16]) -> Result<usize, GeneratorError> {
        if let Some(found) = find_array(&self.data, value) {
            return Ok(found);
        }
        let position = self.data.len();
        self.data.extend_from_slice(value);
        if self.data.len() > usize::from(u16::MAX) {
            return Err(GeneratorError::new(
                "State data is too large for 16-bit offsets",
                None,
            ));
        }
        Ok(position)
    }

    fn finish(self) -> Vec<u16> {
        self.data
    }
}

struct Builder {
    ast: GrammarDeclaration,
    options: BuildOptions,
    terms: TermSet,
    main_tokens: TokenSet,
    local_tokens: Vec<TokenSet>,
    external_tokens: Vec<ExternalTokenSet>,
    external_specializers: Vec<ExternalSpecializer>,
    specialized: BTreeMap<TermId, Vec<Specialization>>,
    explicit_token_conflicts: BTreeSet<(TermId, TermId)>,
    token_origins: BTreeMap<TermId, TokenOrigin>,
    rules: Vec<Rule>,
    built: Vec<BuiltRule>,
    rule_names: BTreeMap<String, Option<Identifier>>,
    named_terms: BTreeMap<String, TermId>,
    dynamic_precedences: Vec<(TermId, i16)>,
    defined_groups: Vec<(TermId, String, RuleDeclaration)>,
    ast_rules: Vec<(TermId, RuleDeclaration)>,
    current_skip: Vec<TermId>,
    skip_rules: Vec<TermId>,
    warnings: Vec<GeneratorWarning>,
}

impl Builder {
    fn prepare(mut self) -> Result<CompiledGrammar, GeneratorError> {
        let mut preserve = self.skip_rules.clone();
        preserve.extend_from_slice(&self.terms.tops);
        let mut rules = simplify_rules(std::mem::take(&mut self.rules), &preserve, &self.terms);
        let finished = self.terms.finish(&mut rules)?;
        self.rules = rules;

        let term_table = self
            .named_terms
            .iter()
            .map(|(name, term)| (name.clone(), self.terms.output_id(*term)))
            .collect::<BTreeMap<_, _>>();
        let first = compute_first_sets(&self.terms, &self.rules);
        let (skip_info, start_terms) = self.prepare_skip_info(&first);
        let mut full = build_full_automaton(&self.terms, &self.rules, &start_terms, &first)?;

        let mut local_metadata = Vec::new();
        for index in 0..self.local_tokens.len() {
            let group_id = u8::try_from(index)
                .map_err(|_| GeneratorError::new("Too many local token groups", None))?;
            local_metadata.push(self.build_local_group(index, &mut full, &skip_info, group_id)?);
        }
        let start_group = u8::try_from(local_metadata.len())
            .map_err(|_| GeneratorError::new("Too many local token groups", None))?;
        let (groups, token_precedence, token_table) =
            self.build_main_token_groups(&mut full, &skip_info, start_group)?;
        self.check_external_conflicts(&full, &skip_info)?;

        let table = finish_automaton(&full, &self.rules, &self.terms);
        let non_skip_states = find_non_skip_states(&table, &self.terms.tops);
        let tokenizers = self.build_tokenizer_metadata(&groups, local_metadata);
        let mut data = DataBuilder::default();
        let skip_data = self.build_skip_data(&table, &skip_info, &mut data)?;
        let forced = self.compute_forced_reductions(&table, &skip_info);
        let state_input = StateFinishInput {
            skip_info: &skip_info,
            skip_data: &skip_data,
            tokenizers: &tokenizers,
            forced_reductions: &forced,
            non_skip_states: &non_skip_states,
        };
        let states = self.finish_states(&table, state_input, &mut data)?;
        let token_precedence_offset = data.store_array(&sequence_with_end(&token_precedence))?;
        let state_data = data.finish();
        let goto = compute_goto_table(&table, &self.terms)?;
        let node_types = self.gather_node_metadata(&finished);
        let syntax = self.gather_syntax_metadata(&finished);
        let top_rules = self.gather_top_rules(&table)?;
        let dialects = self.gather_dialects();
        let dynamic_precedences = self
            .dynamic_precedences
            .iter()
            .map(|(term, precedence)| (self.terms.output_id(*term), *precedence))
            .collect();
        let specializers = self.gather_specializers();
        let property_sources = self.gather_property_sources();
        let external_properties = self.gather_external_properties();
        let context = self.gather_context();
        let term_names = if self.options.include_names {
            finished.term_names.into_iter().collect()
        } else {
            Vec::new()
        };
        Ok(CompiledGrammar {
            states,
            state_data,
            goto,
            token_table,
            tokenizers: tokenizers
                .into_iter()
                .map(|tokenizer| tokenizer.metadata)
                .collect(),
            top_rules,
            max_term: finished.max_term,
            min_repeat_term: finished.min_repeat_term,
            token_precedence: token_precedence_offset,
            node_types,
            external_properties,
            property_sources,
            context,
            dialects,
            dynamic_precedences,
            specializers,
            term_names,
            terms: term_table,
            syntax,
            warnings: self.warnings,
        })
    }

    fn prepare_skip_info(&mut self, first: &FirstSets) -> (Vec<SkipInfo>, Vec<TermId>) {
        let mut start_terms = self.terms.tops.clone();
        let mut result = Vec::new();
        for skip_term in &self.skip_rules {
            let rule_ids = self.terms.terms[*skip_term].rules.clone();
            let mut skip = Vec::new();
            let mut start_tokens = Vec::new();
            let mut retained = Vec::new();
            for rule_id in rule_ids {
                let rule = &self.rules[rule_id];
                let Some(start) = rule.parts.first().copied() else {
                    continue;
                };
                if self.terms.terms[start].terminal() {
                    push_unique(&mut start_tokens, start);
                } else if let Some(entries) = first.get(&start) {
                    for term in entries.iter().flatten() {
                        push_unique(&mut start_tokens, *term);
                    }
                }
                let simple = self.terms.terms[start].terminal()
                    && rule.parts.len() == 1
                    && !self
                        .rules
                        .iter()
                        .any(|other| other.id != rule.id && other.parts.first() == Some(&start));
                if simple {
                    skip.push(start);
                } else {
                    retained.push(rule_id);
                }
            }
            self.terms.terms[*skip_term].rules.clone_from(&retained);
            let rule = (!retained.is_empty()).then_some(*skip_term);
            if rule.is_some() {
                start_terms.push(*skip_term);
            }
            result.push(SkipInfo {
                skip,
                rule,
                start_tokens,
            });
        }
        (result, start_terms)
    }

    fn build_tokenizer_metadata(
        &self,
        groups: &[TokenGroup],
        local: Vec<TokenizerMetadata>,
    ) -> Vec<BuiltTokenizer> {
        let token_start = self
            .ast
            .tokens
            .as_ref()
            .map_or(-1, |tokens| i64::try_from(tokens.start).unwrap_or(i64::MAX));
        let mut ordered = groups
            .iter()
            .map(|group| {
                (
                    token_start,
                    BuiltTokenizer {
                        metadata: TokenizerMetadata::Group {
                            group_id: group.group_id,
                        },
                        group_id: Some(group.group_id),
                        external: None,
                    },
                )
            })
            .collect::<Vec<_>>();
        ordered.extend(
            self.external_tokens
                .iter()
                .enumerate()
                .map(|(index, external)| {
                    (
                        i64::try_from(external.declaration.start).unwrap_or(i64::MAX),
                        BuiltTokenizer {
                            metadata: TokenizerMetadata::External {
                                binding: external.declaration.id.name.clone(),
                                source: external.declaration.source.clone(),
                            },
                            group_id: None,
                            external: Some(index),
                        },
                    )
                }),
        );
        ordered.sort_by_key(|(start, _)| *start);
        let mut result = ordered
            .into_iter()
            .map(|(_, tokenizer)| tokenizer)
            .collect::<Vec<_>>();
        result.extend(
            local
                .into_iter()
                .enumerate()
                .map(|(index, metadata)| BuiltTokenizer {
                    metadata,
                    group_id: u8::try_from(index).ok(),
                    external: None,
                }),
        );
        result
    }

    fn build_skip_data(
        &self,
        states: &[State],
        skip_info: &[SkipInfo],
        data: &mut DataBuilder,
    ) -> Result<Vec<usize>, GeneratorError> {
        skip_info
            .iter()
            .map(|info| {
                let mut actions = Vec::new();
                for term in &info.skip {
                    let action = RuntimeAction::shift(0, false, true).raw();
                    actions.push(self.terms.output_id(*term));
                    push_u32_words(&mut actions, action);
                }
                if let Some(rule) = info.rule {
                    let state = states
                        .iter()
                        .find(|state| state.start_rule == Some(rule))
                        .expect("skip rule has an automaton start state");
                    for action in &state.actions {
                        if let Action::Shift { term, .. } = action {
                            let action = RuntimeAction::shift(
                                u16::try_from(state.id).expect("state count fits generated format"),
                                true,
                                false,
                            )
                            .raw();
                            actions.push(self.terms.output_id(*term));
                            push_u32_words(&mut actions, action);
                        }
                    }
                }
                actions.push(SequenceCode::End.raw());
                actions.push(SequenceCode::Done.raw());
                data.store_array(&actions)
            })
            .collect()
    }

    fn finish_states(
        &self,
        states: &[State],
        input: StateFinishInput<'_>,
        data: &mut DataBuilder,
    ) -> Result<Vec<u32>, GeneratorError> {
        let mut result = vec![0_u32; states.len() * StateField::COUNT];
        let mut shared_actions: Vec<SharedActions> = Vec::new();
        let action_context = ActionStoreContext {
            rules: &self.rules,
            terms: &self.terms,
            skip_info: input.skip_info,
        };
        for state in states {
            let skip_index = self
                .skip_rules
                .iter()
                .position(|skip| *skip == state.skip)
                .expect("state skip set is registered");
            let is_skip = !input.non_skip_states.contains(&state.id);
            let default_reduce = state.default_reduce.map_or(0, |rule| {
                reduce_action(rule, None, &self.rules, &self.terms, input.skip_info)
            });
            let mut flags = 0;
            if is_skip {
                flags |= StateFlag::Skipped.mask();
            }
            if state.positions.iter().any(|position| {
                let rule = &self.rules[position.rule];
                self.terms.terms[rule.name].top() && position.dot == rule.parts.len()
            }) {
                flags |= StateFlag::Accepting.mask();
            }
            let mut skip_reduce = None;
            let mut shared = None;
            if default_reduce == 0 {
                if is_skip {
                    skip_reduce = state.actions.iter().find_map(|action| {
                        let Action::Reduce { term, rule } = action else {
                            return None;
                        };
                        self.terms.terms[*term].eof().then(|| {
                            reduce_action(*rule, None, &self.rules, &self.terms, input.skip_info)
                        })
                    });
                }
                if skip_reduce.is_none() {
                    shared = find_shared_actions(
                        state,
                        states,
                        &mut shared_actions,
                        data,
                        action_context,
                    )?;
                }
            }
            let actions = if default_reduce == 0 {
                state.actions.as_slice()
            } else {
                &[]
            };
            let action_offset =
                store_actions(actions, skip_reduce, shared.as_ref(), data, action_context)?;
            let tokenizer_mask =
                self.tokenizer_mask(state, &input.skip_info[skip_index], input.tokenizers);
            let base = state.id * StateField::COUNT;
            result[base + StateField::Flags.index()] = flags;
            result[base + StateField::Actions.index()] = u32::try_from(action_offset)
                .map_err(|_| GeneratorError::new("State data too large", None))?;
            result[base + StateField::Skip.index()] = u32::try_from(input.skip_data[skip_index])
                .map_err(|_| GeneratorError::new("State data too large", None))?;
            result[base + StateField::TokenizerMask.index()] = tokenizer_mask;
            result[base + StateField::DefaultReduce.index()] = default_reduce;
            result[base + StateField::ForcedReduce.index()] = input.forced_reductions[state.id];
        }
        Ok(result)
    }

    fn tokenizer_mask(&self, state: &State, skip: &SkipInfo, tokenizers: &[BuiltTokenizer]) -> u32 {
        let mut external = BTreeSet::new();
        for mut term in state
            .actions
            .iter()
            .map(Action::term)
            .chain(skip.start_tokens.iter().copied())
        {
            loop {
                match self.token_origins.get(&term).copied() {
                    Some(
                        TokenOrigin::Specialized(base)
                        | TokenOrigin::ExternalSpecializer { base, .. },
                    ) => term = base,
                    Some(TokenOrigin::External(index)) => {
                        external.insert(index);
                        break;
                    }
                    _ => break,
                }
            }
        }
        let mut mask = 0;
        for (index, tokenizer) in tokenizers.iter().enumerate() {
            if tokenizer.group_id == state.token_group
                || tokenizer
                    .external
                    .is_some_and(|external_id| external.contains(&external_id))
            {
                mask |= 1_u32 << index;
            }
        }
        mask
    }

    fn compute_forced_reductions(&self, states: &[State], skip_info: &[SkipInfo]) -> Vec<u32> {
        let mut reductions = vec![0; states.len()];
        let mut candidates = vec![Vec::new(); states.len()];
        let mut goto_edges: BTreeMap<TermId, Vec<GotoParents>> = BTreeMap::new();
        for state in states {
            for action in &state.gotos {
                let Action::Shift { term, target } = *action else {
                    continue;
                };
                let entries = goto_edges.entry(term).or_default();
                if let Some(entry) = entries.iter_mut().find(|entry| entry.target == target) {
                    entry.parents.push(state.id);
                } else {
                    entries.push(GotoParents {
                        parents: vec![state.id],
                        target,
                    });
                }
            }
            let mut positions = state
                .positions
                .iter()
                .filter(|position| {
                    position.dot > 0 && !self.terms.terms[self.rules[position.rule].name].top()
                })
                .cloned()
                .collect::<Vec<_>>();
            positions.sort_by(|left, right| {
                right.dot.cmp(&left.dot).then_with(|| {
                    self.rules[left.rule]
                        .parts
                        .len()
                        .cmp(&self.rules[right.rule].parts.len())
                })
            });
            candidates[state.id] = positions;
        }
        let mut length_one = BTreeMap::new();
        for state in states {
            if let Some(rule) = state.default_reduce
                && !self.rules[rule].parts.is_empty()
            {
                reductions[state.id] =
                    reduce_action(rule, None, &self.rules, &self.terms, skip_info);
                if self.rules[rule].parts.len() == 1 {
                    length_one.insert(state.id, self.rules[rule].name);
                }
            }
        }
        for set_size in 1.. {
            let mut done = true;
            for state in states {
                if state.default_reduce.is_some() {
                    continue;
                }
                let positions = &candidates[state.id];
                if positions.len() != set_size {
                    done &= positions.len() < set_size;
                    continue;
                }
                for position in positions {
                    let term = self.rules[position.rule].name;
                    if position.dot != 1
                        || !creates_forced_cycle(term, state.id, None, &goto_edges, &length_one)
                    {
                        reductions[state.id] = reduce_action(
                            position.rule,
                            Some(position.dot),
                            &self.rules,
                            &self.terms,
                            skip_info,
                        );
                        if position.dot == 1 {
                            length_one.insert(state.id, term);
                        }
                        break;
                    }
                }
            }
            if done {
                return reductions;
            }
        }
        unreachable!("finite candidate sets terminate forced reduction selection")
    }

    fn gather_node_metadata(&self, finished: &FinishedTerms) -> Vec<NodeMetadata> {
        let non_skipped = self.gather_non_skipped_nodes();
        let mut result = finished
            .node_types
            .iter()
            .map(|term_id| {
                let term = &self.terms.terms[*term_id];
                let mut flags = NodeFlags::default();
                if term.top() {
                    flags |= NodeFlags::TOP;
                }
                if *term_id == self.terms.error {
                    flags |= NodeFlags::ERROR;
                }
                if !non_skipped.contains(term_id) && !term.error() {
                    flags |= NodeFlags::SKIPPED;
                }
                if term.node_name.is_none() {
                    flags |= NodeFlags::ANONYMOUS;
                }
                NodeMetadata {
                    id: self.terms.output_id(*term_id),
                    name: term.node_name.clone().unwrap_or_default(),
                    flags,
                    properties: term.props.clone(),
                }
            })
            .collect::<Vec<_>>();
        result.sort_by_key(|node| node.id);
        result
    }

    fn gather_syntax_metadata(&self, finished: &FinishedTerms) -> SyntaxMetadata {
        let visible_terms = finished
            .node_types
            .iter()
            .copied()
            .filter(|term| {
                let term = &self.terms.terms[*term];
                !term.repeated() && term.node_name.is_some()
            })
            .map(|term| self.terms.output_id(term))
            .collect();
        let productions = self
            .rules
            .iter()
            .map(|rule| ProductionMetadata {
                lhs: self.terms.output_id(rule.name),
                rhs: rule
                    .parts
                    .iter()
                    .map(|part| self.terms.output_id(*part))
                    .collect(),
            })
            .collect();
        SyntaxMetadata {
            visible_terms,
            productions,
        }
    }

    fn gather_non_skipped_nodes(&self) -> BTreeSet<TermId> {
        let mut seen = BTreeSet::new();
        let mut work = Vec::new();
        for term in &self.terms.tops {
            if seen.insert(*term) {
                work.push(*term);
            }
        }
        let mut index = 0;
        while index < work.len() {
            for rule_id in &self.terms.terms[work[index]].rules {
                for part in &self.rules[*rule_id].parts {
                    if seen.insert(*part) {
                        work.push(*part);
                    }
                }
            }
            index += 1;
        }
        seen
    }

    fn gather_top_rules(&self, states: &[State]) -> Result<Vec<TopRuleMetadata>, GeneratorError> {
        self.terms
            .tops
            .iter()
            .map(|term| {
                let state = states
                    .iter()
                    .find(|state| state.start_rule == Some(*term))
                    .ok_or_else(|| GeneratorError::new("Top rule has no automaton state", None))?;
                Ok(TopRuleMetadata {
                    name: self.terms.terms[*term]
                        .node_name
                        .clone()
                        .expect("top rules have public names"),
                    state: u16::try_from(state.id)
                        .map_err(|_| GeneratorError::new("Too many parser states", None))?,
                    term: self.terms.output_id(*term),
                })
            })
            .collect()
    }

    fn gather_specializers(&self) -> Vec<SpecializerMetadata> {
        let mut result = Vec::new();
        for external in &self.external_specializers {
            result.push(SpecializerMetadata::External {
                term: self.terms.output_id(
                    external
                        .term
                        .expect("external specializer is resolved during build"),
                ),
                binding: external.declaration.id.name.clone(),
                source: external.declaration.source.clone(),
                extend: external.declaration.kind == SpecializeKind::Extend,
            });
        }
        for (base, entries) in &self.specialized {
            let entries = entries
                .iter()
                .map(|entry| {
                    (
                        entry.value.clone(),
                        SpecializedTokenMetadata {
                            term: self.terms.output_id(entry.term),
                            extend: entry.kind == SpecializeKind::Extend,
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if !entries.is_empty() {
                result.push(SpecializerMetadata::Table {
                    term: self.terms.output_id(*base),
                    entries,
                });
            }
        }
        result
    }

    fn gather_dialects(&self) -> Vec<(String, Vec<u16>)> {
        self.ast
            .dialects
            .iter()
            .enumerate()
            .map(|(index, dialect)| {
                let terms = self
                    .main_tokens
                    .dialect_terms
                    .get(&index)
                    .into_iter()
                    .flatten()
                    .map(|term| self.terms.output_id(*term))
                    .collect();
                (dialect.name.clone(), terms)
            })
            .collect()
    }

    fn gather_property_sources(&self) -> Vec<PropertySourceMetadata> {
        self.ast
            .external_prop_sources
            .iter()
            .map(|source| PropertySourceMetadata {
                binding: source.id.name.clone(),
                source: source.source.clone(),
            })
            .collect()
    }

    fn gather_external_properties(&self) -> Vec<ExternalPropertyMetadata> {
        self.ast
            .external_props
            .iter()
            .map(|property| ExternalPropertyMetadata {
                name: property.id.name.clone(),
                binding: property.external_id.name.clone(),
                source: property.source.clone(),
            })
            .collect()
    }

    fn gather_context(&self) -> Option<ContextMetadata> {
        self.ast.context.as_ref().map(|context| ContextMetadata {
            binding: context.id.name.clone(),
            source: context.source.clone(),
        })
    }

    fn new(ast: GrammarDeclaration, options: BuildOptions) -> Result<Self, GeneratorError> {
        let main_rules = ast
            .tokens
            .as_ref()
            .map_or_else(Vec::new, |tokens| tokens.rules.clone());
        let local_tokens = ast
            .local_tokens
            .iter()
            .map(|tokens| TokenSet::new(tokens.rules.clone()))
            .collect();
        let builder = Self {
            ast,
            options,
            terms: TermSet::new(),
            main_tokens: TokenSet::new(main_rules),
            local_tokens,
            external_tokens: Vec::new(),
            external_specializers: Vec::new(),
            specialized: BTreeMap::new(),
            explicit_token_conflicts: BTreeSet::new(),
            token_origins: BTreeMap::new(),
            rules: Vec::new(),
            built: Vec::new(),
            rule_names: BTreeMap::new(),
            named_terms: BTreeMap::new(),
            dynamic_precedences: Vec::new(),
            defined_groups: Vec::new(),
            ast_rules: Vec::new(),
            current_skip: Vec::new(),
            skip_rules: Vec::new(),
            warnings: Vec::new(),
        };
        builder.initialize()
    }

    fn initialize(mut self) -> Result<Self, GeneratorError> {
        self.register_token_declarations()?;
        let top_rules = self.prepare_skip_scopes()?;
        self.build_top_rules(top_rules)?;
        self.finish_declarations()?;
        Ok(self)
    }

    fn register_token_declarations(&mut self) -> Result<(), GeneratorError> {
        let main_rules = self.main_tokens.rules.clone();
        for rule in &main_rules {
            self.unique(&rule.id)?;
        }
        let local_rules = self
            .local_tokens
            .iter()
            .flat_map(|tokens| tokens.rules.clone())
            .collect::<Vec<_>>();
        for rule in &local_rules {
            self.unique(&rule.id)?;
        }
        let local_fallbacks = self
            .ast
            .local_tokens
            .iter()
            .filter_map(|tokens| tokens.fallback.clone())
            .collect::<Vec<_>>();
        for fallback in &local_fallbacks {
            self.unique(&fallback.id)?;
        }

        let external_tokens = self.ast.external_tokens.clone();
        for declaration in external_tokens {
            let tokens = self.gather_external_tokens(&declaration.tokens)?;
            let index = self.external_tokens.len();
            for term in tokens.values() {
                self.token_origins
                    .insert(*term, TokenOrigin::External(index));
            }
            self.external_tokens.push(ExternalTokenSet {
                declaration,
                tokens,
            });
        }
        let external_specializers = self.ast.external_specializers.clone();
        for declaration in external_specializers {
            let tokens = self.gather_external_tokens(&declaration.tokens)?;
            self.external_specializers.push(ExternalSpecializer {
                declaration,
                tokens,
                term: None,
            });
        }
        Ok(())
    }

    fn prepare_skip_scopes(&mut self) -> Result<Vec<(TermId, RuleDeclaration)>, GeneratorError> {
        let no_skip = self.new_name("%noskip", None, Props::new());
        self.define_rule(no_skip, Vec::new());
        let main_skip_expression = self.ast.main_skip.clone();
        let main_skip = if main_skip_expression.is_some() {
            self.new_name("%mainskip", None, Props::new())
        } else {
            no_skip
        };

        for rule in self.ast.rules.clone() {
            self.ast_rules.push((main_skip, rule));
        }
        let mut top_rules = self
            .ast
            .top_rules
            .clone()
            .into_iter()
            .map(|rule| (main_skip, rule))
            .collect::<Vec<_>>();
        let mut scoped_skip_terms = Vec::new();
        for scoped in self.ast.scoped_skip.clone() {
            let term = if let Some(found) = self
                .ast
                .scoped_skip
                .iter()
                .take(scoped_skip_terms.len())
                .position(|known| known.expression.structurally_eq(&scoped.expression))
            {
                scoped_skip_terms[found]
            } else if main_skip_expression
                .as_ref()
                .is_some_and(|main| main.structurally_eq(&scoped.expression))
            {
                main_skip
            } else if is_empty(&scoped.expression) {
                no_skip
            } else {
                self.new_name("%skip", None, Props::new())
            };
            scoped_skip_terms.push(term);
            self.ast_rules
                .extend(scoped.rules.into_iter().map(|rule| (term, rule)));
            top_rules.extend(scoped.top_rules.into_iter().map(|rule| (term, rule)));
        }

        let grammar_rules = self
            .ast_rules
            .iter()
            .map(|(_, rule)| rule.clone())
            .collect::<Vec<_>>();
        for rule in &grammar_rules {
            self.unique(&rule.id)?;
        }

        self.current_skip.push(no_skip);
        self.skip_rules.push(no_skip);
        if main_skip != no_skip {
            self.skip_rules.push(main_skip);
            let choices = self.normalize_expression(
                main_skip_expression
                    .as_ref()
                    .expect("a distinct main skip has an expression"),
            )?;
            self.define_rule(main_skip, choices);
        }
        for (index, term) in scoped_skip_terms.iter().copied().enumerate() {
            if self.skip_rules.contains(&term) {
                continue;
            }
            self.skip_rules.push(term);
            if term != no_skip {
                let expression = self.ast.scoped_skip[index].expression.clone();
                let choices = self.normalize_expression(&expression)?;
                self.define_rule(term, choices);
            }
        }
        self.current_skip.pop();
        Ok(top_rules)
    }

    fn build_top_rules(
        &mut self,
        mut top_rules: Vec<(TermId, RuleDeclaration)>,
    ) -> Result<(), GeneratorError> {
        top_rules.sort_by_key(|(_, rule)| rule.start);
        for (skip, rule) in top_rules {
            self.unique(&rule.id)?;
            self.mark_used(&rule.id.name);
            self.current_skip.push(skip);
            let request = NodeInfoRequest::new(&rule.props, "a", Some(&rule.id.name))
                .with_expression(&rule.expression);
            let info = self.node_info(request)?;
            let term = self.terms.make_top(info.name.clone(), info.properties);
            if let Some(name) = info.name {
                self.named_terms.insert(name, term);
            }
            let choices = self.normalize_expression(&rule.expression)?;
            self.define_rule(term, choices);
            self.current_skip.pop();
        }
        Ok(())
    }

    fn finish_declarations(&mut self) -> Result<(), GeneratorError> {
        for index in 0..self.external_specializers.len() {
            self.finish_external_specializer(index)?;
        }

        let ast_rules = self.ast_rules.clone();
        for (skip, rule) in ast_rules {
            let is_used = self
                .rule_names
                .get(&rule.id.name)
                .is_some_and(Option::is_some);
            if is_used && is_exported(&rule) && rule.params.is_empty() {
                self.build_rule(&rule, &[], skip, false)?;
                if is_empty(&rule.expression) {
                    self.mark_used(&rule.id.name);
                }
            }
        }

        let unused = self
            .rule_names
            .values()
            .filter_map(Clone::clone)
            .collect::<Vec<_>>();
        for rule in unused {
            self.warn(format!("Unused rule '{}'", rule.name), rule.start);
        }

        self.take_token_precedences(TokenSetId::Main);
        self.take_token_conflicts();
        for index in 0..self.local_tokens.len() {
            self.take_token_precedences(TokenSetId::Local(index));
        }
        for (term, group, rule) in self.defined_groups.clone() {
            self.define_group(term, &group, &rule)?;
        }
        self.check_groups();
        Ok(())
    }

    fn unique(&mut self, identifier: &Identifier) -> Result<(), GeneratorError> {
        if self.rule_names.contains_key(&identifier.name) {
            return Err(self.error(
                format!("Duplicate definition of rule '{}'", identifier.name),
                identifier.start,
            ));
        }
        self.rule_names
            .insert(identifier.name.clone(), Some(identifier.clone()));
        Ok(())
    }

    fn mark_used(&mut self, name: &str) {
        self.rule_names.insert(name.to_owned(), None);
    }

    fn new_name(&mut self, base: &str, node_name: Option<String>, properties: Props) -> TermId {
        let start = usize::from(node_name.is_none());
        for suffix in start.. {
            let candidate = if suffix == 0 {
                base.to_owned()
            } else {
                format!("{base}-{suffix}")
            };
            if !self.terms.names.contains_key(&candidate) {
                return self
                    .terms
                    .make_nonterminal(candidate, node_name, properties);
            }
        }
        unreachable!("unbounded suffix search always finds an unused name")
    }

    fn make_terminal(
        &mut self,
        name: &str,
        node_name: Option<String>,
        properties: Props,
    ) -> TermId {
        let name = self.terms.unique_name(name);
        self.terms.make_terminal(name, node_name, properties)
    }

    fn define_rule(&mut self, name: TermId, choices: Vec<Parts>) {
        for choice in choices {
            let skip = *self
                .current_skip
                .last()
                .expect("rule construction always has a skip context");
            let conflicts = choice.ensure_conflicts();
            self.rules.push(Rule {
                id: self.rules.len(),
                name,
                parts: choice.terms,
                conflicts,
                skip,
            });
        }
    }

    fn resolve(&mut self, expression: &NameExpression) -> Result<Vec<Parts>, GeneratorError> {
        if let Some(term) = self
            .built
            .iter()
            .find(|built| built.matches(expression))
            .map(|built| built.term)
        {
            return Ok(vec![Parts::one(term)]);
        }
        if let Some(term) = self.get_token(TokenSetId::Main, expression)? {
            return Ok(vec![Parts::one(term)]);
        }
        for index in 0..self.local_tokens.len() {
            if let Some(term) = self.get_local_token(index, expression)? {
                return Ok(vec![Parts::one(term)]);
            }
        }
        for external in &self.external_tokens {
            if let Some(term) = external.tokens.get(&expression.id.name) {
                if !expression.arguments.is_empty() {
                    return Err(self.error(
                        "External tokens cannot take arguments",
                        expression.arguments[0].start,
                    ));
                }
                let term = *term;
                self.mark_used(&expression.id.name);
                return Ok(vec![Parts::one(term)]);
            }
        }
        for specializer in &self.external_specializers {
            if let Some(term) = specializer.tokens.get(&expression.id.name) {
                if !expression.arguments.is_empty() {
                    return Err(self.error(
                        "External tokens cannot take arguments",
                        expression.arguments[0].start,
                    ));
                }
                let term = *term;
                self.mark_used(&expression.id.name);
                return Ok(vec![Parts::one(term)]);
            }
        }

        let known = self
            .ast_rules
            .iter()
            .find(|(_, rule)| rule.id.name == expression.id.name)
            .cloned();
        let Some((skip, rule)) = known else {
            return Err(self.error(
                format!("Reference to undefined rule '{}'", expression.id.name),
                expression.start,
            ));
        };
        if rule.params.len() != expression.arguments.len() {
            return Err(self.error(
                format!("Wrong number of arguments for '{}'", expression.id.name),
                expression.start,
            ));
        }
        self.mark_used(&rule.id.name);
        let term = self.build_rule(&rule, &expression.arguments, skip, false)?;
        Ok(vec![Parts::one(term)])
    }

    fn normalize_repeat(&mut self, expression: &Expression) -> Result<Parts, GeneratorError> {
        if let Some(term) = self
            .built
            .iter()
            .find(|built| built.matches_repeat(expression))
            .map(|built| built.term)
        {
            return Ok(Parts::one(term));
        }
        let name = format!("{}+", format_expression(expression));
        let term = self.terms.make_repeat(self.terms.unique_name(&name));
        self.built.push(BuiltRule {
            name: "+".to_owned(),
            arguments: vec![expression.clone()],
            term,
        });
        let mut choices = self.normalize_expression(expression)?;
        choices.push(Parts {
            terms: vec![term, term],
            conflicts: None,
        });
        self.define_rule(term, choices);
        Ok(Parts::one(term))
    }

    fn normalize_sequence(
        &mut self,
        expressions: &[Expression],
        markers: &[Vec<ConflictMarker>],
    ) -> Result<Vec<Parts>, GeneratorError> {
        let choices = expressions
            .iter()
            .map(|expression| self.normalize_expression(expression))
            .collect::<Result<Vec<_>, _>>()?;
        self.complete_sequence(&choices, markers, &Parts::empty(), 0, &Conflicts::default())
    }

    fn complete_sequence(
        &self,
        choices: &[Vec<Parts>],
        markers: &[Vec<ConflictMarker>],
        start: &Parts,
        from: usize,
        end_conflicts: &Conflicts,
    ) -> Result<Vec<Parts>, GeneratorError> {
        let (here, at_end) = self.conflicts_for(&markers[from])?;
        if from == choices.len() {
            let end = start.terms.len();
            return Ok(vec![start.with_conflicts(end, &here.join(end_conflicts))]);
        }
        let mut result = Vec::new();
        for choice in &choices[from] {
            let position = start.terms.len();
            let next = start.concat(choice).with_conflicts(position, &here);
            result.extend(self.complete_sequence(
                choices,
                markers,
                &next,
                from + 1,
                &end_conflicts.join(&at_end),
            )?);
        }
        Ok(result)
    }

    fn normalize_expression(
        &mut self,
        expression: &Expression,
    ) -> Result<Vec<Parts>, GeneratorError> {
        match &expression.kind {
            ExpressionKind::Repeat {
                expression: inner,
                kind: RepeatKind::Optional,
            } => {
                let mut result = vec![Parts::empty()];
                result.extend(self.normalize_expression(inner)?);
                Ok(result)
            }
            ExpressionKind::Repeat {
                expression: inner,
                kind,
            } => {
                let repeated = self.normalize_repeat(inner)?;
                if *kind == RepeatKind::OneOrMore {
                    Ok(vec![repeated])
                } else {
                    Ok(vec![Parts::empty(), repeated])
                }
            }
            ExpressionKind::Choice(expressions) => {
                let mut result = Vec::new();
                for expression in expressions {
                    result.extend(self.normalize_expression(expression)?);
                }
                Ok(result)
            }
            ExpressionKind::Sequence {
                expressions,
                markers,
                ..
            } => self.normalize_sequence(expressions, markers),
            ExpressionKind::Literal(literal) => {
                let term = self.get_literal(literal)?;
                Ok(vec![Parts::one(term)])
            }
            ExpressionKind::Name(name) => self.resolve(name),
            ExpressionKind::Specialize(specialize) => {
                let term = self.resolve_specialization(specialize)?;
                Ok(vec![Parts::one(term)])
            }
            ExpressionKind::InlineRule(rule) => {
                let skip = *self
                    .current_skip
                    .last()
                    .expect("inline rules have a skip context");
                let term = self.build_rule(rule, &[], skip, true)?;
                Ok(vec![Parts::one(term)])
            }
            _ => Err(self.error(
                format!(
                    "This expression may not occur in non-token rules: {}",
                    format_expression(expression)
                ),
                expression.start,
            )),
        }
    }

    fn build_rule(
        &mut self,
        rule: &RuleDeclaration,
        arguments: &[Expression],
        skip: TermId,
        inline: bool,
    ) -> Result<TermId, GeneratorError> {
        let expression = self.substitute_arguments(&rule.expression, arguments, &rule.params)?;
        let allow = if inline { "pg" } else { "pgi" };
        let request = NodeInfoRequest::new(&rule.props, allow, Some(&rule.id.name))
            .with_template(arguments, &rule.params)
            .with_expression(&rule.expression);
        let info = self.node_info(request)?;
        if info.exported.is_some() && !rule.params.is_empty() {
            self.warn("Can't export parameterized rules", rule.start);
        }
        if info.exported.is_some() && inline {
            self.warn("Can't export inline rule", rule.start);
        }
        let mut internal_name = rule.id.name.clone();
        if !arguments.is_empty() {
            let arguments = arguments
                .iter()
                .map(format_expression)
                .collect::<Vec<_>>()
                .join(",");
            internal_name.push('<');
            internal_name.push_str(&arguments);
            internal_name.push('>');
        }
        let term = self.new_name(&internal_name, info.name.clone(), info.properties);
        self.terms.terms[term].set_inline(info.inline);
        if info.dynamic_precedence != 0 {
            self.dynamic_precedences
                .push((term, info.dynamic_precedence));
            self.terms.terms[term].set_preserve(true);
        }
        if (self.terms.terms[term].node_type() || info.exported.is_some()) && rule.params.is_empty()
        {
            if info.name.is_none() {
                self.terms.terms[term].set_preserve(true);
            }
            let public_name = info
                .exported
                .clone()
                .unwrap_or_else(|| rule.id.name.clone());
            self.named_terms.insert(public_name, term);
        }
        if !inline {
            self.built.push(BuiltRule {
                name: rule.id.name.clone(),
                arguments: arguments.to_vec(),
                term,
            });
        }
        self.current_skip.push(skip);
        let choices = self.normalize_expression(&expression)?;
        self.define_rule(term, choices);
        self.current_skip.pop();
        if let Some(group) = info.group {
            self.defined_groups.push((term, group, rule.clone()));
        }
        Ok(term)
    }

    fn conflicts_for(
        &self,
        markers: &[ConflictMarker],
    ) -> Result<(Conflicts, Conflicts), GeneratorError> {
        let mut here = Conflicts::default();
        let mut at_end = Conflicts::default();
        for marker in markers {
            if marker.kind == ConflictMarkerKind::Ambiguity {
                here = here.join(&Conflicts {
                    precedence: 0,
                    ambiguity_groups: vec![marker.id.name.clone()],
                    cut: 0,
                });
                continue;
            }
            let precedence = self.ast.precedences.as_ref().and_then(|precedences| {
                precedences
                    .items
                    .iter()
                    .position(|item| item.id.name == marker.id.name)
                    .map(|index| (precedences, index))
            });
            let Some((precedences, index)) = precedence else {
                return Err(self.error(
                    format!("Reference to unknown precedence: '{}'", marker.id.name),
                    marker.id.start,
                ));
            };
            let item = &precedences.items[index];
            let value =
                i32::try_from(precedences.items.len() - index).expect("precedence count fits i32");
            if item.kind == Some(PrecKind::Cut) {
                here = here.join(&Conflicts {
                    precedence: 0,
                    ambiguity_groups: Vec::new(),
                    cut: value,
                });
            } else {
                here = here.join(&Conflicts {
                    precedence: value << 2,
                    ambiguity_groups: Vec::new(),
                    cut: 0,
                });
                let associativity = match item.kind {
                    Some(PrecKind::Left) => 1,
                    Some(PrecKind::Right) => -1,
                    _ => 0,
                };
                at_end = at_end.join(&Conflicts {
                    precedence: (value << 2) + associativity,
                    ambiguity_groups: Vec::new(),
                    cut: 0,
                });
            }
        }
        Ok((here, at_end))
    }

    fn substitute_arguments(
        &self,
        expression: &Expression,
        arguments: &[Expression],
        parameters: &[Identifier],
    ) -> Result<Expression, GeneratorError> {
        if arguments.is_empty() {
            return Ok(expression.clone());
        }
        let kind = match &expression.kind {
            ExpressionKind::Name(name) => {
                if let Some(index) = parameters
                    .iter()
                    .position(|parameter| parameter.name == name.id.name)
                {
                    let argument = &arguments[index];
                    if name.arguments.is_empty() {
                        return Ok(argument.clone());
                    }
                    if let ExpressionKind::Name(argument_name) = &argument.kind
                        && argument_name.arguments.is_empty()
                    {
                        let mut substituted = name.clone();
                        substituted.id = argument_name.id.clone();
                        ExpressionKind::Name(substituted)
                    } else {
                        return Err(self.error(
                            "Passing arguments to a parameter that already has arguments",
                            expression.start,
                        ));
                    }
                } else {
                    let mut substituted = name.clone();
                    substituted.arguments = name
                        .arguments
                        .iter()
                        .map(|argument| self.substitute_arguments(argument, arguments, parameters))
                        .collect::<Result<_, _>>()?;
                    ExpressionKind::Name(substituted)
                }
            }
            ExpressionKind::Specialize(specialize) => {
                let mut substituted = specialize.clone();
                substituted.props =
                    self.substitute_properties(&specialize.props, arguments, parameters)?;
                substituted.token = Box::new(self.substitute_arguments(
                    &specialize.token,
                    arguments,
                    parameters,
                )?);
                substituted.content = Box::new(self.substitute_arguments(
                    &specialize.content,
                    arguments,
                    parameters,
                )?);
                ExpressionKind::Specialize(substituted)
            }
            ExpressionKind::InlineRule(rule) => {
                let mut substituted = (**rule).clone();
                substituted.props =
                    self.substitute_properties(&rule.props, arguments, parameters)?;
                substituted.expression =
                    self.substitute_arguments(&rule.expression, arguments, parameters)?;
                ExpressionKind::InlineRule(Box::new(substituted))
            }
            ExpressionKind::Choice(expressions) => ExpressionKind::Choice(
                expressions
                    .iter()
                    .map(|item| self.substitute_arguments(item, arguments, parameters))
                    .collect::<Result<_, _>>()?,
            ),
            ExpressionKind::Sequence {
                expressions,
                markers,
                explicitly_empty,
            } => ExpressionKind::Sequence {
                expressions: expressions
                    .iter()
                    .map(|item| self.substitute_arguments(item, arguments, parameters))
                    .collect::<Result<_, _>>()?,
                markers: markers.clone(),
                explicitly_empty: *explicitly_empty,
            },
            ExpressionKind::Repeat { expression, kind } => ExpressionKind::Repeat {
                expression: Box::new(self.substitute_arguments(expression, arguments, parameters)?),
                kind: *kind,
            },
            other => other.clone(),
        };
        Ok(Expression::new(expression.start, kind))
    }

    fn substitute_properties(
        &self,
        properties: &[Prop],
        arguments: &[Expression],
        parameters: &[Identifier],
    ) -> Result<Vec<Prop>, GeneratorError> {
        properties
            .iter()
            .map(|property| {
                let mut property = property.clone();
                for part in &mut property.value {
                    let Some(name) = &part.name else {
                        continue;
                    };
                    let Some(index) = parameters
                        .iter()
                        .position(|parameter| parameter.name == *name)
                    else {
                        continue;
                    };
                    part.value = Some(property_argument(&arguments[index]).ok_or_else(|| {
                        self.error(
                            format!(
                                "Trying to interpolate expression '{}' into a prop",
                                format_expression(&arguments[index])
                            ),
                            part.start,
                        )
                    })?);
                    part.name = None;
                }
                Ok(property)
            })
            .collect()
    }

    fn resolve_specialization(
        &mut self,
        expression: &SpecializeExpression,
    ) -> Result<TermId, GeneratorError> {
        let info = self.node_info(NodeInfoRequest::new(&expression.props, "d", None))?;
        let terminal = self.normalize_expression(&expression.token)?;
        if terminal.len() != 1
            || terminal[0].terms.len() != 1
            || !self.terms.terms[terminal[0].terms[0]].terminal()
        {
            return Err(self.error(
                "The first specialization argument must resolve to a token",
                expression.token.start,
            ));
        }
        let values = literal_choices(&expression.content).ok_or_else(|| {
            self.error(
                "The second specialization argument must be a literal or choice of literals",
                expression.content.start,
            )
        })?;
        let base = terminal[0].terms[0];
        let mut selected = None;
        for value in values {
            let known = self
                .specialized
                .get(&base)
                .and_then(|entries| entries.iter().find(|entry| entry.value == value))
                .cloned();
            if let Some(known) = known {
                if known.kind != expression.kind
                    || known.dialect != info.dialect
                    || known.name != info.name
                    || selected.is_some_and(|term| term != known.term)
                {
                    return Err(self.error(
                        format!("Conflicting specialization for {value:?}"),
                        expression.start,
                    ));
                }
                selected = Some(known.term);
                continue;
            }
            let term = *selected.get_or_insert_with(|| {
                let name = format!("{}/{}", self.terms.terms[base].name, quoted(&value));
                self.make_terminal(&name, info.name.clone(), info.properties.clone())
            });
            if let Some(dialect) = info.dialect {
                self.main_tokens
                    .dialect_terms
                    .entry(dialect)
                    .or_default()
                    .push(term);
            }
            self.specialized
                .entry(base)
                .or_default()
                .push(Specialization {
                    value: value.clone(),
                    name: info.name.clone(),
                    term,
                    kind: expression.kind,
                    dialect: info.dialect,
                });
            self.token_origins
                .insert(term, TokenOrigin::Specialized(base));
            if info.name.is_some() || info.exported.is_some() {
                if info.name.is_none() {
                    self.terms.terms[term].set_preserve(true);
                }
                let name = info
                    .exported
                    .clone()
                    .or_else(|| info.name.clone())
                    .expect("named specialization has a public name");
                self.named_terms.insert(name, term);
            }
        }
        selected.ok_or_else(|| {
            self.error(
                "A specialization must define at least one literal",
                expression.start,
            )
        })
    }

    fn finish_external_specializer(&mut self, index: usize) -> Result<(), GeneratorError> {
        let declaration = self.external_specializers[index].declaration.clone();
        let terms = self.normalize_expression(&declaration.token)?;
        if terms.len() != 1
            || terms[0].terms.len() != 1
            || !self.terms.terms[terms[0].terms[0]].terminal()
        {
            return Err(self.error(
                "The token expression for an external specializer must resolve to a token",
                declaration.token.start,
            ));
        }
        let base = terms[0].terms[0];
        self.external_specializers[index].term = Some(base);
        let tokens = self.external_specializers[index]
            .tokens
            .values()
            .copied()
            .collect::<Vec<_>>();
        for term in tokens {
            self.token_origins.insert(
                term,
                TokenOrigin::ExternalSpecializer {
                    base,
                    specializer: index,
                },
            );
        }
        Ok(())
    }

    fn error(&self, message: impl Into<String>, position: usize) -> GeneratorError {
        debug_assert!(self.ast.start <= position);
        GeneratorError::new(message, Some(position))
    }

    fn warn(&mut self, message: impl Into<String>, position: usize) {
        self.warnings.push(GeneratorWarning::new(message, position));
    }

    fn node_info(&mut self, request: NodeInfoRequest<'_>) -> Result<NodeInfo, GeneratorError> {
        let mut result = NodeInfo::default();
        if let Some(default_name) = request.default_name
            && (request.capabilities.contains('a') || !ignored(default_name))
            && !default_name.contains(' ')
        {
            result.name = Some(default_name.to_owned());
        }
        for property in request.properties {
            self.apply_node_property(&mut result, property, &request)?;
        }
        if self.ast.auto_delimiters
            && (result.name.is_some() || !result.properties.is_empty())
            && let Some(expression) = request.expression
            && let Some((open, close)) = self.find_delimiters(expression)
        {
            let open_name = self.terms.terms[open]
                .node_name
                .clone()
                .expect("delimiter tokens are named");
            let close_name = self.terms.terms[close]
                .node_name
                .clone()
                .expect("delimiter tokens are named");
            add_property(&mut self.terms.terms[open].props, "closedBy", &close_name);
            add_property(&mut self.terms.terms[close].props, "openedBy", &open_name);
        }
        if let Some(default_properties) = request.default_properties {
            for (name, value) in default_properties {
                result
                    .properties
                    .entry(name.clone())
                    .or_insert_with(|| value.clone());
            }
        }
        if !result.properties.is_empty() && result.name.is_none() {
            let position = request.properties.first().map_or_else(
                || request.expression.map_or(0, |expression| expression.start),
                |prop| prop.start,
            );
            return Err(self.error("Node has properties but no name", position));
        }
        if result.inline
            && (!result.properties.is_empty()
                || result.dialect.is_some()
                || result.dynamic_precedence != 0)
        {
            let position = request
                .properties
                .first()
                .map_or(0, |property| property.start);
            return Err(self.error(
                "Inline nodes can't have props, dynamic precedence, or a dialect",
                position,
            ));
        }
        if result.inline {
            result.name = None;
        }
        Ok(result)
    }

    fn apply_node_property(
        &mut self,
        result: &mut NodeInfo,
        property: &Prop,
        request: &NodeInfoRequest<'_>,
    ) -> Result<(), GeneratorError> {
        if !property.at {
            return self.apply_regular_node_property(result, property, request);
        }
        match property.name.as_str() {
            "name" => self.apply_node_name(result, property, request),
            "dialect" => {
                result.dialect = Some(self.resolve_node_dialect(property, request.capabilities)?);
                Ok(())
            }
            "dynamicPrecedence" => {
                result.dynamic_precedence =
                    self.resolve_dynamic_precedence(property, request.capabilities)?;
                Ok(())
            }
            "inline" => self.apply_inline_property(result, property, request.capabilities),
            "isGroup" => {
                result.group = self.resolve_optional_node_name(property, request)?;
                Ok(())
            }
            "export" => {
                result.exported = self.resolve_optional_node_name(property, request)?;
                Ok(())
            }
            other => Err(self.error(
                format!("Unknown built-in prop name '@{other}'"),
                property.start,
            )),
        }
    }

    fn apply_regular_node_property(
        &self,
        result: &mut NodeInfo,
        property: &Prop,
        request: &NodeInfoRequest<'_>,
    ) -> Result<(), GeneratorError> {
        let builtin = matches!(
            property.name.as_str(),
            "closedBy" | "openedBy" | "group" | "isolate"
        );
        let external = self
            .ast
            .external_props
            .iter()
            .any(|known| known.id.name == property.name);
        if !builtin && !external {
            let hint = matches!(
                property.name.as_str(),
                "name" | "dialect" | "dynamicPrecedence" | "export" | "isGroup"
            )
            .then_some(format!(" (did you mean '@{}'?)", property.name))
            .unwrap_or_default();
            return Err(self.error(
                format!("Unknown prop name '{}'{}", property.name, hint),
                property.start,
            ));
        }
        let value = self.finish_property(property, request.arguments, request.parameters)?;
        result.properties.insert(property.name.clone(), value);
        Ok(())
    }

    fn apply_node_name(
        &self,
        result: &mut NodeInfo,
        property: &Prop,
        request: &NodeInfoRequest<'_>,
    ) -> Result<(), GeneratorError> {
        let name = self.finish_property(property, request.arguments, request.parameters)?;
        if name.contains(' ') {
            return Err(self.error(
                format!("Node names cannot have spaces ({name:?})"),
                property.start,
            ));
        }
        result.name = Some(name);
        Ok(())
    }

    fn resolve_node_dialect(
        &self,
        property: &Prop,
        capabilities: &str,
    ) -> Result<usize, GeneratorError> {
        if !capabilities.contains('d') {
            return Err(self.error("Can't specify a dialect on non-token rules", property.start));
        }
        if property.value.len() != 1 || property.value[0].value.is_none() {
            return Err(self.error(
                "The '@dialect' rule prop must hold a plain string value",
                property.start,
            ));
        }
        let dialect = property.value[0]
            .value
            .as_deref()
            .expect("checked plain property value");
        self.ast
            .dialects
            .iter()
            .position(|known| known.name == dialect)
            .ok_or_else(|| {
                self.error(
                    format!("Unknown dialect {dialect:?}"),
                    property.value[0].start,
                )
            })
    }

    fn resolve_dynamic_precedence(
        &self,
        property: &Prop,
        capabilities: &str,
    ) -> Result<i16, GeneratorError> {
        if !capabilities.contains('p') {
            return Err(self.error(
                "Dynamic precedence can only be specified on nonterminals",
                property.start,
            ));
        }
        let value = property
            .value
            .first()
            .and_then(|part| part.value.as_deref())
            .and_then(|value| value.parse::<i16>().ok());
        if property.value.len() != 1 {
            return Err(self.error(
                "The '@dynamicPrecedence' rule prop must hold one integer",
                property.start,
            ));
        }
        value
            .filter(|value| (-10..=10).contains(value))
            .ok_or_else(|| {
                self.error(
                    "The '@dynamicPrecedence' rule prop must hold an integer between -10 and 10",
                    property.start,
                )
            })
    }

    fn apply_inline_property(
        &self,
        result: &mut NodeInfo,
        property: &Prop,
        capabilities: &str,
    ) -> Result<(), GeneratorError> {
        if let Some(value) = property.value.first() {
            return Err(self.error("'@inline' doesn't take a value", value.start));
        }
        if !capabilities.contains('i') {
            return Err(self.error(
                "Inline can only be specified on nonterminals",
                property.start,
            ));
        }
        result.inline = true;
        Ok(())
    }

    fn resolve_optional_node_name(
        &self,
        property: &Prop,
        request: &NodeInfoRequest<'_>,
    ) -> Result<Option<String>, GeneratorError> {
        if property.name == "isGroup" && !request.capabilities.contains('g') {
            return Err(self.error(
                "'@isGroup' can only be specified on nonterminals",
                property.start,
            ));
        }
        if property.value.is_empty() {
            return Ok(request.default_name.map(str::to_owned));
        }
        self.finish_property(property, request.arguments, request.parameters)
            .map(Some)
    }

    fn finish_property(
        &self,
        property: &Prop,
        arguments: &[Expression],
        parameters: &[Identifier],
    ) -> Result<String, GeneratorError> {
        let mut result = String::new();
        for part in &property.value {
            if let Some(value) = &part.value {
                result.push_str(value);
                continue;
            }
            let name = part
                .name
                .as_deref()
                .expect("property parts hold a value or parameter name");
            let Some(position) = parameters
                .iter()
                .position(|parameter| parameter.name == name)
            else {
                return Err(self.error(
                    format!(
                        "Property refers to '{name}', but no parameter by that name is in scope"
                    ),
                    part.start,
                ));
            };
            let Some(value) = property_argument(&arguments[position]) else {
                return Err(self.error(
                    format!(
                        "Expression '{}' can not be used as part of a property value",
                        format_expression(&arguments[position])
                    ),
                    part.start,
                ));
            };
            result.push_str(&value);
        }
        Ok(result)
    }

    fn find_delimiters(&mut self, expression: &Expression) -> Option<(TermId, TermId)> {
        let ExpressionKind::Sequence { expressions, .. } = &expression.kind else {
            return None;
        };
        if expressions.len() < 2 {
            return None;
        }
        let last = self.find_delimiter_token(expressions.last()?)?;
        self.terms.terms[last.0].node_name.as_ref()?;
        let bracket = ["()", "[]", "{}", "<>"].into_iter().find(|bracket| {
            last.1
                .contains(bracket.chars().nth(1).expect("delimiter pair"))
                && !last
                    .1
                    .contains(bracket.chars().next().expect("delimiter pair"))
        })?;
        let first = self.find_delimiter_token(&expressions[0])?;
        self.terms.terms[first.0].node_name.as_ref()?;
        let mut bracket = bracket.chars();
        let open = bracket.next().expect("delimiter pair");
        let close = bracket.next().expect("delimiter pair");
        if first.1.contains(open) && !first.1.contains(close) {
            Some((first.0, last.0))
        } else {
            None
        }
    }

    fn find_delimiter_token(&mut self, expression: &Expression) -> Option<(TermId, String)> {
        match &expression.kind {
            ExpressionKind::Literal(literal) => {
                let term = self.get_literal(literal).ok()?;
                Some((term, literal.value.clone()))
            }
            ExpressionKind::Name(name) if name.arguments.is_empty() => {
                if let Some(rule) = self
                    .ast
                    .rules
                    .iter()
                    .find(|rule| rule.id.name == name.id.name)
                    .cloned()
                {
                    return self.find_delimiter_token(&rule.expression);
                }
                let token = self
                    .main_tokens
                    .rules
                    .iter()
                    .find(|rule| rule.id.name == name.id.name)
                    .cloned()?;
                let ExpressionKind::Literal(literal) = &token.expression.kind else {
                    return None;
                };
                let term = self.get_token(TokenSetId::Main, name).ok()??;
                Some((term, literal.value.clone()))
            }
            _ => None,
        }
    }

    fn define_group(
        &mut self,
        term: TermId,
        group: &str,
        declaration: &RuleDeclaration,
    ) -> Result<(), GeneratorError> {
        let mut recursion = Vec::new();
        let named = self.named_descendants(term, declaration, &mut recursion)?;
        for term in named {
            add_property(&mut self.terms.terms[term].props, "group", group);
        }
        Ok(())
    }

    fn named_descendants(
        &self,
        term: TermId,
        declaration: &RuleDeclaration,
        recursion: &mut Vec<TermId>,
    ) -> Result<Vec<TermId>, GeneratorError> {
        if self.terms.terms[term].node_name.is_some() {
            return Ok(vec![term]);
        }
        if recursion.contains(&term) {
            return Err(self.error(
                format!(
                    "Rule '{}' cannot define a group because it contains a non-named recursive rule ('{}')",
                    declaration.id.name, self.terms.terms[term].name
                ),
                declaration.start,
            ));
        }
        recursion.push(term);
        let mut result = Vec::new();
        for rule in self.rules.iter().filter(|rule| rule.name == term) {
            let mut named_parts = Vec::new();
            for part in &rule.parts {
                let named = self.named_descendants(*part, declaration, recursion)?;
                if !named.is_empty() {
                    named_parts.push(named);
                }
            }
            if named_parts.len() > 1 {
                return Err(self.error(
                    format!(
                        "Rule '{}' cannot define a group because some choices produce multiple named nodes",
                        declaration.id.name
                    ),
                    declaration.start,
                ));
            }
            if let Some(named) = named_parts.pop() {
                result.extend(named);
            }
        }
        recursion.pop();
        Ok(result)
    }

    fn check_groups(&mut self) {
        let mut groups: BTreeMap<String, Vec<TermId>> = BTreeMap::new();
        let node_names = self
            .terms
            .terms
            .iter()
            .filter_map(|term| term.node_name.clone())
            .collect::<BTreeSet<_>>();
        for (term_id, term) in self.terms.terms.iter().enumerate() {
            if term.node_name.is_none() {
                continue;
            }
            if let Some(names) = term.props.get("group") {
                for group in names.split_ascii_whitespace() {
                    groups.entry(group.to_owned()).or_default().push(term_id);
                }
            }
        }
        let names = groups.keys().cloned().collect::<Vec<_>>();
        for (index, name) in names.iter().enumerate() {
            if node_names.contains(name) {
                self.warn(
                    format!("Group name '{name}' conflicts with a node of the same name"),
                    0,
                );
            }
            let terms = &groups[name];
            for other_name in names.iter().skip(index + 1) {
                let other = &groups[other_name];
                let overlap = terms.iter().any(|term| other.contains(term));
                let nested = if terms.len() > other.len() {
                    other.iter().all(|term| terms.contains(term))
                } else {
                    terms.iter().all(|term| other.contains(term))
                };
                if overlap && !nested {
                    self.warn(
                        format!(
                            "Groups '{name}' and '{other_name}' overlap without one being a superset of the other"
                        ),
                        0,
                    );
                }
            }
        }
    }
}

impl Builder {
    fn token_set(&self, set: TokenSetId) -> &TokenSet {
        match set {
            TokenSetId::Main => &self.main_tokens,
            TokenSetId::Local(index) => &self.local_tokens[index],
        }
    }

    fn token_set_mut(&mut self, set: TokenSetId) -> &mut TokenSet {
        match set {
            TokenSetId::Main => &mut self.main_tokens,
            TokenSetId::Local(index) => &mut self.local_tokens[index],
        }
    }

    fn gather_external_tokens(
        &mut self,
        declarations: &[crate::node::NamedNode],
    ) -> Result<BTreeMap<String, TermId>, GeneratorError> {
        let mut result = BTreeMap::new();
        for declaration in declarations {
            self.unique(&declaration.id)?;
            let request = NodeInfoRequest::new(&declaration.props, "d", Some(&declaration.id.name));
            let info = self.node_info(request)?;
            let term = self.make_terminal(&declaration.id.name, info.name.clone(), info.properties);
            if let Some(dialect) = info.dialect {
                self.main_tokens
                    .dialect_terms
                    .entry(dialect)
                    .or_default()
                    .push(term);
            }
            self.named_terms.insert(declaration.id.name.clone(), term);
            result.insert(declaration.id.name.clone(), term);
        }
        Ok(result)
    }

    fn get_token(
        &mut self,
        set: TokenSetId,
        expression: &NameExpression,
    ) -> Result<Option<TermId>, GeneratorError> {
        if let Some(term) = self
            .token_set(set)
            .built
            .iter()
            .find(|built| built.matches(expression))
            .map(|built| built.term)
        {
            return Ok(Some(term));
        }
        let rule = self
            .token_set(set)
            .rules
            .iter()
            .find(|rule| rule.id.name == expression.id.name)
            .cloned();
        let Some(rule) = rule else {
            return Ok(None);
        };
        let parameters = if rule.params.len() == expression.arguments.len() {
            rule.params.as_slice()
        } else {
            &[]
        };
        let request = NodeInfoRequest::new(&rule.props, "d", Some(&rule.id.name))
            .with_template(&expression.arguments, parameters)
            .with_expression(&rule.expression);
        let info = self.node_info(request)?;
        let term_name = format_name_expression(expression);
        let term = self.make_terminal(&term_name, info.name.clone(), info.properties);
        if let Some(dialect) = info.dialect {
            self.token_set_mut(set)
                .dialect_terms
                .entry(dialect)
                .or_default()
                .push(term);
        }
        if (self.terms.terms[term].node_type() || info.exported.is_some()) && rule.params.is_empty()
        {
            if !self.terms.terms[term].node_type() {
                self.terms.terms[term].set_preserve(true);
            }
            let public_name = info
                .exported
                .clone()
                .unwrap_or_else(|| rule.id.name.clone());
            self.named_terms.insert(public_name, term);
        }
        let target = self.token_set_mut(set).nfa.accepting_state(term);
        let start = self.token_set(set).nfa.start;
        self.build_token_rule(set, &rule, expression, start, target, &[])?;
        self.token_set_mut(set).built.push(BuiltRule {
            name: rule.id.name,
            arguments: expression.arguments.clone(),
            term,
        });
        Ok(Some(term))
    }

    fn get_local_token(
        &mut self,
        index: usize,
        expression: &NameExpression,
    ) -> Result<Option<TermId>, GeneratorError> {
        let fallback = self.ast.local_tokens[index].fallback.clone();
        if let Some(fallback) = fallback
            && fallback.id.name == expression.id.name
        {
            if !expression.arguments.is_empty() {
                return Err(self.error(
                    format!("Incorrect number of arguments for {}", expression.id.name),
                    expression.start,
                ));
            }
            let term = if let Some(term) = self.local_tokens[index].fallback {
                term
            } else {
                let request = NodeInfoRequest::new(&fallback.props, "", Some(&expression.id.name));
                let info = self.node_info(request)?;
                let term = self.make_terminal(&expression.id.name, info.name, info.properties);
                if self.terms.terms[term].node_type() || info.exported.is_some() {
                    if !self.terms.terms[term].node_type() {
                        self.terms.terms[term].set_preserve(true);
                    }
                    let public_name = info.exported.unwrap_or_else(|| expression.id.name.clone());
                    self.named_terms.insert(public_name, term);
                }
                self.local_tokens[index].fallback = Some(term);
                self.mark_used(&expression.id.name);
                term
            };
            self.token_origins
                .entry(term)
                .or_insert(TokenOrigin::Local(index));
            return Ok(Some(term));
        }
        let term = self.get_token(TokenSetId::Local(index), expression)?;
        if let Some(term) = term {
            self.token_origins
                .entry(term)
                .or_insert(TokenOrigin::Local(index));
        }
        Ok(term)
    }

    fn get_literal(&mut self, literal: &LiteralExpression) -> Result<TermId, GeneratorError> {
        let key = quoted(&literal.value);
        if let Some(term) = self
            .main_tokens
            .built
            .iter()
            .find(|built| built.name == key)
            .map(|built| built.term)
        {
            return Ok(term);
        }
        let declaration = self
            .ast
            .tokens
            .as_ref()
            .and_then(|tokens| {
                tokens
                    .literals
                    .iter()
                    .find(|known| known.literal == literal.value)
            })
            .cloned();
        let info = if let Some(declaration) = declaration {
            let request = NodeInfoRequest::new(&declaration.props, "da", Some(&literal.value));
            self.node_info(request)?
        } else {
            NodeInfo::default()
        };
        let term = self.make_terminal(&key, info.name, info.properties);
        if let Some(dialect) = info.dialect {
            self.main_tokens
                .dialect_terms
                .entry(dialect)
                .or_default()
                .push(term);
        }
        if let Some(exported) = info.exported {
            self.named_terms.insert(exported, term);
        }
        let target = self.main_tokens.nfa.accepting_state(term);
        let start = self.main_tokens.nfa.start;
        let expression = Expression::new(literal.start, ExpressionKind::Literal(literal.clone()));
        self.build_token_expression(TokenSetId::Main, &expression, start, target, &[])?;
        self.main_tokens.built.push(BuiltRule {
            name: key,
            arguments: Vec::new(),
            term,
        });
        Ok(term)
    }

    fn build_token_rule(
        &mut self,
        set: TokenSetId,
        rule: &RuleDeclaration,
        expression: &NameExpression,
        from: usize,
        target: usize,
        arguments: &[TokenArgument],
    ) -> Result<(), GeneratorError> {
        if rule.params.len() != expression.arguments.len() {
            return Err(self.error(
                format!(
                    "Incorrect number of arguments for token '{}'",
                    expression.id.name
                ),
                expression.start,
            ));
        }
        let recursive = self
            .token_set(set)
            .building
            .iter()
            .find(|building| {
                building.name == expression.id.name
                    && expressions_structurally_eq(&building.arguments, &expression.arguments)
            })
            .cloned();
        if let Some(recursive) = recursive {
            if recursive.target == target {
                self.token_set_mut(set).nfa.epsilon(from, recursive.start);
                return Ok(());
            }
            let building = &self.token_set(set).building;
            let start = building
                .iter()
                .rposition(|known| known.name == expression.id.name)
                .expect("recursive token rule is in the build stack");
            let chain = building[start..]
                .iter()
                .map(|known| known.name.as_str())
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(self.error(
                format!("Invalid (non-tail) recursion in token rules: {chain}"),
                expression.start,
            ));
        }
        self.mark_used(&rule.id.name);
        let start = self.token_set_mut(set).nfa.state();
        self.token_set_mut(set).nfa.epsilon(from, start);
        self.token_set_mut(set).building.push(BuildingTokenRule {
            name: expression.id.name.clone(),
            start,
            target,
            arguments: expression.arguments.clone(),
        });
        let substituted =
            self.substitute_arguments(&rule.expression, &expression.arguments, &rule.params)?;
        let scope = expression
            .arguments
            .iter()
            .zip(&rule.params)
            .map(|(expression, parameter)| TokenArgument {
                name: parameter.name.clone(),
                expression: expression.clone(),
                scope: arguments.to_vec(),
            })
            .collect::<Vec<_>>();
        self.build_token_expression(set, &substituted, start, target, &scope)?;
        self.token_set_mut(set).building.pop();
        Ok(())
    }

    fn build_token_expression(
        &mut self,
        set: TokenSetId,
        expression: &Expression,
        from: usize,
        target: usize,
        arguments: &[TokenArgument],
    ) -> Result<(), GeneratorError> {
        match &expression.kind {
            ExpressionKind::Name(name) => self.build_token_name_expression(
                set,
                name,
                expression.start,
                from,
                target,
                arguments,
            ),
            ExpressionKind::CharClass(class) => {
                if *class == crate::node::CharClass::Eof {
                    self.token_set_mut(set).nfa.eof(from, target);
                } else {
                    for (low, high) in class.ranges() {
                        self.token_set_mut(set).nfa.edge(from, *low, *high, target);
                    }
                }
                Ok(())
            }
            ExpressionKind::Choice(choices) => {
                for choice in choices {
                    self.build_token_expression(set, choice, from, target, arguments)?;
                }
                Ok(())
            }
            ExpressionKind::Sequence {
                expressions,
                markers,
                ..
            } => self.build_token_sequence(set, expressions, markers, from, target, arguments),
            ExpressionKind::Repeat { expression, kind } => {
                self.build_token_repeat(set, expression, *kind, from, target, arguments)
            }
            ExpressionKind::Set(character_set) => {
                let ranges = if character_set.inverted {
                    invert_ranges(&character_set.ranges)
                } else {
                    character_set.ranges.clone()
                };
                for (low, high) in ranges {
                    self.add_range_edges(set, from, target, low, high);
                }
                Ok(())
            }
            ExpressionKind::Literal(literal) => {
                self.build_token_literal(set, literal, from, target);
                Ok(())
            }
            ExpressionKind::Any => {
                self.build_any_token(set, from, target);
                Ok(())
            }
            _ => Err(self.error("Unrecognized expression type in token", expression.start)),
        }
    }

    fn build_token_name_expression(
        &mut self,
        set: TokenSetId,
        name: &NameExpression,
        position: usize,
        from: usize,
        target: usize,
        arguments: &[TokenArgument],
    ) -> Result<(), GeneratorError> {
        if let Some(argument) = arguments
            .iter()
            .find(|argument| argument.name == name.id.name)
        {
            let expression = argument.expression.clone();
            let scope = argument.scope.clone();
            return self.build_token_expression(set, &expression, from, target, &scope);
        }
        let local_rule = self
            .local_tokens
            .iter()
            .enumerate()
            .find_map(|(index, set)| {
                let rule = set.rules.iter().find(|rule| rule.id.name == name.id.name)?;
                Some((TokenSetId::Local(index), rule.clone()))
            });
        let main_rule = self
            .main_tokens
            .rules
            .iter()
            .find(|rule| rule.id.name == name.id.name)
            .cloned()
            .map(|rule| (TokenSetId::Main, rule));
        let Some((_, rule)) = local_rule.or(main_rule) else {
            return Err(self.error(
                format!(
                    "Reference to token rule '{}', which isn't found",
                    name.id.name
                ),
                position,
            ));
        };
        self.build_token_rule(set, &rule, name, from, target, arguments)
    }

    fn build_token_sequence(
        &mut self,
        set: TokenSetId,
        expressions: &[Expression],
        markers: &[Vec<ConflictMarker>],
        mut from: usize,
        target: usize,
        arguments: &[TokenArgument],
    ) -> Result<(), GeneratorError> {
        if let Some(marker) = markers.iter().flatten().next() {
            return Err(self.error("Conflict marker in token expression", marker.start));
        }
        if expressions.is_empty() {
            self.token_set_mut(set).nfa.epsilon(from, target);
            return Ok(());
        }
        for (index, expression) in expressions.iter().enumerate() {
            let next = if index + 1 == expressions.len() {
                target
            } else {
                self.token_set_mut(set).nfa.state()
            };
            self.build_token_expression(set, expression, from, next, arguments)?;
            from = next;
        }
        Ok(())
    }

    fn build_token_repeat(
        &mut self,
        set: TokenSetId,
        expression: &Expression,
        kind: RepeatKind,
        from: usize,
        target: usize,
        arguments: &[TokenArgument],
    ) -> Result<(), GeneratorError> {
        match kind {
            RepeatKind::ZeroOrMore => {
                let loop_state = self.token_set_mut(set).nfa.state();
                self.token_set_mut(set).nfa.epsilon(from, loop_state);
                self.build_token_expression(set, expression, loop_state, loop_state, arguments)?;
                self.token_set_mut(set).nfa.epsilon(loop_state, target);
            }
            RepeatKind::OneOrMore => {
                let loop_state = self.token_set_mut(set).nfa.state();
                self.build_token_expression(set, expression, from, loop_state, arguments)?;
                self.build_token_expression(set, expression, loop_state, loop_state, arguments)?;
                self.token_set_mut(set).nfa.epsilon(loop_state, target);
            }
            RepeatKind::Optional => {
                self.token_set_mut(set).nfa.epsilon(from, target);
                self.build_token_expression(set, expression, from, target, arguments)?;
            }
        }
        Ok(())
    }

    fn build_token_literal(
        &mut self,
        set: TokenSetId,
        literal: &LiteralExpression,
        mut from: usize,
        target: usize,
    ) {
        let characters = literal.value.chars().collect::<Vec<_>>();
        for (index, character) in characters.iter().copied().enumerate() {
            let next = if index + 1 == characters.len() {
                target
            } else {
                self.token_set_mut(set).nfa.state()
            };
            let value = u32::from(character);
            self.token_set_mut(set)
                .nfa
                .edge(from, value, value + 1, next);
            from = next;
        }
    }

    fn build_any_token(&mut self, set: TokenSetId, from: usize, target: usize) {
        self.token_set_mut(set)
            .nfa
            .edge(from, 0, MAX_CODE_POINT + 1, target);
    }

    fn add_range_edges(
        &mut self,
        set: TokenSetId,
        from: usize,
        target: usize,
        low: u32,
        high: u32,
    ) {
        if low < high {
            self.token_set_mut(set)
                .nfa
                .edge(from, low, high.min(MAX_CODE_POINT + 1), target);
        }
    }

    fn take_token_precedences(&mut self, set: TokenSetId) {
        let declarations = match set {
            TokenSetId::Main => self
                .ast
                .tokens
                .as_ref()
                .map_or_else(Vec::new, |tokens| tokens.precedences.clone()),
            TokenSetId::Local(index) => self.ast.local_tokens[index].precedences.clone(),
        };
        let mut relations = Vec::new();
        for declaration in declarations {
            let mut previous = Vec::new();
            for item in declaration.items {
                let mut level = Vec::new();
                match &item {
                    TokenReference::Name(name) => {
                        for built in &self.token_set(set).built {
                            let matches = if name.arguments.is_empty() {
                                built.name == name.id.name
                            } else {
                                built.matches(name)
                            };
                            if matches {
                                level.push(built.term);
                            }
                        }
                    }
                    TokenReference::Literal(literal) => {
                        let key = quoted(&literal.value);
                        if let Some(term) = self
                            .token_set(set)
                            .built
                            .iter()
                            .find(|built| built.name == key)
                            .map(|built| built.term)
                        {
                            level.push(term);
                        }
                    }
                }
                if level.is_empty() {
                    self.warn("Precedence specified for unknown token", declaration.start);
                }
                for term in &level {
                    add_precedence_relation(&mut relations, *term, &previous);
                }
                previous.extend(level);
            }
        }
        self.token_set_mut(set).precedence = relations;
    }

    fn take_token_conflicts(&mut self) {
        let conflicts = self
            .ast
            .tokens
            .as_ref()
            .map_or_else(Vec::new, |tokens| tokens.conflicts.clone());
        for conflict in conflicts {
            let left = self.resolve_token_reference(&conflict.left);
            let right = self.resolve_token_reference(&conflict.right);
            if let (Some(mut left), Some(mut right)) = (left, right) {
                if left < right {
                    std::mem::swap(&mut left, &mut right);
                }
                self.explicit_token_conflicts.insert((left, right));
            }
        }
    }

    fn resolve_token_reference(&mut self, reference: &TokenReference) -> Option<TermId> {
        let found = match reference {
            TokenReference::Name(name) => self
                .main_tokens
                .built
                .iter()
                .find(|built| built.matches(name))
                .map(|built| built.term),
            TokenReference::Literal(literal) => {
                let key = quoted(&literal.value);
                self.main_tokens
                    .built
                    .iter()
                    .find(|built| built.name == key)
                    .map(|built| built.term)
            }
        };
        if found.is_none() {
            self.warn("Conflict specified for unknown token", 0);
        }
        found
    }

    fn preceded_by(&self, set: TokenSetId, term: TermId, other: TermId) -> bool {
        self.token_set(set)
            .precedence
            .iter()
            .find(|relation| relation.term == term)
            .is_some_and(|relation| relation.after.contains(&other))
    }

    fn build_precedence_table(
        &self,
        set: TokenSetId,
        soft_conflicts: &[TokenConflict],
    ) -> Result<Vec<u16>, GeneratorError> {
        let mut relations = self.token_set(set).precedence.clone();
        for conflict in soft_conflicts.iter().filter(|conflict| conflict.soft != 0) {
            if !relations
                .iter()
                .any(|relation| relation.term == conflict.left)
                || !relations
                    .iter()
                    .any(|relation| relation.term == conflict.right)
            {
                continue;
            }
            let (before, after) = if conflict.soft < 0 {
                (conflict.right, conflict.left)
            } else {
                (conflict.left, conflict.right)
            };
            add_precedence_relation(&mut relations, after, &[before]);
            add_precedence_relation(&mut relations, before, &[]);
        }
        let mut result = Vec::new();
        while !relations.is_empty() {
            let found = relations.iter().position(|relation| {
                relation
                    .after
                    .iter()
                    .all(|term| result.contains(&self.terms.output_id(*term)))
            });
            let Some(found) = found else {
                let names = relations
                    .iter()
                    .map(|relation| self.terms.terms[relation.term].name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(GeneratorError::new(
                    format!("Cyclic token precedence relation between {names}"),
                    None,
                ));
            };
            let relation = relations.swap_remove(found);
            result.push(self.terms.output_id(relation.term));
        }
        Ok(result)
    }

    fn build_local_group(
        &mut self,
        index: usize,
        states: &mut [State],
        skip_info: &[SkipInfo],
        group_id: u8,
    ) -> Result<TokenizerMetadata, GeneratorError> {
        let dfa = self.local_tokens[index].nfa.compile();
        if let Some(term) = dfa.states[dfa.start].accepting.first() {
            return Err(GeneratorError::new(
                format!(
                    "Grammar contains zero-length tokens (in '{}')",
                    self.terms.terms[*term].name
                ),
                None,
            ));
        }
        let conflicts = dfa.find_conflicts(|_, _| true);
        for conflict in conflicts {
            if !self.preceded_by(TokenSetId::Local(index), conflict.left, conflict.right)
                && !self.preceded_by(TokenSetId::Local(index), conflict.right, conflict.left)
            {
                return Err(GeneratorError::new(
                    format!(
                        "Overlapping tokens {} and {} in local token group",
                        self.terms.terms[conflict.left].name, self.terms.terms[conflict.right].name
                    ),
                    None,
                ));
            }
        }
        for state in states.iter_mut() {
            if state.default_reduce.is_some() {
                continue;
            }
            let skip_index = self
                .skip_rules
                .iter()
                .position(|skip| *skip == state.skip)
                .expect("state skip set is registered");
            let mut uses_this = None;
            let mut uses_other = skip_info[skip_index].start_tokens.first().copied();
            for action in &state.actions {
                let mut term = action.term();
                while let Some(origin) = self.token_origins.get(&term) {
                    match *origin {
                        TokenOrigin::Specialized(base)
                        | TokenOrigin::ExternalSpecializer { base, .. } => term = base,
                        _ => break,
                    }
                }
                if self.token_origins.get(&term) == Some(&TokenOrigin::Local(index)) {
                    uses_this = Some(term);
                } else {
                    uses_other = Some(term);
                }
            }
            if let Some(uses_this) = uses_this {
                if let Some(uses_other) = uses_other {
                    return Err(GeneratorError::new(
                        format!(
                            "Tokens from a local token group used together with other tokens ({} with {})",
                            self.terms.terms[uses_this].name, self.terms.terms[uses_other].name
                        ),
                        None,
                    ));
                }
                state.token_group = Some(group_id);
            }
        }
        let precedence = self.build_precedence_table(TokenSetId::Local(index), &[])?;
        let table = dfa.to_table(&self.terms, &BTreeMap::new(), &precedence)?;
        let precedence = sequence_with_end(&precedence);
        Ok(TokenizerMetadata::Local {
            table,
            precedence,
            else_token: self.local_tokens[index]
                .fallback
                .map(|term| self.terms.output_id(term)),
        })
    }

    fn build_main_token_groups(
        &mut self,
        states: &mut [State],
        skip_info: &[SkipInfo],
        start_id: u8,
    ) -> Result<MainTokenTables, GeneratorError> {
        let dfa = self.main_tokens.nfa.compile();
        self.reject_zero_length_token(&dfa)?;
        let conflicts = self.main_token_conflicts(&dfa, states, skip_info);
        let groups = self.assign_main_token_groups(states, skip_info, &conflicts.hard, start_id)?;
        if usize::from(start_id) + groups.len() > 16 {
            return Err(GeneratorError::new(
                format!(
                    "Too many different token groups ({}) to represent them as a 16-bit bitfield",
                    groups.len()
                ),
                None,
            ));
        }
        let precedence = self.build_precedence_table(TokenSetId::Main, &conflicts.soft)?;
        let masks = self.token_group_masks(&groups);
        let table = dfa.to_table(&self.terms, &masks, &precedence)?;
        Ok((groups, precedence, table))
    }

    fn reject_zero_length_token(&self, dfa: &TokenDfa) -> Result<(), GeneratorError> {
        let Some(term) = dfa.states[dfa.start].accepting.first() else {
            return Ok(());
        };
        Err(GeneratorError::new(
            format!(
                "Grammar contains zero-length tokens (in '{}')",
                self.terms.terms[*term].name
            ),
            None,
        ))
    }

    fn main_token_conflicts(
        &self,
        dfa: &TokenDfa,
        states: &[State],
        skip_info: &[SkipInfo],
    ) -> TokenConflictSets {
        let together = |left: TermId, right: TermId| {
            states.iter().any(|state| {
                self.state_has_term(state, left, skip_info)
                    && self.state_has_term(state, right, skip_info)
            })
        };
        let mut all_conflicts = dfa
            .find_conflicts(together)
            .into_iter()
            .filter(|conflict| {
                !self.preceded_by(TokenSetId::Main, conflict.left, conflict.right)
                    && !self.preceded_by(TokenSetId::Main, conflict.right, conflict.left)
            })
            .collect::<Vec<_>>();
        for (left, right) in &self.explicit_token_conflicts {
            if let Some(conflict) = all_conflicts
                .iter_mut()
                .find(|conflict| conflict.left == *left && conflict.right == *right)
            {
                conflict.soft = 0;
            } else {
                all_conflicts.push(TokenConflict {
                    left: *left,
                    right: *right,
                    soft: 0,
                });
            }
        }
        let soft = all_conflicts
            .iter()
            .filter(|conflict| conflict.soft != 0)
            .copied()
            .collect::<Vec<_>>();
        let hard = all_conflicts
            .into_iter()
            .filter(|conflict| conflict.soft == 0)
            .collect::<Vec<_>>();
        TokenConflictSets { hard, soft }
    }

    fn assign_main_token_groups(
        &self,
        states: &mut [State],
        skip_info: &[SkipInfo],
        conflicts: &[TokenConflict],
        start_id: u8,
    ) -> Result<Vec<TokenGroup>, GeneratorError> {
        let mut groups: Vec<TokenGroup> = Vec::new();
        let mut errors = Vec::new();
        for state in states.iter_mut() {
            if state.default_reduce.is_some() || state.token_group.is_some() {
                continue;
            }
            let skip_index = self
                .skip_rules
                .iter()
                .position(|skip| *skip == state.skip)
                .expect("state skip set is registered");
            let skip = &skip_info[skip_index].start_tokens;
            for term in skip {
                if state.actions.iter().any(|action| action.term() == *term) {
                    return Err(GeneratorError::new(
                        format!(
                            "Use of token {} conflicts with skip rule",
                            self.terms.terms[*term].name
                        ),
                        None,
                    ));
                }
            }
            let state_terms = self.main_state_terms(state, skip);
            if state_terms.is_empty() {
                continue;
            }
            let (terms, incompatible) =
                self.token_group_constraints(&state_terms, conflicts, &mut errors);
            let compatible = groups
                .iter()
                .position(|group| !incompatible.iter().any(|term| group.terms.contains(term)));
            let group = if let Some(index) = compatible {
                for term in terms {
                    push_unique(&mut groups[index].terms, term);
                }
                index
            } else {
                let id = u8::try_from(groups.len())
                    .ok()
                    .and_then(|offset| start_id.checked_add(offset))
                    .ok_or_else(|| GeneratorError::new("Too many token groups", None))?;
                groups.push(TokenGroup {
                    terms,
                    group_id: id,
                });
                groups.len() - 1
            };
            state.token_group = Some(groups[group].group_id);
        }
        if !errors.is_empty() {
            errors.sort();
            errors.dedup();
            return Err(GeneratorError::new(errors.join("\n\n"), None));
        }
        Ok(groups)
    }

    fn main_state_terms(&self, state: &State, skip: &[TermId]) -> Vec<TermId> {
        let mut state_terms = Vec::new();
        for mut term in state
            .actions
            .iter()
            .map(Action::term)
            .chain(skip.iter().copied())
        {
            loop {
                match self.token_origins.get(&term).copied() {
                    Some(
                        TokenOrigin::Specialized(base)
                        | TokenOrigin::ExternalSpecializer { base, .. },
                    ) => term = base,
                    Some(TokenOrigin::External(_)) => break,
                    _ => {
                        push_unique(&mut state_terms, term);
                        break;
                    }
                }
            }
        }
        state_terms
    }

    fn token_group_constraints(
        &self,
        state_terms: &[TermId],
        conflicts: &[TokenConflict],
        errors: &mut Vec<String>,
    ) -> (Vec<TermId>, Vec<TermId>) {
        let mut terms = Vec::new();
        let mut incompatible = Vec::new();
        for term in state_terms {
            for conflict in conflicts {
                let conflicting = if conflict.left == *term {
                    Some(conflict.right)
                } else if conflict.right == *term {
                    Some(conflict.left)
                } else {
                    None
                };
                let Some(conflicting) = conflicting else {
                    continue;
                };
                if state_terms.contains(&conflicting) {
                    errors.push(format!(
                        "Overlapping tokens {} and {} used in same context",
                        self.terms.terms[*term].name, self.terms.terms[conflicting].name
                    ));
                }
                push_unique(&mut terms, *term);
                push_unique(&mut incompatible, conflicting);
            }
        }
        (terms, incompatible)
    }

    fn token_group_masks(&self, groups: &[TokenGroup]) -> BTreeMap<u16, u16> {
        let mut masks = BTreeMap::new();
        for group in groups {
            let mask = 1_u16 << group.group_id;
            for term in &group.terms {
                let output = self.terms.output_id(*term);
                *masks.entry(output).or_insert(0) |= mask;
            }
        }
        masks
    }

    fn state_has_term(&self, state: &State, term: TermId, skip_info: &[SkipInfo]) -> bool {
        if state.actions.iter().any(|action| action.term() == term) {
            return true;
        }
        let skip_index = self
            .skip_rules
            .iter()
            .position(|skip| *skip == state.skip)
            .expect("state skip set is registered");
        skip_info[skip_index].start_tokens.contains(&term)
    }

    fn check_external_conflicts(
        &mut self,
        states: &[State],
        skip_info: &[SkipInfo],
    ) -> Result<(), GeneratorError> {
        for external in &self.external_tokens {
            let mut conflicting = Vec::new();
            for identifier in &external.declaration.conflicts {
                let Some(term) = self.named_terms.get(&identifier.name).copied() else {
                    continue;
                };
                if self.terms.terms[term].terminal()
                    && !external.tokens.contains_key(&identifier.name)
                {
                    conflicting.push(term);
                }
            }
            for state in states {
                let skip_index = self
                    .skip_rules
                    .iter()
                    .position(|skip| *skip == state.skip)
                    .expect("state skip set is registered");
                let terms = state
                    .actions
                    .iter()
                    .map(Action::term)
                    .chain(skip_info[skip_index].start_tokens.iter().copied());
                let mut relevant = false;
                let mut conflict = None;
                for term in terms {
                    if external.tokens.values().any(|known| *known == term) {
                        relevant = true;
                    } else if conflicting.contains(&term) {
                        conflict = Some(term);
                    }
                }
                if relevant && let Some(conflict) = conflict {
                    return Err(GeneratorError::new(
                        format!(
                            "Tokens from external group used together with conflicting token '{}'",
                            self.terms.terms[conflict].name
                        ),
                        Some(external.declaration.start),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
struct NodeInfo {
    name: Option<String>,
    properties: Props,
    dialect: Option<usize>,
    dynamic_precedence: i16,
    inline: bool,
    group: Option<String>,
    exported: Option<String>,
}

#[derive(Clone, Copy, Debug)]
struct NodeInfoRequest<'a> {
    properties: &'a [Prop],
    capabilities: &'a str,
    default_name: Option<&'a str>,
    arguments: &'a [Expression],
    parameters: &'a [Identifier],
    expression: Option<&'a Expression>,
    default_properties: Option<&'a Props>,
}

impl<'a> NodeInfoRequest<'a> {
    const fn new(
        properties: &'a [Prop],
        capabilities: &'a str,
        default_name: Option<&'a str>,
    ) -> Self {
        Self {
            properties,
            capabilities,
            default_name,
            arguments: &[],
            parameters: &[],
            expression: None,
            default_properties: None,
        }
    }

    const fn with_template(
        mut self,
        arguments: &'a [Expression],
        parameters: &'a [Identifier],
    ) -> Self {
        self.arguments = arguments;
        self.parameters = parameters;
        self
    }

    const fn with_expression(mut self, expression: &'a Expression) -> Self {
        self.expression = Some(expression);
        self
    }
}

fn property_argument(expression: &Expression) -> Option<String> {
    match &expression.kind {
        ExpressionKind::Name(name) if name.arguments.is_empty() => Some(name.id.name.clone()),
        ExpressionKind::Literal(literal) => Some(literal.value.clone()),
        _ => None,
    }
}

fn literal_choices(expression: &Expression) -> Option<Vec<String>> {
    match &expression.kind {
        ExpressionKind::Literal(literal) => Some(vec![literal.value.clone()]),
        ExpressionKind::Choice(choices) => choices
            .iter()
            .map(|choice| match &choice.kind {
                ExpressionKind::Literal(literal) => Some(literal.value.clone()),
                _ => None,
            })
            .collect(),
        ExpressionKind::Sequence { expressions, .. } => {
            let mut value = String::new();
            for expression in expressions {
                let ExpressionKind::Literal(literal) = &expression.kind else {
                    return None;
                };
                value.push_str(&literal.value);
            }
            Some(vec![value])
        }
        _ => None,
    }
}

fn is_empty(expression: &Expression) -> bool {
    matches!(
        &expression.kind,
        ExpressionKind::Sequence { expressions, .. } if expressions.is_empty()
    )
}

fn is_exported(rule: &RuleDeclaration) -> bool {
    rule.props
        .iter()
        .any(|property| property.at && property.name == "export")
}

fn ignored(name: &str) -> bool {
    let Some(first) = name.chars().next() else {
        return true;
    };
    first == '_' || !first.to_uppercase().eq(std::iter::once(first))
}

fn quoted(value: &str) -> String {
    format!("{value:?}")
}

fn format_expression(expression: &Expression) -> String {
    match &expression.kind {
        ExpressionKind::Name(name) => {
            if name.arguments.is_empty() {
                name.id.name.clone()
            } else {
                let arguments = name
                    .arguments
                    .iter()
                    .map(format_expression)
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{}<{arguments}>", name.id.name)
            }
        }
        ExpressionKind::Literal(literal) => quoted(&literal.value),
        ExpressionKind::Choice(choices) => choices
            .iter()
            .map(format_expression)
            .collect::<Vec<_>>()
            .join(" | "),
        ExpressionKind::Sequence {
            expressions,
            explicitly_empty,
            ..
        } => {
            if *explicitly_empty && expressions.is_empty() {
                "()".to_owned()
            } else {
                expressions
                    .iter()
                    .map(format_expression)
                    .collect::<Vec<_>>()
                    .join(" ")
            }
        }
        ExpressionKind::Repeat { expression, kind } => {
            let suffix = match kind {
                RepeatKind::Optional => '?',
                RepeatKind::ZeroOrMore => '*',
                RepeatKind::OneOrMore => '+',
            };
            format!("{}{suffix}", format_expression(expression))
        }
        ExpressionKind::Specialize(specialize) => {
            let kind = match specialize.kind {
                SpecializeKind::Extend => "extend",
                SpecializeKind::Specialize => "specialize",
            };
            format!(
                "@{kind}<{},{}>",
                format_expression(&specialize.token),
                format_expression(&specialize.content)
            )
        }
        ExpressionKind::InlineRule(rule) => rule.id.name.clone(),
        ExpressionKind::Set(set) => format!("{:?}", set.ranges),
        ExpressionKind::Any => "_".to_owned(),
        ExpressionKind::CharClass(class) => format!("@{class:?}"),
    }
}

fn add_property(properties: &mut Props, name: &str, value: &str) {
    let current = properties.entry(name.to_owned()).or_default();
    if current.split_ascii_whitespace().any(|known| known == value) {
        return;
    }
    if !current.is_empty() {
        current.push(' ');
    }
    current.push_str(value);
}

fn format_name_expression(expression: &NameExpression) -> String {
    if expression.arguments.is_empty() {
        expression.id.name.clone()
    } else {
        let arguments = expression
            .arguments
            .iter()
            .map(format_expression)
            .collect::<Vec<_>>()
            .join(",");
        format!("{}<{arguments}>", expression.id.name)
    }
}

fn invert_ranges(ranges: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut position = 0;
    let mut result = Vec::new();
    for (low, high) in ranges {
        if *low > position {
            result.push((position, *low));
        }
        position = *high;
    }
    if position <= MAX_CODE_POINT {
        result.push((position, MAX_CODE_POINT + 1));
    }
    result
}

fn add_precedence_relation(relations: &mut Vec<TokenPrecedence>, term: TermId, after: &[TermId]) {
    if let Some(relation) = relations.iter_mut().find(|relation| relation.term == term) {
        for term in after {
            if !relation.after.contains(term) {
                relation.after.push(*term);
            }
        }
    } else {
        relations.push(TokenPrecedence {
            term,
            after: after.to_vec(),
        });
    }
}

fn find_array(data: &[u16], value: &[u16]) -> Option<usize> {
    if value.is_empty() {
        return Some(0);
    }
    data.windows(value.len())
        .position(|candidate| candidate == value)
}

fn sequence_with_end(values: &[u16]) -> Vec<u16> {
    let mut result = values.to_vec();
    result.push(SequenceCode::End.raw());
    result
}

fn push_u32_words(output: &mut Vec<u16>, value: u32) {
    let low = value & u32::from(u16::MAX);
    let high = value >> 16;
    output.push(u16::try_from(low).expect("masked action word fits u16"));
    output.push(u16::try_from(high).expect("shifted action word fits u16"));
}

fn find_shared_actions(
    state: &State,
    states: &[State],
    shared_actions: &mut Vec<SharedActions>,
    data: &mut DataBuilder,
    context: ActionStoreContext<'_>,
) -> Result<Option<SharedActions>, GeneratorError> {
    if state.actions.len() < MIN_SHARED_ACTIONS {
        return Ok(None);
    }
    let existing = shared_actions
        .iter()
        .filter(|shared| {
            shared
                .actions
                .iter()
                .all(|action| state.actions.contains(action))
        })
        .max_by_key(|shared| shared.actions.len())
        .cloned();
    if existing.is_some() {
        return Ok(existing);
    }
    let mut best = Vec::new();
    for other in states.iter().skip(state.id + 1) {
        if other.default_reduce.is_some() || other.actions.len() < MIN_SHARED_ACTIONS {
            continue;
        }
        let intersection = state
            .actions
            .iter()
            .filter(|action| other.actions.contains(action))
            .cloned()
            .collect::<Vec<_>>();
        if intersection.len() >= MIN_SHARED_ACTIONS && intersection.len() > best.len() {
            best = intersection;
        }
    }
    if best.is_empty() {
        return Ok(None);
    }
    let address = store_actions(&best, None, None, data, context)?;
    let shared = SharedActions {
        actions: best,
        address,
    };
    shared_actions.push(shared.clone());
    Ok(Some(shared))
}

fn store_actions(
    actions: &[Action],
    skip_reduce: Option<u32>,
    shared: Option<&SharedActions>,
    data: &mut DataBuilder,
    context: ActionStoreContext<'_>,
) -> Result<usize, GeneratorError> {
    if skip_reduce.is_none() && shared.is_some_and(|shared| shared.actions.len() == actions.len()) {
        return Ok(shared.expect("checked shared actions").address);
    }
    let mut encoded = Vec::new();
    for action in actions {
        if shared.is_some_and(|shared| shared.actions.contains(action)) {
            continue;
        }
        let (term, raw) = match *action {
            Action::Shift { term, target } => {
                let target = u16::try_from(target)
                    .map_err(|_| GeneratorError::new("Too many parser states", None))?;
                (
                    context.terms.output_id(term),
                    RuntimeAction::shift(target, false, false).raw(),
                )
            }
            Action::Reduce { term, rule } => (
                context.terms.output_id(term),
                reduce_action(rule, None, context.rules, context.terms, context.skip_info),
            ),
        };
        if Some(raw) == skip_reduce {
            continue;
        }
        encoded.push(term);
        push_u32_words(&mut encoded, raw);
    }
    encoded.push(SequenceCode::End.raw());
    if let Some(skip_reduce) = skip_reduce {
        encoded.push(SequenceCode::Other.raw());
        push_u32_words(&mut encoded, skip_reduce);
    } else if let Some(shared) = shared {
        let address = u32::try_from(shared.address)
            .map_err(|_| GeneratorError::new("State data too large", None))?;
        encoded.push(SequenceCode::Next.raw());
        push_u32_words(&mut encoded, address);
    } else {
        encoded.push(SequenceCode::Done.raw());
    }
    data.store_array(&encoded)
}

fn reduce_action(
    rule_id: RuleId,
    depth: Option<usize>,
    rules: &[Rule],
    terms: &TermSet,
    skip_info: &[SkipInfo],
) -> u32 {
    let rule = &rules[rule_id];
    let depth = depth.unwrap_or(rule.parts.len());
    let depth = u32::try_from(depth).expect("production depth fits action encoding");
    let repeat =
        rule.is_repeat_wrap(terms) && usize::try_from(depth).ok() == Some(rule.parts.len());
    let stay = skip_info.iter().any(|info| info.rule == Some(rule.name));
    RuntimeAction::reduce(terms.output_id(rule.name), depth, repeat, stay).raw()
}

fn find_non_skip_states(states: &[State], top_rules: &[TermId]) -> BTreeSet<usize> {
    let mut seen = BTreeSet::new();
    let mut work = states
        .iter()
        .filter(|state| {
            state
                .start_rule
                .is_some_and(|rule| top_rules.contains(&rule))
        })
        .map(|state| state.id)
        .collect::<Vec<_>>();
    for state in &work {
        seen.insert(*state);
    }
    let mut index = 0;
    while index < work.len() {
        let state = &states[work[index]];
        for target in state
            .actions
            .iter()
            .chain(&state.gotos)
            .filter_map(|action| match action {
                Action::Shift { target, .. } => Some(*target),
                Action::Reduce { .. } => None,
            })
        {
            if seen.insert(target) {
                work.push(target);
            }
        }
        index += 1;
    }
    seen
}

fn compute_goto_table(states: &[State], terms: &TermSet) -> Result<Vec<u16>, GeneratorError> {
    let mut goto: BTreeMap<u16, BTreeMap<u16, Vec<u16>>> = BTreeMap::new();
    let mut max_term = 0;
    for state in states {
        for action in &state.gotos {
            let Action::Shift { term, target } = *action else {
                continue;
            };
            let term = terms.output_id(term);
            let target = u16::try_from(target)
                .map_err(|_| GeneratorError::new("Too many parser states", None))?;
            let source = u16::try_from(state.id)
                .map_err(|_| GeneratorError::new("Too many parser states", None))?;
            max_term = max_term.max(term);
            goto.entry(term)
                .or_default()
                .entry(target)
                .or_default()
                .push(source);
        }
    }
    let mut data = DataBuilder::default();
    let mut index = Vec::new();
    for term in 0..=max_term {
        let Some(entries) = goto.get(&term) else {
            index.push(None);
            continue;
        };
        let mut table = Vec::new();
        let mut groups = entries.iter().collect::<Vec<_>>();
        let default_index = groups
            .iter()
            .enumerate()
            .max_by_key(|(_, (target, sources))| (sources.len(), **target))
            .map(|(index, _)| index)
            .expect("a goto term has at least one target");
        let default = groups.remove(default_index);
        groups.push(default);
        for (entry_index, (target, sources)) in groups.iter().copied().enumerate() {
            let last = entry_index + 1 == groups.len();
            store_goto_group(&mut table, *target, sources, last)?;
        }
        let position = data.store_array(&table)?;
        index.push(Some(position));
    }
    finish_goto_table(&index, data.finish())
}

fn finish_goto_table(index: &[Option<usize>], data: Vec<u16>) -> Result<Vec<u16>, GeneratorError> {
    let term_count =
        u16::try_from(index.len()).map_err(|_| GeneratorError::new("Too many goto terms", None))?;
    if term_count == GOTO_COMPRESSED_HEADER {
        return Err(GeneratorError::new("Too many goto terms", None));
    }

    let compressed = encode_goto_header(index);
    let compressed_header_length = compressed.len() + 3;
    let raw_header_length = index.len() + 1;
    let mut result = Vec::new();
    if compressed_header_length < raw_header_length {
        let compressed_length = u16::try_from(compressed.len())
            .map_err(|_| GeneratorError::new("Goto header too large", None))?;
        result.push(GOTO_COMPRESSED_HEADER);
        result.push(term_count);
        result.push(compressed_length);
        result.extend(compressed);
    } else {
        result.push(term_count);
        for position in index {
            let position = position.map_or(1, |position| position + raw_header_length);
            result.push(
                u16::try_from(position)
                    .map_err(|_| GeneratorError::new("Goto table too large", None))?,
            );
        }
    }
    result.extend(data);
    if result.len() > usize::from(u16::MAX) {
        return Err(GeneratorError::new("Goto table too large", None));
    }
    Ok(result)
}

fn encode_goto_header(index: &[Option<usize>]) -> Vec<u16> {
    let mut bytes = Vec::new();
    let mut previous = 0_i64;
    for position in index {
        let Some(position) = position else {
            encode_varint(&mut bytes, 0);
            continue;
        };
        let position = i64::try_from(*position).expect("goto table positions fit i64");
        let delta = position - previous;
        let zigzag =
            u64::try_from((delta << 1) ^ (delta >> 63)).expect("zigzag encoding is nonnegative");
        encode_varint(&mut bytes, zigzag + 1);
        previous = position;
    }
    pack_bytes(&bytes)
}

fn store_goto_group(
    table: &mut Vec<u16>,
    target: u16,
    sources: &[u16],
    last: bool,
) -> Result<(), GeneratorError> {
    let source_count = u16::try_from(sources.len())
        .map_err(|_| GeneratorError::new("Goto group too large", None))?;
    let compressed = encode_goto_source_deltas(sources)?;
    if compressed.len() + 1 < sources.len() {
        table.push(GOTO_COMPRESSED_TAG | u16::from(last));
        table.push(target);
        table.push(source_count);
        table.extend(compressed);
        return Ok(());
    }

    let tag = source_count
        .checked_mul(2)
        .and_then(|value| value.checked_add(u16::from(last)))
        .ok_or_else(|| GeneratorError::new("Goto group too large", None))?;
    table.push(tag);
    table.push(target);
    table.extend_from_slice(sources);
    Ok(())
}

fn encode_goto_source_deltas(sources: &[u16]) -> Result<Vec<u16>, GeneratorError> {
    let mut bytes = Vec::new();
    let mut previous = 0_u16;
    for (index, source) in sources.iter().copied().enumerate() {
        let delta = if index == 0 {
            source
        } else {
            source
                .checked_sub(previous)
                .filter(|delta| *delta != 0)
                .ok_or_else(|| {
                    GeneratorError::new("Goto group sources must be strictly increasing", None)
                })?
        };
        previous = source;

        encode_varint(&mut bytes, u64::from(delta));
    }
    Ok(pack_bytes(&bytes))
}

fn encode_varint(bytes: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = u8::try_from(value & 0x7f).expect("masked varint fits a byte");
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        bytes.push(byte);
        if value == 0 {
            return;
        }
    }
}

fn pack_bytes(bytes: &[u8]) -> Vec<u16> {
    bytes
        .chunks(2)
        .map(|bytes| {
            let low = u16::from(bytes[0]);
            let high = bytes.get(1).copied().map_or(0, u16::from);
            low | (high << 8)
        })
        .collect()
}

fn creates_forced_cycle(
    term: TermId,
    start_state: usize,
    parents: Option<&[usize]>,
    goto_edges: &BTreeMap<TermId, Vec<GotoParents>>,
    reductions: &BTreeMap<usize, TermId>,
) -> bool {
    let Some(edges) = goto_edges.get(&term) else {
        return false;
    };
    edges.iter().any(|edge| {
        let parent_intersection = parents.map_or_else(
            || edge.parents.clone(),
            |parents| {
                parents
                    .iter()
                    .filter(|parent| edge.parents.contains(parent))
                    .copied()
                    .collect()
            },
        );
        if parent_intersection.is_empty() {
            return false;
        }
        if edge.target == start_state {
            return true;
        }
        reductions.get(&edge.target).is_some_and(|next| {
            creates_forced_cycle(
                *next,
                start_state,
                Some(&parent_intersection),
                goto_edges,
                reductions,
            )
        })
    })
}

fn push_unique<T>(values: &mut Vec<T>, value: T)
where
    T: Eq,
{
    if !values.contains(&value) {
        values.push(value);
    }
}

fn simplify_rules(rules: Vec<Rule>, preserve: &[TermId], terms: &TermSet) -> Vec<Rule> {
    let rules = inline_rules(rules, preserve, terms);
    merge_rules(rules, terms)
}

fn inline_rules(mut rules: Vec<Rule>, preserve: &[TermId], terms: &TermSet) -> Vec<Rule> {
    for pass in 0.. {
        let mut inlinable: BTreeMap<TermId, Vec<Rule>> = BTreeMap::new();
        if pass == 0 {
            for rule in &rules {
                if !terms.terms[rule.name].inline() || inlinable.contains_key(&rule.name) {
                    continue;
                }
                let group = rules
                    .iter()
                    .filter(|candidate| candidate.name == rule.name)
                    .cloned()
                    .collect::<Vec<_>>();
                if !group
                    .iter()
                    .any(|candidate| candidate.parts.contains(&rule.name))
                {
                    inlinable.insert(rule.name, group);
                }
            }
        }
        for (index, rule) in rules.iter().enumerate() {
            let unique_name = !rules
                .iter()
                .enumerate()
                .any(|(other_index, other)| index != other_index && other.name == rule.name);
            let compatible_skip = rule.parts.len() == 1
                || rules
                    .iter()
                    .all(|other| other.skip == rule.skip || !other.parts.contains(&rule.name));
            let contains_inlinable = rule.parts.iter().any(|part| inlinable.contains_key(part));
            if !terms.terms[rule.name].interesting()
                && !rule.parts.contains(&rule.name)
                && rule.parts.len() < 3
                && !preserve.contains(&rule.name)
                && compatible_skip
                && !contains_inlinable
                && unique_name
            {
                inlinable.insert(rule.name, vec![rule.clone()]);
            }
        }
        if inlinable.is_empty() {
            return rules;
        }
        let mut expanded = Vec::new();
        for rule in &rules {
            if inlinable.contains_key(&rule.name) {
                continue;
            }
            if !rule.parts.iter().any(|part| inlinable.contains_key(part)) {
                expanded.push(rule.clone());
                continue;
            }
            expand_inlined_rule(
                rule,
                0,
                vec![rule.conflicts[0].clone()],
                Vec::new(),
                &inlinable,
                &mut expanded,
            );
        }
        for (id, rule) in expanded.iter_mut().enumerate() {
            rule.id = id;
        }
        rules = expanded;
    }
    unreachable!("inline pass returns when no candidate remains")
}

fn expand_inlined_rule(
    outer: &Rule,
    position: usize,
    conflicts: Vec<Conflicts>,
    parts: Vec<TermId>,
    inlinable: &BTreeMap<TermId, Vec<Rule>>,
    output: &mut Vec<Rule>,
) {
    if position == outer.parts.len() {
        output.push(Rule {
            id: output.len(),
            name: outer.name,
            parts,
            conflicts,
            skip: outer.skip,
        });
        return;
    }
    let next = outer.parts[position];
    let Some(replacements) = inlinable.get(&next) else {
        let mut conflicts = conflicts;
        conflicts.push(outer.conflicts[position + 1].clone());
        let mut parts = parts;
        parts.push(next);
        expand_inlined_rule(outer, position + 1, conflicts, parts, inlinable, output);
        return;
    };
    for replacement in replacements {
        let mut next_conflicts = conflicts[..conflicts.len() - 1].to_vec();
        let left = conflicts[position].join(&replacement.conflicts[0]);
        next_conflicts.push(left);
        let interior_count = replacement.conflicts.len().saturating_sub(2);
        next_conflicts.extend(
            replacement
                .conflicts
                .iter()
                .skip(1)
                .take(interior_count)
                .cloned(),
        );
        let right = outer.conflicts[position + 1].join(
            replacement
                .conflicts
                .last()
                .expect("a rule has a final conflict position"),
        );
        next_conflicts.push(right);
        let mut next_parts = parts.clone();
        next_parts.extend_from_slice(&replacement.parts);
        expand_inlined_rule(
            outer,
            position + 1,
            next_conflicts,
            next_parts,
            inlinable,
            output,
        );
    }
}

fn merge_rules(rules: Vec<Rule>, terms: &TermSet) -> Vec<Rule> {
    let order = rules
        .iter()
        .map(|rule| rule.name)
        .fold(Vec::new(), |mut names, name| {
            push_unique(&mut names, name);
            names
        });
    let groups = order
        .iter()
        .map(|name| {
            (
                *name,
                rules
                    .iter()
                    .filter(|rule| rule.name == *name)
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut merged = BTreeMap::new();
    for (left_index, left_name) in order.iter().enumerate() {
        if terms.terms[*left_name].interesting() {
            continue;
        }
        let left = &groups[left_name];
        for right_name in order.iter().skip(left_index + 1) {
            if terms.terms[*right_name].interesting() {
                continue;
            }
            let right = &groups[right_name];
            if left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| same_rule_without_name(left, right))
            {
                merged.insert(*left_name, *right_name);
                break;
            }
        }
    }
    if merged.is_empty() {
        return rules;
    }
    rules
        .into_iter()
        .filter_map(|mut rule| {
            if merged.contains_key(&rule.name) {
                return None;
            }
            for part in &mut rule.parts {
                if let Some(replacement) = merged.get(part) {
                    *part = *replacement;
                }
            }
            Some(rule)
        })
        .enumerate()
        .map(|(id, mut rule)| {
            rule.id = id;
            rule
        })
        .collect()
}

fn same_rule_without_name(left: &Rule, right: &Rule) -> bool {
    left.parts.len() == right.parts.len()
        && left.skip == right.skip
        && left.parts == right.parts
        && left.conflicts == right.conflicts
}

#[cfg(test)]
mod goto_encoding_tests {
    use super::{encode_goto_header, encode_goto_source_deltas};

    #[test]
    fn packs_empty_and_shared_goto_header_entries() {
        let encoded = encode_goto_header(&[Some(0), None, Some(0), Some(130), Some(129)]);
        assert_eq!(encoded, [0x0001, 0x8501, 0x0202]);
    }

    #[test]
    fn packs_sorted_source_deltas_two_bytes_per_word() {
        let encoded = encode_goto_source_deltas(&[0, 1, 130, 131, 400]).unwrap();
        assert_eq!(encoded, [0x0100, 0x0181, 0x8d01, 0x0002]);
    }

    #[test]
    fn rejects_unsorted_or_duplicate_sources() {
        assert!(encode_goto_source_deltas(&[1, 1]).is_err());
        assert!(encode_goto_source_deltas(&[2, 1]).is_err());
    }
}
