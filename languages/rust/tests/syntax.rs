#![forbid(unsafe_code)]

use rezel_common::ParseErrorKind;

#[test]
fn edition_2024_modern_syntax_is_accepted() {
    let source = r#"
unsafe extern "C" {
    pub safe fn read(value: *const u8) -> usize;
    pub unsafe fn write(value: *mut u8);
    pub unsafe fn variadic(format: *const u8, args: ...);
    pub safe static VERSION: i32;
    pub static mut STATE: i32;
}

fn modern<'a, T>(value: &'a mut T) -> impl Sized + use<'a, T,> {
    let shared = &&raw const *value;
    let mutable = &raw mut *value;
    (shared, mutable)
}
"#;

    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("Rust 1.95 Edition 2024 item, borrow, and capture syntax");
}

#[test]
fn precise_capture_bounds_enforce_context_free_static_rules() {
    for source in [
        "fn rejected<'a, T>(value: &'a T) -> impl Sized + use<T, 'a> { value }",
        "fn rejected<T>(value: T) -> impl Sized + use<T> + use<T> { value }",
    ] {
        let error = rezel_lang_rust::parser()
            .with_strict(true)
            .parse(source)
            .expect_err("invalid precise capturing bounds must fail strict parsing");
        assert_eq!(error.kind(), ParseErrorKind::Syntax);
    }
}

#[test]
fn context_sensitive_words_remain_identifiers_outside_their_contexts() {
    let source = r"
fn words() {
    let raw = 1;
    let safe = 2;
    let union = 3;
    let macro_rules = 4;
    let default = 5;
    let auto = 6;
    let _ = (raw, safe, union, macro_rules, default, auto);
}
";
    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("weak keywords and parser-contextual words remain identifiers elsewhere");
}

#[test]
fn macro_rules_definition_uses_token_lookahead() {
    let source = r"
macro_rules /* outer /* nested */ comment */ ! // macro name
generated {
    () => {};
}

macro_rules! {}
";
    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("macro_rules definitions and same-named macro invocations are distinguished");
}

#[test]
fn block_match_arms_accept_their_optional_comma() {
    let source = "fn choose(value: bool) { match value { true => {}, false => {} } }";
    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("block match arms may include a trailing comma");
}

#[test]
fn attribute_name_values_accept_expressions() {
    let source = r#"#![doc = include_str!("guide.md")]
fn documented() {}"#;
    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("attribute name-value inputs use Rust expressions");
}

#[test]
fn macro_token_trees_accept_at_punctuation() {
    let source = r"
features! { @TARGET: aarch64; %= ..= <- ~ }

macro_rules! generated {
    ($name:ident) => {
        fn ${concat(prefix_, $name)}() {}
    };
}

macro_rules! transcribe_colons {
    ($ty:ty, $bound:ident, $name:ident) => {
        tokens!($ty: $bound, $name: 0, $ty: $bound, $name: 0);
    };
}
";
    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("macro token trees accept Rust punctuation and metavariable expressions");
}

#[test]
fn inferred_types_are_accepted_in_type_positions() {
    let source = r"
fn inferred(value: u8) {
    let tuple: (u8, _) = (value, value);
    let vector: Vec<_> = Vec::new();
    let _ = value as _;
}
";
    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("the inferred type is a shared Rust type form");
}

#[test]
fn generic_parameters_accept_outer_attributes() {
    let source = r"
struct Allocated<
    T,
    #[cfg(any())] A: Allocator = Global,
>(T, A);
";
    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("generic parameters may carry outer attributes");
}

#[test]
fn const_traits_and_impls_accept_their_front_matter_orders() {
    let source = r"
pub const trait ConstTrait {}
const impl<T> ConstTrait for Wrapper<T> {}
unsafe impl<T> const ConstTrait for Wrapper<T> {}
";
    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("Rust const trait and impl front matter");
}

#[test]
fn const_trait_bounds_share_the_regular_bound_positions() {
    let source = r"
fn bounded<T: [const] First + ~const Second>()
where
    T: const Third,
{
}

fn abstracted(value: impl [const] First + [const] Second) {}

unsafe impl<T> const First for &T
where
    T: [const] Second + ?Sized,
{
}
";
    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("const trait modifiers apply to ordinary trait-bound positions");
}

#[test]
fn associated_item_constraints_accept_bounds_and_gat_arguments() {
    let source = r"
fn constrained<T>()
where
    T: Iterator<Item: Debug> + Trait<Assoc<'static>: Send, Output = u8>,
{
}
";
    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("associated item constraints support bounds, equality, and GAT arguments");
}

#[test]
fn associated_constraints_do_not_capture_nested_generic_arguments() {
    for source in [
        "type Nested = Result<Option<NodeRef<Marker>>, Error>;",
        "type Scoped<'a> = marker::Mut<'a>;",
        "type Node<'a, K, V> = NodeRef<marker::Mut<'a>, K, V, marker::Internal>;",
        "type Optional<'a, K, V> = Option<NodeRef<marker::Mut<'a>, K, V, marker::Internal>>;",
        "type DeeplyNested<'a, K, V> = Result<Option<NodeRef<marker::Mut<'a>, K, V, marker::Internal>>, Error>;",
        "type Qualified<T> = module::Outer<T>::Inner<Option<T>>;",
        "type Repeated<T> = A<B<C<D<E<T>>>>>;",
    ] {
        rezel_lang_rust::parser()
            .with_strict(true)
            .parse(source)
            .unwrap_or_else(|error| {
                panic!("ordinary nested generic type must parse: {source}\n{error}")
            });
    }
}

#[test]
fn function_trait_and_pointer_parameters_use_type_syntax() {
    let source = r#"
fn constrained<F, T>()
where
    F: for<'a> FnOnce(&'a mut T) -> bool,
{
}

type Callback = for<'a> unsafe extern "C" fn(
    #[cfg(any())] value: &'a mut u8,
    _: usize,
    ...
) -> bool;

fn dynamic_callback(callback: &mut dyn FnMut(&i32) -> bool) {}
"#;

    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("function traits take types and bare function pointers allow optional names");
}

#[test]
fn attributed_statements_and_try_blocks_are_accepted() {
    let source = r"
fn attributed() -> Result<u8, Error> {
    #[cfg(any())]
    consume();

    #[cfg(any())]
    {
        consume();
    }

    try { produce()? }
}
";

    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("outer attributes apply to statements and try blocks are block expressions");
}

#[test]
fn declarative_macro_items_follow_the_rustc_shape() {
    let source = r"
pub macro passthrough($($token:tt)*) {
    $($token)*
}

macro empty {}
";

    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("macro 2.0 items have optional parameters and a braced body");
}

#[test]
fn nested_reference_types_and_self_crate_imports_are_accepted() {
    let source = r"
extern crate self as current;

fn compare(left: &&Value, right: &&mut Value) -> bool {
    left == right
}
";

    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("joint ampersands split in types and self may name the current crate");
}

#[test]
fn modern_alias_range_and_discard_forms_are_accepted() {
    let source = r"
use crate::Value as _;

const _: () = {};

trait Family {
    type Member<'a>;
}

impl Family for Value {
    type Member<'a>
        = Borrowed<'a>
    where
        Self: 'a;
}

fn discard(value: Result<u8, Error>) {
    _ = value;
    match 1 {
        0..LIMIT => {}
        _ => {}
    }
}
";

    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("post-RHS where clauses, exclusive range patterns, and discard assignments");
}

#[test]
fn reference_patterns_and_removed_impl_bounds_are_accepted() {
    let source = r"
fn inspect(values: &[u8], error: &(impl Error + ?Sized)) {
    let _ = values.iter().filter(|&&value| value != 0);
    consume(error);
}
";

    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("joint ampersands form nested reference patterns and ? bounds extend impl Trait");
}

#[test]
fn unsafe_attributes_and_attributed_tail_expressions_are_accepted() {
    let source = r#"
#[unsafe(no_mangle)]
pub extern "C" fn exported() {}

fn selected() -> u8 {
    #[cfg(any())]
    {
        0
    }

    #[cfg(not(any()))]
    1
}
"#;

    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("unsafe attributes and attributes on a block tail expression");
}

#[test]
fn less_than_or_equal_after_a_cast_is_not_a_generic_delimiter() {
    let source = "fn is_ascii(value: char) -> bool { value as u32 <= 0x7f }";

    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("the type-argument tokenizer must decline the <= operator");
}

#[test]
fn auto_traits_trait_aliases_and_const_parameter_defaults_are_accepted() {
    let source = r"
pub auto trait AutoTrait {}
pub unsafe auto trait UnsafeAutoTrait {}
pub const unsafe auto trait ConstUnsafeAutoTrait {}

pub trait Thin = Pointee<Metadata = ()> + PointeeSized;

pub unsafe trait TransmuteFrom<Src, const ASSUME: Assume = { Assume::NOTHING }>
where
    Src: ?Sized,
{
}
";

    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("Rust trait front matter, aliases, and const generic defaults");
}

#[test]
fn higher_ranked_dyn_bounds_foreign_types_and_named_variadics_are_accepted() {
    let source = r#"
type Object = dyn for<'a> Trait<'a>;
type Deferred = impl (FnOnce() -> Value) + Send;
type Callback = unsafe extern "efiapi" fn(_: *mut Handle, _: ...) -> Status;

unsafe extern "C" {
    pub type Extern;
}
"#;

    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("higher-ranked bounds and the complete foreign-item type surface");
}

#[test]
fn attributed_elements_labeled_blocks_and_struct_field_indices_are_accepted() {
    let source = r"
fn expressions<T>(value: T) {
    let closure = #[cold] |input: u8| input;
    let values = [
        #[cfg(any())]
        1,
        #[cfg(not(any()))]
        2,
    ];
    let wrapper = Wrapper::<T> { 0: value };

    'done: {
        break 'done;
    }

    match values[0] {
        ..LIMIT => {}
    }

    consume(closure, wrapper);
}
";

    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("attributes and labels apply at their Rust expression boundaries");
}

#[test]
fn inferred_and_negative_const_arguments_are_accepted() {
    let source = r"
extern crate runtime as _;

fn consts() {
    barrier::<-1>();
    let array: [[u8; 64]; 16] = from_fn(|index| [index as _; _]);
    consume(array);
}
";

    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("negative and inferred const arguments use their restricted syntax");
}

#[test]
fn let_chains_accept_multiple_plain_boolean_operands() {
    let source = r"
fn wake(downgrade: bool, is_writer: bool, previous: Option<Node>) {
    if !downgrade
        && is_writer
        && let Some(node) = previous
    {
        consume(node);
    }
}
";

    rezel_lang_rust::parser()
        .with_strict(true)
        .parse(source)
        .expect("top-level && remains a valid separator before a let condition");
}
