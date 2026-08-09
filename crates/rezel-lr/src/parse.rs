use std::any::Any;
use std::cmp::Reverse;
use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use rezel_common::{
    NodeSet, NodeType, ParseError, ParseErrorKind, ParseRequest, ParseWrapper, Parser,
    PartialParse, TextSize, Tree, TreeBuild,
};
use zerocopy::{FromBytes, Immutable};

use crate::action_index::ActionIndex;
use crate::decode::pair;
use crate::goto_index::{GotoIndex, decode_goto_header, decode_goto_sources};
use crate::stack::Stack;
use crate::table::{Action, ReservedTerm, SequenceCode, StateField, StateFlag};
use crate::token::{
    AcceptedToken, InputStream, TokenAsciiIndex, TokenTable, Tokenizer, TokenizerStartIndex,
};

// Non-incremental parses benefit from amortizing compact-tree allocation over
// larger buffers than Lezer's incremental-friendly common default.
const DEFAULT_PARSE_BUFFER_LENGTH: TextSize = TextSize::new(16 * 1024);

/// One named grammar entry point.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TopRule {
    /// Public top-rule name.
    pub name: &'static str,
    /// Initial LR state.
    pub state: u16,
    /// Root node term.
    pub term: u16,
}

/// Tokens disabled unless one dialect is enabled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DialectSpec {
    /// Dialect name used by parser configuration.
    pub name: &'static str,
    /// Terms exclusively enabled by this dialect.
    pub terms: &'static [u16],
}

/// Runtime dialect selection.
#[derive(Clone, Debug)]
pub struct Dialect {
    source: Arc<str>,
    flags: Arc<[bool]>,
    disabled: Option<Arc<[bool]>>,
}

impl Dialect {
    fn from_specs(
        source: Option<&str>,
        specs: &[DialectSpec],
        max_term: u16,
    ) -> Result<Self, ParseError> {
        let mut flags = vec![false; specs.len()];
        let mut requested = BTreeSet::new();
        if let Some(source) = source {
            for name in source.split_ascii_whitespace() {
                let Some(index) = specs.iter().position(|dialect| dialect.name == name) else {
                    return Err(ParseError::new(
                        ParseErrorKind::Configuration,
                        None,
                        format!("unknown dialect {name:?}"),
                    ));
                };
                flags[index] = true;
                requested.insert(name);
            }
        }
        let mut disabled = None;
        for (index, spec) in specs.iter().enumerate() {
            if flags[index] {
                continue;
            }
            for term in spec.terms {
                let disabled =
                    disabled.get_or_insert_with(|| vec![false; usize::from(max_term) + 1]);
                if let Some(slot) = disabled.get_mut(usize::from(*term)) {
                    *slot = true;
                }
            }
        }
        Ok(Self {
            source: requested.into_iter().collect::<Vec<_>>().join(" ").into(),
            flags: flags.into(),
            disabled: disabled.map(Into::into),
        })
    }

    /// Original normalized dialect selection.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Whether a dialect numeric id is enabled.
    #[must_use]
    pub fn enabled(&self, id: usize) -> bool {
        self.flags.get(id).copied().unwrap_or(false)
    }

    pub(crate) fn allows(&self, term: u16) -> bool {
        self.disabled
            .as_ref()
            .is_none_or(|disabled| !disabled.get(usize::from(term)).copied().unwrap_or(true))
    }

    pub(crate) const fn has_disabled_terms(&self) -> bool {
        self.disabled.is_some()
    }
}

/// Type-erased immutable context value used by a context tracker.
#[derive(Clone)]
pub struct ContextValue(Arc<dyn Any + Send + Sync>);

impl ContextValue {
    /// Wrap one typed immutable context.
    #[must_use]
    pub fn new<T>(value: T) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self(Arc::new(value))
    }

    /// Borrow the concrete context type.
    #[must_use]
    pub fn downcast_ref<T>(&self) -> Option<&T>
    where
        T: Send + Sync + 'static,
    {
        self.0.downcast_ref()
    }

    pub(crate) fn same_identity(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for ContextValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ContextValue(..)")
    }
}

type ContextTransition =
    fn(&ContextValue, u16, &Stack, &mut InputStream) -> Result<ContextValue, ParseError>;
type ContextTransitionWithoutInput =
    fn(&ContextValue, u16, &Stack) -> Result<ContextValue, ParseError>;
type ContextOnlyShiftTransition =
    fn(&ContextValue, u16) -> Result<Option<ContextValue>, ParseError>;

#[derive(Clone, Copy)]
enum ContextTransitionKind {
    WithInput(ContextTransition),
    WithoutInput(ContextTransitionWithoutInput),
}

#[derive(Clone, Copy)]
enum ShiftContextTransitionKind {
    WithInput(ContextTransition),
    WithoutInput(ContextTransitionWithoutInput),
    ContextOnly(ContextOnlyShiftTransition),
}

/// Statically linked non-incremental context tracker.
pub struct ContextTracker {
    start: fn() -> ContextValue,
    shift: Option<ShiftContextTransitionKind>,
    shift_term_filter: u64,
    shift_input_terms: Option<&'static [u16]>,
    input_shift_override: Option<ContextTransition>,
    input_shift_override_terms: &'static [u16],
    input_shift_override_filter: u64,
    reduce: Option<ContextTransitionKind>,
    reduce_term_filter: u64,
    hash: fn(&ContextValue) -> u64,
}

impl ContextTracker {
    /// Define a Rust context tracker whose transitions may observe input.
    ///
    /// The runtime positions the input stream at the transition start before
    /// invoking each supplied callback.
    #[must_use]
    pub const fn new(
        start: fn() -> ContextValue,
        shift: Option<ContextTransition>,
        reduce: Option<ContextTransition>,
        hash: fn(&ContextValue) -> u64,
    ) -> Self {
        Self {
            start,
            shift: match shift {
                Some(shift) => Some(ShiftContextTransitionKind::WithInput(shift)),
                None => None,
            },
            shift_term_filter: u64::MAX,
            shift_input_terms: None,
            input_shift_override: None,
            input_shift_override_terms: &[],
            input_shift_override_filter: 0,
            reduce: match reduce {
                Some(reduce) => Some(ContextTransitionKind::WithInput(reduce)),
                None => None,
            },
            reduce_term_filter: u64::MAX,
            hash,
        }
    }

    /// Replace the shift transition with one that cannot observe input.
    ///
    /// The runtime does not reposition the input stream before this callback.
    #[must_use]
    pub const fn with_shift_without_input(mut self, shift: ContextTransitionWithoutInput) -> Self {
        self.shift = Some(ShiftContextTransitionKind::WithoutInput(shift));
        self
    }

    /// Replace the shift transition with one that observes only its context.
    ///
    /// Returning `None` preserves the current context identity. This avoids
    /// cloning the type-erased value or the surrounding stack context when a
    /// transition neither reads parser state nor changes its value.
    #[must_use]
    pub const fn with_context_only_shift(mut self, shift: ContextOnlyShiftTransition) -> Self {
        self.shift = Some(ShiftContextTransitionKind::ContextOnly(shift));
        self
    }

    /// Restrict shift callbacks to a conservative set of relevant terms.
    ///
    /// The callback must return its input [`ContextValue`] identity for every
    /// omitted term. Hash collisions may still invoke it for an omitted term.
    #[must_use]
    pub const fn with_shift_terms(mut self, terms: &'static [u16]) -> Self {
        self.shift_term_filter = context_term_filter(terms);
        self
    }

    /// Restrict input observation to the listed shifted terms.
    ///
    /// The runtime does not reposition the input stream before invoking the
    /// callback for other terms. The callback must not inspect or advance its
    /// `InputStream` argument for a term absent from this list.
    #[must_use]
    pub const fn with_shift_input_terms(mut self, terms: &'static [u16]) -> Self {
        self.shift_input_terms = Some(terms);
        self
    }

    /// Override selected shifts with a transition that observes input.
    ///
    /// Other shifted terms continue to use the tracker's primary transition.
    /// This lets a context-only fast path coexist with a small number of
    /// lexical-mode transitions that need the shifted source spelling.
    #[must_use]
    pub const fn with_input_shift_for_terms(
        mut self,
        shift: ContextTransition,
        terms: &'static [u16],
    ) -> Self {
        self.input_shift_override = Some(shift);
        self.input_shift_override_terms = terms;
        self.input_shift_override_filter = context_term_filter(terms);
        self
    }

    /// Restrict reduce callbacks to a conservative set of relevant terms.
    ///
    /// The callback must return its input [`ContextValue`] identity for every
    /// omitted term. Hash collisions may still invoke it for an omitted term.
    #[must_use]
    pub const fn with_reduce_terms(mut self, terms: &'static [u16]) -> Self {
        self.reduce_term_filter = context_term_filter(terms);
        self
    }

    /// Replace the reduce transition with one that cannot observe input.
    ///
    /// The runtime does not reposition the input stream before this callback.
    #[must_use]
    pub const fn with_reduce_without_input(
        mut self,
        reduce: ContextTransitionWithoutInput,
    ) -> Self {
        self.reduce = Some(ContextTransitionKind::WithoutInput(reduce));
        self
    }

    pub(crate) fn start(&self) -> ContextValue {
        (self.start)()
    }

    #[inline]
    pub(crate) fn shift_uses_input(&self, term: u16) -> bool {
        self.uses_input_shift_override(term)
            || matches!(self.shift, Some(ShiftContextTransitionKind::WithInput(_)))
                && self
                    .shift_input_terms
                    .is_none_or(|terms| terms.contains(&term))
    }

    pub(crate) const fn tracks_shift(&self, term: u16) -> bool {
        self.shift.is_some() && self.shift_term_filter & context_term_bit(term) != 0
            || self.input_shift_override.is_some()
                && self.input_shift_override_filter & context_term_bit(term) != 0
    }

    pub(crate) const fn tracks_reduction(&self, term: u16) -> bool {
        self.reduce.is_some() && self.reduce_term_filter & context_term_bit(term) != 0
    }

    #[inline]
    pub(crate) const fn has_context_only_shift(&self) -> bool {
        matches!(self.shift, Some(ShiftContextTransitionKind::ContextOnly(_)))
    }

    pub(crate) fn context_only_shift(
        &self,
        context: &ContextValue,
        term: u16,
    ) -> Result<Option<ContextValue>, ParseError> {
        let Some(ShiftContextTransitionKind::ContextOnly(shift)) = self.shift else {
            return Ok(None);
        };
        shift(context, term)
    }

    pub(crate) const fn reduction_uses_input(&self) -> bool {
        matches!(self.reduce, Some(ContextTransitionKind::WithInput(_)))
    }

    pub(crate) fn shift(
        &self,
        context: &ContextValue,
        term: u16,
        stack: &Stack,
        input: &mut InputStream,
    ) -> Result<ContextValue, ParseError> {
        if self.uses_input_shift_override(term) {
            let shift = self
                .input_shift_override
                .expect("an input shift override callback is configured");
            return shift(context, term, stack, input);
        }
        match self.shift {
            Some(ShiftContextTransitionKind::WithInput(shift)) => {
                shift(context, term, stack, input)
            }
            Some(ShiftContextTransitionKind::WithoutInput(shift)) => shift(context, term, stack),
            Some(ShiftContextTransitionKind::ContextOnly(shift)) => {
                Ok(shift(context, term)?.unwrap_or_else(|| context.clone()))
            }
            None => Ok(context.clone()),
        }
    }

    pub(crate) fn reduce(
        &self,
        context: &ContextValue,
        term: u16,
        stack: &Stack,
        input: &mut InputStream,
    ) -> Result<ContextValue, ParseError> {
        match self.reduce {
            Some(ContextTransitionKind::WithInput(reduce)) => reduce(context, term, stack, input),
            Some(ContextTransitionKind::WithoutInput(reduce)) => reduce(context, term, stack),
            None => Ok(context.clone()),
        }
    }

    pub(crate) fn hash(&self, context: &ContextValue) -> u64 {
        (self.hash)(context)
    }

    #[inline]
    fn uses_input_shift_override(&self, term: u16) -> bool {
        self.input_shift_override.is_some()
            && self.input_shift_override_filter & context_term_bit(term) != 0
            && self.input_shift_override_terms.contains(&term)
    }
}

const fn context_term_filter(terms: &[u16]) -> u64 {
    let mut filter = 0;
    let mut index = 0;
    while index < terms.len() {
        filter |= context_term_bit(terms[index]);
        index += 1;
    }
    filter
}

const fn context_term_bit(term: u16) -> u64 {
    let folded = term ^ (term >> 6) ^ (term >> 12);
    1_u64 << (folded & 63)
}

impl fmt::Debug for ContextTracker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ContextTracker(..)")
    }
}

/// External or generated token specializer callback.
pub type ExternalSpecializer = fn(&str, &Stack) -> Option<u16>;

/// Whether a specialized token replaces or extends its base token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Specialize {
    /// Replace the base token.
    Replace,
    /// Add another interpretation while retaining the base token.
    Extend,
}

/// One token selected by a generated or external specializer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpecializedToken {
    /// Specialized terminal.
    pub term: u16,
    /// Replacement or extension behavior.
    pub kind: Specialize,
}

impl SpecializedToken {
    /// Construct one specialization decision.
    #[must_use]
    pub const fn new(term: u16, kind: Specialize) -> Self {
        Self { term, kind }
    }
}

/// One statically bound specializer.
#[derive(Clone, Copy)]
pub struct SpecializerSpec {
    /// Base token type.
    pub term: u16,
    /// Rust callback returning the specialized token and behavior.
    pub get: fn(&str, &Stack) -> Option<SpecializedToken>,
}

/// One strict-only validator for the spelling of a base token.
///
/// Validation runs after token precedence has selected a parser-visible token
/// and before an LR action consumes it. Recovering parsers never call the
/// validator.
#[derive(Clone, Copy)]
pub struct StrictTokenValidator {
    term: u16,
    validate: fn(&str) -> Result<(), StrictTokenValidationError>,
}

impl StrictTokenValidator {
    /// Bind one base token term to a strict spelling validator.
    #[must_use]
    pub const fn new(
        term: u16,
        validate: fn(&str) -> Result<(), StrictTokenValidationError>,
    ) -> Self {
        Self { term, validate }
    }
}

impl fmt::Debug for StrictTokenValidator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StrictTokenValidator")
            .field("term", &self.term)
            .finish_non_exhaustive()
    }
}

/// A strict token spelling error at one UTF-8 byte offset in the token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StrictTokenValidationError {
    offset: TextSize,
    message: &'static str,
}

impl StrictTokenValidationError {
    /// Construct a strict token validation error.
    #[must_use]
    pub const fn new(offset: TextSize, message: &'static str) -> Self {
        Self { offset, message }
    }
}

impl fmt::Debug for SpecializerSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SpecializerSpec")
            .field("term", &self.term)
            .finish_non_exhaustive()
    }
}

/// One generated dynamic reduction precedence.
#[repr(C, align(2))]
#[derive(Clone, Copy, Debug, Eq, FromBytes, Immutable, PartialEq)]
pub struct DynamicPrecedence {
    term: u16,
    value: i16,
}

impl DynamicPrecedence {
    /// Construct one generated dynamic-precedence entry.
    #[doc(hidden)]
    #[must_use]
    pub const fn new(term: u16, value: i16) -> Self {
        Self { term, value }
    }
}

/// Static parser data emitted for one grammar.
pub struct Language {
    /// Six compact words per LR state.
    pub states: &'static [u32],
    /// Compact action sequences and token-precedence data.
    pub state_data: &'static [u16],
    /// Compact nonterminal goto table.
    pub goto: &'static [u16],
    /// Generated global token DFA.
    pub token_table: &'static TokenTable,
    /// Tokenizer precedence order and groups.
    pub tokenizers: &'static [Tokenizer],
    /// Grammar entry points.
    pub top_rules: &'static [TopRule],
    /// Highest allocated grammar term.
    pub max_term: u16,
    /// First anonymous repeat term.
    pub min_repeat_term: u16,
    /// Offset of token precedence in [`Self::state_data`].
    pub token_precedence: usize,
    /// Shared node set including generated and externally supplied props.
    pub node_set: fn() -> &'static Arc<NodeSet>,
    /// Optional statically bound context tracker.
    pub context: Option<&'static ContextTracker>,
    /// Dialect declarations.
    pub dialects: &'static [DialectSpec],
    /// Sparse dynamic reduction precedences.
    pub dynamic_precedences: &'static [DynamicPrecedence],
    /// Token specializers.
    pub specializers: &'static [SpecializerSpec],
    /// Optional names for terms outside the node set.
    pub term_names: &'static [(u16, &'static str)],
}

impl fmt::Debug for Language {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Language")
            .field("state_count", &(self.states.len() / StateField::COUNT))
            .field("max_term", &self.max_term)
            .field("min_repeat_term", &self.min_repeat_term)
            .field("top_rules", &self.top_rules)
            .finish_non_exhaustive()
    }
}

/// Explicit runtime resource ceilings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseLimits {
    /// Maximum LR actions executed by one parse.
    pub max_actions: usize,
    /// Maximum simultaneously live parse stacks.
    pub max_stacks: usize,
    /// Maximum LR state frames on one stack.
    pub max_stack_depth: usize,
    /// Maximum postfix CST records on one stack.
    pub max_buffer_records: usize,
    /// Maximum recovery operations.
    pub max_recovery_actions: usize,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            max_actions: 50_000_000,
            max_stacks: ParsePolicy::MAX_AMBIGUOUS_STACKS,
            max_stack_depth: 16_384,
            max_buffer_records: 10_000_000,
            max_recovery_actions: 1_000_000,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ParserTop {
    name: &'static str,
    state: u16,
    term: u16,
    strict: bool,
}

impl ParserTop {
    const fn new(rule: TopRule) -> Self {
        Self {
            name: rule.name,
            state: rule.state,
            term: rule.term,
            strict: false,
        }
    }

    const fn rule(self) -> TopRule {
        TopRule {
            name: self.name,
            state: self.state,
            term: self.term,
        }
    }

    fn set_rule(&mut self, rule: TopRule) {
        self.name = rule.name;
        self.state = rule.state;
        self.term = rule.term;
    }
}

#[derive(Clone)]
pub(crate) struct ParserCore {
    pub(crate) language: &'static Language,
    pub(crate) node_set: Arc<NodeSet>,
    max_node: u16,
    min_repeat_term: u16,
    specializer_filter: u64,
    single_specializer: Option<SpecializerSpec>,
    pub(crate) dialect: Dialect,
    top: ParserTop,
    pub(crate) buffer_length: TextSize,
    pub(crate) limits: ParseLimits,
    pub(crate) context: Option<&'static ContextTracker>,
    action_index: Arc<ActionIndex>,
    goto_index: Arc<GotoIndex>,
    dynamic_precedences: Arc<[i16]>,
    pub(crate) token_ascii_index: Arc<TokenAsciiIndex>,
    pub(crate) local_token_ascii_indices: Arc<[Option<TokenAsciiIndex>]>,
    tokenizer_start_index: Arc<TokenizerStartIndex>,
    strict_token_validators: &'static [StrictTokenValidator],
}

impl fmt::Debug for ParserCore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ParserCore")
            .field("top", &self.top.rule())
            .field("strict", &self.top.strict)
            .field("dialect", &self.dialect)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl ParserCore {
    pub(crate) fn state_slot(&self, state: u16, field: StateField) -> u32 {
        self.language.states[usize::from(state) * StateField::COUNT + field.index()]
    }

    pub(crate) fn state_flag(&self, state: u16, flag: StateFlag) -> bool {
        self.state_slot(state, StateField::Flags) & flag.mask() != 0
    }

    pub(crate) fn state_is_skipped(&self, state: u16) -> bool {
        self.action_index.state_is_skipped(state)
    }

    pub(crate) fn tokenizer_mask(&self, state: u16) -> u32 {
        self.action_index.tokenizer_mask(state)
    }

    pub(crate) fn eof_term(&self) -> u16 {
        self.max_node()
            .checked_add(1)
            .expect("generated node ids leave room for EOF")
    }

    pub(crate) fn max_node(&self) -> u16 {
        self.max_node
    }

    pub(crate) fn min_repeat_term(&self) -> u16 {
        self.min_repeat_term
    }

    pub(crate) fn dynamic_precedence(&self, term: u16) -> i32 {
        self.dynamic_precedences
            .get(usize::from(term))
            .copied()
            .map_or(0, i32::from)
    }

    pub(crate) fn has_specializer(&self, term: u16) -> bool {
        let bit = 1_u64 << (term & 63);
        self.specializer_filter & bit != 0
    }

    pub(crate) fn specialize(
        &self,
        term: u16,
        value: &str,
        stack: &Stack,
    ) -> Option<SpecializedToken> {
        if let Some(specializer) = self.single_specializer {
            if specializer.term != term {
                return None;
            }
            let result = (specializer.get)(value, stack)?;
            return self.dialect.allows(result.term).then_some(result);
        }
        self.language
            .specializers
            .iter()
            .filter(|specializer| specializer.term == term)
            .find_map(|specializer| {
                let result = (specializer.get)(value, stack)?;
                self.dialect.allows(result.term).then_some(result)
            })
    }

    fn strict_token_validators(
        &self,
        term: u16,
    ) -> impl Iterator<Item = StrictTokenValidator> + '_ {
        self.strict_token_validators
            .iter()
            .copied()
            .filter(move |validator| validator.term == term)
    }

    pub(crate) fn get_goto(&self, state: u16, term: u16, loose: bool) -> Option<u16> {
        self.goto_index.get(state, term, loose)
    }

    pub(crate) fn has_action(&self, state: u16, terminal: u16) -> Action {
        self.action_index.first_action(state, terminal)
    }

    pub(crate) fn default_reduce(&self, state: u16) -> Action {
        self.action_index.default_reduce(state)
    }

    pub(crate) fn all_actions<T>(
        &self,
        state: u16,
        mut visit: impl FnMut(Action) -> Option<T>,
    ) -> Option<T> {
        let default = self.default_reduce(state);
        if !default.is_none()
            && let Some(result) = visit(default)
        {
            return Some(result);
        }
        let data = self.language.state_data;
        let mut index = self.state_slot(state, StateField::Actions) as usize;
        // Default reductions neither consume input nor split the stack. Run
        // the complete chain before returning to the parse scheduler.
        loop {
            if data[index] == SequenceCode::End.raw() {
                if data[index + 1] == SequenceCode::Next.raw() {
                    index = pair(data, index + 2) as usize;
                } else {
                    return None;
                }
            }
            if let Some(result) = visit(Action::from_raw(pair(data, index + 1))) {
                return Some(result);
            }
            index += 3;
        }
    }

    pub(crate) fn valid_action(&self, state: u16, action: Action) -> bool {
        self.all_actions(state, |candidate| (candidate == action).then_some(()))
            .is_some()
    }

    pub(crate) fn next_states(&self, state: u16) -> Vec<(u16, u16)> {
        let data = self.language.state_data;
        let mut result = Vec::new();
        let mut index = self.state_slot(state, StateField::Actions) as usize;
        loop {
            if data[index] == SequenceCode::End.raw() {
                if data[index + 1] == SequenceCode::Next.raw() {
                    index = pair(data, index + 2) as usize;
                } else {
                    break;
                }
            }
            let action = Action::from_raw(pair(data, index + 1));
            if !action.is_reduce() {
                let term = data[index];
                let target = action.value();
                if !result.iter().any(|(_, existing)| *existing == target) {
                    result.push((term, target));
                }
            }
            index += 3;
        }
        result
    }

    pub(crate) fn name(&self, term: u16) -> &str {
        if let Some(node_type) = self.node_set.get(term)
            && !node_type.name().is_empty()
        {
            return node_type.name();
        }
        self.language
            .term_names
            .iter()
            .find_map(|(candidate, name)| (*candidate == term).then_some(*name))
            .unwrap_or("?")
    }
}

/// Configurable parser over one generated [`Language`].
#[derive(Clone)]
pub struct LRParser {
    core: Arc<ParserCore>,
    wrappers: Arc<[ParseWrapper]>,
    custom_create_parse: Option<CreateParse>,
}

type CreateParse = fn(&LRParser, ParseRequest) -> Result<Box<dyn PartialParse>, ParseError>;

impl fmt::Debug for LRParser {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LRParser")
            .field("core", &self.core)
            .field("wrapper_count", &self.wrappers.len())
            .field("has_custom_parse", &self.custom_create_parse.is_some())
            .finish()
    }
}

impl LRParser {
    /// Construct the default parser for generated static language data.
    ///
    /// This is the static-table ABI used by generated language crates.
    /// Regular consumers should obtain a parser from the language crate.
    ///
    /// # Panics
    ///
    /// Panics when the generated static tables violate their contract.
    #[doc(hidden)]
    #[must_use]
    pub fn from_language(language: &'static Language) -> Self {
        Self::try_from_language(language)
            .expect("generated Rezel language must contain valid static tables")
    }

    /// Validate and construct a default parser from static language data.
    ///
    /// # Errors
    ///
    /// Returns a configuration error for malformed generated data.
    #[doc(hidden)]
    pub fn try_from_language(language: &'static Language) -> Result<Self, ParseError> {
        validate_language(language)?;
        let node_set = Arc::clone((language.node_set)());
        validate_node_set(language, &node_set)?;
        let max_node = u16::try_from(node_set.types().len() - 1)
            .expect("node-set length was validated at build time");
        let specializer_filter = language
            .specializers
            .iter()
            .fold(0_u64, |filter, specializer| {
                filter | 1_u64 << (specializer.term & 63)
            });
        let single_specializer = match language.specializers {
            [specializer] => Some(*specializer),
            _ => None,
        };
        let top = *language.top_rules.first().ok_or_else(|| {
            ParseError::new(
                ParseErrorKind::Configuration,
                None,
                "language has no top rule",
            )
        })?;
        let dialect = Dialect::from_specs(None, language.dialects, language.max_term)?;
        let action_index = Arc::new(
            ActionIndex::build(language.states, language.state_data)
                .map_err(configuration_error)?,
        );
        let goto_index = Arc::new(GotoIndex::build(language.goto).map_err(configuration_error)?);
        let dynamic_precedences = build_dynamic_precedences(language)?;
        let token_ascii_index = Arc::new(TokenAsciiIndex::build(language.token_table));
        let local_token_ascii_indices = language
            .tokenizers
            .iter()
            .map(|tokenizer| match tokenizer {
                Tokenizer::Local(group) => Some(TokenAsciiIndex::build(group.table)),
                Tokenizer::Group(_) | Tokenizer::External(_) => None,
            })
            .collect::<Vec<_>>()
            .into();
        let tokenizer_start_index = Arc::new(TokenizerStartIndex::build(language.tokenizers));
        let core = ParserCore {
            language,
            node_set,
            max_node,
            min_repeat_term: language.min_repeat_term,
            specializer_filter,
            single_specializer,
            dialect,
            top: ParserTop::new(top),
            buffer_length: DEFAULT_PARSE_BUFFER_LENGTH,
            limits: ParseLimits::default(),
            context: language.context,
            action_index,
            goto_index,
            dynamic_precedences,
            token_ascii_index,
            local_token_ascii_indices,
            tokenizer_start_index,
            strict_token_validators: &[],
        };
        Ok(Self {
            core: Arc::new(core),
            wrappers: Arc::from([]),
            custom_create_parse: None,
        })
    }

    fn with_core(mut self, configure: impl FnOnce(&mut ParserCore)) -> Self {
        let mut core = (*self.core).clone();
        configure(&mut core);
        self.core = Arc::new(core);
        self
    }

    /// Parse a complete UTF-8 source string.
    ///
    /// # Errors
    ///
    /// Returns the first fatal syntax, configuration, input, or budget error.
    pub fn parse(&self, source: &str) -> Result<Tree, ParseError> {
        Parser::parse(self, source)
    }

    /// Parse one complete owned input.
    ///
    /// # Errors
    ///
    /// Returns the first fatal syntax, configuration, input, or budget error.
    pub fn parse_input(&self, input: Arc<dyn rezel_common::Input>) -> Result<Tree, ParseError> {
        Parser::parse_input(self, input)
    }

    /// Start parsing one owned input and return its resumable state.
    ///
    /// # Errors
    ///
    /// Returns an input or language-specific setup error. Parsing errors after
    /// setup are reported by [`PartialParse::advance`].
    pub fn start_parse(
        &self,
        input: Arc<dyn rezel_common::Input>,
    ) -> Result<Box<dyn PartialParse>, ParseError> {
        Parser::start_parse(self, input)
    }

    /// Return a parser that rejects syntax errors instead of recovering.
    #[must_use]
    pub fn with_strict(self, strict: bool) -> Self {
        self.with_core(|core| core.top.strict = strict)
    }

    /// Whether this parser rejects syntax errors instead of recovering.
    #[must_use]
    pub fn is_strict(&self) -> bool {
        self.core.top.strict
    }

    /// Return a parser with strict-only validators for base token spellings.
    ///
    /// Each validator runs only when its token actually contributes a parser
    /// action. Validators are selected by the base term, even when a
    /// specializer replaces it with a keyword or another contextual term.
    #[must_use]
    pub fn with_strict_token_validators(self, validators: &'static [StrictTokenValidator]) -> Self {
        self.with_core(|core| core.strict_token_validators = validators)
    }

    /// Return a parser using one named `@top`.
    ///
    /// # Errors
    ///
    /// Returns a configuration error when the top rule is unknown.
    pub fn with_top(self, name: &str) -> Result<Self, ParseError> {
        let top = self
            .core
            .language
            .top_rules
            .iter()
            .copied()
            .find(|top| top.name == name)
            .ok_or_else(|| {
                ParseError::new(
                    ParseErrorKind::Configuration,
                    None,
                    format!("unknown top rule {name:?}"),
                )
            })?;
        Ok(self.with_core(|core| core.top.set_rule(top)))
    }

    /// Return a parser with the space-separated dialect set enabled.
    ///
    /// # Errors
    ///
    /// Returns a configuration error when a dialect name is unknown.
    pub fn with_dialect(self, dialects: &str) -> Result<Self, ParseError> {
        let dialect = Dialect::from_specs(
            Some(dialects),
            self.core.language.dialects,
            self.core.language.max_term,
        )?;
        Ok(self.with_core(|core| core.dialect = dialect))
    }

    /// Return a parser using the given context tracker.
    #[must_use]
    pub fn with_context_tracker(self, tracker: &'static ContextTracker) -> Self {
        self.with_core(|core| core.context = Some(tracker))
    }

    /// Return a parser with one additional non-incremental parse wrapper.
    #[must_use]
    pub fn with_wrapper(mut self, wrapper: ParseWrapper) -> Self {
        let mut wrappers = self.wrappers.to_vec();
        wrappers.push(wrapper);
        self.wrappers = wrappers.into();
        self
    }

    /// Return a parser with a different packed tree-buffer source limit.
    #[must_use]
    pub fn with_buffer_length(self, length: TextSize) -> Self {
        self.with_core(|core| core.buffer_length = length)
    }

    /// Return a parser with different runtime resource ceilings.
    ///
    /// # Panics
    ///
    /// Panics when the CST record limit cannot be represented by the compact
    /// 32-bit postfix buffer.
    #[must_use]
    pub fn with_limits(self, limits: ParseLimits) -> Self {
        assert!(
            limits.max_buffer_records <= u32::MAX as usize / 4,
            "CST record limit exceeds the 32-bit postfix representation"
        );
        self.with_core(|core| core.limits = limits)
    }

    /// Use a language-owned parse constructor.
    ///
    /// The function may prepare a lexical input or wrap the partial parse for
    /// strict validation. It must call [`Self::create_lr_parse`] to enter the
    /// LR engine after any request preparation.
    #[doc(hidden)]
    #[must_use]
    pub fn with_create_parse(mut self, create_parse: CreateParse) -> Self {
        self.custom_create_parse = Some(create_parse);
        self
    }

    /// Create the ordinary LR partial parse after language input preparation.
    ///
    /// # Errors
    ///
    /// Returns an input error when the request is invalid.
    #[doc(hidden)]
    pub fn create_lr_parse(
        &self,
        request: ParseRequest,
    ) -> Result<Box<dyn PartialParse>, ParseError> {
        let request = request.into_validated()?;
        let mut parse: Box<dyn PartialParse> =
            Box::new(Parse::new(Arc::clone(&self.core), request.clone()));
        for wrapper in &*self.wrappers {
            parse = wrapper(parse, request.clone());
        }
        Ok(parse)
    }

    /// Whether this parser has mixed-parse or other wrappers.
    #[must_use]
    pub fn has_wrappers(&self) -> bool {
        !self.wrappers.is_empty()
    }

    /// Root node type selected by this parser.
    ///
    /// # Panics
    ///
    /// Panics only if immutable generated data somehow changes after the
    /// parser was validated.
    #[must_use]
    pub fn top_node(&self) -> &NodeType {
        self.core
            .node_set
            .get(self.core.top.term)
            .expect("generated top term was validated")
    }

    /// Name associated with a term.
    #[must_use]
    pub fn term_name(&self, term: u16) -> &str {
        self.core.name(term)
    }

    /// Runtime node set.
    #[must_use]
    pub fn node_set(&self) -> &Arc<NodeSet> {
        &self.core.node_set
    }
}

impl Parser for LRParser {
    fn create_parse(&self, request: ParseRequest) -> Result<Box<dyn PartialParse>, ParseError> {
        if let Some(create_parse) = self.custom_create_parse {
            return create_parse(self, request);
        }
        self.create_lr_parse(request)
    }
}

fn validate_language(language: &Language) -> Result<(), ParseError> {
    if language.states.is_empty() || !language.states.len().is_multiple_of(StateField::COUNT) {
        return Err(configuration_error(
            "state table must contain six words per state",
        ));
    }
    if language.state_data.len() < 2 || language.state_data[0] != SequenceCode::End.raw() {
        return Err(configuration_error(
            "state data must start with the empty action sequence",
        ));
    }
    if language.goto.is_empty() {
        return Err(configuration_error("goto table cannot be empty"));
    }
    if language.tokenizers.len() > u32::BITS as usize {
        return Err(configuration_error(
            "tokenizer masks support at most 32 tokenizers",
        ));
    }
    if language.min_repeat_term == 0 {
        return Err(configuration_error(
            "the error node must precede repeat terms",
        ));
    }
    let state_count = language.states.len() / StateField::COUNT;
    for top in language.top_rules {
        if usize::from(top.state) >= state_count {
            return Err(configuration_error("top rule refers to an unknown state"));
        }
    }
    for state in 0..state_count {
        for field in [StateField::Actions, StateField::Skip] {
            let offset = language.states[state * StateField::COUNT + field.index()] as usize;
            if offset >= language.state_data.len() {
                return Err(configuration_error(
                    "state action offset is outside state data",
                ));
            }
        }
        let mask = language.states[state * StateField::COUNT + StateField::TokenizerMask.index()];
        let valid_mask = if language.tokenizers.len() == u32::BITS as usize {
            u32::MAX
        } else {
            (1_u32 << language.tokenizers.len()) - 1
        };
        if mask & !valid_mask != 0 {
            return Err(configuration_error(
                "state tokenizer mask refers to an unknown tokenizer",
            ));
        }
    }
    validate_goto_table(language.goto, state_count)?;
    Ok(())
}

fn build_dynamic_precedences(language: &Language) -> Result<Arc<[i16]>, ParseError> {
    if language.dynamic_precedences.is_empty() {
        return Ok(Arc::from([]));
    }

    let mut values = vec![None; usize::from(language.max_term) + 1];
    for entry in language.dynamic_precedences {
        let Some(slot) = values.get_mut(usize::from(entry.term)) else {
            return Err(configuration_error(
                "dynamic precedence refers to an unknown term",
            ));
        };
        if slot.replace(entry.value).is_some() {
            return Err(configuration_error(
                "dynamic precedence contains a duplicate term",
            ));
        }
    }
    Ok(values
        .into_iter()
        .map(|value| value.unwrap_or(0))
        .collect::<Vec<_>>()
        .into())
}

fn validate_goto_table(table: &[u16], state_count: usize) -> Result<(), ParseError> {
    let (positions, data_start) = decode_goto_header(table).map_err(configuration_error)?;
    for position in positions {
        let Some(mut position) = position else {
            continue;
        };
        if position < data_start {
            return Err(configuration_error("goto group points inside its header"));
        }
        loop {
            let Some(&group_tag) = table.get(position) else {
                return Err(configuration_error("goto group is truncated"));
            };
            let Some(&target) = table.get(position + 1) else {
                return Err(configuration_error("goto group target is truncated"));
            };
            if usize::from(target) >= state_count {
                return Err(configuration_error(
                    "goto group refers to an unknown target state",
                ));
            }
            position += 2;
            let (sources, end) =
                decode_goto_sources(table, position, group_tag).map_err(configuration_error)?;
            if sources
                .iter()
                .any(|source| usize::from(*source) >= state_count)
            {
                return Err(configuration_error(
                    "goto group refers to an unknown source state",
                ));
            }
            if group_tag & 1 != 0 {
                break;
            }
            position = end;
        }
    }
    Ok(())
}

fn validate_node_set(language: &Language, node_set: &NodeSet) -> Result<(), ParseError> {
    if node_set.types().is_empty() || !node_set.types()[0].is_error() {
        return Err(configuration_error("node term zero must be the error node"));
    }
    if usize::from(language.min_repeat_term) > node_set.types().len() {
        return Err(configuration_error(
            "first repeat term exceeds the node-set length",
        ));
    }
    if usize::from(language.max_term) < node_set.types().len() {
        return Err(configuration_error(
            "maximum term does not include every node type",
        ));
    }
    for top in language.top_rules {
        if node_set.get(top.term).is_none() {
            return Err(configuration_error(
                "top rule refers to an unknown node term",
            ));
        }
    }
    Ok(())
}

fn configuration_error(message: &'static str) -> ParseError {
    ParseError::new(ParseErrorKind::Configuration, None, message)
}

#[derive(Clone, Copy, Debug, Default)]
struct CachedToken {
    start: TextSize,
    value: u16,
    end: TextSize,
    mask: u32,
    context: u64,
}

#[derive(Clone, Copy, Debug)]
struct MainToken {
    start: TextSize,
    value: u16,
    end: TextSize,
}

#[derive(Clone, Copy, Debug)]
struct TokenAction {
    action: Action,
    token: u16,
    end: TextSize,
}

impl TokenAction {
    const EMPTY: Self = Self {
        action: Action::NONE,
        token: 0,
        end: TextSize::new(0),
    };
}

#[derive(Debug)]
struct TokenActions {
    first: TokenAction,
    rest: Vec<TokenAction>,
    length: usize,
}

impl TokenActions {
    fn new() -> Self {
        Self {
            first: TokenAction::EMPTY,
            rest: Vec::new(),
            length: 0,
        }
    }

    fn clear(&mut self) {
        self.length = 0;
        self.rest.clear();
    }

    const fn len(&self) -> usize {
        self.length
    }

    const fn is_empty(&self) -> bool {
        self.length == 0
    }

    fn first(&self) -> Option<&TokenAction> {
        (self.length != 0).then_some(&self.first)
    }

    fn get(&self, index: usize) -> Option<&TokenAction> {
        if index == 0 {
            return self.first();
        }
        if index >= self.length {
            return None;
        }
        self.rest.get(index - 1)
    }

    fn put(&mut self, candidate: TokenAction) {
        if self.length == 0 {
            self.first = candidate;
            self.length = 1;
            return;
        }
        if self.first.action == candidate.action
            || self
                .rest
                .iter()
                .any(|existing| existing.action == candidate.action)
        {
            return;
        }
        self.rest.push(candidate);
        self.length += 1;
    }
}

#[derive(Debug)]
struct TokenCache {
    tokens: Vec<CachedToken>,
    main_token: Option<MainToken>,
    actions: TokenActions,
}

impl TokenCache {
    fn new(tokenizer_count: usize) -> Self {
        Self {
            tokens: vec![CachedToken::default(); tokenizer_count],
            main_token: None,
            actions: TokenActions::new(),
        }
    }

    fn get_actions(&mut self, stack: &Stack, stream: &mut InputStream) -> Result<(), ParseError> {
        let core = stack.core();
        let mask = core.tokenizer_mask(stack.state());
        let context = stack.context_hash();
        let token_start = stream.clip_position(stack.position());
        self.actions.clear();
        let mut main = None;
        let mut remaining = if core.tokenizer_start_index.has_filtered(mask) {
            stream.reset(token_start);
            core.tokenizer_start_index.filter(mask, stream.next())
        } else {
            mask
        };
        while remaining != 0 {
            let index = remaining.trailing_zeros() as usize;
            remaining &= remaining - 1;
            let tokenizer = core.language.tokenizers[index];
            let flags = tokenizer.flags();
            if main.is_some() && !flags.fallback {
                continue;
            }
            let stale = {
                let token = &self.tokens[index];
                flags.contextual
                    || token.start != token_start
                    || token.mask != mask
                    || token.context != context
            };
            if stale {
                self.tokens[index] = update_cached_token(
                    index,
                    tokenizer,
                    stack,
                    stream,
                    token_start,
                    mask,
                    context,
                )?;
            }
            let (token, extended) = specialize_cached_token(self.tokens[index], stack, stream);
            if token.value != ReservedTerm::Error.raw() {
                let before = self.actions.len();
                if let Some(extended) = extended {
                    add_actions(stack, extended, token.end, &mut self.actions);
                }
                add_actions(stack, token.value, token.end, &mut self.actions);
                if self.actions.len() > before && core.top.strict {
                    validate_strict_token(core, self.tokens[index], stream)?;
                }
                if !flags.extend {
                    main = Some(MainToken {
                        start: token.start,
                        value: token.value,
                        end: token.end,
                    });
                    if self.actions.len() > before {
                        break;
                    }
                }
            }
        }
        if main.is_none() && token_start == stream.end() {
            let eof = MainToken {
                start: token_start,
                value: core.eof_term(),
                end: token_start,
            };
            add_actions(stack, eof.value, eof.end, &mut self.actions);
            main = Some(eof);
        }
        self.main_token = main;
        Ok(())
    }

    fn main_token(&self, stack: &Stack, stream: &InputStream) -> MainToken {
        self.main_token.unwrap_or_else(|| {
            let token_start = stream.clip_position(stack.position());
            MainToken {
                start: token_start,
                value: if token_start == stream.end() {
                    stack.core().eof_term()
                } else {
                    ReservedTerm::Error.raw()
                },
                end: stream
                    .next_position_from(token_start)
                    .unwrap_or_else(|| stream.end())
                    .max(token_start),
            }
        })
    }
}

fn update_cached_token(
    tokenizer_index: usize,
    tokenizer: Tokenizer,
    stack: &Stack,
    stream: &mut InputStream,
    start: TextSize,
    mask: u32,
    context: u64,
) -> Result<CachedToken, ParseError> {
    stream.reset(start);
    tokenizer.token(tokenizer_index, stream, stack)?;
    let accepted = accepted_or_declined(stream, start);
    Ok(CachedToken {
        start,
        value: accepted.value,
        end: accepted.end,
        mask,
        context,
    })
}

fn validate_strict_token(
    core: &ParserCore,
    token: CachedToken,
    stream: &InputStream,
) -> Result<(), ParseError> {
    let mut validators = core.strict_token_validators(token.value).peekable();
    if validators.peek().is_none() {
        return Ok(());
    }
    let spelling = stream
        .read_scalar_at_boundaries(token.start, token.end)
        .ok_or_else(|| {
            ParseError::new(
                ParseErrorKind::Input,
                Some(token.start),
                "strict token boundaries do not select scalar input",
            )
        })?;
    for validator in validators {
        if let Err(error) = (validator.validate)(&spelling) {
            return Err(ParseError::new(
                ParseErrorKind::Syntax,
                token.start.checked_add(error.offset),
                error.message,
            ));
        }
    }
    Ok(())
}

fn specialize_cached_token(
    mut token: CachedToken,
    stack: &Stack,
    stream: &InputStream,
) -> (CachedToken, Option<u16>) {
    let mut extended = None;
    if token.value != ReservedTerm::Error.raw() {
        let core = stack.core();
        let base_term = token.value;
        if core.has_specializer(base_term)
            && let Some(lexeme) = stream.read_scalar_at_boundaries(token.start, token.end)
            && let Some(result) = core.specialize(base_term, &lexeme, stack)
        {
            match result.kind {
                Specialize::Replace => {
                    token.value = result.term;
                }
                Specialize::Extend => {
                    extended = Some(result.term);
                }
            }
        }
    }
    (token, extended)
}

fn accepted_or_declined(stream: &InputStream, start: TextSize) -> AcceptedToken {
    stream.accepted().unwrap_or(AcceptedToken {
        value: ReservedTerm::Error.raw(),
        // A declined tokenizer is ignored by `TokenCache`. Its endpoint is
        // therefore unobservable; defer decoding the next code point until
        // the parser actually needs one fallback Error token at this position.
        end: start,
    })
}

fn add_actions(stack: &Stack, token: u16, end: TextSize, actions: &mut TokenActions) {
    let core = stack.core();
    let [action_row, skip_row] = core.action_index.state_rows(stack.state());
    let fallback = core.action_index.visit_row(action_row, token, |action| {
        put_action(actions, action, token, end);
    });
    if actions.is_empty()
        && let Some(fallback) = fallback
    {
        put_action(actions, fallback, token, end);
    }
    if !core.action_index.skip_may_match(token) {
        return;
    }
    let fallback = core.action_index.visit_row(skip_row, token, |action| {
        put_action(actions, action, token, end);
    });
    if actions.is_empty()
        && let Some(fallback) = fallback
    {
        put_action(actions, fallback, token, end);
    }
}

fn put_action(actions: &mut TokenActions, action: Action, token: u16, end: TextSize) {
    actions.put(TokenAction { action, token, end });
}

/// Fixed policy that bounds parse-wide recovery and LR/GLR exploration.
struct ParsePolicy;

impl ParsePolicy {
    const RECOVERY_DISTANCE: usize = 5;
    const MAX_REMAINING_STACKS_PER_STEP: usize = 3;
    const MIN_BRANCH_BUFFER_LENGTH: usize = 500;
    const FORCE_REDUCE_LIMIT: usize = 10;

    // Upstream counts three numeric cells per stack frame. Rezel stores one
    // typed Frame, so these retain the same logical depths without `* 3`.
    const CUT_DEPTH: usize = 2_800;
    const CUT_TO_DEPTH: usize = 2_000;

    const MAX_LEFT_ASSOCIATIVE_REDUCTIONS: usize = 300;
    const MAX_AMBIGUOUS_STACKS: usize = 12;
}

#[derive(Debug, Default)]
pub(crate) struct LargeReductionTracker {
    last_start: Option<TextSize>,
    last_size: TextSize,
    count: usize,
}

impl LargeReductionTracker {
    pub(crate) fn observe(&mut self, start: TextSize, size: TextSize) {
        if self.last_start == Some(start) {
            self.count += 1;
            self.last_size = size;
        } else if self.last_size < size {
            self.last_start = Some(start);
            self.last_size = size;
            self.count = 1;
        }
    }

    fn should_force(&self, stack_count: usize) -> bool {
        stack_count == 1 && self.count > ParsePolicy::MAX_LEFT_ASSOCIATIVE_REDUCTIONS
    }

    fn start(&self) -> Option<TextSize> {
        self.last_start
    }

    fn reset_after_force(&mut self) {
        self.last_size = TextSize::from(0);
        self.count = 0;
    }
}

struct Parse {
    core: Arc<ParserCore>,
    request: ParseRequest,
    stacks: Vec<Stack>,
    stack_scratch: Vec<Stack>,
    recovering: usize,
    stream: InputStream,
    tokens: TokenCache,
    stopped_at: Option<TextSize>,
    min_stack_position: TextSize,
    actions: usize,
    recovery_actions: usize,
    large_reductions: LargeReductionTracker,
}

impl Parse {
    fn new(core: Arc<ParserCore>, request: ParseRequest) -> Self {
        let ranges: Arc<[_]> = request.selected_ranges().to_vec().into();
        let stream = InputStream::new(Arc::clone(request.lexical_input()), Arc::clone(&ranges));
        let start = stream.position();
        let stack = Stack::start(Arc::clone(&core), core.top.state, start);
        let tokenizer_count = core.language.tokenizers.len();
        Self {
            core,
            request,
            stacks: vec![stack],
            stack_scratch: Vec::new(),
            recovering: 0,
            stream,
            tokens: TokenCache::new(tokenizer_count),
            stopped_at: None,
            min_stack_position: start,
            actions: 0,
            recovery_actions: 0,
            large_reductions: LargeReductionTracker::default(),
        }
    }

    fn step(&mut self) -> Result<Option<Tree>, ParseError> {
        self.force_large_left_associative_reduction()?;
        let position = self.min_stack_position;
        let mut pending = std::mem::take(&mut self.stacks);
        let mut advanced = std::mem::take(&mut self.stack_scratch);
        advanced.clear();
        let mut stopped = Vec::new();
        while !pending.is_empty() {
            let mut stack = if pending.len() == 1 {
                pending.pop().expect("pending stack exists")
            } else {
                pending.remove(0)
            };
            loop {
                if stack.position() > position {
                    advanced.push(stack);
                    break;
                }
                if self.advance_stack(&mut stack, Some(&mut advanced), Some(&mut pending))? {
                    continue;
                }
                let token = self.tokens.main_token(&stack, &self.stream);
                stopped.push((stack, token.value, token.end));
                break;
            }
        }

        if advanced.is_empty() {
            if let Some(index) = find_finished(&stopped, self.stream.end(), self.stopped_at) {
                let stack = stopped.swap_remove(index).0;
                return Ok(Some(self.stack_to_tree(stack)));
            }
            if self.core.top.strict {
                return Err(ParseError::new(
                    ParseErrorKind::Syntax,
                    Some(position),
                    "no parse",
                ));
            }
            if self.recovering == 0 {
                self.recovering = ParsePolicy::RECOVERY_DISTANCE;
            }
        }

        if self.recovering != 0
            && !stopped.is_empty()
            && let Some(stack) = self.run_recovery(stopped, &mut advanced)?
        {
            let stack = stack.force_all(
                &mut self.stream,
                &mut self.actions,
                &mut self.large_reductions,
            )?;
            return Ok(Some(self.stack_to_tree(stack)));
        }

        if self.recovering != 0 {
            let maximum = if self.recovering == 1 {
                1
            } else {
                self.recovering * ParsePolicy::MAX_REMAINING_STACKS_PER_STEP
            };
            prune_by_score(&mut advanced, maximum);
            if advanced
                .iter()
                .any(|stack| stack.reduce_position() > position)
            {
                self.recovering -= 1;
            }
        } else if advanced.len() > 1 {
            prune_equivalent(&mut advanced);
            prune_by_score(&mut advanced, self.core.limits.max_stacks);
        }

        if advanced.is_empty() {
            return Err(ParseError::new(
                ParseErrorKind::ResourceLimit,
                Some(position),
                "parser recovery produced no live stack",
            ));
        }
        self.min_stack_position = advanced
            .iter()
            .map(Stack::position)
            .min()
            .expect("advanced stacks are nonempty");
        self.stack_scratch = pending;
        self.stacks = advanced;
        Ok(None)
    }

    fn force_large_left_associative_reduction(&mut self) -> Result<(), ParseError> {
        if !self.large_reductions.should_force(self.stacks.len()) {
            return Ok(());
        }
        let start = self
            .large_reductions
            .start()
            .expect("a large reduction count always records its start");
        let mut stack = self
            .stacks
            .pop()
            .expect("the large-reduction guard requires exactly one stack");
        loop {
            if !stack.force_reduce(
                &mut self.stream,
                &mut self.actions,
                &mut self.large_reductions,
            )? {
                break;
            }
            let Some(frame_start) = stack.top_frame_start() else {
                break;
            };
            if frame_start < start {
                break;
            }
        }
        self.large_reductions.reset_after_force();
        self.stacks.push(stack);
        Ok(())
    }

    fn advance_stack(
        &mut self,
        stack: &mut Stack,
        mut advanced: Option<&mut Vec<Stack>>,
        mut split: Option<&mut Vec<Stack>>,
    ) -> Result<bool, ParseError> {
        let start = stack.position();
        if self.stopped_at.is_some_and(|stop| start > stop) {
            self.tokens.main_token = None;
            return stack.force_reduce(
                &mut self.stream,
                &mut self.actions,
                &mut self.large_reductions,
            );
        }
        // Keep the deterministic same-position action chain inside one call.
        loop {
            loop {
                let default_reduce = self.core.default_reduce(stack.state());
                if default_reduce.is_none() {
                    break;
                }
                self.bump_action(stack.position())?;
                stack.reduce(default_reduce, &mut self.stream, &mut self.large_reductions)?;
            }
            if stack.depth() >= ParsePolicy::CUT_DEPTH {
                while stack.depth() > ParsePolicy::CUT_TO_DEPTH
                    && stack.force_reduce(
                        &mut self.stream,
                        &mut self.actions,
                        &mut self.large_reductions,
                    )?
                {}
            }
            self.tokens.get_actions(stack, &mut self.stream)?;
            let token_start = self
                .tokens
                .main_token
                .map_or(stack.position(), |token| token.start);
            let action_count = self.tokens.actions.len();
            if action_count > 1 && split.is_some() {
                // Keep the reusable token-action buffer in place while
                // applying alternatives, as upstream Lezer does. Applying an
                // action mutates the stack and input stream, but not this cache.
                for index in 0..action_count {
                    let choice = *self
                        .tokens
                        .actions
                        .get(index)
                        .expect("token action index stays within the captured length");
                    let last = index + 1 == action_count;
                    if last {
                        self.bump_action(stack.position())?;
                        stack.apply(
                            choice.action,
                            choice.token,
                            token_start,
                            choice.end,
                            &mut self.stream,
                            &mut self.large_reductions,
                        )?;
                        return Ok(true);
                    }
                    let mut local = stack.split();
                    self.bump_action(local.position())?;
                    local.apply(
                        choice.action,
                        choice.token,
                        token_start,
                        choice.end,
                        &mut self.stream,
                        &mut self.large_reductions,
                    )?;
                    if local.position() > start {
                        advanced
                            .as_deref_mut()
                            .expect("split actions provide an advanced stack sink")
                            .push(local);
                    } else {
                        split
                            .as_deref_mut()
                            .expect("split actions provide a pending stack sink")
                            .push(local);
                    }
                }
                return Ok(false);
            }

            let Some(choice) = self.tokens.actions.first().copied() else {
                return Ok(false);
            };
            self.bump_action(stack.position())?;
            stack.apply(
                choice.action,
                choice.token,
                token_start,
                choice.end,
                &mut self.stream,
                &mut self.large_reductions,
            )?;
            if action_count != 1 || stack.position() > start {
                return Ok(true);
            }
        }
    }

    fn advance_fully(
        &mut self,
        stack: &mut Stack,
        advanced: &mut Vec<Stack>,
    ) -> Result<bool, ParseError> {
        let position = stack.position();
        loop {
            if !self.advance_stack(stack, None, None)? {
                return Ok(false);
            }
            if stack.position() > position {
                push_stack_dedup(stack.clone(), advanced);
                return Ok(true);
            }
        }
    }

    fn run_recovery(
        &mut self,
        stopped: Vec<(Stack, u16, TextSize)>,
        advanced: &mut Vec<Stack>,
    ) -> Result<Option<Stack>, ParseError> {
        let mut finished: Option<Stack> = None;
        let mut restarted = false;
        for (mut stack, mut token, mut token_end) in stopped {
            self.bump_recovery(stack.position())?;
            if stack.dead_end() {
                if restarted {
                    continue;
                }
                restarted = true;
                stack.restart()?;
                if self.advance_fully(&mut stack, advanced)? {
                    continue;
                }
            }

            let mut forced = stack.split();
            for _ in 0..ParsePolicy::FORCE_REDUCE_LIMIT {
                if !forced.force_reduce(
                    &mut self.stream,
                    &mut self.actions,
                    &mut self.large_reductions,
                )? {
                    break;
                }
                if self.advance_fully(&mut forced, advanced)? {
                    break;
                }
            }

            for mut inserted in stack.recover_by_insert(token, &mut self.stream)? {
                self.bump_recovery(inserted.position())?;
                let _ = self.advance_fully(&mut inserted, advanced)?;
            }

            if self.stream.end() > stack.position() {
                if token_end == stack.position() {
                    token_end = self
                        .stream
                        .next_position_from(stack.position())
                        .unwrap_or_else(|| self.stream.end());
                    token = ReservedTerm::Error.raw();
                }
                stack.recover_by_delete(token, token_end)?;
                push_stack_dedup(stack, advanced);
            } else if finished
                .as_ref()
                .is_none_or(|current| current.score() < forced.score())
            {
                finished = Some(forced);
            }
        }
        Ok(finished)
    }

    fn stack_to_tree(&self, stack: Stack) -> Tree {
        let start = self
            .request
            .selected_ranges()
            .first()
            .map_or(TextSize::from(0), |range| range.start());
        let length = stack.position() - start;
        let mut build = TreeBuild::new(stack, Arc::clone(&self.core.node_set), self.core.top.term);
        build.start = start;
        build.length = Some(length);
        build.max_buffer_length = self.core.buffer_length;
        build.min_repeat_type = usize::from(self.core.min_repeat_term());
        Tree::build(&build)
    }

    fn bump_action(&mut self, position: TextSize) -> Result<(), ParseError> {
        self.actions += 1;
        if self.actions > self.core.limits.max_actions {
            return Err(ParseError::new(
                ParseErrorKind::ResourceLimit,
                Some(position),
                "parser action budget exhausted",
            ));
        }
        Ok(())
    }

    fn bump_recovery(&mut self, position: TextSize) -> Result<(), ParseError> {
        self.recovery_actions += 1;
        if self.recovery_actions > self.core.limits.max_recovery_actions {
            return Err(ParseError::new(
                ParseErrorKind::ResourceLimit,
                Some(position),
                "parser recovery budget exhausted",
            ));
        }
        Ok(())
    }
}

impl PartialParse for Parse {
    fn advance(&mut self) -> Result<Option<Tree>, ParseError> {
        self.step()
    }

    fn parsed_position(&self) -> TextSize {
        self.min_stack_position
    }

    fn stop_at(&mut self, position: TextSize) -> Result<(), ParseError> {
        if self.stopped_at.is_some_and(|current| current < position) {
            return Err(ParseError::new(
                ParseErrorKind::Input,
                Some(position),
                "cannot move a parser stop point forward",
            ));
        }
        if position > self.stream.end() {
            return Err(ParseError::new(
                ParseErrorKind::Input,
                Some(position),
                "parser stop point exceeds selected input",
            ));
        }
        self.stopped_at = Some(position);
        Ok(())
    }

    fn stopped_at(&self) -> Option<TextSize> {
        self.stopped_at
    }
}

fn find_finished(
    stacks: &[(Stack, u16, TextSize)],
    end: TextSize,
    stopped_at: Option<TextSize>,
) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (index, (stack, _, _)) in stacks.iter().enumerate() {
        let at_end =
            stack.position() == end || stopped_at.is_some_and(|stop| stack.position() > stop);
        if at_end
            && stack.core().state_flag(stack.state(), StateFlag::Accepting)
            && best.is_none_or(|current| stacks[current].0.score() < stack.score())
        {
            best = Some(index);
        }
    }
    best
}

fn push_stack_dedup(stack: Stack, stacks: &mut Vec<Stack>) {
    if let Some(index) = stacks
        .iter()
        .position(|other| other.position() == stack.position() && other.same_state(&stack))
    {
        if stacks[index].score() < stack.score() {
            stacks[index] = stack;
        }
    } else {
        stacks.push(stack);
    }
}

fn prune_equivalent(stacks: &mut Vec<Stack>) {
    let mut left = 0;
    while left + 1 < stacks.len() {
        let mut right = left + 1;
        let mut removed_left = false;
        while right < stacks.len() {
            let same = stacks[left].same_state(&stacks[right]);
            let long_running = stacks[left].buffer_len() > ParsePolicy::MIN_BRANCH_BUFFER_LENGTH
                && stacks[right].buffer_len() > ParsePolicy::MIN_BRANCH_BUFFER_LENGTH;
            if same || long_running {
                let left_rank = (stacks[left].score(), stacks[left].buffer_len());
                let right_rank = (stacks[right].score(), stacks[right].buffer_len());
                if left_rank > right_rank {
                    stacks.remove(right);
                } else {
                    stacks.remove(left);
                    removed_left = true;
                    break;
                }
            } else {
                right += 1;
            }
        }
        if !removed_left {
            left += 1;
        }
    }
}

fn prune_by_score(stacks: &mut Vec<Stack>, maximum: usize) {
    if stacks.len() <= maximum {
        return;
    }
    stacks.sort_by_key(|stack| Reverse(stack.score()));
    stacks.truncate(maximum);
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use super::*;
    use crate::token::{ExternalTokenizer, ExternalTokenizerStart, TokenizerFlags};
    use rezel_common::{
        CodePoint, Input, LexicalInput, NodeFlags, StringInput, TextRange, Utf8Input,
    };

    fn reduction_chain_node_set() -> &'static Arc<NodeSet> {
        static NODE_SET: OnceLock<Arc<NodeSet>> = OnceLock::new();
        NODE_SET.get_or_init(|| {
            Arc::new(NodeSet::new(vec![
                NodeType::new(0, "⚠", NodeFlags::ERROR),
                NodeType::new(1, "First", NodeFlags::TOP),
                NodeType::new(2, "Second", NodeFlags::ANONYMOUS),
            ]))
        })
    }

    static REDUCTION_CHAIN_STATES: [u32; StateField::COUNT * 3] = [
        0,
        0,
        0,
        0,
        Action::reduce(1, 0, false, false).raw(),
        0,
        0,
        0,
        0,
        0,
        Action::reduce(2, 0, false, false).raw(),
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    static REDUCTION_CHAIN_STATE_DATA: [u16; 2] =
        [SequenceCode::End.raw(), SequenceCode::Done.raw()];
    static REDUCTION_CHAIN_GOTO: [u16; 10] = [3, 1, 4, 7, 3, 1, 0, 3, 2, 1];
    static REDUCTION_CHAIN_TOKEN_TABLE: TokenTable = TokenTable::new(&[], &[], &[], &[]);
    static REDUCTION_CHAIN_TOP: [TopRule; 1] = [TopRule {
        name: "Chain",
        state: 0,
        term: 1,
    }];
    static REDUCTION_CHAIN_LANGUAGE: Language = Language {
        states: &REDUCTION_CHAIN_STATES,
        state_data: &REDUCTION_CHAIN_STATE_DATA,
        goto: &REDUCTION_CHAIN_GOTO,
        token_table: &REDUCTION_CHAIN_TOKEN_TABLE,
        tokenizers: &[],
        top_rules: &REDUCTION_CHAIN_TOP,
        max_term: 3,
        min_repeat_term: 3,
        token_precedence: 0,
        node_set: reduction_chain_node_set,
        context: None,
        dialects: &[],
        dynamic_precedences: &[],
        specializers: &[],
        term_names: &[],
    };

    fn scan_scheduler_token(input: &mut InputStream, _stack: &Stack) -> Result<(), ParseError> {
        if input.next() == Some(CodePoint::from(b'x')) {
            input.advance(1);
            input.accept_token(2)?;
        }
        Ok(())
    }

    static SCHEDULER_TOKENIZER: ExternalTokenizer = ExternalTokenizer::new(
        scan_scheduler_token,
        TokenizerFlags {
            contextual: false,
            fallback: false,
            extend: false,
        },
    )
    .with_start(ExternalTokenizerStart::NONE.with_ascii(b'x'));
    static SCHEDULER_TOKENIZERS: [Tokenizer; 1] = [Tokenizer::External(&SCHEDULER_TOKENIZER)];
    static SCHEDULER_STATES: [u32; StateField::COUNT * 3] =
        [0, 2, 0, 1, 0, 0, 0, 7, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0];
    static SCHEDULER_STATE_DATA: [u16; 12] = [
        SequenceCode::End.raw(),
        SequenceCode::Done.raw(),
        2,
        1,
        1,
        SequenceCode::End.raw(),
        SequenceCode::Done.raw(),
        2,
        2,
        0,
        SequenceCode::End.raw(),
        SequenceCode::Done.raw(),
    ];
    static SCHEDULER_GOTO: [u16; 6] = [3, 1, 4, 1, 1, 1];
    static SCHEDULER_TOKEN_TABLE: TokenTable = TokenTable::new(&[], &[], &[], &[]);
    static SCHEDULER_TOP: [TopRule; 1] = [TopRule {
        name: "Scheduler",
        state: 0,
        term: 1,
    }];
    static SCHEDULER_LANGUAGE: Language = Language {
        states: &SCHEDULER_STATES,
        state_data: &SCHEDULER_STATE_DATA,
        goto: &SCHEDULER_GOTO,
        token_table: &SCHEDULER_TOKEN_TABLE,
        tokenizers: &SCHEDULER_TOKENIZERS,
        top_rules: &SCHEDULER_TOP,
        max_term: 3,
        min_repeat_term: 3,
        token_precedence: 0,
        node_set: reduction_chain_node_set,
        context: None,
        dialects: &[],
        dynamic_precedences: &[],
        specializers: &[],
        term_names: &[],
    };

    fn specialize_scheduler_token(value: &str, stack: &Stack) -> Option<SpecializedToken> {
        (value == "x" && stack.state() == 0).then(|| SpecializedToken::new(3, Specialize::Replace))
    }

    fn specialize_scheduler_fallback(value: &str, _stack: &Stack) -> Option<SpecializedToken> {
        (value == "x").then(|| SpecializedToken::new(4, Specialize::Replace))
    }

    fn reject_scheduler_x(value: &str) -> Result<(), StrictTokenValidationError> {
        if value == "x" {
            return Err(StrictTokenValidationError::new(
                TextSize::from(0),
                "strict token rejected x",
            ));
        }
        Ok(())
    }

    static SCHEDULER_STRICT_VALIDATORS: [StrictTokenValidator; 1] =
        [StrictTokenValidator::new(2, reject_scheduler_x)];

    static STATE_SPECIALIZERS: [SpecializerSpec; 2] = [
        SpecializerSpec {
            term: 2,
            get: specialize_scheduler_token,
        },
        SpecializerSpec {
            term: 2,
            get: specialize_scheduler_fallback,
        },
    ];
    static STATE_DIALECT_TERMS: [u16; 1] = [3];
    static STATE_DIALECTS: [DialectSpec; 1] = [DialectSpec {
        name: "state",
        terms: &STATE_DIALECT_TERMS,
    }];
    static STATE_SPECIALIZER_LANGUAGE: Language = Language {
        states: &SCHEDULER_STATES,
        state_data: &SCHEDULER_STATE_DATA,
        goto: &SCHEDULER_GOTO,
        token_table: &SCHEDULER_TOKEN_TABLE,
        tokenizers: &SCHEDULER_TOKENIZERS,
        top_rules: &SCHEDULER_TOP,
        max_term: 4,
        min_repeat_term: 3,
        token_precedence: 0,
        node_set: reduction_chain_node_set,
        context: None,
        dialects: &STATE_DIALECTS,
        dynamic_precedences: &[],
        specializers: &STATE_SPECIALIZERS,
        term_names: &[],
    };

    fn start_context() -> ContextValue {
        ContextValue::new(())
    }

    #[allow(clippy::unnecessary_wraps)]
    fn shift_with_input(
        context: &ContextValue,
        _term: u16,
        _stack: &Stack,
        _input: &mut InputStream,
    ) -> Result<ContextValue, ParseError> {
        Ok(context.clone())
    }

    #[allow(clippy::unnecessary_wraps)]
    fn shift_without_input(
        context: &ContextValue,
        _term: u16,
        _stack: &Stack,
    ) -> Result<ContextValue, ParseError> {
        Ok(context.clone())
    }

    #[allow(clippy::unnecessary_wraps)]
    fn shift_context_only(
        context: &ContextValue,
        term: u16,
    ) -> Result<Option<ContextValue>, ParseError> {
        if term != 7 {
            return Ok(None);
        }
        let previous = context.downcast_ref::<bool>().copied().unwrap_or(false);
        if previous {
            Ok(None)
        } else {
            Ok(Some(ContextValue::new(true)))
        }
    }

    fn hash_context(_context: &ContextValue) -> u64 {
        0
    }

    #[test]
    fn token_actions_store_the_deterministic_choice_inline() {
        let first = TokenAction {
            action: Action::shift(1, false, false),
            token: 3,
            end: TextSize::new(5),
        };
        let second = TokenAction {
            action: Action::shift(2, false, false),
            token: 4,
            end: TextSize::new(7),
        };
        let mut actions = TokenActions::new();

        actions.put(first);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions.rest.capacity(), 0);
        assert_eq!(
            actions.first().map(|choice| choice.action),
            Some(first.action)
        );

        actions.put(TokenAction { token: 9, ..first });
        assert_eq!(actions.len(), 1);

        actions.put(second);
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions.get(0).map(|choice| choice.action),
            Some(first.action)
        );
        assert_eq!(
            actions.get(1).map(|choice| choice.action),
            Some(second.action)
        );
        actions.put(TokenAction { token: 9, ..second });
        assert_eq!(actions.len(), 2);

        actions.clear();
        assert!(actions.is_empty());
        assert!(actions.first().is_none());
    }

    #[test]
    fn context_shift_input_can_be_scoped_to_terms() {
        let default =
            ContextTracker::new(start_context, Some(shift_with_input), None, hash_context);
        assert!(default.shift_uses_input(3));

        let scoped = default.with_shift_input_terms(&[7, 11]);
        assert!(!scoped.shift_uses_input(3));
        assert!(scoped.shift_uses_input(7));
        assert!(scoped.shift_uses_input(11));

        let state_only = ContextTracker::new(start_context, None, None, hash_context)
            .with_shift_without_input(shift_without_input)
            .with_shift_input_terms(&[7]);
        assert!(!state_only.shift_uses_input(7));

        let hybrid = ContextTracker::new(start_context, None, None, hash_context)
            .with_context_only_shift(shift_context_only)
            .with_input_shift_for_terms(shift_with_input, &[7, 11]);
        assert!(hybrid.has_context_only_shift());
        assert!(!hybrid.shift_uses_input(3));
        assert!(hybrid.shift_uses_input(7));
        assert!(hybrid.shift_uses_input(11));
        assert!(hybrid.tracks_shift(3));
        assert!(hybrid.tracks_shift(7));
    }

    #[test]
    fn context_only_shifts_report_identity_preserving_transitions() {
        let tracker = ContextTracker::new(start_context, None, None, hash_context)
            .with_context_only_shift(shift_context_only);
        let initial = ContextValue::new(false);

        assert!(tracker.has_context_only_shift());
        assert!(!tracker.shift_uses_input(7));
        assert!(tracker.context_only_shift(&initial, 3).unwrap().is_none());

        let changed = tracker
            .context_only_shift(&initial, 7)
            .unwrap()
            .expect("term 7 changes the context");
        assert_eq!(changed.downcast_ref::<bool>(), Some(&true));
        assert!(tracker.context_only_shift(&changed, 7).unwrap().is_none());
    }

    #[test]
    fn declined_tokenizer_endpoints_remain_lazy() {
        let empty = CachedToken::default();
        assert_eq!(empty.value, ReservedTerm::Error.raw());

        let raw: Arc<dyn Input> = Arc::new(StringInput::try_new("x").unwrap());
        let lexical: Arc<dyn LexicalInput> = Arc::new(Utf8Input::new(raw));
        let ranges: Arc<[TextRange]> = Arc::from([TextRange::new(0.into(), 1.into())]);
        let mut stream = InputStream::new(lexical, ranges);

        let declined = accepted_or_declined(&stream, 0.into());
        assert_eq!(declined.value, ReservedTerm::Error.raw());
        assert_eq!(declined.end, TextSize::from(0));

        stream.advance(1);
        stream.accept_token(7).unwrap();
        let accepted = accepted_or_declined(&stream, 0.into());
        assert_eq!(accepted.value, 7);
        assert_eq!(accepted.end, TextSize::from(1));
    }

    #[test]
    fn cached_base_tokens_are_specialized_for_each_stack() {
        let parser = LRParser::from_language(&STATE_SPECIALIZER_LANGUAGE);
        let dialect_parser = parser.clone().with_dialect("state").unwrap();
        let raw: Arc<dyn Input> = Arc::new(StringInput::try_new("x").unwrap());
        let lexical: Arc<dyn LexicalInput> = Arc::new(Utf8Input::new(raw));
        let ranges: Arc<[TextRange]> = Arc::from([TextRange::new(0.into(), 1.into())]);
        let stream = InputStream::new(lexical, ranges);
        let cached = CachedToken {
            start: 0.into(),
            value: 2,
            end: 1.into(),
            mask: 0,
            context: 0,
        };
        let dialect_replacing = Stack::start(Arc::clone(&dialect_parser.core), 0, 0.into());
        let replacing = Stack::start(Arc::clone(&parser.core), 0, 0.into());
        let retaining = Stack::start(Arc::clone(&dialect_parser.core), 1, 0.into());

        let (dialect_replaced, _) = specialize_cached_token(cached, &dialect_replacing, &stream);
        let (replaced, _) = specialize_cached_token(cached, &replacing, &stream);
        let (retained, _) = specialize_cached_token(cached, &retaining, &stream);

        assert_eq!(dialect_replaced.value, 3);
        assert_eq!(replaced.value, 4);
        assert_eq!(retained.value, 4);
        assert_eq!(cached.value, 2);
    }

    #[test]
    fn strict_token_validation_runs_only_for_strict_parsers() {
        let parser = LRParser::from_language(&SCHEDULER_LANGUAGE)
            .with_strict_token_validators(&SCHEDULER_STRICT_VALIDATORS);

        parser
            .parse("x")
            .expect("the recovering parser skips strict token validation");
        let error = parser
            .with_strict(true)
            .parse("x")
            .expect_err("the strict parser validates the selected base token");

        assert_eq!(error.kind(), ParseErrorKind::Syntax);
        assert_eq!(error.position(), Some(TextSize::from(0)));
        assert_eq!(error.message(), "strict token rejected x");
    }

    #[test]
    fn context_transitions_can_filter_irrelevant_terms() {
        let default =
            ContextTracker::new(start_context, Some(shift_with_input), None, hash_context);
        assert!(default.tracks_shift(3));

        let scoped = default.with_shift_terms(&[7, 11]);
        assert!(!scoped.tracks_shift(3));
        assert!(scoped.tracks_shift(7));
        assert!(scoped.tracks_shift(11));

        let reduced = ContextTracker::new(start_context, None, None, hash_context)
            .with_reduce_without_input(shift_without_input)
            .with_reduce_terms(&[13]);
        assert!(!reduced.tracks_reduction(3));
        assert!(reduced.tracks_reduction(13));
    }

    #[test]
    fn consecutive_default_reductions_run_before_scheduler_reentry() {
        let parser = LRParser::from_language(&REDUCTION_CHAIN_LANGUAGE);
        let input: Arc<dyn Input> = Arc::new(StringInput::try_new("").unwrap());
        let request = ParseRequest::full(input).into_validated().unwrap();
        let mut parse = Parse::new(Arc::clone(&parser.core), request);
        let mut stack = parse.stacks.pop().unwrap();

        assert!(!parse.advance_stack(&mut stack, None, None).unwrap());
        assert_eq!(stack.state(), 2);
        assert_eq!(stack.depth(), 2);
        assert_eq!(parse.actions, 2);
    }

    #[test]
    fn stop_boundary_clears_a_snapshot_without_tokenization() {
        let parser = LRParser::from_language(&SCHEDULER_LANGUAGE);
        let input: Arc<dyn Input> = Arc::new(StringInput::try_new("x").unwrap());
        let request = ParseRequest::full(input).into_validated().unwrap();
        let mut parse = Parse::new(Arc::clone(&parser.core), request);

        assert!(parse.step().unwrap().is_none());
        assert!(parse.tokens.main_token.is_some());
        parse.stop_at(TextSize::from(0)).unwrap();
        let mut stack = parse.stacks.pop().unwrap();

        assert!(!parse.advance_stack(&mut stack, None, None).unwrap());
        assert!(parse.tokens.main_token.is_none());
    }

    #[test]
    fn unique_token_reductions_run_before_scheduler_reentry() {
        let parser = LRParser::from_language(&SCHEDULER_LANGUAGE);
        let input: Arc<dyn Input> = Arc::new(StringInput::try_new("x").unwrap());
        let request = ParseRequest::full(input).into_validated().unwrap();
        let mut parse = Parse::new(Arc::clone(&parser.core), request);
        let mut stack = parse.stacks.pop().unwrap();

        assert!(parse.advance_stack(&mut stack, None, None).unwrap());
        assert_eq!(stack.state(), 2);
        assert_eq!(stack.position(), TextSize::from(1));
        assert_eq!(parse.actions, 2);
    }
}
