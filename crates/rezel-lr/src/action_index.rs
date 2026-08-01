use crate::table::{Action, ReservedTerm, SequenceCode, StateField, StateFlag};

const NO_ROW: u16 = u16::MAX;
const NO_ENTRY: u16 = u16::MAX;
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

#[derive(Clone, Copy, Debug)]
struct ErrorEntry {
    action: Action,
    order: u16,
}

#[derive(Debug)]
struct ErrorIndex {
    rows: Box<[ErrorEntry]>,
    entry_orders: Box<[u16]>,
}

#[derive(Debug)]
struct DecodedRow {
    entries: Vec<(u16, Action)>,
    next: Option<usize>,
    fallback: Action,
}

#[derive(Clone, Copy, Debug)]
struct StateActions {
    rows: [u16; 2],
    default_reduce: Action,
}

/// Parser-private terminal-action projection over compact generated tables.
#[derive(Debug)]
pub(crate) struct ActionIndex {
    states: Box<[StateActions]>,
    skipped_states: Box<[bool]>,
    tokenizer_masks: Box<[u32]>,
    rows: Box<[ActionRow]>,
    terms: Box<[u16]>,
    actions: Box<[Action]>,
    errors: Option<Box<ErrorIndex>>,
    skip_terms: Box<[u64]>,
    skip_has_catch_all: bool,
}

impl ActionIndex {
    pub(crate) fn build(states: &[u32], state_data: &[u16]) -> Result<Self, &'static str> {
        let state_count = states.len() / StateField::COUNT;
        let mut roots = Vec::with_capacity(state_count);
        let mut skipped_states = Vec::with_capacity(state_count);
        let mut tokenizer_masks = Vec::with_capacity(state_count);
        let mut offsets = Vec::new();
        let mut queued = vec![false; state_data.len()];
        for state in 0..state_count {
            let base = state * StateField::COUNT;
            let actions = states[base + StateField::Actions.index()] as usize;
            let skip = states[base + StateField::Skip.index()] as usize;
            let default_reduce = Action::from_raw(states[base + StateField::DefaultReduce.index()]);
            roots.push(([actions, skip], default_reduce));
            skipped_states
                .push(states[base + StateField::Flags.index()] & StateFlag::Skipped.mask() != 0);
            tokenizer_masks.push(states[base + StateField::TokenizerMask.index()]);
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
        let mut entry_orders = Vec::new();
        let mut error_rows = Vec::with_capacity(decoded_rows.len());
        for (_, decoded) in decoded_rows {
            let length = u16::try_from(decoded.entries.len())
                .map_err(|_| "action sequence has too many entries")?;
            let term_filter = build_term_filter(&decoded.entries);
            let mut indexed_entries = decoded.entries.into_iter().enumerate().collect::<Vec<_>>();
            let error = find_error_entry(&indexed_entries);
            indexed_entries.sort_by_key(|(_, (term, _))| *term);
            let start = u32::try_from(terms.len()).map_err(|_| "action projection is too large")?;
            for (order, (term, action)) in indexed_entries {
                terms.push(term);
                actions.push(action);
                entry_orders.push(u16::try_from(order).expect("entry count was validated"));
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
            error_rows.push(error);
        }
        reject_cycles(&rows)?;
        let states = roots
            .into_iter()
            .map(|(roots, default_reduce)| {
                Ok(StateActions {
                    rows: [row_id(&offsets, roots[0])?, row_id(&offsets, roots[1])?],
                    default_reduce,
                })
            })
            .collect::<Result<Box<[_]>, &'static str>>()?;
        let errors = build_error_index(error_rows, entry_orders);
        let (skip_terms, skip_has_catch_all) =
            build_skip_filter(&states, &rows, &terms, errors.as_deref());

        Ok(Self {
            states,
            skipped_states: skipped_states.into_boxed_slice(),
            tokenizer_masks: tokenizer_masks.into_boxed_slice(),
            rows: rows.into_boxed_slice(),
            terms: terms.into_boxed_slice(),
            actions: actions.into_boxed_slice(),
            errors,
            skip_terms,
            skip_has_catch_all,
        })
    }

    pub(crate) fn skip_may_match(&self, term: u16) -> bool {
        if self.skip_has_catch_all {
            return true;
        }
        let term = usize::from(term);
        let word = term / u64::BITS as usize;
        let bit = term % u64::BITS as usize;
        self.skip_terms
            .get(word)
            .is_some_and(|terms| terms & (1_u64 << bit) != 0)
    }

    pub(crate) fn first_action(&self, state: u16, term: u16) -> Action {
        let action = self.first(state, StateField::Actions, term);
        if !action.is_none() || !self.skip_may_match(term) {
            return action;
        }
        self.first(state, StateField::Skip, term)
    }

    pub(crate) fn first(&self, state: u16, field: StateField, term: u16) -> Action {
        let column = match field {
            StateField::Actions => 0,
            StateField::Skip => 1,
            _ => unreachable!("only action sequence fields are indexed"),
        };
        let row_id = self.states[usize::from(state)].rows[column];
        match self.errors.as_deref() {
            Some(errors) => self.first_with_errors(row_id, term, errors),
            None => self.first_without_errors(row_id, term),
        }
    }

    fn first_without_errors(&self, mut row_id: u16, term: u16) -> Action {
        loop {
            let row = self.rows[usize::from(row_id)];
            let terminal = self.first_terminal_index(row, term);
            if let Some(index) = terminal {
                return self.actions[index];
            }
            if row.next == NO_ROW {
                return row.fallback;
            }
            row_id = row.next;
        }
    }

    fn first_with_errors(&self, mut row_id: u16, term: u16, errors: &ErrorIndex) -> Action {
        loop {
            let row_index = usize::from(row_id);
            let row = self.rows[row_index];
            let error = errors.rows[row_index];
            let terminal = self.first_terminal_index(row, term);
            if error.order != NO_ENTRY
                && terminal.is_none_or(|index| error.order <= errors.entry_orders[index])
            {
                return error.action;
            }
            if let Some(index) = terminal {
                return self.actions[index];
            }
            if row.next == NO_ROW {
                return row.fallback;
            }
            row_id = row.next;
        }
    }

    fn first_terminal_index(&self, row: ActionRow, term: u16) -> Option<usize> {
        if !row.may_contain(term) {
            return None;
        }
        let range = row.range();
        let start = range.start;
        let terms = &self.terms[range];
        if terms.len() <= LINEAR_SEARCH_LIMIT {
            for (index, candidate) in terms.iter().copied().enumerate() {
                if candidate < term {
                    continue;
                }
                if candidate > term {
                    return None;
                }
                return Some(start + index);
            }
            return None;
        }
        let index = terms.partition_point(|candidate| *candidate < term);
        (terms.get(index) == Some(&term)).then_some(start + index)
    }

    #[cfg(test)]
    pub(crate) fn visit(
        &self,
        state: u16,
        field: StateField,
        term: u16,
        visit: impl FnMut(Action),
    ) -> Option<Action> {
        let column = match field {
            StateField::Actions => 0,
            StateField::Skip => 1,
            _ => unreachable!("only action sequence fields are indexed"),
        };
        let row_id = self.states[usize::from(state)].rows[column];
        self.visit_row(row_id, term, visit)
    }

    pub(crate) fn state_rows(&self, state: u16) -> [u16; 2] {
        self.states[usize::from(state)].rows
    }

    pub(crate) fn default_reduce(&self, state: u16) -> Action {
        self.states[usize::from(state)].default_reduce
    }

    pub(crate) fn state_is_skipped(&self, state: u16) -> bool {
        self.skipped_states[usize::from(state)]
    }

    pub(crate) fn tokenizer_mask(&self, state: u16) -> u32 {
        self.tokenizer_masks[usize::from(state)]
    }

    pub(crate) fn visit_row(
        &self,
        mut row_id: u16,
        term: u16,
        mut visit: impl FnMut(Action),
    ) -> Option<Action> {
        loop {
            let row = self.rows[usize::from(row_id)];
            if row.may_contain(term) {
                let range = row.range();
                let start = range.start;
                let terms = &self.terms[range];
                if terms.len() <= LINEAR_SEARCH_LIMIT {
                    for (index, candidate) in terms.iter().copied().enumerate() {
                        if candidate < term {
                            continue;
                        }
                        if candidate > term {
                            break;
                        }
                        visit(self.actions[start + index]);
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

fn find_error_entry(indexed_entries: &[(usize, (u16, Action))]) -> ErrorEntry {
    indexed_entries
        .iter()
        .find(|(_, (term, _))| *term == ReservedTerm::Error.raw())
        .map_or(
            ErrorEntry {
                action: Action::NONE,
                order: NO_ENTRY,
            },
            |(order, (_, action))| ErrorEntry {
                action: *action,
                order: u16::try_from(*order).expect("entry count was validated"),
            },
        )
}

fn build_error_index(rows: Vec<ErrorEntry>, entry_orders: Vec<u16>) -> Option<Box<ErrorIndex>> {
    rows.iter().any(|error| error.order != NO_ENTRY).then(|| {
        Box::new(ErrorIndex {
            rows: rows.into_boxed_slice(),
            entry_orders: entry_orders.into_boxed_slice(),
        })
    })
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
    states: &[StateActions],
    rows: &[ActionRow],
    terms: &[u16],
    errors: Option<&ErrorIndex>,
) -> (Box<[u64]>, bool) {
    let mut visited = vec![false; rows.len()];
    let mut skip_terms = Vec::new();
    let mut skip_has_catch_all = false;
    for state in states {
        let mut row_id = usize::from(state.rows[1]);
        while !visited[row_id] {
            visited[row_id] = true;
            let row = rows[row_id];
            skip_terms.extend_from_slice(&terms[row.range()]);
            skip_has_catch_all |= errors
                .is_some_and(|errors| errors.rows[row_id].order != NO_ENTRY)
                || !row.fallback.is_none();
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
    (filter.into_boxed_slice(), skip_has_catch_all)
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
    fn state_projection_stays_compact_and_preserves_default_reductions() {
        let reduction = Action::reduce(7, 2, false, false);
        let states = [
            StateFlag::Skipped.mask(),
            0,
            0,
            0b0101,
            Action::NONE.raw(),
            0,
            0,
            0,
            0,
            0b1010,
            reduction.raw(),
            0,
        ];
        let data = [END, SequenceCode::Done.raw()];
        let index = ActionIndex::build(&states, &data).unwrap();

        assert_eq!(core::mem::size_of::<StateActions>(), 8);
        assert_eq!(core::mem::size_of::<ActionRow>(), 16);
        assert_eq!(core::mem::size_of::<ErrorEntry>(), 8);
        assert!(index.errors.is_none());
        assert_eq!(index.default_reduce(0), Action::NONE);
        assert_eq!(index.default_reduce(1), reduction);
        assert!(index.state_is_skipped(0));
        assert!(!index.state_is_skipped(1));
        assert_eq!(index.tokenizer_mask(0), 0b0101);
        assert_eq!(index.tokenizer_mask(1), 0b1010);
    }

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
    fn first_preserves_terminal_error_and_continuation_order() {
        let states = [0, 0, 0, 0, 0, 0];
        let error = ReservedTerm::Error.raw();
        let data = [
            3, 10, 0, error, 11, 0, 4, 12, 0, END, NEXT, 13, 0, 5, 14, 0, END, OTHER, 15, 0,
        ];
        let index = ActionIndex::build(&states, &data).unwrap();

        assert!(index.errors.is_some());
        assert_eq!(index.first(0, StateField::Actions, 3).raw(), 10);
        assert_eq!(index.first(0, StateField::Actions, 4).raw(), 11);
        assert_eq!(index.first(0, StateField::Actions, 5).raw(), 11);
        assert_eq!(index.first(0, StateField::Actions, 99).raw(), 11);
    }

    #[test]
    fn first_reaches_continuations_and_terminal_fallbacks() {
        let states = [0, 0, 0, 0, 0, 0];
        let data = [END, NEXT, 4, 0, 5, 14, 0, END, OTHER, 15, 0];
        let index = ActionIndex::build(&states, &data).unwrap();

        assert_eq!(index.first(0, StateField::Actions, 5).raw(), 14);
        assert_eq!(index.first(0, StateField::Actions, 99).raw(), 15);
    }

    #[test]
    fn first_preserves_source_order_on_short_rows() {
        let states = [0, 0, 0, 0, 0, 0];
        let data = [
            7,
            107,
            0,
            2,
            102,
            0,
            5,
            105,
            0,
            5,
            205,
            0,
            6,
            106,
            0,
            END,
            SequenceCode::Done.raw(),
        ];
        let index = ActionIndex::build(&states, &data).unwrap();

        assert_eq!(index.first(0, StateField::Actions, 5).raw(), 105);
        assert_eq!(index.first(0, StateField::Actions, 6).raw(), 106);
        assert!(index.first(0, StateField::Actions, 4).is_none());
    }

    #[test]
    fn first_preserves_source_order_on_binary_search_rows() {
        let states = [0, 0, 0, 0, 0, 0];
        let error = ReservedTerm::Error.raw();
        let data = [
            9,
            109,
            0,
            1,
            101,
            0,
            5,
            105,
            0,
            2,
            102,
            0,
            8,
            108,
            0,
            3,
            103,
            0,
            7,
            107,
            0,
            error,
            190,
            0,
            4,
            104,
            0,
            6,
            106,
            0,
            5,
            205,
            0,
            END,
            SequenceCode::Done.raw(),
        ];
        let index = ActionIndex::build(&states, &data).unwrap();

        assert_eq!(index.first(0, StateField::Actions, 5).raw(), 105);
        assert_eq!(index.first(0, StateField::Actions, 4).raw(), 190);
        assert_eq!(index.first(0, StateField::Actions, 99).raw(), 190);
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
    fn skip_filter_preserves_error_and_other_catch_alls() {
        let states = [0, 0, 2, 0, 0, 0];
        let error = ReservedTerm::Error.raw();
        let data = [
            END,
            SequenceCode::Done.raw(),
            error,
            10,
            0,
            END,
            SequenceCode::Done.raw(),
        ];
        let index = ActionIndex::build(&states, &data).unwrap();

        assert!(index.skip_may_match(5));
        assert_eq!(index.first(0, StateField::Skip, 5).raw(), 10);

        let data = [END, SequenceCode::Done.raw(), END, OTHER, 11, 0];
        let index = ActionIndex::build(&states, &data).unwrap();

        assert!(index.skip_may_match(5));
        assert_eq!(index.first(0, StateField::Skip, 5).raw(), 11);
    }

    #[test]
    fn skip_filter_never_hides_a_possible_action() {
        let error = ReservedTerm::Error.raw();
        let cases = [
            vec![
                END,
                SequenceCode::Done.raw(),
                5,
                10,
                0,
                END,
                SequenceCode::Done.raw(),
            ],
            vec![
                END,
                SequenceCode::Done.raw(),
                END,
                NEXT,
                6,
                0,
                error,
                10,
                0,
                END,
                SequenceCode::Done.raw(),
            ],
            vec![
                END,
                SequenceCode::Done.raw(),
                END,
                NEXT,
                6,
                0,
                END,
                OTHER,
                11,
                0,
            ],
        ];
        for data in cases {
            let states = [0, 2, 0, 0, 0, 0];
            let index = ActionIndex::build(&states, &data).unwrap();
            for term in 0..=u16::MAX {
                let action = index.first(0, StateField::Skip, term);
                assert!(action.is_none() || index.skip_may_match(term));
            }
        }
    }

    #[test]
    fn first_action_preserves_actions_before_skip() {
        let states = [0, 0, 5, 0, 0, 0];
        let data = [
            5,
            10,
            0,
            END,
            SequenceCode::Done.raw(),
            5,
            20,
            0,
            6,
            21,
            0,
            END,
            SequenceCode::Done.raw(),
        ];
        let index = ActionIndex::build(&states, &data).unwrap();

        assert_eq!(index.first_action(0, 5).raw(), 10);
        assert_eq!(index.first_action(0, 6).raw(), 21);
        assert!(index.first_action(0, 7).is_none());

        let error = ReservedTerm::Error.raw();
        let data = [
            error,
            12,
            0,
            END,
            SequenceCode::Done.raw(),
            6,
            21,
            0,
            END,
            SequenceCode::Done.raw(),
        ];
        let index = ActionIndex::build(&states, &data).unwrap();

        assert_eq!(index.first_action(0, 6).raw(), 12);
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

            actions.clear();
            let fallback = index.visit(0, StateField::Skip, term, |action| {
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

        let fallback = index.visit(0, StateField::Skip, 34, |action| {
            actions.push(action.raw());
        });
        assert!(actions.is_empty());
        assert_eq!(fallback.map(Action::raw), Some(200));
    }

    #[test]
    fn short_rows_preserve_negative_filters_and_collision_fallbacks() {
        let states = [0, 0, 0, 0, 0, 0];
        let data = [3, 103, 0, 4, 104, 0, END, OTHER, 200, 0];
        let index = ActionIndex::build(&states, &data).unwrap();
        let [action_row, _] = index.state_rows(0);
        let row = index.rows[usize::from(action_row)];

        assert!(usize::from(row.length) <= LINEAR_SEARCH_LIMIT);
        assert!(row.may_contain(3));
        assert_eq!(term_filter_bit(3), term_filter_bit(34));
        assert!(row.may_contain(34));
        assert!(!row.may_contain(5));

        let mut actions = Vec::new();
        let fallback = index.visit_row(action_row, 3, |action| actions.push(action.raw()));
        assert_eq!(actions, [103]);
        assert_eq!(fallback.map(Action::raw), Some(200));

        actions.clear();
        let fallback = index.visit_row(action_row, 34, |action| actions.push(action.raw()));
        assert!(actions.is_empty());
        assert_eq!(fallback.map(Action::raw), Some(200));

        let fallback = index.visit_row(action_row, 5, |action| actions.push(action.raw()));
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
