use rezel_common::{ParseErrorKind, TextSize};

#[test]
fn default_parser_recovers_and_strict_parser_rejects() {
    let source = "def broken(:\n    pass\n";
    assert!(rezel_lang_python::parser().parse(source).is_ok());
    let error = rezel_lang_python::parser()
        .with_strict(true)
        .parse(source)
        .unwrap_err();
    assert_eq!(error.kind(), ParseErrorKind::Syntax);
}

#[test]
fn invalid_assignment_targets_are_syntax_errors() {
    let source = "1 = value\n";
    rezel_lang_python::parser()
        .parse(source)
        .expect("the recovering parser preserves an editor CST");

    let error = rezel_lang_python::parser()
        .with_strict(true)
        .parse(source)
        .expect_err("a literal is not an assignable Python target");
    assert_eq!(error.kind(), ParseErrorKind::Syntax);
    assert_eq!(error.position(), Some(TextSize::from(0)));
}

#[test]
fn named_official_tops_are_available() {
    for (top, source) in [
        ("Module", "answer = 42\n"),
        ("Expression", "answer + 1"),
        ("Interactive", "answer = 42\n"),
        ("FunctionType", "(int, str) -> bool"),
    ] {
        let parser = rezel_lang_python::parser().with_top(top).unwrap();
        parser
            .with_strict(true)
            .parse(source)
            .unwrap_or_else(|error| panic!("{top} rejected: {error}"));
    }
}

#[test]
fn mixed_ascii_and_unicode_identifiers_parse_as_single_names() {
    let source = "ascii_42 = 1\nªµ = 2\nasciiª = 3\nasciiª42 = 4\nªascii42 = 5\n";
    rezel_lang_python::parser()
        .with_strict(true)
        .parse(source)
        .unwrap_or_else(|error| panic!("rejected mixed identifier boundaries: {error}"));
}

#[test]
fn python_2_syntax_is_not_a_strict_cst_extension() {
    for source in [
        "print value\n",
        "different = left <> right\n",
        "raise Error, value\n",
    ] {
        assert!(
            rezel_lang_python::parser()
                .with_strict(true)
                .parse(source)
                .is_err(),
            "unexpectedly retained Python 2 syntax: {source:?}",
        );
        assert!(
            rezel_lang_python::parser().parse(source).is_ok(),
            "recovery parser should still produce an editor tree",
        );
    }
}

#[test]
fn python_314_unparenthesized_exception_lists_parse_with_current_semantics() {
    rezel_lang_python::parser()
        .with_strict(true)
        .parse("try:\n    operation()\nexcept Error, OtherError:\n    recover()\n")
        .expect("PEP 758 exception list");
    for source in [
        "try:\n    operation()\nexcept Error, OtherError as error:\n    recover(error)\n",
        "try:\n    operation()\nexcept*:\n    recover()\n",
    ] {
        assert!(
            rezel_lang_python::parser()
                .with_strict(true)
                .parse(source)
                .is_err(),
            "unexpectedly accepted {source:?}",
        );
    }
}

#[test]
fn indentation_errors_separate_recovery_and_strict_modes() {
    for (source, position) in [
        ("if True:\n\tpass\n        pass\n", 23_usize),
        ("if True:\n    pass\n  pass\n", 20),
        ("    pass\n", 4),
    ] {
        rezel_lang_python::parser()
            .parse(source)
            .unwrap_or_else(|error| panic!("recovering parse rejected {source:?}: {error}"));

        let error = rezel_lang_python::parser()
            .with_strict(true)
            .parse(source)
            .unwrap_err();
        assert_eq!(error.kind(), ParseErrorKind::Syntax, "{source:?}");
        assert_eq!(
            error.position(),
            Some(TextSize::try_from(position).unwrap())
        );
    }
}

#[test]
fn strict_indentation_accepts_python_structural_cases() {
    for source in [
        "if True:\n\tif False:\n\t\tpass\n\tpass\n",
        "if True:\n\x0c    pass\n",
        "if True:\n    if False:\n        pass\nanswer = 42\n",
        "if True:\n    # comment-only lines do not affect indentation\n\n    pass\nanswer = 42\n",
    ] {
        rezel_lang_python::parser()
            .with_strict(true)
            .parse(source)
            .unwrap_or_else(|error| panic!("strict parse rejected {source:?}: {error}"));
    }
}

#[test]
fn strict_indentation_handles_python_line_endings() {
    for line_ending in ["\n", "\r\n", "\r"] {
        let source = format!("if True:{line_ending}        pass{line_ending}\tpass{line_ending}");
        let error = rezel_lang_python::parser()
            .with_strict(true)
            .parse(&source)
            .unwrap_err();

        assert_eq!(error.kind(), ParseErrorKind::Syntax, "{line_ending:?}");
        let position = source.find("\tpass").unwrap() + 1;
        assert_eq!(
            error.position(),
            Some(TextSize::try_from(position).unwrap()),
            "{line_ending:?}"
        );
    }
}

#[test]
fn escaped_triple_quotes_preserve_following_indentation_boundaries() {
    let delimiter = "\"\"\"";
    let prefix =
        format!("if True:\n        value = {delimiter}before \\{delimiter} after{delimiter}\n");
    let valid = format!("{prefix}        pass\n");
    rezel_lang_python::parser()
        .with_strict(true)
        .parse(&valid)
        .expect("an escaped triple quote does not close its string");

    let invalid = format!("{prefix}\tpass\n");
    let error = rezel_lang_python::parser()
        .with_strict(true)
        .parse(&invalid)
        .unwrap_err();
    assert_eq!(error.kind(), ParseErrorKind::Syntax);
    let position = prefix.len() + 1;
    assert_eq!(
        error.position(),
        Some(TextSize::try_from(position).unwrap())
    );
}

#[test]
fn continued_string_contents_do_not_participate_in_indentation() {
    let prefix = "if True:\n        value = \"before \\\n(\"\n";
    let valid = format!("{prefix}        pass\n");
    rezel_lang_python::parser()
        .with_strict(true)
        .parse(&valid)
        .expect("brackets inside a continued string are not structural");

    let invalid = format!("{prefix}\tpass\n");
    let error = rezel_lang_python::parser()
        .with_strict(true)
        .parse(&invalid)
        .unwrap_err();
    assert_eq!(error.kind(), ParseErrorKind::Syntax);
    let position = prefix.len() + 1;
    assert_eq!(
        error.position(),
        Some(TextSize::try_from(position).unwrap())
    );
}

#[test]
fn python_314_type_defaults_and_template_strings_parse() {
    for source in [
        "type Alias[T = int] = list[T]\n",
        "def render[T = str](value: T) -> T:\n    return value\n",
        "result = t\"value={value!r:>10}\"\n",
    ] {
        rezel_lang_python::parser()
            .with_strict(true)
            .parse(source)
            .unwrap_or_else(|error| panic!("Python 3.14 source rejected: {error}: {source}"));
    }
}

#[test]
fn triple_quoted_strings_parse_as_single_literals() {
    for source in [
        "'''text'''\n",
        "\"\"\"text\"\"\"\n",
        "r'''text'''\n",
        "\"\"\"\nmultiline\n\"\"\"\n",
    ] {
        rezel_lang_python::parser()
            .with_strict(true)
            .parse(source)
            .unwrap_or_else(|error| {
                let recovered = rezel_lang_python::parser().parse(source).unwrap();
                panic!("could not parse {source:?}: {error}; recovered {recovered}")
            });
    }
}

#[test]
fn implicit_line_joining_ignores_continuation_indentation() {
    for source in [
        "value = call('first',\n             second='value')\n",
        "value = call(\n    first='value')\n",
        "value = call('a', 'b', action='store', default=False,\n             help='value')\n",
        "value = call(\n    '-c', '--choice', nargs='+',\n    help='value')\n",
        "def check():\n    parser.add_argument('-v', '--verbose', action='store_true', default=False,\n                        help='print very verbose output for all tests')\n",
        "def check():\n    group.add_argument(\n        '-c', '--choice', nargs='+',\n        help='print a random choice')\n",
        "def check():\n    command = idleConf.GetOption('main','General',\n                                 'print-command-posix')\n",
    ] {
        rezel_lang_python::parser()
            .with_strict(true)
            .parse(source)
            .unwrap_or_else(|error| {
                let recovered = rezel_lang_python::parser().parse(source).unwrap();
                panic!("could not parse {source:?}: {error}; recovered {recovered}")
            });
    }
}

#[test]
fn assignment_target_disambiguation_preserves_expression_contexts() {
    for source in [
        "class Box(Base, metaclass=Meta):\n    pass\n",
        "@registered\nclass Box[T = object](Base, metaclass=Meta):\n    pass\n",
        "@outer\n@pkg.decorator(flag=True)\nasync def compute[T: int = str, *Ts = *tuple[int], **P = [int]](first: T = initial, /, second = other, *items: Ts, named, option: int = 1, **extras: P) -> T:\n    return first\n\n@registered\nclass Box[T = object](Base, metaclass=Meta):\n    pass\n",
        "function(value, name=other)\n",
        "value = expression\n",
        "value\n",
    ] {
        rezel_lang_python::parser()
            .with_strict(true)
            .parse(source)
            .unwrap_or_else(|error| panic!("could not parse {source:?}: {error}"));
    }
}
