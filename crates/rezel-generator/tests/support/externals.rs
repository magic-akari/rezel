use std::cell::Cell;
use std::sync::OnceLock;

use rezel_common::{CodePoint, NodeProp, NodePropConfig, NodePropDeserializer, ParseErrorKind};
use rezel_lr::{ExternalTokenizer, ExternalTokenizerStart, InputStream, Stack, TokenizerFlags};

#[allow(clippy::all, clippy::pedantic, dead_code, missing_docs)]
#[path = "../generated/cases/external_specializer_terms.rs"]
mod external_specializer_terms;
#[allow(clippy::all, clippy::pedantic, dead_code, missing_docs)]
#[path = "../generated/cases/external_tokens_terms.rs"]
mod external_tokens_terms;

use external_specializer_terms::{one as ONE, two as TWO};
use external_tokens_terms::{Dot as DOT, braceClose as BRACE_CLOSE, braceOpen as BRACE_OPEN};

pub static EXT1: ExternalTokenizer = ExternalTokenizer::new(
    tokenize_ext1,
    TokenizerFlags {
        contextual: false,
        fallback: false,
        extend: false,
    },
)
.with_start(
    ExternalTokenizerStart::NONE
        .with_ascii(b'{')
        .with_ascii(b'}')
        .with_ascii(b'.')
        .with_ascii(b'!')
        .with_end(),
);

thread_local! {
    static EXT1_CALLS: Cell<usize> = const { Cell::new(0) };
}

pub fn reset_ext1_calls() {
    EXT1_CALLS.set(0);
}

pub fn ext1_calls() -> usize {
    EXT1_CALLS.get()
}

fn tokenize_ext1(input: &mut InputStream, _stack: &Stack) -> Result<(), rezel_common::ParseError> {
    EXT1_CALLS.set(EXT1_CALLS.get() + 1);
    let Some(next) = input.next() else {
        return Ok(());
    };
    if next == CodePoint::from(b'!') {
        return Err(rezel_common::ParseError::new(
            ParseErrorKind::Input,
            Some(input.position()),
            "external tokenizer fixture error",
        ));
    }
    let term = match next {
        value if value == CodePoint::from(b'{') => BRACE_OPEN,
        value if value == CodePoint::from(b'}') => BRACE_CLOSE,
        value if value == CodePoint::from(b'.') => DOT,
        _ => return Ok(()),
    };
    input.advance(1);
    input.accept_token(term)
}

pub fn spec1(value: &str, _stack: &Stack) -> Option<u16> {
    match value {
        "one" => Some(ONE),
        "two" => Some(TWO),
        _ => None,
    }
}

pub fn tag() -> NodeProp<String> {
    static TAG: OnceLock<NodeProp<String>> = OnceLock::new();
    *TAG.get_or_init(|| {
        NodeProp::new(NodePropConfig {
            deserialize: Some(NodePropDeserializer::Infallible(str::to_owned)),
            ..NodePropConfig::default()
        })
    })
}
