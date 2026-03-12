//! UI IR construction and matching helpers backed by `tlib::TreeArena`.
//!
//! # Source provenance (C++)
//! - `compiler/propagate/propagate.cpp`
//! - `compiler/signals/signals.hh`
//! - `compiler/signals/signals.cpp`
//! - `compiler/transform/signalFIRCompiler.hh`
//! - `compiler/generator/instructions_compiler.cpp`
//! - `compiler/generator/compile.cpp`
//!
//! # Public API mapping status
//! - Public construction API is [`UiBuilder`].
//! - Public inspection API is [`match_ui`] + [`UiMatch`].
//! - [`UiProgram`] is the canonical grouped-UI artifact owned after the
//!   `propagate` boundary in the Rust architecture contract.
//!
//! # Parity invariants
//! - UI nodes are represented as tagged trees with deterministic child order.
//! - Canonical grouped layout lives in this crate, not in backend-local
//!   heuristics.
//! - Group children preserve source traversal order through `cons`/`nil` lists.
//! - Controls are referenced by deterministic [`ControlId`] values rather than
//!   owning layout in DSP signal nodes.

use tlib::{NodeKind, TreeArena, TreeId, list_to_vec, vec_to_list};

pub const CRATE_NAME: &str = "ui";

/// UI node identifier in `TreeArena`.
pub type UiId = TreeId;

/// Stable control identifier joining DSP control references with UI layout.
pub type ControlId = u32;

const UI_GROUP_TAG: &str = "UIGROUP";
const UI_INPUT_CONTROL_TAG: &str = "UIINPUTCONTROL";
const UI_OUTPUT_CONTROL_TAG: &str = "UIOUTPUTCONTROL";
const UI_SOUNDFILE_TAG: &str = "UISOUNDFILE";

/// Stable crate identifier used in workspace-level tooling and diagnostics.
#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}

/// Canonical group-orientation family for grouped UI layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum UiGroupKind {
    Vertical = 0,
    Horizontal = 1,
    Tab = 2,
}

impl UiGroupKind {
    #[must_use]
    /// Decodes one raw arena integer into a group orientation.
    pub fn from_raw(raw: i64) -> Option<Self> {
        match raw {
            0 => Some(Self::Vertical),
            1 => Some(Self::Horizontal),
            2 => Some(Self::Tab),
            _ => None,
        }
    }

    #[must_use]
    /// Returns the stable lowercase name used in dumps and diagnostics.
    pub fn name(self) -> &'static str {
        match self {
            Self::Vertical => "vertical",
            Self::Horizontal => "horizontal",
            Self::Tab => "tab",
        }
    }
}

/// Stable UI control family shared by layout and DSP control references.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ControlKind {
    Button,
    Checkbox,
    VSlider,
    HSlider,
    NumEntry,
    VBargraph,
    HBargraph,
    Soundfile,
}

/// Numeric range metadata for slider-like controls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlRange {
    pub init: f64,
    pub min: f64,
    pub max: f64,
    pub step: f64,
}

/// Canonical control-registry entry referenced by grouped UI layout and later
/// DSP/FIR lowering.
#[derive(Debug)]
pub struct ControlSpec {
    pub id: ControlId,
    pub kind: ControlKind,
    pub label: String,
    pub metadata: Vec<(String, String)>,
    pub range: Option<ControlRange>,
}

/// Source of the canonical root group stored in [`UiProgram`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiRootOrigin {
    Explicit,
    Synthesized,
}

/// Canonical grouped-UI artifact owned after the `propagate` boundary.
///
/// Rust architecture note:
/// - `porting/ui-ir-architecture-contract-2026-03-12-en.md`
///
/// Mapping status:
/// - `adapted` relative to the C++ internal clock-environment/path encoding.
/// - `1:1` behaviorally for grouped `buildUserInterface` ownership.
#[derive(Debug)]
pub struct UiProgram {
    pub arena: TreeArena,
    pub root: UiId,
    pub controls: Vec<ControlSpec>,
    pub root_origin: UiRootOrigin,
}

impl UiProgram {
    #[must_use]
    /// Builds one empty grouped-UI program with a canonical empty vertical root.
    pub fn empty() -> Self {
        let mut arena = TreeArena::new();
        let root = UiBuilder::new(&mut arena).vgroup("", &[]);
        Self {
            arena,
            root,
            controls: Vec::new(),
            root_origin: UiRootOrigin::Synthesized,
        }
    }

    #[must_use]
    /// Returns one control specification by stable [`ControlId`].
    pub fn control(&self, id: ControlId) -> Option<&ControlSpec> {
        self.controls.get(usize::try_from(id).ok()?)
    }

    #[must_use]
    /// Returns `true` when this program has no registered UI controls.
    pub fn is_empty(&self) -> bool {
        self.controls.is_empty()
    }
}

/// Canonical builder API for constructing UI IR nodes.
pub struct UiBuilder<'a> {
    arena: &'a mut TreeArena,
}

impl<'a> UiBuilder<'a> {
    #[must_use]
    /// Creates a `UiBuilder` bound to one mutable `TreeArena`.
    pub fn new(arena: &'a mut TreeArena) -> Self {
        Self { arena }
    }

    #[must_use]
    /// Builds one grouped UI node with the provided ordered child list.
    pub fn group(&mut self, kind: UiGroupKind, label: &str, children: &[UiId]) -> UiId {
        let kind = self.arena.int(kind as i64);
        let label = self.arena.string_lit(label);
        let children = vec_to_list(self.arena, children);
        intern_tag(self.arena, UI_GROUP_TAG, &[kind, label, children])
    }

    #[must_use]
    /// Builds one vertical UI group.
    pub fn vgroup(&mut self, label: &str, children: &[UiId]) -> UiId {
        self.group(UiGroupKind::Vertical, label, children)
    }

    #[must_use]
    /// Builds one horizontal UI group.
    pub fn hgroup(&mut self, label: &str, children: &[UiId]) -> UiId {
        self.group(UiGroupKind::Horizontal, label, children)
    }

    #[must_use]
    /// Builds one tab UI group.
    pub fn tgroup(&mut self, label: &str, children: &[UiId]) -> UiId {
        self.group(UiGroupKind::Tab, label, children)
    }

    #[must_use]
    /// Builds one input-control leaf referencing a stable [`ControlId`].
    pub fn input_control(&mut self, control: ControlId) -> UiId {
        let control = self.arena.int(i64::from(control));
        intern_tag(self.arena, UI_INPUT_CONTROL_TAG, &[control])
    }

    #[must_use]
    /// Builds one output-control leaf referencing a stable [`ControlId`].
    pub fn output_control(&mut self, control: ControlId) -> UiId {
        let control = self.arena.int(i64::from(control));
        intern_tag(self.arena, UI_OUTPUT_CONTROL_TAG, &[control])
    }

    #[must_use]
    /// Builds one soundfile UI leaf referencing a stable [`ControlId`].
    pub fn soundfile(&mut self, control: ControlId) -> UiId {
        let control = self.arena.int(i64::from(control));
        intern_tag(self.arena, UI_SOUNDFILE_TAG, &[control])
    }
}

/// Canonical matcher view for one UI IR node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiMatch<'a> {
    Group {
        kind: UiGroupKind,
        label: &'a str,
        children: Vec<UiId>,
    },
    InputControl(ControlId),
    OutputControl(ControlId),
    Soundfile(ControlId),
    Unknown,
}

/// Decodes one UI IR node into its canonical matcher view.
#[must_use]
pub fn match_ui(arena: &TreeArena, id: UiId) -> UiMatch<'_> {
    let Some(node) = arena.node(id) else {
        return UiMatch::Unknown;
    };
    match (&node.kind, node.children.as_slice()) {
        (NodeKind::Tag(tag), [kind, label, children])
            if arena.tag_name(*tag).unwrap_or("") == UI_GROUP_TAG =>
        {
            let Some(kind) = decode_group_kind(arena, *kind) else {
                return UiMatch::Unknown;
            };
            let Some(label) = decode_label(arena, *label) else {
                return UiMatch::Unknown;
            };
            let Some(children) = list_to_vec(arena, *children) else {
                return UiMatch::Unknown;
            };
            UiMatch::Group {
                kind,
                label,
                children,
            }
        }
        (NodeKind::Tag(tag), [control])
            if arena.tag_name(*tag).unwrap_or("") == UI_INPUT_CONTROL_TAG =>
        {
            decode_control_id(arena, *control).map_or(UiMatch::Unknown, UiMatch::InputControl)
        }
        (NodeKind::Tag(tag), [control])
            if arena.tag_name(*tag).unwrap_or("") == UI_OUTPUT_CONTROL_TAG =>
        {
            decode_control_id(arena, *control).map_or(UiMatch::Unknown, UiMatch::OutputControl)
        }
        (NodeKind::Tag(tag), [control])
            if arena.tag_name(*tag).unwrap_or("") == UI_SOUNDFILE_TAG =>
        {
            decode_control_id(arena, *control).map_or(UiMatch::Unknown, UiMatch::Soundfile)
        }
        _ => UiMatch::Unknown,
    }
}

fn intern_tag(arena: &mut TreeArena, tag: &str, children: &[UiId]) -> UiId {
    let tag_id = arena.intern_tag(tag);
    arena.intern(NodeKind::Tag(tag_id), children)
}

fn decode_group_kind(arena: &TreeArena, id: UiId) -> Option<UiGroupKind> {
    match arena.kind(id) {
        Some(NodeKind::Int(raw)) => UiGroupKind::from_raw(*raw),
        _ => None,
    }
}

fn decode_control_id(arena: &TreeArena, id: UiId) -> Option<ControlId> {
    match arena.kind(id) {
        Some(NodeKind::Int(raw)) => u32::try_from(*raw).ok(),
        _ => None,
    }
}

fn decode_label(arena: &TreeArena, id: UiId) -> Option<&str> {
    match arena.kind(id) {
        Some(NodeKind::StringLiteral(value)) => Some(value),
        Some(NodeKind::Symbol(value)) => Some(value),
        _ => None,
    }
}
