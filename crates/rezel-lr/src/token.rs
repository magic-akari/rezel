use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rezel_common::{
    CodePoint, InputCharacter, InputChunk, LexicalInput, ParseError, ParseErrorKind, TextRange,
    TextSize,
};
use zerocopy::{FromBytes, Immutable};

use crate::stack::Stack;
use crate::table::SequenceCode;

const NO_TOKEN_STATE: u16 = u16::MAX;

/// One generated token-DFA state.
#[repr(C, align(2))]
#[derive(Clone, Copy, Debug, Eq, FromBytes, Immutable, PartialEq)]
pub struct TokenState {
    group_mask: u16,
    accept_start: u16,
    edge_start: u16,
    accept_count: u8,
    edge_count: u8,
}

/// One accepting token term in a generated DFA state.
#[repr(C, align(2))]
#[derive(Clone, Copy, Debug, Eq, FromBytes, Immutable, PartialEq)]
pub struct TokenAccept {
    term: u16,
    group_mask: u16,
}

/// One half-open Unicode code-point transition.
#[repr(C, align(4))]
#[derive(Clone, Copy, Debug, Eq, FromBytes, Immutable, PartialEq)]
pub struct TokenEdge {
    from: u32,
    to: u32,
    target: u16,
}

/// One generated EOF transition, kept outside the character hot path.
#[repr(C, align(2))]
#[derive(Clone, Copy, Debug, Eq, FromBytes, Immutable, PartialEq)]
pub struct TokenEof {
    state: u16,
    target: u16,
}

/// Typed static token-DFA tables emitted by the generator.
#[derive(Clone, Copy, Debug)]
pub struct TokenTable {
    states: &'static [TokenState],
    accepts: &'static [TokenAccept],
    edges: &'static [TokenEdge],
    eof: &'static [TokenEof],
}

impl TokenTable {
    /// Construct one generated token table.
    #[doc(hidden)]
    #[must_use]
    pub const fn new(
        states: &'static [TokenState],
        accepts: &'static [TokenAccept],
        edges: &'static [TokenEdge],
        eof: &'static [TokenEof],
    ) -> Self {
        Self {
            states,
            accepts,
            edges,
            eof,
        }
    }

    fn accepts(self, state: TokenState) -> &'static [TokenAccept] {
        let start = usize::from(state.accept_start);
        let end = start + usize::from(state.accept_count);
        &self.accepts[start..end]
    }

    fn edges(self, state: TokenState) -> &'static [TokenEdge] {
        let start = usize::from(state.edge_start);
        let end = start + usize::from(state.edge_count);
        &self.edges[start..end]
    }

    fn eof_target(self, state: usize) -> Option<usize> {
        let state = u16::try_from(state).ok()?;
        let index = self
            .eof
            .binary_search_by_key(&state, |transition| transition.state)
            .ok()?;
        Some(usize::from(self.eof[index].target))
    }

    #[inline]
    fn transition(self, state: TokenState, next: u32) -> Option<usize> {
        let edges = self.edges(state);
        let mut low = 0_usize;
        let mut high = edges.len();
        while low < high {
            let middle = (low + high) >> 1;
            let edge = edges[middle];
            if next < edge.from {
                high = middle;
            } else if next >= edge.to {
                low = middle + 1;
            } else {
                return Some(usize::from(edge.target));
            }
        }
        None
    }
}

#[derive(Debug)]
pub(crate) struct TokenAsciiIndex {
    transitions: Box<[[u16; 128]]>,
}

impl TokenAsciiIndex {
    pub(crate) fn build(table: &TokenTable) -> Self {
        let mut transitions = Vec::with_capacity(table.states.len());
        for state in table.states {
            let mut ascii = [NO_TOKEN_STATE; 128];
            for edge in table.edges(*state) {
                let from = usize::try_from(edge.from)
                    .unwrap_or(usize::MAX)
                    .min(ascii.len());
                let to = usize::try_from(edge.to)
                    .unwrap_or(usize::MAX)
                    .min(ascii.len());
                if from < to {
                    ascii[from..to].fill(edge.target);
                }
            }
            transitions.push(ascii);
        }
        Self {
            transitions: transitions.into_boxed_slice(),
        }
    }

    #[inline]
    fn transition(&self, state: usize, next: u8) -> Option<usize> {
        let target = self.transitions[state][usize::from(next)];
        (target != NO_TOKEN_STATE).then_some(usize::from(target))
    }
}

/// Flags controlling how a tokenizer participates in one parser state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TokenizerFlags {
    /// Token output depends on the current parse stack.
    pub contextual: bool,
    /// Run when an earlier tokenizer produced no actionable token.
    pub fallback: bool,
    /// Continue running lower-precedence tokenizers after a match.
    pub extend: bool,
}

/// A conservative first-code-point filter for an external tokenizer.
///
/// A match only means that the tokenizer may produce a parser-visible outcome.
/// The callback still makes the parser-aware decision. Every position where it
/// can accept a token or return an error must therefore be included; only a
/// guaranteed decline may be filtered out.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExternalTokenizerStart {
    ascii: [u64; 2],
    non_ascii: bool,
    end: bool,
}

impl ExternalTokenizerStart {
    /// An empty filter.
    pub const NONE: Self = Self {
        ascii: [0; 2],
        non_ascii: false,
        end: false,
    };

    /// Include one ASCII byte.
    ///
    /// # Panics
    ///
    /// Panics when `byte` is not ASCII.
    #[must_use]
    pub const fn with_ascii(mut self, byte: u8) -> Self {
        assert!(byte.is_ascii(), "external tokenizer start must be ASCII");
        let word = byte / 64;
        let bit = byte % 64;
        self.ascii[word as usize] |= 1_u64 << bit;
        self
    }

    /// Include one inclusive ASCII byte range.
    ///
    /// # Panics
    ///
    /// Panics when the range is reversed or either bound is not ASCII.
    #[must_use]
    pub const fn with_ascii_range(mut self, range: std::ops::RangeInclusive<u8>) -> Self {
        let start = *range.start();
        let end = *range.end();
        assert!(
            start <= end && end.is_ascii(),
            "external tokenizer start range must be ordered ASCII"
        );
        let mut byte = start;
        loop {
            self = self.with_ascii(byte);
            if byte == end {
                break;
            }
            byte += 1;
        }
        self
    }

    /// Include every non-ASCII code point.
    #[must_use]
    pub const fn with_non_ascii(mut self) -> Self {
        self.non_ascii = true;
        self
    }

    /// Include end of input.
    #[must_use]
    pub const fn with_end(mut self) -> Self {
        self.end = true;
        self
    }

    #[inline]
    pub(crate) fn matches(self, next: Option<CodePoint>) -> bool {
        let Some(next) = next else {
            return self.end;
        };
        let value = next.as_u32();
        if value >= 0x80 {
            return self.non_ascii;
        }
        let byte = u8::try_from(value).expect("ASCII code point fits in u8");
        let word = byte / 64;
        let bit = byte % 64;
        self.ascii[usize::from(word)] & (1_u64 << bit) != 0
    }
}

/// One parser-generated token DFA group.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TokenGroup {
    /// Bit position selecting accepting terms for this group.
    pub id: u8,
}

impl TokenGroup {
    /// Construct a parser-generated token group.
    #[must_use]
    pub const fn new(id: u8) -> Self {
        Self { id }
    }
}

/// One local token DFA and optional `@else` token.
#[derive(Clone, Copy, Debug)]
pub struct LocalTokenGroup {
    /// Typed token DFA.
    pub table: &'static TokenTable,
    /// Token-precedence sequence.
    pub precedence: &'static [u16],
    /// Token emitted for otherwise unmatched input.
    pub else_token: Option<u16>,
}

impl LocalTokenGroup {
    /// Construct a local token group.
    #[must_use]
    pub const fn new(
        table: &'static TokenTable,
        precedence: &'static [u16],
        else_token: Option<u16>,
    ) -> Self {
        Self {
            table,
            precedence,
            else_token,
        }
    }
}

/// Statically linked Rust external tokenizer.
pub struct ExternalTokenizer {
    callback: fn(&mut InputStream, &Stack) -> Result<(), ParseError>,
    flags: TokenizerFlags,
    start: Option<ExternalTokenizerStart>,
}

impl ExternalTokenizer {
    /// Define an external tokenizer binding.
    #[must_use]
    pub const fn new(
        callback: fn(&mut InputStream, &Stack) -> Result<(), ParseError>,
        flags: TokenizerFlags,
    ) -> Self {
        Self {
            callback,
            flags,
            start: None,
        }
    }

    /// Skip this callback when the next code point cannot start its token.
    #[must_use]
    pub const fn with_start(mut self, start: ExternalTokenizerStart) -> Self {
        self.start = Some(start);
        self
    }

    fn token(&self, input: &mut InputStream, stack: &Stack) -> Result<(), ParseError> {
        (self.callback)(input, stack)
    }

    const fn flags(&self) -> TokenizerFlags {
        self.flags
    }

    const fn start(&self) -> Option<ExternalTokenizerStart> {
        self.start
    }
}

impl std::fmt::Debug for ExternalTokenizer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExternalTokenizer")
            .field("flags", &self.flags)
            .field("start", &self.start)
            .finish_non_exhaustive()
    }
}

/// One tokenizer entry in a generated language.
#[derive(Clone, Copy, Debug)]
pub enum Tokenizer {
    /// A group in the generated global token DFA.
    Group(TokenGroup),
    /// A generated local token DFA.
    Local(LocalTokenGroup),
    /// A statically bound Rust tokenizer.
    External(&'static ExternalTokenizer),
}

impl Tokenizer {
    pub(crate) const fn flags(self) -> TokenizerFlags {
        match self {
            Self::Group(_) | Self::Local(_) => TokenizerFlags {
                contextual: false,
                fallback: false,
                extend: false,
            },
            Self::External(tokenizer) => tokenizer.flags(),
        }
    }

    #[inline]
    pub(crate) const fn start(self) -> Option<ExternalTokenizerStart> {
        match self {
            Self::Group(_) | Self::Local(_) => None,
            Self::External(tokenizer) => tokenizer.start(),
        }
    }

    pub(crate) fn token(
        self,
        tokenizer_index: usize,
        input: &mut InputStream,
        stack: &Stack,
    ) -> Result<(), ParseError> {
        match self {
            Self::Group(group) => {
                let core = stack.core();
                read_token(
                    core.language.token_table,
                    Some(&core.token_ascii_index),
                    input,
                    stack,
                    group.id,
                    core.language.state_data,
                    core.language.token_precedence,
                );
                Ok(())
            }
            Self::Local(group) => {
                let ascii_index = stack.core().local_token_ascii_indices[tokenizer_index]
                    .as_ref()
                    .expect("local tokenizer has an ASCII index");
                read_local_token(group, ascii_index, input, stack)
            }
            Self::External(tokenizer) => tokenizer.token(input, stack),
        }
    }
}

/// Precomputed first-code-point masks for external tokenizers.
///
/// Parser states already encode the tokenizers that can contribute there. This
/// index intersects that state mask with the next input code point once, before
/// dispatch, instead of asking every active external tokenizer separately.
#[derive(Debug)]
pub(crate) struct TokenizerStartIndex {
    ascii: [u32; 128],
    non_ascii: u32,
    end: u32,
    filtered: u32,
    unfiltered: u32,
}

impl TokenizerStartIndex {
    pub(crate) fn build(tokenizers: &[Tokenizer]) -> Self {
        let mut index = Self {
            ascii: [0; 128],
            non_ascii: 0,
            end: 0,
            filtered: 0,
            unfiltered: 0,
        };
        for (position, tokenizer) in tokenizers.iter().copied().enumerate() {
            let bit = 1_u32 << position;
            let Some(start) = tokenizer.start() else {
                index.unfiltered |= bit;
                continue;
            };
            index.filtered |= bit;
            for byte in 0_u8..0x80 {
                if start.matches(Some(CodePoint::from(byte))) {
                    index.ascii[usize::from(byte)] |= bit;
                }
            }
            if start.non_ascii {
                index.non_ascii |= bit;
            }
            if start.end {
                index.end |= bit;
            }
        }
        index
    }

    #[inline]
    pub(crate) fn has_filtered(&self, mask: u32) -> bool {
        mask & self.filtered != 0
    }

    #[inline]
    pub(crate) fn filter(&self, mask: u32, next: Option<CodePoint>) -> u32 {
        let matching = match next {
            None => self.end,
            Some(next) if next.as_u32() >= 0x80 => self.non_ascii,
            Some(next) => self.ascii[next.as_u32() as usize],
        };
        mask & (self.unfiltered | matching)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StreamCursor {
    range_index: usize,
    byte: TextSize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CachedCharacter {
    range_index: usize,
    byte: TextSize,
    character: InputCharacter,
}

#[derive(Clone, Debug)]
struct FastWindow {
    source: Arc<str>,
    source_start: usize,
    source_end: usize,
    raw_start: TextSize,
    raw_end: TextSize,
}

impl FastWindow {
    fn new(chunk: &InputChunk, raw_from: TextSize, raw_limit: TextSize) -> Option<Self> {
        let chunk_raw_start = chunk.raw_start();
        let raw_start = chunk_raw_start.max(raw_from);
        let raw_end = chunk.raw_end().min(raw_limit);
        if raw_start >= raw_end {
            return None;
        }
        let source_range = chunk.source_range();
        let source_offset = usize::from(raw_start - chunk_raw_start);
        let source_start = usize::from(source_range.start()).checked_add(source_offset)?;
        let source_length = usize::from(raw_end - raw_start);
        let source_end = source_start.checked_add(source_length)?;
        let source = chunk.shared_source();
        (source_end <= source.len()).then_some(Self {
            source,
            source_start,
            source_end,
            raw_start,
            raw_end,
        })
    }

    #[inline]
    fn source_position(&self, raw_position: TextSize) -> Option<usize> {
        if raw_position < self.raw_start || raw_position > self.raw_end {
            return None;
        }
        let offset = usize::from(raw_position - self.raw_start);
        self.source_start.checked_add(offset)
    }

    #[inline]
    fn contains(&self, raw_position: TextSize) -> bool {
        self.raw_start <= raw_position && raw_position < self.raw_end
    }
}

/// A validated position captured from one input stream.
///
/// Marks are tied to their originating stream and cannot be constructed by
/// callers. This lets tokenizers save an endpoint without supplying unchecked
/// byte positions or relative offsets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputMark {
    stream_id: u64,
    position: TextSize,
}

/// Result of advancing through one identity-mapped ASCII run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AsciiAdvance {
    count: usize,
    stopped_on_mismatch: bool,
}

impl AsciiAdvance {
    const fn new(count: usize, stopped_on_mismatch: bool) -> Self {
        Self {
            count,
            stopped_on_mismatch,
        }
    }

    /// Number of ASCII bytes consumed from the current input window.
    #[must_use]
    pub const fn count(self) -> usize {
        self.count
    }

    /// Whether the next byte was ASCII and rejected by the predicate.
    ///
    /// A false result may instead mean end of input, a non-ASCII code point,
    /// a translation boundary, or a selected-range boundary.
    #[must_use]
    pub const fn stopped_on_mismatch(self) -> bool {
        self.stopped_on_mismatch
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AcceptedToken {
    pub(crate) value: u16,
    pub(crate) end: TextSize,
}

/// UTF-8-positioned input stream exposed to generated and external tokenizers.
///
/// Tokenizers observe Unicode code points. Ordinary UTF-8 input produces only
/// Unicode scalar values, while a language translation layer may explicitly
/// produce surrogate code points. Positions and token ranges remain original
/// UTF-8 byte offsets.
pub struct InputStream {
    input: Arc<dyn LexicalInput>,
    ranges: Arc<[TextRange]>,
    cursor: StreamCursor,
    end: TextSize,
    token_start: TextSize,
    accepted: Option<AcceptedToken>,
    chunk: Option<InputChunk>,
    window: Option<FastWindow>,
    window_source_position: usize,
    character: Option<CachedCharacter>,
    next_code_point: Option<CodePoint>,
    stream_id: u64,
}

impl std::fmt::Debug for InputStream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InputStream")
            .field("position", &self.position())
            .field("end", &self.end)
            .field("token_start", &self.token_start)
            .field("accepted", &self.accepted)
            .finish_non_exhaustive()
    }
}

impl InputStream {
    pub(crate) fn new(input: Arc<dyn LexicalInput>, ranges: Arc<[TextRange]>) -> Self {
        let end = ranges.last().map_or(TextSize::from(0), |range| range.end());
        let cursor = first_cursor(&ranges);
        let mut stream = Self {
            input,
            ranges,
            cursor,
            end,
            token_start: cursor.byte,
            accepted: None,
            chunk: None,
            window: None,
            window_source_position: 0,
            character: None,
            next_code_point: None,
            stream_id: next_stream_id(),
        };
        stream.refresh_next();
        stream
    }

    /// Current UTF-8 byte position.
    #[must_use]
    pub const fn position(&self) -> TextSize {
        self.cursor.byte
    }

    /// Final selected UTF-8 byte position.
    #[must_use]
    pub const fn end(&self) -> TextSize {
        self.end
    }

    /// Next Unicode code point, or `None` at the end of selected input.
    #[must_use]
    pub const fn next(&self) -> Option<CodePoint> {
        self.next_code_point
    }

    /// Look around the stream in Unicode code points.
    #[must_use]
    pub fn peek(&self, offset: isize) -> Option<CodePoint> {
        match offset.cmp(&0) {
            std::cmp::Ordering::Equal => self.next(),
            std::cmp::Ordering::Greater => {
                let offset = offset.unsigned_abs();
                let chunk = self.identity_lookahead_chunk();
                if let Some(prefix) = chunk.get(..=offset)
                    && prefix.is_ascii()
                {
                    return Some(CodePoint::from(prefix[offset]));
                }
                self.lookahead().nth(offset)
            }
            std::cmp::Ordering::Less => self
                .lookbehind()
                .nth(offset.unsigned_abs().saturating_sub(1)),
        }
    }

    /// Return the code point immediately before the current position.
    ///
    /// This is equivalent to `self.peek(-1)`, with an adjacent ASCII fast path
    /// over the active identity-mapped input window.
    #[must_use]
    #[inline]
    pub fn previous(&self) -> Option<CodePoint> {
        let range = self.ranges.get(self.cursor.range_index)?;
        if self.cursor.byte > range.start()
            && let Some(window) = self.window.as_ref()
            && self.cursor.byte > window.raw_start
            && self.cursor.byte <= window.raw_end
            && let Some(source_position) = window.source_position(self.cursor.byte)
            && let Some(previous_position) = source_position.checked_sub(1)
            && let Some(previous) = window.source.as_bytes().get(previous_position).copied()
            && previous.is_ascii()
        {
            return Some(CodePoint::from(previous));
        }
        self.lookbehind().next()
    }

    /// Iterate forward from the current position in Unicode code points.
    ///
    /// External tokenizers that inspect a run of input should prefer this to
    /// repeatedly calling [`Self::peek`] with increasing offsets.
    pub fn lookahead(&self) -> impl Iterator<Item = CodePoint> + '_ {
        let fast_bytes = self
            .window
            .as_ref()
            .and_then(|window| {
                let start = window.source_position(self.cursor.byte)?;
                window.source.as_bytes().get(start..window.source_end)
            })
            .unwrap_or_default();
        InputLookahead {
            input: &*self.input,
            ranges: &self.ranges,
            cursor: Some(self.cursor),
            initial_chunk: self.chunk.as_ref(),
            loaded_chunk: None,
            fast_window: self.window.as_ref(),
            fast_bytes,
            fast_byte_position: 0,
        }
    }

    /// Return the remaining raw UTF-8 bytes in the current identity-mapped chunk.
    ///
    /// The slice stops at the current selected-range or translation boundary.
    /// An empty slice does not imply end of input: callers that need to cross a
    /// boundary must fall back to [`Self::lookahead`]. External tokenizers may
    /// use this as a fast path when their decision is defined entirely by ASCII
    /// bytes and preserve the code-point iterator as the authoritative fallback.
    #[must_use]
    pub fn identity_lookahead_chunk(&self) -> &[u8] {
        let Some(window) = self.window.as_ref() else {
            return &[];
        };
        let Some(start) = window.source_position(self.cursor.byte) else {
            return &[];
        };
        window
            .source
            .as_bytes()
            .get(start..window.source_end)
            .unwrap_or_default()
    }

    /// Iterate backward from immediately before the current position in Unicode code points.
    ///
    /// Like [`Self::lookahead`], this follows the stream's selected-range
    /// order. It does not move the input cursor. External tokenizers that
    /// inspect a run of preceding input should prefer this to repeatedly
    /// calling [`Self::peek`] with decreasing offsets.
    pub fn lookbehind(&self) -> impl Iterator<Item = CodePoint> + '_ {
        InputLookbehind {
            input: &*self.input,
            ranges: &self.ranges,
            cursor: Some(self.cursor),
        }
    }

    /// Move forward by Unicode code points and return the new next code point.
    pub fn advance(&mut self, count: usize) -> Option<CodePoint> {
        if count == 1 && self.advance_ascii() {
            return self.next_code_point;
        }
        self.advance_general(count)
    }

    fn advance_general(&mut self, count: usize) -> Option<CodePoint> {
        for _ in 0..count {
            let Some(next) = self.advance_current() else {
                self.cursor = end_cursor(&self.ranges);
                self.next_code_point = None;
                return None;
            };
            self.cursor = next;
        }
        self.refresh_next();
        self.next_code_point
    }

    fn advance_ascii(&mut self) -> bool {
        let Some(next) = self
            .next_code_point
            .filter(|next| next.is_ascii())
            .map(|next| u8::try_from(next.as_u32()).expect("ASCII code point fits u8"))
        else {
            return false;
        };

        self.advance_known_ascii_in_window(next)
    }

    fn advance_known_ascii(&mut self, next: u8) {
        debug_assert_eq!(self.next_code_point, Some(CodePoint::from(next)));
        if !self.advance_known_ascii_in_window(next) {
            self.advance_general(1);
        }
    }

    fn advance_known_ascii_in_window(&mut self, next: u8) -> bool {
        let source_position = self.window_source_position;
        let next_byte = {
            let Some(window) = self.window.as_ref() else {
                return false;
            };
            if source_position >= window.source_end {
                return false;
            }
            let bytes = window.source.as_bytes();
            debug_assert_eq!(bytes.get(source_position).copied(), Some(next));
            let next_source_position = source_position + 1;
            if next_source_position < window.source_end {
                bytes.get(next_source_position).copied()
            } else {
                None
            }
        };
        let next_source_position = source_position + 1;

        self.cursor.byte += TextSize::from(1);
        self.window_source_position = next_source_position;
        self.character = None;

        if let Some(next) = next_byte
            && next.is_ascii()
        {
            self.next_code_point = Some(CodePoint::from(next));
            return true;
        }

        if self
            .window
            .as_ref()
            .is_some_and(|window| self.cursor.byte == window.raw_end)
        {
            self.move_past_selected_range_end();
        }
        self.refresh_next();
        true
    }

    fn move_past_selected_range_end(&mut self) {
        let Some(range) = self.ranges.get(self.cursor.range_index) else {
            return;
        };
        if self.cursor.byte < range.end() {
            return;
        }
        self.cursor = next_range_cursor(&self.ranges, self.cursor.range_index)
            .unwrap_or_else(|| end_cursor(&self.ranges));
    }

    fn load_identity_chunk(&mut self, range: TextRange, position: TextSize) {
        self.chunk = self.input.identity_chunk(position);
        self.window = self
            .chunk
            .as_ref()
            .and_then(|chunk| FastWindow::new(chunk, position, range.end()))
            .filter(|window| window.contains(position));
        self.window_source_position = self
            .window
            .as_ref()
            .and_then(|window| window.source_position(position))
            .unwrap_or(0);
    }

    fn sync_window_position(&mut self) -> bool {
        let Some(window) = self.window.as_ref() else {
            return false;
        };
        if !window.contains(self.cursor.byte) {
            return false;
        }
        let Some(source_position) = window.source_position(self.cursor.byte) else {
            return false;
        };
        self.window_source_position = source_position;
        true
    }

    /// Advance over one ASCII run in the current identity-mapped input chunk.
    ///
    /// Translation boundaries, selected-range boundaries, and non-ASCII
    /// input stop the run before `predicate` is called for later bytes.
    pub fn advance_ascii_while(&mut self, predicate: impl FnMut(u8) -> bool) -> usize {
        self.advance_ascii_while_impl::<false>(predicate).count()
    }

    /// Advance over one ASCII run and report whether its predicate rejected
    /// the next byte.
    ///
    /// Unlike inspecting the next code point after [`Self::advance_ascii_while`],
    /// this distinguishes a predicate mismatch from a translation or
    /// selected-range boundary without scanning the boundary again.
    pub fn advance_ascii_while_with_stop(
        &mut self,
        predicate: impl FnMut(u8) -> bool,
    ) -> AsciiAdvance {
        self.advance_ascii_while_impl::<true>(predicate)
    }

    fn advance_ascii_while_impl<const REPORT_STOP: bool>(
        &mut self,
        mut predicate: impl FnMut(u8) -> bool,
    ) -> AsciiAdvance {
        let has_window = self
            .window
            .as_ref()
            .is_some_and(|window| self.window_source_position < window.source_end);
        if !has_window {
            let Some(range) = self.ranges.get(self.cursor.range_index).copied() else {
                return AsciiAdvance::new(0, false);
            };
            if self.cursor.byte >= range.end() {
                return AsciiAdvance::new(0, false);
            }
            self.load_identity_chunk(range, self.cursor.byte);
        }
        let (count, next_byte, window_end) = {
            let Some(window) = self.window.as_ref() else {
                return AsciiAdvance::new(0, false);
            };
            debug_assert_eq!(
                window.source_position(self.cursor.byte),
                Some(self.window_source_position)
            );
            let Some(bytes) = window
                .source
                .as_bytes()
                .get(self.window_source_position..window.source_end)
            else {
                return AsciiAdvance::new(0, false);
            };
            // Keep the count-only hot path as an indexed loop while the
            // reporting monomorphization retains its rejected-byte scan.
            let count = if REPORT_STOP {
                bytes
                    .iter()
                    .copied()
                    .take_while(|byte| byte.is_ascii() && predicate(*byte))
                    .count()
            } else {
                let mut count = 0_usize;
                while let Some(byte) = bytes.get(count).copied() {
                    if !byte.is_ascii() || !predicate(byte) {
                        break;
                    }
                    count += 1;
                }
                count
            };
            let next_byte = bytes.get(count).copied();
            (count, next_byte, window.raw_end)
        };
        let stopped_on_mismatch = REPORT_STOP && next_byte.is_some_and(|byte| byte.is_ascii());
        if count == 0 {
            return AsciiAdvance::new(0, stopped_on_mismatch);
        }
        let Ok(width) = TextSize::try_from(count) else {
            return AsciiAdvance::new(0, false);
        };
        self.cursor.byte += width;
        self.window_source_position += count;
        self.character = None;

        if let Some(next) = next_byte
            && next.is_ascii()
        {
            self.next_code_point = Some(CodePoint::from(next));
            return AsciiAdvance::new(count, true);
        }

        if self.cursor.byte == window_end {
            self.move_past_selected_range_end();
        }
        self.refresh_next();
        AsciiAdvance::new(count, false)
    }

    /// Capture the current validated input boundary.
    #[must_use]
    pub const fn mark(&self) -> InputMark {
        InputMark {
            stream_id: self.stream_id,
            position: self.position(),
        }
    }

    /// Accept a token ending at the current stream position.
    ///
    /// # Errors
    ///
    /// Returns an input error if an internal reset placed the stream before
    /// the token start.
    pub fn accept_token(&mut self, token: u16) -> Result<(), ParseError> {
        self.accept_at(token, self.position())
    }

    /// Accept a token ending at a previously captured boundary.
    ///
    /// # Errors
    ///
    /// Returns an input error when the mark belongs to another stream or
    /// precedes the current token start.
    pub fn accept_token_to(&mut self, token: u16, mark: InputMark) -> Result<(), ParseError> {
        if mark.stream_id != self.stream_id {
            return Err(ParseError::new(
                ParseErrorKind::Input,
                None,
                "token endpoint mark belongs to another input stream",
            ));
        }
        self.accept_at(token, mark.position)
    }

    fn accept_at(&mut self, token: u16, end: TextSize) -> Result<(), ParseError> {
        if end < self.token_start {
            return Err(ParseError::new(
                ParseErrorKind::Input,
                Some(end),
                "token end precedes its start",
            ));
        }
        self.accepted = Some(AcceptedToken { value: token, end });
        Ok(())
    }

    /// Read selected input between two UTF-8 byte positions.
    ///
    /// Identity-mapped input in one selected range is returned by reference.
    /// Translated or discontiguous input is materialized only when required.
    #[must_use]
    pub fn read_scalar(&self, from: TextSize, to: TextSize) -> Option<Cow<'_, str>> {
        self.read_scalar_impl(from, to, true)
    }

    pub(crate) fn read_scalar_at_boundaries(
        &self,
        from: TextSize,
        to: TextSize,
    ) -> Option<Cow<'_, str>> {
        if let [selected] = &*self.ranges
            && from <= to
            && selected.start() <= from
            && to <= selected.end()
        {
            return self.input.scalar_text(TextRange::new(from, to));
        }
        self.read_scalar_impl(from, to, false)
    }

    fn read_scalar_impl(
        &self,
        from: TextSize,
        to: TextSize,
        validate_boundaries: bool,
    ) -> Option<Cow<'_, str>> {
        if from > to {
            return None;
        }
        let mut selected = self
            .ranges
            .iter()
            .take_while(|range| range.start() < to)
            .filter_map(|range| {
                if range.end() <= from {
                    return None;
                }
                let start = range.start().max(from);
                let end = range.end().min(to);
                Some(TextRange::new(start, end))
            });
        let Some(first) = selected.next() else {
            return Some(Cow::Borrowed(""));
        };
        if validate_boundaries
            && (!self.input.is_boundary(first.start()) || !self.input.is_boundary(first.end()))
        {
            return None;
        }
        let first = self.input.scalar_text(first)?;
        let Some(second) = selected.next() else {
            return Some(first);
        };

        let capacity = to.checked_sub(from).unwrap_or(TextSize::from(0));
        let mut result = String::with_capacity(usize::from(capacity));
        result.push_str(&first);
        if validate_boundaries
            && (!self.input.is_boundary(second.start()) || !self.input.is_boundary(second.end()))
        {
            return None;
        }
        result.push_str(&self.input.scalar_text(second)?);
        for range in selected {
            if validate_boundaries
                && (!self.input.is_boundary(range.start()) || !self.input.is_boundary(range.end()))
            {
                return None;
            }
            result.push_str(&self.input.scalar_text(range)?);
        }
        Some(Cow::Owned(result))
    }

    pub(crate) fn reset(&mut self, position: TextSize) {
        self.accepted = None;
        if self.cursor.byte == position {
            self.token_start = position;
            return;
        }
        debug_assert!(self.input.is_boundary(position));
        let cursor = cursor_at_or_after(&self.ranges, position);
        self.token_start = cursor.byte;
        if self.cursor == cursor {
            return;
        }
        self.cursor = cursor;
        self.refresh_next();
    }

    pub(crate) fn clip_position(&self, position: TextSize) -> TextSize {
        cursor_at_or_after(&self.ranges, position).byte
    }

    pub(crate) fn accepted(&self) -> Option<AcceptedToken> {
        self.accepted
    }

    pub(crate) fn accepted_value(&self) -> Option<u16> {
        self.accepted.map(|accepted| accepted.value)
    }

    fn accept_current(&mut self, token: u16) {
        debug_assert!(self.position() >= self.token_start);
        self.accepted = Some(AcceptedToken {
            value: token,
            end: self.position(),
        });
    }

    pub(crate) fn next_position(&self) -> Option<TextSize> {
        if self.next_code_point.is_some_and(CodePoint::is_ascii) {
            let range = self.ranges.get(self.cursor.range_index)?;
            let byte = self.cursor.byte + TextSize::from(1);
            if byte < range.end() {
                return Some(byte);
            }
            return next_range_cursor(&self.ranges, self.cursor.range_index)
                .or_else(|| Some(end_cursor(&self.ranges)))
                .map(|cursor| cursor.byte);
        }
        advance_cursor(&*self.input, &self.ranges, self.cursor).map(|cursor| cursor.byte)
    }

    pub(crate) fn next_position_from(&self, position: TextSize) -> Option<TextSize> {
        let cursor = cursor_at_or_after(&self.ranges, position);
        advance_cursor(&*self.input, &self.ranges, cursor).map(|cursor| cursor.byte)
    }

    fn current_character(&mut self) -> Option<InputCharacter> {
        self.current_character_impl(true)
    }

    fn current_character_after_chunk_load(&mut self) -> Option<InputCharacter> {
        self.current_character_impl(false)
    }

    fn current_character_impl(&mut self, load_identity_chunk: bool) -> Option<InputCharacter> {
        if let Some(cached) = self.character
            && cached.range_index == self.cursor.range_index
            && cached.byte == self.cursor.byte
        {
            return Some(cached.character);
        }
        let range = *self.ranges.get(self.cursor.range_index)?;
        if self.cursor.byte < range.start() || self.cursor.byte >= range.end() {
            return None;
        }
        let mut character = self.chunk_character(range, self.cursor.byte);
        if character.is_none() && load_identity_chunk {
            self.load_identity_chunk(range, self.cursor.byte);
            character = self.chunk_character(range, self.cursor.byte);
        }
        let character =
            character.or_else(|| character_at(&*self.input, range, self.cursor.byte))?;
        self.character = Some(CachedCharacter {
            range_index: self.cursor.range_index,
            byte: self.cursor.byte,
            character,
        });
        Some(character)
    }

    #[inline]
    fn chunk_character(&self, range: TextRange, position: TextSize) -> Option<InputCharacter> {
        if position < range.start() || position >= range.end() {
            return None;
        }
        let character = self.chunk.as_ref()?.character(position)?;
        (character.raw_end() <= range.end()).then_some(character)
    }

    #[inline]
    fn chunk_ascii(&self, range: TextRange, position: TextSize) -> Option<u8> {
        if position < range.start() || position >= range.end() {
            return None;
        }
        self.chunk.as_ref()?.ascii_byte(position)
    }

    fn advance_current(&mut self) -> Option<StreamCursor> {
        let range = *self.ranges.get(self.cursor.range_index)?;
        if self.cursor.byte < range.start() || self.cursor.byte >= range.end() {
            return next_range_cursor(&self.ranges, self.cursor.range_index);
        }
        if self.chunk_ascii(range, self.cursor.byte).is_some() {
            let byte = self.cursor.byte + TextSize::from(1);
            if byte < range.end() {
                return Some(StreamCursor {
                    byte,
                    ..self.cursor
                });
            }
            return next_range_cursor(&self.ranges, self.cursor.range_index)
                .or_else(|| Some(end_cursor(&self.ranges)));
        }
        let byte = self.current_character()?.raw_end();
        if byte < range.end() {
            return Some(StreamCursor {
                byte,
                ..self.cursor
            });
        }
        next_range_cursor(&self.ranges, self.cursor.range_index)
            .or_else(|| Some(end_cursor(&self.ranges)))
    }

    fn refresh_next(&mut self) {
        let Some(range) = self.ranges.get(self.cursor.range_index).copied() else {
            self.next_code_point = None;
            return;
        };
        if !self.sync_window_position() {
            self.load_identity_chunk(range, self.cursor.byte);
        }
        if let Some(byte) = self
            .window
            .as_ref()
            .and_then(|window| window.source.as_bytes().get(self.window_source_position))
            .copied()
            .filter(u8::is_ascii)
        {
            self.next_code_point = Some(CodePoint::from(byte));
            return;
        }
        if let Some(byte) = self.chunk_ascii(range, self.cursor.byte) {
            self.next_code_point = Some(CodePoint::from(byte));
            return;
        }
        self.next_code_point = self
            .current_character_after_chunk_load()
            .map(InputCharacter::value);
    }
}

struct InputLookahead<'a> {
    input: &'a dyn LexicalInput,
    ranges: &'a [TextRange],
    cursor: Option<StreamCursor>,
    initial_chunk: Option<&'a InputChunk>,
    loaded_chunk: Option<InputChunk>,
    fast_window: Option<&'a FastWindow>,
    fast_bytes: &'a [u8],
    fast_byte_position: usize,
}

impl InputLookahead<'_> {
    fn ensure_chunk(&mut self, position: TextSize) {
        let has_loaded = self
            .loaded_chunk
            .as_ref()
            .is_some_and(|chunk| chunk.contains(position));
        let has_initial = self
            .initial_chunk
            .is_some_and(|chunk| chunk.contains(position));
        if !has_loaded && !has_initial {
            self.loaded_chunk = self.input.identity_chunk(position);
        }
    }

    fn chunk(&self, position: TextSize) -> Option<&InputChunk> {
        self.loaded_chunk
            .as_ref()
            .filter(|chunk| chunk.contains(position))
            .or_else(|| self.initial_chunk.filter(|chunk| chunk.contains(position)))
    }

    fn chunk_ascii(&mut self, position: TextSize) -> Option<u8> {
        if let Some(window) = self.fast_window
            && window.contains(position)
            && let Some(source_position) = window.source_position(position)
            && let Some(byte) = window.source.as_bytes().get(source_position).copied()
            && byte.is_ascii()
        {
            return Some(byte);
        }
        self.ensure_chunk(position);
        self.chunk(position)?.ascii_byte(position)
    }

    fn chunk_character(&mut self, range: TextRange, position: TextSize) -> Option<InputCharacter> {
        self.ensure_chunk(position);
        let chunk = self
            .chunk(position)
            .and_then(|chunk| chunk.character(position));
        let character = chunk.or_else(|| character_at(self.input, range, position))?;
        (character.raw_end() <= range.end()).then_some(character)
    }
}

impl Iterator for InputLookahead<'_> {
    type Item = CodePoint;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let cursor = self.cursor?;
            let range = *self.ranges.get(cursor.range_index)?;
            // The iterator's cursor is private, so consecutive identity-mapped
            // ASCII bytes can defer one cursor update until the run ends.
            if let Some(next) = self.fast_bytes.get(self.fast_byte_position).copied()
                && next.is_ascii()
            {
                self.fast_byte_position += 1;
                return Some(CodePoint::from(next));
            }
            if self.fast_byte_position != 0 {
                let consumed = TextSize::try_from(self.fast_byte_position)
                    .expect("an input window fits in TextSize");
                self.cursor = Some(StreamCursor {
                    byte: cursor.byte + consumed,
                    ..cursor
                });
                self.fast_bytes = &[];
                self.fast_byte_position = 0;
                continue;
            }
            self.fast_bytes = &[];
            if cursor.byte < range.start() || cursor.byte >= range.end() {
                self.cursor = next_range_cursor(self.ranges, cursor.range_index);
                continue;
            }
            if let Some(next) = self.chunk_ascii(cursor.byte) {
                let byte = cursor.byte + TextSize::from(1);
                self.cursor = if byte < range.end() {
                    Some(StreamCursor { byte, ..cursor })
                } else {
                    next_range_cursor(self.ranges, cursor.range_index)
                };
                return Some(CodePoint::from(next));
            }
            let character = self.chunk_character(range, cursor.byte)?;
            self.cursor = if character.raw_end() < range.end() {
                Some(StreamCursor {
                    byte: character.raw_end(),
                    ..cursor
                })
            } else {
                next_range_cursor(self.ranges, cursor.range_index)
            };
            return Some(character.value());
        }
    }
}

struct InputLookbehind<'a> {
    input: &'a dyn LexicalInput,
    ranges: &'a [TextRange],
    cursor: Option<StreamCursor>,
}

impl Iterator for InputLookbehind<'_> {
    type Item = CodePoint;

    fn next(&mut self) -> Option<Self::Item> {
        let Some((cursor, character)) =
            retreat_cursor_with_character(self.input, self.ranges, self.cursor?)
        else {
            self.cursor = None;
            return None;
        };
        self.cursor = Some(cursor);
        Some(character.value())
    }
}

fn first_cursor(ranges: &[TextRange]) -> StreamCursor {
    cursor_at_or_after(ranges, TextSize::from(0))
}

fn end_cursor(ranges: &[TextRange]) -> StreamCursor {
    StreamCursor {
        range_index: ranges.len().saturating_sub(1),
        byte: ranges.last().map_or(TextSize::from(0), |range| range.end()),
    }
}

fn cursor_at_or_after(ranges: &[TextRange], position: TextSize) -> StreamCursor {
    if let [range] = ranges {
        let byte = position.max(range.start());
        let byte = byte.min(range.end());
        return StreamCursor {
            range_index: 0,
            byte,
        };
    }
    let mut range_index = ranges.partition_point(|range| range.end() <= position);
    while ranges
        .get(range_index)
        .is_some_and(|range| range.is_empty())
    {
        range_index += 1;
    }
    let Some(range) = ranges.get(range_index) else {
        return end_cursor(ranges);
    };
    StreamCursor {
        range_index,
        byte: position.max(range.start()),
    }
}

fn character_at(
    input: &dyn LexicalInput,
    range: TextRange,
    position: TextSize,
) -> Option<InputCharacter> {
    if position < range.start() || position >= range.end() {
        return None;
    }
    let character = input.character(position)?;
    (character.raw_end() <= range.end()).then_some(character)
}

fn advance_cursor(
    input: &dyn LexicalInput,
    ranges: &[TextRange],
    cursor: StreamCursor,
) -> Option<StreamCursor> {
    let range = ranges.get(cursor.range_index)?;
    if cursor.byte < range.start() || cursor.byte >= range.end() {
        return next_range_cursor(ranges, cursor.range_index);
    }
    let byte = character_at(input, *range, cursor.byte)?.raw_end();
    if byte < range.end() {
        return Some(StreamCursor { byte, ..cursor });
    }
    next_range_cursor(ranges, cursor.range_index).or_else(|| Some(end_cursor(ranges)))
}

fn retreat_cursor_with_character(
    input: &dyn LexicalInput,
    ranges: &[TextRange],
    cursor: StreamCursor,
) -> Option<(StreamCursor, InputCharacter)> {
    let range = ranges.get(cursor.range_index)?;
    let (range_index, byte_limit) = if cursor.byte > range.start() {
        (cursor.range_index, cursor.byte)
    } else {
        let previous = (0..cursor.range_index)
            .rev()
            .find(|index| !ranges[*index].is_empty())?;
        (previous, ranges[previous].end())
    };
    let previous_range = ranges[range_index];
    let (byte, character) = input.character_before(byte_limit)?;
    if byte < previous_range.start() || character.raw_end() != byte_limit {
        return None;
    }
    Some((StreamCursor { range_index, byte }, character))
}

fn next_range_cursor(ranges: &[TextRange], range_index: usize) -> Option<StreamCursor> {
    let next = ((range_index + 1)..ranges.len()).find(|index| !ranges[*index].is_empty())?;
    Some(StreamCursor {
        range_index: next,
        byte: ranges[next].start(),
    })
}

fn next_stream_id() -> u64 {
    static NEXT_STREAM_ID: AtomicU64 = AtomicU64::new(1);

    NEXT_STREAM_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .expect("input stream identity space exhausted")
}

fn read_local_token(
    group: LocalTokenGroup,
    ascii_index: &TokenAsciiIndex,
    input: &mut InputStream,
    stack: &Stack,
) -> Result<(), ParseError> {
    let start = input.position();
    loop {
        let scan_start = input.position();
        let scan_end = input.mark();
        let next_position = input.next_position();
        read_token(
            group.table,
            Some(ascii_index),
            input,
            stack,
            0,
            group.precedence,
            0,
        );
        if input.accepted().is_some() {
            if scan_start > start {
                input.reset(start);
                input.accept_token_to(
                    group
                        .else_token
                        .expect("scanning continued only with @else"),
                    scan_end,
                )?;
            }
            return Ok(());
        }
        if group.else_token.is_none() {
            return Ok(());
        }
        let Some(next) = next_position else {
            return Ok(());
        };
        input.reset(next);
    }
}

fn read_token(
    table: &TokenTable,
    ascii_index: Option<&TokenAsciiIndex>,
    input: &mut InputStream,
    stack: &Stack,
    group: u8,
    precedence_data: &[u16],
    precedence_offset: usize,
) {
    if stack.has_disabled_dialect_terms() {
        read_token_inner::<true>(
            table,
            ascii_index,
            input,
            stack,
            group,
            precedence_data,
            precedence_offset,
        );
    } else {
        read_token_inner::<false>(
            table,
            ascii_index,
            input,
            stack,
            group,
            precedence_data,
            precedence_offset,
        );
    }
}

fn read_token_inner<const CHECK_DIALECT: bool>(
    table: &TokenTable,
    ascii_index: Option<&TokenAsciiIndex>,
    input: &mut InputStream,
    stack: &Stack,
    group: u8,
    precedence_data: &[u16],
    precedence_offset: usize,
) {
    let mut state = 0_usize;
    let group_mask = 1_u16 << group;
    'scan: loop {
        let state_data = table.states[state];
        if state_data.group_mask & group_mask == 0 {
            break;
        }
        for accept in table.accepts(state_data) {
            if accept.group_mask & group_mask != 0 {
                let term = accept.term;
                let current = input.accepted_value();
                let can_accept = (!CHECK_DIALECT || stack.dialect_allows(term))
                    && current.is_none_or(|previous| {
                        previous == term
                            || overrides(term, previous, precedence_data, precedence_offset)
                    });
                if can_accept {
                    input.accept_current(term);
                    break;
                }
            }
        }

        let next = input.next();
        let Some(next) = next else {
            if let Some(target) = table.eof_target(state) {
                state = target;
                continue;
            }
            break;
        };
        let next = next.as_u32();
        if next < 0x80
            && let Some(index) = ascii_index
        {
            let ascii = u8::try_from(next).expect("ASCII code point fits in u8");
            let Some(next_state) = index.transition(state, ascii) else {
                break;
            };
            if next_state == state && advance_ascii_self_loop(input, index, state) {
                continue;
            }
            state = next_state;
            input.advance_known_ascii(ascii);
            continue;
        }
        if let Some(next_state) = table.transition(state_data, next) {
            state = next_state;
            input.advance(1);
            continue 'scan;
        }
        break;
    }
}

#[inline]
fn advance_ascii_self_loop(input: &mut InputStream, index: &TokenAsciiIndex, state: usize) -> bool {
    input.advance_ascii_while(|byte| index.transition(state, byte) == Some(state)) != 0
}

fn find_offset(data: &[u16], start: usize, term: u16) -> Option<usize> {
    data.get(start..)?
        .iter()
        .take_while(|value| **value != SequenceCode::End.raw())
        .position(|value| *value == term)
}

fn overrides(token: u16, previous: u16, data: &[u16], offset: usize) -> bool {
    let previous_offset = find_offset(data, offset, previous);
    previous_offset.is_none() || find_offset(data, offset, token) < previous_offset
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use rezel_common::{Input, StringInput, Utf8Input};

    use super::*;

    struct CountingLexicalInput {
        inner: Utf8Input,
        character_reads: Arc<AtomicUsize>,
        character_before_reads: Arc<AtomicUsize>,
    }

    struct BoundaryTranslationInput {
        inner: Utf8Input,
    }

    impl LexicalInput for BoundaryTranslationInput {
        fn raw(&self) -> &dyn Input {
            self.inner.raw()
        }

        fn identity_chunk(&self, from: TextSize) -> Option<InputChunk> {
            if from == TextSize::from(2) {
                return None;
            }
            let mut chunk = self.inner.identity_chunk(from)?;
            if from < TextSize::from(2) {
                chunk.truncate(TextSize::from(2));
            }
            Some(chunk)
        }

        fn character(&self, from: TextSize) -> Option<InputCharacter> {
            if from == TextSize::from(2) {
                let surrogate = CodePoint::new(0xd800).expect("surrogate is a code point");
                return Some(InputCharacter::new(surrogate, TextSize::from(3)));
            }
            self.inner.character(from)
        }

        fn character_before(&self, before: TextSize) -> Option<(TextSize, InputCharacter)> {
            if before == TextSize::from(3) {
                let surrogate = CodePoint::new(0xd800).expect("surrogate is a code point");
                return Some((
                    TextSize::from(2),
                    InputCharacter::new(surrogate, TextSize::from(3)),
                ));
            }
            self.inner.character_before(before)
        }

        fn scalar_text(&self, range: TextRange) -> Option<Cow<'_, str>> {
            if range.start() <= TextSize::from(2) && TextSize::from(2) < range.end() {
                return None;
            }
            self.inner.scalar_text(range)
        }
    }

    impl LexicalInput for CountingLexicalInput {
        fn raw(&self) -> &dyn Input {
            self.inner.raw()
        }

        fn identity_chunk(&self, _from: TextSize) -> Option<InputChunk> {
            None
        }

        fn character(&self, from: TextSize) -> Option<InputCharacter> {
            self.character_reads.fetch_add(1, Ordering::Relaxed);
            self.inner.character(from)
        }

        fn character_before(&self, before: TextSize) -> Option<(TextSize, InputCharacter)> {
            self.character_before_reads.fetch_add(1, Ordering::Relaxed);
            self.inner.character_before(before)
        }
    }

    #[test]
    fn generated_token_state_stays_compact() {
        assert_eq!(std::mem::size_of::<TokenState>(), 8);
        assert_eq!(std::mem::align_of::<TokenState>(), 2);
        assert_eq!(std::mem::offset_of!(TokenState, group_mask), 0);
        assert_eq!(std::mem::offset_of!(TokenState, accept_start), 2);
        assert_eq!(std::mem::offset_of!(TokenState, edge_start), 4);
        assert_eq!(std::mem::offset_of!(TokenState, accept_count), 6);
        assert_eq!(std::mem::offset_of!(TokenState, edge_count), 7);

        assert_eq!(std::mem::size_of::<TokenAccept>(), 4);
        assert_eq!(std::mem::align_of::<TokenAccept>(), 2);
        assert_eq!(std::mem::offset_of!(TokenAccept, term), 0);
        assert_eq!(std::mem::offset_of!(TokenAccept, group_mask), 2);

        assert_eq!(std::mem::size_of::<TokenEof>(), 4);
        assert_eq!(std::mem::align_of::<TokenEof>(), 2);
        assert_eq!(std::mem::offset_of!(TokenEof, state), 0);
        assert_eq!(std::mem::offset_of!(TokenEof, target), 2);

        assert_eq!(std::mem::size_of::<TokenEdge>(), 12);
        assert_eq!(std::mem::align_of::<TokenEdge>(), 4);
        assert_eq!(std::mem::offset_of!(TokenEdge, from), 0);
        assert_eq!(std::mem::offset_of!(TokenEdge, to), 4);
        assert_eq!(std::mem::offset_of!(TokenEdge, target), 8);
    }

    #[test]
    fn external_tokenizer_start_matches_declared_code_points() {
        let start = ExternalTokenizerStart::NONE
            .with_ascii(b'$')
            .with_ascii_range(b'A'..=b'Z')
            .with_non_ascii()
            .with_end();

        for byte in 0_u8..0x80 {
            let expected = byte == b'$' || byte.is_ascii_uppercase();
            assert_eq!(
                start.matches(Some(CodePoint::from(byte))),
                expected,
                "ASCII {byte:#04x}"
            );
        }
        assert!(start.matches(Some(CodePoint::from('µ'))));
        assert!(start.matches(None));

        let ascii_only = ExternalTokenizerStart::NONE.with_ascii(b'_');
        assert!(ascii_only.matches(Some(CodePoint::from(b'_'))));
        assert!(!ascii_only.matches(Some(CodePoint::from('µ'))));
        assert!(!ascii_only.matches(None));
    }

    #[test]
    fn indexes_external_tokenizer_starts_before_dispatch() {
        fn accept_zero(input: &mut InputStream, _stack: &Stack) -> Result<(), ParseError> {
            input.accept_token(0)
        }

        const FLAGS: TokenizerFlags = TokenizerFlags {
            contextual: false,
            fallback: false,
            extend: false,
        };
        static UNFILTERED: ExternalTokenizer = ExternalTokenizer::new(accept_zero, FLAGS);
        static ASCII: ExternalTokenizer = ExternalTokenizer::new(accept_zero, FLAGS)
            .with_start(ExternalTokenizerStart::NONE.with_ascii(b'$'));
        static NON_ASCII: ExternalTokenizer = ExternalTokenizer::new(accept_zero, FLAGS)
            .with_start(ExternalTokenizerStart::NONE.with_non_ascii());
        static END: ExternalTokenizer = ExternalTokenizer::new(accept_zero, FLAGS)
            .with_start(ExternalTokenizerStart::NONE.with_end());

        let tokenizers = [
            Tokenizer::External(&UNFILTERED),
            Tokenizer::External(&ASCII),
            Tokenizer::External(&NON_ASCII),
            Tokenizer::External(&END),
        ];
        let index = TokenizerStartIndex::build(&tokenizers);
        let all = 0b1111;

        assert!(!index.has_filtered(0b0001));
        assert!(index.has_filtered(0b0010));
        assert_eq!(index.filter(all, Some(CodePoint::from(b'$'))), 0b0011);
        assert_eq!(index.filter(all, Some(CodePoint::from(b'a'))), 0b0001);
        assert_eq!(index.filter(all, Some(CodePoint::from('µ'))), 0b0101);
        assert_eq!(index.filter(all, None), 0b1001);
        assert_eq!(index.filter(0b1010, None), 0b1000);
    }

    #[test]
    fn indexes_ascii_token_transitions() {
        static STATES: &[TokenState] = &[
            TokenState {
                group_mask: 1,
                accept_start: 0,
                edge_start: 0,
                accept_count: 0,
                edge_count: 1,
            },
            TokenState {
                group_mask: 1,
                accept_start: 0,
                edge_start: 1,
                accept_count: 0,
                edge_count: 1,
            },
        ];
        static EDGES: &[TokenEdge] = &[
            TokenEdge {
                from: b'A' as u32,
                to: b'Z' as u32 + 1,
                target: 1,
            },
            TokenEdge {
                from: b'a' as u32,
                to: b'z' as u32 + 1,
                target: 1,
            },
        ];
        static TABLE: TokenTable = TokenTable::new(STATES, &[], EDGES, &[]);
        let index = TokenAsciiIndex::build(&TABLE);

        assert_eq!(index.transition(0, b'A'), Some(1));
        assert_eq!(index.transition(0, b'Z'), Some(1));
        assert_eq!(index.transition(1, b'a'), Some(1));
        assert_eq!(index.transition(1, b'z'), Some(1));
        assert_eq!(index.transition(0, b'a'), None);
    }

    #[test]
    fn optimized_transitions_match_linear_edges() {
        static STATES: &[TokenState] = &[TokenState {
            group_mask: 1,
            accept_start: 0,
            edge_start: 0,
            accept_count: 0,
            edge_count: 5,
        }];
        static EDGES: &[TokenEdge] = &[
            TokenEdge {
                from: 0,
                to: 10,
                target: 1,
            },
            TokenEdge {
                from: 10,
                to: 0x80,
                target: 2,
            },
            TokenEdge {
                from: 0x80,
                to: 0xd800,
                target: 3,
            },
            TokenEdge {
                from: 0xe000,
                to: 0x10_ffff,
                target: 4,
            },
            TokenEdge {
                from: 0x10_ffff,
                to: 0x11_0000,
                target: 5,
            },
        ];
        static TABLE: TokenTable = TokenTable::new(STATES, &[], EDGES, &[]);
        let state = STATES[0];
        let ascii = TokenAsciiIndex::build(&TABLE);

        for next in 0..=CodePoint::MAX {
            let expected = EDGES
                .iter()
                .find(|edge| edge.from <= next && next < edge.to)
                .map(|edge| usize::from(edge.target));
            assert_eq!(TABLE.transition(state, next), expected, "U+{next:04X}");
            if next < 0x80 {
                assert_eq!(
                    ascii.transition(0, u8::try_from(next).unwrap()),
                    expected,
                    "ASCII {next}"
                );
            }
        }
    }

    #[test]
    fn bulk_ascii_self_loop_stops_before_a_different_transition() {
        static STATES: &[TokenState] = &[
            TokenState {
                group_mask: 1,
                accept_start: 0,
                edge_start: 0,
                accept_count: 0,
                edge_count: 1,
            },
            TokenState {
                group_mask: 1,
                accept_start: 0,
                edge_start: 1,
                accept_count: 0,
                edge_count: 2,
            },
            TokenState {
                group_mask: 1,
                accept_start: 0,
                edge_start: 3,
                accept_count: 0,
                edge_count: 0,
            },
        ];
        static EDGES: &[TokenEdge] = &[
            TokenEdge {
                from: b'a' as u32,
                to: b'z' as u32 + 1,
                target: 1,
            },
            TokenEdge {
                from: b'!' as u32,
                to: b'!' as u32 + 1,
                target: 2,
            },
            TokenEdge {
                from: b'a' as u32,
                to: b'z' as u32 + 1,
                target: 1,
            },
        ];
        static TABLE: TokenTable = TokenTable::new(STATES, &[], EDGES, &[]);
        let index = TokenAsciiIndex::build(&TABLE);
        let ranges = Arc::from([TextRange::new(0.into(), 5.into())]);
        let mut input = stream("abc!x", ranges);

        assert_eq!(index.transition(0, b'a'), Some(1));
        input.advance(1);
        assert!(advance_ascii_self_loop(&mut input, &index, 1));
        assert_eq!(input.position(), TextSize::from(3));
        assert_eq!(input.next(), Some(CodePoint::from(b'!')));
        assert_eq!(index.transition(1, b'!'), Some(2));
        assert!(!advance_ascii_self_loop(&mut input, &index, 1));
        assert_eq!(input.position(), TextSize::from(3));
    }

    fn stream(source: &str, ranges: impl Into<Arc<[TextRange]>>) -> InputStream {
        let raw: Arc<dyn Input> = Arc::new(StringInput::try_new(source).unwrap());
        let input: Arc<dyn LexicalInput> = Arc::new(Utf8Input::new(raw));
        InputStream::new(input, ranges.into())
    }

    #[test]
    fn identity_chunk_stops_at_the_final_selected_range() {
        let ranges = Arc::from([TextRange::new(0.into(), 2.into())]);
        let mut input = stream("abX", ranges);

        assert_eq!(input.next(), Some(CodePoint::from(b'a')));
        assert_eq!(input.advance(1), Some(CodePoint::from(b'b')));
        assert_eq!(input.advance(1), None);
        assert_eq!(input.position(), TextSize::from(2));
        assert_eq!(input.next(), None);
    }

    #[test]
    fn single_selected_range_clips_positions_at_its_boundaries() {
        let ranges = Arc::from([TextRange::new(2.into(), 5.into())]);
        let input = stream("XXabcY", ranges);

        assert_eq!(input.position(), TextSize::from(2));
        assert_eq!(input.next(), Some(CodePoint::from(b'a')));
        assert_eq!(input.clip_position(0.into()), TextSize::from(2));
        assert_eq!(input.clip_position(3.into()), TextSize::from(3));
        assert_eq!(input.clip_position(6.into()), TextSize::from(5));
        assert_eq!(input.next_position_from(0.into()), Some(TextSize::from(3)));
        assert_eq!(input.next_position_from(5.into()), None);

        let ranges = Arc::from([TextRange::new(2.into(), 2.into())]);
        let empty = stream("XX", ranges);
        assert_eq!(empty.position(), TextSize::from(2));
        assert_eq!(empty.next(), None);
        assert_eq!(empty.clip_position(0.into()), TextSize::from(2));
        assert_eq!(empty.clip_position(3.into()), TextSize::from(2));
    }

    #[test]
    fn next_position_reuses_ascii_and_preserves_selected_range_boundaries() {
        let character_reads = Arc::new(AtomicUsize::new(0));
        let raw: Arc<dyn Input> = Arc::new(StringInput::try_new("ab").unwrap());
        let lexical: Arc<dyn LexicalInput> = Arc::new(CountingLexicalInput {
            inner: Utf8Input::new(raw),
            character_reads: Arc::clone(&character_reads),
            character_before_reads: Arc::new(AtomicUsize::new(0)),
        });
        let input = InputStream::new(lexical, Arc::from([TextRange::new(0.into(), 2.into())]));

        assert_eq!(character_reads.load(Ordering::Relaxed), 1);
        assert_eq!(input.next_position(), Some(TextSize::from(1)));
        assert_eq!(character_reads.load(Ordering::Relaxed), 1);

        let ranges = Arc::from([
            TextRange::new(0.into(), 1.into()),
            TextRange::new(6.into(), 7.into()),
        ]);
        let mut selected = stream("a😀Xb", ranges);
        assert_eq!(selected.next_position(), Some(TextSize::from(6)));
        selected.advance(1);
        assert_eq!(selected.next_position(), Some(TextSize::from(7)));

        let unicode = stream("a😀", Arc::from([TextRange::new(1.into(), 5.into())]));
        assert_eq!(unicode.next_position(), Some(TextSize::from(5)));
    }

    #[test]
    fn known_ascii_advance_preserves_windows_translation_and_fallback() {
        let ranges = Arc::from([
            TextRange::new(0.into(), 1.into()),
            TextRange::new(2.into(), 3.into()),
        ]);
        let mut selected = stream("aXb", ranges);

        selected.advance_known_ascii(b'a');
        assert_eq!(selected.position(), TextSize::from(2));
        assert_eq!(selected.next(), Some(CodePoint::from(b'b')));
        selected.advance_known_ascii(b'b');
        assert_eq!(selected.position(), TextSize::from(3));
        assert_eq!(selected.next(), None);

        let raw: Arc<dyn Input> = Arc::new(StringInput::try_new("abXc").unwrap());
        let input: Arc<dyn LexicalInput> = Arc::new(BoundaryTranslationInput {
            inner: Utf8Input::new(raw),
        });
        let ranges = Arc::from([TextRange::new(0.into(), 4.into())]);
        let mut translated = InputStream::new(input, ranges);

        translated.advance_known_ascii(b'a');
        translated.advance_known_ascii(b'b');
        assert_eq!(translated.position(), TextSize::from(2));
        assert_eq!(
            translated.next(),
            Some(CodePoint::new(0xd800).expect("surrogate is a code point"))
        );

        let raw: Arc<dyn Input> = Arc::new(StringInput::try_new("ab").unwrap());
        let input: Arc<dyn LexicalInput> = Arc::new(CountingLexicalInput {
            inner: Utf8Input::new(raw),
            character_reads: Arc::new(AtomicUsize::new(0)),
            character_before_reads: Arc::new(AtomicUsize::new(0)),
        });
        let ranges = Arc::from([TextRange::new(0.into(), 2.into())]);
        let mut fallback = InputStream::new(input, ranges);

        fallback.advance_known_ascii(b'a');
        assert_eq!(fallback.position(), TextSize::from(1));
        assert_eq!(fallback.next(), Some(CodePoint::from(b'b')));
    }

    #[test]
    fn prevalidated_scalar_reads_preserve_translation_and_selected_ranges() {
        let raw: Arc<dyn Input> = Arc::new(StringInput::try_new("abXc").unwrap());
        let input: Arc<dyn LexicalInput> = Arc::new(BoundaryTranslationInput {
            inner: Utf8Input::new(raw),
        });
        let ranges = Arc::from([TextRange::new(0.into(), 4.into())]);
        let translated = InputStream::new(input, ranges);

        assert_eq!(
            translated
                .read_scalar_at_boundaries(0.into(), 2.into())
                .as_deref(),
            Some("ab")
        );
        assert!(
            translated
                .read_scalar_at_boundaries(0.into(), 3.into())
                .is_none()
        );

        let ranges = Arc::from([
            TextRange::new(0.into(), 2.into()),
            TextRange::new(3.into(), 5.into()),
        ]);
        let stream = stream("abXcd", ranges);
        assert_eq!(
            stream
                .read_scalar_at_boundaries(0.into(), 5.into())
                .as_deref(),
            Some("abcd")
        );
    }

    #[test]
    fn lookahead_fast_window_stops_at_an_adjacent_translation_boundary() {
        let raw: Arc<dyn Input> = Arc::new(StringInput::try_new("abXc").unwrap());
        let input: Arc<dyn LexicalInput> = Arc::new(BoundaryTranslationInput {
            inner: Utf8Input::new(raw),
        });
        let ranges = Arc::from([
            TextRange::new(0.into(), 2.into()),
            TextRange::new(2.into(), 4.into()),
        ]);
        let stream = InputStream::new(input, ranges);

        assert_eq!(
            stream.lookahead().collect::<Vec<_>>(),
            vec![
                CodePoint::from(b'a'),
                CodePoint::from(b'b'),
                CodePoint::new(0xd800).unwrap(),
                CodePoint::from(b'c'),
            ]
        );
        assert_eq!(stream.identity_lookahead_chunk(), b"ab");
    }

    #[test]
    fn identity_lookahead_chunk_stops_at_each_selected_range() {
        let ranges = Arc::from([
            TextRange::new(0.into(), 2.into()),
            TextRange::new(3.into(), 5.into()),
        ]);
        let mut input = stream("abXcd", ranges);

        assert_eq!(input.identity_lookahead_chunk(), b"ab");
        input.advance(2);
        assert_eq!(input.position(), TextSize::from(3));
        assert_eq!(input.identity_lookahead_chunk(), b"cd");
        input.advance(2);
        assert!(input.identity_lookahead_chunk().is_empty());
    }

    #[test]
    fn one_advance_crosses_an_entire_utf8_scalar() {
        let ranges = Arc::from([TextRange::new(0.into(), 5.into())]);
        let mut input = stream("😀a", ranges);

        assert_eq!(input.next(), Some(CodePoint::from('😀')));
        assert_eq!(input.advance(1), Some(CodePoint::from(b'a')));
        assert_eq!(input.position(), TextSize::from(4));

        input.reset(0.into());

        assert_eq!(input.next(), Some(CodePoint::from('😀')));
    }

    #[test]
    fn resets_reuse_identity_windows_and_preserve_translation_boundaries() {
        let ranges = Arc::from([
            TextRange::new(0.into(), 2.into()),
            TextRange::new(3.into(), 5.into()),
        ]);
        let mut selected = stream("abXcd", ranges);
        selected.advance(3);
        selected.reset(1.into());
        assert_eq!(selected.next(), Some(CodePoint::from(b'b')));
        selected.reset(2.into());
        assert_eq!(selected.position(), TextSize::from(3));
        assert_eq!(selected.next(), Some(CodePoint::from(b'c')));

        let raw: Arc<dyn Input> = Arc::new(StringInput::try_new("abXc").unwrap());
        let input: Arc<dyn LexicalInput> = Arc::new(BoundaryTranslationInput {
            inner: Utf8Input::new(raw),
        });
        let ranges = Arc::from([TextRange::new(0.into(), 4.into())]);
        let mut translated = InputStream::new(input, ranges);
        translated.advance(3);
        translated.reset(1.into());
        assert_eq!(translated.next(), Some(CodePoint::from(b'b')));
        translated.reset(2.into());
        assert_eq!(
            translated.next(),
            Some(CodePoint::new(0xd800).expect("surrogate is a code point"))
        );
        translated.reset(3.into());
        assert_eq!(translated.next(), Some(CodePoint::from(b'c')));
    }

    #[test]
    fn lookahead_preserves_code_points_and_selected_ranges() {
        let ranges = Arc::from([
            TextRange::new(0.into(), 5.into()),
            TextRange::new(6.into(), 7.into()),
        ]);
        let mut input = stream("a😀Xb", ranges);

        assert_eq!(
            input.lookahead().collect::<Vec<_>>(),
            vec![
                CodePoint::from(b'a'),
                CodePoint::from('😀'),
                CodePoint::from(b'b')
            ]
        );

        input.advance(1);
        assert_eq!(
            input.lookahead().collect::<Vec<_>>(),
            vec![CodePoint::from('😀'), CodePoint::from(b'b')]
        );
    }

    #[test]
    fn peek_preserves_ascii_unicode_translation_and_selected_ranges() {
        let input = stream("ab😀c", Arc::from([TextRange::new(0.into(), 7.into())]));
        assert_eq!(input.peek(1), Some(CodePoint::from(b'b')));
        assert_eq!(input.peek(2), Some(CodePoint::from('😀')));
        assert_eq!(input.peek(3), Some(CodePoint::from(b'c')));

        let ranges = Arc::from([
            TextRange::new(0.into(), 1.into()),
            TextRange::new(6.into(), 8.into()),
        ]);
        let selected = stream("a😀Xbc", ranges);
        assert_eq!(selected.peek(1), Some(CodePoint::from(b'b')));
        assert_eq!(selected.peek(2), Some(CodePoint::from(b'c')));

        let raw: Arc<dyn Input> = Arc::new(StringInput::try_new("abXc").unwrap());
        let lexical: Arc<dyn LexicalInput> = Arc::new(BoundaryTranslationInput {
            inner: Utf8Input::new(raw),
        });
        let translated = InputStream::new(lexical, Arc::from([TextRange::new(0.into(), 4.into())]));
        assert_eq!(
            translated.peek(2),
            Some(CodePoint::new(0xd800).expect("surrogate is a code point"))
        );
        assert_eq!(translated.peek(3), Some(CodePoint::from(b'c')));
    }

    #[test]
    fn empty_selected_ranges_never_expose_omitted_input() {
        let ranges = Arc::from([
            TextRange::new(0.into(), 1.into()),
            TextRange::new(2.into(), 2.into()),
            TextRange::new(3.into(), 4.into()),
        ]);
        let mut input = stream("aXYb", ranges);

        assert_eq!(input.next(), Some(CodePoint::from(b'a')));
        assert_eq!(input.clip_position(1.into()), TextSize::from(3));
        assert_eq!(input.next_position_from(1.into()), Some(TextSize::from(4)));
        input.reset(1.into());
        assert_eq!(input.position(), TextSize::from(3));
        assert_eq!(input.next(), Some(CodePoint::from(b'b')));
    }

    #[test]
    fn all_empty_selected_ranges_are_logically_empty() {
        let ranges = Arc::from([
            TextRange::new(0.into(), 0.into()),
            TextRange::new(2.into(), 2.into()),
            TextRange::new(4.into(), 4.into()),
        ]);
        let mut input = stream("aXYb", ranges);

        assert_eq!(input.position(), TextSize::from(4));
        assert_eq!(input.clip_position(0.into()), TextSize::from(4));
        assert_eq!(input.next(), None);
        assert_eq!(input.lookahead().next(), None);
        assert_eq!(input.lookbehind().next(), None);
        assert_eq!(input.next_position_from(0.into()), None);
        input.reset(0.into());
        assert_eq!(input.position(), TextSize::from(4));
        assert_eq!(input.next(), None);
    }

    #[test]
    fn lookbehind_preserves_code_points_selected_ranges_and_position() {
        let ranges = Arc::from([
            TextRange::new(0.into(), 5.into()),
            TextRange::new(6.into(), 7.into()),
        ]);
        let mut input = stream("a😀Xb", ranges);

        assert!(input.lookbehind().next().is_none());
        input.advance(3);
        assert_eq!(input.position(), TextSize::from(7));
        assert_eq!(
            input.lookbehind().collect::<Vec<_>>(),
            vec![
                CodePoint::from(b'b'),
                CodePoint::from('😀'),
                CodePoint::from(b'a')
            ]
        );
        assert_eq!(input.position(), TextSize::from(7));
        assert_eq!(input.next(), None);

        input.reset(1.into());
        assert_eq!(input.next(), Some(CodePoint::from('😀')));
        let mut lookbehind = input.lookbehind();
        assert_eq!(lookbehind.next(), Some(CodePoint::from(b'a')));
        assert_eq!(lookbehind.next(), None);
        assert_eq!(lookbehind.next(), None);
        assert_eq!(input.position(), TextSize::from(1));
    }

    #[test]
    fn sequential_lookahead_resolves_input_linearly() {
        const WIDTH: usize = 128;

        let source: Arc<str> = " ".repeat(WIDTH + 1).into();
        let character_reads = Arc::new(AtomicUsize::new(0));
        let character_before_reads = Arc::new(AtomicUsize::new(0));
        let raw: Arc<dyn Input> = Arc::new(StringInput::try_new(source).unwrap());
        let input: Arc<dyn LexicalInput> = Arc::new(CountingLexicalInput {
            inner: Utf8Input::new(raw),
            character_reads: Arc::clone(&character_reads),
            character_before_reads,
        });
        let ranges = Arc::from([TextRange::new(
            TextSize::from(0),
            TextSize::try_from(WIDTH + 1).unwrap(),
        )]);
        let stream = InputStream::new(input, ranges);

        assert_eq!(stream.lookahead().count(), WIDTH + 1);
        assert!(character_reads.load(Ordering::Relaxed) <= WIDTH + 2);
    }

    #[test]
    fn sequential_lookbehind_resolves_input_linearly() {
        const WIDTH: usize = 128;

        let source: Arc<str> = " ".repeat(WIDTH).into();
        let character_reads = Arc::new(AtomicUsize::new(0));
        let character_before_reads = Arc::new(AtomicUsize::new(0));
        let raw: Arc<dyn Input> = Arc::new(StringInput::try_new(source).unwrap());
        let input: Arc<dyn LexicalInput> = Arc::new(CountingLexicalInput {
            inner: Utf8Input::new(raw),
            character_reads: Arc::clone(&character_reads),
            character_before_reads: Arc::clone(&character_before_reads),
        });
        let ranges = Arc::from([TextRange::new(
            TextSize::from(0),
            TextSize::try_from(WIDTH).unwrap(),
        )]);
        let mut stream = InputStream::new(input, ranges);
        stream.advance(WIDTH);
        character_reads.store(0, Ordering::Relaxed);

        assert_eq!(stream.lookbehind().count(), WIDTH);
        assert!(character_before_reads.load(Ordering::Relaxed) <= WIDTH + 1);
        assert_eq!(character_reads.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn previous_preserves_ascii_unicode_and_selected_range_boundaries() {
        let ranges = Arc::from([
            TextRange::new(0.into(), 5.into()),
            TextRange::new(6.into(), 7.into()),
        ]);
        let mut stream = stream("a😀Xb", ranges);

        assert_eq!(stream.previous(), None);
        stream.advance(1);
        assert_eq!(stream.previous(), Some(CodePoint::from(b'a')));
        stream.advance(1);
        assert_eq!(stream.previous(), Some(CodePoint::from('😀')));
        stream.advance(1);
        assert_eq!(stream.previous(), Some(CodePoint::from(b'b')));

        let raw: Arc<dyn Input> = Arc::new(StringInput::try_new("abXc").unwrap());
        let input: Arc<dyn LexicalInput> = Arc::new(BoundaryTranslationInput {
            inner: Utf8Input::new(raw),
        });
        let ranges = Arc::from([TextRange::new(0.into(), 4.into())]);
        let mut translated = InputStream::new(input, ranges);
        translated.advance(2);
        assert_eq!(translated.previous(), Some(CodePoint::from(b'b')));
        translated.advance(1);
        assert_eq!(
            translated.previous(),
            Some(CodePoint::new(0xd800).expect("surrogate is a code point"))
        );
    }

    #[test]
    fn bulk_ascii_advance_stops_at_logical_and_selected_range_boundaries() {
        let raw: Arc<dyn Input> =
            Arc::new(StringInput::try_new(Arc::<str>::from("abcédef")).unwrap());
        let input: Arc<dyn LexicalInput> = Arc::new(Utf8Input::new(raw));
        let ranges = Arc::from([TextRange::new(0.into(), 8.into())]);
        let mut stream = InputStream::new(input, ranges);

        let advance = stream.advance_ascii_while_with_stop(|byte| byte.is_ascii_alphabetic());
        assert_eq!(advance.count(), 3);
        assert!(!advance.stopped_on_mismatch());
        assert_eq!(stream.position(), TextSize::from(3));
        assert_eq!(stream.next(), Some(CodePoint::from('é')));
        stream.advance(1);
        assert_eq!(
            stream.advance_ascii_while(|byte| byte.is_ascii_alphabetic()),
            3
        );
        assert_eq!(stream.position(), TextSize::from(8));

        let raw: Arc<dyn Input> =
            Arc::new(StringInput::try_new(Arc::<str>::from("abXcd")).unwrap());
        let input: Arc<dyn LexicalInput> = Arc::new(Utf8Input::new(raw));
        let ranges = Arc::from([
            TextRange::new(0.into(), 2.into()),
            TextRange::new(3.into(), 5.into()),
        ]);
        let mut stream = InputStream::new(input, ranges);
        let advance = stream.advance_ascii_while_with_stop(|byte| byte.is_ascii_alphabetic());
        assert_eq!(advance.count(), 2);
        assert!(!advance.stopped_on_mismatch());
        assert_eq!(stream.position(), TextSize::from(3));
        assert_eq!(
            stream.advance_ascii_while(|byte| byte.is_ascii_alphabetic()),
            2
        );
        assert_eq!(stream.position(), TextSize::from(5));

        let raw: Arc<dyn Input> = Arc::new(StringInput::try_new("abc+def").unwrap());
        let input: Arc<dyn LexicalInput> = Arc::new(Utf8Input::new(raw));
        let ranges = Arc::from([TextRange::new(0.into(), 7.into())]);
        let mut stream = InputStream::new(input, ranges);
        let advance = stream.advance_ascii_while_with_stop(|byte| byte.is_ascii_alphabetic());
        assert_eq!(advance.count(), 3);
        assert!(advance.stopped_on_mismatch());
        assert_eq!(stream.next(), Some(CodePoint::from(b'+')));
    }

    #[test]
    fn marks_support_reset_and_discontiguous_selected_ranges() {
        let ranges = Arc::from([
            TextRange::new(0.into(), 2.into()),
            TextRange::new(3.into(), 5.into()),
        ]);
        let mut input = stream("abXcd", ranges);
        let start = input.mark();

        input.advance(3);
        let across_ranges = input.mark();
        assert_eq!(input.position(), TextSize::from(4));

        input.reset(0.into());
        input.accept_token_to(7, across_ranges).unwrap();
        assert_eq!(
            input.accepted(),
            Some(AcceptedToken {
                value: 7,
                end: TextSize::from(4),
            })
        );

        input.reset(3.into());
        let error = input.accept_token_to(7, start).unwrap_err();
        assert_eq!(error.kind(), ParseErrorKind::Input);
    }

    #[test]
    fn marks_cannot_cross_input_streams() {
        let ranges = Arc::from([TextRange::new(0.into(), 1.into())]);
        let first = stream("a", Arc::clone(&ranges));
        let mut second = stream("b", ranges);

        let error = second.accept_token_to(1, first.mark()).unwrap_err();

        assert_eq!(error.kind(), ParseErrorKind::Input);
        assert_eq!(
            error.message(),
            "token endpoint mark belongs to another input stream"
        );
    }
}
