#![forbid(unsafe_code)]

use std::fmt::Write as _;

use rezel_lr::ParseLimits;

const PHP_85_PROGRAMS: &[(&str, &str)] = &[
    (
        "dnf-and-literal-types",
        r"
interface A {}
interface B {}
interface C {}
interface D {}
function dnf((A&B)|(C&D) $value): (A&B)|true|null { return $value; }
function literals(true $yes, false $no, null $nil): true { return true; }
",
    ),
    (
        "readonly-and-asymmetric-visibility",
        r"
final readonly class Model {
    public private(set) readonly int $id;
    public function __construct(
        public protected(set) string $name,
        public final int $code,
    ) {}
}
",
    ),
    (
        "property-hooks",
        r"
class Box {
    #[Observed]
    public private(set) string $value {
        final get => $this->value;
        protected set(string|array $value) {
            $this->value = is_array($value) ? join(', ', $value) : $value;
        }
    }
}
",
    ),
    (
        "pipeline",
        "$result = ' hello ' |> trim(...) |> strtoupper(...);",
    ),
    (
        "clone-with-properties",
        "$copy = clone($object, ['name' => 'copy']); $same = clone($object,);",
    ),
    ("void-cast", "(void) side_effect();"),
    (
        "double-quoted-flexible-heredoc",
        r#"$value = <<<"TEXT"
    hello $name
    TEXT;
"#,
    ),
    (
        "utf8-identifiers",
        "class 𐐀 {} function 東京(𐐀 $値): 𐐀 { return $値; }",
    ),
    (
        "attributes-on-global-constants",
        "#[First] #[Second(name: 'value')] const EXAMPLE = true;",
    ),
];

const ZEND_GRAMMAR_PROGRAMS: &[(&str, &str)] = &[
    (
        "references-and-intersection-types",
        r"
function references(A&B $intersection, A & /* reference */ $value, & ...$rest) {
    foreach ($rest as $key => &$item) {
        $alias = &$item;
        $values = [&$item, 'alias' => &$alias];
        [$first, &$second] = $values;
        list($head, &$tail) = $values;
    }

    return function () use (& /* capture */ $value) { return $value; };
}
",
    ),
    (
        "attribute-placements",
        r"
#[Service]
interface Contract {}

#[Reusable]
trait SharedBehavior {}

$closure = #[Trace] function (): void {};
$arrow = #[Trace] fn(int $value): int => $value;
$anonymous = new #[Entity] class implements Contract {
    use SharedBehavior;
};
",
    ),
    (
        "constant-expressions-and-property-hooks",
        r"
declare(ticks=1 + 1);

enum Permission: int {
    case Read = 1 << 0;
    case Write = 1 << 1;
}

interface Named {
    public string $name {
        get;
        set(string $name);
    }
}
",
    ),
    (
        "destructuring-traits-and-dynamic-members",
        r"
trait First { public function run(): void {} }
trait Second { public function run(): void {} }

class UsesTraits {
    use First, Second {
        Second::run insteadof First;
    }
}

list(,,$third,,$fifth,) = [1, 2, 3, 4, 5, 6];
$property = 'handler';
$method = 'run';
UsesTraits::$$property();
$object->$$method();
",
    ),
    (
        "semi-reserved-identifiers-and-named-arguments",
        r"
cLaSs KeywordNames {
    const ARRAY = 1;
    public function readonly(): void {}
}

function invoke($array, $offset): void {}
invoke(array /* label trivia */ : [], offset: 1);
$result = (binary) 42;
$empty = match ($result) {};
",
    ),
    (
        "binary-prefixed-heredoc-and-nowdoc",
        r"
echo b<<<TEXT
heredoc
TEXT;
echo B<<<'TEXT'
nowdoc
TEXT;
",
    ),
    (
        "halt-compiler-opaque-tail",
        "__HALT_COMPILER();\nnot PHP anymore <?php } $unterminated",
    ),
    (
        "keyword-prefixed-qualified-names",
        "function names(): void { namespace\\local(); fn\\test(); }",
    ),
    (
        "absolute-group-use",
        r"
namespace Imports {
    use \Vendor\Package\{First, Second};
    use function \Vendor\Package\{first, second};
    use const \Vendor\Package\{FIRST, SECOND};
}
",
    ),
    (
        "nested-heredoc-interpolation",
        r"
$value = <<<DOC
    ${<<<DOC
        nested
        DOC}
    DOC;
",
    ),
    (
        "heredoc-interpolation-tag-line",
        r"
$value = <<<TAG
    ${
        TAG
    }
    TAG;
",
    ),
];

#[test]
fn strict_mode_accepts_focused_php_8_5_syntax_families() {
    let parser = rezel_lang_php::program_parser().with_strict(true);
    for &(name, source) in PHP_85_PROGRAMS {
        parser
            .parse(source)
            .unwrap_or_else(|error| panic!("{name}: valid PHP 8.5 failed: {error}\n{source}"));
    }
}

#[test]
fn strict_mode_accepts_zend_grammar_families_without_branching() {
    let parser = rezel_lang_php::program_parser()
        .with_strict(true)
        .with_limits(ParseLimits {
            max_stacks: 1,
            max_recovery_actions: 0,
            ..ParseLimits::default()
        });
    for &(name, source) in ZEND_GRAMMAR_PROGRAMS {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_php::program_parser().parse(source).unwrap();
            panic!("{name}: valid Zend grammar failed: {error}\n{recovered}\n{source}");
        });
    }
}

#[test]
fn match_condition_trailing_comma_converges_with_two_stacks() {
    let source = r"
$label = match ($value) {
    false,
    0,
        => 'false',
    default,
        => 'other',
};
";
    rezel_lang_php::program_parser()
        .with_strict(true)
        .with_limits(ParseLimits {
            max_stacks: 2,
            max_recovery_actions: 0,
            ..ParseLimits::default()
        })
        .parse(source)
        .expect("a match-arm trailing comma converges at the following arrow");
}

#[test]
fn reference_marker_lookahead_stays_within_a_linear_action_budget() {
    let mut source = String::from("function references(");
    for index in 0..1_024 {
        if index != 0 {
            source.push(',');
        }
        write!(source, "& /* reference */ $value{index}").unwrap();
    }
    source.push_str(") {}\n");

    let tree = rezel_lang_php::program_parser()
        .with_strict(true)
        .with_limits(ParseLimits {
            max_actions: 100_000,
            max_stacks: 1,
            max_recovery_actions: 0,
            ..ParseLimits::default()
        })
        .parse(&source)
        .expect("reference-marker lookahead remains bounded under strict parsing");
    assert_eq!(usize::from(tree.len()), source.len());
}

#[test]
fn named_argument_lookahead_stays_within_a_linear_action_budget() {
    let mut source = String::from("invoke(");
    for index in 0..1_024 {
        if index != 0 {
            source.push(',');
        }
        write!(source, "value{index} /* label */ : {index}").unwrap();
    }
    source.push_str(");\n");

    let tree = rezel_lang_php::program_parser()
        .with_strict(true)
        .with_limits(ParseLimits {
            max_actions: 100_000,
            max_stacks: 1,
            max_recovery_actions: 0,
            ..ParseLimits::default()
        })
        .parse(&source)
        .expect("named-argument lookahead remains bounded under strict parsing");
    assert_eq!(usize::from(tree.len()), source.len());
}

#[test]
fn nested_heredoc_scanning_stays_within_a_linear_action_budget() {
    let mut source = String::from("$value = <<<OUTER\n${");
    for index in 0..1_024 {
        write!(source, "<<<INNER{index}\nvalue\nINNER{index} + ").unwrap();
    }
    source.push_str("0}\nOUTER;\n");

    let tree = rezel_lang_php::program_parser()
        .with_strict(true)
        .with_limits(ParseLimits {
            max_actions: 1_024,
            max_stacks: 1,
            max_recovery_actions: 0,
            ..ParseLimits::default()
        })
        .parse(&source)
        .expect("nested heredoc scanning remains one opaque parser token");
    assert_eq!(usize::from(tree.len()), source.len());
}

#[test]
fn template_open_tags_follow_the_default_php_8_5_configuration() {
    for text in ["before <? echo 1; ?> after", "before <?phpX after"] {
        let tree = rezel_lang_php::parser()
            .with_strict(true)
            .parse(text)
            .unwrap();
        assert_eq!(tree.to_string(), "Template(Text)", "{text}");
    }

    for source in ["<?php", "<?php\necho 1;", "<?= 1 ?>"] {
        let tree = rezel_lang_php::parser()
            .with_strict(true)
            .parse(source)
            .unwrap();
        assert!(tree.to_string().contains("PhpOpen"), "{source}: {tree}");
    }
}
