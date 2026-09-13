use rezel_common::{SyntaxLanguage, TypedNode};

use crate::{RustFunctionType, RustKind, RustLanguage, RustType, RustTypePath};

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
