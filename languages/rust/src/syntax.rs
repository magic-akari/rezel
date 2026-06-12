use rezel_common::{SyntaxNode, TextSize, Tree};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RustSyntaxError {
    position: TextSize,
    message: &'static str,
}

impl RustSyntaxError {
    const fn new(position: TextSize, message: &'static str) -> Self {
        Self { position, message }
    }

    pub(crate) const fn position(self) -> TextSize {
        self.position
    }

    pub(crate) const fn message(self) -> &'static str {
        self.message
    }
}

pub(crate) fn validate_syntax(tree: &Tree) -> Result<(), RustSyntaxError> {
    validate_node(&tree.top_node())
}

fn validate_node(node: &SyntaxNode) -> Result<(), RustSyntaxError> {
    if node.name().as_ref() == "LetChain" {
        validate_let_chain(node)?;
    }
    for child in node.children() {
        validate_node(&child)?;
    }
    Ok(())
}

fn validate_let_chain(chain: &SyntaxNode) -> Result<(), RustSyntaxError> {
    for child in chain.children() {
        if child.name().as_ref() == "LetCondition" {
            let scrutinee = child.last_child().ok_or_else(|| {
                RustSyntaxError::new(child.to(), "an expression after `=` in a let condition")
            })?;
            validate_let_chain_operand(&scrutinee)?;
        } else if child.node_type().is_name("Expression") {
            validate_let_chain_operand(&child)?;
        }
    }
    Ok(())
}

fn validate_let_chain_operand(operand: &SyntaxNode) -> Result<(), RustSyntaxError> {
    let excluded = match operand.name().as_ref() {
        "AssignmentExpression" | "RangeExpression" | "StructExpression" => true,
        "BinaryExpression" => operand.child_by_name("LogicOp").is_some(),
        _ => false,
    };
    if excluded {
        return Err(RustSyntaxError::new(
            operand.from(),
            "a let-chain operand without a top-level lazy boolean, range, assignment, or struct expression",
        ));
    }
    Ok(())
}
