//! SVG block-diagram generation for Faust DSP programs.
//!
//! This crate ports the C++ `compiler/draw/` module to Rust, providing the
//! `-svg` compilation flag that produces visual block diagrams of a Faust
//! program's structure.
//!
//! # Architecture
//!
//! ```text
//! draw_schema(arena, process_id, output_dir)
//!   └── translate::generate_diagram_schema(arena, id)   → Box<dyn Schema>
//!         └── schemas::{Block,Seq,Par,…}                 (recursive)
//!   └── TopSchema wrapper (title + margin)
//!   └── SvgDevice::new(file)
//!   └── schema.place(0, 0, LeftRight)
//!   └── schema.draw(dev)
//!   └── TraitCollector::draw(dev)                        (wires on top)
//! ```
//!
//! # C++ source mapping
//!
//! | Rust module | C++ source |
//! |-------------|-----------|
//! | `error` | — (replaces `faustexception`) |
//! | `device` | `device/device.h`, `device/SVGDev.h/cpp` |
//! | `schema` | `schema/schema.h`, `schema/collector.cpp` |
//! | `schemas::block` | `schema/blockSchema.h/cpp`, `schema/inverterSchema.h/cpp` |
//! | `schemas::cable` | `schema/cableSchema.h/cpp`, `schema/cutSchema.h/cpp`, `schema/connectorSchema.h/cpp` |
//! | `schemas::seq` | `schema/seqSchema.h/cpp` |
//! | `schemas::par` | `schema/parSchema.h/cpp` |
//! | `schemas::merge` | `schema/mergeSchema.h/cpp` |
//! | `schemas::split` | `schema/splitSchema.h/cpp` |
//! | `schemas::rec` | `schema/recSchema.h/cpp` |
//! | `schemas::composed` | `schema/topSchema.h/cpp`, `schema/decorateSchema.h/cpp`, `schema/enlargedSchema.h/cpp` |
//! | `schemas::route` | `schema/routeSchema.h/cpp` |
//! | `schemas::multirate` | `schema/ondemandSchema.h/cpp`, `schema/downsamplingSchema.h/cpp`, `schema/upsamplingSchema.h/cpp` |
//! | `translate` | `drawschema.cpp` |

pub mod device;
pub mod error;
pub mod schema;
pub mod schemas;
pub mod translate;

pub use error::DrawError;
pub use schema::{
    Orientation, Placement, Point, Schema, Trait, TraitCollector, COLOR_INV, COLOR_LINK,
    COLOR_NORMAL, COLOR_NUM, COLOR_SLOT, COLOR_UI, D_HORZ, D_LETTER, D_VERT, D_WIRE,
};

pub const CRATE_NAME: &str = "draw";

#[must_use]
/// Returns the stable crate identifier.
pub fn crate_id() -> &'static str {
    CRATE_NAME
}

// ─── Public entry point ───────────────────────────────────────────────────────

/// Generate one SVG block-diagram file from a box expression.
///
/// Builds the schema tree via [`translate::generate_schema`], wraps it in a
/// [`TopSchema`](schemas::composed::TopSchema) (title + margins + output arrows),
/// then renders to `output_path`.
///
/// # Errors
/// Returns [`DrawError::Io`] if the output file cannot be written.
///
/// C++ reference: `drawschema.cpp:234` — `writeSchemaFile`.
pub fn draw_schema(
    arena: &tlib::TreeArena,
    root: boxes::BoxId,
    name: &str,
    output_path: &std::path::Path,
) -> Result<(), DrawError> {
    use device::SvgDevice;
    use schema::TraitCollector;
    use translate::{generate_schema, make_top_schema};

    let inner   = generate_schema(arena, root);
    let mut top = make_top_schema(inner, name, "");

    top.place(0.0, 0.0, Orientation::LeftRight);

    let file = std::fs::File::create(output_path)
        .map_err(DrawError::Io)?;
    let mut dev = SvgDevice::new(file, top.width(), top.height())?;

    top.draw(&mut dev)?;

    let mut collector = TraitCollector::new();
    top.collect_traits(&mut collector);
    collector.draw(&mut dev)?;

    dev.finish().map(|_| ())
}
