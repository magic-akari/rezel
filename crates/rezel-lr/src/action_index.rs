use crate::table::{Action, SequenceCode, StateField};

const NO_ROW: u16 = u16::MAX;
const LINEAR_SEARCH_LIMIT: usize = 8;

#[derive(Clone, Copy, Debug)]
struct ActionRow {
    start: u32,
    end: u32,
    next: u16,
    fallback: Action,
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
            let start = u32::try_from(terms.len()).map_err(|_| "action projection is too large")?;
            for (term, action) in decoded.entries {
                terms.push(term);
                actions.push(action);
            }
            let end = u32::try_from(terms.len()).map_err(|_| "action projection is too large")?;
            let next = decoded
                .next
                .map_or(Ok(NO_ROW), |next| row_id(&offsets, next))?;
            rows.push(ActionRow {
                start,
                end,
                next,
                fallback: decoded.fallback,
            });
        }
        reject_cycles(&rows)?;
        let state_rows = roots
            .into_iter()
            .map(|roots| Ok([row_id(&offsets, roots[0])?, row_id(&offsets, roots[1])?]))
            .collect::<Result<Box<[_]>, &'static str>>()?;

        Ok(Self {
            state_rows,
            rows: rows.into_boxed_slice(),
            terms: terms.into_boxed_slice(),
            actions: actions.into_boxed_slice(),
        })
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
        let mut row_id = self.state_rows[usize::from(state)][column];
        loop {
            let row = self.rows[usize::from(row_id)];
            let start = row.start as usize;
            let end = row.end as usize;
            let terms = &self.terms[start..end];
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
            if row.next == NO_ROW {
                return (!row.fallback.is_none()).then_some(row.fallback);
            }
            row_id = row.next;
        }
    }
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
