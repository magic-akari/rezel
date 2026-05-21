use std::collections::HashMap;

use rezel_common::{TextRange, TextSize};

macro_rules! go_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $($variant:ident => $value:literal),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        #[non_exhaustive]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            /// Public Go spelling.
            #[must_use]
            pub const fn go_name(self) -> &'static str {
                match self {
                    $(Self::$variant => $value),+
                }
            }
        }
    };
}

go_enum! {
    /// Public Go 1.26 `go/ast` concrete node represented by an AST node.
    pub enum GoAstKind {
        ArrayType => "ArrayType",
        AssignStmt => "AssignStmt",
        BasicLit => "BasicLit",
        BinaryExpr => "BinaryExpr",
        BlockStmt => "BlockStmt",
        BranchStmt => "BranchStmt",
        CallExpr => "CallExpr",
        CaseClause => "CaseClause",
        ChanType => "ChanType",
        CommClause => "CommClause",
        Comment => "Comment",
        CommentGroup => "CommentGroup",
        CompositeLit => "CompositeLit",
        DeclStmt => "DeclStmt",
        DeferStmt => "DeferStmt",
        Ellipsis => "Ellipsis",
        EmptyStmt => "EmptyStmt",
        ExprStmt => "ExprStmt",
        Field => "Field",
        FieldList => "FieldList",
        File => "File",
        ForStmt => "ForStmt",
        FuncDecl => "FuncDecl",
        FuncLit => "FuncLit",
        FuncType => "FuncType",
        GenDecl => "GenDecl",
        GoStmt => "GoStmt",
        Ident => "Ident",
        IfStmt => "IfStmt",
        ImportSpec => "ImportSpec",
        IncDecStmt => "IncDecStmt",
        IndexExpr => "IndexExpr",
        IndexListExpr => "IndexListExpr",
        InterfaceType => "InterfaceType",
        KeyValueExpr => "KeyValueExpr",
        LabeledStmt => "LabeledStmt",
        MapType => "MapType",
        ParenExpr => "ParenExpr",
        RangeStmt => "RangeStmt",
        ReturnStmt => "ReturnStmt",
        SelectStmt => "SelectStmt",
        SelectorExpr => "SelectorExpr",
        SendStmt => "SendStmt",
        SliceExpr => "SliceExpr",
        StarExpr => "StarExpr",
        StructType => "StructType",
        SwitchStmt => "SwitchStmt",
        TypeAssertExpr => "TypeAssertExpr",
        TypeSpec => "TypeSpec",
        TypeSwitchStmt => "TypeSwitchStmt",
        UnaryExpr => "UnaryExpr",
        ValueSpec => "ValueSpec",
    }
}

go_enum! {
    /// Ordered public field on a Go 1.26 `go/ast` concrete struct.
    pub enum GoAstField {
        Args => "Args",
        Arrow => "Arrow",
        Assign => "Assign",
        Begin => "Begin",
        Body => "Body",
        Call => "Call",
        Case => "Case",
        Chan => "Chan",
        Closing => "Closing",
        Colon => "Colon",
        Comm => "Comm",
        Comment => "Comment",
        Comments => "Comments",
        Cond => "Cond",
        Decl => "Decl",
        Decls => "Decls",
        Defer => "Defer",
        Dir => "Dir",
        Doc => "Doc",
        Ellipsis => "Ellipsis",
        Elt => "Elt",
        Elts => "Elts",
        Else => "Else",
        EndPos => "EndPos",
        Fields => "Fields",
        FileEnd => "FileEnd",
        FileStart => "FileStart",
        For => "For",
        Fun => "Fun",
        Func => "Func",
        Go => "Go",
        GoVersion => "GoVersion",
        High => "High",
        If => "If",
        Implicit => "Implicit",
        Imports => "Imports",
        Incomplete => "Incomplete",
        Init => "Init",
        Index => "Index",
        Indices => "Indices",
        Interface => "Interface",
        Key => "Key",
        Kind => "Kind",
        Label => "Label",
        Lbrace => "Lbrace",
        Lbrack => "Lbrack",
        Len => "Len",
        Lhs => "Lhs",
        List => "List",
        Low => "Low",
        Lparen => "Lparen",
        Map => "Map",
        Max => "Max",
        Methods => "Methods",
        Name => "Name",
        NamePos => "NamePos",
        Names => "Names",
        Op => "Op",
        Opening => "Opening",
        OpPos => "OpPos",
        Package => "Package",
        Params => "Params",
        Path => "Path",
        Post => "Post",
        Range => "Range",
        Recv => "Recv",
        Results => "Results",
        Return => "Return",
        Rbrace => "Rbrace",
        Rbrack => "Rbrack",
        Rhs => "Rhs",
        Rparen => "Rparen",
        Select => "Select",
        Sel => "Sel",
        Semicolon => "Semicolon",
        Slice3 => "Slice3",
        Slash => "Slash",
        Specs => "Specs",
        Star => "Star",
        Stmt => "Stmt",
        Struct => "Struct",
        Switch => "Switch",
        Tag => "Tag",
        Text => "Text",
        Tok => "Tok",
        TokPos => "TokPos",
        Type => "Type",
        TypeParams => "TypeParams",
        Value => "Value",
        ValueEnd => "ValueEnd",
        ValuePos => "ValuePos",
        Values => "Values",
        X => "X",
        Y => "Y",
    }
}

go_enum! {
    /// Public `go/token.Token` values used by the syntax AST.
    pub enum GoToken {
        Illegal => "ILLEGAL",
        Eof => "EOF",
        Comment => "COMMENT",
        Ident => "IDENT",
        Int => "INT",
        Float => "FLOAT",
        Imag => "IMAG",
        Char => "CHAR",
        String => "STRING",
        Add => "+",
        Sub => "-",
        Mul => "*",
        Quo => "/",
        Rem => "%",
        And => "&",
        Or => "|",
        Xor => "^",
        Shl => "<<",
        Shr => ">>",
        AndNot => "&^",
        AddAssign => "+=",
        SubAssign => "-=",
        MulAssign => "*=",
        QuoAssign => "/=",
        RemAssign => "%=",
        AndAssign => "&=",
        OrAssign => "|=",
        XorAssign => "^=",
        ShlAssign => "<<=",
        ShrAssign => ">>=",
        AndNotAssign => "&^=",
        Land => "&&",
        Lor => "||",
        Arrow => "<-",
        Inc => "++",
        Dec => "--",
        Eql => "==",
        Lss => "<",
        Gtr => ">",
        Assign => "=",
        Not => "!",
        Neq => "!=",
        Leq => "<=",
        Geq => ">=",
        Define => ":=",
        Ellipsis => "...",
        Lparen => "(",
        Lbrack => "[",
        Lbrace => "{",
        Comma => ",",
        Period => ".",
        Semicolon => ";",
        Colon => ":",
        Rparen => ")",
        Rbrack => "]",
        Rbrace => "}",
        Break => "break",
        Case => "case",
        Chan => "chan",
        Const => "const",
        Continue => "continue",
        Default => "default",
        Defer => "defer",
        Else => "else",
        Fallthrough => "fallthrough",
        For => "for",
        Func => "func",
        Go => "go",
        Goto => "goto",
        If => "if",
        Import => "import",
        Interface => "interface",
        Map => "map",
        Package => "package",
        Range => "range",
        Return => "return",
        Select => "select",
        Struct => "struct",
        Switch => "switch",
        Type => "type",
        Var => "var",
        Tilde => "~",
    }
}

/// Public `go/ast.ChanDir` bit mask.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GoChanDirection {
    Send,
    Receive,
    SendReceive,
}

impl GoChanDirection {
    /// Canonical spelling of the corresponding `go/ast.ChanDir` bit mask.
    #[must_use]
    pub const fn go_name(self) -> &'static str {
        match self {
            Self::Send => "SEND",
            Self::Receive => "RECV",
            Self::SendReceive => "SEND|RECV",
        }
    }
}

/// Stable interned string identity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StringId(u32);

impl StringId {
    /// Numeric arena index.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Deterministic AST-owned string arena.
#[derive(Debug, Default)]
pub struct StringInterner {
    values: Vec<Box<str>>,
    ids: HashMap<Box<str>, StringId>,
}

impl StringInterner {
    pub(crate) fn intern(&mut self, value: &str) -> Result<StringId, AstError> {
        if let Some(id) = self.ids.get(value) {
            return Ok(*id);
        }
        let id = StringId(u32::try_from(self.values.len()).map_err(|_| AstError::IndexOverflow)?);
        let value: Box<str> = value.into();
        self.values
            .try_reserve(1)
            .map_err(|_| AstError::AllocationFailed)?;
        self.ids
            .try_reserve(1)
            .map_err(|_| AstError::AllocationFailed)?;
        self.values.push(value.clone());
        self.ids.insert(value, id);
        Ok(id)
    }

    /// Resolve one interned string.
    #[must_use]
    pub fn resolve(&self, id: StringId) -> Option<&str> {
        self.values.get(id.index()).map(AsRef::as_ref)
    }
}

/// Stable node identity in one [`GoAst`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AstNodeId(u32);

impl AstNodeId {
    fn try_from_index(index: usize) -> Result<Self, AstError> {
        Ok(Self(
            u32::try_from(index).map_err(|_| AstError::IndexOverflow)?,
        ))
    }

    /// Construct an identity for an existing arena index.
    #[must_use]
    pub fn from_index(index: usize) -> Option<Self> {
        u32::try_from(index).ok().map(Self)
    }

    /// Numeric arena index.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Independent optional `token.Pos`-derived byte endpoints.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct GoSourceRange {
    start: Option<TextSize>,
    end: Option<TextSize>,
}

impl GoSourceRange {
    /// Construct one source range.
    #[must_use]
    pub const fn new(start: Option<TextSize>, end: Option<TextSize>) -> Self {
        Self { start, end }
    }

    /// Start byte, or `None` for `token.NoPos`.
    #[must_use]
    pub const fn start(self) -> Option<TextSize> {
        self.start
    }

    /// End byte, or `None` for `token.NoPos`.
    #[must_use]
    pub const fn end(self) -> Option<TextSize> {
        self.end
    }

    /// Complete half-open byte range when both endpoints exist.
    #[must_use]
    pub const fn byte_range(self) -> Option<TextRange> {
        match (self.start, self.end) {
            (Some(start), Some(end)) => Some(TextRange::new(start, end)),
            _ => None,
        }
    }
}

impl From<TextRange> for GoSourceRange {
    fn from(range: TextRange) -> Self {
        Self::new(Some(range.start()), Some(range.end()))
    }
}

/// Compact range into the AST's node-list arena.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GoAstNodeList {
    pub(crate) start: u32,
    pub(crate) count: u32,
}

impl GoAstNodeList {
    /// Number of nodes.
    #[must_use]
    pub const fn len(self) -> usize {
        self.count as usize
    }

    /// Whether the list is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.count == 0
    }
}

/// Value of one ordered public Go AST field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum GoAstValue {
    Position(Option<TextSize>),
    String(StringId),
    Token(GoToken),
    Direction(GoChanDirection),
    Bool(bool),
    Node(Option<AstNodeId>),
    Nodes(GoAstNodeList),
}

/// One ordered public field/value pair.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GoAstFieldValue {
    field: GoAstField,
    value: GoAstValue,
}

impl GoAstFieldValue {
    /// Public struct field.
    #[must_use]
    pub const fn field(self) -> GoAstField {
        self.field
    }

    /// Field value.
    #[must_use]
    pub const fn value(self) -> GoAstValue {
        self.value
    }
}

/// One public `go/ast`-aligned AST node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoAstNode {
    kind: GoAstKind,
    source_range: GoSourceRange,
    field_start: u32,
    field_count: u32,
}

impl GoAstNode {
    /// Concrete public Go AST kind.
    #[must_use]
    pub const fn kind(&self) -> GoAstKind {
        self.kind
    }

    /// `Node.Pos` and `Node.End` as zero-based UTF-8 byte offsets.
    #[must_use]
    pub const fn source_range(&self) -> GoSourceRange {
        self.source_range
    }
}

/// Complete strict-only Go AST.
#[derive(Debug)]
pub struct GoAst {
    pub(crate) nodes: Vec<GoAstNode>,
    pub(crate) fields: Vec<GoAstFieldValue>,
    pub(crate) node_lists: Vec<AstNodeId>,
    pub(crate) strings: StringInterner,
    pub(crate) root: AstNodeId,
}

impl GoAst {
    /// Root file node.
    #[must_use]
    pub fn root(&self) -> &GoAstNode {
        &self.nodes[self.root.index()]
    }

    /// Root identity.
    #[must_use]
    pub const fn root_id(&self) -> AstNodeId {
        self.root
    }

    /// Nodes in deterministic arena order.
    #[must_use]
    pub fn nodes(&self) -> &[GoAstNode] {
        &self.nodes
    }

    /// Look up a node.
    #[must_use]
    pub fn node(&self, id: AstNodeId) -> Option<&GoAstNode> {
        self.nodes.get(id.index())
    }

    /// Ordered public fields for one node.
    #[must_use]
    pub fn fields(&self, id: AstNodeId) -> Option<&[GoAstFieldValue]> {
        let node = self.node(id)?;
        let start = usize::try_from(node.field_start).ok()?;
        let count = usize::try_from(node.field_count).ok()?;
        self.fields.get(start..start.checked_add(count)?)
    }

    /// Resolve a node list.
    #[must_use]
    pub fn node_list(&self, list: GoAstNodeList) -> Option<&[AstNodeId]> {
        let start = usize::try_from(list.start).ok()?;
        let count = usize::try_from(list.count).ok()?;
        self.node_lists.get(start..start.checked_add(count)?)
    }

    /// Resolve an interned AST string.
    #[must_use]
    pub fn string(&self, id: StringId) -> Option<&str> {
        self.strings.resolve(id)
    }
}

/// Go AST lowering failure.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AstError {
    RecoveryTree,
    SourceTooLarge,
    InconsistentCst {
        context: &'static str,
        expected: &'static str,
    },
    InvalidSourceRange(TextRange),
    IndexOverflow,
    AllocationFailed,
}

impl std::fmt::Display for AstError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for AstError {}

pub(crate) struct AstBuilder {
    pub(super) nodes: Vec<GoAstNode>,
    pub(super) fields: Vec<Vec<GoAstFieldValue>>,
    pub(super) node_lists: Vec<AstNodeId>,
    strings: StringInterner,
}

impl AstBuilder {
    pub(crate) fn new() -> Self {
        Self {
            nodes: Vec::new(),
            fields: Vec::new(),
            node_lists: Vec::new(),
            strings: StringInterner::default(),
        }
    }

    pub(crate) fn push_node(
        &mut self,
        kind: GoAstKind,
        source_range: GoSourceRange,
    ) -> Result<AstNodeId, AstError> {
        let id = AstNodeId::try_from_index(self.nodes.len())?;
        self.nodes
            .try_reserve(1)
            .map_err(|_| AstError::AllocationFailed)?;
        self.fields
            .try_reserve(1)
            .map_err(|_| AstError::AllocationFailed)?;
        self.nodes.push(GoAstNode {
            kind,
            source_range,
            field_start: 0,
            field_count: 0,
        });
        self.fields.push(Vec::new());
        Ok(id)
    }

    pub(crate) fn intern(&mut self, value: &str) -> Result<StringId, AstError> {
        self.strings.intern(value)
    }

    pub(crate) fn set_range(
        &mut self,
        node: AstNodeId,
        source_range: GoSourceRange,
    ) -> Result<(), AstError> {
        self.nodes
            .get_mut(node.index())
            .ok_or(AstError::IndexOverflow)?
            .source_range = source_range;
        Ok(())
    }

    pub(crate) fn push_field(
        &mut self,
        node: AstNodeId,
        field: GoAstField,
        value: GoAstValue,
    ) -> Result<(), AstError> {
        self.fields
            .get_mut(node.index())
            .ok_or(AstError::IndexOverflow)?
            .push(GoAstFieldValue { field, value });
        Ok(())
    }

    pub(crate) fn push_node_field(
        &mut self,
        node: AstNodeId,
        field: GoAstField,
        child: Option<AstNodeId>,
    ) -> Result<(), AstError> {
        self.push_field(node, field, GoAstValue::Node(child))
    }

    pub(crate) fn push_position_field(
        &mut self,
        node: AstNodeId,
        field: GoAstField,
        position: Option<TextSize>,
    ) -> Result<(), AstError> {
        self.push_field(node, field, GoAstValue::Position(position))
    }

    pub(crate) fn push_string_field(
        &mut self,
        node: AstNodeId,
        field: GoAstField,
        value: &str,
    ) -> Result<(), AstError> {
        let value = self.intern(value)?;
        self.push_field(node, field, GoAstValue::String(value))
    }

    pub(crate) fn push_token_field(
        &mut self,
        node: AstNodeId,
        field: GoAstField,
        token: GoToken,
    ) -> Result<(), AstError> {
        self.push_field(node, field, GoAstValue::Token(token))
    }

    pub(crate) fn push_bool_field(
        &mut self,
        node: AstNodeId,
        field: GoAstField,
        value: bool,
    ) -> Result<(), AstError> {
        self.push_field(node, field, GoAstValue::Bool(value))
    }

    pub(crate) fn push_direction_field(
        &mut self,
        node: AstNodeId,
        field: GoAstField,
        value: GoChanDirection,
    ) -> Result<(), AstError> {
        self.push_field(node, field, GoAstValue::Direction(value))
    }

    pub(crate) fn push_nodes_field(
        &mut self,
        node: AstNodeId,
        field: GoAstField,
        children: &[AstNodeId],
    ) -> Result<(), AstError> {
        let start = u32::try_from(self.node_lists.len()).map_err(|_| AstError::IndexOverflow)?;
        let count = u32::try_from(children.len()).map_err(|_| AstError::IndexOverflow)?;
        self.node_lists
            .try_reserve(children.len())
            .map_err(|_| AstError::AllocationFailed)?;
        self.node_lists.extend_from_slice(children);
        self.push_field(
            node,
            field,
            GoAstValue::Nodes(GoAstNodeList { start, count }),
        )
    }

    pub(crate) fn finish(mut self, root: AstNodeId) -> Result<GoAst, AstError> {
        let mut fields = Vec::new();
        for index in 0..self.nodes.len() {
            let start = u32::try_from(fields.len()).map_err(|_| AstError::IndexOverflow)?;
            let values = &self.fields[index];
            let count = u32::try_from(values.len()).map_err(|_| AstError::IndexOverflow)?;
            fields
                .try_reserve(values.len())
                .map_err(|_| AstError::AllocationFailed)?;
            fields.extend_from_slice(values);
            self.nodes[index].field_start = start;
            self.nodes[index].field_count = count;
        }
        Ok(GoAst {
            nodes: self.nodes,
            fields,
            node_lists: self.node_lists,
            strings: self.strings,
            root,
        })
    }
}
