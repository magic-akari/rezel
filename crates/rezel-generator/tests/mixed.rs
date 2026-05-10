//! Mixed-language parser conformance cases.

use std::sync::Arc;

use rezel_common::{
    NestedParse, Overlay, OverlayMatch, Parser, SyntaxNode, TextRange, TextSize, mounted_prop,
    parse_mixed,
};
use rezel_lr::LRParser;

#[path = "generated/test_parse/basic_inner.rs"]
#[rustfmt::skip]
mod basic_inner;
#[path = "generated/test_parse/basic_outer.rs"]
#[rustfmt::skip]
mod basic_outer;
#[path = "generated/test_parse/blob.rs"]
#[rustfmt::skip]
mod blob;
#[path = "generated/test_parse/empty.rs"]
#[rustfmt::skip]
mod empty;
#[path = "generated/test_parse/expression.rs"]
#[rustfmt::skip]
mod expression;
#[path = "generated/test_parse/parens.rs"]
#[rustfmt::skip]
mod parens;
#[path = "generated/test_parse/script_inner.rs"]
#[rustfmt::skip]
mod script_inner;
#[path = "generated/test_parse/tag_outer.rs"]
#[rustfmt::skip]
mod tag_outer;
#[path = "generated/test_parse/template.rs"]
#[rustfmt::skip]
mod template;

fn size(value: u32) -> TextSize {
    TextSize::from(value)
}

fn parser(language: &'static rezel_lr::Language) -> LRParser {
    LRParser::from_language(language)
}

fn replacement(parser: Arc<dyn Parser>) -> NestedParse {
    NestedParse {
        parser,
        overlay: Overlay::Replace,
        bracketed: false,
    }
}

fn overlay(parser: Arc<dyn Parser>, selection: Overlay) -> NestedParse {
    NestedParse {
        parser,
        overlay: selection,
        bracketed: false,
    }
}

fn ancestry(node: SyntaxNode) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = Some(node);
    while let Some(node) = current {
        result.push(node.name().to_string());
        current = node.parent();
    }
    result
}

#[test]
fn supports_basic_nesting() {
    let inner: Arc<dyn Parser> = Arc::new(parser(&basic_inner::LANGUAGE));
    let mixed =
        parser(&basic_outer::LANGUAGE).with_wrapper(parse_mixed(Arc::new(move |node, _| {
            (node.name().as_ref() == "NestContent").then(|| replacement(Arc::clone(&inner)))
        })));

    assert_eq!(
        mixed.parse("![[((.).)]][[.]]").unwrap().to_string(),
        "O(Bang,Start,I(B(Open,B(Open,Dot,Close),Dot,Close)),End,Start,I(Dot),End)"
    );
    assert_eq!(
        mixed.parse("[[/]]").unwrap().to_string(),
        "O(Start,I(⚠),End)"
    );

    let tree = mixed.parse("[[(.)]]").unwrap();
    let inner = tree.top_node().child_after(size(2)).unwrap();
    assert_eq!(inner.name().as_ref(), "I");
    assert_eq!(inner.range(), TextRange::new(size(2), size(5)));
    assert_eq!(
        inner.first_child().unwrap().range(),
        TextRange::new(size(2), size(5))
    );
}

#[test]
fn supports_conditional_nesting() {
    let inner: Arc<dyn Parser> = Arc::new(parser(&script_inner::LANGUAGE));
    let mixed =
        parser(&tag_outer::LANGUAGE).with_wrapper(parse_mixed(Arc::new(move |node, input| {
            if node.name().as_ref() != "Content" {
                return None;
            }
            let open = node.parent()?.first_child()?;
            (input.read(open.range()).as_ref() == "<script>")
                .then(|| replacement(Arc::clone(&inner)))
        })));

    assert_eq!(
        mixed.parse("<foo>bar</foo>").unwrap().to_string(),
        "T(Tag(Open,Content,Close))"
    );
    assert_eq!(
        mixed.parse("<script>hello</script>").unwrap().to_string(),
        "T(Tag(Open,Script,Close))"
    );
}

#[test]
fn creates_predicate_and_explicit_overlays() {
    let blob: Arc<dyn Parser> = Arc::new(parser(&blob::LANGUAGE).with_buffer_length(size(10)));
    let content_overlay = Overlay::Predicate(Arc::new(|node| {
        if node.name().as_ref() == "Content" {
            OverlayMatch::Node
        } else {
            OverlayMatch::None
        }
    }));
    let predicate_blob = Arc::clone(&blob);
    let mixed = parser(&template::LANGUAGE).with_wrapper(parse_mixed(Arc::new(move |node, _| {
        (node.name().as_ref() == "Doc")
            .then(|| overlay(Arc::clone(&predicate_blob), content_overlay.clone()))
    })));

    let tree = mixed.parse("foo{{bar}}baz{{bug}}").unwrap();
    assert_eq!(tree.to_string(), "Doc(Content,Dir(Word),Content,Dir(Word))");
    let first = tree.resolve_inner(size(1), 0);
    assert_eq!(first.name().as_ref(), "Blob");
    assert_eq!(first.range(), TextRange::new(size(0), size(13)));
    assert_eq!(first.parent().unwrap().name().as_ref(), "Doc");
    assert_eq!(tree.resolve_inner(size(10), 1).name().as_ref(), "Blob");

    let explicit_blob = Arc::clone(&blob);
    let mixed = parser(&template::LANGUAGE).with_wrapper(parse_mixed(Arc::new(move |node, _| {
        (node.name().as_ref() == "Doc").then(|| {
            overlay(
                Arc::clone(&explicit_blob),
                Overlay::Ranges(vec![TextRange::new(size(5), size(7))].into()),
            )
        })
    })));
    let tree = mixed.parse("{{a}}bc{{d}}").unwrap();
    let selected = tree.resolve_inner(size(6), 0);
    assert_eq!(selected.name().as_ref(), "Blob");
    assert_eq!(selected.range(), TextRange::new(size(5), size(7)));
}

#[test]
fn adds_a_mount_for_empty_nodes() {
    let inner: Arc<dyn Parser> = Arc::new(parser(&empty::LANGUAGE));
    let mixed = parser(&template::LANGUAGE).with_wrapper(parse_mixed(Arc::new(move |node, _| {
        (node.name().as_ref() == "BlockContent").then(|| replacement(Arc::clone(&inner)))
    })));

    assert_eq!(
        mixed.parse("a{%%}b{% %}").unwrap().to_string(),
        "Doc(Content,Block(E),Content,Block(E))"
    );
}

#[test]
fn exposes_the_layers_needed_for_resolve_stack() {
    let inner: Arc<dyn Parser> = Arc::new(parser(&parens::LANGUAGE));
    let content_overlay = Overlay::Predicate(Arc::new(|node| {
        if node.name().as_ref() == "Content" {
            OverlayMatch::Node
        } else {
            OverlayMatch::None
        }
    }));
    let mixed = parser(&template::LANGUAGE).with_wrapper(parse_mixed(Arc::new(move |node, _| {
        node.node_type()
            .is_top()
            .then(|| overlay(Arc::clone(&inner), content_overlay.clone()))
    })));
    let source = "(hey{%okay(one)two%}three)!";

    for buffer_length in [1024, 2] {
        let tree = mixed
            .clone()
            .with_buffer_length(size(buffer_length))
            .parse(source)
            .unwrap();

        assert_eq!(
            ancestry(tree.resolve_inner(size(12), 0)),
            ["Text", "Group", "Group", "T", "Doc"]
        );
        assert_eq!(
            ancestry(tree.resolve(size(12), 0)),
            ["Content", "BlockContent", "Block", "Doc"]
        );
        assert_eq!(
            ancestry(tree.resolve_inner(size(2), 0)),
            ["Text", "Group", "T", "Doc"]
        );
        assert_eq!(ancestry(tree.resolve(size(2), 0)), ["Content", "Doc"]);

        let root = tree.top_node();
        let mounted = root.prop(mounted_prop()).unwrap();
        assert_eq!(
            ancestry(mounted.tree.resolve(size(5), 0)),
            ["Text", "Group", "T"]
        );
        assert_eq!(ancestry(tree.resolve(size(5), 0)), ["Block", "Doc"]);
    }
}

fn string_ranges_outside_nested_arrays(root: &SyntaxNode) -> Arc<[TextRange]> {
    fn scan(node: &SyntaxNode, ranges: &mut Vec<TextRange>) {
        if node.name().as_ref() == "String" {
            ranges.push(TextRange::new(node.from() + size(1), node.to() - size(1)));
            return;
        }
        for child in node.children() {
            if child.name().as_ref() != "Array" {
                scan(&child, ranges);
            }
        }
    }

    let mut ranges = Vec::new();
    scan(root, &mut ranges);
    ranges.into()
}

fn assert_nested_overlay(parser: &LRParser) {
    let tree = parser.parse("['x' 100 (['xxx' 20 ('xx')] 'xxx')]").unwrap();
    let outer = tree.resolve_inner(size(2), 1);
    assert_eq!(outer.name().as_ref(), "Blob");
    assert_eq!(outer.range(), TextRange::new(size(2), size(32)));
    let inner = tree.resolve_inner(size(12), 1);
    assert_eq!(inner.name().as_ref(), "Blob");
    assert_eq!(inner.range(), TextRange::new(size(12), size(24)));
}

#[test]
fn supports_nested_explicit_overlays() {
    let blob: Arc<dyn Parser> = Arc::new(parser(&blob::LANGUAGE).with_buffer_length(size(10)));
    let mixed = parser(&expression::LANGUAGE)
        .with_buffer_length(size(2))
        .with_wrapper(parse_mixed(Arc::new(move |node, _| {
            (node.name().as_ref() == "Array").then(|| {
                overlay(
                    Arc::clone(&blob),
                    Overlay::Ranges(string_ranges_outside_nested_arrays(node)),
                )
            })
        })));

    assert_nested_overlay(&mixed);
}

#[test]
fn supports_nested_predicate_overlays() {
    let blob: Arc<dyn Parser> = Arc::new(parser(&blob::LANGUAGE).with_buffer_length(size(10)));
    let string_overlay = Overlay::Predicate(Arc::new(|node| {
        if node.name().as_ref() == "String" {
            OverlayMatch::Range(TextRange::new(node.from() + size(1), node.to() - size(1)))
        } else {
            OverlayMatch::None
        }
    }));
    let mixed = parser(&expression::LANGUAGE)
        .with_buffer_length(size(2))
        .with_wrapper(parse_mixed(Arc::new(move |node, _| {
            (node.name().as_ref() == "Array")
                .then(|| overlay(Arc::clone(&blob), string_overlay.clone()))
        })));

    assert_nested_overlay(&mixed);
}
