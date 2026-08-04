use rezel_common::{IterMode, TextSize, Tree};

use crate::{identifier, terms};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct KotlinSyntaxError {
    position: TextSize,
    message: &'static str,
}

impl KotlinSyntaxError {
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

pub(crate) fn validate_identifiers(tree: &Tree, source: &str) -> Result<(), KotlinSyntaxError> {
    let mut cursor = tree.cursor(IterMode::NONE);
    loop {
        if matches!(
            cursor.node_type().name(),
            "Identifier" | "Definition" | "TypeName"
        ) {
            validate_identifier(cursor.node_type().id(), cursor.from(), cursor.to(), source)?;
        }
        if !cursor.next(true) {
            return Ok(());
        }
    }
}

fn validate_identifier(
    node_type: u16,
    from: TextSize,
    to: TextSize,
    source: &str,
) -> Result<(), KotlinSyntaxError> {
    let spelling = node_text(from, to, source)?;
    let spelling = if node_type == terms::LabelIdentifier {
        spelling.strip_suffix('@').unwrap_or(spelling)
    } else {
        spelling
    };
    if spelling.starts_with('`') {
        return validate_escaped_identifier(from, spelling);
    }
    let mut characters = spelling.chars();
    let Some(first) = characters.next() else {
        return Err(KotlinSyntaxError::new(
            from,
            "a Kotlin identifier must not be empty",
        ));
    };
    if !identifier::is_start(u32::from(first))
        || characters.any(|character| !identifier::is_part(u32::from(character)))
    {
        return Err(KotlinSyntaxError::new(
            from,
            "identifier outside the Kotlin 2.4.10 lexical profile",
        ));
    }
    Ok(())
}

fn validate_escaped_identifier(from: TextSize, spelling: &str) -> Result<(), KotlinSyntaxError> {
    let Some(content) = spelling
        .strip_prefix('`')
        .and_then(|spelling| spelling.strip_suffix('`'))
    else {
        return Err(KotlinSyntaxError::new(
            from,
            "an escaped Kotlin identifier must be terminated",
        ));
    };
    if content.is_empty()
        || content
            .chars()
            .any(|character| matches!(character, '`' | '\n' | '\r'))
    {
        return Err(KotlinSyntaxError::new(
            from,
            "an escaped Kotlin identifier must be nonempty and single-line",
        ));
    }
    Ok(())
}

fn node_text(from: TextSize, to: TextSize, source: &str) -> Result<&str, KotlinSyntaxError> {
    source
        .get(usize::from(from)..usize::from(to))
        .ok_or_else(|| KotlinSyntaxError::new(from, "a Kotlin CST range on UTF-8 boundaries"))
}

#[cfg(test)]
mod tests {
    use super::validate_identifiers;

    #[test]
    fn identifier_validation_preserves_first_error_order() {
        let source = "val ¡ = 0\nval ¢ = 1\n";
        let tree = crate::parser()
            .parse(source)
            .expect("the broad generated identifier token accepts both spellings");
        let error = validate_identifiers(&tree, source)
            .expect_err("strict validation rejects identifiers outside the Kotlin profile");

        assert_eq!(u32::from(error.position()), 4);
        assert_eq!(
            error.message(),
            "identifier outside the Kotlin 2.4.10 lexical profile"
        );
    }

    #[test]
    fn identifier_validation_uses_bounded_call_stack() {
        const DEPTH: usize = 2_048;

        let mut source = String::from("fun deep(value: Int): Int = ");
        source.extend(std::iter::repeat_n('(', DEPTH));
        source.push_str("value");
        source.extend(std::iter::repeat_n(')', DEPTH));
        source.push('\n');
        let tree = crate::parser()
            .parse(&source)
            .expect("the non-strict parser builds the deep positive CST");
        // Keep the deep tree owned by this thread while the validation clone is dropped.
        let validation_tree = tree.clone();

        std::thread::Builder::new()
            .stack_size(128 * 1_024)
            .spawn(move || validate_identifiers(&validation_tree, &source))
            .expect("the bounded-stack validation thread starts")
            .join()
            .expect("identifier validation does not overflow the bounded stack")
            .expect("the deeply nested positive CST has valid identifiers");
        drop(tree);
    }
}
