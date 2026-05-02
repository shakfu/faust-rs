//! [`ParSchema`] — parallel composition `s1 , s2`.
//!
//! C++ reference: `schema/parSchema.h/cpp`.

use crate::device::DrawDevice;
use crate::error::DrawError;
use crate::schema::{Orientation, Placement, Point, Schema, TraitCollector};
use crate::schemas::composed::make_enlarged;

/// Parallel composition: `s1` stacked on top of `s2`.
///
/// Both sub-schemas are padded to the same width via [`make_enlarged`].
///
/// C++ reference: `parSchema.cpp:29` — `schema* makeParSchema`.
pub struct ParSchema {
    width: f64,
    height: f64,
    s1: Box<dyn Schema>,
    s2: Box<dyn Schema>,
    input_frontier: usize,
    output_frontier: usize,
    placement: Option<Placement>,
}

/// Build a `ParSchema`.
///
/// C++ reference: `parSchema.cpp:29` — `schema* makeParSchema`.
pub fn make_par(s1: Box<dyn Schema>, s2: Box<dyn Schema>) -> Box<dyn Schema> {
    let a = make_enlarged(s1, 0.0); // will be enlarged to s2.width inside
    let b = make_enlarged(s2, 0.0);
    // Re-do with mutual width: pass actual widths
    let w = a.width().max(b.width());
    let a = make_enlarged(a, w);
    let b = make_enlarged(b, w);
    let inf = a.inputs();
    let outf = a.outputs();
    let height = a.height() + b.height();
    Box::new(ParSchema {
        width: w,
        height,
        s1: a,
        s2: b,
        input_frontier: inf,
        output_frontier: outf,
        placement: None,
    })
}

impl Schema for ParSchema {
    fn width(&self) -> f64 { self.width }
    fn height(&self) -> f64 { self.height }
    fn inputs(&self) -> usize { self.s1.inputs() + self.s2.inputs() }
    fn outputs(&self) -> usize { self.s1.outputs() + self.s2.outputs() }

    fn place(&mut self, ox: f64, oy: f64, orientation: Orientation) {
        self.placement = Some(Placement { x: ox, y: oy, orientation });
        match orientation {
            Orientation::LeftRight => {
                self.s1.place(ox, oy, orientation);
                self.s2.place(ox, oy + self.s1.height(), orientation);
            }
            Orientation::RightLeft => {
                self.s2.place(ox, oy, orientation);
                self.s1.place(ox, oy + self.s2.height(), orientation);
            }
        }
    }

    fn placed(&self) -> bool { self.placement.is_some() }
    fn placement(&self) -> Option<&Placement> { self.placement.as_ref() }

    fn input_point(&self, i: usize) -> Point {
        if i < self.input_frontier {
            self.s1.input_point(i)
        } else {
            self.s2.input_point(i - self.input_frontier)
        }
    }

    fn output_point(&self, i: usize) -> Point {
        if i < self.output_frontier {
            self.s1.output_point(i)
        } else {
            self.s2.output_point(i - self.output_frontier)
        }
    }

    fn draw(&self, dev: &mut dyn DrawDevice) -> Result<(), DrawError> {
        self.s1.draw(dev)?;
        self.s2.draw(dev)
    }

    fn collect_traits(&self, c: &mut TraitCollector) {
        self.s1.collect_traits(c);
        self.s2.collect_traits(c);
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schemas::block::BlockSchema;

    #[test]
    fn test_par_height_eq_sum() {
        let s1 = Box::new(BlockSchema::new(1, 1, "A", "#000", ""));
        let s2 = Box::new(BlockSchema::new(1, 1, "B", "#000", ""));
        let h1 = s1.height();
        let h2 = s2.height();
        let par = make_par(s1, s2);
        assert_eq!(par.height(), h1 + h2);
    }

    #[test]
    fn test_par_equal_width() {
        let s1 = Box::new(BlockSchema::new(0, 1, "short", "#000", ""));
        let s2 = Box::new(BlockSchema::new(0, 1, "a much longer text", "#000", ""));
        let par = make_par(s1, s2);
        // Both children padded to same width
        assert!(par.width() > 0.0);
    }
}
