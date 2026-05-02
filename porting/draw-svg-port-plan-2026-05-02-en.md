# Draw Module SVG Port Plan — 2026-05-02

## Overview

Port the Faust block-diagram drawing module (`compiler/draw/`) from C++ to Rust,
exposing it via a `-svg` CLI flag (matching the reference C++ compiler).
The output is a directory `<name>-svg/` containing one or more `.svg` files with
a recursive block diagram of the compiled DSP.

**C++ source**: `faust/compiler/draw/`  
**Rust target**: `crates/draw/` (currently a scaffold — `lib.rs` has only `crate_id()`)  
**CLI entry**: `crates/compiler/src/main.rs` — add `-svg` to `normalize_legacy_args` and wire flag

PostScript output is deferred to a later phase; the device abstraction is designed
to accommodate it without API changes.

---

## Source Inventory

All files live under `faust/compiler/draw/`:

### Entry points
| File | Lines | Role |
|------|-------|------|
| `drawschema.hh` | 14 | Public header: `drawSchema(Tree, name, dev)` |
| `drawschema.cpp` | ~700 | Orchestration: folding, directory, scheduling, box→schema translation |
| `sigToGraph.hh` | 16 | Public header: `sigToGraph(Tree sig, ostream&)` |
| `sigToGraph.cpp` | ~347 | Signal tree → Graphviz dot (auxiliary, lower priority) |

### Device abstraction (`device/`)
| File | Role |
|------|------|
| `device/device.h` | Abstract base: 11 pure-virtual drawing primitives |
| `device/SVGDev.h/cpp` | SVG concrete backend, writes XML to `FILE*` |
| `device/PSDev.h/cpp` | PostScript backend (defer) |
| `device/devLib.h` | Factory helpers (defer) |

### Schema tree (`schema/`)
| File | Lines | Schema type |
|------|-------|------------|
| `schema.h` | ~180 | Abstract base: `schema`, `point`, `trait`, `collector` |
| `blockSchema.h/cpp` | ~250 | Rectangle box with inputs/outputs/label/color/link |
| `cableSchema.h/cpp` | ~100 | Single wire pass-through |
| `cutSchema.h/cpp` | ~80 | Terminator (`!`) |
| `connectorSchema.h/cpp` | ~80 | Dangling input/output connector |
| `inverterSchema.h/cpp` | ~80 | Special `*(-1)` symbol |
| `seqSchema.h/cpp` | ~200 | Sequence (`:`) — two schemas side-by-side |
| `parSchema.h/cpp` | ~200 | Parallel (`,`) — two schemas stacked |
| `splitSchema.h/cpp` | ~200 | Split (`<:`) |
| `mergeSchema.h/cpp` | ~200 | Merge (`:>`) |
| `recSchema.h/cpp` | ~200 | Recursion (`~`) with feedback arrow |
| `topSchema.h/cpp` | ~150 | Root wrapper with title label |
| `decorateSchema.h/cpp` | ~150 | Named group border |
| `enlargedSchema.h/cpp` | ~100 | Width padding |
| `routeSchema.h/cpp` | ~150 | Route operator (explicit I/O permutation) |
| `ondemandSchema.h/cpp` | ~120 | Multi-rate annotation |
| `downsamplingSchema.h/cpp` | ~100 | Downsampling box |
| `upsamplingSchema.h/cpp` | ~100 | Upsampling box |
| `collector.cpp` | ~100 | Wire visibility filtering |

**Total**: ~5 400 lines C++ → target ~2 800 lines Rust.

---

## Key Data Structures

### `schema.h` constants
```cpp
const double dWire   = 8;    // spacing between wires
const double dLetter = 4.3;  // character width
const double dHorz   = 4;    // horizontal margin
const double dVert   = 4;    // vertical margin
```

### `device` abstract class (11 methods)
```cpp
rect(x, y, l, h, color, link)
triangle(x, y, l, h, color, link, leftright)
rond(x, y, rayon)         // circle
carre(x, y, cote)         // square
fleche(x, y, rotation, sens)  // arrow
trait(x1, y1, x2, y2)    // solid line
dasharray(x1, y1, x2, y2)  // dashed line
text(x, y, name, link)
label(x, y, name)
markSens(x, y, sens)
Error(message, reason, nb_error, x, y, largeur)
```

### `schema` abstract class
- Size (computed bottom-up): `width()`, `height()`, `inputs()`, `outputs()`
- Placement (top-down): `place(x, y, orientation)` → sets internal `fX`, `fY`, `fOrientation`
- Drawing: `draw(device&)`, `collectTraits(collector&)`
- Port positions: `inputPoint(i)`, `outputPoint(i)`

### `collector` (wire filter)
- `fOutputs: set<point>` — real output endpoints
- `fInputs: set<point>` — real input endpoints
- `fTraits: set<trait>` — all wires
- `computeVisibleTraits()` — filters wires touching real endpoints
- `draw(device&)` — render visible wires

### Color constants (active set in `drawschema.cpp:116–123`)
```cpp
#define linkcolor   "#003366"
#define normalcolor "#4B71A1"
#define uicolor     "#477881"
#define slotcolor   "#47945E"
#define numcolor    "#f44800"
#define invcolor    "#ffffff"
```

### Folding logic (`drawschema.cpp:149–164`)
```
drawSchema(bd, projname, dev):
  gFoldingFlag = boxComplexity(bd) > gFoldThreshold
  mkchDir(projname + "-svg/")
  scheduleDrawing(bd)
  while pendingDrawing(t):
    writeSchemaFile(t)    // per-diagram file
  choldDir()
```
Folding produces one `.svg` per named sub-diagram above `gFoldComplexity`.

### `generateInsideSchema` box→schema mapping (excerpt)
```
xtended (FFT, sin…)     → BlockSchema(arity, 1, name, normalcolor)
isInverter              → InverterSchema(invcolor)
isBoxInt/Real           → BlockSchema(0, 1, str, numcolor)
isBoxWire               → CableSchema()
isBoxCut                → CutSchema()
isBoxPrim0–5            → BlockSchema(n, 1, primNname, normalcolor)
isBoxFFun               → BlockSchema(ffarity, 1, ffname, normalcolor)
isBoxButton/Checkbox/…  → UserInterfaceSchema(t)
isBoxVBargraph/H…       → BargraphSchema(t)
isBoxSeq                → SeqSchema(rec(a), rec(b))
isBoxPar                → ParSchema(rec(a), rec(b))
isBoxSplit              → SplitSchema(rec(a), rec(b))
isBoxMerge              → MergeSchema(rec(a), rec(b))
isBoxRec                → RecSchema(rec(a), rec(b))
isBoxSlot               → BlockSchema(0, 1, name, slotcolor)
isBoxSymbolic           → AbstractionSchema(rec(body), slot)
isBoxVGroup/HGroup/TGroup → DecorateSchema(rec(a), "xgroup(label)")
isBoxRoute              → RouteSchema(ins, outs, connections)
isBoxOndemand           → OnDemandSchema(rec(a))
isBoxDownsample         → DownsamplingSchema(rec(a), rec(b))
isBoxUpsample           → UpsamplingSchema(rec(a), rec(b))
```

---

## Rust Architecture

### Crate layout (`crates/draw/src/`)

```
lib.rs          — public API: draw_schema(), DrawFormat, DrawError
device.rs       — DrawDevice trait + SvgDevice implementation
schema.rs       — Schema trait, Point, Trait, TraitCollector, Orientation, constants
schemas/
  block.rs      — BlockSchema (leaf)
  cable.rs      — CableSchema, CutSchema, ConnectorSchema, InverterSchema
  seq.rs        — SeqSchema
  par.rs        — ParSchema
  split.rs      — SplitSchema
  merge.rs      — MergeSchema
  rec.rs        — RecSchema
  composed.rs   — TopSchema, DecorateSchema, EnlargedSchema
  route.rs      — RouteSchema
  multirate.rs  — OnDemandSchema, DownSamplingSchema, UpSamplingSchema
translate.rs    — box tree → schema tree (generateInsideSchema port)
```

### `DrawDevice` trait
```rust
pub trait DrawDevice {
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64, color: &str, link: &str)
        -> Result<(), DrawError>;
    fn triangle(&mut self, x: f64, y: f64, w: f64, h: f64, color: &str, link: &str, left_right: bool)
        -> Result<(), DrawError>;
    fn circle(&mut self, x: f64, y: f64, radius: f64) -> Result<(), DrawError>;
    fn square(&mut self, x: f64, y: f64, side: f64) -> Result<(), DrawError>;
    fn arrow(&mut self, x: f64, y: f64, rotation: f64, direction: i32) -> Result<(), DrawError>;
    fn line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) -> Result<(), DrawError>;
    fn dashed_line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) -> Result<(), DrawError>;
    fn text(&mut self, x: f64, y: f64, name: &str, link: &str) -> Result<(), DrawError>;
    fn label(&mut self, x: f64, y: f64, name: &str) -> Result<(), DrawError>;
    fn mark_direction(&mut self, x: f64, y: f64, direction: i32) -> Result<(), DrawError>;
    fn error_msg(&mut self, msg: &str, reason: &str, n: usize, x: f64, y: f64, w: f64)
        -> Result<(), DrawError>;
}
```

`SvgDevice` opens a file, writes the `<svg>` header in `new()`, each method emits
the corresponding SVG element, and `Drop` closes the `</svg>` tag.
Use raw `write!()` — no external XML crate needed.

**C++ reference**: `device/device.h`, `device/SVGDev.h`, `device/SVGDev.cpp`

### `Schema` trait
```rust
pub trait Schema {
    fn width(&self) -> f64;
    fn height(&self) -> f64;
    fn inputs(&self) -> usize;
    fn outputs(&self) -> usize;
    fn place(&mut self, x: f64, y: f64, orientation: Orientation);
    fn input_point(&self, i: usize) -> Point;
    fn output_point(&self, i: usize) -> Point;
    fn draw(&self, dev: &mut dyn DrawDevice) -> Result<(), DrawError>;
    fn collect_traits(&self, c: &mut TraitCollector);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Orientation { LeftRight = 1, RightLeft = -1 }

#[derive(Clone, Copy, Debug, PartialOrd, PartialEq)]
pub struct Point { pub x: f64, pub y: f64 }

impl Ord for Point { /* lex order on (x, y) */ }

pub struct Trait { pub start: Point, pub end: Point,
                   pub has_real_input: bool, pub has_real_output: bool }
```

**C++ reference**: `schema/schema.h`

### Layout algorithm (two-pass)
The C++ code mutates schema state in `place()`. Rust uses the same two-pass model:

1. **Bottom-up (sizing)**: schemas compute `width`, `height` in their constructors based on children's sizes and the constants (`dWire`, `dLetter`, `dHorz`, `dVert`). All sizes are `f64`, set once, immutable thereafter.

2. **Top-down (placement)**: `place(x, y, orientation)` is called on the root, propagating coordinates to children. Each schema stores its assigned `x`, `y`, `orientation` in mutable fields (interior mutability via `Cell<f64>` or a `Placed` wrapper struct).

3. **Drawing**: `draw(dev)` walks the placed tree, emitting SVG elements.

4. **Wire collection**: `collect_traits(collector)` gathers `Trait`s. `TraitCollector::compute_visible()` filters them (equivalent to `computeVisibleTraits()`). Final `collector.draw(dev)` renders visible wires on top.

### `TraitCollector`
```rust
pub struct TraitCollector {
    pub outputs: BTreeSet<Point>,
    pub inputs:  BTreeSet<Point>,
    traits:      BTreeSet<Trait>,
}

impl TraitCollector {
    pub fn add_output(&mut self, p: Point);
    pub fn add_input(&mut self, p: Point);
    pub fn add_trait(&mut self, t: Trait);
    pub fn compute_visible(&mut self);   // marks has_real_input / has_real_output
    pub fn draw(&self, dev: &mut dyn DrawDevice) -> Result<(), DrawError>;
}
```

**C++ reference**: `schema/collector.cpp`

### Color constants (`translate.rs`)
```rust
pub const COLOR_LINK:   &str = "#003366";
pub const COLOR_NORMAL: &str = "#4B71A1";
pub const COLOR_UI:     &str = "#477881";
pub const COLOR_SLOT:   &str = "#47945E";
pub const COLOR_NUM:    &str = "#f44800";
pub const COLOR_INV:    &str = "#ffffff";
```

### Public API (`lib.rs`)
```rust
#[derive(Clone, Copy, Debug)]
pub enum DrawFormat { Svg, PostScript }  // PostScript → unimplemented!()

#[derive(Debug)]
pub enum DrawError { Io(std::io::Error), Layout(String) }

/// Generate block-diagram SVG files from a compiled box tree.
/// Creates `<output_dir>/<name>.svg` (and folded sub-diagrams if complex).
///
/// C++ reference: drawschema.cpp::drawSchema() (line 149)
pub fn draw_schema(
    box_tree: &boxes::EvalResult,
    output_dir: &Path,
    format: DrawFormat,
) -> Result<Vec<PathBuf>, DrawError>;
```

The `draw_schema` function:
1. Calls `translate::generate_diagram_schema()` on the process tree
2. Wraps with `TopSchema` (adds title and margin)
3. Creates `SvgDevice` writing to `output_dir/<name>.svg`
4. Calls `place(0.0, 0.0, Orientation::LeftRight)` on root schema
5. Calls `draw(dev)` on root schema
6. Calls `collect_traits(collector); collector.compute_visible(); collector.draw(dev)`
7. If folding enabled: repeats for each pending sub-diagram

**Folding parameters** (match C++ globals):
```rust
pub struct DrawConfig {
    pub fold:             bool,   // gFoldingFlag
    pub fold_threshold:   usize,  // gFoldThreshold  (default: 25)
    pub fold_complexity:  usize,  // gFoldComplexity (default: 2)
}
```

### CLI integration (`crates/compiler/src/main.rs`)

**In `normalize_legacy_args`** (after line 374):
```rust
if arg == "-svg" {
    normalized.push("--svg".to_owned());
    continue;
}
if arg == "-ps" {
    normalized.push("--ps".to_owned());
    continue;
}
```

**In `CliArgs`**:
```rust
/// Generate SVG block diagrams (creates <input>-svg/ directory).
#[arg(long = "svg", action = ArgAction::SetTrue)]
svg: bool,

/// Generate PostScript block diagrams (creates <input>-ps/ directory).
#[arg(long = "ps", action = ArgAction::SetTrue)]
ps: bool,
```

**In `main` dispatch**:
```rust
if args.svg || args.ps {
    let format = if args.ps { DrawFormat::PostScript } else { DrawFormat::Svg };
    let suffix = if args.ps { "ps" } else { "svg" };
    let stem = input_path.file_stem()
        .unwrap_or_default().to_string_lossy();
    let out_dir = input_path.parent().unwrap_or(Path::new("."))
        .join(format!("{stem}-{suffix}"));
    std::fs::create_dir_all(&out_dir)?;
    let files = draw::draw_schema(&eval_result, &out_dir, format)?;
    for f in &files {
        eprintln!("wrote {}", f.display());
    }
}
```

---

## Implementation Phases

### Phase A — Core infrastructure (3–4 days)
**Goal**: compiling skeleton, `DrawDevice` trait, `SvgDevice`, base types.

- [ ] Update `crates/draw/Cargo.toml` — add dependency on `boxes`
- [ ] `src/error.rs` — `DrawError` enum
- [ ] `src/schema.rs` — `Point`, `Trait`, `Orientation`, constants, `TraitCollector`
- [ ] `src/device.rs` — `DrawDevice` trait + `SvgDevice` (write XML via `write!()`)
  - XML entity escaping (`xmlcode` equivalent: `& < > " '`)
  - `<svg>` header with viewBox in constructor
  - `Drop` impl closes `</svg>`
  - All 11 drawing methods
- [ ] `src/schemas/block.rs` — `BlockSchema` (leaf, most used)
- [ ] Unit tests: `SvgDevice` entity escaping, `BlockSchema` sizing

**C++ references**:  
`device/device.h`, `device/SVGDev.cpp`, `schema/schema.h`, `schema/blockSchema.cpp`

### Phase B — Remaining leaf schemas (1–2 days)
- [ ] `src/schemas/cable.rs` — `CableSchema`, `CutSchema`
- [ ] Connector and inverter in same file — `ConnectorSchema`, `InverterSchema`
- [ ] Unit tests for each: correct `width()`, `height()`, `input_point()`, `output_point()`

**C++ references**:  
`schema/cableSchema.cpp`, `schema/cutSchema.cpp`, `schema/connectorSchema.cpp`, `schema/inverterSchema.cpp`

### Phase C — Binary composition schemas (3–4 days)
**Order** (simplest first):
- [ ] `SeqSchema` — inputs from s1, outputs from s2, connecting wires
- [ ] `ParSchema` — stack vertically, split input/output frontiers
- [ ] `MergeSchema` — triangle-shaped merge
- [ ] `SplitSchema` — triangle-shaped split
- [ ] `RecSchema` — feedback loop with arrow marking
- [ ] Unit tests: `SeqSchema` width ≥ sum, `ParSchema` height = sum, wire counts

**C++ references**: `schema/seqSchema.cpp`, `parSchema.cpp`, `mergeSchema.cpp`, `splitSchema.cpp`, `recSchema.cpp`

### Phase D — Decorator/wrapper schemas (1–2 days)
- [ ] `TopSchema` — title text + outer margin
- [ ] `DecorateSchema` — named group border
- [ ] `EnlargedSchema` — width padding
- [ ] Unit tests: dimension invariants

**C++ references**: `schema/topSchema.cpp`, `schema/decorateSchema.cpp`, `schema/enlargedSchema.cpp`

### Phase E — Specialized schemas (1–2 days)
- [ ] `RouteSchema` — explicit I/O permutation (parse connection list)
- [ ] `OnDemandSchema`, `DownSamplingSchema`, `UpSamplingSchema`

**C++ references**: `schema/routeSchema.cpp`, `ondemandSchema.cpp`, `downsamplingSchema.cpp`, `upsamplingSchema.cpp`

### Phase F — Box → schema translation (3–4 days)
Port `generateInsideSchema` and `generateDiagramSchema` from `drawschema.cpp`.

- [ ] `src/translate.rs` — `generate_inside_schema(box_id, box_tree) -> Box<dyn Schema>`
- [ ] Pattern-match on all box variants (see §Key data structures above)
- [ ] `generate_diagram_schema` — folding + `DecorateSchema` for named boxes
- [ ] `draw_config.rs` — `DrawConfig` struct with fold parameters
- [ ] Integration tests: simple DSPs produce non-empty SVG files

**C++ reference**: `drawschema.cpp:359–600`

### Phase G — CLI wiring + end-to-end (1–2 days)
- [ ] `normalize_legacy_args` — map `-svg` → `--svg`, `-ps` → `--ps`
- [ ] `CliArgs` — add `svg: bool`, `ps: bool` fields
- [ ] `main` dispatch — call `draw::draw_schema()`
- [ ] End-to-end smoke test: `faust-rs -svg sine.dsp` produces `sine-svg/process.svg`
- [ ] Parity test: compare element counts and text labels with reference Faust output

### Phase H — SVG visual options (1–2 days) ← **next**

Port the draw-layer visual options that require no eval-stage changes.
All four are additions to `DrawConfig` + wired via CLI flags.

#### H1 — `--draw-route-frame` / `-drf` (draw route frames)

**C++ global**: `gDrawRouteFrame` (default false).  
**Location**: `schema/routeSchema.cpp` — toggles `drawRectangle` + `drawOrientationMark`.  
**Current Rust state**: `RouteSchema::draw()` always draws the rect + mark.  
**Action**: Make rect/mark conditional on a `draw_route_frame: bool` flag in `DrawConfig`.
Without the flag, draw simple cables (no rectangle, no mark). This requires passing
`DrawConfig` into `draw()` or storing the flag on `RouteSchema` at construction time.
**Preferred approach**: thread `DrawConfig` as an extra parameter through `draw()` calls.
Alternative: store `bool` on `RouteSchema`.

#### H2 — `--shadow-blur` / `-blur` (Gaussian drop-shadow on boxes)

**C++ global**: `gShadowBlur` (default false).  
**Location**: `device/SVGDev.cpp` — emits `<defs><filter>` in SVG header, uses
`filter:url(#filter)` style on shadow rects.  
**Action**:
- Add `shadow_blur: bool` to `SvgDevice::new()` parameters.
- When true: emit `<defs>` block with `<feGaussianBlur stdDeviation="1.55">` +
  `<feOffset dx="3" dy="3">` in header.
- Change shadow rect style from `fill:#cccccc` to `fill:#aaaaaa;filter:url(#filter)`.
- All other draw calls unchanged.

#### H3 — `--scaled-svg` / `-sc` (viewBox-only, responsive)

**C++ global**: `gScaledSVG` (default false).  
**Location**: `device/SVGDev.cpp` — when true, omits fixed `width=`/`height=` attributes
so the SVG scales freely; uses only `viewBox`.  
**Action**: Add `scaled: bool` to `SvgDevice::new()`. When true, emit SVG header
without `width=` and `height=` mm attributes (viewBox-only).

#### H4 — `--max-name-size` / `-mns N` (truncate long names, default 40)

**C++ global**: `gMaxNameSize` (default 40).  
**Location**: `compiler/utils/names.cpp` — `checkName()` truncates names > N chars
as `first_third + "..." + last_third`.  
**Action**: Add `max_name_size: usize` (default 40) to `DrawConfig`. In `translate.rs`,
apply a `truncate_name(s, max)` helper before passing text to `make_block()`.

### Phase I — Hierarchical SVG with folding ✅ done

Port the folding mechanism that splits a complex diagram into multiple linked SVG files.
This is the main feature enabling navigation in the block-diagram browser.

**C++ globals ported**: `gFoldThreshold` (default 25), `gFoldComplexity` (default 2),
`gFoldingFlag` (derived), `gBackLink` (back-link map), `gDrawnExp` (visited set),
`gPendingExp` (queue), `gSchemaFileName` (current file name).

#### I1 — `box_complexity` (`crates/boxes/src/complexity.rs`)

Recursive complexity scorer added as `pub fn box_complexity(arena, BoxId) → usize`:
- Cuts, wires, routes, slots, environment → **0**
- All primitives, UI widgets, foreign items → **1**
- Compositions (seq/par/split/merge/rec) → **sum of children**
- Groups (vgroup/hgroup/tgroup) → **complexity of body** (transparent)
- Symbolic/ondemand/up/downsampling → **1 + child**
- Metadata → **transparent**

#### I2 — Definition-name tracking in eval

`LoopDetector` gains `pub(crate) def_names: HashMap<TreeId, String>`.
In `eval_ident_value()`, after forcing a named closure to a box, the mapping
`box_id → name` is recorded in `loop_detector.def_names`.
`eval_entrypoint_full()` extracts the map into `EvalStats.def_names`.
`SignalCompileOutput` gains `pub def_names: HashMap<BoxId, String>`.
The compiler now uses `eval_entrypoint_with_stats` variants to capture def names.

#### I3 — Folding infrastructure (`crates/draw/src/translate.rs`)

New `pub struct FoldState`: holds `def_names`, `pending` queue, `drawn` set,
`current_file`, `folding` flag, `fold_complexity`.

New `generate_diagram_schema(arena, b, config, state)`: checks folding conditions,
schedules sub-diagrams into `pending`, or decorates named non-routing sub-diagrams,
or falls through to `generate_inside_folded`.

New `generate_inside_folded`: same as `generate_inside` but passes `state` to
`generate_diagram_schema` for all composition children.

Helpers: `pub fn legal_file_name(name, box_id) → String` (stem ≤ 16 alphanum + hex
suffix, except `"process"`); `fn is_pure_routing(arena, BoxId) → bool`.

#### I4 — `draw_schema` folding loop (`crates/draw/src/lib.rs`)

`draw_schema()` API changed:
- `output_path: &Path` → `out_dir: &Path`  (directory, not file)
- New parameter: `def_names: &HashMap<BoxId, String>`

New fields in `DrawConfig`: `fold_threshold: usize` (default 25),
`fold_complexity: usize` (default 2).

Main loop: `pending` VecDeque; root is seeded as `(root, name, "")`.  Each iteration
pops one diagram, calls `generate_folded_inside`, wraps in `TopSchema`, renders to
`legal_file_name(diagram_name, box_id)` in `out_dir`.

#### I5 — CLI wiring

`-f N` / `--fold N` and `-fc N` / `--fold-complexity N` added to `CliArgs` and
`normalize_legacy_args`.  `DrawConfig` built from all 6 SVG flags and passed to
`draw_schema` along with `out.def_names`.

### Phase J — Signal → Dot (optional, 1–2 days)
Port `sigToGraph.cpp` for Graphviz dot output.  
Add `--dump-sig-dot` flag writing to stdout.

### Phase K — PostScript (deferred)
`PostScriptDevice` is stubbed with `unimplemented!("PostScript output not yet supported")`.
Implement in a later phase once SVG parity is confirmed.

### Out of scope for draw module

#### `-sd` / `--simplify-diagrams`
**C++ location**: `evaluate/eval.cpp` — calls `boxSimplification(b)` after eval.  
**Scope**: eval stage, not draw. Implement in the `eval` crate as a post-eval pass.

#### `-sn` / `--simple-names`
**C++ location**: `evaluate/eval.cpp` — omits argument details when setting
`DefNameProperty` during function application.  
**Scope**: eval stage. The draw layer receives name strings opaquely; it cannot strip
argument details without re-parsing the name. Implement as an eval-stage flag.

---

## Key Porting Decisions

### 1. Trait objects vs enum dispatch
Use `Box<dyn Schema>` for composite schemas (mirrors C++ pointer semantics).
No enum dispatch — the 18 types vary too widely in size and placement logic.

### 2. Placement state: `Cell<f64>` in schema structs
C++ mutates `fX`, `fY`, `fOrientation` in `place()`. In Rust, store placement fields
as `Cell<f64>` / `Cell<i32>` (or a `PlacedState` wrapper behind `RefCell`) so
`&self` draw methods can read them after a `&mut self` place call.
Simpler alternative: make `place` take `&mut self` and store coords directly in the struct.
**Decision**: `&mut self` for `place()`, `&self` for `draw()` and `collect_traits()`.

### 3. File naming (legalFileName)
Port `legalFileName()` (replaces non-alphanumeric chars with `_`, caps at 128 chars,
appends decimal tree pointer suffix for uniqueness). Keep the logic in `translate.rs`.

### 4. No external SVG crate
Write raw `write!()` to `BufWriter<File>`. Avoids a dependency for a well-bounded output
format. Only risk: malformed XML for unusual labels — cover with entity-escape unit tests.

### 5. `boxComplexity` dependency
The fold decision requires `boxComplexity(tree)`. Import from the `boxes` crate.
If not yet exposed, add a public `box_complexity(id: BoxId, tree: &BoxTree) -> usize` there.

---

## Testing Strategy

### Unit tests (in `draw` crate)
```rust
// schema.rs
fn test_block_sizing()         // BlockSchema(2,1,"test","#000","").width() > 0
fn test_seq_width_geq_sum()    // SeqSchema width ≥ s1.width() + s2.width()
fn test_par_height_eq_sum()    // ParSchema height = s1.height() + s2.height()
fn test_collector_visible()    // only traits with real endpoints survive filter

// device.rs
fn test_svg_entity_escape()    // & < > " ' in label → &amp; &lt; &gt; &quot; &apos;
fn test_svg_header_viewbox()   // first output line contains <svg ... viewBox=
```

### Integration tests (`crates/draw/tests/`)
```rust
fn test_draw_wire()    // process = _; → svg file with exactly 1 cable element
fn test_draw_seq()     // process = _ : *(2); → seq schema contains block + cable
fn test_draw_rec()     // process = +~_; → contains rec schema with arrow
fn test_draw_ui()      // vslider → uicolor block
fn test_draw_folding() // complex DSP with folding → multiple svg files
```

### Parity tests (against reference Faust)
For each corpus DSP:
1. Run reference `faust -svg test.dsp` → reference SVGs
2. Run `faust-rs --svg test.dsp` → candidate SVGs
3. Compare: `<svg>` viewBox dimensions within 5%, same text labels, same element count types

---

## Files to Create / Modify

| File | Action | C++ source |
|------|--------|-----------|
| `crates/draw/Cargo.toml` | update — add `boxes` dep | — |
| `crates/draw/src/lib.rs` | rewrite — public API | — |
| `crates/draw/src/error.rs` | new | — |
| `crates/draw/src/device.rs` | new | `device/device.h`, `device/SVGDev.h/cpp` |
| `crates/draw/src/schema.rs` | new | `schema/schema.h`, `schema/collector.cpp` |
| `crates/draw/src/schemas/block.rs` | new | `schema/blockSchema.h/cpp` |
| `crates/draw/src/schemas/cable.rs` | new | `schema/cableSchema.h/cpp`, `cutSchema.h/cpp`, `connectorSchema.h/cpp`, `inverterSchema.h/cpp` |
| `crates/draw/src/schemas/seq.rs` | new | `schema/seqSchema.h/cpp` |
| `crates/draw/src/schemas/par.rs` | new | `schema/parSchema.h/cpp` |
| `crates/draw/src/schemas/split.rs` | new | `schema/splitSchema.h/cpp` |
| `crates/draw/src/schemas/merge.rs` | new | `schema/mergeSchema.h/cpp` |
| `crates/draw/src/schemas/rec.rs` | new | `schema/recSchema.h/cpp` |
| `crates/draw/src/schemas/composed.rs` | new | `schema/topSchema.h/cpp`, `decorateSchema.h/cpp`, `enlargedSchema.h/cpp` |
| `crates/draw/src/schemas/route.rs` | new | `schema/routeSchema.h/cpp` |
| `crates/draw/src/schemas/multirate.rs` | new | `schema/ondemandSchema.h/cpp`, `downsamplingSchema.h/cpp`, `upsamplingSchema.h/cpp` |
| `crates/draw/src/translate.rs` | new | `drawschema.cpp:359–600` |
| `crates/compiler/src/main.rs` | update — `-svg` flag, dispatch | `drawschema.cpp:149` |

---

## Effort Estimate

| Phase | Description | Days | Status |
|-------|------------|------|--------|
| A | Core infra + SvgDevice | 3–4 | ✅ done |
| B | Leaf schemas | 1–2 | ✅ done |
| C | Composition schemas | 3–4 | ✅ done |
| D | Decorator schemas | 1–2 | ✅ done |
| E | Specialized schemas | 1–2 | ✅ done |
| F | Translation layer | 3–4 | ✅ done |
| G | CLI `-svg` flag | 1–2 | ✅ done |
| H | Visual options: -blur, -sc, -drf, -mns | 1–2 | ✅ done |
| I | Hierarchical folding (-f, -fc) | 3–4 | ✅ done |
| J | Signal → Dot (optional) | 1–2 | 🔲 optional |
| K | PostScript (deferred) | 2–3 | 🔲 deferred |
| — | `-sd`, `-sn` (eval-stage) | — | out of scope |
| **Total** | | **~22–28** | |

---

## Definition of Done

### Phases A–G (complete)
- [x] `cargo test -p draw` passes (24 unit tests)
- [x] `faust-rs -svg <file>.dsp` produces `<stem>-svg/process.svg`
- [x] All box primitives, UI widgets, composition operators render
- [x] `-svg` alias wired via `normalize_legacy_args`
- [x] No `unsafe` code

### Phase H (next)
- [ ] `-blur` / `--shadow-blur`: SVG drop-shadow filter added to boxes
- [ ] `-sc` / `--scaled-svg`: SVG header uses viewBox only (no fixed mm size)
- [ ] `-drf` / `--draw-route-frame`: route boxes drawn as frames vs cables
- [ ] `-mns N` / `--max-name-size`: names > N chars truncated as `first...last`
- [ ] All new flags wired via `normalize_legacy_args`

### Phase I (folding — hierarchical SVGs)
- [x] `box_complexity` implemented in `boxes` crate (`crates/boxes/src/complexity.rs`)
- [x] Def-name tracking in eval: `LoopDetector.def_names`, `EvalStats.def_names`, `SignalCompileOutput.def_names`
- [x] Folding queue produces multiple linked `.svg` files
- [x] `FoldState`, `generate_diagram_schema`, `generate_inside_folded`, `legal_file_name`, `is_pure_routing` in `translate.rs`
- [x] `-f N` / `--fold N` and `-fc N` / `--fold-complexity N` flags wired
- [x] Navigation links between files work in browser

### All phases
- [ ] `cargo clippy --all-targets -- -D warnings` clean
- [ ] PostScript path returns a clear `DrawError::NotSupported` message
- [ ] Block diagram structure visually matches reference Faust on ≥10 corpus DSPs

---

*C++ source: `faust/compiler/draw/` (drawschema.cpp, schema/schema.h, device/SVGDev.h/cpp)*
