use rezel_common::{SyntaxLanguage, SyntaxNode, TypedNode};

use crate::{
    GoCase, GoClauseSimpleStatement, GoDefName, GoExpr, GoForClause, GoKind, GoLanguage,
    GoStatement,
};

pub(crate) enum GoAssignmentOperand {
    Expression(GoExpr),
    Definition(GoDefName),
}

impl GoAssignmentOperand {
    pub(crate) fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::Expression(expression) => expression.syntax(),
            Self::Definition(name) => name.syntax(),
        }
    }

    pub(crate) const fn is_definition(&self) -> bool {
        matches!(self, Self::Definition(_))
    }
}

pub(crate) struct GoAssignmentShape {
    operator: Option<SyntaxNode>,
    left: Vec<GoAssignmentOperand>,
    right: Vec<GoExpr>,
}

impl GoAssignmentShape {
    pub(crate) fn operator(&self) -> Option<&SyntaxNode> {
        self.operator.as_ref()
    }

    pub(crate) fn left(&self) -> &[GoAssignmentOperand] {
        &self.left
    }

    pub(crate) fn right(&self) -> &[GoExpr] {
        &self.right
    }
}

pub(crate) struct GoRangeShape {
    range_token: SyntaxNode,
    operator: Option<SyntaxNode>,
    left: Vec<GoAssignmentOperand>,
    expression: GoExpr,
}

pub(crate) struct GoForClauseShape {
    init: Option<GoClauseSimpleStatement>,
    condition: Option<GoExpr>,
    post: Option<GoClauseSimpleStatement>,
}

impl GoForClauseShape {
    pub(crate) fn init(&self) -> Option<&GoClauseSimpleStatement> {
        self.init.as_ref()
    }

    pub(crate) fn condition(&self) -> Option<&GoExpr> {
        self.condition.as_ref()
    }

    pub(crate) fn post(&self) -> Option<&GoClauseSimpleStatement> {
        self.post.as_ref()
    }
}

impl GoRangeShape {
    pub(crate) fn range_token(&self) -> &SyntaxNode {
        &self.range_token
    }

    pub(crate) fn operator(&self) -> Option<&SyntaxNode> {
        self.operator.as_ref()
    }

    pub(crate) fn left(&self) -> &[GoAssignmentOperand] {
        &self.left
    }

    pub(crate) fn expression(&self) -> &GoExpr {
        &self.expression
    }
}

pub(crate) struct GoStatementShapeError {
    context: &'static str,
    expected: &'static str,
}

impl GoStatementShapeError {
    pub(crate) const fn context(&self) -> &'static str {
        self.context
    }

    pub(crate) const fn expected(&self) -> &'static str {
        self.expected
    }
}

pub(crate) enum GoStatementListItem {
    Statement(GoStatement),
    Semicolon(SyntaxNode),
    RightBrace(SyntaxNode),
}

pub(crate) struct GoClauseShape {
    header: GoCase,
    body: Vec<GoStatementListItem>,
}

impl GoClauseShape {
    pub(crate) fn header(&self) -> &GoCase {
        &self.header
    }

    pub(crate) fn body(&self) -> &[GoStatementListItem] {
        &self.body
    }
}

pub(crate) enum GoStatementListShape {
    Plain(Vec<GoStatementListItem>),
    Switch(Vec<GoClauseShape>),
    Select(Vec<GoClauseShape>),
}

pub(crate) fn statement_list(node: &SyntaxNode) -> GoStatementListShape {
    match kind(node) {
        Some(GoKind::SwitchBlock) => GoStatementListShape::Switch(clauses(node)),
        Some(GoKind::SelectBlock) => GoStatementListShape::Select(clauses(node)),
        _ => GoStatementListShape::Plain(items(node)),
    }
}

pub(crate) fn assignment_shape(node: &SyntaxNode) -> GoAssignmentShape {
    let mut operator = None;
    let mut left = Vec::new();
    let mut right = Vec::new();
    for child in node.children() {
        if is_assignment_operator(&child) {
            operator = Some(child);
            continue;
        }
        if operator.is_none() {
            if let Ok(expression) = GoExpr::downcast_from(child.clone()) {
                left.push(GoAssignmentOperand::Expression(expression));
            } else if let Ok(name) = GoDefName::downcast_from(child) {
                left.push(GoAssignmentOperand::Definition(name));
            }
        } else if let Ok(expression) = GoExpr::downcast_from(child) {
            right.push(expression);
        }
    }
    if operator.is_none() {
        right.extend(left.drain(..).filter_map(|operand| match operand {
            GoAssignmentOperand::Expression(expression) => Some(expression),
            GoAssignmentOperand::Definition(_) => None,
        }));
    }
    GoAssignmentShape {
        operator,
        left,
        right,
    }
}

pub(crate) fn range_shape(node: &SyntaxNode) -> Result<GoRangeShape, GoStatementShapeError> {
    let mut range_token = None;
    let mut operator = None;
    let mut left = Vec::new();
    let mut expression = None;
    for child in node.children() {
        if kind(&child) == Some(GoKind::Range) {
            range_token = Some(child);
            continue;
        }
        if is_assignment_operator(&child) {
            operator = Some(child);
            continue;
        }
        if range_token.is_none() {
            if let Ok(value) = GoExpr::downcast_from(child.clone()) {
                left.push(GoAssignmentOperand::Expression(value));
            } else if let Ok(name) = GoDefName::downcast_from(child) {
                left.push(GoAssignmentOperand::Definition(name));
            }
        } else if let Ok(value) = GoExpr::downcast_from(child) {
            expression = Some(value);
        }
    }
    let range_token = range_token.ok_or(GoStatementShapeError {
        context: "range clause",
        expected: "range token",
    })?;
    let expression = expression.ok_or(GoStatementShapeError {
        context: "range clause",
        expected: "ranged expression",
    })?;
    Ok(GoRangeShape {
        range_token,
        operator,
        left,
        expression,
    })
}

pub(crate) fn for_clause_shape(
    clause: &GoForClause,
) -> Result<GoForClauseShape, GoStatementShapeError> {
    let mut semicolons = clause
        .syntax()
        .children()
        .filter(|child| kind(child) == Some(GoKind::Semicolon));
    let first = semicolons.next().ok_or(GoStatementShapeError {
        context: "for clause",
        expected: "first semicolon",
    })?;
    let second = semicolons.next().ok_or(GoStatementShapeError {
        context: "for clause",
        expected: "second semicolon",
    })?;
    let mut init = None;
    let mut condition = None;
    let mut post = None;
    for child in clause.syntax().children() {
        if child.to() <= first.from() {
            init = GoClauseSimpleStatement::downcast_from(child).ok();
        } else if child.from() >= first.to() && child.to() <= second.from() {
            condition = GoExpr::downcast_from(child).ok();
        } else if child.from() >= second.to() {
            post = GoClauseSimpleStatement::downcast_from(child).ok();
        }
    }
    Ok(GoForClauseShape {
        init,
        condition,
        post,
    })
}

fn items(node: &SyntaxNode) -> Vec<GoStatementListItem> {
    node.children().filter_map(statement_list_item).collect()
}

fn clauses(node: &SyntaxNode) -> Vec<GoClauseShape> {
    let mut clauses = Vec::new();
    let mut current = None;
    let mut body = Vec::new();
    for child in node.children() {
        if let Ok(header) = GoCase::downcast_from(child.clone()) {
            if let Some(header) = current.replace(header) {
                clauses.push(GoClauseShape {
                    header,
                    body: std::mem::take(&mut body),
                });
            }
            continue;
        }
        if let Some(item) = statement_list_item(child) {
            body.push(item);
        }
    }
    if let Some(header) = current {
        clauses.push(GoClauseShape { header, body });
    }
    clauses
}

fn statement_list_item(node: SyntaxNode) -> Option<GoStatementListItem> {
    match kind(&node) {
        Some(GoKind::Semicolon) => Some(GoStatementListItem::Semicolon(node)),
        Some(GoKind::RightBrace) => Some(GoStatementListItem::RightBrace(node)),
        _ => GoStatement::downcast_from(node)
            .ok()
            .map(GoStatementListItem::Statement),
    }
}

fn kind(node: &SyntaxNode) -> Option<GoKind> {
    <GoLanguage as SyntaxLanguage>::kind(node)
}

fn is_assignment_operator(node: &SyntaxNode) -> bool {
    matches!(
        kind(node),
        Some(GoKind::Equals | GoKind::Define | GoKind::UpdateOp)
    )
}
