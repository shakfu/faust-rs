//! Core schema abstractions: [`Schema`] trait, [`Point`], [`Trait`], [`TraitCollector`].
//!
//! Every visual element in a Faust block diagram is a [`Schema`].  Schemas form a tree:
//! leaf schemas (blocks, cables, cuts …) live at the leaves; composite schemas (seq, par, …)
//! wrap children.  The rendering pipeline has three steps:
//!
//! 1. **Sizing** — sizes are computed bottom-up in each schema's constructor.
//! 2. **Placement** — [`Schema::place`] is called top-down, assigning x/y coordinates.
//! 3. **Drawing** — [`Schema::draw`] emits SVG elements; [`Schema::collect_traits`] gathers
//!    wires that are then filtered by [`TraitCollector`] and drawn last.
//!
//! C++ references: `schema/schema.h`, `schema/collector.cpp`.

use std::collections::BTreeSet;

use crate::device::DrawDevice;
use crate::error::DrawError;

// ─── Layout constants ──────────────────────────────────────────────────────────

/// Distance between two parallel wires.
///
/// C++ reference: `schema.h:32` — `const double dWire = 8`
pub const D_WIRE: f64 = 8.0;

/// Approximate width of one character in the label font.
///
/// C++ reference: `schema.h:34` — `const double dLetter = 4.3`
pub const D_LETTER: f64 = 4.3;

/// Horizontal inner margin inside a block.
///
/// C++ reference: `schema.h:35` — `const double dHorz = 4`
pub const D_HORZ: f64 = 4.0;

/// Vertical inner margin inside a block.
///
/// C++ reference: `schema.h:36` — `const double dVert = 4`
pub const D_VERT: f64 = 4.0;

// ─── Color palette ─────────────────────────────────────────────────────────────

/// Fill color for block schemas that carry a link to a sub-diagram.
///
/// C++ reference: `drawschema.cpp:117` — `#define linkcolor "#003366"`
pub const COLOR_LINK: &str = "#003366";

/// Fill color for normal operator/primitive blocks.
///
/// C++ reference: `drawschema.cpp:118` — `#define normalcolor "#4B71A1"`
pub const COLOR_NORMAL: &str = "#4B71A1";

/// Fill color for UI control blocks.
///
/// C++ reference: `drawschema.cpp:119` — `#define uicolor "#477881"`
pub const COLOR_UI: &str = "#477881";

/// Fill color for slot (input/output variable) blocks.
///
/// C++ reference: `drawschema.cpp:120` — `#define slotcolor "#47945E"`
pub const COLOR_SLOT: &str = "#47945E";

/// Fill color for numeric constant blocks.
///
/// C++ reference: `drawschema.cpp:121` — `#define numcolor "#f44800"`
pub const COLOR_NUM: &str = "#f44800";

/// Fill color for inverter (`*(-1)`) triangles.
///
/// C++ reference: `drawschema.cpp:122` — `#define invcolor "#ffffff"`
pub const COLOR_INV: &str = "#ffffff";

// ─── Orientation ───────────────────────────────────────────────────────────────

/// Drawing orientation: left-to-right or right-to-left.
///
/// C++ reference: `schema.h:100` — `enum { kLeftRight = 1, kRightLeft = -1 }`
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Orientation {
    #[default]
    LeftRight,
    RightLeft,
}

impl Orientation {
    /// `1` for `LeftRight`, `-1` for `RightLeft` (matches C++ enum values).
    pub fn sign(self) -> f64 {
        match self {
            Orientation::LeftRight => 1.0,
            Orientation::RightLeft => -1.0,
        }
    }
}

// ─── Point ─────────────────────────────────────────────────────────────────────

/// A 2-D coordinate used for wire endpoints and port positions.
///
/// C++ reference: `schema.h:38` — `struct point`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

impl Eq for Point {}

impl Ord for Point {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap_or(std::cmp::Ordering::Equal)
    }
}

impl PartialOrd for Point {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match self.x.partial_cmp(&other.x) {
            Some(std::cmp::Ordering::Equal) => self.y.partial_cmp(&other.y),
            c => c,
        }
    }
}

// ─── Trait (wire segment) ──────────────────────────────────────────────────────

/// A directed wire segment from `start` to `end`.
///
/// C++ reference: `schema.h:59` — `struct trait`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trait {
    pub start: Point,
    pub end: Point,
    pub has_real_input: bool,
    pub has_real_output: bool,
}

impl Trait {
    pub fn new(start: Point, end: Point) -> Self {
        Self { start, end, has_real_input: false, has_real_output: false }
    }
}

impl Ord for Trait {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap_or(std::cmp::Ordering::Equal)
    }
}

impl PartialOrd for Trait {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match self.start.partial_cmp(&other.start) {
            Some(std::cmp::Ordering::Equal) => self.end.partial_cmp(&other.end),
            c => c,
        }
    }
}

// ─── TraitCollector ────────────────────────────────────────────────────────────

/// Collects wire segments and filters them for visibility before rendering.
///
/// A wire is visible if it connects a real output (registered via [`add_output`])
/// to a real input (registered via [`add_input`]).  The propagation loop in
/// [`compute_visible`] extends the reachable endpoint sets transitively.
///
/// C++ reference: `schema.h:85` — `struct collector`; `schema/collector.cpp`.
#[derive(Default)]
pub struct TraitCollector {
    outputs: BTreeSet<Point>,
    inputs: BTreeSet<Point>,
    traits: BTreeSet<Trait>,
    with_input: BTreeSet<Trait>,
    with_output: BTreeSet<Trait>,
}

impl TraitCollector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `p` as a real output endpoint (wire source).
    pub fn add_output(&mut self, p: Point) {
        self.outputs.insert(p);
    }

    /// Register `p` as a real input endpoint (wire sink).
    pub fn add_input(&mut self, p: Point) {
        self.inputs.insert(p);
    }

    /// Add a wire segment to the collection.
    pub fn add_trait(&mut self, t: Trait) {
        self.traits.insert(t);
    }

    /// Propagate reachability and mark visible wires.
    ///
    /// A wire with `start` reachable from a real output gets marked `has_real_input`
    /// (it has a real upstream source).  A wire with `end` reachable from a real
    /// input gets marked `has_real_output`.  Visible = both.
    ///
    /// C++ reference: `collector.cpp:26` — `computeVisibleTraits`.
    fn compute_visible(&mut self) {
        loop {
            let mut modified = false;
            for t in &self.traits {
                if !self.with_input.contains(t) && self.outputs.contains(&t.start) {
                    let mut t2 = *t;
                    t2.has_real_input = true;
                    self.with_input.insert(t2);
                    self.outputs.insert(t.end);
                    modified = true;
                }
                if !self.with_output.contains(t) && self.inputs.contains(&t.end) {
                    let mut t2 = *t;
                    t2.has_real_output = true;
                    self.with_output.insert(t2);
                    self.inputs.insert(t.start);
                    modified = true;
                }
            }
            if !modified {
                break;
            }
        }
    }

    fn is_visible(&self, t: &Trait) -> bool {
        self.with_input.contains(t) && self.with_output.contains(t)
    }

    /// Draw all visible wire segments to `dev`.
    ///
    /// C++ reference: `collector.cpp:56` — `collector::draw`.
    pub fn draw(&mut self, dev: &mut dyn DrawDevice) -> Result<(), DrawError> {
        self.compute_visible();
        let visible: Vec<Trait> =
            self.traits.iter().filter(|t| self.is_visible(t)).copied().collect();
        for t in &visible {
            dev.line(t.start.x, t.start.y, t.end.x, t.end.y)?;
        }
        Ok(())
    }
}

// ─── Placement state ───────────────────────────────────────────────────────────

/// Placement coordinates recorded by [`Schema::place`].
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub x: f64,
    pub y: f64,
    pub orientation: Orientation,
}

// ─── Schema trait ──────────────────────────────────────────────────────────────

/// Abstract block-diagram schema.
///
/// Sizes (`width`, `height`, `inputs`, `outputs`) are fixed at construction time.
/// Coordinates are set by [`place`](Schema::place) (top-down pass) and then used by
/// [`draw`](Schema::draw) and [`collect_traits`](Schema::collect_traits).
///
/// C++ reference: `schema.h:106` — `class schema`.
pub trait Schema {
    // ── immutable geometry ──

    fn width(&self) -> f64;
    fn height(&self) -> f64;
    fn inputs(&self) -> usize;
    fn outputs(&self) -> usize;

    // ── placement (top-down pass) ──

    /// Assign position and orientation; called exactly once before drawing.
    fn place(&mut self, x: f64, y: f64, orientation: Orientation);

    /// `true` after [`place`](Schema::place) has been called.
    fn placed(&self) -> bool;

    fn placement(&self) -> Option<&Placement>;

    fn x(&self) -> f64 {
        self.placement().map_or(0.0, |p| p.x)
    }
    fn y(&self) -> f64 {
        self.placement().map_or(0.0, |p| p.y)
    }
    fn orientation(&self) -> Orientation {
        self.placement().map_or(Orientation::LeftRight, |p| p.orientation)
    }

    // ── port coordinates (valid after place) ──

    fn input_point(&self, i: usize) -> Point;
    fn output_point(&self, i: usize) -> Point;

    // ── rendering ──

    /// Emit SVG elements for this schema to `dev`.
    fn draw(&self, dev: &mut dyn DrawDevice) -> Result<(), DrawError>;

    /// Add wire segments to `collector` for later filtering and rendering.
    fn collect_traits(&self, collector: &mut TraitCollector);
}
