//! Source recognition across Swift code-item trivia boundaries.
//!
//! External tokenizer registration, parser-state queries, term selection, and
//! token acceptance remain in the parent module. This module classifies only
//! the trivia and source that decide whether a physical line break continues
//! the current construct.

use rezel_common::CodePoint;
use rezel_lr::InputStream;

use super::lexical::{
    is_identifier_start, is_operator_start, scan_lookahead_identifier_after_first,
};

use super::lookahead::{
    TriviaBoundary, scan_trivia_boundary, skip_directive_line, skip_trivia,
    starts_member_access_continuation, trivia_contains_line_break,
};
use super::operator::starts_binary_operator_like_continuation;

const POUND: u32 = b'#' as u32;
const BACKTICK: u32 = b'`' as u32;
const LEFT_BRACE: u32 = b'{' as u32;
const LEFT_PAREN: u32 = b'(' as u32;
const COLON: u32 = b':' as u32;
const COMMA: u32 = b',' as u32;
const MINUS: u32 = b'-' as u32;
const PERIOD: u32 = b'.' as u32;
const RIGHT_ANGLE: u32 = b'>' as u32;
const RIGHT_BRACKET: u32 = b']' as u32;
const RIGHT_PAREN: u32 = b')' as u32;
const DOLLAR: u32 = b'$' as u32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CodeItemBoundary {
    None,
    LineBreak,
    DeclarationEffect,
    EnumCaseParameter,
    PostfixIfConfig,
}

#[derive(Clone, Copy)]
pub(super) enum ShiftRole {
    DeclarationEffect,
    EnumCaseParameter,
    PostfixIfConfig,
    PostfixMember,
    BinaryOperator,
    AsKeyword,
    IsKeyword,
}

pub(super) fn has_line_break(input: &InputStream) -> bool {
    let mut lookahead = input.lookahead().map(CodePoint::as_u32).peekable();
    trivia_contains_line_break(&mut lookahead)
}

pub(super) fn classify_code_item_boundary(
    input: &InputStream,
    mut stop_after_line_break: impl FnMut(&InputStream) -> bool,
    mut can_shift: impl FnMut(ShiftRole) -> bool,
) -> CodeItemBoundary {
    let mut lookahead = input.lookahead().map(CodePoint::as_u32).peekable();
    let first = match scan_trivia_boundary(&mut lookahead, || stop_after_line_break(input)) {
        TriviaBoundary::StoppedAtLineBreak
        | TriviaBoundary::Complete {
            has_line_break: false,
            ..
        } => return CodeItemBoundary::None,
        TriviaBoundary::Complete { first, .. } => first,
    };
    match first {
        Some(LEFT_PAREN) if can_shift(ShiftRole::EnumCaseParameter) => {
            CodeItemBoundary::EnumCaseParameter
        }
        Some(POUND)
            if can_shift(ShiftRole::PostfixIfConfig)
                && starts_postfix_if_config_after_pound(&mut lookahead) =>
        {
            CodeItemBoundary::PostfixIfConfig
        }
        Some(LEFT_BRACE | COLON | COMMA | RIGHT_PAREN | RIGHT_BRACKET) => CodeItemBoundary::None,
        Some(MINUS) if lookahead.peek() == Some(&RIGHT_ANGLE) => CodeItemBoundary::None,
        Some(PERIOD)
            if starts_member_access_continuation(&mut lookahead)
                && can_shift(ShiftRole::PostfixMember) =>
        {
            CodeItemBoundary::None
        }
        Some(first) if is_operator_start(first) => {
            if starts_binary_operator_like_continuation(
                first,
                &mut lookahead,
                can_shift(ShiftRole::BinaryOperator),
            ) {
                CodeItemBoundary::None
            } else {
                CodeItemBoundary::LineBreak
            }
        }
        Some(first) if is_boundary_identifier_start(first) => {
            classify_identifier_boundary(first, &mut lookahead, &mut can_shift)
        }
        _ => CodeItemBoundary::LineBreak,
    }
}

fn is_boundary_identifier_start(first: u32) -> bool {
    matches!(
        u8::try_from(first),
        Ok(b'a' | b'c' | b'e' | b'i' | b'r' | b't' | b'w')
    )
}

fn classify_identifier_boundary(
    first: u32,
    input: &mut std::iter::Peekable<impl Iterator<Item = u32>>,
    can_shift: &mut impl FnMut(ShiftRole) -> bool,
) -> CodeItemBoundary {
    let Some(identifier) = scan_lookahead_identifier_after_first(first, input) else {
        return CodeItemBoundary::LineBreak;
    };
    match identifier.ascii_spelling() {
        Some(b"as") if can_shift(ShiftRole::AsKeyword) => CodeItemBoundary::None,
        Some(b"is") if can_shift(ShiftRole::IsKeyword) => CodeItemBoundary::None,
        Some(b"where" | b"else" | b"catch") => CodeItemBoundary::None,
        Some(b"async" | b"throws" | b"reasync" | b"rethrows")
            if can_shift(ShiftRole::DeclarationEffect) =>
        {
            CodeItemBoundary::DeclarationEffect
        }
        _ => CodeItemBoundary::LineBreak,
    }
}

// SwiftSyntax only classifies `#if` as a postfix-expression suffix when the
// first body, after any leading nested `#if` headers, starts with a dotted
// member suffix. The directive clauses themselves remain grammar-owned.
fn starts_postfix_if_config_after_pound(
    input: &mut std::iter::Peekable<impl Iterator<Item = u32>>,
) -> bool {
    loop {
        let Some(first) = input.next() else {
            return false;
        };
        let Some(directive) = scan_lookahead_identifier_after_first(first, input) else {
            return false;
        };
        if !directive.is(b"if") || !skip_directive_line(input) {
            return false;
        }
        skip_trivia(input);
        if input.peek() != Some(&POUND) {
            return starts_postfix_if_config_clause_entry(input);
        }
        input.next();
    }
}

fn starts_postfix_if_config_clause_entry(
    input: &mut std::iter::Peekable<impl Iterator<Item = u32>>,
) -> bool {
    if input.next() != Some(PERIOD) {
        return false;
    }
    input.peek().copied().is_some_and(|next| {
        (u32::from(b'0')..=u32::from(b'9')).contains(&next)
            || matches!(next, BACKTICK | DOLLAR)
            || is_identifier_start(next)
    })
}

#[cfg(test)]
mod tests {
    use super::{scan_trivia_boundary, starts_postfix_if_config_after_pound};
    use crate::tokens::lookahead::TriviaBoundary;

    fn starts_postfix_if_config(source_after_pound: &str) -> bool {
        starts_postfix_if_config_after_pound(
            &mut source_after_pound.chars().map(u32::from).peekable(),
        )
    }

    #[test]
    fn postfix_if_config_requires_a_dotted_first_body() {
        let cases = [
            ("direct member", "if FEATURE\n.member", true),
            (
                "nested first body",
                "if OUTER\n#if INNER\n  .method()",
                true,
            ),
            (
                "comment trivia",
                "if FEATURE // header\n/* body */\n.`class`",
                true,
            ),
            ("empty first body", "if FEATURE\n#else\n.member", false),
            ("ordinary declaration", "if FEATURE\nstruct S {}", false),
            ("binary expression", "if FEATURE\n+ value", false),
            ("directive word boundary", "ifFeature\n.member", false),
            ("missing member", "if FEATURE\n.", false),
        ];

        for (name, source, expected) in cases {
            assert_eq!(
                starts_postfix_if_config(source),
                expected,
                "{name}: {source}"
            );
        }
    }

    #[test]
    fn trivia_boundaries_preserve_swift_line_end_semantics() {
        let cases = [
            ("line comment at EOF", "// EOF", true, None),
            ("bare slash", "/* no break */ /", false, Some('/')),
            (
                "nested block comment",
                "/* outer /* nested\n */ outer */ next",
                true,
                Some('n'),
            ),
            ("CRLF", "\r\nnext", true, Some('n')),
        ];

        for (name, source, expected_break, expected_first) in cases {
            let mut input = source.chars().map(u32::from).peekable();
            let boundary = scan_trivia_boundary(&mut input, || false);
            assert_eq!(
                boundary,
                TriviaBoundary::Complete {
                    first: expected_first.map(u32::from),
                    has_line_break: expected_break,
                },
                "{name}: {source}"
            );
        }
    }
}
