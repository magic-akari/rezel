#![cfg(feature = "highlight")]

#[test]
fn rust_highlighting_follows_recursive_path_roles() {
    let source = r"
type Alias = crate::module::Trait<T>::Assoc;

fn build<T>(value: T) -> crate::module::Record {
    crate::module::Record { value }
}

fn invoke() {
    crate::module::make!();
}
";
    let tree = rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .unwrap();
    let selected_names = ["Alias", "module", "Trait", "T", "Assoc", "Record", "make"];
    let mut spans = Vec::new();
    rezel_lang_rust::highlight_spans(&tree, None, |span| {
        let range = span.range;
        let text = &source[usize::from(range.start())..usize::from(range.end())];
        if !selected_names.contains(&text) {
            return;
        }
        let tags = span
            .tags
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        spans.push((text, tags));
    });

    assert_eq!(
        spans,
        [
            ("Alias", vec!["typeName".to_owned()]),
            ("module", vec!["namespace".to_owned()]),
            ("Trait", vec!["namespace".to_owned()]),
            ("T", vec!["typeName".to_owned()]),
            ("Assoc", vec!["typeName".to_owned()]),
            ("T", vec!["typeName".to_owned()]),
            ("T", vec!["typeName".to_owned()]),
            ("module", vec!["namespace".to_owned()]),
            ("Record", vec!["typeName".to_owned()]),
            ("module", vec!["namespace".to_owned()]),
            ("Record", vec!["typeName".to_owned()]),
            ("module", vec!["namespace".to_owned()]),
            ("make", vec!["macroName".to_owned()]),
        ],
    );
}
