use rezel_common::{SyntaxLanguage, SyntaxNode, TextRange, TextSize, TypedNode};

use crate::{GoChannelType, GoKind, GoLanguage, GoType, GoTypeName, GoTypeParam};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum GoChannelTypeDirection {
    SendReceive,
    Send,
    Receive,
}

pub(crate) struct GoChannelTypeLayer {
    keyword: SyntaxNode,
    begin: TextSize,
    end: TextSize,
    arrow: Option<SyntaxNode>,
    direction: GoChannelTypeDirection,
}

impl GoChannelTypeLayer {
    pub(crate) fn range(&self) -> TextRange {
        TextRange::new(self.begin, self.end)
    }

    pub(crate) const fn begin(&self) -> TextSize {
        self.begin
    }

    pub(crate) fn arrow(&self) -> Option<&SyntaxNode> {
        self.arrow.as_ref()
    }

    pub(crate) const fn direction(&self) -> GoChannelTypeDirection {
        self.direction
    }

    fn take_receive_prefix(&mut self) -> Option<SyntaxNode> {
        if self.direction != GoChannelTypeDirection::Receive {
            return None;
        }
        let arrow = self.arrow.take()?;
        self.begin = self.keyword.from();
        self.direction = GoChannelTypeDirection::SendReceive;
        Some(arrow)
    }
}

pub(crate) struct GoChannelTypeShape {
    layers: Vec<GoChannelTypeLayer>,
    value: GoType,
}

impl GoChannelTypeShape {
    pub(crate) fn layers(&self) -> &[GoChannelTypeLayer] {
        &self.layers
    }

    pub(crate) const fn value(&self) -> &GoType {
        &self.value
    }

    pub(crate) fn take_receive_prefix(&mut self) -> Option<SyntaxNode> {
        self.layers.first_mut()?.take_receive_prefix()
    }
}

pub(crate) struct GoTypeParameterShape {
    names: Vec<GoTypeName>,
    constraint: SyntaxNode,
}

impl GoTypeParameterShape {
    pub(crate) fn names(&self) -> &[GoTypeName] {
        &self.names
    }

    pub(crate) fn constraint(&self) -> &SyntaxNode {
        &self.constraint
    }
}

pub(crate) struct GoTypeShapeError {
    context: &'static str,
    expected: &'static str,
}

impl GoTypeShapeError {
    pub(crate) const fn context(&self) -> &'static str {
        self.context
    }

    pub(crate) const fn expected(&self) -> &'static str {
        self.expected
    }
}

/// Normalize Lezer's ambiguous nesting of adjacent channel directions to the
/// right-associative channel types produced by `go/parser`.
pub(crate) fn channel_type_shape(
    channel: &GoChannelType,
) -> Result<GoChannelTypeShape, GoTypeShapeError> {
    let mut layers = Vec::new();
    let mut channel = channel.clone();
    let value = loop {
        let keyword = channel.chan_token().ok_or(GoTypeShapeError {
            context: "channel type",
            expected: "chan keyword",
        })?;
        let arrow = channel.arrow_token();
        let direction = match arrow.as_ref() {
            None => GoChannelTypeDirection::SendReceive,
            Some(arrow) if arrow.from() < keyword.from() => GoChannelTypeDirection::Receive,
            Some(_) => GoChannelTypeDirection::Send,
        };
        let begin = match direction {
            GoChannelTypeDirection::Receive => {
                arrow.as_ref().map_or(keyword.from(), SyntaxNode::from)
            }
            GoChannelTypeDirection::SendReceive | GoChannelTypeDirection::Send => keyword.from(),
        };
        layers.push(GoChannelTypeLayer {
            keyword,
            begin,
            end: channel.syntax().to(),
            arrow,
            direction,
        });

        let ty = channel.ty().ok_or(GoTypeShapeError {
            context: "channel type",
            expected: "value type",
        })?;
        match ty {
            GoType::Channel(inner) => channel = inner,
            ty => break ty,
        }
    };

    for index in 0..layers.len().saturating_sub(1) {
        let (outer, inner) = layers.split_at_mut(index + 1);
        let outer = &mut outer[index];
        let inner = &mut inner[0];
        if outer.arrow.is_none()
            && let Some(transferred) = inner.take_receive_prefix()
        {
            outer.arrow = Some(transferred);
            outer.direction = GoChannelTypeDirection::Send;
        }
    }

    Ok(GoChannelTypeShape { layers, value })
}

/// Split the flattened `TypeParam` production at its final type-element role.
pub(crate) fn type_parameter_shape(
    parameter: &GoTypeParam,
) -> Result<GoTypeParameterShape, GoTypeShapeError> {
    let structural = parameter
        .syntax()
        .children()
        .filter(|child| !is_comment(child))
        .collect::<Vec<_>>();
    let (constraint, names) = structural.split_last().ok_or(GoTypeShapeError {
        context: "type parameter",
        expected: "constraint",
    })?;
    let names = names
        .iter()
        .filter_map(|name| GoTypeName::downcast_from(name.clone()).ok())
        .collect();
    Ok(GoTypeParameterShape {
        names,
        constraint: constraint.clone(),
    })
}

fn kind(node: &SyntaxNode) -> Option<GoKind> {
    <GoLanguage as SyntaxLanguage>::kind(node)
}

fn is_comment(node: &SyntaxNode) -> bool {
    matches!(kind(node), Some(GoKind::LineComment | GoKind::BlockComment))
}
