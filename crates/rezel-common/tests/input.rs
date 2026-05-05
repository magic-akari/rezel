#![forbid(unsafe_code)]

use rezel_common::{Input, StringInput};

#[test]
fn shared_chunks_preserve_utf8_units_and_original_boundaries() {
    let input = StringInput::try_new("a😀b").unwrap();
    let mut chunk = input.logical_chunk(1.into()).expect("shared input chunk");

    assert_eq!(chunk.raw_start(), 1.into());
    assert_eq!(chunk.raw_end(), 6.into());
    let emoji = chunk.logical_units(1.into()).expect("emoji units");
    assert_eq!(emoji.units(), &[0xd83d, 0xde00]);
    assert_eq!(emoji.raw_end(), 5.into());

    chunk.truncate(5.into());
    assert_eq!(chunk.raw_end(), 5.into());
    assert!(!chunk.contains(5.into()));
    assert!(chunk.logical_units(5.into()).is_none());
}
