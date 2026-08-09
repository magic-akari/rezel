use rezel_common::{CodePoint, ParseError, first_invalid_identifier_offset};
use rezel_lr::{
    ExternalTokenizer, ExternalTokenizerStart, InputStream, Stack, StrictTokenValidationError,
    StrictTokenValidator, TokenizerFlags,
};
use unicode_normalization::UnicodeNormalization;

pub(crate) const UNICODE_VERSION: (u8, u8, u8) = unicode_ident::UNICODE_VERSION;

pub(crate) fn canonical_name(spelling: &str) -> String {
    spelling.nfkc().collect()
}

const IDENTIFIER_START: ExternalTokenizerStart = ExternalTokenizerStart::NONE
    .with_ascii(b'_')
    .with_ascii_range(b'A'..=b'Z')
    .with_ascii_range(b'a'..=b'z')
    .with_non_ascii();

pub(crate) static TOKENIZER: ExternalTokenizer = ExternalTokenizer::new(
    scan,
    TokenizerFlags {
        contextual: false,
        fallback: false,
        extend: false,
    },
)
.with_start(IDENTIFIER_START);

pub(crate) static STRICT_TOKEN_VALIDATORS: [StrictTokenValidator; 1] = [StrictTokenValidator::new(
    crate::terms::identifier,
    validate,
)];

fn validate(spelling: &str) -> Result<(), StrictTokenValidationError> {
    let invalid = first_invalid_identifier_offset(
        spelling,
        |character| character == '_' || unicode_ident::is_xid_start(character),
        unicode_ident::is_xid_continue,
    );
    match invalid {
        Some(offset) => Err(StrictTokenValidationError::new(
            offset,
            "invalid Python identifier",
        )),
        None => Ok(()),
    }
}

fn scan(input: &mut InputStream, _stack: &Stack) -> Result<(), ParseError> {
    if looks_like_string_prefix(input) {
        return Ok(());
    }
    let Some(first) = input.next().map(CodePoint::as_u32) else {
        return Ok(());
    };
    if !is_identifier_candidate_start(first) {
        return Ok(());
    }
    if first < 0x80 {
        input.advance_ascii_while(|byte| is_identifier_candidate_continue(u32::from(byte)));
    } else {
        input.advance(1);
    }
    loop {
        if input.advance_ascii_while(|byte| is_identifier_candidate_continue(u32::from(byte))) != 0
        {
            continue;
        }
        let Some(character) = input.next().map(CodePoint::as_u32) else {
            break;
        };
        if !is_identifier_candidate_continue(character) {
            break;
        }
        input.advance(1);
    }
    input.accept_token(crate::terms::identifier)
}

fn looks_like_string_prefix(input: &InputStream) -> bool {
    let Some(first) = input.peek(0).and_then(ascii_lowercase) else {
        return false;
    };
    let second = input.peek(1);
    if matches!(second.map(CodePoint::as_u32), Some(0x27 | 0x22)) {
        return matches!(first, b'b' | b'f' | b'r' | b't' | b'u');
    }
    let Some(second_prefix) = second.and_then(ascii_lowercase) else {
        return false;
    };
    if !matches!(input.peek(2).map(CodePoint::as_u32), Some(0x27 | 0x22)) {
        return false;
    }
    matches!(
        (first, second_prefix),
        (b'b' | b'f' | b't', b'r') | (b'r', b'b' | b'f' | b't')
    )
}

fn ascii_lowercase(value: CodePoint) -> Option<u8> {
    let value = u8::try_from(value.as_u32()).ok()?;
    value
        .is_ascii_alphabetic()
        .then(|| value.to_ascii_lowercase())
}

const fn is_identifier_candidate_start(value: u32) -> bool {
    matches!(value, 0x41..=0x5a | 0x5f | 0x61..=0x7a | 0xa1..=0x0010_ffff)
}

const fn is_identifier_candidate_continue(value: u32) -> bool {
    matches!(value, 0x30..=0x39) || is_identifier_candidate_start(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_profile_tracks_unicode_ident() {
        assert_eq!(UNICODE_VERSION, unicode_ident::UNICODE_VERSION);
        assert!(validate("_name").is_ok());
        assert!(validate("λ2").is_ok());
        assert!(validate("name😀").is_err());
    }

    #[test]
    fn parser_applies_validation_only_in_strict_mode() {
        let source = "name😀 = 1\n";
        crate::parser()
            .parse(source)
            .expect("recovering Python accepts the broad identifier candidate");
        let error = crate::parser()
            .with_strict(true)
            .parse(source)
            .expect_err("strict Python validates the selected identifier token");
        assert_eq!(error.message(), "invalid Python identifier");
    }

    #[test]
    #[allow(clippy::unicode_not_nfc)] // The compatibility character is the test input.
    fn canonical_names_follow_cpython_nfkc() {
        assert_eq!(canonical_name("Kelvin"), "Kelvin");
        assert_eq!(canonical_name("变量"), "变量");
    }
}
