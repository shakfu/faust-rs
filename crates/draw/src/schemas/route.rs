//! [`RouteSchema`] — explicit input-to-output permutation.
//!
//! C++ reference: `schema/routeSchema.h/cpp`.

use crate::device::DrawDevice;
use crate::error::DrawError;
use crate::schema::{
    Orientation, Placement, Point, Schema, Trait, TraitCollector, D_HORZ, D_VERT, D_WIRE,
};

const ROUTE_COLOR: &str = "#EEEEAA";

/// Explicit routing block: maps specific inputs to specific outputs.
///
/// `routes` is a flat list of (1-based src, 1-based dst) pairs.
///
/// With `draw_frame = true` (CLI `-drf`): draws a filled rectangle, an orientation
/// mark, and input arrows — matching C++ with `gDrawRouteFrame`.
/// With `draw_frame = false` (default): invisible; only the wire traits carry signal.
///
/// C++ reference: `routeSchema.cpp:34` — `schema* makeRouteSchema`.
pub struct RouteSchema {
    width: f64,
    height: f64,
    inputs: usize,
    outputs: usize,
    routes: Vec<usize>,
    /// Whether to draw the visible rectangle frame + arrows (`-drf` flag).
    draw_frame: bool,
    placement: Option<Placement>,
    input_points: Vec<Point>,
    output_points: Vec<Point>,
}

/// Build a `RouteSchema`.
///
/// `draw_frame` corresponds to the C++ `gDrawRouteFrame` option (`-drf`).
///
/// C++ reference: `routeSchema.cpp:34` — `schema* makeRouteSchema`.
pub fn make_route(
    inputs: usize,
    outputs: usize,
    routes: Vec<usize>,
    draw_frame: bool,
) -> Box<dyn Schema> {
    let minimal = 3.0 * D_WIRE;
    let h = 2.0 * D_VERT + minimal.max(inputs.max(outputs) as f64 * D_WIRE);
    let w = 2.0 * D_HORZ + minimal.max(h * 0.75);
    Box::new(RouteSchema {
        width: w,
        height: h,
        inputs,
        outputs,
        routes,
        draw_frame,
        placement: None,
        input_points: vec![Point::default(); inputs],
        output_points: vec![Point::default(); outputs],
    })
}

impl RouteSchema {
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

    fn is_valid_route(&self, src: usize, dst: usize) -> bool {
        src >= 1 && src <= self.inputs && dst >= 1 && dst <= self.outputs
    }
}

impl Schema for RouteSchema {
    fn width(&self) -> f64 { self.width }
    fn height(&self) -> f64 { self.height }
    fn inputs(&self) -> usize { self.inputs }
    fn outputs(&self) -> usize { self.outputs }

    fn place(&mut self, x: f64, y: f64, orientation: Orientation) {
        self.placement = Some(Placement { x, y, orientation });
        self.place_input_points();
        self.place_output_points();
    }

    fn placed(&self) -> bool { self.placement.is_some() }
    fn placement(&self) -> Option<&Placement> { self.placement.as_ref() }

    fn input_point(&self, i: usize) -> Point { self.input_points[i] }
    fn output_point(&self, i: usize) -> Point { self.output_points[i] }

    /// C++ reference: `routeSchema.cpp:159` — `routeSchema::draw`.
    ///
    /// When `draw_frame` is false (default, no `-drf`), nothing is drawn —
    /// the route is purely structural; wire traits carry the connections.
    fn draw(&self, dev: &mut dyn DrawDevice) -> Result<(), DrawError> {
        assert!(self.placed());
        if !self.draw_frame {
            return Ok(());
        }
        let p = self.placement.unwrap();
        // background rectangle
        dev.rect(
            p.x + D_HORZ, p.y + D_VERT,
            self.width - 2.0 * D_HORZ, self.height - 2.0 * D_VERT,
            ROUTE_COLOR, "",
        )?;
        // orientation mark
        let (mx, my) = match p.orientation {
            Orientation::LeftRight => (p.x + D_HORZ, p.y + D_VERT),
            Orientation::RightLeft => (p.x + self.width - D_HORZ, p.y + self.height - D_VERT),
        };
        dev.mark_direction(mx, my, p.orientation.sign() as i32)?;
        // input arrows
        let dx = if p.orientation == Orientation::LeftRight { D_HORZ } else { -D_HORZ };
        for pt in &self.input_points {
            dev.arrow(pt.x + dx, pt.y, 0.0, p.orientation.sign() as i32)?;
        }
        Ok(())
    }

    /// C++ reference: `routeSchema.cpp:230` — `routeSchema::collectTraits`.
    fn collect_traits(&self, c: &mut TraitCollector) {
        assert!(self.placed());
        let p = self.placement.unwrap();
        let dx = if p.orientation == Orientation::LeftRight { D_HORZ } else { -D_HORZ };

        // input stubs
        for pt in &self.input_points {
            c.add_trait(Trait::new(*pt, Point::new(pt.x + dx, pt.y)));
            c.add_input(Point::new(pt.x + dx, pt.y));
        }
        // output stubs
        for pt in &self.output_points {
            c.add_trait(Trait::new(Point::new(pt.x - dx, pt.y), *pt));
            c.add_output(Point::new(pt.x - dx, pt.y));
        }
        // explicit routing connections
        let mut i = 0;
        while i + 1 < self.routes.len() {
            let src = self.routes[i];
            let dst = self.routes[i + 1];
            i += 2;
            if self.is_valid_route(src, dst) {
                let p1 = self.input_points[src - 1];
                let p2 = self.output_points[dst - 1];
                c.add_trait(Trait::new(
                    Point::new(p1.x + dx, p1.y),
                    Point::new(p2.x - dx, p2.y),
                ));
            }
        }
    }
}
