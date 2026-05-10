pub fn split_case_file(source: &str) -> (&str, &str) {
    source
        .find("\n# ")
        .map_or((source, ""), |boundary| source.split_at(boundary))
}

pub fn expected_diagnostic(grammar: &str) -> Option<&str> {
    grammar.lines().find_map(|line| {
        let (_, message) = line.split_once("//! ")?;
        Some(message.trim())
    })
}
