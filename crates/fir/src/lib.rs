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
//! - FIR nodes are represented as typed enum variants with explicit IDs.
//! - FIR value nodes carry explicit result types, so backend passes do not need
//!   a separate type-reconstruction phase.
//! - Dispatch is explicit and exhaustive via `match_fir`, no RTTI/dynamic-cast.
//! - This crate is independent from `tlib` and `signals`.

pub const CRATE_NAME: &str = "fir";

/// Stable crate identifier used in workspace-level tooling and diagnostics.
#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}

/// FIR node identifier in [`FirStore`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FirId(u32);

impl FirId {
    fn as_index(self) -> usize {
        self.0 as usize
    }
}

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

/// Typed FIR value node.
///
/// # C++ provenance
/// - `compiler/generator/instructions.hh` (`ValueInst` hierarchy)
/// - `compiler/generator/typing_instructions.hh` (type reconstruction pass)
///
/// Rust adaptation:
/// - carries the resulting `typ` directly on value construction to avoid a
///   separate type-reconstruction pass for backend consumers.
#[derive(Clone, Debug, PartialEq)]
pub struct FirValue {
    pub typ: FirType,
    pub kind: FirValueKind,
}

/// Structural kind of a typed FIR value node.
#[derive(Clone, Debug, PartialEq)]
pub enum FirValueKind {
    Int32(i32),
    Int64(i64),
    Float32(f32),
    Float64(f64),
    Bool(bool),
    LoadVar {
        name: String,
        access: AccessType,
    },
    BinOp {
        op: FirBinOp,
        lhs: FirId,
        rhs: FirId,
    },
    Cast {
        value: FirId,
    },
    FunCall {
        name: String,
        args: Vec<FirId>,
    },
}

/// Canonical FIR node representation.
#[derive(Clone, Debug, PartialEq)]
pub enum FirNode {
    Value(FirValue),
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

/// Arena-like FIR storage with stable IDs.
#[derive(Clone, Debug, Default)]
pub struct FirStore {
    nodes: Vec<FirNode>,
}

impl FirStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    #[must_use]
    pub fn node(&self, id: FirId) -> Option<&FirNode> {
        self.nodes.get(id.as_index())
    }

    /// Returns the value type when `id` points to a value node.
    #[must_use]
    pub fn value_type(&self, id: FirId) -> Option<&FirType> {
        match self.node(id) {
            Some(FirNode::Value(value)) => Some(&value.typ),
            _ => None,
        }
    }

    fn push(&mut self, node: FirNode) -> FirId {
        let idx = self.nodes.len();
        self.nodes.push(node);
        FirId(idx as u32)
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

    fn value(&mut self, typ: FirType, kind: FirValueKind) -> FirId {
        self.store.push(FirNode::Value(FirValue { typ, kind }))
    }

    #[must_use]
    pub fn int32(&mut self, value: i32) -> FirId {
        self.value(FirType::Int32, FirValueKind::Int32(value))
    }

    #[must_use]
    pub fn int64(&mut self, value: i64) -> FirId {
        self.value(FirType::Int64, FirValueKind::Int64(value))
    }

    #[must_use]
    pub fn float32(&mut self, value: f32) -> FirId {
        self.value(FirType::Float32, FirValueKind::Float32(value))
    }

    #[must_use]
    pub fn float64(&mut self, value: f64) -> FirId {
        self.value(FirType::Float64, FirValueKind::Float64(value))
    }

    #[must_use]
    pub fn bool_(&mut self, value: bool) -> FirId {
        self.value(FirType::Bool, FirValueKind::Bool(value))
    }

    #[must_use]
    pub fn load_var(&mut self, name: impl Into<String>, access: AccessType, typ: FirType) -> FirId {
        self.value(
            typ,
            FirValueKind::LoadVar {
                name: name.into(),
                access,
            },
        )
    }

    #[must_use]
    pub fn binop(&mut self, op: FirBinOp, lhs: FirId, rhs: FirId, typ: FirType) -> FirId {
        self.value(typ, FirValueKind::BinOp { op, lhs, rhs })
    }

    #[must_use]
    pub fn cast(&mut self, typ: FirType, value: FirId) -> FirId {
        self.value(typ, FirValueKind::Cast { value })
    }

    #[must_use]
    pub fn fun_call(&mut self, name: impl Into<String>, args: &[FirId], typ: FirType) -> FirId {
        self.value(
            typ,
            FirValueKind::FunCall {
                name: name.into(),
                args: args.to_vec(),
            },
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
        self.store.push(FirNode::DeclareVar {
            name: name.into(),
            typ,
            access,
            init,
        })
    }

    #[must_use]
    pub fn store_var(
        &mut self,
        name: impl Into<String>,
        access: AccessType,
        value: FirId,
    ) -> FirId {
        self.store.push(FirNode::StoreVar {
            name: name.into(),
            access,
            value,
        })
    }

    #[must_use]
    pub fn drop_(&mut self, value: FirId) -> FirId {
        self.store.push(FirNode::Drop(value))
    }

    #[must_use]
    pub fn ret(&mut self, value: Option<FirId>) -> FirId {
        self.store.push(FirNode::Return(value))
    }

    #[must_use]
    pub fn block(&mut self, body: &[FirId]) -> FirId {
        self.store.push(FirNode::Block(body.to_vec()))
    }

    #[must_use]
    pub fn if_(&mut self, cond: FirId, then_block: FirId, else_block: Option<FirId>) -> FirId {
        self.store.push(FirNode::If {
            cond,
            then_block,
            else_block,
        })
    }

    #[must_use]
    pub fn simple_for_loop(
        &mut self,
        var: impl Into<String>,
        upper: FirId,
        body: FirId,
        is_reverse: bool,
    ) -> FirId {
        self.store.push(FirNode::SimpleForLoop {
            var: var.into(),
            upper,
            body,
            is_reverse,
        })
    }

    #[must_use]
    pub fn open_box(&mut self, typ: UiBoxType, label: impl Into<String>) -> FirId {
        self.store.push(FirNode::OpenBox {
            typ,
            label: label.into(),
        })
    }

    #[must_use]
    pub fn close_box(&mut self) -> FirId {
        self.store.push(FirNode::CloseBox)
    }

    #[must_use]
    pub fn add_button(
        &mut self,
        typ: ButtonType,
        label: impl Into<String>,
        var: impl Into<String>,
    ) -> FirId {
        self.store.push(FirNode::AddButton {
            typ,
            label: label.into(),
            var: var.into(),
        })
    }

    #[must_use]
    pub fn add_slider(
        &mut self,
        typ: SliderType,
        label: impl Into<String>,
        var: impl Into<String>,
        range: SliderRange,
    ) -> FirId {
        self.store.push(FirNode::AddSlider {
            typ,
            label: label.into(),
            var: var.into(),
            init: range.init,
            lo: range.lo,
            hi: range.hi,
            step: range.step,
        })
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
        self.store.push(FirNode::AddBargraph {
            typ,
            label: label.into(),
            var: var.into(),
            lo,
            hi,
        })
    }

    #[must_use]
    pub fn add_meta_declare(
        &mut self,
        var: impl Into<String>,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> FirId {
        self.store.push(FirNode::AddMetaDeclare {
            var: var.into(),
            key: key.into(),
            value: value.into(),
        })
    }

    #[must_use]
    pub fn label(&mut self, label: impl Into<String>) -> FirId {
        self.store.push(FirNode::Label(label.into()))
    }
}

/// FIR structural matcher result.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FirMatch<'a> {
    Unknown,
    Int32 {
        value: i32,
        typ: &'a FirType,
    },
    Int64 {
        value: i64,
        typ: &'a FirType,
    },
    Float32 {
        value: f32,
        typ: &'a FirType,
    },
    Float64 {
        value: f64,
        typ: &'a FirType,
    },
    Bool {
        value: bool,
        typ: &'a FirType,
    },
    LoadVar {
        name: &'a str,
        access: AccessType,
        typ: &'a FirType,
    },
    BinOp {
        op: FirBinOp,
        lhs: FirId,
        rhs: FirId,
        typ: &'a FirType,
    },
    Cast {
        typ: &'a FirType,
        value: FirId,
    },
    FunCall {
        name: &'a str,
        args: &'a [FirId],
        typ: &'a FirType,
    },
    DeclareVar {
        name: &'a str,
        typ: &'a FirType,
        access: AccessType,
        init: Option<FirId>,
    },
    StoreVar {
        name: &'a str,
        access: AccessType,
        value: FirId,
    },
    Drop(FirId),
    Return(Option<FirId>),
    Block(&'a [FirId]),
    If {
        cond: FirId,
        then_block: FirId,
        else_block: Option<FirId>,
    },
    SimpleForLoop {
        var: &'a str,
        upper: FirId,
        body: FirId,
        is_reverse: bool,
    },
    OpenBox {
        typ: UiBoxType,
        label: &'a str,
    },
    CloseBox,
    AddButton {
        typ: ButtonType,
        label: &'a str,
        var: &'a str,
    },
    AddSlider {
        typ: SliderType,
        label: &'a str,
        var: &'a str,
        init: f64,
        lo: f64,
        hi: f64,
        step: f64,
    },
    AddBargraph {
        typ: BargraphType,
        label: &'a str,
        var: &'a str,
        lo: f64,
        hi: f64,
    },
    AddMetaDeclare {
        var: &'a str,
        key: &'a str,
        value: &'a str,
    },
    Label(&'a str),
}

/// Decodes one [`FirId`] into canonical [`FirMatch`] shape.
#[must_use]
pub fn match_fir<'a>(store: &'a FirStore, id: FirId) -> FirMatch<'a> {
    let Some(node) = store.node(id) else {
        return FirMatch::Unknown;
    };
    match node {
        FirNode::Value(value) => match &value.kind {
            FirValueKind::Int32(v) => FirMatch::Int32 {
                value: *v,
                typ: &value.typ,
            },
            FirValueKind::Int64(v) => FirMatch::Int64 {
                value: *v,
                typ: &value.typ,
            },
            FirValueKind::Float32(v) => FirMatch::Float32 {
                value: *v,
                typ: &value.typ,
            },
            FirValueKind::Float64(v) => FirMatch::Float64 {
                value: *v,
                typ: &value.typ,
            },
            FirValueKind::Bool(v) => FirMatch::Bool {
                value: *v,
                typ: &value.typ,
            },
            FirValueKind::LoadVar { name, access } => FirMatch::LoadVar {
                name,
                access: *access,
                typ: &value.typ,
            },
            FirValueKind::BinOp { op, lhs, rhs } => FirMatch::BinOp {
                op: *op,
                lhs: *lhs,
                rhs: *rhs,
                typ: &value.typ,
            },
            FirValueKind::Cast { value: casted } => FirMatch::Cast {
                typ: &value.typ,
                value: *casted,
            },
            FirValueKind::FunCall { name, args } => FirMatch::FunCall {
                name,
                args,
                typ: &value.typ,
            },
        },
        FirNode::DeclareVar {
            name,
            typ,
            access,
            init,
        } => FirMatch::DeclareVar {
            name,
            typ,
            access: *access,
            init: *init,
        },
        FirNode::StoreVar {
            name,
            access,
            value,
        } => FirMatch::StoreVar {
            name,
            access: *access,
            value: *value,
        },
        FirNode::Drop(value) => FirMatch::Drop(*value),
        FirNode::Return(value) => FirMatch::Return(*value),
        FirNode::Block(body) => FirMatch::Block(body),
        FirNode::If {
            cond,
            then_block,
            else_block,
        } => FirMatch::If {
            cond: *cond,
            then_block: *then_block,
            else_block: *else_block,
        },
        FirNode::SimpleForLoop {
            var,
            upper,
            body,
            is_reverse,
        } => FirMatch::SimpleForLoop {
            var,
            upper: *upper,
            body: *body,
            is_reverse: *is_reverse,
        },
        FirNode::OpenBox { typ, label } => FirMatch::OpenBox { typ: *typ, label },
        FirNode::CloseBox => FirMatch::CloseBox,
        FirNode::AddButton { typ, label, var } => FirMatch::AddButton {
            typ: *typ,
            label,
            var,
        },
        FirNode::AddSlider {
            typ,
            label,
            var,
            init,
            lo,
            hi,
            step,
        } => FirMatch::AddSlider {
            typ: *typ,
            label,
            var,
            init: *init,
            lo: *lo,
            hi: *hi,
            step: *step,
        },
        FirNode::AddBargraph {
            typ,
            label,
            var,
            lo,
            hi,
        } => FirMatch::AddBargraph {
            typ: *typ,
            label,
            var,
            lo: *lo,
            hi: *hi,
        },
        FirNode::AddMetaDeclare { var, key, value } => FirMatch::AddMetaDeclare { var, key, value },
        FirNode::Label(label) => FirMatch::Label(label),
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
                typ: &FirType::Int32
            }
        );
        assert_eq!(
            match_fir(&store, sum),
            FirMatch::BinOp {
                op: FirBinOp::Add,
                lhs: one,
                rhs: two,
                typ: &FirType::Int32
            }
        );
        assert_eq!(
            match_fir(&store, call),
            FirMatch::FunCall {
                name: "foo",
                args: &[sum],
                typ: &FirType::Int32
            }
        );
        assert_eq!(
            match_fir(&store, cast),
            FirMatch::Cast {
                typ: &FirType::Float64,
                value: call
            }
        );
        assert_eq!(store.value_type(cast), Some(&FirType::Float64));
        assert_eq!(store.value_type(sum), Some(&FirType::Int32));
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
                name: "acc",
                typ: &FirType::Int32,
                access: AccessType::Stack,
                init: Some(zero)
            }
        );
        assert_eq!(
            match_fir(&store, loop_),
            FirMatch::SimpleForLoop {
                var: "i",
                upper,
                body,
                is_reverse: false
            }
        );
        assert_eq!(match_fir(&store, block), FirMatch::Block(&[loop_, ret]));
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
                label: "osc"
            }
        );
        assert_eq!(
            match_fir(&store, slider),
            FirMatch::AddSlider {
                typ: SliderType::Horizontal,
                label: "freq",
                var: "fHslider0",
                init: 440.0,
                lo: 20.0,
                hi: 20_000.0,
                step: 1.0
            }
        );
        assert_eq!(
            match_fir(&store, block),
            FirMatch::Block(&[open, slider, close])
        );
    }

    #[test]
    fn match_unknown_on_out_of_range_id() {
        let store = FirStore::new();
        assert_eq!(match_fir(&store, FirId(999)), FirMatch::Unknown);
        assert_eq!(store.value_type(FirId(999)), None);
    }
}
