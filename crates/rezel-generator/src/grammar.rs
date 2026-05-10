use std::collections::{BTreeMap, BTreeSet};

use crate::GeneratorError;

pub type TermId = usize;
pub type RuleId = usize;
pub type Props = BTreeMap<String, String>;

const TERMINAL: u8 = 1;
const TOP: u8 = 2;
const EOF: u8 = 4;
const PRESERVE: u8 = 8;
const REPEATED: u8 = 16;
const INLINE: u8 = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Term {
    pub name: String,
    flags: u8,
    pub node_name: Option<String>,
    pub props: Props,
    pub rules: Vec<RuleId>,
    pub output_id: Option<u16>,
}

impl Term {
    #[must_use]
    pub fn node_type(&self) -> bool {
        self.top() || self.node_name.is_some() || !self.props.is_empty() || self.repeated()
    }

    #[must_use]
    pub const fn terminal(&self) -> bool {
        self.flags & TERMINAL != 0
    }

    #[must_use]
    pub const fn eof(&self) -> bool {
        self.flags & EOF != 0
    }

    #[must_use]
    pub fn error(&self) -> bool {
        self.props.contains_key("error")
    }

    #[must_use]
    pub const fn top(&self) -> bool {
        self.flags & TOP != 0
    }

    #[must_use]
    pub fn interesting(&self) -> bool {
        self.flags != 0 || self.node_name.is_some()
    }

    #[must_use]
    pub const fn repeated(&self) -> bool {
        self.flags & REPEATED != 0
    }

    pub fn set_preserve(&mut self, value: bool) {
        if value {
            self.flags |= PRESERVE;
        } else {
            self.flags &= !PRESERVE;
        }
    }

    #[must_use]
    pub const fn preserve(&self) -> bool {
        self.flags & PRESERVE != 0
    }

    pub fn set_inline(&mut self, value: bool) {
        if value {
            self.flags |= INLINE;
        } else {
            self.flags &= !INLINE;
        }
    }

    #[must_use]
    pub const fn inline(&self) -> bool {
        self.flags & INLINE != 0
    }
}

#[derive(Clone, Debug)]
pub struct FinishedTerms {
    pub node_types: Vec<TermId>,
    pub term_names: BTreeMap<u16, String>,
    pub min_repeat_term: u16,
    pub max_term: u16,
}

#[derive(Clone, Debug)]
pub struct TermSet {
    pub terms: Vec<Term>,
    pub names: BTreeMap<String, TermId>,
    pub eof: TermId,
    pub error: TermId,
    pub tops: Vec<TermId>,
}

impl Default for TermSet {
    fn default() -> Self {
        Self::new()
    }
}

impl TermSet {
    #[must_use]
    pub fn new() -> Self {
        let mut terms = Self {
            terms: Vec::new(),
            names: BTreeMap::new(),
            eof: 0,
            error: 0,
            tops: Vec::new(),
        };
        terms.eof = terms.term("␄", None, TERMINAL | EOF, Props::new());
        terms.error = terms.term("⚠", Some("⚠".to_owned()), PRESERVE, Props::new());
        terms
    }

    pub fn term(
        &mut self,
        name: impl Into<String>,
        node_name: Option<String>,
        flags: u8,
        props: Props,
    ) -> TermId {
        let name = name.into();
        let id = self.terms.len();
        self.terms.push(Term {
            name: name.clone(),
            flags,
            node_name,
            props,
            rules: Vec::new(),
            output_id: None,
        });
        self.names.insert(name, id);
        id
    }

    pub fn make_top(&mut self, node_name: Option<String>, props: Props) -> TermId {
        let term = self.term("@top", node_name, TOP, props);
        self.tops.push(term);
        term
    }

    pub fn make_terminal(
        &mut self,
        name: impl Into<String>,
        node_name: Option<String>,
        props: Props,
    ) -> TermId {
        self.term(name, node_name, TERMINAL, props)
    }

    pub fn make_nonterminal(
        &mut self,
        name: impl Into<String>,
        node_name: Option<String>,
        props: Props,
    ) -> TermId {
        self.term(name, node_name, 0, props)
    }

    pub fn make_repeat(&mut self, name: impl Into<String>) -> TermId {
        self.term(name, None, REPEATED, Props::new())
    }

    #[must_use]
    pub fn unique_name(&self, base: &str) -> String {
        for index in 0.. {
            let candidate = if index == 0 {
                base.to_owned()
            } else {
                format!("{base}-{index}")
            };
            if !self.names.contains_key(&candidate) {
                return candidate;
            }
        }
        unreachable!("unbounded integer sequence always yields a unique name")
    }

    pub fn finish(&mut self, rules: &mut [Rule]) -> Result<FinishedTerms, GeneratorError> {
        for term in &mut self.terms {
            term.rules.clear();
            term.output_id = None;
        }
        for (rule_id, rule) in rules.iter_mut().enumerate() {
            rule.id = rule_id;
            self.terms[rule.name].rules.push(rule_id);
        }

        let mut active = BTreeSet::new();
        for (term_id, term) in self.terms.iter().enumerate() {
            if term.terminal() || term.preserve() {
                active.insert(term_id);
            }
        }
        for rule in rules.iter() {
            active.insert(rule.name);
            active.extend(rule.parts.iter().copied());
        }

        let mut node_types = vec![self.error];
        self.terms[self.error].output_id = Some(0);
        let mut next_id = 1_u32;

        for term_id in active.iter().copied() {
            let term = &mut self.terms[term_id];
            if term.output_id.is_none() && term.node_type() && !term.repeated() {
                term.output_id = Some(checked_term_id(next_id)?);
                next_id += 1;
                node_types.push(term_id);
            }
        }
        let min_repeat_term = checked_term_id(next_id)?;
        for term_id in active.iter().copied() {
            let term = &mut self.terms[term_id];
            if term.repeated() {
                term.output_id = Some(checked_term_id(next_id)?);
                next_id += 1;
                node_types.push(term_id);
            }
        }
        self.terms[self.eof].output_id = Some(checked_term_id(next_id)?);
        next_id += 1;

        let mut term_names = BTreeMap::new();
        for term_id in active {
            let term = &mut self.terms[term_id];
            if term.output_id.is_none() {
                term.output_id = Some(checked_term_id(next_id)?);
                next_id += 1;
            }
            let output_id = term
                .output_id
                .expect("active terms receive an output identifier");
            if !term.name.is_empty() {
                term_names.insert(output_id, term.name.clone());
            }
        }
        if next_id >= 0xfffe {
            return Err(GeneratorError::new("Too many terms", None));
        }
        Ok(FinishedTerms {
            node_types,
            term_names,
            min_repeat_term,
            max_term: checked_term_id(next_id - 1)?,
        })
    }

    #[must_use]
    pub fn output_id(&self, term: TermId) -> u16 {
        self.terms[term]
            .output_id
            .expect("term identifiers are assigned before table emission")
    }
}

fn checked_term_id(value: u32) -> Result<u16, GeneratorError> {
    u16::try_from(value).map_err(|_| GeneratorError::new("Too many terms", None))
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Conflicts {
    pub precedence: i32,
    pub ambiguity_groups: Vec<String>,
    pub cut: i32,
}

impl Conflicts {
    #[must_use]
    pub fn join(&self, other: &Self) -> Self {
        if self == &Self::default() {
            return other.clone();
        }
        if other == &Self::default() || self == other {
            return self.clone();
        }
        Self {
            precedence: self.precedence.max(other.precedence),
            ambiguity_groups: union_sorted(&self.ambiguity_groups, &other.ambiguity_groups),
            cut: self.cut.max(other.cut),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rule {
    pub id: RuleId,
    pub name: TermId,
    pub parts: Vec<TermId>,
    pub conflicts: Vec<Conflicts>,
    pub skip: TermId,
}

impl Rule {
    #[must_use]
    pub fn is_repeat_wrap(&self, terms: &TermSet) -> bool {
        terms.terms[self.name].repeated() && self.parts.len() == 2 && self.parts[0] == self.name
    }

    #[must_use]
    pub fn same_reduce(&self, other: &Self, terms: &TermSet) -> bool {
        self.name == other.name
            && self.parts.len() == other.parts.len()
            && self.is_repeat_wrap(terms) == other.is_repeat_wrap(terms)
    }
}

fn union_sorted<T>(left: &[T], right: &[T]) -> Vec<T>
where
    T: Clone + Ord,
{
    if left.is_empty() {
        return right.to_vec();
    }
    if right.is_empty() {
        return left.to_vec();
    }
    let mut result = left.to_vec();
    for value in right {
        if !result.contains(value) {
            result.push(value.clone());
        }
    }
    result.sort();
    result
}
