const NO_TERM: u32 = u32::MAX;

#[derive(Clone, Copy, Debug)]
struct GotoEntry {
    source: u16,
    target: u16,
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
        if let Some(entry) = exceptions.iter().find(|entry| entry.source == state) {
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
}
