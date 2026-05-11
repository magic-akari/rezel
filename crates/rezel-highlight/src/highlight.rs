use std::sync::Arc;

use rezel_common::{IterMode, NodeType, TextRange, TextSize, Tree, TreeCursor, mounted_prop};

use crate::TagSet;
use crate::style::{Highlighter, StyleMode, get_style_tags_cursor, highlight_tags};

/// One non-empty syntactic tag range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HighlightSpan {
    /// Half-open UTF-8 byte range.
    pub range: TextRange,
    /// Ordered abstract tags active in this range.
    pub tags: TagSet,
}

/// One non-empty class range produced by a downstream highlighter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StyledSpan {
    /// Half-open UTF-8 byte range.
    pub range: TextRange,
    /// Space-separated downstream classes.
    pub classes: Arc<str>,
}

/// Project a syntax tree directly into abstract syntactic tag spans.
///
/// This is the Rezel-facing equivalent of running `highlightTree` with an
/// identity highlighter. Mounted replacement and overlay trees are traversed.
pub fn highlight_spans(
    tree: &Tree,
    range: Option<TextRange>,
    mut put_span: impl FnMut(HighlightSpan),
) {
    highlight_scoped(tree, range, false, |span| {
        if !span.tags.is_empty() {
            put_span(HighlightSpan {
                range: span.range,
                tags: span.tags,
            });
        }
    });
}

/// Highlight a syntax tree with one or more downstream highlighters.
///
/// The callback receives ordered, non-overlapping ranges with non-empty class
/// strings.
pub fn highlight_tree(
    tree: &Tree,
    highlighters: &[&dyn Highlighter],
    range: Option<TextRange>,
    mut put_style: impl FnMut(StyledSpan),
) {
    let range = normalize_range(tree, range);
    if range.is_empty() {
        return;
    }
    let mut builder = StyleSpanBuilder {
        at: range.start(),
        classes: String::new(),
        put_style: &mut put_style,
    };
    highlight_scoped(tree, Some(range), true, |span| {
        let classes = highlight_tags(highlighters, &span.top, &span.tags).unwrap_or_default();
        builder.start_span(span.range.start(), classes);
    });
    builder.finish(range.end());
}

/// Highlight source text while emitting styled and unstyled text fragments
/// and line breaks separately.
pub fn highlight_code(
    code: &str,
    tree: &Tree,
    highlighters: &[&dyn Highlighter],
    range: Option<TextRange>,
    mut put_text: impl FnMut(&str, &str),
    mut put_break: impl FnMut(),
) {
    let range = normalize_range(tree, range);
    let mut position = range.start();
    highlight_tree(tree, highlighters, Some(range), |span| {
        write_to(
            code,
            &mut position,
            span.range.start(),
            "",
            &mut put_text,
            &mut put_break,
        );
        write_to(
            code,
            &mut position,
            span.range.end(),
            &span.classes,
            &mut put_text,
            &mut put_break,
        );
    });
    write_to(
        code,
        &mut position,
        range.end(),
        "",
        &mut put_text,
        &mut put_break,
    );
}

fn write_to(
    code: &str,
    position: &mut TextSize,
    target: TextSize,
    classes: &str,
    put_text: &mut impl FnMut(&str, &str),
    put_break: &mut impl FnMut(),
) {
    if target <= *position {
        return;
    }
    let text = &code[usize::from(*position)..usize::from(target)];
    let mut lines = text.split('\n').peekable();
    while let Some(line) = lines.next() {
        if !line.is_empty() {
            put_text(line, classes);
        }
        if lines.peek().is_some() {
            put_break();
        }
    }
    *position = target;
}

#[derive(Clone)]
struct ScopedSpan {
    range: TextRange,
    tags: TagSet,
    top: NodeType,
}

fn highlight_scoped(
    tree: &Tree,
    range: Option<TextRange>,
    distinguish_top: bool,
    mut put_span: impl FnMut(ScopedSpan),
) {
    let range = normalize_range(tree, range);
    if range.is_empty() {
        return;
    }
    let mut cursor = tree.cursor(IterMode::NONE);
    let top = cursor.node_type().clone();
    let mut builder = TagSpanBuilder {
        at: range.start(),
        tags: TagSet::empty(),
        top: top.clone(),
        distinguish_top,
        put_span: &mut put_span,
    };
    visit_node(&mut cursor, range, &TagSet::empty(), &top, &mut builder);
    builder.finish(range.end());
}

fn normalize_range(tree: &Tree, range: Option<TextRange>) -> TextRange {
    let range = range.unwrap_or_else(|| TextRange::new(TextSize::from(0), tree.len()));
    let from = range.start().min(tree.len());
    let to = range.end().min(tree.len()).max(from);
    TextRange::new(from, to)
}

fn visit_node<Emit>(
    cursor: &mut TreeCursor,
    range: TextRange,
    inherited: &TagSet,
    active_top: &NodeType,
    builder: &mut TagSpanBuilder<'_, Emit>,
) where
    Emit: FnMut(ScopedSpan),
{
    let start = cursor.from();
    let end = cursor.to();
    if start >= range.end() || end <= range.start() {
        return;
    }

    let node_type = cursor.node_type().clone();
    let top = if node_type.is_top() {
        &node_type
    } else {
        active_top
    };
    let rule = get_style_tags_cursor(cursor);
    let own_tags = rule
        .as_ref()
        .map_or_else(TagSet::empty, |rule| rule.tags().clone());
    let current = inherited.append(&own_tags);
    builder.start_span(range.start().max(start), &current, top);
    if rule.as_ref().is_some_and(crate::style::StyleMatch::opaque) {
        return;
    }

    let child_inherited = if rule
        .as_ref()
        .is_some_and(|rule| rule.mode() == StyleMode::Inherit)
    {
        &current
    } else {
        inherited
    };
    let mounted = cursor.tree().and_then(|tree| tree.prop(mounted_prop()));
    if let Some(mounted) = &mounted
        && let Some(overlay) = &mounted.overlay
        && !overlay.is_empty()
    {
        let style = TraversalStyle {
            inherited: child_inherited,
            current: &current,
            top,
        };
        visit_overlay(cursor, range, overlay, style, builder);
        return;
    }
    let empty = TagSet::empty();
    let child_inherited = if mounted.is_some_and(|mounted| mounted.overlay.is_none()) {
        &empty
    } else {
        child_inherited
    };
    visit_children(cursor, range, child_inherited, &current, top, builder);
}

#[derive(Clone, Copy)]
struct TraversalStyle<'a> {
    inherited: &'a TagSet,
    current: &'a TagSet,
    top: &'a NodeType,
}

fn visit_overlay<Emit>(
    cursor: &mut TreeCursor,
    range: TextRange,
    overlay: &[TextRange],
    style: TraversalStyle<'_>,
    builder: &mut TagSpanBuilder<'_, Emit>,
) where
    Emit: FnMut(ScopedSpan),
{
    let host_start = cursor.from();
    let host_end = cursor.to();
    let mut inner = cursor.clone();
    let entered = inner.enter(host_start + overlay[0].start(), 1);
    assert!(entered, "mounted overlay root must be enterable");
    let inner_top = inner.node_type().clone();
    let has_children = cursor.first_child();
    let mut position = host_start;
    for index in 0..=overlay.len() {
        let next = overlay.get(index);
        let next_position = next.map_or(host_end, |range| host_start + range.start());
        let range_start = range.start().max(position);
        let range_end = range.end().min(next_position);
        if range_start < range_end {
            let outer_range = TextRange::new(range_start, range_end);
            builder.start_span(range_start, style.current, style.top);
            if has_children {
                visit_overlay_children(cursor, outer_range, next_position, style, builder);
            }
            builder.start_span(range_end, style.current, style.top);
        }

        let Some(next) = next else {
            break;
        };
        if next_position > range.end() {
            break;
        }
        position = host_start + next.end();
        if position <= range.start() {
            continue;
        }
        let inner_start = range.start().max(host_start + next.start());
        let inner_end = range.end().min(position);
        if inner_start < inner_end {
            let mut inner_cursor = inner.clone();
            let inner_range = TextRange::new(inner_start, inner_end);
            visit_node(
                &mut inner_cursor,
                inner_range,
                &TagSet::empty(),
                &inner_top,
                builder,
            );
            builder.start_span(inner_end, style.current, style.top);
        }
    }
    if has_children {
        cursor.parent();
    }
}

fn visit_children<Emit>(
    cursor: &mut TreeCursor,
    range: TextRange,
    inherited: &TagSet,
    current: &TagSet,
    top: &NodeType,
    builder: &mut TagSpanBuilder<'_, Emit>,
) where
    Emit: FnMut(ScopedSpan),
{
    if !cursor.first_child() {
        return;
    }
    let style = TraversalStyle {
        inherited,
        current,
        top,
    };
    visit_child_range(cursor, range, style, builder);
    cursor.parent();
}

fn visit_child_range<Emit>(
    cursor: &mut TreeCursor,
    range: TextRange,
    style: TraversalStyle<'_>,
    builder: &mut TagSpanBuilder<'_, Emit>,
) where
    Emit: FnMut(ScopedSpan),
{
    loop {
        let child_start = cursor.from();
        let child_end = cursor.to();
        let before_range = child_end <= range.start();
        let after_range = child_start >= range.end();
        if after_range {
            break;
        }
        if !before_range {
            visit_node(cursor, range, style.inherited, style.top, builder);
            builder.start_span(range.end().min(child_end), style.current, style.top);
        }
        if !cursor.next_sibling() {
            break;
        }
    }
}

fn visit_overlay_children<Emit>(
    cursor: &mut TreeCursor,
    range: TextRange,
    stop_at: TextSize,
    style: TraversalStyle<'_>,
    builder: &mut TagSpanBuilder<'_, Emit>,
) where
    Emit: FnMut(ScopedSpan),
{
    while cursor.from() < range.end() {
        visit_node(cursor, range, style.inherited, style.top, builder);
        builder.start_span(range.end().min(cursor.to()), style.current, style.top);
        if cursor.to() >= stop_at || !cursor.next_sibling() {
            break;
        }
    }
}

struct TagSpanBuilder<'emit, Emit> {
    at: TextSize,
    tags: TagSet,
    top: NodeType,
    distinguish_top: bool,
    put_span: &'emit mut Emit,
}

impl<Emit> TagSpanBuilder<'_, Emit>
where
    Emit: FnMut(ScopedSpan),
{
    fn start_span(&mut self, at: TextSize, tags: &TagSet, top: &NodeType) {
        let same_top = !self.distinguish_top || self.top == *top;
        if self.tags == *tags && same_top {
            return;
        }
        self.flush(at);
        if at > self.at {
            self.at = at;
        }
        self.tags = tags.clone();
        self.top = top.clone();
    }

    fn flush(&mut self, to: TextSize) {
        if to > self.at {
            (self.put_span)(ScopedSpan {
                range: TextRange::new(self.at, to),
                tags: self.tags.clone(),
                top: self.top.clone(),
            });
            self.at = to;
        }
    }

    fn finish(&mut self, to: TextSize) {
        self.flush(to);
    }
}

struct StyleSpanBuilder<'emit, Emit> {
    at: TextSize,
    classes: String,
    put_style: &'emit mut Emit,
}

impl<Emit> StyleSpanBuilder<'_, Emit>
where
    Emit: FnMut(StyledSpan),
{
    fn start_span(&mut self, at: TextSize, classes: String) {
        if self.classes == classes {
            return;
        }
        self.flush(at);
        if at > self.at {
            self.at = at;
        }
        self.classes = classes;
    }

    fn flush(&mut self, to: TextSize) {
        if to > self.at && !self.classes.is_empty() {
            (self.put_style)(StyledSpan {
                range: TextRange::new(self.at, to),
                classes: Arc::from(self.classes.as_str()),
            });
        }
        if to > self.at {
            self.at = to;
        }
    }

    fn finish(&mut self, to: TextSize) {
        self.flush(to);
    }
}
