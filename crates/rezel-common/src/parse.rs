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

/// One logical UTF-16 input character and its original byte boundary.
///
/// This is an internal parser-input extension point. Most inputs use the
/// default implementation on [`Input`]. A language-specific lexical
/// translation may override it while preserving original source positions.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogicalUnits {
    units: [u16; 2],
    len: u8,
    raw_end: TextSize,
}

impl LogicalUnits {
    /// Construct one logical UTF-16 sequence.
    ///
    /// # Panics
    ///
    /// Panics when `units` is empty or contains more than two code units.
    #[must_use]
    #[inline]
    pub fn new(units: &[u16], raw_end: TextSize) -> Self {
        let (storage, len) = match units {
            [first] => ([*first, 0], 1),
            [first, second] => ([*first, *second], 2),
            _ => panic!("a logical input character has one or two UTF-16 units"),
        };
        Self {
            units: storage,
            len,
            raw_end,
        }
    }

    /// UTF-16 units in this logical character.
    #[must_use]
    #[inline]
    pub fn units(&self) -> &[u16] {
        &self.units[..usize::from(self.len)]
    }

    /// Original UTF-8 byte boundary after this logical character.
    #[must_use]
    #[inline]
    pub const fn raw_end(self) -> TextSize {
        self.raw_end
    }
}

/// Shared UTF-8 input segment whose bytes map one-to-one to source positions.
///
/// This is an internal parser-input extension point. Language adapters that
/// translate source characters may return identity chunks between translated
/// regions and fall back to [`Input::logical_units`] at translated positions.
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

    /// Read one logical character from this identity-mapped chunk.
    #[must_use]
    #[inline]
    pub fn logical_units(&self, position: TextSize) -> Option<LogicalUnits> {
        if let Some(byte) = self.ascii_byte(position) {
            return Some(LogicalUnits::new(
                &[u16::from(byte)],
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
        let mut units = [0; 2];
        let encoded = character.encode_utf16(&mut units);
        let character_length = TextSize::try_from(character.len_utf8()).ok()?;
        Some(LogicalUnits::new(encoded, position + character_length))
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

    /// Return a shared identity-mapped logical chunk starting at one original
    /// byte position.
    ///
    /// The conservative default exposes no identity chunk. Inputs may opt in
    /// when their source storage and logical character view are known to map
    /// one-to-one over the returned segment.
    #[doc(hidden)]
    fn logical_chunk(&self, _from: TextSize) -> Option<InputChunk> {
        None
    }

    /// Read one logical UTF-16 character at an original byte boundary.
    #[doc(hidden)]
    fn logical_units(&self, from: TextSize) -> Option<LogicalUnits> {
        let character = self.chunk(from).chars().next()?;
        let mut units = [0; 2];
        let encoded = character.encode_utf16(&mut units);
        let character_length = TextSize::try_from(character.len_utf8()).ok()?;
        Some(LogicalUnits::new(encoded, from + character_length))
    }

    /// Read the logical character immediately before an original byte
    /// boundary.
    #[doc(hidden)]
    fn logical_units_before(&self, before: TextSize) -> Option<(TextSize, LogicalUnits)> {
        let text = self.read(TextRange::new(TextSize::from(0), before));
        let (start, character) = text.char_indices().next_back()?;
        let start = TextSize::try_from(start).ok()?;
        let mut units = [0; 2];
        let encoded = character.encode_utf16(&mut units);
        Some((start, LogicalUnits::new(encoded, before)))
    }

    /// Read the lexically translated spelling of one original byte range.
    #[doc(hidden)]
    fn read_logical(&self, range: TextRange) -> Cow<'_, str> {
        self.read(range)
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

    fn logical_chunk(&self, from: TextSize) -> Option<InputChunk> {
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
    ranges: Arc<[TextRange]>,
}

impl fmt::Debug for ParseRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ParseRequest")
            .field("input_length", &self.input.len())
            .field("ranges", &self.ranges)
            .finish()
    }
}

impl ParseRequest {
    /// Build a full-input request.
    #[must_use]
    pub fn full(input: Arc<dyn Input>) -> Self {
        let length = input.len();
        Self {
            input,
            ranges: Arc::from([TextRange::new(TextSize::from(0), length)]),
        }
    }

    pub(crate) fn ranges(input: Arc<dyn Input>, ranges: Vec<TextRange>) -> Self {
        validate_ranges(input.len(), &ranges);
        Self {
            input,
            ranges: ranges.into(),
        }
    }

    /// Input being parsed.
    #[must_use]
    pub fn input(&self) -> &Arc<dyn Input> {
        &self.input
    }

    /// Replace the input view while preserving selected source ranges.
    ///
    /// The replacement must use the same coordinate space and length.
    #[doc(hidden)]
    #[must_use]
    pub fn map_input(self, map: impl FnOnce(Arc<dyn Input>) -> Arc<dyn Input>) -> Self {
        let original_length = self.input.len();
        let input = map(self.input);
        assert_eq!(
            input.len(),
            original_length,
            "a mapped parse input must preserve source coordinates"
        );
        Self {
            input,
            ranges: self.ranges,
        }
    }

    /// Byte ranges selected by an internal mixed parse.
    #[doc(hidden)]
    #[must_use]
    pub fn selected_ranges(&self) -> &[TextRange] {
        &self.ranges
    }
}

fn validate_ranges(input_length: TextSize, ranges: &[TextRange]) {
    assert!(!ranges.is_empty(), "a parse request must contain one range");
    let mut previous_end = TextSize::from(0);
    for (index, range) in ranges.iter().enumerate() {
        assert!(
            range.end() <= input_length,
            "parse range extends past the input"
        );
        assert!(
            index == 0 || range.start() >= previous_end,
            "parse ranges must be sorted and non-overlapping"
        );
        previous_end = range.end();
    }
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
