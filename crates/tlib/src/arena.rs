//! Hash-consed tree arena used as Rust `tlib` core.
//!
//! # Source provenance (C++)
//! - `compiler/tlib/tree.hh`, `compiler/tlib/tree.cpp` (`CTree::make`, hash-cons table)
//! - `compiler/tlib/list.hh`, `compiler/tlib/list.cpp` (`cons/hd/tl`, `nil/list` predicates)
//! - `compiler/tlib/node.hh` (`Node` payload kinds)
//!
//! # Parity invariants
//! - Interning is structural: same node kind + same ordered children => same `TreeId`.
//! - `TreeId` values are arena-local and stable for the arena lifetime.
//! - List API preserves C++ list semantics (`Cons` node of arity 2 + canonical `Nil`).

use std::sync::Arc;

use ahash::AHashMap;

/// Arena-local identifier of an interned tree node.
///
/// Equality on `TreeId` is the fast structural equality primitive used by higher phases.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TreeId(u32);

impl TreeId {
    /// Returns the raw numeric index used inside the arena.
    #[must_use]
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

/// Node payload kind equivalent to Faust C++ `Node` categories plus list tags.
///
/// `FloatBits` stores raw IEEE bits so NaN payloads/signs are preserved exactly.
///
/// `Tag` stores a numeric id interned via [`TreeArena::intern_tag`]. This makes
/// `Hash`, `PartialEq`, and `Clone` on tag nodes O(1) (integer operations) instead
/// of O(string length), matching the C++ compiler's interned `Sym` pointer semantics.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NodeKind {
    /// Canonical empty list node.
    Nil,
    /// Cons-list constructor node.
    Cons,
    /// Symbol identifier payload.
    Symbol(Arc<str>),
    /// String literal payload.
    StringLiteral(Arc<str>),
    /// Signed integer literal payload.
    Int(i64),
    /// Floating-point literal stored as raw IEEE 754 bits.
    FloatBits(u64),
    /// Interned numeric tag id.
    Tag(u32),
}

/// Interned node stored in [`TreeArena`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeNode {
    /// Node payload kind.
    pub kind: NodeKind,
    /// Ordered child list.
    pub children: ChildList,
}

/// Compact children storage optimized for low arity nodes (`0/1/2`) common in Faust IR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChildList {
    /// Zero children.
    Empty,
    /// Single child, inline.
    One([TreeId; 1]),
    /// Two children, inline.
    Two([TreeId; 2]),
    /// Three or more children on heap.
    Many(Box<[TreeId]>),
}

impl ChildList {
    /// Creates an empty children list.
    #[must_use]
    pub fn empty() -> Self {
        Self::Empty
    }

    /// Creates a one-child list.
    #[must_use]
    pub fn one(child: TreeId) -> Self {
        Self::One([child])
    }

    /// Creates a two-children list preserving order.
    #[must_use]
    pub fn two(left: TreeId, right: TreeId) -> Self {
        Self::Two([left, right])
    }

    /// Creates a generic arity list (`>= 0`) from heap-backed storage.
    #[must_use]
    pub fn many(children: Vec<TreeId>) -> Self {
        Self::Many(children.into_boxed_slice())
    }

    /// Number of children.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Empty => 0,
            Self::One(_) => 1,
            Self::Two(_) => 2,
            Self::Many(children) => children.len(),
        }
    }

    /// Returns `true` when this list contains no children.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    /// Returns child at `index` if present.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<TreeId> {
        self.as_slice().get(index).copied()
    }

    /// Returns children as a read-only slice.
    #[must_use]
    pub fn as_slice(&self) -> &[TreeId] {
        match self {
            Self::Empty => &[],
            Self::One(children) => &children[..],
            Self::Two(children) => &children[..],
            Self::Many(children) => children,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct NodeKey {
    kind: NodeKind,
    children: Vec<TreeId>,
}

/// Internal registry that maps tag strings to numeric ids and back.
///
/// This mirrors the C++ compiler's `Sym` interning: each unique tag string is
/// assigned a `u32` id so that all subsequent operations (hash, equality, clone)
/// on `NodeKind::Tag` are O(1) integer operations.
#[derive(Debug)]
struct TagRegistry {
    to_id: AHashMap<Arc<str>, u32>,
    to_str: Vec<Arc<str>>,
}

impl TagRegistry {
    fn new() -> Self {
        Self {
            to_id: AHashMap::new(),
            to_str: Vec::new(),
        }
    }

    /// Interns `tag` and returns its numeric id. Returns existing id on duplicates.
    fn intern(&mut self, tag: &str) -> u32 {
        if let Some(&id) = self.to_id.get(tag) {
            return id;
        }
        let id = self.to_str.len() as u32;
        let arc: Arc<str> = Arc::from(tag);
        self.to_str.push(Arc::clone(&arc));
        self.to_id.insert(arc, id);
        id
    }

    /// Returns the string for a numeric tag id.
    fn name(&self, id: u32) -> Option<&str> {
        self.to_str.get(id as usize).map(|s| s.as_ref())
    }
}

/// Hash-consing arena for tree nodes.
///
/// # Source provenance (C++)
/// Mirrors `CTree::make` sharing behavior from `compiler/tlib/tree.cpp`.
///
/// # Invariants
/// - For a given arena instance, each structural node appears once.
/// - `nil()` always points to the canonical `NodeKind::Nil` node.
/// - Tag strings are interned via the internal tag registry so `NodeKind::Tag(u32)` operations
///   are O(1).
#[derive(Debug)]
pub struct TreeArena {
    nodes: Vec<TreeNode>,
    interner0: AHashMap<NodeKind, TreeId>,
    interner1: AHashMap<(NodeKind, TreeId), TreeId>,
    interner2: AHashMap<(NodeKind, TreeId, TreeId), TreeId>,
    interner_n: AHashMap<NodeKey, TreeId>,
    tag_registry: TagRegistry,
    nil: TreeId,
}

impl Default for TreeArena {
    fn default() -> Self {
        Self::new()
    }
}

impl TreeArena {
    /// Creates an empty arena with canonical `nil` pre-interned.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacities(0, 0, 0, 0, 0)
    }

    /// Creates an arena with symmetric pre-allocation hints.
    ///
    /// This is an optimization helper and does not alter semantics.
    #[must_use]
    pub fn with_capacity(nodes_capacity: usize) -> Self {
        Self::with_capacities(
            nodes_capacity,
            nodes_capacity,
            nodes_capacity,
            nodes_capacity,
            nodes_capacity,
        )
    }

    /// Creates an arena with explicit capacities for nodes and each interner table.
    ///
    /// Capacity values are hints only.
    #[must_use]
    pub fn with_capacities(
        nodes_capacity: usize,
        interner0_capacity: usize,
        interner1_capacity: usize,
        interner2_capacity: usize,
        interner_n_capacity: usize,
    ) -> Self {
        let mut arena = Self {
            nodes: Vec::with_capacity(nodes_capacity),
            interner0: AHashMap::with_capacity(interner0_capacity),
            interner1: AHashMap::with_capacity(interner1_capacity),
            interner2: AHashMap::with_capacity(interner2_capacity),
            interner_n: AHashMap::with_capacity(interner_n_capacity),
            tag_registry: TagRegistry::new(),
            nil: TreeId(0),
        };
        let nil = arena.intern(NodeKind::Nil, &[]);
        arena.nil = nil;
        arena
    }

    /// Returns canonical `nil` node id.
    #[must_use]
    pub fn nil(&self) -> TreeId {
        self.nil
    }

    /// Reserves additional capacity in internal storage/interner tables.
    ///
    /// This is purely a performance hint.
    pub fn reserve(
        &mut self,
        additional_nodes: usize,
        additional_interner0: usize,
        additional_interner1: usize,
        additional_interner2: usize,
        additional_interner_n: usize,
    ) {
        self.nodes.reserve(additional_nodes);
        self.interner0.reserve(additional_interner0);
        self.interner1.reserve(additional_interner1);
        self.interner2.reserve(additional_interner2);
        self.interner_n.reserve(additional_interner_n);
    }

    /// Interns a node and returns its canonical [`TreeId`].
    ///
    /// If an identical node already exists, returns the existing id.
    #[must_use]
    pub fn intern(&mut self, kind: NodeKind, children: &[TreeId]) -> TreeId {
        match children {
            [] => {
                if let Some(id) = self.interner0.get(&kind) {
                    return *id;
                }
                let id = TreeId(self.nodes.len() as u32);
                self.nodes.push(TreeNode {
                    kind: kind.clone(),
                    children: ChildList::empty(),
                });
                self.interner0.insert(kind, id);
                id
            }
            [a] => {
                let key = (kind, *a);
                if let Some(id) = self.interner1.get(&key) {
                    return *id;
                }
                let id = TreeId(self.nodes.len() as u32);
                self.nodes.push(TreeNode {
                    kind: key.0.clone(),
                    children: ChildList::one(*a),
                });
                self.interner1.insert(key, id);
                id
            }
            [a, b] => {
                let key = (kind, *a, *b);
                if let Some(id) = self.interner2.get(&key) {
                    return *id;
                }
                let id = TreeId(self.nodes.len() as u32);
                self.nodes.push(TreeNode {
                    kind: key.0.clone(),
                    children: ChildList::two(*a, *b),
                });
                self.interner2.insert(key, id);
                id
            }
            _ => {
                let key = NodeKey {
                    kind,
                    children: children.to_vec(),
                };
                if let Some(id) = self.interner_n.get(&key) {
                    return *id;
                }
                let id = TreeId(self.nodes.len() as u32);
                self.nodes.push(TreeNode {
                    kind: key.kind.clone(),
                    children: ChildList::many(key.children.clone()),
                });
                self.interner_n.insert(key, id);
                id
            }
        }
    }

    /// List constructor equivalent to C++ `cons(a, b)`.
    #[must_use]
    pub fn cons(&mut self, head: TreeId, tail: TreeId) -> TreeId {
        self.intern(NodeKind::Cons, &[head, tail])
    }

    /// Predicate equivalent to C++ `isNil`.
    #[must_use]
    pub fn is_nil(&self, id: TreeId) -> bool {
        matches!(self.kind(id), Some(NodeKind::Nil))
    }

    /// Predicate equivalent to C++ `isList` (accepts `nil` and `cons`).
    #[must_use]
    pub fn is_list(&self, id: TreeId) -> bool {
        self.is_nil(id) || matches!(self.kind(id), Some(NodeKind::Cons))
    }

    /// Returns list head (`hd`) when `list` is a valid cons cell.
    #[must_use]
    pub fn hd(&self, list: TreeId) -> Option<TreeId> {
        let node = self.node(list)?;
        if !matches!(node.kind, NodeKind::Cons) || node.children.len() != 2 {
            return None;
        }
        node.children.get(0)
    }

    /// Returns list tail (`tl`) when `list` is a valid cons cell.
    #[must_use]
    pub fn tl(&self, list: TreeId) -> Option<TreeId> {
        let node = self.node(list)?;
        if !matches!(node.kind, NodeKind::Cons) || node.children.len() != 2 {
            return None;
        }
        node.children.get(1)
    }

    /// Interns a symbol atom.
    #[must_use]
    pub fn symbol(&mut self, value: impl Into<String>) -> TreeId {
        self.intern(NodeKind::Symbol(Arc::<str>::from(value.into())), &[])
    }

    /// Interns a string literal atom.
    #[must_use]
    pub fn string_lit(&mut self, value: impl Into<String>) -> TreeId {
        self.intern(NodeKind::StringLiteral(Arc::<str>::from(value.into())), &[])
    }

    /// Interns an integer atom.
    #[must_use]
    pub fn int(&mut self, value: i64) -> TreeId {
        self.intern(NodeKind::Int(value), &[])
    }

    /// Interns a floating-point atom preserving exact bit-pattern.
    #[must_use]
    pub fn float(&mut self, value: f64) -> TreeId {
        self.intern(NodeKind::FloatBits(value.to_bits()), &[])
    }

    /// Interns a tag string and returns its numeric tag id.
    ///
    /// This is the low-level API used by IR builders (`BoxBuilder`, `SigBuilder`,
    /// `FirBuilder`) to obtain tag ids for `NodeKind::Tag(u32)`.
    pub fn intern_tag(&mut self, tag: &str) -> u32 {
        self.tag_registry.intern(tag)
    }

    /// Returns the string name for a numeric tag id, or `None` if unknown.
    #[must_use]
    pub fn tag_name(&self, tag_id: u32) -> Option<&str> {
        self.tag_registry.name(tag_id)
    }

    /// Interns a generic tag atom used by higher-level IR builders.
    #[must_use]
    pub fn tag(&mut self, value: impl Into<String>) -> TreeId {
        let s: String = value.into();
        let tag_id = self.tag_registry.intern(&s);
        self.intern(NodeKind::Tag(tag_id), &[])
    }

    /// Returns raw node by id.
    #[must_use]
    pub fn node(&self, id: TreeId) -> Option<&TreeNode> {
        self.nodes.get(id.0 as usize)
    }

    /// Returns node kind by id.
    #[must_use]
    pub fn kind(&self, id: TreeId) -> Option<&NodeKind> {
        self.node(id).map(|node| &node.kind)
    }

    /// Returns children slice by id.
    #[must_use]
    pub fn children(&self, id: TreeId) -> Option<&[TreeId]> {
        self.node(id).map(|node| node.children.as_slice())
    }

    /// Number of interned nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// `true` if arena has no interned nodes.
    ///
    /// Note: current constructors always intern canonical `nil`, so `new()` is not empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}
