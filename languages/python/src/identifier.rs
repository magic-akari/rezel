use rezel_common::{CodePoint, ParseError};
use rezel_lr::{ExternalTokenizer, InputStream, Stack, TokenizerFlags};
use unicode_normalization::UnicodeNormalization;

#[path = "unicode16.rs"]
mod unicode16;

pub(crate) const UNICODE_VERSION: &str = unicode16::UNICODE_VERSION;

pub(crate) fn canonical_name(spelling: &str) -> String {
    spelling.nfkc().collect()
}

pub(crate) static TOKENIZER: ExternalTokenizer = ExternalTokenizer::new(
    scan,
    TokenizerFlags {
        contextual: false,
        fallback: false,
        extend: false,
    },
);

fn scan(input: &mut InputStream, _stack: &Stack) -> Result<(), ParseError> {
    if looks_like_string_prefix(input) {
        return Ok(());
    }
    let Some(first) = input.next().map(CodePoint::as_u32) else {
        return Ok(());
    };
    if !is_identifier_start(first) {
        return Ok(());
    }
    if first < 0x80 {
        input.advance_ascii_while(|byte| is_identifier_continue(u32::from(byte)));
    } else {
        input.advance(1);
    }
    loop {
        if input.advance_ascii_while(|byte| is_identifier_continue(u32::from(byte))) != 0 {
            continue;
        }
        let Some(character) = input.next().map(CodePoint::as_u32) else {
            break;
        };
        if !is_identifier_continue(character) {
            break;
        }
        input.advance(1);
    }
    input.accept_token(crate::terms::identifier)
}

fn looks_like_string_prefix(input: &InputStream) -> bool {
    let Some(first) = input.peek(0).and_then(ascii_lowercase) else {
        return false;
    };
    let second = input.peek(1);
    if matches!(second.map(CodePoint::as_u32), Some(0x27 | 0x22)) {
        return matches!(first, b'b' | b'f' | b'r' | b't' | b'u');
    }
    let Some(second_prefix) = second.and_then(ascii_lowercase) else {
        return false;
    };
    if !matches!(input.peek(2).map(CodePoint::as_u32), Some(0x27 | 0x22)) {
        return false;
    }
    matches!(
        (first, second_prefix),
        (b'b' | b'f' | b't', b'r') | (b'r', b'b' | b'f' | b't')
    )
}

fn ascii_lowercase(value: CodePoint) -> Option<u8> {
    let value = u8::try_from(value.as_u32()).ok()?;
    value
        .is_ascii_alphabetic()
        .then(|| value.to_ascii_lowercase())
}

fn is_identifier_start(value: u32) -> bool {
    if value < 0x80 {
        let value = u8::try_from(value).expect("ASCII code points fit in u8");
        return value.is_ascii_alphabetic() || value == b'_';
    }
    in_ranges(unicode16::XID_START, value)
}

fn is_identifier_continue(value: u32) -> bool {
    if value < 0x80 {
        let value = u8::try_from(value).expect("ASCII code points fit in u8");
        return value.is_ascii_alphanumeric() || value == b'_';
    }
    in_ranges(unicode16::XID_CONTINUE, value)
}

fn in_ranges(ranges: &[(u32, u32)], value: u32) -> bool {
    let index = ranges.partition_point(|&(_, end)| end < value);
    ranges
        .get(index)
        .is_some_and(|&(start, end)| start <= value && value <= end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tables_are_unicode_16() {
        assert_eq!(unicode16::UNICODE_VERSION, "16.0.0");
        assert_eq!(unicode_normalization::UNICODE_VERSION, (16, 0, 0));
        assert!(is_identifier_start(u32::from('_')));
        assert!(is_identifier_start(u32::from('λ')));
        assert!(is_identifier_continue(u32::from('9')));
        assert!(!is_identifier_start(u32::from('9')));
        assert!(!is_identifier_continue(u32::from('😀')));
    }

    #[test]
    fn ascii_fast_path_matches_generated_tables_across_the_byte_range() {
        for value in 0..=u32::from(u8::MAX) {
            assert_eq!(
                is_identifier_start(value),
                in_ranges(unicode16::XID_START, value)
            );
            assert_eq!(
                is_identifier_continue(value),
                in_ranges(unicode16::XID_CONTINUE, value)
            );
        }
    }

    #[test]
    #[allow(clippy::unicode_not_nfc)] // The compatibility character is the test input.
    fn canonical_names_follow_cpython_nfkc() {
        assert_eq!(canonical_name("Kelvin"), "Kelvin");
        assert_eq!(canonical_name("变量"), "变量");
    }
}
