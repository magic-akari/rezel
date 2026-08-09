use rezel_common::first_invalid_identifier_offset;
use rezel_lr::{StrictTokenValidationError, StrictTokenValidator};

use crate::terms;

pub(crate) static STRICT_TOKEN_VALIDATORS: [StrictTokenValidator; 1] =
    [StrictTokenValidator::new(terms::identifier, validate)];

fn validate(spelling: &str) -> Result<(), StrictTokenValidationError> {
    let invalid = first_invalid_identifier_offset(
        spelling,
        |character| character == '_' || unicode_ident::is_xid_start(character),
        unicode_ident::is_xid_continue,
    );
    match invalid {
        Some(offset) => Err(StrictTokenValidationError::new(
            offset,
            "invalid Go identifier",
        )),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::validate;

    #[test]
    fn validates_the_shared_xid_profile() {
        for valid in ["name", "_name", "λ2"] {
            assert!(validate(valid).is_ok(), "{valid:?}");
        }
        for invalid in ["2name", "name😀"] {
            assert!(validate(invalid).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn parser_applies_validation_only_in_strict_mode() {
        let source = "package p\nvar name😀 = 0\n";
        crate::parser()
            .parse(source)
            .expect("recovering Go accepts the broad identifier candidate");
        let error = crate::parser()
            .with_strict(true)
            .parse(source)
            .expect_err("strict Go validates the selected identifier token");
        assert_eq!(error.message(), "invalid Go identifier");
    }
}
