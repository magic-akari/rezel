use rezel_common::{TextRange, TextSize};

use super::{PythonAstField, PythonAstKind};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AstNodeId(u32);

impl AstNodeId {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StringId(u32);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PythonStringId(u32);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BytesId(u32);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct PythonSourceRange {
    start: Option<TextSize>,
    end: Option<TextSize>,
}

impl PythonSourceRange {
    #[must_use]
    pub const fn new(start: Option<TextSize>, end: Option<TextSize>) -> Self {
        Self { start, end }
    }

    #[must_use]
    pub const fn start(self) -> Option<TextSize> {
        self.start
    }

    #[must_use]
    pub const fn end(self) -> Option<TextSize> {
        self.end
    }

    #[must_use]
    pub const fn byte_range(self) -> Option<TextRange> {
        match (self.start, self.end) {
            (Some(start), Some(end)) => Some(TextRange::new(start, end)),
            _ => None,
        }
    }
}

impl From<TextRange> for PythonSourceRange {
    fn from(value: TextRange) -> Self {
        Self::new(Some(value.start()), Some(value.end()))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PythonConstant {
    None,
    Bool(bool),
    Integer(StringId),
    Float(u64),
    Complex { real: u64, imaginary: u64 },
    String(PythonStringId),
    Bytes(BytesId),
    Ellipsis,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PythonAstValue {
    None,
    Bool(bool),
    Integer(StringId),
    String(StringId),
    Strings(Vec<StringId>),
    Constant(PythonConstant),
    Node(AstNodeId),
    Nodes(Vec<AstNodeId>),
    OptionalNodes(Vec<Option<AstNodeId>>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PythonAstFieldValue {
    field: PythonAstField,
    value: PythonAstValue,
}

impl PythonAstFieldValue {
    #[must_use]
    pub const fn field(&self) -> PythonAstField {
        self.field
    }

    #[must_use]
    pub const fn value(&self) -> &PythonAstValue {
        &self.value
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PythonAstNode {
    kind: PythonAstKind,
    source_range: PythonSourceRange,
    fields: Vec<PythonAstFieldValue>,
}

impl PythonAstNode {
    #[must_use]
    pub const fn kind(&self) -> PythonAstKind {
        self.kind
    }

    #[must_use]
    pub const fn source_range(&self) -> PythonSourceRange {
        self.source_range
    }

    #[must_use]
    pub fn fields(&self) -> &[PythonAstFieldValue] {
        &self.fields
    }
}

#[derive(Debug)]
pub struct PythonAst {
    pub(super) nodes: Vec<PythonAstNode>,
    pub(super) strings: Vec<String>,
    pub(super) python_strings: Vec<Vec<u32>>,
    pub(super) bytes: Vec<Vec<u8>>,
    pub(super) root: AstNodeId,
}

impl PythonAst {
    #[must_use]
    pub fn root(&self) -> &PythonAstNode {
        &self.nodes[self.root.index()]
    }

    #[must_use]
    pub const fn root_id(&self) -> AstNodeId {
        self.root
    }

    #[must_use]
    pub fn nodes(&self) -> &[PythonAstNode] {
        &self.nodes
    }

    #[must_use]
    pub fn node(&self, id: AstNodeId) -> Option<&PythonAstNode> {
        self.nodes.get(id.index())
    }

    #[must_use]
    pub fn string(&self, id: StringId) -> Option<&str> {
        self.strings.get(id.0 as usize).map(String::as_str)
    }

    #[must_use]
    pub fn python_string(&self, id: PythonStringId) -> Option<&[u32]> {
        self.python_strings.get(id.0 as usize).map(Vec::as_slice)
    }

    #[must_use]
    pub fn bytes(&self, id: BytesId) -> Option<&[u8]> {
        self.bytes.get(id.0 as usize).map(Vec::as_slice)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PythonAstOptions {
    pub type_comments: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AstError {
    RecoveryTree,
    SourceTooLarge,
    UnsupportedSyntax {
        kind: String,
    },
    InvalidLiteral {
        kind: &'static str,
        spelling: String,
    },
    InvalidSyntax {
        position: TextSize,
        message: &'static str,
    },
    InconsistentCst {
        context: &'static str,
        expected: &'static str,
    },
    IndexOverflow,
}

impl std::fmt::Display for AstError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RecoveryTree => formatter.write_str("cannot lower a recovery tree"),
            Self::SourceTooLarge => formatter.write_str("Python source is too large"),
            Self::UnsupportedSyntax { kind } => {
                write!(
                    formatter,
                    "Python AST lowering does not support CST kind {kind}"
                )
            }
            Self::InvalidLiteral { kind, spelling } => {
                write!(formatter, "invalid Python {kind} literal {spelling:?}")
            }
            Self::InvalidSyntax { position, message } => {
                write!(
                    formatter,
                    "invalid Python syntax at byte {}: {message}",
                    usize::from(*position)
                )
            }
            Self::InconsistentCst { context, expected } => {
                write!(formatter, "inconsistent {context}; expected {expected}")
            }
            Self::IndexOverflow => formatter.write_str("Python AST arena index overflow"),
        }
    }
}

impl std::error::Error for AstError {}

pub(super) struct AstBuilder {
    nodes: Vec<PythonAstNode>,
    strings: Vec<String>,
    python_strings: Vec<Vec<u32>>,
    bytes: Vec<Vec<u8>>,
}

impl AstBuilder {
    pub(super) const fn new() -> Self {
        Self {
            nodes: Vec::new(),
            strings: Vec::new(),
            python_strings: Vec::new(),
            bytes: Vec::new(),
        }
    }

    pub(super) fn push_node(
        &mut self,
        kind: PythonAstKind,
        range: PythonSourceRange,
    ) -> Result<AstNodeId, AstError> {
        let raw = u32::try_from(self.nodes.len()).map_err(|_| AstError::IndexOverflow)?;
        self.nodes.push(PythonAstNode {
            kind,
            source_range: range,
            fields: Vec::new(),
        });
        Ok(AstNodeId(raw))
    }

    pub(super) fn push_field(
        &mut self,
        node: AstNodeId,
        field: PythonAstField,
        value: PythonAstValue,
    ) -> Result<(), AstError> {
        self.nodes
            .get_mut(node.index())
            .ok_or(AstError::IndexOverflow)?
            .fields
            .push(PythonAstFieldValue { field, value });
        Ok(())
    }

    pub(super) fn intern(&mut self, value: &str) -> Result<StringId, AstError> {
        if let Some(index) = self.strings.iter().position(|candidate| candidate == value) {
            return Ok(StringId(
                u32::try_from(index).map_err(|_| AstError::IndexOverflow)?,
            ));
        }
        let index = u32::try_from(self.strings.len()).map_err(|_| AstError::IndexOverflow)?;
        self.strings.push(value.to_owned());
        Ok(StringId(index))
    }

    pub(super) fn intern_python_string(
        &mut self,
        value: &[u32],
    ) -> Result<PythonStringId, AstError> {
        if let Some(index) = self
            .python_strings
            .iter()
            .position(|candidate| candidate == value)
        {
            return Ok(PythonStringId(
                u32::try_from(index).map_err(|_| AstError::IndexOverflow)?,
            ));
        }
        let index =
            u32::try_from(self.python_strings.len()).map_err(|_| AstError::IndexOverflow)?;
        self.python_strings.push(value.to_owned());
        Ok(PythonStringId(index))
    }

    pub(super) fn intern_bytes(&mut self, value: &[u8]) -> Result<BytesId, AstError> {
        if let Some(index) = self.bytes.iter().position(|candidate| candidate == value) {
            return Ok(BytesId(
                u32::try_from(index).map_err(|_| AstError::IndexOverflow)?,
            ));
        }
        let index = u32::try_from(self.bytes.len()).map_err(|_| AstError::IndexOverflow)?;
        self.bytes.push(value.to_owned());
        Ok(BytesId(index))
    }

    pub(super) fn finish(self, root: AstNodeId) -> PythonAst {
        PythonAst {
            nodes: self.nodes,
            strings: self.strings,
            python_strings: self.python_strings,
            bytes: self.bytes,
            root,
        }
    }
}
