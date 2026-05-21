use rezel_common::{SyntaxNode, TypedNode};

use crate::{
    GoConstDecl, GoConstSpec, GoExpr, GoFunctionDecl, GoFunctionLiteral, GoFunctionType,
    GoImportDecl, GoMethodDecl, GoMethodElem, GoParameters, GoSpec, GoSpecList, GoType, GoTypeDecl,
    GoTypeParams, GoVarDecl, GoVarSpec,
};

/// The grammatical keyword family of a general declaration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GoGeneralDeclarationKind {
    Import,
    Const,
    Type,
    Var,
}

/// One general declaration with its common grammatical roles normalized.
#[derive(Clone, Copy)]
pub(crate) enum GoGeneralDeclaration<'a> {
    Import(&'a GoImportDecl),
    Const(&'a GoConstDecl),
    Type(&'a GoTypeDecl),
    Var(&'a GoVarDecl),
}

impl GoGeneralDeclaration<'_> {
    pub(crate) const fn kind(&self) -> GoGeneralDeclarationKind {
        match self {
            Self::Import(_) => GoGeneralDeclarationKind::Import,
            Self::Const(_) => GoGeneralDeclarationKind::Const,
            Self::Type(_) => GoGeneralDeclarationKind::Type,
            Self::Var(_) => GoGeneralDeclarationKind::Var,
        }
    }

    pub(crate) fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::Import(node) => node.syntax(),
            Self::Const(node) => node.syntax(),
            Self::Type(node) => node.syntax(),
            Self::Var(node) => node.syntax(),
        }
    }

    pub(crate) fn keyword_token(&self) -> Option<SyntaxNode> {
        match self {
            Self::Import(node) => node.keyword_token(),
            Self::Const(node) => node.keyword_token(),
            Self::Type(node) => node.keyword_token(),
            Self::Var(node) => node.keyword_token(),
        }
    }

    pub(crate) fn spec(&self) -> Option<GoSpec> {
        match self {
            Self::Import(node) => node.spec().map(GoSpec::Import),
            Self::Const(node) => node.spec().map(GoSpec::Const),
            Self::Type(node) => node.spec().map(GoSpec::Type),
            Self::Var(node) => node.spec().map(GoSpec::Var),
        }
    }

    pub(crate) fn spec_list(&self) -> Option<GoSpecList> {
        match self {
            Self::Import(node) => node.spec_list(),
            Self::Const(node) => node.spec_list(),
            Self::Type(node) => node.spec_list(),
            Self::Var(node) => node.spec_list(),
        }
    }
}

/// A const or var specification whose optional type and values share one
/// flattened production.
#[derive(Clone, Copy)]
pub(crate) enum GoValueSpec<'a> {
    Const(&'a GoConstSpec),
    Var(&'a GoVarSpec),
}

impl GoValueSpec<'_> {
    pub(crate) fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::Const(node) => node.syntax(),
            Self::Var(node) => node.syntax(),
        }
    }

    pub(crate) fn names(&self) -> Vec<crate::GoDefName> {
        match self {
            Self::Const(node) => node.names().collect(),
            Self::Var(node) => node.names().collect(),
        }
    }

    /// Interpret the flattened optional type and value list in one pass.
    pub(crate) fn parts(&self) -> GoValueSpecParts {
        let syntax = self.syntax();
        let equals = match self {
            Self::Const(node) => node.equals_token(),
            Self::Var(node) => node.equals_token(),
        };
        let mut ty = None;
        let mut values = Vec::new();
        for child in syntax.children() {
            if let Ok(candidate) = GoType::downcast_from(child.clone())
                && equals
                    .as_ref()
                    .is_none_or(|token| child.to() <= token.from())
            {
                ty = Some(candidate);
                continue;
            }
            if let Ok(candidate) = GoExpr::downcast_from(child)
                && equals
                    .as_ref()
                    .is_some_and(|token| candidate.syntax().from() >= token.to())
            {
                values.push(candidate);
            }
        }
        GoValueSpecParts { ty, values }
    }
}

pub(crate) struct GoValueSpecParts {
    ty: Option<GoType>,
    values: Vec<GoExpr>,
}

impl GoValueSpecParts {
    pub(crate) fn ty(&self) -> Option<&GoType> {
        self.ty.as_ref()
    }

    pub(crate) fn values(&self) -> &[GoExpr] {
        &self.values
    }
}

/// The common grammatical roles of every production containing a Go
/// signature.
#[derive(Clone, Copy)]
pub(crate) enum GoSignature<'a> {
    FunctionDeclaration(&'a GoFunctionDecl),
    MethodDeclaration(&'a GoMethodDecl),
    FunctionType(&'a GoFunctionType),
    FunctionLiteral(&'a GoFunctionLiteral),
    MethodElement(&'a GoMethodElem),
}

impl GoSignature<'_> {
    pub(crate) fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::FunctionDeclaration(node) => node.syntax(),
            Self::MethodDeclaration(node) => node.syntax(),
            Self::FunctionType(node) => node.syntax(),
            Self::FunctionLiteral(node) => node.syntax(),
            Self::MethodElement(node) => node.syntax(),
        }
    }

    pub(crate) fn func_token(&self) -> Option<SyntaxNode> {
        match self {
            Self::FunctionDeclaration(node) => node.func_token(),
            Self::MethodDeclaration(node) => node.func_token(),
            Self::FunctionType(node) => node.func_token(),
            Self::FunctionLiteral(node) => node.func_token(),
            Self::MethodElement(_) => None,
        }
    }

    pub(crate) fn type_parameters(&self) -> Option<GoTypeParams> {
        match self {
            Self::FunctionDeclaration(node) => node.type_parameters(),
            Self::MethodDeclaration(_)
            | Self::FunctionType(_)
            | Self::FunctionLiteral(_)
            | Self::MethodElement(_) => None,
        }
    }

    pub(crate) fn parameters(&self) -> Option<GoParameters> {
        match self {
            Self::FunctionDeclaration(node) => node.parameters(),
            Self::MethodDeclaration(node) => node.parameters(),
            Self::FunctionType(node) => node.parameters(),
            Self::FunctionLiteral(node) => node.parameters(),
            Self::MethodElement(node) => node.parameters(),
        }
    }

    pub(crate) fn result_parameters(&self) -> Option<GoParameters> {
        match self {
            Self::FunctionDeclaration(node) => node.result_parameters(),
            Self::MethodDeclaration(node) => node.result_parameters(),
            Self::FunctionType(node) => node.result_parameters(),
            Self::FunctionLiteral(node) => node.result_parameters(),
            Self::MethodElement(node) => node.result_parameters(),
        }
    }

    pub(crate) fn result_type(&self) -> Option<GoType> {
        match self {
            Self::FunctionDeclaration(node) => node.result_type(),
            Self::MethodDeclaration(node) => node.result_type(),
            Self::FunctionType(node) => node.result_type(),
            Self::FunctionLiteral(node) => node.result_type(),
            Self::MethodElement(node) => node.result_type(),
        }
    }
}
