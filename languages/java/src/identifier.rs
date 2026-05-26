use rezel_common::ParseError;
use rezel_lr::{ExternalTokenizer, InputStream, Stack, TokenizerFlags};

#[path = "unicode17.rs"]
mod unicode17;

pub(crate) static TOKENIZER: ExternalTokenizer = ExternalTokenizer::new(
    scan,
    TokenizerFlags {
        contextual: false,
        fallback: false,
        extend: false,
    },
);

fn scan(input: &mut InputStream, _stack: &Stack) -> Result<(), ParseError> {
    let Some((first, width)) = next_code_point(input, 0) else {
        return Ok(());
    };
    if !is_java_identifier_start(first) {
        return Ok(());
    }

    if first <= 0x7f {
        input.advance_ascii_while(|byte| is_java_identifier_part(u32::from(byte)));
    } else {
        input.advance(width);
    }
    loop {
        if input.advance_ascii_while(|byte| is_java_identifier_part(u32::from(byte))) != 0 {
            continue;
        }
        let Some((character, width)) = next_code_point(input, 0) else {
            break;
        };
        if !is_java_identifier_part(character) {
            break;
        }
        input.advance(width);
    }
    input.accept_token(crate::terms::identifier, 0)
}

fn next_code_point(input: &mut InputStream, offset: isize) -> Option<(u32, usize)> {
    let first = input.peek(offset)?;
    if !(0xd800..=0xdbff).contains(&first) {
        return Some((u32::from(first), 1));
    }
    let second = input.peek(offset + 1)?;
    if !(0xdc00..=0xdfff).contains(&second) {
        return Some((u32::from(first), 1));
    }
    let high = u32::from(first) - 0xd800;
    let low = u32::from(second) - 0xdc00;
    Some((0x1_0000 + (high << 10) + low, 2))
}

fn is_java_identifier_start(value: u32) -> bool {
    if value < 0xa0 {
        return matches!(value, 0x24 | 0x41..=0x5a | 0x5f | 0x61..=0x7a);
    }
    in_ranges(unicode17::JAVA_IDENTIFIER_START, value)
}

fn is_java_identifier_part(value: u32) -> bool {
    if value < 0xa0 {
        return matches!(
            value,
            0x00..=0x08
                | 0x0e..=0x1b
                | 0x24
                | 0x30..=0x39
                | 0x41..=0x5a
                | 0x5f
                | 0x61..=0x7a
                | 0x7f..=0x9f
        );
    }
    in_ranges(unicode17::JAVA_IDENTIFIER_PART, value)
}

fn in_ranges(ranges: &[(u32, u32)], value: u32) -> bool {
    let index = ranges.partition_point(|&(_, end)| end < value);
    ranges
        .get(index)
        .is_some_and(|&(start, end)| start <= value && value <= end)
}

/// Return the JLS identifier spelling after removing ignorable characters.
#[must_use]
pub(crate) fn canonical_name(spelling: &str) -> String {
    spelling
        .chars()
        .filter(|character| !in_ranges(unicode17::JAVA_IDENTIFIER_IGNORABLE, u32::from(*character)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::unicode17::{
        JAVA_IDENTIFIER_IGNORABLE, JAVA_IDENTIFIER_PART, JAVA_IDENTIFIER_START,
    };
    use super::*;

    #[test]
    fn generated_ranges_cover_java_identifier_categories() {
        assert!(in_ranges(JAVA_IDENTIFIER_START, u32::from('$')));
        assert!(in_ranges(JAVA_IDENTIFIER_START, 0x20ac));
        assert!(!in_ranges(JAVA_IDENTIFIER_START, 0x200c));
        assert!(in_ranges(JAVA_IDENTIFIER_PART, 0x200c));
        assert!(in_ranges(JAVA_IDENTIFIER_IGNORABLE, 0x200c));
        assert!(!in_ranges(JAVA_IDENTIFIER_PART, u32::from('😀')));
    }

    #[test]
    fn latin_one_fast_path_matches_generated_identifier_ranges() {
        for value in 0..0xa0 {
            assert_eq!(
                is_java_identifier_start(value),
                in_ranges(JAVA_IDENTIFIER_START, value)
            );
            assert_eq!(
                is_java_identifier_part(value),
                in_ranges(JAVA_IDENTIFIER_PART, value)
            );
        }
    }

    #[test]
    fn canonical_names_remove_ignorable_characters() {
        assert_eq!(canonical_name("a\u{200c}b"), "ab");
        assert_eq!(canonical_name("€value"), "€value");
    }
}
