use rezel_common::{CodePoint, ParseError};
use rezel_lr::{ExternalTokenizer, ExternalTokenizerStart, InputStream, Stack, TokenizerFlags};

use crate::terms;

const B: u32 = b'b' as u32;
const C: u32 = b'c' as u32;
const E: u32 = b'e' as u32;
const F: u32 = b'f' as u32;
const R: u32 = b'r' as u32;
const LOWER_A: u32 = b'a' as u32;
const LOWER_Z: u32 = b'z' as u32;
const UPPER_A: u32 = b'A' as u32;
const UPPER_E: u32 = b'E' as u32;
const UPPER_Z: u32 = b'Z' as u32;
const ZERO: u32 = b'0' as u32;
const UNDERSCORE: u32 = b'_' as u32;
const DOT: u32 = b'.' as u32;
const PLUS: u32 = b'+' as u32;
const MINUS: u32 = b'-' as u32;
const HASH: u32 = b'#' as u32;
const QUOTE: u32 = b'"' as u32;
const PIPE: u32 = b'|' as u32;
const LESS_THAN: u32 = b'<' as u32;
const GREATER_THAN: u32 = b'>' as u32;
const EQUAL: u32 = b'=' as u32;

const FLAGS: TokenizerFlags = TokenizerFlags {
    contextual: false,
    fallback: false,
    extend: false,
};

const LITERAL_START: ExternalTokenizerStart = ExternalTokenizerStart::NONE
    .with_ascii_range(b'0'..=b'9')
    .with_ascii(b'b')
    .with_ascii(b'c')
    .with_ascii(b'r');
const CLOSURE_PARAM_START: ExternalTokenizerStart = ExternalTokenizerStart::NONE.with_ascii(b'|');
const TYPE_PARAMETER_DELIMITER_START: ExternalTokenizerStart = ExternalTokenizerStart::NONE
    .with_ascii(b'<')
    .with_ascii(b'>');

pub(crate) static LITERALS: ExternalTokenizer =
    ExternalTokenizer::new(scan_literals, FLAGS).with_start(LITERAL_START);

pub(crate) static CLOSURE_PARAM: ExternalTokenizer =
    ExternalTokenizer::new(scan_closure_param, FLAGS).with_start(CLOSURE_PARAM_START);

pub(crate) static TYPE_PARAMETER_DELIMITERS: ExternalTokenizer =
    ExternalTokenizer::new(scan_type_parameter_delimiters, FLAGS)
        .with_start(TYPE_PARAMETER_DELIMITER_START);

fn scan_literals(input: &mut InputStream, _stack: &Stack) -> Result<(), ParseError> {
    match current(input) {
        Some(value) if is_number(value) => scan_number(input),
        Some(B | C | R) => scan_raw_string(input),
        _ => Ok(()),
    }
}

fn scan_number(input: &mut InputStream) -> Result<(), ParseError> {
    let mut is_float = false;
    advance_while(input, is_number_or_underscore);

    if current(input) == Some(DOT) {
        is_float = true;
        input.advance(1);
        if current(input).is_some_and(is_number) {
            advance_while(input, is_number_or_underscore);
        } else if current(input).is_some_and(|value| value == DOT || value > 0x7f || is_word(value))
        {
            return Ok(());
        }
    }

    if matches!(current(input), Some(E | UPPER_E)) {
        is_float = true;
        input.advance(1);
        if matches!(current(input), Some(PLUS | MINUS)) {
            input.advance(1);
        }
        if !current(input).is_some_and(is_number_or_underscore) {
            return Ok(());
        }
        advance_while(input, is_number_or_underscore);
    }

    if current(input) == Some(F) {
        let after = peek(input, 1);
        let after_after = peek(input, 2);
        let after_after_after = peek(input, 3);
        let suffix_width = if matches!(
            (after, after_after),
            (Some(value), Some(next))
                if (value == ZERO + 1 && next == ZERO + 6)
                    || (value == ZERO + 3 && next == ZERO + 2)
                    || (value == ZERO + 6 && next == ZERO + 4)
        ) {
            3
        } else if (after, after_after, after_after_after)
            == (Some(ZERO + 1), Some(ZERO + 2), Some(ZERO + 8))
        {
            4
        } else {
            return Ok(());
        };
        input.advance(suffix_width);
        is_float = true;
    }

    if is_float {
        input.accept_token(terms::Float)?;
    }
    Ok(())
}

fn scan_raw_string(input: &mut InputStream) -> Result<(), ParseError> {
    if matches!(current(input), Some(B | C)) {
        input.advance(1);
    }
    if current(input) != Some(R) {
        return Ok(());
    }
    input.advance(1);

    let mut hashes = 0;
    while current(input) == Some(HASH) {
        hashes += 1;
        input.advance(1);
    }
    if current(input) != Some(QUOTE) {
        return Ok(());
    }
    input.advance(1);

    'content: loop {
        if current(input).is_none() {
            return Ok(());
        }
        let is_quote = current(input) == Some(QUOTE);
        input.advance(1);
        if !is_quote {
            continue;
        }
        for _ in 0..hashes {
            if current(input) != Some(HASH) {
                continue 'content;
            }
            input.advance(1);
        }
        input.accept_token(terms::RawString)?;
        return Ok(());
    }
}

fn scan_closure_param(input: &mut InputStream, _stack: &Stack) -> Result<(), ParseError> {
    if current(input) == Some(PIPE) {
        input.advance(1);
        input.accept_token(terms::closureParamDelim)?;
    }
    Ok(())
}

fn scan_type_parameter_delimiters(
    input: &mut InputStream,
    _stack: &Stack,
) -> Result<(), ParseError> {
    match current(input) {
        Some(LESS_THAN) if peek(input, 1) != Some(EQUAL) => {
            input.advance(1);
            input.accept_token(terms::tpOpen)?;
        }
        Some(GREATER_THAN) => {
            input.advance(1);
            input.accept_token(terms::tpClose)?;
        }
        _ => {}
    }
    Ok(())
}

fn current(input: &InputStream) -> Option<u32> {
    input.next().map(CodePoint::as_u32)
}

fn peek(input: &InputStream, offset: isize) -> Option<u32> {
    input.peek(offset).map(CodePoint::as_u32)
}

fn advance_while(input: &mut InputStream, predicate: impl Fn(u32) -> bool) {
    while current(input).is_some_and(&predicate) {
        input.advance(1);
    }
}

fn is_number(value: u32) -> bool {
    (ZERO..=ZERO + 9).contains(&value)
}

fn is_number_or_underscore(value: u32) -> bool {
    is_number(value) || value == UNDERSCORE
}

fn is_word(value: u32) -> bool {
    (LOWER_A..=LOWER_Z).contains(&value)
        || (UPPER_A..=UPPER_Z).contains(&value)
        || is_number(value)
        || value == UNDERSCORE
}
