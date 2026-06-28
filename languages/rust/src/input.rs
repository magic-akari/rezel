use std::borrow::Cow;
use std::iter::Peekable;
use std::sync::Arc;

use rezel_common::{
    CodePoint, Input, InputCharacter, InputChunk, LexicalInput, TextRange, TextSize,
};

const BANG: u32 = b'!' as u32;
const BYTE_ORDER_MARK: u32 = 0xfeff;
const HASH: u32 = b'#' as u32;
const LEFT_BRACKET: u32 = b'[' as u32;
const LINE_FEED: u32 = b'\n' as u32;
const SLASH: u32 = b'/' as u32;
const SPACE: u32 = b' ' as u32;
const STAR: u32 = b'*' as u32;

/// Rust's source-file view over original UTF-8 input.
///
/// The Reference removes one leading byte order mark and an optional shebang
/// before tokenization. Rezel exposes spaces over those raw characters so the
/// grammar ignores them while every source coordinate remains a raw UTF-8 byte
/// position.
#[derive(Clone)]
pub(crate) struct RustInput {
    raw: Arc<dyn Input>,
    hidden_ranges: Arc<[TextRange]>,
}

impl std::fmt::Debug for RustInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RustInput")
            .field("length", &self.raw.len())
            .field("hidden_ranges", &self.hidden_ranges)
            .finish()
    }
}

impl RustInput {
    pub(crate) fn new(raw: Arc<dyn Input>) -> Self {
        let hidden_ranges = source_prefix_ranges(&*raw).into();
        Self { raw, hidden_ranges }
    }

    fn hidden_range_at_or_after(&self, position: TextSize) -> Option<TextRange> {
        let index = self
            .hidden_ranges
            .partition_point(|range| range.end() <= position);
        self.hidden_ranges.get(index).copied()
    }

    fn hides(&self, position: TextSize) -> bool {
        self.hidden_range_at_or_after(position)
            .is_some_and(|range| range.start() <= position)
    }

    fn range_has_hidden_text(&self, range: TextRange) -> bool {
        let index = self
            .hidden_ranges
            .partition_point(|hidden| hidden.end() <= range.start());
        self.hidden_ranges
            .get(index)
            .is_some_and(|hidden| hidden.start() < range.end())
    }
}

impl LexicalInput for RustInput {
    fn raw(&self) -> &dyn Input {
        &*self.raw
    }

    fn identity_chunk(&self, from: TextSize) -> Option<InputChunk> {
        let hidden = self.hidden_range_at_or_after(from);
        if hidden.is_some_and(|range| range.start() <= from) {
            return None;
        }
        let next_hidden = hidden.map_or_else(|| self.raw.len(), TextRange::start);
        let mut chunk = self.raw.identity_chunk(from)?;
        let end = chunk.raw_end().min(next_hidden);
        chunk.truncate(end);
        chunk.contains(from).then_some(chunk)
    }

    fn character(&self, from: TextSize) -> Option<InputCharacter> {
        let character = raw_character(&*self.raw, from)?;
        if self.hides(from) {
            return Some(InputCharacter::new(
                CodePoint::from(b' '),
                character.raw_end(),
            ));
        }
        Some(character)
    }

    fn character_before(&self, before: TextSize) -> Option<(TextSize, InputCharacter)> {
        let (start, character) = raw_character_before(&*self.raw, before)?;
        if self.hides(start) {
            return Some((
                start,
                InputCharacter::new(CodePoint::from(b' '), character.raw_end()),
            ));
        }
        Some((start, character))
    }

    fn is_boundary(&self, position: TextSize) -> bool {
        self.raw.is_boundary(position)
    }

    fn scalar_text(&self, range: TextRange) -> Option<Cow<'_, str>> {
        if !self.range_has_hidden_text(range) {
            return Some(self.raw.read(range));
        }
        debug_assert!(self.is_boundary(range.start()) && self.is_boundary(range.end()));
        let mut result = String::new();
        let mut position = range.start();
        while position < range.end() {
            let character = self.character(position)?;
            if character.raw_end() > range.end() {
                return None;
            }
            result.push(character.value().as_char()?);
            position = character.raw_end();
        }
        (position == range.end()).then_some(Cow::Owned(result))
    }
}

fn source_prefix_ranges(input: &dyn Input) -> Vec<TextRange> {
    let mut ranges = Vec::with_capacity(2);
    let mut shebang_start = TextSize::from(0);
    if let Some(character) = raw_character(input, shebang_start)
        && character.value().as_u32() == BYTE_ORDER_MARK
    {
        ranges.push(TextRange::new(shebang_start, character.raw_end()));
        shebang_start = character.raw_end();
    }
    if let Some(shebang_end) = shebang_end(input, shebang_start) {
        ranges.push(TextRange::new(shebang_start, shebang_end));
    }
    ranges
}

fn shebang_end(input: &dyn Input, start: TextSize) -> Option<TextSize> {
    let mut prefix = RawCharacters::new(input, start);
    if next_value(&mut prefix) != Some(HASH) || next_value(&mut prefix) != Some(BANG) {
        return None;
    }

    let mut lookahead = RawCharacters::new(input, prefix.position()).peekable();
    if next_significant_is_left_bracket(&mut lookahead) {
        return None;
    }

    let line = RawCharacters::new(input, start);
    for (position, character) in line {
        if character.value().as_u32() == LINE_FEED {
            return Some(position);
        }
    }
    Some(input.len())
}

fn next_significant_is_left_bracket(characters: &mut Peekable<RawCharacters<'_>>) -> bool {
    loop {
        while peek_value(characters).is_some_and(is_whitespace) {
            characters.next();
        }
        if peek_value(characters) != Some(SLASH) {
            return peek_value(characters) == Some(LEFT_BRACKET);
        }

        characters.next();
        match peek_value(characters) {
            Some(SLASH) => {
                characters.next();
                while let Some(value) = next_value(characters) {
                    if value == LINE_FEED {
                        break;
                    }
                }
            }
            Some(STAR) => {
                characters.next();
                if !skip_block_comment(characters) {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

fn skip_block_comment(characters: &mut Peekable<RawCharacters<'_>>) -> bool {
    let mut depth = 1usize;
    while let Some(value) = next_value(characters) {
        if value == SLASH && peek_value(characters) == Some(STAR) {
            characters.next();
            depth += 1;
        } else if value == STAR && peek_value(characters) == Some(SLASH) {
            characters.next();
            depth -= 1;
            if depth == 0 {
                return true;
            }
        }
    }
    false
}

#[derive(Clone)]
struct RawCharacters<'a> {
    input: &'a dyn Input,
    position: TextSize,
}

impl<'a> RawCharacters<'a> {
    fn new(input: &'a dyn Input, position: TextSize) -> Self {
        Self { input, position }
    }

    fn position(&self) -> TextSize {
        self.position
    }
}

impl Iterator for RawCharacters<'_> {
    type Item = (TextSize, InputCharacter);

    fn next(&mut self) -> Option<Self::Item> {
        if self.position >= self.input.len() {
            return None;
        }
        let start = self.position;
        let character = raw_character(self.input, start)?;
        self.position = character.raw_end();
        Some((start, character))
    }
}

fn peek_value(characters: &mut Peekable<RawCharacters<'_>>) -> Option<u32> {
    characters
        .peek()
        .map(|(_, character)| character.value().as_u32())
}

fn next_value(characters: &mut impl Iterator<Item = (TextSize, InputCharacter)>) -> Option<u32> {
    characters
        .next()
        .map(|(_, character)| character.value().as_u32())
}

fn raw_character(input: &dyn Input, from: TextSize) -> Option<InputCharacter> {
    if let Some(chunk) = input.identity_chunk(from)
        && let Some(character) = chunk.character(from)
    {
        return Some(character);
    }
    if !input.is_boundary(from) {
        return None;
    }
    let character = input.chunk(from).chars().next()?;
    let width = TextSize::try_from(character.len_utf8()).ok()?;
    Some(InputCharacter::new(
        CodePoint::from(character),
        from + width,
    ))
}

fn raw_character_before(input: &dyn Input, before: TextSize) -> Option<(TextSize, InputCharacter)> {
    input.character_before(before)
}

pub(crate) const fn is_whitespace(value: u32) -> bool {
    matches!(
        value,
        0x0009
            | 0x000a
            | 0x000b
            | 0x000c
            | 0x000d
            | SPACE
            | 0x0085
            | 0x200e
            | 0x200f
            | 0x2028
            | 0x2029
    )
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
            let mut to = (from + self.chunk_length).min(self.source.len());
            while to > from && !self.source.is_char_boundary(to) {
                to -= 1;
            }
            if to == from && from < self.source.len() {
                let width = self.source[from..]
                    .chars()
                    .next()
                    .expect("input position precedes one character")
                    .len_utf8();
                to = from + width;
            }
            Cow::Borrowed(&self.source[from..to])
        }

        fn read(&self, range: TextRange) -> Cow<'_, str> {
            Cow::Borrowed(&self.source[usize::from(range.start())..usize::from(range.end())])
        }

        fn is_boundary(&self, position: TextSize) -> bool {
            let position = usize::from(position);
            position <= self.source.len() && self.source.is_char_boundary(position)
        }
    }

    fn input(source: &str) -> RustInput {
        RustInput::new(Arc::new(StringInput::try_new(source).unwrap()))
    }

    fn logical(source: &str) -> String {
        input(source)
            .scalar_text(TextRange::new(0.into(), source.len().try_into().unwrap()))
            .expect("Rust source preprocessing stays scalar-valued")
            .into_owned()
    }

    #[test]
    fn removes_the_source_prefix_without_changing_raw_boundaries() {
        let source = "\u{feff}#!/usr/bin/env rustx\nfn main() {}";
        let input = input(source);
        let logical = logical(source);
        let prefix = logical.strip_suffix("fn main() {}").unwrap();

        assert!(
            prefix
                .chars()
                .all(|character| matches!(character, ' ' | '\n'))
        );
        for (position, _) in source.char_indices() {
            assert!(input.is_boundary(position.try_into().unwrap()));
        }
        assert!(input.is_boundary(source.len().try_into().unwrap()));
    }

    #[test]
    fn scans_the_source_prefix_across_input_chunks() {
        let source: Arc<str> = Arc::from("\u{feff}#!/usr/bin/env rustx\nfn main() {}");
        let input = RustInput::new(Arc::new(ChunkedInput {
            source: Arc::clone(&source),
            chunk_length: 2,
        }));
        let logical = input
            .scalar_text(TextRange::new(0.into(), source.len().try_into().unwrap()))
            .unwrap();
        assert!(logical.ends_with("fn main() {}"));
    }

    #[test]
    fn preserves_inner_attributes_after_reference_trivia() {
        for source in [
            "#! /* outer /* inner */ tail */ [allow(dead_code)]\nfn main() {}",
            "#! // ordinary comment\n [allow(dead_code)]\nfn main() {}",
            "#! /// documentation comment\n [allow(dead_code)]\nfn main() {}",
        ] {
            assert!(input(source).hidden_ranges.is_empty(), "{source:?}");
        }
    }

    #[test]
    fn whitespace_matches_the_rust_reference_set() {
        for value in [
            0x0009, 0x000a, 0x000b, 0x000c, 0x000d, 0x0020, 0x0085, 0x200e, 0x200f, 0x2028, 0x2029,
        ] {
            assert!(is_whitespace(value), "U+{value:04X}");
        }
        for value in [0x0008, 0x00a0, 0x1680, BYTE_ORDER_MARK] {
            assert!(!is_whitespace(value), "U+{value:04X}");
        }
    }
}
