//! [`SeqSchema`] — sequential composition `s1 : s2`.
//!
//! C++ reference: `schema/seqSchema.h/cpp`.

use crate::device::DrawDevice;
use crate::error::DrawError;
use crate::schema::{D_WIRE, Orientation, Placement, Point, Schema, Trait, TraitCollector};
use crate::schemas::cable::CableSchema;
use crate::schemas::par::make_par;

// ─── Direction helpers ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dir {
    Hor,
    Up,
    Down,
}

fn direction(a: Point, b: Point) -> Dir {
    if a.y > b.y {
        Dir::Up
    } else if a.y < b.y {
        Dir::Down
    } else {
        Dir::Hor
    }
}

// ─── computeHorzGap ────────────────────────────────────────────────────────────

/// Horizontal gap needed to route internal connections without crossings.
///
/// C++ reference: `seqSchema.cpp:343` — `static double computeHorzGap`.
fn compute_horz_gap(a: &mut dyn Schema, b: &mut dyn Schema) -> f64 {
    if a.outputs() == 0 {
        return 0.0;
    }
    let ya = (b.height() - a.height()).max(0.0) * 0.5;
    let yb = (a.height() - b.height()).max(0.0) * 0.5;
    a.place(0.0, ya, Orientation::LeftRight);
    b.place(0.0, yb, Orientation::LeftRight);

    let mut max_group = [0_usize; 3];
    let mut gdir = direction(a.output_point(0), b.input_point(0));
    let mut gsize = 1_usize;

    for i in 1..a.outputs() {
        let d = direction(a.output_point(i), b.input_point(i));
        if d == gdir {
            gsize += 1;
        } else {
            let idx = match gdir {
                Dir::Up => 0,
                Dir::Down => 1,
                Dir::Hor => 2,
            };
            if gsize > max_group[idx] {
                max_group[idx] = gsize;
            }
            gsize = 1;
            gdir = d;
        }
    }
    let idx = match gdir {
        Dir::Up => 0,
        Dir::Down => 1,
        Dir::Hor => 2,
    };
    if gsize > max_group[idx] {
        max_group[idx] = gsize;
    }

    D_WIRE * max_group[0].max(max_group[1]) as f64
}

// ─── SeqSchema ─────────────────────────────────────────────────────────────────

/// Sequential composition: outputs of `s1` are wired to inputs of `s2`.
///
/// Cables are automatically added if the output count of `s1` does not match the
/// input count of `s2`.
///
/// C++ reference: `seqSchema.cpp:43` — `schema* makeSeqSchema`.
pub struct SeqSchema {
    width: f64,
    height: f64,
    s1: Box<dyn Schema>,
    s2: Box<dyn Schema>,
    horz_gap: f64,
    placement: Option<Placement>,
}

/// Build a `SeqSchema`, balancing port counts with cables as needed.
///
/// C++ reference: `seqSchema.cpp:43` — `schema* makeSeqSchema`.
pub fn make_seq(s1: Box<dyn Schema>, s2: Box<dyn Schema>) -> Box<dyn Schema> {
    let o = s1.outputs();
    let i = s2.inputs();
    let (mut a, mut b): (Box<dyn Schema>, Box<dyn Schema>) = if o < i {
        (make_par(s1, Box::new(CableSchema::new(i - o))), s2)
    } else if o > i {
        (s1, make_par(s2, Box::new(CableSchema::new(o - i))))
    } else {
        (s1, s2)
    };
    let hgap = compute_horz_gap(a.as_mut(), b.as_mut());
    let w = a.width() + hgap + b.width();
    let h = a.height().max(b.height());
    Box::new(SeqSchema {
        width: w,
        height: h,
        s1: a,
        s2: b,
        horz_gap: hgap,
        placement: None,
    })
}

impl Schema for SeqSchema {
    fn width(&self) -> f64 {
        self.width
    }
    fn height(&self) -> f64 {
        self.height
    }
    fn inputs(&self) -> usize {
        self.s1.inputs()
    }
    fn outputs(&self) -> usize {
        self.s2.outputs()
    }

    fn place(&mut self, ox: f64, oy: f64, orientation: Orientation) {
        self.placement = Some(Placement {
            x: ox,
            y: oy,
            orientation,
        });
        let y1 = (self.s2.height() - self.s1.height()).max(0.0) * 0.5;
        let y2 = (self.s1.height() - self.s2.height()).max(0.0) * 0.5;
        match orientation {
            Orientation::LeftRight => {
                self.s1.place(ox, oy + y1, orientation);
                self.s2
                    .place(ox + self.s1.width() + self.horz_gap, oy + y2, orientation);
            }
            Orientation::RightLeft => {
                self.s2.place(ox, oy + y2, orientation);
                self.s1
                    .place(ox + self.s2.width() + self.horz_gap, oy + y1, orientation);
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
        self.s1.input_point(i)
    }
    fn output_point(&self, i: usize) -> Point {
        self.s2.output_point(i)
    }

    fn draw(&self, dev: &mut dyn DrawDevice) -> Result<(), DrawError> {
        assert!(self.placed());
        self.s1.draw(dev)?;
        self.s2.draw(dev)
    }

    /// C++ reference: `seqSchema.cpp:234` — `seqSchema::collectInternalWires`.
    fn collect_traits(&self, c: &mut TraitCollector) {
        assert!(self.placed());
        self.s1.collect_traits(c);
        self.s2.collect_traits(c);
        self.collect_internal_wires(c);
    }
}

impl SeqSchema {
    fn collect_internal_wires(&self, c: &mut TraitCollector) {
        let n = self.s1.outputs();
        let hgap = self.horz_gap;
        let orientation = self
            .placement
            .map_or(Orientation::LeftRight, |p| p.orientation);

        let mut dx = 0.0_f64;
        let mut mx = 0.0_f64;
        let mut dir = Dir::Hor;
        let mut first = true;

        for i in 0..n {
            let src = self.s1.output_point(i);
            let dst = self.s2.input_point(i);
            let d = direction(src, dst);

            if first || d != dir {
                (mx, dx) = match (orientation, d) {
                    (Orientation::LeftRight, Dir::Up) => (0.0, D_WIRE),
                    (Orientation::LeftRight, Dir::Down) => (hgap, -D_WIRE),
                    (Orientation::RightLeft, Dir::Up) => (-hgap, D_WIRE),
                    (Orientation::RightLeft, Dir::Down) => (0.0, -D_WIRE),
                    _ => (0.0, 0.0),
                };
                dir = d;
                first = false;
            } else {
                mx += dx;
            }

            if src.y == dst.y {
                c.add_trait(Trait::new(src, dst));
            } else {
                c.add_trait(Trait::new(src, Point::new(src.x + mx, src.y)));
                c.add_trait(Trait::new(
                    Point::new(src.x + mx, src.y),
                    Point::new(src.x + mx, dst.y),
                ));
                c.add_trait(Trait::new(Point::new(src.x + mx, dst.y), dst));
            }
        }
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schemas::block::BlockSchema;

    #[test]
    fn test_seq_width_geq_sum() {
        let s1 = Box::new(BlockSchema::new(1, 1, "A", "#000", ""));
        let s2 = Box::new(BlockSchema::new(1, 1, "B", "#000", ""));
        let w1 = s1.width();
        let w2 = s2.width();
        let seq = make_seq(s1, s2);
        assert!(seq.width() >= w1 + w2);
    }

    #[test]
    fn test_seq_auto_cables() {
        // s1 has 1 output, s2 has 2 inputs → cable(1) added via par to s1
        // par(s1, cable(1)) has inputs = s1.inputs(0) + cable.inputs(1) = 1
        let s1 = Box::new(BlockSchema::new(0, 1, "src", "#000", ""));
        let s2 = Box::new(BlockSchema::new(2, 1, "dst", "#000", ""));
        let seq = make_seq(s1, s2);
        assert_eq!(seq.inputs(), 1);
        assert_eq!(seq.outputs(), 1);
    }
}
