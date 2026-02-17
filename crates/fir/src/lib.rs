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
//! # Parity invariants
//! - FIR nodes are represented as hash-consed trees in `tlib::TreeArena`.
//! - Identical FIR nodes are structurally shared automatically by interning.
//! - FIR value nodes carry explicit result types, so backend passes do not need
//!   a separate type-reconstruction phase.
//! - Dispatch is explicit and exhaustive via `match_fir`, no RTTI/dynamic-cast.

use tlib::{NodeKind, TreeArena, TreeId};

pub const CRATE_NAME: &str = "fir";

/// Stable crate identifier used in workspace-level tooling and diagnostics.
#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}

/// FIR node identifier in [`FirStore`].
pub type FirId = TreeId;

/// Memory-access class for FIR variable nodes.
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
#[derive(Clone, Debug, PartialEq)]
pub enum FirType {
    Int32,
    Int64,
    Float32,
    Float64,
    Bool,
    Void,
    Ptr(Box<FirType>),
    Array(Box<FirType>, usize),
    Vector(Box<FirType>, usize),
    Struct(String),
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
    pub init: f64,
    pub lo: f64,
    pub hi: f64,
    pub step: f64,
}

/// FIR storage using `tlib::TreeArena` hash-consing.
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
    #[must_use]
    pub fn new() -> Self {
        Self {
            arena: TreeArena::new(),
        }
    }

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
pub struct FirBuilder<'a> {
    store: &'a mut FirStore,
}

impl<'a> FirBuilder<'a> {
    #[must_use]
    pub fn new(store: &'a mut FirStore) -> Self {
        Self { store }
    }

    #[must_use]
    pub fn int32(&mut self, value: i32) -> FirId {
        let typ = encode_type(&mut self.store.arena, &FirType::Int32);
        let val = self.store.arena.int(i64::from(value));
        intern_tag(&mut self.store.arena, FIR_V_INT32_TAG, &[typ, val])
    }

    #[must_use]
    pub fn int64(&mut self, value: i64) -> FirId {
        let typ = encode_type(&mut self.store.arena, &FirType::Int64);
        let val = self.store.arena.int(value);
        intern_tag(&mut self.store.arena, FIR_V_INT64_TAG, &[typ, val])
    }

    #[must_use]
    pub fn float32(&mut self, value: f32) -> FirId {
        let typ = encode_type(&mut self.store.arena, &FirType::Float32);
        let bits = self.store.arena.int(i64::from(value.to_bits()));
        intern_tag(&mut self.store.arena, FIR_V_FLOAT32_TAG, &[typ, bits])
    }

    #[must_use]
    pub fn float64(&mut self, value: f64) -> FirId {
        let typ = encode_type(&mut self.store.arena, &FirType::Float64);
        let val = self.store.arena.float(value);
        intern_tag(&mut self.store.arena, FIR_V_FLOAT64_TAG, &[typ, val])
    }

    #[must_use]
    pub fn bool_(&mut self, value: bool) -> FirId {
        let typ = encode_type(&mut self.store.arena, &FirType::Bool);
        let val = self.store.arena.int(if value { 1 } else { 0 });
        intern_tag(&mut self.store.arena, FIR_V_BOOL_TAG, &[typ, val])
    }

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

    #[must_use]
    pub fn cast(&mut self, typ: FirType, value: FirId) -> FirId {
        let typ_id = encode_type(&mut self.store.arena, &typ);
        intern_tag(&mut self.store.arena, FIR_V_CAST_TAG, &[typ_id, value])
    }

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

    #[must_use]
    pub fn drop_(&mut self, value: FirId) -> FirId {
        intern_tag(&mut self.store.arena, FIR_DROP_TAG, &[value])
    }

    #[must_use]
    pub fn ret(&mut self, value: Option<FirId>) -> FirId {
        let value_id = value.unwrap_or_else(|| self.store.arena.nil());
        intern_tag(&mut self.store.arena, FIR_RETURN_TAG, &[value_id])
    }

    #[must_use]
    pub fn block(&mut self, body: &[FirId]) -> FirId {
        let list = encode_list(&mut self.store.arena, body);
        intern_tag(&mut self.store.arena, FIR_BLOCK_TAG, &[list])
    }

    #[must_use]
    pub fn if_(&mut self, cond: FirId, then_block: FirId, else_block: Option<FirId>) -> FirId {
        let else_id = else_block.unwrap_or_else(|| self.store.arena.nil());
        intern_tag(
            &mut self.store.arena,
            FIR_IF_TAG,
            &[cond, then_block, else_id],
        )
    }

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

    #[must_use]
    pub fn open_box(&mut self, typ: UiBoxType, label: impl Into<String>) -> FirId {
        let typ_id = encode_ui_box_type(&mut self.store.arena, typ);
        let label_id = self.store.arena.symbol(label);
        intern_tag(&mut self.store.arena, FIR_OPEN_BOX_TAG, &[typ_id, label_id])
    }

    #[must_use]
    pub fn close_box(&mut self) -> FirId {
        intern_tag(&mut self.store.arena, FIR_CLOSE_BOX_TAG, &[])
    }

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
    LoadVar {
        name: String,
        access: AccessType,
        typ: FirType,
    },
    BinOp {
        op: FirBinOp,
        lhs: FirId,
        rhs: FirId,
        typ: FirType,
    },
    Cast {
        typ: FirType,
        value: FirId,
    },
    FunCall {
        name: String,
        args: Vec<FirId>,
        typ: FirType,
    },
    DeclareVar {
        name: String,
        typ: FirType,
        access: AccessType,
        init: Option<FirId>,
    },
    StoreVar {
        name: String,
        access: AccessType,
        value: FirId,
    },
    Drop(FirId),
    Return(Option<FirId>),
    Block(Vec<FirId>),
    If {
        cond: FirId,
        then_block: FirId,
        else_block: Option<FirId>,
    },
    SimpleForLoop {
        var: String,
        upper: FirId,
        body: FirId,
        is_reverse: bool,
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
    AddMetaDeclare {
        var: String,
        key: String,
        value: String,
    },
    Label(String),
}

/// Decodes one [`FirId`] into canonical [`FirMatch`] shape.
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
        (FIR_V_CAST_TAG, [typ, value]) => {
            let Some(typ) = decode_type(&store.arena, *typ) else {
                return FirMatch::Unknown;
            };
            FirMatch::Cast { typ, value: *value }
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
        (FIR_DROP_TAG, [value]) => FirMatch::Drop(*value),
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
        _ => FirMatch::Unknown,
    }
}

const FIR_TYPE_INT32_TAG: &str = "FIRTYPE_INT32";
const FIR_TYPE_INT64_TAG: &str = "FIRTYPE_INT64";
const FIR_TYPE_FLOAT32_TAG: &str = "FIRTYPE_FLOAT32";
const FIR_TYPE_FLOAT64_TAG: &str = "FIRTYPE_FLOAT64";
const FIR_TYPE_BOOL_TAG: &str = "FIRTYPE_BOOL";
const FIR_TYPE_VOID_TAG: &str = "FIRTYPE_VOID";
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
const FIR_V_LOAD_VAR_TAG: &str = "FIRV_LOADVAR";
const FIR_V_BINOP_TAG: &str = "FIRV_BINOP";
const FIR_V_CAST_TAG: &str = "FIRV_CAST";
const FIR_V_FUNCALL_TAG: &str = "FIRV_FUNCALL";

const FIR_DECLARE_VAR_TAG: &str = "FIRST_DECLAREVAR";
const FIR_STORE_VAR_TAG: &str = "FIRST_STOREVAR";
const FIR_DROP_TAG: &str = "FIRST_DROP";
const FIR_RETURN_TAG: &str = "FIRST_RETURN";
const FIR_BLOCK_TAG: &str = "FIRST_BLOCK";
const FIR_IF_TAG: &str = "FIRST_IF";
const FIR_SIMPLE_FOR_LOOP_TAG: &str = "FIRST_SIMPLEFOR";
const FIR_OPEN_BOX_TAG: &str = "FIRST_OPENBOX";
const FIR_CLOSE_BOX_TAG: &str = "FIRST_CLOSEBOX";
const FIR_ADD_BUTTON_TAG: &str = "FIRST_ADDBUTTON";
const FIR_ADD_SLIDER_TAG: &str = "FIRST_ADDSLIDER";
const FIR_ADD_BARGRAPH_TAG: &str = "FIRST_ADDBARGRAPH";
const FIR_ADD_META_DECLARE_TAG: &str = "FIRST_ADDMETA";
const FIR_LABEL_TAG: &str = "FIRST_LABEL";

fn is_value_tag(tag: &str) -> bool {
    matches!(
        tag,
        FIR_V_INT32_TAG
            | FIR_V_INT64_TAG
            | FIR_V_FLOAT32_TAG
            | FIR_V_FLOAT64_TAG
            | FIR_V_BOOL_TAG
            | FIR_V_LOAD_VAR_TAG
            | FIR_V_BINOP_TAG
            | FIR_V_CAST_TAG
            | FIR_V_FUNCALL_TAG
    )
}

fn intern_tag(arena: &mut TreeArena, tag: &str, children: &[FirId]) -> FirId {
    let tag_id = arena.intern_tag(tag);
    arena.intern(NodeKind::Tag(tag_id), children)
}

fn encode_list(arena: &mut TreeArena, values: &[FirId]) -> FirId {
    let mut out = arena.nil();
    for value in values.iter().rev() {
        out = arena.cons(*value, out);
    }
    out
}

fn decode_list(arena: &TreeArena, mut list: FirId) -> Option<Vec<FirId>> {
    let mut out = Vec::new();
    while !arena.is_nil(list) {
        let head = arena.hd(list)?;
        out.push(head);
        list = arena.tl(list)?;
    }
    Some(out)
}

fn encode_type(arena: &mut TreeArena, typ: &FirType) -> FirId {
    match typ {
        FirType::Int32 => intern_tag(arena, FIR_TYPE_INT32_TAG, &[]),
        FirType::Int64 => intern_tag(arena, FIR_TYPE_INT64_TAG, &[]),
        FirType::Float32 => intern_tag(arena, FIR_TYPE_FLOAT32_TAG, &[]),
        FirType::Float64 => intern_tag(arena, FIR_TYPE_FLOAT64_TAG, &[]),
        FirType::Bool => intern_tag(arena, FIR_TYPE_BOOL_TAG, &[]),
        FirType::Void => intern_tag(arena, FIR_TYPE_VOID_TAG, &[]),
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
        FirType::Struct(name) => {
            let name_id = arena.symbol(name.clone());
            intern_tag(arena, FIR_TYPE_STRUCT_TAG, &[name_id])
        }
        FirType::Fun { args, ret } => {
            let args_ids: Vec<_> = args.iter().map(|a| encode_type(arena, a)).collect();
            let args_list = encode_list(arena, &args_ids);
            let ret_id = encode_type(arena, ret);
            intern_tag(arena, FIR_TYPE_FUN_TAG, &[args_list, ret_id])
        }
    }
}

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
        (FIR_TYPE_BOOL_TAG, []) => Some(FirType::Bool),
        (FIR_TYPE_VOID_TAG, []) => Some(FirType::Void),
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
        (FIR_TYPE_STRUCT_TAG, [name]) => Some(FirType::Struct(decode_symbol(arena, *name)?)),
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

fn encode_ui_box_type(arena: &mut TreeArena, typ: UiBoxType) -> FirId {
    arena.int(match typ {
        UiBoxType::Vertical => 0,
        UiBoxType::Horizontal => 1,
        UiBoxType::Tab => 2,
    })
}

fn decode_ui_box_type(arena: &TreeArena, id: FirId) -> Option<UiBoxType> {
    match decode_i64(arena, id)? {
        0 => Some(UiBoxType::Vertical),
        1 => Some(UiBoxType::Horizontal),
        2 => Some(UiBoxType::Tab),
        _ => None,
    }
}

fn encode_button_type(arena: &mut TreeArena, typ: ButtonType) -> FirId {
    arena.int(match typ {
        ButtonType::Button => 0,
        ButtonType::Checkbox => 1,
    })
}

fn decode_button_type(arena: &TreeArena, id: FirId) -> Option<ButtonType> {
    match decode_i64(arena, id)? {
        0 => Some(ButtonType::Button),
        1 => Some(ButtonType::Checkbox),
        _ => None,
    }
}

fn encode_slider_type(arena: &mut TreeArena, typ: SliderType) -> FirId {
    arena.int(match typ {
        SliderType::Horizontal => 0,
        SliderType::Vertical => 1,
        SliderType::NumEntry => 2,
    })
}

fn decode_slider_type(arena: &TreeArena, id: FirId) -> Option<SliderType> {
    match decode_i64(arena, id)? {
        0 => Some(SliderType::Horizontal),
        1 => Some(SliderType::Vertical),
        2 => Some(SliderType::NumEntry),
        _ => None,
    }
}

fn encode_bargraph_type(arena: &mut TreeArena, typ: BargraphType) -> FirId {
    arena.int(match typ {
        BargraphType::Horizontal => 0,
        BargraphType::Vertical => 1,
    })
}

fn decode_bargraph_type(arena: &TreeArena, id: FirId) -> Option<BargraphType> {
    match decode_i64(arena, id)? {
        0 => Some(BargraphType::Horizontal),
        1 => Some(BargraphType::Vertical),
        _ => None,
    }
}

fn decode_symbol(arena: &TreeArena, id: FirId) -> Option<String> {
    match arena.kind(id)? {
        NodeKind::Symbol(s) => Some(s.to_string()),
        NodeKind::StringLiteral(s) => Some(s.to_string()),
        _ => None,
    }
}

fn decode_i64(arena: &TreeArena, id: FirId) -> Option<i64> {
    match arena.kind(id)? {
        NodeKind::Int(v) => Some(*v),
        _ => None,
    }
}

fn decode_i32(arena: &TreeArena, id: FirId) -> Option<i32> {
    i32::try_from(decode_i64(arena, id)?).ok()
}

fn decode_f32_bits(arena: &TreeArena, id: FirId) -> Option<f32> {
    let bits = u32::try_from(decode_i64(arena, id)?).ok()?;
    Some(f32::from_bits(bits))
}

fn decode_f64(arena: &TreeArena, id: FirId) -> Option<f64> {
    match arena.kind(id)? {
        NodeKind::FloatBits(bits) => Some(f64::from_bits(*bits)),
        NodeKind::Int(v) => Some(*v as f64),
        _ => None,
    }
}

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
