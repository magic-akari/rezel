use std::collections::{BTreeMap, BTreeSet};

use crate::GeneratorError;
use crate::grammar::{TermId, TermSet};

const UTF16_LIMIT: i64 = 0x1_0000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Edge {
    pub from: i32,
    pub to: i32,
    pub target: usize,
}

#[derive(Clone, Debug, Default)]
pub struct NfaState {
    pub accepting: Vec<TermId>,
    pub edges: Vec<Edge>,
}

#[derive(Clone, Debug)]
pub struct TokenNfa {
    pub states: Vec<NfaState>,
    pub start: usize,
}

impl Default for TokenNfa {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenNfa {
    #[must_use]
    pub fn new() -> Self {
        Self {
            states: vec![NfaState::default()],
            start: 0,
        }
    }

    pub fn state(&mut self) -> usize {
        let id = self.states.len();
        self.states.push(NfaState::default());
        id
    }

    pub fn accepting_state(&mut self, term: TermId) -> usize {
        let state = self.state();
        self.states[state].accepting.push(term);
        state
    }

    pub fn edge(&mut self, from_state: usize, from: u32, to: u32, target: usize) {
        self.states[from_state].edges.push(Edge {
            from: i32::try_from(from).expect("UTF-16 bounds fit i32"),
            to: i32::try_from(to).expect("UTF-16 bounds fit i32"),
            target,
        });
    }

    pub fn epsilon(&mut self, from_state: usize, target: usize) {
        self.states[from_state].edges.push(Edge {
            from: -1,
            to: -1,
            target,
        });
    }

    #[must_use]
    pub fn compile(&self) -> TokenDfa {
        let start_set = self.closure(self.start);
        let mut sets = vec![start_set.clone()];
        let mut set_ids = BTreeMap::from([(start_set, 0_usize)]);
        let mut states = Vec::new();
        let mut cursor = 0;
        while cursor < sets.len() {
            let set = sets[cursor].clone();
            let mut accepting = Vec::new();
            let mut edges = Vec::new();
            for state in &set {
                for term in &self.states[*state].accepting {
                    push_unique(&mut accepting, *term);
                }
                edges.extend(
                    self.states[*state]
                        .edges
                        .iter()
                        .filter(|edge| edge.from >= 0)
                        .copied(),
                );
            }
            let mut transitions = Vec::new();
            for merged in self.merge_edges(&edges) {
                let mut targets = merged.targets;
                targets.sort_unstable();
                targets.dedup();
                let target = if let Some(target) = set_ids.get(&targets) {
                    *target
                } else {
                    let target = sets.len();
                    sets.push(targets.clone());
                    set_ids.insert(targets, target);
                    target
                };
                transitions.push(DfaEdge {
                    from: merged.from,
                    to: merged.to,
                    target,
                });
            }
            accepting.sort_unstable();
            states.push(DfaState {
                accepting,
                edges: transitions,
            });
            cursor += 1;
        }
        minimize(TokenDfa { states, start: 0 })
    }

    fn closure(&self, start: usize) -> Vec<usize> {
        let mut seen = BTreeSet::new();
        let mut result = Vec::new();
        let mut stack = vec![start];
        while let Some(state) = stack.pop() {
            if !seen.insert(state) {
                continue;
            }
            let current = &self.states[state];
            let has_labeled = current.edges.iter().any(|edge| edge.from >= 0);
            let uniquely_accepting = !current.accepting.is_empty()
                && !current.edges.iter().any(|edge| {
                    edge.from < 0
                        && same_set(&current.accepting, &self.states[edge.target].accepting)
                });
            if has_labeled || uniquely_accepting {
                result.push(state);
            }
            for edge in current.edges.iter().filter(|edge| edge.from < 0) {
                stack.push(edge.target);
            }
        }
        result.sort_unstable();
        result
    }

    fn merge_edges(&self, edges: &[Edge]) -> Vec<MergedEdge> {
        let mut boundaries = Vec::new();
        for edge in edges {
            if edge.from >= 0 && edge.from != i32::from(u16::MAX) {
                push_unique(&mut boundaries, edge.from);
                push_unique(&mut boundaries, edge.to);
            }
        }
        boundaries.sort_unstable();
        let mut result = Vec::new();
        for pair in boundaries.windows(2) {
            let from = pair[0];
            let to = pair[1];
            let mut targets = Vec::new();
            for edge in edges {
                if edge.to > from && edge.from < to {
                    for target in self.closure(edge.target) {
                        push_unique(&mut targets, target);
                    }
                }
            }
            if !targets.is_empty() {
                result.push(MergedEdge { from, to, targets });
            }
        }
        let eof_edges = edges
            .iter()
            .filter(|edge| edge.from == i32::from(u16::MAX) && edge.to == i32::from(u16::MAX));
        let mut eof_targets = Vec::new();
        for edge in eof_edges {
            for target in self.closure(edge.target) {
                push_unique(&mut eof_targets, target);
            }
        }
        if !eof_targets.is_empty() {
            result.push(MergedEdge {
                from: i32::from(u16::MAX),
                to: i32::from(u16::MAX),
                targets: eof_targets,
            });
        }
        result
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MergedEdge {
    from: i32,
    to: i32,
    targets: Vec<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DfaEdge {
    pub from: i32,
    pub to: i32,
    pub target: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DfaState {
    pub accepting: Vec<TermId>,
    pub edges: Vec<DfaEdge>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenDfa {
    pub states: Vec<DfaState>,
    pub start: usize,
}

impl TokenDfa {
    /// Encode the minimized token automaton in the runtime's compact format.
    ///
    /// # Errors
    ///
    /// Returns an error when offsets exceed the 16-bit runtime encoding.
    pub fn to_array(
        &self,
        terms: &TermSet,
        group_masks: &BTreeMap<u16, u16>,
        precedence: &[u16],
    ) -> Result<Vec<u16>, GeneratorError> {
        let order = self.reachable_order();
        let mut offsets = vec![0_usize; self.states.len()];
        let mut data = Vec::<i64>::new();
        for state_id in order {
            let state = &self.states[state_id];
            let start = data.len();
            let accept_end = start + 3 + state.accepting.len() * 2;
            offsets[state_id] = start;
            data.push(i64::from(self.state_mask(state_id, terms, group_masks)));
            data.push(
                i64::try_from(accept_end)
                    .map_err(|_| GeneratorError::new("Tokenizer tables are too large", None))?,
            );
            data.push(
                i64::try_from(state.edges.len())
                    .map_err(|_| GeneratorError::new("Tokenizer tables are too large", None))?,
            );

            let mut accepting = state.accepting.clone();
            accepting.sort_by_key(|term| {
                let id = terms.output_id(*term);
                precedence
                    .iter()
                    .position(|candidate| *candidate == id)
                    .unwrap_or(usize::MAX)
            });
            for term in accepting {
                let id = terms.output_id(term);
                data.push(i64::from(id));
                data.push(i64::from(group_masks.get(&id).copied().unwrap_or(u16::MAX)));
            }
            for edge in &state.edges {
                data.push(i64::from(edge.from));
                data.push(i64::from(edge.to));
                let marker = -i64::try_from(edge.target)
                    .map_err(|_| GeneratorError::new("Tokenizer tables are too large", None))?
                    - 1;
                data.push(marker);
            }
        }
        for value in &mut data {
            if *value < 0 {
                let state = usize::try_from(-*value - 1)
                    .expect("negative transition markers encode state ids");
                *value = i64::try_from(offsets[state])
                    .map_err(|_| GeneratorError::new("Tokenizer tables are too large", None))?;
            }
        }
        if data.len() > usize::from(u16::MAX) {
            return Err(GeneratorError::new(
                "Tokenizer tables too big to represent with 16-bit offsets.",
                None,
            ));
        }
        data.into_iter().map(encode_token_table_value).collect()
    }

    #[must_use]
    pub fn find_conflicts(
        &self,
        occur_together: impl Fn(TermId, TermId) -> bool,
    ) -> Vec<TokenConflict> {
        let cycle_terms = self.cycle_terms();
        let mut conflicts = Vec::new();
        let order = self.reachable_order();
        for state_id in order {
            let state = &self.states[state_id];
            for left in 0..state.accepting.len() {
                for right in (left + 1)..state.accepting.len() {
                    add_conflict(
                        &mut conflicts,
                        state.accepting[left],
                        state.accepting[right],
                        0,
                    );
                }
            }
            let descendants = self.reachable_from(state_id);
            for descendant in descendants {
                if descendant == state_id {
                    continue;
                }
                for term in &self.states[descendant].accepting {
                    let has_cycle = cycle_terms.contains(term);
                    for original in &state.accepting {
                        if term == original {
                            continue;
                        }
                        let hard = !has_cycle
                            && !cycle_terms.contains(original)
                            && occur_together(*term, *original);
                        let soft = i8::from(hard);
                        add_conflict(&mut conflicts, *term, *original, soft);
                    }
                }
            }
        }
        conflicts
    }

    fn reachable_order(&self) -> Vec<usize> {
        self.reachable_from(self.start)
    }

    fn reachable_from(&self, start: usize) -> Vec<usize> {
        let mut result = Vec::new();
        let mut seen = BTreeSet::new();
        let mut stack = vec![start];
        while let Some(state) = stack.pop() {
            if !seen.insert(state) {
                continue;
            }
            result.push(state);
            for edge in self.states[state].edges.iter().rev() {
                stack.push(edge.target);
            }
        }
        result
    }

    fn cycle_terms(&self) -> BTreeSet<TermId> {
        let mut closure = vec![BTreeSet::new(); self.states.len()];
        for (state_id, state) in self.states.iter().enumerate() {
            for edge in &state.edges {
                closure[state_id].insert(edge.target);
            }
        }
        loop {
            let snapshot = closure.clone();
            let mut changed = false;
            for state_id in 0..closure.len() {
                let descendants = snapshot[state_id].clone();
                for descendant in descendants {
                    for target in &snapshot[descendant] {
                        changed |= closure[state_id].insert(*target);
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let mut result = BTreeSet::new();
        for (state_id, descendants) in closure.iter().enumerate() {
            if descendants.contains(&state_id) {
                result.extend(self.states[state_id].accepting.iter().copied());
            }
        }
        result
    }

    fn state_mask(
        &self,
        state_id: usize,
        terms: &TermSet,
        group_masks: &BTreeMap<u16, u16>,
    ) -> u16 {
        let mut mask = 0;
        for reachable in self.reachable_from(state_id) {
            for term in &self.states[reachable].accepting {
                let id = terms.output_id(*term);
                mask |= group_masks.get(&id).copied().unwrap_or(u16::MAX);
            }
        }
        mask
    }
}

fn encode_token_table_value(value: i64) -> Result<u16, GeneratorError> {
    if value == UTF16_LIMIT {
        return Ok(0);
    }
    u16::try_from(value)
        .map_err(|_| GeneratorError::new("Tokenizer table value exceeds 16 bits", None))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TokenConflict {
    pub left: TermId,
    pub right: TermId,
    pub soft: i8,
}

fn add_conflict(
    conflicts: &mut Vec<TokenConflict>,
    mut left: TermId,
    mut right: TermId,
    mut soft: i8,
) {
    if left < right {
        std::mem::swap(&mut left, &mut right);
        soft = -soft;
    }
    if let Some(found) = conflicts
        .iter_mut()
        .find(|conflict| conflict.left == left && conflict.right == right)
    {
        if found.soft != soft {
            found.soft = 0;
        }
    } else {
        conflicts.push(TokenConflict { left, right, soft });
    }
}

fn minimize(mut dfa: TokenDfa) -> TokenDfa {
    let mut partition = accepting_partition(&dfa.states);
    loop {
        let mut next = vec![usize::MAX; dfa.states.len()];
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for state_id in 0..dfa.states.len() {
            if next[state_id] != usize::MAX {
                continue;
            }
            let original_group = partition[state_id];
            let candidates = partition
                .iter()
                .enumerate()
                .filter_map(|(candidate, group)| (*group == original_group).then_some(candidate));
            for candidate in candidates {
                let target_group = groups
                    .iter()
                    .position(|group| {
                        partition[group[0]] == original_group
                            && equivalent_dfa_states(
                                &dfa.states[candidate],
                                &dfa.states[group[0]],
                                &partition,
                            )
                    })
                    .unwrap_or_else(|| {
                        groups.push(Vec::new());
                        groups.len() - 1
                    });
                groups[target_group].push(candidate);
                next[candidate] = target_group;
            }
        }
        if next == partition {
            break;
        }
        partition = next;
    }

    let group_count = partition.iter().copied().max().map_or(0, |value| value + 1);
    let mut states = Vec::with_capacity(group_count);
    for group in 0..group_count {
        let source = partition
            .iter()
            .position(|candidate| *candidate == group)
            .expect("every partition group has a member");
        let mut state = dfa.states[source].clone();
        for edge in &mut state.edges {
            edge.target = partition[edge.target];
        }
        states.push(state);
    }
    dfa.start = partition[dfa.start];
    dfa.states = states;
    dfa
}

fn accepting_partition(states: &[DfaState]) -> Vec<usize> {
    let mut groups: Vec<Vec<TermId>> = Vec::new();
    states
        .iter()
        .map(|state| {
            groups
                .iter()
                .position(|accepting| accepting == &state.accepting)
                .unwrap_or_else(|| {
                    groups.push(state.accepting.clone());
                    groups.len() - 1
                })
        })
        .collect()
}

fn equivalent_dfa_states(left: &DfaState, right: &DfaState, partition: &[usize]) -> bool {
    left.edges.len() == right.edges.len()
        && left.edges.iter().zip(&right.edges).all(|(left, right)| {
            left.from == right.from
                && left.to == right.to
                && partition[left.target] == partition[right.target]
        })
}

fn same_set<T>(left: &[T], right: &[T]) -> bool
where
    T: Eq,
{
    left == right
}

fn push_unique<T>(values: &mut Vec<T>, value: T)
where
    T: Eq,
{
    if !values.contains(&value) {
        values.push(value);
    }
}
