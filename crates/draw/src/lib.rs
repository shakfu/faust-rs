//! SVG block-diagram generation for Faust DSP programs.
//!
//! This crate ports the C++ `compiler/draw/` module to Rust, providing the
//! `-svg` compilation flag that produces visual block diagrams of a Faust
//! program's structure.
//!
//! # Architecture
//!
//! ```text
//! draw_schema(arena, process_id, config, output_path)
//!   └── translate::generate_schema(arena, id, config)  → Box<dyn Schema>
//!         └── schemas::{Block,Seq,Par,…}                (recursive)
//!   └── TopSchema wrapper (title + margin)
//!   └── SvgDevice::new(file, width, height, config)
//!   └── schema.place(0, 0, LeftRight)
//!   └── schema.draw(dev)
//!   └── TraitCollector::draw(dev)                       (wires on top)
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
    COLOR_INV, COLOR_LINK, COLOR_NORMAL, COLOR_NUM, COLOR_SLOT, COLOR_UI, D_HORZ, D_LETTER, D_VERT,
    D_WIRE, Orientation, Placement, Point, Schema, Trait, TraitCollector,
};

pub const CRATE_NAME: &str = "draw";

#[must_use]
/// Returns the stable crate identifier.
pub fn crate_id() -> &'static str {
    CRATE_NAME
}

// ─── DrawConfig ───────────────────────────────────────────────────────────────

/// Visual and layout options for SVG block-diagram generation.
///
/// Matches the set of C++ globals consumed by the draw module.
/// All fields have documented defaults matching the reference compiler.
///
/// C++ references: `global.hh` fields `gShadowBlur`, `gScaledSVG`,
/// `gDrawRouteFrame`, `gMaxNameSize`.
#[derive(Clone, Debug)]
pub struct DrawConfig {
    /// Add a Gaussian drop-shadow filter to each box rectangle.
    ///
    /// Emits a `<defs><filter>` block in the SVG header and applies
    /// `filter:url(#filter)` to shadow rects.
    ///
    /// C++ global: `gShadowBlur` (default false). CLI: `-blur` / `--shadow-blur`.
    pub shadow_blur: bool,

    /// Emit a viewBox-only SVG header (no fixed `width=` / `height=` mm attributes).
    ///
    /// Makes the SVG responsive — scales freely to container size.
    ///
    /// C++ global: `gScaledSVG` (default false). CLI: `-sc` / `--scaled-svg`.
    pub scaled_svg: bool,

    /// Draw a solid rectangle frame around route boxes instead of cable stubs.
    ///
    /// Without this flag, route boxes draw only the filled background + arrows.
    /// With this flag, an explicit orientation mark is added.
    ///
    /// C++ global: `gDrawRouteFrame` (default false). CLI: `-drf` / `--draw-route-frame`.
    pub draw_route_frame: bool,

    /// Maximum character length for block labels; longer names are truncated.
    ///
    /// Truncation keeps the first third and last third of the name, joined by `"..."`.
    ///
    /// C++ global: `gMaxNameSize` (default 40). CLI: `-mns N` / `--max-name-size N`.
    pub max_name_size: usize,

    /// Fold diagrams whose total `box_complexity` exceeds this threshold into
    /// separate SVG files with clickable back-links.
    ///
    /// When 0, folding is disabled (single-file output).
    ///
    /// C++ global: `gFoldThreshold` (default 25). CLI: `-f N` / `--fold N`.
    pub fold_threshold: usize,

    /// Minimum per-expression complexity for a named sub-diagram to be
    /// extracted into its own file when folding is active.
    ///
    /// C++ global: `gFoldComplexity` (default 2). CLI: `-fc N` / `--fold-complexity N`.
    pub fold_complexity: usize,
}

impl Default for DrawConfig {
    fn default() -> Self {
        Self {
            shadow_blur: false,
            scaled_svg: false,
            draw_route_frame: false,
            max_name_size: 40,
            fold_threshold: 25,
            fold_complexity: 2,
        }
    }
}

// ─── Public entry point ───────────────────────────────────────────────────────

/// Generate SVG block-diagram file(s) from a box expression.
///
/// Writes `process.svg` (and optionally sub-diagram files) into `out_dir`.
///
/// When `config.fold_threshold > 0` and the root diagram complexity exceeds
/// the threshold, named sub-diagrams are extracted into separate SVG files
/// with clickable hyperlinks — mirroring the C++ `-f N` folding behaviour.
///
/// `def_names` maps evaluated `BoxId` values to their Faust definition names,
/// as populated by the evaluator (`setDefNameProperty` equivalent).
///
/// # Errors
/// Returns [`DrawError::Io`] if any output file cannot be written.
///
/// C++ reference: `drawschema.cpp:149` — `drawSchema` / `writeSchemaFile`.
pub fn draw_schema(
    arena: &tlib::TreeArena,
    root: boxes::BoxId,
    name: &str,
    out_dir: &std::path::Path,
    config: &DrawConfig,
    def_names: &std::collections::HashMap<boxes::BoxId, String>,
) -> Result<(), DrawError> {
    use std::collections::{HashSet, VecDeque};

    use device::SvgDevice;
    use schema::TraitCollector;
    use translate::{FoldState, generate_folded_inside, make_top_schema};

    let folding =
        config.fold_threshold > 0 && boxes::box_complexity(arena, root) > config.fold_threshold;

    // Queue: (box_id, diagram_name, back_link_file)
    let mut pending: VecDeque<(boxes::BoxId, String, String)> = VecDeque::new();
    let mut drawn: HashSet<boxes::BoxId> = HashSet::new();

    let root_file = "process.svg".to_owned();
    pending.push_back((root, name.to_owned(), String::new()));
    drawn.insert(root);

    while let Some((box_id, diagram_name, back_link)) = pending.pop_front() {
        let file_name = translate::legal_file_name(&diagram_name, box_id);
        let file_path = out_dir.join(&file_name);

        let mut state = FoldState {
            def_names,
            pending: &mut pending,
            drawn: &mut drawn,
            current_file: file_name.clone(),
            folding,
            fold_complexity: config.fold_complexity,
        };

        let inner = generate_folded_inside(arena, box_id, config, &mut state);
        let mut top = make_top_schema(inner, &diagram_name, &back_link);
        top.place(0.0, 0.0, Orientation::LeftRight);

        let file = std::fs::File::create(&file_path).map_err(DrawError::Io)?;
        let mut dev = SvgDevice::new(file, top.width(), top.height(), config)?;
        top.draw(&mut dev)?;
        let mut collector = TraitCollector::new();
        top.collect_traits(&mut collector);
        collector.draw(&mut dev)?;
        dev.finish().map(|_| ())?;

        // Only the root is initially drawn; mark sub-diagrams as drawn when popped.
        let _ = root_file.as_str(); // suppress unused warning
    }

    Ok(())
}
