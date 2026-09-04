use rezel_common::NodePropSource;

#[cfg(feature = "highlight")]
use rezel_highlight::{StandardTags, TagSet};

pub(crate) fn php_highlighting() -> NodePropSource {
    #[cfg(feature = "highlight")]
    {
        let rules = php_highlight_rules(rezel_highlight::tags());
        rezel_highlight::style_tags(rules).expect("the pinned PHP highlight selectors are valid")
    }

    #[cfg(not(feature = "highlight"))]
    {
        rezel_common::group_prop().source(|_| None)
    }
}

#[cfg(feature = "highlight")]
fn php_highlight_rules(tags: &'static StandardTags) -> Vec<(&'static str, TagSet)> {
    let mut rules = keyword_rules(tags);
    rules.extend(syntax_rules(tags));
    rules
}

#[cfg(feature = "highlight")]
fn keyword_rules(tags: &StandardTags) -> Vec<(&'static str, TagSet)> {
    vec![
        (
            "Visibility abstract final static",
            TagSet::from(tags.modifier),
        ),
        (
            "for foreach while do if else elseif switch try catch finally return throw break continue default case",
            TagSet::from(tags.control_keyword),
        ),
        (
            "endif endfor endforeach endswitch endwhile declare enddeclare goto match",
            TagSet::from(tags.control_keyword),
        ),
        (
            "and or xor yield unset clone instanceof insteadof",
            TagSet::from(tags.operator_keyword),
        ),
        (
            "function fn class trait implements extends const enum global interface use var",
            TagSet::from(tags.definition_keyword),
        ),
        (
            "include include_once require require_once namespace",
            TagSet::from(tags.module_keyword),
        ),
        (
            "new from echo print array list as",
            TagSet::from(tags.keyword),
        ),
        ("null", TagSet::from(tags.null)),
        ("Boolean", TagSet::from(tags.bool_)),
    ]
}

#[cfg(feature = "highlight")]
fn syntax_rules(tags: &StandardTags) -> Vec<(&'static str, TagSet)> {
    vec![
        ("VariableName", TagSet::from(tags.variable_name)),
        ("NamespaceName/...", TagSet::from(tags.namespace)),
        ("NamedType/...", TagSet::from(tags.type_name)),
        ("Name", TagSet::from(tags.name)),
        (
            "CallExpression/Name",
            TagSet::from(tags.function.apply(tags.variable_name)),
        ),
        ("LabelStatement/Name", TagSet::from(tags.label_name)),
        ("MemberExpression/Name", TagSet::from(tags.property_name)),
        (
            "MemberExpression/VariableName",
            TagSet::from(tags.special.apply(tags.property_name)),
        ),
        (
            "ScopedExpression/ClassMemberName/Name",
            TagSet::from(tags.property_name),
        ),
        (
            "ScopedExpression/ClassMemberName/VariableName",
            TagSet::from(tags.special.apply(tags.property_name)),
        ),
        (
            "CallExpression/MemberExpression/Name",
            TagSet::from(tags.function.apply(tags.property_name)),
        ),
        (
            "CallExpression/ScopedExpression/ClassMemberName/Name",
            TagSet::from(tags.function.apply(tags.property_name)),
        ),
        (
            "MethodDeclaration/Name FunctionDefinition/Name",
            TagSet::from(
                tags.function
                    .apply(tags.definition.apply(tags.variable_name)),
            ),
        ),
        (
            "ClassDeclaration/Name",
            TagSet::from(tags.definition.apply(tags.class_name)),
        ),
        ("UpdateOp", TagSet::from(tags.update_operator)),
        ("ArithOp", TagSet::from(tags.arithmetic_operator)),
        (
            "LogicOp IntersectionType/&",
            TagSet::from(tags.logic_operator),
        ),
        ("BitOp", TagSet::from(tags.bitwise_operator)),
        ("CompareOp", TagSet::from(tags.compare_operator)),
        ("ControlOp", TagSet::from(tags.control_operator)),
        ("AssignOp", TagSet::from(tags.definition_operator)),
        ("$ ConcatOp", TagSet::from(tags.operator)),
        ("LineComment", TagSet::from(tags.line_comment)),
        ("BlockComment", TagSet::from(tags.block_comment)),
        ("Integer", TagSet::from(tags.integer)),
        ("Float", TagSet::from(tags.float)),
        ("String", TagSet::from(tags.string)),
        (
            "ShellExpression",
            TagSet::from(tags.special.apply(tags.string)),
        ),
        ("=> ->", TagSet::from(tags.punctuation)),
        ("( )", TagSet::from(tags.paren)),
        ("#[ [ ]", TagSet::from(tags.square_bracket)),
        ("${ { }", TagSet::from(tags.brace)),
        ("-> ?->", TagSet::from(tags.deref_operator)),
        (", ; :: : \\", TagSet::from(tags.separator)),
        (
            "PhpOpen PhpClose",
            TagSet::from(tags.processing_instruction),
        ),
    ]
}
