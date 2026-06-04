use std::collections::{BTreeMap, BTreeSet};

use crate::GeneratorError;
use crate::grammar::{TermId, TermSet};

const CODE_POINT_LIMIT: u32 = 0x11_0000;
const NO_STATE: u16 = u16::MAX;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NfaEdge {
    CodePoint { from: u32, to: u32, target: usize },
    Eof { target: usize },
    Epsilon { target: usize },
}

impl NfaEdge {
    const fn target(self) -> usize {
        match self {
            Self::CodePoint { target, .. } | Self::Eof { target } | Self::Epsilon { target } => {
                target
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct NfaState {
    pub accepting: Vec<TermId>,
    edges: Vec<NfaEdge>,
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
        debug_assert!(from < to);
        debug_assert!(to <= CODE_POINT_LIMIT);
        self.states[from_state]
            .edges
            .push(NfaEdge::CodePoint { from, to, target });
    }

    pub fn eof(&mut self, from_state: usize, target: usize) {
        self.states[from_state].edges.push(NfaEdge::Eof { target });
    }

    pub fn epsilon(&mut self, from_state: usize, target: usize) {
        self.states[from_state]
            .edges
            .push(NfaEdge::Epsilon { target });
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
            let mut code_point_edges = Vec::new();
            let mut eof_targets = Vec::new();
            for state in &set {
                for term in &self.states[*state].accepting {
                    push_unique(&mut accepting, *term);
                }
                for edge in &self.states[*state].edges {
                    match *edge {
                        NfaEdge::CodePoint { .. } => code_point_edges.push(*edge),
                        NfaEdge::Eof { target } => {
                            for target in self.closure(target) {
                                push_unique(&mut eof_targets, target);
                            }
                        }
                        NfaEdge::Epsilon { .. } => {}
                    }
                }
            }
            let mut transitions = Vec::new();
            for merged in self.merge_edges(&code_point_edges) {
                let mut targets = merged.targets;
                targets.sort_unstable();
                targets.dedup();
                let target = intern_set(targets, &mut sets, &mut set_ids);
                transitions.push(DfaEdge {
                    from: merged.from,
                    to: merged.to,
                    target,
                });
            }
            eof_targets.sort_unstable();
            eof_targets.dedup();
            let eof =
                (!eof_targets.is_empty()).then(|| intern_set(eof_targets, &mut sets, &mut set_ids));
            accepting.sort_unstable();
            states.push(DfaState {
                accepting,
                edges: transitions,
                eof,
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
            let has_labeled = current
                .edges
                .iter()
                .any(|edge| !matches!(edge, NfaEdge::Epsilon { .. }));
            let uniquely_accepting = !current.accepting.is_empty()
                && !current.edges.iter().any(|edge| {
                    matches!(edge, NfaEdge::Epsilon { .. })
                        && same_set(&current.accepting, &self.states[edge.target()].accepting)
                });
            if has_labeled || uniquely_accepting {
                result.push(state);
            }
            for edge in &current.edges {
                if let NfaEdge::Epsilon { target } = edge {
                    stack.push(*target);
                }
            }
        }
        result.sort_unstable();
        result
    }

    fn merge_edges(&self, edges: &[NfaEdge]) -> Vec<MergedEdge> {
        let mut boundaries = Vec::new();
        for edge in edges {
            if let NfaEdge::CodePoint { from, to, .. } = *edge {
                push_unique(&mut boundaries, from);
                push_unique(&mut boundaries, to);
            }
        }
        boundaries.sort_unstable();
        let mut result = Vec::new();
        for pair in boundaries.windows(2) {
            let from = pair[0];
            let to = pair[1];
            let mut targets = Vec::new();
            for edge in edges {
                if let NfaEdge::CodePoint {
                    from: edge_from,
                    to: edge_to,
                    target,
                } = *edge
                    && edge_to > from
                    && edge_from < to
                {
                    for target in self.closure(target) {
                        push_unique(&mut targets, target);
                    }
                }
            }
            if !targets.is_empty() {
                result.push(MergedEdge { from, to, targets });
            }
        }
        result
    }
}

fn intern_set(
    targets: Vec<usize>,
    sets: &mut Vec<Vec<usize>>,
    set_ids: &mut BTreeMap<Vec<usize>, usize>,
) -> usize {
    if let Some(target) = set_ids.get(&targets) {
        return *target;
    }
    let target = sets.len();
    sets.push(targets.clone());
    set_ids.insert(targets, target);
    target
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MergedEdge {
    from: u32,
    to: u32,
    targets: Vec<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DfaEdge {
    pub from: u32,
    pub to: u32,
    pub target: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DfaState {
    pub accepting: Vec<TermId>,
    pub edges: Vec<DfaEdge>,
    pub eof: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenDfa {
    pub states: Vec<DfaState>,
    pub start: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EncodedTokenState {
    pub group_mask: u16,
    pub accept_start: u16,
    pub edge_start: u16,
    pub accept_count: u8,
    pub edge_count: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EncodedTokenEof {
    pub state: u16,
    pub target: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EncodedTokenAccept {
    pub term: u16,
    pub group_mask: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EncodedTokenEdge {
    pub from: u32,
    pub to: u32,
    pub target: u16,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EncodedTokenTable {
    pub states: Vec<EncodedTokenState>,
    pub accepts: Vec<EncodedTokenAccept>,
    pub edges: Vec<EncodedTokenEdge>,
    pub eof: Vec<EncodedTokenEof>,
}

impl TokenDfa {
    /// Encode the minimized token automaton as typed static-table data.
    ///
    /// # Errors
    ///
    /// Returns an error when offsets exceed the 16-bit runtime encoding.
    pub fn to_table(
        &self,
        terms: &TermSet,
        group_masks: &BTreeMap<u16, u16>,
        precedence: &[u16],
    ) -> Result<EncodedTokenTable, GeneratorError> {
        let order = self.reachable_order();
        if order.len() > usize::from(NO_STATE) {
            return Err(GeneratorError::new(
                "Tokenizer has too many states for 16-bit state ids",
                None,
            ));
        }
        let mut state_ids = vec![NO_STATE; self.states.len()];
        for (encoded, state_id) in order.iter().copied().enumerate() {
            state_ids[state_id] = u16::try_from(encoded)
                .expect("token state count was checked against the reserved sentinel");
        }

        let mut table = EncodedTokenTable::default();
        for state_id in order {
            let state = &self.states[state_id];
            let encoded_state = state_ids[state_id];
            let accept_start = table.accepts.len();
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
                table.accepts.push(EncodedTokenAccept {
                    term: id,
                    group_mask: group_masks.get(&id).copied().unwrap_or(u16::MAX),
                });
            }
            let accept_end = table.accepts.len();
            let accept_count = table_count(accept_end - accept_start, "accepts")?;
            let edge_start = table.edges.len();
            let mut previous_edge_end = None;
            for edge in &state.edges {
                if edge.from >= edge.to || edge.to > CODE_POINT_LIMIT {
                    return Err(GeneratorError::new(
                        "Tokenizer edge is outside the Unicode code-point domain",
                        None,
                    ));
                }
                if previous_edge_end.is_some_and(|end| edge.from < end) {
                    return Err(GeneratorError::new(
                        "Tokenizer state edges must be sorted and non-overlapping",
                        None,
                    ));
                }
                let target = state_ids.get(edge.target).copied().unwrap_or(NO_STATE);
                if target == NO_STATE {
                    return Err(GeneratorError::new(
                        "Tokenizer edge refers to an unreachable state",
                        None,
                    ));
                }
                table.edges.push(EncodedTokenEdge {
                    from: edge.from,
                    to: edge.to,
                    target,
                });
                previous_edge_end = Some(edge.to);
            }
            let edge_end = table.edges.len();
            let edge_count = table_count(edge_end - edge_start, "edges")?;
            if let Some(target) = state.eof {
                let target = state_ids.get(target).copied().unwrap_or(NO_STATE);
                if target == NO_STATE {
                    return Err(GeneratorError::new(
                        "Tokenizer EOF transition refers to an unreachable state",
                        None,
                    ));
                }
                table.eof.push(EncodedTokenEof {
                    state: encoded_state,
                    target,
                });
            }
            table.states.push(EncodedTokenState {
                group_mask: self.state_mask(state_id, terms, group_masks),
                accept_start: table_index(accept_start)?,
                edge_start: table_index(edge_start)?,
                accept_count,
                edge_count,
            });
        }
        Ok(table)
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
            if let Some(target) = self.states[state].eof {
                stack.push(target);
            }
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
            if let Some(target) = state.eof {
                closure[state_id].insert(target);
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

fn table_index(value: usize) -> Result<u16, GeneratorError> {
    u16::try_from(value)
        .map_err(|_| GeneratorError::new("Tokenizer table exceeds 16-bit indices", None))
}

fn table_count(value: usize, kind: &str) -> Result<u8, GeneratorError> {
    u8::try_from(value).map_err(|_| {
        GeneratorError::new(
            format!("Tokenizer state has too many {kind} for 8-bit counts"),
            None,
        )
    })
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
        state.eof = state.eof.map(|target| partition[target]);
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
    left.eof.map(|target| partition[target]) == right.eof.map(|target| partition[target])
        && left.edges.len() == right.edges.len()
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
