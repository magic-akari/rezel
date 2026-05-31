use std::sync::Arc;

use rezel_common::{ParseError, ParseErrorKind, PostfixBuffer, PostfixCursor, TextSize};

use crate::parse::{ContextTracker, ContextValue, LargeReductionTracker, ParserCore};
use crate::table::{Action, ReservedTerm, SequenceCode, StateField, StateFlag};
use crate::token::InputStream;

/// Score penalties applied by the three stack recovery strategies.
#[repr(i32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecoveryPenalty {
    Insert = 200,
    Delete = 190,
    Reduce = 100,
}

/// Fixed policy used by stack-local recovery operations.
struct RecoveryPolicy;

impl RecoveryPolicy {
    const MAX_NEXT: usize = 4;
    const MAX_INSERT_DEPTH: usize = 300;
    const DAMPEN_INSERT_DEPTH: usize = 120;
    const MIN_LARGE_REDUCTION_SPAN: TextSize = TextSize::new(2_000);
}

// Eight postfix records, with four `u32` fields per record.
const MIN_BRANCH_BUFFER_CAPACITY: usize = 8 * 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Frame {
    state: u16,
    start: TextSize,
    buffer_offset: usize,
}

#[derive(Clone)]
struct StackContext {
    tracker: &'static ContextTracker,
    value: ContextValue,
    hash: u64,
}

impl std::fmt::Debug for StackContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StackContext")
            .field("value", &self.value)
            .field("hash", &self.hash)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
struct BufferChunk {
    base: usize,
    data: Vec<u32>,
    parent: Option<Arc<Self>>,
}

/// Backward cursor over one stack's shared postfix-buffer chain.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct StackBufferCursor<'a> {
    buffer: &'a [u32],
    base: usize,
    index: usize,
    position: usize,
    parent: Option<&'a BufferChunk>,
}

impl<'a> StackBufferCursor<'a> {
    fn new(stack: &'a Stack) -> Self {
        let mut cursor = Self {
            buffer: &stack.buffer,
            base: stack.buffer_base,
            index: stack.buffer.len(),
            position: stack.buffer_base + stack.buffer.len(),
            parent: stack.parent_buffer.as_deref(),
        };
        cursor.move_to_parent();
        cursor
    }

    fn move_to_parent(&mut self) {
        if self.index != 0 {
            return;
        }
        let Some(parent) = self.parent else {
            return;
        };
        self.index = self
            .base
            .checked_sub(parent.base)
            .expect("postfix parent begins after its child");
        debug_assert_eq!(self.index, parent.data.len());
        self.buffer = &parent.data;
        self.base = parent.base;
        self.parent = parent.parent.as_deref();
    }
}

impl PostfixCursor for StackBufferCursor<'_> {
    fn id(&self) -> u16 {
        self.buffer[self.index - 4]
            .try_into()
            .expect("node type ids must fit in 16 bits")
    }

    fn start(&self) -> TextSize {
        TextSize::from(self.buffer[self.index - 3])
    }

    fn end(&self) -> TextSize {
        TextSize::from(self.buffer[self.index - 2])
    }

    fn size(&self) -> usize {
        self.buffer[self.index - 1] as usize
    }

    fn position(&self) -> usize {
        self.position
    }

    fn next(&mut self) {
        self.index = self
            .index
            .checked_sub(4)
            .expect("postfix cursor moved before its buffer");
        self.position = self
            .position
            .checked_sub(4)
            .expect("postfix cursor moved before the stack");
        self.move_to_parent();
    }
}

/// One live LR/GLR parse stack.
#[derive(Clone)]
pub struct Stack {
    core: Arc<ParserCore>,
    frames: Vec<Frame>,
    state: u16,
    reduce_position: TextSize,
    position: TextSize,
    score: i32,
    buffer: Vec<u32>,
    buffer_base: usize,
    // Absolute base at which this logical branch started writing. Physical
    // prefix chunks may be frozen later, but Lezer's pruning policy ranks
    // branches by the equivalent local-buffer length.
    branch_buffer_base: usize,
    parent_buffer: Option<Arc<BufferChunk>>,
    context: Option<StackContext>,
    parse_start: TextSize,
}

impl std::fmt::Debug for Stack {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let states = self
            .frames
            .iter()
            .map(|frame| frame.state)
            .chain(std::iter::once(self.state))
            .collect::<Vec<_>>();
        formatter
            .debug_struct("Stack")
            .field("states", &states)
            .field("position", &self.position)
            .field("score", &self.score)
            .field("buffer_records", &(self.buffer.len() / 4))
            .finish_non_exhaustive()
    }
}

impl Stack {
    pub(crate) fn start(core: Arc<ParserCore>, state: u16, position: TextSize) -> Self {
        let context = core.context.map(|tracker| {
            let value = tracker.start();
            let hash = tracker.hash(&value);
            StackContext {
                tracker,
                value,
                hash,
            }
        });
        Self {
            core,
            frames: Vec::new(),
            state,
            reduce_position: position,
            position,
            score: 0,
            buffer: Vec::new(),
            buffer_base: 0,
            branch_buffer_base: 0,
            parent_buffer: None,
            context,
            parse_start: position,
        }
    }

    /// Current LR state.
    #[must_use]
    pub const fn state(&self) -> u16 {
        self.state
    }

    /// Input byte position reached by this stack.
    #[must_use]
    pub const fn position(&self) -> TextSize {
        self.position
    }

    /// Dynamic precedence and recovery score.
    #[must_use]
    pub const fn score(&self) -> i32 {
        self.score
    }

    /// Borrow the typed tracker context.
    #[must_use]
    pub fn context<T>(&self) -> Option<&T>
    where
        T: Send + Sync + 'static,
    {
        self.context.as_ref()?.value.downcast_ref()
    }

    /// Check whether a term can shift after zero or more reductions.
    #[must_use]
    pub fn can_shift(&self, term: u16) -> bool {
        let mut simulation = SimulatedStack::new(self);
        loop {
            let default = Action::from_raw(
                self.core
                    .state_slot(simulation.state, StateField::DefaultReduce),
            );
            let action = if default.is_none() {
                self.core.has_action(simulation.state, term)
            } else {
                default
            };
            if action.is_none() {
                return false;
            }
            if !action.is_reduce() {
                return true;
            }
            let Some(next) = simulation.reduce(action) else {
                return false;
            };
            simulation = next;
        }
    }

    /// Whether one generated dialect id is enabled.
    #[must_use]
    pub fn dialect_enabled(&self, dialect: usize) -> bool {
        self.core.dialect.enabled(dialect)
    }

    pub(crate) fn core(&self) -> &Arc<ParserCore> {
        &self.core
    }

    pub(crate) fn context_hash(&self) -> u64 {
        self.context.as_ref().map_or(0, |context| context.hash)
    }

    pub(crate) const fn reduce_position(&self) -> TextSize {
        self.reduce_position
    }

    pub(crate) fn depth(&self) -> usize {
        self.frames.len()
    }

    pub(crate) fn top_frame_start(&self) -> Option<TextSize> {
        self.frames.last().map(|frame| frame.start)
    }

    pub(crate) fn buffer_len(&self) -> usize {
        self.buffer_base + self.buffer.len() - self.branch_buffer_base
    }

    fn push_state(&mut self, state: u16, start: TextSize) -> Result<(), ParseError> {
        if self.frames.len() >= self.core.limits.max_stack_depth {
            return Err(ParseError::new(
                ParseErrorKind::ResourceLimit,
                Some(self.position),
                "parser stack depth limit exceeded",
            ));
        }
        self.frames.push(Frame {
            state: self.state,
            start,
            buffer_offset: self.buffer_base + self.buffer.len(),
        });
        self.state = state;
        Ok(())
    }

    pub(crate) fn reduce(
        &mut self,
        action: Action,
        input: &mut InputStream,
        large_reductions: &mut LargeReductionTracker,
    ) -> Result<(), ParseError> {
        let depth = action.reduction_depth();
        let term = action.value();
        let is_node = term < self.core.min_repeat_term();
        let dynamic_precedence = self.core.dynamic_precedence(term);
        if dynamic_precedence != 0 {
            self.score += dynamic_precedence;
        }

        if depth == 0 {
            if is_node && self.reduce_position < self.position {
                self.reduce_position = self.position;
            }
            let goto = self
                .core
                .get_goto(self.state, term, true)
                .ok_or_else(|| configuration_error(self.position, "missing zero-depth goto"))?;
            self.push_state(goto, self.reduce_position)?;
            if is_node {
                self.store_node(term, self.reduce_position, self.reduce_position, 4, true)?;
            }
            self.reduce_context(term, self.reduce_position, input)?;
            return Ok(());
        }

        let stays = action.is_stay();
        let stay_depth = usize::from(stays) * 2;
        let consumed_frames = depth.saturating_sub(1) + stay_depth;
        let base = self
            .frames
            .len()
            .checked_sub(consumed_frames)
            .ok_or_else(|| {
                configuration_error(self.position, "reduction exceeds the parser stack")
            })?;
        let start = if base == 0 {
            self.parse_start
        } else {
            self.frames[base - 1].start
        };
        if is_node && start == self.reduce_position && self.reduce_position < self.position {
            self.reduce_position = self.position;
        }
        let reduction_size = self
            .reduce_position
            .checked_sub(start)
            .unwrap_or(TextSize::new(0));
        if reduction_size >= RecoveryPolicy::MIN_LARGE_REDUCTION_SPAN {
            let anonymous = self
                .core
                .node_set
                .get(term)
                .is_some_and(rezel_common::NodeType::is_anonymous);
            if !anonymous {
                large_reductions.observe(start, reduction_size);
            }
        }
        let buffer_base = if base == 0 {
            0
        } else {
            self.frames[base - 1].buffer_offset
        };
        let buffer_end = self.buffer_base + self.buffer.len();
        let count = buffer_end.checked_sub(buffer_base).ok_or_else(|| {
            configuration_error(self.position, "reduction buffer base is invalid")
        })?;

        if is_node || action.is_repeat_reduction() {
            let end = if self.core.state_flag(self.state, StateFlag::Skipped) {
                self.position
            } else {
                self.reduce_position
            };
            self.store_node(term, start, end, count + 4, true)?;
        }
        if stays {
            self.state = self
                .frames
                .get(base)
                .ok_or_else(|| {
                    configuration_error(self.position, "stay reduction has no retained state")
                })?
                .state;
        } else {
            let base_state = self
                .frames
                .get(base.wrapping_sub(1))
                .ok_or_else(|| {
                    configuration_error(self.position, "reduction has no goto base state")
                })?
                .state;
            self.state = self
                .core
                .get_goto(base_state, term, true)
                .ok_or_else(|| configuration_error(self.position, "missing reduction goto"))?;
        }
        self.frames.truncate(base);
        self.reduce_context(term, start, input)
    }

    fn store_node(
        &mut self,
        term: u16,
        start: TextSize,
        end: TextSize,
        mut size: usize,
        must_sink: bool,
    ) -> Result<(), ParseError> {
        if (self.buffer_base + self.buffer.len()) / 4 >= self.core.limits.max_buffer_records {
            return Err(ParseError::new(
                ParseErrorKind::ResourceLimit,
                Some(start),
                "parser CST record limit exceeded",
            ));
        }
        if term == ReservedTerm::Error.raw() {
            let top = self.buffer_base + self.buffer.len();
            let error_is_above_stack = self
                .frames
                .last()
                .is_some_and(|frame| frame.buffer_offset < top);
            let local_top = self.buffer.len();
            if (self.frames.is_empty() || error_is_above_stack)
                && local_top >= 4
                && self.buffer[local_top - 4] == u32::from(ReservedTerm::Error.raw())
            {
                if start == end {
                    return Ok(());
                }
                if self.buffer[local_top - 2] >= u32::from(start) {
                    self.buffer[local_top - 2] = u32::from(end);
                    return Ok(());
                }
            }
        }
        if !must_sink || self.position == end {
            self.push_record(term, start, end, size);
            return Ok(());
        }

        let mut index = self.buffer.len();
        let previous_is_error =
            index >= 4 && self.buffer[index - 4] == u32::from(ReservedTerm::Error.raw());
        if !previous_is_error {
            let must_move = (0..index)
                .step_by(4)
                .rev()
                .take_while(|offset| self.buffer[*offset + 2] > u32::from(end))
                .any(|offset| self.buffer[offset + 3] > 0);
            if must_move {
                self.buffer.resize(index + 4, 0);
                while index > 0 && self.buffer[index - 2] > u32::from(end) {
                    self.buffer.copy_within((index - 4)..index, index);
                    index -= 4;
                    if size > 4 {
                        size -= 4;
                    }
                }
                write_record(&mut self.buffer[index..index + 4], term, start, end, size);
                return Ok(());
            }
        }
        self.push_record(term, start, end, size);
        Ok(())
    }

    // `LRParser::with_limits` bounds the complete postfix buffer to `u32`.
    #[allow(clippy::cast_possible_truncation)]
    fn push_record(&mut self, term: u16, start: TextSize, end: TextSize, size: usize) {
        debug_assert!(u32::try_from(size).is_ok());
        if self.buffer.is_empty()
            && self.parent_buffer.is_some()
            && self.buffer.capacity() < MIN_BRANCH_BUFFER_CAPACITY
        {
            // A GLR split freezes the shared prefix and leaves each branch
            // with an empty mutable tail. Allocate only when the branch
            // actually writes, but avoid regrowing that short tail one record
            // at a time.
            self.buffer.reserve_exact(MIN_BRANCH_BUFFER_CAPACITY);
        }
        let record = [
            u32::from(term),
            u32::from(start),
            u32::from(end),
            size as u32,
        ];
        self.buffer.extend_from_slice(&record);
    }

    fn shift(
        &mut self,
        action: Action,
        term: u16,
        start: TextSize,
        end: TextSize,
        input: &mut InputStream,
    ) -> Result<(), ParseError> {
        if action.is_goto_shift() {
            return self.push_state(action.value(), self.position);
        }
        let is_node = term <= self.core.max_node();
        if !action.is_stay() {
            let next_state = action.value();
            self.position = end;
            let skipped = self.core.state_flag(next_state, StateFlag::Skipped);
            if !skipped && (end > start || is_node) {
                self.reduce_position = end;
            }
            self.push_state(
                next_state,
                if skipped {
                    start
                } else {
                    start.min(self.reduce_position)
                },
            )?;
            self.shift_context(term, start, input)?;
            if is_node {
                self.push_record(term, start, end, 4);
            }
            return Ok(());
        }

        self.position = end;
        self.shift_context(term, start, input)?;
        if is_node {
            self.push_record(term, start, end, 4);
        }
        Ok(())
    }

    pub(crate) fn apply(
        &mut self,
        action: Action,
        term: u16,
        start: TextSize,
        end: TextSize,
        input: &mut InputStream,
        large_reductions: &mut LargeReductionTracker,
    ) -> Result<(), ParseError> {
        if action.is_reduce() {
            self.reduce(action, input, large_reductions)
        } else {
            self.shift(action, term, start, end, input)
        }
    }

    pub(crate) fn split(&mut self) -> Self {
        let mut offset = self.buffer.len();
        if offset >= 4 && self.buffer[offset - 4] == u32::from(ReservedTerm::Error.raw()) {
            offset -= 4;
        }
        while offset > 0 && self.buffer[offset - 2] > u32::from(self.reduce_position) {
            offset -= 4;
        }

        if offset > 0 {
            let mut prefix = std::mem::take(&mut self.buffer);
            let suffix = prefix.split_off(offset);
            let parent = Arc::new(BufferChunk {
                base: self.buffer_base,
                data: prefix,
                parent: self.parent_buffer.clone(),
            });
            self.buffer_base += offset;
            self.parent_buffer = Some(parent);
            self.buffer = suffix;
        }
        Self {
            core: Arc::clone(&self.core),
            frames: self.frames.clone(),
            state: self.state,
            reduce_position: self.reduce_position,
            position: self.position,
            score: self.score,
            buffer: self.buffer.clone(),
            buffer_base: self.buffer_base,
            branch_buffer_base: self.buffer_base,
            parent_buffer: self.parent_buffer.clone(),
            context: self.context.clone(),
            parse_start: self.parse_start,
        }
    }

    pub(crate) fn recover_by_delete(&mut self, term: u16, end: TextSize) -> Result<(), ParseError> {
        let is_node = term <= self.core.max_node();
        if is_node {
            self.store_node(term, self.position, end, 4, false)?;
        }
        self.store_node(
            ReservedTerm::Error.raw(),
            self.position,
            end,
            if is_node { 8 } else { 4 },
            false,
        )?;
        self.position = end;
        self.reduce_position = end;
        self.score -= RecoveryPenalty::Delete as i32;
        Ok(())
    }

    pub(crate) fn recover_by_insert(
        &mut self,
        next: u16,
        input: &mut InputStream,
    ) -> Result<Vec<Self>, ParseError> {
        if self.frames.len() >= RecoveryPolicy::MAX_INSERT_DEPTH {
            return Ok(Vec::new());
        }
        let mut next_states = self.core.next_states(self.state);
        if next_states.len() > RecoveryPolicy::MAX_NEXT * 2
            || self.frames.len() >= RecoveryPolicy::DAMPEN_INSERT_DEPTH
        {
            let mut best = Vec::new();
            for (term, state) in &next_states {
                if *state != self.state && !self.core.has_action(*state, next).is_none() {
                    best.push((*term, *state));
                }
            }
            if self.frames.len() < RecoveryPolicy::DAMPEN_INSERT_DEPTH {
                for (term, state) in &next_states {
                    if best.len() >= RecoveryPolicy::MAX_NEXT * 2 {
                        break;
                    }
                    if !best.iter().any(|(_, existing)| existing == state) {
                        best.push((*term, *state));
                    }
                }
            }
            next_states = best;
        }
        let mut result = Vec::new();
        for (term, state) in next_states {
            if result.len() >= RecoveryPolicy::MAX_NEXT || state == self.state {
                continue;
            }
            let mut stack = self.split();
            stack.push_state(state, self.position)?;
            stack.store_node(
                ReservedTerm::Error.raw(),
                stack.position,
                stack.position,
                4,
                true,
            )?;
            stack.shift_context(term, self.position, input)?;
            stack.reduce_position = self.position;
            stack.score -= RecoveryPenalty::Insert as i32;
            result.push(stack);
        }
        Ok(result)
    }

    pub(crate) fn force_reduce(
        &mut self,
        input: &mut InputStream,
        actions: &mut usize,
        large_reductions: &mut LargeReductionTracker,
    ) -> Result<bool, ParseError> {
        let mut reduction =
            Action::from_raw(self.core.state_slot(self.state, StateField::ForcedReduce));
        if !reduction.is_reduce() {
            return Ok(false);
        }
        if !self.core.valid_action(self.state, reduction) {
            let depth = reduction.reduction_depth();
            let term = reduction.value();
            let target = self.frames.len().checked_sub(depth);
            let invalid = target
                .and_then(|target| self.frames.get(target))
                .is_none_or(|frame| self.core.get_goto(frame.state, term, false).is_none());
            if invalid {
                let Some(backup) = self.find_forced_reduction() else {
                    return Ok(false);
                };
                reduction = backup;
            }
            self.store_node(
                ReservedTerm::Error.raw(),
                self.position,
                self.position,
                4,
                true,
            )?;
            self.score -= RecoveryPenalty::Reduce as i32;
        }
        *actions += 1;
        if *actions > self.core.limits.max_actions {
            return Err(ParseError::new(
                ParseErrorKind::ResourceLimit,
                Some(self.position),
                "parser action budget exhausted",
            ));
        }
        self.reduce_position = self.position;
        self.reduce(reduction, input, large_reductions)?;
        Ok(true)
    }

    fn find_forced_reduction(&self) -> Option<Action> {
        let mut seen = Vec::new();
        self.explore_forced_reduction(self.state, 0, &mut seen)
    }

    fn explore_forced_reduction(
        &self,
        state: u16,
        depth: usize,
        seen: &mut Vec<u16>,
    ) -> Option<Action> {
        if seen.contains(&state) {
            return None;
        }
        seen.push(state);
        let mut actions = Vec::new();
        let _ = self.core.all_actions(state, |action| {
            actions.push(action);
            None::<()>
        });
        for action in actions {
            if action.is_stay() || action.is_goto_shift() {
                continue;
            }
            if action.is_reduce() {
                let reduction_depth = action.reduction_depth();
                let relative_depth = reduction_depth.saturating_sub(depth);
                if relative_depth > 1 {
                    let term = action.value();
                    let target = self.frames.len().checked_sub(relative_depth);
                    if target
                        .and_then(|target| self.frames.get(target))
                        .is_some_and(|frame| self.core.get_goto(frame.state, term, false).is_some())
                    {
                        let depth = u32::try_from(relative_depth).ok()?;
                        return Some(Action::reduce(term, depth, false, false));
                    }
                }
            } else if let Some(found) =
                self.explore_forced_reduction(action.value(), depth + 1, seen)
            {
                return Some(found);
            }
        }
        None
    }

    pub(crate) fn force_all(
        mut self,
        input: &mut InputStream,
        actions: &mut usize,
        large_reductions: &mut LargeReductionTracker,
    ) -> Result<Self, ParseError> {
        while !self.core.state_flag(self.state, StateFlag::Accepting) {
            if !self.force_reduce(input, actions, large_reductions)? {
                self.store_node(
                    ReservedTerm::Error.raw(),
                    self.position,
                    self.position,
                    4,
                    true,
                )?;
                break;
            }
        }
        Ok(self)
    }

    pub(crate) fn dead_end(&self) -> bool {
        if self.frames.len() != 1 {
            return false;
        }
        let action_offset = self.core.state_slot(self.state, StateField::Actions) as usize;
        let default_reduce =
            Action::from_raw(self.core.state_slot(self.state, StateField::DefaultReduce));
        self.core.language.state_data[action_offset] == SequenceCode::End.raw()
            && default_reduce.is_none()
    }

    pub(crate) fn restart(&mut self) -> Result<(), ParseError> {
        self.store_node(
            ReservedTerm::Error.raw(),
            self.position,
            self.position,
            4,
            true,
        )?;
        self.state = self.frames[0].state;
        self.frames.clear();
        Ok(())
    }

    pub(crate) fn same_state(&self, other: &Self) -> bool {
        self.state == other.state
            && self.frames.len() == other.frames.len()
            && self
                .frames
                .iter()
                .zip(&other.frames)
                .all(|(left, right)| left.state == right.state)
    }

    pub(crate) fn dialect_allows(&self, term: u16) -> bool {
        self.core.dialect.allows(term)
    }

    fn shift_context(
        &mut self,
        term: u16,
        start: TextSize,
        input: &mut InputStream,
    ) -> Result<(), ParseError> {
        let Some(context) = self.context.clone() else {
            return Ok(());
        };
        input.reset(start);
        let value = context.tracker.shift(&context.value, term, self, input)?;
        if context.value.same_identity(&value) {
            return Ok(());
        }
        let hash = context.tracker.hash(&value);
        self.context = Some(StackContext {
            tracker: context.tracker,
            value,
            hash,
        });
        Ok(())
    }

    fn reduce_context(
        &mut self,
        term: u16,
        start: TextSize,
        input: &mut InputStream,
    ) -> Result<(), ParseError> {
        let Some(context) = self.context.as_ref() else {
            return Ok(());
        };
        if !context.tracker.tracks_reductions() {
            return Ok(());
        }
        let context = context.clone();
        input.reset(start);
        let value = context.tracker.reduce(&context.value, term, self, input)?;
        if context.value.same_identity(&value) {
            return Ok(());
        }
        let hash = context.tracker.hash(&value);
        self.context = Some(StackContext {
            tracker: context.tracker,
            value,
            hash,
        });
        Ok(())
    }
}

impl PostfixBuffer for Stack {
    type Cursor<'a> = StackBufferCursor<'a>;

    fn postfix_cursor(&self) -> Self::Cursor<'_> {
        StackBufferCursor::new(self)
    }
}

struct SimulatedStack<'a> {
    start: &'a Stack,
    state: u16,
    frame_count: usize,
    owned_frames: Option<Vec<Frame>>,
}

impl<'a> SimulatedStack<'a> {
    fn new(start: &'a Stack) -> Self {
        Self {
            start,
            state: start.state,
            frame_count: start.frames.len(),
            owned_frames: None,
        }
    }

    fn reduce(mut self, action: Action) -> Option<Self> {
        let term = action.value();
        let depth = action.reduction_depth();
        if depth == 0 {
            let frames = self
                .owned_frames
                .get_or_insert_with(|| self.start.frames[..self.frame_count].to_vec());
            frames.push(Frame {
                state: self.state,
                start: TextSize::from(0),
                buffer_offset: 0,
            });
            self.frame_count += 1;
        } else {
            self.frame_count = self.frame_count.checked_sub(depth.saturating_sub(1))?;
            if let Some(frames) = &mut self.owned_frames {
                frames.truncate(self.frame_count);
            }
        }
        let base = match &self.owned_frames {
            Some(frames) => frames.last()?.state,
            None => {
                self.start
                    .frames
                    .get(self.frame_count.checked_sub(1)?)?
                    .state
            }
        };
        self.state = self.start.core.get_goto(base, term, true)?;
        Some(self)
    }
}

// `LRParser::with_limits` bounds the complete postfix buffer to `u32`.
#[allow(clippy::cast_possible_truncation)]
fn write_record(record: &mut [u32], term: u16, start: TextSize, end: TextSize, size: usize) {
    debug_assert!(u32::try_from(size).is_ok());
    record[0] = u32::from(term);
    record[1] = u32::from(start);
    record[2] = u32::from(end);
    record[3] = size as u32;
}

fn configuration_error(position: TextSize, message: &'static str) -> ParseError {
    ParseError::new(ParseErrorKind::Configuration, Some(position), message)
}
