//! Decorator schemas: [`EnlargedSchema`], [`DecorateSchema`], [`TopSchema`].
//!
//! These wrap another schema to add padding, a labeled dashed border, or a
//! top-level title with output arrows.
//!
//! C++ references:
//! - `schema/enlargedSchema.h/cpp` — `enlargedSchema`
//! - `schema/decorateSchema.h/cpp` — `decorateSchema`
//! - `schema/topSchema.h/cpp`      — `topSchema`

use crate::device::DrawDevice;
use crate::error::DrawError;
use crate::schema::{D_LETTER, Orientation, Placement, Point, Schema, Trait, TraitCollector};

// ─── EnlargedSchema ────────────────────────────────────────────────────────────

/// Extend a schema to a minimum width by adding equal margins on both sides.
///
/// If `required_width <= s.width()` the schema is returned unchanged.
///
/// C++ reference: `enlargedSchema.cpp:33` — `schema* makeEnlargedSchema`.
pub struct EnlargedSchema {
    width: f64,
    height: f64,
    inputs: usize,
    outputs: usize,
    inner: Box<dyn Schema>,
    placement: Option<Placement>,
    input_points: Vec<Point>,
    output_points: Vec<Point>,
}

/// Return `s` (possibly wrapped) ensuring width ≥ `required_width`.
///
/// C++ reference: `enlargedSchema.cpp:33` — `schema* makeEnlargedSchema`.
pub fn make_enlarged(s: Box<dyn Schema>, required_width: f64) -> Box<dyn Schema> {
    if required_width <= s.width() {
        return s;
    }
    let ins = s.inputs();
    let outs = s.outputs();
    let h = s.height();
    Box::new(EnlargedSchema {
        width: required_width,
        height: h,
        inputs: ins,
        outputs: outs,
        inner: s,
        placement: None,
        input_points: vec![Point::default(); ins],
        output_points: vec![Point::default(); outs],
    })
}

impl Schema for EnlargedSchema {
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

    /// C++ reference: `enlargedSchema.cpp:62` — `enlargedSchema::place`.
    fn place(&mut self, ox: f64, oy: f64, orientation: Orientation) {
        self.placement = Some(Placement {
            x: ox,
            y: oy,
            orientation,
        });
        let mut dx = (self.width - self.inner.width()) / 2.0;
        self.inner.place(ox + dx, oy, orientation);
        if orientation == Orientation::RightLeft {
            dx = -dx;
        }

        for i in 0..self.inputs {
            let p = self.inner.input_point(i);
            self.input_points[i] = Point::new(p.x - dx, p.y);
        }
        for i in 0..self.outputs {
            let p = self.inner.output_point(i);
            self.output_points[i] = Point::new(p.x + dx, p.y);
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

    fn draw(&self, dev: &mut dyn DrawDevice) -> Result<(), DrawError> {
        assert!(self.placed());
        self.inner.draw(dev)
    }

    /// C++ reference: `enlargedSchema.cpp:136` — `enlargedSchema::collectTraits`.
    fn collect_traits(&self, c: &mut TraitCollector) {
        assert!(self.placed());
        self.inner.collect_traits(c);

        for i in 0..self.inputs {
            let p = self.input_points[i];
            let q = self.inner.input_point(i);
            c.add_trait(Trait::new(p, q));
        }
        for i in 0..self.outputs {
            let q = self.inner.output_point(i);
            let p = self.output_points[i];
            c.add_trait(Trait::new(q, p));
        }
    }
}

// ─── DecorateSchema ────────────────────────────────────────────────────────────

/// Surround a schema with a dashed-border group box and a label.
///
/// The inner schema is inset by `margin` on all sides; ports are extended
/// accordingly.
///
/// C++ reference: `decorateSchema.cpp:43` — `decorateSchema::decorateSchema`.
pub struct DecorateSchema {
    width: f64,
    height: f64,
    inputs: usize,
    outputs: usize,
    inner: Box<dyn Schema>,
    margin: f64,
    text: String,
    placement: Option<Placement>,
    input_points: Vec<Point>,
    output_points: Vec<Point>,
}

/// Build a `DecorateSchema`.
///
/// C++ reference: `decorateSchema.cpp:33` — `schema* makeDecorateSchema`.
pub fn make_decorate(s: Box<dyn Schema>, margin: f64, text: impl Into<String>) -> Box<dyn Schema> {
    let ins = s.inputs();
    let outs = s.outputs();
    let w = s.width() + 2.0 * margin;
    let h = s.height() + 2.0 * margin;
    Box::new(DecorateSchema {
        width: w,
        height: h,
        inputs: ins,
        outputs: outs,
        inner: s,
        margin,
        text: text.into(),
        placement: None,
        input_points: vec![Point::default(); ins],
        output_points: vec![Point::default(); outs],
    })
}

impl Schema for DecorateSchema {
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

    /// C++ reference: `decorateSchema.cpp:63` — `decorateSchema::place`.
    fn place(&mut self, ox: f64, oy: f64, orientation: Orientation) {
        self.placement = Some(Placement {
            x: ox,
            y: oy,
            orientation,
        });
        let m = self.margin;
        self.inner.place(ox + m, oy + m, orientation);

        let dm = if orientation == Orientation::RightLeft {
            -m
        } else {
            m
        };
        for i in 0..self.inputs {
            let p = self.inner.input_point(i);
            self.input_points[i] = Point::new(p.x - dm, p.y);
        }
        for i in 0..self.outputs {
            let p = self.inner.output_point(i);
            self.output_points[i] = Point::new(p.x + dm, p.y);
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

    /// C++ reference: `decorateSchema.cpp:111` — `decorateSchema::draw`.
    fn draw(&self, dev: &mut dyn DrawDevice) -> Result<(), DrawError> {
        assert!(self.placed());
        self.inner.draw(dev)?;

        let tw = (2 + self.text.chars().count()) as f64 * D_LETTER * 0.75;
        let m = self.margin;
        let p = self.placement.unwrap();
        let x0 = p.x + m / 2.0;
        let y0 = p.y + m / 2.0;
        let x1 = p.x + self.width - m / 2.0;
        let y1 = p.y + self.height - m / 2.0;
        let tl = p.x + m;
        let tr = (tl + tw).min(x1);

        dev.dashed_line(x0, y0, x0, y1)?;
        dev.dashed_line(x0, y1, x1, y1)?;
        dev.dashed_line(x1, y1, x1, y0)?;
        dev.dashed_line(x0, y0, tl, y0)?;
        dev.dashed_line(tr, y0, x1, y0)?;
        dev.label(tl, y0, &self.text)?;
        Ok(())
    }

    /// C++ reference: `decorateSchema.cpp:156` — `decorateSchema::collectTraits`.
    fn collect_traits(&self, c: &mut TraitCollector) {
        assert!(self.placed());
        self.inner.collect_traits(c);

        for i in 0..self.inputs {
            let p = self.input_points[i];
            let q = self.inner.input_point(i);
            c.add_trait(Trait::new(p, q));
        }
        for i in 0..self.outputs {
            let p = self.inner.output_point(i);
            let q = self.output_points[i];
            c.add_trait(Trait::new(p, q));
        }
    }
}

// ─── TopSchema ─────────────────────────────────────────────────────────────────

/// Root wrapper: white background, title label, output arrows, and optional link.
///
/// Constructed as `DecorateSchema(inner, margin/2, text)` wrapped in a `TopSchema`
/// with the remaining half-margin.
///
/// C++ reference: `topSchema.cpp:34` — `schema* makeTopSchema`.
pub struct TopSchema {
    width: f64,
    height: f64,
    inner: Box<dyn Schema>,
    margin: f64,
    text: String,
    link: String,
    placement: Option<Placement>,
}

/// Build a `TopSchema`.
///
/// C++ reference: `topSchema.cpp:34` — `schema* makeTopSchema`.
pub fn make_top(
    s: Box<dyn Schema>,
    margin: f64,
    text: impl Into<String>,
    link: impl Into<String>,
) -> Box<dyn Schema> {
    let decorated = make_decorate(s, margin / 2.0, text.into());
    let w = decorated.width() + margin; // margin split equally: half/2 each side → total +margin
    let h = decorated.height() + margin;
    Box::new(TopSchema {
        width: w,
        height: h,
        inner: decorated,
        margin: margin / 2.0,
        text: String::new(),
        link: link.into(),
        placement: None,
    })
}

impl Schema for TopSchema {
    fn width(&self) -> f64 {
        self.width
    }
    fn height(&self) -> f64 {
        self.height
    }
    fn inputs(&self) -> usize {
        0
    }
    fn outputs(&self) -> usize {
        0
    }

    fn place(&mut self, ox: f64, oy: f64, orientation: Orientation) {
        self.placement = Some(Placement {
            x: ox,
            y: oy,
            orientation,
        });
        self.inner
            .place(ox + self.margin, oy + self.margin, orientation);
    }

    fn placed(&self) -> bool {
        self.placement.is_some()
    }
    fn placement(&self) -> Option<&Placement> {
        self.placement.as_ref()
    }

    fn input_point(&self, _i: usize) -> Point {
        panic!("TopSchema has no inputs")
    }
    fn output_point(&self, _i: usize) -> Point {
        panic!("TopSchema has no outputs")
    }

    /// C++ reference: `topSchema.cpp:91` — `topSchema::draw`.
    fn draw(&self, dev: &mut dyn DrawDevice) -> Result<(), DrawError> {
        assert!(self.placed());
        let p = self.placement.unwrap();

        // white background
        dev.rect(
            p.x,
            p.y,
            self.width - 1.0,
            self.height - 1.0,
            "#ffffff",
            &self.link,
        )?;

        // label
        dev.label(p.x + self.margin, p.y + self.margin / 2.0, &self.text)?;

        self.inner.draw(dev)?;

        // output arrows
        for i in 0..self.inner.outputs() {
            let pt = self.inner.output_point(i);
            dev.arrow(pt.x, pt.y, 0.0, p.orientation.sign() as i32)?;
        }
        Ok(())
    }

    /// C++ reference: `topSchema.cpp:114` — `topSchema::collectTraits`.
    fn collect_traits(&self, c: &mut TraitCollector) {
        assert!(self.placed());
        self.inner.collect_traits(c);

        for i in 0..self.inner.inputs() {
            c.add_output(self.inner.input_point(i));
        }
        for i in 0..self.inner.outputs() {
            c.add_input(self.inner.output_point(i));
        }
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schemas::block::BlockSchema;

    #[test]
    fn test_enlarged_width() {
        let b = Box::new(BlockSchema::new(1, 1, "x", "#000", ""));
        let orig_w = b.width();
        let req = orig_w + 20.0;
        let e = make_enlarged(b, req);
        assert_eq!(e.width(), req);
    }

    #[test]
    fn test_enlarged_passthrough_when_not_needed() {
        let b: Box<dyn Schema> = Box::new(BlockSchema::new(1, 1, "x", "#000", ""));
        let w = b.width();
        let e = make_enlarged(b, w - 1.0); // smaller required → passthrough
        assert_eq!(e.width(), w);
    }

    #[test]
    fn test_decorate_adds_margin() {
        let b = Box::new(BlockSchema::new(1, 1, "x", "#000", ""));
        let w = b.width();
        let h = b.height();
        let d = make_decorate(b, 10.0, "lbl");
        assert_eq!(d.width(), w + 20.0);
        assert_eq!(d.height(), h + 20.0);
    }

    #[test]
    fn test_top_sizing() {
        let b = Box::new(BlockSchema::new(0, 1, "process", "#000", ""));
        let t = make_top(b, 20.0, "process", "");
        assert!(t.width() > 0.0);
        assert!(t.height() > 0.0);
    }

    #[test]
    fn test_top_no_ports() {
        let b = Box::new(BlockSchema::new(0, 1, "x", "#000", ""));
        let t = make_top(b, 20.0, "x", "");
        assert_eq!(t.inputs(), 0);
        assert_eq!(t.outputs(), 0);
    }
}
