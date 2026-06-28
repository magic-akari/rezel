#![forbid(unsafe_code)]

use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

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

#[test]
fn default_reverse_character_reads_are_utf8_bounded() {
    struct BoundedReverseInput {
        inner: StringInput,
        reads: AtomicUsize,
        maximum_width: AtomicUsize,
    }

    impl Input for BoundedReverseInput {
        fn len(&self) -> TextSize {
            self.inner.len()
        }

        fn chunk(&self, from: TextSize) -> Cow<'_, str> {
            self.inner.chunk(from)
        }

        fn read(&self, range: TextRange) -> Cow<'_, str> {
            let width = usize::from(range.len());
            assert!(
                width <= 4,
                "reverse reads must stay within one UTF-8 scalar"
            );
            self.reads.fetch_add(1, Ordering::Relaxed);
            self.maximum_width.fetch_max(width, Ordering::Relaxed);
            self.inner.read(range)
        }

        fn is_boundary(&self, position: TextSize) -> bool {
            self.inner.is_boundary(position)
        }
    }

    let source = format!("{}😀", "a".repeat(4_096));
    let input = BoundedReverseInput {
        inner: StringInput::try_new(source).unwrap(),
        reads: AtomicUsize::new(0),
        maximum_width: AtomicUsize::new(0),
    };
    let mut before = input.len();
    let mut characters = 0;
    while let Some((start, character)) = input.character_before(before) {
        assert_eq!(character.raw_end(), before);
        before = start;
        characters += 1;
    }

    assert_eq!(before, TextSize::from(0));
    assert_eq!(characters, 4_097);
    assert_eq!(input.reads.load(Ordering::Relaxed), characters);
    assert_eq!(input.maximum_width.load(Ordering::Relaxed), 4);
}
