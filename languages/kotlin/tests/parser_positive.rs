#![forbid(unsafe_code)]

// Every source in this inventory is accepted by the Kotlin 2.4.10 K1 light parser.
// This suite verifies recognition only; CST and typed syntax have separate gates.
const EXPECTED_K1_ACCEPTED: usize = 601;
const EXPECTED_STRICT_ACCEPTED: usize = 601;

// A uniform delimiter keeps mechanically added Kotlin sources readable even
// when only some entries contain quotes.
#[allow(clippy::needless_raw_string_hashes)]
const K1_ACCEPTED: &[(&str, &str)] = &[
    (
        "all_annotation_use_site_target_matches_kotlin_2_4-000",
        r#"annotation class Ann
class C(@all:Ann val x:Int)"#,
    ),
    (
        "all_annotation_use_site_target_matches_kotlin_2_4-001",
        r#"annotation class Ann
@all:Ann class C"#,
    ),
    (
        "all_annotation_use_site_target_matches_kotlin_2_4-002",
        r#"annotation class Ann
fun f(@all:Ann x:Int){}"#,
    ),
    (
        "all_annotation_use_site_target_matches_kotlin_2_4-003",
        r#"annotation class Ann
class C(@all : Ann val x:Int)"#,
    ),
    (
        "all_annotation_use_site_target_matches_kotlin_2_4-004",
        r#"annotation class A
annotation class B
class C(@all:[A B] val x:Int)"#,
    ),
    (
        "all_annotation_use_site_target_matches_kotlin_2_4-005",
        r#"fun all(all:Int){ val value=all }"#,
    ),
    (
        "an-000",
        r#"fun test(c:Boolean){ if(c)
fun String.(){}().member[0]!!++ else value; val global=fun(){}() }"#,
    ),
    (
        "anno-000",
        r#"
annotation class A
annotation class B
fun test(value: Any) {
    when (
        @A @B val subject = value
    ) {
        else -> Unit
    }
    try {
        Unit
    } catch (
        @A @B error: Exception,
    ) {
        Unit
    }
}
"#,
    ),
    (
        "annotated_and_labeled_trailing_lambdas_preserve_k1_ownership-000",
        r#"fun host(){ foo @Ann { value } }"#,
    ),
    (
        "annotated_and_labeled_trailing_lambdas_preserve_k1_ownership-001",
        r#"fun host(){ foo label@ { value } }"#,
    ),
    (
        "annotated_and_labeled_trailing_lambdas_preserve_k1_ownership-002",
        r#"fun host(){ foo() @Ann label@ { value } }"#,
    ),
    (
        "annotated_and_labeled_trailing_lambdas_preserve_k1_ownership-003",
        r#"fun host(){ foo @First @Second `label`@
{ value } }"#,
    ),
    (
        "annotated_and_labeled_trailing_lambdas_preserve_k1_ownership-004",
        r#"fun host(){ foo /* outer
 /* inner */ tail */ @Ann { value } }"#,
    ),
    (
        "annotated_and_labeled_trailing_lambdas_preserve_k1_ownership-005",
        r#"fun host(c:Boolean){ if(c) foo @Ann { value } else other }"#,
    ),
    (
        "annotated_and_labeled_trailing_lambdas_preserve_k1_ownership-006",
        r#"fun host(){ foo @Ann label@ { value } }"#,
    ),
    (
        "annotated_and_labeled_trailing_lambdas_require_the_calls_semantic_line-000",
        r#"fun host(){ foo
@Ann { value } }"#,
    ),
    (
        "annotated_and_labeled_trailing_lambdas_require_the_calls_semantic_line-001",
        r#"fun host(){ foo
label@ { value } }"#,
    ),
    (
        "annotated_and_labeled_trailing_lambdas_require_the_calls_semantic_line-002",
        r#"fun host(){ foo // end
@Ann { value } }"#,
    ),
    (
        "annotated_anonymous_functions_preserve_k1_identity_and_postfix_boundaries-000",
        r#"annotation class A
fun host(){ val value = @A fun(){} }"#,
    ),
    (
        "annotated_anonymous_functions_preserve_k1_identity_and_postfix_boundaries-001",
        r#"annotation class A
fun host(){ val value = @A fun String.(){} }"#,
    ),
    (
        "annotated_anonymous_functions_preserve_k1_identity_and_postfix_boundaries-002",
        r#"annotation class A
fun host(){ val value = @A fun List<String>.(){} }"#,
    ),
    (
        "annotated_anonymous_functions_preserve_k1_identity_and_postfix_boundaries-003",
        r#"annotation class A
fun host(){ val value = @A fun (String).(){} }"#,
    ),
    (
        "annotated_anonymous_functions_preserve_k1_identity_and_postfix_boundaries-004",
        r#"annotation class A
fun host(){ val value = @A fun `Type`.(){} }"#,
    ),
    (
        "annotated_anonymous_functions_preserve_k1_identity_and_postfix_boundaries-005",
        r#"annotation class A
fun host(){ val value = @A fun 类型.(){} }"#,
    ),
    (
        "annotated_anonymous_functions_preserve_k1_identity_and_postfix_boundaries-006",
        r#"annotation class A
fun host(){ val value = @A fun /* header */
(){} }"#,
    ),
    (
        "annotated_anonymous_functions_preserve_k1_identity_and_postfix_boundaries-007",
        r#"annotation class A
fun host(){ val value = @A suspend fun(){} }"#,
    ),
    (
        "annotated_anonymous_functions_preserve_k1_identity_and_postfix_boundaries-008",
        r#"annotation class A
fun host(){ val value = @A inline fun(){} }"#,
    ),
    (
        "annotated_anonymous_functions_preserve_k1_identity_and_postfix_boundaries-009",
        r#"annotation class A
fun host(){ val value = @A suspend @A inline fun(){} }"#,
    ),
    (
        "annotated_anonymous_functions_preserve_k1_identity_and_postfix_boundaries-010",
        r#"annotation class A
fun host(){ val value = (@A fun(){})() }"#,
    ),
    (
        "annotated_anonymous_functions_preserve_k1_identity_and_postfix_boundaries-011",
        r#"annotation class A
@A fun named(){}"#,
    ),
    (
        "annotated_anonymous_functions_preserve_k1_identity_and_postfix_boundaries-012",
        r#"annotation class A
@A fun<T> generic(){}"#,
    ),
    (
        "annotated_anonymous_functions_preserve_k1_identity_and_postfix_boundaries-013",
        r#"annotation class A
@A fun String.named(){}"#,
    ),
    (
        "annotated_anonymous_functions_preserve_k1_identity_and_postfix_boundaries-014",
        r#"annotation class A
@A fun (String).named(){}"#,
    ),
    (
        "annotated_anonymous_functions_preserve_k1_identity_and_postfix_boundaries-015",
        r#"annotation class A
@A fun `named`(){}"#,
    ),
    (
        "annotated_anonymous_functions_preserve_k1_identity_and_postfix_boundaries-016",
        r#"annotation class A
@A suspend fun named(){}"#,
    ),
    (
        "annotated_anonymous_functions_preserve_k1_identity_and_postfix_boundaries-017",
        r#"annotation class A
@A fun interface Named {}"#,
    ),
    (
        "annotated_anonymous_functions_preserve_k1_identity_and_postfix_boundaries-018",
        r#"annotation class A
fun host(){ @A funny() }"#,
    ),
    (
        "annotated_anonymous_functions_preserve_k1_identity_and_postfix_boundaries-019",
        r#"annotation class A
fun host(c:Boolean){ if(c)
@A fun(){} else @A fun() = 1 }"#,
    ),
    (
        "annotated_anonymous_functions_preserve_k1_identity_and_postfix_boundaries-020",
        r#"annotation class A
fun host(c:Boolean){ if(c) @A suspend fun(){} else value }"#,
    ),
    (
        "annotated_anonymous_functions_preserve_k1_identity_and_postfix_boundaries-021",
        r#"annotation class A
fun host(c:Boolean){ val global=@A fun String.(){}; if(c) @A fun(){} else @A fun() = 1 }"#,
    ),
    (
        "annotated_assi-000",
        r#"annotation class A
fun foo(): Int = 0
fun host(condition: Boolean, values: IntArray, index: Int) {
    var name = 0
    @A name = 1
    @A values.size = 2
    @A values[index] += 1
    @A foo() = 1
    @A name!! += 1
    @A name++ += 1
    @A -name += 1
    @A { name } += 1
    if (condition) @A name = 2 else @A values[index] += 2
}
"#,
    ),
    (
        "annotated_co-000",
        r#"annotation class A
fun host(condition: Boolean, values: List<Int>): Int {
    @A while (condition) break
    @A for (value in values) continue
    @A do break while (condition)
    if (condition) @A while (condition) break else @A return 1
    when { condition -> @A return 2; else -> Unit }
    return 3
}
"#,
    ),
    (
        "annotated_control_and_jump_expressions_preserve_outer_boundaries-000",
        r#"annotation class A
fun f(c:Boolean){ if(c) @A while(c) break
else Unit }"#,
    ),
    (
        "annotated_control_and_jump_expressions_preserve_outer_boundaries-001",
        r#"annotation class A
fun f(c:Boolean){ while(c) @A continue }"#,
    ),
    (
        "annotated_control_and_jump_expressions_preserve_outer_boundaries-002",
        r#"annotation class A
fun f(c:Boolean){ do @A break while(c) }"#,
    ),
    (
        "annotated_control_and_jump_expressions_preserve_outer_boundaries-003",
        r#"annotation class A
fun f(c:Boolean){ @A @A while(c) break }"#,
    ),
    (
        "annotated_expr-000",
        r#"annotation class A
annotation class B
fun consume(vararg values: Any?) = Unit
fun host(condition: Boolean, value: Int) {
    val local = @A value
    consume(@A value + 1)
    consume(@A consume(value).hashCode())
    consume(@A { value })
    consume(@A @B value)
    if (condition) @A consume(value) else @A Unit
    val literal = @A object {}
    @A object Named {}
    @A fun named() {}
    label@ @A value
    return
    @A value
}
"#,
    ),
    (
        "annotated_expression_unary_and_return_boundaries_match_k1-000",
        r#"fun f() { label@ value }"#,
    ),
    (
        "annotated_expression_unary_and_return_boundaries_match_k1-001",
        r#"annotation class A
fun f() { label@ @A value }"#,
    ),
    (
        "annotated_expression_unary_and_return_boundaries_match_k1-002",
        r#"annotation class A
fun f(x:Int)=@A -x"#,
    ),
    (
        "annotated_expression_unary_and_return_boundaries_match_k1-003",
        r#"annotation class A
fun f(x:Int)=@A +x"#,
    ),
    (
        "annotated_expression_unary_and_return_boundaries_match_k1-004",
        r#"annotation class A
fun f(x:Int)=@A ++x"#,
    ),
    (
        "annotated_expression_unary_and_return_boundaries_match_k1-005",
        r#"annotation class A
fun f(x:Int)=@A --x"#,
    ),
    (
        "annotated_expression_unary_and_return_boundaries_match_k1-006",
        r#"annotation class A
fun f(x:Int):Int { return (@A x) }"#,
    ),
    (
        "annotated_expression_unary_and_return_boundaries_match_k1-007",
        r#"annotation class A
fun f() { return
@A value }"#,
    ),
    (
        "annotated_expression_unary_and_return_boundaries_match_k1-008",
        r#"annotation class A
fun f() = @A object /* nested /* body */ */ {}"#,
    ),
    (
        "annotated_expression_unary_and_return_boundaries_match_k1-009",
        r#"annotation class A
fun f() = @A objectName"#,
    ),
    (
        "annotation_at_signs_are_attached_and_contextual-000",
        r#"@Ann("x") class C"#,
    ),
    (
        "annotation_at_signs_are_attached_and_contextual-001",
        r#"@First@Second class C"#,
    ),
    (
        "annotation_at_signs_are_attached_and_contextual-002",
        r#"fun test(){ loop@ while(false) break@loop; this@Outer }"#,
    ),
    (
        "annotation_at_signs_are_attached_and_contextual-003",
        r#"@sample.First@sample.Second class Qualified"#,
    ),
    (
        "annotation_at_signs_are_attached_and_contextual-004",
        r#"@`First`@`Second` class Escaped"#,
    ),
    (
        "annotation_first_declarations_are_control_structure_bodies-000",
        r#"@A fun local(){}"#,
    ),
    (
        "annotation_first_declarations_are_control_structure_bodies-001",
        r#"@A fun interface Local{}"#,
    ),
    (
        "annotation_first_declarations_are_control_structure_bodies-002",
        r#"@A private class Local{}"#,
    ),
    (
        "annotation_first_declarations_are_control_structure_bodies-003",
        r#"@A enum class Local{ Entry }"#,
    ),
    (
        "annotation_first_declarations_are_control_structure_bodies-004",
        r#"@A object Local{}"#,
    ),
    (
        "annotation_first_declarations_are_control_structure_bodies-005",
        r#"@A typealias Local=String"#,
    ),
    (
        "annotation_first_declarations_are_control_structure_bodies-006",
        r#"annotation class A
fun host(c:Boolean){{ if(c) {snippet} else Unit }}"#,
    ),
    (
        "annotation_first_declarations_are_control_structure_bodies-007",
        r#"annotation class A
fun host(c:Boolean){ if(c)
@A fun local(){} else Unit }"#,
    ),
    (
        "annotation_first_declarations_are_control_structure_bodies-008",
        r#"annotation class A
fun host(c:Boolean){ if(c) /* outer
 /* inner */ tail */ @A suspend fun local(){} else Unit }"#,
    ),
    (
        "annotation_first_declarations_are_control_structure_bodies-009",
        r#"annotation class A
fun host(c:Boolean){ while(c) @A annotation class Local }"#,
    ),
    (
        "annotation_first_declarations_are_control_structure_bodies-010",
        r#"annotation class A
fun host(xs:List<Int>){ for(x in xs) @A val (left,right)=Pair(x,x) }"#,
    ),
    (
        "annotation_first_declarations_are_control_structure_bodies-011",
        r#"annotation class A
fun host(c:Boolean){ do @A object Local{}
while(c) }"#,
    ),
    (
        "annotation_first_declarations_are_control_structure_bodies-012",
        r#"annotation class A
fun host(c:Boolean){ when { c -> @A typealias Local=String; else -> Unit } }"#,
    ),
    (
        "annotation_first_declarations_are_control_structure_bodies-013",
        r#"annotation class A
fun host(c:Boolean){ if(c) @A fun <T> String.local(value:T):T = value else Unit }"#,
    ),
    (
        "annotation_first_declarations_are_control_structure_bodies-014",
        r#"annotation class A
fun host(c:Boolean){ if(c) @A class Local<T> where T:Any {} /* boundary
 */ else Unit }"#,
    ),
    (
        "annotation_first_declarations_are_control_structure_bodies-015",
        r#"annotation class A
fun host(c:Boolean){ if(c) @A object {} else Unit }"#,
    ),
    (
        "annotation_first_declarations_are_control_structure_bodies-016",
        r#"annotation class A
fun host(c:Boolean){ if(c) @A fun(){} else Unit }"#,
    ),
    (
        "annotation_first_declarations_are_control_structure_bodies-017",
        r#"annotation class A
fun host(c:Boolean){ if(c) @A funny() else Unit }"#,
    ),
    (
        "annotations_attach_to_when_subjects_and_catch_parameters-000",
        r#"
fun test(value: Int, condition: Boolean) = when (value) {
    1, -> "one"
    2, 3, /* trailing */
        -> "few"
    4, [5] -> "collection"
    6, if (condition) 7 else 8 -> "conditional"
    else -> "other"
}
"#,
    ),
    (
        "annotations_preserve_use_site_and_multi_entry_shapes-000",
        r#"@field:Ann val x = 1"#,
    ),
    (
        "annotations_preserve_use_site_and_multi_entry_shapes-001",
        r#"@get:Ann val x = 1"#,
    ),
    (
        "annotations_preserve_use_site_and_multi_entry_shapes-002",
        r#"@field
:Ann val x = 1"#,
    ),
    (
        "annotations_preserve_use_site_and_multi_entry_shapes-003",
        r#"@[First Second] class C"#,
    ),
    (
        "annotations_preserve_use_site_and_multi_entry_shapes-004",
        r#"@field:[First Second(1)] val x = 1"#,
    ),
    (
        "annotations_reach_declaration_internal_positions-000",
        r#"fun f(@Ann x: Int, @Ann vararg ys: String) {}"#,
    ),
    (
        "annotations_reach_declaration_internal_positions-001",
        r#"fun <@Ann T> f(x: T) {}"#,
    ),
    (
        "annotations_reach_declaration_internal_positions-002",
        r#"fun <T> f(x: T) where @Ann T : Any {}"#,
    ),
    (
        "annotations_reach_declaration_internal_positions-003",
        r#"fun f(pair: Pair<Int, Int>) { val (@A x, @B y: Int) = pair }"#,
    ),
    (
        "annotations_reach_declaration_internal_positions-004",
        r#"fun f(xs: List<Int>) { for (@Ann x in xs) consume(x) }"#,
    ),
    (
        "annotations_reach_declaration_internal_positions-005",
        r#"fun f(value: Int) { when (@Ann val x = value) { else -> x } }"#,
    ),
    (
        "annotations_reach_declaration_internal_positions-006",
        r#"@Ann class C : @Delegate Base(), @Other Interface"#,
    ),
    (
        "annotations_reach_declaration_internal_positions-007",
        r#"class C : @Delegate Base(), @Other Interface"#,
    ),
    (
        "anon-000",
        r#"val plain = fun<T,>(value: T): T = value
val annotated = @Ann fun<T: Comparable<T>>() {}
val contextual = context(item: Item) fun<T>() {}()
fun host(c: Boolean) { if (c) fun<T>() {} else Unit }"#,
    ),
    (
        "anonymous_function_control_bodies_follow_k1_postfix_boundaries-000",
        r#"fun test(c:Boolean){ if(c) fun(){} else value }"#,
    ),
    (
        "anonymous_function_control_bodies_follow_k1_postfix_boundaries-001",
        r#"fun test(c:Boolean){ if(c)
fun(){} else value }"#,
    ),
    (
        "anonymous_function_control_bodies_follow_k1_postfix_boundaries-002",
        r#"fun test(c:Boolean){ if(c) fun
(){} else value }"#,
    ),
    (
        "anonymous_function_control_bodies_follow_k1_postfix_boundaries-003",
        r#"fun test(c:Boolean){ while(c) fun(){} }"#,
    ),
    (
        "anonymous_function_control_bodies_follow_k1_postfix_boundaries-004",
        r#"fun test(xs:List<Int>){ for(x in xs) fun(){} }"#,
    ),
    (
        "anonymous_function_control_bodies_follow_k1_postfix_boundaries-005",
        r#"fun test(c:Boolean){ do fun(){} while(c) }"#,
    ),
    (
        "anonymous_function_control_bodies_follow_k1_postfix_boundaries-006",
        r#"fun test(c:Boolean){ when { c -> fun(){}; else -> value } }"#,
    ),
    (
        "anonymous_function_control_bodies_follow_k1_postfix_boundaries-007",
        r#"fun test(c:Boolean){ if(c) fun String.(){} else value }"#,
    ),
    (
        "anonymous_function_control_bodies_follow_k1_postfix_boundaries-008",
        r#"fun test(c:Boolean){ if(c) fun (String).(){} else value }"#,
    ),
    (
        "anonymous_function_control_bodies_follow_k1_postfix_boundaries-009",
        r#"fun test(c:Boolean){ if(c) fun interface Local{} else value }"#,
    ),
    (
        "anonymous_function_control_bodies_follow_k1_postfix_boundaries-010",
        r#"fun test(c:Boolean){ if(c) fun(){}() else value }"#,
    ),
    (
        "anonymous_function_control_bodies_follow_k1_postfix_boundaries-011",
        r#"fun test(c:Boolean){ if(c) fun(){}.member else value }"#,
    ),
    (
        "anonymous_function_control_bodies_follow_k1_postfix_boundaries-012",
        r#"fun test(c:Boolean){ if(c) fun(){}[0] else value }"#,
    ),
    (
        "anonymous_function_control_bodies_follow_k1_postfix_boundaries-013",
        r#"fun test(c:Boolean){ if(c) fun(){}!! else value }"#,
    ),
    (
        "anonymous_function_control_bodies_follow_k1_postfix_boundaries-014",
        r#"fun test(c:Boolean){ if(c) fun(){}++ else value }"#,
    ),
    (
        "anonymous_function_control_bodies_follow_k1_postfix_boundaries-015",
        r#"fun test(c:Boolean){ if(c) fun(){}::class else value }"#,
    ),
    (
        "anonymous_function_control_bodies_follow_k1_postfix_boundaries-016",
        r#"fun test(){ val a=fun(){}(); val b=fun(){}.member; val c=fun(){}[0]; val d=fun(){}!!; val e=fun(){}++ }"#,
    ),
    (
        "anonymous_function_control_bodies_follow_k1_postfix_boundaries-017",
        r#"fun test(c:Boolean){ while(c) fun(){}
next() }"#,
    ),
    (
        "anonymous_function_control_bodies_follow_k1_postfix_boundaries-018",
        r#"fun test(c:Boolean){ if(c) fun(){}
else value }"#,
    ),
    (
        "anonymous_function_control_bodies_follow_k1_postfix_boundaries-019",
        r#"fun test(c:Boolean){ if(c) fun interface Local{}
else value }"#,
    ),
    (
        "anonymous_function_parameters_preserve_optional_types_defaults_and_ranges-000",
        r#"fun host(){ val f = fun(x) = x }"#,
    ),
    (
        "anonymous_function_parameters_preserve_optional_types_defaults_and_ranges-001",
        r#"fun host(){ val f = fun(x, y: Int,): Int = x + y }"#,
    ),
    (
        "anonymous_function_parameters_preserve_optional_types_defaults_and_ranges-002",
        r#"fun host(){ val f = fun String.(suffix) = this + suffix }"#,
    ),
    (
        "anonymous_function_parameters_preserve_optional_types_defaults_and_ranges-003",
        r#"fun host(){ val f = fun(x = 1) = x }"#,
    ),
    (
        "anonymous_function_parameters_preserve_optional_types_defaults_and_ranges-004",
        r#"annotation class A; fun host(){ val f = @A fun(x)=x }"#,
    ),
    (
        "anonymous_function_parameters_preserve_optional_types_defaults_and_ranges-005",
        r#"fun host(c:Boolean){ if(c) fun(x)=x else fun(y:Int)=y }"#,
    ),
    (
        "anonymous_function_parameters_preserve_optional_types_defaults_and_ranges-006",
        r#"fun host(){ val f = fun(x, y: Int = 1,): Int = x + y }"#,
    ),
    (
        "assignment_control_bodies_preserve_follow_boundaries-000",
        r#"fun test() { (value) = other }"#,
    ),
    (
        "assignment_control_bodies_preserve_follow_boundaries-001",
        r#"fun test() { (value) += other }"#,
    ),
    (
        "assignment_control_bodies_preserve_follow_boundaries-002",
        r#"fun test(c: Boolean) { if (c) x = 1 else y = 2 }"#,
    ),
    (
        "assignment_control_bodies_preserve_follow_boundaries-003",
        r#"fun test(c: Boolean) { if (c) (x) = 1 else (y) += 2 }"#,
    ),
    (
        "assignment_control_bodies_preserve_follow_boundaries-004",
        r#"fun test(c: Boolean) { if (c) x = 1
else y = 2 }"#,
    ),
    (
        "assignment_control_bodies_preserve_follow_boundaries-005",
        r#"fun test(c: Boolean) { if (c) x = 1; /* join */ else y = 2 }"#,
    ),
    (
        "assignment_control_bodies_preserve_follow_boundaries-006",
        r#"fun test(a: Boolean, b: Boolean) { if (a) if (b) x = 1 else y = 2 else z = 3 }"#,
    ),
    (
        "assignment_control_bodies_preserve_follow_boundaries-007",
        r#"fun test(c: Boolean) { if (c) x = if (c) 1 else 2 else y = 3 }"#,
    ),
    (
        "assignment_control_bodies_preserve_follow_boundaries-008",
        r#"fun test(c: Boolean) { if (c) x = { value } else y = 2 }"#,
    ),
    (
        "assignment_control_bodies_preserve_follow_boundaries-009",
        r#"fun test(c: Boolean) { while (c) value.member = 1 }"#,
    ),
    (
        "assignment_control_bodies_preserve_follow_boundaries-010",
        r#"fun test(c: Boolean) { do values[index] += 1 while (c) }"#,
    ),
    (
        "assignment_control_bodies_preserve_follow_boundaries-011",
        r#"fun test(values: Values) { for (value in values) total += value }"#,
    ),
    (
        "assignment_control_bodies_preserve_follow_boundaries-012",
        r#"fun test(c: Boolean) { when { c -> x = 1; else -> y = 2 } }"#,
    ),
    (
        "assignment_control_bodies_preserve_follow_boundaries-013",
        r#"fun test(c: Boolean) { when { c -> if (c) x = 1; else -> value } }"#,
    ),
    (
        "assignment_control_bodies_preserve_follow_boundaries-014",
        r#"fun test(c: Boolean) { if (c) target[index] += 1 else value = 2 }"#,
    ),
    (
        "assignment_targets_follow_k1s_parser_level_expression_surface-000",
        r#"fun host(){ 1=value; call()=value; (left+right)=value }"#,
    ),
    (
        "assignment_targets_follow_k1s_parser_level_expression_surface-001",
        r#"fun host(){ call()+=value; value!!+=value; value+++=value }"#,
    ),
    (
        "assignment_targets_follow_k1s_parser_level_expression_surface-002",
        r#"fun host(){ ++value+=other; -value+=other; !flag+=other }"#,
    ),
    (
        "assignment_targets_follow_k1s_parser_level_expression_surface-003",
        r#"fun host(){ { value }+=other; (fun(){})+=other; ::host+=other }"#,
    ),
    (
        "assignment_targets_follow_k1s_parser_level_expression_surface-004",
        r#"annotation class A
fun host(){ @A call()=1; label@ value!!+=2 }"#,
    ),
    (
        "assignment_targets_follow_k1s_parser_level_expression_surface-005",
        r#"fun host(c:Boolean){ if(c) call()=1 else value!!+=2 }"#,
    ),
    (
        "assignment_targets_follow_k1s_parser_level_expression_surface-006",
        r#"fun host(c:Boolean){ while(c) (left+right)=value }"#,
    ),
    (
        "assignment_targets_follow_k1s_parser_level_expression_surface-007",
        r#"fun host(c:Boolean){ do ++value+=other while(c) }"#,
    ),
    (
        "assignment_targets_follow_k1s_parser_level_expression_surface-008",
        r#"fun host(values:Values){ for(value in values) call()+=value }"#,
    ),
    (
        "assignment_targets_follow_k1s_parser_level_expression_surface-009",
        r#"fun host(c:Boolean){ when { c -> (left+right)=value; else -> value+++=other } }"#,
    ),
    (
        "assignment_targets_follow_k1s_parser_level_expression_surface-010",
        r#"fun host(){ call()+value }"#,
    ),
    (
        "assignment_targets_follow_k1s_parser_level_expression_surface-011",
        r#"fun host(){ left==right }"#,
    ),
    (
        "assignment_targets_follow_k1s_parser_level_expression_surface-012",
        r#"fun host(c:Boolean){ if(c) call() else value }"#,
    ),
    (
        "braced_control_bodies_win_over_lambda_receivers-000",
        r#"fun test(flag: Boolean) { if (flag) { value }() }"#,
    ),
    (
        "braced_control_bodies_win_over_lambda_receivers-001",
        r#"fun test() { { value }() }"#,
    ),
    (
        "braced_control_bodies_win_over_lambda_receivers-002",
        r#"fun test() { ({ value })() }"#,
    ),
    (
        "braced_control_bodies_win_over_lambda_receivers-003",
        r#"fun test() { foo { value } }"#,
    ),
    (
        "catch_paramet-000",
        r#"fun test() {
try {} catch (error: Exception,) {}
}
"#,
    ),
    (
        "companion_objects_accept_the_kotlin_2_4_data_modifier_position-000",
        r#"class Host { companion data object Named }"#,
    ),
    (
        "companion_objects_accept_the_kotlin_2_4_data_modifier_position-001",
        r#"class Host { companion
data
object Named }"#,
    ),
    (
        "companion_objects_accept_the_kotlin_2_4_data_modifier_position-002",
        r#"class Host { data companion object Named }"#,
    ),
    (
        "companion_objects_accept_the_kotlin_2_4_data_modifier_position-003",
        r#"class Host { companion data data object Named }"#,
    ),
    (
        "companion_objects_accept_the_kotlin_2_4_data_modifier_position-004",
        r#"class Host { companion value object Named }"#,
    ),
    (
        "companion_objects_accept_the_kotlin_2_4_data_modifier_position-005",
        r#"class Host { companion data object Named : Base {} }"#,
    ),
    (
        "complete_expressions_are_owned_by_control_bodies-000",
        r#"fun test(c:Boolean){ if(c) left + right else value }"#,
    ),
    (
        "complete_expressions_are_owned_by_control_bodies-001",
        r#"fun test(c:Boolean){ if(c) left * right + third else value }"#,
    ),
    (
        "complete_expressions_are_owned_by_control_bodies-002",
        r#"fun test(c:Boolean){ if(c) left ?: right else value }"#,
    ),
    (
        "complete_expressions_are_owned_by_control_bodies-003",
        r#"fun test(c:Boolean){ if(c) left..right else value }"#,
    ),
    (
        "complete_expressions_are_owned_by_control_bodies-004",
        r#"fun test(c:Boolean){ if(c) left < right else value }"#,
    ),
    (
        "complete_expressions_are_owned_by_control_bodies-005",
        r#"fun test(c:Boolean){ if(c) left == right else value }"#,
    ),
    (
        "complete_expressions_are_owned_by_control_bodies-006",
        r#"fun test(c:Boolean){ if(c) left && right || third else value }"#,
    ),
    (
        "complete_expressions_are_owned_by_control_bodies-007",
        r#"fun test(c:Boolean){ if(c) value as String else value }"#,
    ),
    (
        "complete_expressions_are_owned_by_control_bodies-008",
        r#"fun test(c:Boolean){ if(c) value is String else value }"#,
    ),
    (
        "complete_expressions_are_owned_by_control_bodies-009",
        r#"fun test(c:Boolean){ if(c) value in values else value }"#,
    ),
    (
        "complete_expressions_are_owned_by_control_bodies-010",
        r#"fun test(c:Boolean){ if(c) left +
 right else value }"#,
    ),
    (
        "complete_expressions_are_owned_by_control_bodies-011",
        r#"fun test(c:Boolean){ if(c) left + /* join
 */ right else value }"#,
    ),
    (
        "complete_expressions_are_owned_by_control_bodies-012",
        r#"fun test(c:Boolean){ if(c) left + right; else value }"#,
    ),
    (
        "complete_expressions_are_owned_by_control_bodies-013",
        r#"fun test(c:Boolean){ while(c) left + right
next() }"#,
    ),
    (
        "complete_expressions_are_owned_by_control_bodies-014",
        r#"fun test(c:Boolean){ for(x in values) left ?: right
next() }"#,
    ),
    (
        "complete_expressions_are_owned_by_control_bodies-015",
        r#"fun test(c:Boolean){ when { c -> left + right
else -> value } }"#,
    ),
    (
        "complete_expressions_are_owned_by_control_bodies-016",
        r#"fun test(a:Boolean,b:Boolean){ if(a) if(b) left + right else middle else outer }"#,
    ),
    (
        "complete_expressions_are_owned_by_control_bodies-017",
        r#"fun test(c:Boolean){ val result=if(c) left + right else value * other }"#,
    ),
    (
        "compound_word_operators_are_indivisible_tokens-000",
        r#"fun test(value: Any, values: List<Any>) { value as? String }"#,
    ),
    (
        "compound_word_operators_are_indivisible_tokens-001",
        r#"fun test(value: Any) { value !is String }"#,
    ),
    (
        "compound_word_operators_are_indivisible_tokens-002",
        r#"fun test(value: Any, values: List<Any>) { value !in values }"#,
    ),
    (
        "compound_word_operators_are_indivisible_tokens-003",
        r#"fun test(value: Any, values: List<Any>) { value as String; value is String; value in values }"#,
    ),
    (
        "compound_word_operators_are_indivisible_tokens-004",
        r#"fun test(value: Any) { !isBoolean(value); !inRange(value) }"#,
    ),
    (
        "compound_word_operators_are_indivisible_tokens-005",
        r#"fun test(value: Any, values: List<Any>) { value as? String; value !is String; value !in values }"#,
    ),
    (
        "context_anonymous_functions_are_direct_control_bodies-000",
        r#"fun host(c:Boolean){ while(c) context(item:Item) fun(){} }"#,
    ),
    (
        "context_anonymous_functions_are_direct_control_bodies-001",
        r#"fun host(xs:Items){ for(x in xs) context(item:Item) fun(){} }"#,
    ),
    (
        "context_anonymous_functions_are_direct_control_bodies-002",
        r#"fun host(c:Boolean){ do context(item:Item) fun(){} while(c) }"#,
    ),
    (
        "context_anonymous_functions_are_direct_control_bodies-003",
        r#"fun host(c:Boolean){ when { c -> context(item:Item) fun(){}; else -> Unit } }"#,
    ),
    (
        "context_anonymous_functions_are_direct_control_bodies-004",
        r#"annotation class A; fun host(c:Boolean){ if(c) @A context(item:Item) fun(){} else Unit }"#,
    ),
    (
        "context_anonymous_functions_are_direct_control_bodies-005",
        r#"fun host(c:Boolean){ if(c) context(item:Item) fun(){}() else Unit }"#,
    ),
    (
        "context_anonymous_functions_are_direct_control_bodies-006",
        r#"fun host(c:Boolean){ if(c) context(item:Item) fun(){} else Unit }"#,
    ),
    (
        "context_para-000",
        r#"typealias Legacy = context(Item) () -> Unit
val callback: context(item: Item, scope: Scope,) () -> Unit = {}"#,
    ),
    (
        "context_param-000",
        r#"val callback = @Ann context(item: Item) fun Receiver.(value: Int) = value
val invoked = context(scope: Scope) fun() {}()"#,
    ),
    (
        "context_parameters_cover_k1_modifier_positions_and_boundaries-000",
        r#"context(item: Item) class C"#,
    ),
    (
        "context_parameters_cover_k1_modifier_positions_and_boundaries-001",
        r#"public context(item: Item) enum class E { A }"#,
    ),
    (
        "context_parameters_cover_k1_modifier_positions_and_boundaries-002",
        r#"enum context(item: Item) class E { A }"#,
    ),
    (
        "context_parameters_cover_k1_modifier_positions_and_boundaries-003",
        r#"context(item: Item) object O {}"#,
    ),
    (
        "context_parameters_cover_k1_modifier_positions_and_boundaries-004",
        r#"context(item: Item) typealias Alias = String"#,
    ),
    (
        "context_parameters_cover_k1_modifier_positions_and_boundaries-005",
        r#"class C context(item: Item) constructor()"#,
    ),
    (
        "context_parameters_cover_k1_modifier_positions_and_boundaries-006",
        r#"class C { context(item: Item) constructor() }"#,
    ),
    (
        "context_parameters_cover_k1_modifier_positions_and_boundaries-007",
        r#"class C(context(item: Item) val value: Int)"#,
    ),
    (
        "context_parameters_cover_k1_modifier_positions_and_boundaries-008",
        r#"class C { context(item: Item) init {} }"#,
    ),
    (
        "context_parameters_cover_k1_modifier_positions_and_boundaries-009",
        r#"enum class E { context(item: Item) A }"#,
    ),
    (
        "context_parameters_cover_k1_modifier_positions_and_boundaries-010",
        r#"context(item: Item) val value = 1"#,
    ),
    (
        "context_parameters_cover_k1_modifier_positions_and_boundaries-011",
        r#"fun outer() { context(item: Item) val (a, b) = pair }"#,
    ),
    (
        "context_parameters_cover_k1_modifier_positions_and_boundaries-012",
        r#"val value: Int context(item: Item) get() = 1"#,
    ),
    (
        "context_parameters_cover_k1_modifier_positions_and_boundaries-013",
        r#"var value: Int = 0; context(item: Item) set(value) {}"#,
    ),
    (
        "context_parameters_cover_k1_modifier_positions_and_boundaries-014",
        r#"context(item: Item) fun interface I"#,
    ),
    (
        "context_parameters_cover_k1_modifier_positions_and_boundaries-015",
        r#"@Ann context(item: Item) @Other fun host() {}"#,
    ),
    (
        "context_parameters_cover_k1_modifier_positions_and_boundaries-016",
        r#"context(item: Item) context(scope: Scope) fun host() {}"#,
    ),
    (
        "context_parameters_cover_k1_modifier_positions_and_boundaries-017",
        r#"typealias F = context(Item) () -> Unit"#,
    ),
    (
        "context_parameters_cover_k1_modifier_positions_and_boundaries-018",
        r#"val f: context(item: Item) () -> Unit = {}"#,
    ),
    (
        "context_parameters_cover_k1_modifier_positions_and_boundaries-019",
        r#"val f = context(item: Item) fun() {}"#,
    ),
    (
        "context_parameters_cover_k1_modifier_positions_and_boundaries-020",
        r#"val f = @Ann context(item: Item) fun Receiver.(value: Int) = value"#,
    ),
    (
        "context_parameters_cover_k1_modifier_positions_and_boundaries-021",
        r#"val f = context(item: Item) public fun() {}()"#,
    ),
    (
        "context_parameters_prefix_anonymous_functions_and_their_postfix_chains-000",
        r#"context(item: Item) fun named() {}"#,
    ),
    (
        "control_body_returns_keep_operands_and_outer_statement_boundaries-000",
        r#"fun f(c:Boolean) { if(c) return a }"#,
    ),
    (
        "control_body_returns_keep_operands_and_outer_statement_boundaries-001",
        r#"fun f(c:Boolean) { if(c) return a.b }"#,
    ),
    (
        "control_body_returns_keep_operands_and_outer_statement_boundaries-002",
        r#"fun f(c:Boolean) { if(c) return a() }"#,
    ),
    (
        "control_body_returns_keep_operands_and_outer_statement_boundaries-003",
        r#"fun f(c:Boolean) { if(c) return a.b() }"#,
    ),
    (
        "control_body_returns_keep_operands_and_outer_statement_boundaries-004",
        r#"fun f(c:Boolean) { if(c) return a + b }"#,
    ),
    (
        "control_body_returns_keep_operands_and_outer_statement_boundaries-005",
        r#"fun f(c:Boolean) { if(c) return a to b }"#,
    ),
    (
        "control_body_returns_keep_operands_and_outer_statement_boundaries-006",
        r#"fun f(a:Boolean,b:Boolean) { if(a) if(b) return x to y }"#,
    ),
    (
        "control_body_returns_keep_operands_and_outer_statement_boundaries-007",
        r#"fun f(c:Boolean) { if(c) return a as T }"#,
    ),
    (
        "control_body_returns_keep_operands_and_outer_statement_boundaries-008",
        r#"fun f(c:Boolean) { if(c) return "a" }"#,
    ),
    (
        "control_body_returns_keep_operands_and_outer_statement_boundaries-009",
        r#"fun f(c:Boolean) { if(c) return a else return b }"#,
    ),
    (
        "control_body_returns_keep_operands_and_outer_statement_boundaries-010",
        r#"fun f(c:Boolean){ if(c) return a to b
x() }"#,
    ),
    (
        "control_function_ali-000",
        r#"fun host(c:Boolean){ while(c) fun local() {} /* boundary
 nested */
next() }"#,
    ),
    (
        "d-000",
        r#"fun String?.tag() {}
fun safe(value: String?) = value?.length
"#,
    ),
    (
        "declaration_bodies_continue_across_newlines-000",
        r#"fun value(): String
{ return "value" }"#,
    ),
    (
        "declaration_bodies_continue_across_newlines-001",
        r#"fun value()
// body
{ return Unit }"#,
    ),
    (
        "declaration_bodies_continue_across_newlines-002",
        r#"fun value()
/* body */
{ return Unit }"#,
    ),
    (
        "declaration_rec-000",
        r#"annotation class Ann
fun @Ann pkg.String.ext() {}
fun @Ann @Ann
String?.nullable() {}
fun @Ann dynamic.js() {}
val @Ann String.size: Int get() = 1
fun (@Ann String).boxed() {}
fun List<@Ann String>.element() {}
"#,
    ),
    (
        "delegating_declarations_own_their_class_body_braces-000",
        r#"class Value : Contract by delegate {}"#,
    ),
    (
        "delegating_declarations_own_their_class_body_braces-001",
        r#"object Value : Contract by delegate {}"#,
    ),
    (
        "delegating_declarations_own_their_class_body_braces-002",
        r#"class Host { companion object : Contract by delegate {} }"#,
    ),
    (
        "delegating_declarations_own_their_class_body_braces-003",
        r#"class Value : Contract by create() {}"#,
    ),
    (
        "delegating_declarations_own_their_class_body_braces-004",
        r#"class Value : Contract by holder.delegate {}"#,
    ),
    (
        "delegating_declarations_own_their_class_body_braces-005",
        r#"class Value : Contract by first ?: second {}"#,
    ),
    (
        "delegating_declarations_own_their_class_body_braces-006",
        r#"class Value(c: Boolean) : Contract by if (c) first else second {}"#,
    ),
    (
        "delegating_declarations_own_their_class_body_braces-007",
        r#"class Value : Contract by (create { value }) {}"#,
    ),
    (
        "delegating_declarations_own_their_class_body_braces-008",
        r#"val value = object : Contract by delegate {}"#,
    ),
    (
        "delegating_declarations_own_their_class_body_braces-009",
        r#"val value = object : Contract by create() {}"#,
    ),
    (
        "delegating_declarations_own_their_class_body_braces-010",
        r#"val value = object : Contract by holder.delegate {}"#,
    ),
    (
        "delegating_declarations_own_their_class_body_braces-011",
        r#"val value = object : Contract by object : Contract {} {}"#,
    ),
    (
        "delegating_declarations_own_their_class_body_braces-012",
        r#"enum class Value : Contract by delegate { Entry }"#,
    ),
    (
        "delegating_object_bodies_preserve_visible_cst_identity-000",
        r#"val value = object : Contract by holder.delegate { override fun get() = Unit }"#,
    ),
    (
        "delegation_specifiers_accept_function_parenthesized_and_suspend_types-000",
        r#"class Host(value: () -> Unit) : () -> Unit by value"#,
    ),
    (
        "delegation_specifiers_accept_function_parenthesized_and_suspend_types-001",
        r#"class Host(value: () -> Unit) : (() -> Unit) by value"#,
    ),
    (
        "delegation_specifiers_accept_function_parenthesized_and_suspend_types-002",
        r#"class Host(value: Base) : (Base) by value"#,
    ),
    (
        "delegation_specifiers_accept_function_parenthesized_and_suspend_types-003",
        r#"class Host(value: () -> Unit) : suspend () -> Unit by value"#,
    ),
    (
        "delegation_specifiers_accept_function_parenthesized_and_suspend_types-004",
        r#"class Host : () -> Unit"#,
    ),
    (
        "delegation_specifiers_accept_function_parenthesized_and_suspend_types-005",
        r#"class Host : (() -> Unit)"#,
    ),
    (
        "delegation_specifiers_accept_function_parenthesized_and_suspend_types-006",
        r#"class Host : (Base)"#,
    ),
    (
        "delegation_specifiers_accept_function_parenthesized_and_suspend_types-007",
        r#"class Host : suspend () -> Unit"#,
    ),
    (
        "destructuring_entries_cover_name_based_and_positional_forms-000",
        r#"fun host(pair:Pair<Int,Int>){ val (left)=pair; val (first=left,second)=pair }"#,
    ),
    (
        "destructuring_entries_cover_name_based_and_positional_forms-001",
        r#"fun host(pair:Pair<Int,Int>){ val [@A left:Int,right,]=pair }"#,
    ),
    (
        "destructuring_entries_cover_name_based_and_positional_forms-002",
        r#"fun host(xs:List<Pair<Int,Int>>){ for((@A left:Int=first,right,) in xs) consume(left); for([left,right] in xs) consume(right) }"#,
    ),
    (
        "destructuring_entries_cover_name_based_and_positional_forms-003",
        r#"fun host(){ val first={ (left=source,right) -> left }; val second={ [left,right] -> right } }"#,
    ),
    (
        "destructuring_entries_cover_name_based_and_positional_forms-004",
        r#"fun host(pair:Pair<Int,Int>){ val (first=left,second:Int)=pair; val [third,fourth]=pair; val plain=1 }"#,
    ),
    (
        "direct_arrow_lambda_control_bodies_preserve_postfix_ownership-000",
        r#"fun test(c: Boolean) { if (c) { -> value } else { -> other } }"#,
    ),
    (
        "direct_arrow_lambda_control_bodies_preserve_postfix_ownership-001",
        r#"fun test(c: Boolean) { while (c) { /* lead */
 -> value } }"#,
    ),
    (
        "direct_arrow_lambda_control_bodies_preserve_postfix_ownership-002",
        r#"fun test(c: Boolean) { for (value in values) { -> value } }"#,
    ),
    (
        "direct_arrow_lambda_control_bodies_preserve_postfix_ownership-003",
        r#"fun test(c: Boolean) { do { -> value } while (c) }"#,
    ),
    (
        "direct_arrow_lambda_control_bodies_preserve_postfix_ownership-004",
        r#"fun test(c: Boolean) { when { c -> { -> value }; else -> { -> other } } }"#,
    ),
    (
        "direct_arrow_lambda_control_bodies_preserve_postfix_ownership-005",
        r#"fun test(c: Boolean) { if (c) { value -> value } else { -> other } }"#,
    ),
    (
        "direct_arrow_lambda_control_bodies_preserve_postfix_ownership-006",
        r#"fun test(c: Boolean) { if (c) { value: Int -> value } else { -> other } }"#,
    ),
    (
        "direct_arrow_lambda_control_bodies_preserve_postfix_ownership-007",
        r#"fun test(c: Boolean) { if (c) { (left, right) -> left } else { -> other } }"#,
    ),
    (
        "direct_arrow_lambda_control_bodies_preserve_postfix_ownership-008",
        r#"fun test(c: Boolean) { if (c) { value: Int, -> value } else { -> other } }"#,
    ),
    (
        "direct_arrow_lambda_control_bodies_preserve_postfix_ownership-009",
        r#"fun test(c: Boolean) { if (c) { /* lead */
 value: Int, -> value } else { -> other } }"#,
    ),
    (
        "direct_arrow_lambda_control_bodies_preserve_postfix_ownership-010",
        r#"fun test(c: Boolean) { if (c) { callback: () -> Int -> callback } else { -> other } }"#,
    ),
    (
        "direct_arrow_lambda_control_bodies_preserve_postfix_ownership-011",
        r#"fun test(c: Boolean) { while (c) { value: Int -> value } }"#,
    ),
    (
        "direct_arrow_lambda_control_bodies_preserve_postfix_ownership-012",
        r#"fun test(values: Values) { for (value in values) { element: Int -> element } }"#,
    ),
    (
        "direct_arrow_lambda_control_bodies_preserve_postfix_ownership-013",
        r#"fun test(c: Boolean) { do { value: Int -> value } while (c) }"#,
    ),
    (
        "direct_arrow_lambda_control_bodies_preserve_postfix_ownership-014",
        r#"fun test(c: Boolean) { when { c -> { value: Int -> value }; else -> { (left, right) -> left } } }"#,
    ),
    (
        "direct_arrow_lambda_control_bodies_preserve_postfix_ownership-015",
        r#"fun test() { val zero = { -> value }; call { value: Int -> value } }"#,
    ),
    (
        "direct_arrow_lambda_control_bodies_preserve_postfix_ownership-016",
        r#"fun test(c: Boolean) { if (c) { value }() }"#,
    ),
    (
        "direct_arrow_lambda_control_bodies_preserve_postfix_ownership-017",
        r#"fun test(c: Boolean) { if (c) { /* lead /* nested */ tail */
 -> value }() }"#,
    ),
    (
        "direct_arrow_lambda_control_bodies_preserve_postfix_ownership-018",
        r#"fun test(c: Boolean) {{ if (c) {{ value: Int -> value }}{suffix} }}"#,
    ),
    (
        "direct_arrow_lambda_control_bodies_preserve_postfix_ownership-019",
        r#"fun test(c: Boolean) { if (c) { value } }"#,
    ),
    (
        "direct_arrow_lambda_control_bodies_preserve_postfix_ownership-020",
        r#"fun test(c: Boolean) { if (c) { value as () -> Unit } }"#,
    ),
    (
        "direct_arrow_lambda_control_bodies_preserve_postfix_ownership-021",
        r#"fun test() { val callback = { value: Int -> value } }"#,
    ),
    (
        "do_while_allows_an_omitted_body_without_consuming_the_separator-000",
        r#"fun test(){ do while(false); foo() }"#,
    ),
    (
        "do_while_allows_an_omitted_body_without_consuming_the_separator-001",
        r#"fun test(){ do
while(false); foo() }"#,
    ),
    (
        "do_while_allows_an_omitted_body_without_consuming_the_separator-002",
        r#"fun test(){ do /* empty
 body */ while(false); foo() }"#,
    ),
    (
        "do_while_allows_an_omitted_body_without_consuming_the_separator-003",
        r#"fun test(){ do foo() while(false) }"#,
    ),
    (
        "do_while_allows_an_omitted_body_without_consuming_the_separator-004",
        r#"fun test(){ do {} while(false) }"#,
    ),
    (
        "do_while_trailer_keyword_respects_identifier_boundaries-000",
        r#"fun test(a:Boolean){{ do {body} while(a) }}"#,
    ),
    (
        "dynamic_types_are_distinct_from_qualified_user_types-000",
        r#"typealias Direct = dynamic"#,
    ),
    (
        "dynamic_types_are_distinct_from_qualified_user_types-001",
        r#"typealias Nullable = dynamic?"#,
    ),
    (
        "dynamic_types_are_distinct_from_qualified_user_types-002",
        r#"typealias Receiver = dynamic.() -> Unit"#,
    ),
    (
        "dynamic_types_are_distinct_from_qualified_user_types-003",
        r#"typealias Projected = List<dynamic>"#,
    ),
    (
        "dynamic_types_are_distinct_from_qualified_user_types-004",
        r#"fun consume(value: dynamic) {}"#,
    ),
    (
        "dynamic_types_are_distinct_from_qualified_user_types-005",
        r#"typealias Qualified = dynamic.foo"#,
    ),
    (
        "dynamic_types_are_distinct_from_qualified_user_types-006",
        r#"fun use(){ val dynamic = value; dynamic.foo() }"#,
    ),
    (
        "empty_do_while_does_not_consume_the_following_loop-000",
        r#"fun test(a:Boolean,b:Boolean){ do
while(a)
while(b){} }"#,
    ),
    (
        "empty_do_while_uses_a_contextual_trailer_keyword-000",
        r#"fun test(a:Boolean){ do while(a); consume() }"#,
    ),
    (
        "empty_do_while_uses_a_contextual_trailer_keyword-001",
        r#"fun test(a:Boolean){ do
while(a)
consume() }"#,
    ),
    (
        "empty_do_while_uses_a_contextual_trailer_keyword-002",
        r#"fun test(a:Boolean){ do /* outer
 /* nested */ tail */ while(a); consume() }"#,
    ),
    (
        "empty_do_while_uses_a_contextual_trailer_keyword-003",
        r#"fun test(a:Boolean){ do // trailer
while(a); consume() }"#,
    ),
    (
        "empty_if_branches_preserve_else_and_semicolon_ownership-000",
        r#"fun test(c: Boolean) { if (c); foo() }"#,
    ),
    (
        "empty_if_branches_preserve_else_and_semicolon_ownership-001",
        r#"fun test(c: Boolean) { if (c) else value }"#,
    ),
    (
        "empty_if_branches_preserve_else_and_semicolon_ownership-002",
        r#"fun test(c: Boolean) { if (c) else ; }"#,
    ),
    (
        "empty_if_branches_preserve_else_and_semicolon_ownership-003",
        r#"fun test(c: Boolean) { if (c) foo(); else bar() }"#,
    ),
    (
        "empty_if_branches_preserve_else_and_semicolon_ownership-004",
        r#"fun test(c: Boolean) { if (c) foo()
; /* join */ else bar() }"#,
    ),
    (
        "empty_if_branches_preserve_else_and_semicolon_ownership-005",
        r#"fun test(a: Boolean, b: Boolean) { if (a) if (b) else ; else value }"#,
    ),
    (
        "empty_if_branches_preserve_else_and_semicolon_ownership-006",
        r#"fun test(c: Boolean) { when { c -> if (c)
else /* next */ -> value } }"#,
    ),
    (
        "empty_if_branches_preserve_else_and_semicolon_ownership-007",
        r#"fun test(c: Boolean) { when { c -> if (c) foo(); /* join */ else /* next */ -> value } }"#,
    ),
    (
        "empty_loop_body_marker_preserves_the_for_outer_separator-000",
        r#"fun test(){ for (x in 0..1); foo() }"#,
    ),
    (
        "empty_loop_body_marker_preserves_the_for_outer_separator-001",
        r#"fun test(){ for (x in 0..1)
; foo() }"#,
    ),
    (
        "empty_loop_body_marker_preserves_the_for_outer_separator-002",
        r#"fun test(){ for (x in 0..1) /* empty
 body */ ; // separator
foo() }"#,
    ),
    (
        "empty_loop_body_marker_preserves_the_for_outer_separator-003",
        r#"fun test(){ for (x in 0..1)
foo() }"#,
    ),
    (
        "empty_loop_body_marker_preserves_the_for_outer_separator-004",
        r#"fun test(){ for (x in 0..1)
{} }"#,
    ),
    (
        "empty_loop_body_marker_preserves_the_for_outer_separator-005",
        r#"fun test(){ for (x in 0..1) foo(); bar() }"#,
    ),
    (
        "empty_loop_body_marker_preserves_the_while_outer_separator-000",
        r#"fun test(){ while(false); foo() }"#,
    ),
    (
        "empty_loop_body_marker_preserves_the_while_outer_separator-001",
        r#"fun test(){ while(false)
; foo() }"#,
    ),
    (
        "empty_loop_body_marker_preserves_the_while_outer_separator-002",
        r#"fun test(){ while(false) /* body
 marker */ ; // separator
foo() }"#,
    ),
    (
        "empty_loop_body_marker_preserves_the_while_outer_separator-003",
        r#"fun test(){ while(false) {} }"#,
    ),
    (
        "empty_loop_body_marker_preserves_the_while_outer_separator-004",
        r#"fun test(){ while(false) foo() }"#,
    ),
    (
        "enum_headers_select_enum_bodies-000",
        r#"enum class Empty {}"#,
    ),
    (
        "enum_headers_select_enum_bodies-001",
        r#"enum class Members { ; fun member() {} }"#,
    ),
    (
        "enum_headers_select_enum_bodies-002",
        r#"enum class Values { A, B; fun member() {} }"#,
    ),
    (
        "enum_headers_select_enum_bodies-003",
        r#"public enum class Public { A }"#,
    ),
    (
        "enum_headers_select_enum_bodies-004",
        r#"enum public class Reordered { A }"#,
    ),
    (
        "enum_headers_select_enum_bodies-005",
        r#"enum fun function() {}"#,
    ),
    (
        "enum_headers_select_enum_bodies-006",
        r#"enum val property = 1"#,
    ),
    (
        "enum_headers_select_enum_bodies-007",
        r#"enum object ObjectValue"#,
    ),
    (
        "enum_headers_select_enum_bodies-008",
        r#"enum interface InterfaceValue"#,
    ),
    (
        "enum_headers_select_enum_bodies-009",
        r#"public enum class Members { ; fun member() {} }"#,
    ),
    (
        "explicit_backing-000",
        r#"class C {
var value: Int
private field: String = source
get() = field
set(value) {}
val inferred: Int field
val initialized: Int field: Int = 1
}
val top: Int = source; field: String = source"#,
    ),
    (
        "explicit_backing_fields_preserve_scope_ranges_and_accessor_boundaries-000",
        r#"fun host() { val value = 1 }"#,
    ),
    (
        "external_t-000",
        r#"class 数据 { /* 外 /* nested */ 部 */ val 名 = "你好 😀" }"#,
    ),
    (
        "file_annotations_precede_file_contents_without_separators-000",
        r#"@file:JvmName("Oracle")
package oracle"#,
    ),
    (
        "file_annotations_precede_file_contents_without_separators-001",
        r#"@file:[A B] package oracle"#,
    ),
    (
        "file_annotations_precede_file_contents_without_separators-002",
        r#"@file:A @file:B class C"#,
    ),
    (
        "file_annotations_precede_file_contents_without_separators-003",
        r#"@file
:
A
package oracle"#,
    ),
    (
        "file_annotations_precede_file_contents_without_separators-004",
        r#"#!/usr/bin/env kotlin
@file:A
class C"#,
    ),
    (
        "file_annotations_precede_file_contents_without_separators-005",
        r#"package oracle
@file:A class C"#,
    ),
    (
        "file_annotations_precede_file_contents_without_separators-006",
        r#"@file:A @file:[B C] package oracle"#,
    ),
    (
        "file_annotations_precede_file_contents_without_separators-007",
        r#"package p
@file:A class C"#,
    ),
    (
        "for_variable_annotations_follow_k1_ordering-000",
        r#"annotation class A; fun f(xs:List<Int>){ for (@A item in xs) {} }"#,
    ),
    (
        "for_variable_annotations_follow_k1_ordering-001",
        r#"annotation class A; fun f(xs:List<Int>){ for (@A val item in xs) {} }"#,
    ),
    (
        "for_variable_annotations_follow_k1_ordering-002",
        r#"annotation class A; fun f(xs:List<Pair<Int,Int>>){ for (@A val (first, second) in xs) {} }"#,
    ),
    (
        "for_variable_annotations_follow_k1_ordering-003",
        r#"annotation class A; fun f(xs:List<Int>){ for (@A /* before var */ var item in xs) {} }"#,
    ),
    (
        "for_variable_annotations_follow_k1_ordering-004",
        r#"annotation class A; annotation class B; fun f(xs:List<Pair<Int,Int>>){ for (@[A B] (first, second) in xs) {} }"#,
    ),
    (
        "for_variable_annotations_follow_k1_ordering-005",
        r#"annotation class A; annotation class B; fun f(xs:List<Pair<Int,Int>>){ for ((@A first, @B second) in xs) {} }"#,
    ),
    (
        "function-000",
        r#"typealias F = (@Ann value: Int, @Other vararg second: String = default,) -> Unit
typealias G = (@TypeAnn Int) -> Unit"#,
    ),
    (
        "function_type_receiver_delimiters_are_contextual-000",
        r#"typealias Simple = A.() -> B
"#,
    ),
    (
        "function_type_receiver_delimiters_are_contextual-001",
        r#"typealias Qualified = A.B.() -> C
"#,
    ),
    (
        "function_type_receiver_delimiters_are_contextual-002",
        r#"typealias Parenthesized = (A).() -> B
"#,
    ),
    (
        "function_type_receiver_delimiters_are_contextual-003",
        r#"typealias Attached = A?.() -> B
"#,
    ),
    (
        "function_type_receiver_delimiters_are_contextual-004",
        r#"typealias Newline = A
?.
() -> B
"#,
    ),
    (
        "function_type_receiver_delimiters_are_contextual-005",
        r#"typealias Multiple = A??.() -> B
"#,
    ),
    (
        "function_type_receiver_delimiters_are_contextual-006",
        r#"typealias Mixed = A? ?.() -> B
"#,
    ),
    (
        "function_type_receiver_delimiters_are_contextual-007",
        r#"typealias Line = A?.// receiver
() -> B
"#,
    ),
    (
        "function_type_receiver_delimiters_are_contextual-008",
        r#"typealias Block = A?./* outer /* nested */ end */() -> B
"#,
    ),
    (
        "function_type_receiver_delimiters_are_contextual-009",
        r#"typealias DotTrivia = A
./* outer /* nested */ 0123456789012345678901234567890123456789012345678901234567890123456789 */() -> B
"#,
    ),
    (
        "function_type_receiver_delimiters_are_contextual-010",
        r#"typealias FormFeed = A?.() -> B
"#,
    ),
    (
        "function_type_receiver_delimiters_are_contextual-011",
        r#"fun safe(a: A?) { a?.b }
"#,
    ),
    (
        "generic_calls_own_attached_annotated_trailing_lambdas-000",
        r#"fun host(){ foo<Int>@Ann { value } }"#,
    ),
    (
        "generic_calls_own_attached_annotated_trailing_lambdas-001",
        r#"fun host(){ foo<Int>@Ann label@ { value } }"#,
    ),
    (
        "generic_postfix_suffixes_preserve_comparison_acceptance-000",
        r#"fun host(){ foo<T> }"#,
    ),
    (
        "generic_postfix_suffixes_preserve_comparison_acceptance-001",
        r#"fun host(){ foo<T>.member }"#,
    ),
    (
        "generic_postfix_suffixes_preserve_comparison_acceptance-002",
        r#"fun host(){ foo<T>::member }"#,
    ),
    (
        "generic_postfix_suffixes_preserve_comparison_acceptance-003",
        r#"fun host(){ foo<T>[index] }"#,
    ),
    (
        "generic_postfix_suffixes_preserve_comparison_acceptance-004",
        r#"fun host(){ foo<T>!! }"#,
    ),
    (
        "generic_postfix_suffixes_preserve_comparison_acceptance-005",
        r#"fun host(){ foo<T>++ }"#,
    ),
    (
        "generic_postfix_suffixes_preserve_comparison_acceptance-006",
        r#"fun host(){ foo<T> = value }"#,
    ),
    (
        "generic_postfix_suffixes_preserve_comparison_acceptance-007",
        r#"fun host(){ foo<T>() }"#,
    ),
    (
        "generic_postfix_suffixes_preserve_comparison_acceptance-008",
        r#"fun host(){ foo<T>()() }"#,
    ),
    (
        "generic_postfix_suffixes_preserve_comparison_acceptance-009",
        r#"fun host(){ foo.bar<T> }"#,
    ),
    (
        "generic_postfix_suffixes_preserve_comparison_acceptance-010",
        r#"fun host(){ foo().bar<T> }"#,
    ),
    (
        "generic_postfix_suffixes_preserve_comparison_acceptance-011",
        r#"fun host(){ List<T>::size }"#,
    ),
    (
        "generic_postfix_suffixes_preserve_comparison_acceptance-012",
        r#"fun host(){ pkg.List<T>::size }"#,
    ),
    (
        "generic_postfix_suffixes_preserve_comparison_acceptance-013",
        r#"fun host(){ foo<Map<String, List<Int>>,>() }"#,
    ),
    (
        "generic_postfix_suffixes_preserve_comparison_acceptance-014",
        r#"fun host(){ a < b }"#,
    ),
    (
        "identifiers_follow_the_pinned_kotlin_2_4_10_lexer_profile-000",
        r#"fun host() { val _ = 0; val λ = 1; val 東 = 2; val 𐐀 = 3 }"#,
    ),
    (
        "identifiers_follow_the_pinned_kotlin_2_4_10_lexer_profile-001",
        r#"fun host() { val a٣ = 1; val a𑁦 = 2 }"#,
    ),
    (
        "identifiers_follow_the_pinned_kotlin_2_4_10_lexer_profile-002",
        r#"fun host() { val `when` = 1; val `with space` = 2; val `😀` = 3; val `a.b/$[]<>:` = 4 }"#,
    ),
    (
        "identifiers_follow_the_pinned_kotlin_2_4_10_lexer_profile-003",
        r#"fun host(pair: Pair<Int, Int>) { val (_, value) = pair; pair.let { _, item -> item } }"#,
    ),
    (
        "import_aliases_preserve_k1_ranges_and_identity-000",
        r#"import foo.*
fun host() {}"#,
    ),
    (
        "import_lists_and_header_separators_preserve_k1_ranges-000",
        r#"import foo as bar
import foo.*
fun host() {}"#,
    ),
    (
        "infix_function_calls_follow_k1_line_and_keyword_boundaries-000",
        r#"fun host(a: Any, b: Any) {{ val result = {expression} }}"#,
    ),
    (
        "infix_function_calls_follow_k1_line_and_keyword_boundaries-001",
        r#"fun host(a: Any, b: Any) { val result = a to b }"#,
    ),
    (
        "jump_expressions_preserve_statement_identity_and_return_ownership-000",
        r#"fun host(flag:Boolean){ while(flag){ val value = source ?: break } }"#,
    ),
    (
        "jump_expressions_preserve_statement_identity_and_return_ownership-001",
        r#"fun host(items:List<Int>){ for(item in items){ val value = item ?: continue } }"#,
    ),
    (
        "jump_expressions_preserve_statement_identity_and_return_ownership-002",
        r#"fun host(){ val value = source ?: return fallback() }"#,
    ),
    (
        "jump_expressions_preserve_statement_identity_and_return_ownership-003",
        r#"fun host(){ val value = source ?: return@host fallback }"#,
    ),
    (
        "jump_expressions_preserve_statement_identity_and_return_ownership-004",
        r#"fun host(){ val value = source ?: @Mark break }"#,
    ),
    (
        "jump_expressions_preserve_statement_identity_and_return_ownership-005",
        r#"fun host(){ val value = (return) }"#,
    ),
    (
        "jump_expressions_preserve_statement_identity_and_return_ownership-006",
        r#"fun host(){ break.foo(); continue(); break[0]; break!!; break++; break::class; return.foo() }"#,
    ),
    (
        "jump_expressions_preserve_statement_identity_and_return_ownership-007",
        r#"fun host(){ break { value }; return { value } }"#,
    ),
    (
        "jump_expressions_preserve_statement_identity_and_return_ownership-008",
        r#"fun host(){ breakfast(); continuation(); returning() }"#,
    ),
    (
        "jump_expressions_preserve_statement_identity_and_return_ownership-009",
        r#"fun host(flag:Boolean){ while(flag){ break; val value = source ?: break } }"#,
    ),
    (
        "jump_expressions_preserve_statement_identity_and_return_ownership-010",
        r#"val same = source ?: return fallback(); "#,
    ),
    (
        "jump_expressions_preserve_statement_identity_and_return_ownership-011",
        r#"val comment = source ?: return /* internal
line */ fallback()"#,
    ),
    (
        "kotlin_parser_implements_the_common_parser_interface-000",
        r#"fun main() { val answer = 42 }"#,
    ),
    (
        "labeled_cont-000",
        r#"fun host(c:Boolean){ if(c) outer@ @A private @B inline fun local() {} else value; global@ fun String.(){} }"#,
    ),
    (
        "labeled_control_bodies_follow_k1_statement_boundaries-000",
        r#"fun test(c:Boolean){ if(c) loop@ while(c) consume() else consume() }"#,
    ),
    (
        "labeled_control_bodies_follow_k1_statement_boundaries-001",
        r#"fun test(c:Boolean){ while(c) property@ val value=1 }"#,
    ),
    (
        "labeled_control_bodies_follow_k1_statement_boundaries-002",
        r#"fun test(c:Boolean,xs:List<Int>){ for(x in xs) local@ class Local }"#,
    ),
    (
        "labeled_control_bodies_follow_k1_statement_boundaries-003",
        r#"fun test(c:Boolean){ do local@ fun() {} while(c) }"#,
    ),
    (
        "labeled_control_bodies_follow_k1_statement_boundaries-004",
        r#"fun test(c:Boolean){ when { c -> branch@ @Ann private fun local() {}; else -> fallback@ @Ann { value -> value } } }"#,
    ),
    (
        "labeled_control_bodies_follow_k1_statement_boundaries-005",
        r#"fun test(c:Boolean){ if(c) outer@ inner@ while(c) consume() else consume() }"#,
    ),
    (
        "labeled_control_bodies_follow_k1_statement_boundaries-006",
        r#"fun test(c:Boolean){ if(c) /* lead
 */ `loop`@ /* body
 */ while(c) consume() else consume() }"#,
    ),
    (
        "labeled_control_bodies_follow_k1_statement_boundaries-007",
        r#"fun test(c:Boolean){ if(c) local@ @A private @B inline fun local() {} else consume() }"#,
    ),
    (
        "labeled_control_bodies_follow_k1_statement_boundaries-008",
        r#"fun test(c:Boolean){ if(c) local@ @Ann object Local {} else consume() }"#,
    ),
    (
        "labeled_control_bodies_follow_k1_statement_boundaries-009",
        r#"fun test(){ val callback=fun(){}; local@ fun String.(){} }"#,
    ),
    (
        "local_destructuring_has_its_own_declaration_identity-000",
        r#"fun host(pair:Pair<Int,Int>){ val (left,right)=pair }"#,
    ),
    (
        "local_destructuring_has_its_own_declaration_identity-001",
        r#"fun host(){ val (left,right) }"#,
    ),
    (
        "local_destructuring_has_its_own_declaration_identity-002",
        r#"fun host(delegate:Any){ val (left,right) by delegate }"#,
    ),
    (
        "local_destructuring_has_its_own_declaration_identity-003",
        r#"fun host(pair:Pair<Int,Int>){ var (@A left,right:Int,)=pair }"#,
    ),
    (
        "local_destructuring_has_its_own_declaration_identity-004",
        r#"fun host(pair:Pair<Int,Int>){ label@ val (left,right)=pair }"#,
    ),
    (
        "local_destructuring_has_its_own_declaration_identity-005",
        r#"annotation class A; fun host(pair:Pair<Int,Int>){ @A val (left,right)=pair }"#,
    ),
    (
        "local_destructuring_has_its_own_declaration_identity-006",
        r#"annotation class A; fun host(c:Boolean,pair:Pair<Int,Int>){ if(c) @A val (left,right)=pair else Unit }"#,
    ),
    (
        "local_destructuring_has_its_own_declaration_identity-007",
        r#"fun host(pair:Pair<Int,Int>){ val (@A left,right:Int,)=pair; val plain=1 }"#,
    ),
    (
        "multi_dollar_strings_preserve_prefix_width_and_nested_contexts-000",
        r#"fun host(outer: Int, inner: Int) { val text = $$"""$outer $${ $$$"""$$inner $$$inner""" } $$outer"""; val plain = "$outer" }"#,
    ),
    (
        "multi_dollar_strings_preserve_prefix_width_and_nested_contexts-001",
        r#"fun host(name: Int) { val text = $$"$name $$name"; val escaped = $$"\$$name $$name" }"#,
    ),
    (
        "multi_dollar_strings_preserve_prefix_width_and_nested_contexts-002",
        r#"fun host(value: Int) { val text = $"""$value""" }"#,
    ),
    (
        "n-000",
        r#"typealias Plain = @Ann (T) -> R
typealias Nested = @Ann ((T) -> R) -> R
typealias Named = @Ann (value: T) -> R
typealias Stringy = @Ann (@Arg(")") T) -> R
typealias WithArguments = @Ann(value) (T) -> R"#,
    ),
    (
        "named_and_annotated_value_arguments_follow_k1-000",
        r#"fun f(value:Int){}
fun g(){ f(value=1) }"#,
    ),
    (
        "named_and_annotated_value_arguments_follow_k1-001",
        r#"fun f(vararg xs:Int){}
fun g(values:IntArray){ f(xs=*values) }"#,
    ),
    (
        "named_and_annotated_value_arguments_follow_k1-002",
        r#"annotation class A
fun f(value:Int){}
fun g(){ f(@A value) }"#,
    ),
    (
        "named_and_annotated_value_arguments_follow_k1-003",
        r#"annotation class A
fun f(value:Int){}
fun g(){ f(@A value=1) }"#,
    ),
    (
        "named_and_annotated_value_arguments_follow_k1-004",
        r#"annotation class A
fun f(value:Int){}
fun g(){ f(@A @A
value=1) }"#,
    ),
    (
        "named_and_annotated_value_arguments_follow_k1-005",
        r#"annotation class A
fun f(vararg xs:Int){}
fun g(values:IntArray){ f(value=1, @A @A other=2, xs=*values) }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-000",
        r#"fun host(c:Boolean){ if(c) fun yes() {} else fun no() = 0 }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-001",
        r#"fun host(c:Boolean){ while(c) fun local() {} }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-002",
        r#"fun host(xs:List<Int>){ for(x in xs) fun local() = x }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-003",
        r#"fun host(c:Boolean){ do fun local() {} while(c) }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-004",
        r#"fun host(c:Boolean){ when { c -> fun yes() {}; else -> fun no() = 0 } }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-005",
        r#"fun host(c:Boolean){ if(c) /* outer
 /* inner */ tail */ fun local() {} }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-006",
        r#"fun host(c:Boolean){ if(c) fun
<T> local(value:T):T = value }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-007",
        r#"fun host(c:Boolean){ if(c) fun`local`() {} }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-008",
        r#"fun host(c:Boolean){ if(c) fun (String).local() {} }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-009",
        r#"fun host(c:Boolean){ if(c) fun local()
{} }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-010",
        r#"fun host(c:Boolean){ if(c) fun local():
Int = 1 }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-011",
        r#"fun host(c:Boolean){ if(c) fun <T> local(value:T):T
where T:Any, T:Comparable<T>
= value }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-012",
        r#"fun host(c:Boolean){ if(c) fun local() = value
.member }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-013",
        r#"fun host(c:Boolean){ if(c) fun local() = value
?: other }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-014",
        r#"fun host(c:Boolean){ if(c) fun local() = value
+ other }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-015",
        r#"fun host(c:Boolean){ while(c) fun local() {}
next() }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-016",
        r#"fun host(c:Boolean){ while(c) fun local() {} // boundary
next() }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-017",
        r#"fun host(c:Boolean){ if(c) fun local() {}
else value }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-018",
        r#"fun host(c:Boolean){ if(c) fun local() {}; else value }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-019",
        r#"fun host(c:Boolean){ if(c) fun local() else value }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-020",
        r#"fun host(a:Boolean,b:Boolean){ if(a) if(b) fun local() else inner else outer }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-021",
        r#"fun host(c:Boolean){ do fun local() {}
while(c) }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-022",
        r#"fun host(c:Boolean){ do fun local() {} /* trailer
 nested */ while(c) }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-023",
        r#"fun host(c:Boolean){ while(c) fun local() {}; next() }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-024",
        r#"fun host(c:Boolean){ when { c -> if(c) fun local() {}
else -> value } }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-025",
        r#"fun host(c:Boolean){ if(c) funny() else funinterface() }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-026",
        r#"fun host(c:Boolean){ if(c) funλ() else fun1() }"#,
    ),
    (
        "named_local_function_control_bodies_follow_k1_line_boundaries-027",
        r#"fun host(c:Boolean){ if(c) `fun`() else value }"#,
    ),
    (
        "named_script_top_preserves_regular_file_boundaries-000",
        r#"public context(first: A, second: B = default,) suspend fun host() {}
context(Item) typealias F = String"#,
    ),
    (
        "nested_loops_are_control_structure_bodies_with_statement_identity-000",
        r#"fun test(c:Boolean){ if(c) while(c) consume() else consume() }"#,
    ),
    (
        "nested_loops_are_control_structure_bodies_with_statement_identity-001",
        r#"fun test(c:Boolean){ when { c -> while(c) consume(); else -> consume() } }"#,
    ),
    (
        "nested_loops_are_control_structure_bodies_with_statement_identity-002",
        r#"fun test(c:Boolean,xs:List<Int>){ for(x in xs) while(c) consume() }"#,
    ),
    (
        "nested_loops_are_control_structure_bodies_with_statement_identity-003",
        r#"fun test(c:Boolean,xs:List<Int>){ while(c) for(x in xs) consume() }"#,
    ),
    (
        "nested_loops_are_control_structure_bodies_with_statement_identity-004",
        r#"fun test(c:Boolean,xs:List<Int>){ do for(x in xs) consume() while(c) }"#,
    ),
    (
        "nested_loops_are_control_structure_bodies_with_statement_identity-005",
        r#"fun test(c:Boolean){ if(c) do consume() while(c) else consume() }"#,
    ),
    (
        "nested_loops_are_control_structure_bodies_with_statement_identity-006",
        r#"fun test(a:Boolean,b:Boolean){ do do consume() while(a) while(b) }"#,
    ),
    (
        "nested_loops_are_control_structure_bodies_with_statement_identity-007",
        r#"fun test(a:Boolean,b:Boolean){ while(a) while(b); consume() }"#,
    ),
    (
        "nested_loops_are_control_structure_bodies_with_statement_identity-008",
        r#"fun test(a:Boolean,xs:List<Int>){ for(x in xs) for(y in xs); consume() }"#,
    ),
    (
        "nested_loops_are_control_structure_bodies_with_statement_identity-009",
        r#"fun test(c:Boolean){ if(c) while(c); else consume() }"#,
    ),
    (
        "nested_loops_are_control_structure_bodies_with_statement_identity-010",
        r#"fun test(c:Boolean){ if(c) do while(c); else consume() }"#,
    ),
    (
        "nested_loops_are_control_structure_bodies_with_statement_identity-011",
        r#"fun test(a:Boolean,b:Boolean){ if(a) while(b) if(a) consume() else inner() else outer() }"#,
    ),
    (
        "nested_loops_are_control_structure_bodies_with_statement_identity-012",
        r#"fun test(a:Boolean,b:Boolean,xs:List<Int>){ if(a) while(b) for(x in xs) consume() else fallback() }"#,
    ),
    (
        "no_argument_annotations_preserve_function_type_parentheses-000",
        r#"class C(vararg values: Int, noinline first: () -> Unit, crossinline second: () -> Unit)
vararg fun produce() {}
noinline class Marker
crossinline val value = 1
fun consume(vararg values: Int) {}"#,
    ),
    (
        "nullable_callable_references_match_k1_postfix_receivers-000",
        r#"fun host(){ String?::length }"#,
    ),
    (
        "nullable_callable_references_match_k1_postfix_receivers-001",
        r#"fun host(){ List<T>?::size }"#,
    ),
    (
        "nullable_callable_references_match_k1_postfix_receivers-002",
        r#"fun host(){ pkg.List<T>?::size }"#,
    ),
    (
        "nullable_callable_references_match_k1_postfix_receivers-003",
        r#"fun host(){ (String)?::length }"#,
    ),
    (
        "nullable_callable_references_match_k1_postfix_receivers-004",
        r#"fun host(){ dynamic?::class }"#,
    ),
    (
        "nullable_callable_references_match_k1_postfix_receivers-005",
        r#"fun host(){ foo()?::bar }"#,
    ),
    (
        "nullable_callable_references_match_k1_postfix_receivers-006",
        r#"fun host(){ (foo())?::bar }"#,
    ),
    (
        "nullable_callable_references_match_k1_postfix_receivers-007",
        r#"fun host(){ 1?::toString }"#,
    ),
    (
        "nullable_callable_references_match_k1_postfix_receivers-008",
        r#"fun host(){ String??::length }"#,
    ),
    (
        "nullable_parenthesized_declaration_receivers_match_k1-000",
        r#"class A
fun (A)?.nullable() {}"#,
    ),
    (
        "nullable_parenthesized_declaration_receivers_match_k1-001",
        r#"class A
val (A)?.property: Int get() = 1"#,
    ),
    (
        "nullable_parenthesized_declaration_receivers_match_k1-002",
        r#"fun ((Int) -> String)?.invokeNullable() {}"#,
    ),
    (
        "nullable_parenthesized_declaration_receivers_match_k1-003",
        r#"class A
fun (A) ?.spaced() {}"#,
    ),
    (
        "nullable_parenthesized_declaration_receivers_match_k1-004",
        r#"class A
fun (A)? .spacedDot() {}"#,
    ),
    (
        "nullable_parenthesized_declaration_receivers_match_k1-005",
        r#"class A
fun (A)
?.newline() {}"#,
    ),
    (
        "nullable_parenthesized_declaration_receivers_match_k1-006",
        r#"class A
fun (A)??.multiple() {}"#,
    ),
    (
        "nullable_parenthesized_declaration_receivers_match_k1-007",
        r#"class A
fun (A)?./* receiver */commented() {}"#,
    ),
    (
        "nullable_parenthesized_declaration_receivers_match_k1-008",
        r#"class A
fun (A)?.() {}"#,
    ),
    (
        "nullable_parenthesized_declaration_receivers_match_k1-009",
        r#"class A
fun (A)??.multiple() {}
val (A)?.property: Int get() = 1"#,
    ),
    (
        "nameless_function_receivers_follow_k1_name_boundaries-000",
        r#"fun A.() {}"#,
    ),
    (
        "nameless_function_receivers_follow_k1_name_boundaries-001",
        r#"fun (A).() {}"#,
    ),
    (
        "nameless_function_receivers_follow_k1_name_boundaries-002",
        r#"fun A?.() {}"#,
    ),
    (
        "nameless_function_receivers_follow_k1_name_boundaries-003",
        r#"fun <T> T.() {}"#,
    ),
    (
        "nameless_function_receivers_follow_k1_name_boundaries-004",
        r#"fun host(){ fun A.() {} }"#,
    ),
    (
        "nameless_function_receivers_follow_k1_name_boundaries-005",
        r#"fun host(){ fun (A)?.() {} }"#,
    ),
    (
        "package_header_separators_preserve_k1_ranges-000",
        r#"package foo.bar; import foo
fun host() {}"#,
    ),
    (
        "package_header_separators_preserve_k1_ranges-001",
        r#"package foo.bar
import foo
fun host() {}"#,
    ),
    (
        "parenthesized_navigation_matches_k1_without_accepting_dot_class-000",
        r#"fun host(value:Any,other:Any){ val result=value.(other) }"#,
    ),
    (
        "parenthesized_navigation_matches_k1_without_accepting_dot_class-001",
        r#"fun host(value:Any,other:Any){ val result=value?.(other) }"#,
    ),
    (
        "parenthesized_navigation_matches_k1_without_accepting_dot_class-002",
        r#"fun host(value:Any,other:Any){ val result=value.(other).next }"#,
    ),
    (
        "parenthesized_navigation_matches_k1_without_accepting_dot_class-003",
        r#"fun host(value:Any,other:Any){ val result=value.(other)() }"#,
    ),
    (
        "parenthesized_navigation_matches_k1_without_accepting_dot_class-004",
        r#"fun host(other:Any){ val result={ value }.(other) }"#,
    ),
    (
        "parenthesized_navigation_matches_k1_without_accepting_dot_class-005",
        r#"fun host(other:Any){ val result=fun() {}.(other) }"#,
    ),
    (
        "parenthesized_user_types_preserve_definitely_non_null_shapes-000",
        r#"fun <T> f(x: (T) & Any) {}"#,
    ),
    (
        "parenthesized_user_types_preserve_definitely_non_null_shapes-001",
        r#"fun <T> f(x: ((T)) & (Any)) {}"#,
    ),
    (
        "parenthesized_user_types_preserve_definitely_non_null_shapes-002",
        r#"fun <T> f(x: (T) & @Right Any) {}"#,
    ),
    (
        "parenthesized_user_types_preserve_definitely_non_null_shapes-003",
        r#"typealias Plain = (String)"#,
    ),
    (
        "postfix_chains_remain_inside_control_structure_bodies-000",
        r#"fun test(flag: Boolean) { while (flag) foo().bar() }"#,
    ),
    (
        "postfix_chains_remain_inside_control_structure_bodies-001",
        r#"fun test(flag: Boolean) { do foo().bar() while (flag) }"#,
    ),
    (
        "postfix_chains_remain_inside_control_structure_bodies-002",
        r#"fun test(values: Values) { for (value in values) foo().bar() }"#,
    ),
    (
        "postfix_chains_remain_inside_control_structure_bodies-003",
        r#"fun test(flag: Boolean) { when { flag -> foo().bar(); else -> baz() } }"#,
    ),
    (
        "postfix_chains_remain_inside_control_structure_bodies-004",
        r#"fun test(flag: Boolean) { if (flag) foo().bar() else baz() }"#,
    ),
    (
        "prefix_bang_and_not_null_assertion_acceptance_matches_k1-000",
        r#"fun test(){ val value = !!true }"#,
    ),
    (
        "prefix_bang_and_not_null_assertion_acceptance_matches_k1-001",
        r#"fun test(){ val value = !!!true }"#,
    ),
    (
        "prefix_bang_and_not_null_assertion_acceptance_matches_k1-002",
        r#"fun test(){ val value = ! !true }"#,
    ),
    (
        "prefix_bang_and_not_null_assertion_acceptance_matches_k1-003",
        r#"fun test(){ val value = !true!! }"#,
    ),
    (
        "prefix_bang_and_not_null_assertion_acceptance_matches_k1-004",
        r#"fun test(){ val value = true!!!! }"#,
    ),
    (
        "prefix_bangs_and_not_null_assertions_keep_distinct_cst_ownership-000",
        r#"fun test(){ val a = !true!!; val b = true!!!! }"#,
    ),
    (
        "primary_constructors_continue_across_semantic_lines-000",
        r#"class Value
constructor(value: Int) {}"#,
    ),
    (
        "primary_constructors_continue_across_semantic_lines-001",
        r#"class Value<T>
constructor(value: T) {}"#,
    ),
    (
        "primary_constructors_continue_across_semantic_lines-002",
        r#"class Value
// constructor
constructor(value: Int) {}"#,
    ),
    (
        "primary_constructors_continue_across_semantic_lines-003",
        r#"class Value
/* constructor */
constructor(value: Int) {}"#,
    ),
    (
        "primary_constructors_continue_across_semantic_lines-004",
        r#"class Value
@Mark constructor(value: Int) {}"#,
    ),
    (
        "primary_constructors_continue_across_semantic_lines-005",
        r#"class Value
private constructor(value: Int) {}"#,
    ),
    (
        "primary_constructors_continue_across_semantic_lines-006",
        r#"class Value
public constructor(value: Int) {}"#,
    ),
    (
        "primary_constructors_continue_across_semantic_lines-007",
        r#"class Value
internal constructor(value: Int) {}"#,
    ),
    (
        "primary_constructors_continue_across_semantic_lines-008",
        r#"class Value
protected constructor(value: Int) {}"#,
    ),
    (
        "primary_constructors_continue_across_semantic_lines-009",
        r#"class Value
inline constructor(value: Int) {}"#,
    ),
    (
        "primary_constructors_continue_across_semantic_lines-010",
        r#"class Value
actual constructor(value: Int) {}"#,
    ),
    (
        "primary_constructors_continue_across_semantic_lines-011",
        r#"class Value
inline fun next() {}"#,
    ),
    (
        "primary_constructors_continue_across_semantic_lines-012",
        r#"class Value
public fun next() {}"#,
    ),
    (
        "primary_constructors_continue_across_semantic_lines-013",
        r#"class Value
@Mark class Next"#,
    ),
    (
        "primary_constructors_continue_across_semantic_lines-014",
        r#"class Value
actual class Next"#,
    ),
    (
        "projection_annotations_before_variance_match_k1_rejection-000",
        r#"typealias Valid<T> = List<out @Ann T>"#,
    ),
    (
        "property_boundaries_do_not_depend_on_bounded_accessor_lookahead-000",
        r#"class C { val x = 1
public get() = 1 }"#,
    ),
    (
        "property_boundaries_do_not_depend_on_bounded_accessor_lookahead-001",
        r#"class C { val x = 1
@Ann get() = 1 }"#,
    ),
    (
        "property_boundaries_do_not_depend_on_bounded_accessor_lookahead-002",
        r#"class C { val x = 1
public fun next() {} }"#,
    ),
    (
        "property_boundaries_do_not_depend_on_bounded_accessor_lookahead-003",
        r#"class C { val x = 1
@Ann fun next() {} }"#,
    ),
    (
        "property_boundaries_do_not_depend_on_bounded_accessor_lookahead-004",
        r#"class C { val x = 1
public val next = 2 }"#,
    ),
    (
        "property_boundaries_do_not_depend_on_bounded_accessor_lookahead-005",
        r#"fun test() { val x = 1
println(x) }"#,
    ),
    (
        "property_boundaries_do_not_depend_on_bounded_accessor_lookahead-006",
        r#"class C { val x = 1 public fun next() {} }"#,
    ),
    (
        "property_boundaries_do_not_depend_on_bounded_accessor_lookahead-007",
        r#"class C { val x = 1 public val next = 2 }"#,
    ),
    (
        "property_boundaries_do_not_depend_on_bounded_accessor_lookahead-008",
        r#"class C { val x = 1 /* trailing
comment */
public fun next() {} }"#,
    ),
    (
        "receiver_types_remain_in_the_type_group-000",
        r#"fun (String).extension() {}"#,
    ),
    (
        "reserved_words_and_operators_are_not_split_into_valid_tokens-000",
        r#"fun host(){ `typeof`(); typeofValue(); typeof_() }"#,
    ),
    (
        "reserved_words_and_operators_are_not_split_into_valid_tokens-001",
        r#"fun host(){ ; ; first; ;second }"#,
    ),
    (
        "reserved_words_and_operators_are_not_split_into_valid_tokens-002",
        r#"fun host(){ val text="typeof ;; => ... #"; /* typeof ;; => ... # */ Unit }"#,
    ),
    (
        "reserved_words_and_operators_are_not_split_into_valid_tokens-003",
        r#"fun host(){ ; ; }"#,
    ),
    (
        "semicolons_preserve_property_and_statement_-000",
        r#"val top = object : Contract {}; enum class Value { A }
class Container { val member = 1; fun next() {} }
val accessor: Int; @Ann private get() = 2"#,
    ),
    (
        "semicolons_preserve_property_and_statement_ownership-000",
        r#"fun host() { val first = 1; val second = 2; consume() }"#,
    ),
    (
        "short_string_interpolation_uses_the_same_identifie-000",
        r#"fun host(a𑁦: Int) { val value = "$a𑁦/$a²/$Ⅰ" }"#,
    ),
    (
        "short_this_interpolation_preserves_identifier_identity-000",
        r#"class Host { fun f() = "$this/$thisX/$this_/$Δvalue/$`spaced name`" }"#,
    ),
    (
        "short_this_interpolation_preserves_identifier_identity-001",
        r#"class Host { fun f() = "${this}" }"#,
    ),
    (
        "short_this_interpolation_preserves_identifier_identity-002",
        r#"class Host { fun f() = $$"$ literal, $$this/$${this}/$$thisX/$$this_" }"#,
    ),
    (
        "short_this_interpolation_preserves_identifier_identity-003",
        r#"class Host { fun f() = "$this" }"#,
    ),
    (
        "statement_continuation_keywords_respect_identifier_boundaries-000",
        r#"fun f(c:Boolean) { if(c) Unit
else1() }"#,
    ),
    (
        "statement_continuation_keywords_respect_identifier_boundaries-001",
        r#"fun f(c:Boolean) { if(c) Unit
elseλ() }"#,
    ),
    (
        "statement_continuation_keywords_respect_identifier_boundaries-002",
        r#"fun f(c:Boolean) { if(c) Unit
else Unit }"#,
    ),
    (
        "statement_continuation_keywords_respect_identifier_boundaries-003",
        r#"fun f(value:Any) { value
as`Type` }"#,
    ),
    (
        "string_and_character_escapes_match_kotlin_lexical_grammar-000",
        r#"fun host() { val text = "\t\b\r\n\'\"\\\$\u0041" }"#,
    ),
    (
        "string_and_character_escapes_match_kotlin_lexical_grammar-001",
        r#"fun host() { val values = listOf('\t', '\b', '\r', '\n', '\'', '\"', '\\', '\$', '\u0041') }"#,
    ),
    (
        "string_and_character_escapes_match_kotlin_lexical_grammar-002",
        r#"fun host() { val raw = """\q \u12xz""" }"#,
    ),
    (
        "string_and_character_escapes_match_kotlin_lexical_grammar-003",
        r#"fun host() { val text = "\q" }"#,
    ),
    (
        "string_and_character_escapes_match_kotlin_lexical_grammar-004",
        r#"fun host() { val text = "\a" }"#,
    ),
    (
        "string_and_character_escapes_match_kotlin_lexical_grammar-005",
        r#"fun host() { val text = "\u123" }"#,
    ),
    (
        "string_and_character_escapes_match_kotlin_lexical_grammar-006",
        r#"fun host() { val text = "\u12xz" }"#,
    ),
    (
        "string_and_character_escapes_match_kotlin_lexical_grammar-007",
        r#"fun host() { val value = '\q' }"#,
    ),
    (
        "string_and_character_escapes_match_kotlin_lexical_grammar-008",
        r#"fun host() { val value = '\a' }"#,
    ),
    (
        "string_and_character_escapes_match_kotlin_lexical_grammar-009",
        r#"fun host() { val value = '\u123' }"#,
    ),
    (
        "string_and_character_escapes_match_kotlin_lexical_grammar-010",
        r#"fun host() { val value = '\u12xz' }"#,
    ),
    (
        "string_templates_follow_k1_multiline_and_error_boundaries-000",
        r#"fun host(name: String) { val value = "$"; val digit = "$1"; val spaced = "$ name"; val escaped = "\$name" }"#,
    ),
    (
        "string_templates_follow_k1_multiline_and_error_boundaries-001",
        r#"fun host(value: Int) { val value = "nested=${if (value > 0) "${value}" else "none"}" }"#,
    ),
    (
        "string_templates_follow_k1_multiline_and_error_boundaries-002",
        r#"fun host(name: String, value: Int) { val value = """line
$name
${value + 1}
quotes "" here
closing quote: """" }"#,
    ),
    (
        "string_templates_follow_k1_multiline_and_error_boundaries-003",
        r#"fun host(value: Int) { val value = "lambda=${run { value }}" }"#,
    ),
    (
        "string_templates_preserve_segments_interpolation_and_delimiters-000",
        r#"fun host(name: String, value: Int) { val text = "hello $name=${value + 1}" }"#,
    ),
    (
        "type_modifiers_preserve_type_groups_and_annotation_identity-000",
        r#"@Decl class Marker
"#,
    ),
    (
        "type_modifiers_preserve_type_groups_and_annotation_identity-001",
        r#"val direct: @Type String = ""
"#,
    ),
    (
        "type_modifiers_preserve_type_groups_and_annotation_identity-002",
        r#"val function: suspend (Int) -> @Result String = { "" }
"#,
    ),
    (
        "type_modifiers_preserve_type_groups_and_annotation_identity-003",
        r#"val projected: List<out @Element String> = listOf()
"#,
    ),
    (
        "type_modifiers_reach_supported_type_positions-000",
        r#"typealias Direct = @Ann String"#,
    ),
    (
        "type_modifiers_reach_supported_type_positions-001",
        r#"typealias Nullable = @Ann String?"#,
    ),
    (
        "type_modifiers_reach_supported_type_positions-002",
        r#"typealias Parenthesized = (@Ann String)"#,
    ),
    (
        "type_modifiers_reach_supported_type_positions-003",
        r#"typealias Function = @Ann("type") (Int) -> @Result String"#,
    ),
    (
        "type_modifiers_reach_supported_type_positions-004",
        r#"typealias Suspended = suspend (Int) -> Unit"#,
    ),
    (
        "type_modifiers_reach_supported_type_positions-005",
        r#"typealias Receiver = @Outer String.() -> Unit"#,
    ),
    (
        "type_modifiers_reach_supported_type_positions-006",
        r#"val anonymous = fun @Receiver String.() {}"#,
    ),
    (
        "type_modifiers_reach_supported_type_positions-007",
        r#"typealias Projected = List<out @Element String>"#,
    ),
    (
        "type_modifiers_reach_supported_type_positions-008",
        r#"fun <T> constrained(x: @Left T & @Right Any) {}"#,
    ),
    (
        "typed_destructuring_lambda_parameters_preserve_ranges-000",
        r#"fun host(){ val lambda={ (left,right): Pair<Int,Int> -> left } }"#,
    ),
    (
        "typed_destructuring_lambda_parameters_preserve_ranges-001",
        r#"fun host(){ val lambda={ (left:Int,right:Int): Pair<Int,Int> -> left } }"#,
    ),
    (
        "typed_destructuring_lambda_parameters_preserve_ranges-002",
        r#"fun host(){ val lambda={ (left,right,): Pair<Int,Int> -> left } }"#,
    ),
    (
        "typed_destructuring_lambda_parameters_preserve_ranges-003",
        r#"fun host(values:List<Pair<Int,Int>>){ values.map { (left,right): Pair<Int,Int> -> left } }"#,
    ),
    (
        "typed_destructuring_lambda_parameters_preserve_ranges-004",
        r#"fun host(c:Boolean){ if(c) { (left,right): Pair<Int,Int> -> left } else Unit }"#,
    ),
    (
        "typed_destructuring_lambda_parameters_preserve_ranges-005",
        r#"fun host(c:Boolean){ while(c) { (left,right): Pair<Int,Int> -> left } }"#,
    ),
    (
        "value_argument_names_preserve_k1_cst_shape-000",
        r#"import foo as bar
import foo.bar as baz
import foo.*
fun host() {}"#,
    ),
    (
        "whitespace_matches_the_pinned_kotlin_2_4_10_lexer_profile-000",
        r#"fun host(){ val text = "a b"; /*   */ Unit }"#,
    ),
    (
        "wildcard_imports_are_terminal_and_have_dedicated_cst_identity-000",
        r#"import foo.bar.*
fun host() {}"#,
    ),
    (
        "wildcard_imports_are_terminal_and_have_dedicated_cst_identity-001",
        r#"import foo.
*
fun host() {}"#,
    ),
    (
        "wildcard_imports_are_terminal_and_have_dedicated_cst_identity-002",
        r#"import foo./* wildcard */*
fun host() {}"#,
    ),
    (
        "wildcard_imports_are_terminal_and_have_dedicated_cst_identity-003",
        r#"import foo as bar
fun host() {}"#,
    ),
    (
        "wildcard_imports_are_terminal_and_have_dedicated_cst_identity-004",
        r#"import foo; import bar as baz
fun host() {}"#,
    ),
];

const KNOWN_STRICT_REJECTIONS: &[(&str, u32)] = &[];

#[derive(Debug, Eq, PartialEq)]
struct StrictRejection {
    case: &'static str,
    byte: Option<u32>,
}

#[test]
fn k1_accepted_inline_corpus_matches_the_strict_rejection_snapshot() {
    assert_eq!(K1_ACCEPTED.len(), EXPECTED_K1_ACCEPTED);

    let parser = rezel_lang_kotlin::parser().with_strict(true);
    let mut actual = Vec::new();
    for &(case, source) in K1_ACCEPTED {
        if let Err(error) = parser.parse(source) {
            actual.push(StrictRejection {
                case,
                byte: error.position().map(u32::from),
            });
        }
    }
    let expected = KNOWN_STRICT_REJECTIONS
        .iter()
        .map(|&(case, byte)| StrictRejection {
            case,
            byte: Some(byte),
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, expected, "inline Kotlin strict rejection snapshot");
    assert_eq!(
        EXPECTED_K1_ACCEPTED - actual.len(),
        EXPECTED_STRICT_ACCEPTED,
        "inline Kotlin strict support inventory"
    );
}
