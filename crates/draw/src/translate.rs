//! Translation layer: box expression → [`Schema`] tree.
//!
//! [`generate_schema`] is the main entry point.  It recursively maps
//! [`BoxMatch`] variants to `Box<dyn Schema>`, mirroring the C++
//! `generateDiagramSchema` / `generateInsideSchema` pair in
//! `drawschema.cpp`.
//!
//! Folding (splitting large diagrams into multiple SVG files) is not
//! yet implemented; every diagram is rendered in-line.
//!
//! C++ reference: `compiler/draw/drawschema.cpp`.

use boxes::{BoxId, BoxMatch, match_box};
use tlib::{NodeKind, TreeArena, tree_to_str};

use crate::DrawConfig;
use crate::schema::{
    COLOR_LINK, COLOR_NORMAL, COLOR_NUM, COLOR_SLOT, COLOR_UI, Schema,
};
use crate::schemas::{
    block::make_block,
    cable::{CableSchema, CutSchema, ConnectorSchema},
    composed::make_decorate,
    merge::make_merge,
    multirate::{make_downsampling, make_ondemand, make_upsampling},
    par::make_par,
    rec::make_rec,
    route::make_route,
    seq::make_seq,
    split::make_split,
};

// ─── Top-level entry ────────────────────────────────────────────────────────────

/// Convert a box expression rooted at `b` into a schema tree.
///
/// This is a direct port of C++ `generateDiagramSchema` (non-folding path).
/// All sub-diagrams are rendered inline; folding is deferred to a later phase.
///
/// C++ reference: `drawschema.cpp:359` — `generateDiagramSchema`.
pub fn generate_schema(arena: &TreeArena, b: BoxId, config: &DrawConfig) -> Box<dyn Schema> {
    generate_inside(arena, b, config)
}

// ─── Inside dispatch ────────────────────────────────────────────────────────────

/// Map one `BoxId` to a schema, ignoring folding / naming decorations.
///
/// C++ reference: `drawschema.cpp:396` — `generateInsideSchema`.
fn generate_inside(arena: &TreeArena, b: BoxId, config: &DrawConfig) -> Box<dyn Schema> {
    let max = config.max_name_size;
    // Short-circuit for purely static short labels — helper for dynamic names.
    let tn = |s: String| truncate_name(s, max);

    match match_box(arena, b) {
        // ── Leaf values ──────────────────────────────────────────────
        BoxMatch::Int(i) => make_block(0, 1, i.to_string(), COLOR_NUM, ""),
        BoxMatch::Real(r) => make_block(0, 1, format_real(r), COLOR_NUM, ""),
        BoxMatch::Waveform(_) => make_block(0, 2, "waveform{...}", COLOR_NORMAL, ""),
        BoxMatch::Wire => Box::new(CableSchema::new(1)),
        BoxMatch::Cut => Box::new(CutSchema::new()),

        // ── Binary arithmetic primitives (2 → 1) ─────────────────────
        BoxMatch::Add  => make_block(2, 1, "+",  COLOR_NORMAL, ""),
        BoxMatch::Sub  => make_block(2, 1, "-",  COLOR_NORMAL, ""),
        BoxMatch::Mul  => make_block(2, 1, "*",  COLOR_NORMAL, ""),
        BoxMatch::Div  => make_block(2, 1, "/",  COLOR_NORMAL, ""),
        BoxMatch::Rem  => make_block(2, 1, "%",  COLOR_NORMAL, ""),
        BoxMatch::And  => make_block(2, 1, "&",  COLOR_NORMAL, ""),
        BoxMatch::Or   => make_block(2, 1, "|",  COLOR_NORMAL, ""),
        BoxMatch::Xor  => make_block(2, 1, "xor", COLOR_NORMAL, ""),
        BoxMatch::Lsh  => make_block(2, 1, "<<", COLOR_NORMAL, ""),
        BoxMatch::Rsh  => make_block(2, 1, ">>", COLOR_NORMAL, ""),
        BoxMatch::Lt   => make_block(2, 1, "<",  COLOR_NORMAL, ""),
        BoxMatch::Le   => make_block(2, 1, "<=", COLOR_NORMAL, ""),
        BoxMatch::Gt   => make_block(2, 1, ">",  COLOR_NORMAL, ""),
        BoxMatch::Ge   => make_block(2, 1, ">=", COLOR_NORMAL, ""),
        BoxMatch::Eq   => make_block(2, 1, "==", COLOR_NORMAL, ""),
        BoxMatch::Ne   => make_block(2, 1, "!=", COLOR_NORMAL, ""),
        BoxMatch::Pow  => make_block(2, 1, "pow", COLOR_NORMAL, ""),
        BoxMatch::Atan2 => make_block(2, 1, "atan2", COLOR_NORMAL, ""),
        BoxMatch::Fmod  => make_block(2, 1, "fmod",  COLOR_NORMAL, ""),
        BoxMatch::Remainder => make_block(2, 1, "remainder", COLOR_NORMAL, ""),
        BoxMatch::Min  => make_block(2, 1, "min", COLOR_NORMAL, ""),
        BoxMatch::Max  => make_block(2, 1, "max", COLOR_NORMAL, ""),
        BoxMatch::Delay => make_block(2, 1, "@", COLOR_NORMAL, ""),

        // ── Unary math primitives (1 → 1) ────────────────────────────
        BoxMatch::Acos  => make_block(1, 1, "acos",  COLOR_NORMAL, ""),
        BoxMatch::Asin  => make_block(1, 1, "asin",  COLOR_NORMAL, ""),
        BoxMatch::Atan  => make_block(1, 1, "atan",  COLOR_NORMAL, ""),
        BoxMatch::Cos   => make_block(1, 1, "cos",   COLOR_NORMAL, ""),
        BoxMatch::Sin   => make_block(1, 1, "sin",   COLOR_NORMAL, ""),
        BoxMatch::Tan   => make_block(1, 1, "tan",   COLOR_NORMAL, ""),
        BoxMatch::Exp   => make_block(1, 1, "exp",   COLOR_NORMAL, ""),
        BoxMatch::Log   => make_block(1, 1, "log",   COLOR_NORMAL, ""),
        BoxMatch::Log10 => make_block(1, 1, "log10", COLOR_NORMAL, ""),
        BoxMatch::Sqrt  => make_block(1, 1, "sqrt",  COLOR_NORMAL, ""),
        BoxMatch::Abs   => make_block(1, 1, "abs",   COLOR_NORMAL, ""),
        BoxMatch::Floor => make_block(1, 1, "floor", COLOR_NORMAL, ""),
        BoxMatch::Ceil  => make_block(1, 1, "ceil",  COLOR_NORMAL, ""),
        BoxMatch::Rint  => make_block(1, 1, "rint",  COLOR_NORMAL, ""),
        BoxMatch::Round => make_block(1, 1, "round", COLOR_NORMAL, ""),
        BoxMatch::IntCast   => make_block(1, 1, "int",   COLOR_NORMAL, ""),
        BoxMatch::FloatCast => make_block(1, 1, "float", COLOR_NORMAL, ""),
        BoxMatch::Delay1    => make_block(1, 1, "mem",   COLOR_NORMAL, ""),
        BoxMatch::Prefix    => make_block(2, 1, "prefix", COLOR_NORMAL, ""),

        // ── Select (1-based control + n data inputs) ─────────────────
        BoxMatch::Select2  => make_block(3, 1, "select2", COLOR_NORMAL, ""),
        BoxMatch::Select3  => make_block(4, 1, "select3", COLOR_NORMAL, ""),

        // ── Tables ───────────────────────────────────────────────────
        BoxMatch::ReadOnlyTable  => make_block(3, 1, "rdtable",  COLOR_NORMAL, ""),
        BoxMatch::WriteReadTable => make_block(5, 1, "rwtable",  COLOR_NORMAL, ""),

        // ── Misc primitives ──────────────────────────────────────────
        BoxMatch::AssertBounds => make_block(3, 1, "assertbounds", COLOR_NORMAL, ""),
        BoxMatch::Lowest       => make_block(1, 1, "lowest",       COLOR_NORMAL, ""),
        BoxMatch::Highest      => make_block(1, 1, "highest",      COLOR_NORMAL, ""),
        BoxMatch::Attach       => make_block(2, 2, "attach",       COLOR_NORMAL, ""),
        BoxMatch::Enable       => make_block(2, 2, "enable",       COLOR_NORMAL, ""),
        BoxMatch::Control      => make_block(2, 2, "control",      COLOR_NORMAL, ""),

        // ── Composition operators ─────────────────────────────────────
        BoxMatch::Seq(a, b)   => make_seq(generate_inside(arena, a, config), generate_inside(arena, b, config)),
        BoxMatch::Par(a, b)   => make_par(generate_inside(arena, a, config), generate_inside(arena, b, config)),
        BoxMatch::Split(a, b) => make_split(generate_inside(arena, a, config), generate_inside(arena, b, config)),
        BoxMatch::Merge(a, b) => make_merge(generate_inside(arena, a, config), generate_inside(arena, b, config)),
        BoxMatch::Rec(a, b)   => make_rec(generate_inside(arena, a, config), generate_inside(arena, b, config)),

        // ── Metadata: transparent pass-through ───────────────────────
        BoxMatch::Metadata(a, _b) => generate_inside(arena, a, config),

        // ── Groups: decorate with a labeled dashed border ────────────
        BoxMatch::VGroup(label, body) => {
            let name = tn(format!("vgroup({})", extract_name(arena, label)));
            make_decorate(generate_schema(arena, body, config), 10.0, name)
        }
        BoxMatch::HGroup(label, body) => {
            let name = tn(format!("hgroup({})", extract_name(arena, label)));
            make_decorate(generate_schema(arena, body, config), 10.0, name)
        }
        BoxMatch::TGroup(label, body) => {
            let name = tn(format!("tgroup({})", extract_name(arena, label)));
            make_decorate(generate_schema(arena, body, config), 10.0, name)
        }

        // ── UI elements ───────────────────────────────────────────────
        BoxMatch::Button(label) => {
            let s = tn(format!("button({})", extract_name(arena, label)));
            make_block(0, 1, s, COLOR_UI, "")
        }
        BoxMatch::Checkbox(label) => {
            let s = tn(format!("checkbox({})", extract_name(arena, label)));
            make_block(0, 1, s, COLOR_UI, "")
        }
        BoxMatch::VSlider(label, cur, min, max_id, step) => {
            let s = tn(format!(
                "vslider({}, {}, {}, {}, {})",
                extract_name(arena, label),
                format_node(arena, cur),
                format_node(arena, min),
                format_node(arena, max_id),
                format_node(arena, step),
            ));
            make_block(0, 1, s, COLOR_UI, "")
        }
        BoxMatch::HSlider(label, cur, min, max_id, step) => {
            let s = tn(format!(
                "hslider({}, {}, {}, {}, {})",
                extract_name(arena, label),
                format_node(arena, cur),
                format_node(arena, min),
                format_node(arena, max_id),
                format_node(arena, step),
            ));
            make_block(0, 1, s, COLOR_UI, "")
        }
        BoxMatch::NumEntry(label, cur, min, max_id, step) => {
            let s = tn(format!(
                "nentry({}, {}, {}, {}, {})",
                extract_name(arena, label),
                format_node(arena, cur),
                format_node(arena, min),
                format_node(arena, max_id),
                format_node(arena, step),
            ));
            make_block(0, 1, s, COLOR_UI, "")
        }
        BoxMatch::HBargraph(label, min, max_id) => {
            let s = tn(format!(
                "hbargraph({}, {}, {})",
                extract_name(arena, label),
                format_node(arena, min),
                format_node(arena, max_id),
            ));
            make_block(1, 1, s, COLOR_UI, "")
        }
        BoxMatch::VBargraph(label, min, max_id) => {
            let s = tn(format!(
                "vbargraph({}, {}, {})",
                extract_name(arena, label),
                format_node(arena, min),
                format_node(arena, max_id),
            ));
            make_block(1, 1, s, COLOR_UI, "")
        }
        BoxMatch::Soundfile(label, chan) => {
            let n = extract_int(arena, chan).unwrap_or(1) as usize;
            let s = tn(format!("soundfile({}, {})", extract_name(arena, label), n));
            make_block(2, 2 + n, s, COLOR_UI, "")
        }

        // ── Route ─────────────────────────────────────────────────────
        BoxMatch::Route(a, b, c) => {
            let ins    = extract_int(arena, a).unwrap_or(0).max(0) as usize;
            let outs   = extract_int(arena, b).unwrap_or(0).max(0) as usize;
            let routes = flatten_int_tree(arena, c);
            make_route(ins, outs, routes, config.draw_route_frame)
        }

        // ── Multi-rate wrappers ───────────────────────────────────────
        BoxMatch::Ondemand(inner)    => make_ondemand(generate_inside(arena, inner, config)),
        BoxMatch::Upsampling(inner)  => make_upsampling(generate_inside(arena, inner, config)),
        BoxMatch::Downsampling(inner) => make_downsampling(generate_inside(arena, inner, config)),

        // ── Slots (lambda variable placeholders) ─────────────────────
        BoxMatch::Slot(i) => {
            let name = format!("[{i}]");
            make_block(0, 1, name, COLOR_SLOT, "")
        }

        // ── Abstraction / Symbolic ────────────────────────────────────
        BoxMatch::Symbolic(a, b) => {
            let slot = generate_slot_schema(arena, a);
            generate_abstraction(arena, slot, b, config)
        }

        // ── FFI / foreign items ───────────────────────────────────────
        BoxMatch::FFun(ff) => {
            let name = tn(extract_name(arena, ff));
            make_block(1, 1, name, COLOR_NORMAL, "")
        }
        BoxMatch::Ffunction(ff, _typ, _file) => {
            let name = tn(extract_name(arena, ff));
            make_block(1, 1, name, COLOR_NORMAL, "")
        }
        BoxMatch::FConst(_typ, name, _file) => {
            let s = tn(extract_name(arena, name));
            make_block(0, 1, s, COLOR_NORMAL, "")
        }
        BoxMatch::FVar(_typ, name, _file) => {
            let s = tn(extract_name(arena, name));
            make_block(0, 1, s, COLOR_NORMAL, "")
        }

        // ── Named identifier ─────────────────────────────────────────
        BoxMatch::Ident(s) => make_block(0, 1, tn(s.to_owned()), COLOR_LINK, ""),

        // ── Environment ──────────────────────────────────────────────
        BoxMatch::Environment => make_block(0, 0, "environment{...}", COLOR_NORMAL, ""),

        // ── Inverter (*(-1)) shorthand ────────────────────────────────
        // Note: isInverter check in C++ looks at gGlobal->gInverter table.
        // Without that table we can't detect inverters; they fall through to Mul.
        // The InverterSchema can be produced by the caller if needed.

        // ── Anything else: placeholder block ─────────────────────────
        _ => make_block(1, 1, "?", COLOR_NORMAL, ""),
    }
}

// ─── Abstraction helper ──────────────────────────────────────────────────────

/// Build an abstraction schema by placing input slots before the body.
///
/// C++ reference: `drawschema.cpp:654` — `generateAbstractionSchema`.
fn generate_abstraction(arena: &TreeArena, mut x: Box<dyn Schema>, body: BoxId, config: &DrawConfig) -> Box<dyn Schema> {
    let mut t = body;
    loop {
        match match_box(arena, t) {
            BoxMatch::Symbolic(a, b) => {
                let slot = generate_slot_schema(arena, a);
                x = make_par(x, slot);
                t = b;
            }
            _ => break,
        }
    }
    make_seq(x, generate_inside(arena, t, config))
}

/// Build a 1→0 input-slot block schema.
///
/// C++ reference: `drawschema.cpp:627` — `generateInputSlotSchema`.
fn generate_slot_schema(arena: &TreeArena, slot_id: BoxId) -> Box<dyn Schema> {
    let name = match match_box(arena, slot_id) {
        BoxMatch::Slot(i) => format!("[{i}]"),
        _ => "slot".to_owned(),
    };
    make_block(1, 0, name, COLOR_SLOT, "")
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Truncate a display name to at most `max` chars: keep first and last thirds.
///
/// C++ reference: `blockSchema.cpp` — constructor truncation with `gMaxNameSize`.
fn truncate_name(s: String, max: usize) -> String {
    if max == 0 || s.len() <= max {
        return s;
    }
    let third = (max / 3).max(1);
    let head = s.get(..third).unwrap_or(&s[..s.len().min(third)]);
    let tail_start = s.len().saturating_sub(third);
    let tail = &s[tail_start..];
    format!("{head}...{tail}")
}

/// Format a real number the same way C++ `boxpp` does: trim trailing zeros.
fn format_real(r: f64) -> String {
    if r.fract() == 0.0 && r.abs() < 1e15 {
        format!("{r:.1}")
    } else {
        format!("{r}")
    }
}

/// Extract a single integer value from a box node (for route dimensions).
fn extract_int(arena: &TreeArena, b: BoxId) -> Option<i32> {
    match match_box(arena, b) {
        BoxMatch::Int(i) => Some(i),
        BoxMatch::Real(r) => Some(r as i32),
        _ => None,
    }
}

/// Flatten a parallel tree of integers into a `Vec<usize>` (for route tables).
///
/// C++ reference: `drawschema.cpp:172` — `isIntTree`.
fn flatten_int_tree(arena: &TreeArena, b: BoxId) -> Vec<usize> {
    let mut out = Vec::new();
    flatten_int_tree_inner(arena, b, &mut out);
    out
}

fn flatten_int_tree_inner(arena: &TreeArena, b: BoxId, out: &mut Vec<usize>) {
    match match_box(arena, b) {
        BoxMatch::Int(i)  => out.push(i.max(0) as usize),
        BoxMatch::Real(r) => out.push(r.max(0.0) as usize),
        BoxMatch::Par(x, y) => {
            flatten_int_tree_inner(arena, x, out);
            flatten_int_tree_inner(arena, y, out);
        }
        _ => {}
    }
}

/// Extract a display name from a label / identifier node.
fn extract_name(arena: &TreeArena, b: BoxId) -> String {
    // tree_to_str handles Symbol nodes; check StringLiteral separately
    match arena.kind(b) {
        Some(NodeKind::Symbol(s)) | Some(NodeKind::StringLiteral(s)) => return s.to_string(),
        _ => {}
    }
    if let Some(s) = tree_to_str(arena, b) {
        return s.to_owned();
    }
    match match_box(arena, b) {
        BoxMatch::Ident(s) => s.to_owned(),
        BoxMatch::Int(i)   => i.to_string(),
        BoxMatch::Real(r)  => format_real(r),
        _                  => "?".to_owned(),
    }
}

/// Format a numeric node for UI parameter display.
fn format_node(arena: &TreeArena, b: BoxId) -> String {
    match match_box(arena, b) {
        BoxMatch::Int(i)  => i.to_string(),
        BoxMatch::Real(r) => format_real(r),
        _                 => extract_name(arena, b),
    }
}

// ─── Public helpers ───────────────────────────────────────────────────────────

/// Build a `TopSchema` wrapper around a generated schema.
///
/// The inner schema is padded with connector stubs for each input/output,
/// then wrapped in a `TopSchema` (white background, title, output arrows).
///
/// C++ reference: `drawschema.cpp:261` — `writeSchemaFile` inner lambda.
pub fn make_top_schema(
    inner: Box<dyn Schema>,
    name: impl Into<String>,
    link: impl Into<String>,
) -> Box<dyn Schema> {
    use crate::schemas::composed::make_top;

    let ins  = inner.inputs();
    let outs = inner.outputs();
    let with_inputs  = add_schema_inputs(ins, inner);
    let with_outputs = add_schema_outputs(outs, with_inputs);
    make_top(with_outputs, 20.0, name, link)
}

/// Prepend `n` connector stubs as inputs.
///
/// C++ reference: `drawschema.cpp:665` — `addSchemaInputs`.
fn add_schema_inputs(n: usize, x: Box<dyn Schema>) -> Box<dyn Schema> {
    if n == 0 { return x; }
    let mut y: Option<Box<dyn Schema>> = None;
    for _ in 0..n {
        let z: Box<dyn Schema> = Box::new(ConnectorSchema::new());
        y = Some(match y {
            None    => z,
            Some(p) => make_par(p, z),
        });
    }
    make_seq(y.unwrap(), x)
}

/// Append `n` connector stubs as outputs.
///
/// C++ reference: `drawschema.cpp:683` — `addSchemaOutputs`.
fn add_schema_outputs(n: usize, x: Box<dyn Schema>) -> Box<dyn Schema> {
    if n == 0 { return x; }
    let mut y: Option<Box<dyn Schema>> = None;
    for _ in 0..n {
        let z: Box<dyn Schema> = Box::new(ConnectorSchema::new());
        y = Some(match y {
            None    => z,
            Some(p) => make_par(p, z),
        });
    }
    make_seq(x, y.unwrap())
}
