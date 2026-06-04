#![forbid(unsafe_code)]

use std::sync::Arc;

use rezel_common::{
    CodePoint, Input, ParseErrorKind, ParseRequest, StringInput, TextRange, TextSize,
};

#[test]
fn code_points_include_surrogates_but_not_values_above_unicode() {
    let surrogate = CodePoint::new(0xd800).expect("surrogates are code points");
    let maximum = CodePoint::new(CodePoint::MAX).expect("Unicode maximum is a code point");

    assert!(surrogate.is_surrogate());
    assert!(!surrogate.is_scalar());
    assert_eq!(surrogate.as_char(), None);
    assert!(maximum.is_scalar());
    assert_eq!(maximum.as_char(), char::from_u32(CodePoint::MAX));
    assert_eq!(CodePoint::new(CodePoint::MAX + 1), None);
}

#[test]
fn shared_chunks_preserve_code_points_and_original_boundaries() {
    let input = StringInput::try_new("a😀b").unwrap();
    let mut chunk = input.identity_chunk(1.into()).expect("shared input chunk");

    assert_eq!(chunk.raw_start(), 1.into());
    assert_eq!(chunk.raw_end(), 6.into());
    let emoji = chunk.character(1.into()).expect("emoji code point");
    assert_eq!(emoji.value(), CodePoint::from('😀'));
    assert_eq!(emoji.raw_end(), 5.into());
    assert!(chunk.is_boundary(1.into()));
    assert!(!chunk.is_boundary(2.into()));
    assert!(chunk.is_boundary(5.into()));

    chunk.truncate(5.into());
    assert_eq!(chunk.raw_end(), 5.into());
    assert!(!chunk.contains(5.into()));
    assert!(chunk.character(5.into()).is_none());
}

#[test]
fn string_input_reports_utf8_boundaries_without_reading() {
    let input = StringInput::try_new("a😀b").unwrap();

    assert!(input.is_boundary(0.into()));
    assert!(input.is_boundary(1.into()));
    assert!(!input.is_boundary(2.into()));
    assert!(!input.is_boundary(3.into()));
    assert!(!input.is_boundary(4.into()));
    assert!(input.is_boundary(5.into()));
    assert!(input.is_boundary(6.into()));
    assert!(!input.is_boundary(7.into()));
}

#[test]
fn parse_ranges_reject_utf8_interior_endpoints() {
    let input: Arc<dyn Input> = Arc::new(StringInput::try_new("a😀b").unwrap());

    for position in 2..=4 {
        let position = TextSize::from(position);
        let error = ParseRequest::ranges(
            Arc::clone(&input),
            vec![TextRange::new(TextSize::from(0), position)],
        )
        .unwrap_err();

        assert_eq!(error.kind(), ParseErrorKind::Input);
        assert_eq!(error.position(), Some(position));
        assert_eq!(
            error.message(),
            "parse range endpoint is not a UTF-8 boundary"
        );
    }
}
