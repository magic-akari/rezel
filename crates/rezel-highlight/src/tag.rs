use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};

/// One abstract syntactic highlighting tag.
///
/// Tags have process-local identity. A tag may derive from another tag, in
/// which case highlighters can fall back through its ancestry.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Tag(u32);

impl Tag {
    /// Define an unnamed tag, optionally deriving it from an unmodified tag.
    ///
    /// # Panics
    ///
    /// Panics when `parent` is a modified tag.
    #[must_use]
    pub fn define(parent: Option<Self>) -> Self {
        Self::define_named("?", parent)
    }

    /// Define a named tag, optionally deriving it from an unmodified tag.
    ///
    /// # Panics
    ///
    /// Panics when `parent` is a modified tag.
    #[must_use]
    pub fn define_named(name: impl Into<Arc<str>>, parent: Option<Self>) -> Self {
        with_registry(|registry| registry.define_tag(name.into(), parent))
    }

    /// Name used when formatting this tag.
    #[must_use]
    pub fn name(self) -> Arc<str> {
        with_registry(|registry| Arc::clone(&registry.tag(self).name))
    }

    /// This tag followed by all less-specific fallback tags.
    #[must_use]
    pub fn set(self) -> TagSet {
        with_registry(|registry| TagSet(Arc::clone(&registry.tag(self).set)))
    }

    /// Base unmodified tag, when this tag carries modifiers.
    #[must_use]
    pub fn base(self) -> Option<Self> {
        with_registry(|registry| registry.tag(self).base)
    }

    /// Modifiers applied to this tag in canonical order.
    #[must_use]
    pub fn modifiers(self) -> Arc<[Modifier]> {
        with_registry(|registry| Arc::clone(&registry.tag(self).modifiers))
    }

    pub(crate) const fn id(self) -> u32 {
        self.0
    }
}

impl fmt::Debug for Tag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("Tag")
            .field(&self.to_string())
            .finish()
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_registry(|registry| {
            let data = registry.tag(*self);
            let mut name = data.name.to_string();
            for modifier in data.modifiers.iter() {
                let modifier_name = &registry.modifier(*modifier).name;
                if !modifier_name.is_empty() {
                    name = format!("{modifier_name}({name})");
                }
            }
            formatter.write_str(&name)
        })
    }
}

/// A composable highlighting tag modifier.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Modifier(u16);

impl Modifier {
    /// Define a new modifier.
    #[must_use]
    pub fn define(name: impl Into<Arc<str>>) -> Self {
        with_registry(|registry| registry.define_modifier(name.into()))
    }

    /// Apply this modifier to a tag.
    ///
    /// Reapplying a modifier is idempotent. Applying a group of modifiers in
    /// different orders produces the same tag identity.
    #[must_use]
    pub fn apply(self, tag: Tag) -> Tag {
        with_registry(|registry| registry.apply_modifier(self, tag))
    }

    /// Modifier name.
    #[must_use]
    pub fn name(self) -> Arc<str> {
        with_registry(|registry| Arc::clone(&registry.modifier(self).name))
    }
}

impl fmt::Debug for Modifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("Modifier")
            .field(&self.name())
            .finish()
    }
}

/// An immutable ordered collection of highlighting tags.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct TagSet(Arc<[Tag]>);

impl TagSet {
    /// Construct an empty tag set.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Tags in source order.
    #[must_use]
    pub fn as_slice(&self) -> &[Tag] {
        &self.0
    }

    /// Number of tags.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the set contains no tags.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterate over tags in source order.
    pub fn iter(&self) -> std::slice::Iter<'_, Tag> {
        self.0.iter()
    }

    pub(crate) fn append(&self, other: &Self) -> Self {
        if other.is_empty() {
            return self.clone();
        }
        if self.is_empty() {
            return other.clone();
        }
        let mut tags = Vec::with_capacity(self.len() + other.len());
        tags.extend_from_slice(self.as_slice());
        tags.extend_from_slice(other.as_slice());
        Self(tags.into())
    }
}

impl fmt::Debug for TagSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.0.iter()).finish()
    }
}

impl From<Tag> for TagSet {
    fn from(tag: Tag) -> Self {
        Self(Arc::from([tag]))
    }
}

impl From<Vec<Tag>> for TagSet {
    fn from(tags: Vec<Tag>) -> Self {
        Self(tags.into())
    }
}

impl<const N: usize> From<[Tag; N]> for TagSet {
    fn from(tags: [Tag; N]) -> Self {
        Self(Arc::from(tags))
    }
}

impl From<&[Tag]> for TagSet {
    fn from(tags: &[Tag]) -> Self {
        Self(Arc::from(tags))
    }
}

impl<'a> IntoIterator for &'a TagSet {
    type Item = &'a Tag;
    type IntoIter = std::slice::Iter<'a, Tag>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Clone)]
struct TagData {
    name: Arc<str>,
    set: Arc<[Tag]>,
    base: Option<Tag>,
    modifiers: Arc<[Modifier]>,
}

struct ModifierData {
    name: Arc<str>,
}

#[derive(Default)]
struct Registry {
    tags: Vec<Option<TagData>>,
    modifiers: Vec<ModifierData>,
    modified: BTreeMap<(u32, Vec<u16>), Tag>,
}

impl Registry {
    fn define_tag(&mut self, name: Arc<str>, parent: Option<Tag>) -> Tag {
        if let Some(parent) = parent {
            assert!(
                self.tag(parent).base.is_none(),
                "cannot derive from a modified tag"
            );
        }
        let tag = self.reserve_tag();
        let mut set = vec![tag];
        if let Some(parent) = parent {
            set.extend_from_slice(&self.tag(parent).set);
        }
        self.tags[tag.0 as usize] = Some(TagData {
            name,
            set: set.into(),
            base: None,
            modifiers: Arc::from([]),
        });
        tag
    }

    fn define_modifier(&mut self, name: Arc<str>) -> Modifier {
        let id = u16::try_from(self.modifiers.len()).expect("too many highlighting modifiers");
        self.modifiers.push(ModifierData { name });
        Modifier(id)
    }

    fn apply_modifier(&mut self, modifier: Modifier, tag: Tag) -> Tag {
        let data = self.tag(tag);
        if data.modifiers.contains(&modifier) {
            return tag;
        }
        let base = data.base.unwrap_or(tag);
        let mut modifiers = data.modifiers.to_vec();
        modifiers.push(modifier);
        modifiers.sort_unstable();
        self.modified_tag(base, &modifiers)
    }

    fn modified_tag(&mut self, base: Tag, modifiers: &[Modifier]) -> Tag {
        if modifiers.is_empty() {
            return base;
        }
        let key = (
            base.0,
            modifiers.iter().map(|modifier| modifier.0).collect(),
        );
        if let Some(tag) = self.modified.get(&key) {
            return *tag;
        }

        let name = Arc::clone(&self.tag(base).name);
        let base_set = self.tag(base).set.to_vec();
        let tag = self.reserve_tag();
        self.modified.insert(key, tag);

        let configurations = power_set(modifiers);
        let mut set = Vec::new();
        for parent in base_set {
            if !self.tag(parent).modifiers.is_empty() {
                continue;
            }
            for configuration in &configurations {
                set.push(self.modified_tag(parent, configuration));
            }
        }
        self.tags[tag.0 as usize] = Some(TagData {
            name,
            set: set.into(),
            base: Some(base),
            modifiers: Arc::from(modifiers),
        });
        tag
    }

    fn reserve_tag(&mut self) -> Tag {
        let id = u32::try_from(self.tags.len()).expect("too many highlighting tags");
        self.tags.push(None);
        Tag(id)
    }

    fn tag(&self, tag: Tag) -> &TagData {
        self.tags
            .get(tag.0 as usize)
            .and_then(Option::as_ref)
            .expect("unknown or incomplete highlighting tag")
    }

    fn modifier(&self, modifier: Modifier) -> &ModifierData {
        self.modifiers
            .get(usize::from(modifier.0))
            .expect("unknown highlighting modifier")
    }
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

fn with_registry<T>(operation: impl FnOnce(&mut Registry) -> T) -> T {
    let mut registry = registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    operation(&mut registry)
}

fn power_set(modifiers: &[Modifier]) -> Vec<Vec<Modifier>> {
    let mut sets = vec![Vec::new()];
    for modifier in modifiers {
        let existing = sets.len();
        for index in 0..existing {
            let mut set = sets[index].clone();
            set.push(*modifier);
            sets.push(set);
        }
    }
    sets.sort_by_key(|set| std::cmp::Reverse(set.len()));
    sets
}

/// The official syntactic tag vocabulary and modifier set.
#[derive(Debug)]
pub struct StandardTags {
    pub comment: Tag,
    pub line_comment: Tag,
    pub block_comment: Tag,
    pub doc_comment: Tag,
    pub name: Tag,
    pub variable_name: Tag,
    pub type_name: Tag,
    pub tag_name: Tag,
    pub property_name: Tag,
    pub attribute_name: Tag,
    pub class_name: Tag,
    pub label_name: Tag,
    pub namespace: Tag,
    pub macro_name: Tag,
    pub literal: Tag,
    pub string: Tag,
    pub doc_string: Tag,
    pub character: Tag,
    pub attribute_value: Tag,
    pub number: Tag,
    pub integer: Tag,
    pub float: Tag,
    pub bool_: Tag,
    pub regexp: Tag,
    pub escape: Tag,
    pub color: Tag,
    pub url: Tag,
    pub keyword: Tag,
    pub self_: Tag,
    pub null: Tag,
    pub atom: Tag,
    pub unit: Tag,
    pub modifier: Tag,
    pub operator_keyword: Tag,
    pub control_keyword: Tag,
    pub definition_keyword: Tag,
    pub module_keyword: Tag,
    pub operator: Tag,
    pub deref_operator: Tag,
    pub arithmetic_operator: Tag,
    pub logic_operator: Tag,
    pub bitwise_operator: Tag,
    pub compare_operator: Tag,
    pub update_operator: Tag,
    pub definition_operator: Tag,
    pub type_operator: Tag,
    pub control_operator: Tag,
    pub punctuation: Tag,
    pub separator: Tag,
    pub bracket: Tag,
    pub angle_bracket: Tag,
    pub square_bracket: Tag,
    pub paren: Tag,
    pub brace: Tag,
    pub content: Tag,
    pub heading: Tag,
    pub heading1: Tag,
    pub heading2: Tag,
    pub heading3: Tag,
    pub heading4: Tag,
    pub heading5: Tag,
    pub heading6: Tag,
    pub content_separator: Tag,
    pub list: Tag,
    pub quote: Tag,
    pub emphasis: Tag,
    pub strong: Tag,
    pub link: Tag,
    pub monospace: Tag,
    pub strikethrough: Tag,
    pub inserted: Tag,
    pub deleted: Tag,
    pub changed: Tag,
    pub invalid: Tag,
    pub meta: Tag,
    pub document_meta: Tag,
    pub annotation: Tag,
    pub processing_instruction: Tag,
    pub definition: Modifier,
    pub constant: Modifier,
    pub function: Modifier,
    pub standard: Modifier,
    pub local: Modifier,
    pub special: Modifier,
}

impl StandardTags {
    fn new() -> Self {
        let comment = Tag::define_named("comment", None);
        let name = Tag::define_named("name", None);
        let type_name = Tag::define_named("typeName", Some(name));
        let property_name = Tag::define_named("propertyName", Some(name));
        let literal = Tag::define_named("literal", None);
        let string = Tag::define_named("string", Some(literal));
        let number = Tag::define_named("number", Some(literal));
        let keyword = Tag::define_named("keyword", None);
        let operator = Tag::define_named("operator", None);
        let punctuation = Tag::define_named("punctuation", None);
        let bracket = Tag::define_named("bracket", Some(punctuation));
        let content = Tag::define_named("content", None);
        let heading = Tag::define_named("heading", Some(content));
        let meta = Tag::define_named("meta", None);

        Self {
            comment,
            line_comment: Tag::define_named("lineComment", Some(comment)),
            block_comment: Tag::define_named("blockComment", Some(comment)),
            doc_comment: Tag::define_named("docComment", Some(comment)),
            name,
            variable_name: Tag::define_named("variableName", Some(name)),
            type_name,
            tag_name: Tag::define_named("tagName", Some(type_name)),
            property_name,
            attribute_name: Tag::define_named("attributeName", Some(property_name)),
            class_name: Tag::define_named("className", Some(name)),
            label_name: Tag::define_named("labelName", Some(name)),
            namespace: Tag::define_named("namespace", Some(name)),
            macro_name: Tag::define_named("macroName", Some(name)),
            literal,
            string,
            doc_string: Tag::define_named("docString", Some(string)),
            character: Tag::define_named("character", Some(string)),
            attribute_value: Tag::define_named("attributeValue", Some(string)),
            number,
            integer: Tag::define_named("integer", Some(number)),
            float: Tag::define_named("float", Some(number)),
            bool_: Tag::define_named("bool", Some(literal)),
            regexp: Tag::define_named("regexp", Some(literal)),
            escape: Tag::define_named("escape", Some(literal)),
            color: Tag::define_named("color", Some(literal)),
            url: Tag::define_named("url", Some(literal)),
            keyword,
            self_: Tag::define_named("self", Some(keyword)),
            null: Tag::define_named("null", Some(keyword)),
            atom: Tag::define_named("atom", Some(keyword)),
            unit: Tag::define_named("unit", Some(keyword)),
            modifier: Tag::define_named("modifier", Some(keyword)),
            operator_keyword: Tag::define_named("operatorKeyword", Some(keyword)),
            control_keyword: Tag::define_named("controlKeyword", Some(keyword)),
            definition_keyword: Tag::define_named("definitionKeyword", Some(keyword)),
            module_keyword: Tag::define_named("moduleKeyword", Some(keyword)),
            operator,
            deref_operator: Tag::define_named("derefOperator", Some(operator)),
            arithmetic_operator: Tag::define_named("arithmeticOperator", Some(operator)),
            logic_operator: Tag::define_named("logicOperator", Some(operator)),
            bitwise_operator: Tag::define_named("bitwiseOperator", Some(operator)),
            compare_operator: Tag::define_named("compareOperator", Some(operator)),
            update_operator: Tag::define_named("updateOperator", Some(operator)),
            definition_operator: Tag::define_named("definitionOperator", Some(operator)),
            type_operator: Tag::define_named("typeOperator", Some(operator)),
            control_operator: Tag::define_named("controlOperator", Some(operator)),
            punctuation,
            separator: Tag::define_named("separator", Some(punctuation)),
            bracket,
            angle_bracket: Tag::define_named("angleBracket", Some(bracket)),
            square_bracket: Tag::define_named("squareBracket", Some(bracket)),
            paren: Tag::define_named("paren", Some(bracket)),
            brace: Tag::define_named("brace", Some(bracket)),
            content,
            heading,
            heading1: Tag::define_named("heading1", Some(heading)),
            heading2: Tag::define_named("heading2", Some(heading)),
            heading3: Tag::define_named("heading3", Some(heading)),
            heading4: Tag::define_named("heading4", Some(heading)),
            heading5: Tag::define_named("heading5", Some(heading)),
            heading6: Tag::define_named("heading6", Some(heading)),
            content_separator: Tag::define_named("contentSeparator", Some(content)),
            list: Tag::define_named("list", Some(content)),
            quote: Tag::define_named("quote", Some(content)),
            emphasis: Tag::define_named("emphasis", Some(content)),
            strong: Tag::define_named("strong", Some(content)),
            link: Tag::define_named("link", Some(content)),
            monospace: Tag::define_named("monospace", Some(content)),
            strikethrough: Tag::define_named("strikethrough", Some(content)),
            inserted: Tag::define_named("inserted", None),
            deleted: Tag::define_named("deleted", None),
            changed: Tag::define_named("changed", None),
            invalid: Tag::define_named("invalid", None),
            meta,
            document_meta: Tag::define_named("documentMeta", Some(meta)),
            annotation: Tag::define_named("annotation", Some(meta)),
            processing_instruction: Tag::define_named("processingInstruction", Some(meta)),
            definition: Modifier::define("definition"),
            constant: Modifier::define("constant"),
            function: Modifier::define("function"),
            standard: Modifier::define("standard"),
            local: Modifier::define("local"),
            special: Modifier::define("special"),
        }
    }
}

/// Return the lazily initialized official tag vocabulary.
#[must_use]
pub fn tags() -> &'static StandardTags {
    static TAGS: OnceLock<StandardTags> = OnceLock::new();
    TAGS.get_or_init(StandardTags::new)
}
