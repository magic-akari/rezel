use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use rezel_common::{
    IterMode, NodeFlags, NodeSet, NodeType, SyntaxNode, TextRange, TextSize, Tree, TreeBuild,
    TreeChild, group_prop,
};

const SIMPLE_COUNTS: [(&str, usize); 6] =
    [("a", 7), ("b", 3), ("c", 3), ("Br", 3), ("Pa", 2), ("T", 1)];

fn node_set() -> Arc<NodeSet> {
    static NODE_SET: OnceLock<Arc<NodeSet>> = OnceLock::new();

    Arc::clone(NODE_SET.get_or_init(|| {
        let mut types = ["T", "a", "b", "c", "Pa", "Br"]
            .into_iter()
            .enumerate()
            .map(|(id, name)| {
                let node_type = NodeType::new(id.try_into().unwrap(), name, NodeFlags::default());
                if matches!(name, "a" | "b" | "c") {
                    node_type.with_prop(group_prop(), vec!["atom".to_owned()])
                } else {
                    node_type
                }
            })
            .collect::<Vec<_>>();
        types.push(NodeType::new(
            types.len().try_into().unwrap(),
            "",
            NodeFlags::ANONYMOUS,
        ));
        Arc::new(NodeSet::new(types))
    }))
}

fn id(node_set: &NodeSet, name: &str) -> u16 {
    node_set
        .types()
        .iter()
        .find(|node_type| node_type.name() == name)
        .unwrap()
        .id()
}

fn mk(spec: &str) -> Tree {
    let node_set = node_set();
    let mut starts = Vec::new();
    let mut buffer = Vec::new();

    for (position, byte) in spec.bytes().enumerate() {
        let position: u32 = position.try_into().unwrap();
        match byte {
            b'a' | b'b' | b'c' => {
                let name = char::from(byte).to_string();
                buffer.extend_from_slice(&[
                    u32::from(id(&node_set, &name)),
                    position,
                    position + 1,
                    4,
                ]);
            }
            b'(' | b'[' => {
                starts.push((buffer.len(), position));
            }
            b')' | b']' => {
                let (start_offset, start) = starts.pop().unwrap();
                let name = if byte == b')' { "Pa" } else { "Br" };
                let size: u32 = (buffer.len() + 4 - start_offset).try_into().unwrap();
                buffer.extend_from_slice(&[
                    u32::from(id(&node_set, name)),
                    start,
                    position + 1,
                    size,
                ]);
            }
            _ => panic!("invalid tree fixture byte"),
        }
    }
    assert!(starts.is_empty(), "unclosed tree fixture delimiter");

    let repeat_id = node_set.types().last().unwrap().id();
    let mut build = TreeBuild::new(buffer, node_set, 0);
    build.max_buffer_length = 10.into();
    build.min_repeat_type = usize::from(repeat_id);
    Tree::build(&build)
}

fn recursive_spec(depth: usize) -> String {
    if depth == 0 {
        return (0..20).map(|index| char::from(b"abc"[index % 3])).collect();
    }

    let inner = recursive_spec(depth - 1);
    format!("({inner})[{inner}]")
}

fn recur() -> Tree {
    static TREE: OnceLock<Tree> = OnceLock::new();
    TREE.get_or_init(|| mk(&recursive_spec(6))).clone()
}

fn simple() -> Tree {
    static TREE: OnceLock<Tree> = OnceLock::new();
    TREE.get_or_init(|| mk("aaaa(bbb[ccc][aaa][()])")).clone()
}

fn anonymous_tree() -> Tree {
    static TREE: OnceLock<Tree> = OnceLock::new();
    TREE.get_or_init(|| {
        let node_set = node_set();
        let anonymous = Tree::new(
            NodeType::none(),
            vec![
                TreeChild::Tree(Tree::new(
                    node_set.get(1).unwrap().clone(),
                    Vec::new(),
                    Vec::new(),
                    1.into(),
                )),
                TreeChild::Tree(Tree::new(
                    node_set.get(2).unwrap().clone(),
                    Vec::new(),
                    Vec::new(),
                    1.into(),
                )),
            ],
            vec![0.into(), 1.into()],
            2.into(),
        );
        Tree::new(
            node_set.get(0).unwrap().clone(),
            vec![TreeChild::Tree(anonymous)],
            vec![0.into()],
            2.into(),
        )
    })
    .clone()
}

fn packing_tree(records: &[(u16, u32, u32)], max_buffer_length: u32) -> Tree {
    static NODE_SET: OnceLock<Arc<NodeSet>> = OnceLock::new();
    let node_set = Arc::clone(NODE_SET.get_or_init(|| {
        Arc::new(NodeSet::new(vec![
            NodeType::new(0, "T", NodeFlags::TOP),
            NodeType::new(1, "Comment", NodeFlags::SKIPPED),
            NodeType::new(2, "Item", NodeFlags::default()),
        ]))
    }));
    let mut buffer = Vec::with_capacity(records.len() * 4);
    for &(id, start, end) in records {
        buffer.extend_from_slice(&[u32::from(id), start, end, 4]);
    }
    let mut build = TreeBuild::new(buffer, node_set, 0);
    build.length = Some(4.into());
    build.max_buffer_length = max_buffer_length.into();
    Tree::build(&build)
}

fn syntax_snapshot(tree: &Tree) -> Vec<(String, TextRange)> {
    let mut cursor = tree.cursor(IterMode::INCLUDE_ANONYMOUS);
    let mut snapshot = Vec::new();
    loop {
        let range = TextRange::new(cursor.from(), cursor.to());
        snapshot.push((cursor.name().to_string(), range));
        if !cursor.next(true) {
            return snapshot;
        }
    }
}

#[test]
fn empty_names_do_not_imply_anonymous_nodes() {
    let empty = NodeType::new(0, "", NodeFlags::default());
    let anonymous = NodeType::new(1, "", NodeFlags::ANONYMOUS);

    assert!(!empty.is_anonymous());
    assert!(anonymous.is_anonymous());
}

#[test]
fn unencodable_tree_buffer_candidates_fall_back_without_changing_the_tree() {
    let overlapping = [(1, 2, 3), (2, 0, 4)];
    let unpacked = packing_tree(&overlapping, 0);
    let fallback = packing_tree(&overlapping, 4);

    assert_eq!(fallback.to_string(), unpacked.to_string());
    assert_eq!(syntax_snapshot(&fallback), syntax_snapshot(&unpacked));
    assert_eq!(fallback.positions(), unpacked.positions());
    assert!(
        fallback
            .children()
            .iter()
            .all(|child| matches!(child, TreeChild::Tree(_)))
    );

    let ordered = [(1, 0, 1), (2, 2, 4)];
    let packed = packing_tree(&ordered, 4);
    let ordered_unpacked = packing_tree(&ordered, 0);
    assert_eq!(syntax_snapshot(&packed), syntax_snapshot(&ordered_unpacked));
    assert!(matches!(packed.children(), [TreeChild::Buffer(_)]));
}

fn names(nodes: &[SyntaxNode]) -> String {
    nodes
        .iter()
        .map(|node| node.name().to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn children_between(
    node: &SyntaxNode,
    name: &str,
    before: Option<&str>,
    after: Option<&str>,
) -> Vec<SyntaxNode> {
    let mut children = node.children();
    if let Some(before) = before {
        loop {
            let Some(child) = children.next() else {
                return Vec::new();
            };
            if child.node_type().is_name(before) {
                break;
            }
        }
    }

    let mut result = Vec::new();
    for child in children {
        if after.is_some_and(|after| child.node_type().is_name(after)) {
            return result;
        }
        if child.node_type().is_name(name) {
            result.push(child);
        }
    }
    if after.is_none() { result } else { Vec::new() }
}

mod syntax_node {
    use super::*;

    #[test]
    fn can_resolve_at_the_top_level() {
        let mut node = simple().resolve(2.into(), -1);
        assert_eq!(node.from(), 1.into());
        assert_eq!(node.to(), 2.into());
        assert_eq!(node.name().as_ref(), "a");

        let parent = node.parent().unwrap();
        assert_eq!(parent.name().as_ref(), "T");
        assert!(parent.parent().is_none());

        node = simple().resolve(2.into(), 1);
        assert_eq!(node.from(), 2.into());
        assert_eq!(node.to(), 3.into());

        node = simple().resolve(2.into(), 0);
        assert_eq!(node.name().as_ref(), "T");
        assert_eq!(node.from(), 0.into());
        assert_eq!(node.to(), 23.into());
    }

    #[test]
    fn can_resolve_deeper() {
        let node = simple().resolve(10.into(), 1);
        assert_eq!(node.name().as_ref(), "c");
        assert_eq!(node.from(), 10.into());

        let bracket = node.parent().unwrap();
        assert_eq!(bracket.name().as_ref(), "Br");
        let paren = bracket.parent().unwrap();
        assert_eq!(paren.name().as_ref(), "Pa");
        let top = paren.parent().unwrap();
        assert_eq!(top.name().as_ref(), "T");
    }

    #[test]
    fn can_resolve_in_a_large_tree() {
        let mut node = recur().resolve(10.into(), 1);
        let mut depth = 1;
        while let Some(parent) = node.parent() {
            node = parent;
            depth += 1;
        }
        assert_eq!(depth, 8);
    }

    mod get_child {
        use super::*;

        #[test]
        fn can_get_children_by_group() {
            let tree = mk("aa(bb)[aabbcc]").top_node();
            assert_eq!(names(&tree.children_by_name("atom")), "a,a");
            assert_eq!(
                names(&tree.first_child().unwrap().children_by_name("atom")),
                ""
            );
            assert_eq!(
                names(&tree.last_child().unwrap().children_by_name("atom")),
                "a,a,b,b,c,c"
            );
        }

        #[test]
        fn can_get_single_children() {
            let tree = mk("abc()").top_node();
            assert!(tree.child_by_name("Br").is_none());
            assert_eq!(tree.child_by_name("Pa").unwrap().name().as_ref(), "Pa");
        }

        #[test]
        fn can_get_children_between_others() {
            let tree = mk("aa(bb)[aabbcc]").top_node();
            assert!(!children_between(&tree, "Pa", Some("atom"), Some("Br")).is_empty());
            assert!(children_between(&tree, "Pa", Some("atom"), Some("atom")).is_empty());

            let last = tree.last_child().unwrap();
            assert_eq!(
                names(&children_between(&last, "b", Some("a"), Some("c"))),
                "b,b"
            );
            assert_eq!(names(&children_between(&last, "a", None, Some("c"))), "a,a");
            assert_eq!(names(&children_between(&last, "c", Some("b"), None)), "c,c");
            assert_eq!(names(&children_between(&last, "b", Some("c"), None)), "");
        }
    }

    #[test]
    fn skips_anonymous_nodes() {
        let tree = anonymous_tree();
        assert_eq!(tree.to_string(), "T(a,b)");
        assert_eq!(tree.resolve(1.into(), 0).name().as_ref(), "T");
        assert_eq!(tree.top_node().last_child().unwrap().name().as_ref(), "b");
        assert_eq!(tree.top_node().first_child().unwrap().name().as_ref(), "a");
        assert_eq!(
            tree.top_node()
                .child_after(1.into())
                .unwrap()
                .name()
                .as_ref(),
            "b"
        );
    }

    #[test]
    fn allows_access_to_the_underlying_tree() {
        let tree = mk("aaa[bbbbb(bb)bbbbbbb]aaa");
        let mut node = tree.top_node().first_child().unwrap();
        while node.name().as_ref() != "Br" {
            node = node.next_sibling().unwrap();
        }
        let bracket = node.tree().unwrap();
        assert_eq!(bracket.node_type().name(), "Br");

        node = node.first_child().unwrap();
        while node.name().as_ref() != "Pa" {
            node = node.next_sibling().unwrap();
        }
        assert!(node.tree().is_none());
        assert_eq!(node.to_tree().to_string(), "Pa(b,b)");

        node = node.first_child().unwrap();
        assert_eq!(node.name().as_ref(), "b");
        assert_eq!(node.to_tree().to_string(), "b");
        assert!(node.to_tree().children().is_empty());
    }
}

mod tree_cursor {
    use super::*;

    #[test]
    fn iterates_over_all_nodes() {
        let mut count = BTreeMap::<String, usize>::new();
        let mut position = TextSize::from(0);
        let mut cursor = simple().cursor(IterMode::NONE);

        loop {
            assert!(cursor.from() >= position);
            position = cursor.from();
            *count.entry(cursor.name().to_string()).or_default() += 1;
            if !cursor.next(true) {
                break;
            }
        }

        for (name, expected) in SIMPLE_COUNTS {
            assert_eq!(count.get(name), Some(&expected));
        }
    }

    #[test]
    fn iterates_over_all_nodes_in_reverse() {
        let mut count = BTreeMap::<String, usize>::new();
        let mut position = TextSize::from(100);
        let mut cursor = simple().cursor(IterMode::NONE);

        loop {
            assert!(cursor.to() <= position);
            position = cursor.to();
            *count.entry(cursor.name().to_string()).or_default() += 1;
            if !cursor.previous(true) {
                break;
            }
        }

        for (name, expected) in SIMPLE_COUNTS {
            assert_eq!(count.get(name), Some(&expected));
        }
    }

    #[test]
    fn works_with_internal_iteration() {
        let tree = simple();
        let mut open_count = BTreeMap::<String, usize>::new();
        let mut close_count = BTreeMap::<String, usize>::new();
        tree.iterate(
            TextRange::new(0.into(), tree.len()),
            IterMode::NONE,
            |node| {
                *open_count.entry(node.name().to_string()).or_default() += 1;
                true
            },
            |node| {
                *close_count.entry(node.name().to_string()).or_default() += 1;
            },
        );

        for (name, expected) in SIMPLE_COUNTS {
            assert_eq!(open_count.get(name), Some(&expected));
            assert_eq!(close_count.get(name), Some(&expected));
        }
    }

    #[test]
    fn handles_iterating_out_of_bounds() {
        let hits = Cell::new(0);
        Tree::empty().iterate(
            TextRange::new(0.into(), 200.into()),
            IterMode::NONE,
            |_| {
                hits.set(hits.get() + 1);
                true
            },
            |_| {
                hits.set(hits.get() + 1);
            },
        );
        assert_eq!(hits.get(), 0);
    }

    #[test]
    fn internal_iteration_can_be_limited_to_a_range() {
        let tree = simple();
        let mut seen = Vec::new();
        tree.iterate(
            TextRange::new(3.into(), 14.into()),
            IterMode::NONE,
            |node| {
                seen.push(node.name().to_string());
                node.name().as_ref() != "Br"
            },
            |_| {},
        );
        assert_eq!(seen.join(","), "T,a,a,Pa,b,b,b,Br,Br");
    }

    #[test]
    fn can_leave_nodes() {
        let mut cursor = simple().cursor(IterMode::NONE);
        assert!(!cursor.parent());
        cursor.next(true);
        cursor.next(true);
        assert_eq!(cursor.from(), 1.into());
        assert!(cursor.parent());
        assert_eq!(cursor.from(), 0.into());
        for _ in 0..6 {
            cursor.next(true);
        }
        assert_eq!(cursor.from(), 5.into());
        assert!(cursor.parent());
        assert_eq!(cursor.from(), 4.into());
        assert!(cursor.parent());
        assert_eq!(cursor.from(), 0.into());
        assert!(!cursor.parent());
    }

    #[test]
    fn can_move_to_a_given_position() {
        let tree = recur();
        let start = TextSize::from(u32::from(tree.len()) >> 1);
        let mut cursor = tree.cursor_at(start, 1, IterMode::NONE);
        loop {
            assert!(cursor.from() >= start);
            if !cursor.next(true) {
                break;
            }
        }
    }

    #[test]
    fn can_move_into_a_parent_node() {
        let mut cursor = simple().cursor_at(10.into(), 0, IterMode::NONE);
        cursor.move_to(2.into(), 0);
        assert_eq!(cursor.name().as_ref(), "T");
    }

    #[test]
    fn can_move_to_a_specific_sibling() {
        let mut cursor = simple().cursor(IterMode::NONE);
        assert!(cursor.child_after(2.into()));
        assert_eq!(cursor.to(), 3.into());
        cursor.parent();
        assert!(cursor.child_before(5.into()));
        assert_eq!(cursor.from(), 4.into());
        assert!(cursor.child_after(11.into()));
        assert_eq!(cursor.from(), 8.into());
        assert!(cursor.child_before(10.into()));
        assert_eq!(cursor.from(), 9.into());
        assert!(!simple().cursor(IterMode::NONE).child_before(0.into()));
        assert!(!simple().cursor(IterMode::NONE).child_after(100.into()));
    }

    #[test]
    fn can_produce_nodes() {
        let cursor = simple().cursor_at(8.into(), 1, IterMode::NONE);
        let node = cursor.node();
        assert_eq!(node.name().as_ref(), "Br");
        assert_eq!(node.from(), 8.into());

        let paren = node.parent().unwrap();
        assert_eq!(paren.name().as_ref(), "Pa");
        assert_eq!(paren.from(), 4.into());
        let top = paren.parent().unwrap();
        assert_eq!(top.name().as_ref(), "T");
        assert_eq!(top.from(), 0.into());
        assert!(top.parent().is_none());
    }

    #[test]
    fn can_produce_node_from_cursors_created_from_nodes() {
        let node = simple()
            .top_node()
            .last_child()
            .unwrap()
            .child_after(8.into())
            .unwrap()
            .child_after(10.into())
            .unwrap();
        let mut cursor = node.cursor(IterMode::NONE);
        assert_eq!(cursor.name().as_ref(), "c");
        assert_eq!(cursor.from(), 10.into());
        assert!(cursor.parent());

        let node = cursor.node();
        assert_eq!(node.name().as_ref(), "Br");
        assert_eq!(node.from(), 8.into());
        let paren = node.parent().unwrap();
        assert_eq!(paren.name().as_ref(), "Pa");
        assert_eq!(paren.from(), 4.into());
        let top = paren.parent().unwrap();
        assert_eq!(top.name().as_ref(), "T");
        assert!(top.parent().is_none());
    }

    #[test]
    fn skips_anonymous_nodes() {
        let mut cursor = anonymous_tree().cursor(IterMode::NONE);
        cursor.move_to(1.into(), 0);
        assert_eq!(cursor.name().as_ref(), "T");
        assert!(cursor.first_child());
        assert_eq!(cursor.name().as_ref(), "a");
        assert!(cursor.next_sibling());
        assert_eq!(cursor.name().as_ref(), "b");
        assert!(!cursor.next(true));
    }

    #[test]
    fn stops_at_anonymous_nodes_when_configured_as_full() {
        let mut cursor = anonymous_tree().cursor(IterMode::INCLUDE_ANONYMOUS);
        cursor.move_to(1.into(), 0);
        assert!(cursor.node_type().is(&NodeType::none()));
        assert_eq!(cursor.node().tree().unwrap().len(), 2.into());
        assert!(cursor.first_child());
        assert_eq!(cursor.name().as_ref(), "a");
        assert!(cursor.parent());
        assert!(cursor.node_type().is(&NodeType::none()));
    }
}

mod match_context {
    use super::*;

    #[test]
    fn can_match_on_nodes() {
        assert!(
            simple()
                .resolve(10.into(), 1)
                .matches_context(&["T", "Pa", "Br"])
        );
    }

    #[test]
    fn can_match_wildcards() {
        assert!(
            simple()
                .resolve(10.into(), 1)
                .matches_context(&["T", "", "Br"])
        );
    }

    #[test]
    fn can_mismatch_on_nodes() {
        assert!(!simple().resolve(10.into(), 1).matches_context(&["Q", "Br"]));
    }

    #[test]
    fn can_match_on_cursor() {
        let mut cursor = simple().cursor(IterMode::NONE);
        for _ in 0..3 {
            assert!(cursor.enter(15.into(), -1));
        }
        assert!(cursor.matches_context(&["T", "Pa", "Br"]));
    }
}
