use crate::TextSize;

/// Return the UTF-8 byte offset of the first invalid identifier character.
///
/// Character policy remains owned by the caller so language crates can
/// combine one shared Unicode profile with language-specific additions and
/// exclusions.
#[must_use]
pub fn first_invalid_identifier_offset(
    spelling: &str,
    is_start: impl FnOnce(char) -> bool,
    mut is_continue: impl FnMut(char) -> bool,
) -> Option<TextSize> {
    let mut characters = spelling.char_indices();
    let Some((_, first)) = characters.next() else {
        return Some(TextSize::from(0));
    };
    if !is_start(first) {
        return Some(TextSize::from(0));
    }
    characters.find_map(|(offset, character)| {
        (!is_continue(character)).then(|| {
            let offset = u32::try_from(offset).unwrap_or(u32::MAX);
            TextSize::new(offset)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::first_invalid_identifier_offset;
    use crate::TextSize;

    #[test]
    fn reports_utf8_offsets_from_caller_owned_predicates() {
        let start = |character: char| character.is_ascii_alphabetic() || character == 'λ';
        let part = |character: char| start(character) || character.is_ascii_digit();

        assert_eq!(
            first_invalid_identifier_offset("", start, part),
            Some(0.into())
        );
        assert_eq!(
            first_invalid_identifier_offset("9name", start, part),
            Some(0.into())
        );
        assert_eq!(
            first_invalid_identifier_offset("λ2!", start, part),
            Some(TextSize::from(3))
        );
        assert_eq!(first_invalid_identifier_offset("λ2", start, part), None);
    }
}
