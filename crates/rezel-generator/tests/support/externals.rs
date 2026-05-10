use std::sync::OnceLock;

use rezel_common::{NodeProp, NodePropConfig, NodePropDeserializer};
use rezel_lr::{ExternalTokenizer, InputStream, Stack, TokenizerFlags};

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
);

fn tokenize_ext1(input: &mut InputStream, _stack: &Stack) -> Result<(), rezel_common::ParseError> {
    let Some(next) = input.next() else {
        return Ok(());
    };
    let term = match next {
        value if value == u16::from(b'{') => BRACE_OPEN,
        value if value == u16::from(b'}') => BRACE_CLOSE,
        value if value == u16::from(b'.') => DOT,
        _ => return Ok(()),
    };
    input.advance(1);
    input.accept_token(term, 0)
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
