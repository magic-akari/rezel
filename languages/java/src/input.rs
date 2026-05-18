use std::borrow::Cow;
use std::sync::Arc;

use rezel_common::{Input, InputChunk, LogicalUnits, TextRange, TextSize};

/// Java's JLS §3.3 Unicode-escape view over original UTF-8 input.
///
/// The parser observes translated UTF-16 units while all ranges continue to
/// address the wrapped input.
#[derive(Clone)]
pub(crate) struct JavaInput {
    raw: Arc<dyn Input>,
    escapes: Arc<[UnicodeEscape]>,
    final_sub: Option<TextSize>,
    malformed_escapes: Arc<[TextSize]>,
}

impl std::fmt::Debug for JavaInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JavaInput")
            .field("length", &self.raw.len())
            .field("escape_count", &self.escapes.len())
            .field("final_sub", &self.final_sub)
            .field("malformed_escape_count", &self.malformed_escapes.len())
            .finish()
    }
}

impl JavaInput {
    pub(crate) fn new(raw: Arc<dyn Input>) -> Self {
        let ScanResult {
            escapes,
            final_sub,
            malformed_escapes,
        } = scan_unicode_escapes(&*raw);
        Self {
            raw,
            escapes: escapes.into(),
            final_sub,
            malformed_escapes: malformed_escapes.into(),
        }
    }

    pub(crate) fn malformed_escape_in(&self, ranges: &[TextRange]) -> Option<TextSize> {
        for range in ranges {
            let index = self
                .malformed_escapes
                .partition_point(|position| *position < range.start());
            if let Some(position) = self.malformed_escapes.get(index).copied()
                && position < range.end()
            {
                return Some(position);
            }
        }
        None
    }

    fn escape_starting_at(&self, position: TextSize) -> Option<UnicodeEscape> {
        self.escapes
            .binary_search_by_key(&position, |escape| escape.range.start())
            .ok()
            .map(|index| self.escapes[index])
    }

    fn escape_ending_at(&self, position: TextSize) -> Option<UnicodeEscape> {
        let index = self
            .escapes
            .partition_point(|escape| escape.range.end() < position);
        self.escapes
            .get(index)
            .copied()
            .filter(|escape| escape.range.end() == position)
    }

    fn lexical_escape_value(&self, escape: UnicodeEscape) -> u16 {
        const REPLACEMENT_CHARACTER: u16 = 0xfffd;

        // The generated token DFA ranges over Unicode scalar values, whereas
        // Java Unicode escapes may produce isolated UTF-16 surrogates. Preserve
        // valid pairs and substitute only isolated code units for lexing; raw
        // source slices and byte ranges remain unchanged.
        if (0xd800..=0xdbff).contains(&escape.value) {
            return self
                .escape_starting_at(escape.range.end())
                .filter(|next| (0xdc00..=0xdfff).contains(&next.value))
                .map_or(REPLACEMENT_CHARACTER, |_| escape.value);
        }
        if (0xdc00..=0xdfff).contains(&escape.value) {
            return self
                .escape_ending_at(escape.range.start())
                .filter(|previous| (0xd800..=0xdbff).contains(&previous.value))
                .map_or(REPLACEMENT_CHARACTER, |_| escape.value);
        }
        escape.value
    }

    fn range_has_translation(&self, range: TextRange) -> bool {
        if self
            .final_sub
            .is_some_and(|position| range.start() <= position && position < range.end())
        {
            return true;
        }
        let index = self
            .escapes
            .partition_point(|escape| escape.range.end() <= range.start());
        self.escapes
            .get(index)
            .is_some_and(|escape| escape.range.start() < range.end())
    }
}

impl Input for JavaInput {
    fn len(&self) -> TextSize {
        self.raw.len()
    }

    fn chunk(&self, from: TextSize) -> Cow<'_, str> {
        self.raw.chunk(from)
    }

    fn line_chunks(&self) -> bool {
        self.raw.line_chunks()
    }

    fn read(&self, range: TextRange) -> Cow<'_, str> {
        self.raw.read(range)
    }

    fn logical_chunk(&self, from: TextSize) -> Option<InputChunk> {
        if self.final_sub.is_none() && self.escapes.is_empty() {
            return self.raw.logical_chunk(from);
        }
        let escape_index = self
            .escapes
            .partition_point(|escape| escape.range.start() < from);
        let next_escape = self
            .escapes
            .get(escape_index)
            .map(|escape| escape.range.start());
        let next_translation = next_escape
            .into_iter()
            .chain(self.final_sub.filter(|position| *position >= from))
            .min()
            .unwrap_or_else(|| self.raw.len());
        if next_translation == from {
            return None;
        }
        let mut chunk = self.raw.logical_chunk(from)?;
        let end = chunk.raw_end().min(next_translation);
        chunk.truncate(end);
        chunk.contains(from).then_some(chunk)
    }

    fn logical_units(&self, from: TextSize) -> Option<LogicalUnits> {
        if self.final_sub.is_none() && self.escapes.is_empty() {
            return self.raw.logical_units(from);
        }
        if self.final_sub == Some(from) {
            return Some(LogicalUnits::new(
                &[u16::from(b' ')],
                from + TextSize::from(1),
            ));
        }
        if let Some(escape) = self.escape_starting_at(from) {
            let value = self.lexical_escape_value(escape);
            return Some(LogicalUnits::new(&[value], escape.range.end()));
        }
        self.raw.logical_units(from)
    }

    fn logical_units_before(&self, before: TextSize) -> Option<(TextSize, LogicalUnits)> {
        if self.final_sub.is_none() && self.escapes.is_empty() {
            return self.raw.logical_units_before(before);
        }
        if self
            .final_sub
            .is_some_and(|position| position + TextSize::from(1) == before)
        {
            let start = before - TextSize::from(1);
            return Some((start, LogicalUnits::new(&[u16::from(b' ')], before)));
        }
        if let Some(escape) = self.escape_ending_at(before) {
            let value = self.lexical_escape_value(escape);
            return Some((escape.range.start(), LogicalUnits::new(&[value], before)));
        }
        self.raw.logical_units_before(before)
    }

    fn read_logical(&self, range: TextRange) -> Cow<'_, str> {
        if !self.range_has_translation(range) {
            return self.raw.read_logical(range);
        }
        let mut units = Vec::new();
        let mut position = range.start();
        while position < range.end() {
            let Some(logical) = self.logical_units(position) else {
                break;
            };
            if logical.raw_end() > range.end() {
                break;
            }
            units.extend_from_slice(logical.units());
            position = logical.raw_end();
        }
        Cow::Owned(String::from_utf16_lossy(&units))
    }
}

#[derive(Clone, Copy, Debug)]
struct UnicodeEscape {
    range: TextRange,
    value: u16,
}

struct ScanResult {
    escapes: Vec<UnicodeEscape>,
    final_sub: Option<TextSize>,
    malformed_escapes: Vec<TextSize>,
}

fn scan_unicode_escapes(input: &dyn Input) -> ScanResult {
    let mut escapes = Vec::new();
    let mut position = TextSize::from(0);
    let mut trailing_backslashes = 0_usize;
    let mut previous_was_escape = false;
    let mut malformed_escapes = Vec::new();

    'scan: while position < input.len() {
        let chunk = input.chunk(position);
        if chunk.is_empty() {
            break;
        }
        for (offset, byte) in chunk.as_bytes().iter().copied().enumerate() {
            let offset = TextSize::try_from(offset).expect("input chunk fits in text coordinates");
            let character_start = position + offset;
            let eligible = previous_was_escape || trailing_backslashes.is_multiple_of(2);
            if byte == b'\\' && eligible {
                match unicode_escape_at(input, character_start) {
                    EscapeScan::Valid { value, end } => {
                        escapes.push(UnicodeEscape {
                            range: TextRange::new(character_start, end),
                            value,
                        });
                        previous_was_escape = true;
                        trailing_backslashes = if value == u16::from(b'\\') {
                            trailing_backslashes.saturating_add(1)
                        } else {
                            0
                        };
                        position = end;
                        continue 'scan;
                    }
                    EscapeScan::Malformed => {
                        malformed_escapes.push(character_start);
                    }
                    EscapeScan::NotEscape => {}
                }
            }

            previous_was_escape = false;
            trailing_backslashes = if byte == b'\\' {
                trailing_backslashes.saturating_add(1)
            } else {
                0
            };
        }
        position += TextSize::try_from(chunk.len()).expect("input chunk fits in text coordinates");
    }

    let final_sub = input
        .logical_units_before(input.len())
        .and_then(|(start, logical)| {
            (logical.raw_end() == input.len() && logical.units() == [u16::from(0x1a_u8)])
                .then_some(start)
        });
    ScanResult {
        escapes,
        final_sub,
        malformed_escapes,
    }
}

enum EscapeScan {
    Valid { value: u16, end: TextSize },
    Malformed,
    NotEscape,
}

fn unicode_escape_at(input: &dyn Input, offset: TextSize) -> EscapeScan {
    let mut cursor = offset + TextSize::from(1);
    if ascii_byte(input, cursor) != Some(b'u') {
        return EscapeScan::NotEscape;
    }
    while ascii_byte(input, cursor) == Some(b'u') {
        cursor += TextSize::from(1);
    }
    let Some(end) = cursor.checked_add(TextSize::from(4)) else {
        return EscapeScan::Malformed;
    };
    if end > input.len() {
        return EscapeScan::Malformed;
    }
    let mut value = 0_u16;
    let mut position = cursor;
    while position < end {
        let Some(digit) = ascii_byte(input, position).and_then(hex_value) else {
            return EscapeScan::Malformed;
        };
        value = value * 16 + u16::from(digit);
        position += TextSize::from(1);
    }
    EscapeScan::Valid { value, end }
}

fn ascii_byte(input: &dyn Input, position: TextSize) -> Option<u8> {
    input
        .chunk(position)
        .as_bytes()
        .first()
        .copied()
        .filter(u8::is_ascii)
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use rezel_common::StringInput;

    use super::*;

    struct ChunkedInput {
        source: Arc<str>,
        chunk_length: usize,
    }

    impl Input for ChunkedInput {
        fn len(&self) -> TextSize {
            self.source.len().try_into().unwrap()
        }

        fn chunk(&self, from: TextSize) -> Cow<'_, str> {
            let from = usize::from(from);
            let to = (from + self.chunk_length).min(self.source.len());
            Cow::Borrowed(&self.source[from..to])
        }

        fn read(&self, range: TextRange) -> Cow<'_, str> {
            Cow::Borrowed(&self.source[usize::from(range.start())..usize::from(range.end())])
        }
    }

    fn input(source: &str) -> JavaInput {
        JavaInput::new(Arc::new(StringInput::try_new(source).unwrap()))
    }

    fn cooked(source: &str) -> String {
        input(source)
            .read_logical(TextRange::new(0.into(), source.len().try_into().unwrap()))
            .into_owned()
    }

    #[test]
    fn translates_only_eligible_unicode_escapes() {
        assert_eq!(cooked(r"cl\u0061ss"), "class");
        assert_eq!(cooked(r"\\u0061"), r"\\u0061");
        assert_eq!(cooked(r"\\\u0061"), r"\\a");
        assert_eq!(cooked(r"\u005cu0061"), r"\u0061");
        assert_eq!(cooked(r"\u005c\u005c"), r"\\");
    }

    #[test]
    fn scans_unicode_escapes_across_input_chunks() {
        let raw: Arc<str> = Arc::from(r"cl\u0061ss");
        let input = JavaInput::new(Arc::new(ChunkedInput {
            source: Arc::clone(&raw),
            chunk_length: 2,
        }));
        assert_eq!(
            input
                .read_logical(TextRange::new(0.into(), raw.len().try_into().unwrap()))
                .as_ref(),
            "class"
        );
    }

    #[test]
    fn retains_raw_boundaries_for_escaped_units() {
        let input = input(r"cl\u0061ss");
        let logical = input.logical_units(2.into()).unwrap();
        assert_eq!(logical.units(), &[u16::from(b'a')]);
        assert_eq!(logical.raw_end(), 8.into());
        let (start, previous) = input.logical_units_before(8.into()).unwrap();
        assert_eq!(start, 2.into());
        assert_eq!(previous, logical);
    }

    #[test]
    fn shared_chunks_stop_at_translated_units() {
        let input = input(r"cl\u0061ss");
        let prefix = input.logical_chunk(0.into()).expect("identity prefix");
        assert_eq!(prefix.raw_start(), 0.into());
        assert_eq!(prefix.raw_end(), 2.into());
        assert!(input.logical_chunk(2.into()).is_none());

        let suffix = input.logical_chunk(8.into()).expect("identity suffix");
        assert_eq!(suffix.raw_start(), 8.into());
        assert_eq!(suffix.raw_end(), 10.into());
    }

    #[test]
    fn preserves_utf16_surrogates_and_ignores_final_sub() {
        let pair = input(r"\uD83D\uDE00");
        assert_eq!(pair.logical_units(0.into()).unwrap().units(), &[0xd83d]);
        assert_eq!(pair.logical_units(6.into()).unwrap().units(), &[0xde00]);
        assert_eq!(cooked(r"\uD83D\uDE00"), "😀");
        assert_eq!(
            input(r"\uD800").logical_units(0.into()).unwrap().units(),
            &[0xfffd]
        );
        assert_eq!(
            input(r"\uD800")
                .logical_units_before(6.into())
                .unwrap()
                .1
                .units(),
            &[0xfffd]
        );
        assert_eq!(
            input(r"\uDC00").logical_units(0.into()).unwrap().units(),
            &[0xfffd]
        );
        assert_eq!(cooked("class A {}\u{1a}"), "class A {} ");
    }

    #[test]
    fn records_malformed_eligible_escapes() {
        let translated = input(r"\uu12xz \u123x");
        assert_eq!(
            &*translated.malformed_escapes,
            &[TextSize::from(0), TextSize::from(8)]
        );
        assert_eq!(
            translated.malformed_escape_in(&[TextRange::new(8.into(), 14.into())]),
            Some(8.into())
        );
        assert!(input(r"\\uu12xz").malformed_escapes.is_empty());
    }
}
