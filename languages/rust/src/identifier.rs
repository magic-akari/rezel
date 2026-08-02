use rezel_common::{CodePoint, ParseError};
use rezel_lr::{ExternalTokenizer, ExternalTokenizerStart, InputStream, Stack, TokenizerFlags};
use std::iter::Peekable;

use crate::{input::is_whitespace, terms};

const BANG: u32 = b'!' as u32;
const HASH: u32 = b'#' as u32;
const LINE_FEED: u32 = b'\n' as u32;
const QUOTE: u32 = b'\'' as u32;
const DOUBLE_QUOTE: u32 = b'"' as u32;
const LOWER_R: u32 = b'r' as u32;
const SLASH: u32 = b'/' as u32;
const STAR: u32 = b'*' as u32;
const UNDERSCORE: u32 = b'_' as u32;
const MACRO_RULES: &str = "macro_rules";

/// Unicode version used by Rust 1.95 identifiers.
pub(crate) const UNICODE_VERSION: &str = "17.0.0";

const IDENTIFIER_START: ExternalTokenizerStart = ExternalTokenizerStart::NONE
    .with_ascii(b'_')
    .with_ascii_range(b'A'..=b'Z')
    .with_ascii_range(b'a'..=b'z')
    .with_non_ascii();
const MACRO_RULES_START: ExternalTokenizerStart = ExternalTokenizerStart::NONE.with_ascii(b'm');
const LIFETIME_START: ExternalTokenizerStart = ExternalTokenizerStart::NONE.with_ascii(b'\'');
const METAVARIABLE_START: ExternalTokenizerStart = ExternalTokenizerStart::NONE.with_ascii(b'$');

const FLAGS: TokenizerFlags = TokenizerFlags {
    contextual: false,
    fallback: false,
    extend: false,
};

pub(crate) static MACRO_RULES_TOKENIZER: ExternalTokenizer =
    ExternalTokenizer::new(scan_macro_rules, FLAGS).with_start(MACRO_RULES_START);
pub(crate) static TOKEN_IDENTIFIER_TOKENIZER: ExternalTokenizer =
    ExternalTokenizer::new(scan_token_identifier, FLAGS).with_start(IDENTIFIER_START);
pub(crate) static IDENTIFIER_TOKENIZER: ExternalTokenizer =
    ExternalTokenizer::new(scan_identifier, FLAGS).with_start(IDENTIFIER_START);
pub(crate) static LIFETIME_TOKENIZER: ExternalTokenizer =
    ExternalTokenizer::new(scan_lifetime, FLAGS).with_start(LIFETIME_START);
pub(crate) static METAVARIABLE_TOKENIZER: ExternalTokenizer =
    ExternalTokenizer::new(scan_metavariable, FLAGS).with_start(METAVARIABLE_START);

fn scan_macro_rules(input: &mut InputStream, _stack: &Stack) -> Result<(), ParseError> {
    if !scan_macro_rules_name(input) {
        return Ok(());
    }
    if current(input).is_some_and(is_reserved_prefix_delimiter) {
        return Ok(());
    }
    if macro_rules_definition_follows(input) {
        input.accept_token(terms::macroRulesKeyword)?;
    }
    Ok(())
}

fn scan_macro_rules_name(input: &mut InputStream) -> bool {
    let expected = MACRO_RULES.as_bytes();
    let mut matched = 0;
    input.advance_ascii_while(|byte| {
        if expected.get(matched) != Some(&byte) {
            return false;
        }
        matched += 1;
        true
    });
    matched == expected.len() && current(input).is_none_or(|value| !is_xid_continue(value))
}

fn scan_token_identifier(input: &mut InputStream, _stack: &Stack) -> Result<(), ParseError> {
    scan_identifier_as(input, terms::tokenIdentifier)
}

fn scan_identifier(input: &mut InputStream, _stack: &Stack) -> Result<(), ParseError> {
    scan_identifier_as(input, terms::identifier)
}

fn scan_identifier_as(input: &mut InputStream, term: u16) -> Result<(), ParseError> {
    let Some(first) = current(input) else {
        return Ok(());
    };
    let raw = first == LOWER_R && peek(input, 1) == Some(HASH);
    if raw {
        input.advance(2);
        let Some(reserved) = scan_raw_identifier_body(input) else {
            return Ok(());
        };
        if reserved {
            return Ok(());
        }
    } else {
        if !scan_filtered_untracked_identifier_body(input, first) {
            return Ok(());
        }
        if current(input).is_some_and(is_reserved_prefix_delimiter) {
            return Ok(());
        }
    }

    input.accept_token(term)
}

fn scan_metavariable(input: &mut InputStream, _stack: &Stack) -> Result<(), ParseError> {
    input.advance(1);
    if !scan_untracked_identifier_body(input) {
        return Ok(());
    }
    input.accept_token(terms::Metavariable)
}

fn scan_lifetime(input: &mut InputStream, _stack: &Stack) -> Result<(), ParseError> {
    input.advance(1);
    let raw = current(input) == Some(LOWER_R) && peek(input, 1) == Some(HASH);
    if raw {
        input.advance(2);
        let Some(reserved) = scan_raw_identifier_body(input) else {
            return Ok(());
        };
        if current(input) == Some(QUOTE) || reserved {
            return Ok(());
        }
    } else {
        if !scan_untracked_identifier_body(input) {
            return Ok(());
        }
        if matches!(current(input), Some(QUOTE | HASH)) {
            return Ok(());
        }
    }
    input.accept_token(terms::quoteIdentifier)
}

fn scan_untracked_identifier_body(input: &mut InputStream) -> bool {
    let Some(first) = current(input) else {
        return false;
    };
    if first != UNDERSCORE && !is_xid_start(first) {
        return false;
    }

    scan_valid_untracked_identifier_body(input, first);
    true
}

fn scan_filtered_untracked_identifier_body(input: &mut InputStream, first: u32) -> bool {
    if first >= 0x80 && !is_xid_start(first) {
        return false;
    }

    scan_valid_untracked_identifier_body(input, first);
    true
}

fn scan_valid_untracked_identifier_body(input: &mut InputStream, first: u32) {
    if first < 0x80 {
        advance_untracked_ascii_identifier(input);
    } else {
        input.advance(1);
        advance_untracked_ascii_identifier(input);
    }
    while let Some(value) = current(input) {
        if value < 0x80 || !is_xid_continue(value) {
            break;
        }
        input.advance(1);
        advance_untracked_ascii_identifier(input);
    }
}

fn advance_untracked_ascii_identifier(input: &mut InputStream) -> usize {
    input.advance_ascii_while(|byte| is_xid_continue(u32::from(byte)))
}

fn scan_raw_identifier_body(input: &mut InputStream) -> Option<bool> {
    let start = input.position();
    if !scan_untracked_identifier_body(input) {
        return None;
    }
    let name = input.read_scalar(start, input.position())?;
    Some(is_reserved_raw_name(&name))
}

fn macro_rules_definition_follows(input: &InputStream) -> bool {
    // rustc's `is_macro_rules_item` makes the same token-level decision. Keep
    // that bounded syntactic lookahead in the tokenizer instead of opening an
    // LR branch whose lifetime depends on the macro body.
    let lookahead = input.lookahead().map(CodePoint::as_u32);
    macro_rules_definition_tokens_follow(lookahead)
}

fn macro_rules_definition_tokens_follow(tokens: impl IntoIterator<Item = u32>) -> bool {
    let mut tokens = tokens.into_iter().peekable();
    if !skip_trivia(&mut tokens) || tokens.next() != Some(BANG) {
        return false;
    }
    skip_trivia(&mut tokens) && next_token_is_identifier(&mut tokens)
}

fn skip_trivia<I>(tokens: &mut Peekable<I>) -> bool
where
    I: Iterator<Item = u32>,
{
    loop {
        while tokens.peek().copied().is_some_and(is_whitespace) {
            tokens.next();
        }
        if tokens.peek().copied() != Some(SLASH) {
            return true;
        }

        tokens.next();
        match tokens.peek().copied() {
            Some(SLASH) => {
                tokens.next();
                while tokens
                    .peek()
                    .copied()
                    .is_some_and(|value| value != LINE_FEED)
                {
                    tokens.next();
                }
            }
            Some(STAR) => {
                tokens.next();
                if !skip_block_comment(tokens) {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

fn skip_block_comment<I>(tokens: &mut Peekable<I>) -> bool
where
    I: Iterator<Item = u32>,
{
    let mut depth = 1usize;
    while let Some(value) = tokens.next() {
        if value == SLASH && tokens.peek().copied() == Some(STAR) {
            tokens.next();
            depth += 1;
        } else if value == STAR && tokens.peek().copied() == Some(SLASH) {
            tokens.next();
            depth -= 1;
            if depth == 0 {
                return true;
            }
        }
    }
    false
}

fn next_token_is_identifier<I>(tokens: &mut Peekable<I>) -> bool
where
    I: Iterator<Item = u32>,
{
    let Some(mut first) = tokens.next() else {
        return false;
    };
    let raw = first == LOWER_R && tokens.peek().copied() == Some(HASH);
    if raw {
        tokens.next();
        let Some(raw_first) = tokens.next() else {
            return false;
        };
        first = raw_first;
    }
    if first != UNDERSCORE && !is_xid_start(first) {
        return false;
    }

    if raw {
        return !consume_reserved_raw_name(first, tokens);
    }

    while tokens.peek().copied().is_some_and(is_xid_continue) {
        tokens.next();
    }
    tokens
        .peek()
        .copied()
        .is_none_or(|value| !is_reserved_prefix_delimiter(value))
}

fn consume_reserved_raw_name<I>(first: u32, tokens: &mut Peekable<I>) -> bool
where
    I: Iterator<Item = u32>,
{
    let mut spelling = [0_u8; 5];
    let mut length = 0;
    let mut possible = push_reserved_raw_name_byte(&mut spelling, &mut length, first);
    while tokens.peek().copied().is_some_and(is_xid_continue) {
        let value = tokens.next().expect("peeked identifier continuation");
        if possible {
            possible = push_reserved_raw_name_byte(&mut spelling, &mut length, value);
        }
    }
    possible && is_reserved_raw_name_bytes(&spelling[..length])
}

fn push_reserved_raw_name_byte(spelling: &mut [u8; 5], length: &mut usize, value: u32) -> bool {
    let Ok(byte) = u8::try_from(value) else {
        return false;
    };
    let Some(slot) = spelling.get_mut(*length) else {
        return false;
    };
    *slot = byte;
    *length += 1;
    true
}

fn is_reserved_raw_name(name: &str) -> bool {
    is_reserved_raw_name_bytes(name.as_bytes())
}

fn is_reserved_raw_name_bytes(name: &[u8]) -> bool {
    matches!(name, b"_" | b"crate" | b"self" | b"Self" | b"super")
}

fn is_reserved_prefix_delimiter(value: u32) -> bool {
    matches!(value, HASH | QUOTE | DOUBLE_QUOTE)
}

fn is_xid_start(value: u32) -> bool {
    if value < 0x80 {
        let value = u8::try_from(value).expect("ASCII code points fit in u8");
        return value.is_ascii_alphabetic();
    }
    char::from_u32(value).is_some_and(unicode_ident::is_xid_start)
}

fn is_xid_continue(value: u32) -> bool {
    if value < 0x80 {
        let value = u8::try_from(value).expect("ASCII code points fit in u8");
        return value.is_ascii_alphanumeric() || value == b'_';
    }
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
    fn ascii_fast_path_matches_unicode_ident_across_the_byte_range() {
        for value in 0..=u32::from(u8::MAX) {
            let character = char::from_u32(value).expect("byte values are Unicode scalars");
            assert_eq!(is_xid_start(value), unicode_ident::is_xid_start(character));
            assert_eq!(
                is_xid_continue(value),
                unicode_ident::is_xid_continue(character)
            );
        }
    }

    #[test]
    fn reserved_raw_names_match_the_reference() {
        for (name, reserved) in [
            ("_", true),
            ("crate", true),
            ("self", true),
            ("Self", true),
            ("super", true),
            ("gen", false),
            ("selfish", false),
            ("super_long", false),
            ("crate東", false),
        ] {
            assert_eq!(is_reserved_raw_name(name), reserved);
            let mut characters = name.chars().map(u32::from);
            let first = characters.next().expect("raw-name fixture is nonempty");
            assert_eq!(
                consume_reserved_raw_name(first, &mut characters.peekable()),
                reserved
            );
        }
    }

    #[test]
    fn macro_rules_definition_lookahead_skips_nested_trivia() {
        let source = " /* outer /* inner */ tail */ ! // name\n r#generated";
        assert!(macro_rules_definition_tokens_follow(
            source.chars().map(u32::from)
        ));
    }

    #[test]
    fn macro_rules_invocation_is_not_a_definition() {
        for source in ["! {}", "! ()", "! /* missing name */ {"] {
            assert!(!macro_rules_definition_tokens_follow(
                source.chars().map(u32::from)
            ));
        }
    }
}
