use std::any::Any;
use std::cmp::Reverse;
use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use rezel_common::{
    DEFAULT_BUFFER_LENGTH, NodeSet, NodeType, ParseError, ParseErrorKind, ParseRequest,
    ParseWrapper, Parser, PartialParse, TextSize, Tree, TreeBuild,
};

use crate::decode::pair;
use crate::stack::Stack;
use crate::table::{Action, ReservedTerm, SequenceCode, StateField, StateFlag};
use crate::token::{AcceptedToken, InputStream, Tokenizer};

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

/// Statically linked non-incremental context tracker.
pub struct ContextTracker {
    start: fn() -> ContextValue,
    shift: Option<ContextTransition>,
    reduce: Option<ContextTransition>,
    hash: fn(&ContextValue) -> u64,
}

impl ContextTracker {
    /// Define a Rust context tracker.
    #[must_use]
    pub const fn new(
        start: fn() -> ContextValue,
        shift: Option<ContextTransition>,
        reduce: Option<ContextTransition>,
        hash: fn(&ContextValue) -> u64,
    ) -> Self {
        Self {
            start,
            shift,
            reduce,
            hash,
        }
    }

    pub(crate) fn start(&self) -> ContextValue {
        (self.start)()
    }

    pub(crate) fn shift(
        &self,
        context: &ContextValue,
        term: u16,
        stack: &Stack,
        input: &mut InputStream,
    ) -> Result<ContextValue, ParseError> {
        self.shift.map_or_else(
            || Ok(context.clone()),
            |shift| shift(context, term, stack, input),
        )
    }

    pub(crate) fn reduce(
        &self,
        context: &ContextValue,
        term: u16,
        stack: &Stack,
        input: &mut InputStream,
    ) -> Result<ContextValue, ParseError> {
        self.reduce.map_or_else(
            || Ok(context.clone()),
            |reduce| reduce(context, term, stack, input),
        )
    }

    pub(crate) fn hash(&self, context: &ContextValue) -> u64 {
        (self.hash)(context)
    }
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

impl fmt::Debug for SpecializerSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SpecializerSpec")
            .field("term", &self.term)
            .finish_non_exhaustive()
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
    pub token_data: &'static [u16],
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
    /// Dynamic reduction precedences.
    pub dynamic_precedences: &'static [i16],
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
    pub(crate) dialect: Dialect,
    top: ParserTop,
    pub(crate) buffer_length: TextSize,
    pub(crate) limits: ParseLimits,
    pub(crate) context: Option<&'static ContextTracker>,
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
        self.language
            .dynamic_precedences
            .get(usize::from(term))
            .copied()
            .map_or(0, i32::from)
    }

    pub(crate) fn get_goto(&self, state: u16, term: u16, loose: bool) -> Option<u16> {
        let table = self.language.goto;
        if usize::from(term) >= usize::from(table[0]) {
            return None;
        }
        let mut position = usize::from(table[usize::from(term) + 1]);
        if position <= usize::from(table[0]) {
            return None;
        }
        loop {
            let group_tag = table[position];
            position += 1;
            let last = group_tag & 1 != 0;
            let target = table[position];
            position += 1;
            if last && loose {
                return Some(target);
            }
            let end = position + usize::from(group_tag >> 1);
            for candidate in &table[position..end] {
                if *candidate == state {
                    return Some(target);
                }
            }
            if last {
                return None;
            }
            position = end;
        }
    }

    pub(crate) fn has_action(&self, state: u16, terminal: u16) -> Action {
        let data = self.language.state_data;
        for field in [StateField::Actions, StateField::Skip] {
            let mut index = self.state_slot(state, field) as usize;
            loop {
                let mut next = data[index];
                if next == SequenceCode::End.raw() {
                    if data[index + 1] == SequenceCode::Next.raw() {
                        index = pair(data, index + 2) as usize;
                        next = data[index];
                    } else {
                        if data[index + 1] == SequenceCode::Other.raw() {
                            return Action::from_raw(pair(data, index + 2));
                        }
                        break;
                    }
                }
                if next == terminal || next == ReservedTerm::Error.raw() {
                    return Action::from_raw(pair(data, index + 1));
                }
                index += 3;
            }
        }
        Action::NONE
    }

    pub(crate) fn all_actions<T>(
        &self,
        state: u16,
        mut visit: impl FnMut(Action) -> Option<T>,
    ) -> Option<T> {
        let default = Action::from_raw(self.state_slot(state, StateField::DefaultReduce));
        if !default.is_none()
            && let Some(result) = visit(default)
        {
            return Some(result);
        }
        let data = self.language.state_data;
        let mut index = self.state_slot(state, StateField::Actions) as usize;
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
        let top = *language.top_rules.first().ok_or_else(|| {
            ParseError::new(
                ParseErrorKind::Configuration,
                None,
                "language has no top rule",
            )
        })?;
        let dialect = Dialect::from_specs(None, language.dialects, language.max_term)?;
        let core = ParserCore {
            language,
            node_set,
            max_node,
            min_repeat_term: language.min_repeat_term,
            dialect,
            top: ParserTop::new(top),
            buffer_length: DEFAULT_BUFFER_LENGTH,
            limits: ParseLimits::default(),
            context: language.context,
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
        request: &ParseRequest,
    ) -> Result<Box<dyn PartialParse>, ParseError> {
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
        self.create_lr_parse(&request)
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

fn validate_goto_table(table: &[u16], state_count: usize) -> Result<(), ParseError> {
    let Some(&term_count) = table.first() else {
        return Err(configuration_error("goto table cannot be empty"));
    };
    let header_length = usize::from(term_count) + 1;
    if table.len() < header_length {
        return Err(configuration_error("goto table header is truncated"));
    }
    for term in 0..usize::from(term_count) {
        let mut position = usize::from(table[term + 1]);
        if position < header_length {
            if position != 1 {
                return Err(configuration_error("goto table has an invalid empty entry"));
            }
            continue;
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
            let end = position
                .checked_add(usize::from(group_tag >> 1))
                .ok_or_else(|| configuration_error("goto group length overflows"))?;
            let Some(sources) = table.get(position..end) else {
                return Err(configuration_error("goto group sources are truncated"));
            };
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
    value: Option<u16>,
    end: TextSize,
    extended: Option<u16>,
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

#[derive(Debug)]
struct TokenCache {
    tokens: Vec<CachedToken>,
    main_token: Option<MainToken>,
    actions: Vec<TokenAction>,
}

impl TokenCache {
    fn new(tokenizer_count: usize) -> Self {
        Self {
            tokens: vec![CachedToken::default(); tokenizer_count],
            main_token: None,
            actions: Vec::new(),
        }
    }

    fn get_actions(&mut self, stack: &Stack, stream: &mut InputStream) -> Result<(), ParseError> {
        let core = stack.core();
        let mask = core.state_slot(stack.state(), StateField::TokenizerMask);
        let context = stack.context_hash();
        self.actions.clear();
        let mut main = None;
        for (index, tokenizer) in core.language.tokenizers.iter().copied().enumerate() {
            if (1_u32 << index) & mask == 0 {
                continue;
            }
            let flags = tokenizer.flags();
            if main.is_some() && !flags.fallback {
                continue;
            }
            let stale = {
                let token = &self.tokens[index];
                flags.contextual
                    || token.start != stack.position()
                    || token.mask != mask
                    || token.context != context
            };
            if stale {
                self.tokens[index] = update_cached_token(tokenizer, stack, stream, mask, context)?;
            }
            let token = &self.tokens[index];
            if token.value != Some(ReservedTerm::Error.raw()) {
                let before = self.actions.len();
                if let Some(extended) = token.extended {
                    add_actions(stack, extended, token.end, &mut self.actions);
                }
                if let Some(value) = token.value {
                    add_actions(stack, value, token.end, &mut self.actions);
                }
                if !flags.extend {
                    main = Some(MainToken {
                        start: token.start,
                        value: token.value.expect("accepted main token has a value"),
                        end: token.end,
                    });
                    if self.actions.len() > before {
                        break;
                    }
                }
            }
        }
        if main.is_none() && stack.position() == stream.end() {
            let eof = MainToken {
                start: stack.position(),
                value: core.eof_term(),
                end: stack.position(),
            };
            add_actions(stack, eof.value, eof.end, &mut self.actions);
            main = Some(eof);
        }
        self.main_token = main;
        Ok(())
    }

    fn main_token(&self, stack: &Stack, stream: &InputStream) -> MainToken {
        self.main_token.unwrap_or_else(|| MainToken {
            start: stack.position(),
            value: if stack.position() == stream.end() {
                stack.core().eof_term()
            } else {
                ReservedTerm::Error.raw()
            },
            end: stream
                .next_position_from(stack.position())
                .unwrap_or_else(|| stream.end())
                .max(stack.position()),
        })
    }
}

fn update_cached_token(
    tokenizer: Tokenizer,
    stack: &Stack,
    stream: &mut InputStream,
    mask: u32,
    context: u64,
) -> Result<CachedToken, ParseError> {
    let start = stream.clip_position(stack.position());
    stream.reset(start);
    tokenizer.token(stream, stack)?;
    let accepted = stream.accepted().unwrap_or_else(|| AcceptedToken {
        value: ReservedTerm::Error.raw(),
        end: stream
            .next_position_from(start)
            .unwrap_or_else(|| stream.end())
            .max(start),
    });
    let mut token = CachedToken {
        start,
        value: Some(accepted.value),
        end: accepted.end,
        extended: None,
        mask,
        context,
    };
    if token.value != Some(ReservedTerm::Error.raw()) {
        let core = stack.core();
        for specializer in core.language.specializers {
            if Some(specializer.term) != token.value {
                continue;
            }
            let lexeme = stream.read(token.start, token.end);
            if let Some(result) = (specializer.get)(&lexeme, stack)
                && core.dialect.allows(result.term)
            {
                match result.kind {
                    Specialize::Replace => token.value = Some(result.term),
                    Specialize::Extend => token.extended = Some(result.term),
                }
            }
            break;
        }
    }
    Ok(token)
}

fn add_actions(stack: &Stack, token: u16, end: TextSize, actions: &mut Vec<TokenAction>) {
    let core = stack.core();
    let data = core.language.state_data;
    for field in [StateField::Actions, StateField::Skip] {
        let mut index = core.state_slot(stack.state(), field) as usize;
        loop {
            if data[index] == SequenceCode::End.raw() {
                match data[index + 1] {
                    value if value == SequenceCode::Next.raw() => {
                        index = pair(data, index + 2) as usize;
                    }
                    value if value == SequenceCode::Other.raw() && actions.is_empty() => {
                        put_action(actions, Action::from_raw(pair(data, index + 2)), token, end);
                        break;
                    }
                    _ => break,
                }
            }
            if data[index] == token {
                put_action(actions, Action::from_raw(pair(data, index + 1)), token, end);
            }
            index += 3;
        }
    }
}

fn put_action(actions: &mut Vec<TokenAction>, action: Action, token: u16, end: TextSize) {
    if actions.iter().any(|candidate| candidate.action == action) {
        return;
    }
    actions.push(TokenAction { action, token, end });
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
        let stream = InputStream::new(Arc::clone(request.input()), Arc::clone(&ranges));
        let start = ranges
            .first()
            .map_or(TextSize::from(0), |range| range.start());
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
            let mut stack = pending.remove(0);
            loop {
                self.tokens.main_token = None;
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
            return stack.force_reduce(
                &mut self.stream,
                &mut self.actions,
                &mut self.large_reductions,
            );
        }
        let default_reduce = Action::from_raw(
            self.core
                .state_slot(stack.state(), StateField::DefaultReduce),
        );
        if !default_reduce.is_none() {
            self.bump_action(stack.position())?;
            stack.reduce(default_reduce, &mut self.stream, &mut self.large_reductions)?;
            return Ok(true);
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
        if split.is_none() || self.tokens.actions.len() <= 1 {
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
            return Ok(true);
        }

        // Keep the reusable token-action buffer in place while applying the
        // alternatives, as upstream Lezer does. Applying an action mutates
        // the stack and input stream, but not this cache.
        let action_count = self.tokens.actions.len();
        for index in 0..action_count {
            let choice = self.tokens.actions[index];
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
        Ok(false)
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
