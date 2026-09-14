use rezel_common::{SyntaxLanguage, TextRange, TypedNode};

use crate::{
    RustArithmeticOperator, RustFunctionType, RustGenericArgumentValue, RustKind, RustLanguage,
    RustNumericLiteral, RustType, RustTypeArgumentList, RustTypePath,
};

/// One generic argument, which may span multiple direct CST children.
///
/// Negative literals use sibling operator and literal nodes in the grammar.
/// This view groups those nodes without synthesizing a syntax node.
#[derive(Clone, Debug)]
pub enum RustGenericArgument {
    /// An argument represented by one syntax node.
    Value(RustGenericArgumentValue),
    /// A leading minus and its numeric literal, if present in a recovering tree.
    NegativeLiteral {
        operator: RustArithmeticOperator,
        literal: Option<RustNumericLiteral>,
    },
}

impl RustGenericArgument {
    /// Return the argument's byte range, including a leading minus and intervening trivia.
    #[must_use]
    pub fn range(&self) -> TextRange {
        match self {
            Self::Value(value) => value.syntax().range(),
            Self::NegativeLiteral { operator, literal } => {
                let end = literal
                    .as_ref()
                    .map_or(operator.syntax().to(), |literal| literal.syntax().to());
                TextRange::new(operator.syntax().from(), end)
            }
        }
    }

    /// Slice the complete argument from its original source.
    #[must_use]
    pub fn text<'source>(&self, source: &'source str) -> Option<&'source str> {
        let range = self.range();
        source.get(usize::from(range.start())..usize::from(range.end()))
    }
}

impl RustTypeArgumentList {
    /// Iterate arguments, grouping each leading minus with its numeric literal.
    ///
    /// Comments between the minus and literal remain inside the argument's range.
    /// A missing literal in a recovering tree never consumes the following argument.
    pub fn arguments(&self) -> impl Iterator<Item = RustGenericArgument> {
        let mut children = self
            .syntax()
            .children()
            .filter(|child| {
                !matches!(
                    RustLanguage::kind(child),
                    Some(RustKind::LineComment | RustKind::BlockComment)
                )
            })
            .peekable();
        std::iter::from_fn(move || {
            while let Some(child) = children.next() {
                let child = match RustArithmeticOperator::downcast_from(child) {
                    Ok(operator) => {
                        let literal = children
                            .peek()
                            .cloned()
                            .and_then(|child| RustNumericLiteral::downcast_from(child).ok());
                        if literal.is_some() {
                            children.next();
                        }
                        return Some(RustGenericArgument::NegativeLiteral { operator, literal });
                    }
                    Err(child) => child,
                };
                if let Ok(value) = RustGenericArgumentValue::downcast_from(child) {
                    return Some(RustGenericArgument::Value(value));
                }
            }
            None
        })
    }
}

impl RustFunctionType {
    /// Return the path in parenthesized trait syntax such as `Fn(u8) -> bool`.
    ///
    /// Function pointer types have no trait path, even when their return type
    /// is a path. Missing paths in recovering trees also return `None`.
    #[must_use]
    pub fn trait_path(&self) -> Option<RustTypePath> {
        for child in self.syntax().children() {
            match RustLanguage::kind(&child) {
                Some(RustKind::TypePath) => return RustTypePath::downcast_from(child).ok(),
                Some(RustKind::Fn | RustKind::ParamList | RustKind::ThinArrow) => return None,
                _ => {}
            }
        }
        None
    }

    /// Return the explicit type following this signature's `->`.
    ///
    /// Returns `None` for an omitted return type or missing syntax in a
    /// recovering tree. Does not synthesize the implicit unit type.
    #[must_use]
    pub fn return_type(&self) -> Option<RustType> {
        let mut children = self.syntax().children();
        children.find(|child| RustLanguage::kind(child) == Some(RustKind::ThinArrow))?;
        children.find_map(|child| RustType::downcast_from(child).ok())
    }
}
