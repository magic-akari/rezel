use rezel_common::NodePropSource;

#[cfg(feature = "highlight")]
use rezel_highlight::TagSet;

pub(crate) fn rust_highlighting() -> NodePropSource {
    #[cfg(feature = "highlight")]
    {
        let tags = rezel_highlight::tags();
        rezel_highlight::style_tags([
            (
                "const macro_rules struct union enum type fn impl trait let static",
                TagSet::from(tags.definition_keyword),
            ),
            ("mod use crate", TagSet::from(tags.module_keyword)),
            (
                "pub unsafe safe async mut extern default move",
                TagSet::from(tags.modifier),
            ),
            (
                "for if else loop while match continue break return await",
                TagSet::from(tags.control_keyword),
            ),
            ("as in ref raw", TagSet::from(tags.operator_keyword)),
            (
                "where _ crate super dyn abstract become box do final gen macro override priv try typeof unsized virtual yield",
                TagSet::from(tags.keyword),
            ),
            ("self", TagSet::from(tags.self_)),
            ("String", TagSet::from(tags.string)),
            ("Char", TagSet::from(tags.character)),
            ("RawString", TagSet::from(tags.special.apply(tags.string))),
            ("Boolean", TagSet::from(tags.bool_)),
            ("Identifier", TagSet::from(tags.variable_name)),
            (
                "CallExpression/Identifier",
                TagSet::from(tags.function.apply(tags.variable_name)),
            ),
            (
                "BoundIdentifier",
                TagSet::from(tags.definition.apply(tags.variable_name)),
            ),
            (
                "FunctionItem/BoundIdentifier",
                TagSet::from(
                    tags.function
                        .apply(tags.definition.apply(tags.variable_name)),
                ),
            ),
            ("LoopLabel", TagSet::from(tags.label_name)),
            ("FieldIdentifier", TagSet::from(tags.property_name)),
            (
                "CallExpression/FieldExpression/FieldIdentifier",
                TagSet::from(tags.function.apply(tags.property_name)),
            ),
            (
                "Lifetime",
                TagSet::from(tags.special.apply(tags.variable_name)),
            ),
            ("ScopeIdentifier", TagSet::from(tags.namespace)),
            ("TypeIdentifier", TagSet::from(tags.type_name)),
            (
                "TypePath/Identifier",
                TagSet::from(tags.type_name),
            ),
            (
                "TypePath/TypePath/Identifier ScopedIdentifier/ScopedIdentifier/Identifier",
                TagSet::from(tags.namespace),
            ),
            (
                "StructExpression/Identifier StructExpression/ScopedIdentifier/Identifier StructPattern/Identifier StructPattern/ScopedIdentifier/Identifier TuplePattern/Identifier TuplePattern/ScopedIdentifier/Identifier",
                TagSet::from(tags.type_name),
            ),
            (
                "MacroInvocation/Identifier MacroInvocation/ScopedIdentifier/Identifier MacroInvocation/TypePath/Identifier",
                TagSet::from(tags.macro_name),
            ),
            ("\"!\"", TagSet::from(tags.macro_name)),
            ("UpdateOp", TagSet::from(tags.update_operator)),
            ("LineComment", TagSet::from(tags.line_comment)),
            ("BlockComment", TagSet::from(tags.block_comment)),
            ("Integer", TagSet::from(tags.integer)),
            ("Float", TagSet::from(tags.float)),
            ("ArithOp", TagSet::from(tags.arithmetic_operator)),
            ("LogicOp", TagSet::from(tags.logic_operator)),
            ("BitOp", TagSet::from(tags.bitwise_operator)),
            ("CompareOp", TagSet::from(tags.compare_operator)),
            ("=", TagSet::from(tags.definition_operator)),
            (".. ... => ->", TagSet::from(tags.punctuation)),
            ("( )", TagSet::from(tags.paren)),
            ("[ ]", TagSet::from(tags.square_bracket)),
            ("{ }", TagSet::from(tags.brace)),
            (". DerefOp", TagSet::from(tags.deref_operator)),
            ("&", TagSet::from(tags.operator)),
            (", ; ::", TagSet::from(tags.separator)),
            ("Attribute/...", TagSet::from(tags.meta)),
        ])
        .expect("the pinned Rust highlight selectors are valid")
    }

    #[cfg(not(feature = "highlight"))]
    {
        rezel_common::group_prop().source(|_| None)
    }
}
