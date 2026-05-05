use std::any::Any;
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::{MountedTree, NodeType};

const CLOSED_BY_ID: u32 = 0;
const OPENED_BY_ID: u32 = 1;
const GROUP_ID: u32 = 2;
const ISOLATE_ID: u32 = 3;
const MOUNTED_ID: u32 = 4;
const FIRST_CUSTOM_ID: u32 = 32;

static NEXT_PROPERTY_ID: AtomicU32 = AtomicU32::new(FIRST_CUSTOM_ID);

/// Type-erased immutable value stored in node metadata.
pub type PropertyValue = Arc<dyn Any + Send + Sync>;

type ErasedCombine = Arc<dyn Fn(&PropertyValue, &PropertyValue) -> PropertyValue + Send + Sync>;
type PropertySourceFn = dyn Fn(&NodeType) -> Option<PropertyAssignment> + Send + Sync;

/// Text decoder used by a grammar-declared node property.
pub enum NodePropDeserializer<T> {
    /// Decoder that cannot reject its input.
    Infallible(fn(&str) -> T),
    /// Decoder that validates and may reject its input.
    Fallible(fn(&str) -> Result<T, PropertyError>),
}

impl<T> Clone for NodePropDeserializer<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for NodePropDeserializer<T> {}

/// Error returned while decoding a property declared in a grammar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyError {
    message: String,
}

impl PropertyError {
    /// Construct a property decoding error.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for PropertyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for PropertyError {}

/// Configuration for a custom [`NodeProp`].
pub struct NodePropConfig<T> {
    /// Decode the textual form used by a grammar.
    pub deserialize: Option<NodePropDeserializer<T>>,
    /// Combine a previous and a newly supplied value.
    pub combine: Option<fn(&T, &T) -> T>,
    /// Store the value on individual tree nodes instead of node types.
    pub per_node: bool,
}

impl<T> Clone for NodePropConfig<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for NodePropConfig<T> {}

impl<T> Default for NodePropConfig<T> {
    fn default() -> Self {
        Self {
            deserialize: None,
            combine: None,
            per_node: false,
        }
    }
}

/// A typed key for metadata stored on node types or individual trees.
pub struct NodeProp<T> {
    id: u32,
    per_node: bool,
    deserialize: Option<NodePropDeserializer<T>>,
    combine: Option<fn(&T, &T) -> T>,
    marker: PhantomData<fn() -> T>,
}

impl<T> Clone for NodeProp<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for NodeProp<T> {}

impl<T> fmt::Debug for NodeProp<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NodeProp")
            .field("id", &self.id)
            .field("per_node", &self.per_node)
            .finish_non_exhaustive()
    }
}

impl<T> NodeProp<T>
where
    T: Any + Clone + Send + Sync + 'static,
{
    /// Create a new property key.
    #[must_use]
    pub fn new(config: NodePropConfig<T>) -> Self {
        Self {
            id: NEXT_PROPERTY_ID.fetch_add(1, Ordering::Relaxed),
            per_node: config.per_node,
            deserialize: config.deserialize,
            combine: config.combine,
            marker: PhantomData,
        }
    }

    pub(crate) const fn builtin(
        id: u32,
        per_node: bool,
        deserialize: Option<NodePropDeserializer<T>>,
    ) -> Self {
        Self {
            id,
            per_node,
            deserialize,
            combine: None,
            marker: PhantomData,
        }
    }

    /// Return this property's stable process-local identifier.
    #[must_use]
    pub const fn id(self) -> u32 {
        self.id
    }

    /// Whether this property is stored on individual nodes.
    #[must_use]
    pub const fn is_per_node(self) -> bool {
        self.per_node
    }

    /// Decode a grammar-provided textual value.
    ///
    /// # Errors
    ///
    /// Returns an error when the property has no decoder or when decoding
    /// rejects the value.
    pub fn deserialize(self, value: &str) -> Result<T, PropertyError> {
        let Some(deserializer) = self.deserialize else {
            return Err(PropertyError::new(
                "this node property does not define a deserializer",
            ));
        };
        match deserializer {
            NodePropDeserializer::Infallible(deserialize) => Ok(deserialize(value)),
            NodePropDeserializer::Fallible(deserialize) => deserialize(value),
        }
    }

    /// Build a source that adds this property to matching node types.
    ///
    /// # Panics
    ///
    /// Panics when called for a per-node property, since node-set extension
    /// can only attach type properties.
    pub fn source(
        self,
        matcher: impl Fn(&NodeType) -> Option<T> + Send + Sync + 'static,
    ) -> NodePropSource {
        assert!(
            !self.per_node,
            "cannot add a per-node property to node types"
        );
        let combine = self.combine.map(|combine| {
            Arc::new(move |left: &PropertyValue, right: &PropertyValue| {
                let left = left
                    .downcast_ref::<T>()
                    .expect("node property value has the declared type");
                let right = right
                    .downcast_ref::<T>()
                    .expect("node property value has the declared type");
                Arc::new(combine(left, right)) as PropertyValue
            }) as ErasedCombine
        });
        NodePropSource {
            apply: Arc::new(move |node_type| {
                let value = matcher(node_type)?;
                Some(PropertyAssignment {
                    id: self.id,
                    value: Arc::new(value),
                    combine: combine.clone(),
                })
            }),
        }
    }

    /// Build a source from a space-separated node/group selector map.
    #[must_use]
    pub fn add_map(self, selectors: BTreeMap<String, T>) -> NodePropSource {
        let mut direct = BTreeMap::new();
        for (selector, value) in selectors {
            for name in selector.split_ascii_whitespace() {
                direct.insert(name.to_owned(), value.clone());
            }
        }
        self.source(move |node_type| {
            direct.get(node_type.name()).cloned().or_else(|| {
                node_type
                    .groups()
                    .iter()
                    .find_map(|group| direct.get(group).cloned())
            })
        })
    }

    pub(crate) fn read(self, values: &BTreeMap<u32, PropertyValue>) -> Option<&T> {
        values.get(&self.id)?.downcast_ref()
    }
}

pub(crate) struct PropertyAssignment {
    id: u32,
    value: PropertyValue,
    combine: Option<ErasedCombine>,
}

/// A function that derives at most one property assignment for a node type.
#[derive(Clone)]
pub struct NodePropSource {
    apply: Arc<PropertySourceFn>,
}

impl fmt::Debug for NodePropSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NodePropSource(..)")
    }
}

impl NodePropSource {
    pub(crate) fn apply_to(
        &self,
        node_type: &NodeType,
        values: &mut BTreeMap<u32, PropertyValue>,
    ) -> bool {
        let Some(assignment) = (self.apply)(node_type) else {
            return false;
        };
        if let (Some(previous), Some(combine)) =
            (values.get(&assignment.id), assignment.combine.as_ref())
        {
            let combined = combine(previous, &assignment.value);
            values.insert(assignment.id, combined);
        } else {
            values.insert(assignment.id, assignment.value);
        }
        true
    }
}

/// Matching closing-delimiter names for opening delimiter node types.
#[must_use]
pub const fn closed_by_prop() -> NodeProp<Vec<String>> {
    NodeProp::builtin(
        CLOSED_BY_ID,
        false,
        Some(NodePropDeserializer::Infallible(split_names)),
    )
}

/// Matching opening-delimiter names for closing delimiter node types.
#[must_use]
pub const fn opened_by_prop() -> NodeProp<Vec<String>> {
    NodeProp::builtin(
        OPENED_BY_ID,
        false,
        Some(NodePropDeserializer::Infallible(split_names)),
    )
}

/// Node-type group names.
#[must_use]
pub const fn group_prop() -> NodeProp<Vec<String>> {
    NodeProp::builtin(
        GROUP_ID,
        false,
        Some(NodePropDeserializer::Infallible(split_names)),
    )
}

/// Bidirectional-text isolate mode.
#[must_use]
pub const fn isolate_prop() -> NodeProp<String> {
    NodeProp::builtin(
        ISOLATE_ID,
        false,
        Some(NodePropDeserializer::Fallible(parse_isolate)),
    )
}

/// A mounted mixed-language subtree attached to an individual tree.
#[must_use]
pub const fn mounted_prop() -> NodeProp<MountedTree> {
    NodeProp::builtin(MOUNTED_ID, true, None)
}

fn split_names(value: &str) -> Vec<String> {
    value.split_ascii_whitespace().map(str::to_owned).collect()
}

fn parse_isolate(value: &str) -> Result<String, PropertyError> {
    let value = if value.is_empty() { "auto" } else { value };
    if matches!(value, "rtl" | "ltr" | "auto") {
        Ok(value.to_owned())
    } else {
        Err(PropertyError::new(format!(
            "invalid isolate value {value:?}"
        )))
    }
}
