use crate::table::{Action, SequenceCode, StateField};

const NO_ROW: u16 = u16::MAX;
const LINEAR_SEARCH_LIMIT: usize = 8;

#[derive(Clone, Copy, Debug)]
struct ActionRow {
    start: u32,
    term_filter: u32,
    fallback: Action,
    length: u16,
    next: u16,
}

impl ActionRow {
    fn range(self) -> core::ops::Range<usize> {
        let start = self.start as usize;
        start..start + usize::from(self.length)
    }

    fn may_contain(self, term: u16) -> bool {
        self.term_filter & term_filter_bit(term) != 0
    }
}

#[derive(Debug)]
struct DecodedRow {
    entries: Vec<(u16, Action)>,
    next: Option<usize>,
    fallback: Action,
}

/// Parser-private terminal-action projection over compact generated tables.
#[derive(Debug)]
pub(crate) struct ActionIndex {
    state_rows: Box<[[u16; 2]]>,
    rows: Box<[ActionRow]>,
    terms: Box<[u16]>,
    actions: Box<[Action]>,
    skip_terms: Box<[u64]>,
    skip_has_fallback: bool,
}

impl ActionIndex {
    pub(crate) fn build(states: &[u32], state_data: &[u16]) -> Result<Self, &'static str> {
        let state_count = states.len() / StateField::COUNT;
        let mut roots = Vec::with_capacity(state_count);
        let mut offsets = Vec::new();
        let mut queued = vec![false; state_data.len()];
        for state in 0..state_count {
            let base = state * StateField::COUNT;
            let actions = states[base + StateField::Actions.index()] as usize;
            let skip = states[base + StateField::Skip.index()] as usize;
            roots.push([actions, skip]);
            queue_offset(&mut offsets, &mut queued, actions)?;
            queue_offset(&mut offsets, &mut queued, skip)?;
        }

        let mut decoded_rows = Vec::new();
        let mut index = 0;
        while index < offsets.len() {
            let offset = offsets[index];
            let decoded = decode_row(state_data, offset)?;
            if let Some(next) = decoded.next {
                queue_offset(&mut offsets, &mut queued, next)?;
            }
            decoded_rows.push((offset, decoded));
            index += 1;
        }
        decoded_rows.sort_unstable_by_key(|(offset, _)| *offset);
        if decoded_rows.len() > usize::from(NO_ROW) {
            return Err("state data contains too many action sequences");
        }
        offsets.clear();
        offsets.extend(decoded_rows.iter().map(|(offset, _)| *offset));

        let mut rows = Vec::with_capacity(decoded_rows.len());
        let mut terms = Vec::new();
        let mut actions = Vec::new();
        for (_, mut decoded) in decoded_rows {
            decoded.entries.sort_by_key(|(term, _)| *term);
            let length = u16::try_from(decoded.entries.len())
                .map_err(|_| "action sequence has too many entries")?;
            let term_filter = build_term_filter(&decoded.entries);
            let start = u32::try_from(terms.len()).map_err(|_| "action projection is too large")?;
            for (term, action) in decoded.entries {
                terms.push(term);
                actions.push(action);
            }
            let end = start
                .checked_add(u32::from(length))
                .ok_or("action projection is too large")?;
            debug_assert_eq!(end as usize, terms.len());
            let next = decoded
                .next
                .map_or(Ok(NO_ROW), |next| row_id(&offsets, next))?;
            rows.push(ActionRow {
                start,
                term_filter,
                fallback: decoded.fallback,
                length,
                next,
            });
        }
        reject_cycles(&rows)?;
        let state_rows = roots
            .into_iter()
            .map(|roots| Ok([row_id(&offsets, roots[0])?, row_id(&offsets, roots[1])?]))
            .collect::<Result<Box<[_]>, &'static str>>()?;
        let (skip_terms, skip_has_fallback) = build_skip_filter(&state_rows, &rows, &terms);

        Ok(Self {
            state_rows,
            rows: rows.into_boxed_slice(),
            terms: terms.into_boxed_slice(),
            actions: actions.into_boxed_slice(),
            skip_terms,
            skip_has_fallback,
        })
    }

    pub(crate) fn skip_may_match(&self, term: u16) -> bool {
        if self.skip_has_fallback {
            return true;
        }
        let term = usize::from(term);
        let word = term / u64::BITS as usize;
        let bit = term % u64::BITS as usize;
        self.skip_terms
            .get(word)
            .is_some_and(|terms| terms & (1_u64 << bit) != 0)
    }

    pub(crate) fn visit(
        &self,
        state: u16,
        field: StateField,
        term: u16,
        mut visit: impl FnMut(Action),
    ) -> Option<Action> {
        let column = match field {
            StateField::Actions => 0,
            StateField::Skip => 1,
            _ => unreachable!("only action sequence fields are indexed"),
        };
        let filter_rows = field == StateField::Actions;
        let mut row_id = self.state_rows[usize::from(state)][column];
        loop {
            let row = self.rows[usize::from(row_id)];
            let filter_row = filter_rows && usize::from(row.length) > LINEAR_SEARCH_LIMIT;
            if !filter_row || row.may_contain(term) {
                let range = row.range();
                let start = range.start;
                let terms = &self.terms[range];
                if terms.len() <= LINEAR_SEARCH_LIMIT {
                    for (index, candidate) in terms.iter().copied().enumerate() {
                        if candidate == term {
                            visit(self.actions[start + index]);
                        }
                    }
                } else {
                    let mut index = terms.partition_point(|candidate| *candidate < term);
                    while terms.get(index) == Some(&term) {
                        visit(self.actions[start + index]);
                        index += 1;
                    }
                }
            }
            if row.next == NO_ROW {
                return (!row.fallback.is_none()).then_some(row.fallback);
            }
            row_id = row.next;
        }
    }
}

fn build_term_filter(entries: &[(u16, Action)]) -> u32 {
    let mut filter = 0;
    for (term, _) in entries {
        filter |= term_filter_bit(*term);
    }
    filter
}

const fn term_filter_bit(term: u16) -> u32 {
    let folded = term ^ (term >> 5) ^ (term >> 10);
    1_u32 << (folded & 31)
}

fn build_skip_filter(
    state_rows: &[[u16; 2]],
    rows: &[ActionRow],
    terms: &[u16],
) -> (Box<[u64]>, bool) {
    let mut visited = vec![false; rows.len()];
    let mut skip_terms = Vec::new();
    let mut skip_has_fallback = false;
    for state in state_rows {
        let mut row_id = usize::from(state[1]);
        while !visited[row_id] {
            visited[row_id] = true;
            let row = rows[row_id];
            skip_terms.extend_from_slice(&terms[row.range()]);
            skip_has_fallback |= !row.fallback.is_none();
            if row.next == NO_ROW {
                break;
            }
            row_id = usize::from(row.next);
        }
    }
    let word_count = skip_terms
        .iter()
        .copied()
        .max()
        .map_or(0, |term| usize::from(term) / u64::BITS as usize + 1);
    let mut filter = vec![0_u64; word_count];
    for term in skip_terms {
        let term = usize::from(term);
        let word = term / u64::BITS as usize;
        let bit = term % u64::BITS as usize;
        filter[word] |= 1_u64 << bit;
    }
    (filter.into_boxed_slice(), skip_has_fallback)
}

fn decode_row(data: &[u16], offset: usize) -> Result<DecodedRow, &'static str> {
    let mut entries = Vec::new();
    let mut index = offset;
    loop {
        let term = *data
            .get(index)
            .ok_or("action sequence is outside state data")?;
        if term == SequenceCode::End.raw() {
            let code = *data
                .get(index + 1)
                .ok_or("action sequence terminator is truncated")?;
            return match code {
                value if value == SequenceCode::Done.raw() => Ok(DecodedRow {
                    entries,
                    next: None,
                    fallback: Action::NONE,
                }),
                value if value == SequenceCode::Next.raw() => Ok(DecodedRow {
                    entries,
                    next: Some(read_pair(data, index + 2)? as usize),
                    fallback: Action::NONE,
                }),
                value if value == SequenceCode::Other.raw() => {
                    let fallback = Action::from_raw(read_pair(data, index + 2)?);
                    if fallback.is_none() {
                        return Err("action sequence fallback cannot be empty");
                    }
                    Ok(DecodedRow {
                        entries,
                        next: None,
                        fallback,
                    })
                }
                _ => Err("action sequence has an unknown terminator"),
            };
        }
        let action = Action::from_raw(read_pair(data, index + 1)?);
        if action.is_none() {
            return Err("action sequence cannot contain an empty action");
        }
        entries.push((term, action));
        index += 3;
    }
}

fn read_pair(data: &[u16], offset: usize) -> Result<u32, &'static str> {
    let low = u32::from(*data.get(offset).ok_or("action pair is truncated")?);
    let high = u32::from(*data.get(offset + 1).ok_or("action pair is truncated")?);
    Ok(low | (high << 16))
}

fn queue_offset(
    offsets: &mut Vec<usize>,
    queued: &mut [bool],
    offset: usize,
) -> Result<(), &'static str> {
    let queued = queued
        .get_mut(offset)
        .ok_or("action sequence is outside state data")?;
    if !*queued {
        *queued = true;
        offsets.push(offset);
    }
    Ok(())
}

fn row_id(offsets: &[usize], offset: usize) -> Result<u16, &'static str> {
    let index = offsets
        .binary_search(&offset)
        .map_err(|_| "action sequence refers to an unknown continuation")?;
    u16::try_from(index).map_err(|_| "action sequence id does not fit in 16 bits")
}

fn reject_cycles(rows: &[ActionRow]) -> Result<(), &'static str> {
    let mut states = vec![0_u8; rows.len()];
    let mut path = Vec::new();
    for start in 0..rows.len() {
        let mut current = start;
        while states[current] == 0 {
            states[current] = 1;
            path.push(current);
            let next = rows[current].next;
            if next == NO_ROW {
                break;
            }
            current = usize::from(next);
        }
        if rows[current].next != NO_ROW && states[current] == 1 {
            return Err("action sequence continuations form a cycle");
        }
        for row in path.drain(..) {
            states[row] = 2;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const END: u16 = SequenceCode::End.raw();
    const NEXT: u16 = SequenceCode::Next.raw();
    const OTHER: u16 = SequenceCode::Other.raw();

    #[test]
    fn preserves_chained_actions_and_fallbacks() {
        let states = [0, 0, 0, 0, 0, 0];
        let data = [
            3, 10, 0, 3, 11, 0, END, NEXT, 10, 0, 3, 12, 0, 4, 13, 0, END, OTHER, 14, 0,
        ];
        let index = ActionIndex::build(&states, &data).unwrap();

        let mut actions = Vec::new();
        let fallback = index.visit(0, StateField::Actions, 3, |action| {
            actions.push(action.raw());
        });
        assert_eq!(actions, [10, 11, 12]);
        assert_eq!(fallback.map(Action::raw), Some(14));

        actions.clear();
        let fallback = index.visit(0, StateField::Actions, 9, |action| {
            actions.push(action.raw());
        });
        assert!(actions.is_empty());
        assert_eq!(fallback.map(Action::raw), Some(14));
        assert!(index.skip_may_match(9));
    }

    #[test]
    fn filters_terminals_that_cannot_enter_skip_actions() {
        let states = [0, 0, 2, 0, 0, 0];
        let data = [
            END,
            SequenceCode::Done.raw(),
            5,
            10,
            0,
            END,
            SequenceCode::Done.raw(),
        ];
        let index = ActionIndex::build(&states, &data).unwrap();

        assert!(index.skip_may_match(5));
        assert!(!index.skip_may_match(4));
        assert!(!index.skip_may_match(128));
    }

    #[test]
    fn long_rows_preserve_members_and_collision_fallbacks() {
        let states = [0, 0, 0, 0, 0, 0];
        let mut data = Vec::new();
        for term in 1..=9_u16 {
            data.extend_from_slice(&[term, term + 100, 0]);
        }
        data.extend_from_slice(&[END, OTHER, 200, 0]);
        let index = ActionIndex::build(&states, &data).unwrap();

        for term in 1..=9_u16 {
            let mut actions = Vec::new();
            let fallback = index.visit(0, StateField::Actions, term, |action| {
                actions.push(action.raw());
            });
            assert_eq!(actions, [u32::from(term + 100)]);
            assert_eq!(fallback.map(Action::raw), Some(200));
        }

        assert_eq!(term_filter_bit(3), term_filter_bit(34));
        let mut actions = Vec::new();
        let fallback = index.visit(0, StateField::Actions, 34, |action| {
            actions.push(action.raw());
        });
        assert!(actions.is_empty());
        assert_eq!(fallback.map(Action::raw), Some(200));
    }

    #[test]
    fn rejects_truncated_and_cyclic_sequences() {
        let states = [0, 0, 0, 0, 0, 0];
        assert_eq!(
            ActionIndex::build(&states, &[END, NEXT]).unwrap_err(),
            "action pair is truncated"
        );
        assert_eq!(
            ActionIndex::build(&states, &[END, NEXT, 0, 0]).unwrap_err(),
            "action sequence continuations form a cycle"
        );
    }
}
