//! [`RecSchema`] — recursive composition `s1 ~ s2`.
//!
//! C++ reference: `schema/recSchema.h/cpp`.

use crate::device::DrawDevice;
use crate::error::DrawError;
use crate::schema::{D_WIRE, Orientation, Placement, Point, Schema, Trait, TraitCollector};
use crate::schemas::composed::make_enlarged;

/// Recursive composition: feedback from outputs of `s1` back to inputs of `s2`.
///
/// Width = `max(s1.width, s2.width) + 2 * margin` where `margin = D_WIRE * max(s2.inputs, s2.outputs)`.
///
/// C++ reference: `recSchema.cpp:35` — `schema* makeRecSchema`.
pub struct RecSchema {
    width: f64,
    height: f64,
    s1: Box<dyn Schema>,
    s2: Box<dyn Schema>,
    placement: Option<Placement>,
    input_points: Vec<Point>,
    output_points: Vec<Point>,
}

/// Build a `RecSchema`.
///
/// C++ reference: `recSchema.cpp:35` — `schema* makeRecSchema`.
pub fn make_rec(s1: Box<dyn Schema>, s2: Box<dyn Schema>) -> Box<dyn Schema> {
    let a = make_enlarged(s1, s2.width());
    let b = make_enlarged(s2, a.width());
    let margin = D_WIRE * b.inputs().max(b.outputs()) as f64;
    let w = a.width() + 2.0 * margin;
    let ins = a.inputs().saturating_sub(b.outputs());
    let outs = a.outputs();
    let h = a.height() + b.height();
    Box::new(RecSchema {
        width: w,
        height: h,
        s1: a,
        s2: b,
        placement: None,
        input_points: vec![Point::default(); ins],
        output_points: vec![Point::default(); outs],
    })
}

impl Schema for RecSchema {
    fn width(&self) -> f64 {
        self.width
    }
    fn height(&self) -> f64 {
        self.height
    }
    fn inputs(&self) -> usize {
        self.input_points.len()
    }
    fn outputs(&self) -> usize {
        self.output_points.len()
    }

    /// C++ reference: `recSchema.cpp:72` — `recSchema::place`.
    fn place(&mut self, ox: f64, oy: f64, orientation: Orientation) {
        self.placement = Some(Placement {
            x: ox,
            y: oy,
            orientation,
        });

        let dx1 = (self.width - self.s1.width()) / 2.0;
        let dx2 = (self.width - self.s2.width()) / 2.0;

        match orientation {
            Orientation::LeftRight => {
                self.s2.place(ox + dx2, oy, Orientation::RightLeft);
                self.s1
                    .place(ox + dx1, oy + self.s2.height(), Orientation::LeftRight);
            }
            Orientation::RightLeft => {
                self.s1.place(ox + dx1, oy, Orientation::RightLeft);
                self.s2
                    .place(ox + dx2, oy + self.s1.height(), Orientation::LeftRight);
            }
        }

        let adx1 = if orientation == Orientation::RightLeft {
            -dx1
        } else {
            dx1
        };

        let skip = self.s2.outputs();
        for i in 0..self.input_points.len() {
            let p = self.s1.input_point(i + skip);
            self.input_points[i] = Point::new(p.x - adx1, p.y);
        }
        for i in 0..self.output_points.len() {
            let p = self.s1.output_point(i);
            self.output_points[i] = Point::new(p.x + adx1, p.y);
        }
    }

    fn placed(&self) -> bool {
        self.placement.is_some()
    }
    fn placement(&self) -> Option<&Placement> {
        self.placement.as_ref()
    }

    fn input_point(&self, i: usize) -> Point {
        self.input_points[i]
    }
    fn output_point(&self, i: usize) -> Point {
        self.output_points[i]
    }

    /// C++ reference: `recSchema.cpp:128` — `recSchema::draw`.
    fn draw(&self, dev: &mut dyn DrawDevice) -> Result<(), DrawError> {
        assert!(self.placed());
        self.s1.draw(dev)?;
        self.s2.draw(dev)?;

        let orientation = self.orientation();
        let dw = if orientation == Orientation::LeftRight {
            D_WIRE
        } else {
            -D_WIRE
        };
        for i in 0..self.s2.inputs() {
            let p = self.s1.output_point(i);
            draw_delay_sign(dev, p.x + i as f64 * dw, p.y, dw / 2.0)?;
        }
        Ok(())
    }

    /// C++ reference: `recSchema.cpp:159` — `recSchema::collectTraits`.
    fn collect_traits(&self, c: &mut TraitCollector) {
        assert!(self.placed());
        self.s1.collect_traits(c);
        self.s2.collect_traits(c);

        let orientation = self.orientation();

        // feedback connections: s1.output[i] → s2.input[i]
        for i in 0..self.s2.inputs() {
            collect_feedback(
                c,
                self.s1.output_point(i),
                self.s2.input_point(i),
                i as f64 * D_WIRE,
                self.output_points[i],
                orientation,
            );
        }

        // non-recursive outputs
        for i in self.s2.inputs()..self.outputs() {
            let p = self.s1.output_point(i);
            let q = self.output_points[i];
            c.add_trait(Trait::new(p, q));
        }

        // input lines
        let skip = self.s2.outputs();
        for i in 0..self.inputs() {
            let p = self.input_points[i];
            let q = self.s1.input_point(i + skip);
            c.add_trait(Trait::new(p, q));
        }

        // feed-front connections: s2.output[i] → s1.input[i]
        for i in 0..self.s2.outputs() {
            collect_feedfront(
                c,
                self.s2.output_point(i),
                self.s1.input_point(i),
                i as f64 * D_WIRE,
                orientation,
            );
        }
    }
}

// ─── Helpers ───────────────────────────────────────────────────────────────────

/// Draw the `[z^-1]` delay sign for a feedback connection.
///
/// C++ reference: `recSchema.cpp:147` — `recSchema::drawDelaySign`.
fn draw_delay_sign(dev: &mut dyn DrawDevice, x: f64, y: f64, size: f64) -> Result<(), DrawError> {
    dev.line(x - size / 2.0, y, x - size / 2.0, y - size)?;
    dev.line(x - size / 2.0, y - size, x + size / 2.0, y - size)?;
    dev.line(x + size / 2.0, y - size, x + size / 2.0, y)?;
    Ok(())
}

/// C++ reference: `recSchema.cpp:198` — `recSchema::collectFeedback`.
fn collect_feedback(
    c: &mut TraitCollector,
    src: Point,
    dst: Point,
    dx: f64,
    out: Point,
    orientation: Orientation,
) {
    let ox = src.x
        + if orientation == Orientation::LeftRight {
            dx
        } else {
            -dx
        };
    let ct = if orientation == Orientation::LeftRight {
        D_WIRE / 2.0
    } else {
        -D_WIRE / 2.0
    };

    let up = Point::new(ox, src.y - ct);
    let br = Point::new(ox + ct / 2.0, src.y);

    c.add_output(up);
    c.add_output(br);
    c.add_input(br);

    c.add_trait(Trait::new(up, Point::new(ox, dst.y)));
    c.add_trait(Trait::new(Point::new(ox, dst.y), Point::new(dst.x, dst.y)));
    c.add_trait(Trait::new(src, br));
    c.add_trait(Trait::new(br, out));
}

/// C++ reference: `recSchema.cpp:221` — `recSchema::collectFeedfront`.
fn collect_feedfront(
    c: &mut TraitCollector,
    src: Point,
    dst: Point,
    dx: f64,
    orientation: Orientation,
) {
    let ox = src.x
        + if orientation == Orientation::LeftRight {
            -dx
        } else {
            dx
        };
    c.add_trait(Trait::new(src, Point::new(ox, src.y)));
    c.add_trait(Trait::new(Point::new(ox, src.y), Point::new(ox, dst.y)));
    c.add_trait(Trait::new(Point::new(ox, dst.y), dst));
}
