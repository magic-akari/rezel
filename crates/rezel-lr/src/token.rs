use std::borrow::Cow;
use std::sync::Arc;

use rezel_common::{
    Input, InputChunk, LogicalUnits, ParseError, ParseErrorKind, TextRange, TextSize,
};

use crate::stack::Stack;
use crate::table::SequenceCode;

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
    /// Compact token DFA.
    pub data: &'static [u16],
    /// Offset of this group's token-precedence sequence.
    pub precedence_offset: usize,
    /// Token emitted for otherwise unmatched input.
    pub else_token: Option<u16>,
}

impl LocalTokenGroup {
    /// Construct a local token group.
    #[must_use]
    pub const fn new(
        data: &'static [u16],
        precedence_offset: usize,
        else_token: Option<u16>,
    ) -> Self {
        Self {
            data,
            precedence_offset,
            else_token,
        }
    }
}

/// Statically linked Rust external tokenizer.
pub struct ExternalTokenizer {
    callback: fn(&mut InputStream, &Stack) -> Result<(), ParseError>,
    flags: TokenizerFlags,
}

impl ExternalTokenizer {
    /// Define an external tokenizer binding.
    #[must_use]
    pub const fn new(
        callback: fn(&mut InputStream, &Stack) -> Result<(), ParseError>,
        flags: TokenizerFlags,
    ) -> Self {
        Self { callback, flags }
    }

    fn token(&self, input: &mut InputStream, stack: &Stack) -> Result<(), ParseError> {
        (self.callback)(input, stack)
    }

    const fn flags(&self) -> TokenizerFlags {
        self.flags
    }
}

impl std::fmt::Debug for ExternalTokenizer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExternalTokenizer")
            .field("flags", &self.flags)
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

    pub(crate) fn token(self, input: &mut InputStream, stack: &Stack) -> Result<(), ParseError> {
        match self {
            Self::Group(group) => {
                let core = stack.core();
                read_token(
                    core.language.token_data,
                    input,
                    stack,
                    group.id,
                    core.language.state_data,
                    core.language.token_precedence,
                );
                Ok(())
            }
            Self::Local(group) => read_local_token(group, input, stack),
            Self::External(tokenizer) => tokenizer.token(input, stack),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StreamCursor {
    range_index: usize,
    byte: TextSize,
    trailing_surrogate: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CachedLogical {
    range_index: usize,
    byte: TextSize,
    units: LogicalUnits,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AcceptedToken {
    pub(crate) value: u16,
    pub(crate) end: TextSize,
}

/// UTF-8-positioned input stream exposed to generated and external tokenizers.
///
/// Token DFAs observe UTF-16 code units to preserve Lezer grammar token
/// semantics. All public positions and accepted token ranges remain UTF-8 byte
/// offsets.
pub struct InputStream {
    input: Arc<dyn Input>,
    ranges: Arc<[TextRange]>,
    cursor: StreamCursor,
    end: TextSize,
    token_start: TextSize,
    accepted: Option<AcceptedToken>,
    chunk: Option<InputChunk>,
    logical: Option<CachedLogical>,
    next_unit: Option<u16>,
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
    pub(crate) fn new(input: Arc<dyn Input>, ranges: Arc<[TextRange]>) -> Self {
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
            logical: None,
            next_unit: None,
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

    /// Next UTF-16 code unit, or `None` at the end of selected input.
    #[must_use]
    pub const fn next(&self) -> Option<u16> {
        self.next_unit
    }

    /// Look around the stream in UTF-16 code units.
    #[must_use]
    pub fn peek(&mut self, offset: isize) -> Option<u16> {
        if offset == 0 {
            return self.next();
        }
        if offset == 1
            && !self.cursor.trailing_surrogate
            && let Some(current) = self.current_logical()
            && current.units().len() == 2
        {
            return Some(current.units()[1]);
        }
        if offset == -1
            && self.cursor.trailing_surrogate
            && let Some(current) = self.current_logical()
        {
            return Some(current.units()[0]);
        }
        let cursor = self.offset_cursor(self.cursor, offset)?;
        code_unit_at(&*self.input, &self.ranges, cursor)
    }

    /// Move forward by UTF-16 code units and return the new next unit.
    pub fn advance(&mut self, count: usize) -> Option<u16> {
        for _ in 0..count {
            let Some(next) = self.advance_current() else {
                self.cursor = end_cursor(&self.ranges);
                self.next_unit = None;
                return None;
            };
            self.cursor = next;
        }
        self.refresh_next();
        self.next_unit
    }

    /// Accept a token ending at the current stream position plus a UTF-16
    /// code-unit offset.
    ///
    /// # Errors
    ///
    /// Returns an input error when the requested end is outside selected
    /// ranges, inside a UTF-8 scalar, or before the token start.
    pub fn accept_token(&mut self, token: u16, end_offset: isize) -> Result<(), ParseError> {
        let Some(cursor) = self.offset_cursor(self.cursor, end_offset) else {
            return Err(ParseError::new(
                ParseErrorKind::Input,
                Some(self.position()),
                "token end is outside selected input",
            ));
        };
        let end = boundary_position(&*self.input, cursor)?;
        self.accept_token_to(token, end)
    }

    /// Accept a token ending at an explicit UTF-8 byte position.
    ///
    /// # Errors
    ///
    /// Returns an input error for a reversed or non-boundary token range.
    pub fn accept_token_to(&mut self, token: u16, end: TextSize) -> Result<(), ParseError> {
        if end < self.token_start {
            return Err(ParseError::new(
                ParseErrorKind::Input,
                Some(end),
                "token end precedes its start",
            ));
        }
        if end != self.end && !is_selected_boundary(&*self.input, &self.ranges, end) {
            return Err(ParseError::new(
                ParseErrorKind::Input,
                Some(end),
                "token end is not a selected UTF-8 boundary",
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
    pub fn read(&self, from: TextSize, to: TextSize) -> Cow<'_, str> {
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
            return Cow::Borrowed("");
        };
        let first = self.input.read_logical(first);
        let Some(second) = selected.next() else {
            return first;
        };

        let capacity = to.checked_sub(from).unwrap_or(TextSize::from(0));
        let mut result = String::with_capacity(usize::from(capacity));
        result.push_str(&first);
        result.push_str(&self.input.read_logical(second));
        for range in selected {
            result.push_str(&self.input.read_logical(range));
        }
        Cow::Owned(result)
    }

    pub(crate) fn reset(&mut self, position: TextSize) {
        self.token_start = position;
        self.accepted = None;
        if self.cursor.byte == position && !self.cursor.trailing_surrogate {
            return;
        }
        self.cursor = cursor_at(&self.ranges, position);
        self.refresh_next();
    }

    pub(crate) fn clip_position(&self, position: TextSize) -> TextSize {
        for range in &*self.ranges {
            if range.end() > position {
                return position.max(range.start());
            }
        }
        self.end
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
        self.next_position_from_cursor(self.cursor)
    }

    pub(crate) fn next_position_from(&self, position: TextSize) -> Option<TextSize> {
        self.next_position_from_cursor(cursor_at(&self.ranges, position))
    }

    fn next_position_from_cursor(&self, mut cursor: StreamCursor) -> Option<TextSize> {
        loop {
            cursor = advance_cursor(&*self.input, &self.ranges, cursor)?;
            if !cursor.trailing_surrogate {
                return Some(cursor.byte);
            }
        }
    }

    fn offset_cursor(&self, mut cursor: StreamCursor, offset: isize) -> Option<StreamCursor> {
        if offset >= 0 {
            for _ in 0..offset.unsigned_abs() {
                cursor = advance_cursor(&*self.input, &self.ranges, cursor)?;
            }
        } else {
            for _ in 0..offset.unsigned_abs() {
                cursor = retreat_cursor(&*self.input, &self.ranges, cursor)?;
            }
        }
        Some(cursor)
    }

    fn current_logical(&mut self) -> Option<LogicalUnits> {
        if let Some(cached) = self.logical
            && cached.range_index == self.cursor.range_index
            && cached.byte == self.cursor.byte
        {
            return Some(cached.units);
        }
        let range = *self.ranges.get(self.cursor.range_index)?;
        if self.cursor.byte >= range.end() {
            return None;
        }
        let mut units = self.chunk_units(range, self.cursor.byte);
        if units.is_none() {
            self.chunk = self.input.logical_chunk(self.cursor.byte);
            units = self.chunk_units(range, self.cursor.byte);
        }
        let units = units.or_else(|| logical_units_at(&*self.input, range, self.cursor.byte))?;
        self.logical = Some(CachedLogical {
            range_index: self.cursor.range_index,
            byte: self.cursor.byte,
            units,
        });
        Some(units)
    }

    #[inline]
    fn chunk_units(&self, range: TextRange, position: TextSize) -> Option<LogicalUnits> {
        if position >= range.end() {
            return None;
        }
        let units = self.chunk.as_ref()?.logical_units(position)?;
        (units.raw_end() <= range.end()).then_some(units)
    }

    #[inline]
    fn chunk_ascii(&self, range: TextRange, position: TextSize) -> Option<u8> {
        if position >= range.end() {
            return None;
        }
        self.chunk.as_ref()?.ascii_byte(position)
    }

    fn advance_current(&mut self) -> Option<StreamCursor> {
        let range = *self.ranges.get(self.cursor.range_index)?;
        if self.cursor.byte >= range.end() {
            return next_range_cursor(&self.ranges, self.cursor.range_index);
        }
        if !self.cursor.trailing_surrogate && self.chunk_ascii(range, self.cursor.byte).is_some() {
            let byte = self.cursor.byte + TextSize::from(1);
            if byte < range.end() {
                return Some(StreamCursor {
                    byte,
                    trailing_surrogate: false,
                    ..self.cursor
                });
            }
            return next_range_cursor(&self.ranges, self.cursor.range_index)
                .or_else(|| Some(end_cursor(&self.ranges)));
        }
        let logical = self.current_logical()?;
        if logical.units().len() == 2 && !self.cursor.trailing_surrogate {
            return Some(StreamCursor {
                trailing_surrogate: true,
                ..self.cursor
            });
        }
        let byte = logical.raw_end();
        if byte < range.end() {
            return Some(StreamCursor {
                byte,
                trailing_surrogate: false,
                ..self.cursor
            });
        }
        next_range_cursor(&self.ranges, self.cursor.range_index)
            .or_else(|| Some(end_cursor(&self.ranges)))
    }

    fn refresh_next(&mut self) {
        let Some(range) = self.ranges.get(self.cursor.range_index).copied() else {
            self.next_unit = None;
            return;
        };
        if !self.cursor.trailing_surrogate
            && let Some(byte) = self.chunk_ascii(range, self.cursor.byte)
        {
            self.next_unit = Some(u16::from(byte));
            return;
        }
        self.next_unit = self.current_logical().map(|logical| {
            let units = logical.units();
            if self.cursor.trailing_surrogate && units.len() == 2 {
                units[1]
            } else {
                units[0]
            }
        });
    }
}

fn first_cursor(ranges: &[TextRange]) -> StreamCursor {
    let range_index = ranges
        .iter()
        .position(|range| !range.is_empty())
        .unwrap_or_else(|| ranges.len().saturating_sub(1));
    StreamCursor {
        range_index,
        byte: ranges
            .get(range_index)
            .map_or(TextSize::from(0), |range| range.start()),
        trailing_surrogate: false,
    }
}

fn end_cursor(ranges: &[TextRange]) -> StreamCursor {
    StreamCursor {
        range_index: ranges.len().saturating_sub(1),
        byte: ranges.last().map_or(TextSize::from(0), |range| range.end()),
        trailing_surrogate: false,
    }
}

fn cursor_at(ranges: &[TextRange], position: TextSize) -> StreamCursor {
    let range_index = ranges
        .iter()
        .position(|range| position >= range.start() && position < range.end())
        .unwrap_or_else(|| ranges.len().saturating_sub(1));
    StreamCursor {
        range_index,
        byte: position,
        trailing_surrogate: false,
    }
}

fn logical_units_at(
    input: &dyn Input,
    range: TextRange,
    position: TextSize,
) -> Option<LogicalUnits> {
    let units = input.logical_units(position)?;
    (units.raw_end() <= range.end()).then_some(units)
}

fn code_unit_at(input: &dyn Input, ranges: &[TextRange], cursor: StreamCursor) -> Option<u16> {
    let range = ranges.get(cursor.range_index)?;
    if cursor.byte >= range.end() {
        return None;
    }
    let logical = logical_units_at(input, *range, cursor.byte)?;
    let units = logical.units();
    Some(if cursor.trailing_surrogate && units.len() == 2 {
        units[1]
    } else {
        units[0]
    })
}

fn advance_cursor(
    input: &dyn Input,
    ranges: &[TextRange],
    cursor: StreamCursor,
) -> Option<StreamCursor> {
    let range = ranges.get(cursor.range_index)?;
    if cursor.byte >= range.end() {
        return next_range_cursor(ranges, cursor.range_index);
    }
    let logical = logical_units_at(input, *range, cursor.byte)?;
    if logical.units().len() == 2 && !cursor.trailing_surrogate {
        return Some(StreamCursor {
            trailing_surrogate: true,
            ..cursor
        });
    }
    let byte = logical.raw_end();
    if byte < range.end() {
        return Some(StreamCursor {
            byte,
            trailing_surrogate: false,
            ..cursor
        });
    }
    next_range_cursor(ranges, cursor.range_index).or_else(|| Some(end_cursor(ranges)))
}

fn retreat_cursor(
    input: &dyn Input,
    ranges: &[TextRange],
    cursor: StreamCursor,
) -> Option<StreamCursor> {
    if cursor.trailing_surrogate {
        return Some(StreamCursor {
            trailing_surrogate: false,
            ..cursor
        });
    }
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
    let (byte, logical) = input.logical_units_before(byte_limit)?;
    if byte < previous_range.start() || logical.raw_end() != byte_limit {
        return None;
    }
    Some(StreamCursor {
        range_index,
        byte,
        trailing_surrogate: logical.units().len() == 2,
    })
}

fn next_range_cursor(ranges: &[TextRange], range_index: usize) -> Option<StreamCursor> {
    let next = ((range_index + 1)..ranges.len()).find(|index| !ranges[*index].is_empty())?;
    Some(StreamCursor {
        range_index: next,
        byte: ranges[next].start(),
        trailing_surrogate: false,
    })
}

fn boundary_position(input: &dyn Input, cursor: StreamCursor) -> Result<TextSize, ParseError> {
    if cursor.trailing_surrogate {
        return Err(ParseError::new(
            ParseErrorKind::Input,
            Some(cursor.byte),
            "token end falls inside a UTF-8 scalar",
        ));
    }
    if cursor.byte == input.len() || input.chunk(cursor.byte).is_char_boundary(0) {
        Ok(cursor.byte)
    } else {
        Err(ParseError::new(
            ParseErrorKind::Input,
            Some(cursor.byte),
            "token end is not a UTF-8 boundary",
        ))
    }
}

fn is_selected_boundary(input: &dyn Input, ranges: &[TextRange], position: TextSize) -> bool {
    ranges.iter().any(|range| {
        position >= range.start()
            && position <= range.end()
            && input
                .read(TextRange::new(range.start(), position))
                .is_char_boundary(usize::from(position - range.start()))
    })
}

fn read_local_token(
    group: LocalTokenGroup,
    input: &mut InputStream,
    stack: &Stack,
) -> Result<(), ParseError> {
    let start = input.position();
    loop {
        let scan_start = input.position();
        let next_position = input.next_position();
        read_token(
            group.data,
            input,
            stack,
            0,
            group.data,
            group.precedence_offset,
        );
        if input.accepted().is_some() {
            if scan_start > start {
                input.reset(start);
                input.accept_token_to(
                    group
                        .else_token
                        .expect("scanning continued only with @else"),
                    scan_start,
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
    data: &[u16],
    input: &mut InputStream,
    stack: &Stack,
    group: u8,
    precedence_data: &[u16],
    precedence_offset: usize,
) {
    let mut state = 0_usize;
    let group_mask = 1_u16 << group;
    'scan: loop {
        if data.get(state).copied().unwrap_or(0) & group_mask == 0 {
            break;
        }
        let accept_end = usize::from(data[state + 1]);
        let mut index = state + 3;
        while index < accept_end {
            if data[index + 1] & group_mask != 0 {
                let term = data[index];
                let current = input.accepted_value();
                let can_accept = stack.dialect_allows(term)
                    && current.is_none_or(|previous| {
                        previous == term
                            || overrides(term, previous, precedence_data, precedence_offset)
                    });
                if can_accept {
                    input.accept_current(term);
                    break;
                }
            }
            index += 2;
        }

        let next = input.next();
        let mut low = 0_usize;
        let mut high = usize::from(data[state + 2]);
        if next.is_none()
            && high > low
            && data[accept_end + high * 3 - 3] == SequenceCode::End.raw()
        {
            state = usize::from(data[accept_end + high * 3 - 1]);
            continue;
        }
        let Some(next) = next else {
            break;
        };
        while low < high {
            let middle = (low + high) >> 1;
            let edge = accept_end + middle * 3;
            let from = u32::from(data[edge]);
            let to = if data[edge + 1] == 0 {
                0x1_0000
            } else {
                u32::from(data[edge + 1])
            };
            let next = u32::from(next);
            if next < from {
                high = middle;
            } else if next >= to {
                low = middle + 1;
            } else {
                state = usize::from(data[edge + 2]);
                input.advance(1);
                continue 'scan;
            }
        }
        break;
    }
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
    use rezel_common::StringInput;

    use super::*;

    fn stream(source: &str, ranges: impl Into<Arc<[TextRange]>>) -> InputStream {
        let input: Arc<dyn Input> = Arc::new(StringInput::try_new(source).unwrap());
        InputStream::new(input, ranges.into())
    }

    #[test]
    fn identity_chunk_stops_at_the_final_selected_range() {
        let ranges = Arc::from([TextRange::new(0.into(), 2.into())]);
        let mut input = stream("abX", ranges);

        assert_eq!(input.next(), Some(u16::from(b'a')));
        assert_eq!(input.advance(1), Some(u16::from(b'b')));
        assert_eq!(input.advance(1), None);
        assert_eq!(input.position(), TextSize::from(2));
        assert_eq!(input.next(), None);
    }

    #[test]
    fn reset_rewinds_from_inside_a_surrogate_pair() {
        let ranges = Arc::from([TextRange::new(0.into(), 5.into())]);
        let mut input = stream("😀a", ranges);

        assert_eq!(input.next(), Some(0xd83d));
        assert_eq!(input.advance(1), Some(0xde00));
        assert_eq!(input.position(), TextSize::from(0));

        input.reset(0.into());

        assert_eq!(input.next(), Some(0xd83d));
    }
}
