//! Diagnostic dump utilities for box trees.

use std::fmt::Write;

use tlib::{NodeKind, TreeArena};

use crate::BoxId;

/// Returns a deterministic, shape-oriented textual representation of the box tree
/// rooted at `root`.
///
/// Arena addresses and node IDs are excluded; the output is stable across arena
/// rebuilds and suitable for parser differential tests and snapshots.
#[must_use]
pub fn dump_box(arena: &TreeArena, root: BoxId) -> String {
    let mut out = String::new();
    dump_node(arena, root, &mut out);
    out
}

/// Recursive structural dumper used by [`dump_box`].
fn dump_node(arena: &TreeArena, id: BoxId, out: &mut String) {
    let Some(node) = arena.node(id) else {
        write!(out, "<invalid:{}>", id.as_u32()).expect("String write cannot fail");
        return;
    };

    match &node.kind {
        NodeKind::Nil => out.push_str("nil"),
        NodeKind::Cons => {
            out.push_str("cons(");
            if let Some(head) = node.children.get(0) {
                dump_node(arena, head, out);
            } else {
                out.push_str("<missing>");
            }
            out.push_str(", ");
            if let Some(tail) = node.children.get(1) {
                dump_node(arena, tail, out);
            } else {
                out.push_str("<missing>");
            }
            out.push(')');
        }
        NodeKind::Symbol(name) => {
            write!(out, "sym({name:?})").expect("String write cannot fail");
        }
        NodeKind::StringLiteral(value) => {
            write!(out, "str({value:?})").expect("String write cannot fail");
        }
        NodeKind::Int(value) => {
            write!(out, "int({value})").expect("String write cannot fail");
        }
        NodeKind::FloatBits(bits) => {
            write!(out, "float_bits(0x{bits:016x})").expect("String write cannot fail");
        }
        NodeKind::Tag(tag) => {
            match arena.tag_name(*tag) {
                Some(name) => out.push_str(name),
                None => write!(out, "<tag:{tag}>").expect("String write cannot fail"),
            }
            out.push('(');
            for (idx, child) in node.children.as_slice().iter().enumerate() {
                if idx > 0 {
                    out.push_str(", ");
                }
                dump_node(arena, *child, out);
            }
            out.push(')');
        }
    }
}
