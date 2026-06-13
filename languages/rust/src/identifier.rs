use rezel_common::{CodePoint, ParseError};
use rezel_lr::{ExternalTokenizer, InputStream, Stack, TokenizerFlags};

use crate::terms;

const DOLLAR: u32 = b'$' as u32;
const HASH: u32 = b'#' as u32;
const QUOTE: u32 = b'\'' as u32;
const DOUBLE_QUOTE: u32 = b'"' as u32;
const LOWER_R: u32 = b'r' as u32;
const UNDERSCORE: u32 = b'_' as u32;

enum IdentifierSpelling {
    Untracked,
    Ascii(String),
    NonAscii,
}

/// Unicode version used by Rust 1.95 identifiers.
pub(crate) const UNICODE_VERSION: &str = "17.0.0";

pub(crate) static TOKENIZER: ExternalTokenizer = ExternalTokenizer::new(
    scan,
    TokenizerFlags {
        contextual: true,
        fallback: false,
        extend: false,
    },
);

fn scan(input: &mut InputStream, stack: &Stack) -> Result<(), ParseError> {
    match current(input) {
        Some(DOLLAR) if stack.can_shift(terms::Metavariable) => scan_metavariable(input),
        Some(QUOTE) if stack.can_shift(terms::quoteIdentifier) => scan_lifetime(input),
        Some(_) => scan_identifier(input, stack),
        None => Ok(()),
    }
}

fn scan_identifier(input: &mut InputStream, stack: &Stack) -> Result<(), ParseError> {
    let raw = current(input) == Some(LOWER_R) && peek(input, 1) == Some(HASH);
    if raw {
        input.advance(2);
    }

    let Some(name) = scan_identifier_body(input, raw) else {
        return Ok(());
    };
    if raw {
        if name.is_reserved_raw_name() {
            return Ok(());
        }
    } else if current(input).is_some_and(is_reserved_prefix_delimiter) {
        return Ok(());
    }

    let term = if stack.can_shift(terms::tokenIdentifier) {
        terms::tokenIdentifier
    } else {
        terms::identifier
    };
    input.accept_token(term)
}

fn scan_metavariable(input: &mut InputStream) -> Result<(), ParseError> {
    input.advance(1);
    if scan_identifier_body(input, false).is_none() {
        return Ok(());
    }
    input.accept_token(terms::Metavariable)
}

fn scan_lifetime(input: &mut InputStream) -> Result<(), ParseError> {
    input.advance(1);
    let raw = current(input) == Some(LOWER_R) && peek(input, 1) == Some(HASH);
    if raw {
        input.advance(2);
    }

    let Some(name) = scan_identifier_body(input, raw) else {
        return Ok(());
    };
    if current(input) == Some(QUOTE) {
        return Ok(());
    }
    if raw {
        if name.is_reserved_raw_name() {
            return Ok(());
        }
    } else if current(input) == Some(HASH) {
        return Ok(());
    }
    input.accept_token(terms::quoteIdentifier)
}

fn scan_identifier_body(
    input: &mut InputStream,
    capture_ascii: bool,
) -> Option<IdentifierSpelling> {
    let first = current(input)?;
    if first != UNDERSCORE && !is_xid_start(first) {
        return None;
    }

    let mut spelling = if capture_ascii {
        IdentifierSpelling::Ascii(String::new())
    } else {
        IdentifierSpelling::Untracked
    };
    push_ascii(&mut spelling, first);
    input.advance(1);
    while let Some(value) = current(input)
        && is_xid_continue(value)
    {
        push_ascii(&mut spelling, value);
        input.advance(1);
    }
    Some(spelling)
}

fn push_ascii(spelling: &mut IdentifierSpelling, value: u32) {
    let IdentifierSpelling::Ascii(text) = spelling else {
        return;
    };
    let Ok(byte) = u8::try_from(value) else {
        *spelling = IdentifierSpelling::NonAscii;
        return;
    };
    text.push(char::from(byte));
}

impl IdentifierSpelling {
    fn is_reserved_raw_name(&self) -> bool {
        matches!(self, Self::Ascii(name) if is_reserved_raw_name(name))
    }
}

fn is_reserved_raw_name(name: &str) -> bool {
    matches!(name, "_" | "crate" | "self" | "Self" | "super")
}

fn is_reserved_prefix_delimiter(value: u32) -> bool {
    matches!(value, HASH | QUOTE | DOUBLE_QUOTE)
}

fn is_xid_start(value: u32) -> bool {
    char::from_u32(value).is_some_and(unicode_ident::is_xid_start)
}

fn is_xid_continue(value: u32) -> bool {
    char::from_u32(value).is_some_and(unicode_ident::is_xid_continue)
}

fn current(input: &InputStream) -> Option<u32> {
    input.next().map(CodePoint::as_u32)
}

fn peek(input: &InputStream, offset: isize) -> Option<u32> {
    input.peek(offset).map(CodePoint::as_u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_profile_matches_rust_1_95() {
        assert_eq!(UNICODE_VERSION, "17.0.0");
        assert_eq!(unicode_ident::UNICODE_VERSION, (17, 0, 0));
        assert!(is_xid_start(u32::from('東')));
        assert!(is_xid_start(0x088f));
        assert!(is_xid_continue(u32::from('9')));
        assert!(!is_xid_start(u32::from('_')));
        assert!(!is_xid_continue(u32::from('😀')));
    }

    #[test]
    fn reserved_raw_names_match_the_reference() {
        for name in ["_", "crate", "self", "Self", "super"] {
            assert!(is_reserved_raw_name(name));
        }
        assert!(!is_reserved_raw_name("gen"));
        assert!(!is_reserved_raw_name("selfish"));
    }
}
