use std::fmt;

/// Parsed Lezer grammar source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GrammarDeclaration {
    pub start: usize,
    pub rules: Vec<RuleDeclaration>,
    pub top_rules: Vec<RuleDeclaration>,
    pub tokens: Option<TokenDeclaration>,
    pub local_tokens: Vec<LocalTokenDeclaration>,
    pub context: Option<ContextDeclaration>,
    pub external_tokens: Vec<ExternalTokenDeclaration>,
    pub external_specializers: Vec<ExternalSpecializeDeclaration>,
    pub external_prop_sources: Vec<ExternalPropSourceDeclaration>,
    pub precedences: Option<PrecDeclaration>,
    pub main_skip: Option<Expression>,
    pub scoped_skip: Vec<ScopedSkipDeclaration>,
    pub dialects: Vec<Identifier>,
    pub external_props: Vec<ExternalPropDeclaration>,
    pub auto_delimiters: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScopedSkipDeclaration {
    pub expression: Expression,
    pub top_rules: Vec<RuleDeclaration>,
    pub rules: Vec<RuleDeclaration>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleDeclaration {
    pub start: usize,
    pub id: Identifier,
    pub props: Vec<Prop>,
    pub params: Vec<Identifier>,
    pub expression: Expression,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrecDeclaration {
    pub start: usize,
    pub items: Vec<PrecItem>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrecKind {
    Left,
    Right,
    Cut,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrecItem {
    pub id: Identifier,
    pub kind: Option<PrecKind>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenPrecDeclaration {
    pub start: usize,
    pub items: Vec<TokenReference>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenConflictDeclaration {
    pub start: usize,
    pub left: TokenReference,
    pub right: TokenReference,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TokenReference {
    Name(NameExpression),
    Literal(LiteralExpression),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenDeclaration {
    pub start: usize,
    pub precedences: Vec<TokenPrecDeclaration>,
    pub conflicts: Vec<TokenConflictDeclaration>,
    pub rules: Vec<RuleDeclaration>,
    pub literals: Vec<LiteralDeclaration>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalTokenDeclaration {
    pub start: usize,
    pub precedences: Vec<TokenPrecDeclaration>,
    pub rules: Vec<RuleDeclaration>,
    pub fallback: Option<NamedNode>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiteralDeclaration {
    pub start: usize,
    pub literal: String,
    pub props: Vec<Prop>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextDeclaration {
    pub start: usize,
    pub id: Identifier,
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalTokenDeclaration {
    pub start: usize,
    pub id: Identifier,
    pub source: String,
    pub tokens: Vec<NamedNode>,
    pub conflicts: Vec<Identifier>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalSpecializeDeclaration {
    pub start: usize,
    pub kind: SpecializeKind,
    pub token: Expression,
    pub id: Identifier,
    pub source: String,
    pub tokens: Vec<NamedNode>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalPropSourceDeclaration {
    pub start: usize,
    pub id: Identifier,
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalPropDeclaration {
    pub start: usize,
    pub id: Identifier,
    pub external_id: Identifier,
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NamedNode {
    pub id: Identifier,
    pub props: Vec<Prop>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Identifier {
    pub start: usize,
    pub name: String,
}

impl fmt::Display for Identifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Expression {
    pub start: usize,
    pub kind: ExpressionKind,
}

impl Expression {
    #[must_use]
    pub const fn new(start: usize, kind: ExpressionKind) -> Self {
        Self { start, kind }
    }

    #[must_use]
    pub const fn precedence(&self) -> u8 {
        match self.kind {
            ExpressionKind::Choice(_) => 1,
            ExpressionKind::Sequence { .. } => 2,
            ExpressionKind::Repeat { .. } => 3,
            _ => 10,
        }
    }

    #[must_use]
    pub fn structurally_eq(&self, other: &Self) -> bool {
        match (&self.kind, &other.kind) {
            (ExpressionKind::Name(left), ExpressionKind::Name(right)) => {
                left.id.name == right.id.name
                    && expressions_structurally_eq(&left.arguments, &right.arguments)
            }
            (ExpressionKind::Specialize(left), ExpressionKind::Specialize(right)) => {
                left.kind == right.kind
                    && props_structurally_eq(&left.props, &right.props)
                    && left.token.structurally_eq(&right.token)
                    && left.content.structurally_eq(&right.content)
            }
            (ExpressionKind::InlineRule(left), ExpressionKind::InlineRule(right)) => {
                left.id.name == right.id.name
                    && props_structurally_eq(&left.props, &right.props)
                    && left.expression.structurally_eq(&right.expression)
            }
            (ExpressionKind::Choice(left), ExpressionKind::Choice(right)) => {
                expressions_structurally_eq(left, right)
            }
            (
                ExpressionKind::Sequence {
                    expressions: left_expressions,
                    markers: left_markers,
                    ..
                },
                ExpressionKind::Sequence {
                    expressions: right_expressions,
                    markers: right_markers,
                    ..
                },
            ) => {
                expressions_structurally_eq(left_expressions, right_expressions)
                    && marker_sets_structurally_eq(left_markers, right_markers)
            }
            (
                ExpressionKind::Repeat {
                    expression: left,
                    kind: left_kind,
                },
                ExpressionKind::Repeat {
                    expression: right,
                    kind: right_kind,
                },
            ) => left_kind == right_kind && left.structurally_eq(right),
            (ExpressionKind::Literal(left), ExpressionKind::Literal(right)) => {
                left.value == right.value
            }
            (ExpressionKind::Set(left), ExpressionKind::Set(right)) => {
                left.inverted == right.inverted && left.ranges == right.ranges
            }
            (ExpressionKind::Any, ExpressionKind::Any) => true,
            (ExpressionKind::CharClass(left), ExpressionKind::CharClass(right)) => left == right,
            _ => false,
        }
    }
}

fn expressions_structurally_eq(left: &[Expression], right: &[Expression]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.structurally_eq(right))
}

fn marker_sets_structurally_eq(
    left: &[Vec<ConflictMarker>],
    right: &[Vec<ConflictMarker>],
) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| left.kind == right.kind && left.id.name == right.id.name)
        })
}

fn props_structurally_eq(left: &[Prop], right: &[Prop]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.name == right.name
                && left.value.len() == right.value.len()
                && left
                    .value
                    .iter()
                    .zip(&right.value)
                    .all(|(left, right)| left.value == right.value && left.name == right.name)
        })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExpressionKind {
    Name(NameExpression),
    Specialize(SpecializeExpression),
    InlineRule(Box<RuleDeclaration>),
    Choice(Vec<Expression>),
    Sequence {
        expressions: Vec<Expression>,
        markers: Vec<Vec<ConflictMarker>>,
        explicitly_empty: bool,
    },
    Repeat {
        expression: Box<Expression>,
        kind: RepeatKind,
    },
    Literal(LiteralExpression),
    Set(SetExpression),
    Any,
    CharClass(CharClass),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NameExpression {
    pub start: usize,
    pub id: Identifier,
    pub arguments: Vec<Expression>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpecializeExpression {
    pub start: usize,
    pub kind: SpecializeKind,
    pub props: Vec<Prop>,
    pub token: Box<Expression>,
    pub content: Box<Expression>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpecializeKind {
    Extend,
    Specialize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConflictMarker {
    pub start: usize,
    pub id: Identifier,
    pub kind: ConflictMarkerKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictMarkerKind {
    Ambiguity,
    Precedence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepeatKind {
    Optional,
    ZeroOrMore,
    OneOrMore,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiteralExpression {
    pub start: usize,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetExpression {
    pub start: usize,
    pub ranges: Vec<(u32, u32)>,
    pub inverted: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CharClass {
    AsciiLetter,
    AsciiLowercase,
    AsciiUppercase,
    Digit,
    Whitespace,
    Eof,
}

impl CharClass {
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "asciiLetter" => Some(Self::AsciiLetter),
            "asciiLowercase" => Some(Self::AsciiLowercase),
            "asciiUppercase" => Some(Self::AsciiUppercase),
            "digit" => Some(Self::Digit),
            "whitespace" => Some(Self::Whitespace),
            "eof" => Some(Self::Eof),
            _ => None,
        }
    }

    #[must_use]
    pub fn ranges(self) -> &'static [(u32, u32)] {
        match self {
            Self::AsciiLetter => &[(65, 91), (97, 123)],
            Self::AsciiLowercase => &[(97, 123)],
            Self::AsciiUppercase => &[(65, 91)],
            Self::Digit => &[(48, 58)],
            Self::Whitespace => &[
                (9, 14),
                (32, 33),
                (133, 134),
                (160, 161),
                (5760, 5761),
                (8192, 8203),
                (8232, 8234),
                (8239, 8240),
                (8287, 8288),
                (12288, 12289),
            ],
            Self::Eof => &[],
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Prop {
    pub start: usize,
    pub at: bool,
    pub name: String,
    pub value: Vec<PropPart>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropPart {
    pub start: usize,
    pub value: Option<String>,
    pub name: Option<String>,
}
