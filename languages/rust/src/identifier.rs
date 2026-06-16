use rezel_common::{CodePoint, ParseError};
use rezel_lr::{ExternalTokenizer, InputStream, Stack, TokenizerFlags};
use std::iter::Peekable;

use crate::terms;

const BANG: u32 = b'!' as u32;
const CARRIAGE_RETURN: u32 = b'\r' as u32;
const DOLLAR: u32 = b'$' as u32;
const HASH: u32 = b'#' as u32;
const LINE_FEED: u32 = b'\n' as u32;
const LOWER_M: u32 = b'm' as u32;
const QUOTE: u32 = b'\'' as u32;
const DOUBLE_QUOTE: u32 = b'"' as u32;
const LOWER_R: u32 = b'r' as u32;
const SLASH: u32 = b'/' as u32;
const SPACE: u32 = b' ' as u32;
const STAR: u32 = b'*' as u32;
const TAB: u32 = b'\t' as u32;
const UNDERSCORE: u32 = b'_' as u32;
const MACRO_RULES: &str = "macro_rules";

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

    let macro_rules_candidate =
        !raw && current(input) == Some(LOWER_M) && stack.can_shift(terms::macroRulesKeyword);
    let Some(name) = scan_identifier_body(input, raw || macro_rules_candidate) else {
        return Ok(());
    };
    if raw {
        if name.is_reserved_raw_name() {
            return Ok(());
        }
    } else if current(input).is_some_and(is_reserved_prefix_delimiter) {
        return Ok(());
    }

    let term = if macro_rules_candidate
        && name.is_ascii(MACRO_RULES)
        && macro_rules_definition_follows(input)
    {
        terms::macroRulesKeyword
    } else if stack.can_shift(terms::tokenIdentifier) {
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

    let mut spelling = if raw {
        IdentifierSpelling::Ascii(String::new())
    } else {
        IdentifierSpelling::Untracked
    };
    push_ascii(&mut spelling, first);
    while tokens.peek().copied().is_some_and(is_xid_continue) {
        let value = tokens.next().expect("peeked identifier continuation");
        push_ascii(&mut spelling, value);
    }

    if raw {
        !spelling.is_reserved_raw_name()
    } else {
        tokens
            .peek()
            .copied()
            .is_none_or(|value| !is_reserved_prefix_delimiter(value))
    }
}

fn is_reserved_raw_name(name: &str) -> bool {
    matches!(name, "_" | "crate" | "self" | "Self" | "super")
}

fn is_reserved_prefix_delimiter(value: u32) -> bool {
    matches!(value, HASH | QUOTE | DOUBLE_QUOTE)
}

fn is_whitespace(value: u32) -> bool {
    matches!(value, SPACE | TAB | CARRIAGE_RETURN | LINE_FEED)
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
