//! Non-incremental runtime conformance cases.
//!
//! Incremental reuse, ranged input, and wall-clock performance checks are
//! intentionally outside this file's scope.

use std::cell::RefCell;

use rezel_common::{IterMode, SyntaxNode, TextRange, TextSize, Tree, TreeChild};
use rezel_lr::LRParser;

#[path = "generated/test_parse/contentless.rs"]
#[rustfmt::skip]
mod contentless;
#[path = "generated/test_parse/parse.rs"]
#[rustfmt::skip]
mod parse;
#[path = "generated/test_parse/sequence.rs"]
#[rustfmt::skip]
mod sequence;
#[path = "generated/test_parse/tops.rs"]
#[rustfmt::skip]
mod tops;

fn size(value: u32) -> TextSize {
    TextSize::from(value)
}

fn parser(language: &'static rezel_lr::Language) -> LRParser {
    LRParser::from_language(language)
}

fn preorder(tree: &Tree, mode: IterMode) -> Vec<SyntaxNode> {
    let mut cursor = tree.cursor(mode);
    let mut nodes = vec![cursor.node()];
    while cursor.next(true) {
        nodes.push(cursor.node());
    }
    nodes
}

fn named(tree: &Tree, name: &str) -> Vec<SyntaxNode> {
    preorder(tree, IterMode::NONE)
        .into_iter()
        .filter(|node| node.name().as_ref() == name)
        .collect()
}

fn assert_node(node: &SyntaxNode, name: &str, from: u32, to: u32) {
    assert_eq!(node.name().as_ref(), name);
    assert_eq!(node.range(), TextRange::new(size(from), size(to)));
}

#[test]
fn reports_node_positions() {
    let source = "if 1 { while 2 { foo(bar(baz bug)); } }";
    let tree = parser(&parse::LANGUAGE).parse(source).unwrap();

    assert_eq!(tree.len(), size(39));
    assert_node(&named(&tree, "Cond")[0], "Cond", 0, 39);
    assert_node(&named(&tree, "Num")[0], "Num", 3, 4);
    assert_node(&named(&tree, "Loop")[0], "Loop", 7, 37);
    assert_node(&named(&tree, "Num")[1], "Num", 13, 14);
    assert_node(&named(&tree, "Call")[0], "Call", 17, 34);
    assert_node(&named(&tree, "Call")[1], "Call", 21, 33);
    assert_node(&named(&tree, "Var")[1], "Var", 21, 24);
    assert_node(&named(&tree, "Var")[3], "Var", 29, 32);
}

#[test]
fn resolves_nodes_in_buffers_and_materialized_trees() {
    let source = "while 111 { one; two(three 20); }";

    for buffer_length in [1024, 2] {
        let tree = parser(&parse::LANGUAGE)
            .with_buffer_length(size(buffer_length))
            .parse(source)
            .unwrap();

        let mut cursor = tree.cursor_at(size(7), 0, IterMode::NONE);
        assert_node(&cursor.node(), "Num", 6, 9);
        assert!(cursor.parent());
        assert_node(&cursor.node(), "Loop", 0, 33);

        cursor.move_to(size(22), 0);
        assert_node(&cursor.node(), "Var", 21, 26);
        assert!(cursor.parent());
        assert_node(&cursor.node(), "Call", 17, 30);

        cursor.move_to(size(18), 0);
        assert_node(&cursor.node(), "Var", 17, 20);

        cursor.move_to(size(6), 0);
        assert_eq!(cursor.name().as_ref(), "Loop");
        cursor.move_to(size(9), 0);
        assert_eq!(cursor.name().as_ref(), "Loop");

        cursor.move_to(size(20), 0);
        assert_eq!(cursor.name().as_ref(), "Call");
        assert!(cursor.first_child());
        assert_eq!(cursor.name().as_ref(), "Var");
        assert!(cursor.next_sibling());
        assert_eq!(cursor.name().as_ref(), "Var");
        assert!(cursor.next_sibling());
        assert_eq!(cursor.name().as_ref(), "Num");
        assert!(!cursor.next_sibling());
    }
}

fn iteration_events(tree: &Tree, range: TextRange) -> Vec<(String, u32)> {
    let events = RefCell::new(Vec::new());
    tree.iterate(
        range,
        IterMode::NONE,
        |node| {
            events
                .borrow_mut()
                .push((node.name().to_string(), u32::from(node.from())));
            true
        },
        |node| {
            events
                .borrow_mut()
                .push((format!("/{}", node.name()), u32::from(node.to())));
        },
    );
    events.into_inner()
}

fn expected_events(values: &[(&str, u32)]) -> Vec<(String, u32)> {
    values
        .iter()
        .map(|(name, position)| ((*name).to_owned(), *position))
        .collect()
}

#[test]
fn iterates_full_and_partial_trees_across_storage_modes() {
    let source = "while 1 { a; b; c(d e); } while 2 { f; }";
    let full = expected_events(&[
        ("T", 0),
        ("Loop", 0),
        ("Num", 6),
        ("/Num", 7),
        ("Block", 8),
        ("Var", 10),
        ("/Var", 11),
        ("Var", 13),
        ("/Var", 14),
        ("Call", 16),
        ("Var", 16),
        ("/Var", 17),
        ("Var", 18),
        ("/Var", 19),
        ("Var", 20),
        ("/Var", 21),
        ("/Call", 22),
        ("/Block", 25),
        ("/Loop", 25),
        ("Loop", 26),
        ("Num", 32),
        ("/Num", 33),
        ("Block", 34),
        ("Var", 36),
        ("/Var", 37),
        ("/Block", 40),
        ("/Loop", 40),
        ("/T", 40),
    ]);
    let partial = expected_events(&[
        ("T", 0),
        ("Loop", 0),
        ("Block", 8),
        ("Var", 13),
        ("/Var", 14),
        ("Call", 16),
        ("Var", 16),
        ("/Var", 17),
        ("Var", 18),
        ("/Var", 19),
        ("/Call", 22),
        ("/Block", 25),
        ("/Loop", 25),
        ("/T", 40),
    ]);

    for buffer_length in [1024, 2] {
        let tree = parser(&parse::LANGUAGE)
            .with_buffer_length(size(buffer_length))
            .parse(source)
            .unwrap();
        assert_eq!(
            iteration_events(&tree, TextRange::new(size(0), tree.len())),
            full
        );
        assert_eq!(
            iteration_events(&tree, TextRange::new(size(13), size(19))),
            partial
        );
    }
}

#[test]
fn iteration_can_skip_a_subtree() {
    let tree = parser(&parse::LANGUAGE)
        .parse("foo(baz(baz), bug(quux)")
        .unwrap();
    let variables = RefCell::new(0);

    tree.iterate(
        TextRange::new(size(0), tree.len()),
        IterMode::NONE,
        |node| {
            if node.name().as_ref() == "Var" {
                *variables.borrow_mut() += 1;
            }
            !(node.name().as_ref() == "Call" && node.from() == size(4))
        },
        |_| {},
    );

    assert_eq!(variables.into_inner(), 3);
}

fn semantic_count(tree: &Tree, name: &str) -> usize {
    named(tree, name).len()
}

fn storage_depth(tree: &Tree) -> usize {
    tree.children()
        .iter()
        .map(|child| match child {
            TreeChild::Tree(child) => storage_depth(child) + 1,
            TreeChild::Buffer(_) => 2,
        })
        .max()
        .unwrap_or(1)
}

fn storage_breadth(tree: &Tree) -> usize {
    tree.children()
        .iter()
        .filter_map(|child| match child {
            TreeChild::Tree(child) => Some(storage_breadth(child)),
            TreeChild::Buffer(_) => None,
        })
        .fold(tree.children().len(), usize::max)
}

#[test]
fn balances_long_sequences_without_changing_their_semantics() {
    let source = "x".repeat(1000);
    let tree = parser(&sequence::LANGUAGE)
        .with_strict(true)
        .with_buffer_length(size(10))
        .parse(&source)
        .unwrap();

    assert_eq!(semantic_count(&tree, "X"), 1000);
    assert_eq!(tree.len(), size(1000));

    let depth = storage_depth(&tree);
    let breadth = storage_breadth(&tree);
    assert!(
        (4..=6).contains(&depth),
        "unexpected sequence depth: {depth}"
    );
    assert!(
        (5..=10).contains(&breadth),
        "unexpected sequence breadth: {breadth}"
    );
}

#[test]
fn preserves_a_long_contentless_repeat() {
    let source = format!("a[{}]", "b".repeat(500));
    let tree = parser(&contentless::LANGUAGE)
        .with_buffer_length(size(10))
        .parse(&source)
        .unwrap();

    assert_eq!(tree.to_string(), "T(A,B)");
    assert_node(&named(&tree, "A")[0], "A", 0, 1);
    assert_node(&named(&tree, "B")[0], "B", 1, 503);
    assert!(storage_depth(&tree) >= 5);
}

#[test]
fn skipped_tokens_do_not_defeat_sequence_balancing() {
    let source = "xc".repeat(1000);
    let tree = parser(&sequence::LANGUAGE)
        .with_strict(true)
        .with_buffer_length(size(10))
        .parse(&source)
        .unwrap();

    assert_eq!(semantic_count(&tree, "X"), 1000);
    assert_eq!(tree.len(), size(2000));

    let depth = storage_depth(&tree);
    let breadth = storage_breadth(&tree);
    assert!(
        (4..=6).contains(&depth),
        "unexpected sequence depth: {depth}"
    );
    assert!(
        (5..=10).contains(&breadth),
        "unexpected sequence breadth: {breadth}"
    );
}

#[test]
fn long_sequence_nodes_retain_exact_positions() {
    let source = format!("{}y;;;;;;;;;{}", "x".repeat(100), "x".repeat(90));
    let tree = parser(&sequence::LANGUAGE).parse(&source).unwrap();
    let nodes = preorder(&tree, IterMode::NONE);

    assert_node(&nodes[0], "T", 0, 200);
    assert_eq!(nodes.len(), 192);
    for (index, node) in nodes[1..=100].iter().enumerate() {
        let from = u32::try_from(index).unwrap();
        assert_node(node, "X", from, from + 1);
    }
    assert_node(&nodes[101], "Y", 100, 110);
    for (index, node) in nodes[102..].iter().enumerate() {
        let from = 110 + u32::try_from(index).unwrap();
        assert_node(node, "X", from, from + 1);
    }
}

#[test]
fn selects_named_and_default_top_rules() {
    let default = parser(&tops::LANGUAGE);
    assert_eq!(default.parse("bc").unwrap().to_string(), "X(FOO(B),C)");
    assert_eq!(
        default
            .clone()
            .with_top("X")
            .unwrap()
            .parse("bc")
            .unwrap()
            .to_string(),
        "X(FOO(B),C)"
    );
    assert_eq!(
        default
            .with_top("Y")
            .unwrap()
            .parse("bc")
            .unwrap()
            .to_string(),
        "Y(B,C)"
    );
}
