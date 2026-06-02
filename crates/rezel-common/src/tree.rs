use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::ops::{BitOr, BitOrAssign};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use crate::prop::{NodePropSource, PropertyValue, group_prop, mounted_prop};
use crate::{NodeProp, Parser, TextRange, TextSize};

/// Default maximum source length represented by one packed tree buffer.
pub const DEFAULT_BUFFER_LENGTH: TextSize = TextSize::new(1024);

const MAX_NODE_TYPES: usize = u16::MAX as usize + 1;
const MAX_TREE_BUFFER_WORDS: usize = 65_532;
const BALANCE_BRANCH_FACTOR: usize = 8;
static NEXT_NODE_TYPE_IDENTITY: AtomicU64 = AtomicU64::new(1);

/// Flags attached to a syntax node type.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct NodeFlags(u8);

impl NodeFlags {
    /// Top-level grammar node.
    pub const TOP: Self = Self(1);
    /// Node produced by a skip rule.
    pub const SKIPPED: Self = Self(2);
    /// Error node.
    pub const ERROR: Self = Self(4);
    /// Generated node without a declared name.
    pub const ANONYMOUS: Self = Self(8);

    /// Whether all bits in `other` are present.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for NodeFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for NodeFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

#[derive(Clone)]
struct NodeTypeData {
    identity: u64,
    name: Arc<str>,
    props: Arc<BTreeMap<u32, PropertyValue>>,
    id: u16,
    flags: NodeFlags,
}

/// A syntax node type.
#[derive(Clone)]
pub struct NodeType(Arc<NodeTypeData>);

impl fmt::Debug for NodeType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NodeType")
            .field("name", &self.name())
            .field("id", &self.id())
            .field("flags", &self.0.flags)
            .finish_non_exhaustive()
    }
}

impl PartialEq for NodeType {
    fn eq(&self, other: &Self) -> bool {
        self.0.identity == other.0.identity
    }
}

impl Eq for NodeType {}

impl NodeType {
    /// Define a node type.
    #[must_use]
    pub fn new(id: u16, name: impl Into<Arc<str>>, flags: NodeFlags) -> Self {
        let name = name.into();
        let identity = NEXT_NODE_TYPE_IDENTITY.fetch_add(1, Ordering::Relaxed);
        Self(Arc::new(NodeTypeData {
            identity,
            name,
            props: Arc::new(BTreeMap::new()),
            id,
            flags,
        }))
    }

    /// Return the dummy anonymous node type.
    #[must_use]
    pub fn none() -> Self {
        static NONE: OnceLock<NodeType> = OnceLock::new();
        NONE.get_or_init(|| Self::new(0, "", NodeFlags::ANONYMOUS))
            .clone()
    }

    /// Return a copy with one type-level property.
    ///
    /// # Panics
    ///
    /// Panics when passed a per-node property.
    #[must_use]
    pub fn with_prop<T>(self, property: NodeProp<T>, value: T) -> Self
    where
        T: Clone + Send + Sync + 'static,
    {
        assert!(
            !property.is_per_node(),
            "cannot store a per-node property on a node type"
        );
        let mut props = (*self.0.props).clone();
        props.insert(property.id(), Arc::new(value));
        Self(Arc::new(NodeTypeData {
            identity: self.0.identity,
            name: Arc::clone(&self.0.name),
            props: Arc::new(props),
            id: self.0.id,
            flags: self.0.flags,
        }))
    }

    /// Declared name, or an empty string for anonymous types.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.0.name
    }

    fn name_arc(&self) -> Arc<str> {
        Arc::clone(&self.0.name)
    }

    /// Numeric term identifier.
    #[must_use]
    pub fn id(&self) -> u16 {
        self.0.id
    }

    /// Whether both handles identify the same grammar node type.
    ///
    /// Property extension preserves this identity, whereas independently
    /// constructed node sets never share it even when their numeric ids and
    /// names match.
    #[must_use]
    pub fn is(&self, other: &Self) -> bool {
        self.0.identity == other.0.identity
    }

    /// Retrieve a typed node property.
    #[must_use]
    pub fn prop<T>(&self, property: NodeProp<T>) -> Option<&T>
    where
        T: Clone + Send + Sync + 'static,
    {
        property.read(&self.0.props)
    }

    /// Whether this is a grammar top node.
    #[must_use]
    pub fn is_top(&self) -> bool {
        self.0.flags.contains(NodeFlags::TOP)
    }

    /// Whether this node came from a skip rule.
    #[must_use]
    pub fn is_skipped(&self) -> bool {
        self.0.flags.contains(NodeFlags::SKIPPED)
    }

    /// Whether this is an error node.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.0.flags.contains(NodeFlags::ERROR)
    }

    /// Whether this node was generated as anonymous tree structure.
    #[must_use]
    pub fn is_anonymous(&self) -> bool {
        self.0.flags.contains(NodeFlags::ANONYMOUS)
    }

    /// Match a node name, group name, or numeric term id.
    #[must_use]
    pub fn is_name(&self, name: &str) -> bool {
        self.name() == name || self.groups().iter().any(|group| group == name)
    }

    /// Group names assigned to this type.
    #[must_use]
    pub fn groups(&self) -> &[String] {
        self.prop(group_prop()).map_or(&[], Vec::as_slice)
    }

    fn with_properties(&self, props: BTreeMap<u32, PropertyValue>) -> Self {
        Self(Arc::new(NodeTypeData {
            identity: self.0.identity,
            name: Arc::clone(&self.0.name),
            props: Arc::new(props),
            id: self.0.id,
            flags: self.0.flags,
        }))
    }
}

/// A coherent collection of node types indexed by term id.
#[derive(Clone, Debug)]
pub struct NodeSet {
    types: Arc<[NodeType]>,
}

impl NodeSet {
    /// Construct a checked node set.
    ///
    /// # Panics
    ///
    /// Panics when ids do not match array positions or the set exceeds the
    /// 16-bit tree-buffer representation.
    #[must_use]
    pub fn new(types: Vec<NodeType>) -> Self {
        assert!(
            types.len() <= MAX_NODE_TYPES,
            "a node set cannot contain more than 65536 node types"
        );
        for (index, node_type) in types.iter().enumerate() {
            assert_eq!(
                usize::from(node_type.id()),
                index,
                "node type ids must match their node-set positions"
            );
        }
        Self {
            types: types.into(),
        }
    }

    /// All node types in id order.
    #[must_use]
    pub fn types(&self) -> &[NodeType] {
        &self.types
    }

    /// Retrieve a node type by term id.
    #[must_use]
    pub fn get(&self, id: u16) -> Option<&NodeType> {
        self.types.get(usize::from(id))
    }

    /// Copy this set while extending type-level properties.
    #[must_use]
    pub fn extend(&self, sources: &[NodePropSource]) -> Self {
        let types = self
            .types
            .iter()
            .map(|node_type| {
                let mut props = (*node_type.0.props).clone();
                let mut changed = false;
                for source in sources {
                    changed |= source.apply_to(node_type, &mut props);
                }
                if changed {
                    node_type.with_properties(props)
                } else {
                    node_type.clone()
                }
            })
            .collect();
        Self::new(types)
    }
}

/// A mounted mixed-language subtree.
#[derive(Clone)]
pub struct MountedTree {
    /// Inner syntax tree.
    pub tree: Tree,
    /// Relative overlay ranges, or `None` when the inner tree replaces the
    /// host node.
    pub overlay: Option<Arc<[TextRange]>>,
    /// Parser that created the subtree.
    pub parser: Arc<dyn Parser>,
    /// Whether the nested content is bracket-delimited.
    pub bracketed: bool,
}

impl fmt::Debug for MountedTree {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MountedTree")
            .field("tree", &self.tree)
            .field("overlay", &self.overlay)
            .field("parser", &"<parser>")
            .field("bracketed", &self.bracketed)
            .finish()
    }
}

/// One direct child in the compact tree representation.
#[derive(Clone, Debug)]
pub enum TreeChild {
    /// Stand-alone tree node.
    Tree(Tree),
    /// Packed group of small nodes.
    Buffer(TreeBuffer),
}

impl TreeChild {
    /// Source length covered by this child.
    #[must_use]
    pub fn len(&self) -> TextSize {
        match self {
            Self::Tree(tree) => tree.len(),
            Self::Buffer(buffer) => buffer.len(),
        }
    }

    /// Whether this child covers no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == TextSize::from(0)
    }
}

struct TreeData {
    node_type: NodeType,
    children: Arc<[TreeChild]>,
    positions: Arc<[TextSize]>,
    length: TextSize,
    props: RwLock<BTreeMap<u32, PropertyValue>>,
}

/// An immutable compact syntax tree node.
#[derive(Clone)]
pub struct Tree(Arc<TreeData>);

impl fmt::Debug for Tree {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Tree")
            .field("type", &self.node_type())
            .field("children", &self.children())
            .field("positions", &self.positions())
            .field("length", &self.len())
            .finish_non_exhaustive()
    }
}

impl fmt::Display for Tree {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.render())
    }
}

impl Tree {
    /// Construct a syntax tree node.
    ///
    /// # Panics
    ///
    /// Panics when the child and position counts differ.
    #[must_use]
    pub fn new(
        node_type: NodeType,
        children: Vec<TreeChild>,
        positions: Vec<TextSize>,
        length: TextSize,
    ) -> Self {
        assert_eq!(
            children.len(),
            positions.len(),
            "each tree child must have one relative position"
        );
        Self(Arc::new(TreeData {
            node_type,
            children: children.into(),
            positions: positions.into(),
            length,
            props: RwLock::new(BTreeMap::new()),
        }))
    }

    /// Empty dummy tree.
    #[must_use]
    pub fn empty() -> Self {
        Self::new(NodeType::none(), Vec::new(), Vec::new(), TextSize::from(0))
    }

    /// Build a compact tree from postfix parser records.
    ///
    /// # Panics
    ///
    /// Panics when records are malformed or refer to unknown node types.
    #[must_use]
    pub fn build<B>(build: &TreeBuild<B>) -> Self
    where
        B: PostfixBuffer,
    {
        build_tree(build)
    }

    /// Node type of this tree.
    #[must_use]
    pub fn node_type(&self) -> &NodeType {
        &self.0.node_type
    }

    /// Direct compact children.
    #[must_use]
    pub fn children(&self) -> &[TreeChild] {
        &self.0.children
    }

    /// Relative child positions.
    #[must_use]
    pub fn positions(&self) -> &[TextSize] {
        &self.0.positions
    }

    /// Total source length.
    #[must_use]
    pub fn len(&self) -> TextSize {
        self.0.length
    }

    /// Whether this tree covers no source bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.length == TextSize::from(0)
    }

    /// Attach a per-node property.
    ///
    /// # Panics
    ///
    /// Panics when passed a type-level property.
    pub fn set_prop<T>(&self, property: NodeProp<T>, value: T)
    where
        T: Clone + Send + Sync + 'static,
    {
        assert!(
            property.is_per_node(),
            "tree nodes only store per-node properties"
        );
        self.0
            .props
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(property.id(), Arc::new(value));
    }

    /// Retrieve a per-node or type-level property.
    #[must_use]
    pub fn prop<T>(&self, property: NodeProp<T>) -> Option<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        if !property.is_per_node() {
            return self.node_type().prop(property).cloned();
        }
        let props = self
            .0
            .props
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        property.read(&props).cloned()
    }

    /// Top syntax node.
    #[must_use]
    pub fn top_node(&self) -> SyntaxNode {
        syntax_root(self.clone(), TextSize::from(0))
    }

    /// Cursor positioned at the top node.
    #[must_use]
    pub fn cursor(&self, mode: IterMode) -> TreeCursor {
        TreeCursor::from_tree(self.clone(), TextSize::from(0), mode)
    }

    /// Cursor moved to the innermost node at a position.
    #[must_use]
    pub fn cursor_at(&self, position: TextSize, side: i8, mode: IterMode) -> TreeCursor {
        let mut cursor = self.cursor(mode);
        cursor.move_to(position, side);
        cursor
    }

    /// Resolve the innermost regular node at a position.
    #[must_use]
    pub fn resolve(&self, position: TextSize, side: i8) -> SyntaxNode {
        resolve_node(self.top_node(), position, side, false)
    }

    /// Resolve the innermost node, entering mounted overlays.
    #[must_use]
    pub fn resolve_inner(&self, position: TextSize, side: i8) -> SyntaxNode {
        resolve_node(self.top_node(), position, side, true)
    }

    /// Balance a wide tree into anonymous internal nodes.
    #[must_use]
    pub fn balance(&self) -> Self {
        if self.children().len() <= BALANCE_BRANCH_FACTOR {
            return self.clone();
        }
        balance_children(
            self.node_type().clone(),
            self.children(),
            self.positions(),
            self.len(),
        )
    }

    /// Return an equivalent tree in which all packed buffers are materialized
    /// as stand-alone tree nodes.
    #[doc(hidden)]
    #[must_use]
    pub fn materialize(&self) -> Self {
        let mut children = Vec::new();
        let mut positions = Vec::new();
        for (index, child) in self.children().iter().enumerate() {
            let position = self.positions()[index];
            match child {
                TreeChild::Tree(tree) => {
                    children.push(TreeChild::Tree(tree.materialize()));
                    positions.push(position);
                }
                TreeChild::Buffer(buffer) => {
                    let mut buffer_index = 0;
                    while buffer_index < buffer.data().len() {
                        let relative = TextSize::from(u32::from(buffer.data()[buffer_index + 1]));
                        let tree = materialize_buffer_node(buffer, buffer_index).materialize();
                        children.push(TreeChild::Tree(tree));
                        positions.push(position + relative);
                        buffer_index = usize::from(buffer.data()[buffer_index + 3]);
                    }
                }
            }
        }
        let result = Self::new(self.node_type().clone(), children, positions, self.len());
        let props = self
            .0
            .props
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        *result
            .0
            .props
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = props;
        result
    }

    fn copy_with_children(&self, children: Vec<TreeChild>) -> Self {
        let result = Self::new(
            self.node_type().clone(),
            children,
            self.positions().to_vec(),
            self.len(),
        );
        let props = self
            .0
            .props
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        *result
            .0
            .props
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = props;
        result
    }

    fn with_replaced_child(&self, index: usize, replacement: TreeChild) -> Self {
        let mut children = self.children().to_vec();
        children[index] = replacement;
        self.copy_with_children(children)
    }

    pub(crate) fn same_identity(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Iterate over visible nodes touching a byte range.
    pub fn iterate(
        &self,
        range: TextRange,
        mode: IterMode,
        mut enter: impl FnMut(&SyntaxNode) -> bool,
        mut leave: impl FnMut(&SyntaxNode),
    ) {
        let include_anonymous = mode.contains(IterMode::INCLUDE_ANONYMOUS);
        let mut cursor = self.cursor(mode | IterMode::INCLUDE_ANONYMOUS);
        loop {
            let mut entered = false;
            let touches = cursor.from() <= range.end() && cursor.to() >= range.start();
            let should_enter = !include_anonymous && cursor.node_type().is_anonymous();
            if touches && (should_enter || enter(&cursor.node())) {
                if cursor.first_child() {
                    continue;
                }
                entered = true;
            }

            loop {
                if entered && (include_anonymous || !cursor.node_type().is_anonymous()) {
                    leave(&cursor.node());
                }
                if cursor.next_sibling() {
                    break;
                }
                if !cursor.parent() {
                    return;
                }
                entered = true;
            }
        }
    }

    fn render(&self) -> String {
        if let Some(mounted) = self.prop(mounted_prop())
            && mounted.overlay.is_none()
        {
            return mounted.tree.render();
        }
        let mut children = Vec::new();
        for child in self.children() {
            let rendered = match child {
                TreeChild::Tree(tree) => tree.render(),
                TreeChild::Buffer(buffer) => buffer.render(),
            };
            if !rendered.is_empty() {
                children.push(rendered);
            }
        }
        render_node(self.node_type(), &children)
    }
}

/// Packed prefix-order tree nodes stored as 16-bit quads.
#[derive(Clone)]
pub struct TreeBuffer(Arc<TreeBufferData>);

struct TreeBufferData {
    data: Arc<[u16]>,
    length: TextSize,
    node_set: Arc<NodeSet>,
}

impl fmt::Debug for TreeBuffer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TreeBuffer")
            .field("quad_count", &(self.0.data.len() / 4))
            .field("length", &self.0.length)
            .finish_non_exhaustive()
    }
}

impl TreeBuffer {
    /// Construct a checked packed tree buffer.
    ///
    /// # Panics
    ///
    /// Panics when the buffer is not made of valid quads.
    #[must_use]
    pub fn new(data: Vec<u16>, length: TextSize, node_set: Arc<NodeSet>) -> Self {
        assert_eq!(data.len() % 4, 0, "tree buffers contain four-word records");
        validate_buffer(&data, &node_set);
        Self(Arc::new(TreeBufferData {
            data: data.into(),
            length,
            node_set,
        }))
    }

    /// Packed `(type, from, to, end_index)` words.
    #[must_use]
    pub fn data(&self) -> &[u16] {
        &self.0.data
    }

    /// Total source length represented by the buffer.
    #[must_use]
    pub fn len(&self) -> TextSize {
        self.0.length
    }

    /// Whether the buffer covers no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.length == TextSize::new(0)
    }

    /// Node set used by packed type ids.
    #[must_use]
    pub fn node_set(&self) -> &Arc<NodeSet> {
        &self.0.node_set
    }

    /// Extract part of this buffer and rebase its positions.
    ///
    /// # Panics
    ///
    /// Panics when indices are not record boundaries or are out of bounds.
    #[must_use]
    pub fn slice(&self, start: usize, end: usize, from: TextSize) -> Self {
        assert!(start <= end && end <= self.0.data.len());
        assert_eq!(start % 4, 0);
        assert_eq!(end % 4, 0);
        let mut data = self.0.data[start..end].to_vec();
        let mut length = TextSize::from(0);
        for record in data.chunks_exact_mut(4) {
            record[1] = u32::from(
                TextSize::from(u32::from(record[1]))
                    .checked_sub(from)
                    .expect("slice base must not exceed node start"),
            )
            .try_into()
            .expect("rebased tree-buffer position exceeds 16 bits");
            record[2] = u32::from(
                TextSize::from(u32::from(record[2]))
                    .checked_sub(from)
                    .expect("slice base must not exceed node end"),
            )
            .try_into()
            .expect("rebased tree-buffer position exceeds 16 bits");
            record[3] = usize::from(record[3])
                .checked_sub(start)
                .expect("slice start must not exceed node end index")
                .try_into()
                .expect("rebased tree-buffer index exceeds 16 bits");
            length = length.max(TextSize::from(u32::from(record[2])));
        }
        Self::new(data, length, Arc::clone(&self.0.node_set))
    }

    fn render(&self) -> String {
        let mut nodes = Vec::new();
        let mut index = 0;
        while index < self.0.data.len() {
            nodes.push(self.render_node(index));
            index = usize::from(self.0.data[index + 3]);
        }
        nodes.join(",")
    }

    fn render_node(&self, index: usize) -> String {
        let node_type = self
            .0
            .node_set
            .get(self.0.data[index])
            .expect("validated tree-buffer type");
        let end = usize::from(self.0.data[index + 3]);
        let mut children = Vec::new();
        let mut child = index + 4;
        while child < end {
            children.push(self.render_node(child));
            child = usize::from(self.0.data[child + 3]);
        }
        render_node(node_type, &children)
    }
}

fn validate_buffer(data: &[u16], node_set: &NodeSet) {
    let mut enclosing_ends = Vec::new();
    for (index, record) in data.chunks_exact(4).enumerate() {
        let offset = index * 4;
        while enclosing_ends.last().is_some_and(|end| *end <= offset) {
            enclosing_ends.pop();
        }
        assert!(
            node_set.get(record[0]).is_some(),
            "tree buffer refers to an unknown node type"
        );
        assert!(record[1] <= record[2], "tree-buffer range is reversed");
        let end = usize::from(record[3]);
        assert!(
            end >= offset + 4 && end <= data.len() && end % 4 == 0,
            "tree-buffer end index is invalid"
        );
        if let Some(parent_end) = enclosing_ends.last() {
            assert!(
                end <= *parent_end,
                "tree-buffer child end exceeds enclosing parent end"
            );
        }
        enclosing_ends.push(end);
    }
}

/// Postfix tree-construction input produced by a parser.
#[derive(Clone, Debug)]
pub struct TreeBuild<B = Vec<u32>> {
    /// Four integers per record: term id, start, end, subtree record size.
    pub buffer: B,
    /// Node types used by term ids.
    pub node_set: Arc<NodeSet>,
    /// Top node id wrapping the records.
    pub top_id: u16,
    /// Absolute start position represented by the records.
    pub start: TextSize,
    /// Optional explicit outer length.
    pub length: Option<TextSize>,
    /// Maximum byte length placed in one packed buffer.
    pub max_buffer_length: TextSize,
    /// First anonymous grammar term used only to group EBNF repetitions.
    pub min_repeat_type: usize,
}

impl<B> TreeBuild<B> {
    /// Create tree-build input using the default buffer threshold.
    #[must_use]
    pub fn new(buffer: B, node_set: Arc<NodeSet>, top_id: u16) -> Self {
        let min_repeat_type = node_set.types().len();
        Self {
            buffer,
            node_set,
            top_id,
            start: TextSize::from(0),
            length: None,
            max_buffer_length: DEFAULT_BUFFER_LENGTH,
            min_repeat_type,
        }
    }
}

/// A postfix parser-record buffer that can create a backward cursor.
///
/// This is an internal static-runtime ABI used by `rezel-lr`. Language users
/// should construct trees through their generated parser.
#[doc(hidden)]
pub trait PostfixBuffer {
    /// Cursor borrowing this buffer.
    type Cursor<'a>: PostfixCursor
    where
        Self: 'a;

    /// Create a cursor positioned just after the last record.
    fn postfix_cursor(&self) -> Self::Cursor<'_>;
}

/// A backward cursor over postfix parser records.
///
/// This mirrors the cursor contract consumed by Lezer's compact tree builder.
#[doc(hidden)]
pub trait PostfixCursor: Clone {
    /// Current node term.
    fn id(&self) -> u16;

    /// Current node start.
    fn start(&self) -> TextSize;

    /// Current node end.
    fn end(&self) -> TextSize;

    /// Number of postfix words covered by the current node.
    fn size(&self) -> usize;

    /// Absolute cursor position in postfix words.
    fn position(&self) -> usize;

    /// Move to the previous record.
    fn next(&mut self);
}

impl PostfixBuffer for Vec<u32> {
    type Cursor<'a> = FlatPostfixCursor<'a>;

    fn postfix_cursor(&self) -> Self::Cursor<'_> {
        FlatPostfixCursor::new(self)
    }
}

fn build_tree<B>(build: &TreeBuild<B>) -> Tree
where
    B: PostfixBuffer,
{
    assert!(
        build.max_buffer_length <= TextSize::new(u32::from(u16::MAX)),
        "tree-buffer source length exceeds 16 bits"
    );
    let cursor = build.buffer.postfix_cursor();
    assert_eq!(
        cursor.position() % 4,
        0,
        "postfix tree records must contain four values"
    );
    let top = build
        .node_set
        .get(build.top_id)
        .unwrap_or_else(|| panic!("unknown top node type id {}", build.top_id))
        .clone();
    let mut builder = CompactTreeBuilder { build, cursor };
    let mut children = Vec::new();
    let mut positions = Vec::new();
    while builder.cursor.position() > 0 {
        builder.take_node(build.start, 0, &mut children, &mut positions, None, 0);
    }
    children.reverse();
    positions.reverse();
    let inferred = children
        .last()
        .zip(positions.last())
        .map_or(TextSize::from(0), |(child, position)| {
            *position + child.len()
        });
    Tree::new(top, children, positions, build.length.unwrap_or(inferred))
}

/// Backward cursor over one contiguous postfix buffer.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct FlatPostfixCursor<'a> {
    buffer: &'a [u32],
    index: usize,
}

impl<'a> FlatPostfixCursor<'a> {
    fn new(buffer: &'a [u32]) -> Self {
        Self {
            buffer,
            index: buffer.len(),
        }
    }
}

impl PostfixCursor for FlatPostfixCursor<'_> {
    fn id(&self) -> u16 {
        self.buffer[self.index - 4]
            .try_into()
            .expect("node type ids must fit in 16 bits")
    }

    fn start(&self) -> TextSize {
        TextSize::from(self.buffer[self.index - 3])
    }

    fn end(&self) -> TextSize {
        TextSize::from(self.buffer[self.index - 2])
    }

    fn size(&self) -> usize {
        self.buffer[self.index - 1] as usize
    }

    fn position(&self) -> usize {
        self.index
    }

    fn next(&mut self) {
        self.index = self
            .index
            .checked_sub(4)
            .expect("postfix cursor moved before its buffer");
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct BufferScan {
    size: usize,
    start: TextSize,
    skip: usize,
}

struct CompactTreeBuilder<'a, B>
where
    B: PostfixBuffer,
{
    build: &'a TreeBuild<B>,
    cursor: B::Cursor<'a>,
}

#[derive(Clone, Copy)]
struct NodeRequest {
    parent_start: TextSize,
    min_position: usize,
    in_repeat: Option<u16>,
    depth: usize,
}

struct BuiltNode {
    child: TreeChild,
    position: TextSize,
}

struct PendingNode {
    start: TextSize,
    end: TextSize,
    end_position: usize,
    start_position: TextSize,
    node_type: NodeType,
    local_children: Vec<TreeChild>,
    local_positions: Vec<TextSize>,
    local_repeat: Option<u16>,
    last_group: usize,
    last_end: TextSize,
    depth: usize,
}

enum StartedNode {
    Built(BuiltNode),
    Pending(PendingNode),
}

impl<B> CompactTreeBuilder<'_, B>
where
    B: PostfixBuffer,
{
    #[allow(clippy::too_many_arguments)]
    fn take_node(
        &mut self,
        parent_start: TextSize,
        min_position: usize,
        children: &mut Vec<TreeChild>,
        positions: &mut Vec<TextSize>,
        in_repeat: Option<u16>,
        depth: usize,
    ) {
        let mut pending: Vec<PendingNode> = Vec::new();
        let mut request = Some(NodeRequest {
            parent_start,
            min_position,
            in_repeat,
            depth,
        });
        loop {
            if let Some(next) = request.take() {
                match self.start_node(next) {
                    StartedNode::Built(node) => {
                        if let Some(parent) = pending.last_mut() {
                            parent.local_children.push(node.child);
                            parent.local_positions.push(node.position);
                        } else {
                            children.push(node.child);
                            positions.push(node.position);
                            return;
                        }
                    }
                    StartedNode::Pending(node) => pending.push(node),
                }
            }

            loop {
                let Some(node) = pending.last_mut() else {
                    return;
                };
                if self.cursor.position() > node.end_position {
                    if node.local_repeat == Some(self.cursor.id()) {
                        if self
                            .cursor
                            .end()
                            .checked_add(self.build.max_buffer_length)
                            .is_some_and(|end| end <= node.last_end)
                        {
                            make_repeat_leaf(
                                &mut node.local_children,
                                &mut node.local_positions,
                                node.start,
                                node.last_group,
                                self.cursor.end(),
                                node.last_end,
                                node.node_type.clone(),
                            );
                            node.last_group = node.local_children.len();
                            node.last_end = self.cursor.end();
                        }
                        self.cursor.next();
                        continue;
                    }
                    if node.depth > 2_500 {
                        self.take_flat_node(
                            node.start,
                            node.end_position,
                            &mut node.local_children,
                            &mut node.local_positions,
                        );
                        continue;
                    }
                    request = Some(NodeRequest {
                        parent_start: node.start,
                        min_position: node.end_position,
                        in_repeat: node.local_repeat,
                        depth: node.depth + 1,
                    });
                    break;
                }

                let node = pending.pop().expect("pending node exists");
                let built = Self::finish_node(node);
                if let Some(parent) = pending.last_mut() {
                    parent.local_children.push(built.child);
                    parent.local_positions.push(built.position);
                } else {
                    children.push(built.child);
                    positions.push(built.position);
                    return;
                }
            }
        }
    }

    fn start_node(&mut self, request: NodeRequest) -> StartedNode {
        let id = self.cursor.id();
        let start = self.cursor.start();
        let end = self.cursor.end();
        let size = self.cursor.size();
        assert!(start <= end, "postfix node range is reversed");
        assert!(
            size >= 4 && size.is_multiple_of(4) && size <= self.cursor.position(),
            "postfix node size is invalid"
        );
        let node_type = self
            .build
            .node_set
            .get(id)
            .unwrap_or_else(|| panic!("unknown node type id {id}"))
            .clone();
        let mut start_position = start - request.parent_start;
        let packed = if end - start <= self.build.max_buffer_length {
            self.find_buffer_size(
                self.cursor.position() - request.min_position,
                request.in_repeat,
            )
        } else {
            None
        };

        if let Some(packed) = packed {
            let mut data = vec![0; packed.size - packed.skip];
            let end_position = self.cursor.position() - packed.size;
            let mut index = data.len();
            while self.cursor.position() > end_position {
                index = self.copy_to_buffer(packed.start, &mut data, index);
            }
            assert_eq!(index, 0, "packed tree buffer was not filled");
            start_position = packed.start - request.parent_start;
            return StartedNode::Built(BuiltNode {
                child: TreeChild::Buffer(TreeBuffer::new(
                    data,
                    end - packed.start,
                    Arc::clone(&self.build.node_set),
                )),
                position: start_position,
            });
        }

        let end_position = self.cursor.position() - size;
        self.cursor.next();
        StartedNode::Pending(PendingNode {
            start,
            end,
            end_position,
            start_position,
            node_type,
            local_children: Vec::new(),
            local_positions: Vec::new(),
            local_repeat: (usize::from(id) >= self.build.min_repeat_type).then_some(id),
            last_group: 0,
            last_end: end,
            depth: request.depth,
        })
    }

    fn finish_node(mut node: PendingNode) -> BuiltNode {
        if node.local_repeat.is_some()
            && node.last_group > 0
            && node.last_group < node.local_children.len()
        {
            make_repeat_leaf(
                &mut node.local_children,
                &mut node.local_positions,
                node.start,
                node.last_group,
                node.start,
                node.last_end,
                node.node_type.clone(),
            );
        }
        node.local_children.reverse();
        node.local_positions.reverse();
        let tree = if node.local_repeat.is_some() && node.last_group > 0 {
            balance_repeat(
                &node.node_type,
                &node.local_children,
                &node.local_positions,
                node.end - node.start,
            )
        } else {
            Tree::new(
                node.node_type,
                node.local_children,
                node.local_positions,
                node.end - node.start,
            )
        };
        BuiltNode {
            child: TreeChild::Tree(tree),
            position: node.start_position,
        }
    }

    fn take_flat_node(
        &mut self,
        parent_start: TextSize,
        min_position: usize,
        children: &mut Vec<TreeChild>,
        positions: &mut Vec<TextSize>,
    ) {
        let mut nodes = Vec::new();
        let mut stop_at = None;
        while self.cursor.position() > min_position {
            if nodes.len() >= MAX_TREE_BUFFER_WORDS / 4 {
                break;
            }
            let size = self.cursor.size();
            if size > 4 {
                self.cursor.next();
                continue;
            }
            if stop_at.is_some_and(|stop| self.cursor.start() < stop) {
                break;
            }
            stop_at.get_or_insert(
                self.cursor
                    .end()
                    .checked_sub(self.build.max_buffer_length)
                    .unwrap_or(TextSize::from(0)),
            );
            nodes.push((self.cursor.id(), self.cursor.start(), self.cursor.end()));
            self.cursor.next();
        }
        let Some((_, start, _)) = nodes.last().copied() else {
            return;
        };
        let end = nodes[0].2;
        let mut data = Vec::with_capacity(nodes.len() * 4);
        for (id, from, to) in nodes.into_iter().rev() {
            let record_end = data.len() + 4;
            data.extend_from_slice(&[
                id,
                u32::from(from - start)
                    .try_into()
                    .expect("flat tree-buffer start exceeds 16 bits"),
                u32::from(to - start)
                    .try_into()
                    .expect("flat tree-buffer end exceeds 16 bits"),
                record_end
                    .try_into()
                    .expect("flat tree-buffer index exceeds 16 bits"),
            ]);
        }
        children.push(TreeChild::Buffer(TreeBuffer::new(
            data,
            end - start,
            Arc::clone(&self.build.node_set),
        )));
        positions.push(start - parent_start);
    }

    fn find_buffer_size(&self, max_size: usize, in_repeat: Option<u16>) -> Option<BufferScan> {
        let mut cursor = self.cursor.clone();
        let minimum_position = cursor.position() - max_size;
        let minimum_start = cursor
            .end()
            .checked_sub(self.build.max_buffer_length)
            .unwrap_or(TextSize::from(0));
        let mut size = 0;
        let mut start = TextSize::from(0);
        let mut skip = 0;
        let mut result = BufferScan::default();

        while cursor.position() > minimum_position {
            let node_size = cursor.size();
            if in_repeat == Some(cursor.id()) {
                result = BufferScan { size, start, skip };
                skip += 4;
                size += 4;
                cursor.next();
                continue;
            }
            let Some(node_start_position) = cursor.position().checked_sub(node_size) else {
                break;
            };
            if node_start_position < minimum_position || cursor.start() < minimum_start {
                break;
            }
            let mut local_skipped =
                usize::from(usize::from(cursor.id()) >= self.build.min_repeat_type) * 4;
            let node_start = cursor.start();
            cursor.next();
            while cursor.position() > node_start_position {
                if usize::from(cursor.id()) >= self.build.min_repeat_type {
                    local_skipped += 4;
                }
                cursor.next();
            }
            start = node_start;
            size += node_size;
            skip += local_skipped;
        }
        if in_repeat.is_none() || size == max_size {
            result = BufferScan { size, start, skip };
        }
        let data_size = result.size.saturating_sub(result.skip);
        (result.size > 4 && data_size <= MAX_TREE_BUFFER_WORDS).then_some(result)
    }

    fn copy_to_buffer(
        &mut self,
        buffer_start: TextSize,
        data: &mut [u16],
        mut index: usize,
    ) -> usize {
        let id = self.cursor.id();
        let start = self.cursor.start();
        let end = self.cursor.end();
        let size = self.cursor.size();
        self.cursor.next();
        if usize::from(id) >= self.build.min_repeat_type {
            return index;
        }
        let start_index = index;
        if size > 4 {
            let end_position = self.cursor.position() - (size - 4);
            while self.cursor.position() > end_position {
                index = self.copy_to_buffer(buffer_start, data, index);
            }
        }
        index -= 4;
        data[index] = id;
        data[index + 1] = u32::from(start - buffer_start)
            .try_into()
            .expect("tree-buffer node start exceeds 16 bits");
        data[index + 2] = u32::from(end - buffer_start)
            .try_into()
            .expect("tree-buffer node end exceeds 16 bits");
        data[index + 3] = start_index
            .try_into()
            .expect("tree-buffer node index exceeds 16 bits");
        index
    }
}

fn make_repeat_leaf(
    children: &mut Vec<TreeChild>,
    positions: &mut Vec<TextSize>,
    base: TextSize,
    index: usize,
    from: TextSize,
    to: TextSize,
    node_type: NodeType,
) {
    let mut local_children = Vec::new();
    let mut local_positions = Vec::new();
    while children.len() > index {
        local_children.push(children.pop().expect("repeat child exists"));
        let position = positions.pop().expect("repeat child position exists");
        local_positions.push(position + base - from);
    }
    children.push(TreeChild::Tree(Tree::new(
        node_type,
        local_children,
        local_positions,
        to - from,
    )));
    positions.push(from - base);
}

fn balance_repeat(
    node_type: &NodeType,
    children: &[TreeChild],
    positions: &[TextSize],
    length: TextSize,
) -> Tree {
    let mut node_sizes = HashMap::new();
    balance_range(
        node_type,
        &mut node_sizes,
        children,
        positions,
        0,
        children.len(),
        TextSize::from(0),
        length,
        Some(node_type.clone()),
        node_type,
    )
}

#[allow(clippy::too_many_arguments)]
fn balance_range(
    balance_type: &NodeType,
    node_sizes: &mut HashMap<usize, usize>,
    children: &[TreeChild],
    positions: &[TextSize],
    from: usize,
    to: usize,
    start: TextSize,
    length: TextSize,
    top_type: Option<NodeType>,
    inner_type: &NodeType,
) -> Tree {
    let mut total = 0;
    for child in &children[from..to] {
        total += balanced_node_size(balance_type, child, node_sizes);
    }
    let maximum_child = (total * 3).div_ceil(BALANCE_BRANCH_FACTOR * 2);
    let mut local_children = Vec::new();
    let mut local_positions = Vec::new();
    divide_balanced(
        balance_type,
        node_sizes,
        children,
        positions,
        from,
        to,
        TextSize::from(0),
        start,
        maximum_child,
        inner_type,
        &mut local_children,
        &mut local_positions,
    );
    let node_type = top_type.unwrap_or_else(|| inner_type.clone());
    if local_children.len() == 1
        && local_positions == [TextSize::from(0)]
        && matches!(&local_children[0], TreeChild::Tree(tree) if tree.node_type().is(&node_type) && tree.len() == length)
    {
        let TreeChild::Tree(tree) = &local_children[0] else {
            unreachable!();
        };
        return tree.clone();
    }
    Tree::new(node_type, local_children, local_positions, length)
}

#[allow(clippy::too_many_arguments)]
fn divide_balanced(
    balance_type: &NodeType,
    node_sizes: &mut HashMap<usize, usize>,
    children: &[TreeChild],
    positions: &[TextSize],
    from: usize,
    to: usize,
    offset: TextSize,
    start: TextSize,
    maximum_child: usize,
    inner_type: &NodeType,
    local_children: &mut Vec<TreeChild>,
    local_positions: &mut Vec<TextSize>,
) {
    let mut index = from;
    while index < to {
        let group_from = index;
        let group_start = positions[index];
        let mut group_size = balanced_node_size(balance_type, &children[index], node_sizes);
        index += 1;
        while index < to {
            let next_size = balanced_node_size(balance_type, &children[index], node_sizes);
            if group_size + next_size >= maximum_child {
                break;
            }
            group_size += next_size;
            index += 1;
        }

        if index == group_from + 1 {
            if group_size > maximum_child {
                let TreeChild::Tree(only) = &children[group_from] else {
                    unreachable!("only trees can have a balanced size above one");
                };
                divide_balanced(
                    balance_type,
                    node_sizes,
                    only.children(),
                    only.positions(),
                    0,
                    only.children().len(),
                    positions[group_from] + offset,
                    start,
                    maximum_child,
                    inner_type,
                    local_children,
                    local_positions,
                );
                continue;
            }
            local_children.push(children[group_from].clone());
        } else {
            let group_length = positions[index - 1] + children[index - 1].len() - group_start;
            let tree = balance_range(
                balance_type,
                node_sizes,
                children,
                positions,
                group_from,
                index,
                group_start,
                group_length,
                None,
                inner_type,
            );
            local_children.push(TreeChild::Tree(tree));
        }
        local_positions.push(group_start + offset - start);
    }
}

fn balanced_node_size(
    balance_type: &NodeType,
    node: &TreeChild,
    node_sizes: &mut HashMap<usize, usize>,
) -> usize {
    let TreeChild::Tree(tree) = node else {
        return 1;
    };
    if !tree.node_type().is(balance_type) {
        return 1;
    }

    let children = tree.children();
    let Some(TreeChild::Tree(first)) = children.first() else {
        return 1;
    };
    if !first.node_type().is(balance_type) {
        return 1;
    }

    let identity = Arc::as_ptr(&tree.0) as usize;
    if let Some(size) = node_sizes.get(&identity) {
        return *size;
    }

    let mut size = 1;
    for child in children {
        let TreeChild::Tree(child_tree) = child else {
            size = 1;
            break;
        };
        if !child_tree.node_type().is(balance_type) {
            size = 1;
            break;
        }
        size += balanced_node_size(balance_type, child, node_sizes);
    }
    // Leaves and nodes that stop at an incompatible child are already known
    // to have the unit weight. Keep those common cases out of the cache, so
    // a flat list of anonymous leaves does not pay one hash-table operation
    // per child. Recursively expandable nodes still get cached below.
    if size > 1 {
        node_sizes.insert(identity, size);
    }
    size
}

fn render_node(node_type: &NodeType, children: &[String]) -> String {
    if node_type.name().is_empty() {
        return children.join(",");
    }
    let name = if node_type
        .name()
        .chars()
        .all(|character| character == '_' || character == '$' || character.is_alphanumeric())
        || node_type.is_error()
    {
        node_type.name().to_owned()
    } else {
        format!("{:?}", node_type.name())
    };
    if children.is_empty() {
        name
    } else {
        format!("{name}({})", children.join(","))
    }
}

fn balance_children(
    top_type: NodeType,
    children: &[TreeChild],
    positions: &[TextSize],
    length: TextSize,
) -> Tree {
    let anonymous = NodeType::none();
    let mut node_sizes = HashMap::new();
    balance_range(
        &anonymous,
        &mut node_sizes,
        children,
        positions,
        0,
        children.len(),
        TextSize::from(0),
        length,
        Some(top_type),
        &anonymous,
    )
}

/// Cursor traversal options.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct IterMode(u8);

impl IterMode {
    /// Default traversal.
    pub const NONE: Self = Self(0);
    /// Visit only stand-alone trees, excluding packed buffer nodes.
    pub const EXCLUDE_BUFFERS: Self = Self(1);
    /// Include anonymous balancing/repetition nodes.
    pub const INCLUDE_ANONYMOUS: Self = Self(2);
    /// Ignore regular replacement mounts.
    pub const IGNORE_MOUNTS: Self = Self(4);
    /// Do not enter overlay mounts.
    pub const IGNORE_OVERLAYS: Self = Self(8);
    /// Enter bracketed overlays at either boundary.
    pub const ENTER_BRACKETED: Self = Self(16);

    /// Whether all bits in `other` are present.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for IterMode {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for IterMode {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

const SIDE_BEFORE: i8 = -2;
const SIDE_AT_OR_BEFORE: i8 = -1;
const SIDE_AROUND: i8 = 0;
const SIDE_AT_OR_AFTER: i8 = 1;
const SIDE_AFTER: i8 = 2;
const SIDE_DONT_CARE: i8 = 4;

fn signed_index(index: usize) -> isize {
    isize::try_from(index).expect("tree child index exceeds isize range")
}

fn unsigned_index(index: isize) -> usize {
    usize::try_from(index).expect("tree child index must not be negative")
}

fn buffer_node_type(buffer: &TreeBuffer, index: usize) -> &NodeType {
    buffer
        .node_set()
        .get(buffer.data()[index])
        .expect("validated tree-buffer type")
}

#[derive(Clone, Debug)]
enum NodeLocation {
    Tree(Arc<TreeNodeLocation>),
    Buffer(Arc<BufferNodeLocation>),
}

#[derive(Debug)]
struct TreeNodeLocation {
    tree: Tree,
    from: TextSize,
    child_index: Option<usize>,
    parent: Option<Arc<TreeNodeLocation>>,
}

#[derive(Debug)]
struct BufferContext {
    parent: Arc<TreeNodeLocation>,
    buffer: TreeBuffer,
    child_index: usize,
    start: TextSize,
}

#[derive(Debug)]
struct BufferNodeLocation {
    context: Arc<BufferContext>,
    parent: Option<Arc<BufferNodeLocation>>,
    index: usize,
}

impl NodeLocation {
    fn node_type_ref(&self) -> &NodeType {
        match self {
            Self::Tree(node) => node.tree.node_type(),
            Self::Buffer(node) => buffer_node_type(&node.context.buffer, node.index),
        }
    }

    fn node_type(&self) -> NodeType {
        self.node_type_ref().clone()
    }

    fn from(&self) -> TextSize {
        match self {
            Self::Tree(node) => node.from,
            Self::Buffer(node) => {
                node.context.start
                    + TextSize::from(u32::from(node.context.buffer.data()[node.index + 1]))
            }
        }
    }

    fn to(&self) -> TextSize {
        match self {
            Self::Tree(node) => node.from + node.tree.len(),
            Self::Buffer(node) => {
                node.context.start
                    + TextSize::from(u32::from(node.context.buffer.data()[node.index + 2]))
            }
        }
    }

    fn raw_parent(&self) -> Option<Self> {
        match self {
            Self::Tree(node) => node.parent.clone().map(Self::Tree),
            Self::Buffer(node) => node
                .parent
                .clone()
                .map(Self::Buffer)
                .or_else(|| Some(Self::Tree(Arc::clone(&node.context.parent)))),
        }
    }

    fn visible_parent(&self, mode: IterMode) -> Option<Self> {
        let mut parent = self.raw_parent();
        while !mode.contains(IterMode::INCLUDE_ANONYMOUS) {
            let Some(current) = parent.as_ref() else {
                break;
            };
            if !current.node_type_ref().is_anonymous() {
                break;
            }
            parent = current.raw_parent();
        }
        parent
    }

    fn first_child(&self, mode: IterMode) -> Option<Self> {
        match self {
            Self::Tree(node) => node.next_child(0, 1, TextSize::from(0), SIDE_DONT_CARE, mode),
            Self::Buffer(node) => (!mode.contains(IterMode::EXCLUDE_BUFFERS))
                .then(|| node.child(1, TextSize::from(0), SIDE_DONT_CARE))
                .flatten()
                .map(Self::Buffer),
        }
    }

    fn last_child(&self, mode: IterMode) -> Option<Self> {
        match self {
            Self::Tree(node) => node.next_child(
                signed_index(node.tree.children().len()) - 1,
                -1,
                TextSize::from(0),
                SIDE_DONT_CARE,
                mode,
            ),
            Self::Buffer(node) => (!mode.contains(IterMode::EXCLUDE_BUFFERS))
                .then(|| node.child(-1, TextSize::from(0), SIDE_DONT_CARE))
                .flatten()
                .map(Self::Buffer),
        }
    }

    fn child_after(&self, position: TextSize, mode: IterMode) -> Option<Self> {
        match self {
            Self::Tree(node) => node.next_child(0, 1, position, SIDE_AFTER, mode),
            Self::Buffer(node) => (!mode.contains(IterMode::EXCLUDE_BUFFERS))
                .then(|| node.child(1, position, SIDE_AFTER))
                .flatten()
                .map(Self::Buffer),
        }
    }

    fn child_before(&self, position: TextSize, mode: IterMode) -> Option<Self> {
        match self {
            Self::Tree(node) => node.next_child(
                signed_index(node.tree.children().len()) - 1,
                -1,
                position,
                SIDE_BEFORE,
                mode,
            ),
            Self::Buffer(node) => (!mode.contains(IterMode::EXCLUDE_BUFFERS))
                .then(|| node.child(-1, position, SIDE_BEFORE))
                .flatten()
                .map(Self::Buffer),
        }
    }

    fn enter(&self, position: TextSize, side: i8, mode: IterMode) -> Option<Self> {
        match self {
            Self::Tree(node) => node.enter(position, side, mode),
            Self::Buffer(node) => (!mode.contains(IterMode::EXCLUDE_BUFFERS))
                .then(|| node.child(1, position, side))
                .flatten()
                .map(Self::Buffer),
        }
    }

    fn next_sibling(&self, mode: IterMode) -> Option<Self> {
        match self {
            Self::Tree(node) => {
                let parent = node.parent.as_ref()?;
                let index = node.child_index?;
                parent.next_child(
                    signed_index(index) + 1,
                    1,
                    TextSize::from(0),
                    SIDE_DONT_CARE,
                    mode,
                )
            }
            Self::Buffer(node) => node.next_sibling(mode),
        }
    }

    fn previous_sibling(&self, mode: IterMode) -> Option<Self> {
        match self {
            Self::Tree(node) => {
                let parent = node.parent.as_ref()?;
                let index = node.child_index?;
                parent.next_child(
                    signed_index(index) - 1,
                    -1,
                    TextSize::from(0),
                    SIDE_DONT_CARE,
                    mode,
                )
            }
            Self::Buffer(node) => node.previous_sibling(mode),
        }
    }

    fn is_overlay_root(&self) -> bool {
        matches!(self, Self::Tree(node) if node.parent.is_some() && node.child_index.is_none())
    }
}

impl From<Arc<TreeNodeLocation>> for NodeLocation {
    fn from(node: Arc<TreeNodeLocation>) -> Self {
        Self::Tree(node)
    }
}

impl From<Arc<BufferNodeLocation>> for NodeLocation {
    fn from(node: Arc<BufferNodeLocation>) -> Self {
        Self::Buffer(node)
    }
}

impl TreeNodeLocation {
    #[allow(clippy::too_many_lines)]
    fn next_child(
        self: &Arc<Self>,
        mut index: isize,
        direction: i8,
        position: TextSize,
        side: i8,
        mode: IterMode,
    ) -> Option<NodeLocation> {
        let mut parent = Arc::clone(self);
        loop {
            let children = parent.tree.children();
            let end = if direction > 0 {
                signed_index(children.len())
            } else {
                -1
            };
            while index != end {
                let child_index = unsigned_index(index);
                let child = &children[child_index];
                let child_from = parent.from + parent.tree.positions()[child_index];
                index += isize::from(direction);

                let TreeChild::Tree(child_tree) = child else {
                    if mode.contains(IterMode::EXCLUDE_BUFFERS) {
                        continue;
                    }
                    let TreeChild::Buffer(buffer) = child else {
                        unreachable!();
                    };
                    if !check_side(side, position, child_from, child_from + buffer.len()) {
                        continue;
                    }
                    let Some(record) = find_buffer_child(
                        buffer,
                        0,
                        buffer.data().len(),
                        direction,
                        position,
                        child_from,
                        side,
                    ) else {
                        continue;
                    };
                    let context = Arc::new(BufferContext {
                        parent: Arc::clone(&parent),
                        buffer: buffer.clone(),
                        child_index,
                        start: child_from,
                    });
                    return Some(NodeLocation::Buffer(Arc::new(BufferNodeLocation {
                        context,
                        parent: None,
                        index: record,
                    })));
                };

                let mounted = child_tree.prop(mounted_prop());
                let enters_bracketed = mode.contains(IterMode::ENTER_BRACKETED)
                    && mounted.as_ref().is_some_and(|mounted| {
                        mounted.overlay.is_none()
                            && mounted.bracketed
                            && position >= child_from
                            && position <= child_from + child_tree.len()
                    });
                if !enters_bracketed
                    && !check_side(side, position, child_from, child_from + child_tree.len())
                {
                    continue;
                }
                if !mode.contains(IterMode::INCLUDE_ANONYMOUS)
                    && child_tree.node_type().is_anonymous()
                    && !has_visible_child(child_tree)
                {
                    continue;
                }

                if !mode.contains(IterMode::IGNORE_MOUNTS)
                    && let Some(mounted) = mounted
                    && mounted.overlay.is_none()
                {
                    return Some(NodeLocation::Tree(Arc::new(Self {
                        tree: mounted.tree,
                        from: child_from,
                        child_index: Some(child_index),
                        parent: Some(Arc::clone(&parent)),
                    })));
                }

                let child = Arc::new(Self {
                    tree: child_tree.clone(),
                    from: child_from,
                    child_index: Some(child_index),
                    parent: Some(Arc::clone(&parent)),
                });
                if mode.contains(IterMode::INCLUDE_ANONYMOUS)
                    || !child.tree.node_type().is_anonymous()
                {
                    return Some(NodeLocation::Tree(child));
                }
                return child.next_child(
                    if direction < 0 {
                        signed_index(child.tree.children().len()) - 1
                    } else {
                        0
                    },
                    direction,
                    position,
                    side,
                    mode,
                );
            }

            if mode.contains(IterMode::INCLUDE_ANONYMOUS) || !parent.tree.node_type().is_anonymous()
            {
                return None;
            }
            let ancestor = parent.parent.clone()?;
            index = match parent.child_index {
                Some(child_index) => signed_index(child_index) + isize::from(direction),
                None if direction < 0 => -1,
                None => signed_index(ancestor.tree.children().len()),
            };
            parent = ancestor;
        }
    }

    fn enter(
        self: &Arc<Self>,
        position: TextSize,
        side: i8,
        mode: IterMode,
    ) -> Option<NodeLocation> {
        if !mode.contains(IterMode::IGNORE_OVERLAYS)
            && let Some(mounted) = self.tree.prop(mounted_prop())
            && let Some(overlay) = &mounted.overlay
            && let Some(relative) = position.checked_sub(self.from)
        {
            let enter_bracketed = mode.contains(IterMode::ENTER_BRACKETED) && mounted.bracketed;
            for range in overlay.iter() {
                let starts_before = if side > 0 || enter_bracketed {
                    range.start() <= relative
                } else {
                    range.start() < relative
                };
                let ends_after = if side < 0 || enter_bracketed {
                    range.end() >= relative
                } else {
                    range.end() > relative
                };
                if starts_before && ends_after {
                    return Some(NodeLocation::Tree(Arc::new(Self {
                        tree: mounted.tree,
                        from: self.from + overlay[0].start(),
                        child_index: None,
                        parent: Some(Arc::clone(self)),
                    })));
                }
            }
        }
        self.next_child(0, 1, position, side, mode)
    }
}

impl BufferNodeLocation {
    fn child(self: &Arc<Self>, direction: i8, position: TextSize, side: i8) -> Option<Arc<Self>> {
        let data = self.context.buffer.data();
        let index = find_buffer_child(
            &self.context.buffer,
            self.index + 4,
            usize::from(data[self.index + 3]),
            direction,
            position,
            self.context.start,
            side,
        )?;
        Some(Arc::new(Self {
            context: Arc::clone(&self.context),
            parent: Some(Arc::clone(self)),
            index,
        }))
    }

    fn next_sibling(self: &Arc<Self>, mode: IterMode) -> Option<NodeLocation> {
        let data = self.context.buffer.data();
        let end = self
            .parent
            .as_ref()
            .map_or(data.len(), |parent| usize::from(data[parent.index + 3]));
        let next = usize::from(data[self.index + 3]);
        if next < end {
            return Some(NodeLocation::Buffer(Arc::new(Self {
                context: Arc::clone(&self.context),
                parent: self.parent.clone(),
                index: next,
            })));
        }
        if self.parent.is_some() {
            return None;
        }
        self.context.parent.next_child(
            signed_index(self.context.child_index) + 1,
            1,
            TextSize::from(0),
            SIDE_DONT_CARE,
            mode,
        )
    }

    fn previous_sibling(self: &Arc<Self>, mode: IterMode) -> Option<NodeLocation> {
        let start = self.parent.as_ref().map_or(0, |parent| parent.index + 4);
        if self.index != start {
            let previous = find_buffer_child(
                &self.context.buffer,
                start,
                self.index,
                -1,
                TextSize::from(0),
                self.context.start,
                SIDE_DONT_CARE,
            )?;
            return Some(NodeLocation::Buffer(Arc::new(Self {
                context: Arc::clone(&self.context),
                parent: self.parent.clone(),
                index: previous,
            })));
        }
        if self.parent.is_some() {
            return None;
        }
        self.context.parent.next_child(
            signed_index(self.context.child_index) - 1,
            -1,
            TextSize::from(0),
            SIDE_DONT_CARE,
            mode,
        )
    }
}

fn find_buffer_child(
    buffer: &TreeBuffer,
    start: usize,
    end: usize,
    direction: i8,
    position: TextSize,
    buffer_start: TextSize,
    side: i8,
) -> Option<usize> {
    let mut index = start;
    let mut found = None;
    while index != end {
        let data = buffer.data();
        let from = buffer_start + TextSize::from(u32::from(data[index + 1]));
        let to = buffer_start + TextSize::from(u32::from(data[index + 2]));
        if check_side(side, position, from, to) {
            found = Some(index);
            if direction > 0 {
                break;
            }
        }
        index = usize::from(data[index + 3]);
    }
    found
}

fn has_visible_child(tree: &Tree) -> bool {
    tree.children().iter().any(|child| match child {
        TreeChild::Tree(child) => !child.node_type().is_anonymous() || has_visible_child(child),
        TreeChild::Buffer(_) => true,
    })
}

/// Stable immutable view of one syntax node.
#[derive(Clone)]
pub struct SyntaxNode {
    location: NodeLocation,
}

/// Lazy iterator over one syntax node's visible direct children.
///
/// Anonymous balancing and repetition nodes are flattened unless the node's
/// traversal mode includes [`IterMode::INCLUDE_ANONYMOUS`].
#[derive(Clone)]
pub struct SyntaxChildren {
    next: Option<SyntaxNode>,
}

impl fmt::Debug for SyntaxChildren {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyntaxChildren")
            .field("has_next", &self.next.is_some())
            .finish()
    }
}

impl SyntaxChildren {
    fn new(parent: &SyntaxNode) -> Self {
        let next = parent.first_child();
        Self { next }
    }
}

impl Iterator for SyntaxChildren {
    type Item = SyntaxNode;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.next.take()?;
        self.next = current.next_sibling();
        Some(current)
    }
}

impl fmt::Debug for SyntaxNode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyntaxNode")
            .field("name", &self.name())
            .field("from", &self.from())
            .field("to", &self.to())
            .finish_non_exhaustive()
    }
}

impl fmt::Display for SyntaxNode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_tree().render())
    }
}

impl SyntaxNode {
    /// Start byte.
    #[must_use]
    pub fn from(&self) -> TextSize {
        self.location.from()
    }

    /// End byte.
    #[must_use]
    pub fn to(&self) -> TextSize {
        self.location.to()
    }

    /// Half-open byte range.
    #[must_use]
    pub fn range(&self) -> TextRange {
        TextRange::new(self.from(), self.to())
    }

    /// Node type.
    #[must_use]
    pub fn node_type(&self) -> NodeType {
        self.location.node_type()
    }

    /// Node type name.
    #[must_use]
    pub fn name(&self) -> Arc<str> {
        self.location.node_type_ref().name_arc()
    }

    /// Visible parent.
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        self.location
            .visible_parent(IterMode::NONE)
            .map(|location| Self { location })
    }

    /// First visible child.
    #[must_use]
    pub fn first_child(&self) -> Option<Self> {
        self.location
            .first_child(IterMode::NONE)
            .map(|location| Self { location })
    }

    /// Last visible child.
    #[must_use]
    pub fn last_child(&self) -> Option<Self> {
        self.location
            .last_child(IterMode::NONE)
            .map(|location| Self { location })
    }

    /// First child ending after a position.
    #[must_use]
    pub fn child_after(&self, position: TextSize) -> Option<Self> {
        self.location
            .child_after(position, IterMode::NONE)
            .map(|location| Self { location })
    }

    /// Last child starting before a position.
    #[must_use]
    pub fn child_before(&self, position: TextSize) -> Option<Self> {
        self.location
            .child_before(position, IterMode::NONE)
            .map(|location| Self { location })
    }

    /// Next visible sibling.
    #[must_use]
    pub fn next_sibling(&self) -> Option<Self> {
        self.location
            .next_sibling(IterMode::NONE)
            .map(|location| Self { location })
    }

    /// Previous visible sibling.
    #[must_use]
    pub fn previous_sibling(&self) -> Option<Self> {
        self.location
            .previous_sibling(IterMode::NONE)
            .map(|location| Self { location })
    }

    /// Underlying stand-alone tree, if this node is not packed.
    #[must_use]
    pub fn tree(&self) -> Option<Tree> {
        match &self.location {
            NodeLocation::Tree(node) => Some(node.tree.clone()),
            NodeLocation::Buffer(_) => None,
        }
    }

    /// Read a per-node or type property.
    #[must_use]
    pub fn prop<T>(&self, property: NodeProp<T>) -> Option<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        if let Some(tree) = self.tree() {
            tree.prop(property)
        } else if property.is_per_node() {
            None
        } else {
            self.node_type().prop(property).cloned()
        }
    }

    /// Materialize this node as an independent tree.
    #[must_use]
    pub fn to_tree(&self) -> Tree {
        match &self.location {
            NodeLocation::Tree(node) => node.tree.clone(),
            NodeLocation::Buffer(node) => buffer_node_to_tree(&node.context.buffer, node.index),
        }
    }

    /// Cursor starting at this node.
    #[must_use]
    pub fn cursor(&self, mode: IterMode) -> TreeCursor {
        TreeCursor::new(self, mode)
    }

    /// Resolve inside or around this node.
    #[must_use]
    pub fn resolve(&self, position: TextSize, side: i8) -> Self {
        resolve_node(self.clone(), position, side, false)
    }

    /// Resolve while entering mounted overlays.
    #[must_use]
    pub fn resolve_inner(&self, position: TextSize, side: i8) -> Self {
        resolve_node(self.clone(), position, side, true)
    }

    /// Return the first direct child matching a name/group/id.
    #[must_use]
    pub fn child_by_name(&self, name: &str) -> Option<Self> {
        self.children()
            .find(|child| child.node_type().is_name(name))
    }

    /// Return all direct children matching a name/group.
    #[must_use]
    pub fn children_by_name(&self, name: &str) -> Vec<Self> {
        self.children()
            .filter(|child| child.node_type().is_name(name))
            .collect()
    }

    /// Match a direct-parent context, using empty strings as wildcards.
    #[must_use]
    pub fn matches_context<T>(&self, context: &[T]) -> bool
    where
        T: AsRef<str>,
    {
        let mut parent = self.parent();
        for expected in context.iter().rev() {
            let Some(current) = parent else {
                return false;
            };
            let expected = expected.as_ref();
            if !expected.is_empty() && expected != current.node_type().name() {
                return false;
            }
            parent = current.parent();
        }
        true
    }

    /// Iterate over visible direct children without materializing an
    /// intermediate collection.
    #[must_use]
    pub fn children(&self) -> SyntaxChildren {
        SyntaxChildren::new(self)
    }

    fn enter(&self, position: TextSize, side: i8, mode: IterMode) -> Option<Self> {
        self.location
            .enter(position, side, mode)
            .map(|location| Self { location })
    }
}

fn syntax_root(tree: Tree, from: TextSize) -> SyntaxNode {
    SyntaxNode {
        location: NodeLocation::Tree(Arc::new(TreeNodeLocation {
            tree,
            from,
            child_index: None,
            parent: None,
        })),
    }
}

fn resolve_node(mut node: SyntaxNode, position: TextSize, side: i8, overlays: bool) -> SyntaxNode {
    while !covers(node.from(), node.to(), position, side) {
        if !overlays && node.location.is_overlay_root() {
            return node;
        }
        let Some(parent) = node.parent() else {
            return node;
        };
        node = parent;
    }

    let mode = if overlays {
        IterMode::NONE
    } else {
        IterMode::IGNORE_OVERLAYS
    };
    if overlays {
        let mut scan = node.clone();
        while let Some(parent) = scan.parent() {
            if scan.location.is_overlay_root()
                && parent
                    .enter(position, side, mode)
                    .is_none_or(|inner| inner.from() != scan.from())
            {
                node = parent.clone();
            }
            scan = parent;
        }
    }
    loop {
        let next = node.enter(position, side, mode);
        let Some(child) = next else {
            return node;
        };
        node = child;
    }
}

fn covers(from: TextSize, to: TextSize, position: TextSize, side: i8) -> bool {
    check_side(side, position, from, to)
}

fn check_side(side: i8, position: TextSize, from: TextSize, to: TextSize) -> bool {
    match side {
        SIDE_BEFORE => from < position,
        SIDE_AT_OR_BEFORE => to >= position && from < position,
        SIDE_AROUND => from < position && to > position,
        SIDE_AT_OR_AFTER => from <= position && to > position,
        SIDE_AFTER => to > position,
        SIDE_DONT_CARE => true,
        _ => false,
    }
}

fn materialize_buffer_node(buffer: &TreeBuffer, index: usize) -> Tree {
    let data = buffer.data();
    let node_type = buffer
        .node_set()
        .get(data[index])
        .expect("validated tree-buffer type")
        .clone();
    let from = TextSize::from(u32::from(data[index + 1]));
    let to = TextSize::from(u32::from(data[index + 2]));
    let end = usize::from(data[index + 3]);
    let mut children = Vec::new();
    let mut positions = Vec::new();
    let mut child = index + 4;
    while child < end {
        positions.push(TextSize::from(u32::from(data[child + 1])) - from);
        children.push(TreeChild::Tree(materialize_buffer_node(buffer, child)));
        child = usize::from(data[child + 3]);
    }
    Tree::new(node_type, children, positions, to - from)
}

fn buffer_node_to_tree(buffer: &TreeBuffer, index: usize) -> Tree {
    let data = buffer.data();
    let node_type = buffer_node_type(buffer, index).clone();
    let from = TextSize::from(u32::from(data[index + 1]));
    let to = TextSize::from(u32::from(data[index + 2]));
    let start = index + 4;
    let end = usize::from(data[index + 3]);
    let mut children = Vec::new();
    let mut positions = Vec::new();
    if start < end {
        children.push(TreeChild::Buffer(buffer.slice(start, end, from)));
        positions.push(TextSize::from(0));
    }
    Tree::new(node_type, children, positions, to - from)
}

struct SplitBufferPath {
    tree: Tree,
    target: Tree,
    child_path: Vec<usize>,
}

fn push_buffer_slice(
    buffer: &TreeBuffer,
    start: usize,
    end: usize,
    children: &mut Vec<TreeChild>,
    positions: &mut Vec<TextSize>,
    offset: TextSize,
) {
    if start >= end {
        return;
    }
    let from = TextSize::from(u32::from(buffer.data()[start + 1]));
    children.push(TreeChild::Buffer(buffer.slice(start, end, from)));
    positions.push(from - offset);
}

#[allow(clippy::too_many_arguments)]
fn split_buffer_path(
    buffer: &TreeBuffer,
    record_path: &[usize],
    depth: usize,
    start: usize,
    end: usize,
    node_type: NodeType,
    offset: TextSize,
    length: TextSize,
) -> SplitBufferPath {
    let target_index = record_path[depth];
    assert!(
        target_index >= start && target_index < end,
        "buffer record path leaves its enclosing node"
    );

    let data = buffer.data();
    let mut children = Vec::new();
    let mut positions = Vec::new();
    push_buffer_slice(
        buffer,
        start,
        target_index,
        &mut children,
        &mut positions,
        offset,
    );

    let from = TextSize::from(u32::from(data[target_index + 1]));
    let to = TextSize::from(u32::from(data[target_index + 2]));
    let child_index = children.len();
    let (child, target, mut child_path) = if depth + 1 == record_path.len() {
        let target = buffer_node_to_tree(buffer, target_index);
        (
            target.clone(),
            target,
            Vec::with_capacity(record_path.len()),
        )
    } else {
        let nested = split_buffer_path(
            buffer,
            record_path,
            depth + 1,
            target_index + 4,
            usize::from(data[target_index + 3]),
            buffer_node_type(buffer, target_index).clone(),
            from,
            to - from,
        );
        (nested.tree, nested.target, nested.child_path)
    };

    children.push(TreeChild::Tree(child));
    positions.push(from - offset);
    push_buffer_slice(
        buffer,
        usize::from(data[target_index + 3]),
        end,
        &mut children,
        &mut positions,
        offset,
    );

    child_path.push(child_index);
    SplitBufferPath {
        tree: Tree::new(node_type, children, positions, length),
        target,
        child_path,
    }
}

#[derive(Clone, Debug)]
struct CursorTreeFrame {
    tree: Tree,
    from: TextSize,
    child_index: Option<usize>,
}

impl CursorTreeFrame {
    fn from_location(node: &TreeNodeLocation) -> Self {
        Self {
            tree: node.tree.clone(),
            from: node.from,
            child_index: node.child_index,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct CursorNodeCache {
    tree_nodes: Option<Arc<Vec<Arc<TreeNodeLocation>>>>,
    buffer_node: Option<Arc<BufferNodeLocation>>,
}

/// Mutable cursor over a compact syntax tree.
#[derive(Debug)]
pub struct TreeCursor {
    tree_path: Arc<Vec<CursorTreeFrame>>,
    buffer: Option<Arc<BufferContext>>,
    stack: Arc<Vec<usize>>,
    index: usize,
    mode: IterMode,
    has_tree_nodes: AtomicBool,
    node_cache: Mutex<CursorNodeCache>,
}

impl Clone for TreeCursor {
    fn clone(&self) -> Self {
        let node_cache = self
            .node_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        Self {
            tree_path: Arc::clone(&self.tree_path),
            buffer: self.buffer.as_ref().map(Arc::clone),
            stack: Arc::clone(&self.stack),
            index: self.index,
            mode: self.mode,
            has_tree_nodes: AtomicBool::new(self.has_tree_nodes.load(Ordering::Relaxed)),
            node_cache: Mutex::new(node_cache),
        }
    }
}

impl TreeCursor {
    fn from_tree(tree: Tree, from: TextSize, mode: IterMode) -> Self {
        let mode = IterMode(mode.0 & !IterMode::ENTER_BRACKETED.0);
        Self {
            tree_path: Arc::new(vec![CursorTreeFrame {
                tree,
                from,
                child_index: None,
            }]),
            buffer: None,
            stack: Arc::new(Vec::new()),
            index: 0,
            mode,
            has_tree_nodes: AtomicBool::new(false),
            node_cache: Mutex::new(CursorNodeCache::default()),
        }
    }

    fn new(node: &SyntaxNode, mode: IterMode) -> Self {
        let mode = IterMode(mode.0 & !IterMode::ENTER_BRACKETED.0);
        match &node.location {
            NodeLocation::Tree(tree) => Self::from_tree_node(tree, mode),
            NodeLocation::Buffer(node) => Self::from_buffer_node(node, mode),
        }
    }

    fn from_tree_node(node: &Arc<TreeNodeLocation>, mode: IterMode) -> Self {
        let tree_path = Arc::new(tree_path_for(node));
        let tree_nodes = Arc::new(tree_nodes_for(node));
        Self {
            tree_path,
            buffer: None,
            stack: Arc::new(Vec::new()),
            index: 0,
            mode,
            has_tree_nodes: AtomicBool::new(true),
            node_cache: Mutex::new(CursorNodeCache {
                tree_nodes: Some(tree_nodes),
                buffer_node: None,
            }),
        }
    }

    fn from_buffer_node(node: &Arc<BufferNodeLocation>, mode: IterMode) -> Self {
        let mut stack = Vec::new();
        let mut parent = node.parent.clone();
        while let Some(current) = parent.take() {
            stack.push(current.index);
            parent = current.parent.as_ref().map(Arc::clone);
        }
        stack.reverse();
        let tree_path = Arc::new(tree_path_for(&node.context.parent));
        let tree_nodes = Arc::new(tree_nodes_for(&node.context.parent));
        Self {
            tree_path,
            buffer: Some(Arc::clone(&node.context)),
            stack: Arc::new(stack),
            index: node.index,
            mode,
            has_tree_nodes: AtomicBool::new(true),
            node_cache: Mutex::new(CursorNodeCache {
                tree_nodes: Some(tree_nodes),
                buffer_node: Some(Arc::clone(node)),
            }),
        }
    }

    fn current_tree_frame(&self) -> &CursorTreeFrame {
        self.tree_path
            .last()
            .expect("tree cursor always has a tree frame")
    }

    fn push_tree_frame(&mut self, frame: CursorTreeFrame) {
        self.push_tree_frame_with_node(frame, None);
    }

    fn push_tree_frame_with_node(
        &mut self,
        frame: CursorTreeFrame,
        existing_node: Option<Arc<TreeNodeLocation>>,
    ) {
        if self.has_tree_nodes.load(Ordering::Relaxed) {
            let mut cache = self
                .node_cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(nodes) = cache.tree_nodes.as_mut() {
                let nodes = Arc::make_mut(nodes);
                if nodes.len() == self.tree_path.len() {
                    let node = existing_node.unwrap_or_else(|| {
                        Arc::new(TreeNodeLocation {
                            tree: frame.tree.clone(),
                            from: frame.from,
                            child_index: frame.child_index,
                            parent: nodes.last().cloned(),
                        })
                    });
                    nodes.push(node);
                } else {
                    cache.tree_nodes = None;
                    self.has_tree_nodes.store(false, Ordering::Relaxed);
                }
            }
        }
        Arc::make_mut(&mut self.tree_path).push(frame);
    }

    fn cached_current_tree_node(&self) -> Option<Arc<TreeNodeLocation>> {
        if !self.has_tree_nodes.load(Ordering::Relaxed) {
            return None;
        }
        let cache = self
            .node_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.tree_nodes.as_ref().and_then(|nodes| {
            (nodes.len() == self.tree_path.len()).then(|| {
                nodes
                    .last()
                    .expect("tree cursor always has a tree frame")
                    .clone()
            })
        })
    }

    fn pop_tree_frame(&mut self) -> Option<CursorTreeFrame> {
        let frame = Arc::make_mut(&mut self.tree_path).pop();
        if frame.is_some() {
            self.truncate_cached_tree_nodes(self.tree_path.len());
        }
        frame
    }

    fn pop_tree_frame_with_node(
        &mut self,
    ) -> Option<(CursorTreeFrame, Option<Arc<TreeNodeLocation>>)> {
        let node = self.cached_current_tree_node();
        let frame = self.pop_tree_frame()?;
        Some((frame, node))
    }

    fn truncate_tree_path(&mut self, length: usize) {
        if self.tree_path.len() > length {
            Arc::make_mut(&mut self.tree_path).truncate(length);
            self.truncate_cached_tree_nodes(length);
        }
    }

    fn truncate_cached_tree_nodes(&self, length: usize) {
        if !self.has_tree_nodes.load(Ordering::Relaxed) {
            return;
        }
        let mut cache = self
            .node_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(nodes) = cache.tree_nodes.as_mut() {
            if nodes.len() >= length {
                Arc::make_mut(nodes).truncate(length);
            } else {
                cache.tree_nodes = None;
                self.has_tree_nodes.store(false, Ordering::Relaxed);
            }
        }
    }

    fn leave_buffer(&mut self) {
        if self.buffer.take().is_some() {
            self.stack = Arc::new(Vec::new());
        }
    }

    fn enter_buffer(&mut self, context: Arc<BufferContext>, index: usize) -> bool {
        self.buffer = Some(context);
        self.stack = Arc::new(Vec::new());
        self.index = index;
        true
    }

    fn yield_buffer(&mut self, index: usize) -> bool {
        self.index = index;
        true
    }

    fn current_tree_node(&self) -> Arc<TreeNodeLocation> {
        let mut cache = self
            .node_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(nodes) = cache.tree_nodes.as_ref()
            && nodes.len() == self.tree_path.len()
        {
            return Arc::clone(nodes.last().expect("tree cursor always has a tree frame"));
        }

        let mut parent = None;
        let mut nodes = Vec::with_capacity(self.tree_path.len());
        for frame in self.tree_path.iter() {
            let node = Arc::new(TreeNodeLocation {
                tree: frame.tree.clone(),
                from: frame.from,
                child_index: frame.child_index,
                parent,
            });
            parent = Some(Arc::clone(&node));
            nodes.push(node);
        }
        let node = parent.expect("tree cursor always has a tree frame");
        cache.tree_nodes = Some(Arc::new(nodes));
        self.has_tree_nodes.store(true, Ordering::Relaxed);
        node
    }

    fn current_buffer_node(&self) -> Arc<BufferNodeLocation> {
        let context = self.buffer.as_ref().expect("cursor is in a buffer");
        let mut cache = self
            .node_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut parent = None;
        let mut depth = 0;

        if let Some(cached) = cache.buffer_node.as_ref()
            && Arc::ptr_eq(&cached.context, context)
        {
            let mut index = self.index;
            let mut stack_depth = self.stack.len();
            'find_cached_parent: loop {
                let mut candidate = Some(Arc::clone(cached));
                while let Some(current) = candidate.take() {
                    if current.index == index {
                        if index == self.index {
                            return current;
                        }
                        parent = Some(current);
                        depth = stack_depth + 1;
                        break 'find_cached_parent;
                    }
                    candidate = current.parent.as_ref().map(Arc::clone);
                }
                if stack_depth == 0 {
                    break;
                }
                stack_depth -= 1;
                index = self.stack[stack_depth];
            }
        }

        for index in self.stack.iter().skip(depth) {
            parent = Some(Arc::new(BufferNodeLocation {
                context: Arc::clone(context),
                parent,
                index: *index,
            }));
        }
        let result = Arc::new(BufferNodeLocation {
            context: Arc::clone(context),
            parent,
            index: self.index,
        });
        cache.buffer_node = Some(Arc::clone(&result));
        result
    }

    /// Current node.
    #[must_use]
    pub fn node(&self) -> SyntaxNode {
        let location = match &self.buffer {
            Some(_) => NodeLocation::Buffer(self.current_buffer_node()),
            None => NodeLocation::Tree(self.current_tree_node()),
        };
        SyntaxNode { location }
    }

    /// Underlying stand-alone tree, when the cursor is not in a packed buffer.
    #[must_use]
    pub fn tree(&self) -> Option<Tree> {
        if self.buffer.is_some() {
            None
        } else {
            Some(self.current_tree_frame().tree.clone())
        }
    }

    pub(crate) fn root_tree(&self) -> Tree {
        self.tree_path
            .first()
            .expect("tree cursor always has a root frame")
            .tree
            .clone()
    }

    pub(crate) fn materialize_current<F>(&mut self, mut replace: F) -> Tree
    where
        F: FnMut(&Tree, &Tree),
    {
        let Some(context) = self.buffer.clone() else {
            return self.current_tree_frame().tree.clone();
        };

        let mut record_path = Vec::with_capacity(self.stack.len() + 1);
        record_path.extend(self.stack.iter().copied());
        record_path.push(self.index);

        let SplitBufferPath {
            tree: split_tree,
            target,
            mut child_path,
        } = split_buffer_path(
            &context.buffer,
            &record_path,
            0,
            0,
            context.buffer.data().len(),
            NodeType::none(),
            TextSize::from(0),
            context.buffer.len(),
        );

        let old_path = Arc::clone(&self.tree_path);
        let last = old_path.len() - 1;
        let mut rebuilt_path = old_path.as_ref().clone();
        let mut replacements = Vec::with_capacity(old_path.len());

        let mut replacement = old_path[last]
            .tree
            .with_replaced_child(context.child_index, TreeChild::Tree(split_tree.clone()));
        replacements.push((old_path[last].tree.clone(), replacement.clone()));
        rebuilt_path[last].tree = replacement.clone();

        for index in (0..last).rev() {
            replacement =
                self.replace_path_child(&old_path[index].tree, &old_path[index + 1], replacement);
            replacements.push((old_path[index].tree.clone(), replacement.clone()));
            rebuilt_path[index].tree = replacement.clone();
        }

        let mut tree_path = rebuilt_path;
        let mut current_tree = split_tree;
        let mut current_from = context.start;
        tree_path.push(CursorTreeFrame {
            tree: current_tree.clone(),
            from: current_from,
            child_index: Some(context.child_index),
        });
        child_path.reverse();
        for index in child_path {
            let child_from = current_from + current_tree.positions()[index];
            let TreeChild::Tree(child) = &current_tree.children()[index] else {
                unreachable!("split buffer path always descends through trees");
            };
            current_tree = child.clone();
            current_from = child_from;
            tree_path.push(CursorTreeFrame {
                tree: current_tree.clone(),
                from: current_from,
                child_index: Some(index),
            });
        }
        debug_assert!(target.same_identity(&current_tree));

        self.tree_path = Arc::new(tree_path);
        self.buffer = None;
        self.stack = Arc::new(Vec::new());
        self.index = 0;
        self.has_tree_nodes.store(false, Ordering::Relaxed);
        let mut cache = self
            .node_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.tree_nodes = None;
        cache.buffer_node = None;
        drop(cache);

        for (old, new) in replacements {
            replace(&old, &new);
        }
        target
    }

    /// Current node type.
    #[must_use]
    pub fn node_type(&self) -> &NodeType {
        match &self.buffer {
            Some(context) => buffer_node_type(&context.buffer, self.index),
            None => self.current_tree_frame().tree.node_type(),
        }
    }

    /// Current node name.
    #[must_use]
    pub fn name(&self) -> Arc<str> {
        self.node_type().name_arc()
    }

    /// Current start byte.
    #[must_use]
    pub fn from(&self) -> TextSize {
        match &self.buffer {
            Some(context) => {
                context.start + TextSize::from(u32::from(context.buffer.data()[self.index + 1]))
            }
            None => self.current_tree_frame().from,
        }
    }

    /// Current end byte.
    #[must_use]
    pub fn to(&self) -> TextSize {
        if let Some(context) = &self.buffer {
            return context.start
                + TextSize::from(u32::from(context.buffer.data()[self.index + 2]));
        }
        let frame = self.current_tree_frame();
        frame.from + frame.tree.len()
    }

    /// Move to the first visible child.
    pub fn first_child(&mut self) -> bool {
        self.enter_child(1, TextSize::from(0), SIDE_DONT_CARE)
    }

    /// Move to the last visible child.
    pub fn last_child(&mut self) -> bool {
        self.enter_child(-1, TextSize::from(0), SIDE_DONT_CARE)
    }

    /// Move to the first child ending after a position.
    pub fn child_after(&mut self, position: TextSize) -> bool {
        self.enter_child(1, position, SIDE_AFTER)
    }

    /// Move to the last child starting before a position.
    pub fn child_before(&mut self, position: TextSize) -> bool {
        self.enter_child(-1, position, SIDE_BEFORE)
    }

    /// Enter the child covering a position.
    pub fn enter(&mut self, position: TextSize, side: i8) -> bool {
        if self.buffer.is_some() {
            if self.mode.contains(IterMode::EXCLUDE_BUFFERS) {
                return false;
            }
            return self.enter_child(1, position, side);
        }
        self.enter_tree(position, side)
    }

    /// Move to the visible parent.
    pub fn parent(&mut self) -> bool {
        if self.buffer.is_some() {
            if let Some(index) = Arc::make_mut(&mut self.stack).pop() {
                return self.yield_buffer(index);
            }
            let mut target = self.tree_path.len() - 1;
            if !self.mode.contains(IterMode::INCLUDE_ANONYMOUS) {
                while target > 0 && self.tree_path[target].tree.node_type().is_anonymous() {
                    target -= 1;
                }
            }
            self.truncate_tree_path(target + 1);
            self.leave_buffer();
            return true;
        }

        let length = self.tree_path.len();
        if length == 1 {
            return false;
        }
        let mut target = length - 2;
        if !self.mode.contains(IterMode::INCLUDE_ANONYMOUS) {
            while target > 0 && self.tree_path[target].tree.node_type().is_anonymous() {
                target -= 1;
            }
        }
        self.truncate_tree_path(target + 1);
        true
    }

    /// Move to the next visible sibling.
    pub fn next_sibling(&mut self) -> bool {
        self.sibling(1)
    }

    /// Move to the previous visible sibling.
    pub fn previous_sibling(&mut self) -> bool {
        self.sibling(-1)
    }

    /// Advance in pre-order.
    pub fn next(&mut self, enter: bool) -> bool {
        self.move_cursor(1, enter)
    }

    /// Advance in reverse pre-order.
    pub fn previous(&mut self, enter: bool) -> bool {
        self.move_cursor(-1, enter)
    }

    /// Move to the innermost node covering a position.
    pub fn move_to(&mut self, position: TextSize, side: i8) -> &mut Self {
        while !covers(self.from(), self.to(), position, side) {
            if !self.parent() {
                break;
            }
        }
        while self.enter_child(1, position, side) {}
        self
    }

    /// Match direct parent context.
    #[must_use]
    pub fn matches_context<T>(&self, context: &[T]) -> bool
    where
        T: AsRef<str>,
    {
        let Some(buffer) = self.buffer.as_ref() else {
            return matches_tree_path_context(
                &self.tree_path,
                self.tree_path.len() - 1,
                context,
                context.len(),
            );
        };

        let mut remaining = context.len();
        for index in self.stack.iter().rev() {
            let node_type = buffer_node_type(&buffer.buffer, *index);
            if node_type.is_anonymous() {
                continue;
            }
            if remaining == 0 {
                return true;
            }
            remaining -= 1;
            let expected = context[remaining].as_ref();
            if !expected.is_empty() && expected != node_type.name() {
                return false;
            }
        }
        matches_tree_path_context(&self.tree_path, self.tree_path.len(), context, remaining)
    }

    fn replace_path_child(
        &self,
        parent: &Tree,
        child: &CursorTreeFrame,
        replacement: Tree,
    ) -> Tree {
        let Some(index) = child.child_index else {
            let Some(mut mounted) = parent.prop(mounted_prop()) else {
                unreachable!("overlay cursor frame has a mounted parent");
            };
            debug_assert!(mounted.overlay.is_some());
            debug_assert!(mounted.tree.same_identity(&child.tree));
            mounted.tree = replacement;
            let parent = parent.copy_with_children(parent.children().to_vec());
            parent.set_prop(mounted_prop(), mounted);
            return parent;
        };

        let TreeChild::Tree(host_child) = &parent.children()[index] else {
            unreachable!("tree cursor frames always point at tree children");
        };
        if !self.mode.contains(IterMode::IGNORE_MOUNTS)
            && let Some(mut mounted) = host_child.prop(mounted_prop())
            && mounted.overlay.is_none()
            && mounted.tree.same_identity(&child.tree)
        {
            mounted.tree = replacement;
            let host_child = host_child.copy_with_children(host_child.children().to_vec());
            host_child.set_prop(mounted_prop(), mounted);
            return parent.with_replaced_child(index, TreeChild::Tree(host_child));
        }
        parent.with_replaced_child(index, TreeChild::Tree(replacement))
    }

    fn enter_child(&mut self, direction: i8, position: TextSize, side: i8) -> bool {
        if let Some(buffer) = self.buffer.clone() {
            let data = buffer.buffer.data();
            let Some(index) = find_buffer_child(
                &buffer.buffer,
                self.index + 4,
                usize::from(data[self.index + 3]),
                direction,
                position,
                buffer.start,
                side,
            ) else {
                return false;
            };
            Arc::make_mut(&mut self.stack).push(self.index);
            return self.yield_buffer(index);
        }
        self.enter_tree_child(direction, position, side)
    }

    fn enter_tree(&mut self, position: TextSize, side: i8) -> bool {
        let frame = self.current_tree_frame().clone();
        if !self.mode.contains(IterMode::IGNORE_OVERLAYS)
            && let Some(mounted) = frame.tree.prop(mounted_prop())
            && let Some(overlay) = mounted.overlay.as_ref()
            && let Some(relative) = position.checked_sub(frame.from)
        {
            let enter_bracketed =
                self.mode.contains(IterMode::ENTER_BRACKETED) && mounted.bracketed;
            for range in overlay.iter() {
                let starts_before = if side > 0 || enter_bracketed {
                    range.start() <= relative
                } else {
                    range.start() < relative
                };
                let ends_after = if side < 0 || enter_bracketed {
                    range.end() >= relative
                } else {
                    range.end() > relative
                };
                if starts_before && ends_after {
                    self.push_tree_frame(CursorTreeFrame {
                        tree: mounted.tree.clone(),
                        from: frame.from + overlay[0].start(),
                        child_index: None,
                    });
                    return true;
                }
            }
        }
        self.enter_tree_child(1, position, side)
    }

    fn enter_tree_child(&mut self, direction: i8, position: TextSize, side: i8) -> bool {
        let start = if direction < 0 {
            signed_index(self.current_tree_frame().tree.children().len()) - 1
        } else {
            0
        };
        self.find_tree_child(start, direction, position, side)
    }

    #[allow(clippy::too_many_lines)]
    fn find_tree_child(
        &mut self,
        mut index: isize,
        direction: i8,
        position: TextSize,
        side: i8,
    ) -> bool {
        let original_length = self.tree_path.len();
        let mut removed = Vec::new();

        'search: loop {
            let child_count = self.current_tree_frame().tree.children().len();
            let end = if direction > 0 {
                signed_index(child_count)
            } else {
                -1
            };
            while index != end {
                let child_index = unsigned_index(index);
                let (child, child_from) = {
                    let parent = self.current_tree_frame();
                    (
                        parent.tree.children()[child_index].clone(),
                        parent.from + parent.tree.positions()[child_index],
                    )
                };
                index += isize::from(direction);

                let TreeChild::Tree(child_tree) = child else {
                    if self.mode.contains(IterMode::EXCLUDE_BUFFERS) {
                        continue;
                    }
                    let TreeChild::Buffer(buffer) = child else {
                        unreachable!();
                    };
                    if !check_side(side, position, child_from, child_from + buffer.len()) {
                        continue;
                    }
                    let Some(record) = find_buffer_child(
                        &buffer,
                        0,
                        buffer.data().len(),
                        direction,
                        position,
                        child_from,
                        side,
                    ) else {
                        continue;
                    };
                    let context = Arc::new(BufferContext {
                        parent: self.current_tree_node(),
                        buffer,
                        child_index,
                        start: child_from,
                    });
                    return self.enter_buffer(context, record);
                };

                let mounted = child_tree.prop(mounted_prop());
                let enters_bracketed = self.mode.contains(IterMode::ENTER_BRACKETED)
                    && mounted.as_ref().is_some_and(|mounted| {
                        mounted.overlay.is_none()
                            && mounted.bracketed
                            && position >= child_from
                            && position <= child_from + child_tree.len()
                    });
                if !enters_bracketed
                    && !check_side(side, position, child_from, child_from + child_tree.len())
                {
                    continue;
                }
                if !self.mode.contains(IterMode::INCLUDE_ANONYMOUS)
                    && child_tree.node_type().is_anonymous()
                    && !has_visible_child(&child_tree)
                {
                    continue;
                }

                if !self.mode.contains(IterMode::IGNORE_MOUNTS)
                    && let Some(mounted) = mounted
                    && mounted.overlay.is_none()
                {
                    self.push_tree_frame(CursorTreeFrame {
                        tree: mounted.tree,
                        from: child_from,
                        child_index: Some(child_index),
                    });
                    self.leave_buffer();
                    return true;
                }

                let frame = CursorTreeFrame {
                    tree: child_tree,
                    from: child_from,
                    child_index: Some(child_index),
                };
                if self.mode.contains(IterMode::INCLUDE_ANONYMOUS)
                    || !frame.tree.node_type().is_anonymous()
                {
                    self.push_tree_frame(frame);
                    self.leave_buffer();
                    return true;
                }
                index = if direction < 0 {
                    signed_index(frame.tree.children().len()) - 1
                } else {
                    0
                };
                self.push_tree_frame(frame);
                continue 'search;
            }

            if self.mode.contains(IterMode::INCLUDE_ANONYMOUS)
                || !self.current_tree_frame().tree.node_type().is_anonymous()
            {
                self.restore_tree_path(original_length, removed);
                return false;
            }

            let previous_length = self.tree_path.len();
            let (frame, cached_node) = self
                .pop_tree_frame_with_node()
                .expect("tree cursor always has a tree frame");
            let child_index = frame.child_index;
            if previous_length <= original_length {
                removed.push((frame, cached_node));
            }
            if self.tree_path.is_empty() {
                self.restore_tree_path(original_length, removed);
                return false;
            }
            index = match child_index {
                Some(child_index) => signed_index(child_index) + isize::from(direction),
                None if direction < 0 => -1,
                None => signed_index(self.current_tree_frame().tree.children().len()),
            };
        }
    }

    fn restore_tree_path(
        &mut self,
        original_length: usize,
        removed: Vec<(CursorTreeFrame, Option<Arc<TreeNodeLocation>>)>,
    ) {
        self.truncate_tree_path(original_length);
        for (frame, node) in removed.into_iter().rev() {
            self.push_tree_frame_with_node(frame, node);
        }
    }

    fn sibling(&mut self, direction: i8) -> bool {
        if let Some(buffer) = self.buffer.clone() {
            let data = buffer.buffer.data();
            if direction < 0 {
                let parent_start = self.stack.last().map_or(0, |index| index + 4);
                if self.index != parent_start {
                    let Some(previous) = find_buffer_child(
                        &buffer.buffer,
                        parent_start,
                        self.index,
                        -1,
                        TextSize::from(0),
                        buffer.start,
                        SIDE_DONT_CARE,
                    ) else {
                        return false;
                    };
                    return self.yield_buffer(previous);
                }
            } else {
                let end = self
                    .stack
                    .last()
                    .map_or(data.len(), |index| usize::from(data[index + 3]));
                let next = usize::from(data[self.index + 3]);
                if next < end {
                    return self.yield_buffer(next);
                }
            }
            if self.stack.is_empty() {
                return self.find_tree_child(
                    signed_index(buffer.child_index) + isize::from(direction),
                    direction,
                    TextSize::from(0),
                    SIDE_DONT_CARE,
                );
            }
            return false;
        }

        let Some(index) = self.current_tree_frame().child_index else {
            return false;
        };
        if self.tree_path.len() == 1 {
            return false;
        }
        let cached_node = self.cached_current_tree_node();
        let frame = self
            .pop_tree_frame()
            .expect("tree cursor always has a tree frame");
        let found = self.find_tree_child(
            signed_index(index) + isize::from(direction),
            direction,
            TextSize::from(0),
            SIDE_DONT_CARE,
        );
        if !found {
            self.push_tree_frame_with_node(frame, cached_node);
        }
        found
    }

    fn move_cursor(&mut self, direction: i8, enter: bool) -> bool {
        if enter && self.enter_child(direction, TextSize::from(0), SIDE_DONT_CARE) {
            return true;
        }
        loop {
            if self.sibling(direction) {
                return true;
            }
            if self.at_last_node(direction) || !self.parent() {
                return false;
            }
        }
    }

    fn at_last_node(&self, direction: i8) -> bool {
        let (mut index, mut frame_index) = if let Some(buffer) = self.buffer.as_ref() {
            let data = buffer.buffer.data();
            let mut current = self.index;
            let mut parent_start = self.stack.last().map_or(0, |index| index + 4);
            let mut parent_end = self
                .stack
                .last()
                .map_or(data.len(), |index| usize::from(data[index + 3]));
            if has_buffer_sibling(data, current, parent_start, parent_end, direction) {
                return false;
            }
            for depth in (0..self.stack.len()).rev() {
                current = self.stack[depth];
                parent_start = if depth == 0 {
                    0
                } else {
                    self.stack[depth - 1] + 4
                };
                parent_end = if depth == 0 {
                    data.len()
                } else {
                    usize::from(data[self.stack[depth - 1] + 3])
                };
                if has_buffer_sibling(data, current, parent_start, parent_end, direction) {
                    return false;
                }
            }
            (signed_index(buffer.child_index), self.tree_path.len() - 1)
        } else {
            let current = self.current_tree_frame();
            let Some(parent_index) = self.tree_path.len().checked_sub(2) else {
                return true;
            };
            (current.child_index.map_or(-1, signed_index), parent_index)
        };

        loop {
            let current = &self.tree_path[frame_index];
            if index > -1 {
                let mut sibling = index + isize::from(direction);
                let end = if direction < 0 {
                    -1
                } else {
                    signed_index(current.tree.children().len())
                };
                while sibling != end {
                    let child = &current.tree.children()[unsigned_index(sibling)];
                    if is_visible_cursor_child(child, self.mode) {
                        return false;
                    }
                    sibling += isize::from(direction);
                }
            }
            if frame_index == 0 {
                return true;
            }
            index = current.child_index.map_or(-1, signed_index);
            frame_index -= 1;
        }
    }
}

fn has_buffer_sibling(
    data: &[u16],
    index: usize,
    parent_start: usize,
    parent_end: usize,
    direction: i8,
) -> bool {
    if direction > 0 {
        return usize::from(data[index + 3]) < parent_end;
    }
    index != parent_start
}

fn is_visible_cursor_child(child: &TreeChild, mode: IterMode) -> bool {
    match child {
        TreeChild::Buffer(_) => !mode.contains(IterMode::EXCLUDE_BUFFERS),
        TreeChild::Tree(tree) => {
            mode.contains(IterMode::INCLUDE_ANONYMOUS)
                || !tree.node_type().is_anonymous()
                || has_visible_child(tree)
        }
    }
}

fn tree_path_for(node: &Arc<TreeNodeLocation>) -> Vec<CursorTreeFrame> {
    let mut path = Vec::new();
    let mut current = Some(Arc::clone(node));
    while let Some(node) = current.take() {
        path.push(CursorTreeFrame::from_location(&node));
        current = node.parent.as_ref().map(Arc::clone);
    }
    path.reverse();
    path
}

fn tree_nodes_for(node: &Arc<TreeNodeLocation>) -> Vec<Arc<TreeNodeLocation>> {
    let mut nodes = Vec::new();
    let mut current = Some(Arc::clone(node));
    while let Some(node) = current.take() {
        current = node.parent.as_ref().map(Arc::clone);
        nodes.push(node);
    }
    nodes.reverse();
    nodes
}

fn matches_tree_path_context<T>(
    path: &[CursorTreeFrame],
    mut end: usize,
    context: &[T],
    mut remaining: usize,
) -> bool
where
    T: AsRef<str>,
{
    while remaining > 0 {
        if end == 0 {
            return false;
        }
        end -= 1;
        let current = &path[end];
        if !current.tree.node_type().is_anonymous() {
            remaining -= 1;
            let expected = context[remaining].as_ref();
            if !expected.is_empty() && expected != current.tree.node_type().name() {
                return false;
            }
        }
    }
    true
}
