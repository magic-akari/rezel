use rezel_common::{SyntaxLanguage, SyntaxNode, TypedNode};

use super::types::{GoChannelTypeShape, channel_type_shape};
use crate::{GoAstExpr, GoConversion, GoExpr, GoKind, GoLanguage, GoType};

/// One layer of the LR-shaped Go expression CST.
pub(crate) enum GoExpressionShape {
    Atom(SyntaxNode),
    Unary {
        operator: SyntaxNode,
        operand: SyntaxNode,
    },
    Binary {
        left: SyntaxNode,
        operator: SyntaxNode,
        right: SyntaxNode,
    },
    Postfix {
        base: SyntaxNode,
        expression: SyntaxNode,
    },
}

/// A strict CST shape that cannot be interpreted as a Go expression.
pub(crate) struct GoExpressionShapeError {
    context: &'static str,
    expected: &'static str,
}

pub(crate) struct GoIndexShape {
    left_bracket: SyntaxNode,
    indices: Vec<GoAstExpr>,
    right_bracket: SyntaxNode,
    list: bool,
}

pub(crate) struct GoSliceShape {
    left_bracket: SyntaxNode,
    low: Option<GoExpr>,
    high: Option<GoExpr>,
    max: Option<GoExpr>,
    right_bracket: SyntaxNode,
    full: bool,
}

pub(crate) enum GoConversionFunction {
    Type(GoType),
    Channel(GoChannelTypeShape),
}

pub(crate) struct GoConversionShape {
    function: GoConversionFunction,
    prefixes: Vec<SyntaxNode>,
    argument: GoExpr,
    left_paren: SyntaxNode,
    right_paren: SyntaxNode,
}

impl GoConversionShape {
    pub(crate) const fn function(&self) -> &GoConversionFunction {
        &self.function
    }

    pub(crate) fn prefixes(&self) -> &[SyntaxNode] {
        &self.prefixes
    }

    pub(crate) const fn argument(&self) -> &GoExpr {
        &self.argument
    }

    pub(crate) const fn left_paren(&self) -> &SyntaxNode {
        &self.left_paren
    }

    pub(crate) const fn right_paren(&self) -> &SyntaxNode {
        &self.right_paren
    }
}

impl GoSliceShape {
    pub(crate) fn left_bracket(&self) -> &SyntaxNode {
        &self.left_bracket
    }

    pub(crate) fn low(&self) -> Option<&GoExpr> {
        self.low.as_ref()
    }

    pub(crate) fn high(&self) -> Option<&GoExpr> {
        self.high.as_ref()
    }

    pub(crate) fn max(&self) -> Option<&GoExpr> {
        self.max.as_ref()
    }

    pub(crate) fn right_bracket(&self) -> &SyntaxNode {
        &self.right_bracket
    }

    pub(crate) const fn is_full(&self) -> bool {
        self.full
    }
}

impl GoIndexShape {
    pub(crate) fn left_bracket(&self) -> &SyntaxNode {
        &self.left_bracket
    }

    pub(crate) fn indices(&self) -> &[GoAstExpr] {
        &self.indices
    }

    pub(crate) fn right_bracket(&self) -> &SyntaxNode {
        &self.right_bracket
    }

    pub(crate) const fn is_list(&self) -> bool {
        self.list
    }
}

impl GoExpressionShapeError {
    pub(crate) const fn context(&self) -> &'static str {
        self.context
    }

    pub(crate) const fn expected(&self) -> &'static str {
        self.expected
    }
}

/// Recover the Go expression precedence hidden by Lezer's broad conversion
/// production. Unparenthesized pointer and receive-channel prefixes are unary
/// operators in `go/parser`; a conversion to either type must parenthesize it.
pub(crate) fn conversion_shape(
    conversion: &GoConversion,
) -> Result<GoConversionShape, GoExpressionShapeError> {
    let mut function = conversion.ty().ok_or(GoExpressionShapeError {
        context: "conversion",
        expected: "type",
    })?;
    let argument = conversion.expression().ok_or(GoExpressionShapeError {
        context: "conversion",
        expected: "expression",
    })?;
    let left_paren = conversion
        .left_paren_token()
        .ok_or(GoExpressionShapeError {
            context: "conversion",
            expected: "left parenthesis",
        })?;
    let right_paren = conversion
        .right_paren_token()
        .ok_or(GoExpressionShapeError {
            context: "conversion",
            expected: "right parenthesis",
        })?;
    let mut prefixes = Vec::new();

    loop {
        match &function {
            GoType::Pointer(pointer) => {
                let star = pointer.star_token().ok_or(GoExpressionShapeError {
                    context: "pointer conversion",
                    expected: "star",
                })?;
                function = pointer.ty().ok_or(GoExpressionShapeError {
                    context: "pointer conversion",
                    expected: "pointee type",
                })?;
                prefixes.push(star);
            }
            GoType::Channel(channel) => {
                let mut channel =
                    channel_type_shape(channel).map_err(|error| GoExpressionShapeError {
                        context: error.context(),
                        expected: error.expected(),
                    })?;
                if let Some(arrow) = channel.take_receive_prefix() {
                    prefixes.push(arrow);
                }
                return Ok(GoConversionShape {
                    function: GoConversionFunction::Channel(channel),
                    prefixes,
                    argument,
                    left_paren,
                    right_paren,
                });
            }
            _ => break,
        }
    }

    Ok(GoConversionShape {
        function: GoConversionFunction::Type(function),
        prefixes,
        argument,
        left_paren,
        right_paren,
    })
}

/// Read one expression layer without discarding the association already chosen
/// by the generated LR parser.
pub(crate) fn expression_shape(
    node: &SyntaxNode,
) -> Result<GoExpressionShape, GoExpressionShapeError> {
    match kind(node) {
        Some(GoKind::UnaryExp) => {
            let (operator, operand) = unary_shape(node)?;
            Ok(GoExpressionShape::Unary { operator, operand })
        }
        Some(GoKind::BinaryExp) => {
            let mut expressions = node.children().filter(is_expression);
            let left = expressions.next().ok_or(GoExpressionShapeError {
                context: "binary expression",
                expected: "left operand",
            })?;
            let right = expressions.next().ok_or(GoExpressionShapeError {
                context: "binary expression",
                expected: "right operand",
            })?;
            let operator = node
                .children()
                .find(is_operator)
                .ok_or(GoExpressionShapeError {
                    context: "binary expression",
                    expected: "operator",
                })?;
            Ok(GoExpressionShape::Binary {
                left,
                operator,
                right,
            })
        }
        _ if is_postfix_expression(node) => {
            let base = node
                .children()
                .find(is_expression_base)
                .ok_or(GoExpressionShapeError {
                    context: "postfix expression",
                    expected: "base expression",
                })?;
            Ok(GoExpressionShape::Postfix {
                base,
                expression: node.clone(),
            })
        }
        _ => Ok(GoExpressionShape::Atom(node.clone())),
    }
}

fn unary_shape(node: &SyntaxNode) -> Result<(SyntaxNode, SyntaxNode), GoExpressionShapeError> {
    let operator = node
        .children()
        .find(is_operator)
        .ok_or(GoExpressionShapeError {
            context: "unary expression",
            expected: "operator",
        })?;
    let operand = node
        .children()
        .find(is_expression)
        .ok_or(GoExpressionShapeError {
            context: "unary expression",
            expected: "operand",
        })?;
    Ok((operator, operand))
}

/// Read the bracket and index roles shared by single and multi-index CST
/// productions in one traversal. The first expression is the postfix base.
pub(crate) fn index_shape(node: &SyntaxNode) -> Result<GoIndexShape, GoExpressionShapeError> {
    let list = match kind(node) {
        Some(GoKind::IndexExpr) => false,
        Some(GoKind::IndexListExpr) => true,
        _ => {
            return Err(GoExpressionShapeError {
                context: "index expression",
                expected: "IndexExpr or IndexListExpr",
            });
        }
    };
    let mut left_bracket = None;
    let mut right_bracket = None;
    let mut saw_base = false;
    let mut indices = Vec::new();
    for child in node.children() {
        match kind(&child) {
            Some(GoKind::LeftBracket) => left_bracket = Some(child),
            Some(GoKind::RightBracket) => right_bracket = Some(child),
            _ => {
                if let Ok(operand) = GoAstExpr::downcast_from(child) {
                    if saw_base {
                        indices.push(operand);
                    } else {
                        saw_base = true;
                    }
                }
            }
        }
    }
    let left_bracket = left_bracket.ok_or(GoExpressionShapeError {
        context: "index expression",
        expected: "left bracket",
    })?;
    let right_bracket = right_bracket.ok_or(GoExpressionShapeError {
        context: "index expression",
        expected: "right bracket",
    })?;
    Ok(GoIndexShape {
        left_bracket,
        indices,
        right_bracket,
        list,
    })
}

/// Interpret optional slice bounds relative to the production's colon tokens.
pub(crate) fn slice_shape(node: &SyntaxNode) -> Result<GoSliceShape, GoExpressionShapeError> {
    if kind(node) != Some(GoKind::SliceExpr) {
        return Err(GoExpressionShapeError {
            context: "slice expression",
            expected: "SliceExpr",
        });
    }
    let mut left_bracket = None;
    let mut right_bracket = None;
    let mut colons = Vec::new();
    let mut saw_base = false;
    let mut bounds = Vec::new();
    for child in node.children() {
        match kind(&child) {
            Some(GoKind::LeftBracket) => left_bracket = Some(child),
            Some(GoKind::RightBracket) => right_bracket = Some(child),
            Some(GoKind::Colon) => colons.push(child),
            _ => {
                if let Ok(expression) = GoExpr::downcast_from(child) {
                    if saw_base {
                        bounds.push(expression);
                    } else {
                        saw_base = true;
                    }
                }
            }
        }
    }
    let first_colon = colons.first().ok_or(GoExpressionShapeError {
        context: "slice expression",
        expected: "colon",
    })?;
    let second_colon = colons.get(1);
    let low = bounds
        .iter()
        .find(|expression| expression.syntax().to() <= first_colon.from())
        .cloned();
    let high = bounds
        .iter()
        .find(|expression| {
            expression.syntax().from() >= first_colon.to()
                && second_colon
                    .as_ref()
                    .is_none_or(|colon| expression.syntax().to() <= colon.from())
        })
        .cloned();
    let max = second_colon.and_then(|colon| {
        bounds
            .iter()
            .find(|expression| expression.syntax().from() >= colon.to())
            .cloned()
    });
    let left_bracket = left_bracket.ok_or(GoExpressionShapeError {
        context: "slice expression",
        expected: "left bracket",
    })?;
    let right_bracket = right_bracket.ok_or(GoExpressionShapeError {
        context: "slice expression",
        expected: "right bracket",
    })?;
    Ok(GoSliceShape {
        left_bracket,
        low,
        high,
        max,
        right_bracket,
        full: second_colon.is_some(),
    })
}

fn kind(node: &SyntaxNode) -> Option<GoKind> {
    <GoLanguage as SyntaxLanguage>::kind(node)
}

fn is_expression(node: &SyntaxNode) -> bool {
    GoExpr::downcast_from(node.clone()).is_ok()
}

fn is_expression_base(node: &SyntaxNode) -> bool {
    is_expression(node)
        || GoType::downcast_from(node.clone()).is_ok()
        || matches!(kind(node), Some(GoKind::Make | GoKind::New))
}

fn is_operator(node: &SyntaxNode) -> bool {
    matches!(
        kind(node),
        Some(
            GoKind::ArithOp
                | GoKind::BitOp
                | GoKind::DerefOp
                | GoKind::LogicOp
                | GoKind::CompareOp
                | GoKind::IncDecOp
                | GoKind::UpdateOp
                | GoKind::ChannelArrow
        )
    )
}

fn is_postfix_expression(node: &SyntaxNode) -> bool {
    matches!(
        kind(node),
        Some(
            GoKind::SelectorExpr
                | GoKind::IndexExpr
                | GoKind::IndexListExpr
                | GoKind::ParameterizedExpr
                | GoKind::SliceExpr
                | GoKind::TypeAssertion
                | GoKind::CallExpr
        )
    )
}
