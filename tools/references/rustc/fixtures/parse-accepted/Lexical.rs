#![allow(dead_code, uncommon_codepoints, unused_variables)]

macro_rules! sink {
    ($($token:tt)*) => {};
}

fn lexical<'r#type>(࢏value: &'r#type str) {
    let 東京 = ࢏value;
    let r#gen = 1usize;
    let value2em = 1;
    let _ = c"æ\xC3\u{00E6}";
    let _ = cr##"raw \" C"##;
    let _ = b"ASCII\xFF";
    let _ = br#"ASCII only"#;
    let _ = '\u{1F980}';
    let _ = b'\xFF';
    let _ = 0b________1;
    let _ = 0o__70_i16;
    let _ = 0x01_f32;
    let _ = 123_;
    let _ = 2.;
    let _ = 1e+__2f64;
    let _ = 12E+99_f64;
    let _ = 東京;
    let _ = r#gen;
    let _ = value2em;
    sink!(r#let#foo);
}
