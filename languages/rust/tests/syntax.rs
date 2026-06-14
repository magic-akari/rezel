#![forbid(unsafe_code)]

use rezel_common::ParseErrorKind;

#[test]
fn edition_2024_modern_syntax_is_accepted() {
    let source = r#"
unsafe extern "C" {
    pub safe fn read(value: *const u8) -> usize;
    pub unsafe fn write(value: *mut u8);
    pub unsafe fn variadic(format: *const u8, args: ...);
    pub safe static VERSION: i32;
    pub static mut STATE: i32;
}

fn modern<'a, T>(value: &'a mut T) -> impl Sized + use<'a, T,> {
    let shared = &&raw const *value;
    let mutable = &raw mut *value;
    (shared, mutable)
}
"#;

    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("Rust 1.95 Edition 2024 item, borrow, and capture syntax");
}

#[test]
fn precise_capture_bounds_enforce_context_free_static_rules() {
    for source in [
        "fn rejected<'a, T>(value: &'a T) -> impl Sized + use<T, 'a> { value }",
        "fn rejected<T>(value: T) -> impl Sized + use<T> + use<T> { value }",
    ] {
        let error = rezel_lang_rust::parser()
            .with_strict(true)
            .parse(source)
            .expect_err("invalid precise capturing bounds must fail strict parsing");
        assert_eq!(error.kind(), ParseErrorKind::Syntax);
    }
}

#[test]
fn weak_keywords_remain_identifiers_outside_their_contexts() {
    let source = "fn weak() { let raw = 1; let safe = 2; let _ = (&raw, &safe); }";
    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("raw and safe remain weak keywords");
}
