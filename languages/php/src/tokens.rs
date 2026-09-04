use rezel_common::{CodePoint, ParseError};
use rezel_lr::{ExternalTokenizer, ExternalTokenizerStart, InputStream, Stack, TokenizerFlags};

use crate::terms;

const OPEN_PAREN: u32 = b'(' as u32;
const CLOSE_PAREN: u32 = b')' as u32;
const COLON: u32 = b':' as u32;
const SEMICOLON_CHAR: u32 = b';' as u32;
const EQUALS: u32 = b'=' as u32;
const AMPERSAND: u32 = b'&' as u32;
const LESS_THAN: u32 = b'<' as u32;
const QUESTION: u32 = b'?' as u32;
const GREATER_THAN: u32 = b'>' as u32;
const SPACE: u32 = b' ' as u32;
const TAB: u32 = b'\t' as u32;
const NEWLINE: u32 = b'\n' as u32;
const CARRIAGE_RETURN: u32 = b'\r' as u32;
const APOSTROPHE: u32 = b'\'' as u32;
const QUOTE: u32 = b'"' as u32;
const BACKSLASH: u32 = b'\\' as u32;
const DOLLAR: u32 = b'$' as u32;
const OPEN_BRACE: u32 = b'{' as u32;
const CLOSE_BRACE: u32 = b'}' as u32;
const OPEN_BRACKET: u32 = b'[' as u32;
const MINUS: u32 = b'-' as u32;
const PERIOD: u32 = b'.' as u32;
const SLASH: u32 = b'/' as u32;
const STAR: u32 = b'*' as u32;
const HASH: u32 = b'#' as u32;

struct HeredocFrame {
    tag: Vec<u32>,
    interpolation_depth: usize,
    interpolated: bool,
}

enum FastQualifiedNameWidth {
    Found(usize),
    Rejected,
    NeedsLookahead,
}

pub(crate) static OPEN_TAG_BOUNDARY: ExternalTokenizer = ExternalTokenizer::new(
    scan_open_tag_boundary,
    TokenizerFlags {
        contextual: false,
        fallback: false,
        extend: false,
    },
)
.with_start(
    ExternalTokenizerStart::NONE
        .with_ascii(b' ')
        .with_ascii(b'\t')
        .with_ascii(b'\n')
        .with_ascii(b'\r')
        .with_end(),
);

pub(crate) static EXPRESSION: ExternalTokenizer = ExternalTokenizer::new(
    scan_expression,
    TokenizerFlags {
        contextual: false,
        fallback: false,
        extend: false,
    },
)
.with_start(
    ExternalTokenizerStart::NONE
        .with_ascii(b'(')
        .with_ascii(b'&')
        .with_ascii(b'<')
        .with_ascii(b'|')
        .with_ascii(b'_')
        .with_ascii_range(b'A'..=b'Z')
        .with_ascii_range(b'a'..=b'z')
        .with_non_ascii(),
);

pub(crate) static INTERPOLATED: ExternalTokenizer = ExternalTokenizer::new(
    scan_interpolated,
    TokenizerFlags {
        contextual: true,
        fallback: false,
        extend: false,
    },
);

pub(crate) static SET_VISIBILITY: ExternalTokenizer = ExternalTokenizer::new(
    scan_set_visibility,
    TokenizerFlags {
        contextual: false,
        fallback: false,
        extend: false,
    },
)
.with_start(
    ExternalTokenizerStart::NONE
        .with_ascii(b'p')
        .with_ascii(b'P'),
);

pub(crate) static SEMICOLON: ExternalTokenizer = ExternalTokenizer::new(
    scan_semicolon,
    TokenizerFlags {
        contextual: true,
        fallback: false,
        extend: false,
    },
)
.with_start(ExternalTokenizerStart::NONE.with_ascii(b'?'));

pub(crate) static EOF_TOKEN: ExternalTokenizer = ExternalTokenizer::new(
    scan_eof,
    TokenizerFlags {
        contextual: false,
        fallback: false,
        extend: false,
    },
)
.with_start(ExternalTokenizerStart::NONE.with_end());

pub(crate) static HALT_COMPILER: ExternalTokenizer = ExternalTokenizer::new(
    scan_halt_compiler_tail,
    TokenizerFlags {
        contextual: true,
        fallback: false,
        extend: false,
    },
);

macro_rules! keyword_match {
    ($stack:expr, $value:expr, $($spelling:literal => $term:expr),+ $(,)?) => {{
        $(
            if $value.eq_ignore_ascii_case($spelling) {
                return $stack.can_shift($term).then_some($term);
            }
        )+
        None
    }};
}

pub(crate) fn keywords(value: &str, stack: &Stack) -> Option<u16> {
    let first = value.as_bytes().first().map(u8::to_ascii_lowercase)?;
    match first {
        b'_' => keyword_match!(
            stack,
            value,
            "__halt_compiler" => terms::haltCompiler,
        ),
        b'a'..=b'g' => keywords_a_to_g(first, value, stack),
        b'i'..=b'p' => keywords_i_to_p(first, value, stack),
        b'r'..=b'y' => keywords_r_to_y(first, value, stack),
        _ => None,
    }
}

fn keywords_a_to_g(first: u8, value: &str, stack: &Stack) -> Option<u16> {
    match first {
        b'a' => keyword_match!(
            stack,
            value,
            "abstract" => terms::_abstract,
            "and" => terms::and,
            "array" => terms::array,
            "as" => terms::_as,
        ),
        b'b' => keyword_match!(
            stack,
            value,
            "break" => terms::_break,
        ),
        b'c' => keyword_match!(
            stack,
            value,
            "case" => terms::case,
            "catch" => terms::catch,
            "class" => terms::class,
            "clone" => terms::clone,
            "const" => terms::_const,
            "continue" => terms::_continue,
        ),
        b'd' => keyword_match!(
            stack,
            value,
            "declare" => terms::declare,
            "default" => terms::default,
            "do" => terms::_do,
        ),
        b'e' => keyword_match!(
            stack,
            value,
            "echo" => terms::echo,
            "else" => terms::_else,
            "elseif" => terms::elseif,
            "enddeclare" => terms::enddeclare,
            "endfor" => terms::endfor,
            "endforeach" => terms::endforeach,
            "endif" => terms::endif,
            "endswitch" => terms::endswitch,
            "endwhile" => terms::endwhile,
            "enum" => terms::_enum,
            "extends" => terms::extends,
        ),
        b'f' => keyword_match!(
            stack,
            value,
            "false" => terms::Boolean,
            "final" => terms::_final,
            "finally" => terms::finally,
            "fn" => terms::_fn,
            "for" => terms::_for,
            "foreach" => terms::foreach,
            "from" => terms::from,
            "function" => terms::function,
        ),
        b'g' => keyword_match!(
            stack,
            value,
            "global" => terms::global,
            "goto" => terms::goto,
        ),
        _ => None,
    }
}

fn keywords_i_to_p(first: u8, value: &str, stack: &Stack) -> Option<u16> {
    match first {
        b'i' => keyword_match!(
            stack,
            value,
            "if" => terms::_if,
            "implements" => terms::implements,
            "include" => terms::include,
            "include_once" => terms::include_once,
            "instanceof" => terms::instanceof,
            "insteadof" => terms::insteadof,
            "interface" => terms::interface,
        ),
        b'l' => keyword_match!(stack, value, "list" => terms::list),
        b'm' => keyword_match!(stack, value, "match" => terms::_match),
        b'n' => keyword_match!(
            stack,
            value,
            "namespace" => terms::namespace,
            "new" => terms::new,
            "null" => terms::null,
        ),
        b'o' => keyword_match!(stack, value, "or" => terms::or),
        b'p' => keyword_match!(
            stack,
            value,
            "print" => terms::print,
            "private" => terms::Visibility,
            "protected" => terms::Visibility,
            "public" => terms::Visibility,
        ),
        _ => None,
    }
}

fn keywords_r_to_y(first: u8, value: &str, stack: &Stack) -> Option<u16> {
    match first {
        b'r' => keyword_match!(
            stack,
            value,
            "readonly" => terms::readonly,
            "require" => terms::require,
            "require_once" => terms::require_once,
            "return" => terms::_return,
        ),
        b's' => keyword_match!(stack, value, "switch" => terms::switch),
        b't' => keyword_match!(
            stack,
            value,
            "throw" => terms::throw,
            "trait" => terms::_trait,
            "true" => terms::Boolean,
            "try" => terms::_try,
        ),
        b'u' => keyword_match!(
            stack,
            value,
            "unset" => terms::unset,
            "use" => terms::_use,
        ),
        b'v' => keyword_match!(stack, value, "var" => terms::var),
        b'w' => keyword_match!(stack, value, "while" => terms::_while),
        b'x' => keyword_match!(stack, value, "xor" => terms::xor),
        b'y' => keyword_match!(stack, value, "yield" => terms::_yield),
        _ => None,
    }
}

fn scan_expression(input: &mut InputStream, stack: &Stack) -> Result<(), ParseError> {
    match input.next().map(CodePoint::as_u32) {
        Some(OPEN_PAREN) => scan_cast(input),
        Some(AMPERSAND) => scan_reference_ampersand(input),
        Some(next)
            if (next == u32::from(b'b') || next == u32::from(b'B'))
                && input.peek(1).map(CodePoint::as_u32) == Some(LESS_THAN)
                && input.peek(2).map(CodePoint::as_u32) == Some(LESS_THAN)
                && input.peek(3).map(CodePoint::as_u32) == Some(LESS_THAN) =>
        {
            input.advance(1);
            scan_heredoc(input)
        }
        Some(next)
            if (next == u32::from(b'r') || next == u32::from(b'R'))
                && stack.can_shift(terms::readonlyCallName)
                && starts_readonly_call(input) =>
        {
            scan_readonly_call_name(input)
        }
        Some(next)
            if is_identifier_start(next)
                && keyword_qualified_name_width(input, stack).is_some()
                && stack.can_shift(terms::keywordQualifiedName) =>
        {
            scan_keyword_qualified_name(input, stack)
        }
        Some(next)
            if (next == u32::from(b'u') || next == u32::from(b'U'))
                && stack.can_shift(terms::groupUseKeyword)
                && starts_absolute_group_use(input) =>
        {
            input.advance(3);
            input.accept_token(terms::groupUseKeyword)?;
            Ok(())
        }
        Some(next)
            if (next == u32::from(b'f') || next == u32::from(b'F'))
                && stack.can_shift(terms::groupUseFunction)
                && starts_absolute_group_keyword(input, b"function") =>
        {
            input.advance(b"function".len());
            input.accept_token(terms::groupUseFunction)?;
            Ok(())
        }
        Some(next)
            if (next == u32::from(b'c') || next == u32::from(b'C'))
                && stack.can_shift(terms::groupUseConst)
                && starts_absolute_group_keyword(input, b"const") =>
        {
            input.advance(b"const".len());
            input.accept_token(terms::groupUseConst)?;
            Ok(())
        }
        Some(next)
            if is_identifier_start(next)
                && (stack.can_shift(terms::argumentName)
                    || stack.can_shift(terms::declarationName)) =>
        {
            scan_contextual_name(input, stack)
        }
        Some(LESS_THAN)
            if input.peek(1).map(CodePoint::as_u32) == Some(LESS_THAN)
                && input.peek(2).map(CodePoint::as_u32) == Some(LESS_THAN) =>
        {
            scan_heredoc(input)
        }
        Some(next)
            if next == u32::from(b'|')
                && input.peek(1).map(CodePoint::as_u32) == Some(GREATER_THAN) =>
        {
            input.advance(2);
            input.accept_token(terms::pipeOp)?;
            Ok(())
        }
        _ => Ok(()),
    }
}

fn scan_readonly_call_name(input: &mut InputStream) -> Result<(), ParseError> {
    const READONLY: &[u8] = b"readonly";
    input.advance(READONLY.len());
    input.accept_token(terms::readonlyCallName)?;
    Ok(())
}

fn starts_readonly_call(input: &InputStream) -> bool {
    const READONLY: &[u8] = b"readonly";
    input_matches_ascii(input, READONLY)
        && !input
            .peek(READONLY.len().cast_signed())
            .map(CodePoint::as_u32)
            .is_some_and(|next| is_identifier_start(next) || is_ascii_digit(next))
        && {
            let mut lookahead = input
                .lookahead()
                .skip(READONLY.len())
                .map(CodePoint::as_u32)
                .peekable();
            skip_php_trivia(&mut lookahead);
            lookahead.next() == Some(OPEN_PAREN)
        }
}

fn scan_keyword_qualified_name(input: &mut InputStream, stack: &Stack) -> Result<(), ParseError> {
    let width = keyword_qualified_name_width(input, stack)
        .expect("keyword-qualified-name guard established a width");
    input.advance(width);
    input.accept_token(terms::keywordQualifiedName)?;
    Ok(())
}

fn keyword_qualified_name_width(input: &InputStream, stack: &Stack) -> Option<usize> {
    // A truncated identity chunk or non-ASCII byte must fall back so selected
    // ranges and translated inputs keep the code-point iterator authoritative.
    match ascii_keyword_qualified_name_width(input.identity_lookahead_chunk(), stack) {
        FastQualifiedNameWidth::Found(width) => return Some(width),
        FastQualifiedNameWidth::Rejected => return None,
        FastQualifiedNameWidth::NeedsLookahead => {}
    }
    keyword_qualified_name_width_lookahead(input, stack)
}

fn ascii_keyword_qualified_name_width(input: &[u8], stack: &Stack) -> FastQualifiedNameWidth {
    const MAX_KEYWORD_WIDTH: usize = 16;

    let mut width = 0_usize;
    while let Some(&next) = input.get(width) {
        if !next.is_ascii() {
            return FastQualifiedNameWidth::NeedsLookahead;
        }
        if !is_identifier_start(u32::from(next)) && !next.is_ascii_digit() {
            break;
        }
        width += 1;
        if width > MAX_KEYWORD_WIDTH {
            return FastQualifiedNameWidth::Rejected;
        }
    }
    let Some(&next) = input.get(width) else {
        return FastQualifiedNameWidth::NeedsLookahead;
    };
    if next != b'\\' {
        return FastQualifiedNameWidth::Rejected;
    }

    let first = std::str::from_utf8(&input[..width]).expect("ASCII keyword is valid UTF-8");
    let specialized = keywords(first, stack).is_some()
        || first.eq_ignore_ascii_case("static") && stack.can_shift(terms::_static);
    if !specialized {
        return FastQualifiedNameWidth::Rejected;
    }
    width += 1;

    loop {
        let Some(&first) = input.get(width) else {
            return FastQualifiedNameWidth::NeedsLookahead;
        };
        if !first.is_ascii() {
            return FastQualifiedNameWidth::NeedsLookahead;
        }
        if !is_identifier_start(u32::from(first)) {
            return FastQualifiedNameWidth::Rejected;
        }
        width += 1;

        while let Some(&next) = input.get(width) {
            if !next.is_ascii() {
                return FastQualifiedNameWidth::NeedsLookahead;
            }
            if !is_identifier_start(u32::from(next)) && !next.is_ascii_digit() {
                break;
            }
            width += 1;
        }
        let Some(&next) = input.get(width) else {
            return FastQualifiedNameWidth::NeedsLookahead;
        };
        if next != b'\\' {
            return FastQualifiedNameWidth::Found(width);
        }
        width += 1;
    }
}

fn keyword_qualified_name_width_lookahead(input: &InputStream, stack: &Stack) -> Option<usize> {
    let mut lookahead = input.lookahead().map(CodePoint::as_u32).peekable();
    let mut first = [0_u8; 16];
    let mut first_len = 0_usize;
    let mut width = 0_usize;
    while let Some(next) = lookahead
        .peek()
        .copied()
        .filter(|next| is_identifier_start(*next) || is_ascii_digit(*next))
    {
        let next = u8::try_from(next).ok()?;
        let slot = first.get_mut(first_len)?;
        *slot = next;
        first_len += 1;
        width += 1;
        lookahead.next();
    }
    if lookahead.next() != Some(BACKSLASH) {
        return None;
    }
    let first = std::str::from_utf8(&first[..first_len]).ok()?;
    let specialized = keywords(first, stack).is_some()
        || first.eq_ignore_ascii_case("static") && stack.can_shift(terms::_static);
    if !specialized {
        return None;
    }
    width += 1;

    loop {
        if !lookahead.peek().copied().is_some_and(is_identifier_start) {
            return None;
        }
        while lookahead
            .peek()
            .copied()
            .is_some_and(|next| is_identifier_start(next) || is_ascii_digit(next))
        {
            width += 1;
            lookahead.next();
        }
        if lookahead.peek().copied() != Some(BACKSLASH) {
            return Some(width);
        }
        width += 1;
        lookahead.next();
    }
}

fn starts_absolute_group_use(input: &InputStream) -> bool {
    starts_absolute_group_keyword(input, b"use")
}

fn starts_absolute_group_keyword(input: &InputStream, keyword: &[u8]) -> bool {
    if !input_matches_ascii(input, keyword)
        || input
            .peek(keyword.len().cast_signed())
            .map(CodePoint::as_u32)
            .is_some_and(|next| is_identifier_start(next) || is_ascii_digit(next))
    {
        return false;
    }

    let mut lookahead = input
        .lookahead()
        .skip(keyword.len())
        .map(CodePoint::as_u32)
        .peekable();
    skip_php_trivia(&mut lookahead);
    if lookahead.next() != Some(BACKSLASH) || !consume_name(&mut lookahead) {
        return false;
    }
    loop {
        if lookahead.next() != Some(BACKSLASH) {
            return false;
        }
        if lookahead.peek().copied() == Some(OPEN_BRACE) {
            return true;
        }
        if !consume_name(&mut lookahead) {
            return false;
        }
    }
}

fn consume_name(input: &mut std::iter::Peekable<impl Iterator<Item = u32>>) -> bool {
    if !input.peek().copied().is_some_and(is_identifier_start) {
        return false;
    }
    input.next();
    while input
        .peek()
        .copied()
        .is_some_and(|next| is_identifier_start(next) || is_ascii_digit(next))
    {
        input.next();
    }
    true
}

fn scan_contextual_name(input: &mut InputStream, stack: &Stack) -> Result<(), ParseError> {
    input.advance(1);
    while input
        .next()
        .map(CodePoint::as_u32)
        .is_some_and(|next| is_identifier_start(next) || is_ascii_digit(next))
    {
        input.advance(1);
    }
    let delimiter = {
        let mut lookahead = input.lookahead().map(CodePoint::as_u32).peekable();
        skip_php_trivia(&mut lookahead);
        let delimiter = lookahead.next();
        let is_scope = delimiter == Some(COLON) && lookahead.peek().copied() == Some(COLON);
        (!is_scope).then_some(delimiter).flatten()
    };
    if stack.can_shift(terms::argumentName) && delimiter == Some(COLON) {
        input.accept_token(terms::argumentName)?;
    } else if stack.can_shift(terms::declarationName)
        && matches!(delimiter, Some(OPEN_PAREN | EQUALS | SEMICOLON_CHAR))
    {
        input.accept_token(terms::declarationName)?;
    }
    Ok(())
}

fn scan_reference_ampersand(input: &mut InputStream) -> Result<(), ParseError> {
    input.advance(1);
    let (starts_variable, starts_variadic) = {
        let mut lookahead = input.lookahead().map(CodePoint::as_u32).peekable();
        skip_php_trivia(&mut lookahead);
        let starts_variable = lookahead.peek().copied() == Some(DOLLAR);
        let starts_variadic = starts_with_variadic(&mut lookahead);
        (starts_variable, starts_variadic)
    };
    if starts_variable || starts_variadic {
        input.accept_token(terms::referenceAmpersand)?;
    }
    Ok(())
}

fn skip_php_trivia(lookahead: &mut std::iter::Peekable<impl Iterator<Item = u32>>) {
    loop {
        while lookahead.peek().copied().is_some_and(is_space) {
            lookahead.next();
        }
        match lookahead.peek().copied() {
            Some(HASH) => skip_line_comment(lookahead),
            Some(SLASH) => {
                lookahead.next();
                match lookahead.peek().copied() {
                    Some(SLASH) => skip_line_comment(lookahead),
                    Some(STAR) => skip_block_comment(lookahead),
                    _ => return,
                }
            }
            _ => return,
        }
    }
}

fn skip_line_comment(lookahead: &mut impl Iterator<Item = u32>) {
    for next in lookahead {
        if next == NEWLINE || next == CARRIAGE_RETURN {
            break;
        }
    }
}

fn skip_block_comment(lookahead: &mut std::iter::Peekable<impl Iterator<Item = u32>>) {
    lookahead.next();
    while let Some(next) = lookahead.next() {
        if next == STAR && lookahead.peek().copied() == Some(SLASH) {
            lookahead.next();
            break;
        }
    }
}

fn starts_with_variadic(lookahead: &mut std::iter::Peekable<impl Iterator<Item = u32>>) -> bool {
    lookahead.next() == Some(PERIOD)
        && lookahead.next() == Some(PERIOD)
        && lookahead.next() == Some(PERIOD)
}

fn scan_open_tag_boundary(input: &mut InputStream, _stack: &Stack) -> Result<(), ParseError> {
    if input
        .next()
        .map(CodePoint::as_u32)
        .is_none_or(|next| matches!(next, SPACE | TAB | NEWLINE | CARRIAGE_RETURN))
    {
        input.accept_token(terms::openTagBoundary)?;
    }
    Ok(())
}

fn scan_set_visibility(input: &mut InputStream, _stack: &Stack) -> Result<(), ParseError> {
    for spelling in ["public(set)", "protected(set)", "private(set)"] {
        if input_matches_ascii(input, spelling.as_bytes()) {
            input.advance(spelling.len());
            input.accept_token(terms::SetVisibility)?;
            return Ok(());
        }
    }
    Ok(())
}

fn input_matches_ascii(input: &InputStream, expected: &[u8]) -> bool {
    expected
        .iter()
        .copied()
        .enumerate()
        .all(|(offset, expected)| {
            input
                .peek(offset.cast_signed())
                .map(CodePoint::as_u32)
                .and_then(|actual| u8::try_from(actual).ok())
                .is_some_and(|actual| actual.eq_ignore_ascii_case(&expected))
        })
}

fn scan_cast(input: &mut InputStream) -> Result<(), ParseError> {
    input.advance(1);
    let is_cast = {
        let mut lookahead = input.lookahead().map(CodePoint::as_u32).peekable();
        skip_ascii_space(&mut lookahead);
        let mut spelling = Vec::new();
        while let Some(next) = lookahead
            .peek()
            .copied()
            .filter(|next| is_ascii_letter(*next))
        {
            spelling.push(u8::try_from(next).expect("ASCII letter fits u8"));
            lookahead.next();
        }
        skip_ascii_space(&mut lookahead);
        lookahead.peek().copied() == Some(CLOSE_PAREN) && is_cast_name(&spelling)
    };
    if is_cast {
        input.accept_token(terms::castOpen)?;
    }
    Ok(())
}

fn scan_heredoc(input: &mut InputStream) -> Result<(), ParseError> {
    let Some(frame) = scan_heredoc_header(input) else {
        return Ok(());
    };
    let mut frames = vec![frame];
    let mut line_start = true;

    loop {
        if input.next().is_none() {
            return Ok(());
        }
        let frame = frames
            .last_mut()
            .expect("the outer heredoc frame remains until acceptance");

        if line_start && frame.interpolation_depth == 0 {
            while matches!(input.next().map(CodePoint::as_u32), Some(SPACE | TAB)) {
                input.advance(1);
            }
            if heredoc_tag_matches(input, &frame.tag) {
                input.advance(frame.tag.len());
                frames.pop();
                if frames.is_empty() {
                    input.accept_token(terms::HeredocString)?;
                    return Ok(());
                }
                line_start = false;
                continue;
            }
        }
        line_start = false;

        let Some(next) = input.next().map(CodePoint::as_u32) else {
            return Ok(());
        };
        let frame = frames
            .last_mut()
            .expect("an unmatched heredoc frame remains");
        if frame.interpolated && frame.interpolation_depth == 0 {
            if next == BACKSLASH
                && let Some(escaped) = input.peek(1).map(CodePoint::as_u32)
            {
                input.advance(2);
                line_start = matches!(escaped, NEWLINE | CARRIAGE_RETURN);
                continue;
            }
            let braced_variable =
                next == DOLLAR && input.peek(1).map(CodePoint::as_u32) == Some(OPEN_BRACE);
            let braced_expression =
                next == OPEN_BRACE && input.peek(1).map(CodePoint::as_u32) == Some(DOLLAR);
            if braced_variable {
                input.advance(2);
                frame.interpolation_depth = 1;
                continue;
            }
            if braced_expression {
                input.advance(1);
                frame.interpolation_depth = 1;
                continue;
            }
        } else if frame.interpolated {
            let binary_prefix = matches!(next, value if value == u32::from(b'b') || value == u32::from(b'B'))
                && input.peek(1).map(CodePoint::as_u32) == Some(LESS_THAN)
                && input.peek(2).map(CodePoint::as_u32) == Some(LESS_THAN)
                && input.peek(3).map(CodePoint::as_u32) == Some(LESS_THAN);
            let heredoc_prefix = next == LESS_THAN
                && input.peek(1).map(CodePoint::as_u32) == Some(LESS_THAN)
                && input.peek(2).map(CodePoint::as_u32) == Some(LESS_THAN);
            if binary_prefix || heredoc_prefix {
                if binary_prefix {
                    input.advance(1);
                }
                if let Some(nested) = scan_heredoc_header(input) {
                    frames.push(nested);
                    line_start = true;
                    continue;
                }
            }

            if matches!(next, APOSTROPHE | QUOTE) {
                consume_quoted(input, next);
                continue;
            }
            if next == HASH || next == SLASH && input.peek(1).map(CodePoint::as_u32) == Some(SLASH)
            {
                consume_line_comment(input);
                continue;
            }
            if next == SLASH && input.peek(1).map(CodePoint::as_u32) == Some(STAR) {
                consume_block_comment(input);
                continue;
            }
            if next == OPEN_BRACE {
                frame.interpolation_depth += 1;
            } else if next == CLOSE_BRACE {
                frame.interpolation_depth -= 1;
            }
        }

        input.advance(1);
        line_start = matches!(next, NEWLINE | CARRIAGE_RETURN);
    }
}

fn scan_heredoc_header(input: &mut InputStream) -> Option<HeredocFrame> {
    input.advance(3);
    while matches!(input.next().map(CodePoint::as_u32), Some(SPACE | TAB)) {
        input.advance(1);
    }

    let delimiter = input
        .next()
        .map(CodePoint::as_u32)
        .filter(|next| matches!(*next, APOSTROPHE | QUOTE));
    if delimiter.is_some() {
        input.advance(1);
    }
    let first = input
        .next()
        .map(CodePoint::as_u32)
        .filter(|next| is_identifier_start(*next))?;
    let mut tag = vec![first];
    input.advance(1);
    while let Some(next) = input.next().map(CodePoint::as_u32) {
        if !is_identifier_start(next) && !is_ascii_digit(next) {
            break;
        }
        tag.push(next);
        input.advance(1);
    }
    if let Some(delimiter) = delimiter {
        if input.next().map(CodePoint::as_u32) != Some(delimiter) {
            return None;
        }
        input.advance(1);
    }
    if !matches!(
        input.next().map(CodePoint::as_u32),
        Some(NEWLINE | CARRIAGE_RETURN)
    ) {
        return None;
    }
    input.advance(1);
    Some(HeredocFrame {
        tag,
        interpolation_depth: 0,
        interpolated: delimiter != Some(APOSTROPHE),
    })
}

fn heredoc_tag_matches(input: &InputStream, tag: &[u32]) -> bool {
    let mut lookahead = input.lookahead().map(CodePoint::as_u32);
    for expected in tag {
        if lookahead.next() != Some(*expected) {
            return false;
        }
    }
    lookahead
        .next()
        .is_none_or(|next| !is_identifier_start(next) && !is_ascii_digit(next))
}

fn consume_quoted(input: &mut InputStream, delimiter: u32) {
    input.advance(1);
    while let Some(next) = input.next().map(CodePoint::as_u32) {
        input.advance(1);
        if next == BACKSLASH && input.next().is_some() {
            input.advance(1);
        } else if next == delimiter {
            break;
        }
    }
}

fn consume_line_comment(input: &mut InputStream) {
    while input
        .next()
        .map(CodePoint::as_u32)
        .is_some_and(|next| !matches!(next, NEWLINE | CARRIAGE_RETURN))
    {
        input.advance(1);
    }
}

fn consume_block_comment(input: &mut InputStream) {
    input.advance(2);
    while let Some(next) = input.next().map(CodePoint::as_u32) {
        input.advance(1);
        if next == STAR && input.next().map(CodePoint::as_u32) == Some(SLASH) {
            input.advance(1);
            break;
        }
    }
}

fn scan_eof(input: &mut InputStream, _stack: &Stack) -> Result<(), ParseError> {
    if input.next().is_none() {
        input.accept_token(terms::eof)?;
    }
    Ok(())
}

fn scan_halt_compiler_tail(input: &mut InputStream, stack: &Stack) -> Result<(), ParseError> {
    if !stack.can_shift(terms::haltCompilerTail) {
        return Ok(());
    }
    while input.next().is_some() {
        input.advance(1);
    }
    input.accept_token(terms::haltCompilerTail)?;
    Ok(())
}

fn scan_semicolon(input: &mut InputStream, stack: &Stack) -> Result<(), ParseError> {
    if input.next().map(CodePoint::as_u32) == Some(QUESTION)
        && input.peek(1).map(CodePoint::as_u32) == Some(GREATER_THAN)
        && stack.can_shift(terms::automaticSemicolon)
    {
        input.accept_token(terms::automaticSemicolon)?;
    }
    Ok(())
}

fn scan_interpolated(input: &mut InputStream, stack: &Stack) -> Result<(), ParseError> {
    let mut content = false;
    loop {
        let next = input.next().map(CodePoint::as_u32);
        let interpolation_start = next == Some(DOLLAR)
            && matches!(
                input.peek(1).map(CodePoint::as_u32),
                Some(after) if is_identifier_start(after) || after == OPEN_BRACE
            );
        let braced_interpolation =
            next == Some(OPEN_BRACE) && input.peek(1).map(CodePoint::as_u32) == Some(DOLLAR);
        if next.is_none() || next == Some(QUOTE) || interpolation_start || braced_interpolation {
            break;
        }

        if next == Some(BACKSLASH) {
            let escaped = escape_width(input);
            if escaped != 0 {
                if content {
                    break;
                }
                input.advance(escaped);
                input.accept_token(terms::EscapeSequence)?;
                return Ok(());
            }
        }

        let starts_postfix = next == Some(OPEN_BRACKET)
            || next == Some(MINUS)
                && input.peek(1).map(CodePoint::as_u32) == Some(GREATER_THAN)
                && input
                    .peek(2)
                    .map(CodePoint::as_u32)
                    .is_some_and(is_identifier_start)
            || next == Some(QUESTION)
                && input.peek(1).map(CodePoint::as_u32) == Some(MINUS)
                && input.peek(2).map(CodePoint::as_u32) == Some(GREATER_THAN)
                && input
                    .peek(3)
                    .map(CodePoint::as_u32)
                    .is_some_and(is_identifier_start);
        if !content && starts_postfix && stack.can_shift(terms::afterInterpolation) {
            break;
        }

        input.advance(1);
        content = true;
    }
    if content {
        input.accept_token(terms::interpolatedStringContent)?;
    }
    Ok(())
}

fn escape_width(input: &InputStream) -> usize {
    let after = input.peek(1).map(CodePoint::as_u32);
    if after.is_some_and(is_simple_escape) {
        return 2;
    }
    if after.is_some_and(is_octal_digit) {
        let mut width = 2_usize;
        while width < 5
            && input
                .peek(width.cast_signed())
                .map(CodePoint::as_u32)
                .is_some_and(is_octal_digit)
        {
            width += 1;
        }
        return width;
    }
    if after == Some(u32::from(b'x'))
        && input
            .peek(2)
            .map(CodePoint::as_u32)
            .is_some_and(is_hex_digit)
    {
        return if input
            .peek(3)
            .map(CodePoint::as_u32)
            .is_some_and(is_hex_digit)
        {
            4
        } else {
            3
        };
    }
    if after == Some(u32::from(b'u')) && input.peek(2).map(CodePoint::as_u32) == Some(OPEN_BRACE) {
        let mut width = 3_usize;
        loop {
            match input.peek(width.cast_signed()).map(CodePoint::as_u32) {
                Some(CLOSE_BRACE) => return if width == 3 { 0 } else { width + 1 },
                Some(next) if is_hex_digit(next) => width += 1,
                _ => return 0,
            }
        }
    }
    0
}

fn skip_ascii_space(input: &mut std::iter::Peekable<impl Iterator<Item = u32>>) {
    while input.peek().copied().is_some_and(is_space) {
        input.next();
    }
}

fn is_cast_name(value: &[u8]) -> bool {
    [
        b"int".as_slice(),
        b"integer",
        b"bool",
        b"boolean",
        b"binary",
        b"float",
        b"double",
        b"real",
        b"string",
        b"array",
        b"object",
        b"unset",
        b"void",
    ]
    .into_iter()
    .any(|candidate| value.eq_ignore_ascii_case(candidate))
}

const fn is_space(value: u32) -> bool {
    matches!(value, SPACE | TAB | NEWLINE | CARRIAGE_RETURN)
}

const fn is_ascii_letter(value: u32) -> bool {
    value >= b'a' as u32 && value <= b'z' as u32 || value >= b'A' as u32 && value <= b'Z' as u32
}

const fn is_ascii_digit(value: u32) -> bool {
    value >= b'0' as u32 && value <= b'9' as u32
}

const fn is_octal_digit(value: u32) -> bool {
    value >= b'0' as u32 && value <= b'7' as u32
}

const fn is_hex_digit(value: u32) -> bool {
    is_ascii_digit(value)
        || value >= b'a' as u32 && value <= b'f' as u32
        || value >= b'A' as u32 && value <= b'F' as u32
}

const fn is_simple_escape(value: u32) -> bool {
    value == b'n' as u32
        || value == b'r' as u32
        || value == b't' as u32
        || value == b'v' as u32
        || value == b'e' as u32
        || value == b'f' as u32
        || value == BACKSLASH
        || value == DOLLAR
        || value == QUOTE
        || value == OPEN_BRACE
}

const fn is_identifier_start(value: u32) -> bool {
    value == b'_' as u32 || value >= 0x80 || is_ascii_letter(value)
}
