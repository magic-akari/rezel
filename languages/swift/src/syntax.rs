use rezel_common::{Input, ParseError, ParseErrorKind, SyntaxNode, TextSize, Tree};

use crate::tokens::lexical::{
    is_identifier_continue, is_identifier_start, is_operator_continue, is_operator_start,
};

pub(crate) fn validate_syntax(tree: &Tree, input: &dyn Input) -> Result<(), ParseError> {
    let mut pending = vec![tree.top_node()];
    while let Some(node) = pending.pop() {
        let node_type = node.node_type();
        if node_type.is_name("Identifier") {
            validate_identifier(&node, input)?;
            validate_identifier_role(&node, input)?;
        } else if node_type.is_name("ArgumentLabel") {
            reject_unescaped_spelling(&node, input, "inout", "invalid Swift argument label")?;
        } else if node_type.is_name("ModuleSelector") {
            validate_module_selector(&node, input)?;
        } else if node_type.is_name("IdentifierPattern") {
            validate_identifier_pattern(&node, input)?;
        } else if node_type.is_name("TypeIdentifierName") {
            validate_type_identifier_name(&node, input)?;
        } else if node_type.is_name("InvalidUsingDeclaration") {
            return Err(syntax_error(&node, "invalid Swift using declaration"));
        }
        pending.extend(node.children());
    }
    Ok(())
}

fn validate_identifier_role(node: &SyntaxNode, input: &dyn Input) -> Result<(), ParseError> {
    if input.read(node.range()).as_ref() != "inout" {
        return Ok(());
    }
    let Some(parent) = node.parent() else {
        return Ok(());
    };
    let label_in_enum_case = parent.node_type().is_name("EnumCaseParameter")
        && node
            .next_sibling()
            .is_some_and(|sibling| input.read(sibling.range()).as_ref() == ":");
    let label_in_decl_name = parent.node_type().is_name("DeclNameArgument");
    if label_in_enum_case || label_in_decl_name {
        return Err(syntax_error(node, "invalid Swift argument label"));
    }
    Ok(())
}

fn validate_module_selector(node: &SyntaxNode, input: &dyn Input) -> Result<(), ParseError> {
    let Some(module_name) = node.first_child() else {
        return Ok(());
    };
    reject_unescaped_spelling(&module_name, input, "self", "invalid Swift module selector")
}

fn validate_identifier_pattern(node: &SyntaxNode, input: &dyn Input) -> Result<(), ParseError> {
    let mut parent = node.parent();
    while let Some(current) = parent {
        let node_type = current.node_type();
        if node_type.is_name("OptionalBindingCondition") {
            return Ok(());
        }
        if node_type.is_name("VariableDeclaration") {
            return reject_unescaped_spelling(
                node,
                input,
                "self",
                "invalid Swift variable binding pattern",
            );
        }
        if node_type.is_name("CodeBlockItem") {
            break;
        }
        parent = current.parent();
    }
    Ok(())
}

fn validate_type_identifier_name(node: &SyntaxNode, input: &dyn Input) -> Result<(), ParseError> {
    let is_root_identifier_type = node
        .parent()
        .is_some_and(|parent| parent.node_type().is_name("IdentifierType"));
    if !is_root_identifier_type {
        return Ok(());
    }
    reject_unescaped_spelling(node, input, "self", "invalid Swift root type name")
}

fn reject_unescaped_spelling(
    node: &SyntaxNode,
    input: &dyn Input,
    spelling: &str,
    message: &'static str,
) -> Result<(), ParseError> {
    if input.read(node.range()).as_ref() == spelling {
        return Err(syntax_error(node, message));
    }
    Ok(())
}

fn syntax_error(node: &SyntaxNode, message: &'static str) -> ParseError {
    ParseError::new(ParseErrorKind::Syntax, Some(node.from()), message)
}

fn validate_identifier(node: &SyntaxNode, input: &dyn Input) -> Result<(), ParseError> {
    let text = input.read(node.range());
    if !text.starts_with('`') {
        return validate_ordinary_identifier(&text).map_err(|offset| {
            let offset = TextSize::try_from(offset).expect("identifier offset fits in TextSize");
            ParseError::new(
                ParseErrorKind::Syntax,
                Some(node.from() + offset),
                "invalid Swift identifier",
            )
        });
    }
    validate_raw_identifier(&text).map_err(|offset| {
        let offset = TextSize::try_from(offset).expect("raw identifier offset fits in TextSize");
        ParseError::new(
            ParseErrorKind::Syntax,
            Some(node.from() + offset),
            "invalid Swift raw identifier",
        )
    })
}

fn validate_ordinary_identifier(text: &str) -> Result<(), usize> {
    let mut characters = text.char_indices();
    let Some((_, first)) = characters.next() else {
        return Err(0);
    };
    if first == '$' {
        let mut length = 0_usize;
        let mut all_digits = true;
        for (index, character) in characters {
            let code_point = character as u32;
            if !is_identifier_continue(code_point) {
                return Err(index);
            }
            length += 1;
            all_digits &= character.is_ascii_digit();
        }
        return (length == 0 || !all_digits).then_some(()).ok_or(1);
    }
    if !is_identifier_start(first as u32) {
        return Err(0);
    }
    let mut length = 1_usize;
    for (index, character) in characters {
        if !is_identifier_continue(character as u32) {
            return Err(index);
        }
        length += 1;
    }
    (first != '_' || length > 1).then_some(()).ok_or(0)
}

fn validate_raw_identifier(text: &str) -> Result<(), usize> {
    let Some(content) = text
        .strip_prefix('`')
        .and_then(|text| text.strip_suffix('`'))
    else {
        return Err(0);
    };
    if content.is_empty() {
        return Err(1);
    }

    let mut has_non_operator = false;
    let mut has_non_whitespace = false;
    for (index, character) in content.char_indices() {
        let code_point = character as u32;
        if character == '`'
            || matches!(character, '\n' | '\r' | '\\')
            || is_forbidden_raw_identifier_whitespace(code_point)
            || is_unprintable_ascii(code_point)
        {
            return Err(index + 1);
        }
        if !is_permitted_raw_identifier_whitespace(code_point) {
            has_non_whitespace = true;
        }
        if (index == 0 && !is_operator_start(code_point)) || !is_operator_continue(code_point) {
            has_non_operator = true;
        }
    }

    if has_non_operator && has_non_whitespace {
        Ok(())
    } else {
        Err(1)
    }
}

const fn is_forbidden_raw_identifier_whitespace(code_point: u32) -> bool {
    code_point >= 0x0009 && code_point <= 0x000D
        || code_point == 0x0085
        || code_point == 0x00A0
        || code_point == 0x1680
        || code_point >= 0x2000 && code_point <= 0x200A
        || code_point >= 0x2028 && code_point <= 0x2029
        || code_point == 0x202F
        || code_point == 0x205F
        || code_point == 0x3000
}

const fn is_permitted_raw_identifier_whitespace(code_point: u32) -> bool {
    matches!(code_point, 0x0020 | 0x200E | 0x200F)
}

const fn is_unprintable_ascii(code_point: u32) -> bool {
    code_point < 0x20 || code_point == 0x7F
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rezel_common::{Input, ParseError, StringInput};

    use super::{validate_ordinary_identifier, validate_raw_identifier, validate_syntax};

    fn validate_source(source: &str) -> Result<(), ParseError> {
        let input: Arc<dyn Input> = Arc::new(StringInput::try_new(source)?);
        let tree = crate::parser()
            .with_strict(true)
            .parse_input(Arc::clone(&input))?;
        validate_syntax(&tree, &*input)
    }

    #[test]
    fn ordinary_identifier_validation_matches_swift_scalar_ranges() {
        for valid in ["name", "_name", "你好", "a\u{0308}", "$", "$0name"] {
            assert!(validate_ordinary_identifier(valid).is_ok(), "{valid:?}");
        }
        for invalid in ["_", "0name", "\u{0308}a", "a\u{00a0}b", "$123"] {
            assert!(
                validate_ordinary_identifier(invalid).is_err(),
                "{invalid:?}"
            );
        }
    }

    #[test]
    fn raw_identifier_validation_matches_swift_lexical_restrictions() {
        for valid in ["`name`", "`hello world`", "`$`", "`你 好`"] {
            assert!(validate_raw_identifier(valid).is_ok(), "{valid:?}");
        }
        for invalid in [
            "``",
            "`+++`",
            "`   `",
            "`contains\\slash`",
            "`contains\u{00a0}nbsp`",
            "`contains\u{007f}delete`",
        ] {
            assert!(validate_raw_identifier(invalid).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn syntax_validation_is_an_explicit_post_parse_phase() {
        for source in [
            "let `+++` = 0\n",
            "let `   ` = 0\n",
            "let `contains\u{00a0}nbsp` = 0\n",
            "let `contains\u{007f}delete` = 0\n",
            "let \u{0308}name = 0\n",
            "let name\u{00a0}suffix = 0\n",
            "let name\u{2192}suffix = 0\n",
            "using func",
            "enum E { case f(inout: Int) }",
            "value.member(inout: value)",
        ] {
            crate::parser()
                .with_strict(true)
                .parse(source)
                .unwrap_or_else(|error| panic!("parser rejected {source:?}: {error}"));
            assert!(validate_source(source).is_err(), "{source:?}");
        }

        for source in [
            "func `protocol`() {}\n",
            "let `hello world` = 0\n",
            "let `$` = 0\n",
            "let 你好 = 0\n",
            "let cafe\u{0308} = 0\n",
        ] {
            validate_source(source)
                .unwrap_or_else(|error| panic!("validator rejected {source:?}: {error}"));
        }
    }
}
