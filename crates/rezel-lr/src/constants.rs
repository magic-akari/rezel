/// One encoded LR shift or reduction action.
///
/// Actions are bit fields rather than a closed enum: the low 16 bits contain
/// a generated state or term id, and reduction actions additionally encode a
/// stack depth. Keeping the raw representation behind this transparent type
/// makes those semantics explicit without changing the generated table ABI.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Action(u32);

impl Action {
    const REDUCE_FLAG: u32 = 1 << 16;
    const REPEAT_OR_GOTO_FLAG: u32 = 1 << 17;
    const STAY_FLAG: u32 = 1 << 18;
    const REDUCE_DEPTH_SHIFT: u32 = 19;
    const VALUE_MASK: u32 = u16::MAX as u32;

    /// The sentinel used when no action is available.
    pub const NONE: Self = Self(0);

    /// Wrap one action read from a generated table.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// Return the generated table representation.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Construct one shift action.
    #[must_use]
    pub const fn shift(state: u16, goto: bool, stay: bool) -> Self {
        let mut raw = state as u32;
        if goto {
            raw |= Self::REPEAT_OR_GOTO_FLAG;
        }
        if stay {
            raw |= Self::STAY_FLAG;
        }
        Self(raw)
    }

    /// Construct one reduction action.
    #[must_use]
    pub const fn reduce(term: u16, depth: u32, repeat: bool, stay: bool) -> Self {
        let mut raw = Self::REDUCE_FLAG | term as u32 | (depth << Self::REDUCE_DEPTH_SHIFT);
        if repeat {
            raw |= Self::REPEAT_OR_GOTO_FLAG;
        }
        if stay {
            raw |= Self::STAY_FLAG;
        }
        Self(raw)
    }

    /// Whether this is the no-action sentinel.
    #[must_use]
    pub const fn is_none(self) -> bool {
        self.0 == 0
    }

    /// Whether this action reduces a production.
    #[must_use]
    pub const fn is_reduce(self) -> bool {
        self.0 & Self::REDUCE_FLAG != 0
    }

    /// Whether this reduction combines repeat-term instances.
    #[must_use]
    pub const fn is_repeat_reduction(self) -> bool {
        self.is_reduce() && self.0 & Self::REPEAT_OR_GOTO_FLAG != 0
    }

    /// Whether this shift enters a skip rule without consuming a token.
    #[must_use]
    pub const fn is_goto_shift(self) -> bool {
        !self.is_reduce() && self.0 & Self::REPEAT_OR_GOTO_FLAG != 0
    }

    /// Whether this action retains the current state.
    #[must_use]
    pub const fn is_stay(self) -> bool {
        self.0 & Self::STAY_FLAG != 0
    }

    /// Return the shifted state or reduced term id.
    #[must_use]
    pub const fn value(self) -> u16 {
        (self.0 & Self::VALUE_MASK) as u16
    }

    /// Return the stack depth encoded by a reduction.
    #[must_use]
    pub const fn reduction_depth(self) -> usize {
        (self.0 >> Self::REDUCE_DEPTH_SHIFT) as usize
    }
}

/// One flag stored in a generated LR state record.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateFlag {
    /// Nodes produced in this state belong outside the following reduction.
    Skipped = 1,
    /// This state accepts its grammar entry point.
    Accepting = 2,
}

impl StateFlag {
    #[must_use]
    pub const fn mask(self) -> u32 {
        self as u32
    }
}

/// One field in a generated LR state record.
#[repr(usize)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateField {
    Flags = 0,
    Actions = 1,
    Skip = 2,
    TokenizerMask = 3,
    DefaultReduce = 4,
    ForcedReduce = 5,
}

impl StateField {
    /// Number of compact words in one state record.
    pub const COUNT: usize = 6;

    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Reserved control values embedded in compact `u16` sequences.
#[repr(u16)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SequenceCode {
    Done = 0,
    Next = 1,
    Other = 2,
    End = u16::MAX,
}

/// Goto-group tag reserved for delta-compressed source-state lists.
///
/// The low bit remains the end-of-term marker. All other tag values retain
/// the legacy `source_count << 1` representation, including empty groups.
pub const GOTO_COMPRESSED_TAG: u16 = 1 << 15;

/// Goto-table marker for a delta-compressed term header.
pub const GOTO_COMPRESSED_HEADER: u16 = u16::MAX;

impl SequenceCode {
    #[must_use]
    pub const fn raw(self) -> u16 {
        self as u16
    }
}

/// Terms reserved by the LR runtime.
#[repr(u16)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReservedTerm {
    Error = 0,
}

impl ReservedTerm {
    #[must_use]
    pub const fn raw(self) -> u16 {
        self as u16
    }
}
