use std::sync::LazyLock;

use rezel_common::{CodePoint, ParseError};
use rezel_lr::{
    ContextTracker, ContextValue, ExternalTokenizer, ExternalTokenizerStart, InputStream, Stack,
    TokenizerFlags,
};

use crate::terms;

const NEWLINE: u32 = 10;
const CARRIAGE_RETURN: u32 = 13;
const SPACE: u32 = 32;
const TAB: u32 = 9;
const SLASH: u32 = 47;
const ASTERISK: u32 = 42;
const CLOSE_PAREN: u32 = 41;
const CLOSE_BRACE: u32 = 125;
const OPEN_BRACKET: u32 = 91;
const LESS_THAN: u32 = 60;
const MINUS: u32 = 45;

static BOOLEAN_CONTEXTS: LazyLock<[ContextValue; 2]> =
    LazyLock::new(|| [ContextValue::new(false), ContextValue::new(true)]);

const SEMICOLON_START: ExternalTokenizerStart = ExternalTokenizerStart::NONE
    .with_ascii(b'\n')
    .with_ascii(b'\r')
    .with_ascii(b' ')
    .with_ascii(b'\t')
    .with_ascii(b'/')
    .with_ascii(b')')
    .with_ascii(b'}')
    .with_end();

pub(crate) static SEMICOLON: ExternalTokenizer = ExternalTokenizer::new(
    scan_semicolon,
    TokenizerFlags {
        contextual: true,
        fallback: false,
        extend: false,
    },
)
.with_start(SEMICOLON_START);

const INDEX_TYPE_START: ExternalTokenizerStart = ExternalTokenizerStart::NONE
    .with_ascii(b'[')
    .with_ascii(b'<')
    .with_ascii(b'c')
    .with_ascii(b'f')
    .with_ascii(b'i')
    .with_ascii(b'm')
    .with_ascii(b's');

pub(crate) static INDEX_TYPE: ExternalTokenizer = ExternalTokenizer::new(
    scan_index_type,
    TokenizerFlags {
        contextual: false,
        fallback: false,
        extend: true,
    },
)
.with_start(INDEX_TYPE_START);

pub(crate) static TRACK_TOKENS: ContextTracker =
    ContextTracker::new(start_context, None, None, hash_context)
        .with_shift_without_input(shift_context);

fn scan_semicolon(input: &mut InputStream, stack: &Stack) -> Result<(), ParseError> {
    let context = stack.context::<bool>().copied().unwrap_or(false);
    let next = input.next().map(CodePoint::as_u32);
    let should_look_ahead = matches!(next, Some(SPACE | TAB | SLASH));
    let should_insert = if should_look_ahead {
        scan_semicolon_lookahead(input, context)
    } else {
        let line_end = matches!(next, None | Some(NEWLINE | CARRIAGE_RETURN));
        let closing_delimiter = matches!(next, Some(CLOSE_PAREN | CLOSE_BRACE));
        context && line_end || closing_delimiter
    };
    if should_insert {
        input.accept_token(terms::insertedSemi)?;
    }
    Ok(())
}

fn scan_index_type(input: &mut InputStream, stack: &Stack) -> Result<(), ParseError> {
    if !stack.can_shift(terms::indexTypeStart) {
        return Ok(());
    }
    let starts = {
        let mut lookahead = input.lookahead().map(CodePoint::as_u32).peekable();
        match lookahead.peek().copied() {
            Some(OPEN_BRACKET) => true,
            Some(LESS_THAN) => starts_receive_channel_type(&mut lookahead),
            Some(_) => starts_type_keyword(&mut lookahead),
            None => false,
        }
    };
    if starts {
        input.accept_token(terms::indexTypeStart)?;
    }
    Ok(())
}

fn starts_type_keyword(input: &mut std::iter::Peekable<impl Iterator<Item = u32>>) -> bool {
    match input.peek().copied() {
        Some(next) if next == u32::from(b'c') => next_word_is(input, b"chan"),
        Some(next) if next == u32::from(b'f') => next_word_is(input, b"func"),
        Some(next) if next == u32::from(b'i') => next_word_is(input, b"interface"),
        Some(next) if next == u32::from(b'm') => next_word_is(input, b"map"),
        Some(next) if next == u32::from(b's') => next_word_is(input, b"struct"),
        _ => false,
    }
}

fn starts_receive_channel_type(input: &mut std::iter::Peekable<impl Iterator<Item = u32>>) -> bool {
    if input.next() != Some(LESS_THAN) || input.next() != Some(MINUS) {
        return false;
    }
    skip_go_trivia(input) && next_word_is(input, b"chan")
}

fn next_word_is(
    input: &mut std::iter::Peekable<impl Iterator<Item = u32>>,
    expected: &[u8],
) -> bool {
    for &byte in expected {
        if input.next() != Some(u32::from(byte)) {
            return false;
        }
    }
    input.peek().copied().is_none_or(|next| {
        let is_ascii_alphanumeric =
            u8::try_from(next).is_ok_and(|next| next.is_ascii_alphanumeric());
        next < 128 && next != u32::from(b'_') && !is_ascii_alphanumeric
    })
}

fn skip_go_trivia(input: &mut std::iter::Peekable<impl Iterator<Item = u32>>) -> bool {
    loop {
        while matches!(
            input.peek().copied(),
            Some(SPACE | TAB | NEWLINE | CARRIAGE_RETURN)
        ) {
            input.next();
        }
        if input.peek() != Some(&SLASH) {
            return true;
        }
        input.next();
        match input.next() {
            Some(SLASH) => {
                for next in input.by_ref() {
                    if matches!(next, NEWLINE | CARRIAGE_RETURN) {
                        break;
                    }
                }
            }
            Some(ASTERISK) => {
                let mut closed = false;
                while let Some(next) = input.next() {
                    if next == ASTERISK && input.peek() == Some(&SLASH) {
                        input.next();
                        closed = true;
                        break;
                    }
                }
                if !closed {
                    return false;
                }
            }
            _ => return true,
        }
    }
}

fn scan_semicolon_lookahead(input: &InputStream, context: bool) -> bool {
    let mut lookahead = input.lookahead().map(CodePoint::as_u32).peekable();
    loop {
        while matches!(lookahead.peek().copied(), Some(SPACE | TAB)) {
            lookahead.next();
        }
        let Some(next) = lookahead.next() else {
            return context;
        };
        if matches!(next, NEWLINE | CARRIAGE_RETURN) {
            return context;
        }
        if next == SLASH && lookahead.peek() == Some(&SLASH) {
            return context;
        }
        if next == SLASH && lookahead.peek() == Some(&ASTERISK) {
            lookahead.next();
            if scan_block_comment(&mut lookahead) {
                return context;
            }
            continue;
        }
        return matches!(next, CLOSE_PAREN | CLOSE_BRACE);
    }
}

fn scan_block_comment(input: &mut std::iter::Peekable<impl Iterator<Item = u32>>) -> bool {
    loop {
        let Some(next) = input.next() else {
            return true;
        };
        if matches!(next, NEWLINE | CARRIAGE_RETURN) {
            return true;
        }
        if next == ASTERISK && input.peek() == Some(&SLASH) {
            input.next();
            return false;
        }
    }
}

fn start_context() -> ContextValue {
    boolean_context(false)
}

// Context transitions are fallible at the shared runtime boundary even though
// this particular transition is total.
#[allow(clippy::unnecessary_wraps)]
fn shift_context(
    context: &ContextValue,
    term: u16,
    _stack: &Stack,
) -> Result<ContextValue, ParseError> {
    let previous = context.downcast_ref::<bool>().copied().unwrap_or(false);
    let next = if term == terms::space {
        previous
    } else {
        is_semicolon_predecessor(term)
    };
    if next == previous {
        Ok(context.clone())
    } else {
        Ok(boolean_context(next))
    }
}

fn boolean_context(value: bool) -> ContextValue {
    BOOLEAN_CONTEXTS[usize::from(value)].clone()
}

fn is_semicolon_predecessor(term: u16) -> bool {
    matches!(
        term,
        terms::IncDecOp
            | terms::identifier
            | terms::Rune
            | terms::String
            | terms::Number
            | terms::predeclaredBool
            | terms::predeclaredNil
            | terms::_break
            | terms::_continue
            | terms::_return
            | terms::fallthrough
            | terms::closeParen
            | terms::closeBracket
            | terms::closeBrace
    )
}

fn hash_context(context: &ContextValue) -> u64 {
    u64::from(context.downcast_ref::<bool>().copied().unwrap_or(false))
}
