use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::sync::{Arc, OnceLock};

use rezel_common::{NodeProp, NodePropConfig, NodePropSource, NodeType, SyntaxNode, TreeCursor};

use crate::{TagSet, tags};

/// How a matching selector affects descendant highlighting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StyleMode {
    /// Descendants use their own innermost matching style.
    Normal,
    /// This style is inherited by all descendants.
    Inherit,
    /// This node is styled without traversing descendants.
    Opaque,
}

#[derive(Clone, Debug)]
struct Rule {
    tags: TagSet,
    mode: StyleMode,
    context: Option<Arc<[Arc<str>]>>,
}

impl Rule {
    fn depth(&self) -> usize {
        self.context.as_ref().map_or(0, |context| context.len())
    }

    fn matches_node(&self, node: &SyntaxNode) -> bool {
        let Some(context) = &self.context else {
            return true;
        };
        node.matches_context(context.as_ref())
    }

    fn matches_cursor(&self, cursor: &TreeCursor) -> bool {
        let Some(context) = &self.context else {
            return true;
        };
        cursor.matches_context(context.as_ref())
    }
}

#[derive(Clone, Debug, Default)]
struct RuleSet(Arc<[Rule]>);

/// One selector syntax error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectorError {
    path: String,
}

impl SelectorError {
    fn new(path: &str) -> Self {
        Self {
            path: path.to_owned(),
        }
    }

    /// Invalid selector path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
}

impl fmt::Display for SelectorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid highlight selector path: {}", self.path)
    }
}

impl Error for SelectorError {}

/// Tags and traversal mode selected for one syntax node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StyleMatch {
    tags: TagSet,
    mode: StyleMode,
}

impl StyleMatch {
    /// Selected tags.
    #[must_use]
    pub fn tags(&self) -> &TagSet {
        &self.tags
    }

    /// Whether this rule prevents descent into child nodes.
    #[must_use]
    pub const fn opaque(&self) -> bool {
        matches!(self.mode, StyleMode::Opaque)
    }

    /// Whether this rule is inherited by child nodes.
    #[must_use]
    pub const fn inherit(&self) -> bool {
        matches!(self.mode, StyleMode::Inherit)
    }

    pub(crate) const fn mode(&self) -> StyleMode {
        self.mode
    }
}

/// Compile `styleTags`-compatible selector entries into a node-property
/// source.
///
/// Selector entries preserve input order. Each value may be one tag or an
/// ordered [`TagSet`].
///
/// # Errors
///
/// Returns an error when a selector is not valid Lezer selector syntax.
pub fn style_tags<I, K, V>(spec: I) -> Result<NodePropSource, SelectorError>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: Into<TagSet>,
{
    let mut by_name = BTreeMap::<String, Vec<Rule>>::new();
    for (selector, tags) in spec {
        let selector = selector.as_ref();
        let tags = tags.into();
        for path in selector.split(' ').filter(|path| !path.is_empty()) {
            let parsed = parse_selector(path)?;
            let last = parsed.pieces.len() - 1;
            let target = parsed.pieces[last].clone();
            if target.is_empty() {
                return Err(SelectorError::new(path));
            }
            let context = (last > 0).then(|| Arc::from(parsed.pieces[..last].to_vec()));
            let rule = Rule {
                tags: tags.clone(),
                mode: parsed.mode,
                context,
            };
            insert_rule(by_name.entry(target.to_string()).or_default(), rule);
        }
    }
    let by_name = by_name
        .into_iter()
        .map(|(name, rules)| (name, RuleSet(rules.into())))
        .collect();
    Ok(rule_node_prop().add_map(by_name))
}

/// Return the matching style rule for one syntax node.
#[must_use]
pub fn get_style_tags(node: &SyntaxNode) -> Option<StyleMatch> {
    let node_type = node.node_type();
    let rules = node_type.prop(rule_node_prop())?;
    rules
        .0
        .iter()
        .find(|rule| rule.matches_node(node))
        .map(style_match)
}

pub(crate) fn get_style_tags_cursor(cursor: &TreeCursor) -> Option<StyleMatch> {
    let rules = cursor.node_type().prop(rule_node_prop())?;
    rules
        .0
        .iter()
        .find(|rule| rule.matches_cursor(cursor))
        .map(style_match)
}

fn style_match(rule: &Rule) -> StyleMatch {
    StyleMatch {
        tags: rule.tags.clone(),
        mode: rule.mode,
    }
}

fn rule_node_prop() -> NodeProp<RuleSet> {
    static PROPERTY: OnceLock<NodeProp<RuleSet>> = OnceLock::new();
    *PROPERTY.get_or_init(|| {
        NodeProp::new(NodePropConfig {
            combine: Some(combine_rule_sets),
            ..NodePropConfig::default()
        })
    })
}

fn combine_rule_sets(left: &RuleSet, right: &RuleSet) -> RuleSet {
    let mut left_index = 0;
    let mut right_index = 0;
    let mut combined: Vec<Rule> = Vec::with_capacity(left.0.len() + right.0.len());
    while left_index < left.0.len() || right_index < right.0.len() {
        let take_right = left_index == left.0.len()
            || (right_index < right.0.len()
                && left.0[left_index].depth() >= right.0[right_index].depth());
        let rule = if take_right {
            let rule = right.0[right_index].clone();
            right_index += 1;
            rule
        } else {
            let rule = left.0[left_index].clone();
            left_index += 1;
            rule
        };
        let duplicate_default = combined.last().is_some_and(|previous| {
            previous.mode == rule.mode && previous.context.is_none() && rule.context.is_none()
        });
        if !duplicate_default {
            combined.push(rule);
        }
    }
    RuleSet(combined.into())
}

fn insert_rule(rules: &mut Vec<Rule>, rule: Rule) {
    let index = rules.partition_point(|existing| existing.depth() >= rule.depth());
    rules.insert(index, rule);
}

struct ParsedSelector {
    pieces: Vec<Arc<str>>,
    mode: StyleMode,
}

fn parse_selector(path: &str) -> Result<ParsedSelector, SelectorError> {
    let mut pieces = Vec::new();
    let mut mode = StyleMode::Normal;
    let mut position = 0;
    while position < path.len() {
        if position > 0 && path[position..].starts_with("...") && position + 3 == path.len() {
            mode = StyleMode::Inherit;
            position = path.len();
            break;
        }

        let (piece, next, quoted) = parse_piece(path, position)?;
        let piece = if !quoted && piece.as_ref() == "*" {
            Arc::from("")
        } else {
            piece
        };
        pieces.push(piece);
        position = next;
        if position == path.len() {
            break;
        }

        let separator = path.as_bytes()[position];
        position += 1;
        if separator == b'!' && position == path.len() {
            mode = StyleMode::Opaque;
            break;
        }
        if separator != b'/' {
            return Err(SelectorError::new(path));
        }
        if position == path.len() {
            return Err(SelectorError::new(path));
        }
    }
    if pieces.is_empty() || position != path.len() {
        return Err(SelectorError::new(path));
    }
    Ok(ParsedSelector { pieces, mode })
}

fn parse_piece(path: &str, start: usize) -> Result<(Arc<str>, usize, bool), SelectorError> {
    if path.as_bytes().get(start) == Some(&b'"') {
        let mut escaped = false;
        for (relative, byte) in path.as_bytes()[start + 1..].iter().enumerate() {
            if escaped {
                escaped = false;
                continue;
            }
            if *byte == b'\\' {
                escaped = true;
                continue;
            }
            if *byte == b'"' {
                let end = start + relative + 2;
                let decoded = serde_json::from_str::<String>(&path[start..end])
                    .map_err(|_| SelectorError::new(path))?;
                return Ok((Arc::from(decoded), end, true));
            }
        }
        return Err(SelectorError::new(path));
    }

    let length = path.as_bytes()[start..]
        .iter()
        .position(|byte| matches!(byte, b'/' | b'!'))
        .unwrap_or(path.len() - start);
    if length == 0 {
        return Err(SelectorError::new(path));
    }
    let end = start + length;
    Ok((Arc::from(&path[start..end]), end, false))
}

/// Mapping from abstract tags to a downstream class string.
pub trait Highlighter: Send + Sync {
    /// Resolve classes for an ordered tag set.
    fn style(&self, tags: &TagSet) -> Option<String>;

    /// Whether this highlighter applies to the given grammar top node.
    fn scope(&self, _node: &NodeType) -> bool {
        true
    }
}

/// One tag-to-class mapping entry.
#[derive(Clone, Debug)]
pub struct TagStyle {
    tags: TagSet,
    class: Arc<str>,
}

impl TagStyle {
    /// Construct one mapping.
    #[must_use]
    pub fn new(tags: impl Into<TagSet>, class: impl Into<Arc<str>>) -> Self {
        Self {
            tags: tags.into(),
            class: class.into(),
        }
    }
}

/// Shared scope predicate used by a tag highlighter.
pub type ScopePredicate = Arc<dyn Fn(&NodeType) -> bool + Send + Sync>;

/// Optional scope and universal class for [`tag_highlighter`].
#[derive(Clone, Default)]
pub struct TagHighlighterOptions {
    /// Limit this highlighter to selected grammar top node types.
    pub scope: Option<ScopePredicate>,
    /// Class included for every token in scope.
    pub all: Option<Arc<str>>,
}

impl fmt::Debug for TagHighlighterOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TagHighlighterOptions")
            .field("scope", &self.scope.as_ref().map(|_| "<predicate>"))
            .field("all", &self.all)
            .finish()
    }
}

/// Concrete highlighter produced by [`tag_highlighter`].
#[derive(Clone)]
pub struct TagHighlighter {
    classes: BTreeMap<u32, Arc<str>>,
    options: TagHighlighterOptions,
}

impl fmt::Debug for TagHighlighter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TagHighlighter")
            .field("classes", &self.classes)
            .field("options", &self.options)
            .finish()
    }
}

impl Highlighter for TagHighlighter {
    fn style(&self, tags: &TagSet) -> Option<String> {
        let mut result = self
            .options
            .all
            .as_deref()
            .filter(|class| !class.is_empty())
            .map(str::to_owned);
        for tag in tags {
            for candidate in tag.set().as_slice() {
                let Some(class) = self.classes.get(&candidate.id()) else {
                    continue;
                };
                if class.is_empty() {
                    continue;
                }
                if let Some(result) = &mut result {
                    result.push(' ');
                    result.push_str(class);
                } else {
                    result = Some(class.to_string());
                }
                break;
            }
        }
        result
    }

    fn scope(&self, node: &NodeType) -> bool {
        self.options.scope.as_ref().is_none_or(|scope| scope(node))
    }
}

/// Define a highlighter from ordered tag/class mappings.
#[must_use]
pub fn tag_highlighter(
    styles: impl IntoIterator<Item = TagStyle>,
    options: TagHighlighterOptions,
) -> TagHighlighter {
    let mut classes = BTreeMap::new();
    for style in styles {
        for tag in &style.tags {
            classes.insert(tag.id(), Arc::clone(&style.class));
        }
    }
    TagHighlighter { classes, options }
}

/// The official stable `tok-*` class mapping.
#[must_use]
pub fn class_highlighter() -> &'static TagHighlighter {
    static HIGHLIGHTER: OnceLock<TagHighlighter> = OnceLock::new();
    HIGHLIGHTER.get_or_init(|| {
        let tags = tags();
        tag_highlighter(
            [
                TagStyle::new(tags.link, "tok-link"),
                TagStyle::new(tags.heading, "tok-heading"),
                TagStyle::new(tags.emphasis, "tok-emphasis"),
                TagStyle::new(tags.strong, "tok-strong"),
                TagStyle::new(tags.keyword, "tok-keyword"),
                TagStyle::new(tags.atom, "tok-atom"),
                TagStyle::new(tags.bool_, "tok-bool"),
                TagStyle::new(tags.url, "tok-url"),
                TagStyle::new(tags.label_name, "tok-labelName"),
                TagStyle::new(tags.inserted, "tok-inserted"),
                TagStyle::new(tags.deleted, "tok-deleted"),
                TagStyle::new(tags.literal, "tok-literal"),
                TagStyle::new(tags.string, "tok-string"),
                TagStyle::new(tags.number, "tok-number"),
                TagStyle::new(
                    [tags.regexp, tags.escape, tags.special.apply(tags.string)],
                    "tok-string2",
                ),
                TagStyle::new(tags.variable_name, "tok-variableName"),
                TagStyle::new(
                    tags.local.apply(tags.variable_name),
                    "tok-variableName tok-local",
                ),
                TagStyle::new(
                    tags.definition.apply(tags.variable_name),
                    "tok-variableName tok-definition",
                ),
                TagStyle::new(tags.special.apply(tags.variable_name), "tok-variableName2"),
                TagStyle::new(
                    tags.definition.apply(tags.property_name),
                    "tok-propertyName tok-definition",
                ),
                TagStyle::new(tags.type_name, "tok-typeName"),
                TagStyle::new(tags.namespace, "tok-namespace"),
                TagStyle::new(tags.class_name, "tok-className"),
                TagStyle::new(tags.macro_name, "tok-macroName"),
                TagStyle::new(tags.property_name, "tok-propertyName"),
                TagStyle::new(tags.operator, "tok-operator"),
                TagStyle::new(tags.comment, "tok-comment"),
                TagStyle::new(tags.meta, "tok-meta"),
                TagStyle::new(tags.invalid, "tok-invalid"),
                TagStyle::new(tags.punctuation, "tok-punctuation"),
            ],
            TagHighlighterOptions::default(),
        )
    })
}

pub(crate) fn highlight_tags(
    highlighters: &[&dyn Highlighter],
    top: &NodeType,
    tags: &TagSet,
) -> Option<String> {
    let mut result: Option<String> = None;
    for highlighter in highlighters
        .iter()
        .copied()
        .filter(|highlighter| highlighter.scope(top))
    {
        let Some(value) = highlighter.style(tags) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        if let Some(result) = &mut result {
            result.push(' ');
            result.push_str(&value);
        } else {
            result = Some(value);
        }
    }
    result
}
