use std::sync::LazyLock;

use rezel_common::{CodePoint, ParseError};
use rezel_lr::{
    ContextTracker, ContextValue, ExternalTokenizer, InputStream, Stack, TokenizerFlags,
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

static BOOLEAN_CONTEXTS: LazyLock<[ContextValue; 2]> =
    LazyLock::new(|| [ContextValue::new(false), ContextValue::new(true)]);

pub(crate) static SEMICOLON: ExternalTokenizer = ExternalTokenizer::new(
    scan_semicolon,
    TokenizerFlags {
        contextual: true,
        fallback: false,
        extend: false,
    },
);

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
