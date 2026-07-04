#![forbid(unsafe_code)]

#[cfg(feature = "parser")]
mod verifier;

#[cfg(feature = "parser")]
fn main() {
    verifier::run();
}

#[cfg(not(feature = "parser"))]
fn main() {
    eprintln!(
        "the Swift P0 corpus verifier is retained as parser infrastructure; \
         rerun with `--features parser` after a Swift parser implementation exists"
    );
    std::process::exit(2);
}
