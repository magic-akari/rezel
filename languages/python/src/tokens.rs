use rezel_common::ParseError;
use rezel_lr::{
    ContextTracker, ContextValue, ExternalTokenizer, InputStream, Stack, TokenizerFlags,
};

use crate::{indentation::IndentColumns, terms};

const LF: u16 = 10;
const CR: u16 = 13;
const SPACE: u16 = 32;
const TAB: u16 = 9;
const HASH: u16 = 35;
const OPEN_BRACE: u16 = 123;
const CLOSE_BRACE: u16 = 125;
const SINGLE_QUOTE: u16 = 39;
const DOUBLE_QUOTE: u16 = 34;
const BACKSLASH: u16 = 92;

const BRACKETED: u8 = 1;
const STRING: u8 = 2;
const DOUBLE: u8 = 4;
const LONG: u8 = 8;
const RAW: u8 = 16;
const FORMAT: u8 = 32;

#[derive(Clone)]
struct PythonContext {
    parent: Option<ContextValue>,
    indent: usize,
    flags: u8,
    hash: u64,
}

pub(crate) static NEWLINES: ExternalTokenizer = ExternalTokenizer::new(
    scan_newlines,
    TokenizerFlags {
        contextual: true,
        fallback: false,
        extend: false,
    },
);

pub(crate) static INDENTATION: ExternalTokenizer = ExternalTokenizer::new(
    scan_indentation,
    TokenizerFlags {
        contextual: true,
        fallback: false,
        extend: false,
    },
);

pub(crate) static STRINGS: ExternalTokenizer = ExternalTokenizer::new(
    scan_strings,
    TokenizerFlags {
        contextual: true,
        fallback: false,
        extend: false,
    },
);

pub(crate) static TRACK_INDENT: ContextTracker = ContextTracker::new(
    start_context,
    Some(shift_context),
    Some(reduce_context),
    hash_context,
);

fn scan_newlines(input: &mut InputStream, stack: &Stack) -> Result<(), ParseError> {
    let context = context(stack);
    if input.next().is_none() {
        if stack.can_shift(terms::eof) {
            input.accept_token(terms::eof, 0)?;
        }
        return Ok(());
    }
    if context.flags & BRACKETED != 0 {
        if input.next().is_some_and(is_line_break) && stack.can_shift(terms::newlineBracketed) {
            input.accept_token(terms::newlineBracketed, 1)?;
        }
        return Ok(());
    }
    let previous = input.peek(-1);
    if previous.is_none_or(is_line_break) && stack.can_shift(terms::blankLineStart) {
        let mut spaces = 0_isize;
        while matches!(input.next(), Some(SPACE | TAB)) {
            input.advance(1);
            spaces += 1;
        }
        if input
            .next()
            .is_none_or(|next| is_line_break(next) || next == HASH)
        {
            input.accept_token(terms::blankLineStart, -spaces)?;
        }
    } else if input.next().is_some_and(is_line_break) && stack.can_shift(terms::newline) {
        input.accept_token(terms::newline, 1)?;
    }
    Ok(())
}

fn scan_indentation(input: &mut InputStream, stack: &Stack) -> Result<(), ParseError> {
    let context = context(stack);
    if context.flags != 0 {
        return Ok(());
    }
    let previous = input.peek(-1);
    if previous.is_some_and(|character| !is_line_break(character)) {
        return Ok(());
    }
    let mut columns = IndentColumns::default();
    let mut characters = 0_isize;
    while let Some(character) = input.next() {
        if !columns.advance(u32::from(character)) {
            break;
        }
        input.advance(1);
        characters += 1;
    }
    let next_is_blank = input
        .next()
        .is_none_or(|next| is_line_break(next) || next == HASH);
    if next_is_blank {
        return Ok(());
    }
    match columns.visual().cmp(&context.indent) {
        std::cmp::Ordering::Equal => {}
        std::cmp::Ordering::Greater => {
            input.accept_token(terms::indent, 0)?;
        }
        std::cmp::Ordering::Less => {
            input.accept_token(terms::dedent, -characters)?;
        }
    }
    Ok(())
}

fn scan_strings(input: &mut InputStream, stack: &Stack) -> Result<(), ParseError> {
    let flags = context(stack).flags;
    let quote = if flags & DOUBLE != 0 {
        DOUBLE_QUOTE
    } else {
        SINGLE_QUOTE
    };
    let long = flags & LONG != 0;
    let escapes = flags & RAW == 0;
    let format = flags & FORMAT != 0;
    let start = input.position();
    loop {
        match input.next() {
            None => break,
            Some(OPEN_BRACE) if format => {
                if input.peek(1) == Some(OPEN_BRACE) {
                    input.advance(2);
                } else if input.position() == start {
                    input.accept_token(terms::replacementStart, 1)?;
                    return Ok(());
                } else {
                    break;
                }
            }
            Some(BACKSLASH) if escapes => {
                if input.position() != start {
                    break;
                }
                input.advance(1);
                if let Some(escaped) = input.next() {
                    input.advance(1);
                    skip_escape(input, escaped);
                }
                input.accept_token(terms::Escape, 0)?;
                return Ok(());
            }
            Some(BACKSLASH)
                if format && matches!(input.peek(1), Some(OPEN_BRACE | CLOSE_BRACE)) =>
            {
                input.advance(1);
            }
            Some(BACKSLASH) if input.peek(1).is_some() => {
                input.advance(2);
            }
            Some(next)
                if next == quote
                    && (!long || input.peek(1) == Some(quote) && input.peek(2) == Some(quote)) =>
            {
                if input.position() == start {
                    input.accept_token(terms::stringEnd, if long { 3 } else { 1 })?;
                    return Ok(());
                }
                break;
            }
            Some(LF) => {
                if long {
                    input.advance(1);
                } else if input.position() == start {
                    input.accept_token(terms::stringEnd, 0)?;
                    return Ok(());
                } else {
                    break;
                }
            }
            Some(_) => {
                input.advance(1);
            }
        }
    }
    if input.position() > start {
        input.accept_token(terms::stringContent, 0)?;
    }
    Ok(())
}

fn skip_escape(input: &mut InputStream, escaped: u16) {
    match escaped {
        111 => {
            for _ in 0..2 {
                if input.next().is_some_and(|next| (48..=55).contains(&next)) {
                    input.advance(1);
                }
            }
        }
        120 => skip_hex(input, 2),
        117 => skip_hex(input, 4),
        85 => skip_hex(input, 8),
        78 if input.next() == Some(OPEN_BRACE) => {
            input.advance(1);
            while input.next().is_some_and(|next| {
                next != CLOSE_BRACE && next != SINGLE_QUOTE && next != DOUBLE_QUOTE && next != LF
            }) {
                input.advance(1);
            }
            if input.next() == Some(CLOSE_BRACE) {
                input.advance(1);
            }
        }
        _ => {}
    }
}

fn skip_hex(input: &mut InputStream, count: usize) {
    for _ in 0..count {
        if input.next().is_some_and(is_hex) {
            input.advance(1);
        }
    }
}

fn is_hex(value: u16) -> bool {
    (48..=57).contains(&value) || (65..=70).contains(&value) || (97..=102).contains(&value)
}

fn is_line_break(value: u16) -> bool {
    matches!(value, LF | CR)
}

fn start_context() -> ContextValue {
    ContextValue::new(PythonContext {
        parent: None,
        indent: 0,
        flags: 0,
        hash: 0,
    })
}

#[allow(clippy::unnecessary_wraps)] // ContextTracker callbacks share a fallible signature.
fn shift_context(
    value: &ContextValue,
    term: u16,
    stack: &Stack,
    input: &mut InputStream,
) -> Result<ContextValue, ParseError> {
    let current = value
        .downcast_ref::<PythonContext>()
        .expect("Python parser context");
    if term == terms::indent {
        let whitespace = input.read(input.position(), stack.position());
        let indentation = count_indent(&whitespace);
        return Ok(child_context(value, indentation.visual(), 0));
    }
    if term == terms::dedent {
        return Ok(current.parent.clone().unwrap_or_else(start_context));
    }
    if matches!(
        term,
        terms::ParenL | terms::BracketL | terms::BraceL | terms::replacementStart
    ) {
        return Ok(child_context(value, 0, BRACKETED));
    }
    if let Some(flags) = string_flags(term) {
        return Ok(child_context(value, 0, flags | (current.flags & BRACKETED)));
    }
    Ok(value.clone())
}

#[allow(clippy::unnecessary_wraps)] // ContextTracker callbacks share a fallible signature.
fn reduce_context(
    value: &ContextValue,
    term: u16,
    _stack: &Stack,
    _input: &mut InputStream,
) -> Result<ContextValue, ParseError> {
    let current = value
        .downcast_ref::<PythonContext>()
        .expect("Python parser context");
    let closes_bracket = current.flags & BRACKETED != 0 && is_bracketed_node(term);
    let closes_string = current.flags & STRING != 0
        && matches!(
            term,
            terms::String | terms::FormatString | terms::TemplateString
        );
    if closes_bracket || closes_string {
        return Ok(current.parent.clone().unwrap_or_else(start_context));
    }
    Ok(value.clone())
}

fn child_context(parent: &ContextValue, indent: usize, flags: u8) -> ContextValue {
    let parent_hash = parent
        .downcast_ref::<PythonContext>()
        .map_or(0, |context| context.hash);
    let hash = parent_hash
        .wrapping_mul(257)
        .wrapping_add(indent as u64)
        .wrapping_mul(67)
        .wrapping_add(u64::from(flags));
    ContextValue::new(PythonContext {
        parent: Some(parent.clone()),
        indent,
        flags,
        hash,
    })
}

fn context(stack: &Stack) -> &PythonContext {
    stack
        .context::<PythonContext>()
        .expect("Python parser context")
}

fn count_indent(value: &str) -> IndentColumns {
    IndentColumns::from_whitespace(value)
        .expect("shifted Python indentation contains only space, tab, and form feed")
}

fn hash_context(value: &ContextValue) -> u64 {
    value
        .downcast_ref::<PythonContext>()
        .map_or(0, |context| context.hash)
}

fn string_flags(term: u16) -> Option<u8> {
    Some(match term {
        terms::stringStart => STRING,
        terms::stringStartD => STRING | DOUBLE,
        terms::stringStartL => STRING | LONG,
        terms::stringStartLD => STRING | LONG | DOUBLE,
        terms::stringStartR => STRING | RAW,
        terms::stringStartRD => STRING | RAW | DOUBLE,
        terms::stringStartRL => STRING | RAW | LONG,
        terms::stringStartRLD => STRING | RAW | LONG | DOUBLE,
        terms::stringStartF | terms::stringStartT => STRING | FORMAT,
        terms::stringStartFD | terms::stringStartTD => STRING | FORMAT | DOUBLE,
        terms::stringStartFL | terms::stringStartTL => STRING | FORMAT | LONG,
        terms::stringStartFLD | terms::stringStartTLD => STRING | FORMAT | LONG | DOUBLE,
        terms::stringStartFR | terms::stringStartTR => STRING | FORMAT | RAW,
        terms::stringStartFRD | terms::stringStartTRD => STRING | FORMAT | RAW | DOUBLE,
        terms::stringStartFRL | terms::stringStartTRL => STRING | FORMAT | RAW | LONG,
        terms::stringStartFRLD | terms::stringStartTRLD => STRING | FORMAT | RAW | LONG | DOUBLE,
        _ => return None,
    })
}

fn is_bracketed_node(term: u16) -> bool {
    matches!(
        term,
        terms::ParenthesizedExpression
            | terms::parenthesizedWithItems
            | terms::TupleExpression
            | terms::ComprehensionExpression
            | terms::importList
            | terms::ArgList
            | terms::ParamList
            | terms::ArrayExpression
            | terms::ArrayComprehensionExpression
            | terms::subscript
            | terms::SetExpression
            | terms::SetComprehensionExpression
            | terms::FormatString
            | terms::TemplateString
            | terms::FormatReplacement
            | terms::TemplateInterpolation
            | terms::nestedFormatReplacement
            | terms::DictionaryExpression
            | terms::DictionaryComprehensionExpression
            | terms::SequencePattern
            | terms::MappingPattern
            | terms::PatternArgList
            | terms::TypeParamList
    )
}
