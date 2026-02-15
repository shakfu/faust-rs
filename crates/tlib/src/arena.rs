use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TreeId(u32);

impl TreeId {
    #[must_use]
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeNode {
    pub kind: NodeKind,
    pub children: Vec<TreeId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct NodeKey {
    kind: NodeKind,
    children: Vec<TreeId>,
}

#[derive(Debug)]
pub struct TreeArena {
    nodes: Vec<TreeNode>,
    interner0: HashMap<NodeKind, TreeId>,
    interner1: HashMap<(NodeKind, TreeId), TreeId>,
    interner2: HashMap<(NodeKind, TreeId, TreeId), TreeId>,
    interner_n: HashMap<NodeKey, TreeId>,
    nil: TreeId,
}

impl Default for TreeArena {
    fn default() -> Self {
        Self::new()
    }
}

impl TreeArena {
    #[must_use]
    pub fn new() -> Self {
        let mut arena = Self {
            nodes: Vec::new(),
            interner0: HashMap::new(),
            interner1: HashMap::new(),
            interner2: HashMap::new(),
            interner_n: HashMap::new(),
            nil: TreeId(0),
        };
        let nil = arena.intern(NodeKind::Nil, &[]);
        arena.nil = nil;
        arena
    }

    #[must_use]
    pub fn nil(&self) -> TreeId {
        self.nil
    }

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
                    children: Vec::new(),
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
                    children: vec![*a],
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
                    children: vec![*a, *b],
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
                    children: key.children.clone(),
                });
                self.interner_n.insert(key, id);
                id
            }
        }
    }

    #[must_use]
    pub fn cons(&mut self, head: TreeId, tail: TreeId) -> TreeId {
        self.intern(NodeKind::Cons, &[head, tail])
    }

    #[must_use]
    pub fn is_nil(&self, id: TreeId) -> bool {
        matches!(self.kind(id), Some(NodeKind::Nil))
    }

    #[must_use]
    pub fn is_list(&self, id: TreeId) -> bool {
        self.is_nil(id) || matches!(self.kind(id), Some(NodeKind::Cons))
    }

    #[must_use]
    pub fn hd(&self, list: TreeId) -> Option<TreeId> {
        let node = self.node(list)?;
        if !matches!(node.kind, NodeKind::Cons) || node.children.len() != 2 {
            return None;
        }
        Some(node.children[0])
    }

    #[must_use]
    pub fn tl(&self, list: TreeId) -> Option<TreeId> {
        let node = self.node(list)?;
        if !matches!(node.kind, NodeKind::Cons) || node.children.len() != 2 {
            return None;
        }
        Some(node.children[1])
    }

    #[must_use]
    pub fn symbol(&mut self, value: impl Into<String>) -> TreeId {
        self.intern(NodeKind::Symbol(Arc::<str>::from(value.into())), &[])
    }

    #[must_use]
    pub fn string_lit(&mut self, value: impl Into<String>) -> TreeId {
        self.intern(NodeKind::StringLiteral(Arc::<str>::from(value.into())), &[])
    }

    #[must_use]
    pub fn int(&mut self, value: i64) -> TreeId {
        self.intern(NodeKind::Int(value), &[])
    }

    #[must_use]
    pub fn float(&mut self, value: f64) -> TreeId {
        self.intern(NodeKind::FloatBits(value.to_bits()), &[])
    }

    #[must_use]
    pub fn tag(&mut self, value: impl Into<String>) -> TreeId {
        self.intern(NodeKind::Tag(Arc::<str>::from(value.into())), &[])
    }

    #[must_use]
    pub fn node(&self, id: TreeId) -> Option<&TreeNode> {
        self.nodes.get(id.0 as usize)
    }

    #[must_use]
    pub fn kind(&self, id: TreeId) -> Option<&NodeKind> {
        self.node(id).map(|node| &node.kind)
    }

    #[must_use]
    pub fn children(&self, id: TreeId) -> Option<&[TreeId]> {
        self.node(id).map(|node| node.children.as_slice())
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}
