//! [`MergeSchema`] — merge composition `s1 :> s2`.
//!
//! C++ reference: `schema/mergeSchema.h/cpp`.

use crate::device::DrawDevice;
use crate::error::DrawError;
use crate::schema::{Orientation, Placement, Point, Schema, Trait, TraitCollector, D_WIRE};
use crate::schemas::composed::make_enlarged;

/// Merge composition: outputs of `s1` are merged (fan-in) to inputs of `s2`.
///
/// C++ reference: `mergeSchema.cpp:35` — `schema* makeMergeSchema`.
pub struct MergeSchema {
    width: f64,
    height: f64,
    s1: Box<dyn Schema>,
    s2: Box<dyn Schema>,
    horz_gap: f64,
    placement: Option<Placement>,
}

/// Build a `MergeSchema`.
///
/// C++ reference: `mergeSchema.cpp:35` — `schema* makeMergeSchema`.
pub fn make_merge(s1: Box<dyn Schema>, s2: Box<dyn Schema>) -> Box<dyn Schema> {
    let a = make_enlarged(s1, D_WIRE);
    let b = make_enlarged(s2, D_WIRE);
    let hgap = (a.height() + b.height()) / 4.0;
    let w = a.width() + b.width() + hgap;
    let h = a.height().max(b.height());
    Box::new(MergeSchema { width: w, height: h, s1: a, s2: b, horz_gap: hgap, placement: None })
}

impl Schema for MergeSchema {
    fn width(&self) -> f64 { self.width }
    fn height(&self) -> f64 { self.height }
    fn inputs(&self) -> usize { self.s1.inputs() }
    fn outputs(&self) -> usize { self.s2.outputs() }

    fn place(&mut self, ox: f64, oy: f64, orientation: Orientation) {
        self.placement = Some(Placement { x: ox, y: oy, orientation });
        let dy1 = (self.s2.height() - self.s1.height()).max(0.0) / 2.0;
        let dy2 = (self.s1.height() - self.s2.height()).max(0.0) / 2.0;
        match orientation {
            Orientation::LeftRight => {
                self.s1.place(ox, oy + dy1, orientation);
                self.s2.place(ox + self.s1.width() + self.horz_gap, oy + dy2, orientation);
            }
            Orientation::RightLeft => {
                self.s2.place(ox, oy + dy2, orientation);
                self.s1.place(ox + self.s2.width() + self.horz_gap, oy + dy1, orientation);
            }
        }
    }

    fn placed(&self) -> bool { self.placement.is_some() }
    fn placement(&self) -> Option<&Placement> { self.placement.as_ref() }

    fn input_point(&self, i: usize) -> Point { self.s1.input_point(i) }
    fn output_point(&self, i: usize) -> Point { self.s2.output_point(i) }

    fn draw(&self, dev: &mut dyn DrawDevice) -> Result<(), DrawError> {
        assert!(self.placed());
        self.s1.draw(dev)?;
        self.s2.draw(dev)
    }

    /// C++ reference: `mergeSchema.cpp:122` — `mergeSchema::collectTraits`.
    fn collect_traits(&self, c: &mut TraitCollector) {
        assert!(self.placed());
        self.s1.collect_traits(c);
        self.s2.collect_traits(c);

        let r = self.s2.inputs();
        if r > 0 {
            for i in 0..self.s1.outputs() {
                let p = self.s1.output_point(i);
                let q = self.s2.input_point(i % r);
                c.add_trait(Trait::new(p, q));
            }
        }
    }
}
