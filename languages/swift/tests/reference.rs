#![forbid(unsafe_code)]

const ACCEPTED: &str = include_str!("fixtures/accepted/core.swift");
const CONTEXTUAL_ANY_TYPES: &str = include_str!("fixtures/accepted/contextual-any-types.swift");
const INLINE_ARRAY_TYPES: &str = include_str!("fixtures/accepted/inline-array-types.swift");
const RECURSIVE_OPTIONALS: &str = include_str!("fixtures/accepted/recursive-optionals.swift");
const REJECTED: &str = include_str!("fixtures/rejected/broken-function.swift");

#[test]
fn strict_acceptance_matches_the_curated_official_oracle_boundary() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for accepted in [
        ACCEPTED,
        CONTEXTUAL_ANY_TYPES,
        INLINE_ARRAY_TYPES,
        RECURSIVE_OPTIONALS,
    ] {
        parser.parse(accepted).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(accepted).unwrap();
            panic!("officially accepted fixture was rejected: {error}\n{recovered}");
        });
    }
    assert!(parser.parse(REJECTED).is_err());
    assert!(rezel_lang_swift::parser().parse(REJECTED).is_ok());
}
