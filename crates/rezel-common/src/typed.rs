use std::marker::PhantomData;

use crate::{SyntaxChildren, SyntaxNode};

/// Maps concrete CST node identities to one language-specific kind enum.
pub trait SyntaxLanguage {
    /// Stable kinds declared by the language grammar.
    type Kind: Copy + Eq;

    /// Return the kind of a node that belongs to this language.
    ///
    /// Nodes from another language must return `None`, even when their numeric
    /// term id and display name happen to match.
    fn kind(node: &SyntaxNode) -> Option<Self::Kind>;
}

/// One zero-copy typed view over a generic [`SyntaxNode`].
pub trait TypedNode: Clone + Sized {
    /// Language whose node identities this wrapper accepts.
    type Language: SyntaxLanguage;

    /// Downcast a generic node without losing it on failure.
    ///
    /// # Errors
    ///
    /// Returns the original node unchanged when it does not belong to this
    /// wrapper's language and concrete node kind.
    fn downcast_from(node: SyntaxNode) -> Result<Self, SyntaxNode>;

    /// Borrow the underlying generic node.
    fn syntax(&self) -> &SyntaxNode;

    /// Recover the underlying generic node.
    fn into_syntax(self) -> SyntaxNode;

    /// Slice this node's UTF-8 byte range from its original source.
    ///
    /// Returns `None` when the provided string is not the source associated
    /// with this tree or the range does not lie on UTF-8 boundaries.
    fn text<'source>(&self, source: &'source str) -> Option<&'source str> {
        let range = self.syntax().range();
        source.get(usize::from(range.start())..usize::from(range.end()))
    }
}

/// Lazy typed projection over direct CST children.
#[derive(Clone)]
pub struct TypedChildren<T>
where
    T: TypedNode,
{
    children: SyntaxChildren,
    marker: PhantomData<fn() -> T>,
}

impl<T> TypedChildren<T>
where
    T: TypedNode,
{
    /// Wrap one lazy generic child iterator.
    #[doc(hidden)]
    #[must_use]
    pub fn new(children: SyntaxChildren) -> Self {
        Self {
            children,
            marker: PhantomData,
        }
    }
}

impl<T> Iterator for TypedChildren<T>
where
    T: TypedNode,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.children
            .by_ref()
            .find_map(|node| T::downcast_from(node).ok())
    }
}
