//! `xtask` CLI entry point for repository maintenance workflows.
//!
//! # Role
//! - Hosts developer/CI automation that should not be part of runtime compiler
//!   crates (golden generation/checks, parity reports, differential reports).
//!
//! # Primary workflows
//! - Golden snapshots:
//!   - `golden-check`, `golden-check-cpp`
//!   - `golden-gen-rust`, `golden-gen-cpp`
//! - Runtime trace validation (interp backend):
//!   - `interp-trace-dump` (Phase 1 harness prototype)
//!   - `interp-trace-gen`, `interp-trace-check` (Phase 2 snapshot scaffold)
//!   - `interp-trace-dump-cppfbc` (C++ Faust `.fbc` -> Rust interp runtime)
//!   - `interp-trace-gen-cppfbc` (batch-generate persisted traces from C++ `.fbc`)
//!   - `fir-dump-scan` (structural scan of `dump_fir` loop body expansion)
//! - Backend alignment:
//!   - `backend-align-smoke` (CI-friendly smoke alignment orchestration,
//!     including `opt_level=0` vs `opt_level=max` interpreter drift checks)
//!   - `backend-align-nightly` (broader alignment orchestration)
//! - Developer navigation:
//!   - `code-graphs` (Mermaid/DOT/SVG crate graphs, curated IR overview, and a
//!     public API source-scan index)
//! - Wasm integration:
//!   - `build-faustwasm-compiler-module` (`wasm-ffi` -> verified `.wasm`)
//! - Differential reports:
//!   - parser parity report
//!   - corpus status report
//!   - backend diff reports
//!
//! # Design invariants
//! - Deterministic corpus file ordering.
//! - Normalized output text before snapshot comparison.
//! - Fail-fast behavior when one case diverges to preserve CI signal quality.
//! - Generated documentation uses repository-relative paths where practical.
//! - The command surface stays intentionally simple: argument parsing is local to
//!   each workflow instead of adding a runtime CLI dependency to this helper
//!   crate.

use fir::dump_fir;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;
use wasmparser::{ExternalKind, Parser, Payload};

/// Human-readable command summary printed when no command or an unknown command
/// is provided.
///
/// The project intentionally keeps `xtask` argument parsing lightweight. This
/// string is the canonical short-form help for the dispatcher below; the longer
/// workflow documentation lives in `crates/xtask/README.md`.
const USAGE: &str = "\
Usage:
  cargo run -p xtask -- golden-check
  cargo run -p xtask -- golden-check-cpp
  cargo run -p xtask -- golden-gen-rust
  cargo run -p xtask -- golden-gen-cpp [-- <extra args passed to FAUST_CPP_BIN>]
  cargo run -p xtask -- interp-trace-dump --case <tests/corpus/foo.dsp> [--scenario zeros|impulse|ramp|sine] [--lane fast] [--strict-fir-types]
  cargo run -p xtask -- interp-trace-dump-cppfbc --case <tests/corpus/foo.dsp> [--scenario zeros|impulse|ramp|sine] [--faust-bin /path/to/faust]
  cargo run -p xtask -- interp-trace-gen-cppfbc [--case <tests/corpus/foo.dsp>] [--scenario zeros|impulse|ramp|sine] [--out-dir <dir>] [--faust-bin /path/to/faust]
  cargo run -p xtask -- interp-trace-gen [--case <tests/runtime_corpus/foo.dsp>] [--lane fast] [--strict-fir-types]
  cargo run -p xtask -- interp-trace-check [--case <tests/runtime_corpus/foo.dsp>] [--lane fast] [--strict-fir-types]
  cargo run -p xtask -- fir-dump-scan [--case <tests/corpus/foo.dsp> ...] [--lane fast]
  cargo run -p xtask -- build-faustwasm-compiler-module [--debug]
  cargo run -p xtask -- backend-align-smoke [--case <tests/runtime_corpus/foo.dsp> ...] [--strict-fir-types] [--skip-golden] [--skip-fir-dump-scan]
  cargo run -p xtask -- backend-align-nightly [--strict-fir-types] [--skip-golden] [--skip-fir-dump-scan]
  cargo run -p xtask -- code-graphs [--out-dir <dir>]
  cargo run -p xtask -- parser-parity-report
  cargo run -p xtask -- corpus-status-report
  cargo run -p xtask -- cpp-backend-diff-report
  cargo run -p xtask -- c-fastlane-diff-report
  cargo run -p xtask -- backend-full-corpus-diff-report
  cargo run -p xtask -- table-fastlane-diff-report
\nEnvironment for golden-gen-cpp:
  FAUST_CPP_BIN   Path to reference C++ faust binary
\nEnvironment for golden-check:
  GOLDEN_REF      rust (default) or cpp
";

/// Local checkout of the reference C++ Faust source tree used by static parser
/// report generation.
///
/// Runtime workflows that need a C++ Faust executable use `FAUST_CPP_BIN` or an
/// explicit `--faust-bin` instead.
const CPP_SOURCE_ROOT: &str = "/Users/letz/Developpements/RUST/faust";

/// Parser parity report output path, relative to the workspace root.
const PARITY_REPORT_REL_PATH: &str = "porting/phases/phase-3-parser-parity-report-en.md";

/// Corpus accept/reject diff report output path, relative to the workspace root.
const CORPUS_STATUS_REPORT_REL_PATH: &str =
    "porting/phases/phase-4-corpus-status-diff-report-en.md";

/// C++ backend differential report output path, relative to the workspace root.
const CPP_BACKEND_DIFF_REPORT_REL_PATH: &str =
    "porting/phases/phase-6-cpp-backend-diff-report-en.md";

/// C fast-lane differential report output path, relative to the workspace root.
const C_FASTLANE_DIFF_REPORT_REL_PATH: &str = "porting/phases/phase-6-c-fastlane-diff-report-en.md";

/// Full backend corpus diff report output path, relative to the workspace root.
const BACKEND_FULL_CORPUS_DIFF_REPORT_REL_PATH: &str =
    "porting/phases/phase-6-backend-full-corpus-diff-report-en.md";

/// Table lowering fast-lane report output path, relative to the workspace root.
const TABLE_FASTLANE_DIFF_REPORT_REL_PATH: &str =
    "porting/phases/phase-6-table-fastlane-diff-report-en.md";

/// `xtask` process entry point.
fn main() {
    if let Err(err) = run() {
        eprintln!("xtask error: {err}");
        std::process::exit(1);
    }
}

/// Dispatches one `xtask` subcommand.
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        print!("{USAGE}");
        return Ok(());
    };

    match command.as_str() {
        "golden-check" => golden_check(None)?,
        "golden-check-cpp" => golden_check(Some(GoldenRef::Cpp))?,
        "golden-gen-rust" => golden_gen_rust()?,
        "golden-gen-cpp" => {
            let mut passthrough: Vec<OsString> = Vec::new();
            let mut separator_seen = false;
            for arg in args {
                if separator_seen {
                    passthrough.push(OsString::from(arg));
                } else if arg == "--" {
                    separator_seen = true;
                }
            }
            golden_gen_cpp(&passthrough)?;
        }
        "interp-trace-dump" => interp_trace_dump(args)?,
        "interp-trace-dump-cppfbc" => interp_trace_dump_cppfbc(args)?,
        "interp-trace-gen-cppfbc" => interp_trace_gen_cppfbc(args)?,
        "interp-trace-gen" => interp_trace_gen(args)?,
        "interp-trace-check" => interp_trace_check(args)?,
        "fir-dump-scan" => fir_dump_scan(args)?,
        "build-faustwasm-compiler-module" => build_faustwasm_compiler_module(args)?,
        "backend-align-smoke" => backend_align_smoke(args)?,
        "backend-align-nightly" => backend_align_nightly(args)?,
        "code-graphs" => code_graphs(args)?,
        "parser-parity-report" => parser_parity_report()?,
        "corpus-status-report" => corpus_status_report()?,
        "cpp-backend-diff-report" => cpp_backend_diff_report()?,
        "c-fastlane-diff-report" => c_fastlane_diff_report()?,
        "backend-full-corpus-diff-report" => backend_full_corpus_diff_report()?,
        "table-fastlane-diff-report" => table_fastlane_diff_report()?,
        _ => {
            print!("{USAGE}");
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Shared workspace/path helpers
// ---------------------------------------------------------------------------

/// Returns the canonical workspace root path.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .to_path_buf()
        })
}

/// Formats a path relative to the workspace root when possible.
fn workspace_relative_path(path: &Path) -> String {
    let root = workspace_root();
    if let Ok(relative) = path.strip_prefix(&root) {
        return relative.display().to_string();
    }
    if let Ok(canonical) = path.canonicalize()
        && let Ok(relative) = canonical.strip_prefix(&root)
    {
        return relative.display().to_string();
    }
    path.display().to_string()
}

// ---------------------------------------------------------------------------
// `code-graphs` support
// ---------------------------------------------------------------------------

/// Minimal subset of `cargo metadata --format-version 1` used by the
/// `code-graphs` workflow.
///
/// Keeping this structure narrow makes the generator resilient to unrelated
/// metadata additions while still allowing typed access to workspace members and
/// package-level dependency data.
#[derive(Debug, Deserialize)]
struct CargoMetadata {
    /// All packages reported by Cargo for the current workspace query.
    packages: Vec<CargoPackage>,
    /// Package IDs that belong to the active workspace.
    workspace_members: Vec<String>,
}

/// Package metadata needed to render crate nodes, dependency edges, and the
/// public API source-scan index.
#[derive(Debug, Deserialize)]
struct CargoPackage {
    /// Cargo package name, used as the graph label and public API section title.
    name: String,
    /// Stable package ID from Cargo metadata.
    id: String,
    /// Declared dependencies. Path dependencies are used to identify internal
    /// workspace edges.
    dependencies: Vec<CargoDependency>,
    /// Package manifest path; its parent directory is used to map path
    /// dependencies back to package names.
    manifest_path: PathBuf,
    /// Build targets. Library-like targets provide source roots for the public
    /// item index.
    targets: Vec<CargoTarget>,
}

/// Dependency metadata needed to decide whether an edge is internal to the
/// workspace.
#[derive(Debug, Deserialize)]
struct CargoDependency {
    /// Dependency package name as written in Cargo metadata.
    name: String,
    /// Local path for path dependencies. Registry dependencies have no path and
    /// are intentionally omitted from internal crate graphs.
    path: Option<PathBuf>,
}

/// Target metadata used to discover crate source roots.
#[derive(Debug, Deserialize)]
struct CargoTarget {
    /// Cargo target kinds, for example `lib`, `rlib`, `cdylib`, or `bin`.
    kind: Vec<String>,
    /// Main source file for the target.
    src_path: PathBuf,
}

/// Parsed options for `cargo run -p xtask -- code-graphs`.
#[derive(Debug)]
struct CodeGraphOptions {
    /// Destination directory for generated Mermaid, DOT, SVG, README, and public
    /// API index files.
    out_dir: PathBuf,
}

impl Default for CodeGraphOptions {
    fn default() -> Self {
        Self {
            out_dir: workspace_root().join("docs/code-graphs"),
        }
    }
}

/// One public source item found by the lightweight public API scanner.
///
/// This is deliberately a source index, not a semantic model. Rustdoc remains
/// the authoritative API representation; this index is optimized for quick
/// navigation across the workspace.
#[derive(Debug)]
struct PublicItem {
    /// Item category such as `struct`, `enum`, `trait`, `fn`, or `use`.
    kind: String,
    /// Parsed item name. For re-exports, this is the first path-like token after
    /// `pub use`.
    name: String,
    /// Source file containing the public item.
    path: PathBuf,
    /// One-based source line number.
    line: usize,
}

/// Parses flags for the `code-graphs` documentation generator.
fn parse_code_graph_options(
    mut args: impl Iterator<Item = String>,
) -> Result<CodeGraphOptions, Box<dyn std::error::Error>> {
    let mut options = CodeGraphOptions::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out-dir" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --out-dir".into());
                };
                options.out_dir = PathBuf::from(value);
            }
            other => {
                return Err(format!(
                    "usage: cargo run -p xtask -- code-graphs [--out-dir <dir>]\nunknown option: {other}"
                )
                .into());
            }
        }
    }
    if options.out_dir.is_relative() {
        options.out_dir = workspace_root().join(&options.out_dir);
    }
    Ok(options)
}

/// Generates workspace/dependency graphs, IR overview graphs, and a public API
/// index for developer navigation.
///
/// Output is deterministic for stable `cargo metadata` and stable source file
/// ordering. SVG generation requires Graphviz `dot`; failing to render one SVG
/// fails the workflow so checked-in visualizations do not silently go stale.
fn code_graphs(args: impl Iterator<Item = String>) -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_code_graph_options(args)?;
    fs::create_dir_all(&options.out_dir)?;

    let metadata = load_cargo_metadata()?;
    let workspace = workspace_packages(&metadata);
    let edges = internal_dependency_edges(&workspace);

    write_text(
        &options.out_dir.join("workspace-crates.mmd"),
        &render_workspace_mermaid(&workspace),
    )?;
    write_text(
        &options.out_dir.join("workspace-crates.dot"),
        &render_workspace_dot(&workspace),
    )?;
    write_text(
        &options.out_dir.join("internal-crate-deps.mmd"),
        &render_internal_deps_mermaid(&workspace, &edges),
    )?;
    write_text(
        &options.out_dir.join("internal-crate-deps.dot"),
        &render_internal_deps_dot(&workspace, &edges),
    )?;
    write_text(
        &options.out_dir.join("ir-overview.mmd"),
        &render_ir_overview_mermaid(),
    )?;
    write_text(
        &options.out_dir.join("ir-overview.dot"),
        &render_ir_overview_dot(),
    )?;
    render_svg_with_dot(
        &options.out_dir.join("workspace-crates.dot"),
        &options.out_dir.join("workspace-crates.svg"),
    )?;
    render_svg_with_dot(
        &options.out_dir.join("internal-crate-deps.dot"),
        &options.out_dir.join("internal-crate-deps.svg"),
    )?;
    render_svg_with_dot(
        &options.out_dir.join("ir-overview.dot"),
        &options.out_dir.join("ir-overview.svg"),
    )?;
    write_text(
        &options.out_dir.join("public-api-index.md"),
        &render_public_api_index(&workspace)?,
    )?;
    write_text(
        &options.out_dir.join("README.md"),
        &render_code_graphs_readme(),
    )?;

    println!(
        "wrote code graph documentation to {}",
        workspace_relative_path(&options.out_dir)
    );
    Ok(())
}

/// Loads typed workspace metadata by invoking Cargo rather than parsing
/// manifests by hand.
///
/// This keeps feature resolution, package IDs, target metadata, and path
/// dependencies aligned with Cargo's own view of the workspace.
fn load_cargo_metadata() -> Result<CargoMetadata, Box<dyn std::error::Error>> {
    let output = Command::new("cargo")
        .current_dir(workspace_root())
        .arg("metadata")
        .arg("--no-deps")
        .arg("--format-version")
        .arg("1")
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

/// Returns workspace packages sorted by package name.
///
/// Sorting makes generated graph files stable across platforms and independent
/// of the ordering returned by Cargo.
fn workspace_packages(metadata: &CargoMetadata) -> Vec<&CargoPackage> {
    let members: BTreeSet<&str> = metadata
        .workspace_members
        .iter()
        .map(String::as_str)
        .collect();
    let mut packages: Vec<&CargoPackage> = metadata
        .packages
        .iter()
        .filter(|package| members.contains(package.id.as_str()))
        .collect();
    packages.sort_by(|left, right| left.name.cmp(&right.name));
    packages
}

/// Builds deterministic dependency edges between workspace packages.
///
/// Registry dependencies are ignored. Path dependencies are matched through
/// manifest parent directories so package renames in `Cargo.toml` still resolve
/// to the actual workspace package name.
fn internal_dependency_edges(packages: &[&CargoPackage]) -> Vec<(String, String)> {
    let manifest_parent_to_name: BTreeMap<PathBuf, String> = packages
        .iter()
        .filter_map(|package| {
            package
                .manifest_path
                .parent()
                .map(|parent| (parent.to_path_buf(), package.name.clone()))
        })
        .collect();
    let names: BTreeSet<&str> = packages
        .iter()
        .map(|package| package.name.as_str())
        .collect();
    let mut edges = BTreeSet::new();

    for package in packages {
        for dep in &package.dependencies {
            let dep_name = dep
                .path
                .as_ref()
                .and_then(|path| manifest_parent_to_name.get(path))
                .map_or(dep.name.as_str(), String::as_str);
            if names.contains(dep_name) {
                edges.insert((package.name.clone(), dep_name.to_owned()));
            }
        }
    }

    edges.into_iter().collect()
}

/// Renders a Mermaid graph containing one node per workspace crate.
fn render_workspace_mermaid(packages: &[&CargoPackage]) -> String {
    let mut out = String::from("flowchart LR\n");
    for package in packages {
        let id = graph_id(&package.name);
        let label = mermaid_label(&package.name);
        let _ = writeln!(out, "    {id}[\"{label}\"]");
    }
    out
}

/// Renders a DOT graph containing one node per workspace crate.
fn render_workspace_dot(packages: &[&CargoPackage]) -> String {
    let mut out = String::from("digraph workspace_crates {\n");
    out.push_str("    rankdir=LR;\n");
    out.push_str("    node [shape=box, style=\"rounded\"];\n");
    for package in packages {
        let _ = writeln!(out, "    \"{}\";", dot_escape(&package.name));
    }
    out.push_str("}\n");
    out
}

/// Renders a Mermaid graph of internal workspace crate dependencies.
fn render_internal_deps_mermaid(packages: &[&CargoPackage], edges: &[(String, String)]) -> String {
    let mut out = render_workspace_mermaid(packages);
    for (from, to) in edges {
        let _ = writeln!(out, "    {} --> {}", graph_id(from), graph_id(to));
    }
    out
}

/// Renders a DOT graph of internal workspace crate dependencies.
fn render_internal_deps_dot(packages: &[&CargoPackage], edges: &[(String, String)]) -> String {
    let mut out = render_workspace_dot(packages);
    out.truncate(out.trim_end_matches("}\n").len());
    for (from, to) in edges {
        let _ = writeln!(
            out,
            "    \"{}\" -> \"{}\";",
            dot_escape(from),
            dot_escape(to)
        );
    }
    out.push_str("}\n");
    out
}

/// Renders a curated Mermaid overview of the main Faust IR layers.
///
/// This graph is intentionally hand-authored: it documents architectural
/// relationships that cannot be inferred reliably from Cargo dependencies
/// alone.
fn render_ir_overview_mermaid() -> String {
    String::from(
        "flowchart LR\n\
         \n\
             subgraph Boxes[\"boxes IR\"]\n\
                 BoxBuilder[\"BoxBuilder\"]\n\
                 BoxTree[\"BoxId / TreeArena\"]\n\
                 BoxMatch[\"match_box / BoxMatch\"]\n\
                 BoxBuilder --> BoxTree --> BoxMatch\n\
             end\n\
         \n\
             subgraph UI[\"ui IR\"]\n\
                 UiProgram[\"UiProgram\"]\n\
                 UiItems[\"groups / controls / metadata\"]\n\
                 UiProgram --> UiItems\n\
             end\n\
         \n\
             subgraph Signals[\"signals IR\"]\n\
                 SigBuilder[\"SigBuilder\"]\n\
                 SigTree[\"SigId / TreeArena\"]\n\
                 SigMatch[\"match_sig / SigMatch\"]\n\
                 AdRules[\"ad_rules\"]\n\
                 SigBuilder --> SigTree --> SigMatch\n\
                 AdRules --> SigBuilder\n\
             end\n\
         \n\
             subgraph FIR[\"fir IR\"]\n\
                 FirStore[\"FirStore\"]\n\
                 FirModule[\"FirModule\"]\n\
                 FirVerifier[\"checker\"]\n\
                 FirInliner[\"inliner\"]\n\
                 FirStore --> FirModule --> FirVerifier\n\
                 FirModule --> FirInliner\n\
             end\n\
         \n\
             boxes[\"boxes crate\"] --> eval[\"eval crate\"] --> propagate[\"propagate crate\"]\n\
             propagate --> Signals\n\
             propagate --> UI\n\
             Signals --> transform[\"transform crate\"] --> FIR --> codegen[\"codegen crate\"]\n",
    )
}

/// Renders the same curated IR overview as DOT for Graphviz/SVG output.
fn render_ir_overview_dot() -> String {
    String::from(
        "digraph ir_overview {\n\
             rankdir=LR;\n\
             node [shape=box, style=\"rounded\"];\n\
             \"boxes::BoxBuilder\" -> \"BoxId / TreeArena\" -> \"match_box\";\n\
             \"ui::UiProgram\" -> \"UI groups/controls/metadata\";\n\
             \"signals::SigBuilder\" -> \"SigId / TreeArena\" -> \"match_sig\";\n\
             \"signals::ad_rules\" -> \"signals::SigBuilder\";\n\
             \"fir::FirStore\" -> \"FirModule\" -> \"fir::checker\";\n\
             \"FirModule\" -> \"fir::inliner\";\n\
             \"boxes\" -> \"eval\" -> \"propagate\";\n\
             \"propagate\" -> \"signals\";\n\
             \"propagate\" -> \"ui\";\n\
             \"signals\" -> \"transform\" -> \"fir\" -> \"codegen\";\n\
         }\n",
    )
}

/// Renders a Markdown index of public source items for every workspace package.
///
/// The scan is intentionally syntactic and conservative. It is useful for
/// navigation and broad API inventory, but it does not expand macros, resolve
/// visibility through modules, or replace rustdoc.
fn render_public_api_index(
    packages: &[&CargoPackage],
) -> Result<String, Box<dyn std::error::Error>> {
    let mut out = String::from(
        "# Public API Index\n\n\
         Generated by `cargo run -p xtask -- code-graphs`.\n\n\
         This is a lightweight source scan for public items. Use `cargo doc \
         --workspace --no-deps` for authoritative Rust API documentation.\n",
    );

    for package in packages {
        let items = public_items_for_package(package)?;
        let _ = writeln!(out, "\n## `{}`\n", package.name);
        if items.is_empty() {
            out.push_str("_No direct public items found by the source scan._\n");
            continue;
        }
        out.push_str("| Kind | Name | Location |\n|---|---|---|\n");
        for item in items {
            let rel = workspace_relative_path(&item.path);
            let _ = writeln!(
                out,
                "| `{}` | `{}` | `{rel}:{}` |",
                item.kind, item.name, item.line
            );
        }
    }

    Ok(out)
}

/// Collects public source items for a single package by scanning library-like
/// targets.
///
/// FFI crates in this workspace use target kinds such as `rlib`, `staticlib`,
/// and `cdylib`, so all library-like target kinds are included.
fn public_items_for_package(
    package: &CargoPackage,
) -> Result<Vec<PublicItem>, Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    for target in &package.targets {
        if target.kind.iter().any(|kind| {
            matches!(
                kind.as_str(),
                "lib" | "rlib" | "dylib" | "staticlib" | "cdylib"
            )
        }) {
            collect_rs_files(
                target
                    .src_path
                    .parent()
                    .ok_or("library target has no source parent")?,
                &mut files,
            )?;
        }
    }
    files.sort();

    let mut items = Vec::new();
    for file in files {
        let text = fs::read_to_string(&file)?;
        for (idx, line) in text.lines().enumerate() {
            if let Some((kind, name)) = parse_public_item_line(line) {
                items.push(PublicItem {
                    kind,
                    name,
                    path: file.clone(),
                    line: idx + 1,
                });
            }
        }
    }
    items.sort_by(|left, right| {
        workspace_relative_path(&left.path)
            .cmp(&workspace_relative_path(&right.path))
            .then(left.line.cmp(&right.line))
    });
    Ok(items)
}

/// Recursively collects Rust source files below `dir`.
fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), io::Error> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

/// Parses one source line as a top-level public item declaration.
///
/// The parser deliberately ignores `pub(crate)`, `pub(super)`, and `pub(self)`
/// items because the generated index is meant to approximate externally visible
/// API surfaces. Multi-line signatures are represented by the first line only.
fn parse_public_item_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || !trimmed.starts_with("pub ") {
        return None;
    }
    let rest = trimmed.strip_prefix("pub ")?.trim_start();
    if rest.starts_with("crate")
        || rest.starts_with("(crate)")
        || rest.starts_with("(super)")
        || rest.starts_with("(self)")
        || rest.starts_with("super")
        || rest.starts_with("self")
    {
        return None;
    }

    let rest = rest
        .strip_prefix("unsafe ")
        .unwrap_or(rest)
        .strip_prefix("extern ")
        .unwrap_or(rest)
        .strip_prefix("async ")
        .unwrap_or(rest);
    let rest = rest.strip_prefix("\"C\" ").unwrap_or(rest);

    for kind in [
        "struct", "enum", "trait", "fn", "type", "const", "static", "mod", "use",
    ] {
        if let Some(after_kind) = rest
            .strip_prefix(kind)
            .and_then(|tail| tail.strip_prefix(' '))
        {
            return Some((kind.to_owned(), parse_item_name(after_kind)));
        }
    }
    None
}

/// Extracts the first identifier/path-like token after a recognized item kind.
fn parse_item_name(input: &str) -> String {
    let name: String = input
        .trim_start()
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == ':')
        .collect();
    if name.is_empty() {
        "_".to_owned()
    } else {
        name.trim_end_matches(':').to_owned()
    }
}

/// Renders the generated `docs/code-graphs/README.md` entry point.
fn render_code_graphs_readme() -> String {
    String::from(
        "# Code Graphs\n\n\
         Generated by:\n\n\
         ```bash\n\
         cargo run -p xtask -- code-graphs\n\
         ```\n\n\
         Files:\n\n\
         - `workspace-crates.mmd` / `workspace-crates.dot` / `workspace-crates.svg`: workspace crate nodes from `cargo metadata`.\n\
         - `internal-crate-deps.mmd` / `internal-crate-deps.dot` / `internal-crate-deps.svg`: internal crate dependency edges from `cargo metadata`.\n\
         - `ir-overview.mmd` / `ir-overview.dot` / `ir-overview.svg`: curated overview of the main `boxes`, `signals`, `fir`, and `ui` IR relationships.\n\
         - `public-api-index.md`: lightweight source-scan index of public items. Use Rustdoc as the authoritative API reference.\n\
         \n\
         ## Rendered SVG\n\n\
         ### Workspace Crates\n\n\
         ![Workspace crates](workspace-crates.svg)\n\n\
         ### Internal Crate Dependencies\n\n\
         ![Internal crate dependencies](internal-crate-deps.svg)\n\n\
         ### IR Overview\n\n\
         ![IR overview](ir-overview.svg)\n",
    )
}

/// Writes a UTF-8 text artifact.
fn write_text(path: &Path, text: &str) -> Result<(), io::Error> {
    fs::write(path, text)
}

/// Converts one DOT file to SVG with Graphviz `dot`.
///
/// The generated SVG is checked in so users can inspect graphs without local
/// Mermaid or Graphviz tooling. Regeneration still requires `dot`.
fn render_svg_with_dot(dot_path: &Path, svg_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("dot")
        .arg("-Tsvg")
        .arg(dot_path)
        .arg("-o")
        .arg(svg_path)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "failed to render {} with Graphviz `dot`",
            workspace_relative_path(dot_path)
        )
        .into())
    }
}

/// Converts a crate name into a Mermaid-safe node identifier.
fn graph_id(name: &str) -> String {
    let mut out = String::from("crate_");
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    out
}

/// Escapes a label for Mermaid quoted-node syntax.
fn mermaid_label(label: &str) -> String {
    label.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Escapes a label for DOT quoted strings.
fn dot_escape(label: &str) -> String {
    label.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Enumerates all compile corpus `.dsp` files in deterministic order.
fn corpus_files() -> Result<Vec<PathBuf>, io::Error> {
    let root = workspace_root();
    let corpus_dir = root.join("tests/corpus");
    let mut files = Vec::new();

    for entry in fs::read_dir(corpus_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "dsp") {
            files.push(path);
        }
    }

    files.sort();
    Ok(files)
}

/// Enumerates all runtime corpus `.dsp` files in deterministic order.
fn runtime_corpus_files() -> Result<Vec<PathBuf>, io::Error> {
    let root = workspace_root();
    let dir = root.join("tests/runtime_corpus");
    let mut files = Vec::new();
    if !dir.exists() {
        return Ok(files);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "dsp") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// Returns the root directory for persisted runtime trace snapshots.
fn runtime_trace_snapshot_root() -> PathBuf {
    workspace_root().join("tests/runtime_traces").join("rust")
}

// ---------------------------------------------------------------------------
// `build-faustwasm-compiler-module`
// ---------------------------------------------------------------------------

/// Parsed options for the `build-faustwasm-compiler-module` workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FaustwasmCompilerModuleOptions {
    /// `true` builds the release artifact; `false` builds the debug artifact.
    release: bool,
}

impl Default for FaustwasmCompilerModuleOptions {
    fn default() -> Self {
        Self { release: true }
    }
}

/// Parses flags for the `build-faustwasm-compiler-module` workflow.
fn parse_faustwasm_compiler_module_options(
    args: impl Iterator<Item = String>,
) -> Result<FaustwasmCompilerModuleOptions, Box<dyn std::error::Error>> {
    let mut options = FaustwasmCompilerModuleOptions::default();
    for arg in args {
        match arg.as_str() {
            "--debug" => options.release = false,
            other => {
                return Err(format!(
                    "usage: cargo run -p xtask -- build-faustwasm-compiler-module [--debug]\nunknown option: {other}"
                )
                .into());
            }
        }
    }
    Ok(options)
}

/// Builds the raw Rust compiler module consumed by the future embedded
/// `faustwasm` path and verifies that its exported ABI matches the documented
/// `wasm-ffi` contract.
fn build_faustwasm_compiler_module(
    args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_faustwasm_compiler_module_options(args)?;
    let root = workspace_root();
    let profile = if options.release { "release" } else { "debug" };

    let mut cargo = Command::new("cargo");
    cargo
        .current_dir(&root)
        .arg("build")
        .arg("-p")
        .arg("wasm-ffi")
        .arg("--target")
        .arg("wasm32-unknown-unknown");
    if options.release {
        cargo.arg("--release");
    }

    let status = cargo.status()?;
    if !status.success() {
        return Err(
            "failed to build `wasm-ffi` for `wasm32-unknown-unknown`; ensure the target is installed (for example with `rustup target add wasm32-unknown-unknown`) and try again"
                .into(),
        );
    }

    let module_path = root
        .join("target")
        .join("wasm32-unknown-unknown")
        .join(profile)
        .join("faust_wasm_ffi.wasm");
    let bytes = fs::read(&module_path)?;
    verify_wasm_ffi_exports(&bytes)?;
    println!(
        "faustwasm compiler module ready: {}",
        workspace_relative_path(&module_path)
    );
    Ok(())
}

/// Lists the minimum raw export surface expected by the `faustwasm`
/// embedded-compiler adapter.
///
/// The verifier checks a freshly built `.wasm` module against this list so ABI
/// regressions are caught in the same workflow that produces the artifact.
fn required_wasm_ffi_exports() -> &'static [&'static str] {
    &[
        "memory",
        "faust_wasm_alloc",
        "faust_wasm_dealloc",
        "faust_wasm_version_ptr",
        "faust_wasm_version_len",
        "faust_wasm_compile_dsp",
        "faust_wasm_result_is_ok",
        "faust_wasm_result_wasm_ptr",
        "faust_wasm_result_wasm_len",
        "faust_wasm_result_json_ptr",
        "faust_wasm_result_json_len",
        "faust_wasm_result_compile_options_ptr",
        "faust_wasm_result_compile_options_len",
        "faust_wasm_result_error_ptr",
        "faust_wasm_result_error_len",
        "faust_wasm_result_free",
        "faust_wasm_get_info",
        "faust_wasm_expand_dsp",
        "faust_wasm_generate_aux_files",
        "faust_wasm_generate_aux_files_json",
        "faust_wasm_text_result_is_ok",
        "faust_wasm_text_result_ptr",
        "faust_wasm_text_result_len",
        "faust_wasm_text_result_free",
    ]
}

/// Verifies that a compiled `wasm-ffi` module exports the documented raw ABI.
fn verify_wasm_ffi_exports(bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let mut exported_functions = BTreeSet::new();
    let mut has_memory_export = false;

    for payload in Parser::new(0).parse_all(bytes) {
        match payload? {
            Payload::ExportSection(section) => {
                for export in section {
                    let export = export?;
                    match export.kind {
                        ExternalKind::Memory if export.name == "memory" => {
                            has_memory_export = true;
                        }
                        ExternalKind::Func => {
                            exported_functions.insert(export.name.to_owned());
                        }
                        _ => {}
                    }
                }
            }
            Payload::End(_) => break,
            _ => {}
        }
    }

    let mut missing = Vec::new();
    if !has_memory_export {
        missing.push("memory".to_owned());
    }
    for export in required_wasm_ffi_exports() {
        if *export != "memory" && !exported_functions.contains(*export) {
            missing.push((*export).to_owned());
        }
    }

    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "`wasm-ffi` module is missing required exports: {}",
            missing.join(", ")
        )
        .into())
    }
}

// ---------------------------------------------------------------------------
// Backend alignment orchestration
// ---------------------------------------------------------------------------

/// Default runtime cases for the CI-friendly backend alignment smoke workflow.
const BACKEND_ALIGN_SMOKE_DEFAULT_CASES: &[&str] = &[
    "tests/runtime_corpus/trace_01_passthrough.dsp",
    "tests/runtime_corpus/trace_07_nonlinear_clip.dsp",
    "tests/runtime_corpus/trace_38_sine_phasor.dsp",
];

/// Default FIR dump cases for the CI-friendly backend alignment smoke workflow.
const BACKEND_ALIGN_SMOKE_FIR_CASES: &[&str] = &[
    "tests/corpus/rep_01_passthrough.dsp",
    "tests/corpus/rep_07_nonlinear_clip.dsp",
    "tests/corpus/rep_38_sine_phasor.dsp",
];

/// Returns the stable case identifier derived from a corpus path.
fn case_name(path: &Path) -> Result<String, io::Error> {
    path.file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid corpus filename"))
}

#[derive(Debug, Default)]
/// Parsed options for the CI-friendly backend alignment smoke workflow.
struct BackendAlignSmokeOptions {
    /// Explicit runtime corpus cases selected with repeated `--case`.
    cases: Vec<PathBuf>,
    /// Whether FIR type diagnostics should make runtime traces fail early.
    strict_fir_types: bool,
    /// Skip the golden snapshot check phase.
    skip_golden: bool,
    /// Skip the structural FIR dump scan phase.
    skip_fir_dump_scan: bool,
}

/// Runs the reduced backend-alignment smoke workflow used in CI.
fn backend_align_smoke(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_backend_align_smoke_options(&mut args)?;
    println!("backend-align-smoke: start");

    if !options.skip_golden {
        println!("backend-align-smoke: golden-check");
        golden_check(None)?;
    } else {
        println!("backend-align-smoke: skip golden-check");
    }

    let cases = backend_align_smoke_cases(&options)?;
    if cases.is_empty() {
        return Err("backend-align-smoke: no runtime cases selected".into());
    }

    println!("backend-align-smoke: cranelift-subset-strict-check");
    cranelift_subset_strict_check_cases(&cases)?;
    println!("backend-align-smoke: cranelift-ffi-runtime-diff-smoke");
    run_cranelift_ffi_runtime_diff_smoke()?;
    println!("backend-align-smoke: interp-opt-level-diff");
    interp_trace_diff_opt_levels_cases(&cases, options.strict_fir_types)?;

    for case in &cases {
        let mut trace_check_args = vec![
            "--case".to_owned(),
            case.display().to_string(),
            "--lane".to_owned(),
            "fast".to_owned(),
        ];
        if options.strict_fir_types {
            trace_check_args.push("--strict-fir-types".to_owned());
        }
        println!("backend-align-smoke: interp-trace-check {}", case.display());
        interp_trace_check(trace_check_args.into_iter())?;
    }

    if !options.skip_fir_dump_scan {
        let mut scan_args: Vec<String> = Vec::new();
        for case in backend_align_smoke_fir_cases()? {
            scan_args.push("--case".to_owned());
            scan_args.push(case.display().to_string());
        }
        scan_args.push("--lane".to_owned());
        scan_args.push("fast".to_owned());
        println!("backend-align-smoke: fir-dump-scan (fast lane corpus subset)");
        fir_dump_scan(scan_args.into_iter())?;
    } else {
        println!("backend-align-smoke: skip fir-dump-scan");
    }

    println!(
        "backend-align-smoke: OK (runtime_cases={}, strict_fir_types={}, golden={}, cranelift_strict_subset=true, interp_opt_levels=true, fir_dump_scan={})",
        cases.len(),
        options.strict_fir_types,
        !options.skip_golden,
        !options.skip_fir_dump_scan
    );
    Ok(())
}

/// Parses CLI flags for `backend-align-smoke`.
fn parse_backend_align_smoke_options(
    args: &mut impl Iterator<Item = String>,
) -> Result<BackendAlignSmokeOptions, Box<dyn std::error::Error>> {
    let mut options = BackendAlignSmokeOptions::default();
    let iter = args.by_ref();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--case" => {
                let Some(path) = iter.next() else {
                    return Err("--case requires a path".into());
                };
                options.cases.push(PathBuf::from(path));
            }
            "--strict-fir-types" => options.strict_fir_types = true,
            "--skip-golden" => options.skip_golden = true,
            "--skip-fir-dump-scan" => options.skip_fir_dump_scan = true,
            "--help" | "-h" => {
                return Err("usage: cargo run -p xtask -- backend-align-smoke [--case <tests/runtime_corpus/foo.dsp> ...] [--strict-fir-types] [--skip-golden] [--skip-fir-dump-scan]".into());
            }
            other => return Err(format!("unknown backend-align-smoke option: {other}").into()),
        }
    }
    Ok(options)
}

/// Resolves the runtime corpus subset used by `backend-align-smoke`.
///
/// When explicit `--case` flags are present they win; otherwise the baked-in
/// smoke subset is materialized under the workspace root and existence-checked.
fn backend_align_smoke_cases(
    options: &BackendAlignSmokeOptions,
) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    if !options.cases.is_empty() {
        return Ok(options.cases.clone());
    }
    let root = workspace_root();
    let mut cases = Vec::new();
    for rel in BACKEND_ALIGN_SMOKE_DEFAULT_CASES {
        let path = root.join(rel);
        if !path.exists() {
            return Err(format!(
                "backend-align-smoke default case missing: {}",
                path.display()
            )
            .into());
        }
        cases.push(path);
    }
    Ok(cases)
}

/// Resolves the FIR corpus subset scanned by `backend-align-smoke`.
///
/// This list stays separate from the runtime-trace subset because it targets
/// `dump_fir` structural coverage rather than runtime execution coverage.
fn backend_align_smoke_fir_cases() -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let root = workspace_root();
    let mut cases = Vec::new();
    for rel in BACKEND_ALIGN_SMOKE_FIR_CASES {
        let path = root.join(rel);
        if !path.exists() {
            return Err(format!(
                "backend-align-smoke default FIR case missing: {}",
                path.display()
            )
            .into());
        }
        cases.push(path);
    }
    Ok(cases)
}

/// Verifies that each case lowers through the strict Cranelift subset path.
///
/// This intentionally enables `fail_on_subset_gap` so the nightly/smoke flows
/// catch matcher/lowerer drift instead of silently falling back.
fn cranelift_subset_strict_check_cases(
    cases: &[PathBuf],
) -> Result<(), Box<dyn std::error::Error>> {
    let compiler = compiler::Compiler::new();
    for case in cases {
        let fir = compiler
            .compile_file_default_to_fir_with_lane(case, compiler::SignalFirLane::TransformFastLane)
            .map_err(|e| {
                format!(
                    "Cranelift strict subset FIR compile failed for {}: {e}",
                    case.display()
                )
            })?;
        let options = codegen::backends::cranelift::CraneliftOptions {
            fail_on_subset_gap: true,
            ..codegen::backends::cranelift::CraneliftOptions::default()
        };
        codegen::backends::cranelift::generate_cranelift_module(&fir.store, fir.module, &options)
            .map_err(|e| {
            format!(
                "Cranelift strict subset check failed for {}: {e}",
                case.display()
            )
        })?;
    }
    println!(
        "cranelift-subset-strict-check: {} case(s) compiled without fallback",
        cases.len()
    );
    Ok(())
}

/// Runs the standalone `cranelift-ffi` smoke tests used by backend alignment.
fn run_cranelift_ffi_runtime_diff_smoke() -> Result<(), Box<dyn std::error::Error>> {
    const TESTS: [&str; 2] = [
        "cranelift_interp_runtime_diff_smoke_corpus",
        "cranelift_ui_meta_callback_smoke_path",
    ];
    for test_name in TESTS {
        let status = Command::new("cargo")
            .arg("test")
            .arg("-p")
            .arg("cranelift-ffi")
            .arg(test_name)
            .arg("--")
            .arg("--nocapture")
            .status()?;
        if !status.success() {
            return Err(format!("cranelift-ffi smoke test failed: {test_name}").into());
        }
    }
    Ok(())
}

/// Parsed options for the broader nightly backend-alignment workflow.
#[derive(Debug, Default)]
struct BackendAlignNightlyOptions {
    /// Whether FIR type diagnostics should make runtime traces fail early.
    strict_fir_types: bool,
    /// Skip the golden snapshot check phase.
    skip_golden: bool,
    /// Skip the structural FIR dump scan phase.
    skip_fir_dump_scan: bool,
}

/// Runs the broader nightly backend-alignment workflow.
fn backend_align_nightly(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_backend_align_nightly_options(&mut args)?;
    println!("backend-align-nightly: start");

    if !options.skip_golden {
        println!("backend-align-nightly: golden-check");
        golden_check(None)?;
    } else {
        println!("backend-align-nightly: skip golden-check");
    }

    let nightly_cases = runtime_corpus_files()?;
    println!("backend-align-nightly: cranelift-subset-strict-check (all runtime cases)");
    cranelift_subset_strict_check_cases(&nightly_cases)?;
    println!("backend-align-nightly: cranelift-ffi-runtime-diff-smoke");
    run_cranelift_ffi_runtime_diff_smoke()?;

    let mut trace_check_args = vec!["--lane".to_owned(), "fast".to_owned()];
    if options.strict_fir_types {
        trace_check_args.push("--strict-fir-types".to_owned());
    }
    println!("backend-align-nightly: interp-trace-check (all runtime cases, fast lane)");
    interp_trace_check(trace_check_args.into_iter())?;

    if !options.skip_fir_dump_scan {
        println!("backend-align-nightly: fir-dump-scan (all corpus cases, fast lane)");
        fir_dump_scan(["--lane".to_owned(), "fast".to_owned()].into_iter())?;
    } else {
        println!("backend-align-nightly: skip fir-dump-scan");
    }

    println!(
        "backend-align-nightly: OK (strict_fir_types={}, golden={}, cranelift_strict_subset=true, fir_dump_scan={})",
        options.strict_fir_types, !options.skip_golden, !options.skip_fir_dump_scan
    );
    Ok(())
}

/// Parses CLI flags for `backend-align-nightly`.
fn parse_backend_align_nightly_options(
    args: &mut impl Iterator<Item = String>,
) -> Result<BackendAlignNightlyOptions, Box<dyn std::error::Error>> {
    let mut options = BackendAlignNightlyOptions::default();
    for arg in args.by_ref() {
        match arg.as_str() {
            "--strict-fir-types" => options.strict_fir_types = true,
            "--skip-golden" => options.skip_golden = true,
            "--skip-fir-dump-scan" => options.skip_fir_dump_scan = true,
            "--help" | "-h" => {
                return Err("usage: cargo run -p xtask -- backend-align-nightly [--strict-fir-types] [--skip-golden] [--skip-fir-dump-scan]".into());
            }
            other => return Err(format!("unknown backend-align-nightly option: {other}").into()),
        }
    }
    Ok(options)
}

// ---------------------------------------------------------------------------
// `fir-dump-scan`
// ---------------------------------------------------------------------------

/// Parsed options for `fir-dump-scan`.
#[derive(Debug)]
struct FirDumpScanOptions {
    /// Explicit compile corpus cases selected with repeated `--case`.
    cases: Vec<PathBuf>,
    /// Signal-to-FIR lane used before rendering `dump_fir`.
    lane: TraceLane,
}

impl Default for FirDumpScanOptions {
    /// Returns the default `fir-dump-scan` settings.
    ///
    /// The fast lane is the default because it is the active parity target for
    /// the structural loop-expansion checks in this workflow.
    fn default() -> Self {
        Self {
            cases: Vec::new(),
            lane: TraceLane::Fast,
        }
    }
}

/// Scans `dump_fir` output for unexpanded loop-body placeholders.
fn fir_dump_scan(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_fir_dump_scan_options(&mut args)?;
    let cases = if options.cases.is_empty() {
        corpus_files()?
    } else {
        options.cases
    };
    let compiler = compiler::Compiler::new();

    let mut compiled_cases = 0usize;
    let mut skipped_compile = 0usize;
    let mut loop_nodes_seen = 0usize;
    let mut issues: Vec<String> = Vec::new();

    for case in cases {
        let lowered = match compiler
            .compile_file_default_to_fir_with_lane(&case, options.lane.to_signal_fir_lane())
        {
            Ok(out) => out,
            Err(e) => {
                skipped_compile += 1;
                println!("skip {} (FIR compile failed: {e})", case.display());
                continue;
            }
        };
        let rendered = dump_fir(&lowered.store, lowered.module);
        compiled_cases += 1;
        loop_nodes_seen += count_loop_nodes_in_dump(&rendered);

        let missing = find_unexpanded_loop_bodies(&rendered);
        if missing.is_empty() {
            println!("ok {} [lane={}]", case.display(), options.lane.as_str());
            continue;
        }

        for (loop_kind, loop_id, body_id) in missing {
            issues.push(format!(
                "{} [lane={}] {loop_kind} node #{loop_id} body #{body_id} not expanded in dump_fir output",
                case.display(),
                options.lane.as_str()
            ));
        }
    }

    if !issues.is_empty() {
        for issue in &issues {
            println!("[FAIL] {issue}");
        }
        return Err(format!(
            "fir-dump-scan failed: {} issue(s) across {} compiled case(s) (skipped_compile={})",
            issues.len(),
            compiled_cases,
            skipped_compile
        )
        .into());
    }

    println!(
        "fir-dump-scan: OK (lane={}, compiled_cases={}, skipped_compile={}, loop_nodes_seen={})",
        options.lane.as_str(),
        compiled_cases,
        skipped_compile,
        loop_nodes_seen
    );
    Ok(())
}

/// Parses `fir-dump-scan` command-line flags into [`FirDumpScanOptions`].
fn parse_fir_dump_scan_options(
    args: &mut impl Iterator<Item = String>,
) -> Result<FirDumpScanOptions, Box<dyn std::error::Error>> {
    let mut options = FirDumpScanOptions::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--case" => {
                let Some(path) = args.next() else {
                    return Err("--case requires a path".into());
                };
                options.cases.push(PathBuf::from(path));
            }
            "--lane" => {
                let Some(value) = args.next() else {
                    return Err("--lane requires fast".into());
                };
                options.lane = TraceLane::parse(&value)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
            }
            "--help" | "-h" => {
                return Err("usage: cargo run -p xtask -- fir-dump-scan [--case <tests/corpus/foo.dsp> ...] [--lane fast]".into());
            }
            other => return Err(format!("unknown fir-dump-scan option: {other}").into()),
        }
    }
    Ok(options)
}

/// Counts loop statements rendered by `dump_fir`.
///
/// The scanner uses string matching instead of reparsing FIR because the goal
/// is specifically to validate the textual dumper's body expansion behavior.
fn count_loop_nodes_in_dump(rendered: &str) -> usize {
    rendered.matches("SimpleForLoop {").count()
        + rendered.matches("ForLoop {").count()
        + rendered.matches("IteratorForLoop {").count()
}

/// Finds loop entries whose referenced body ids never appear as expanded nodes.
fn find_unexpanded_loop_bodies(rendered: &str) -> Vec<(&'static str, u32, u32)> {
    let mut issues = Vec::new();
    for line in rendered.lines() {
        let Some((loop_kind, loop_id, body_id)) = parse_loop_line_body_ids(line) else {
            continue;
        };
        let body_marker = format!("#{body_id} ");
        if !rendered.contains(&body_marker) {
            issues.push((loop_kind, loop_id, body_id));
        }
    }
    issues
}

/// Parses one `dump_fir` line to extract `(loop_kind, loop_id, body_id)`.
///
/// Returns `None` for non-loop lines or if the textual shape does not match the
/// current dumper contract.
fn parse_loop_line_body_ids(line: &str) -> Option<(&'static str, u32, u32)> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix('#')?;
    let loop_id_end = rest.find(' ')?;
    let loop_id = rest[..loop_id_end].parse().ok()?;
    let rest = &rest[loop_id_end + 1..];

    let loop_kind = if rest.starts_with("SimpleForLoop {") {
        "SimpleForLoop"
    } else if rest.starts_with("ForLoop {") {
        "ForLoop"
    } else if rest.starts_with("IteratorForLoop {") {
        "IteratorForLoop"
    } else {
        return None;
    };

    let body_key = "body: TreeId(";
    let body_pos = rest.find(body_key)?;
    let body_tail = &rest[body_pos + body_key.len()..];
    let body_end = body_tail.find(')')?;
    let body_id = body_tail[..body_end].parse().ok()?;
    Some((loop_kind, loop_id, body_id))
}

// ---------------------------------------------------------------------------
// Golden snapshot workflows
// ---------------------------------------------------------------------------

/// Enumerates the corpus/golden pairs checked by `golden-check`.
///
/// Rust references enumerate directly from `tests/corpus`, while C++ references
/// enumerate the snapshot directories so missing snapshots are reported as
/// absent corpus sources instead of silently skipped.
fn golden_cases_for_check(golden_ref: GoldenRef) -> Result<Vec<(String, PathBuf)>, io::Error> {
    let root = workspace_root();
    match golden_ref {
        GoldenRef::Rust => {
            let mut cases = Vec::new();
            for file in corpus_files()? {
                cases.push((case_name(&file)?, file));
            }
            Ok(cases)
        }
        GoldenRef::Cpp => {
            let golden_root = root.join("tests/golden").join(golden_ref.as_dir_name());
            let mut cases = Vec::new();
            for entry in fs::read_dir(golden_root)? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let case = entry
                    .file_name()
                    .to_str()
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid golden case directory name",
                        )
                    })?;
                let expected = entry.path().join("compiler_stdout.txt");
                if expected.is_file() {
                    let source = root.join("tests/corpus").join(format!("{case}.dsp"));
                    cases.push((case, source));
                }
            }
            cases.sort_by(|a, b| a.0.cmp(&b.0));
            Ok(cases)
        }
    }
}

/// Golden reference family used by snapshot workflows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GoldenRef {
    /// Rust-generated reference snapshots under `tests/golden/rust`.
    Rust,
    /// C++ reference snapshots under `tests/golden/cpp`.
    Cpp,
}

impl GoldenRef {
    /// Returns the snapshot subdirectory name for this reference family.
    fn as_dir_name(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Cpp => "cpp",
        }
    }
}

/// Returns the on-disk golden snapshot path for one case/reference family.
fn golden_file_for_ref(case: &str, golden_ref: GoldenRef) -> PathBuf {
    workspace_root()
        .join("tests/golden")
        .join(golden_ref.as_dir_name())
        .join(case)
        .join("compiler_stdout.txt")
}

/// Normalizes generated text before snapshot comparison.
fn normalize(text: &str) -> String {
    let mut normalized = text.replace("\r\n", "\n");
    let mut lines: Vec<String> = normalized
        .lines()
        .map(|line| line.trim_end().to_string())
        .collect();

    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }

    normalized = lines.join("\n");
    normalized.push('\n');
    normalized
}

// ---------------------------------------------------------------------------
// Runtime trace workflows
// ---------------------------------------------------------------------------

/// Input scenario used by runtime-trace generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TraceScenario {
    /// All input channels receive zero.
    Zeros,
    /// The first sample of each input channel is one, followed by zeroes.
    Impulse,
    /// Input channels receive a deterministic increasing ramp.
    Ramp,
    /// Input channels receive a deterministic sine wave.
    Sine,
}

impl TraceScenario {
    /// Returns the stable CLI / snapshot string for this scenario.
    fn as_str(self) -> &'static str {
        match self {
            Self::Zeros => "zeros",
            Self::Impulse => "impulse",
            Self::Ramp => "ramp",
            Self::Sine => "sine",
        }
    }

    /// Parses a CLI/runtime-trace scenario name.
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "zeros" => Ok(Self::Zeros),
            "impulse" => Ok(Self::Impulse),
            "ramp" => Ok(Self::Ramp),
            "sine" => Ok(Self::Sine),
            _ => Err(format!(
                "unknown scenario '{s}' (expected: zeros|impulse|ramp|sine)"
            )),
        }
    }
}

/// Interpreter lane used by runtime-trace workflows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TraceLane {
    /// Active transform fast lane.
    Fast,
}

impl TraceLane {
    /// Returns the stable textual label used in logs and snapshots.
    fn as_str(self) -> &'static str {
        match self {
            Self::Fast => "fast-lane",
        }
    }

    /// Parses the accepted CLI aliases for one interpreter lane.
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "fast" | "fast-lane" | "transform" => Ok(Self::Fast),
            _ => Err(format!("unknown lane '{s}' (expected: fast)")),
        }
    }

    /// Maps the CLI/runtime-trace lane to the compiler's signal-to-FIR lane.
    fn to_signal_fir_lane(self) -> compiler::SignalFirLane {
        match self {
            Self::Fast => compiler::SignalFirLane::TransformFastLane,
        }
    }
}

/// Parsed options for `interp-trace-dump`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct InterpTraceDumpOptions {
    /// DSP source file to compile and execute.
    case: PathBuf,
    /// Deterministic input pattern.
    scenario: TraceScenario,
    /// Lowering lane used before interpreter bytecode generation.
    lane: TraceLane,
    /// Runtime sample rate.
    sample_rate: usize,
    /// Number of frames per compute block.
    block_size: usize,
    /// Number of compute blocks to execute.
    num_blocks: usize,
    /// Whether FIR type diagnostics should reject the trace.
    strict_fir_types: bool,
    /// Optional JSON output path. When absent, JSON is printed to stdout.
    out: Option<PathBuf>,
}

/// Parsed options for `interp-trace-dump-cppfbc`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct InterpTraceCppFbcDumpOptions {
    /// Shared trace execution options.
    trace: InterpTraceDumpOptions,
    /// Optional C++ Faust executable override.
    faust_bin: Option<PathBuf>,
}

/// Parsed options for batch generation from C++ `.fbc` files.
#[derive(Clone, Debug, PartialEq, Eq)]
struct InterpTraceCppFbcBatchOptions {
    /// Optional single compile corpus case; absent means all default corpus
    /// cases.
    case: Option<PathBuf>,
    /// Deterministic input pattern.
    scenario: TraceScenario,
    /// Runtime sample rate.
    sample_rate: usize,
    /// Number of frames per compute block.
    block_size: usize,
    /// Number of compute blocks to execute.
    num_blocks: usize,
    /// Output root for persisted JSON traces.
    out_dir: PathBuf,
    /// Optional C++ Faust executable override.
    faust_bin: Option<PathBuf>,
}

impl Default for InterpTraceCppFbcBatchOptions {
    /// Returns default batch settings for generating C++ `.fbc` traces.
    fn default() -> Self {
        Self {
            case: None,
            scenario: TraceScenario::Impulse,
            sample_rate: 48_000,
            block_size: 64,
            num_blocks: 1,
            out_dir: workspace_root().join("tests/runtime_traces").join("cppfbc"),
            faust_bin: None,
        }
    }
}

impl Default for InterpTraceDumpOptions {
    /// Returns default options for one interpreter trace run.
    fn default() -> Self {
        Self {
            case: PathBuf::new(),
            scenario: TraceScenario::Zeros,
            lane: TraceLane::Fast,
            sample_rate: 48_000,
            block_size: 64,
            num_blocks: 4,
            strict_fir_types: false,
            out: None,
        }
    }
}

/// Persisted runtime trace payload used by snapshot workflows.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct RuntimeTrace {
    /// Repository-relative DSP source path.
    dsp_path: String,
    /// Signal-to-FIR lane label.
    lane: String,
    /// Input scenario label.
    scenario: String,
    /// Runtime sample rate.
    sample_rate: usize,
    /// Number of frames per compute block.
    block_size: usize,
    /// Number of compute blocks executed.
    num_blocks: usize,
    /// Number of DSP input channels.
    num_inputs: usize,
    /// Number of DSP output channels.
    num_outputs: usize,
    /// Output samples by channel.
    outputs: Vec<Vec<f32>>,
}

/// Numeric tolerances used when comparing runtime traces.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TraceCompareTolerances {
    /// Absolute tolerance.
    abs_tol: f32,
    /// Relative tolerance.
    rel_tol: f32,
}

impl Default for TraceCompareTolerances {
    /// Returns the default absolute/relative float tolerances for trace diffing.
    fn default() -> Self {
        Self {
            abs_tol: 1.0e-6,
            rel_tol: 1.0e-5,
        }
    }
}

/// One concrete runtime-trace mismatch entry.
#[derive(Clone, Debug, PartialEq)]
struct TraceMismatch {
    /// Field or payload area that mismatched.
    field: String,
    /// Optional output channel index for sample mismatches.
    channel: Option<usize>,
    /// Optional sample index for sample mismatches.
    sample: Option<usize>,
    /// Expected float value for sample mismatches.
    expected: Option<f32>,
    /// Actual float value for sample mismatches.
    actual: Option<f32>,
}

/// Shared batch options for runtime-trace generation/checking flows.
#[derive(Clone, Debug, PartialEq, Eq)]
struct InterpTraceBatchOptions {
    /// Optional single runtime corpus case; absent means all runtime corpus
    /// cases.
    case: Option<PathBuf>,
    /// Lowering lane used before interpreter bytecode generation.
    lane: TraceLane,
    /// Runtime sample rate.
    sample_rate: usize,
    /// Number of frames per compute block.
    block_size: usize,
    /// Number of compute blocks to execute.
    num_blocks: usize,
    /// Whether FIR type diagnostics should reject traces.
    strict_fir_types: bool,
}

impl Default for InterpTraceBatchOptions {
    /// Returns default options for runtime-trace batch generation/checking.
    fn default() -> Self {
        Self {
            case: None,
            lane: TraceLane::Fast,
            sample_rate: 48_000,
            block_size: 64,
            num_blocks: 4,
            strict_fir_types: false,
        }
    }
}

/// Executes one Rust interpreter trace run and writes/prints the JSON payload.
fn interp_trace_dump(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_interp_trace_dump_options(&mut args)?;
    let trace = run_interp_trace_case(&options)?;
    let json = render_runtime_trace_json(&trace);
    if let Some(path) = &options.out {
        fs::write(path, json)?;
    } else {
        print!("{json}");
    }
    Ok(())
}

/// Executes one C++ `.fbc`-backed trace run and writes/prints the JSON payload.
fn interp_trace_dump_cppfbc(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_interp_trace_dump_cppfbc_options(&mut args)?;
    let trace = run_interp_trace_case_from_cpp_fbc(&options)?;
    let json = render_runtime_trace_json(&trace);
    if let Some(path) = &options.trace.out {
        fs::write(path, json)?;
    } else {
        print!("{json}");
    }
    Ok(())
}

/// Generates C++ `.fbc` trace snapshots for one case or the default corpus.
fn interp_trace_gen_cppfbc(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_interp_trace_gen_cppfbc_options(&mut args)?;
    let mut cases = if let Some(case) = &options.case {
        vec![case.clone()]
    } else {
        corpus_files()?
            .into_iter()
            .filter(|p| {
                p.file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|n| n.starts_with("rep_"))
            })
            .collect()
    };
    cases.sort();
    if cases.is_empty() {
        return Err("no corpus cases found for interp-trace-gen-cppfbc".into());
    }

    let mut generated = 0usize;
    for case in cases {
        let case_id = case_name(&case)?;
        let trace = run_interp_trace_case_from_cpp_fbc(&InterpTraceCppFbcDumpOptions {
            trace: InterpTraceDumpOptions {
                case: case.clone(),
                scenario: options.scenario,
                lane: TraceLane::Fast,
                sample_rate: options.sample_rate,
                block_size: options.block_size,
                num_blocks: options.num_blocks,
                strict_fir_types: false,
                out: None,
            },
            faust_bin: options.faust_bin.clone(),
        })?;
        let case_dir = options.out_dir.join(&case_id);
        fs::create_dir_all(&case_dir)?;
        let path = case_dir.join(format!("{}.json", options.scenario.as_str()));
        fs::write(&path, render_runtime_trace_json(&trace))?;
        println!("generated {}", path.display());
        generated += 1;
    }
    println!(
        "interp-trace-gen-cppfbc: generated {generated} trace snapshot(s) in {}",
        options.out_dir.display()
    );
    Ok(())
}

/// Parses CLI options for `interp-trace-dump`.
fn parse_interp_trace_dump_options(
    args: &mut impl Iterator<Item = String>,
) -> Result<InterpTraceDumpOptions, Box<dyn std::error::Error>> {
    let mut options = InterpTraceDumpOptions::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--case" => {
                let Some(path) = args.next() else {
                    return Err("missing value after --case".into());
                };
                options.case = PathBuf::from(path);
            }
            "--scenario" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --scenario".into());
                };
                options.scenario = TraceScenario::parse(&value)
                    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            }
            "--lane" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --lane".into());
                };
                options.lane = TraceLane::parse(&value)
                    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            }
            "--sample-rate" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --sample-rate".into());
                };
                options.sample_rate = value.parse::<usize>()?;
            }
            "--block-size" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --block-size".into());
                };
                options.block_size = value.parse::<usize>()?;
            }
            "--num-blocks" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --num-blocks".into());
                };
                options.num_blocks = value.parse::<usize>()?;
            }
            "--strict-fir-types" => {
                options.strict_fir_types = true;
            }
            "--out" => {
                let Some(path) = args.next() else {
                    return Err("missing value after --out".into());
                };
                options.out = Some(PathBuf::from(path));
            }
            "--help" | "-h" => {
                return Err("usage: cargo run -p xtask -- interp-trace-dump --case <path> [--scenario zeros|impulse|ramp|sine] [--lane fast] [--sample-rate N] [--block-size N] [--num-blocks N] [--strict-fir-types] [--out path]".into());
            }
            other => {
                return Err(format!("unknown interp-trace-dump option: {other}").into());
            }
        }
    }

    if options.case.as_os_str().is_empty() {
        return Err("interp-trace-dump requires --case <path>".into());
    }
    if options.block_size == 0 || options.num_blocks == 0 {
        return Err("block-size and num-blocks must be > 0".into());
    }
    Ok(options)
}

/// Parses CLI options for `interp-trace-dump-cppfbc`.
///
/// The lane is fixed to the C++ `.fbc` runtime path, so flags that would alter
/// FIR-lane semantics are rejected here instead of ignored.
fn parse_interp_trace_dump_cppfbc_options(
    args: &mut impl Iterator<Item = String>,
) -> Result<InterpTraceCppFbcDumpOptions, Box<dyn std::error::Error>> {
    let mut options = InterpTraceCppFbcDumpOptions {
        trace: InterpTraceDumpOptions::default(),
        faust_bin: None,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--case" => {
                let Some(path) = args.next() else {
                    return Err("missing value after --case".into());
                };
                options.trace.case = PathBuf::from(path);
            }
            "--scenario" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --scenario".into());
                };
                options.trace.scenario = TraceScenario::parse(&value)
                    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            }
            "--faust-bin" => {
                let Some(path) = args.next() else {
                    return Err("missing value after --faust-bin".into());
                };
                options.faust_bin = Some(PathBuf::from(path));
            }
            "--sample-rate" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --sample-rate".into());
                };
                options.trace.sample_rate = value.parse::<usize>()?;
            }
            "--block-size" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --block-size".into());
                };
                options.trace.block_size = value.parse::<usize>()?;
            }
            "--num-blocks" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --num-blocks".into());
                };
                options.trace.num_blocks = value.parse::<usize>()?;
            }
            "--out" => {
                let Some(path) = args.next() else {
                    return Err("missing value after --out".into());
                };
                options.trace.out = Some(PathBuf::from(path));
            }
            "--lane" => {
                return Err(
                    "--lane is not supported for interp-trace-dump-cppfbc (source is C++ .fbc)"
                        .into(),
                );
            }
            "--strict-fir-types" => {
                return Err(
                    "--strict-fir-types is not applicable to interp-trace-dump-cppfbc".into(),
                );
            }
            "--help" | "-h" => {
                return Err("usage: cargo run -p xtask -- interp-trace-dump-cppfbc --case <path> [--scenario zeros|impulse|ramp|sine] [--faust-bin /path/to/faust] [--sample-rate N] [--block-size N] [--num-blocks N] [--out path]".into());
            }
            other => {
                return Err(format!("unknown interp-trace-dump-cppfbc option: {other}").into());
            }
        }
    }
    if options.trace.case.as_os_str().is_empty() {
        return Err("interp-trace-dump-cppfbc requires --case <path>".into());
    }
    if options.trace.block_size == 0 || options.trace.num_blocks == 0 {
        return Err("block-size and num-blocks must be > 0".into());
    }
    options.trace.lane = TraceLane::Fast;
    Ok(options)
}

/// Parses CLI options for `interp-trace-gen-cppfbc`.
fn parse_interp_trace_gen_cppfbc_options(
    args: &mut impl Iterator<Item = String>,
) -> Result<InterpTraceCppFbcBatchOptions, Box<dyn std::error::Error>> {
    let mut options = InterpTraceCppFbcBatchOptions::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--case" => {
                let Some(path) = args.next() else {
                    return Err("missing value after --case".into());
                };
                options.case = Some(PathBuf::from(path));
            }
            "--scenario" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --scenario".into());
                };
                options.scenario = TraceScenario::parse(&value)
                    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            }
            "--faust-bin" => {
                let Some(path) = args.next() else {
                    return Err("missing value after --faust-bin".into());
                };
                options.faust_bin = Some(PathBuf::from(path));
            }
            "--sample-rate" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --sample-rate".into());
                };
                options.sample_rate = value.parse::<usize>()?;
            }
            "--block-size" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --block-size".into());
                };
                options.block_size = value.parse::<usize>()?;
            }
            "--num-blocks" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --num-blocks".into());
                };
                options.num_blocks = value.parse::<usize>()?;
            }
            "--out-dir" => {
                let Some(path) = args.next() else {
                    return Err("missing value after --out-dir".into());
                };
                options.out_dir = PathBuf::from(path);
            }
            "--help" | "-h" => {
                return Err("usage: cargo run -p xtask -- interp-trace-gen-cppfbc [--case <path>] [--scenario zeros|impulse|ramp|sine] [--faust-bin /path/to/faust] [--sample-rate N] [--block-size N] [--num-blocks N] [--out-dir <dir>]".into());
            }
            other => return Err(format!("unknown interp-trace-gen-cppfbc option: {other}").into()),
        }
    }
    if options.block_size == 0 || options.num_blocks == 0 {
        return Err("block-size and num-blocks must be > 0".into());
    }
    Ok(options)
}

/// Generates Rust runtime-trace snapshots for the selected runtime corpus cases.
fn interp_trace_gen(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_interp_trace_batch_options(&mut args)?;
    let cases = runtime_trace_cases(&options)?;
    let mut generated = 0usize;
    for case in cases {
        let case_id = case_name(&case)?;
        fs::create_dir_all(runtime_trace_snapshot_root().join(&case_id))?;
        let scenarios = trace_scenarios_for_runtime_case(&case)?;
        if scenarios.is_empty() {
            println!(
                "skip {} (no snapshot-enabled scenarios yet)",
                case.display()
            );
            continue;
        }
        for scenario in scenarios {
            let trace = run_interp_trace_case(&InterpTraceDumpOptions {
                case: case.clone(),
                scenario,
                lane: options.lane,
                sample_rate: options.sample_rate,
                block_size: options.block_size,
                num_blocks: options.num_blocks,
                strict_fir_types: options.strict_fir_types,
                out: None,
            })?;
            let path = runtime_trace_snapshot_path(&case_id, scenario);
            fs::write(&path, render_runtime_trace_json(&trace))?;
            println!("generated {}", path.display());
            generated += 1;
        }
    }
    println!("interp-trace-gen: generated {generated} trace snapshot(s)");
    Ok(())
}

/// Recomputes Rust runtime traces and compares them against checked-in snapshots.
fn interp_trace_check(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_interp_trace_batch_options(&mut args)?;
    let tol = TraceCompareTolerances::default();
    let cases = runtime_trace_cases(&options)?;
    let mut checked = 0usize;
    for case in cases {
        let case_id = case_name(&case)?;
        let scenarios = trace_scenarios_for_runtime_case(&case)?;
        if scenarios.is_empty() {
            println!(
                "skip {} (no snapshot-enabled scenarios yet)",
                case.display()
            );
            continue;
        }
        for scenario in scenarios {
            let expected_path = runtime_trace_snapshot_path(&case_id, scenario);
            let expected_text = fs::read_to_string(&expected_path).map_err(|err| {
                io::Error::new(
                    err.kind(),
                    format!(
                        "missing runtime trace snapshot {}: {err} (run interp-trace-gen)",
                        expected_path.display()
                    ),
                )
            })?;
            let expected = parse_runtime_trace_json(&expected_text)?;
            let trace = run_interp_trace_case(&InterpTraceDumpOptions {
                case: case.clone(),
                scenario,
                lane: options.lane,
                sample_rate: options.sample_rate,
                block_size: options.block_size,
                num_blocks: options.num_blocks,
                strict_fir_types: options.strict_fir_types,
                out: None,
            })?;
            let actual = render_runtime_trace_json(&trace);
            let actual_parsed = parse_runtime_trace_json(&actual)?;
            if let Err(mismatch) = compare_runtime_traces(&expected, &actual_parsed, tol) {
                return Err(format!(
                    "interp-trace-check failed for {} [{}]: mismatch {:?} ({})",
                    case.display(),
                    scenario.as_str(),
                    mismatch,
                    expected_path.display()
                )
                .into());
            }
            println!("ok {} [{}]", case.display(), scenario.as_str());
            checked += 1;
        }
    }
    println!("interp-trace-check: {checked} trace snapshot(s) matched");
    Ok(())
}

/// Compares `opt_level=0` and `opt_level=max` interpreter traces on selected cases.
///
/// This is a low-cost metamorphic guardrail: the bytecode optimizer may change
/// execution strategy but must not change observable sample outputs.
fn interp_trace_diff_opt_levels_cases(
    cases: &[PathBuf],
    strict_fir_types: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let tol = TraceCompareTolerances::default();
    let default_options = InterpTraceBatchOptions::default();
    let mut compared = 0usize;

    for case in cases {
        let scenarios = trace_scenarios_for_runtime_case(case)?;
        if scenarios.is_empty() {
            println!(
                "skip {} (no snapshot-enabled scenarios yet)",
                case.display()
            );
            continue;
        }

        for scenario in scenarios {
            let base = InterpTraceDumpOptions {
                case: case.clone(),
                scenario,
                lane: TraceLane::Fast,
                sample_rate: default_options.sample_rate,
                block_size: default_options.block_size,
                num_blocks: default_options.num_blocks,
                strict_fir_types,
                out: None,
            };
            let unoptimized = run_interp_trace_case_with_opt_level(&base, 0)?;
            let optimized = run_interp_trace_case_with_opt_level(
                &base,
                codegen::backends::interp::MAX_OPT_LEVEL.into(),
            )?;
            if let Err(mismatch) = compare_runtime_traces(&unoptimized, &optimized, tol) {
                return Err(format!(
                    "interp opt-level diff failed for {} [{}]: mismatch {:?}",
                    case.display(),
                    scenario.as_str(),
                    mismatch
                )
                .into());
            }
            println!(
                "match {} [{}] (interp opt_level=0 vs opt_level=max)",
                case.display(),
                scenario.as_str()
            );
            compared += 1;
        }
    }

    println!("interp opt-level diff: {compared} trace(s) matched");
    Ok(())
}

/// Parses shared batch options for `interp-trace-gen` and `interp-trace-check`.
fn parse_interp_trace_batch_options(
    args: &mut impl Iterator<Item = String>,
) -> Result<InterpTraceBatchOptions, Box<dyn std::error::Error>> {
    let mut options = InterpTraceBatchOptions::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--case" => {
                let Some(path) = args.next() else {
                    return Err("missing value after --case".into());
                };
                options.case = Some(PathBuf::from(path));
            }
            "--lane" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --lane".into());
                };
                options.lane = TraceLane::parse(&value)
                    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            }
            "--sample-rate" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --sample-rate".into());
                };
                options.sample_rate = value.parse::<usize>()?;
            }
            "--block-size" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --block-size".into());
                };
                options.block_size = value.parse::<usize>()?;
            }
            "--num-blocks" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --num-blocks".into());
                };
                options.num_blocks = value.parse::<usize>()?;
            }
            "--strict-fir-types" => {
                options.strict_fir_types = true;
            }
            "--help" | "-h" => {
                return Err("usage: cargo run -p xtask -- interp-trace-gen [--case <path>] [--lane fast] [--sample-rate N] [--block-size N] [--num-blocks N] [--strict-fir-types]".into());
            }
            other => return Err(format!("unknown interp-trace batch option: {other}").into()),
        }
    }
    if options.block_size == 0 || options.num_blocks == 0 {
        return Err("block-size and num-blocks must be > 0".into());
    }
    Ok(options)
}

/// Resolves the case list for a batch runtime-trace workflow.
fn runtime_trace_cases(
    options: &InterpTraceBatchOptions,
) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    if let Some(case) = &options.case {
        return Ok(vec![case.clone()]);
    }
    let cases = runtime_corpus_files()?;
    if cases.is_empty() {
        return Err("no runtime trace corpus files found in tests/runtime_corpus".into());
    }
    Ok(cases)
}

/// Returns the enabled runtime-trace scenarios for one runtime corpus case.
///
/// The mapping is intentionally explicit so newly added runtime cases must opt
/// in with scenario choices instead of silently inheriting an arbitrary default.
fn trace_scenarios_for_runtime_case(
    case: &Path,
) -> Result<Vec<TraceScenario>, Box<dyn std::error::Error>> {
    let name = case_name(case)?;
    let scenarios = match name.as_str() {
        "trace_01_passthrough" => vec![TraceScenario::Impulse, TraceScenario::Ramp],
        "trace_02_gain_bias_typed" => vec![],
        "trace_03_stereo_mix" => vec![],
        "trace_07_nonlinear_clip" => vec![],
        "trace_09_ui_slider" => vec![TraceScenario::Impulse],
        "trace_22_parallel_mix" => vec![],
        "trace_31_extended_primitives_typed" => vec![TraceScenario::Zeros],
        "trace_38_sine_phasor" => vec![],
        other => {
            return Err(format!(
                "no runtime trace scenario mapping defined for {other} (update xtask)"
            )
            .into());
        }
    };
    Ok(scenarios)
}

/// Returns the checked-in snapshot path for one runtime trace case/scenario.
fn runtime_trace_snapshot_path(case_id: &str, scenario: TraceScenario) -> PathBuf {
    runtime_trace_snapshot_root()
        .join(case_id)
        .join(format!("{}.json", scenario.as_str()))
}

/// Runs one DSP through the Rust interpreter backend and captures the outputs.
fn run_interp_trace_case(
    options: &InterpTraceDumpOptions,
) -> Result<RuntimeTrace, Box<dyn std::error::Error>> {
    run_interp_trace_case_with_opt_level(options, 0)
}

/// Runs one DSP through the Rust interpreter backend with an explicit optimizer level.
fn run_interp_trace_case_with_opt_level(
    options: &InterpTraceDumpOptions,
    opt_level: i32,
) -> Result<RuntimeTrace, Box<dyn std::error::Error>> {
    let compiler = compiler::Compiler::new().with_fir_verify_options(compiler::FirVerifyOptions {
        enabled: true,
        strict: false,
    });

    let signals = compiler.compile_file_default_to_signals(&options.case)?;
    let fir = compiler
        .compile_file_default_to_fir_with_lane(&options.case, options.lane.to_signal_fir_lane())?;
    if options.strict_fir_types {
        enforce_strict_fir_type_diagnostics(&fir.store, fir.module, &options.case)?;
    }

    let interp_options = codegen::backends::interp::InterpOptions {
        opt_level,
        module_name: None,
    };
    let mut factory = codegen::backends::interp::generate_interp_module::<f32>(
        &fir.store,
        fir.module,
        &interp_options,
    )?;
    let mut instance = codegen::backends::interp::FbcDspInstance::new(&mut factory);
    instance.init(options.sample_rate as i32);

    let total_samples = options.block_size * options.num_blocks;
    let input_channels = generate_trace_inputs(
        options.scenario,
        signals.process_arity.inputs,
        total_samples,
        options.sample_rate,
    );
    let mut output_channels = vec![vec![0.0f32; total_samples]; signals.process_arity.outputs];

    for block_idx in 0..options.num_blocks {
        let start = block_idx * options.block_size;
        let end = start + options.block_size;
        let input_refs: Vec<&[f32]> = input_channels.iter().map(|ch| &ch[start..end]).collect();
        let mut output_refs: Vec<&mut [f32]> = output_channels
            .iter_mut()
            .map(|ch| &mut ch[start..end])
            .collect();
        instance
            .try_compute(options.block_size as i32, &input_refs, &mut output_refs)
            .map_err(|e| {
                format!(
                    "interp runtime execution failed in compute block (block_idx={}): {e}",
                    block_idx
                )
            })?;
    }

    Ok(RuntimeTrace {
        dsp_path: workspace_relative_path(&options.case),
        lane: options.lane.as_str().to_string(),
        scenario: options.scenario.as_str().to_string(),
        sample_rate: options.sample_rate,
        block_size: options.block_size,
        num_blocks: options.num_blocks,
        num_inputs: signals.process_arity.inputs,
        num_outputs: signals.process_arity.outputs,
        outputs: output_channels,
    })
}

/// Resolves the Faust C++ compiler binary used to generate `.fbc` fixtures.
fn resolve_faust_cpp_bin(explicit: Option<&Path>) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    if let Some(path) = std::env::var_os("FAUST_CPP_BIN") {
        return Ok(PathBuf::from(path));
    }
    Ok(PathBuf::from("faust"))
}

/// Invokes the Faust C++ compiler to produce an interpreter `.fbc` file.
fn compile_dsp_to_cpp_fbc(
    faust_bin: &Path,
    dsp_case: &Path,
    fbc_out: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::new(faust_bin);
    cmd.arg("-lang").arg("interp");
    for inc in default_import_search_paths(dsp_case) {
        cmd.arg("-I").arg(inc);
    }
    cmd.arg(dsp_case);
    cmd.arg("-o").arg(fbc_out);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    let output = cmd.output().map_err(|e| {
        io::Error::new(
            e.kind(),
            format!(
                "failed to spawn Faust C++ binary {}: {e}",
                faust_bin.display()
            ),
        )
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "faust -lang interp failed for {} with status {}\nstdout:\n{}\nstderr:\n{}",
            dsp_case.display(),
            output.status,
            stdout.trim(),
            stderr.trim()
        )
        .into());
    }
    if !fbc_out.is_file() {
        return Err(format!(
            "faust reported success but did not produce .fbc output: {}",
            fbc_out.display()
        )
        .into());
    }
    Ok(())
}

/// Runs one trace case by first compiling the DSP through the C++ `.fbc` path.
fn run_interp_trace_case_from_cpp_fbc(
    options: &InterpTraceCppFbcDumpOptions,
) -> Result<RuntimeTrace, Box<dyn std::error::Error>> {
    let faust_bin = resolve_faust_cpp_bin(options.faust_bin.as_deref())?;
    let case_id = case_name(&options.trace.case)?;
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let fbc_path = std::env::temp_dir().join(format!("faust_rs_xtask_{case_id}_{pid}_{nanos}.fbc"));
    compile_dsp_to_cpp_fbc(&faust_bin, &options.trace.case, &fbc_path)?;

    let trace_result = (|| -> Result<RuntimeTrace, Box<dyn std::error::Error>> {
        let file = fs::File::open(&fbc_path)?;
        let mut reader = io::BufReader::new(file);
        let mut factory: codegen::backends::interp::FbcDspFactory<f32> =
            codegen::backends::interp::read_fbc(&mut reader).map_err(|e| {
                format!(
                    "failed to read C++ generated .fbc {}: {e}",
                    fbc_path.display()
                )
            })?;
        let num_inputs = factory.num_inputs.max(0) as usize;
        let num_outputs = factory.num_outputs.max(0) as usize;

        let mut instance = codegen::backends::interp::FbcDspInstance::new(&mut factory);
        instance.init(options.trace.sample_rate as i32);

        let total_samples = options.trace.block_size * options.trace.num_blocks;
        let input_channels = generate_trace_inputs(
            options.trace.scenario,
            num_inputs,
            total_samples,
            options.trace.sample_rate,
        );
        let mut output_channels = vec![vec![0.0f32; total_samples]; num_outputs];
        for block_idx in 0..options.trace.num_blocks {
            let start = block_idx * options.trace.block_size;
            let end = start + options.trace.block_size;
            let input_refs: Vec<&[f32]> = input_channels.iter().map(|ch| &ch[start..end]).collect();
            let mut output_refs: Vec<&mut [f32]> = output_channels
                .iter_mut()
                .map(|ch| &mut ch[start..end])
                .collect();
            instance
                .try_compute(
                    options.trace.block_size as i32,
                    &input_refs,
                    &mut output_refs,
                )
                .map_err(|e| {
                    format!(
                        "Rust interp runtime failed on C++ .fbc (block_idx={}): {e}",
                        block_idx
                    )
                })?;
        }

        Ok(RuntimeTrace {
            dsp_path: workspace_relative_path(&options.trace.case),
            lane: "cpp-fbc".to_string(),
            scenario: options.trace.scenario.as_str().to_string(),
            sample_rate: options.trace.sample_rate,
            block_size: options.trace.block_size,
            num_blocks: options.trace.num_blocks,
            num_inputs,
            num_outputs,
            outputs: output_channels,
        })
    })();

    let _ = fs::remove_file(&fbc_path);
    trace_result
}

/// Rejects traces when FIR verification reported type-focused diagnostics.
///
/// The filter intentionally keeps only typing/layout families so runtime-trace
/// workflows can opt into stronger type hygiene without failing on unrelated
/// structural warnings.
fn enforce_strict_fir_type_diagnostics(
    store: &fir::FirStore,
    module: fir::FirId,
    case: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let report = fir::checker::verify_fir_module(store, module);
    let type_diags: Vec<&fir::checker::FirDiagnostic> = report
        .diagnostics
        .iter()
        .filter(|d| is_fir_type_diagnostic_code(d.code))
        .collect();
    if type_diags.is_empty() {
        return Ok(());
    }

    let mut msg = format!(
        "strict FIR type diagnostics present for {}: {} diagnostic(s)",
        case.display(),
        type_diags.len()
    );
    for d in type_diags.iter().take(4) {
        let sev = match d.severity {
            fir::checker::Severity::Error => "error",
            fir::checker::Severity::Warning => "warning",
        };
        let fn_ctx = d
            .context
            .function_name
            .as_deref()
            .map(|f| format!(" (fn={f})"))
            .unwrap_or_default();
        msg.push_str(&format!("\n- {sev} [{}] {}{}", d.code, d.message, fn_ctx));
    }
    if type_diags.len() > 4 {
        msg.push_str(&format!("\n- ... {} more", type_diags.len() - 4));
    }
    Err(msg.into())
}

/// Returns `true` when a FIR diagnostic code belongs to the strict type subset.
fn is_fir_type_diagnostic_code(code: &str) -> bool {
    code.starts_with("FIR-B")
        || code.starts_with("FIR-U")
        || code.starts_with("FIR-C")
        || code.starts_with("FIR-FC")
        || code.starts_with("FIR-T")
        || code.starts_with("FIR-MA")
        || matches!(code, "FIR-R01" | "FIR-L03" | "FIR-SW01")
}

/// Generates deterministic numeric input channels for one trace scenario.
fn generate_trace_inputs(
    scenario: TraceScenario,
    num_inputs: usize,
    total_samples: usize,
    sample_rate: usize,
) -> Vec<Vec<f32>> {
    let mut inputs = vec![vec![0.0f32; total_samples]; num_inputs];
    match scenario {
        TraceScenario::Zeros => {}
        TraceScenario::Impulse => {
            if total_samples > 0 {
                for channel in &mut inputs {
                    channel[0] = 1.0;
                }
            }
        }
        TraceScenario::Ramp => {
            if total_samples == 0 {
                return inputs;
            }
            let denom = (total_samples.saturating_sub(1)).max(1) as f32;
            for channel in &mut inputs {
                for (i, sample) in channel.iter_mut().enumerate() {
                    *sample = (i as f32) / denom;
                }
            }
        }
        TraceScenario::Sine => {
            let sr = sample_rate.max(1) as f32;
            let freq_hz = 440.0f32;
            let w = core::f32::consts::TAU * freq_hz / sr;
            for (ch_idx, channel) in inputs.iter_mut().enumerate() {
                let phase = (ch_idx as f32) * 0.25 * core::f32::consts::TAU;
                for (i, sample) in channel.iter_mut().enumerate() {
                    *sample = (w * (i as f32) + phase).sin();
                }
            }
        }
    }
    inputs
}

/// Renders a checked-in runtime-trace JSON payload.
///
/// The structure is kept stable and explicit instead of deriving `Serialize`
/// directly from [`RuntimeTrace`] so snapshot formatting stays deterministic.
fn render_runtime_trace_json(trace: &RuntimeTrace) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{{");
    let _ = writeln!(out, "  \"schema_version\": 1,");
    let _ = writeln!(out, "  \"dsp\": \"{}\",", json_escape(&trace.dsp_path));
    let _ = writeln!(out, "  \"backend\": \"interp\",");
    let _ = writeln!(out, "  \"pipeline\": {{");
    let _ = writeln!(out, "    \"signal_fir_lane\": \"{}\"", trace.lane);
    let _ = writeln!(out, "  }},");
    let _ = writeln!(out, "  \"runtime\": {{");
    let _ = writeln!(out, "    \"sample_rate\": {},", trace.sample_rate);
    let _ = writeln!(out, "    \"block_size\": {},", trace.block_size);
    let _ = writeln!(out, "    \"num_blocks\": {}", trace.num_blocks);
    let _ = writeln!(out, "  }},");
    let _ = writeln!(out, "  \"scenario\": {{");
    let _ = writeln!(out, "    \"name\": \"{}\",", trace.scenario);
    let _ = writeln!(out, "    \"inputs\": {},", trace.num_inputs);
    let _ = writeln!(out, "    \"outputs\": {}", trace.num_outputs);
    let _ = writeln!(out, "  }},");
    let _ = writeln!(out, "  \"outputs\": [");
    for (ch_idx, channel) in trace.outputs.iter().enumerate() {
        let _ = write!(out, "    [");
        for (i, sample) in channel.iter().enumerate() {
            if i > 0 {
                let _ = write!(out, ", ");
            }
            let _ = write!(out, "{:.9}", sample);
        }
        let _ = writeln!(
            out,
            "]{}",
            if ch_idx + 1 == trace.outputs.len() {
                ""
            } else {
                ","
            }
        );
    }
    let _ = writeln!(out, "  ]");
    let _ = writeln!(out, "}}");
    out
}

/// Escapes a string for inclusion in the hand-written runtime-trace JSON output.
fn json_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// Serde-facing schema used to parse persisted runtime-trace snapshots.
#[derive(Debug, Deserialize)]
struct RuntimeTraceJson {
    dsp: String,
    pipeline: RuntimeTracePipelineJson,
    runtime: RuntimeTraceRuntimeJson,
    scenario: RuntimeTraceScenarioJson,
    outputs: Vec<Vec<f32>>,
}

/// Nested `pipeline` section of a persisted runtime-trace snapshot.
#[derive(Debug, Deserialize)]
struct RuntimeTracePipelineJson {
    signal_fir_lane: String,
}

/// Nested `runtime` section of a persisted runtime-trace snapshot.
#[derive(Debug, Deserialize)]
struct RuntimeTraceRuntimeJson {
    sample_rate: usize,
    block_size: usize,
    num_blocks: usize,
}

/// Nested `scenario` section of a persisted runtime-trace snapshot.
#[derive(Debug, Deserialize)]
struct RuntimeTraceScenarioJson {
    name: String,
    inputs: usize,
    outputs: usize,
}

/// Parses one runtime-trace snapshot JSON payload into [`RuntimeTrace`].
fn parse_runtime_trace_json(text: &str) -> Result<RuntimeTrace, Box<dyn std::error::Error>> {
    let parsed: RuntimeTraceJson = serde_json::from_str(text)?;
    Ok(RuntimeTrace {
        dsp_path: parsed.dsp,
        lane: parsed.pipeline.signal_fir_lane,
        scenario: parsed.scenario.name,
        sample_rate: parsed.runtime.sample_rate,
        block_size: parsed.runtime.block_size,
        num_blocks: parsed.runtime.num_blocks,
        num_inputs: parsed.scenario.inputs,
        num_outputs: parsed.scenario.outputs,
        outputs: parsed.outputs,
    })
}

/// Compares two runtime traces field-by-field with float tolerances on samples.
fn compare_runtime_traces(
    expected: &RuntimeTrace,
    actual: &RuntimeTrace,
    tol: TraceCompareTolerances,
) -> Result<(), TraceMismatch> {
    if expected.dsp_path != actual.dsp_path {
        return Err(TraceMismatch {
            field: "dsp".into(),
            channel: None,
            sample: None,
            expected: None,
            actual: None,
        });
    }
    if expected.lane != actual.lane {
        return Err(TraceMismatch {
            field: "pipeline.signal_fir_lane".into(),
            channel: None,
            sample: None,
            expected: None,
            actual: None,
        });
    }
    if expected.scenario != actual.scenario {
        return Err(TraceMismatch {
            field: "scenario.name".into(),
            channel: None,
            sample: None,
            expected: None,
            actual: None,
        });
    }
    if expected.sample_rate != actual.sample_rate {
        return Err(TraceMismatch {
            field: "runtime.sample_rate".into(),
            channel: None,
            sample: None,
            expected: None,
            actual: None,
        });
    }
    if expected.block_size != actual.block_size {
        return Err(TraceMismatch {
            field: "runtime.block_size".into(),
            channel: None,
            sample: None,
            expected: None,
            actual: None,
        });
    }
    if expected.num_blocks != actual.num_blocks {
        return Err(TraceMismatch {
            field: "runtime.num_blocks".into(),
            channel: None,
            sample: None,
            expected: None,
            actual: None,
        });
    }
    if expected.num_inputs != actual.num_inputs {
        return Err(TraceMismatch {
            field: "scenario.inputs".into(),
            channel: None,
            sample: None,
            expected: None,
            actual: None,
        });
    }
    if expected.num_outputs != actual.num_outputs {
        return Err(TraceMismatch {
            field: "scenario.outputs".into(),
            channel: None,
            sample: None,
            expected: None,
            actual: None,
        });
    }
    if expected.outputs.len() != actual.outputs.len() {
        return Err(TraceMismatch {
            field: "outputs.channel_count".into(),
            channel: None,
            sample: None,
            expected: None,
            actual: None,
        });
    }
    for (ch_idx, (exp_ch, act_ch)) in expected.outputs.iter().zip(&actual.outputs).enumerate() {
        if exp_ch.len() != act_ch.len() {
            return Err(TraceMismatch {
                field: "outputs.sample_count".into(),
                channel: Some(ch_idx),
                sample: None,
                expected: None,
                actual: None,
            });
        }
        for (i, (&e, &a)) in exp_ch.iter().zip(act_ch.iter()).enumerate() {
            if !trace_sample_equal(e, a, tol) {
                return Err(TraceMismatch {
                    field: "outputs".into(),
                    channel: Some(ch_idx),
                    sample: Some(i),
                    expected: Some(e),
                    actual: Some(a),
                });
            }
        }
    }
    Ok(())
}

/// Compares two floating-point samples using mixed absolute/relative tolerance.
fn trace_sample_equal(expected: f32, actual: f32, tol: TraceCompareTolerances) -> bool {
    if expected.is_nan() || actual.is_nan() {
        return expected.is_nan() && actual.is_nan();
    }
    if expected.is_infinite() || actual.is_infinite() {
        return expected == actual;
    }
    let diff = (expected - actual).abs();
    let scale = expected.abs().max(actual.abs());
    diff <= tol.abs_tol + tol.rel_tol * scale
}

/// Renders the Rust golden snapshot text for one corpus input.
fn render_rust_snapshot(input: &Path) -> Result<String, io::Error> {
    let source = fs::read_to_string(input)?;
    let name = input
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid input filename"))?;
    Ok(compiler::golden_snapshot(name, &source))
}

/// Returns the default import search paths used for corpus/golden compilation.
fn default_import_search_paths(input: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(parent) = input.parent() {
        paths.push(parent.to_path_buf());
    }
    {
        let path = PathBuf::from("/usr/local/share/faust");
        if path.is_dir() {
            paths.push(path);
        }
    }
    paths
}

/// Compiles one DSP through the Rust C++ backend and returns the rendered source.
fn render_rust_cpp_output(input: &Path) -> Result<String, compiler::CompilerError> {
    let compiler = compiler::Compiler::new();
    let options = codegen::backends::cpp::CppOptions::default();
    let search_paths = default_import_search_paths(input);
    compiler.compile_file_to_cpp(input, &search_paths, &options)
}

/// Regenerates all Rust golden snapshots from `tests/corpus`.
fn golden_gen_rust() -> Result<(), Box<dyn std::error::Error>> {
    let files = corpus_files()?;
    for file in files {
        let case = case_name(&file)?;
        let output = golden_file_for_ref(&case, GoldenRef::Rust);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let snapshot = normalize(&render_rust_snapshot(&file)?);
        fs::write(&output, snapshot)?;
        println!("updated {}", output.display());
    }
    Ok(())
}

/// Regenerates C++ golden snapshots using the external Faust reference binary.
fn golden_gen_cpp(extra_args: &[OsString]) -> Result<(), Box<dyn std::error::Error>> {
    let cpp_bin = std::env::var_os("FAUST_CPP_BIN").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "FAUST_CPP_BIN is not set. Example: FAUST_CPP_BIN=/path/to/faust",
        )
    })?;

    let files = corpus_files()?;
    for file in files {
        let case = case_name(&file)?;
        let output = golden_file_for_ref(&case, GoldenRef::Cpp);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut cmd = Command::new(&cpp_bin);
        cmd.arg(&file);
        for arg in extra_args {
            cmd.arg(arg);
        }

        let result = cmd.output()?;
        if !result.status.success() {
            return Err(format!(
                "C++ reference command failed for {} with status {}",
                file.display(),
                result.status
            )
            .into());
        }

        let stdout = String::from_utf8(result.stdout)?;
        fs::write(&output, normalize(&stdout))?;
        println!("updated {}", output.display());
    }

    Ok(())
}

/// Resolves the active golden reference family from `GOLDEN_REF` or defaults.
fn golden_ref_from_env() -> Result<GoldenRef, Box<dyn std::error::Error>> {
    let Some(raw) = std::env::var_os("GOLDEN_REF") else {
        return Ok(GoldenRef::Rust);
    };
    let value = raw
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid GOLDEN_REF value"))?;
    match value {
        "rust" => Ok(GoldenRef::Rust),
        "cpp" => Ok(GoldenRef::Cpp),
        _ => Err(format!("invalid GOLDEN_REF={value}; expected rust or cpp").into()),
    }
}

/// Validates generated snapshots against the selected Rust or C++ golden family.
fn golden_check(forced: Option<GoldenRef>) -> Result<(), Box<dyn std::error::Error>> {
    let golden_ref = match forced {
        Some(value) => value,
        None => golden_ref_from_env()?,
    };

    let files = golden_cases_for_check(golden_ref)?;
    if files.is_empty() {
        return Err(format!(
            "no golden cases found for reference `{}`",
            golden_ref.as_dir_name()
        )
        .into());
    }
    let mut failures = 0usize;

    for (case, file) in files {
        if !file.exists() {
            return Err(format!(
                "missing corpus file for golden case `{case}`: {}",
                file.display()
            )
            .into());
        }
        let expected_path = golden_file_for_ref(&case, golden_ref);
        let expected = fs::read_to_string(&expected_path).map_err(|err| {
            io::Error::new(
                err.kind(),
                format!(
                    "missing golden file {} (run golden-gen-rust or golden-gen-cpp): {err}",
                    expected_path.display()
                ),
            )
        })?;

        let actual = match golden_ref {
            GoldenRef::Rust => normalize(&render_rust_snapshot(&file)?),
            GoldenRef::Cpp => match render_rust_cpp_output(&file) {
                Ok(output) => normalize(&output),
                Err(error) => format!("__RUST_CPP_ERROR__\n{error}\n"),
            },
        };
        let expected = normalize(&expected);

        if actual != expected {
            failures += 1;
            println!("[FAIL] {case}");
            println!("  expected: {}", expected_path.display());
            println!("  first diff:");
            print_first_diff(&expected, &actual);
        } else {
            println!("[OK] {case}");
        }
    }

    if failures > 0 {
        return Err(format!("golden-check failed: {failures} case(s) differ").into());
    }

    Ok(())
}

/// Prints the first differing line between two normalized snapshot texts.
fn print_first_diff(expected: &str, actual: &str) {
    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();
    let max = expected_lines.len().max(actual_lines.len());

    for idx in 0..max {
        let e = expected_lines.get(idx).copied().unwrap_or("<missing>");
        let a = actual_lines.get(idx).copied().unwrap_or("<missing>");
        if e != a {
            println!("    line {}", idx + 1);
            println!("      expected: {e}");
            println!("      actual:   {a}");
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Differential report generation
// ---------------------------------------------------------------------------

/// Regenerates the parser/lexer parity coverage report under `porting/`.
fn parser_parity_report() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();
    let cpp_root = PathBuf::from(CPP_SOURCE_ROOT);

    let cpp_parser = cpp_root.join("compiler/parser/faustparser.y");
    let cpp_lexer = cpp_root.join("compiler/parser/faustlexer.l");
    let rust_parser = root.join("crates/parser/src/grammar/faustparser.y");
    let rust_lexer = root.join("crates/parser/src/grammar/faustlexer.l");
    let report_path = root.join(PARITY_REPORT_REL_PATH);

    for path in [&cpp_parser, &cpp_lexer, &rust_parser, &rust_lexer] {
        if !path.exists() {
            return Err(format!("missing input file for parity report: {}", path.display()).into());
        }
    }

    let cpp_parser_src = fs::read_to_string(&cpp_parser)?;
    let cpp_lexer_src = fs::read_to_string(&cpp_lexer)?;
    let rust_parser_src = fs::read_to_string(&rust_parser)?;
    let rust_lexer_src = fs::read_to_string(&rust_lexer)?;

    let cpp_parser_tokens = extract_parser_tokens(&cpp_parser_src);
    let rust_parser_tokens = extract_parser_tokens(&rust_parser_src);
    let cpp_lexer_tokens = extract_cpp_lexer_emitted_tokens(&cpp_lexer_src);
    let rust_lexer_tokens = extract_rust_lexer_emitted_tokens(&rust_lexer_src);
    let cpp_lexer_states = extract_lexer_states(&cpp_lexer_src);
    let rust_lexer_states = extract_lexer_states(&rust_lexer_src);
    let cpp_nonterms = extract_cpp_nonterminals(&cpp_parser_src);
    let rust_nonterms = extract_rust_nonterminals(&rust_parser_src);

    let parser_token_extra = diff_sorted(&rust_parser_tokens, &cpp_parser_tokens);
    let parser_token_missing_exact = diff_sorted(&cpp_parser_tokens, &rust_parser_tokens);
    let (parser_token_alias_covered, parser_token_missing_unresolved) = partition_with_aliases(
        &parser_token_missing_exact,
        &rust_parser_tokens,
        token_aliases,
    );

    let lexer_state_extra = diff_sorted(&rust_lexer_states, &cpp_lexer_states);
    let lexer_state_missing = diff_sorted(&cpp_lexer_states, &rust_lexer_states);

    let nonterm_extra = diff_sorted(&rust_nonterms, &cpp_nonterms);
    let nonterm_missing_exact = diff_sorted(&cpp_nonterms, &rust_nonterms);
    let (nonterm_alias_covered, nonterm_missing_unresolved) =
        partition_with_aliases(&nonterm_missing_exact, &rust_nonterms, nonterminal_aliases);

    let cpp_declared_not_lexed = diff_sorted(&cpp_parser_tokens, &cpp_lexer_tokens);
    let rust_declared_not_lexed = diff_sorted(&rust_parser_tokens, &rust_lexer_tokens);
    let cpp_lexed_not_declared = diff_sorted(&cpp_lexer_tokens, &cpp_parser_tokens);
    let rust_lexed_not_declared = diff_sorted(&rust_lexer_tokens, &rust_parser_tokens);

    let mut out = String::new();
    writeln!(
        &mut out,
        "# Phase 3 Parser/Lexer Parity Coverage Report (Auto-generated)"
    )?;
    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "Generated by: `cargo run -p xtask -- parser-parity-report`"
    )?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Inputs")?;
    writeln!(&mut out, "- C++ parser: `{}`", cpp_parser.display())?;
    writeln!(&mut out, "- C++ lexer: `{}`", cpp_lexer.display())?;
    writeln!(&mut out, "- Rust parser: `{}`", rust_parser.display())?;
    writeln!(&mut out, "- Rust lexer: `{}`", rust_lexer.display())?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Summary")?;
    writeln!(
        &mut out,
        "- Parser token coverage: C++ declared `{}` / Rust declared `{}` / unresolved missing `{}`",
        cpp_parser_tokens.len(),
        rust_parser_tokens.len(),
        parser_token_missing_unresolved.len()
    )?;
    writeln!(
        &mut out,
        "- Lexer state coverage: C++ `{}` / Rust `{}` / unresolved missing `{}`",
        cpp_lexer_states.len(),
        rust_lexer_states.len(),
        lexer_state_missing.len()
    )?;
    writeln!(
        &mut out,
        "- Grammar nonterminal coverage (name-based): C++ `{}` / Rust `{}` / unresolved missing `{}`",
        cpp_nonterms.len(),
        rust_nonterms.len(),
        nonterm_missing_unresolved.len()
    )?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Parser Tokens (C++ `%token` vs Rust `%token`)")?;
    writeln!(
        &mut out,
        "_Note: `exact name` mismatches below are not necessarily missing functionality; they can be covered by explicit alias mapping._"
    )?;
    render_list(
        &mut out,
        "Exact-name mismatch candidates (C++ name not present as-is in Rust)",
        &parser_token_missing_exact,
    )?;
    render_alias_list(
        &mut out,
        "Exact-name mismatches covered by explicit alias mapping (no action required)",
        &parser_token_alias_covered,
    )?;
    render_list(
        &mut out,
        "Unresolved missing after alias mapping (action required)",
        &parser_token_missing_unresolved,
    )?;
    render_list(&mut out, "Extra in Rust", &parser_token_extra)?;

    writeln!(&mut out)?;
    writeln!(&mut out, "## Lexer States (`%x`/`%s`)")?;
    render_list(
        &mut out,
        "Missing in Rust lexer state declarations",
        &lexer_state_missing,
    )?;
    render_list(
        &mut out,
        "Extra in Rust lexer state declarations",
        &lexer_state_extra,
    )?;

    writeln!(&mut out)?;
    writeln!(&mut out, "## Grammar Nonterminals (name-based)")?;
    writeln!(
        &mut out,
        "_Note: `exact name` mismatches below are not necessarily missing functionality; they can be covered by explicit alias mapping (for example dedicated C++ rules grouped under `Primitive` in Rust)._"
    )?;
    render_list(
        &mut out,
        "Exact-name mismatch candidates (C++ nonterminal not present as-is in Rust)",
        &nonterm_missing_exact,
    )?;
    render_alias_list(
        &mut out,
        "Exact-name mismatches covered by explicit alias mapping (no action required)",
        &nonterm_alias_covered,
    )?;
    render_list(
        &mut out,
        "Unresolved missing after alias mapping (action required)",
        &nonterm_missing_unresolved,
    )?;
    render_list(&mut out, "Extra in Rust", &nonterm_extra)?;

    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "## Parser/Lexer Internal Consistency (declared tokens vs lexer emissions)"
    )?;
    render_list(
        &mut out,
        "C++ parser-declared tokens not emitted by C++ lexer",
        &cpp_declared_not_lexed,
    )?;
    render_list(
        &mut out,
        "Rust parser-declared tokens not emitted by Rust lexer",
        &rust_declared_not_lexed,
    )?;
    render_list(
        &mut out,
        "C++ lexer-emitted tokens not declared in C++ parser",
        &cpp_lexed_not_declared,
    )?;
    render_list(
        &mut out,
        "Rust lexer-emitted tokens not declared in Rust parser",
        &rust_lexed_not_declared,
    )?;

    let unresolved_total = parser_token_missing_unresolved.len()
        + lexer_state_missing.len()
        + nonterm_missing_unresolved.len();
    let consistency_issues_total = cpp_declared_not_lexed.len()
        + rust_declared_not_lexed.len()
        + cpp_lexed_not_declared.len()
        + rust_lexed_not_declared.len();

    writeln!(&mut out)?;
    writeln!(&mut out, "## Next Actions")?;
    if unresolved_total == 0 {
        writeln!(
            &mut out,
            "- Unresolved missing items after alias mapping are `0` for parser tokens, lexer states, and grammar nonterminals."
        )?;
    } else {
        writeln!(
            &mut out,
            "- Resolve all items listed in `Unresolved missing after alias mapping (action required)` for tokens and nonterminals."
        )?;
    }
    if consistency_issues_total > 0 {
        writeln!(
            &mut out,
            "- Triage items listed under `Parser/Lexer Internal Consistency` (C++ or Rust declared/emitted token mismatches)."
        )?;
    }
    writeln!(
        &mut out,
        "- Keep this report regenerated at each parser/lexer migration increment to track closure toward 100% parity."
    )?;

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&report_path, out)?;
    println!("updated {}", report_path.display());
    Ok(())
}

#[derive(Clone, Debug)]
/// Normalized success/failure summary for one compiler/reference case run.
struct CaseStatus {
    ok: bool,
    stage: &'static str,
    reason: String,
}

/// Regenerates the C++ vs Rust corpus status report for the parser/signal path.
fn corpus_status_report() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();
    let report_path = root.join(CORPUS_STATUS_REPORT_REL_PATH);
    let files = corpus_files()?;
    let compiler = compiler::Compiler::new();
    let (cpp_bin, cpp_bin_is_fallback) = resolve_cpp_faust_bin();

    let mut total = 0usize;
    let mut ok_ok = 0usize;
    let mut err_err = 0usize;
    let mut ok_err = 0usize;
    let mut err_ok = 0usize;

    let mut rows = Vec::with_capacity(files.len());
    for file in files {
        let case = case_name(&file)?;
        let cpp = cpp_case_status(&cpp_bin, &file)?;
        let rust = rust_case_status(&compiler, &file);
        total = total.saturating_add(1);

        match (cpp.ok, rust.ok) {
            (true, true) => ok_ok = ok_ok.saturating_add(1),
            (false, false) => err_err = err_err.saturating_add(1),
            (true, false) => ok_err = ok_err.saturating_add(1),
            (false, true) => err_ok = err_ok.saturating_add(1),
        }

        rows.push((case, cpp, rust));
    }

    let mut out = String::new();
    writeln!(
        &mut out,
        "# Phase 4 Corpus C++ vs Rust Status Differential Report (Auto-generated)"
    )?;
    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "Generated by: `cargo run -p xtask -- corpus-status-report`"
    )?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Inputs")?;
    writeln!(&mut out, "- Corpus: `tests/corpus/*.dsp`")?;
    writeln!(&mut out, "- C++ binary: `{}`", cpp_bin.display())?;
    if cpp_bin_is_fallback {
        writeln!(
            &mut out,
            "- Note: fallback to `faust` from PATH because `{}/build/bin/faust` was not found.",
            CPP_SOURCE_ROOT
        )?;
        writeln!(
            &mut out,
            "- Action: set `FAUST_CPP_BIN` explicitly to the source-of-truth C++ binary when available."
        )?;
    }
    writeln!(
        &mut out,
        "- Rust path: `compiler::Compiler::compile_file_default_to_signals`"
    )?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Summary")?;
    writeln!(&mut out, "- Total cases: `{total}`")?;
    writeln!(&mut out, "- `OK/OK`: `{ok_ok}`")?;
    writeln!(&mut out, "- `ERR/ERR`: `{err_err}`")?;
    writeln!(&mut out, "- `OK/ERR` (C++ ok, Rust err): `{ok_err}`")?;
    writeln!(&mut out, "- `ERR/OK` (C++ err, Rust ok): `{err_ok}`")?;
    writeln!(&mut out)?;

    writeln!(&mut out, "## Parity Mismatches")?;
    writeln!(
        &mut out,
        "| Case | Class | C++ | Rust stage | Rust reason | C++ reason |"
    )?;
    writeln!(
        &mut out,
        "|------|-------|-----|------------|-------------|------------|"
    )?;
    for (case, cpp, rust) in &rows {
        let class = match (cpp.ok, rust.ok) {
            (true, false) => "OK/ERR",
            (false, true) => "ERR/OK",
            _ => continue,
        };
        writeln!(
            &mut out,
            "| `{}` | `{}` | `{}` | `{}` | `{}` | `{}` |",
            case,
            class,
            status_cell(cpp),
            rust.stage,
            markdown_escape(&rust.reason),
            markdown_escape(&cpp.reason),
        )?;
    }
    writeln!(&mut out)?;

    writeln!(&mut out, "## Full Matrix")?;
    writeln!(&mut out, "| Case | C++ | Rust | Rust stage | Rust reason |")?;
    writeln!(&mut out, "|------|-----|------|------------|-------------|")?;
    for (case, cpp, rust) in &rows {
        writeln!(
            &mut out,
            "| `{}` | `{}` | `{}` | `{}` | `{}` |",
            case,
            status_cell(cpp),
            status_cell(rust),
            rust.stage,
            markdown_escape(&rust.reason),
        )?;
    }
    writeln!(&mut out)?;
    writeln!(&mut out, "## Next Actions")?;
    writeln!(
        &mut out,
        "- Treat all `OK/ERR` and `ERR/OK` rows as parity tasks in parser/eval/propagate."
    )?;
    writeln!(
        &mut out,
        "- Re-run this report after each parity fix touching `tests/corpus` behavior."
    )?;

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&report_path, out)?;
    println!("updated {}", report_path.display());
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Reduced shell signature extracted from generated C++ backend text.
///
/// The report uses this instead of full-text diffing so parity checks stay
/// stable while inner statement lowering is still evolving.
struct ShellSignature {
    faustclass: Option<String>,
    class_decl: Option<String>,
    has_restrict_define: bool,
    has_exp10_aliases: bool,
}

#[derive(Clone, Debug)]
/// One row in a backend differential report table.
struct CppDiffRow {
    case: String,
    class: &'static str,
    rust_reason: String,
    cpp_reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Reduced shell signature extracted from generated C backend text.
struct CShellSignature {
    has_typedef_struct: bool,
    has_faustfloat_define: bool,
    has_restrict_define: bool,
    has_instance_constants_fn: bool,
    has_instance_reset_ui_fn: bool,
    has_instance_clear_fn: bool,
    has_instance_init_fn: bool,
    has_build_ui_fn: bool,
    has_compute_fn: bool,
    has_instance_init_ordered_calls: bool,
}

/// Regenerates the representative C++ backend shell-signature diff report.
fn cpp_backend_diff_report() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();
    let report_path = root.join(CPP_BACKEND_DIFF_REPORT_REL_PATH);
    let compiler = compiler::Compiler::new();
    let (cpp_bin, cpp_bin_is_fallback) = resolve_cpp_faust_bin();
    let options = codegen::backends::cpp::CppOptions {
        class_name: Some("mydsp".to_owned()),
        ..codegen::backends::cpp::CppOptions::default()
    };

    let representative = [
        "rep_01_passthrough.dsp",
        "rep_05_one_pole_lowpass.dsp",
        "rep_09_ui_slider.dsp",
        "rep_17_ui_groups.dsp",
        "rep_20_environment_waveform.dsp",
        "rep_22_parallel_mix.dsp",
        "rep_28_nested_ui_groups.dsp",
        "rep_31_extended_primitives.dsp",
    ];

    let mut rows = Vec::with_capacity(representative.len());
    let mut ok = 0usize;
    let mut diff = 0usize;
    let mut unsupported = 0usize;

    for case in representative {
        let path = root.join("tests").join("corpus").join(case);
        let rust_output = compiler.compile_file_default_to_cpp(&path, &options);
        let cpp_output = Command::new(&cpp_bin).arg(&path).output();

        let row = match (rust_output, cpp_output) {
            (Ok(_), Err(err)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: "Rust path ok".to_owned(),
                    cpp_reason: format!("cannot run `{}`: {err}", cpp_bin.display()),
                }
            }
            (Err(err), Err(cpp_err)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: format!("cannot run `{}`: {cpp_err}", cpp_bin.display()),
                }
            }
            (Ok(rust_text), Ok(cpp_output)) if cpp_output.status.success() => {
                let cpp_text = String::from_utf8(cpp_output.stdout)?;
                let rust_sig = extract_shell_signature(&rust_text);
                let cpp_sig = extract_shell_signature(&cpp_text);
                if rust_sig == cpp_sig {
                    ok = ok.saturating_add(1);
                    CppDiffRow {
                        case: case.to_owned(),
                        class: "OK",
                        rust_reason: "shell signature matches".to_owned(),
                        cpp_reason: "ok".to_owned(),
                    }
                } else {
                    diff = diff.saturating_add(1);
                    CppDiffRow {
                        case: case.to_owned(),
                        class: "DIFF",
                        rust_reason: format!("rust={rust_sig:?}"),
                        cpp_reason: format!("cpp={cpp_sig:?}"),
                    }
                }
            }
            (Ok(_), Ok(cpp_output)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: "Rust path ok".to_owned(),
                    cpp_reason: first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stderr))
                        .or_else(|| {
                            first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stdout))
                        })
                        .unwrap_or_else(|| format!("failed with status {}", cpp_output.status)),
                }
            }
            (Err(err), Ok(cpp_output)) if cpp_output.status.success() => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: "C++ path ok".to_owned(),
                }
            }
            (Err(err), Ok(cpp_output)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stderr))
                        .or_else(|| {
                            first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stdout))
                        })
                        .unwrap_or_else(|| format!("failed with status {}", cpp_output.status)),
                }
            }
        };
        rows.push(row);
    }

    let mut out = String::new();
    writeln!(
        &mut out,
        "# Phase 6 C++ Backend Differential Report (Module-First, Shell-Normalized)"
    )?;
    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "Generated by: `cargo run -p xtask -- cpp-backend-diff-report`"
    )?;
    writeln!(&mut out, "- C++ binary: `{}`", cpp_bin.display())?;
    if cpp_bin_is_fallback {
        writeln!(
            &mut out,
            "- Note: fallback to `faust` from PATH because `{}/build/bin/faust` was not found.",
            CPP_SOURCE_ROOT
        )?;
    }
    writeln!(
        &mut out,
        "- Normalization: compare module-shell signature only"
    )?;
    writeln!(&mut out, "  - `#define FAUSTCLASS <name>`")?;
    writeln!(&mut out, "  - `class <name> : public dsp`")?;
    writeln!(
        &mut out,
        "  - presence of `RESTRICT` and Apple `exp10` aliases"
    )?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Summary")?;
    writeln!(&mut out, "- Cases: `{}`", rows.len())?;
    writeln!(&mut out, "- `OK`: `{ok}`")?;
    writeln!(&mut out, "- `DIFF`: `{diff}`")?;
    writeln!(&mut out, "- `UNSUPPORTED`: `{unsupported}`")?;
    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "| Case | Status | Rust detail | C++ detail |\n|------|--------|-------------|------------|"
    )?;
    for row in &rows {
        writeln!(
            &mut out,
            "| `{}` | `{}` | `{}` | `{}` |",
            row.case,
            row.class,
            markdown_escape(&row.rust_reason),
            markdown_escape(&row.cpp_reason)
        )?;
    }
    writeln!(&mut out)?;
    writeln!(&mut out, "## Notes")?;
    writeln!(
        &mut out,
        "- This report tracks module-shell parity while full production signal->FIR lowering is still in progress."
    )?;
    writeln!(
        &mut out,
        "- `DIFF` rows are expected to shrink as statement/value lowering and orchestration parity advance."
    )?;

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&report_path, out)?;
    println!("updated {}", report_path.display());
    Ok(())
}

/// Regenerates the representative table fast-lane parity report.
fn table_fastlane_diff_report() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();
    let report_path = root.join(TABLE_FASTLANE_DIFF_REPORT_REL_PATH);
    let compiler = compiler::Compiler::new();
    let (cpp_bin, cpp_bin_is_fallback) = resolve_cpp_faust_bin();
    let options = codegen::backends::cpp::CppOptions {
        class_name: Some("mydsp".to_owned()),
        ..codegen::backends::cpp::CppOptions::default()
    };

    let representative = [
        "rep_20_environment_waveform.dsp",
        "rep_30_environment_access_pair.dsp",
        "rep_34_table_rdtable_readonly_const.dsp",
        "rep_35_table_rwtable_runtime_write.dsp",
        "rep_36_table_rdtable_negative_index.dsp",
        "rep_37_table_rwtable_negative_indices.dsp",
    ];

    let mut rows = Vec::with_capacity(representative.len());
    let mut ok = 0usize;
    let mut diff = 0usize;
    let mut unsupported = 0usize;

    for case in representative {
        let path = root.join("tests").join("corpus").join(case);
        let rust_output = compiler.compile_file_default_to_cpp_with_lane(
            &path,
            &options,
            compiler::SignalFirLane::TransformFastLane,
        );
        let cpp_output = Command::new(&cpp_bin).arg(&path).output();

        let row = match (rust_output, cpp_output) {
            (Ok(_), Err(err)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: "Rust fast-lane ok".to_owned(),
                    cpp_reason: format!("cannot run `{}`: {err}", cpp_bin.display()),
                }
            }
            (Err(err), Err(cpp_err)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: format!("cannot run `{}`: {cpp_err}", cpp_bin.display()),
                }
            }
            (Ok(rust_text), Ok(cpp_output)) if cpp_output.status.success() => {
                let cpp_text = String::from_utf8(cpp_output.stdout)?;
                let rust_sig = extract_shell_signature(&rust_text);
                let cpp_sig = extract_shell_signature(&cpp_text);
                if rust_sig == cpp_sig {
                    ok = ok.saturating_add(1);
                    CppDiffRow {
                        case: case.to_owned(),
                        class: "OK",
                        rust_reason: "shell signature matches".to_owned(),
                        cpp_reason: "ok".to_owned(),
                    }
                } else {
                    diff = diff.saturating_add(1);
                    CppDiffRow {
                        case: case.to_owned(),
                        class: "DIFF",
                        rust_reason: format!("rust={rust_sig:?}"),
                        cpp_reason: format!("cpp={cpp_sig:?}"),
                    }
                }
            }
            (Ok(_), Ok(cpp_output)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: "Rust fast-lane ok".to_owned(),
                    cpp_reason: first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stderr))
                        .or_else(|| {
                            first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stdout))
                        })
                        .unwrap_or_else(|| format!("failed with status {}", cpp_output.status)),
                }
            }
            (Err(err), Ok(cpp_output)) if cpp_output.status.success() => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: "C++ path ok".to_owned(),
                }
            }
            (Err(err), Ok(cpp_output)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stderr))
                        .or_else(|| {
                            first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stdout))
                        })
                        .unwrap_or_else(|| format!("failed with status {}", cpp_output.status)),
                }
            }
        };
        rows.push(row);
    }

    let mut out = String::new();
    writeln!(
        &mut out,
        "# Phase 6 Table Fast-Lane Differential Report (C++ vs Rust)"
    )?;
    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "Generated by: `cargo run -p xtask -- table-fastlane-diff-report`"
    )?;
    writeln!(&mut out, "- C++ binary: `{}`", cpp_bin.display())?;
    if cpp_bin_is_fallback {
        writeln!(
            &mut out,
            "- Note: fallback to `faust` from PATH because `{}/build/bin/faust` was not found.",
            CPP_SOURCE_ROOT
        )?;
    }
    writeln!(
        &mut out,
        "- Rust route: `compiler::SignalFirLane::TransformFastLane`"
    )?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Summary")?;
    writeln!(&mut out, "- Cases: `{}`", rows.len())?;
    writeln!(&mut out, "- `OK`: `{ok}`")?;
    writeln!(&mut out, "- `DIFF`: `{diff}`")?;
    writeln!(&mut out, "- `UNSUPPORTED`: `{unsupported}`")?;
    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "| Case | Status | Rust detail | C++ detail |\n|------|--------|-------------|------------|"
    )?;
    for row in &rows {
        writeln!(
            &mut out,
            "| `{}` | `{}` | `{}` | `{}` |",
            row.case,
            row.class,
            markdown_escape(&row.rust_reason),
            markdown_escape(&row.cpp_reason)
        )?;
    }
    writeln!(&mut out)?;
    writeln!(&mut out, "## Notes")?;
    writeln!(
        &mut out,
        "- Comparison is shell-signature based (`FAUSTCLASS`, class declaration, macro aliases)."
    )?;
    writeln!(
        &mut out,
        "- This report focuses on table-oriented fixtures for Step 2J closure."
    )?;

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&report_path, out)?;
    println!("updated {}", report_path.display());
    Ok(())
}

/// Regenerates the representative C fast-lane shell-signature diff report.
fn c_fastlane_diff_report() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();
    let report_path = root.join(C_FASTLANE_DIFF_REPORT_REL_PATH);
    let compiler = compiler::Compiler::new();
    let (cpp_bin, cpp_bin_is_fallback) = resolve_cpp_faust_bin();
    let options = codegen::backends::c::COptions {
        class_name: Some("mydsp".to_owned()),
        ..codegen::backends::c::COptions::default()
    };

    let representative = [
        "rep_01_passthrough.dsp",
        "rep_05_one_pole_lowpass.dsp",
        "rep_07_nonlinear_clip.dsp",
        "rep_09_ui_slider.dsp",
        "rep_10_two_in_two_out_ui.dsp",
        "rep_17_ui_groups.dsp",
        "rep_20_environment_waveform.dsp",
        "rep_22_parallel_mix.dsp",
        "rep_23_feedback_simple.dsp",
        "rep_28_nested_ui_groups.dsp",
        "rep_30_environment_access_pair.dsp",
        "rep_31_extended_primitives.dsp",
        "rep_34_table_rdtable_readonly_const.dsp",
        "rep_35_table_rwtable_runtime_write.dsp",
        "rep_36_table_rdtable_negative_index.dsp",
        "rep_37_table_rwtable_negative_indices.dsp",
    ];

    let mut rows = Vec::with_capacity(representative.len());
    let mut ok = 0usize;
    let mut diff = 0usize;
    let mut unsupported = 0usize;

    for case in representative {
        let path = root.join("tests").join("corpus").join(case);
        let rust_output = compiler.compile_file_default_to_c_with_lane(
            &path,
            &options,
            compiler::SignalFirLane::TransformFastLane,
        );
        let cpp_output = Command::new(&cpp_bin)
            .arg(&path)
            .arg("-lang")
            .arg("c")
            .arg("-cn")
            .arg("mydsp")
            .output();

        let row = match (rust_output, cpp_output) {
            (Ok(_), Err(err)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: "Rust C fast-lane ok".to_owned(),
                    cpp_reason: format!("cannot run `{}`: {err}", cpp_bin.display()),
                }
            }
            (Err(err), Err(cpp_err)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: format!("cannot run `{}`: {cpp_err}", cpp_bin.display()),
                }
            }
            (Ok(rust_text), Ok(cpp_output)) if cpp_output.status.success() => {
                let cpp_text = String::from_utf8(cpp_output.stdout)?;
                let rust_sig = extract_c_shell_signature(&rust_text);
                let cpp_sig = extract_c_shell_signature(&cpp_text);
                if rust_sig == cpp_sig {
                    ok = ok.saturating_add(1);
                    CppDiffRow {
                        case: case.to_owned(),
                        class: "OK",
                        rust_reason: "C shell signature matches".to_owned(),
                        cpp_reason: "ok".to_owned(),
                    }
                } else {
                    diff = diff.saturating_add(1);
                    CppDiffRow {
                        case: case.to_owned(),
                        class: "DIFF",
                        rust_reason: format!("rust={rust_sig:?}"),
                        cpp_reason: format!("cpp={cpp_sig:?}"),
                    }
                }
            }
            (Ok(_), Ok(cpp_output)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: "Rust C fast-lane ok".to_owned(),
                    cpp_reason: first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stderr))
                        .or_else(|| {
                            first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stdout))
                        })
                        .unwrap_or_else(|| format!("failed with status {}", cpp_output.status)),
                }
            }
            (Err(err), Ok(cpp_output)) if cpp_output.status.success() => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: "C++ C backend path ok".to_owned(),
                }
            }
            (Err(err), Ok(cpp_output)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stderr))
                        .or_else(|| {
                            first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stdout))
                        })
                        .unwrap_or_else(|| format!("failed with status {}", cpp_output.status)),
                }
            }
        };
        rows.push(row);
    }

    let mut out = String::new();
    writeln!(
        &mut out,
        "# Phase 6 C Fast-Lane Differential Report (C++ `-lang c` vs Rust)"
    )?;
    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "Generated by: `cargo run -p xtask -- c-fastlane-diff-report`"
    )?;
    writeln!(&mut out, "- C++ binary: `{}`", cpp_bin.display())?;
    if cpp_bin_is_fallback {
        writeln!(
            &mut out,
            "- Note: fallback to `faust` from PATH because `{}/build/bin/faust` was not found.",
            CPP_SOURCE_ROOT
        )?;
    }
    writeln!(
        &mut out,
        "- C++ command: `faust <case>.dsp -lang c -cn mydsp`"
    )?;
    writeln!(
        &mut out,
        "- Rust route: `compiler::SignalFirLane::TransformFastLane` + `--dump-c`"
    )?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Summary")?;
    writeln!(&mut out, "- Cases: `{}`", rows.len())?;
    writeln!(&mut out, "- `OK`: `{ok}`")?;
    writeln!(&mut out, "- `DIFF`: `{diff}`")?;
    writeln!(&mut out, "- `UNSUPPORTED`: `{unsupported}`")?;
    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "| Case | Status | Rust detail | C++ detail |\n|------|--------|-------------|------------|"
    )?;
    for row in &rows {
        writeln!(
            &mut out,
            "| `{}` | `{}` | `{}` | `{}` |",
            row.case,
            row.class,
            markdown_escape(&row.rust_reason),
            markdown_escape(&row.cpp_reason)
        )?;
    }
    writeln!(&mut out)?;
    writeln!(&mut out, "## Notes")?;
    writeln!(
        &mut out,
        "- Comparison is C-shell signature based (typedef/defines/lifecycle/UI/compute function presence and init call ordering)."
    )?;
    writeln!(
        &mut out,
        "- This report is the Step 7B guardrail for C fast-lane parity progression."
    )?;

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&report_path, out)?;
    println!("updated {}", report_path.display());
    Ok(())
}

/// Regenerates the full-corpus C/C++ backend fast-lane differential report.
fn backend_full_corpus_diff_report() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();
    let report_path = root.join(BACKEND_FULL_CORPUS_DIFF_REPORT_REL_PATH);
    let compiler = compiler::Compiler::new();
    let (cpp_bin, cpp_bin_is_fallback) = resolve_cpp_faust_bin();
    let files = corpus_files()?;
    let cpp_options = codegen::backends::cpp::CppOptions {
        class_name: Some("mydsp".to_owned()),
        ..codegen::backends::cpp::CppOptions::default()
    };
    let c_options = codegen::backends::c::COptions {
        class_name: Some("mydsp".to_owned()),
        ..codegen::backends::c::COptions::default()
    };

    let mut cpp_rows = Vec::with_capacity(files.len());
    let mut cpp_ok = 0usize;
    let mut cpp_diff = 0usize;
    let mut cpp_unsupported = 0usize;

    let mut c_rows = Vec::with_capacity(files.len());
    let mut c_ok = 0usize;
    let mut c_diff = 0usize;
    let mut c_unsupported = 0usize;

    for file in &files {
        let case = case_name(file)?;

        let rust_cpp = compiler.compile_file_default_to_cpp_with_lane(
            file,
            &cpp_options,
            compiler::SignalFirLane::TransformFastLane,
        );
        let cpp_cpp = Command::new(&cpp_bin)
            .arg(file)
            .arg("-lang")
            .arg("cpp")
            .arg("-cn")
            .arg("mydsp")
            .output();
        let cpp_row = match (rust_cpp, cpp_cpp) {
            (Ok(_), Err(err)) => {
                cpp_unsupported = cpp_unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.clone(),
                    class: "UNSUPPORTED",
                    rust_reason: "Rust C++ fast-lane ok".to_owned(),
                    cpp_reason: format!("cannot run `{}`: {err}", cpp_bin.display()),
                }
            }
            (Err(err), Err(cpp_err)) => {
                cpp_unsupported = cpp_unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.clone(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: format!("cannot run `{}`: {cpp_err}", cpp_bin.display()),
                }
            }
            (Ok(rust_text), Ok(cpp_output)) if cpp_output.status.success() => {
                let cpp_text = String::from_utf8(cpp_output.stdout)?;
                let rust_sig = extract_shell_signature(&rust_text);
                let cpp_sig = extract_shell_signature(&cpp_text);
                if rust_sig == cpp_sig {
                    cpp_ok = cpp_ok.saturating_add(1);
                    CppDiffRow {
                        case: case.clone(),
                        class: "OK",
                        rust_reason: "shell signature matches".to_owned(),
                        cpp_reason: "ok".to_owned(),
                    }
                } else {
                    cpp_diff = cpp_diff.saturating_add(1);
                    CppDiffRow {
                        case: case.clone(),
                        class: "DIFF",
                        rust_reason: format!("rust={rust_sig:?}"),
                        cpp_reason: format!("cpp={cpp_sig:?}"),
                    }
                }
            }
            (Ok(_), Ok(cpp_output)) => {
                cpp_unsupported = cpp_unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.clone(),
                    class: "UNSUPPORTED",
                    rust_reason: "Rust C++ fast-lane ok".to_owned(),
                    cpp_reason: first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stderr))
                        .or_else(|| {
                            first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stdout))
                        })
                        .unwrap_or_else(|| format!("failed with status {}", cpp_output.status)),
                }
            }
            (Err(err), Ok(cpp_output)) if cpp_output.status.success() => {
                cpp_unsupported = cpp_unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.clone(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: "C++ reference path ok".to_owned(),
                }
            }
            (Err(err), Ok(cpp_output)) => {
                cpp_unsupported = cpp_unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.clone(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stderr))
                        .or_else(|| {
                            first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stdout))
                        })
                        .unwrap_or_else(|| format!("failed with status {}", cpp_output.status)),
                }
            }
        };
        cpp_rows.push(cpp_row);

        let rust_c = compiler.compile_file_default_to_c_with_lane(
            file,
            &c_options,
            compiler::SignalFirLane::TransformFastLane,
        );
        let cpp_c = Command::new(&cpp_bin)
            .arg(file)
            .arg("-lang")
            .arg("c")
            .arg("-cn")
            .arg("mydsp")
            .output();
        let c_row = match (rust_c, cpp_c) {
            (Ok(_), Err(err)) => {
                c_unsupported = c_unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.clone(),
                    class: "UNSUPPORTED",
                    rust_reason: "Rust C fast-lane ok".to_owned(),
                    cpp_reason: format!("cannot run `{}`: {err}", cpp_bin.display()),
                }
            }
            (Err(err), Err(cpp_err)) => {
                c_unsupported = c_unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.clone(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: format!("cannot run `{}`: {cpp_err}", cpp_bin.display()),
                }
            }
            (Ok(rust_text), Ok(cpp_output)) if cpp_output.status.success() => {
                let cpp_text = String::from_utf8(cpp_output.stdout)?;
                let rust_sig = extract_c_shell_signature(&rust_text);
                let cpp_sig = extract_c_shell_signature(&cpp_text);
                if rust_sig == cpp_sig {
                    c_ok = c_ok.saturating_add(1);
                    CppDiffRow {
                        case: case.clone(),
                        class: "OK",
                        rust_reason: "C shell signature matches".to_owned(),
                        cpp_reason: "ok".to_owned(),
                    }
                } else {
                    c_diff = c_diff.saturating_add(1);
                    CppDiffRow {
                        case: case.clone(),
                        class: "DIFF",
                        rust_reason: format!("rust={rust_sig:?}"),
                        cpp_reason: format!("cpp={cpp_sig:?}"),
                    }
                }
            }
            (Ok(_), Ok(cpp_output)) => {
                c_unsupported = c_unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.clone(),
                    class: "UNSUPPORTED",
                    rust_reason: "Rust C fast-lane ok".to_owned(),
                    cpp_reason: first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stderr))
                        .or_else(|| {
                            first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stdout))
                        })
                        .unwrap_or_else(|| format!("failed with status {}", cpp_output.status)),
                }
            }
            (Err(err), Ok(cpp_output)) if cpp_output.status.success() => {
                c_unsupported = c_unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.clone(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: "C reference path ok".to_owned(),
                }
            }
            (Err(err), Ok(cpp_output)) => {
                c_unsupported = c_unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.clone(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stderr))
                        .or_else(|| {
                            first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stdout))
                        })
                        .unwrap_or_else(|| format!("failed with status {}", cpp_output.status)),
                }
            }
        };
        c_rows.push(c_row);
    }

    let mut out = String::new();
    writeln!(
        &mut out,
        "# Phase 6 Backend Full-Corpus Differential Report (Rust fast-lane vs C++ reference)"
    )?;
    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "Generated by: `cargo run -p xtask -- backend-full-corpus-diff-report`"
    )?;
    writeln!(&mut out, "- C++ binary: `{}`", cpp_bin.display())?;
    if cpp_bin_is_fallback {
        writeln!(
            &mut out,
            "- Note: fallback to `faust` from PATH because `{}/build/bin/faust` was not found.",
            CPP_SOURCE_ROOT
        )?;
    }
    writeln!(&mut out, "- Corpus: `tests/corpus/*.dsp`")?;
    writeln!(
        &mut out,
        "- Rust route: `compiler::SignalFirLane::TransformFastLane`"
    )?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Summary")?;
    writeln!(&mut out, "- Cases: `{}`", files.len())?;
    writeln!(
        &mut out,
        "- C++ backend parity: `OK={cpp_ok}` `DIFF={cpp_diff}` `UNSUPPORTED={cpp_unsupported}`"
    )?;
    writeln!(
        &mut out,
        "- C backend parity: `OK={c_ok}` `DIFF={c_diff}` `UNSUPPORTED={c_unsupported}`"
    )?;
    writeln!(&mut out)?;

    writeln!(&mut out, "## C++ Backend Matrix")?;
    writeln!(
        &mut out,
        "| Case | Status | Rust detail | C++ detail |\n|------|--------|-------------|------------|"
    )?;
    for row in &cpp_rows {
        writeln!(
            &mut out,
            "| `{}` | `{}` | `{}` | `{}` |",
            row.case,
            row.class,
            markdown_escape(&row.rust_reason),
            markdown_escape(&row.cpp_reason)
        )?;
    }
    writeln!(&mut out)?;

    writeln!(&mut out, "## C Backend Matrix")?;
    writeln!(
        &mut out,
        "| Case | Status | Rust detail | C++ detail |\n|------|--------|-------------|------------|"
    )?;
    for row in &c_rows {
        writeln!(
            &mut out,
            "| `{}` | `{}` | `{}` | `{}` |",
            row.case,
            row.class,
            markdown_escape(&row.rust_reason),
            markdown_escape(&row.cpp_reason)
        )?;
    }
    writeln!(&mut out)?;
    writeln!(&mut out, "## Notes")?;
    writeln!(
        &mut out,
        "- C++ reference command: `faust <case>.dsp -lang cpp -cn mydsp` (shell-signature metric)."
    )?;
    writeln!(
        &mut out,
        "- C reference command: `faust <case>.dsp -lang c -cn mydsp` (C-shell-signature metric)."
    )?;

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&report_path, out)?;
    println!("updated {}", report_path.display());
    Ok(())
}

/// Extracts the normalized C++ backend shell signature from emitted source text.
fn extract_shell_signature(text: &str) -> ShellSignature {
    let mut faustclass = None::<String>;
    let mut class_decl = None::<String>;
    let mut has_restrict_define = false;
    let mut has_exp10f_alias = false;
    let mut has_exp10_alias = false;

    for raw in text.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("#define FAUSTCLASS ") {
            faustclass = Some(rest.trim().to_owned());
        }
        if let Some(rest) = line.strip_prefix("class ")
            && let Some((name, _)) = rest.split_once(" : public dsp")
        {
            class_decl = Some(name.trim().to_owned());
        }
        if line.contains("#define RESTRICT") {
            has_restrict_define = true;
        }
        if line == "#define exp10f __exp10f" {
            has_exp10f_alias = true;
        }
        if line == "#define exp10 __exp10" {
            has_exp10_alias = true;
        }
    }

    ShellSignature {
        faustclass,
        class_decl,
        has_restrict_define,
        has_exp10_aliases: has_exp10f_alias && has_exp10_alias,
    }
}

/// Extracts the normalized C backend shell signature from emitted source text.
fn extract_c_shell_signature(text: &str) -> CShellSignature {
    let has_typedef_struct = text.contains("typedef struct {");
    let has_faustfloat_define = text.contains("#ifndef FAUSTFLOAT");
    let has_restrict_define = text.contains("#define RESTRICT");
    let has_instance_constants_fn = text.contains("void instanceConstants");
    let has_instance_reset_ui_fn = text.contains("void instanceResetUserInterface");
    let has_instance_clear_fn = text.contains("void instanceClear");
    let has_instance_init_fn = text.contains("void instanceInit");
    let has_build_ui_fn = text.contains("void buildUserInterface");
    let has_compute_fn = text.contains("void compute");

    let has_instance_init_ordered_calls = has_ordered_instance_init_calls(text);

    CShellSignature {
        has_typedef_struct,
        has_faustfloat_define,
        has_restrict_define,
        has_instance_constants_fn,
        has_instance_reset_ui_fn,
        has_instance_clear_fn,
        has_instance_init_fn,
        has_build_ui_fn,
        has_compute_fn,
        has_instance_init_ordered_calls,
    }
}

/// Detects the canonical `instanceInit` call ordering in generated C output.
fn has_ordered_instance_init_calls(text: &str) -> bool {
    let mut search_from = 0usize;
    while let Some(rel) = text[search_from..].find("void instanceInit") {
        let start = search_from + rel;
        let tail = &text[start..];
        let end = tail.find("}\n").unwrap_or(tail.len());
        let body = &tail[..end];
        let c_i = body.find("instanceConstants");
        let r_i = body.find("instanceResetUserInterface");
        let cl_i = body.find("instanceClear");
        if matches!((c_i, r_i, cl_i), (Some(a), Some(b), Some(c)) if a < b && b < c) {
            return true;
        }
        search_from = start + "void instanceInit".len();
    }
    false
}

/// Resolves the preferred C++ reference compiler path for report workflows.
///
/// Returns `(path, used_fallback)` where `used_fallback` records whether the
/// helper had to fall back to `faust` from `PATH`.
fn resolve_cpp_faust_bin() -> (PathBuf, bool) {
    if let Some(path) = std::env::var_os("FAUST_CPP_BIN") {
        return (PathBuf::from(path), false);
    }
    let built = Path::new(CPP_SOURCE_ROOT).join("build/bin/faust");
    if built.exists() {
        return (built, false);
    }
    (PathBuf::from("faust"), true)
}

/// Runs one DSP through the C++ reference compiler and summarizes the outcome.
fn cpp_case_status(cpp_bin: &Path, input: &Path) -> Result<CaseStatus, Box<dyn std::error::Error>> {
    let status = Command::new(cpp_bin)
        .arg(input)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        return Ok(CaseStatus {
            ok: true,
            stage: "ok",
            reason: "ok".to_owned(),
        });
    }

    let output = Command::new(cpp_bin).arg(input).output()?;
    let reason = first_non_empty_line(&String::from_utf8_lossy(&output.stderr))
        .or_else(|| first_non_empty_line(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_else(|| format!("failed with status {}", output.status));
    Ok(CaseStatus {
        ok: false,
        stage: "error",
        reason,
    })
}

/// Runs one DSP through the Rust compile-to-signals path and summarizes the outcome.
fn rust_case_status(compiler: &compiler::Compiler, input: &Path) -> CaseStatus {
    match compiler.compile_file_default_to_signals(input) {
        Ok(_) => CaseStatus {
            ok: true,
            stage: "ok",
            reason: "ok".to_owned(),
        },
        Err(err) => {
            let (stage, reason) = match &err {
                compiler::CompilerError::Import(_) => ("import", err.to_string()),
                compiler::CompilerError::Parse { .. } => ("parse", err.to_string()),
                compiler::CompilerError::Eval { .. } => ("eval", err.to_string()),
                compiler::CompilerError::Propagate { .. } => ("propagate", err.to_string()),
                compiler::CompilerError::Type { .. } => ("type", err.to_string()),
                compiler::CompilerError::Transform { .. } => ("transform", err.to_string()),
                compiler::CompilerError::FirVerify { .. } => ("fir", err.to_string()),
                compiler::CompilerError::Codegen { .. } => ("codegen", err.to_string()),
                compiler::CompilerError::CodegenC { .. } => ("codegen", err.to_string()),
                compiler::CompilerError::CodegenJulia { .. } => ("codegen", err.to_string()),
                compiler::CompilerError::CodegenInterp { .. } => ("codegen", err.to_string()),
                compiler::CompilerError::CodegenWasm { .. } => ("codegen", err.to_string()),
                compiler::CompilerError::MissingRoot { .. } => ("parse", err.to_string()),
            };
            CaseStatus {
                ok: false,
                stage,
                reason,
            }
        }
    }
}

/// Returns the compact `OK` / `ERR` cell text used in Markdown tables.
fn status_cell(status: &CaseStatus) -> &'static str {
    if status.ok { "OK" } else { "ERR" }
}

/// Returns the first non-empty trimmed line from command output.
fn first_non_empty_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

/// Escapes a string for safe embedding in one-cell Markdown tables.
fn markdown_escape(value: &str) -> String {
    value.replace('|', "\\|").replace(['\n', '\r'], " ")
}

/// Extracts declared token names from a yacc-style parser grammar.
fn extract_parser_tokens(source: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        let rest = if let Some(rest) = trimmed.strip_prefix("%token") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("%left") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("%right") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("%nonassoc") {
            rest
        } else {
            continue;
        };
        let rest = rest.trim();
        for raw in rest.split_whitespace() {
            let part = raw.trim_matches(|c: char| c == ',' || c == ';');
            if part.starts_with('<') || part.starts_with("/*") || part.starts_with("//") {
                continue;
            }
            if is_token_name(part) {
                set.insert(part.to_owned());
            }
        }
    }
    set
}

/// Extracts nonterminal heads from the C++ yacc grammar body.
fn extract_cpp_nonterminals(source: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in grammar_section(source).lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('%') || trimmed.starts_with('|') {
            continue;
        }
        let Some((head, _)) = trimmed.split_once(':') else {
            continue;
        };
        let head = head.trim();
        if is_ident_name(head) {
            set.insert(head.to_ascii_lowercase());
        }
    }
    set
}

/// Extracts nonterminal heads from the Rust grammar prototype body.
fn extract_rust_nonterminals(source: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in grammar_section(source).lines() {
        let trimmed = line.trim_start();
        let Some((head, _)) = trimmed.split_once("->") else {
            continue;
        };
        let head = head.trim();
        if is_ident_name(head) {
            set.insert(head.to_ascii_lowercase());
        }
    }
    set
}

/// Extracts declared lexer start states from a lex/flex source file.
fn extract_lexer_states(source: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        let rest = if let Some(rest) = trimmed.strip_prefix("%x") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("%s") {
            rest
        } else {
            continue;
        };
        for state in rest.split_whitespace() {
            let state = state.trim_matches(|c: char| c == ';');
            if is_ident_name(state) {
                set.insert(state.to_ascii_lowercase());
            }
        }
    }
    set
}

/// Extracts token names emitted by the C++ lexer implementation.
fn extract_cpp_lexer_emitted_tokens(source: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in source.lines() {
        let mut rest = line;
        while let Some(idx) = rest.find("return ") {
            let after = &rest[idx + "return ".len()..];
            if let Some(token) = scan_token_name(after) {
                set.insert(token);
            }
            rest = &after[after.char_indices().nth(1).map_or(after.len(), |(i, _)| i)..];
        }
    }
    set
}

/// Extracts token names emitted by the Rust lexer prototype.
fn extract_rust_lexer_emitted_tokens(source: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in source.lines() {
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0usize;
        while i < chars.len() {
            if chars[i] != '\'' {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '\'' {
                j += 1;
            }
            if j >= chars.len() {
                break;
            }
            let candidate: String = chars[i + 1..j].iter().collect();
            if is_token_name(&candidate) {
                set.insert(candidate);
            }
            // Move one character forward so overlapping quotes are still discovered.
            i += 1;
        }
    }
    set
}

/// Returns the grammar body between the first two yacc `%%` markers.
fn grammar_section(source: &str) -> &str {
    let mut marks = source.match_indices("%%");
    let Some((first, _)) = marks.next() else {
        return source;
    };
    let Some((second, _)) = marks.next() else {
        return &source[first + 2..];
    };
    &source[first + 2..second]
}

/// Returns `true` when `s` matches the token naming convention used in reports.
fn is_token_name(s: &str) -> bool {
    let mut has_upper = false;
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            has_upper = true;
        } else if !(c.is_ascii_digit() || c == '_') {
            return false;
        }
    }
    has_upper
}

/// Returns `true` when `s` is a simple ASCII identifier.
fn is_ident_name(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Scans the first token-like identifier from a lexer `return ...` fragment.
fn scan_token_name(source: &str) -> Option<String> {
    let mut start = None;
    for (idx, c) in source.char_indices() {
        if c.is_ascii_uppercase() || c == '_' {
            start = Some(idx);
            break;
        }
    }
    let start = start?;
    let token: String = source[start..]
        .chars()
        .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
        .collect();
    if is_token_name(&token) {
        Some(token)
    } else {
        None
    }
}

/// Returns `left - right` as a sorted vector.
fn diff_sorted(left: &BTreeSet<String>, right: &BTreeSet<String>) -> Vec<String> {
    left.difference(right).cloned().collect()
}

/// Returns accepted Rust alias names for one C++ token name.
fn token_aliases(cpp_name: &str) -> &'static [&'static str] {
    match cpp_name {
        "VIRG" => &["PAR"],
        "LISTING" => &["BLST"],
        _ => &[],
    }
}

/// Returns accepted Rust alias names for one C++ nonterminal head.
fn nonterminal_aliases(cpp_name: &str) -> &'static [&'static str] {
    match cpp_name {
        "params" => &["paramlist"],
        "recinition" => &["recdefinition"],
        "ident" => &["identexpr"],
        "fun" => &["funname"],
        "string" => &["rawstring", "uqstring", "fstring"],
        "doc" => &["doccontent"],
        "doctxt" | "doceqn" | "docdgm" | "docmtd" | "doclst" | "docntc" => &["docelem"],
        "lstattrdef" => &["lstattr"],
        "lstattrval" => &["lstattrvalue"],
        "ffunction" | "fconst" | "fvariable" | "fpar" | "fseq" | "fsum" | "fprod" | "finputs"
        | "foutputs" | "fondemand" | "fupsampling" | "fdownsampling" | "button" | "checkbox"
        | "vslider" | "hslider" | "nentry" | "vgroup" | "hgroup" | "tgroup" | "vbargraph"
        | "hbargraph" | "soundfile" => &["primitive"],
        _ => &[],
    }
}

/// Splits missing names into alias-covered and unresolved subsets.
fn partition_with_aliases(
    missing_exact: &[String],
    rust_set: &BTreeSet<String>,
    aliases: impl Fn(&str) -> &'static [&'static str],
) -> (Vec<(String, Vec<String>)>, Vec<String>) {
    let mut covered = Vec::new();
    let mut unresolved = Vec::new();

    for item in missing_exact {
        let mapped_hits = aliases(item)
            .iter()
            .copied()
            .filter(|candidate| rust_set.contains(*candidate))
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        if mapped_hits.is_empty() {
            unresolved.push(item.clone());
        } else {
            covered.push((item.clone(), mapped_hits));
        }
    }
    (covered, unresolved)
}

/// Renders one Markdown bullet section for a report.
fn render_list(out: &mut String, title: &str, items: &[String]) -> Result<(), std::fmt::Error> {
    writeln!(out, "### {title}")?;
    if items.is_empty() {
        writeln!(out, "- (none)")?;
    } else {
        for item in items {
            writeln!(out, "- `{item}`")?;
        }
    }
    Ok(())
}

/// Renders one Markdown section describing alias-covered mismatches.
fn render_alias_list(
    out: &mut String,
    title: &str,
    items: &[(String, Vec<String>)],
) -> Result<(), std::fmt::Error> {
    writeln!(out, "### {title}")?;
    if items.is_empty() {
        writeln!(out, "- (none)")?;
        return Ok(());
    }
    for (source, targets) in items {
        let mapped = targets
            .iter()
            .map(|v| format!("`{v}`"))
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(out, "- `{source}` -> {mapped}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_scenario_parse_accepts_known_names() {
        assert_eq!(TraceScenario::parse("zeros").unwrap(), TraceScenario::Zeros);
        assert_eq!(
            TraceScenario::parse("impulse").unwrap(),
            TraceScenario::Impulse
        );
        assert_eq!(TraceScenario::parse("ramp").unwrap(), TraceScenario::Ramp);
        assert_eq!(TraceScenario::parse("sine").unwrap(), TraceScenario::Sine);
    }

    #[test]
    fn trace_lane_parse_accepts_fast_aliases() {
        assert_eq!(TraceLane::parse("fast").unwrap(), TraceLane::Fast);
        assert_eq!(TraceLane::parse("fast-lane").unwrap(), TraceLane::Fast);
        assert_eq!(TraceLane::parse("transform").unwrap(), TraceLane::Fast);
    }

    #[test]
    fn parse_interp_trace_dump_defaults_and_required_case() {
        let mut args = vec![
            "--case".to_string(),
            "tests/corpus/rep_31_extended_primitives.dsp".to_string(),
        ]
        .into_iter();
        let opts = parse_interp_trace_dump_options(&mut args).unwrap();
        assert_eq!(opts.scenario, TraceScenario::Zeros);
        assert_eq!(opts.lane, TraceLane::Fast);
        assert_eq!(opts.sample_rate, 48_000);
        assert_eq!(opts.block_size, 64);
        assert_eq!(opts.num_blocks, 4);
        assert!(!opts.strict_fir_types);
    }

    #[test]
    fn parse_interp_trace_dump_accepts_strict_fir_types_flag() {
        let mut args = vec![
            "--case".to_string(),
            "tests/runtime_corpus/trace_01_passthrough.dsp".to_string(),
            "--strict-fir-types".to_string(),
        ]
        .into_iter();
        let opts = parse_interp_trace_dump_options(&mut args).unwrap();
        assert!(opts.strict_fir_types);
    }

    #[test]
    fn parse_interp_trace_batch_defaults() {
        let mut args = std::iter::empty::<String>();
        let opts = parse_interp_trace_batch_options(&mut args).unwrap();
        assert_eq!(opts.case, None);
        assert_eq!(opts.lane, TraceLane::Fast);
        assert_eq!(opts.sample_rate, 48_000);
        assert_eq!(opts.block_size, 64);
        assert_eq!(opts.num_blocks, 4);
        assert!(!opts.strict_fir_types);
    }

    #[test]
    fn parse_interp_trace_batch_accepts_strict_fir_types_flag() {
        let mut args = vec!["--strict-fir-types".to_string()].into_iter();
        let opts = parse_interp_trace_batch_options(&mut args).unwrap();
        assert!(opts.strict_fir_types);
    }

    #[test]
    fn fir_type_diagnostic_code_filter_matches_expected_groups() {
        assert!(is_fir_type_diagnostic_code("FIR-B03"));
        assert!(is_fir_type_diagnostic_code("FIR-U02"));
        assert!(is_fir_type_diagnostic_code("FIR-C01"));
        assert!(is_fir_type_diagnostic_code("FIR-FC03"));
        assert!(is_fir_type_diagnostic_code("FIR-T02"));
        assert!(is_fir_type_diagnostic_code("FIR-MA04"));
        assert!(is_fir_type_diagnostic_code("FIR-L03"));
        assert!(is_fir_type_diagnostic_code("FIR-SW01"));
        assert!(!is_fir_type_diagnostic_code("FIR-M07"));
        assert!(!is_fir_type_diagnostic_code("FIR-SC01"));
    }

    #[test]
    fn runtime_trace_scenario_mapping_for_typed_primitives() {
        let scenarios = trace_scenarios_for_runtime_case(Path::new(
            "tests/runtime_corpus/trace_31_extended_primitives_typed.dsp",
        ))
        .unwrap();
        assert_eq!(scenarios, vec![TraceScenario::Zeros]);
    }

    #[test]
    fn runtime_trace_snapshot_path_uses_case_and_scenario() {
        let path = runtime_trace_snapshot_path("trace_01_passthrough", TraceScenario::Impulse);
        let expected = runtime_trace_snapshot_root()
            .join("trace_01_passthrough")
            .join("impulse.json");
        assert_eq!(path, expected);
    }

    #[test]
    fn generate_impulse_inputs_sets_first_sample_only() {
        let inputs = generate_trace_inputs(TraceScenario::Impulse, 2, 5, 48_000);
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0], vec![1.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(inputs[1], vec![1.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn render_runtime_trace_json_contains_expected_keys() {
        let trace = RuntimeTrace {
            dsp_path: "tests/corpus/example.dsp".into(),
            lane: "fast-lane".into(),
            scenario: "zeros".into(),
            sample_rate: 48_000,
            block_size: 64,
            num_blocks: 1,
            num_inputs: 1,
            num_outputs: 1,
            outputs: vec![vec![0.0, 1.0]],
        };
        let json = render_runtime_trace_json(&trace);
        assert!(json.contains("\"backend\": \"interp\""));
        assert!(json.contains("\"signal_fir_lane\": \"fast-lane\""));
        assert!(json.contains("\"scenario\""));
        assert!(json.contains("\"outputs\""));
    }

    #[test]
    fn parse_runtime_trace_json_roundtrip() {
        let trace = RuntimeTrace {
            dsp_path: "tests/runtime_corpus/trace_01_passthrough.dsp".into(),
            lane: "fast-lane".into(),
            scenario: "impulse".into(),
            sample_rate: 48_000,
            block_size: 64,
            num_blocks: 1,
            num_inputs: 1,
            num_outputs: 1,
            outputs: vec![vec![1.0, 0.0]],
        };
        let parsed = parse_runtime_trace_json(&render_runtime_trace_json(&trace)).unwrap();
        assert_eq!(parsed, trace);
    }

    #[test]
    fn compare_runtime_traces_tolerates_small_float_delta() {
        let a = RuntimeTrace {
            dsp_path: "x".into(),
            lane: "normalized".into(),
            scenario: "zeros".into(),
            sample_rate: 48_000,
            block_size: 64,
            num_blocks: 1,
            num_inputs: 0,
            num_outputs: 1,
            outputs: vec![vec![1.0]],
        };
        let mut b = a.clone();
        b.outputs[0][0] = 1.0 + 1.0e-7;
        assert!(compare_runtime_traces(&a, &b, TraceCompareTolerances::default()).is_ok());
    }

    #[test]
    fn compare_runtime_traces_reports_large_float_delta() {
        let a = RuntimeTrace {
            dsp_path: "x".into(),
            lane: "normalized".into(),
            scenario: "zeros".into(),
            sample_rate: 48_000,
            block_size: 64,
            num_blocks: 1,
            num_inputs: 0,
            num_outputs: 1,
            outputs: vec![vec![1.0]],
        };
        let mut b = a.clone();
        b.outputs[0][0] = 1.1;
        let mismatch =
            compare_runtime_traces(&a, &b, TraceCompareTolerances::default()).unwrap_err();
        assert_eq!(mismatch.field, "outputs");
        assert_eq!(mismatch.channel, Some(0));
        assert_eq!(mismatch.sample, Some(0));
    }

    #[test]
    fn interp_trace_opt_level_diff_matches_on_passthrough_case() {
        let case = workspace_root().join("tests/runtime_corpus/trace_01_passthrough.dsp");
        interp_trace_diff_opt_levels_cases(&[case], false).unwrap();
    }

    #[test]
    fn parse_faustwasm_compiler_module_options_defaults_to_release() {
        let options =
            parse_faustwasm_compiler_module_options(std::iter::empty::<String>()).unwrap();
        assert!(options.release);
    }

    #[test]
    fn parse_faustwasm_compiler_module_options_accepts_debug_flag() {
        let options =
            parse_faustwasm_compiler_module_options(vec!["--debug".to_owned()].into_iter())
                .unwrap();
        assert!(!options.release);
    }

    #[test]
    fn verify_wasm_ffi_exports_accepts_expected_surface() {
        let bytes = wat::parse_str(
            r#"
            (module
              (memory (export "memory") 1)
              (func (export "faust_wasm_alloc"))
              (func (export "faust_wasm_dealloc"))
              (func (export "faust_wasm_version_ptr"))
              (func (export "faust_wasm_version_len"))
              (func (export "faust_wasm_compile_dsp"))
              (func (export "faust_wasm_result_is_ok"))
              (func (export "faust_wasm_result_wasm_ptr"))
              (func (export "faust_wasm_result_wasm_len"))
              (func (export "faust_wasm_result_json_ptr"))
              (func (export "faust_wasm_result_json_len"))
              (func (export "faust_wasm_result_compile_options_ptr"))
              (func (export "faust_wasm_result_compile_options_len"))
              (func (export "faust_wasm_result_error_ptr"))
              (func (export "faust_wasm_result_error_len"))
              (func (export "faust_wasm_result_free"))
              (func (export "faust_wasm_get_info"))
              (func (export "faust_wasm_expand_dsp"))
              (func (export "faust_wasm_generate_aux_files"))
              (func (export "faust_wasm_generate_aux_files_json"))
              (func (export "faust_wasm_text_result_is_ok"))
              (func (export "faust_wasm_text_result_ptr"))
              (func (export "faust_wasm_text_result_len"))
              (func (export "faust_wasm_text_result_free"))
            )
            "#,
        )
        .unwrap();

        verify_wasm_ffi_exports(&bytes).unwrap();
    }

    #[test]
    fn verify_wasm_ffi_exports_rejects_missing_exports() {
        let bytes = wat::parse_str(
            r#"
            (module
              (memory (export "memory") 1)
              (func (export "faust_wasm_alloc"))
            )
            "#,
        )
        .unwrap();

        let error = verify_wasm_ffi_exports(&bytes).unwrap_err().to_string();
        assert!(error.contains("faust_wasm_compile_dsp"));
        assert!(error.contains("faust_wasm_text_result_free"));
    }
}
