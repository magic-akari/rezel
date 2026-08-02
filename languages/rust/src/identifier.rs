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

enum IdentifierSpelling {
    Ascii(String),
    NonAscii,
}

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
    let Some(name) = scan_captured_identifier_body(input) else {
        return Ok(());
    };
    if current(input).is_some_and(is_reserved_prefix_delimiter) {
        return Ok(());
    }
    if name.is_ascii(MACRO_RULES) && macro_rules_definition_follows(input) {
        input.accept_token(terms::macroRulesKeyword)?;
    }
    Ok(())
}

fn scan_token_identifier(input: &mut InputStream, _stack: &Stack) -> Result<(), ParseError> {
    scan_identifier_as(input, terms::tokenIdentifier)
}

fn scan_identifier(input: &mut InputStream, _stack: &Stack) -> Result<(), ParseError> {
    scan_identifier_as(input, terms::identifier)
}

fn scan_identifier_as(input: &mut InputStream, term: u16) -> Result<(), ParseError> {
    let raw = current(input) == Some(LOWER_R) && peek(input, 1) == Some(HASH);
    if raw {
        input.advance(2);
        let Some(name) = scan_captured_identifier_body(input) else {
            return Ok(());
        };
        if name.is_reserved_raw_name() {
            return Ok(());
        }
    } else {
        if !scan_untracked_identifier_body(input) {
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
        let Some(name) = scan_captured_identifier_body(input) else {
            return Ok(());
        };
        if current(input) == Some(QUOTE) || name.is_reserved_raw_name() {
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
    true
}

fn advance_untracked_ascii_identifier(input: &mut InputStream) -> usize {
    input.advance_ascii_while(|byte| is_xid_continue(u32::from(byte)))
}

fn scan_captured_identifier_body(input: &mut InputStream) -> Option<IdentifierSpelling> {
    let first = current(input)?;
    if first != UNDERSCORE && !is_xid_start(first) {
        return None;
    }

    let mut spelling = IdentifierSpelling::Ascii(String::new());
    if first < 0x80 {
        advance_captured_ascii_identifier(input, &mut spelling);
    } else {
        push_ascii(&mut spelling, first);
        input.advance(1);
        advance_captured_ascii_identifier(input, &mut spelling);
    }
    while let Some(value) = current(input) {
        if value < 0x80 || !is_xid_continue(value) {
            break;
        }
        push_ascii(&mut spelling, value);
        input.advance(1);
        advance_captured_ascii_identifier(input, &mut spelling);
    }
    Some(spelling)
}

fn advance_captured_ascii_identifier(
    input: &mut InputStream,
    spelling: &mut IdentifierSpelling,
) -> usize {
    match spelling {
        IdentifierSpelling::NonAscii => {
            input.advance_ascii_while(|byte| is_xid_continue(u32::from(byte)))
        }
        IdentifierSpelling::Ascii(text) => input.advance_ascii_while(|byte| {
            if !is_xid_continue(u32::from(byte)) {
                return false;
            }
            text.push(char::from(byte));
            true
        }),
    }
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
    fn is_ascii(&self, expected: &str) -> bool {
        matches!(self, Self::Ascii(name) if name == expected)
    }

    fn is_reserved_raw_name(&self) -> bool {
        matches!(self, Self::Ascii(name) if is_reserved_raw_name(name))
    }
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
        let mut spelling = IdentifierSpelling::Ascii(String::new());
        push_ascii(&mut spelling, first);
        while tokens.peek().copied().is_some_and(is_xid_continue) {
            let value = tokens.next().expect("peeked identifier continuation");
            push_ascii(&mut spelling, value);
        }
        return !spelling.is_reserved_raw_name();
    }

    while tokens.peek().copied().is_some_and(is_xid_continue) {
        tokens.next();
    }
    tokens
        .peek()
        .copied()
        .is_none_or(|value| !is_reserved_prefix_delimiter(value))
}

fn is_reserved_raw_name(name: &str) -> bool {
    matches!(name, "_" | "crate" | "self" | "Self" | "super")
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
        for name in ["_", "crate", "self", "Self", "super"] {
            assert!(is_reserved_raw_name(name));
        }
        assert!(!is_reserved_raw_name("gen"));
        assert!(!is_reserved_raw_name("selfish"));
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
