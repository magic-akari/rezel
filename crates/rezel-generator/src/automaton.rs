use std::cell::OnceCell;
use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use crate::GeneratorError;
use crate::grammar::{Conflicts, Rule, RuleId, TermId, TermSet};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Position {
    pub rule: RuleId,
    pub dot: usize,
    pub ahead: Vec<TermId>,
    pub ambiguity_ahead: Vec<String>,
    pub skip_ahead: TermId,
}

impl Position {
    #[must_use]
    pub fn next(&self, rules: &[Rule]) -> Option<TermId> {
        rules[self.rule].parts.get(self.dot).copied()
    }

    #[must_use]
    pub fn advance(&self) -> Self {
        Self {
            rule: self.rule,
            dot: self.dot + 1,
            ahead: self.ahead.clone(),
            ambiguity_ahead: self.ambiguity_ahead.clone(),
            skip_ahead: self.skip_ahead,
        }
    }

    #[must_use]
    pub fn skip(&self, rules: &[Rule]) -> TermId {
        let rule = &rules[self.rule];
        if self.dot == rule.parts.len() {
            self.skip_ahead
        } else {
            rule.skip
        }
    }

    #[must_use]
    pub fn conflicts(&self, rules: &[Rule], position: usize) -> Conflicts {
        let mut conflicts = rules[self.rule].conflicts[position].clone();
        if position == rules[self.rule].parts.len() && !self.ambiguity_ahead.is_empty() {
            conflicts = conflicts.join(&Conflicts {
                precedence: 0,
                ambiguity_groups: self.ambiguity_ahead.clone(),
                cut: 0,
            });
        }
        conflicts
    }

    #[must_use]
    pub fn same_core(&self, other: &Self) -> bool {
        self.rule == other.rule && self.dot == other.dot
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    Shift { term: TermId, target: usize },
    Reduce { term: TermId, rule: RuleId },
}

impl Action {
    #[must_use]
    pub const fn term(&self) -> TermId {
        match *self {
            Self::Shift { term, .. } | Self::Reduce { term, .. } => term,
        }
    }

    #[must_use]
    pub fn equivalent(&self, other: &Self, rules: &[Rule], terms: &TermSet) -> bool {
        match (self, other) {
            (
                Self::Shift { term, target },
                Self::Shift {
                    term: other_term,
                    target: other_target,
                },
            ) => term == other_term && target == other_target,
            (
                Self::Reduce { term, rule },
                Self::Reduce {
                    term: other_term,
                    rule: other_rule,
                },
            ) => term == other_term && rules[*rule].same_reduce(&rules[*other_rule], terms),
            _ => false,
        }
    }

    #[must_use]
    fn equivalent_mapped(
        &self,
        other: &Self,
        mapping: &[usize],
        rules: &[Rule],
        terms: &TermSet,
    ) -> bool {
        match (self, other) {
            (
                Self::Shift { term, target },
                Self::Shift {
                    term: other_term,
                    target: other_target,
                },
            ) => term == other_term && mapping[*target] == mapping[*other_target],
            (
                Self::Reduce { term, rule },
                Self::Reduce {
                    term: other_term,
                    rule: other_rule,
                },
            ) => term == other_term && rules[*rule].same_reduce(&rules[*other_rule], terms),
            _ => false,
        }
    }

    #[must_use]
    fn mapped(&self, mapping: &[usize]) -> Self {
        match *self {
            Self::Shift { term, target } => Self::Shift {
                term,
                target: mapping[target],
            },
            Self::Reduce { term, rule } => Self::Reduce { term, rule },
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ActionTermRanges {
    term: TermId,
    shift_start: u32,
    shift_end: u32,
    reduce_start: u32,
    reduce_end: u32,
}

impl ActionTermRanges {
    #[must_use]
    fn len(&self) -> usize {
        (self.shift_end - self.shift_start + self.reduce_end - self.reduce_start) as usize
    }

    fn any_index(&self, mut predicate: impl FnMut(usize) -> bool) -> bool {
        for index in self.shift_start as usize..self.shift_end as usize {
            if predicate(index) {
                return true;
            }
        }
        for index in self.reduce_start as usize..self.reduce_end as usize {
            if predicate(index) {
                return true;
            }
        }
        false
    }

    fn all_indices(&self, mut predicate: impl FnMut(usize) -> bool) -> bool {
        for index in self.shift_start as usize..self.shift_end as usize {
            if !predicate(index) {
                return false;
            }
        }
        for index in self.reduce_start as usize..self.reduce_end as usize {
            if !predicate(index) {
                return false;
            }
        }
        true
    }
}

fn action_offset(value: usize) -> u32 {
    u32::try_from(value).expect("a state action index fits in u32")
}

struct ActionIndexCache {
    by_state: Vec<OnceCell<Vec<ActionTermRanges>>>,
}

impl ActionIndexCache {
    fn new(state_count: usize) -> Self {
        let mut by_state = Vec::with_capacity(state_count);
        by_state.resize_with(state_count, OnceCell::new);
        Self { by_state }
    }

    fn actions_by_term(&self, state: &State) -> &[ActionTermRanges] {
        self.by_state
            .get(state.id)
            .expect("canonical state id indexes the action cache")
            .get_or_init(|| build_action_index(&state.actions))
            .as_slice()
    }
}

fn build_action_index(actions: &[Action]) -> Vec<ActionTermRanges> {
    // State::finish groups each action kind by term. A term can therefore
    // contribute at most one shift range and one reduction range.
    let first_reduce = actions.partition_point(|action| matches!(action, Action::Shift { .. }));
    let mut result = Vec::with_capacity(actions.len());
    let mut start = 0;
    while start < actions.len() {
        let end_limit = if start < first_reduce {
            first_reduce
        } else {
            actions.len()
        };
        let term = actions[start].term();
        let mut end = start + 1;
        while end < end_limit && actions[end].term() == term {
            end += 1;
        }
        let (shift_start, shift_end, reduce_start, reduce_end) = if start < first_reduce {
            (action_offset(start), action_offset(end), 0, 0)
        } else {
            (0, 0, action_offset(start), action_offset(end))
        };
        result.push(ActionTermRanges {
            term,
            shift_start,
            shift_end,
            reduce_start,
            reduce_end,
        });
        start = end;
    }
    result.sort_unstable_by_key(|ranges| ranges.term);

    let mut write = 0;
    for read in 0..result.len() {
        let ranges = result[read];
        if write > 0 && result[write - 1].term == ranges.term {
            let previous = &mut result[write - 1];
            if ranges.shift_start != ranges.shift_end {
                assert_eq!(previous.shift_start, previous.shift_end);
                previous.shift_start = ranges.shift_start;
                previous.shift_end = ranges.shift_end;
            }
            if ranges.reduce_start != ranges.reduce_end {
                assert_eq!(previous.reduce_start, previous.reduce_end);
                previous.reduce_start = ranges.reduce_start;
                previous.reduce_end = ranges.reduce_end;
            }
        } else {
            result[write] = ranges;
            write += 1;
        }
    }
    result.truncate(write);
    result
}

#[derive(Clone, Debug)]
pub struct State {
    pub id: usize,
    pub positions: Vec<Position>,
    pub actions: Vec<Action>,
    pub action_positions: Vec<Vec<Position>>,
    pub gotos: Vec<Action>,
    pub token_group: Option<u8>,
    pub default_reduce: Option<RuleId>,
    pub flags: u32,
    pub skip: TermId,
    pub start_rule: Option<TermId>,
}

impl State {
    fn new(id: usize, positions: Vec<Position>, skip: TermId, start_rule: Option<TermId>) -> Self {
        Self {
            id,
            positions,
            actions: Vec::new(),
            action_positions: Vec::new(),
            gotos: Vec::new(),
            token_group: None,
            default_reduce: None,
            flags: 0,
            skip,
            start_rule,
        }
    }

    fn add_action(
        &mut self,
        value: Action,
        positions: Vec<Position>,
        rules: &[Rule],
        terms: &TermSet,
    ) -> Result<bool, GeneratorError> {
        let mut index = 0;
        while index < self.actions.len() {
            let action = &self.actions[index];
            if action.term() != value.term() {
                index += 1;
                continue;
            }
            if action.equivalent(&value, rules, terms) {
                return Ok(false);
            }

            let full_positions = add_origins(&positions, &self.positions, rules);
            let action_positions =
                add_origins(&self.action_positions[index], &self.positions, rules);
            let conflicts = conflicts_at(&full_positions, rules);
            let action_conflicts = conflicts_at(&action_positions, rules);
            let repeat_difference =
                compare_repeat_precedence(&full_positions, &action_positions, rules, terms);
            let difference = repeat_difference + conflicts.precedence - action_conflicts.precedence;
            if difference > 0 {
                self.actions.remove(index);
                self.action_positions.remove(index);
                continue;
            }
            if difference < 0 {
                return Ok(false);
            }
            let allowed = conflicts
                .ambiguity_groups
                .iter()
                .any(|group| action_conflicts.ambiguity_groups.contains(group));
            if allowed {
                index += 1;
                continue;
            }
            let conflict_kind = match action {
                Action::Shift { .. } => "shift/reduce",
                Action::Reduce { .. } => "reduce/reduce",
            };
            let term = &terms.terms[value.term()].name;
            return Err(GeneratorError::new(
                format!("{conflict_kind} conflict on token {term}"),
                None,
            ));
        }
        self.actions.push(value);
        self.action_positions.push(positions);
        Ok(true)
    }

    fn finish(&mut self, rules: &[Rule], terms: &TermSet) {
        if let Some(Action::Reduce { rule, .. }) = self.actions.first() {
            let rule = *rule;
            if self.actions.iter().all(|action| {
                let Action::Reduce {
                    rule: other_rule, ..
                } = action
                else {
                    return false;
                };
                rules[rule].same_reduce(&rules[*other_rule], terms)
            }) {
                self.default_reduce = Some(rule);
            }
        }
        self.actions
            .sort_by(|left, right| compare_actions(left, right, rules, terms));
        self.gotos
            .sort_by(|left, right| compare_actions(left, right, rules, terms));
    }

    #[must_use]
    fn behavior_eq(&self, other: &Self, rules: &[Rule], terms: &TermSet) -> bool {
        match (self.default_reduce, other.default_reduce) {
            (Some(left), Some(right)) => {
                return rules[left].same_reduce(&rules[right], terms);
            }
            (Some(_), None) | (None, Some(_)) => return false,
            (None, None) => {}
        }
        self.skip == other.skip
            && self.token_group == other.token_group
            && action_sets_equal(&self.actions, &other.actions, rules, terms)
            && action_sets_equal(&self.gotos, &other.gotos, rules, terms)
    }
}

pub type FirstSets = BTreeMap<TermId, Vec<Option<TermId>>>;

#[derive(Default)]
struct AutomatonIndex {
    cores: HashMap<Vec<Position>, usize>,
    state_by_hash: HashMap<u64, usize>,
    next_state_with_hash: Vec<usize>,
}

const NO_STATE: usize = usize::MAX;

impl AutomatonIndex {
    fn find_core(&self, core: &[Position]) -> Option<usize> {
        self.cores.get(core).copied()
    }

    fn remember_core(&mut self, core: Vec<Position>, state: usize) {
        self.cores.insert(core, state);
    }

    fn find_state(&self, hash: u64, positions: &[Position], states: &[State]) -> Option<usize> {
        let mut state = *self.state_by_hash.get(&hash)?;
        loop {
            if states[state].positions == positions {
                return Some(state);
            }
            state = self.next_state_with_hash[state];
            if state == NO_STATE {
                return None;
            }
        }
    }

    fn remember_state(&mut self, hash: u64, state: usize) {
        debug_assert_eq!(state, self.next_state_with_hash.len());
        self.next_state_with_hash.push(NO_STATE);

        match self.state_by_hash.entry(hash) {
            Entry::Vacant(entry) => {
                entry.insert(state);
            }
            Entry::Occupied(entry) => {
                let mut tail = *entry.get();
                loop {
                    let next = self.next_state_with_hash[tail];
                    if next == NO_STATE {
                        self.next_state_with_hash[tail] = state;
                        break;
                    }
                    tail = next;
                }
            }
        }
    }
}

/// Compute a cheap bucket key. Candidates are always compared exactly, so a
/// collision can only add comparison work and cannot change the automaton.
fn hash_positions(positions: &[Position]) -> u64 {
    positions
        .iter()
        .fold(hash_value(5_381, positions.len()), |hash, position| {
            let hash = hash_value(hash_value(hash, position.rule), position.dot);
            let hash = hash_value(hash, position.skip_ahead);
            let hash = position
                .ahead
                .iter()
                .fold(hash_value(hash, position.ahead.len()), |hash, term| {
                    hash_value(hash, *term)
                });
            position.ambiguity_ahead.iter().fold(
                hash_value(hash, position.ambiguity_ahead.len()),
                |hash, group| {
                    group
                        .as_bytes()
                        .iter()
                        .fold(hash_value(hash, group.len()), |hash, byte| {
                            hash_value(hash, usize::from(*byte))
                        })
                },
            )
        })
}

fn hash_value(hash: u64, value: usize) -> u64 {
    hash.wrapping_mul(33).wrapping_add(value as u64)
}

#[must_use]
pub fn compute_first_sets(terms: &TermSet, rules: &[Rule]) -> FirstSets {
    let mut table = BTreeMap::new();
    for (term_id, term) in terms.terms.iter().enumerate() {
        if !term.terminal() {
            table.insert(term_id, Vec::new());
        }
    }
    loop {
        let mut changed = false;
        for (term_id, term) in terms.terms.iter().enumerate() {
            if term.terminal() {
                continue;
            }
            for rule_id in &term.rules {
                let rule = &rules[*rule_id];
                let mut found = false;
                let mut additions = Vec::new();
                for part in &rule.parts {
                    found = true;
                    if terms.terms[*part].terminal() {
                        push_unique(&mut additions, Some(*part));
                    } else {
                        for entry in &table[part] {
                            match entry {
                                Some(term) => push_unique(&mut additions, Some(*term)),
                                None => found = false,
                            }
                        }
                    }
                    if found {
                        break;
                    }
                }
                if !found {
                    push_unique(&mut additions, None);
                }
                let target = table
                    .get_mut(&term_id)
                    .expect("nonterminal has a first-set entry");
                let before = target.len();
                for addition in additions {
                    push_unique(target, addition);
                }
                changed |= target.len() != before;
            }
        }
        if !changed {
            for values in table.values_mut() {
                values.sort_unstable();
            }
            return table;
        }
    }
}

/// Construct the full Canonical LR(1) automaton.
///
/// # Errors
///
/// Returns grammar conflicts and inconsistent skip contexts.
pub fn build_full_automaton(
    terms: &TermSet,
    rules: &[Rule],
    start_terms: &[TermId],
    first: &FirstSets,
) -> Result<Vec<State>, GeneratorError> {
    let mut states = Vec::new();
    // Core lookup does not iterate the map, so state discovery and ids remain
    // determined by the breadth-first traversal below.
    let mut index = AutomatonIndex::default();
    add_start_states(start_terms, &mut states, &mut index, terms, rules, first)?;

    let mut filled = 0;
    while filled < states.len() {
        fill_state(filled, &mut states, &mut index, terms, rules, first)?;
        filled += 1;
    }
    for state in &mut states {
        state.finish(rules, terms);
    }
    Ok(states)
}

fn add_start_states(
    start_terms: &[TermId],
    states: &mut Vec<State>,
    index: &mut AutomatonIndex,
    terms: &TermSet,
    rules: &[Rule],
    first: &FirstSets,
) -> Result<(), GeneratorError> {
    for start_term in start_terms {
        let start_skip = terms.terms[*start_term].rules.first().map_or_else(
            || {
                terms
                    .names
                    .get("%noskip")
                    .copied()
                    .expect("%noskip is defined before automaton construction")
            },
            |rule| rules[*rule].skip,
        );
        let core = terms.terms[*start_term]
            .rules
            .iter()
            .map(|rule| Position {
                rule: *rule,
                dot: 0,
                ahead: vec![terms.eof],
                ambiguity_ahead: Vec::new(),
                skip_ahead: start_skip,
            })
            .collect::<Vec<_>>();
        let _ = get_state(core, Some(*start_term), states, index, terms, rules, first)?;
    }
    Ok(())
}

fn fill_state(
    state_id: usize,
    states: &mut Vec<State>,
    index: &mut AutomatonIndex,
    terms: &TermSet,
    rules: &[Rule],
    first: &FirstSets,
) -> Result<(), GeneratorError> {
    let (by_term, at_end) = partition_positions(&states[state_id].positions, terms, rules);
    add_transitions(state_id, by_term, states, index, terms, rules, first)?;
    if add_reductions(state_id, at_end, states, rules, terms)? {
        retain_compatible_gotos(&mut states[state_id], first);
    }
    Ok(())
}

fn partition_positions(
    positions: &[Position],
    terms: &TermSet,
    rules: &[Rule],
) -> (BTreeMap<TermId, Vec<Position>>, Vec<Position>) {
    let mut by_term = BTreeMap::new();
    let mut at_end = Vec::new();
    for position in positions {
        let rule = &rules[position.rule];
        if position.dot == rule.parts.len() {
            if !terms.terms[rule.name].top() {
                at_end.push(position.clone());
            }
        } else {
            by_term
                .entry(rule.parts[position.dot])
                .or_insert_with(Vec::new)
                .push(position.clone());
        }
    }
    (by_term, at_end)
}

fn add_transitions(
    state_id: usize,
    by_term: BTreeMap<TermId, Vec<Position>>,
    states: &mut Vec<State>,
    index: &mut AutomatonIndex,
    terms: &TermSet,
    rules: &[Rule],
    first: &FirstSets,
) -> Result<(), GeneratorError> {
    for (term, source_positions) in by_term {
        let advanced = source_positions
            .iter()
            .map(Position::advance)
            .collect::<Vec<_>>();
        let terminal = terms.terms[term].terminal();
        let core = if terminal {
            apply_cut(&advanced, rules)
        } else {
            advanced
        };
        let target = get_state(core, None, states, index, terms, rules, first)?
            .expect("non-empty transition core produces a state");
        let action = Action::Shift { term, target };
        if terminal {
            states[state_id].add_action(action, source_positions, rules, terms)?;
        } else {
            states[state_id].gotos.push(action);
        }
    }
    Ok(())
}

fn add_reductions(
    state_id: usize,
    positions: Vec<Position>,
    states: &mut [State],
    rules: &[Rule],
    terms: &TermSet,
) -> Result<bool, GeneratorError> {
    let mut replaced = false;
    for position in positions {
        for ahead in &position.ahead {
            let before = states[state_id].actions.len();
            states[state_id].add_action(
                Action::Reduce {
                    term: *ahead,
                    rule: position.rule,
                },
                vec![position.clone()],
                rules,
                terms,
            )?;
            replaced |= states[state_id].actions.len() == before;
        }
    }
    Ok(replaced)
}

fn retain_compatible_gotos(state: &mut State, first: &FirstSets) {
    let actions = state.actions.clone();
    state.gotos.retain(|goto| {
        first[&goto.term()].iter().flatten().any(|term| {
            actions.iter().any(|action| {
                matches!(
                    action,
                    Action::Shift {
                        term: action_term,
                        ..
                    } if action_term == term
                )
            })
        })
    });
}

/// Apply Lezer's conservative same-core collapse and identical-state merge.
#[must_use]
pub fn finish_automaton(full: &[State], rules: &[Rule], terms: &TermSet) -> Vec<State> {
    let collapsed = collapse_automaton(full, rules, terms);
    merge_identical(collapsed, rules, terms)
}

fn get_state(
    mut core: Vec<Position>,
    top: Option<TermId>,
    states: &mut Vec<State>,
    index: &mut AutomatonIndex,
    terms: &TermSet,
    rules: &[Rule],
    first: &FirstSets,
) -> Result<Option<usize>, GeneratorError> {
    if core.is_empty() {
        return Ok(None);
    }
    core.sort();
    let mut skip = None;
    for position in &core {
        let current = position.skip(rules);
        match skip {
            Some(previous) if previous != current => {
                return Err(GeneratorError::new("Inconsistent skip sets", None));
            }
            None => skip = Some(current),
            _ => {}
        }
    }
    if let Some(state) = index.find_core(&core) {
        if states[state].skip != skip.expect("core is non-empty") {
            return Err(GeneratorError::new("Inconsistent skip sets", None));
        }
        return Ok(Some(state));
    }

    let set = closure(&core, terms, rules, first)?;
    let set_hash = hash_positions(&set);
    let found = if top.is_none() {
        index.find_state(set_hash, &set, states)
    } else {
        None
    };
    let state = if let Some(state) = found {
        state
    } else {
        let id = states.len();
        states.push(State::new(id, set, skip.expect("core is non-empty"), top));
        index.remember_state(set_hash, id);
        id
    };
    index.remember_core(core, state);
    Ok(Some(state))
}

fn closure(
    core: &[Position],
    terms: &TermSet,
    rules: &[Rule],
    first: &FirstSets,
) -> Result<Vec<Position>, GeneratorError> {
    let mut result = core.to_vec();
    // Closure additions always start at dot zero, and rule ids are dense.
    let mut zero_dot = vec![None; rules.len()];
    for (index, position) in result.iter().enumerate() {
        if position.dot == 0 && zero_dot[position.rule].is_none() {
            zero_dot[position.rule] = Some(index);
        }
    }
    // Revisit only positions whose lookahead contribution changed. This is
    // the same fixed point as rescanning the whole closure on every pass.
    let mut pending = (0..result.len()).collect::<VecDeque<_>>();
    let mut queued = vec![true; result.len()];
    while let Some(index) = pending.pop_front() {
        queued[index] = false;
        let position = result[index].clone();
        let Some(next) = position.next(rules) else {
            continue;
        };
        if terms.terms[next].terminal() {
            continue;
        }
        let ahead = terms_ahead(
            &rules[position.rule],
            position.dot,
            &position.ahead,
            terms,
            first,
        );
        let ambiguity_ahead = position.conflicts(rules, position.dot + 1).ambiguity_groups;
        let rule = &rules[position.rule];
        let skip_ahead = if position.dot == rule.parts.len() - 1 {
            position.skip_ahead
        } else {
            rule.skip
        };
        for nested_rule in &terms.terms[next].rules {
            if let Some(existing_index) = zero_dot[*nested_rule] {
                let existing = &mut result[existing_index];
                if existing.skip_ahead != skip_ahead {
                    return Err(GeneratorError::new("Inconsistent skip sets", None));
                }
                let before_ahead = existing.ahead.len();
                for term in &ahead {
                    push_unique(&mut existing.ahead, *term);
                }
                let before_ambiguity = existing.ambiguity_ahead.len();
                existing.ambiguity_ahead =
                    union_strings(&existing.ambiguity_ahead, &ambiguity_ahead);
                let changed = existing.ahead.len() != before_ahead
                    || existing.ambiguity_ahead.len() != before_ambiguity;
                if changed && !queued[existing_index] {
                    queued[existing_index] = true;
                    pending.push_back(existing_index);
                }
                continue;
            }
            let nested_index = result.len();
            zero_dot[*nested_rule] = Some(nested_index);
            result.push(Position {
                rule: *nested_rule,
                dot: 0,
                ahead: ahead.clone(),
                ambiguity_ahead: ambiguity_ahead.clone(),
                skip_ahead,
            });
            queued.push(true);
            pending.push_back(nested_index);
        }
    }
    for position in &mut result {
        position.ahead.sort_unstable();
        position.ambiguity_ahead.sort();
    }
    result.sort();
    result.dedup();
    Ok(result)
}

fn terms_ahead(
    rule: &Rule,
    position: usize,
    after: &[TermId],
    terms: &TermSet,
    first: &FirstSets,
) -> Vec<TermId> {
    let mut found = Vec::new();
    for next in rule.parts.iter().skip(position + 1) {
        let mut nullable = false;
        if terms.terms[*next].terminal() {
            push_unique(&mut found, *next);
        } else {
            for term in &first[next] {
                match term {
                    Some(term) => push_unique(&mut found, *term),
                    None => nullable = true,
                }
            }
        }
        if !nullable {
            return found;
        }
    }
    for term in after {
        push_unique(&mut found, *term);
    }
    found
}

fn add_origins(positions: &[Position], context: &[Position], rules: &[Rule]) -> Vec<Position> {
    let mut result = positions.to_vec();
    let mut cursor = 0;
    while cursor < result.len() {
        let next = result[cursor].clone();
        if next.dot == 0 {
            let name = rules[next.rule].name;
            for position in context {
                if position.next(rules) == Some(name) && !result.contains(position) {
                    result.push(position.clone());
                }
            }
        }
        cursor += 1;
    }
    result
}

fn conflicts_at(positions: &[Position], rules: &[Rule]) -> Conflicts {
    positions
        .iter()
        .fold(Conflicts::default(), |result, position| {
            result.join(&position.conflicts(rules, position.dot))
        })
}

fn compare_repeat_precedence(
    left: &[Position],
    right: &[Position],
    rules: &[Rule],
    terms: &TermSet,
) -> i32 {
    for position in left {
        let rule = &rules[position.rule];
        if !terms.terms[rule.name].repeated() {
            continue;
        }
        for other in right {
            let other_rule = &rules[other.rule];
            if other_rule.name != rule.name {
                continue;
            }
            if rule.is_repeat_wrap(terms) && position.dot == 2 {
                return 1;
            }
            if other_rule.is_repeat_wrap(terms) && other.dot == 2 {
                return -1;
            }
        }
    }
    0
}

fn apply_cut(positions: &[Position], rules: &[Rule]) -> Vec<Position> {
    let mut found = Vec::new();
    let mut cut = 1;
    for position in positions {
        let value = rules[position.rule].conflicts[position.dot - 1].cut;
        if value < cut {
            continue;
        }
        if value > cut {
            cut = value;
            found.clear();
        }
        found.push(position.clone());
    }
    if found.is_empty() {
        positions.to_vec()
    } else {
        found
    }
}

fn compare_actions(
    left: &Action,
    right: &Action,
    rules: &[Rule],
    terms: &TermSet,
) -> std::cmp::Ordering {
    let left_kind = matches!(left, Action::Reduce { .. });
    let right_kind = matches!(right, Action::Reduce { .. });
    left_kind
        .cmp(&right_kind)
        .then_with(|| {
            terms
                .output_id(left.term())
                .cmp(&terms.output_id(right.term()))
        })
        .then_with(|| match (left, right) {
            (Action::Shift { target, .. }, Action::Shift { target: other, .. }) => {
                target.cmp(other)
            }
            (Action::Reduce { rule, .. }, Action::Reduce { rule: other, .. }) => terms
                .output_id(rules[*rule].name)
                .cmp(&terms.output_id(rules[*other].name))
                .then_with(|| rules[*rule].parts.len().cmp(&rules[*other].parts.len())),
            _ => std::cmp::Ordering::Equal,
        })
}

fn collapse_automaton(states: &[State], rules: &[Rule], terms: &TermSet) -> Vec<State> {
    // The full automaton is immutable during collapse, so keep this derived
    // index local instead of extending State's lifetime and clone semantics.
    let action_indexes = ActionIndexCache::new(states.len());
    let mut mapping = Vec::with_capacity(states.len());
    let mut groups: Vec<Group> = Vec::new();
    for (state_id, state) in states.iter().enumerate() {
        let mut matched = None;
        if state.start_rule.is_none() {
            for (group_id, group) in groups.iter().enumerate() {
                let other = &states[group.members[0]];
                if state.token_group == other.token_group
                    && state.skip == other.skip
                    && other.start_rule.is_none()
                    && same_position_core(&state.positions, &other.positions)
                {
                    matched = Some(group_id);
                    break;
                }
            }
        }
        let group_id = if let Some(group_id) = matched {
            groups[group_id].members.push(state_id);
            group_id
        } else {
            let group_id = groups.len();
            groups.push(Group {
                origin: group_id,
                members: vec![state_id],
            });
            group_id
        };
        mapping.push(group_id);
    }
    let context = CollapseContext {
        states,
        rules,
        terms,
        action_indexes: &action_indexes,
    };

    loop {
        let mut conflicts = false;
        let initial_group_count = groups.len();
        for group_id in 0..initial_group_count {
            let mut left = 0;
            while left + 1 < groups[group_id].members.len() {
                let mut right = left + 1;
                while right < groups[group_id].members.len() {
                    let left_state = groups[group_id].members[left];
                    let right_state = groups[group_id].members[right];
                    if context.can_merge(
                        &context.states[left_state],
                        &context.states[right_state],
                        &mapping,
                    ) {
                        right += 1;
                        continue;
                    }
                    conflicts = true;
                    context.spill(group_id, right, &mut groups, &mut mapping);
                }
                left += 1;
            }
        }
        if !conflicts {
            return merge_states(context.states, &mapping, context.rules, context.terms);
        }
    }
}

#[derive(Clone, Debug)]
struct Group {
    origin: usize,
    members: Vec<usize>,
}

struct CollapseContext<'a> {
    states: &'a [State],
    rules: &'a [Rule],
    terms: &'a TermSet,
    action_indexes: &'a ActionIndexCache,
}

impl CollapseContext<'_> {
    fn spill(&self, group_id: usize, index: usize, groups: &mut Vec<Group>, mapping: &mut [usize]) {
        let state_id = groups[group_id].members.swap_remove(index);
        let origin = groups[group_id].origin;
        let destination = ((group_id + 1)..groups.len()).find(|candidate| {
            mapping[state_id] = *candidate;
            groups[*candidate].origin == origin
                && groups[*candidate].members.iter().all(|member| {
                    self.can_merge(&self.states[state_id], &self.states[*member], mapping)
                })
        });
        if let Some(destination) = destination {
            groups[destination].members.push(state_id);
            return;
        }
        mapping[state_id] = groups.len();
        groups.push(Group {
            origin,
            members: vec![state_id],
        });
    }

    fn can_merge(&self, left: &State, right: &State, mapping: &[usize]) -> bool {
        for goto in &left.gotos {
            for other in &right.gotos {
                if goto.term() == other.term()
                    && !goto.equivalent_mapped(other, mapping, self.rules, self.terms)
                {
                    return false;
                }
            }
        }
        let right_actions_by_term = self.action_indexes.actions_by_term(right);
        for action in &left.actions {
            let Ok(right_index) =
                right_actions_by_term.binary_search_by_key(&action.term(), |ranges| ranges.term)
            else {
                continue;
            };
            let right_actions = right_actions_by_term[right_index];
            let has_conflict = right_actions.any_index(|index| {
                !right.actions[index].equivalent_mapped(action, mapping, self.rules, self.terms)
            });
            if !has_conflict {
                continue;
            }
            if right_actions.len() == 1 {
                return false;
            }
            let left_actions_by_term = self.action_indexes.actions_by_term(left);
            let left_index = left_actions_by_term
                .binary_search_by_key(&action.term(), |ranges| ranges.term)
                .expect("each action is indexed by its term");
            let left_actions = left_actions_by_term[left_index];
            let equal = left_actions.len() == right_actions.len()
                && left_actions.all_indices(|left_index| {
                    right_actions.any_index(|right_index| {
                        left.actions[left_index].equivalent_mapped(
                            &right.actions[right_index],
                            mapping,
                            self.rules,
                            self.terms,
                        )
                    })
                });
            if !equal {
                return false;
            }
        }
        true
    }
}

fn merge_states(
    states: &[State],
    mapping: &[usize],
    rules: &[Rule],
    terms: &TermSet,
) -> Vec<State> {
    let count = mapping.iter().copied().max().map_or(0, |value| value + 1);
    let mut result = (0..count)
        .map(|id| {
            let source = states
                .iter()
                .find(|state| mapping[state.id] == id)
                .expect("every mapped group has a source state");
            let mut state =
                State::new(id, source.positions.clone(), source.skip, source.start_rule);
            state.token_group = source.token_group;
            state.default_reduce = source.default_reduce;
            state
        })
        .collect::<Vec<_>>();

    for state in states {
        let target_id = mapping[state.id];
        let target = &mut result[target_id];
        target.flags |= state.flags;
        for (index, action) in state.actions.iter().enumerate() {
            let mapped = action.mapped(mapping);
            if !target
                .actions
                .iter()
                .any(|known| known.equivalent(&mapped, rules, terms))
            {
                target.actions.push(mapped);
                target
                    .action_positions
                    .push(state.action_positions[index].clone());
            }
        }
        for goto in &state.gotos {
            let mapped = goto.mapped(mapping);
            if !target
                .gotos
                .iter()
                .any(|known| known.equivalent(&mapped, rules, terms))
            {
                target.gotos.push(mapped);
            }
        }
    }
    result
}

fn merge_identical(mut states: Vec<State>, rules: &[Rule], terms: &TermSet) -> Vec<State> {
    loop {
        let mut mapping = vec![0; states.len()];
        let mut result: Vec<State> = Vec::new();
        let mut merged = false;
        for (index, state) in states.iter().enumerate() {
            let found = result
                .iter()
                .position(|known| state.behavior_eq(known, rules, terms));
            if let Some(target) = found {
                mapping[index] = target;
                merged = true;
                let mut additions = state
                    .positions
                    .iter()
                    .filter(|position| {
                        !result[target]
                            .positions
                            .iter()
                            .any(|known| known.same_core(position))
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                additions.append(&mut result[target].positions);
                additions.sort();
                result[target].positions = additions;
            } else {
                mapping[index] = result.len();
                result.push(state.clone());
            }
        }
        if !merged {
            return states;
        }
        for state in &mut result {
            if state.default_reduce.is_none() {
                state.actions = state
                    .actions
                    .iter()
                    .map(|action| action.mapped(&mapping))
                    .collect();
                state.gotos = state
                    .gotos
                    .iter()
                    .map(|action| action.mapped(&mapping))
                    .collect();
            }
        }
        for (id, state) in result.iter_mut().enumerate() {
            state.id = id;
        }
        states = result;
    }
}

fn same_position_core(left: &[Position], right: &[Position]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.same_core(right))
}

fn action_sets_equal(left: &[Action], right: &[Action], rules: &[Rule], terms: &TermSet) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.equivalent(right, rules, terms))
}

fn push_unique<T>(values: &mut Vec<T>, value: T)
where
    T: Eq,
{
    if !values.contains(&value) {
        values.push(value);
    }
}

fn union_strings(left: &[String], right: &[String]) -> Vec<String> {
    left.iter()
        .chain(right)
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
