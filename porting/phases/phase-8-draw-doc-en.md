# Phase 8 — Draw (SVG) & Documentator (LaTeX)

> **Crates**: `draw`, `doc`
> **Estimate**: 15–20 person days
> **Prerequisites**: Phases 1–4 (boxes + signals)

---

## 1. C++ Inventory

### 1.1 draw/ — 6,136 lines, 46 files

**Rendering engine:**

| File | Lines | Role |
|---------|--------|------|
| `drawschema.hh/.cpp` | ~400 | Entry point: `drawSchema(process, path, format)` |
| `sigToGraph.hh/.cpp` | ~300 | Signal conversion → dot graph |

**Devices (graphics outputs):**

| File | Lines | Role |
|---------|--------|------|
| `device/device.h` | ~50 | Abstract interface `Device` |
| `device/devLib.h` | ~30 | Factory |
| `device/SVGDev.h/.cpp` | ~400 | Show SVG |
| `device/PSDev.h/.cpp` | ~400 | PostScript output |

**Schemas (visual components) — 18 types:**

| File | Lines | Role |
|---------|--------|------|
| `schema/schema.h` | ~200 | Base class `schema` (abstract) |
| `schema/blockSchema.h/.cpp` | ~250 | Primitive blocks (names, operators) |
| `schema/seqSchema.h/.cpp` | ~200 | Sequential composition (`:`) |
| `schema/parSchema.h/.cpp` | ~200 | Parallel Composition (`,`) |
| `schema/splitSchema.h/.cpp` | ~200 | Split (`<:`) |
| `schema/mergeSchema.h/.cpp` | ~200 | Merge (`:>`) |
| `schema/recSchema.h/.cpp` | ~200 | Recursion (`~`) |
| `schema/topSchema.h/.cpp` | ~150 | Root schema |
| `schema/decorateSchema.h/.cpp` | ~200 | Decoration (labels, groups) |
| `schema/enlargedSchema.h/.cpp` | ~150 | Enlargement |
| `schema/cableSchema.h/.cpp` | ~200 | Cables (connection wires) |
| `schema/cutSchema.h/.cpp` | ~100 | Cut (`!`) |
| `schema/connectorSchema.h/.cpp` | ~100 | Connectors |
| `schema/inverterSchema.h/.cpp` | ~100 | Inverters |
| `schema/routeSchema.h/.cpp` | ~150 | Route |
| `schema/collector.cpp` | ~100 | Schema Collector |
| `schema/ondemandSchema.h/.cpp` | ~150 | On-demand (multi-rate) |
| `schema/downsamplingSchema.h/.cpp` | ~150 | Downsampling |
| `schema/upsamplingSchema.h/.cpp` | ~150 | Upsampling |

### 1.2 documentator/ — 4,470 lines, 17 files

| File | Lines | Role |
|---------|--------|------|
| `doc.hh/.cpp` | ~600 | Entry point: `printDoc()` — LaTeX generation |
| `doc_Text.hh/.cpp` | ~300 | Text management for the doc |
| `doc_autodoc.hh/.cpp` | ~400 | Self-documentation of the Faust code |
| `doc_compile.hh/.cpp` | ~800 | Compiling equations for the doc |
| `doc_lang.hh/.cpp` | ~300 | Multilingual translations (fr, en, de, it) |
| `doc_metadatas.hh/.cpp` | ~200 | Document metadata |
| `doc_notice.hh/.cpp` | ~400 | Warnings and notices |
| `doc_sharing.cpp` | ~200 | Sharing expressions in the doc |
| `lateq.hh/.cpp` | ~400 | Generating LaTeX equations (`Lateq`) |

---

## 2. Mapping C++ → Rust

### 2.1 draw

```rust
/// Output format
#[derive(Clone, Copy)]
pub enum DrawFormat { Svg, PostScript }

/// Main entry point
pub fn draw_schema(
    arena: &TreeArena,
    process: TreeId,
    path: &Path,
    format: DrawFormat,
) -> io::Result<()>;

/// Signal → Graphviz dot graph conversion
pub fn sig_to_dot(
    arena: &TreeArena,
    signals: &[TreeId],
    output: &mut dyn Write,
) -> io::Result<()>;

/// Rendering device trait
pub trait DrawDevice {
    fn begin(&mut self, width: f64, height: f64);
    fn end(&mut self);
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64, color: &str);
    fn line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64);
    fn text(&mut self, x: f64, y: f64, text: &str, size: f64);
    fn group_begin(&mut self, label: &str);
    fn group_end(&mut self);
}

pub struct SvgDevice { writer: Box<dyn Write> }
impl DrawDevice for SvgDevice { /* ... */ }

pub struct PsDevice { writer: Box<dyn Write> }
impl DrawDevice for PsDevice { /* ... */ }

/// Schema: visual component of a block diagram
pub trait Schema {
    fn width(&self) -> f64;
    fn height(&self) -> f64;
    fn inputs(&self) -> usize;
    fn outputs(&self) -> usize;
    fn draw(&self, x: f64, y: f64, device: &mut dyn DrawDevice);
    fn input_point(&self, i: usize) -> (f64, f64);
    fn output_point(&self, i: usize) -> (f64, f64);
}

/// The 18 types of schemas → enum or trait objects
pub enum SchemaKind {
    Block(BlockSchema),
    Seq(SeqSchema),
    Par(ParSchema),
    Split(SplitSchema),
    Merge(MergeSchema),
    Rec(RecSchema),
    Top(TopSchema),
    Decorate(DecorateSchema),
    Enlarged(EnlargedSchema),
    Cable(CableSchema),
    Cut(CutSchema),
    Connector(ConnectorSchema),
    Inverter(InverterSchema),
    Route(RouteSchema),
    OnDemand(OnDemandSchema),
    DownSampling(DownSamplingSchema),
    UpSampling(UpSamplingSchema),
}
```

### 2.2 doc

```rust
/// LaTeX documentation generation from a Faust program
pub fn generate_doc(
    arena: &TreeArena,
    process: TreeId,
    signals: &[TreeId],
    output_dir: &Path,
    format: &str,  // "tex"
    version: &str,
    config: &DocConfig,
) -> io::Result<()>;

/// Documentation configuration
pub struct DocConfig {
    pub language: DocLanguage,
    pub include_notice: bool,
    pub include_equations: bool,
    pub include_diagrams: bool,
}

#[derive(Clone, Copy)]
pub enum DocLanguage { English, French, German, Italian }

/// LaTeX equation generation
pub struct LatexEquationWriter {
    equations: Vec<String>,
}

impl LatexEquationWriter {
    pub fn signal_to_latex(&mut self, arena: &TreeArena, sig: TreeId) -> String;
    pub fn emit(&self, output: &mut dyn Write) -> io::Result<()>;
}
```

---

## 3. Dependencies

```
draw → tlib, boxes, signals, errors
doc  → tlib, boxes, signals, draw, errors
```

No heavy external dependencies. Optionally:
- `svg` crate for SVG validation
- `quick-xml` for structured SVG broadcast

---

## 4. Known pitfalls

### 4.1 Layout calculation
The calculation of positions (width, height, entry/exit points) is recursive and interdependent. C++ uses mutable pointers. In Rust, we can calculate the layout in two passes: (1) bottom-up for sizes, (2) top-down for positions.

### 4.2 Little used PostScript
The PostScript format is rarely used. It can be postponed or simplified.

### 4.3 Documentator and overall status
`doc.cpp` massively accesses `gGlobal` for paths, options, etc. Pass an `DocConfig` explicitly.

---

## 5. Testing

- **Unit**: Each type of schema (size, I/O positions)
- **Integration**: `draw_schema(process, "test.svg", Svg)` produces a valid SVG
- **Visual**: Compare the generated SVGs with those of C++ (overlay diff)
- **LaTeX**: The generated document compiles with `pdflatex`

---

## 6. "Done" criteria

- [ ] SVGs visually identical to those of the C++ compiler
- [ ] 18 types of functional diagrams
- [ ] `sigToGraph` produces valid dot output
- [ ] Compilable LaTeX documentation
- [ ] Correct LaTeX equations for fundamental signals

---

## 7. Detailed Effort

| Sub-module | LOC C++ | Estimated LOC Rust | Days |
|-------------|---------|-----------------|-------|
| draw/schemas (18 types) | 3,400 | 2,500 | 7–8 |
| draw/devices (SVG + PS) | 900 | 600 | 2 |
| draw/drawschema + sigToGraph | 700 | 500 | 2 |
| documentator/ | 4,470 | 3,000 | 6–8 |
| Tests + docs | — | 500 | 2 |
| **Total Phase 8** | **9,470** (draw) + **4,470** (doc) | **7,100** | **19–22** |

**Note**: This phase is relatively independent and can be developed **in parallel** with phases 6 and 7.
