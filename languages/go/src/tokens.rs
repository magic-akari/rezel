use std::sync::LazyLock;

use rezel_common::ParseError;
use rezel_lr::{
    ContextTracker, ContextValue, ExternalTokenizer, InputStream, Stack, TokenizerFlags,
};

use crate::terms;

const NEWLINE: u16 = 10;
const CARRIAGE_RETURN: u16 = 13;
const SPACE: u16 = 32;
const TAB: u16 = 9;
const SLASH: u16 = 47;
const ASTERISK: u16 = 42;
const CLOSE_PAREN: u16 = 41;
const CLOSE_BRACE: u16 = 125;

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
    ContextTracker::new(start_context, Some(shift_context), None, hash_context);

fn scan_semicolon(input: &mut InputStream, stack: &Stack) -> Result<(), ParseError> {
    let context = stack.context::<bool>().copied().unwrap_or(false);
    let mut scan = 0_isize;
    loop {
        let next = input.peek(scan);
        if matches!(next, Some(SPACE | TAB)) {
            scan += 1;
            continue;
        }
        let line_end = matches!(next, None | Some(NEWLINE | CARRIAGE_RETURN));
        let line_comment = next == Some(SLASH) && input.peek(scan + 1) == Some(SLASH);
        if context && (line_end || line_comment) {
            input.accept_token(terms::insertedSemi, 0)?;
            return Ok(());
        }
        let block_comment = next == Some(SLASH) && input.peek(scan + 1) == Some(ASTERISK);
        if block_comment {
            let (after, contains_line_end) = scan_block_comment(input, scan);
            if contains_line_end {
                if context {
                    input.accept_token(terms::insertedSemi, 0)?;
                }
                return Ok(());
            }
            scan = after;
            continue;
        }
        if matches!(next, Some(CLOSE_PAREN | CLOSE_BRACE)) {
            input.accept_token(terms::insertedSemi, 0)?;
        }
        return Ok(());
    }
}

fn scan_block_comment(input: &mut InputStream, start: isize) -> (isize, bool) {
    let mut scan = start + 2;
    loop {
        match input.peek(scan) {
            None | Some(NEWLINE | CARRIAGE_RETURN) => return (scan, true),
            Some(ASTERISK) if input.peek(scan + 1) == Some(SLASH) => {
                return (scan + 2, false);
            }
            Some(_) => scan += 1,
        }
    }
}

fn start_context() -> ContextValue {
    boolean_context(false)
}

// The shared context-tracker ABI is fallible even though this transition is
// total. Keeping the exact callback shape avoids a language-specific adapter.
#[allow(clippy::unnecessary_wraps)]
fn shift_context(
    context: &ContextValue,
    term: u16,
    _stack: &Stack,
    _input: &mut InputStream,
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
