use crate::table::{GOTO_COMPRESSED_HEADER, GOTO_COMPRESSED_TAG};

const NO_TERM: u32 = u32::MAX;
const MAX_LINEAR_EXCEPTION_COUNT: usize = 16;

#[derive(Clone, Copy, Debug)]
struct GotoEntry {
    source: u16,
    target: u16,
}

fn sort_exceptions(entries: &mut [GotoEntry]) -> Result<(), &'static str> {
    for index in 1..entries.len() {
        let entry = entries[index];
        let mut insertion = index;
        while insertion != 0 && entry.source < entries[insertion - 1].source {
            entries[insertion] = entries[insertion - 1];
            insertion -= 1;
        }
        if insertion != 0 && entries[insertion - 1].source == entry.source {
            return Err("goto projection has duplicate exception sources");
        }
        entries[insertion] = entry;
    }
    Ok(())
}

fn balance_exceptions(entries: &mut [GotoEntry]) -> Result<(), &'static str> {
    sort_exceptions(entries)?;
    let sorted = entries.to_vec();
    let mut next = 0;
    fill_balanced(entries, &sorted, 0, &mut next);
    Ok(())
}

fn fill_balanced(entries: &mut [GotoEntry], sorted: &[GotoEntry], index: usize, next: &mut usize) {
    if index >= entries.len() {
        return;
    }
    fill_balanced(entries, sorted, index * 2 + 1, next);
    entries[index] = sorted[*next];
    *next += 1;
    fill_balanced(entries, sorted, index * 2 + 2, next);
}

#[derive(Clone, Copy, Debug)]
struct GotoTerm {
    exception_start: u32,
    exception_end: u32,
    default_start: u32,
    default_end: u32,
    default_target: u16,
}

#[cold]
#[inline(never)]
pub(crate) fn decode_goto_sources(
    table: &[u16],
    position: usize,
    group_tag: u16,
) -> Result<(Vec<u16>, usize), &'static str> {
    let compressed = group_tag & !1 == GOTO_COMPRESSED_TAG;
    if !compressed {
        let raw_count = usize::from(group_tag >> 1);
        let end = position
            .checked_add(raw_count)
            .ok_or("goto projection group length overflows")?;
        let sources = table
            .get(position..end)
            .ok_or("goto projection sources are truncated")?;
        return Ok((sources.to_vec(), end));
    }

    let source_count = usize::from(
        *table
            .get(position)
            .ok_or("goto compressed source count is truncated")?,
    );
    if source_count == 0 {
        return Err("goto compressed source group is empty");
    }
    decode_goto_source_deltas(table, position + 1, source_count)
}

pub(crate) fn decode_goto_header(
    table: &[u16],
) -> Result<(Vec<Option<usize>>, usize), &'static str> {
    let marker = *table.first().ok_or("goto table cannot be empty")?;
    if marker != GOTO_COMPRESSED_HEADER {
        return decode_raw_goto_header(table, marker);
    }

    let term_count = usize::from(
        *table
            .get(1)
            .ok_or("goto compressed header term count is truncated")?,
    );
    let header_words = usize::from(
        *table
            .get(2)
            .ok_or("goto compressed header length is truncated")?,
    );
    let data_start = 3_usize
        .checked_add(header_words)
        .ok_or("goto compressed header length overflows")?;
    if data_start > table.len() {
        return Err("goto compressed header is truncated");
    }

    let byte_limit = header_words
        .checked_mul(2)
        .ok_or("goto compressed header length overflows")?;
    let mut byte_index = 0_usize;
    let mut previous = 0_i64;
    let mut positions = Vec::with_capacity(term_count);
    for _ in 0..term_count {
        let code = decode_header_varint(table, 3, &mut byte_index, byte_limit)?;
        if code == 0 {
            positions.push(None);
            continue;
        }

        let zigzag = code - 1;
        let magnitude =
            i64::try_from(zigzag >> 1).map_err(|_| "goto compressed header delta overflows")?;
        let sign = -i64::try_from(zigzag & 1).expect("zigzag sign fits i64");
        let delta = magnitude ^ sign;
        let relative = previous
            .checked_add(delta)
            .filter(|position| *position >= 0)
            .ok_or("goto compressed header position overflows")?;
        let relative =
            usize::try_from(relative).map_err(|_| "goto compressed header position overflows")?;
        let position = data_start
            .checked_add(relative)
            .ok_or("goto compressed header position overflows")?;
        positions.push(Some(position));
        previous =
            i64::try_from(relative).map_err(|_| "goto compressed header position overflows")?;
    }

    let remaining = byte_limit - byte_index;
    if remaining > 1 {
        return Err("goto compressed header has trailing data");
    }
    if remaining == 1 && packed_byte(table, 3, byte_index) != 0 {
        return Err("goto compressed header has nonzero padding");
    }
    Ok((positions, data_start))
}

fn decode_raw_goto_header(
    table: &[u16],
    term_count: u16,
) -> Result<(Vec<Option<usize>>, usize), &'static str> {
    let header_length = usize::from(term_count) + 1;
    let raw_positions = table
        .get(1..header_length)
        .ok_or("goto table header is truncated")?;
    let mut positions = Vec::with_capacity(raw_positions.len());
    for position in raw_positions {
        let position = usize::from(*position);
        if position < header_length {
            if position != 1 {
                return Err("goto table has an invalid empty entry");
            }
            positions.push(None);
        } else {
            positions.push(Some(position));
        }
    }
    Ok((positions, header_length))
}

fn decode_header_varint(
    table: &[u16],
    word_start: usize,
    byte_index: &mut usize,
    byte_limit: usize,
) -> Result<u64, &'static str> {
    let mut value = 0_u64;
    let mut shift = 0_u32;
    loop {
        if *byte_index >= byte_limit {
            return Err("goto compressed header entries are truncated");
        }
        let byte = packed_byte(table, word_start, *byte_index);
        *byte_index += 1;
        if shift == 63 && byte & 0x7e != 0 {
            return Err("goto compressed header delta overflows");
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
        if shift >= u64::BITS {
            return Err("goto compressed header delta overflows");
        }
    }
}

fn packed_byte(table: &[u16], word_start: usize, byte_index: usize) -> u8 {
    let word = table[word_start + byte_index / 2];
    if byte_index & 1 == 0 {
        u8::try_from(word & 0xff).expect("masked table byte fits u8")
    } else {
        u8::try_from(word >> 8).expect("shifted table byte fits u8")
    }
}

#[cold]
#[inline(never)]
fn decode_goto_source_deltas(
    table: &[u16],
    position: usize,
    source_count: usize,
) -> Result<(Vec<u16>, usize), &'static str> {
    let mut sources = Vec::with_capacity(source_count);
    let mut byte_index = 0_usize;
    let mut previous = 0_u16;
    for source_index in 0..source_count {
        let mut value = 0_u32;
        let mut shift = 0_u32;
        loop {
            let word_index = position
                .checked_add(byte_index / 2)
                .ok_or("goto compressed source offset overflows")?;
            let word = *table
                .get(word_index)
                .ok_or("goto compressed sources are truncated")?;
            let byte = if byte_index & 1 == 0 {
                u8::try_from(word & 0xff).expect("masked source byte fits u8")
            } else {
                u8::try_from(word >> 8).expect("shifted source byte fits u8")
            };
            byte_index += 1;

            value |= u32::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 14 {
                return Err("goto compressed source delta overflows");
            }
        }

        let delta = u16::try_from(value).map_err(|_| "goto compressed source delta overflows")?;
        let source = if source_index == 0 {
            delta
        } else {
            if delta == 0 {
                return Err("goto compressed sources are not strictly increasing");
            }
            previous
                .checked_add(delta)
                .ok_or("goto compressed source delta overflows")?
        };
        sources.push(source);
        previous = source;
    }
    let words_read = byte_index.div_ceil(2);
    let end = position
        .checked_add(words_read)
        .ok_or("goto compressed source offset overflows")?;
    Ok((sources, end))
}

impl GotoTerm {
    const EMPTY: Self = Self {
        exception_start: NO_TERM,
        exception_end: NO_TERM,
        default_start: NO_TERM,
        default_end: NO_TERM,
        default_target: 0,
    };
}

/// Parser-private projection over generated term-major goto groups.
#[derive(Debug)]
pub(crate) struct GotoIndex {
    terms: Box<[GotoTerm]>,
    exceptions: Box<[GotoEntry]>,
    default_sources: Box<[u16]>,
}

impl GotoIndex {
    pub(crate) fn build(table: &[u16]) -> Result<Self, &'static str> {
        let (positions, _) = decode_goto_header(table)?;
        let mut terms = vec![GotoTerm::EMPTY; positions.len()];
        let mut exceptions = Vec::new();
        let mut default_sources = Vec::new();
        for (slot, position) in terms.iter_mut().zip(positions) {
            let Some(mut position) = position else {
                continue;
            };
            let exception_start =
                u32::try_from(exceptions.len()).map_err(|_| "goto projection is too large")?;
            loop {
                let group_tag = *table
                    .get(position)
                    .ok_or("goto projection group is truncated")?;
                let target = *table
                    .get(position + 1)
                    .ok_or("goto projection target is truncated")?;
                position = position
                    .checked_add(2)
                    .ok_or("goto projection offset overflows")?;
                let (sources, end) = decode_goto_sources(table, position, group_tag)?;
                if group_tag & 1 == 0 {
                    exceptions.extend(
                        sources
                            .into_iter()
                            .map(|source| GotoEntry { source, target }),
                    );
                    position = end;
                    continue;
                }
                let exception_end =
                    u32::try_from(exceptions.len()).map_err(|_| "goto projection is too large")?;
                let exception_range = exception_start as usize..exception_end as usize;
                if exception_range.len() > MAX_LINEAR_EXCEPTION_COUNT {
                    balance_exceptions(&mut exceptions[exception_range])?;
                }
                let default_start = u32::try_from(default_sources.len())
                    .map_err(|_| "goto projection is too large")?;
                default_sources.extend_from_slice(&sources);
                let default_end = u32::try_from(default_sources.len())
                    .map_err(|_| "goto projection is too large")?;
                *slot = GotoTerm {
                    exception_start,
                    exception_end,
                    default_start,
                    default_end,
                    default_target: target,
                };
                break;
            }
        }
        Ok(Self {
            terms: terms.into_boxed_slice(),
            exceptions: exceptions.into_boxed_slice(),
            default_sources: default_sources.into_boxed_slice(),
        })
    }

    pub(crate) fn get(&self, state: u16, term: u16, loose: bool) -> Option<u16> {
        let term = *self.terms.get(usize::from(term))?;
        if term.exception_start == NO_TERM {
            return None;
        }
        let exceptions =
            &self.exceptions[term.exception_start as usize..term.exception_end as usize];
        let entry = if exceptions.len() <= MAX_LINEAR_EXCEPTION_COUNT {
            exceptions.iter().find(|entry| entry.source == state)
        } else {
            let mut index = 0;
            loop {
                let Some(entry) = exceptions.get(index) else {
                    break None;
                };
                if entry.source == state {
                    break Some(entry);
                }
                index = index * 2 + usize::from(entry.source < state) + 1;
            }
        };
        if let Some(entry) = entry {
            return Some(entry.target);
        }
        if loose {
            return Some(term.default_target);
        }
        let default_sources =
            &self.default_sources[term.default_start as usize..term.default_end as usize];
        default_sources
            .contains(&state)
            .then_some(term.default_target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_strict_groups_and_loose_fallbacks() {
        let table = [2, 3, 1, 2 << 1, 4, 1, 3, (3 << 1) | 1, 8, 0, 2, 4];
        let index = GotoIndex::build(&table).unwrap();

        assert_eq!(index.get(1, 0, true), Some(4));
        assert_eq!(index.get(3, 0, true), Some(4));
        assert_eq!(index.get(7, 0, true), Some(8));
        assert_eq!(index.get(4, 0, false), Some(8));
        assert_eq!(index.get(7, 0, false), None);
        assert_eq!(index.get(0, 1, true), None);
        assert_eq!(index.get(0, 2, true), None);
    }

    #[test]
    fn expands_delta_compressed_source_groups() {
        let table = [1, 2, 0x8001, 8, 5, 0x0100, 0x0181, 0x8d01, 0x0002];
        let index = GotoIndex::build(&table).unwrap();

        for source in [0, 1, 130, 131, 400] {
            assert_eq!(index.get(source, 0, false), Some(8));
        }
        assert_eq!(index.get(2, 0, false), None);
        assert_eq!(index.get(2, 0, true), Some(8));
    }

    #[test]
    fn expands_delta_compressed_term_headers() {
        let table = [GOTO_COMPRESSED_HEADER, 3, 2, 0x0001, 0x0001, 5, 8, 0, 2];
        let index = GotoIndex::build(&table).unwrap();

        for term in [0, 2] {
            assert_eq!(index.get(0, term, false), Some(8));
            assert_eq!(index.get(2, term, false), Some(8));
        }
        assert_eq!(index.get(0, 1, false), None);
    }

    #[test]
    fn rejects_malformed_delta_compressed_term_headers() {
        assert_eq!(
            GotoIndex::build(&[GOTO_COMPRESSED_HEADER, 1, 0]).unwrap_err(),
            "goto compressed header entries are truncated"
        );
        assert_eq!(
            GotoIndex::build(&[GOTO_COMPRESSED_HEADER, 1, 2, 0, 0]).unwrap_err(),
            "goto compressed header has trailing data"
        );
        assert_eq!(
            GotoIndex::build(&[GOTO_COMPRESSED_HEADER, 1, 1, 2]).unwrap_err(),
            "goto compressed header position overflows"
        );
    }

    #[test]
    fn rejects_malformed_delta_compressed_source_groups() {
        assert_eq!(
            GotoIndex::build(&[1, 2, 0x8001, 8, 0]).unwrap_err(),
            "goto compressed source group is empty"
        );
        assert_eq!(
            GotoIndex::build(&[1, 2, 0x8001, 8, 2, 0]).unwrap_err(),
            "goto compressed sources are not strictly increasing"
        );
        assert_eq!(
            GotoIndex::build(&[1, 2, 0x8001, 8, 1, 0x8080]).unwrap_err(),
            "goto compressed sources are truncated"
        );
    }

    #[test]
    fn balances_long_exception_rows_for_logarithmic_lookup() {
        for source_count in MAX_LINEAR_EXCEPTION_COUNT + 1..=64 {
            let sources = (0..source_count)
                .map(|source| u16::try_from(source).unwrap())
                .rev()
                .collect::<Vec<_>>();
            let source_count = u16::try_from(sources.len()).unwrap();
            let mut table = vec![1, 2, source_count << 1, 4];
            table.extend_from_slice(&sources);
            table.extend_from_slice(&[1, 8]);

            let index = GotoIndex::build(&table).unwrap();

            for source in sources {
                assert_eq!(index.get(source, 0, true), Some(4));
            }
            assert_eq!(index.get(source_count, 0, true), Some(8));
            assert!(
                !index
                    .exceptions
                    .windows(2)
                    .all(|entries| entries[0].source < entries[1].source)
            );
        }
    }

    #[test]
    fn retains_short_exception_row_order() {
        let sources = [3, 1, 2];
        let source_count = u16::try_from(sources.len()).unwrap();
        let mut table = vec![1, 2, source_count << 1, 4];
        table.extend_from_slice(&sources);
        table.extend_from_slice(&[1, 8]);

        let index = GotoIndex::build(&table).unwrap();
        let projected = index
            .exceptions
            .iter()
            .map(|entry| entry.source)
            .collect::<Vec<_>>();

        assert_eq!(projected, sources);
    }

    #[test]
    fn rejects_duplicate_sources_in_long_exception_rows() {
        let mut sources = (0..=MAX_LINEAR_EXCEPTION_COUNT)
            .map(|source| u16::try_from(source).unwrap())
            .collect::<Vec<_>>();
        sources[MAX_LINEAR_EXCEPTION_COUNT] = 0;
        let source_count = u16::try_from(sources.len()).unwrap();
        let mut table = vec![1, 2, source_count << 1, 4];
        table.extend_from_slice(&sources);
        table.extend_from_slice(&[1, 8]);

        assert_eq!(
            GotoIndex::build(&table).unwrap_err(),
            "goto projection has duplicate exception sources"
        );
    }
}
