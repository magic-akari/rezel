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
        let term_count = usize::from(*table.first().ok_or("goto table cannot be empty")?);
        let header_length = term_count + 1;
        let mut terms = vec![GotoTerm::EMPTY; term_count];
        let mut exceptions = Vec::new();
        let mut default_sources = Vec::new();
        for (term, slot) in terms.iter_mut().enumerate() {
            let mut position = usize::from(
                *table
                    .get(term + 1)
                    .ok_or("goto table header is truncated")?,
            );
            if position < header_length {
                continue;
            }
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
                let end = position
                    .checked_add(usize::from(group_tag >> 1))
                    .ok_or("goto projection group length overflows")?;
                let sources = table
                    .get(position..end)
                    .ok_or("goto projection sources are truncated")?;
                if group_tag & 1 == 0 {
                    exceptions.extend(
                        sources
                            .iter()
                            .copied()
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
                default_sources.extend_from_slice(sources);
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
