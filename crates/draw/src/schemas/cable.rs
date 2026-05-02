//! Leaf schemas: [`CableSchema`], [`CutSchema`], [`ConnectorSchema`].
//!
//! These are the simplest schemas — zero-width cables, terminators, and invisible
//! connector squares used to pad unused I/O.
//!
//! C++ references:
//! - `schema/cableSchema.h/cpp`   — `cableSchema`
//! - `schema/cutSchema.h/cpp`     — `cutSchema`
//! - `schema/connectorSchema.h/cpp` — `connectorSchema`

use crate::device::DrawDevice;
use crate::error::DrawError;
use crate::schema::{D_HORZ, D_WIRE, Orientation, Placement, Point, Schema, Trait, TraitCollector};

// ─── CableSchema ──────────────────────────────────────────────────────────────

/// `n` parallel wire pass-throughs (zero width, height = n × D_WIRE).
///
/// C++ reference: `schema/cableSchema.cpp:41` — `cableSchema::cableSchema`.
pub struct CableSchema {
    n: usize,
    placement: Option<Placement>,
    points: Vec<Point>,
}

impl CableSchema {
    /// C++ reference: `cableSchema.cpp:32` — `schema* makeCableSchema(unsigned int n)`.
    pub fn new(n: usize) -> Self {
        assert!(n > 0, "CableSchema requires n > 0");
        Self {
            n,
            placement: None,
            points: vec![Point::default(); n],
        }
    }
}

impl Schema for CableSchema {
    fn width(&self) -> f64 {
        0.0
    }
    fn height(&self) -> f64 {
        self.n as f64 * D_WIRE
    }
    fn inputs(&self) -> usize {
        self.n
    }
    fn outputs(&self) -> usize {
        self.n
    }

    fn place(&mut self, x: f64, y: f64, orientation: Orientation) {
        self.placement = Some(Placement { x, y, orientation });
        match orientation {
            Orientation::LeftRight => {
                for i in 0..self.n {
                    self.points[i] = Point::new(x, y + D_WIRE / 2.0 + i as f64 * D_WIRE);
                }
            }
            Orientation::RightLeft => {
                let h = self.height();
                for i in 0..self.n {
                    self.points[i] = Point::new(x, y + h - D_WIRE / 2.0 - i as f64 * D_WIRE);
                }
            }
        }
    }

    fn placed(&self) -> bool {
        self.placement.is_some()
    }
    fn placement(&self) -> Option<&Placement> {
        self.placement.as_ref()
    }

    fn input_point(&self, i: usize) -> Point {
        self.points[i]
    }
    fn output_point(&self, i: usize) -> Point {
        self.points[i]
    }

    /// Nothing to draw — wires appear only when the schema is enlarged.
    fn draw(&self, _dev: &mut dyn DrawDevice) -> Result<(), DrawError> {
        Ok(())
    }
    fn collect_traits(&self, _c: &mut TraitCollector) {}
}

// ─── CutSchema ────────────────────────────────────────────────────────────────

/// A terminator: 1 input, 0 outputs.  Visually a tiny dot (not currently drawn).
///
/// C++ reference: `schema/cutSchema.cpp:41` — `cutSchema::cutSchema`.
pub struct CutSchema {
    placement: Option<Placement>,
    point: Point,
}

impl CutSchema {
    /// C++ reference: `cutSchema.cpp:30` — `schema* makeCutSchema()`.
    pub fn new() -> Self {
        Self {
            placement: None,
            point: Point::default(),
        }
    }
}

impl Default for CutSchema {
    fn default() -> Self {
        Self::new()
    }
}

impl Schema for CutSchema {
    fn width(&self) -> f64 {
        0.0
    }
    fn height(&self) -> f64 {
        D_WIRE / 100.0
    }
    fn inputs(&self) -> usize {
        1
    }
    fn outputs(&self) -> usize {
        0
    }

    fn place(&mut self, x: f64, y: f64, orientation: Orientation) {
        self.placement = Some(Placement { x, y, orientation });
        self.point = Point::new(x, y + self.height() * 0.5);
    }

    fn placed(&self) -> bool {
        self.placement.is_some()
    }
    fn placement(&self) -> Option<&Placement> {
        self.placement.as_ref()
    }

    fn input_point(&self, i: usize) -> Point {
        assert_eq!(i, 0);
        self.point
    }

    fn output_point(&self, _i: usize) -> Point {
        panic!("CutSchema has no outputs")
    }

    /// Nothing visible.
    fn draw(&self, _dev: &mut dyn DrawDevice) -> Result<(), DrawError> {
        Ok(())
    }
    fn collect_traits(&self, _c: &mut TraitCollector) {}
}

// ─── ConnectorSchema ──────────────────────────────────────────────────────────

/// An invisible 1→1 square of size D_WIRE used to pad unused I/O ports.
///
/// C++ reference: `schema/connectorSchema.cpp:40` — `connectorSchema::connectorSchema`.
pub struct ConnectorSchema {
    placement: Option<Placement>,
    input_pt: Point,
    output_pt: Point,
}

impl ConnectorSchema {
    /// C++ reference: `connectorSchema.cpp:31` — `schema* makeConnectorSchema()`.
    pub fn new() -> Self {
        Self {
            placement: None,
            input_pt: Point::default(),
            output_pt: Point::default(),
        }
    }
}

impl Default for ConnectorSchema {
    fn default() -> Self {
        Self::new()
    }
}

impl Schema for ConnectorSchema {
    fn width(&self) -> f64 {
        D_WIRE
    }
    fn height(&self) -> f64 {
        D_WIRE
    }
    fn inputs(&self) -> usize {
        1
    }
    fn outputs(&self) -> usize {
        1
    }

    fn place(&mut self, x: f64, y: f64, orientation: Orientation) {
        self.placement = Some(Placement { x, y, orientation });
        let h = self.height();
        match orientation {
            Orientation::LeftRight => {
                let py = y + (h - D_WIRE * 0.0) / 2.0; // N=1 → (h - 0)/2
                self.input_pt = Point::new(x, py);
                self.output_pt = Point::new(x + D_WIRE, py);
            }
            Orientation::RightLeft => {
                let py = y + h - (h - D_WIRE * 0.0) / 2.0;
                self.input_pt = Point::new(x + D_WIRE, py);
                self.output_pt = Point::new(x, py);
            }
        }
    }

    fn placed(&self) -> bool {
        self.placement.is_some()
    }
    fn placement(&self) -> Option<&Placement> {
        self.placement.as_ref()
    }

    fn input_point(&self, i: usize) -> Point {
        assert_eq!(i, 0);
        self.input_pt
    }

    fn output_point(&self, i: usize) -> Point {
        assert_eq!(i, 0);
        self.output_pt
    }

    fn draw(&self, _dev: &mut dyn DrawDevice) -> Result<(), DrawError> {
        Ok(())
    }

    /// C++ reference: `connectorSchema.cpp:147` — `connectorSchema::collectTraits`.
    fn collect_traits(&self, c: &mut TraitCollector) {
        assert!(self.placed());
        let p = self.placement.unwrap();
        let dx = if p.orientation == Orientation::LeftRight {
            D_HORZ
        } else {
            -D_HORZ
        };

        let ip = self.input_pt;
        c.add_trait(Trait::new(ip, Point::new(ip.x + dx, ip.y)));
        c.add_input(Point::new(ip.x + dx, ip.y));

        let op = self.output_pt;
        c.add_trait(Trait::new(Point::new(op.x - dx, op.y), op));
        c.add_output(Point::new(op.x - dx, op.y));
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cable_width_zero() {
        assert_eq!(CableSchema::new(1).width(), 0.0);
    }

    #[test]
    fn test_cable_height_proportional() {
        assert_eq!(CableSchema::new(3).height(), 3.0 * D_WIRE);
    }

    #[test]
    fn test_cable_points_same() {
        let mut c = CableSchema::new(2);
        c.place(0.0, 0.0, Orientation::LeftRight);
        assert_eq!(c.input_point(0), c.output_point(0));
    }

    #[test]
    fn test_cut_schema() {
        let mut cut = CutSchema::new();
        assert_eq!(cut.inputs(), 1);
        assert_eq!(cut.outputs(), 0);
        cut.place(0.0, 0.0, Orientation::LeftRight);
        let _ = cut.input_point(0);
    }

    #[test]
    fn test_connector_ports() {
        let mut conn = ConnectorSchema::new();
        assert_eq!(conn.inputs(), 1);
        assert_eq!(conn.outputs(), 1);
        conn.place(0.0, 0.0, Orientation::LeftRight);
        let ip = conn.input_point(0);
        let op = conn.output_point(0);
        assert!(
            op.x > ip.x,
            "output should be to the right of input in LeftRight"
        );
    }
}
