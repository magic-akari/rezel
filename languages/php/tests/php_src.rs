#![forbid(unsafe_code)]

use rezel_lr::ParseLimits;

const PHP_SRC_8_5_9: &[(&str, &str)] = &[
    (
        "pipe_operator/complex_ordering.phpt",
        include_str!("fixtures/php-src-8.5.9/pipe_operator__complex_ordering.php"),
    ),
    (
        "pipe_operator/precedence_comparison.phpt",
        include_str!("fixtures/php-src-8.5.9/pipe_operator__precedence_comparison.php"),
    ),
    (
        "clone/clone_with_001.phpt",
        include_str!("fixtures/php-src-8.5.9/clone__clone_with_001.php"),
    ),
    (
        "clone/clone_with_014.phpt",
        include_str!("fixtures/php-src-8.5.9/clone__clone_with_014.php"),
    ),
    (
        "asymmetric_visibility/variation.phpt",
        include_str!("fixtures/php-src-8.5.9/asymmetric_visibility__variation.php"),
    ),
    (
        "asymmetric_visibility/cpp_private.phpt",
        include_str!("fixtures/php-src-8.5.9/asymmetric_visibility__cpp_private.php"),
    ),
    (
        "property_hooks/explicit_set_value_parameter_type.phpt",
        include_str!(
            "fixtures/php-src-8.5.9/property_hooks__explicit_set_value_parameter_type.php"
        ),
    ),
    (
        "property_hooks/gh17101.phpt",
        include_str!("fixtures/php-src-8.5.9/property_hooks__gh17101.php"),
    ),
    (
        "property_hooks/attributes.phpt",
        include_str!("fixtures/php-src-8.5.9/property_hooks__attributes.php"),
    ),
    (
        "type_declarations/dnf_types/dnf_2_intersection.phpt",
        include_str!("fixtures/php-src-8.5.9/type_declarations__dnf_types__dnf_2_intersection.php"),
    ),
    (
        "type_declarations/literal_types/true_standalone.phpt",
        include_str!(
            "fixtures/php-src-8.5.9/type_declarations__literal_types__true_standalone.php"
        ),
    ),
    (
        "type_declarations/literal_types/false_standalone.phpt",
        include_str!(
            "fixtures/php-src-8.5.9/type_declarations__literal_types__false_standalone.php"
        ),
    ),
    (
        "heredoc_nowdoc/flexible-heredoc-complex-test1.phpt",
        include_str!("fixtures/php-src-8.5.9/heredoc_nowdoc__flexible-heredoc-complex-test1.php"),
    ),
    (
        "heredoc_nowdoc/nowdoc_016.phpt",
        include_str!("fixtures/php-src-8.5.9/heredoc_nowdoc__nowdoc_016.php"),
    ),
    (
        "ctor_promotion/ctor_promotion_additional_modifiers.phpt",
        include_str!(
            "fixtures/php-src-8.5.9/ctor_promotion__ctor_promotion_additional_modifiers.php"
        ),
    ),
    (
        "ctor_promotion/ctor_promotion_final.phpt",
        include_str!("fixtures/php-src-8.5.9/ctor_promotion__ctor_promotion_final.php"),
    ),
    (
        "readonly_classes/readonly_class_final_modifier.phpt",
        include_str!("fixtures/php-src-8.5.9/readonly_classes__readonly_class_final_modifier.php"),
    ),
    (
        "attributes/constants/ast_export.phpt",
        include_str!("fixtures/php-src-8.5.9/attributes__constants__ast_export.php"),
    ),
    (
        "grammar/semi_reserved_010.phpt",
        include_str!("fixtures/php-src-8.5.9/grammar__semi_reserved_010.php"),
    ),
    (
        "exit/exit_statements.phpt",
        include_str!("fixtures/php-src-8.5.9/exit__exit_statements.php"),
    ),
];

#[test]
fn strict_mode_accepts_curated_php_src_8_5_9_programs() {
    let parser = rezel_lang_php::parser()
        .with_strict(true)
        .with_limits(ParseLimits {
            max_stacks: 1,
            max_recovery_actions: 0,
            ..ParseLimits::default()
        });
    for &(path, source) in PHP_SRC_8_5_9 {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_php::parser().parse(source).unwrap();
            panic!("Zend/tests/{path}: positive php-src fixture failed: {error}\n{recovered}");
        });
    }
}
