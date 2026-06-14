#![allow(dead_code)]

unsafe extern "C" {
    pub safe fn safe_foreign(value: i32) -> i32;
    pub unsafe fn unsafe_foreign(value: *const u8) -> usize;
    pub fn implicit_unsafe();
    pub unsafe fn variadic(format: *const u8, args: ...);

    pub safe static READ_ONLY: i32;
    pub unsafe static EXPLICIT_UNSAFE: i32;
    pub static mut MUTABLE: i32;
}

fn raw_borrows(value: &mut i32) -> (*const i32, *mut i32) {
    let shared = &raw const *value;
    let mutable = &raw mut *value;
    let _nested = &&raw const *value;
    (shared, mutable)
}

fn precise<'a, 'b, T, const N: usize>(
    value: &'a T,
    bytes: [u8; N],
) -> impl Sized + use<'a, T, N,> {
    (value, bytes)
}

fn capture_none() -> impl Sized + use<> {
    ()
}

fn weak_keywords() {
    let raw = 1;
    let safe = 2;
    let _ = (&raw, &safe);
}
