//! [`BlockSchema`] and [`InverterSchema`] — the most fundamental leaf schemas.
//!
//! A `BlockSchema` is a colored rectangle with a label, optional link, and a set
//! of input/output wire endpoints.  `InverterSchema` is a thin subtype that renders
//! as a filled triangle instead of a rectangle.
//!
//! C++ references:
//! - `schema/blockSchema.h/cpp` — `blockSchema` class.
//! - `schema/inverterSchema.h/cpp` — `inverterSchema` (derives `blockSchema`).

use crate::device::DrawDevice;
use crate::error::DrawError;
use crate::schema::{
    D_HORZ, D_LETTER, D_VERT, D_WIRE, Orientation, Placement, Point, Schema, Trait, TraitCollector,
};

// ─── Quantize ─────────────────────────────────────────────────────────────────

/// Round `n` up to the next multiple of `q = 3`, then multiply by `D_LETTER`.
///
/// C++ reference: `blockSchema.cpp:30` — `static double quantize(int n)`.
fn quantize(n: usize) -> f64 {
    let q = 3_usize;
    D_LETTER * (q * n.div_ceil(q)) as f64
}

// ─── BlockSchema ──────────────────────────────────────────────────────────────

/// A colored rectangle with label text, optional hyperlink, and wire endpoints.
///
/// # Sizing
/// ```text
/// w = 2*D_HORZ + max(3*D_WIRE, quantize(len(text)))
/// h = 2*D_VERT + max(3*D_WIRE, max(inputs, outputs) * D_WIRE)
/// ```
///
/// C++ reference: `schema/blockSchema.cpp`.
pub struct BlockSchema {
    // ── fixed geometry ──
    width: f64,
    height: f64,
    inputs: usize,
    outputs: usize,
    // ── label / style ──
    pub(crate) text: String,
    pub(crate) color: String,
    pub(crate) link: String,
    // ── placement ──
    placement: Option<Placement>,
    input_points: Vec<Point>,
    output_points: Vec<Point>,
}

impl BlockSchema {
    /// Construct a `BlockSchema` and compute its optimal size.
    ///
    /// C++ reference: `blockSchema.cpp:41` — `schema* makeBlockSchema(…)`.
    pub fn new(
        inputs: usize,
        outputs: usize,
        text: impl Into<String>,
        color: impl Into<String>,
        link: impl Into<String>,
    ) -> Self {
        let text = text.into();
        let minimal = 3.0 * D_WIRE;
        let w = 2.0 * D_HORZ + minimal.max(quantize(text.chars().count()));
        let h = 2.0 * D_VERT + minimal.max((inputs.max(outputs) as f64) * D_WIRE);
        Self {
            width: w,
            height: h,
            inputs,
            outputs,
            text,
            color: color.into(),
            link: link.into(),
            placement: None,
            input_points: vec![Point::default(); inputs],
            output_points: vec![Point::default(); outputs],
        }
    }

    fn place_input_points(&mut self) {
        let p = self.placement.unwrap();
        let n = self.inputs as f64;
        match p.orientation {
            Orientation::LeftRight => {
                let px = p.x;
                let py = p.y + (self.height - D_WIRE * (n - 1.0)) / 2.0;
                for i in 0..self.inputs {
                    self.input_points[i] = Point::new(px, py + i as f64 * D_WIRE);
                }
            }
            Orientation::RightLeft => {
                let px = p.x + self.width;
                let py = p.y + self.height - (self.height - D_WIRE * (n - 1.0)) / 2.0;
                for i in 0..self.inputs {
                    self.input_points[i] = Point::new(px, py - i as f64 * D_WIRE);
                }
            }
        }
    }

    fn place_output_points(&mut self) {
        let p = self.placement.unwrap();
        let n = self.outputs as f64;
        match p.orientation {
            Orientation::LeftRight => {
                let px = p.x + self.width;
                let py = p.y + (self.height - D_WIRE * (n - 1.0)) / 2.0;
                for i in 0..self.outputs {
                    self.output_points[i] = Point::new(px, py + i as f64 * D_WIRE);
                }
            }
            Orientation::RightLeft => {
                let px = p.x;
                let py = p.y + self.height - (self.height - D_WIRE * (n - 1.0)) / 2.0;
                for i in 0..self.outputs {
                    self.output_points[i] = Point::new(px, py - i as f64 * D_WIRE);
                }
            }
        }
    }
}

impl Schema for BlockSchema {
    fn width(&self) -> f64 {
        self.width
    }
    fn height(&self) -> f64 {
        self.height
    }
    fn inputs(&self) -> usize {
        self.inputs
    }
    fn outputs(&self) -> usize {
        self.outputs
    }

    fn place(&mut self, x: f64, y: f64, orientation: Orientation) {
        self.placement = Some(Placement { x, y, orientation });
        self.place_input_points();
        self.place_output_points();
    }

    fn placed(&self) -> bool {
        self.placement.is_some()
    }
    fn placement(&self) -> Option<&Placement> {
        self.placement.as_ref()
    }

    fn input_point(&self, i: usize) -> Point {
        assert!(self.placed(), "BlockSchema not yet placed");
        self.input_points[i]
    }

    fn output_point(&self, i: usize) -> Point {
        assert!(self.placed(), "BlockSchema not yet placed");
        self.output_points[i]
    }

    /// C++ reference: `blockSchema.cpp:162` — `blockSchema::draw`.
    fn draw(&self, dev: &mut dyn DrawDevice) -> Result<(), DrawError> {
        assert!(self.placed());
        let p = self.placement.unwrap();

        // rectangle
        dev.rect(
            p.x + D_HORZ,
            p.y + D_VERT,
            self.width - 2.0 * D_HORZ,
            self.height - 2.0 * D_VERT,
            &self.color,
            &self.link,
        )?;

        // centered text
        dev.text(
            p.x + self.width / 2.0,
            p.y + self.height / 2.0,
            &self.text,
            &self.link,
        )?;

        // orientation mark
        let (mx, my) = match p.orientation {
            Orientation::LeftRight => (p.x + D_HORZ, p.y + D_VERT),
            Orientation::RightLeft => (p.x + self.width - D_HORZ, p.y + self.height - D_VERT),
        };
        dev.mark_direction(mx, my, p.orientation.sign() as i32)?;

        // input arrows
        let dx = if p.orientation == Orientation::LeftRight {
            D_HORZ
        } else {
            -D_HORZ
        };
        for pt in &self.input_points {
            dev.arrow(pt.x + dx, pt.y, 0.0, p.orientation.sign() as i32)?;
        }

        Ok(())
    }

    /// C++ reference: `blockSchema.cpp:226` — `blockSchema::collectTraits`.
    fn collect_traits(&self, c: &mut TraitCollector) {
        assert!(self.placed());
        let p = self.placement.unwrap();
        let dx = if p.orientation == Orientation::LeftRight {
            D_HORZ
        } else {
            -D_HORZ
        };

        // input wires: external end → rect border
        for pt in &self.input_points {
            c.add_trait(Trait::new(*pt, Point::new(pt.x + dx, pt.y)));
            c.add_input(Point::new(pt.x + dx, pt.y));
        }

        // output wires: rect border → external end
        for pt in &self.output_points {
            c.add_trait(Trait::new(Point::new(pt.x - dx, pt.y), *pt));
            c.add_output(Point::new(pt.x - dx, pt.y));
        }
    }
}

/// Convenience constructor returning `Box<dyn Schema>`.
pub fn make_block(
    inputs: usize,
    outputs: usize,
    text: impl Into<String>,
    color: impl Into<String>,
    link: impl Into<String>,
) -> Box<dyn crate::schema::Schema> {
    Box::new(BlockSchema::new(inputs, outputs, text, color, link))
}

// ─── InverterSchema ───────────────────────────────────────────────────────────

/// A `*(-1)` operator displayed as a filled triangle.
///
/// Derives from `BlockSchema` with fixed size 2.5×D_WIRE × D_WIRE.
///
/// C++ reference: `schema/inverterSchema.cpp`.
pub struct InverterSchema {
    inner: BlockSchema,
}

impl InverterSchema {
    /// C++ reference: `inverterSchema.cpp:41` — `inverterSchema::inverterSchema`.
    pub fn new(color: impl Into<String>) -> Self {
        Self {
            inner: BlockSchema {
                width: 2.5 * D_WIRE,
                height: D_WIRE,
                inputs: 1,
                outputs: 1,
                text: "-1".into(),
                color: color.into(),
                link: String::new(),
                placement: None,
                input_points: vec![Point::default()],
                output_points: vec![Point::default()],
            },
        }
    }
}

impl Schema for InverterSchema {
    fn width(&self) -> f64 {
        self.inner.width()
    }
    fn height(&self) -> f64 {
        self.inner.height()
    }
    fn inputs(&self) -> usize {
        self.inner.inputs()
    }
    fn outputs(&self) -> usize {
        self.inner.outputs()
    }

    fn place(&mut self, x: f64, y: f64, orientation: Orientation) {
        self.inner.place(x, y, orientation);
    }

    fn placed(&self) -> bool {
        self.inner.placed()
    }
    fn placement(&self) -> Option<&Placement> {
        self.inner.placement()
    }
    fn input_point(&self, i: usize) -> Point {
        self.inner.input_point(i)
    }
    fn output_point(&self, i: usize) -> Point {
        self.inner.output_point(i)
    }

    /// Triangle instead of rectangle.
    ///
    /// C++ reference: `inverterSchema.cpp:50` — `inverterSchema::draw`.
    fn draw(&self, dev: &mut dyn DrawDevice) -> Result<(), DrawError> {
        assert!(self.inner.placed());
        let p = self.inner.placement.unwrap();
        dev.triangle(
            p.x + D_HORZ,
            p.y + 0.5,
            self.inner.width - 2.0 * D_HORZ,
            self.inner.height - 1.0,
            &self.inner.color,
            &self.inner.link,
            p.orientation == Orientation::LeftRight,
        )?;
        Ok(())
    }

    fn collect_traits(&self, c: &mut TraitCollector) {
        self.inner.collect_traits(c);
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_sizing() {
        let b = BlockSchema::new(2, 1, "test", "#000", "");
        assert!(b.width() > 0.0);
        assert!(b.height() >= 3.0 * D_WIRE + 2.0 * D_VERT);
        assert_eq!(b.inputs(), 2);
        assert_eq!(b.outputs(), 1);
    }

    #[test]
    fn test_block_sizing_zero_ports() {
        let b = BlockSchema::new(0, 1, "sin", "#000", "");
        // minimal = 3*dWire; with 0/1 ports max(0,1)*dWire < 3*dWire, so height = minimal+2*dVert
        assert_eq!(b.height(), 3.0 * D_WIRE + 2.0 * D_VERT);
    }

    #[test]
    fn test_block_place_and_input_point() {
        let mut b = BlockSchema::new(1, 1, "add", "#000", "");
        b.place(0.0, 0.0, Orientation::LeftRight);
        let ip = b.input_point(0);
        assert_eq!(ip.x, 0.0); // input at x=0 for LeftRight
    }

    #[test]
    fn test_inverter_sizing() {
        let inv = InverterSchema::new("#fff");
        assert_eq!(inv.width(), 2.5 * D_WIRE);
        assert_eq!(inv.height(), D_WIRE);
    }
}
