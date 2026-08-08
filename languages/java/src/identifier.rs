use rezel_common::ParseError;
use rezel_lr::{ExternalTokenizer, ExternalTokenizerStart, InputStream, Stack, TokenizerFlags};

#[path = "unicode17.rs"]
mod unicode17;

const IDENTIFIER_START: ExternalTokenizerStart = ExternalTokenizerStart::NONE
    .with_ascii(b'$')
    .with_ascii(b'_')
    .with_ascii_range(b'A'..=b'Z')
    .with_ascii_range(b'a'..=b'z')
    .with_non_ascii();

pub(crate) static TOKENIZER: ExternalTokenizer = ExternalTokenizer::new(
    scan,
    TokenizerFlags {
        contextual: false,
        fallback: false,
        extend: false,
    },
)
.with_start(IDENTIFIER_START);

fn scan(input: &mut InputStream, _stack: &Stack) -> Result<(), ParseError> {
    let Some(first) = input.next().map(rezel_common::CodePoint::as_u32) else {
        return Ok(());
    };
    if !is_java_identifier_start(first) {
        return Ok(());
    }

    if first <= 0x7f {
        input.advance_ascii_while(|byte| is_java_identifier_part(u32::from(byte)));
    } else {
        input.advance(1);
    }
    loop {
        if input.advance_ascii_while(|byte| is_java_identifier_part(u32::from(byte))) != 0 {
            continue;
        }
        let Some(character) = input.next().map(rezel_common::CodePoint::as_u32) else {
            break;
        };
        if !is_java_identifier_part(character) {
            break;
        }
        input.advance(1);
    }
    input.accept_token(crate::terms::identifier)
}

pub(crate) fn specialize_record(value: &str, stack: &Stack) -> Option<u16> {
    if value != "record" {
        return None;
    }
    stack
        .can_shift(crate::terms::record)
        .then_some(crate::terms::record)
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
