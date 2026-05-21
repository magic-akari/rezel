//! Crate-private grammatical views over Go syntax productions.

mod declarations;
mod expressions;
mod statements;
mod types;

pub(crate) use declarations::{
    GoGeneralDeclaration, GoGeneralDeclarationKind, GoSignature, GoValueSpec,
};
pub(crate) use expressions::{
    GoConversionFunction, GoExpressionShape, conversion_shape, expression_shape, index_shape,
    slice_shape,
};
pub(crate) use statements::{
    GoAssignmentOperand, GoClauseShape, GoStatementListItem, GoStatementListShape,
    assignment_shape, for_clause_shape, range_shape, statement_list,
};
pub(crate) use types::{
    GoChannelTypeDirection, GoChannelTypeLayer, GoChannelTypeShape, channel_type_shape,
    type_parameter_shape,
};
