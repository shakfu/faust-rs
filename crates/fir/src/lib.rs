//! FIR construction and matching helpers.
//!
//! # Source provenance (C++)
//! - `compiler/generator/instructions.hh`
//! - `compiler/generator/instructions_type.hh`
//! - `compiler/generator/instructions.cpp`
//! - `compiler/generator/fir/fir_code_checker.hh`
//!
//! # Public API mapping status
//! - Public construction API is [`FirBuilder`], aligned with the canonical
//!   `BoxBuilder` and `SigBuilder` style used in `crates/boxes` and
//!   `crates/signals`.
//! - Public inspection API is [`match_fir`] + [`FirMatch`].
//!
//! # Type model parity notes
//! - `FirType::UI`, `FirType::Sound`, and `FirType::Meta` represent the
//!   C++ FIR API handle layer historically spelled through pointer kinds
//!   (`kUI_ptr`, `kSound_ptr`, `kMeta_ptr`) in `instructions_type.hh`.
//! - Generic pointer nesting remains explicit with `FirType::Ptr(...)`
//!   (for example `FAUSTFLOAT**` is `Ptr(Ptr(FaustFloat))`).
//! - Canonical DSP API signatures should therefore use:
//!   - `metadata(Meta)` (pointer-shaped handle),
//!   - `buildUserInterface(UI)` (pointer-shaped handle),
//!   - `compute(Int32, Ptr(Ptr(FaustFloat)), Ptr(Ptr(FaustFloat)))`.
//!
//! # Parity invariants
//! - FIR nodes are represented as hash-consed trees in `tlib::TreeArena`.
//! - Identical FIR nodes are structurally shared automatically by interning.
//! - FIR value nodes carry explicit result types, so backend passes do not need
//!   a separate type-reconstruction phase.
//! - Dispatch is explicit and exhaustive via `match_fir`, no RTTI/dynamic-cast.

pub mod checker;
pub mod inliner;

use std::collections::HashSet;
use std::fmt::Write as _;

use tlib::{NodeKind, TreeArena, TreeId, tree_to_double, tree_to_int};

pub const CRATE_NAME: &str = "fir";

/// Stable crate identifier used in workspace-level tooling and diagnostics.
#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}

/// FIR node identifier in [`FirStore`].
pub type FirId = TreeId;

/// Memory-access class for FIR variable nodes.
///
/// This is a semantic storage class, not a target-specific address space:
/// backends may map these categories to different concrete layouts as long as
/// lifetime/visibility semantics remain equivalent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AccessType {
    Stack,
    Struct,
    Static,
    FunArgs,
    Loop,
    Global,
}

/// FIR primitive type model.
///
/// FIR keeps result types explicit on value nodes so backend passes can emit
/// code without reconstructing types from context.
#[derive(Clone, Debug, PartialEq)]
pub enum FirType {
    Int32,
    Int64,
    Float32,
    Float64,
    /// Backend-defined scalar sample/control type (`FAUSTFLOAT` in C++).
    FaustFloat,
    Quad,
    FixedPoint,
    Bool,
    Void,
    Obj,
    /// C++ parity note: API handle kind equivalent to `kSound_ptr`.
    ///
    /// Semantics: this variant already models a pointer-shaped handle.
    /// Use `FirType::Ptr(Box::new(FirType::Sound))` only for explicit extra
    /// pointer indirection (for example `Soundfile**`).
    Sound,
    /// C++ parity note: API handle kind equivalent to `kUI_ptr`.
    ///
    /// Semantics: this variant already models a pointer-shaped handle.
    /// Canonical FIR API signatures therefore use `UI` directly
    /// (`buildUserInterface(UI)`), not `Ptr(UI)`.
    UI,
    /// C++ parity note: API handle kind equivalent to `kMeta_ptr`.
    ///
    /// Semantics: this variant already models a pointer-shaped handle.
    /// Use `Ptr(Meta)` only when an additional pointer level is intended.
    Meta,
    /// Generic explicit pointer constructor.
    ///
    /// This is used for pointer depth that is semantically relevant in FIR
    /// values/signatures (for example `FAUSTFLOAT**` in `compute`), while
    /// `UI`/`Sound`/`Meta` already encode their base API-handle pointer level.
    Ptr(Box<FirType>),
    Array(Box<FirType>, usize),
    Vector(Box<FirType>, usize),
    Struct(String, Vec<FirType>),
    Fun {
        args: Vec<FirType>,
        ret: Box<FirType>,
    },
}

/// FIR binary operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FirBinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    And,
    Or,
    Xor,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Canonical FIR math operation identifiers used by backend-agnostic lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FirMathOp {
    Pow,
    Min,
    Max,
    Sin,
    Cos,
    Acos,
    Asin,
    Atan,
    Atan2,
    Tan,
    Exp,
    Log,
    Log10,
    Sqrt,
    Abs,
    Fmod,
    Remainder,
    Floor,
    Ceil,
    Rint,
    Round,
}

impl FirMathOp {
    /// Returns the backend-agnostic FIR symbol used for this operation.
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Pow => "pow",
            Self::Min => "fmin",
            Self::Max => "fmax",
            Self::Sin => "sin",
            Self::Cos => "cos",
            Self::Acos => "acos",
            Self::Asin => "asin",
            Self::Atan => "atan",
            Self::Atan2 => "atan2",
            Self::Tan => "tan",
            Self::Exp => "exp",
            Self::Log => "log",
            Self::Log10 => "log10",
            Self::Sqrt => "sqrt",
            Self::Abs => "fabs",
            Self::Fmod => "fmod",
            Self::Remainder => "remainder",
            Self::Floor => "floor",
            Self::Ceil => "ceil",
            Self::Rint => "rint",
            Self::Round => "round",
        }
    }

    /// Parses a FIR math symbol, accepting both canonical and `std::` forms.
    #[must_use]
    pub fn from_symbol(name: &str) -> Option<Self> {
        let symbol = name.strip_prefix("std::").unwrap_or(name);
        match symbol {
            "pow" => Some(Self::Pow),
            "fmin" => Some(Self::Min),
            "fmax" => Some(Self::Max),
            "sin" => Some(Self::Sin),
            "cos" => Some(Self::Cos),
            "acos" => Some(Self::Acos),
            "asin" => Some(Self::Asin),
            "atan" => Some(Self::Atan),
            "atan2" => Some(Self::Atan2),
            "tan" => Some(Self::Tan),
            "exp" => Some(Self::Exp),
            "log" => Some(Self::Log),
            "log10" => Some(Self::Log10),
            "sqrt" => Some(Self::Sqrt),
            "fabs" => Some(Self::Abs),
            "fmod" => Some(Self::Fmod),
            "remainder" => Some(Self::Remainder),
            "floor" => Some(Self::Floor),
            "ceil" => Some(Self::Ceil),
            "rint" => Some(Self::Rint),
            "round" => Some(Self::Round),
            _ => None,
        }
    }
}

/// UI box orientation for FIR UI instructions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UiBoxType {
    Vertical,
    Horizontal,
    Tab,
}

/// FIR UI button kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ButtonType {
    Button,
    Checkbox,
}

/// FIR UI slider kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SliderType {
    Horizontal,
    Vertical,
    NumEntry,
}

/// FIR UI bargraph kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BargraphType {
    Horizontal,
    Vertical,
}

/// Slider range/value payload used by FIR UI slider instructions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderRange {
    /// Initial widget value.
    pub init: f64,
    /// Lower bound.
    pub lo: f64,
    /// Upper bound.
    pub hi: f64,
    /// Step increment.
    pub step: f64,
}

/// Named type used for function signatures.
#[derive(Clone, Debug, PartialEq)]
pub struct NamedType {
    /// Source-level or ABI-level item name.
    pub name: String,
    /// Declared FIR type for the named item.
    pub typ: FirType,
}

/// FIR storage using `tlib::TreeArena` hash-consing.
///
/// `FirId`s are store-local handles. They must not be mixed across stores
/// without explicit rebuilding or cloning through a dedicated helper.
#[derive(Debug)]
pub struct FirStore {
    arena: TreeArena,
}

impl Default for FirStore {
    fn default() -> Self {
        Self::new()
    }
}

impl FirStore {
    /// Creates a new instance of this type.
    #[must_use]
    pub fn new() -> Self {
        Self {
            arena: TreeArena::new(),
        }
    }

    /// Returns the number of elements currently stored.
    #[must_use]
    pub fn len(&self) -> usize {
        self.arena.len()
    }

    /// Returns `true` when there are no FIR nodes besides canonical `nil`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.arena.len() <= 1
    }

    /// Returns the value type when `id` points to a value node.
    #[must_use]
    pub fn value_type(&self, id: FirId) -> Option<FirType> {
        let node = self.arena.node(id)?;
        let NodeKind::Tag(tag_id) = &node.kind else {
            return None;
        };
        let tag = self.arena.tag_name(*tag_id)?;
        if !is_value_tag(tag) {
            return None;
        }
        let typ_id = *node.children.as_slice().first()?;
        decode_type(&self.arena, typ_id)
    }
}

/// Canonical builder API for constructing FIR nodes.
///
/// Builder methods create the normalized node shapes expected by `match_fir`
/// and downstream backends, including explicit types on value nodes and stable
/// encodings for lists and declarations.
pub struct FirBuilder<'a> {
    store: &'a mut FirStore,
}

impl<'a> FirBuilder<'a> {
    #[must_use]
    /// Creates a new instance of this type.
    pub fn new(store: &'a mut FirStore) -> Self {
        Self { store }
    }

    /// C++ parity: `Int32NumInst`.
    #[must_use]
    pub fn int32(&mut self, value: i32) -> FirId {
        let typ = encode_type(&mut self.store.arena, &FirType::Int32);
        let val = self.store.arena.int(i64::from(value));
        intern_tag(&mut self.store.arena, FIR_V_INT32_TAG, &[typ, val])
    }

    /// C++ parity: `Int64NumInst`.
    #[must_use]
    pub fn int64(&mut self, value: i64) -> FirId {
        let typ = encode_type(&mut self.store.arena, &FirType::Int64);
        let val = self.store.arena.int(value);
        intern_tag(&mut self.store.arena, FIR_V_INT64_TAG, &[typ, val])
    }

    /// C++ parity: `FloatNumInst`.
    #[must_use]
    pub fn float32(&mut self, value: f32) -> FirId {
        let typ = encode_type(&mut self.store.arena, &FirType::Float32);
        let bits = self.store.arena.int(i64::from(value.to_bits()));
        intern_tag(&mut self.store.arena, FIR_V_FLOAT32_TAG, &[typ, bits])
    }

    /// C++ parity: `DoubleNumInst`.
    #[must_use]
    pub fn float64(&mut self, value: f64) -> FirId {
        let typ = encode_type(&mut self.store.arena, &FirType::Float64);
        let val = self.store.arena.float(value);
        intern_tag(&mut self.store.arena, FIR_V_FLOAT64_TAG, &[typ, val])
    }

    /// C++ parity: `BoolNumInst`.
    #[must_use]
    pub fn bool_(&mut self, value: bool) -> FirId {
        let typ = encode_type(&mut self.store.arena, &FirType::Bool);
        let val = self.store.arena.int(if value { 1 } else { 0 });
        intern_tag(&mut self.store.arena, FIR_V_BOOL_TAG, &[typ, val])
    }

    /// C++ parity: `QuadNumInst`.
    #[must_use]
    pub fn quad(&mut self, value: f64) -> FirId {
        let typ = encode_type(&mut self.store.arena, &FirType::Quad);
        let val = self.store.arena.float(value);
        intern_tag(&mut self.store.arena, FIR_V_QUAD_TAG, &[typ, val])
    }

    /// C++ parity: `FixedPointNumInst`.
    #[must_use]
    pub fn fixed_point(&mut self, value: f64) -> FirId {
        let typ = encode_type(&mut self.store.arena, &FirType::FixedPoint);
        let val = self.store.arena.float(value);
        intern_tag(&mut self.store.arena, FIR_V_FIXED_POINT_TAG, &[typ, val])
    }

    /// C++ parity: `ValueArrayInst`.
    #[must_use]
    pub fn value_array(&mut self, values: &[FirId], typ: FirType) -> FirId {
        let typ_id = encode_type(&mut self.store.arena, &typ);
        let values_id = encode_list(&mut self.store.arena, values);
        intern_tag(
            &mut self.store.arena,
            FIR_V_VALUE_ARRAY_TAG,
            &[typ_id, values_id],
        )
    }

    /// C++ parity: `Int32ArrayNumInst`.
    #[must_use]
    pub fn int32_array(&mut self, values: &[i32]) -> FirId {
        let typ = encode_type(
            &mut self.store.arena,
            &FirType::Array(Box::new(FirType::Int32), values.len()),
        );
        let value_ids: Vec<_> = values
            .iter()
            .map(|v| self.store.arena.int(i64::from(*v)))
            .collect();
        let values_id = encode_list(&mut self.store.arena, &value_ids);
        intern_tag(
            &mut self.store.arena,
            FIR_V_INT32_ARRAY_TAG,
            &[typ, values_id],
        )
    }

    /// C++ parity: `FloatArrayNumInst`.
    #[must_use]
    pub fn float32_array(&mut self, values: &[f32]) -> FirId {
        let typ = encode_type(
            &mut self.store.arena,
            &FirType::Array(Box::new(FirType::Float32), values.len()),
        );
        let value_ids: Vec<_> = values
            .iter()
            .map(|v| self.store.arena.int(i64::from(v.to_bits())))
            .collect();
        let values_id = encode_list(&mut self.store.arena, &value_ids);
        intern_tag(
            &mut self.store.arena,
            FIR_V_FLOAT32_ARRAY_TAG,
            &[typ, values_id],
        )
    }

    /// C++ parity: `DoubleArrayNumInst`.
    #[must_use]
    pub fn float64_array(&mut self, values: &[f64]) -> FirId {
        let typ = encode_type(
            &mut self.store.arena,
            &FirType::Array(Box::new(FirType::Float64), values.len()),
        );
        let value_ids: Vec<_> = values.iter().map(|v| self.store.arena.float(*v)).collect();
        let values_id = encode_list(&mut self.store.arena, &value_ids);
        intern_tag(
            &mut self.store.arena,
            FIR_V_FLOAT64_ARRAY_TAG,
            &[typ, values_id],
        )
    }

    /// C++ parity: `QuadArrayNumInst`.
    #[must_use]
    pub fn quad_array(&mut self, values: &[f64]) -> FirId {
        let typ = encode_type(
            &mut self.store.arena,
            &FirType::Array(Box::new(FirType::Quad), values.len()),
        );
        let value_ids: Vec<_> = values.iter().map(|v| self.store.arena.float(*v)).collect();
        let values_id = encode_list(&mut self.store.arena, &value_ids);
        intern_tag(
            &mut self.store.arena,
            FIR_V_QUAD_ARRAY_TAG,
            &[typ, values_id],
        )
    }

    /// C++ parity: `FixedPointArrayNumInst`.
    #[must_use]
    pub fn fixed_point_array(&mut self, values: &[f64]) -> FirId {
        let typ = encode_type(
            &mut self.store.arena,
            &FirType::Array(Box::new(FirType::FixedPoint), values.len()),
        );
        let value_ids: Vec<_> = values.iter().map(|v| self.store.arena.float(*v)).collect();
        let values_id = encode_list(&mut self.store.arena, &value_ids);
        intern_tag(
            &mut self.store.arena,
            FIR_V_FIXED_POINT_ARRAY_TAG,
            &[typ, values_id],
        )
    }

    /// C++ parity: `LoadVarInst`.
    #[must_use]
    pub fn load_var(&mut self, name: impl Into<String>, access: AccessType, typ: FirType) -> FirId {
        let typ_id = encode_type(&mut self.store.arena, &typ);
        let name_id = self.store.arena.symbol(name);
        let access_id = encode_access(&mut self.store.arena, access);
        intern_tag(
            &mut self.store.arena,
            FIR_V_LOAD_VAR_TAG,
            &[typ_id, name_id, access_id],
        )
    }

    /// C++ parity helper: explicit table read expression.
    #[must_use]
    pub fn load_table(
        &mut self,
        name: impl Into<String>,
        access: AccessType,
        index: FirId,
        typ: FirType,
    ) -> FirId {
        let typ_id = encode_type(&mut self.store.arena, &typ);
        let name_id = self.store.arena.symbol(name);
        let access_id = encode_access(&mut self.store.arena, access);
        intern_tag(
            &mut self.store.arena,
            FIR_V_LOAD_TABLE_TAG,
            &[typ_id, name_id, access_id, index],
        )
    }

    /// C++ parity: `LoadVarAddressInst`.
    #[must_use]
    pub fn load_var_address(
        &mut self,
        name: impl Into<String>,
        access: AccessType,
        typ: FirType,
    ) -> FirId {
        let typ_id = encode_type(&mut self.store.arena, &typ);
        let name_id = self.store.arena.symbol(name);
        let access_id = encode_access(&mut self.store.arena, access);
        intern_tag(
            &mut self.store.arena,
            FIR_V_LOAD_VAR_ADDRESS_TAG,
            &[typ_id, name_id, access_id],
        )
    }

    /// C++ parity: `TeeVarInst`.
    #[must_use]
    pub fn tee_var(
        &mut self,
        name: impl Into<String>,
        access: AccessType,
        value: FirId,
        typ: FirType,
    ) -> FirId {
        let typ_id = encode_type(&mut self.store.arena, &typ);
        let name_id = self.store.arena.symbol(name);
        let access_id = encode_access(&mut self.store.arena, access);
        intern_tag(
            &mut self.store.arena,
            FIR_V_TEE_VAR_TAG,
            &[typ_id, name_id, access_id, value],
        )
    }

    /// C++ parity: `BinopInst`.
    #[must_use]
    pub fn binop(&mut self, op: FirBinOp, lhs: FirId, rhs: FirId, typ: FirType) -> FirId {
        let typ_id = encode_type(&mut self.store.arena, &typ);
        let op_id = encode_binop(&mut self.store.arena, op);
        intern_tag(
            &mut self.store.arena,
            FIR_V_BINOP_TAG,
            &[typ_id, op_id, lhs, rhs],
        )
    }

    /// C++ parity: `NegInst`.
    #[must_use]
    pub fn neg(&mut self, value: FirId, typ: FirType) -> FirId {
        let typ_id = encode_type(&mut self.store.arena, &typ);
        intern_tag(&mut self.store.arena, FIR_V_NEG_TAG, &[typ_id, value])
    }

    /// C++ parity: `CastInst`.
    #[must_use]
    pub fn cast(&mut self, typ: FirType, value: FirId) -> FirId {
        let typ_id = encode_type(&mut self.store.arena, &typ);
        intern_tag(&mut self.store.arena, FIR_V_CAST_TAG, &[typ_id, value])
    }

    /// C++ parity: `BitcastInst`.
    #[must_use]
    pub fn bitcast(&mut self, typ: FirType, value: FirId) -> FirId {
        let typ_id = encode_type(&mut self.store.arena, &typ);
        intern_tag(&mut self.store.arena, FIR_V_BITCAST_TAG, &[typ_id, value])
    }

    /// C++ parity: `Select2Inst`.
    #[must_use]
    pub fn select2(
        &mut self,
        cond: FirId,
        then_value: FirId,
        else_value: FirId,
        typ: FirType,
    ) -> FirId {
        let typ_id = encode_type(&mut self.store.arena, &typ);
        intern_tag(
            &mut self.store.arena,
            FIR_V_SELECT2_TAG,
            &[typ_id, cond, then_value, else_value],
        )
    }

    /// C++ parity: `FunCallInst`.
    #[must_use]
    pub fn fun_call(&mut self, name: impl Into<String>, args: &[FirId], typ: FirType) -> FirId {
        let typ_id = encode_type(&mut self.store.arena, &typ);
        let name_id = self.store.arena.symbol(name);
        let args_id = encode_list(&mut self.store.arena, args);
        intern_tag(
            &mut self.store.arena,
            FIR_V_FUNCALL_TAG,
            &[typ_id, name_id, args_id],
        )
    }

    /// C++ parity helper: typed math call that avoids stringly-typed lowering sites.
    #[must_use]
    pub fn math_call(&mut self, op: FirMathOp, args: &[FirId], typ: FirType) -> FirId {
        self.fun_call(op.symbol(), args, typ)
    }

    /// C++ parity: `NullValueInst`.
    #[must_use]
    pub fn null_value(&mut self, typ: FirType) -> FirId {
        let typ_id = encode_type(&mut self.store.arena, &typ);
        intern_tag(&mut self.store.arena, FIR_V_NULL_TAG, &[typ_id])
    }

    /// C++ parity: `NewDSPInst`.
    #[must_use]
    pub fn new_dsp(&mut self, name: impl Into<String>, typ: FirType) -> FirId {
        let typ_id = encode_type(&mut self.store.arena, &typ);
        let name_id = self.store.arena.symbol(name);
        intern_tag(&mut self.store.arena, FIR_V_NEW_DSP_TAG, &[typ_id, name_id])
    }

    /// C++ parity: `DeclareVarInst`.
    #[must_use]
    pub fn declare_var(
        &mut self,
        name: impl Into<String>,
        typ: FirType,
        access: AccessType,
        init: Option<FirId>,
    ) -> FirId {
        let name_id = self.store.arena.symbol(name);
        let typ_id = encode_type(&mut self.store.arena, &typ);
        let access_id = encode_access(&mut self.store.arena, access);
        let init_id = init.unwrap_or_else(|| self.store.arena.nil());
        intern_tag(
            &mut self.store.arena,
            FIR_DECLARE_VAR_TAG,
            &[name_id, typ_id, access_id, init_id],
        )
    }

    /// C++ parity helper: explicit table declaration with literal initial values.
    #[must_use]
    pub fn declare_table(
        &mut self,
        name: impl Into<String>,
        access: AccessType,
        elem_type: FirType,
        values: &[FirId],
    ) -> FirId {
        let name_id = self.store.arena.symbol(name);
        let access_id = encode_access(&mut self.store.arena, access);
        let typ_id = encode_type(&mut self.store.arena, &elem_type);
        let values_id = encode_list(&mut self.store.arena, values);
        intern_tag(
            &mut self.store.arena,
            FIR_DECLARE_TABLE_TAG,
            &[name_id, access_id, typ_id, values_id],
        )
    }

    /// C++ parity: `DeclareFunInst`.
    ///
    /// Pass `body: Some(id)` for a full function definition or `body: None` for
    /// a pure prototype (forward declaration / pure-virtual equivalent).
    #[must_use]
    pub fn declare_fun(
        &mut self,
        name: impl Into<String>,
        typ: FirType,
        args: &[NamedType],
        body: Option<FirId>,
        is_inline: bool,
    ) -> FirId {
        let name_id = self.store.arena.symbol(name);
        let typ_id = encode_type(&mut self.store.arena, &typ);
        let args_id = encode_named_types(&mut self.store.arena, args);
        let inline_id = self.store.arena.int(if is_inline { 1 } else { 0 });
        match body {
            Some(body_id) => intern_tag(
                &mut self.store.arena,
                FIR_DECLARE_FUN_TAG,
                &[name_id, typ_id, args_id, body_id, inline_id],
            ),
            None => intern_tag(
                &mut self.store.arena,
                FIR_DECLARE_FUN_PROTO_TAG,
                &[name_id, typ_id, args_id, inline_id],
            ),
        }
    }

    /// C++ parity: `DeclareStructTypeInst`.
    #[must_use]
    pub fn declare_struct_type(&mut self, typ: FirType) -> FirId {
        let typ_id = encode_type(&mut self.store.arena, &typ);
        intern_tag(
            &mut self.store.arena,
            FIR_DECLARE_STRUCT_TYPE_TAG,
            &[typ_id],
        )
    }

    /// C++ parity: `DeclareBufferIterators`.
    #[must_use]
    pub fn declare_buffer_iterators(
        &mut self,
        name1: impl Into<String>,
        name2: impl Into<String>,
        channels: i32,
        typ: FirType,
        mutable: bool,
        chunk: bool,
    ) -> FirId {
        let name1_id = self.store.arena.symbol(name1);
        let name2_id = self.store.arena.symbol(name2);
        let channels_id = self.store.arena.int(i64::from(channels));
        let typ_id = encode_type(&mut self.store.arena, &typ);
        let mutable_id = self.store.arena.int(if mutable { 1 } else { 0 });
        let chunk_id = self.store.arena.int(if chunk { 1 } else { 0 });
        intern_tag(
            &mut self.store.arena,
            FIR_DECLARE_BUFFER_ITERATORS_TAG,
            &[
                name1_id,
                name2_id,
                channels_id,
                typ_id,
                mutable_id,
                chunk_id,
            ],
        )
    }

    /// C++ parity: `StoreVarInst`.
    #[must_use]
    pub fn store_var(
        &mut self,
        name: impl Into<String>,
        access: AccessType,
        value: FirId,
    ) -> FirId {
        let name_id = self.store.arena.symbol(name);
        let access_id = encode_access(&mut self.store.arena, access);
        intern_tag(
            &mut self.store.arena,
            FIR_STORE_VAR_TAG,
            &[name_id, access_id, value],
        )
    }

    /// C++ parity helper: explicit table write statement.
    #[must_use]
    pub fn store_table(
        &mut self,
        name: impl Into<String>,
        access: AccessType,
        index: FirId,
        value: FirId,
    ) -> FirId {
        let name_id = self.store.arena.symbol(name);
        let access_id = encode_access(&mut self.store.arena, access);
        intern_tag(
            &mut self.store.arena,
            FIR_STORE_TABLE_TAG,
            &[name_id, access_id, index, value],
        )
    }

    /// C++ parity: `ShiftArrayVarInst`.
    #[must_use]
    pub fn shift_array_var(
        &mut self,
        name: impl Into<String>,
        access: AccessType,
        delay: i32,
    ) -> FirId {
        let name_id = self.store.arena.symbol(name);
        let access_id = encode_access(&mut self.store.arena, access);
        let delay_id = self.store.arena.int(i64::from(delay));
        intern_tag(
            &mut self.store.arena,
            FIR_SHIFT_ARRAY_VAR_TAG,
            &[name_id, access_id, delay_id],
        )
    }

    /// C++ parity: `DropInst`.
    #[must_use]
    pub fn drop_(&mut self, value: FirId) -> FirId {
        intern_tag(&mut self.store.arena, FIR_DROP_TAG, &[value])
    }

    /// C++ parity: `NullStatementInst`.
    #[must_use]
    pub fn null_statement(&mut self) -> FirId {
        intern_tag(&mut self.store.arena, FIR_NULL_STATEMENT_TAG, &[])
    }

    /// C++ parity: `RetInst`.
    #[must_use]
    pub fn ret(&mut self, value: Option<FirId>) -> FirId {
        let value_id = value.unwrap_or_else(|| self.store.arena.nil());
        intern_tag(&mut self.store.arena, FIR_RETURN_TAG, &[value_id])
    }

    /// C++ parity: `BlockInst`.
    #[must_use]
    pub fn block(&mut self, body: &[FirId]) -> FirId {
        let list = encode_list(&mut self.store.arena, body);
        intern_tag(&mut self.store.arena, FIR_BLOCK_TAG, &[list])
    }

    /// C++ parity: `IfInst`.
    #[must_use]
    pub fn if_(&mut self, cond: FirId, then_block: FirId, else_block: Option<FirId>) -> FirId {
        let else_id = else_block.unwrap_or_else(|| self.store.arena.nil());
        intern_tag(
            &mut self.store.arena,
            FIR_IF_TAG,
            &[cond, then_block, else_id],
        )
    }

    /// C++ parity: `ControlInst`.
    #[must_use]
    pub fn control(&mut self, cond: FirId, stmt: FirId) -> FirId {
        intern_tag(&mut self.store.arena, FIR_CONTROL_TAG, &[cond, stmt])
    }

    /// C++ parity: `ForLoopInst`.
    #[must_use]
    pub fn for_loop(
        &mut self,
        var: impl Into<String>,
        init: FirId,
        end: FirId,
        step: FirId,
        body: FirId,
        is_reverse: bool,
    ) -> FirId {
        let var_id = self.store.arena.symbol(var);
        let reverse = self.store.arena.int(if is_reverse { 1 } else { 0 });
        intern_tag(
            &mut self.store.arena,
            FIR_FOR_LOOP_TAG,
            &[var_id, init, end, step, body, reverse],
        )
    }

    /// C++ parity: `SimpleForLoopInst`.
    #[must_use]
    pub fn simple_for_loop(
        &mut self,
        var: impl Into<String>,
        upper: FirId,
        body: FirId,
        is_reverse: bool,
    ) -> FirId {
        let var_id = self.store.arena.symbol(var);
        let reverse = self.store.arena.int(if is_reverse { 1 } else { 0 });
        intern_tag(
            &mut self.store.arena,
            FIR_SIMPLE_FOR_LOOP_TAG,
            &[var_id, upper, body, reverse],
        )
    }

    /// C++ parity: `IteratorForLoopInst`.
    #[must_use]
    pub fn iterator_for_loop(
        &mut self,
        iterators: &[&str],
        is_reverse: bool,
        body: FirId,
    ) -> FirId {
        let iter_ids: Vec<_> = iterators
            .iter()
            .map(|name| self.store.arena.symbol(*name))
            .collect();
        let iter_list = encode_list(&mut self.store.arena, &iter_ids);
        let reverse = self.store.arena.int(if is_reverse { 1 } else { 0 });
        intern_tag(
            &mut self.store.arena,
            FIR_ITERATOR_FOR_LOOP_TAG,
            &[iter_list, reverse, body],
        )
    }

    /// C++ parity: `WhileLoopInst`.
    #[must_use]
    pub fn while_loop(&mut self, cond: FirId, body: FirId) -> FirId {
        intern_tag(&mut self.store.arena, FIR_WHILE_LOOP_TAG, &[cond, body])
    }

    /// C++ parity: `SwitchInst`.
    #[must_use]
    pub fn switch(&mut self, cond: FirId, cases: &[(i64, FirId)], default: Option<FirId>) -> FirId {
        let cases_id = encode_switch_cases(&mut self.store.arena, cases);
        let default_id = default.unwrap_or_else(|| self.store.arena.nil());
        intern_tag(
            &mut self.store.arena,
            FIR_SWITCH_TAG,
            &[cond, cases_id, default_id],
        )
    }

    /// C++ parity: `ModuleInst`.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn module(
        &mut self,
        num_inputs: usize,
        num_outputs: usize,
        name: impl Into<String>,
        dsp_struct: FirId,
        globals: FirId,
        functions: FirId,
        static_decls: FirId,
    ) -> FirId {
        let name_id = self.store.arena.symbol(name);
        let num_inputs_id = self.store.arena.int(num_inputs as i64);
        let num_outputs_id = self.store.arena.int(num_outputs as i64);
        intern_tag(
            &mut self.store.arena,
            FIR_MODULE_TAG,
            &[
                num_inputs_id,
                num_outputs_id,
                name_id,
                dsp_struct,
                globals,
                functions,
                static_decls,
            ],
        )
    }

    /// C++ parity: `OpenboxInst`.
    #[must_use]
    pub fn open_box(&mut self, typ: UiBoxType, label: impl Into<String>) -> FirId {
        let typ_id = encode_ui_box_type(&mut self.store.arena, typ);
        let label_id = self.store.arena.symbol(label);
        intern_tag(&mut self.store.arena, FIR_OPEN_BOX_TAG, &[typ_id, label_id])
    }

    /// C++ parity: `CloseboxInst`.
    #[must_use]
    pub fn close_box(&mut self) -> FirId {
        intern_tag(&mut self.store.arena, FIR_CLOSE_BOX_TAG, &[])
    }

    /// C++ parity: `AddButtonInst`.
    #[must_use]
    pub fn add_button(
        &mut self,
        typ: ButtonType,
        label: impl Into<String>,
        var: impl Into<String>,
    ) -> FirId {
        let typ_id = encode_button_type(&mut self.store.arena, typ);
        let label_id = self.store.arena.symbol(label);
        let var_id = self.store.arena.symbol(var);
        intern_tag(
            &mut self.store.arena,
            FIR_ADD_BUTTON_TAG,
            &[typ_id, label_id, var_id],
        )
    }

    /// C++ parity: `AddSliderInst`.
    #[must_use]
    pub fn add_slider(
        &mut self,
        typ: SliderType,
        label: impl Into<String>,
        var: impl Into<String>,
        range: SliderRange,
    ) -> FirId {
        let typ_id = encode_slider_type(&mut self.store.arena, typ);
        let label_id = self.store.arena.symbol(label);
        let var_id = self.store.arena.symbol(var);
        let init_id = self.store.arena.float(range.init);
        let lo_id = self.store.arena.float(range.lo);
        let hi_id = self.store.arena.float(range.hi);
        let step_id = self.store.arena.float(range.step);
        intern_tag(
            &mut self.store.arena,
            FIR_ADD_SLIDER_TAG,
            &[typ_id, label_id, var_id, init_id, lo_id, hi_id, step_id],
        )
    }

    /// C++ parity: `AddBargraphInst`.
    #[must_use]
    pub fn add_bargraph(
        &mut self,
        typ: BargraphType,
        label: impl Into<String>,
        var: impl Into<String>,
        lo: f64,
        hi: f64,
    ) -> FirId {
        let typ_id = encode_bargraph_type(&mut self.store.arena, typ);
        let label_id = self.store.arena.symbol(label);
        let var_id = self.store.arena.symbol(var);
        let lo_id = self.store.arena.float(lo);
        let hi_id = self.store.arena.float(hi);
        intern_tag(
            &mut self.store.arena,
            FIR_ADD_BARGRAPH_TAG,
            &[typ_id, label_id, var_id, lo_id, hi_id],
        )
    }

    /// C++ parity: compatibility helper for `AddSoundfileInst` when URL is not provided.
    #[must_use]
    pub fn add_soundfile(&mut self, label: impl Into<String>, var: impl Into<String>) -> FirId {
        self.add_soundfile_with_url(label, "", var)
    }

    /// C++ parity: `AddSoundfileInst`.
    #[must_use]
    pub fn add_soundfile_with_url(
        &mut self,
        label: impl Into<String>,
        url: impl Into<String>,
        var: impl Into<String>,
    ) -> FirId {
        let label_id = self.store.arena.symbol(label);
        let url_id = self.store.arena.symbol(url);
        let var_id = self.store.arena.symbol(var);
        intern_tag(
            &mut self.store.arena,
            FIR_ADD_SOUNDFILE_TAG,
            &[label_id, url_id, var_id],
        )
    }

    /// C++ parity: `AddMetaDeclareInst`.
    #[must_use]
    pub fn add_meta_declare(
        &mut self,
        var: impl Into<String>,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> FirId {
        let var_id = self.store.arena.symbol(var);
        let key_id = self.store.arena.symbol(key);
        let value_id = self.store.arena.symbol(value);
        intern_tag(
            &mut self.store.arena,
            FIR_ADD_META_DECLARE_TAG,
            &[var_id, key_id, value_id],
        )
    }

    /// C++ parity: `LabelInst`.
    #[must_use]
    pub fn label(&mut self, label: impl Into<String>) -> FirId {
        let label_id = self.store.arena.symbol(label);
        intern_tag(&mut self.store.arena, FIR_LABEL_TAG, &[label_id])
    }
}

/// FIR structural matcher result.
#[derive(Clone, Debug, PartialEq)]
pub enum FirMatch {
    Unknown,
    Int32 {
        value: i32,
        typ: FirType,
    },
    Int64 {
        value: i64,
        typ: FirType,
    },
    Float32 {
        value: f32,
        typ: FirType,
    },
    Float64 {
        value: f64,
        typ: FirType,
    },
    Bool {
        value: bool,
        typ: FirType,
    },
    Quad {
        value: f64,
        typ: FirType,
    },
    FixedPoint {
        value: f64,
        typ: FirType,
    },
    ValueArray {
        values: Vec<FirId>,
        typ: FirType,
    },
    Int32Array {
        values: Vec<i32>,
        typ: FirType,
    },
    Float32Array {
        values: Vec<f32>,
        typ: FirType,
    },
    Float64Array {
        values: Vec<f64>,
        typ: FirType,
    },
    QuadArray {
        values: Vec<f64>,
        typ: FirType,
    },
    FixedPointArray {
        values: Vec<f64>,
        typ: FirType,
    },
    LoadVar {
        name: String,
        access: AccessType,
        typ: FirType,
    },
    LoadTable {
        name: String,
        access: AccessType,
        index: FirId,
        typ: FirType,
    },
    LoadVarAddress {
        name: String,
        access: AccessType,
        typ: FirType,
    },
    TeeVar {
        name: String,
        access: AccessType,
        value: FirId,
        typ: FirType,
    },
    BinOp {
        op: FirBinOp,
        lhs: FirId,
        rhs: FirId,
        typ: FirType,
    },
    Neg {
        value: FirId,
        typ: FirType,
    },
    Cast {
        typ: FirType,
        value: FirId,
    },
    Bitcast {
        typ: FirType,
        value: FirId,
    },
    Select2 {
        cond: FirId,
        then_value: FirId,
        else_value: FirId,
        typ: FirType,
    },
    FunCall {
        name: String,
        args: Vec<FirId>,
        typ: FirType,
    },
    NullValue {
        typ: FirType,
    },
    NewDsp {
        name: String,
        typ: FirType,
    },
    DeclareVar {
        name: String,
        typ: FirType,
        access: AccessType,
        init: Option<FirId>,
    },
    DeclareTable {
        name: String,
        access: AccessType,
        elem_type: FirType,
        values: Vec<FirId>,
    },
    DeclareFun {
        name: String,
        typ: FirType,
        args: Vec<NamedType>,
        /// `None` when this is a prototype-only declaration (no body).
        body: Option<FirId>,
        is_inline: bool,
    },
    DeclareStructType {
        typ: FirType,
    },
    DeclareBufferIterators {
        name1: String,
        name2: String,
        channels: i32,
        typ: FirType,
        mutable: bool,
        chunk: bool,
    },
    StoreVar {
        name: String,
        access: AccessType,
        value: FirId,
    },
    StoreTable {
        name: String,
        access: AccessType,
        index: FirId,
        value: FirId,
    },
    ShiftArrayVar {
        name: String,
        access: AccessType,
        delay: i32,
    },
    Drop(FirId),
    NullStatement,
    Return(Option<FirId>),
    Block(Vec<FirId>),
    If {
        cond: FirId,
        then_block: FirId,
        else_block: Option<FirId>,
    },
    Control {
        cond: FirId,
        stmt: FirId,
    },
    ForLoop {
        var: String,
        init: FirId,
        end: FirId,
        step: FirId,
        body: FirId,
        is_reverse: bool,
    },
    SimpleForLoop {
        var: String,
        upper: FirId,
        body: FirId,
        is_reverse: bool,
    },
    IteratorForLoop {
        iterators: Vec<String>,
        is_reverse: bool,
        body: FirId,
    },
    WhileLoop {
        cond: FirId,
        body: FirId,
    },
    Switch {
        cond: FirId,
        cases: Vec<(i64, FirId)>,
        default: Option<FirId>,
    },
    OpenBox {
        typ: UiBoxType,
        label: String,
    },
    CloseBox,
    AddButton {
        typ: ButtonType,
        label: String,
        var: String,
    },
    AddSlider {
        typ: SliderType,
        label: String,
        var: String,
        init: f64,
        lo: f64,
        hi: f64,
        step: f64,
    },
    AddBargraph {
        typ: BargraphType,
        label: String,
        var: String,
        lo: f64,
        hi: f64,
    },
    AddSoundfile {
        label: String,
        url: String,
        var: String,
    },
    AddMetaDeclare {
        var: String,
        key: String,
        value: String,
    },
    Label(String),
    Module {
        num_inputs: usize,
        num_outputs: usize,
        name: String,
        dsp_struct: FirId,
        globals: FirId,
        functions: FirId,
        static_decls: FirId,
    },
}

/// Decodes one [`FirId`] into canonical [`FirMatch`] shape.
///
/// This is the only structural decoder other crates should need. Malformed or
/// partially built trees degrade to [`FirMatch::Unknown`] instead of panicking.
#[must_use]
pub fn match_fir(store: &FirStore, id: FirId) -> FirMatch {
    let Some(node) = store.arena.node(id) else {
        return FirMatch::Unknown;
    };
    let NodeKind::Tag(tag_id) = &node.kind else {
        return FirMatch::Unknown;
    };
    let Some(tag) = store.arena.tag_name(*tag_id) else {
        return FirMatch::Unknown;
    };
    let ch = node.children.as_slice();

    match (tag, ch) {
        (FIR_V_INT32_TAG, [typ, v]) => {
            let (Some(typ), Some(value)) = (
                decode_type(&store.arena, *typ),
                decode_i32(&store.arena, *v),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::Int32 { value, typ }
        }
        (FIR_V_INT64_TAG, [typ, v]) => {
            let (Some(typ), Some(value)) = (
                decode_type(&store.arena, *typ),
                decode_i64(&store.arena, *v),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::Int64 { value, typ }
        }
        (FIR_V_FLOAT32_TAG, [typ, bits]) => {
            let (Some(typ), Some(value)) = (
                decode_type(&store.arena, *typ),
                decode_f32_bits(&store.arena, *bits),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::Float32 { value, typ }
        }
        (FIR_V_FLOAT64_TAG, [typ, v]) => {
            let (Some(typ), Some(value)) = (
                decode_type(&store.arena, *typ),
                decode_f64(&store.arena, *v),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::Float64 { value, typ }
        }
        (FIR_V_BOOL_TAG, [typ, v]) => {
            let (Some(typ), Some(value)) = (
                decode_type(&store.arena, *typ),
                decode_bool(&store.arena, *v),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::Bool { value, typ }
        }
        (FIR_V_QUAD_TAG, [typ, v]) => {
            let (Some(typ), Some(value)) = (
                decode_type(&store.arena, *typ),
                decode_f64(&store.arena, *v),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::Quad { value, typ }
        }
        (FIR_V_FIXED_POINT_TAG, [typ, v]) => {
            let (Some(typ), Some(value)) = (
                decode_type(&store.arena, *typ),
                decode_f64(&store.arena, *v),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::FixedPoint { value, typ }
        }
        (FIR_V_VALUE_ARRAY_TAG, [typ, values]) => {
            let (Some(typ), Some(values)) = (
                decode_type(&store.arena, *typ),
                decode_list(&store.arena, *values),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::ValueArray { values, typ }
        }
        (FIR_V_INT32_ARRAY_TAG, [typ, values]) => {
            let (Some(typ), Some(values)) = (
                decode_type(&store.arena, *typ),
                decode_i32_list(&store.arena, *values),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::Int32Array { values, typ }
        }
        (FIR_V_FLOAT32_ARRAY_TAG, [typ, values]) => {
            let (Some(typ), Some(values)) = (
                decode_type(&store.arena, *typ),
                decode_f32_bits_list(&store.arena, *values),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::Float32Array { values, typ }
        }
        (FIR_V_FLOAT64_ARRAY_TAG, [typ, values]) => {
            let (Some(typ), Some(values)) = (
                decode_type(&store.arena, *typ),
                decode_f64_list(&store.arena, *values),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::Float64Array { values, typ }
        }
        (FIR_V_QUAD_ARRAY_TAG, [typ, values]) => {
            let (Some(typ), Some(values)) = (
                decode_type(&store.arena, *typ),
                decode_f64_list(&store.arena, *values),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::QuadArray { values, typ }
        }
        (FIR_V_FIXED_POINT_ARRAY_TAG, [typ, values]) => {
            let (Some(typ), Some(values)) = (
                decode_type(&store.arena, *typ),
                decode_f64_list(&store.arena, *values),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::FixedPointArray { values, typ }
        }
        (FIR_V_LOAD_VAR_TAG, [typ, name, access]) => {
            let (Some(typ), Some(name), Some(access)) = (
                decode_type(&store.arena, *typ),
                decode_symbol(&store.arena, *name),
                decode_access(&store.arena, *access),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::LoadVar { name, access, typ }
        }
        (FIR_V_LOAD_TABLE_TAG, [typ, name, access, index]) => {
            let (Some(typ), Some(name), Some(access)) = (
                decode_type(&store.arena, *typ),
                decode_symbol(&store.arena, *name),
                decode_access(&store.arena, *access),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::LoadTable {
                name,
                access,
                index: *index,
                typ,
            }
        }
        (FIR_V_LOAD_VAR_ADDRESS_TAG, [typ, name, access]) => {
            let (Some(typ), Some(name), Some(access)) = (
                decode_type(&store.arena, *typ),
                decode_symbol(&store.arena, *name),
                decode_access(&store.arena, *access),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::LoadVarAddress { name, access, typ }
        }
        (FIR_V_TEE_VAR_TAG, [typ, name, access, value]) => {
            let (Some(typ), Some(name), Some(access)) = (
                decode_type(&store.arena, *typ),
                decode_symbol(&store.arena, *name),
                decode_access(&store.arena, *access),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::TeeVar {
                name,
                access,
                value: *value,
                typ,
            }
        }
        (FIR_V_BINOP_TAG, [typ, op, lhs, rhs]) => {
            let (Some(typ), Some(op)) = (
                decode_type(&store.arena, *typ),
                decode_binop(&store.arena, *op),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::BinOp {
                op,
                lhs: *lhs,
                rhs: *rhs,
                typ,
            }
        }
        (FIR_V_NEG_TAG, [typ, value]) => {
            let Some(typ) = decode_type(&store.arena, *typ) else {
                return FirMatch::Unknown;
            };
            FirMatch::Neg { value: *value, typ }
        }
        (FIR_V_CAST_TAG, [typ, value]) => {
            let Some(typ) = decode_type(&store.arena, *typ) else {
                return FirMatch::Unknown;
            };
            FirMatch::Cast { typ, value: *value }
        }
        (FIR_V_BITCAST_TAG, [typ, value]) => {
            let Some(typ) = decode_type(&store.arena, *typ) else {
                return FirMatch::Unknown;
            };
            FirMatch::Bitcast { typ, value: *value }
        }
        (FIR_V_SELECT2_TAG, [typ, cond, then_value, else_value]) => {
            let Some(typ) = decode_type(&store.arena, *typ) else {
                return FirMatch::Unknown;
            };
            FirMatch::Select2 {
                cond: *cond,
                then_value: *then_value,
                else_value: *else_value,
                typ,
            }
        }
        (FIR_V_FUNCALL_TAG, [typ, name, args]) => {
            let (Some(typ), Some(name), Some(args)) = (
                decode_type(&store.arena, *typ),
                decode_symbol(&store.arena, *name),
                decode_list(&store.arena, *args),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::FunCall { name, args, typ }
        }
        (FIR_V_NULL_TAG, [typ]) => {
            let Some(typ) = decode_type(&store.arena, *typ) else {
                return FirMatch::Unknown;
            };
            FirMatch::NullValue { typ }
        }
        (FIR_V_NEW_DSP_TAG, [typ, name]) => {
            let (Some(typ), Some(name)) = (
                decode_type(&store.arena, *typ),
                decode_symbol(&store.arena, *name),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::NewDsp { name, typ }
        }
        (FIR_DECLARE_VAR_TAG, [name, typ, access, init]) => {
            let (Some(name), Some(typ), Some(access)) = (
                decode_symbol(&store.arena, *name),
                decode_type(&store.arena, *typ),
                decode_access(&store.arena, *access),
            ) else {
                return FirMatch::Unknown;
            };
            let init = if store.arena.is_nil(*init) {
                None
            } else {
                Some(*init)
            };
            FirMatch::DeclareVar {
                name,
                typ,
                access,
                init,
            }
        }
        (FIR_DECLARE_TABLE_TAG, [name, access, typ, values]) => {
            let (Some(name), Some(access), Some(elem_type), Some(values)) = (
                decode_symbol(&store.arena, *name),
                decode_access(&store.arena, *access),
                decode_type(&store.arena, *typ),
                decode_list(&store.arena, *values),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::DeclareTable {
                name,
                access,
                elem_type,
                values,
            }
        }
        (FIR_DECLARE_FUN_TAG, [name, typ, args, body, is_inline]) => {
            let (Some(name), Some(typ), Some(args), Some(is_inline)) = (
                decode_symbol(&store.arena, *name),
                decode_type(&store.arena, *typ),
                decode_named_types(&store.arena, *args),
                decode_bool(&store.arena, *is_inline),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::DeclareFun {
                name,
                typ,
                args,
                body: Some(*body),
                is_inline,
            }
        }
        (FIR_DECLARE_FUN_PROTO_TAG, [name, typ, args, is_inline]) => {
            let (Some(name), Some(typ), Some(args), Some(is_inline)) = (
                decode_symbol(&store.arena, *name),
                decode_type(&store.arena, *typ),
                decode_named_types(&store.arena, *args),
                decode_bool(&store.arena, *is_inline),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::DeclareFun {
                name,
                typ,
                args,
                body: None,
                is_inline,
            }
        }
        (FIR_DECLARE_STRUCT_TYPE_TAG, [typ]) => {
            let Some(typ) = decode_type(&store.arena, *typ) else {
                return FirMatch::Unknown;
            };
            FirMatch::DeclareStructType { typ }
        }
        (FIR_DECLARE_BUFFER_ITERATORS_TAG, [name1, name2, channels, typ, mutable, chunk]) => {
            let (Some(name1), Some(name2), Some(channels), Some(typ), Some(mutable), Some(chunk)) = (
                decode_symbol(&store.arena, *name1),
                decode_symbol(&store.arena, *name2),
                decode_i32(&store.arena, *channels),
                decode_type(&store.arena, *typ),
                decode_bool(&store.arena, *mutable),
                decode_bool(&store.arena, *chunk),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::DeclareBufferIterators {
                name1,
                name2,
                channels,
                typ,
                mutable,
                chunk,
            }
        }
        (FIR_STORE_VAR_TAG, [name, access, value]) => {
            let (Some(name), Some(access)) = (
                decode_symbol(&store.arena, *name),
                decode_access(&store.arena, *access),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::StoreVar {
                name,
                access,
                value: *value,
            }
        }
        (FIR_STORE_TABLE_TAG, [name, access, index, value]) => {
            let (Some(name), Some(access)) = (
                decode_symbol(&store.arena, *name),
                decode_access(&store.arena, *access),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::StoreTable {
                name,
                access,
                index: *index,
                value: *value,
            }
        }
        (FIR_SHIFT_ARRAY_VAR_TAG, [name, access, delay]) => {
            let (Some(name), Some(access), Some(delay)) = (
                decode_symbol(&store.arena, *name),
                decode_access(&store.arena, *access),
                decode_i32(&store.arena, *delay),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::ShiftArrayVar {
                name,
                access,
                delay,
            }
        }
        (FIR_DROP_TAG, [value]) => FirMatch::Drop(*value),
        (FIR_NULL_STATEMENT_TAG, []) => FirMatch::NullStatement,
        (FIR_RETURN_TAG, [value]) => {
            let value = if store.arena.is_nil(*value) {
                None
            } else {
                Some(*value)
            };
            FirMatch::Return(value)
        }
        (FIR_BLOCK_TAG, [body]) => {
            let Some(body) = decode_list(&store.arena, *body) else {
                return FirMatch::Unknown;
            };
            FirMatch::Block(body)
        }
        (FIR_IF_TAG, [cond, then_block, else_block]) => {
            let else_block = if store.arena.is_nil(*else_block) {
                None
            } else {
                Some(*else_block)
            };
            FirMatch::If {
                cond: *cond,
                then_block: *then_block,
                else_block,
            }
        }
        (FIR_CONTROL_TAG, [cond, stmt]) => FirMatch::Control {
            cond: *cond,
            stmt: *stmt,
        },
        (FIR_FOR_LOOP_TAG, [var, init, end, step, body, is_reverse]) => {
            let (Some(var), Some(is_reverse)) = (
                decode_symbol(&store.arena, *var),
                decode_bool(&store.arena, *is_reverse),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::ForLoop {
                var,
                init: *init,
                end: *end,
                step: *step,
                body: *body,
                is_reverse,
            }
        }
        (FIR_SIMPLE_FOR_LOOP_TAG, [var, upper, body, is_reverse]) => {
            let (Some(var), Some(is_reverse)) = (
                decode_symbol(&store.arena, *var),
                decode_bool(&store.arena, *is_reverse),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::SimpleForLoop {
                var,
                upper: *upper,
                body: *body,
                is_reverse,
            }
        }
        (FIR_ITERATOR_FOR_LOOP_TAG, [iterators, is_reverse, body]) => {
            let (Some(iterators), Some(is_reverse)) = (
                decode_symbols_list(&store.arena, *iterators),
                decode_bool(&store.arena, *is_reverse),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::IteratorForLoop {
                iterators,
                is_reverse,
                body: *body,
            }
        }
        (FIR_WHILE_LOOP_TAG, [cond, body]) => FirMatch::WhileLoop {
            cond: *cond,
            body: *body,
        },
        (FIR_SWITCH_TAG, [cond, cases, default]) => {
            let Some(cases) = decode_switch_cases(&store.arena, *cases) else {
                return FirMatch::Unknown;
            };
            let default = if store.arena.is_nil(*default) {
                None
            } else {
                Some(*default)
            };
            FirMatch::Switch {
                cond: *cond,
                cases,
                default,
            }
        }
        (FIR_OPEN_BOX_TAG, [typ, label]) => {
            let (Some(typ), Some(label)) = (
                decode_ui_box_type(&store.arena, *typ),
                decode_symbol(&store.arena, *label),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::OpenBox { typ, label }
        }
        (FIR_CLOSE_BOX_TAG, []) => FirMatch::CloseBox,
        (FIR_ADD_BUTTON_TAG, [typ, label, var]) => {
            let (Some(typ), Some(label), Some(var)) = (
                decode_button_type(&store.arena, *typ),
                decode_symbol(&store.arena, *label),
                decode_symbol(&store.arena, *var),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::AddButton { typ, label, var }
        }
        (FIR_ADD_SLIDER_TAG, [typ, label, var, init, lo, hi, step]) => {
            let (Some(typ), Some(label), Some(var), Some(init), Some(lo), Some(hi), Some(step)) = (
                decode_slider_type(&store.arena, *typ),
                decode_symbol(&store.arena, *label),
                decode_symbol(&store.arena, *var),
                decode_f64(&store.arena, *init),
                decode_f64(&store.arena, *lo),
                decode_f64(&store.arena, *hi),
                decode_f64(&store.arena, *step),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::AddSlider {
                typ,
                label,
                var,
                init,
                lo,
                hi,
                step,
            }
        }
        (FIR_ADD_BARGRAPH_TAG, [typ, label, var, lo, hi]) => {
            let (Some(typ), Some(label), Some(var), Some(lo), Some(hi)) = (
                decode_bargraph_type(&store.arena, *typ),
                decode_symbol(&store.arena, *label),
                decode_symbol(&store.arena, *var),
                decode_f64(&store.arena, *lo),
                decode_f64(&store.arena, *hi),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::AddBargraph {
                typ,
                label,
                var,
                lo,
                hi,
            }
        }
        (FIR_ADD_SOUNDFILE_TAG, [label, url, var]) => {
            let (Some(label), Some(url), Some(var)) = (
                decode_symbol(&store.arena, *label),
                decode_symbol(&store.arena, *url),
                decode_symbol(&store.arena, *var),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::AddSoundfile { label, url, var }
        }
        // Compatibility with older rust snapshots where URL was not encoded.
        (FIR_ADD_SOUNDFILE_TAG, [label, var]) => {
            let (Some(label), Some(var)) = (
                decode_symbol(&store.arena, *label),
                decode_symbol(&store.arena, *var),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::AddSoundfile {
                label,
                url: String::new(),
                var,
            }
        }
        (FIR_ADD_META_DECLARE_TAG, [var, key, value]) => {
            let (Some(var), Some(key), Some(value)) = (
                decode_symbol(&store.arena, *var),
                decode_symbol(&store.arena, *key),
                decode_symbol(&store.arena, *value),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::AddMetaDeclare { var, key, value }
        }
        (FIR_LABEL_TAG, [label]) => {
            let Some(label) = decode_symbol(&store.arena, *label) else {
                return FirMatch::Unknown;
            };
            FirMatch::Label(label)
        }
        (
            FIR_MODULE_TAG,
            [
                num_inputs,
                num_outputs,
                name,
                dsp_struct,
                globals,
                functions,
                static_decls,
            ],
        ) => {
            let Some(name) = decode_symbol(&store.arena, *name) else {
                return FirMatch::Unknown;
            };
            let Some(raw_num_inputs) = decode_i64(&store.arena, *num_inputs) else {
                return FirMatch::Unknown;
            };
            let Some(raw_num_outputs) = decode_i64(&store.arena, *num_outputs) else {
                return FirMatch::Unknown;
            };
            let (Ok(num_inputs), Ok(num_outputs)) = (
                usize::try_from(raw_num_inputs),
                usize::try_from(raw_num_outputs),
            ) else {
                return FirMatch::Unknown;
            };
            FirMatch::Module {
                num_inputs,
                num_outputs,
                name,
                dsp_struct: *dsp_struct,
                globals: *globals,
                functions: *functions,
                static_decls: *static_decls,
            }
        }
        _ => FirMatch::Unknown,
    }
}

/// Deterministic structural dump helper for FIR differential checks.
///
/// The dump is rooted at `root` and recursively expands child FIR ids.
#[must_use]
pub fn dump_fir(store: &FirStore, root: FirId) -> String {
    let mut out = String::new();
    let mut seen = HashSet::new();
    dump_node(store, root, 0, &mut out, &mut seen);
    out
}

fn dump_node(
    store: &FirStore,
    id: FirId,
    depth: usize,
    out: &mut String,
    seen: &mut HashSet<FirId>,
) {
    let indent = "  ".repeat(depth);
    let node = match_fir(store, id);
    let _ = writeln!(out, "{indent}#{} {:?}", id.as_u32(), node);
    if !seen.insert(id) {
        return;
    }
    for child in child_ids(&node) {
        dump_node(store, child, depth + 1, out, seen);
    }
}

/// Returns the immediate FIR children that should be traversed structurally.
///
/// This is the canonical edge list used by [`dump_fir`] and similar internal
/// walkers. It follows semantic children only; encoded type/access atoms remain
/// implicit because they are reconstructed by [`match_fir`].
fn child_ids(node: &FirMatch) -> Vec<FirId> {
    match node {
        FirMatch::Unknown
        | FirMatch::Int32 { .. }
        | FirMatch::Int64 { .. }
        | FirMatch::Float32 { .. }
        | FirMatch::Float64 { .. }
        | FirMatch::Bool { .. }
        | FirMatch::Quad { .. }
        | FirMatch::FixedPoint { .. }
        | FirMatch::Int32Array { .. }
        | FirMatch::Float32Array { .. }
        | FirMatch::Float64Array { .. }
        | FirMatch::QuadArray { .. }
        | FirMatch::FixedPointArray { .. }
        | FirMatch::LoadVar { .. }
        | FirMatch::LoadVarAddress { .. }
        | FirMatch::NullValue { .. }
        | FirMatch::NewDsp { .. }
        | FirMatch::DeclareStructType { .. }
        | FirMatch::DeclareBufferIterators { .. }
        | FirMatch::ShiftArrayVar { .. }
        | FirMatch::NullStatement
        | FirMatch::OpenBox { .. }
        | FirMatch::CloseBox
        | FirMatch::AddButton { .. }
        | FirMatch::AddSlider { .. }
        | FirMatch::AddBargraph { .. }
        | FirMatch::AddSoundfile { .. }
        | FirMatch::AddMetaDeclare { .. }
        | FirMatch::Label(_) => Vec::new(),
        FirMatch::ValueArray { values, .. }
        | FirMatch::FunCall { args: values, .. }
        | FirMatch::DeclareTable { values, .. }
        | FirMatch::Block(values) => values.clone(),
        FirMatch::LoadTable { index, .. }
        | FirMatch::TeeVar { value: index, .. }
        | FirMatch::Neg { value: index, .. }
        | FirMatch::Cast { value: index, .. }
        | FirMatch::Bitcast { value: index, .. }
        | FirMatch::StoreVar { value: index, .. }
        | FirMatch::Drop(index) => vec![*index],
        FirMatch::SimpleForLoop { upper, body, .. } => vec![*upper, *body],
        FirMatch::BinOp { lhs, rhs, .. } => vec![*lhs, *rhs],
        FirMatch::Select2 {
            cond,
            then_value,
            else_value,
            ..
        } => vec![*cond, *then_value, *else_value],
        FirMatch::DeclareVar { init, .. } => init.iter().copied().collect(),
        FirMatch::DeclareFun { body: Some(b), .. } => vec![*b],
        FirMatch::DeclareFun { body: None, .. } => vec![],
        FirMatch::StoreTable { index, value, .. } => vec![*index, *value],
        FirMatch::Return(value) => value.iter().copied().collect(),
        FirMatch::If {
            cond,
            then_block,
            else_block,
        } => {
            let mut out = vec![*cond, *then_block];
            out.extend(else_block.iter().copied());
            out
        }
        FirMatch::Control { cond, stmt } => vec![*cond, *stmt],
        FirMatch::ForLoop {
            init,
            end,
            step,
            body,
            ..
        } => vec![*init, *end, *step, *body],
        FirMatch::IteratorForLoop { body, .. } => vec![*body],
        FirMatch::WhileLoop { cond, body } => vec![*cond, *body],
        FirMatch::Switch {
            cond,
            cases,
            default,
        } => {
            let mut out = vec![*cond];
            out.extend(cases.iter().map(|(_, block)| *block));
            out.extend(default.iter().copied());
            out
        }
        FirMatch::Module {
            dsp_struct,
            globals,
            functions,
            static_decls,
            ..
        } => vec![*dsp_struct, *globals, *functions, *static_decls],
    }
}

const FIR_TYPE_INT32_TAG: &str = "FIRTYPE_INT32";
const FIR_TYPE_INT64_TAG: &str = "FIRTYPE_INT64";
const FIR_TYPE_FLOAT32_TAG: &str = "FIRTYPE_FLOAT32";
const FIR_TYPE_FLOAT64_TAG: &str = "FIRTYPE_FLOAT64";
const FIR_TYPE_FAUSTFLOAT_TAG: &str = "FIRTYPE_FAUSTFLOAT";
const FIR_TYPE_QUAD_TAG: &str = "FIRTYPE_QUAD";
const FIR_TYPE_FIXED_POINT_TAG: &str = "FIRTYPE_FIXEDPOINT";
const FIR_TYPE_BOOL_TAG: &str = "FIRTYPE_BOOL";
const FIR_TYPE_VOID_TAG: &str = "FIRTYPE_VOID";
const FIR_TYPE_OBJ_TAG: &str = "FIRTYPE_OBJ";
const FIR_TYPE_SOUND_TAG: &str = "FIRTYPE_SOUND";
const FIR_TYPE_UI_TAG: &str = "FIRTYPE_UI";
const FIR_TYPE_META_TAG: &str = "FIRTYPE_META";
const FIR_TYPE_PTR_TAG: &str = "FIRTYPE_PTR";
const FIR_TYPE_ARRAY_TAG: &str = "FIRTYPE_ARRAY";
const FIR_TYPE_VECTOR_TAG: &str = "FIRTYPE_VECTOR";
const FIR_TYPE_STRUCT_TAG: &str = "FIRTYPE_STRUCT";
const FIR_TYPE_FUN_TAG: &str = "FIRTYPE_FUN";

const FIR_V_INT32_TAG: &str = "FIRV_INT32";
const FIR_V_INT64_TAG: &str = "FIRV_INT64";
const FIR_V_FLOAT32_TAG: &str = "FIRV_FLOAT32";
const FIR_V_FLOAT64_TAG: &str = "FIRV_FLOAT64";
const FIR_V_BOOL_TAG: &str = "FIRV_BOOL";
const FIR_V_QUAD_TAG: &str = "FIRV_QUAD";
const FIR_V_FIXED_POINT_TAG: &str = "FIRV_FIXEDPOINT";
const FIR_V_VALUE_ARRAY_TAG: &str = "FIRV_VALUEARRAY";
const FIR_V_INT32_ARRAY_TAG: &str = "FIRV_INT32ARRAY";
const FIR_V_FLOAT32_ARRAY_TAG: &str = "FIRV_FLOAT32ARRAY";
const FIR_V_FLOAT64_ARRAY_TAG: &str = "FIRV_FLOAT64ARRAY";
const FIR_V_QUAD_ARRAY_TAG: &str = "FIRV_QUADARRAY";
const FIR_V_FIXED_POINT_ARRAY_TAG: &str = "FIRV_FIXEDPOINTARRAY";
const FIR_V_LOAD_VAR_TAG: &str = "FIRV_LOADVAR";
const FIR_V_LOAD_TABLE_TAG: &str = "FIRV_LOADTABLE";
const FIR_V_LOAD_VAR_ADDRESS_TAG: &str = "FIRV_LOADVARADDRESS";
const FIR_V_TEE_VAR_TAG: &str = "FIRV_TEEVAR";
const FIR_V_BINOP_TAG: &str = "FIRV_BINOP";
const FIR_V_NEG_TAG: &str = "FIRV_NEG";
const FIR_V_CAST_TAG: &str = "FIRV_CAST";
const FIR_V_BITCAST_TAG: &str = "FIRV_BITCAST";
const FIR_V_SELECT2_TAG: &str = "FIRV_SELECT2";
const FIR_V_FUNCALL_TAG: &str = "FIRV_FUNCALL";
const FIR_V_NULL_TAG: &str = "FIRV_NULL";
const FIR_V_NEW_DSP_TAG: &str = "FIRV_NEWDSP";

const FIR_DECLARE_VAR_TAG: &str = "FIRST_DECLAREVAR";
const FIR_DECLARE_TABLE_TAG: &str = "FIRST_DECLARETABLE";
const FIR_DECLARE_FUN_TAG: &str = "FIRST_DECLAREFUN";
const FIR_DECLARE_FUN_PROTO_TAG: &str = "FIRST_DECLAREFUN_PROTO";
const FIR_DECLARE_STRUCT_TYPE_TAG: &str = "FIRST_DECLARESTRUCTTYPE";
const FIR_DECLARE_BUFFER_ITERATORS_TAG: &str = "FIRST_DECLAREBUFFERITERATORS";
const FIR_STORE_VAR_TAG: &str = "FIRST_STOREVAR";
const FIR_STORE_TABLE_TAG: &str = "FIRST_STORETABLE";
const FIR_SHIFT_ARRAY_VAR_TAG: &str = "FIRST_SHIFTARRAYVAR";
const FIR_DROP_TAG: &str = "FIRST_DROP";
const FIR_NULL_STATEMENT_TAG: &str = "FIRST_NULLSTATEMENT";
const FIR_RETURN_TAG: &str = "FIRST_RETURN";
const FIR_BLOCK_TAG: &str = "FIRST_BLOCK";
const FIR_IF_TAG: &str = "FIRST_IF";
const FIR_CONTROL_TAG: &str = "FIRST_CONTROL";
const FIR_FOR_LOOP_TAG: &str = "FIRST_FORLOOP";
const FIR_SIMPLE_FOR_LOOP_TAG: &str = "FIRST_SIMPLEFOR";
const FIR_ITERATOR_FOR_LOOP_TAG: &str = "FIRST_ITERATORFOR";
const FIR_WHILE_LOOP_TAG: &str = "FIRST_WHILELOOP";
const FIR_SWITCH_TAG: &str = "FIRST_SWITCH";
const FIR_OPEN_BOX_TAG: &str = "FIRST_OPENBOX";
const FIR_CLOSE_BOX_TAG: &str = "FIRST_CLOSEBOX";
const FIR_ADD_BUTTON_TAG: &str = "FIRST_ADDBUTTON";
const FIR_ADD_SLIDER_TAG: &str = "FIRST_ADDSLIDER";
const FIR_ADD_BARGRAPH_TAG: &str = "FIRST_ADDBARGRAPH";
const FIR_ADD_SOUNDFILE_TAG: &str = "FIRST_ADDSOUNDFILE";
const FIR_ADD_META_DECLARE_TAG: &str = "FIRST_ADDMETA";
const FIR_LABEL_TAG: &str = "FIRST_LABEL";
const FIR_MODULE_TAG: &str = "FIRST_MODULE";
const FIR_NAMED_TYPE_TAG: &str = "FIR_NAMEDTYPE";
const FIR_SWITCH_CASE_TAG: &str = "FIR_SWITCHCASE";

/// Returns `true` when `tag` names a FIR value-producing node.
///
/// [`FirStore::value_type`] relies on this whitelist to decide whether the
/// first encoded child stores a result type.
fn is_value_tag(tag: &str) -> bool {
    matches!(
        tag,
        FIR_V_INT32_TAG
            | FIR_V_INT64_TAG
            | FIR_V_FLOAT32_TAG
            | FIR_V_FLOAT64_TAG
            | FIR_V_BOOL_TAG
            | FIR_V_QUAD_TAG
            | FIR_V_FIXED_POINT_TAG
            | FIR_V_VALUE_ARRAY_TAG
            | FIR_V_INT32_ARRAY_TAG
            | FIR_V_FLOAT32_ARRAY_TAG
            | FIR_V_FLOAT64_ARRAY_TAG
            | FIR_V_QUAD_ARRAY_TAG
            | FIR_V_FIXED_POINT_ARRAY_TAG
            | FIR_V_LOAD_VAR_TAG
            | FIR_V_LOAD_TABLE_TAG
            | FIR_V_LOAD_VAR_ADDRESS_TAG
            | FIR_V_TEE_VAR_TAG
            | FIR_V_BINOP_TAG
            | FIR_V_NEG_TAG
            | FIR_V_CAST_TAG
            | FIR_V_BITCAST_TAG
            | FIR_V_SELECT2_TAG
            | FIR_V_FUNCALL_TAG
            | FIR_V_NULL_TAG
            | FIR_V_NEW_DSP_TAG
    )
}

/// Interns one tag node in the underlying [`TreeArena`].
///
/// This is the one place where FIR tag spelling meets TreeArena hash-consing.
/// All builder-side encoders route through it so identical tag/child shapes are
/// structurally shared automatically.
fn intern_tag(arena: &mut TreeArena, tag: &str, children: &[FirId]) -> FirId {
    let tag_id = arena.intern_tag(tag);
    arena.intern(NodeKind::Tag(tag_id), children)
}

/// Encodes an ordered FIR id slice as a canonical `cons`/`nil` list.
///
/// FIR keeps list structure explicit in the TreeArena representation so it can
/// round-trip through hash-consing without side tables.
fn encode_list(arena: &mut TreeArena, values: &[FirId]) -> FirId {
    let mut out = arena.nil();
    for value in values.iter().rev() {
        out = arena.cons(*value, out);
    }
    out
}

/// Decodes a canonical `cons`/`nil` list back into a flat FIR id vector.
///
/// Returns `None` if `list` is not a well-formed canonical list.
fn decode_list(arena: &TreeArena, mut list: FirId) -> Option<Vec<FirId>> {
    let mut out = Vec::new();
    while !arena.is_nil(list) {
        let head = arena.hd(list)?;
        out.push(head);
        list = arena.tl(list)?;
    }
    Some(out)
}

/// Decodes a FIR list whose payload nodes must all be `i32` literals.
fn decode_i32_list(arena: &TreeArena, list: FirId) -> Option<Vec<i32>> {
    let ids = decode_list(arena, list)?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.push(decode_i32(arena, id)?);
    }
    Some(out)
}

/// Decodes a FIR list whose payload nodes store `f32` values as bit patterns.
fn decode_f32_bits_list(arena: &TreeArena, list: FirId) -> Option<Vec<f32>> {
    let ids = decode_list(arena, list)?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.push(decode_f32_bits(arena, id)?);
    }
    Some(out)
}

/// Decodes a FIR list whose payload nodes must all be `f64`-compatible scalars.
fn decode_f64_list(arena: &TreeArena, list: FirId) -> Option<Vec<f64>> {
    let ids = decode_list(arena, list)?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.push(decode_f64(arena, id)?);
    }
    Some(out)
}

/// Decodes a FIR list whose payload nodes must all be symbols/string literals.
fn decode_symbols_list(arena: &TreeArena, list: FirId) -> Option<Vec<String>> {
    let ids = decode_list(arena, list)?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.push(decode_symbol(arena, id)?);
    }
    Some(out)
}

/// Encodes one `(name, type)` pair for function signatures and similar payloads.
fn encode_named_type(arena: &mut TreeArena, value: &NamedType) -> FirId {
    let name_id = arena.symbol(value.name.clone());
    let type_id = encode_type(arena, &value.typ);
    intern_tag(arena, FIR_NAMED_TYPE_TAG, &[name_id, type_id])
}

/// Encodes a stable ordered list of [`NamedType`] values.
fn encode_named_types(arena: &mut TreeArena, values: &[NamedType]) -> FirId {
    let ids: Vec<_> = values.iter().map(|v| encode_named_type(arena, v)).collect();
    encode_list(arena, &ids)
}

/// Decodes one encoded [`NamedType`] pair.
fn decode_named_type(arena: &TreeArena, id: FirId) -> Option<NamedType> {
    let node = arena.node(id)?;
    let NodeKind::Tag(tag_id) = &node.kind else {
        return None;
    };
    let tag = arena.tag_name(*tag_id)?;
    let [name, typ] = node.children.as_slice() else {
        return None;
    };
    if tag != FIR_NAMED_TYPE_TAG {
        return None;
    }
    Some(NamedType {
        name: decode_symbol(arena, *name)?,
        typ: decode_type(arena, *typ)?,
    })
}

/// Decodes a canonical list of encoded [`NamedType`] nodes.
fn decode_named_types(arena: &TreeArena, list: FirId) -> Option<Vec<NamedType>> {
    let ids = decode_list(arena, list)?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.push(decode_named_type(arena, id)?);
    }
    Some(out)
}

/// Encodes one `switch` case pair `(constant_value, block_id)`.
fn encode_switch_case(arena: &mut TreeArena, value: i64, block: FirId) -> FirId {
    let value_id = arena.int(value);
    intern_tag(arena, FIR_SWITCH_CASE_TAG, &[value_id, block])
}

/// Encodes all `switch` cases as a canonical ordered list.
fn encode_switch_cases(arena: &mut TreeArena, cases: &[(i64, FirId)]) -> FirId {
    let ids: Vec<_> = cases
        .iter()
        .map(|(value, block)| encode_switch_case(arena, *value, *block))
        .collect();
    encode_list(arena, &ids)
}

/// Decodes one encoded `switch` case node.
fn decode_switch_case(arena: &TreeArena, id: FirId) -> Option<(i64, FirId)> {
    let node = arena.node(id)?;
    let NodeKind::Tag(tag_id) = &node.kind else {
        return None;
    };
    let tag = arena.tag_name(*tag_id)?;
    let [value, block] = node.children.as_slice() else {
        return None;
    };
    if tag != FIR_SWITCH_CASE_TAG {
        return None;
    }
    Some((decode_i64(arena, *value)?, *block))
}

/// Decodes a canonical ordered list of encoded `switch` cases.
fn decode_switch_cases(arena: &TreeArena, list: FirId) -> Option<Vec<(i64, FirId)>> {
    let ids = decode_list(arena, list)?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.push(decode_switch_case(arena, id)?);
    }
    Some(out)
}

/// Encodes the explicit FIR type model into its canonical tree representation.
///
/// The representation is intentionally self-describing and recursive so value
/// nodes can carry types inline with no auxiliary type table.
fn encode_type(arena: &mut TreeArena, typ: &FirType) -> FirId {
    match typ {
        FirType::Int32 => intern_tag(arena, FIR_TYPE_INT32_TAG, &[]),
        FirType::Int64 => intern_tag(arena, FIR_TYPE_INT64_TAG, &[]),
        FirType::Float32 => intern_tag(arena, FIR_TYPE_FLOAT32_TAG, &[]),
        FirType::Float64 => intern_tag(arena, FIR_TYPE_FLOAT64_TAG, &[]),
        FirType::FaustFloat => intern_tag(arena, FIR_TYPE_FAUSTFLOAT_TAG, &[]),
        FirType::Quad => intern_tag(arena, FIR_TYPE_QUAD_TAG, &[]),
        FirType::FixedPoint => intern_tag(arena, FIR_TYPE_FIXED_POINT_TAG, &[]),
        FirType::Bool => intern_tag(arena, FIR_TYPE_BOOL_TAG, &[]),
        FirType::Void => intern_tag(arena, FIR_TYPE_VOID_TAG, &[]),
        FirType::Obj => intern_tag(arena, FIR_TYPE_OBJ_TAG, &[]),
        FirType::Sound => intern_tag(arena, FIR_TYPE_SOUND_TAG, &[]),
        FirType::UI => intern_tag(arena, FIR_TYPE_UI_TAG, &[]),
        FirType::Meta => intern_tag(arena, FIR_TYPE_META_TAG, &[]),
        FirType::Ptr(inner) => {
            let inner_id = encode_type(arena, inner);
            intern_tag(arena, FIR_TYPE_PTR_TAG, &[inner_id])
        }
        FirType::Array(inner, size) => {
            let inner_id = encode_type(arena, inner);
            let size_id = arena.int(i64::try_from(*size).unwrap_or(i64::MAX));
            intern_tag(arena, FIR_TYPE_ARRAY_TAG, &[inner_id, size_id])
        }
        FirType::Vector(inner, lanes) => {
            let inner_id = encode_type(arena, inner);
            let lanes_id = arena.int(i64::try_from(*lanes).unwrap_or(i64::MAX));
            intern_tag(arena, FIR_TYPE_VECTOR_TAG, &[inner_id, lanes_id])
        }
        FirType::Struct(name, fields) => {
            let name_id = arena.symbol(name.clone());
            let field_ids: Vec<_> = fields.iter().map(|f| encode_type(arena, f)).collect();
            let fields_list = encode_list(arena, &field_ids);
            intern_tag(arena, FIR_TYPE_STRUCT_TAG, &[name_id, fields_list])
        }
        FirType::Fun { args, ret } => {
            let args_ids: Vec<_> = args.iter().map(|a| encode_type(arena, a)).collect();
            let args_list = encode_list(arena, &args_ids);
            let ret_id = encode_type(arena, ret);
            intern_tag(arena, FIR_TYPE_FUN_TAG, &[args_list, ret_id])
        }
    }
}

/// Decodes a canonical tree-encoded FIR type.
fn decode_type(arena: &TreeArena, id: FirId) -> Option<FirType> {
    let node = arena.node(id)?;
    let NodeKind::Tag(tag_id) = &node.kind else {
        return None;
    };
    let tag = arena.tag_name(*tag_id)?;
    let ch = node.children.as_slice();
    match (tag, ch) {
        (FIR_TYPE_INT32_TAG, []) => Some(FirType::Int32),
        (FIR_TYPE_INT64_TAG, []) => Some(FirType::Int64),
        (FIR_TYPE_FLOAT32_TAG, []) => Some(FirType::Float32),
        (FIR_TYPE_FLOAT64_TAG, []) => Some(FirType::Float64),
        (FIR_TYPE_FAUSTFLOAT_TAG, []) => Some(FirType::FaustFloat),
        (FIR_TYPE_QUAD_TAG, []) => Some(FirType::Quad),
        (FIR_TYPE_FIXED_POINT_TAG, []) => Some(FirType::FixedPoint),
        (FIR_TYPE_BOOL_TAG, []) => Some(FirType::Bool),
        (FIR_TYPE_VOID_TAG, []) => Some(FirType::Void),
        (FIR_TYPE_OBJ_TAG, []) => Some(FirType::Obj),
        (FIR_TYPE_SOUND_TAG, []) => Some(FirType::Sound),
        (FIR_TYPE_UI_TAG, []) => Some(FirType::UI),
        (FIR_TYPE_META_TAG, []) => Some(FirType::Meta),
        (FIR_TYPE_PTR_TAG, [inner]) => Some(FirType::Ptr(Box::new(decode_type(arena, *inner)?))),
        (FIR_TYPE_ARRAY_TAG, [inner, size]) => {
            let size = usize::try_from(decode_i64(arena, *size)?).ok()?;
            Some(FirType::Array(Box::new(decode_type(arena, *inner)?), size))
        }
        (FIR_TYPE_VECTOR_TAG, [inner, lanes]) => {
            let lanes = usize::try_from(decode_i64(arena, *lanes)?).ok()?;
            Some(FirType::Vector(
                Box::new(decode_type(arena, *inner)?),
                lanes,
            ))
        }
        (FIR_TYPE_STRUCT_TAG, [name, fields]) => {
            let name = decode_symbol(arena, *name)?;
            let field_ids = decode_list(arena, *fields)?;
            let mut decoded_fields = Vec::with_capacity(field_ids.len());
            for fid in field_ids {
                decoded_fields.push(decode_type(arena, fid)?);
            }
            Some(FirType::Struct(name, decoded_fields))
        }
        (FIR_TYPE_FUN_TAG, [args, ret]) => {
            let args_ids = decode_list(arena, *args)?;
            let mut out = Vec::with_capacity(args_ids.len());
            for arg in args_ids {
                out.push(decode_type(arena, arg)?);
            }
            let ret = decode_type(arena, *ret)?;
            Some(FirType::Fun {
                args: out,
                ret: Box::new(ret),
            })
        }
        _ => None,
    }
}

/// Encodes one [`AccessType`] as its stable small integer code.
///
/// The numeric mapping is an internal representation contract and must remain
/// synchronized with [`decode_access`].
fn encode_access(arena: &mut TreeArena, access: AccessType) -> FirId {
    arena.int(match access {
        AccessType::Stack => 0,
        AccessType::Struct => 1,
        AccessType::Static => 2,
        AccessType::FunArgs => 3,
        AccessType::Loop => 4,
        AccessType::Global => 5,
    })
}

/// Decodes one small integer access-code back into [`AccessType`].
fn decode_access(arena: &TreeArena, id: FirId) -> Option<AccessType> {
    match decode_i64(arena, id)? {
        0 => Some(AccessType::Stack),
        1 => Some(AccessType::Struct),
        2 => Some(AccessType::Static),
        3 => Some(AccessType::FunArgs),
        4 => Some(AccessType::Loop),
        5 => Some(AccessType::Global),
        _ => None,
    }
}

/// Encodes one [`FirBinOp`] as its stable small integer code.
fn encode_binop(arena: &mut TreeArena, op: FirBinOp) -> FirId {
    arena.int(match op {
        FirBinOp::Add => 0,
        FirBinOp::Sub => 1,
        FirBinOp::Mul => 2,
        FirBinOp::Div => 3,
        FirBinOp::Rem => 4,
        FirBinOp::And => 5,
        FirBinOp::Or => 6,
        FirBinOp::Xor => 7,
        FirBinOp::Eq => 8,
        FirBinOp::Ne => 9,
        FirBinOp::Lt => 10,
        FirBinOp::Le => 11,
        FirBinOp::Gt => 12,
        FirBinOp::Ge => 13,
    })
}

/// Decodes one small integer opcode back into [`FirBinOp`].
fn decode_binop(arena: &TreeArena, id: FirId) -> Option<FirBinOp> {
    match decode_i64(arena, id)? {
        0 => Some(FirBinOp::Add),
        1 => Some(FirBinOp::Sub),
        2 => Some(FirBinOp::Mul),
        3 => Some(FirBinOp::Div),
        4 => Some(FirBinOp::Rem),
        5 => Some(FirBinOp::And),
        6 => Some(FirBinOp::Or),
        7 => Some(FirBinOp::Xor),
        8 => Some(FirBinOp::Eq),
        9 => Some(FirBinOp::Ne),
        10 => Some(FirBinOp::Lt),
        11 => Some(FirBinOp::Le),
        12 => Some(FirBinOp::Gt),
        13 => Some(FirBinOp::Ge),
        _ => None,
    }
}

/// Encodes one UI container orientation as a compact integer atom.
fn encode_ui_box_type(arena: &mut TreeArena, typ: UiBoxType) -> FirId {
    arena.int(match typ {
        UiBoxType::Vertical => 0,
        UiBoxType::Horizontal => 1,
        UiBoxType::Tab => 2,
    })
}

/// Decodes one encoded UI container orientation.
fn decode_ui_box_type(arena: &TreeArena, id: FirId) -> Option<UiBoxType> {
    match decode_i64(arena, id)? {
        0 => Some(UiBoxType::Vertical),
        1 => Some(UiBoxType::Horizontal),
        2 => Some(UiBoxType::Tab),
        _ => None,
    }
}

/// Encodes one UI button kind as a compact integer atom.
fn encode_button_type(arena: &mut TreeArena, typ: ButtonType) -> FirId {
    arena.int(match typ {
        ButtonType::Button => 0,
        ButtonType::Checkbox => 1,
    })
}

/// Decodes one encoded UI button kind.
fn decode_button_type(arena: &TreeArena, id: FirId) -> Option<ButtonType> {
    match decode_i64(arena, id)? {
        0 => Some(ButtonType::Button),
        1 => Some(ButtonType::Checkbox),
        _ => None,
    }
}

/// Encodes one UI slider kind as a compact integer atom.
fn encode_slider_type(arena: &mut TreeArena, typ: SliderType) -> FirId {
    arena.int(match typ {
        SliderType::Horizontal => 0,
        SliderType::Vertical => 1,
        SliderType::NumEntry => 2,
    })
}

/// Decodes one encoded UI slider kind.
fn decode_slider_type(arena: &TreeArena, id: FirId) -> Option<SliderType> {
    match decode_i64(arena, id)? {
        0 => Some(SliderType::Horizontal),
        1 => Some(SliderType::Vertical),
        2 => Some(SliderType::NumEntry),
        _ => None,
    }
}

/// Encodes one UI bargraph kind as a compact integer atom.
fn encode_bargraph_type(arena: &mut TreeArena, typ: BargraphType) -> FirId {
    arena.int(match typ {
        BargraphType::Horizontal => 0,
        BargraphType::Vertical => 1,
    })
}

/// Decodes one encoded UI bargraph kind.
fn decode_bargraph_type(arena: &TreeArena, id: FirId) -> Option<BargraphType> {
    match decode_i64(arena, id)? {
        0 => Some(BargraphType::Horizontal),
        1 => Some(BargraphType::Vertical),
        _ => None,
    }
}

/// Decodes a symbol-bearing atom.
///
/// FIR accepts both interned symbols and string literals here because some UI
/// payloads are stored as string literals in the TreeArena.
fn decode_symbol(arena: &TreeArena, id: FirId) -> Option<String> {
    match arena.kind(id)? {
        NodeKind::Symbol(s) => Some(s.to_string()),
        NodeKind::StringLiteral(s) => Some(s.to_string()),
        _ => None,
    }
}

/// Decodes one integer atom as `i64`.
fn decode_i64(arena: &TreeArena, id: FirId) -> Option<i64> {
    tree_to_int(arena, id)
}

/// Decodes one integer atom as `i32`, failing on out-of-range values.
fn decode_i32(arena: &TreeArena, id: FirId) -> Option<i32> {
    i32::try_from(decode_i64(arena, id)?).ok()
}

/// Decodes one integer atom as the raw IEEE-754 bits of an `f32`.
fn decode_f32_bits(arena: &TreeArena, id: FirId) -> Option<f32> {
    let bits = u32::try_from(decode_i64(arena, id)?).ok()?;
    Some(f32::from_bits(bits))
}

/// Decodes one numeric atom as `f64`.
///
/// The fallback from integer to float preserves the permissive literal handling
/// historically used by the C++ FIR printers/builders.
fn decode_f64(arena: &TreeArena, id: FirId) -> Option<f64> {
    tree_to_double(arena, id).or_else(|| tree_to_int(arena, id).map(|v| v as f64))
}

/// Decodes one integer atom as a canonical boolean (`0`/`1` only).
fn decode_bool(arena: &TreeArena, id: FirId) -> Option<bool> {
    match decode_i64(arena, id)? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_and_match_cover_core_value_nodes() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let one = b.int32(1);
        let two = b.int32(2);
        let sum = b.binop(FirBinOp::Add, one, two, FirType::Int32);
        let call = b.fun_call("foo", &[sum], FirType::Int32);
        let cast = b.cast(FirType::Float64, call);

        assert_eq!(
            match_fir(&store, one),
            FirMatch::Int32 {
                value: 1,
                typ: FirType::Int32
            }
        );
        assert_eq!(
            match_fir(&store, sum),
            FirMatch::BinOp {
                op: FirBinOp::Add,
                lhs: one,
                rhs: two,
                typ: FirType::Int32
            }
        );
        assert_eq!(
            match_fir(&store, call),
            FirMatch::FunCall {
                name: "foo".to_string(),
                args: vec![sum],
                typ: FirType::Int32
            }
        );
        assert_eq!(
            match_fir(&store, cast),
            FirMatch::Cast {
                typ: FirType::Float64,
                value: call
            }
        );

        assert_eq!(store.value_type(cast), Some(FirType::Float64));
        assert_eq!(store.value_type(sum), Some(FirType::Int32));
    }

    #[test]
    fn builder_and_match_cover_stmt_nodes() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let zero = b.int32(0);
        let dec = b.declare_var("acc", FirType::Int32, AccessType::Stack, Some(zero));
        let upper = b.int32(64);
        let body = b.block(&[dec]);
        let loop_ = b.simple_for_loop("i", upper, body, false);
        let ret = b.ret(Some(zero));
        let block = b.block(&[loop_, ret]);

        assert_eq!(
            match_fir(&store, dec),
            FirMatch::DeclareVar {
                name: "acc".to_string(),
                typ: FirType::Int32,
                access: AccessType::Stack,
                init: Some(zero)
            }
        );
        assert_eq!(
            match_fir(&store, loop_),
            FirMatch::SimpleForLoop {
                var: "i".to_string(),
                upper,
                body,
                is_reverse: false
            }
        );
        assert_eq!(match_fir(&store, block), FirMatch::Block(vec![loop_, ret]));
    }

    #[test]
    fn dump_fir_expands_simple_for_loop_body() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let one = b.int32(1);
        let upper = b.int32(8);
        let body_stmt = b.store_var("acc", AccessType::Stack, one);
        let body = b.block(&[body_stmt]);
        let loop_ = b.simple_for_loop("i", upper, body, false);
        let root = b.block(&[loop_]);

        let dump = dump_fir(&store, root);
        assert!(dump.contains("SimpleForLoop"));
        assert!(dump.contains("StoreVar { name: \"acc\""));
        assert!(dump.contains("Int32 { value: 1"));
        assert!(dump.contains(&format!("#{}", body.as_u32())));
        assert!(dump.contains(&format!("#{}", body_stmt.as_u32())));
    }

    #[test]
    fn dump_fir_expands_for_loop_body() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let zero = b.int32(0);
        let one = b.int32(1);
        let ten = b.int32(10);
        let init = b.declare_var("i", FirType::Int32, AccessType::Loop, Some(zero));
        let body_stmt = b.store_var("acc", AccessType::Stack, one);
        let body = b.block(&[body_stmt]);
        let loop_ = b.for_loop("i", init, ten, one, body, false);
        let root = b.block(&[loop_]);

        let dump = dump_fir(&store, root);
        assert!(dump.contains("ForLoop {"));
        assert!(dump.contains("StoreVar { name: \"acc\""));
        assert!(dump.contains(&format!("#{}", body.as_u32())));
        assert!(dump.contains(&format!("#{}", body_stmt.as_u32())));
    }

    #[test]
    fn dump_fir_expands_iterator_for_loop_body() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let one = b.int32(1);
        let body_stmt = b.store_var("acc", AccessType::Stack, one);
        let body = b.block(&[body_stmt]);
        let loop_ = b.iterator_for_loop(&["i0", "i1"], false, body);
        let root = b.block(&[loop_]);

        let dump = dump_fir(&store, root);
        assert!(dump.contains("IteratorForLoop {"));
        assert!(dump.contains("StoreVar { name: \"acc\""));
        assert!(dump.contains(&format!("#{}", body.as_u32())));
        assert!(dump.contains(&format!("#{}", body_stmt.as_u32())));
    }

    #[test]
    fn builder_and_match_cover_ui_nodes() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let open = b.open_box(UiBoxType::Vertical, "osc");
        let slider = b.add_slider(
            SliderType::Horizontal,
            "freq",
            "fHslider0",
            SliderRange {
                init: 440.0,
                lo: 20.0,
                hi: 20_000.0,
                step: 1.0,
            },
        );
        let close = b.close_box();
        let block = b.block(&[open, slider, close]);

        assert_eq!(
            match_fir(&store, open),
            FirMatch::OpenBox {
                typ: UiBoxType::Vertical,
                label: "osc".to_string()
            }
        );
        assert_eq!(
            match_fir(&store, slider),
            FirMatch::AddSlider {
                typ: SliderType::Horizontal,
                label: "freq".to_string(),
                var: "fHslider0".to_string(),
                init: 440.0,
                lo: 20.0,
                hi: 20_000.0,
                step: 1.0
            }
        );
        assert_eq!(
            match_fir(&store, block),
            FirMatch::Block(vec![open, slider, close])
        );
    }

    #[test]
    fn builder_and_match_cover_extended_cpp_families() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let x = b.load_var("x", AccessType::Stack, FirType::Float64);
        let neg = b.neg(x, FirType::Float64);
        let addr = b.load_var_address(
            "x",
            AccessType::Stack,
            FirType::Ptr(Box::new(FirType::Float64)),
        );
        let tee = b.tee_var("x", AccessType::Stack, neg, FirType::Float64);
        let cond = b.bool_(true);
        let sel = b.select2(cond, tee, x, FirType::Float64);
        let nullv = b.null_value(FirType::Void);
        let newdsp = b.new_dsp("MyDSP", FirType::Obj);
        let soundfile = b.add_soundfile("sf", "fSound0");

        assert_eq!(
            match_fir(&store, addr),
            FirMatch::LoadVarAddress {
                name: "x".to_string(),
                access: AccessType::Stack,
                typ: FirType::Ptr(Box::new(FirType::Float64))
            }
        );
        assert_eq!(
            match_fir(&store, sel),
            FirMatch::Select2 {
                cond,
                then_value: tee,
                else_value: x,
                typ: FirType::Float64
            }
        );
        assert_eq!(
            match_fir(&store, nullv),
            FirMatch::NullValue { typ: FirType::Void }
        );
        assert_eq!(
            match_fir(&store, newdsp),
            FirMatch::NewDsp {
                name: "MyDSP".to_string(),
                typ: FirType::Obj
            }
        );
        assert_eq!(
            match_fir(&store, soundfile),
            FirMatch::AddSoundfile {
                label: "sf".to_string(),
                url: String::new(),
                var: "fSound0".to_string()
            }
        );
    }

    #[test]
    fn builder_and_match_cover_remaining_cpp_families() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let q = b.quad(1.25);
        let fx = b.fixed_point(0.5);
        let arr_i32 = b.int32_array(&[1, 2, 3]);
        let arr_f32 = b.float32_array(&[1.0, 2.0]);
        let arr_f64 = b.float64_array(&[3.5, 4.5]);
        let arr_q = b.quad_array(&[0.125, 0.25]);
        let arr_fx = b.fixed_point_array(&[0.75, 0.875]);
        let value_array = b.value_array(&[q, fx], FirType::Array(Box::new(FirType::Float64), 2));

        let dbi = b.declare_buffer_iterators("in", "out", 2, FirType::Float32, true, false);
        let body = b.block(&[dbi]);
        let ifor = b.iterator_for_loop(&["i", "j"], true, body);
        let sound = b.add_soundfile_with_url("sf", "stereo.wav", "fSound0");

        assert_eq!(
            match_fir(&store, q),
            FirMatch::Quad {
                value: 1.25,
                typ: FirType::Quad
            }
        );
        assert_eq!(
            match_fir(&store, fx),
            FirMatch::FixedPoint {
                value: 0.5,
                typ: FirType::FixedPoint
            }
        );
        assert_eq!(
            match_fir(&store, arr_i32),
            FirMatch::Int32Array {
                values: vec![1, 2, 3],
                typ: FirType::Array(Box::new(FirType::Int32), 3)
            }
        );
        assert_eq!(
            match_fir(&store, arr_f32),
            FirMatch::Float32Array {
                values: vec![1.0, 2.0],
                typ: FirType::Array(Box::new(FirType::Float32), 2)
            }
        );
        assert_eq!(
            match_fir(&store, arr_f64),
            FirMatch::Float64Array {
                values: vec![3.5, 4.5],
                typ: FirType::Array(Box::new(FirType::Float64), 2)
            }
        );
        assert_eq!(
            match_fir(&store, arr_q),
            FirMatch::QuadArray {
                values: vec![0.125, 0.25],
                typ: FirType::Array(Box::new(FirType::Quad), 2)
            }
        );
        assert_eq!(
            match_fir(&store, arr_fx),
            FirMatch::FixedPointArray {
                values: vec![0.75, 0.875],
                typ: FirType::Array(Box::new(FirType::FixedPoint), 2)
            }
        );
        assert_eq!(
            match_fir(&store, value_array),
            FirMatch::ValueArray {
                values: vec![q, fx],
                typ: FirType::Array(Box::new(FirType::Float64), 2)
            }
        );
        assert_eq!(
            match_fir(&store, dbi),
            FirMatch::DeclareBufferIterators {
                name1: "in".to_string(),
                name2: "out".to_string(),
                channels: 2,
                typ: FirType::Float32,
                mutable: true,
                chunk: false
            }
        );
        assert_eq!(
            match_fir(&store, ifor),
            FirMatch::IteratorForLoop {
                iterators: vec!["i".to_string(), "j".to_string()],
                is_reverse: true,
                body
            }
        );
        assert_eq!(
            match_fir(&store, sound),
            FirMatch::AddSoundfile {
                label: "sf".to_string(),
                url: "stereo.wav".to_string(),
                var: "fSound0".to_string()
            }
        );
    }

    #[test]
    fn builder_and_match_cover_table_nodes() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let i0 = b.int32(0);
        let v0 = b.float64(1.0);
        let v1 = b.float64(-2.0);
        let table = b.declare_table("fTbl0", AccessType::Struct, FirType::FaustFloat, &[v0, v1]);
        let read = b.load_table("fTbl0", AccessType::Struct, i0, FirType::FaustFloat);
        let write = b.store_table("fTbl0", AccessType::Struct, i0, read);

        assert_eq!(
            match_fir(&store, table),
            FirMatch::DeclareTable {
                name: "fTbl0".to_string(),
                access: AccessType::Struct,
                elem_type: FirType::FaustFloat,
                values: vec![v0, v1]
            }
        );
        assert_eq!(
            match_fir(&store, read),
            FirMatch::LoadTable {
                name: "fTbl0".to_string(),
                access: AccessType::Struct,
                index: i0,
                typ: FirType::FaustFloat
            }
        );
        assert_eq!(
            match_fir(&store, write),
            FirMatch::StoreTable {
                name: "fTbl0".to_string(),
                access: AccessType::Struct,
                index: i0,
                value: read
            }
        );
        assert_eq!(store.value_type(read), Some(FirType::FaustFloat));
    }

    #[test]
    fn builder_and_match_cover_faust_dsp_api_fun_signatures() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let body = b.block(&[]);

        let metadata_args = vec![NamedType {
            name: "m".to_string(),
            typ: FirType::Meta,
        }];
        let metadata_ty = FirType::Fun {
            args: vec![FirType::Meta],
            ret: Box::new(FirType::Void),
        };
        let metadata = b.declare_fun(
            "metadata",
            metadata_ty.clone(),
            &metadata_args,
            Some(body),
            false,
        );

        let ui_args = vec![NamedType {
            name: "ui_interface".to_string(),
            typ: FirType::UI,
        }];
        let ui_ty = FirType::Fun {
            args: vec![FirType::UI],
            ret: Box::new(FirType::Void),
        };
        let build_ui = b.declare_fun(
            "buildUserInterface",
            ui_ty.clone(),
            &ui_args,
            Some(body),
            false,
        );

        let compute_args = vec![
            NamedType {
                name: "count".to_string(),
                typ: FirType::Int32,
            },
            NamedType {
                name: "inputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
            NamedType {
                name: "outputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
        ];
        let compute_ty = FirType::Fun {
            args: vec![
                FirType::Int32,
                FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            ],
            ret: Box::new(FirType::Void),
        };
        let compute = b.declare_fun(
            "compute",
            compute_ty.clone(),
            &compute_args,
            Some(body),
            false,
        );

        assert_eq!(
            match_fir(&store, metadata),
            FirMatch::DeclareFun {
                name: "metadata".to_string(),
                typ: metadata_ty,
                args: metadata_args,
                body: Some(body),
                is_inline: false
            }
        );
        assert_eq!(
            match_fir(&store, build_ui),
            FirMatch::DeclareFun {
                name: "buildUserInterface".to_string(),
                typ: ui_ty,
                args: ui_args,
                body: Some(body),
                is_inline: false
            }
        );
        assert_eq!(
            match_fir(&store, compute),
            FirMatch::DeclareFun {
                name: "compute".to_string(),
                typ: compute_ty,
                args: compute_args,
                body: Some(body),
                is_inline: false
            }
        );
    }

    #[test]
    fn builder_and_match_cover_declare_fun_proto() {
        let mut store = FirStore::new();
        let args = vec![NamedType {
            name: "x".to_string(),
            typ: FirType::FaustFloat,
        }];
        let typ = FirType::Fun {
            args: vec![FirType::FaustFloat],
            ret: Box::new(FirType::FaustFloat),
        };
        let (proto, proto_dup, proto_with_body) = {
            let mut b = FirBuilder::new(&mut store);
            let p = b.declare_fun("myHelper", typ.clone(), &args, None, false);
            let pd = b.declare_fun("myHelper", typ.clone(), &args, None, false);
            let body = b.block(&[]);
            let pb = b.declare_fun("myHelper", typ.clone(), &args, Some(body), false);
            (p, pd, pb)
        };
        // Prototypes are hash-consed.
        assert_eq!(proto, proto_dup);
        // A prototype and a definition with the same signature are distinct nodes.
        assert_ne!(proto, proto_with_body);
        // Round-trip decode.
        assert_eq!(
            match_fir(&store, proto),
            FirMatch::DeclareFun {
                name: "myHelper".to_string(),
                typ,
                args,
                body: None,
                is_inline: false,
            }
        );
    }

    #[test]
    fn structurally_identical_nodes_are_shared() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let a1 = b.int32(42);
        let a2 = b.int32(42);
        assert_eq!(a1, a2);

        let add1 = b.binop(FirBinOp::Add, a1, a2, FirType::Int32);
        let add2 = b.binop(FirBinOp::Add, a1, a2, FirType::Int32);
        assert_eq!(add1, add2);
    }

    #[test]
    fn match_unknown_on_non_fir_node() {
        let mut store = FirStore::new();
        let raw = store.arena.int(999);
        assert_eq!(match_fir(&store, raw), FirMatch::Unknown);
        assert_eq!(store.value_type(raw), None);
    }
}
