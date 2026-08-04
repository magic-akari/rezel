#![forbid(unsafe_code)]

use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rezel_common::{
    Input, IterMode, ParseErrorKind, ParseRequest, Parser, StringInput, TextRange, TextSize,
    TypedNode,
};
use rezel_lang_kotlin::{
    KotlinAnonymousFunction, KotlinCharacterLiteral, KotlinFile, KotlinFunctionDeclaration,
    KotlinStringLiteral,
};
use rezel_lr::ParseLimits;

const CONTROLLED_SLICE: &str = "package bench\n\n\
fun compute(value: Int): Int {\n\
    var total = value\n\
    for (step in 0 until 8) {\n\
        if (step % 2 == 0) { total += step } else { total -= step }\n\
    }\n\
    return total\n\
}\n";

#[test]
fn parser_implements_the_common_interface() {
    let parser: Arc<dyn Parser> = Arc::new(rezel_lang_kotlin::parser());
    let tree = parser
        .parse(CONTROLLED_SLICE)
        .expect("the declared Kotlin positive contract parses");
    assert_eq!(tree.len(), CONTROLLED_SLICE.len().try_into().unwrap());
}

#[test]
fn strict_parser_accepts_generated_and_escaped_identifiers() {
    let source = "fun λ(`value name`: Int): Int = `value name`\n";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("generated lexing plus strict validation accepts Kotlin identifiers");
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn typed_root_and_function_remain_navigable() {
    let input = Arc::new(StringInput::try_new(CONTROLLED_SLICE).unwrap());
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse_input(input)
        .expect("the declared Kotlin positive contract parses strictly");
    let root = KotlinFile::downcast_from(tree.top_node()).expect("typed Kotlin file root");
    let function = root
        .syntax()
        .children()
        .find_map(|node| KotlinFunctionDeclaration::downcast_from(node).ok())
        .expect("typed function declaration");
    assert_eq!(function.syntax().name().as_ref(), "FunctionDeclaration");
}

#[test]
fn accepts_core_declaration_and_expression_forms() {
    let source = r#"package sample
import kotlin.collections.*
import kotlin.math.abs as absolute

public data class Box(val value: Int, var label: String = "box") {
    private val doubled: Int = value * 2
    fun map(input: Int): Int = absolute(input) + doubled
}

internal object Registry {
    val defaultValue: Int = Box(1).value!!
}

typealias NumberBox = Box
"#;
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("core declaration and expression forms parse");
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn accepts_core_type_forms() {
    let source = r"package sample

typealias Mapper<T> = (T) -> List<T?>

public class Pipeline<in Input, out Output>(val transform: Input.(Input?) -> Output) where Input : Any {
    fun <Next> map(value: Next): List<out Next?> = value
}

val dynamicValue: dynamic = null
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("core type forms parse");
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn accepts_core_control_flow_forms() {
    let source = r"package sample

annotation class Marker

fun classify(value: Int): Int {
    var current = value
    while (current != 0) { current -= 1 }
    do { current += 1 } while (current == 0)
    val result = when (val subject = current) {
        0 -> [0, 1]
        1, 2, -> [2]
        !is List<*> -> [subject]
        else -> throw current
    }
    val guarded = try { result } catch (@Marker error: Error) { [current] } finally { current += 1 }
    if (current == 1) return result.size
    return guarded.size
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("core control-flow forms parse");
    let cst = tree.to_string();
    assert!(cst.contains("WhenSubject"));
    assert!(cst.contains("AnnotationEntry"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_named_functions_as_control_structure_bodies() {
    let source = r"fun host(c:Boolean, xs:List<Int>) {
    if(c) fun yes() {} else fun no() = 0
    while(c) fun local() {}
    for(x in xs) fun take() = x
    do fun again() {} while(c)
    if(c) fun <T> generic(value:T):T
        where T:Any, T:Comparable<T> = value
    if(c) fun() {} else Unit
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("a hard fun prefix selects a named control-body declaration");
    let cst = tree.to_string();
    assert_eq!(cst.matches("FunctionDeclaration").count(), 7);
    assert!(cst.contains("AnonymousFunction"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn do_while_distinguishes_an_empty_body_from_a_nested_loop() {
    let strict = rezel_lang_kotlin::parser().with_strict(true);
    for source in [
        "fun test(){ do while(false); next() }",
        "fun test(){ do\nwhile(false); next() }",
        "fun test(){ do /* empty\n body */ while(false); next() }",
        "fun test(){ do next() while(false) }",
        "fun test(){ do {} while(false) }",
        "fun test(){ do for (item in items) use(item) while(false) }",
    ] {
        strict
            .parse(source)
            .unwrap_or_else(|error| panic!("official do-while shape failed: {error}"));
    }

    let source = "fun test(){ do while(false); next() }";
    let semicolon = source.find(';').expect("the separator is present");
    let tree = strict
        .parse(source)
        .expect("an omitted do-while body parses strictly");
    let mut do_while_end = None;
    let mut has_body = false;
    tree.iterate(
        TextRange::new(0.into(), tree.len()),
        IterMode::INCLUDE_ANONYMOUS,
        |node| {
            if node.name().as_ref() == "DoWhileStatement" {
                do_while_end = Some(usize::from(node.to()));
            } else if node.name().as_ref() == "ControlStructureBody" {
                has_body = true;
            }
            true
        },
        |_| {},
    );
    assert_eq!(do_while_end, Some(semicolon));
    assert!(!has_body);
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn accepts_when_guards() {
    let source = r"fun classify(value: Any): Int = when (value) {
    is String if value.isNotEmpty() -> value.length
    else -> 0
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("Kotlin 2.4 when guards parse after a primary condition");
    let cst = tree.to_string();
    assert!(cst.contains("WhenGuard"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn when_entries_can_return_explicit_parameter_lambdas() {
    let source = r"fun mapper(prefix: String) = when {
    prefix.isEmpty() -> { value: String -> value }
    else -> { value: String -> prefix + value }
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("a when entry distinguishes an explicit lambda from a block");
    let cst = tree.to_string();
    assert_eq!(cst.matches("WhenEntry(").count(), 2, "{cst}");
    assert_eq!(cst.matches("LambdaLiteral(").count(), 2, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_else_entries_after_control_bodies() {
    let source = r"fun equivalent(left: Any?, right: Any?): Boolean {
    when {
        left === right -> return true
        left == null -> return false

        else -> if (left != right) return false
    }
    return true
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("else starts a when entry without requiring a fallback separator token");
    let cst = tree.to_string();
    assert_eq!(cst.matches("WhenEntry").count(), 3);
    assert!(!cst.contains('⚠'));
}

#[test]
fn newline_ownership_keeps_else_catch_and_finally_structural() {
    let source = r"fun choose(flag: Boolean): Int {
    val selected = if (flag) 1
    else 2
    val guarded = try { selected }
    catch (error: Error) { 0 }
    finally { selected += 1 }
    return guarded
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("structural followers own their preceding newline");
    let cst = tree.to_string();
    assert!(cst.contains("IfExpression"));
    assert!(cst.contains("CatchClause"));
    assert!(cst.contains("FinallyClause"));
}

#[test]
fn structural_keywords_remain_available_as_declaration_names() {
    let source = r"class Module(val import: String)

fun catch(value: Int): Int = value
fun finally(value: Int): Int = value

fun handle() {
    try {} catch (error: Exception) {} finally {}
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("Kotlin contextual structural words remain valid declaration names");
    let cst = tree.to_string();

    assert_eq!(cst.matches("FunctionDeclaration").count(), 3);
    assert_eq!(cst.matches("CatchClause").count(), 1);
    assert_eq!(cst.matches("FinallyClause").count(), 1);
}

#[test]
fn accepts_file_and_declaration_annotations() {
    let source = r#"@file:kotlin.jvm.JvmName("SampleKt")
@file:Suppress("unused")

package sample

@file:AfterPackage

@Target(AnnotationTarget.CLASS)
public annotation class Marker

@Marker
class Marked(@param:Marker val value: Int)
"#;
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("file and declaration annotations parse");
    let cst = tree.to_string();
    assert_eq!(cst.matches("FileAnnotation(").count(), 3, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_a_leading_shebang_before_file_annotations() {
    let source = "#!/usr/bin/env kotlin\n@file:A\nclass C\n";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("a Kotlin source file accepts one leading shebang");
    let shebang = tree
        .top_node()
        .child_by_name("ShebangLine")
        .expect("the leading shebang remains visible in the CST");

    assert_eq!(usize::from(shebang.from()), 0);
    assert_eq!(usize::from(shebang.to()), source.find('\n').unwrap());
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn accepts_multiline_headers_delegation_and_extension_receivers() {
    let source = r"interface Source<T>

open class Base<T>

class Derived<T>(
    val source: Source<T>,
    val fallback: T,
) : Base<T>(), Source<T> {
    val Source<T>.first: T = fallback

    fun <R> Source<out R>.choose(value: R): R {
        return if (value >= fallback) value else fallback
    }
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("multiline headers, delegation, and extension receivers parse");
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn accepts_calls_and_logical_operators() {
    let source = r"fun <T> select(values: List<T>, fallback: T): T {
    val copied = ArrayList(values.size)
    val selected = copied.first
    return if (selected is Pair && selected in copied) selected.first else fallback
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("calls and logical operators parse");
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn accepts_expression_continuations_after_newlines() {
    let source = r"fun test(str: String, foo: String) {
    str

        .length
    str

        ?.length
    str

        as String
    str

        as? String
    str

        ?: foo
    true

        || false
    false

        && true
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("leading member, cast, Elvis, and logical operators continue expressions");
    let cst = tree.to_string();
    assert_eq!(cst.matches("MemberExpression").count(), 2);
    assert_eq!(cst.matches("CastExpression").count(), 2);
    assert_eq!(cst.matches("BinaryExpression").count(), 3);
    assert!(!cst.contains("expressionContinuationNewline"));
    assert!(!cst.contains('⚠'));

    let crlf = source.replace('\n', "\r\n");
    rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(&crlf)
        .expect("CRLF expression continuations parse identically");
    rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse("fun boundary(str: String) {\n    str\n    asValue\n}\n")
        .expect("an identifier beginning with as remains a new statement");
}

#[test]
fn parenthesized_navigation_targets_remain_postfix_members() {
    let source = r"fun host(value: Any, other: Any) {
    val plain = value.(other)
    val safe = value?.(other)
    val chained = value.(other).next
    val invoked = value.(other)()
    val lambda = { value }.(other)
    val anonymous = fun() {}.(other)
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("parenthesized navigation targets parse as postfix members");
    let cst = tree.to_string();

    assert_eq!(cst.matches("MemberExpression(").count(), 7, "{cst}");
    assert_eq!(cst.matches("ParenthesizedExpression(").count(), 6, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_spread_and_multiline_named_arguments() {
    let source = r"annotation class Mark
fun copy(options: Array<OpenOption>, value: String) {
    target(*options, named =
        value, @Mark annotated = value)
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("spread and multiline named value arguments follow the official grammar");
    let cst = tree.to_string();
    assert_eq!(cst.matches("ValueArgument(").count(), 3);
    assert_eq!(cst.matches("AnnotationEntry(").count(), 1);
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_newline_constructor_delegation() {
    let source = r"interface Marker

class Generator(seed: Int) :
    Marker {
    constructor(seed1: Int, seed2: Int) :
        this(seed1 + seed2)
}

enum class Mode :
    Marker { First }

object Singleton :
    Marker {}

fun make(): Marker = object :
    Marker {}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("a secondary constructor may delegate after the colon's newline");
    let cst = tree.to_string();
    assert!(cst.contains("SecondaryConstructor"));
    assert!(cst.contains("ConstructorDelegationCall"));
    assert_eq!(cst.matches("DelegationSpecifiers").count(), 4);
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_multiline_delegation_lists() {
    let source = r"class EntriesIterator<K, V>(delegate: Iterator<K>) :
    BaseIterator<K, V>(delegate),
    MutableIterator<MutableMap.MutableEntry<K, V>> {
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("a delegation list may continue after a comma and newline");
    let cst = tree.to_string();
    assert!(cst.contains("DelegationSpecifiers"));
    assert!(cst.contains("ConstructorInvocation"));
    assert!(cst.contains("UserType"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_annotated_loop_statements() {
    let source = r"annotation class Mark

fun collect(values: List<Any>, output: MutableList<String>) {
    @Mark loop@ for (value in values) if (value is String) output.add(value)
    @Mark local@ fun nested() {}
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("annotations and labels may prefix loops and local declarations");
    let cst = tree.to_string();
    assert_eq!(cst.matches("AnnotationEntry").count(), 2);
    assert_eq!(cst.matches("Label").count(), 2);
    assert!(cst.contains("ForStatement"));
    assert!(cst.contains("FunctionDeclaration"));
    assert!(!cst.contains("LabeledStatement"));
    assert!(!cst.contains("AnnotatedLoopStatement"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn annotated_loops_are_control_structure_bodies() {
    let source = r"annotation class Mark

fun nested(flag: Boolean, values: List<Int>) {
    if (flag) @Mark while (flag) break else Unit
    when {
        flag -> @Mark for (value in values) continue
        else -> Unit
    }
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("annotations may prefix loops used directly as control bodies");
    let cst = tree.to_string();

    assert_eq!(cst.matches("AnnotationEntry").count(), 2, "{cst}");
    assert_eq!(cst.matches("WhileStatement").count(), 1, "{cst}");
    assert_eq!(cst.matches("ForStatement").count(), 1, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn labeled_annotated_lambdas_are_control_structure_bodies() {
    let source = r"annotation class Mark

fun nested(flag: Boolean) {
    when {
        flag -> branch@ @Mark { value: Int -> value }
        else -> Unit
    }
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("labeled annotated lambdas may be used directly as control bodies");
    let cst = tree.to_string();

    assert_eq!(cst.matches("Label(").count(), 1, "{cst}");
    assert_eq!(cst.matches("AnnotationEntry").count(), 1, "{cst}");
    assert_eq!(cst.matches("LambdaLiteral").count(), 1, "{cst}");
    assert_eq!(cst.matches("LambdaParameter(").count(), 1, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn prefixed_local_properties_are_control_structure_bodies() {
    let source = r"annotation class Mark

fun nested(flag: Boolean, pairs: List<Pair<Int, Int>>) {
    while (flag) property@ val value = 1
    for (pair in pairs) @Mark val (left, right) = pair
    if (flag) @Mark var (first, second) = Pair(1, 2) else Unit
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("prefixed local properties may be used directly as control bodies");
    let cst = tree.to_string();

    assert_eq!(cst.matches("PropertyDeclaration(").count(), 3, "{cst}");
    assert_eq!(cst.matches("MultiVariableDeclaration(").count(), 2, "{cst}");
    assert_eq!(cst.matches("AnnotationEntry").count(), 2, "{cst}");
    assert_eq!(cst.matches("Label(").count(), 1, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn prefixed_local_objects_are_control_structure_bodies() {
    let source = r"annotation class Mark

fun nested(flag: Boolean) {
    do @Mark object Local {} while (flag)
    if (flag) local@ @Mark object Other {} else Unit
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("prefixed local objects retain declaration ownership in control bodies");
    let cst = tree.to_string();

    assert_eq!(cst.matches("ObjectDeclaration(").count(), 2, "{cst}");
    assert_eq!(cst.matches("ClassBody(").count(), 2, "{cst}");
    assert_eq!(cst.matches("AnnotationEntry").count(), 2, "{cst}");
    assert_eq!(cst.matches("Label(").count(), 1, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn specialized_local_classes_are_control_structure_bodies() {
    let source = r"annotation class Mark

fun nested(flag: Boolean) {
    if (flag) fun interface Local { fun run() } else Unit
    while (flag) label@ class Plain
    if (flag) @Mark annotation class Meta
    if (flag) @Mark class Box<T> where T : Any {} else Unit
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("specialized local class forms retain declaration ownership");
    let cst = tree.to_string();

    assert_eq!(cst.matches("ClassDeclaration(").count(), 5, "{cst}");
    assert_eq!(cst.matches("TypeParameters(").count(), 1, "{cst}");
    assert_eq!(cst.matches("TypeConstraints(").count(), 1, "{cst}");
    assert_eq!(cst.matches("AnnotationEntry").count(), 2, "{cst}");
    assert_eq!(cst.matches("Label(").count(), 1, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_annotated_expressions() {
    let source = r#"fun cast(value: Any): String =
    @Suppress("UNCHECKED_CAST") (value as String)
"#;
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("annotations are unary prefixes of ordinary expressions");
    let cst = tree.to_string();
    assert!(cst.contains("AnnotatedExpression"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_constructors_companions_fun_interfaces_and_enums() {
    let source = r"interface Key

fun interface Factory<T> {
    fun create(): T
}

enum class Mode(val code: Int) {
    FIRST(1),
    SECOND(2),
    ;

    companion object Named : Key {
        val default: Mode = FIRST
    }
    companion data data object Repeated
    companion value object Value
}

class Box<T> private constructor(val value: T) {
    constructor(value: T, ignored: Boolean) : this(value)
    init { value }
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("constructors, companions, fun interfaces, and enums parse");
    let cst = tree.to_string();
    assert_eq!(cst.matches("CompanionObject").count(), 3);
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_enum_in_parser_level_modifier_lists() {
    let source = r"enum public class Reordered { A }
enum fun function() {}
enum val property = 1
enum object ObjectValue
enum interface InterfaceValue
enum context(item: Item) class Contextual { A }
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("enum participates in the official parser-level modifier list");
    let cst = tree.to_string();
    assert_eq!(cst.matches("EnumClassBody").count(), 2);
    assert!(cst.contains("FunctionDeclaration"));
    assert!(cst.contains("PropertyDeclaration"));
    assert!(cst.contains("ObjectDeclaration"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn parameter_modifiers_share_the_official_modifier_surface() {
    let source = r"class C(vararg values: Int, noinline first: () -> Unit, crossinline second: () -> Unit)
vararg fun produce() {}
noinline class Marker
crossinline val value = 1
fun consume(vararg values: Int) {}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("parameter modifiers participate in the official general modifier rule");
    let cst = tree.to_string();

    assert_eq!(cst.matches("ParameterModifier(").count(), 7, "{cst}");
    assert_eq!(cst.matches("FunctionDeclaration(").count(), 2, "{cst}");
    assert_eq!(cst.matches("ClassDeclaration(").count(), 2, "{cst}");
    assert_eq!(cst.matches("PropertyDeclaration(").count(), 1, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn control_bodies_accept_annotated_functions_and_labeled_loops() {
    let source = r"annotation class A
fun host(a: Boolean, b: Boolean) {
    if (a) @A fun local() {} else Unit
    if (a) @A suspend fun suspended() {} else Unit
    if (a) outer@ inner@ while (b) Unit else Unit
    if (a) branch@ @A private inline fun labeled() {} else Unit
    if (a) callback@ fun() {} else Unit
    if (a) fun`local`() {} else Unit
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("control bodies retain official statement prefixes");
    let cst = tree.to_string();

    assert_eq!(cst.matches("FunctionDeclaration(").count(), 5, "{cst}");
    assert_eq!(cst.matches("AnonymousFunction(").count(), 1, "{cst}");
    assert_eq!(cst.matches("Label(").count(), 4, "{cst}");
    assert!(cst.contains("LoopStatement"), "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn empty_if_branches_keep_outer_separator_ownership() {
    let source = r"fun test(a: Boolean, b: Boolean) {
    if (a); next()
    if (a) else value
    if (a) else;
    if (a) if (b) else; else value
    when { a -> if (a)
        else -> value }
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("K1 empty if branches retain their outer separators");
    let cst = tree.to_string();
    assert_eq!(cst.matches("IfExpression(").count(), 6, "{cst}");

    let mut empty_bodies = 0;
    tree.iterate(
        TextRange::new(0.into(), tree.len()),
        IterMode::INCLUDE_ANONYMOUS,
        |node| {
            if node.name().as_ref() == "ControlStructureBody" && node.from() == node.to() {
                empty_bodies += 1;
            }
            true
        },
        |_| {},
    );
    assert_eq!(empty_bodies, 7, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn optional_return_operands_only_start_on_expression_tokens() {
    let source = "fun host() { val value = (return) }";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("a closing delimiter leaves the return operand absent");
    let return_start = source.find("return").unwrap();
    let mut return_range = None;
    tree.iterate(
        TextRange::new(0.into(), tree.len()),
        IterMode::INCLUDE_ANONYMOUS,
        |node| {
            if node.name().as_ref() == "ReturnExpression" {
                return_range = Some((usize::from(node.from()), usize::from(node.to())));
            }
            true
        },
        |_| {},
    );
    assert_eq!(return_range, Some((return_start, return_start + 6)));
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn loop_and_if_semicolons_preserve_outer_statement_ownership() {
    let source = r"fun test(a: Boolean, b: Boolean, values: List<Int>) {
    for (value in values); consume(value)
    while (a); consume(a)
    if (a) consume(a); else consume(b)
    if (a) while (b); else consume(a)
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("loop and if semicolons remain available to their outer owner");
    let cst = tree.to_string();

    assert_eq!(cst.matches("ForStatement(").count(), 1, "{cst}");
    assert_eq!(cst.matches("WhileStatement(").count(), 2, "{cst}");
    assert_eq!(cst.matches("IfExpression(").count(), 2, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn anonymous_functions_share_the_official_receiver_type_surface() {
    let source = r"annotation class A
val plain = fun String.() {}
val annotated = fun @A String.() {}
val parenthesized = fun (String).() {}
fun host(condition: Boolean) {
    if (condition) fun (String).() {} else Unit
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("anonymous functions accept the official receiver type surface");
    let cst = tree.to_string();

    assert_eq!(cst.matches("AnonymousFunction(").count(), 4, "{cst}");
    assert_eq!(cst.matches("ReceiverType(").count(), 4, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn when_entry_separators_connect_the_final_else_entry() {
    let source = r"fun test(condition: Boolean) {
    when { condition -> while (condition) consume(); else -> consume() }
    when { condition -> fun() {}; else -> Unit }
    when { condition -> result = first; else -> result = second }
    when { condition -> { -> first }; else -> { -> second } }
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("a statement separator connects any supported when body to else");
    let cst = tree.to_string();

    assert_eq!(cst.matches("WhenExpression(").count(), 4, "{cst}");
    assert_eq!(cst.matches("WhenEntry(").count(), 8, "{cst}");
    assert!(cst.contains("WhileStatement"), "{cst}");
    assert!(cst.contains("AnonymousFunction"), "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn type_aliases_are_declarations_in_every_control_body_role() {
    let source = r"annotation class Mark
fun host(condition: Boolean) {
    if (condition) @Mark typealias IfAlias = String else Unit
    while (condition) @Mark typealias WhileAlias = String
    do @Mark typealias DoAlias = String while (condition)
    when { condition -> @Mark typealias WhenAlias = String; else -> Unit }
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("type aliases are statements in control-structure bodies");
    let cst = tree.to_string();

    assert_eq!(cst.matches("TypeAliasDeclaration").count(), 4, "{cst}");
    assert_eq!(cst.matches("ControlStructureBody").count(), 6, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn class_bodies_accept_empty_member_statements() {
    let source = r"class Plain {
    ;
    fun member() {}
    ;;
}

enum class Values {
    ;
    fun member() {}
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("class and enum bodies accept empty member statements");
    let cst = tree.to_string();

    assert_eq!(cst.matches("ClassDeclaration(").count(), 2, "{cst}");
    assert_eq!(cst.matches("FunctionDeclaration(").count(), 2, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_this_super_reified_and_disjunction() {
    let source = r"open class Base {
    fun inherited(): Int = 1
}

class Derived : Base() {
    inline fun <reified T> choose(code: Int): Derived {
        if (code < 0 || code > 1) return this
        super.inherited()
        return this
    }
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("this, super, reified parameters, and disjunction parse");
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn accepts_implicit_trailing_lambdas() {
    let source = r"fun select(values: List<Int>, ready: Boolean): Int {
    val first = sequence result@ { return@result }
    checkIsMutable()
    retry@ while (ready) break@retry
    return values.firstOrNull { it > 0 } ?: values.elementAtOrElse(0) { 1 }
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("plain trailing lambdas and a next-line statement label parse");
    let cst = tree.to_string();
    assert!(cst.contains("AnnotatedLambda"));
    assert_eq!(cst.matches("Label").count(), 2);
    assert!(cst.contains("WhileStatement"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_explicit_lambda_parameters() {
    let source = r"fun transform(values: List<Int>): List<Int> {
    return values.map { value -> value + 1 }.filter { candidate: Int -> candidate > 0 }
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("explicit lambda parameters parse");
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn accepts_direct_arrows_in_zero_parameter_lambdas() {
    let source = "fun host() { val zero = { -> value }; call { -> other } }";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("a lambda arrow may appear without parameters");
    let cst = tree.to_string();
    assert_eq!(cst.matches("LambdaLiteral").count(), 2);
    assert!(!cst.contains('⚠'));
}

#[test]
fn direct_arrow_control_bodies_use_a_bounded_prefix_guard() {
    let source = r"fun host(condition: Boolean, values: List<Int>) {
    if (condition) { -> 1 } else { -> 0 }
    while (condition) { /* leading /* nested */ trivia */
        -> 2
    }
    for (value in values) { -> value }
    do { -> 3 } while (condition)
    if (condition) { // postfix ownership
        -> 4
    }()
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("a direct arrow selects a lambda without branching ordinary block parsing");
    let cst = tree.to_string();
    assert_eq!(cst.matches("LambdaLiteral").count(), 6, "{cst}");
    assert!(!cst.contains("directControlLambda"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn parameterized_control_lambdas_preserve_lambda_ownership() {
    let source = r"fun host(condition: Boolean, values: List<Int>) {
    if (condition) { value: Int -> value } else { -> 0 }
    while (condition) { value: Int -> value }
    for (item in values) { (left, right): Pair<Int, Int> -> left + right + item }
    do { value: Int, -> value } while (condition)
    when { condition -> { callback: () -> Int -> callback() }; else -> { return } }
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("parameterized lambda braces stay expressions in every control-body role");
    let cst = tree.to_string();
    assert_eq!(cst.matches("LambdaLiteral(").count(), 6, "{cst}");
    assert_eq!(cst.matches("Block(").count(), 2, "{cst}");
    assert_eq!(cst.matches("MultiVariableDeclaration(").count(), 1, "{cst}");
    assert_eq!(cst.matches("FunctionType(").count(), 1, "{cst}");
    assert!(!cst.contains('⚠'), "{cst}");
}

#[test]
fn accepts_casts_updates_ranges_safe_access_and_semicolons() {
    let source = r"fun update(value: Any?, count: Int): Int {
    var current = count; current++
    val cast = value as? Int
    val range = 0..<current
    return cast?.inc() ?: range.first
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("casts, updates, ranges, safe access, and semicolons parse");
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn nullable_type_operators_preserve_receiver_and_safe_access_roles() {
    let source = r"class Receiver
typealias NullableReceiverFunction = Receiver?.() -> Unit
typealias RepeatedNullableReceiverFunction = Receiver??.() -> Unit

fun Receiver?.extension(): Receiver? = this
fun Receiver??.repeated(): Receiver?? = this

fun classify(value: Any?, receiver: Receiver?): Any? {
    if (value is CharSequence?) value.length
    val cast = value as List<String>?
    receiver?.extension()
    return cast
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("nullable type operators remain distinct from receiver and safe-access roles");
    let function_receiver = source
        .find("Receiver?.()")
        .expect("function receiver is present")
        + "Receiver".len();
    let declaration_receiver = source
        .find("Receiver?.extension")
        .expect("declaration receiver is present")
        + "Receiver".len();
    let repeated_function_receiver = source
        .find("Receiver??.()")
        .expect("repeated function receiver is present")
        + "Receiver?".len();
    let repeated_declaration_receiver = source
        .find("Receiver??.repeated")
        .expect("repeated declaration receiver is present")
        + "Receiver?".len();
    let safe_access = source
        .find("receiver?.extension")
        .expect("safe access is present")
        + "receiver".len();
    let mut receiver_questions = Vec::new();
    tree.iterate(
        TextRange::new(0.into(), tree.len()),
        IterMode::INCLUDE_ANONYMOUS,
        |node| {
            let range = (usize::from(node.from()), usize::from(node.to()));
            if node.name().as_ref() == "?"
                && node.node_type().id() == rezel_lang_kotlin::terms::receiverQuestion
            {
                receiver_questions.push(range);
            }
            true
        },
        |_| {},
    );
    assert_eq!(
        receiver_questions,
        vec![
            (function_receiver, function_receiver + 1),
            (repeated_function_receiver, repeated_function_receiver + 1,),
            (declaration_receiver, declaration_receiver + 1),
            (
                repeated_declaration_receiver,
                repeated_declaration_receiver + 1,
            ),
        ]
    );
    assert!(!receiver_questions.contains(&(safe_access, safe_access + 1)));
    let cst = tree.to_string();
    assert!(cst.contains("TypeCheckExpression"));
    assert!(cst.contains("CastExpression"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn nullable_declaration_receivers_allow_trivia_before_the_dot() {
    let source = r"class Receiver
fun (Receiver)? .spaced() {}
fun (Receiver)?
.newline() {}
fun (Receiver)? /* outer /* inner */ tail */ .commented() {}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("nullable declaration receivers allow skipped trivia before their final dot");
    let cst = tree.to_string();
    assert_eq!(cst.matches("NullableType").count(), 3, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_postfix_updates() {
    let source = r"fun update(count: Int): Int {
    var current = count
    current++
    current--
    return current
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("postfix increment and decrement expressions parse");
    let cst = tree.to_string();
    assert_eq!(cst.matches("PostfixUpdateExpression").count(), 2);
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_object_literals() {
    let source = r#"interface Marker

fun plain(): Any = object {
    override fun toString(): String = "plain"
}

fun delegated(): Marker = object : Marker {}
"#;
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("plain and delegated object literals parse");
    let cst = tree.to_string();
    assert_eq!(cst.matches("ObjectLiteral").count(), 2);
    assert_eq!(cst.matches("DelegationSpecifiers").count(), 1);
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_a_semicolon_as_an_empty_for_body() {
    let source = r"fun consume(values: IntArray) {
    for (value in values);
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("a semicolon forms an empty for-loop body");
    let cst = tree.to_string();
    assert!(cst.contains("ForStatement"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_inline_property_getters() {
    let source = r"class Counter(private val value: Int) {
    val doubled: Int get() = value * 2
}

fun read(counter: Counter): Int = counter.doubled
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("an inline getter remains part of its property declaration");
    let cst = tree.to_string();
    assert!(cst.contains("Getter"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_newline_property_getters_and_local_properties() {
    let source = r"class Counter(private val value: Int) {
    val doubled: Int
        get() = value * 2
}

fun read(counter: Counter): Int {
    val local = counter.doubled
    return local
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("member getters and local properties use their own grammar paths");
    let cst = tree.to_string();
    assert!(cst.contains("Getter"));
    assert!(cst.matches("PropertyDeclaration").count() >= 2);
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_getter_then_setter_property_accessors() {
    let source = r"class Counter {
    var value: Int = 0
        get() = field
        set(input: Int) { field = input }
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("a setter follows its property's getter");
    let cst = tree.to_string();
    assert!(cst.contains("Getter"));
    assert!(cst.contains("Setter"));
    assert!(!cst.contains("PropertyAccessors"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn semicolon_prefixed_accessors_remain_owned_by_the_property() {
    let source = r"annotation class Ann
val accessor: Int; @Ann private get() = 2
var value: Int = 0; context(item: Item) set(input) {}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("a semicolon before accessor modifiers stays inside the property");
    let cst = tree.to_string();
    assert_eq!(cst.matches("PropertyDeclaration(").count(), 2, "{cst}");
    assert_eq!(cst.matches("Getter(").count(), 1, "{cst}");
    assert_eq!(cst.matches("Setter(").count(), 1, "{cst}");
    assert!(cst.contains("ContextDeclarationModifier("), "{cst}");
    assert!(!cst.contains('⚠'), "{cst}");
}

#[test]
fn explicit_backing_fields_are_property_components() {
    let source = r"class ShoppingCart {
    val items: List<String>
        field: MutableList<String> = mutableListOf()

    fun addItem(item: String) { items.add(item) }
}

class ParserSurface {
    var value: Int
        private field: String = source
        get() = field.length
        set(input) {}
}

val top: Int = source; field: String = source
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("explicit backing fields stay inside their property declaration");
    let cst = tree.to_string();
    assert_eq!(cst.matches("BackingField(").count(), 3, "{cst}");
    assert_eq!(cst.matches("Getter(").count(), 1, "{cst}");
    assert_eq!(cst.matches("Setter(").count(), 1, "{cst}");
    assert!(!cst.contains('⚠'), "{cst}");
}

#[test]
fn accepts_annotated_statements_when_tests_and_numeric_suffixes() {
    let source = r#"fun inspect(value: Any?, result: MutableList<Long>): Long {
    @Suppress("UNCHECKED_CAST")
    if (value is List<*>) return value.size.toLong()

    when (value) {
        is List -> result[0] = 0x10UL
        !in result -> result[0] += 1L
        else -> result[0] = 0b10L
    }
    @Suppress("UNCHECKED_CAST")
    return value as Long
}

fun Any?.description(): String = toString()
"#;
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("annotated statements, when tests, and numeric suffixes parse");
    let cst = tree.to_string();
    assert_eq!(cst.matches("AnnotationEntry").count(), 2);
    assert!(cst.contains("IfExpression"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_official_numeric_literal_spellings() {
    let source = r"val upperHex = 0X1
val upperBinary = 0B0001_0010
val leadingDot = .1_1
val exponent = 6.022___137e+2_3f
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("official integer prefixes and real-literal spellings parse");
    let cst = tree.to_string();
    assert!(cst.contains("IntegerLiteral"));
    assert!(cst.contains("RealLiteral"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn malformed_unicode_character_escapes_remain_character_literals() {
    let source = r"fun host() { val short = '\u123'; val nonHex = '\u12xz' }
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("Kotlin parses quoted malformed Unicode escapes before reporting diagnostics");
    let expected = ["'\\u123'", "'\\u12xz'"];
    let mut actual = Vec::new();
    tree.iterate(
        TextRange::new(0.into(), tree.len()),
        IterMode::NONE,
        |node| {
            if KotlinCharacterLiteral::downcast_from(node.clone()).is_ok() {
                actual.push((
                    usize::from(node.from()),
                    usize::from(node.to()),
                    node.node_type().id(),
                ));
            }
            true
        },
        |_| {},
    );
    let expected = expected.map(|literal| {
        let from = source.find(literal).expect("literal is present");
        (
            from,
            from + literal.len(),
            rezel_lang_kotlin::terms::CharacterLiteral,
        )
    });
    assert_eq!(actual, expected);
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn annotation_arguments_and_function_parameters_resolve_after_parentheses() {
    let source = r#"typealias Plain = @Ann (T) -> R
typealias Nested = @Ann ((T) -> R) -> R
typealias Named = @Ann (value: T) -> R
typealias Stringy = @Ann (@Arg("parameter") T) -> R
typealias WithArguments = @Ann("annotation") (T) -> R
"#;
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("annotation arguments and function parameters resolve after their closing parens");
    let mut annotations = Vec::new();
    let mut function_types = 0;
    let mut value_arguments = 0;
    tree.iterate(
        TextRange::new(0.into(), tree.len()),
        IterMode::NONE,
        |node| {
            match node.name().as_ref() {
                "AnnotationEntry" => {
                    let from = usize::from(node.from());
                    let to = usize::from(node.to());
                    annotations.push(&source[from..to]);
                }
                "FunctionType" => function_types += 1,
                "ValueArguments" => value_arguments += 1,
                _ => {}
            }
            true
        },
        |_| {},
    );
    assert_eq!(
        annotations,
        [
            "@Ann",
            "@Ann",
            "@Ann",
            "@Ann",
            "@Arg(\"parameter\")",
            "@Ann(\"annotation\")",
        ]
    );
    assert_eq!(function_types, 6);
    assert_eq!(value_arguments, 2);
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn accepts_definitely_non_nullable_types() {
    let source = r"fun <T> requireNonNull(value: T): T & Any = value as (T & Any)
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("definitely non-nullable return and cast types parse");
    let cst = tree.to_string();
    assert!(cst.contains("DefinitelyNonNullableType"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_multiline_type_constraints() {
    let source = r"abstract class MutableTable<Element, Table, TableBuilder>
        where Element : Comparable<Element>,
              Table : Any,
              TableBuilder : Any
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("type constraints continue after comma-separated newlines");
    let cst = tree.to_string();
    assert_eq!(cst.matches("TypeConstraint(").count(), 3);
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_assignments_continued_after_the_operator() {
    let source = r"fun update() {
    var result = 0
    result =
        1
    result +=
        2
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("an assignment continues after its operator");
    let cst = tree.to_string();
    assert_eq!(cst.matches("AssignmentStatement(").count(), 2);
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_comments_and_newlines_before_else() {
    let source = r"fun choose(flag: Boolean): Int {
    if (flag) {
        return 1
    }
    // The comment splits the surrounding physical newlines.

    else {
        return 2
    }
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("else follows the official repeated-newline boundary");
    let cst = tree.to_string();

    assert_eq!(cst.matches("IfExpression(").count(), 1, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_newlines_inside_if_conditions() {
    let source = r"fun choose(left: Boolean, right: Boolean): Int {
    return if (
        left &&
        right
    ) 1 else 0
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("an if condition permits newlines inside its parentheses");
    let cst = tree.to_string();
    assert_eq!(cst.matches("IfExpression(").count(), 1);
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_function_bodies_after_newlines() {
    let source = r"fun top(): Int
    = 42

class Holder {
    fun block()
    {
    }
}

interface Contract {
    fun declarationOnly(): Int
    fun implemented(): Int
        = 7
}

fun host() {
    fun local(): Int
        = 1
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("function bodies follow the official newline boundary");
    let cst = tree.to_string();

    assert_eq!(cst.matches("FunctionDeclaration(").count(), 6, "{cst}");
    assert_eq!(cst.matches("FunctionBody(").count(), 5, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_secondary_constructor_suffixes_after_newlines() {
    let source = r"class Value private constructor(val value: Int) {
    constructor(value: String)
        : this(value.length)

    constructor()
    : this(0)
    {
    }

    constructor(flag: Boolean)
    {
    }
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("secondary-constructor suffixes follow official newline boundaries");
    let cst = tree.to_string();

    assert_eq!(cst.matches("SecondaryConstructor(").count(), 3, "{cst}");
    assert_eq!(
        cst.matches("ConstructorDelegationCall(").count(),
        2,
        "{cst}"
    );
    assert_eq!(cst.matches("Block(").count(), 2, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn keeps_identifier_comparisons_alive() {
    let source = r"fun scan(index: Int, size: Int, values: List<Int>): Int {
    while (index <
        size && values[index] > 0) return values[index]
    return values[index]
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("identifier comparisons remain alive");
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn accepts_bare_generic_calls_without_losing_comparisons() {
    let source = r"fun <T> collect(values: List<T>, index: Int, size: Int): List<T> {
    val first = ArrayList<T>()
    val second = arrayOfNulls<Any?>(size)
    val third = iterator<List<T>> { yield(values) }
    while (index < size && values[index] > first.size) return values
    return values
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("a completed call suffix resolves the type-argument/comparison ambiguity");
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn accepts_property_accessors() {
    let source = r"class Counter {
    private var storage: Int = 0

    var value: Int
        get() = storage

    var sink: Int = 0
        private set

    val doubled: Int get() = value * 2
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("property getters and setters parse");
    let cst = tree.to_string();
    assert!(cst.contains("Getter"));
    assert!(cst.contains("Setter"));
    assert!(cst.contains("Modifier"));
    assert!(!cst.contains("AccessorVisibilityModifier"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn modifier_led_members_end_same_line_property_initializers() {
    let source = r"class Functions {
    val value = 1 public fun next() {}
}
class Properties {
    val value = 1 public val next = 2
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("a modifier-led member ends the preceding property initializer");
    let cst = tree.to_string();
    assert_eq!(cst.matches("PropertyDeclaration(").count(), 3, "{cst}");
    assert_eq!(cst.matches("FunctionDeclaration(").count(), 1, "{cst}");
    assert_eq!(cst.matches("Modifier(").count(), 2, "{cst}");
    assert!(!cst.contains("Getter("), "{cst}");
    assert!(!cst.contains("Setter("), "{cst}");
    assert!(!cst.contains('⚠'), "{cst}");
}

#[test]
fn accepts_object_literals_infix_names_reals_and_detached_constructors() {
    let source = r"interface Source

value class Amount
@PublishedApi
internal constructor(val raw: Int) : Source {
    val epsilon: Float = 1.4E-45F
}

fun source(): Source = object : Source {
    val combined: Int = 1 and 3
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("object literals, infix names, reals, and detached constructors parse");
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn accepts_context_types_comments_and_multiline_parentheses() {
    let source = r#"class Holder {
    @SinceKotlin("2.2")
    // An intervening comment must not detach the modifier.
    fun <T, R> run(value: T, block: context(T) () -> R): R {
        val folded = (value.hashCode() > 0 ||
            value.hashCode() == 0)
        return block(value)
    }
}
"#;
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("context function types, modifier comments, and multiline parentheses parse");
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn accepts_line_string_interpolation() {
    let source = r#"fun render(value: String?): String =
    "NotNull(${if (value != null) "value=$value" else "value not initialized yet"}), literal=$"

fun String.self(): String = "$this"
fun escaped(value: String): String = "\$value"
"#;
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("line strings support expression, identifier, and escaped interpolation");
    let cst = tree.to_string();
    assert_eq!(cst.matches("StringLiteral").count(), 5);
    assert_eq!(cst.matches("InterpolatedExpression").count(), 1);
    assert_eq!(cst.matches("InterpolatedIdentifier").count(), 2);
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_multiline_string_interpolation() {
    let source = r#"fun render(value: String): String = """
first line
value=$value
length=${value.length}
literal dollar=$ and two quotes=""
"""
"#;
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("multiline strings support raw content and interpolation");
    let cst = tree.to_string();
    assert_eq!(cst.matches("LiteralConstant").count(), 1);
    assert_eq!(cst.matches("StringLiteral").count(), 1);
    assert_eq!(cst.matches("InterpolatedExpression").count(), 1);
    assert_eq!(cst.matches("InterpolatedIdentifier").count(), 1);
    assert!(!cst.contains('⚠'));

    let mut pending = vec![tree.top_node()];
    let mut typed_string = None;
    while let Some(node) = pending.pop() {
        typed_string = KotlinStringLiteral::downcast_from(node.clone()).ok();
        if typed_string.is_some() {
            break;
        }
        pending.extend(node.children());
    }
    assert!(typed_string.is_some());
}

#[test]
fn accepts_multi_dollar_string_interpolation() {
    let source = r#"class Host {
    fun render(outer: Int, inner: Int): String {
        val nested = $$"""$outer $${ $$$"""$$inner $$$inner""" } $$outer"""
        val line = $$"$outer $$outer"
        val escaped = $$"\$$outer $$outer"
        val single = $"""$outer"""
        val self = $$"$ literal, $$this/$${this}/$$thisX/$$this_"
        val plain = "$outer"
        return nested
    }
}"#;
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("multi-dollar strings parse strictly");
    let cst = tree.to_string();
    assert_eq!(cst.matches("StringLiteral").count(), 7);
    assert_eq!(cst.matches("InterpolatedExpression").count(), 2);
    assert_eq!(cst.matches("InterpolatedIdentifier").count(), 9);
    assert!(!cst.contains('⚠'));
}

#[test]
fn multi_dollar_string_runs_are_inspected_linearly() {
    struct CountingInput {
        inner: StringInput,
        character_reads: Arc<AtomicUsize>,
    }

    impl Input for CountingInput {
        fn len(&self) -> TextSize {
            self.inner.len()
        }

        fn chunk(&self, from: TextSize) -> Cow<'_, str> {
            self.character_reads.fetch_add(1, Ordering::Relaxed);
            self.inner.chunk(from)
        }

        fn read(&self, range: TextRange) -> Cow<'_, str> {
            self.inner.read(range)
        }

        fn is_boundary(&self, position: TextSize) -> bool {
            self.inner.is_boundary(position)
        }
    }

    fn measured_reads(width: usize) -> usize {
        let dollars = "$".repeat(width);
        let source = format!("fun linear(value: Int) {{ val text = {dollars}\"{dollars}value\" }}");
        let character_reads = Arc::new(AtomicUsize::new(0));
        let input: Arc<dyn Input> = Arc::new(CountingInput {
            inner: StringInput::try_new(source).unwrap(),
            character_reads: Arc::clone(&character_reads),
        });
        rezel_lang_kotlin::parser()
            .with_strict(true)
            .parse_input(input)
            .expect("long multi-dollar runs parse strictly");
        character_reads.load(Ordering::Relaxed)
    }

    let small_reads = measured_reads(2_048);
    let large_reads = measured_reads(4_096);
    assert!(
        large_reads <= small_reads * 5 / 2,
        "doubling dollar-run width increased character reads from {small_reads} to {large_reads}"
    );
}

#[test]
fn multiline_string_end_consumes_the_complete_quote_run() {
    let source =
        "fun host() { val four = \"\"\"content\"\"\"\"; val five = \"\"\"content\"\"\"\"\" }\n";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("a multiline string closing token owns every quote in its final run");
    let cst = tree.to_string();
    assert_eq!(cst.matches("StringLiteral").count(), 2, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_lambdas_as_regular_value_arguments() {
    let source = r"fun transform(value: Int): Any =
    consume(value, { item -> item + value }, { it })
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("lambdas in value argument lists parse as expressions");
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn accepts_basic_anonymous_functions() {
    let source = r"typealias Mapper = (Int) -> Int

annotation class Mark

fun mapper(offset: Int): Mapper =
    fun(value: Int): Int = value + offset

fun action(): () -> Unit = fun() {}

fun apply(transform: Mapper): Int = transform(1)

fun annotated(): Mapper = fun(@Mark value: Int): Int = value

fun call(): Int = apply(fun(value: Int): Int {
    return value + 1
})
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("typed anonymous functions with expression and block bodies parse");
    let cst = tree.to_string();
    assert_eq!(cst.matches("AnonymousFunction").count(), 4);
    assert_eq!(cst.matches("FunctionValueParameters").count(), 9);
    assert!(!cst.contains('⚠'));

    let mut pending = vec![tree.top_node()];
    let mut anonymous_functions = 0;
    while let Some(node) = pending.pop() {
        if KotlinAnonymousFunction::downcast_from(node.clone()).is_ok() {
            anonymous_functions += 1;
        }
        pending.extend(node.children());
    }
    assert_eq!(anonymous_functions, 4);
}

#[test]
fn anonymous_function_parameters_allow_optional_types_and_defaults() {
    let source = r"fun host() {
    val transform = fun(value, increment: Int = 1,): Int = value + increment
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("anonymous function parameters may omit types and provide defaults");
    let cst = tree.to_string();

    assert_eq!(cst.matches("AnonymousFunction(").count(), 1, "{cst}");
    assert_eq!(cst.matches("FunctionValueParameter(").count(), 2, "{cst}");
    assert_eq!(cst.matches("PropertyInitializer(").count(), 2, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_suspend_anonymous_functions() {
    let source = r"typealias SuspendMapper = suspend (Int) -> Int

fun expressionBody(): SuspendMapper =
    suspend fun(value: Int): Int = value + 1

fun blockBody(): SuspendMapper = suspend fun(value: Int): Int {
    return value + 1
}

fun localStatement() {
    suspend fun(value: Int): Int = value
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("suspend anonymous functions converge with function declarations");
    let cst = tree.to_string();
    assert_eq!(cst.matches("AnonymousFunction").count(), 3);
    assert!(!cst.contains('⚠'));
}

#[test]
fn anonymous_functions_reuse_declaration_modifier_lists() {
    let source = r"annotation class A

fun host() {
    val inlineValue = @A inline fun() {}
    val mixed = @A suspend @A inline fun() {}
    val contextual = context(item: Item) public fun() {}()
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("K1 anonymous functions accept the regular declaration modifier surface");
    let cst = tree.to_string();

    assert_eq!(cst.matches("AnonymousFunction(").count(), 3, "{cst}");
    assert_eq!(cst.matches("AnnotationEntry(").count(), 3, "{cst}");
    assert!(cst.contains("ContextDeclarationModifier("), "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn anonymous_functions_reuse_type_parameter_lists() {
    let source = r"annotation class Ann

val plain = fun<T,>(value: T): T = value
val annotated = @Ann fun<T: Comparable<T>>() {}
val contextual = context(item: Item) fun<T>() {}()
fun host(c: Boolean) { if (c) fun<T>() {} else Unit }
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("K1 anonymous functions accept declaration type-parameter lists");
    let cst = tree.to_string();

    assert_eq!(cst.matches("AnonymousFunction(").count(), 4, "{cst}");
    assert_eq!(cst.matches("TypeParameters(").count(), 4, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_receiver_anonymous_functions() {
    let source = r"class Outer {
    class Inner
}

fun String.named(suffix: String): String = this + suffix

val append: String.(String) -> String =
    fun String.(suffix: String): String = this + suffix

val read: Outer.Inner.() -> Int = fun Outer.Inner.(): Int {
    return hashCode()
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("anonymous receivers coexist with named extension declarations");
    let cst = tree.to_string();
    assert_eq!(cst.matches("AnonymousFunction").count(), 2);
    assert_eq!(cst.matches("FunctionDeclaration").count(), 1);
    assert!(!cst.contains('⚠'));
}

#[test]
fn bounds_anonymous_function_ambiguities() {
    use std::fmt::Write as _;

    let mut source =
        String::from("annotation class Mark\nclass Outer { class Inner }\nfun host() {\n");
    for index in 0..64 {
        writeln!(
            source,
            "val transform{index}: (Int) -> Int = fun(@Mark value: Int): Int = value"
        )
        .unwrap();
        writeln!(source, "suspend fun(value{index}: Int): Int = value{index}").unwrap();
        writeln!(
            source,
            "val receiver{index}: Outer.Inner.() -> Int = fun Outer.Inner.(): Int = {index}"
        )
        .unwrap();
        writeln!(
            source,
            "val contextual{index} = context(item{index}: Int) fun(value: Int): Int = value"
        )
        .unwrap();
    }
    source.push_str("}\n");

    let parser = rezel_lang_kotlin::parser()
        .with_strict(true)
        .with_limits(ParseLimits {
            max_stacks: 2,
            ..ParseLimits::default()
        });
    let tree = parser
        .parse(&source)
        .expect("anonymous parameter, modifier, and receiver ambiguities stay narrowly bounded");
    let cst = tree.to_string();
    assert_eq!(cst.matches("AnonymousFunction").count(), 256);
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_multi_annotations() {
    let source = r#"@[Suppress("DEPRECATION") OptIn(ExperimentalStdlibApi::class)]
fun annotated(): Unit = Unit
"#;
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("a bracketed multi-annotation follows the official annotation production");
    let cst = tree.to_string();
    assert_eq!(cst.matches("AnnotationEntry").count(), 1);
    assert_eq!(cst.matches("ValueArguments").count(), 2);
    assert!(cst.contains("CallableReference"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_primary_lambdas_bare_callable_references_and_labels() {
    let source = r"class Factory

fun use(flag: Boolean): Any {
    val constructor = ::Factory
    val transform = { value: Int -> value + 1 }
    outer@ while (flag) { break@outer }
    return constructor
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("primary lambdas, bare callable references, and statement labels parse");
    let cst = tree.to_string();
    assert!(cst.contains("LambdaLiteral"));
    assert!(cst.contains("CallableReference"));
    assert!(cst.contains("Label"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_generic_callable_references() {
    let source = r"class Factory

fun references(array: Array<Any?>) {
    val constructor = ::Factory
    val component = Class<*>::getComponentType
    val arrayClass = Array<Any?>::class.java
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("callable references accept generic receiver suffixes");
    let cst = tree.to_string();
    assert_eq!(cst.matches("CallableReference").count(), 3);
    assert_eq!(cst.matches("TypeProjection").count(), 3);
    assert!(!cst.contains('⚠'));
}

#[test]
fn lambda_bodies_keep_identifier_callable_references_as_statements() {
    let source = r"fun classify(value: Any?) = value?.let { it::class }
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("a lambda body without an arrow rolls its identifier prefix back to statements");
    let cst = tree.to_string();
    assert_eq!(cst.matches("LambdaLiteral").count(), 1);
    assert_eq!(cst.matches("CallableReference").count(), 1);
    assert!(!cst.contains('⚠'));
}

#[test]
fn nullable_postfix_receivers_form_callable_references() {
    let source = r"fun references(value: Any?) {
    Any?::toString
    value?.let { it?::class }
    value.foo()?::bar
    Array<*>?::contentToString
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("nullable type and postfix receivers retain their question before double colon");
    let cst = tree.to_string();
    assert_eq!(cst.matches("CallableReference").count(), 4);
    assert!(!cst.contains('⚠'));
}

#[test]
fn safe_member_assignments_win_over_expression_prefixes() {
    let source = r"fun expose(value: Any?, flag: Boolean) {
    value?.isAccessible = flag
    value?.count += 1
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("an assignment operator owns a completed safe-member target on the same line");
    let cst = tree.to_string();
    assert_eq!(cst.matches("AssignmentStatement").count(), 2);
    assert_eq!(cst.matches("MemberExpression").count(), 2);
    assert!(!cst.contains('⚠'));
}

#[test]
fn assigns_through_parenthesized_cast_receivers() {
    let targets = [
        "(value as? Accessible)?.isAccessible",
        "(this.asReflectCallable() as? Accessible)?.isAccessible",
        "(this.asReflectCallable()?.member as? Accessible)?.isAccessible",
        "(this.asReflectCallable()?.callerWithDefaults?.member as? Accessible)?.isAccessible",
    ];
    for target in targets {
        let source = format!("fun expose(value: Any?) {{\n    {target} = true\n}}\n");
        let tree = rezel_lang_kotlin::parser()
            .with_strict(true)
            .parse(&source)
            .unwrap_or_else(|error| panic!("assignable target failed: {error}\n{source}"));
        let cst = tree.to_string();
        assert_eq!(cst.matches("AssignmentStatement").count(), 1, "{source}");
        assert_eq!(cst.matches("CastExpression").count(), 1, "{source}");
        assert!(cst.contains("MemberExpression"), "{source}");
        assert!(!cst.contains('⚠'));
    }

    let statements = targets
        .map(|target| format!("    {target} = true"))
        .join("\n");
    let source = format!("fun expose(value: Any?) {{\n{statements}\n}}\n");
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(&source)
        .unwrap_or_else(|error| panic!("assignment sequence failed: {error}\n{source}"));
    assert_eq!(
        tree.to_string().matches("AssignmentStatement").count(),
        targets.len()
    );
}

#[test]
fn anonymous_functions_are_selected_by_parameters_after_fun() {
    let source = r#"fun select(flag: Boolean): Any =
    if (flag) null else fun(): String { return "block" }

fun expression(): Any = fun(): String = "expression"
"#;
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("an opening parenthesis after fun selects an anonymous function expression");
    let cst = tree.to_string();
    assert_eq!(cst.matches("AnonymousFunction").count(), 2);
    assert!(!cst.contains('⚠'));
}

#[test]
fn secondary_constructor_parameters_keep_declaration_roles() {
    let source = r"interface Printer

class Smart(private val printer: Printer) : Printer by printer {
    constructor(text: String, offset: Int = 0) : this(printer)
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("secondary-constructor parameters retain declaration names before their types");
    let cst = tree.to_string();
    assert_eq!(cst.matches("SecondaryConstructor").count(), 1);
    assert_eq!(cst.matches("ConstructorDelegationCall").count(), 1);
    assert!(!cst.contains('⚠'));
}

#[test]
fn generic_calls_accept_function_type_arguments() {
    let source = r"fun render() {
    property<(String) -> String>({ it })
    value.unsafeCast<() -> Any?>().invoke()
    property<((String) -> String)?>({ null })
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("generic calls accept direct and nullable function-type arguments");
    let cst = tree.to_string();
    assert_eq!(cst.matches("FunctionType(").count(), 3);
    assert_eq!(cst.matches("TypeArguments(").count(), 3);
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_suspend_function_types() {
    let source = r"public fun <T> contextValue(block: suspend () -> T): T = block()
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("suspend function types parse");
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn single_parameter_function_types_share_the_parenthesized_prefix() {
    let source = r"typealias Transform = (String) -> String

fun cast(value: Any): Transform = value as (String) -> String
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("an arrow after a single parenthesized type owns the shared prefix");
    let cst = tree.to_string();
    assert_eq!(cst.matches("FunctionType(").count(), 2, "{cst}");
    assert_eq!(cst.matches("FunctionTypeParameters(").count(), 2, "{cst}");
    assert!(cst.contains("CastExpression"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_labeled_this_references() {
    let source = r"open class Parent
class Child : Parent() {
    fun self(): Child = this@Child
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("labeled this references parse");
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn accepts_labeled_jumps() {
    let source = r"fun select(flag: Boolean): Int {
        if (flag) return@select 1
        while (flag) { continue@outer }
        break@outer
        return 0
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("labeled jumps follow the official postfix forms");
    let cst = tree.to_string();
    assert!(cst.contains("ReturnAt"));
    assert!(cst.contains("ContinueAt"));
    assert!(cst.contains("BreakAt"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_super_qualifiers() {
    let source = r"open class Parent {
    open fun value(): Int = 1
}
class Child : Parent() {
    override fun value(): Int = super.value()
    fun inherited(): Int = super<Parent>@Child.hashCode()
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("super type and label qualifiers follow the official primary expression");
    let cst = tree.to_string();
    assert_eq!(cst.matches("SuperExpression").count(), 2);
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_detached_constructor_parameters_and_return_types() {
    let source = r"class Holder
    internal constructor
    (block: () -> Unit) {
    val value:
        Int = 1
}
@Marker class Following
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("detached constructor parameters and multiline return types parse");
    let cst = tree.to_string();
    assert_eq!(cst.matches("ClassDeclaration").count(), 2);
    assert_eq!(cst.matches("PrimaryConstructor").count(), 1);
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_annotated_value_parameters() {
    let source = r"typealias Callback = (@BuilderInference value: Int, @Other vararg values: String = default) -> Unit
fun <T> sequence(
    @BuilderInference block: suspend SequenceScope<T>.() -> Unit,
): Sequence<T> = Sequence { iterator(block) }
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("parameter annotations follow the official parameter-modifier production");
    let cst = tree.to_string();
    assert_eq!(cst.matches("FunctionTypeParameter").count(), 4);
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_annotated_receiver_types() {
    let source = r"public inline val @receiver:AccessibleLateinitPropertyLiteral KProperty0<*>.isInitialized: Boolean = true
public fun @receiver:AccessibleLateinitPropertyLiteral String.marked(): String = this
public fun (suspend () -> String).invokeNow(): String = this()
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("receiver annotations belong to receiver-type modifiers");
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn accepts_annotated_anonymous_function_receivers() {
    let source = r"val anonymous = fun @Receiver String.() {}";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("anonymous function receivers accept type modifiers");
    let cst = tree.to_string();
    assert!(cst.contains("AnonymousFunction"));
    assert!(cst.contains("TypeModifiers"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_property_delegates() {
    let source = r"class Holder {
    val member: Int
        by lazy { 42 }
}

val answer: Int
    by
    lazy { 42 }

fun local() {
    val value: Int
        by lazy { 42 }
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("property delegates follow the official by-expression branch");
    let cst = tree.to_string();
    assert_eq!(cst.matches("PropertyDelegate").count(), 3, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_local_destructuring_declarations() {
    let source = r"fun inspect(pair: Pair<Int, String>): String {
    val (index = fallback, value: String) = pair
    val [left, right] = pair
    return value
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("local destructuring follows the official multi-variable production");
    let cst = tree.to_string();
    assert_eq!(cst.matches("MultiVariableDeclaration").count(), 2);
    assert_eq!(cst.matches("PropertyInitializer").count(), 3);
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_destructuring_in_for_loops() {
    let source = r"fun inspect(values: List<Pair<Int, String>>): Int {
    for ((index: Int, value: String) in values) consume(index, value)
    return values.size
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("for loops reuse the official multi-variable declaration");
    let cst = tree.to_string();
    assert!(cst.contains("MultiVariableDeclaration"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn destructuring_entries_own_their_annotations() {
    let source = r"annotation class Mark
fun inspect(pair: Pair<Int, Int>, pairs: List<Pair<Int, Int>>) {
    val (@Mark left: Int, right) = pair
    for ((@Mark first: Int, second) in pairs) consume(first, second)
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("annotations are direct children of destructuring variable declarations");
    let cst = tree.to_string();
    assert_eq!(cst.matches("MultiVariableDeclaration").count(), 2, "{cst}");
    assert_eq!(cst.matches("AnnotationEntry").count(), 2, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn for_variables_accept_annotations_and_declaration_keywords() {
    let source = r"annotation class Mark
fun inspect(values: List<Int>, pairs: List<Pair<Int, Int>>) {
    for (@Mark item in values) consume(item)
    for (@Mark val item in values) consume(item)
    for (@Mark var (left, right) in pairs) consume(left, right)
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("for variables preserve annotations and optional declaration keywords");
    let cst = tree.to_string();
    assert_eq!(cst.matches("AnnotationEntry").count(), 3);
    assert_eq!(cst.matches("MultiVariableDeclaration").count(), 1);
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_destructuring_lambda_parameters() {
    let source = r"fun inspect(values: List<Pair<Int, String>>): Int {
    return values.fold(0) { (total, ignored), item -> total + 1 }
}

";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("lambda parameters reuse the official multi-variable declaration");
    let cst = tree.to_string();
    assert!(cst.contains("MultiVariableDeclaration"));
    assert!(!cst.contains('⚠'));
}

#[test]
fn destructuring_lambda_entries_accept_k1_defaults_and_brackets() {
    let source = r"fun host() {
    val first = { (left = source, right) -> left }
    val second = { [left, right] -> right }
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("K1 lambda destructuring keeps entry defaults and bracket delimiters");
    let cst = tree.to_string();
    assert_eq!(cst.matches("LambdaLiteral(").count(), 2, "{cst}");
    assert_eq!(cst.matches("MultiVariableDeclaration(").count(), 2, "{cst}");
    assert_eq!(cst.matches("PropertyInitializer(").count(), 3, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_context_parameters_on_functions() {
    let source = r"@InlineOnly
context(context: @NoInfer A)
public inline fun <A> contextOf(): @NoInfer A = context
context(Item) typealias Legacy = String
public context(first: A, second: B = default) suspend fun host() {}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("context parameters are function-specific modifiers");
    let cst = tree.to_string();
    assert_eq!(cst.matches("ContextDeclarationModifier").count(), 3);
    assert!(!cst.contains('⚠'));
}

#[test]
fn accepts_nested_block_comments() {
    let source = r"fun value(): Int {
    /* outer
       /* nested
          /* deep */
          still nested */
       still outer */
    return 42
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("nested block comments use recursive CFG inside a local token group");
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn layout_distinguishes_continuations_from_statement_boundaries() {
    let source = r"fun layout(a: Int, b: Int, value: Any?, flag: Boolean): Any? {
    val sum = a +
        b
    a
    b
    val member = value
        ?.hashCode()
    val casted = value
        as? Any
    val fallback = value
        ?: b
    val logical = flag
        && true
    do { a }
    while (flag)
    return fallback
}
";
    for candidate in [source.to_owned(), source.replace('\n', "\r\n")] {
        let tree = rezel_lang_kotlin::parser()
            .with_strict(true)
            .parse(&candidate)
            .expect("layout continuations and statement boundaries parse for LF and CRLF");
        let cst = tree.to_string();
        assert!(cst.contains("MemberExpression"));
        assert!(cst.contains("CastExpression"));
        assert!(cst.contains("DoWhileStatement"));
        assert!(!cst.contains('⚠'));
    }
}

#[test]
fn layout_defers_partial_fast_window_keywords() {
    let padding = " ".repeat(61);
    let source = format!("fun choose(flag: Boolean): Int = if (flag) 1\n{padding}else 2\n");
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(&source)
        .expect("a continuation keyword crossing the fast window stays structural");
    assert!(tree.to_string().contains("IfExpression"));
}

#[test]
fn layout_treats_block_comment_line_breaks_as_lexically_hidden() {
    let source = r"fun comments(a: Int, b: Int) {
    val inline = a /* same line */ + b
    a /* physical
         /* nested */ line */ +b
    a // line comment
    +b
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("only line-comment terminators create layout boundaries");
    let cst = tree.to_string();
    assert_eq!(cst.matches("BinaryExpression(").count(), 2, "{cst}");
    assert_eq!(cst.matches("UnaryExpression(").count(), 1, "{cst}");
}

#[test]
fn block_comment_line_breaks_do_not_split_jump_operands() {
    let source = r"fun returns(value: Int): Int {
    return /* physical
        line */ value
}

fun nested(value: Int): Int {
    return /* outer /* nested
        line */ tail */ value
}

fun throws(error: Throwable) {
    throw /* physical
        line */ error
}
";
    for candidate in [source.to_owned(), source.replace('\n', "\r\n")] {
        let tree = rezel_lang_kotlin::parser()
            .with_strict(true)
            .parse(&candidate)
            .expect("block comments are hidden trivia for K1 jump ownership");
        let cst = tree.to_string();
        assert_eq!(cst.matches("ReturnExpression(").count(), 2, "{cst}");
        assert_eq!(cst.matches("ThrowExpression(").count(), 1, "{cst}");
        assert!(!cst.contains('⚠'));
    }
}

#[test]
fn annotated_trailing_lambdas_follow_the_official_call_suffix_shape() {
    let source = r"annotation class Ann
fun host() {
    foo @Ann { value }
    foo @Ann label@ { value }
    foo<Int>@Ann label@ { value }
}
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("annotations and an optional label belong to the trailing lambda suffix");
    let cst = tree.to_string();
    assert_eq!(cst.matches("CallExpression(").count(), 3, "{cst}");
    assert_eq!(cst.matches("AnnotatedLambda(").count(), 3, "{cst}");
    assert_eq!(cst.matches("LambdaLiteral(").count(), 3, "{cst}");
    assert_eq!(cst.matches("Label(").count(), 2, "{cst}");
    assert!(!cst.contains('⚠'));
}

#[test]
fn adjacent_annotations_preserve_type_and_label_ownership() {
    let source = r"annotation class First
annotation class Second

@First@Second class Plain
@sample.First@sample.Second class Qualified
@`First`@`Second` class Escaped

fun labels() { loop@ while (false) break@loop }
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("adjacent annotations and labels retain distinct lexical ownership");
    let cst = tree.to_string();
    assert_eq!(cst.matches("AnnotationEntry(").count(), 6, "{cst}");
    assert_eq!(cst.matches("ClassDeclaration(").count(), 5, "{cst}");
    assert_eq!(cst.matches("Label(").count(), 1, "{cst}");
    assert!(!cst.contains('⚠'), "{cst}");
}

#[test]
fn layout_keeps_return_and_throw_operands_on_their_physical_line() {
    let source = r"fun returns(flag: Boolean): Int {
    if (flag) return 1
    return
    2
}

fun throws(error: Throwable) { throw error }
";
    let tree = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse(source)
        .expect("same-line return and throw operands parse");
    let cst = tree.to_string();
    assert_eq!(cst.matches("ReturnExpression(").count(), 2, "{cst}");
    assert!(cst.contains("ReturnExpression(return)"), "{cst}");
}

#[test]
fn rejects_throw_operands_on_the_next_physical_line() {
    let error = rezel_lang_kotlin::parser()
        .with_strict(true)
        .parse("fun fail(error: Throwable) { throw\nerror }")
        .expect_err("a required throw operand cannot start on the next physical line");
    assert_eq!(error.kind(), ParseErrorKind::Syntax);
}

#[test]
fn layout_synthetic_terms_are_invisible_and_progress_is_bounded() {
    let source = "fun stable(a: Int, b: Int) {\n\n a +\n b;\n\n a\n b\n sequence result@ { return@result }\n a to b\n}\nclass Detached\ninternal constructor()\n@Marker class Following\n";
    let parser = rezel_lang_kotlin::parser()
        .with_strict(true)
        .with_limits(ParseLimits {
            max_stacks: 2,
            ..ParseLimits::default()
        });
    for _ in 0..2 {
        let tree = parser
            .parse(source)
            .expect("zero-width layout terms make finite progress under a narrow stack budget");
        assert_eq!(tree.len(), source.len().try_into().unwrap());
        let cst = tree.to_string();
        for hidden in [
            "lineBreakTrivia",
            "sameLineJump",
            "sameLineLambda",
            "sameLineRange",
            "sameLineNegatedTypeOperator",
            "sameLineOperator",
            "sameLineIdentifier",
            "lineBreakPrefix",
            "insertedStatementEnd",
        ] {
            assert!(!cst.contains(hidden), "{hidden} leaked into the CST: {cst}");
        }
    }

    let recovery = rezel_lang_kotlin::parser()
        .parse("fun recover() { value\n)\n}")
        .expect("layout insertion remains compatible with recovery");
    assert!(recovery.to_string().contains('⚠'));
}

#[test]
fn layout_respects_selected_ranges() {
    let source = "?fun ???host() {\nvalue\n}";
    let input: Arc<dyn Input> = Arc::new(StringInput::try_new(source).unwrap());
    let request = ParseRequest::ranges(
        input,
        vec![
            TextRange::new(0.into(), 0.into()),
            TextRange::new(1.into(), 5.into()),
            TextRange::new(8.into(), source.len().try_into().unwrap()),
        ],
    )
    .expect("the selected ranges are valid UTF-8 boundaries");
    let mut parse = rezel_lang_kotlin::parser()
        .with_strict(true)
        .create_parse(request)
        .expect("the selected-range Kotlin parse starts");
    let tree = loop {
        if let Some(tree) = parse
            .advance()
            .expect("the selected logical Kotlin source parses")
        {
            break tree;
        }
    };
    assert!(tree.to_string().contains("FunctionDeclaration"));
    assert!(!tree.to_string().contains('⚠'));
}

#[test]
fn layout_comment_inspection_is_linear() {
    struct CountingInput {
        inner: StringInput,
        read_bytes: Arc<AtomicUsize>,
    }

    impl Input for CountingInput {
        fn len(&self) -> TextSize {
            self.inner.len()
        }

        fn chunk(&self, from: TextSize) -> Cow<'_, str> {
            self.inner.chunk(from)
        }

        fn read(&self, range: TextRange) -> Cow<'_, str> {
            self.read_bytes
                .fetch_add(usize::from(range.end() - range.start()), Ordering::Relaxed);
            self.inner.read(range)
        }

        fn is_boundary(&self, position: TextSize) -> bool {
            self.inner.is_boundary(position)
        }
    }

    fn measured_reads(width: usize) -> (usize, usize) {
        let padding = "x".repeat(width);
        let source = format!("fun linear() {{ value /* {padding}\n{padding} */ + next }}\n");
        let read_bytes = Arc::new(AtomicUsize::new(0));
        let input: Arc<dyn Input> = Arc::new(CountingInput {
            inner: StringInput::try_new(source.as_str()).unwrap(),
            read_bytes: Arc::clone(&read_bytes),
        });
        rezel_lang_kotlin::parser()
            .with_strict(true)
            .parse_input(input)
            .expect("a long comment with a physical line break parses");
        (read_bytes.load(Ordering::Relaxed), source.len())
    }

    let (small_reads, small_bytes) = measured_reads(2_048);
    let (large_reads, large_bytes) = measured_reads(4_096);
    assert!(
        small_reads <= small_bytes * 8 && large_reads <= large_bytes * 8,
        "comment layout inspection exceeded its linear constant: {small_reads}/{small_bytes}, {large_reads}/{large_bytes}"
    );
    assert!(
        large_reads <= small_reads * 5 / 2,
        "doubling comment width increased inspected bytes from {small_reads} to {large_reads}"
    );
}
