use std::borrow::Cow;
use std::error::Error;
use std::fmt;
use std::sync::Arc;

use crate::{TextRange, TextSize, Tree};

/// Broad category of parser failure.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ParseErrorKind {
    /// Strict-mode syntax rejection.
    Syntax,
    /// Invalid parser configuration or generated tables.
    Configuration,
    /// A configured resource budget was exhausted.
    ResourceLimit,
    /// Input or tokenizer contract violation.
    Input,
}

/// Fatal parse failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    kind: ParseErrorKind,
    position: Option<TextSize>,
    message: String,
}

impl ParseError {
    /// Construct a parse error.
    #[must_use]
    pub fn new(
        kind: ParseErrorKind,
        position: Option<TextSize>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            position,
            message: message.into(),
        }
    }

    /// Error category.
    #[must_use]
    pub const fn kind(&self) -> ParseErrorKind {
        self.kind
    }

    /// Relevant UTF-8 byte position, when available.
    #[must_use]
    pub const fn position(&self) -> Option<TextSize> {
        self.position
    }

    /// Human-readable detail.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(position) = self.position {
            write!(
                formatter,
                "{} at byte {}",
                self.message,
                u32::from(position)
            )
        } else {
            formatter.write_str(&self.message)
        }
    }
}

impl Error for ParseError {}

/// One Unicode code point, including values in the surrogate range.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CodePoint(u32);

impl CodePoint {
    /// Largest Unicode code point.
    pub const MAX: u32 = 0x10_ffff;

    /// Construct a code point.
    #[must_use]
    pub const fn new(value: u32) -> Option<Self> {
        if value <= Self::MAX {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Numeric code-point value.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }

    /// Convert a Unicode scalar value to `char`.
    #[must_use]
    pub const fn as_char(self) -> Option<char> {
        char::from_u32(self.0)
    }

    /// Whether this code point is ASCII.
    #[must_use]
    pub const fn is_ascii(self) -> bool {
        self.0 <= 0x7f
    }

    /// Whether this code point is a Unicode scalar value.
    #[must_use]
    pub const fn is_scalar(self) -> bool {
        self.0 < 0xd800 || self.0 > 0xdfff
    }

    /// Whether this code point is a UTF-16 surrogate.
    #[must_use]
    pub const fn is_surrogate(self) -> bool {
        !self.is_scalar()
    }
}

impl From<u8> for CodePoint {
    fn from(value: u8) -> Self {
        Self(u32::from(value))
    }
}

impl From<u16> for CodePoint {
    fn from(value: u16) -> Self {
        Self(u32::from(value))
    }
}

impl From<char> for CodePoint {
    fn from(value: char) -> Self {
        Self(u32::from(value))
    }
}

/// One logical code point and its original byte boundary.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputCharacter {
    value: CodePoint,
    raw_end: TextSize,
}

impl InputCharacter {
    /// Construct one logical input character.
    #[must_use]
    #[inline]
    pub const fn new(value: CodePoint, raw_end: TextSize) -> Self {
        Self { value, raw_end }
    }

    /// Logical code point.
    #[must_use]
    #[inline]
    pub const fn value(self) -> CodePoint {
        self.value
    }

    /// Original UTF-8 byte boundary after this logical character.
    #[must_use]
    #[inline]
    pub const fn raw_end(self) -> TextSize {
        self.raw_end
    }
}

/// Shared UTF-8 input segment whose bytes map one-to-one to source positions.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct InputChunk {
    source: Arc<str>,
    source_range: TextRange,
    raw_start: TextSize,
}

impl InputChunk {
    /// Construct one identity-mapped chunk.
    ///
    /// # Panics
    ///
    /// Panics when the source range is invalid or does not use UTF-8
    /// boundaries.
    #[must_use]
    pub fn new(source: Arc<str>, source_range: TextRange, raw_start: TextSize) -> Self {
        let source_start = usize::from(source_range.start());
        let source_end = usize::from(source_range.end());
        assert!(
            source_end <= source.len(),
            "input chunk extends past its source"
        );
        assert!(
            source.is_char_boundary(source_start) && source.is_char_boundary(source_end),
            "input chunk must use UTF-8 boundaries"
        );
        assert!(
            raw_start.checked_add(source_range.len()).is_some(),
            "input chunk raw range overflows"
        );
        Self {
            source,
            source_range,
            raw_start,
        }
    }

    /// First original byte position represented by this chunk.
    #[must_use]
    #[inline]
    pub const fn raw_start(&self) -> TextSize {
        self.raw_start
    }

    /// Exclusive original byte position represented by this chunk.
    #[must_use]
    #[inline]
    pub fn raw_end(&self) -> TextSize {
        self.raw_start + self.source_range.len()
    }

    /// Whether this chunk contains an original byte position.
    #[must_use]
    #[inline]
    pub fn contains(&self, position: TextSize) -> bool {
        self.raw_start <= position && position < self.raw_end()
    }

    /// Whether a raw position is a UTF-8 boundary in this chunk.
    #[must_use]
    #[inline]
    pub fn is_boundary(&self, position: TextSize) -> bool {
        let Some(offset) = position.checked_sub(self.raw_start) else {
            return false;
        };
        let Some(source_position) = self.source_range.start().checked_add(offset) else {
            return false;
        };
        source_position <= self.source_range.end()
            && self.source.is_char_boundary(usize::from(source_position))
    }

    /// Read one directly mapped ASCII byte from this chunk.
    #[must_use]
    #[inline]
    pub fn ascii_byte(&self, position: TextSize) -> Option<u8> {
        let offset = position.checked_sub(self.raw_start)?;
        let source_position = self.source_range.start().checked_add(offset)?;
        if source_position >= self.source_range.end() {
            return None;
        }
        let byte = *self.source.as_bytes().get(usize::from(source_position))?;
        byte.is_ascii().then_some(byte)
    }

    /// Clone the shared source backing this chunk.
    #[doc(hidden)]
    #[must_use]
    pub fn shared_source(&self) -> Arc<str> {
        Arc::clone(&self.source)
    }

    /// Byte range in the shared source backing this chunk.
    #[doc(hidden)]
    #[must_use]
    pub const fn source_range(&self) -> TextRange {
        self.source_range
    }

    /// Read one Unicode scalar from this identity-mapped chunk.
    #[must_use]
    #[inline]
    pub fn character(&self, position: TextSize) -> Option<InputCharacter> {
        if let Some(byte) = self.ascii_byte(position) {
            return Some(InputCharacter::new(
                CodePoint::from(byte),
                position + TextSize::from(1),
            ));
        }
        let offset = position.checked_sub(self.raw_start)?;
        let source_position = self.source_range.start().checked_add(offset)?;
        if source_position >= self.source_range.end() {
            return None;
        }
        let character = self
            .source
            .get(usize::from(source_position)..usize::from(self.source_range.end()))?
            .chars()
            .next()?;
        let character_length = TextSize::try_from(character.len_utf8()).ok()?;
        Some(InputCharacter::new(
            CodePoint::from(character),
            position + character_length,
        ))
    }

    /// Shorten this chunk at one original UTF-8 byte boundary.
    ///
    /// # Panics
    ///
    /// Panics when the requested end is outside the chunk or is not a UTF-8
    /// boundary in the shared source.
    pub fn truncate(&mut self, raw_end: TextSize) {
        assert!(
            raw_end >= self.raw_start && raw_end <= self.raw_end(),
            "truncated input chunk end is outside the chunk"
        );
        let source_end = self.source_range.start() + (raw_end - self.raw_start);
        assert!(
            self.source.is_char_boundary(usize::from(source_end)),
            "truncated input chunk must end at a UTF-8 boundary"
        );
        self.source_range = TextRange::new(self.source_range.start(), source_end);
    }
}

/// Immutable UTF-8 parser input.
pub trait Input: Send + Sync {
    /// Input length in UTF-8 bytes.
    fn len(&self) -> TextSize;

    /// Whether the input has no bytes.
    fn is_empty(&self) -> bool {
        self.len() == TextSize::from(0)
    }

    /// Read a chunk starting at a UTF-8 boundary.
    fn chunk(&self, from: TextSize) -> Cow<'_, str>;

    /// Whether chunks are already split at line boundaries.
    fn line_chunks(&self) -> bool {
        false
    }

    /// Read one byte range.
    fn read(&self, range: TextRange) -> Cow<'_, str>;

    /// Read the Unicode scalar immediately before one UTF-8 boundary.
    ///
    /// The default reads at most the four bytes of one UTF-8 scalar. Inputs
    /// with direct access to their backing storage may override it.
    #[doc(hidden)]
    fn character_before(&self, before: TextSize) -> Option<(TextSize, InputCharacter)> {
        if before == TextSize::from(0) || !self.is_boundary(before) {
            return None;
        }
        let minimum = before
            .checked_sub(TextSize::from(4))
            .unwrap_or(TextSize::from(0));
        let mut start = before - TextSize::from(1);
        while start > minimum && !self.is_boundary(start) {
            start -= TextSize::from(1);
        }
        if !self.is_boundary(start) {
            return None;
        }
        let text = self.read(TextRange::new(start, before));
        let mut characters = text.chars();
        let character = characters.next()?;
        if characters.next().is_some() || character.len_utf8() != usize::from(before - start) {
            return None;
        }
        Some((
            start,
            InputCharacter::new(CodePoint::from(character), before),
        ))
    }

    /// Whether a position is a valid UTF-8 byte boundary.
    fn is_boundary(&self, position: TextSize) -> bool;

    /// Return a shared identity-mapped chunk starting at one original byte
    /// position.
    ///
    /// The conservative default exposes no identity chunk. Inputs may opt in
    /// when their source storage maps one-to-one over the returned segment.
    #[doc(hidden)]
    fn identity_chunk(&self, _from: TextSize) -> Option<InputChunk> {
        None
    }
}

/// Language-specific lexical view over one raw UTF-8 input.
///
/// Ordinary inputs expose Unicode scalar values. Translation layers may
/// additionally expose surrogate code points when the source language is
/// defined in terms of UTF-16 code units.
#[doc(hidden)]
pub trait LexicalInput: Send + Sync {
    /// Underlying raw input.
    fn raw(&self) -> &dyn Input;

    /// Input length in original UTF-8 bytes.
    #[doc(hidden)]
    fn len(&self) -> TextSize {
        self.raw().len()
    }

    /// Whether the lexical input has no raw bytes.
    #[doc(hidden)]
    fn is_empty(&self) -> bool {
        self.len() == TextSize::from(0)
    }

    /// Shared identity-mapped region beginning at one logical boundary.
    #[doc(hidden)]
    fn identity_chunk(&self, from: TextSize) -> Option<InputChunk> {
        self.raw().identity_chunk(from)
    }

    /// Read one logical code point at an original byte boundary.
    ///
    /// The caller has already established that `from` is a complete logical
    /// boundary.
    #[doc(hidden)]
    fn character(&self, from: TextSize) -> Option<InputCharacter> {
        debug_assert!(self.raw().is_boundary(from));
        let character = self.raw().chunk(from).chars().next()?;
        let character_length = TextSize::try_from(character.len_utf8()).ok()?;
        Some(InputCharacter::new(
            CodePoint::from(character),
            from + character_length,
        ))
    }

    /// Read the logical character immediately before an original byte
    /// boundary.
    ///
    /// The caller has already established that `before` is a complete logical
    /// boundary.
    #[doc(hidden)]
    fn character_before(&self, before: TextSize) -> Option<(TextSize, InputCharacter)> {
        debug_assert!(self.raw().is_boundary(before));
        self.raw().character_before(before)
    }

    /// Whether a raw position is a complete logical-character boundary.
    #[doc(hidden)]
    fn is_boundary(&self, position: TextSize) -> bool {
        self.raw().is_boundary(position)
    }

    /// Read translated text between complete logical boundaries when every
    /// logical code point is a scalar value.
    ///
    /// `None` represents a translated spelling that cannot be encoded as
    /// UTF-8, such as one containing an isolated surrogate.
    #[doc(hidden)]
    fn scalar_text(&self, range: TextRange) -> Option<Cow<'_, str>> {
        Some(self.raw().read(range))
    }
}

/// Identity lexical view for ordinary UTF-8 input.
#[doc(hidden)]
#[derive(Clone)]
pub struct Utf8Input {
    raw: Arc<dyn Input>,
}

impl Utf8Input {
    /// Wrap one raw UTF-8 input without lexical translation.
    #[must_use]
    pub fn new(raw: Arc<dyn Input>) -> Self {
        Self { raw }
    }
}

impl fmt::Debug for Utf8Input {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Utf8Input")
            .field("length", &self.raw.len())
            .finish()
    }
}

impl LexicalInput for Utf8Input {
    fn raw(&self) -> &dyn Input {
        &*self.raw
    }
}

/// Owned string input.
#[derive(Clone, Debug)]
pub struct StringInput {
    source: Arc<str>,
    length: TextSize,
}

impl StringInput {
    /// Construct an owned parser input.
    ///
    /// # Errors
    ///
    /// Returns an input error when the source is larger than the 32-bit text
    /// coordinate space.
    pub fn try_new(source: impl Into<Arc<str>>) -> Result<Self, ParseError> {
        let source = source.into();
        let length = TextSize::try_from(source.len()).map_err(|_| {
            ParseError::new(
                ParseErrorKind::Input,
                None,
                "input exceeds the 32-bit text coordinate space",
            )
        })?;
        Ok(Self { source, length })
    }

    /// Borrow the complete source.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.source
    }
}

impl Input for StringInput {
    fn len(&self) -> TextSize {
        self.length
    }

    fn chunk(&self, from: TextSize) -> Cow<'_, str> {
        Cow::Borrowed(
            self.source
                .get(usize::from(from)..)
                .expect("input chunk must start at a UTF-8 boundary"),
        )
    }

    fn read(&self, range: TextRange) -> Cow<'_, str> {
        Cow::Borrowed(
            self.source
                .get(usize::from(range.start())..usize::from(range.end()))
                .expect("input range must use UTF-8 boundaries"),
        )
    }

    fn character_before(&self, before: TextSize) -> Option<(TextSize, InputCharacter)> {
        let text = self.source.get(..usize::from(before))?;
        let (start, character) = text.char_indices().next_back()?;
        let start = TextSize::try_from(start).ok()?;
        Some((
            start,
            InputCharacter::new(CodePoint::from(character), before),
        ))
    }

    fn is_boundary(&self, position: TextSize) -> bool {
        position <= self.length && self.source.is_char_boundary(usize::from(position))
    }

    fn identity_chunk(&self, from: TextSize) -> Option<InputChunk> {
        if from >= self.length || !self.source.is_char_boundary(usize::from(from)) {
            return None;
        }
        Some(InputChunk::new(
            Arc::clone(&self.source),
            TextRange::new(from, self.length),
            from,
        ))
    }
}

/// Internal immutable request passed from parser entry points and mixed
/// parsing to concrete parsers.
#[derive(Clone)]
pub struct ParseRequest {
    input: Arc<dyn Input>,
    lexical_input: Arc<dyn LexicalInput>,
    ranges: Arc<[TextRange]>,
    validated: bool,
}

impl fmt::Debug for ParseRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ParseRequest")
            .field("input_length", &self.input.len())
            .field("ranges", &self.ranges)
            .field("validated", &self.validated)
            .finish_non_exhaustive()
    }
}

impl ParseRequest {
    /// Build a full-input request.
    #[must_use]
    pub fn full(input: Arc<dyn Input>) -> Self {
        let length = input.len();
        let lexical_input = Arc::new(Utf8Input::new(Arc::clone(&input)));
        Self {
            input,
            lexical_input,
            ranges: Arc::from([TextRange::new(TextSize::from(0), length)]),
            validated: false,
        }
    }

    /// Build a request over selected raw byte ranges.
    ///
    /// Empty ranges contribute no logical code points. Advancing or resetting
    /// skips them while preserving their raw endpoints for mixed-parse
    /// bookkeeping.
    ///
    /// # Errors
    ///
    /// Returns an input error when ranges overlap, extend past the input, or
    /// use endpoints that are not UTF-8 boundaries.
    #[doc(hidden)]
    pub fn ranges(input: Arc<dyn Input>, ranges: Vec<TextRange>) -> Result<Self, ParseError> {
        let lexical_input = Arc::new(Utf8Input::new(Arc::clone(&input)));
        let mut request = Self {
            input,
            lexical_input,
            ranges: ranges.into(),
            validated: false,
        };
        request.ensure_validated()?;
        Ok(request)
    }

    /// Input being parsed.
    #[must_use]
    pub fn input(&self) -> &Arc<dyn Input> {
        &self.input
    }

    /// Language-specific lexical input view.
    #[doc(hidden)]
    #[must_use]
    pub fn lexical_input(&self) -> &Arc<dyn LexicalInput> {
        &self.lexical_input
    }

    /// Replace the lexical view while preserving raw source coordinates.
    ///
    /// # Errors
    ///
    /// Returns an input error when the replacement does not wrap the same raw
    /// input or when a selected endpoint splits a translated character.
    #[doc(hidden)]
    pub fn with_lexical_input(
        mut self,
        lexical_input: Arc<dyn LexicalInput>,
    ) -> Result<Self, ParseError> {
        if !std::ptr::eq(self.input.as_ref(), lexical_input.raw()) {
            return Err(ParseError::new(
                ParseErrorKind::Input,
                None,
                "a lexical input must wrap the parse request's raw input",
            ));
        }
        self.lexical_input = lexical_input;
        self.validated = false;
        self.ensure_validated()?;
        Ok(self)
    }

    /// Byte ranges selected by an internal mixed parse.
    #[doc(hidden)]
    #[must_use]
    pub fn selected_ranges(&self) -> &[TextRange] {
        &self.ranges
    }

    /// Validate raw and translated input boundaries.
    ///
    /// # Errors
    ///
    /// Returns an input error for malformed selected ranges or endpoints that
    /// are not complete raw and logical character boundaries.
    #[doc(hidden)]
    pub fn validate(&self) -> Result<(), ParseError> {
        validate_ranges(&*self.input, &*self.lexical_input, &self.ranges)
    }

    /// Validate this request unless its current immutable input view was
    /// already checked.
    ///
    /// # Errors
    ///
    /// Returns the same input errors as [`Self::validate`].
    #[doc(hidden)]
    pub fn into_validated(mut self) -> Result<Self, ParseError> {
        self.ensure_validated()?;
        Ok(self)
    }

    fn ensure_validated(&mut self) -> Result<(), ParseError> {
        if !self.validated {
            self.validate()?;
            self.validated = true;
        }
        Ok(())
    }
}

fn validate_ranges(
    input: &dyn Input,
    lexical_input: &dyn LexicalInput,
    ranges: &[TextRange],
) -> Result<(), ParseError> {
    if ranges.is_empty() {
        return Err(ParseError::new(
            ParseErrorKind::Input,
            None,
            "a parse request must contain at least one range",
        ));
    }
    let input_length = input.len();
    if lexical_input.len() != input_length {
        return Err(ParseError::new(
            ParseErrorKind::Input,
            None,
            "a lexical input must preserve raw source coordinates",
        ));
    }
    let mut previous_end = TextSize::from(0);
    for (index, range) in ranges.iter().enumerate() {
        if range.end() > input_length {
            return Err(ParseError::new(
                ParseErrorKind::Input,
                Some(range.end()),
                "parse range extends past the input",
            ));
        }
        if index != 0 && range.start() < previous_end {
            return Err(ParseError::new(
                ParseErrorKind::Input,
                Some(range.start()),
                "parse ranges must be sorted and non-overlapping",
            ));
        }
        for position in [range.start(), range.end()] {
            if !input.is_boundary(position) {
                return Err(ParseError::new(
                    ParseErrorKind::Input,
                    Some(position),
                    "parse range endpoint is not a UTF-8 boundary",
                ));
            }
            if !lexical_input.is_boundary(position) {
                return Err(ParseError::new(
                    ParseErrorKind::Input,
                    Some(position),
                    "parse range endpoint splits a translated character",
                ));
            }
        }
        previous_end = range.end();
    }
    Ok(())
}

/// In-progress non-incremental parse.
pub trait PartialParse: Send {
    /// Advance parsing and return the final tree once complete.
    ///
    /// # Errors
    ///
    /// Returns a fatal syntax, configuration, input, or budget error.
    fn advance(&mut self) -> Result<Option<Tree>, ParseError>;

    /// Input byte position reached by this parse.
    fn parsed_position(&self) -> TextSize;

    /// Stop the current parse at or after this byte position.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested position moves an existing stop
    /// point forward or lies outside the input.
    fn stop_at(&mut self, position: TextSize) -> Result<(), ParseError>;

    /// Current requested stop point.
    fn stopped_at(&self) -> Option<TextSize>;
}

/// Common non-incremental parser interface.
pub trait Parser: Send + Sync {
    /// Construct the parser-specific partial parse.
    ///
    /// # Errors
    ///
    /// Returns an input or parser-specific setup error.
    fn create_parse(&self, request: ParseRequest) -> Result<Box<dyn PartialParse>, ParseError>;

    /// Start parsing an owned input.
    ///
    /// # Errors
    ///
    /// Returns an input or parser-specific setup error.
    fn start_parse(&self, input: Arc<dyn Input>) -> Result<Box<dyn PartialParse>, ParseError> {
        self.create_parse(ParseRequest::full(input))
    }

    /// Parse a complete owned input.
    ///
    /// # Errors
    ///
    /// Returns the first fatal parser error.
    fn parse_input(&self, input: Arc<dyn Input>) -> Result<Tree, ParseError> {
        run_to_completion(self.start_parse(input)?)
    }

    /// Parse a complete UTF-8 string.
    ///
    /// # Errors
    ///
    /// Returns the first fatal parser error.
    fn parse(&self, source: &str) -> Result<Tree, ParseError> {
        let input: Arc<dyn Input> = Arc::new(StringInput::try_new(source)?);
        self.parse_input(input)
    }
}

/// Wrapper that can add non-incremental parsing behavior around a parser.
pub type ParseWrapper =
    Arc<dyn Fn(Box<dyn PartialParse>, ParseRequest) -> Box<dyn PartialParse> + Send + Sync>;

pub(crate) fn run_to_completion(mut partial: Box<dyn PartialParse>) -> Result<Tree, ParseError> {
    loop {
        if let Some(tree) = partial.advance()? {
            return Ok(tree);
        }
    }
}
