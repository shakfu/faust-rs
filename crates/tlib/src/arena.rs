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
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Nil,
    Cons,
    Symbol(Arc<str>),
    StringLiteral(Arc<str>),
    Int(i64),
    FloatBits(u64),
    Tag(Arc<str>),
}

/// Interned node stored in [`TreeArena`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeNode {
    pub kind: NodeKind,
    pub children: ChildList,
}

/// Compact children storage optimized for low arity nodes (`0/1/2`) common in Faust IR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChildList {
    Empty,
    One([TreeId; 1]),
    Two([TreeId; 2]),
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

/// Hash-consing arena for tree nodes.
///
/// # Source provenance (C++)
/// Mirrors `CTree::make` sharing behavior from `compiler/tlib/tree.cpp`.
///
/// # Invariants
/// - For a given arena instance, each structural node appears once.
/// - `nil()` always points to the canonical `NodeKind::Nil` node.
#[derive(Debug)]
pub struct TreeArena {
    nodes: Vec<TreeNode>,
    interner0: AHashMap<NodeKind, TreeId>,
    interner1: AHashMap<(NodeKind, TreeId), TreeId>,
    interner2: AHashMap<(NodeKind, TreeId, TreeId), TreeId>,
    interner_n: AHashMap<NodeKey, TreeId>,
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

    /// Interns a generic tag atom used by higher-level IR builders.
    #[must_use]
    pub fn tag(&mut self, value: impl Into<String>) -> TreeId {
        self.intern(NodeKind::Tag(Arc::<str>::from(value.into())), &[])
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
