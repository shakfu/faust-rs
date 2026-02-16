//! Signal construction and matching helpers backed by `tlib::TreeArena`.
//!
//! # Source provenance (C++)
//! - `compiler/signals/signals.hh`
//! - `compiler/signals/signals.cpp`
//! - `compiler/signals/binop.hh`
//!
//! # Public API mapping status
//! - Public construction API is [`SigBuilder`], aligned with the canonical
//!   `BoxBuilder` style used in `crates/boxes`.
//! - Public inspection API is [`match_sig`] + [`SigMatch`].
//!
//! # Parity invariants
//! - Signal nodes are represented as tagged trees with deterministic child order.
//! - Numeric constants are direct `Int` / `FloatBits` nodes.
//! - Slider parameter payload keeps Faust list encoding (`list4(init,min,max,step)`).

use std::fmt::Write;

use tlib::{NodeKind, TreeArena, TreeId};

pub const CRATE_NAME: &str = "signals";

/// Signal node identifier in `TreeArena`.
pub type SigId = TreeId;

const SIG_INPUT_TAG: &str = "SIGINPUT";
const SIG_OUTPUT_TAG: &str = "SIGOUTPUT";
const SIG_DELAY1_TAG: &str = "SIGDELAY1";
const SIG_DELAY_TAG: &str = "SIGDELAY";
const SIG_PREFIX_TAG: &str = "SIGPREFIX";
const SIG_INT_CAST_TAG: &str = "SIGINTCAST";
const SIG_BIT_CAST_TAG: &str = "SIGBITCAST";
const SIG_FLOAT_CAST_TAG: &str = "SIGFLOATCAST";
const SIG_RDTBL_TAG: &str = "SIGRDTBL";
const SIG_WRTBL_TAG: &str = "SIGWRTBL";
const SIG_GEN_TAG: &str = "SIGGEN";
const SIG_SELECT2_TAG: &str = "SIGSELECT2";
const SIG_ASSERT_BOUNDS_TAG: &str = "SIGASSERTBOUNDS";
const SIG_LOWEST_TAG: &str = "SIGLOWEST";
const SIG_HIGHEST_TAG: &str = "SIGHIGHEST";
const SIG_BINOP_TAG: &str = "SIGBINOP";
const SIG_FFUN_TAG: &str = "SIGFFUN";
const SIG_FCONST_TAG: &str = "SIGFCONST";
const SIG_FVAR_TAG: &str = "SIGFVAR";
const SIG_PROJ_TAG: &str = "SIGPROJ";
const SIG_BUTTON_TAG: &str = "SIGBUTTON";
const SIG_CHECKBOX_TAG: &str = "SIGCHECKBOX";
const SIG_VSLIDER_TAG: &str = "SIGVSLIDER";
const SIG_HSLIDER_TAG: &str = "SIGHSLIDER";
const SIG_NUMENTRY_TAG: &str = "SIGNUMENTRY";
const SIG_VBARGRAPH_TAG: &str = "SIGVBARGRAPH";
const SIG_HBARGRAPH_TAG: &str = "SIGHBARGRAPH";
const SIG_ATTACH_TAG: &str = "SIGATTACH";
const SIG_ENABLE_TAG: &str = "SIGENABLE";
const SIG_CONTROL_TAG: &str = "SIGCONTROL";
const SIG_WAVEFORM_TAG: &str = "SIGWAVEFORM";
const SIG_SOUNDFILE_TAG: &str = "SIGSOUNDFILE";
const SIG_SEQ_TAG: &str = "SIGSEQ";
const SIG_ZEROPAD_TAG: &str = "SIGZEROPAD";
const SIG_OD_TAG: &str = "SIGOD";
const SIG_US_TAG: &str = "SIGUS";
const SIG_DS_TAG: &str = "SIGDS";
const SIG_REC_TAG: &str = "SIGREC";

/// Stable crate identifier used in workspace-level tooling and diagnostics.
#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}

/// Binary signal operators (aligned with C++ `SOperator` order).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum BinOp {
    Add = 0,
    Sub = 1,
    Mul = 2,
    Div = 3,
    Rem = 4,
    Lsh = 5,
    ARsh = 6,
    LRsh = 7,
    Gt = 8,
    Lt = 9,
    Ge = 10,
    Le = 11,
    Eq = 12,
    Ne = 13,
    And = 14,
    Or = 15,
    Xor = 16,
}

impl BinOp {
    #[must_use]
    pub fn from_raw(raw: i64) -> Option<Self> {
        match raw {
            0 => Some(Self::Add),
            1 => Some(Self::Sub),
            2 => Some(Self::Mul),
            3 => Some(Self::Div),
            4 => Some(Self::Rem),
            5 => Some(Self::Lsh),
            6 => Some(Self::ARsh),
            7 => Some(Self::LRsh),
            8 => Some(Self::Gt),
            9 => Some(Self::Lt),
            10 => Some(Self::Ge),
            11 => Some(Self::Le),
            12 => Some(Self::Eq),
            13 => Some(Self::Ne),
            14 => Some(Self::And),
            15 => Some(Self::Or),
            16 => Some(Self::Xor),
            _ => None,
        }
    }

    #[must_use]
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Rem => "%",
            Self::Lsh => "<<",
            Self::ARsh => ">>",
            Self::LRsh => ">>>",
            Self::Gt => ">",
            Self::Lt => "<",
            Self::Ge => ">=",
            Self::Le => "<=",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::And => "&",
            Self::Or => "|",
            Self::Xor => "^",
        }
    }
}

/// Canonical builder API for constructing signal nodes.
pub struct SigBuilder<'a> {
    arena: &'a mut TreeArena,
}

impl<'a> SigBuilder<'a> {
    #[must_use]
    pub fn new(arena: &'a mut TreeArena) -> Self {
        Self { arena }
    }

    #[must_use]
    pub fn int(&mut self, n: i64) -> SigId {
        self.arena.int(n)
    }

    #[must_use]
    pub fn real(&mut self, r: f64) -> SigId {
        self.arena.float(r)
    }

    #[must_use]
    pub fn input(&mut self, index: i64) -> SigId {
        let idx = self.arena.int(index);
        intern_tag(self.arena, SIG_INPUT_TAG, &[idx])
    }

    #[must_use]
    pub fn output(&mut self, index: i64, sig: SigId) -> SigId {
        let idx = self.arena.int(index);
        intern_tag(self.arena, SIG_OUTPUT_TAG, &[idx, sig])
    }

    #[must_use]
    pub fn delay1(&mut self, sig: SigId) -> SigId {
        intern_tag(self.arena, SIG_DELAY1_TAG, &[sig])
    }

    #[must_use]
    pub fn delay(&mut self, sig: SigId, amount: SigId) -> SigId {
        intern_tag(self.arena, SIG_DELAY_TAG, &[sig, amount])
    }

    #[must_use]
    pub fn prefix(&mut self, init: SigId, sig: SigId) -> SigId {
        intern_tag(self.arena, SIG_PREFIX_TAG, &[init, sig])
    }

    #[must_use]
    pub fn int_cast(&mut self, sig: SigId) -> SigId {
        if matches!(self.arena.kind(sig), Some(NodeKind::Int(_))) {
            sig
        } else {
            intern_tag(self.arena, SIG_INT_CAST_TAG, &[sig])
        }
    }

    #[must_use]
    pub fn bit_cast(&mut self, sig: SigId) -> SigId {
        intern_tag(self.arena, SIG_BIT_CAST_TAG, &[sig])
    }

    #[must_use]
    pub fn float_cast(&mut self, sig: SigId) -> SigId {
        match self.arena.kind(sig) {
            Some(NodeKind::Int(v)) => self.arena.float(*v as f64),
            Some(NodeKind::FloatBits(_)) => sig,
            _ => intern_tag(self.arena, SIG_FLOAT_CAST_TAG, &[sig]),
        }
    }

    #[must_use]
    pub fn generate(&mut self, content: SigId) -> SigId {
        intern_tag(self.arena, SIG_GEN_TAG, &[content])
    }

    #[must_use]
    pub fn wrtbl(&mut self, size: SigId, generator: SigId, widx: SigId, wsig: SigId) -> SigId {
        intern_tag(self.arena, SIG_WRTBL_TAG, &[size, generator, widx, wsig])
    }

    #[must_use]
    pub fn wrtbl_readonly(&mut self, size: SigId, generator: SigId) -> SigId {
        let nil = self.arena.nil();
        self.wrtbl(size, generator, nil, nil)
    }

    #[must_use]
    pub fn rdtbl(&mut self, tbl: SigId, ridx: SigId) -> SigId {
        intern_tag(self.arena, SIG_RDTBL_TAG, &[tbl, ridx])
    }

    #[must_use]
    pub fn write_read_table(
        &mut self,
        size: SigId,
        init: SigId,
        widx: SigId,
        wsig: SigId,
        ridx: SigId,
    ) -> SigId {
        let generator = self.generate(init);
        let tbl = self.wrtbl(size, generator, widx, wsig);
        self.rdtbl(tbl, ridx)
    }

    #[must_use]
    pub fn read_only_table(&mut self, size: SigId, init: SigId, ridx: SigId) -> SigId {
        let generator = self.generate(init);
        let tbl = self.wrtbl_readonly(size, generator);
        self.rdtbl(tbl, ridx)
    }

    #[must_use]
    pub fn select2(&mut self, selector: SigId, s1: SigId, s2: SigId) -> SigId {
        intern_tag(self.arena, SIG_SELECT2_TAG, &[selector, s1, s2])
    }

    #[must_use]
    pub fn select3(&mut self, selector: SigId, s1: SigId, s2: SigId, s3: SigId) -> SigId {
        let zero = self.int(0);
        let one = self.int(1);
        let eq0 = self.eq(selector, zero);
        let eq1 = self.eq(selector, one);
        let inner = self.select2(eq1, s3, s2);
        self.select2(eq0, inner, s1)
    }

    #[must_use]
    pub fn assert_bounds(&mut self, s1: SigId, s2: SigId, s3: SigId) -> SigId {
        intern_tag(self.arena, SIG_ASSERT_BOUNDS_TAG, &[s1, s2, s3])
    }

    #[must_use]
    pub fn lowest(&mut self, sig: SigId) -> SigId {
        intern_tag(self.arena, SIG_LOWEST_TAG, &[sig])
    }

    #[must_use]
    pub fn highest(&mut self, sig: SigId) -> SigId {
        intern_tag(self.arena, SIG_HIGHEST_TAG, &[sig])
    }

    #[must_use]
    pub fn binop(&mut self, op: BinOp, x: SigId, y: SigId) -> SigId {
        let opn = self.arena.int(op as i64);
        intern_tag(self.arena, SIG_BINOP_TAG, &[opn, x, y])
    }

    #[must_use]
    pub fn add(&mut self, x: SigId, y: SigId) -> SigId {
        self.binop(BinOp::Add, x, y)
    }

    #[must_use]
    pub fn sub(&mut self, x: SigId, y: SigId) -> SigId {
        self.binop(BinOp::Sub, x, y)
    }

    #[must_use]
    pub fn mul(&mut self, x: SigId, y: SigId) -> SigId {
        self.binop(BinOp::Mul, x, y)
    }

    #[must_use]
    pub fn div(&mut self, x: SigId, y: SigId) -> SigId {
        self.binop(BinOp::Div, x, y)
    }

    #[must_use]
    pub fn rem(&mut self, x: SigId, y: SigId) -> SigId {
        self.binop(BinOp::Rem, x, y)
    }

    #[must_use]
    pub fn and(&mut self, x: SigId, y: SigId) -> SigId {
        self.binop(BinOp::And, x, y)
    }

    #[must_use]
    pub fn or(&mut self, x: SigId, y: SigId) -> SigId {
        self.binop(BinOp::Or, x, y)
    }

    #[must_use]
    pub fn xor(&mut self, x: SigId, y: SigId) -> SigId {
        self.binop(BinOp::Xor, x, y)
    }

    #[must_use]
    pub fn lsh(&mut self, x: SigId, y: SigId) -> SigId {
        self.binop(BinOp::Lsh, x, y)
    }

    #[must_use]
    pub fn arsh(&mut self, x: SigId, y: SigId) -> SigId {
        self.binop(BinOp::ARsh, x, y)
    }

    #[must_use]
    pub fn lrsh(&mut self, x: SigId, y: SigId) -> SigId {
        self.binop(BinOp::LRsh, x, y)
    }

    #[must_use]
    pub fn gt(&mut self, x: SigId, y: SigId) -> SigId {
        self.binop(BinOp::Gt, x, y)
    }

    #[must_use]
    pub fn lt(&mut self, x: SigId, y: SigId) -> SigId {
        self.binop(BinOp::Lt, x, y)
    }

    #[must_use]
    pub fn ge(&mut self, x: SigId, y: SigId) -> SigId {
        self.binop(BinOp::Ge, x, y)
    }

    #[must_use]
    pub fn le(&mut self, x: SigId, y: SigId) -> SigId {
        self.binop(BinOp::Le, x, y)
    }

    #[must_use]
    pub fn eq(&mut self, x: SigId, y: SigId) -> SigId {
        self.binop(BinOp::Eq, x, y)
    }

    #[must_use]
    pub fn ne(&mut self, x: SigId, y: SigId) -> SigId {
        self.binop(BinOp::Ne, x, y)
    }

    #[must_use]
    pub fn ffun(&mut self, ff: SigId, largs: SigId) -> SigId {
        intern_tag(self.arena, SIG_FFUN_TAG, &[ff, largs])
    }

    #[must_use]
    pub fn fconst(&mut self, ty: SigId, name: SigId, file: SigId) -> SigId {
        intern_tag(self.arena, SIG_FCONST_TAG, &[ty, name, file])
    }

    #[must_use]
    pub fn fvar(&mut self, ty: SigId, name: SigId, file: SigId) -> SigId {
        intern_tag(self.arena, SIG_FVAR_TAG, &[ty, name, file])
    }

    #[must_use]
    pub fn proj(&mut self, index: i64, group: SigId) -> SigId {
        let idx = self.arena.int(index);
        intern_tag(self.arena, SIG_PROJ_TAG, &[idx, group])
    }

    #[must_use]
    pub fn rec(&mut self, body: SigId) -> SigId {
        intern_tag(self.arena, SIG_REC_TAG, &[body])
    }

    #[must_use]
    pub fn button(&mut self, label: SigId) -> SigId {
        intern_tag(self.arena, SIG_BUTTON_TAG, &[label])
    }

    #[must_use]
    pub fn checkbox(&mut self, label: SigId) -> SigId {
        intern_tag(self.arena, SIG_CHECKBOX_TAG, &[label])
    }

    #[must_use]
    pub fn vslider(
        &mut self,
        label: SigId,
        init: SigId,
        min: SigId,
        max: SigId,
        step: SigId,
    ) -> SigId {
        let params = list4(self.arena, init, min, max, step);
        intern_tag(self.arena, SIG_VSLIDER_TAG, &[label, params])
    }

    #[must_use]
    pub fn hslider(
        &mut self,
        label: SigId,
        init: SigId,
        min: SigId,
        max: SigId,
        step: SigId,
    ) -> SigId {
        let params = list4(self.arena, init, min, max, step);
        intern_tag(self.arena, SIG_HSLIDER_TAG, &[label, params])
    }

    #[must_use]
    pub fn numentry(
        &mut self,
        label: SigId,
        init: SigId,
        min: SigId,
        max: SigId,
        step: SigId,
    ) -> SigId {
        let params = list4(self.arena, init, min, max, step);
        intern_tag(self.arena, SIG_NUMENTRY_TAG, &[label, params])
    }

    #[must_use]
    pub fn vbargraph(&mut self, label: SigId, min: SigId, max: SigId, sig: SigId) -> SigId {
        intern_tag(self.arena, SIG_VBARGRAPH_TAG, &[label, min, max, sig])
    }

    #[must_use]
    pub fn hbargraph(&mut self, label: SigId, min: SigId, max: SigId, sig: SigId) -> SigId {
        intern_tag(self.arena, SIG_HBARGRAPH_TAG, &[label, min, max, sig])
    }

    #[must_use]
    pub fn waveform(&mut self, values: &[SigId]) -> SigId {
        intern_tag(self.arena, SIG_WAVEFORM_TAG, values)
    }

    #[must_use]
    pub fn soundfile(&mut self, label: SigId) -> SigId {
        intern_tag(self.arena, SIG_SOUNDFILE_TAG, &[label])
    }

    #[must_use]
    pub fn attach(&mut self, x: SigId, y: SigId) -> SigId {
        intern_tag(self.arena, SIG_ATTACH_TAG, &[x, y])
    }

    #[must_use]
    pub fn enable(&mut self, x: SigId, y: SigId) -> SigId {
        intern_tag(self.arena, SIG_ENABLE_TAG, &[x, y])
    }

    #[must_use]
    pub fn control(&mut self, x: SigId, y: SigId) -> SigId {
        intern_tag(self.arena, SIG_CONTROL_TAG, &[x, y])
    }

    #[must_use]
    pub fn seq(&mut self, x: SigId, y: SigId) -> SigId {
        intern_tag(self.arena, SIG_SEQ_TAG, &[x, y])
    }

    #[must_use]
    pub fn zero_pad(&mut self, x: SigId, y: SigId) -> SigId {
        intern_tag(self.arena, SIG_ZEROPAD_TAG, &[x, y])
    }

    #[must_use]
    pub fn on_demand(&mut self, sigs: &[SigId]) -> SigId {
        intern_tag(self.arena, SIG_OD_TAG, sigs)
    }

    #[must_use]
    pub fn upsampling(&mut self, sigs: &[SigId]) -> SigId {
        intern_tag(self.arena, SIG_US_TAG, sigs)
    }

    #[must_use]
    pub fn downsampling(&mut self, sigs: &[SigId]) -> SigId {
        intern_tag(self.arena, SIG_DS_TAG, sigs)
    }
}

/// Signal structural matcher result.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SigMatch<'a> {
    Unknown,
    Int(i64),
    Real(f64),
    Input(i64),
    Output(i64, SigId),
    Delay1(SigId),
    Delay(SigId, SigId),
    Prefix(SigId, SigId),
    IntCast(SigId),
    BitCast(SigId),
    FloatCast(SigId),
    Gen(SigId),
    RdTbl(SigId, SigId),
    WrTbl(SigId, SigId, SigId, SigId),
    Select2(SigId, SigId, SigId),
    AssertBounds(SigId, SigId, SigId),
    Lowest(SigId),
    Highest(SigId),
    BinOp(BinOp, SigId, SigId),
    FFun(SigId, SigId),
    FConst(SigId, SigId, SigId),
    FVar(SigId, SigId, SigId),
    Proj(i64, SigId),
    Rec(SigId),
    Button(SigId),
    Checkbox(SigId),
    VSlider(SigId, SigId, SigId, SigId, SigId),
    HSlider(SigId, SigId, SigId, SigId, SigId),
    NumEntry(SigId, SigId, SigId, SigId, SigId),
    VBargraph(SigId, SigId, SigId, SigId),
    HBargraph(SigId, SigId, SigId, SigId),
    Attach(SigId, SigId),
    Enable(SigId, SigId),
    Control(SigId, SigId),
    Waveform(&'a [SigId]),
    Soundfile(SigId),
    Seq(SigId, SigId),
    ZeroPad(SigId, SigId),
    OnDemand(&'a [SigId]),
    Upsampling(&'a [SigId]),
    Downsampling(&'a [SigId]),
}

/// Decodes one `SigId` into canonical `SigMatch` shape.
#[must_use]
pub fn match_sig<'a>(arena: &'a TreeArena, id: SigId) -> SigMatch<'a> {
    let Some(node) = arena.node(id) else {
        return SigMatch::Unknown;
    };
    match &node.kind {
        NodeKind::Int(v) => SigMatch::Int(*v),
        NodeKind::FloatBits(bits) => SigMatch::Real(f64::from_bits(*bits)),
        NodeKind::Tag(tag_id) => {
            let tag = arena.tag_name(*tag_id).unwrap_or("");
            let ch = node.children.as_slice();
            match (tag, ch) {
                (SIG_INPUT_TAG, [idx]) => match arena.kind(*idx) {
                    Some(NodeKind::Int(i)) => SigMatch::Input(*i),
                    _ => SigMatch::Unknown,
                },
                (SIG_OUTPUT_TAG, [idx, s]) => match arena.kind(*idx) {
                    Some(NodeKind::Int(i)) => SigMatch::Output(*i, *s),
                    _ => SigMatch::Unknown,
                },
                (SIG_DELAY1_TAG, [s]) => SigMatch::Delay1(*s),
                (SIG_DELAY_TAG, [s0, s1]) => SigMatch::Delay(*s0, *s1),
                (SIG_PREFIX_TAG, [s0, s1]) => SigMatch::Prefix(*s0, *s1),
                (SIG_INT_CAST_TAG, [x]) => SigMatch::IntCast(*x),
                (SIG_BIT_CAST_TAG, [x]) => SigMatch::BitCast(*x),
                (SIG_FLOAT_CAST_TAG, [x]) => SigMatch::FloatCast(*x),
                (SIG_GEN_TAG, [x]) => SigMatch::Gen(*x),
                (SIG_RDTBL_TAG, [tbl, ri]) => SigMatch::RdTbl(*tbl, *ri),
                (SIG_WRTBL_TAG, [size, generator, wi, ws]) => {
                    SigMatch::WrTbl(*size, *generator, *wi, *ws)
                }
                (SIG_SELECT2_TAG, [selector, s1, s2]) => SigMatch::Select2(*selector, *s1, *s2),
                (SIG_ASSERT_BOUNDS_TAG, [s1, s2, s3]) => SigMatch::AssertBounds(*s1, *s2, *s3),
                (SIG_LOWEST_TAG, [s]) => SigMatch::Lowest(*s),
                (SIG_HIGHEST_TAG, [s]) => SigMatch::Highest(*s),
                (SIG_BINOP_TAG, [op, x, y]) => match arena.kind(*op) {
                    Some(NodeKind::Int(raw)) => match BinOp::from_raw(*raw) {
                        Some(bop) => SigMatch::BinOp(bop, *x, *y),
                        None => SigMatch::Unknown,
                    },
                    _ => SigMatch::Unknown,
                },
                (SIG_FFUN_TAG, [ff, largs]) => SigMatch::FFun(*ff, *largs),
                (SIG_FCONST_TAG, [ty, name, file]) => SigMatch::FConst(*ty, *name, *file),
                (SIG_FVAR_TAG, [ty, name, file]) => SigMatch::FVar(*ty, *name, *file),
                (SIG_PROJ_TAG, [idx, group]) => match arena.kind(*idx) {
                    Some(NodeKind::Int(i)) => SigMatch::Proj(*i, *group),
                    _ => SigMatch::Unknown,
                },
                (SIG_REC_TAG, [body]) => SigMatch::Rec(*body),
                (SIG_BUTTON_TAG, [lbl]) => SigMatch::Button(*lbl),
                (SIG_CHECKBOX_TAG, [lbl]) => SigMatch::Checkbox(*lbl),
                (SIG_VSLIDER_TAG, [lbl, params]) => {
                    let Some((init, min, max, step)) = slider_params4(arena, *params) else {
                        return SigMatch::Unknown;
                    };
                    SigMatch::VSlider(*lbl, init, min, max, step)
                }
                (SIG_HSLIDER_TAG, [lbl, params]) => {
                    let Some((init, min, max, step)) = slider_params4(arena, *params) else {
                        return SigMatch::Unknown;
                    };
                    SigMatch::HSlider(*lbl, init, min, max, step)
                }
                (SIG_NUMENTRY_TAG, [lbl, params]) => {
                    let Some((init, min, max, step)) = slider_params4(arena, *params) else {
                        return SigMatch::Unknown;
                    };
                    SigMatch::NumEntry(*lbl, init, min, max, step)
                }
                (SIG_VBARGRAPH_TAG, [lbl, min, max, x]) => {
                    SigMatch::VBargraph(*lbl, *min, *max, *x)
                }
                (SIG_HBARGRAPH_TAG, [lbl, min, max, x]) => {
                    SigMatch::HBargraph(*lbl, *min, *max, *x)
                }
                (SIG_ATTACH_TAG, [x, y]) => SigMatch::Attach(*x, *y),
                (SIG_ENABLE_TAG, [x, y]) => SigMatch::Enable(*x, *y),
                (SIG_CONTROL_TAG, [x, y]) => SigMatch::Control(*x, *y),
                (SIG_WAVEFORM_TAG, values) => SigMatch::Waveform(values),
                (SIG_SOUNDFILE_TAG, [label]) => SigMatch::Soundfile(*label),
                (SIG_SEQ_TAG, [x, y]) => SigMatch::Seq(*x, *y),
                (SIG_ZEROPAD_TAG, [x, y]) => SigMatch::ZeroPad(*x, *y),
                (SIG_OD_TAG, sigsubs) => SigMatch::OnDemand(sigsubs),
                (SIG_US_TAG, sigsubs) => SigMatch::Upsampling(sigsubs),
                (SIG_DS_TAG, sigsubs) => SigMatch::Downsampling(sigsubs),
                _ => SigMatch::Unknown,
            }
        }
        _ => SigMatch::Unknown,
    }
}

/// Deterministic structural dump helper for signal differential checks.
///
/// Output is shape-and-label based and intentionally excludes arena addresses.
#[must_use]
pub fn dump_sig(arena: &TreeArena, root: SigId) -> String {
    let mut out = String::new();
    dump_node(arena, root, &mut out);
    out
}

fn intern_tag(arena: &mut TreeArena, tag: &str, children: &[SigId]) -> SigId {
    let tag_id = arena.intern_tag(tag);
    arena.intern(NodeKind::Tag(tag_id), children)
}

fn list4(arena: &mut TreeArena, a: SigId, b: SigId, c: SigId, d: SigId) -> SigId {
    let nil = arena.nil();
    let l3 = arena.cons(d, nil);
    let l2 = arena.cons(c, l3);
    let l1 = arena.cons(b, l2);
    arena.cons(a, l1)
}

fn slider_params4(arena: &TreeArena, params: SigId) -> Option<(SigId, SigId, SigId, SigId)> {
    let n0 = arena.node(params)?;
    if !matches!(n0.kind, NodeKind::Cons) || n0.children.len() != 2 {
        return None;
    }
    let init = n0.children.get(0)?;

    let n1 = arena.node(n0.children.get(1)?)?;
    if !matches!(n1.kind, NodeKind::Cons) || n1.children.len() != 2 {
        return None;
    }
    let min = n1.children.get(0)?;

    let n2 = arena.node(n1.children.get(1)?)?;
    if !matches!(n2.kind, NodeKind::Cons) || n2.children.len() != 2 {
        return None;
    }
    let max = n2.children.get(0)?;

    let n3 = arena.node(n2.children.get(1)?)?;
    if !matches!(n3.kind, NodeKind::Cons) || n3.children.len() != 2 {
        return None;
    }
    let step = n3.children.get(0)?;

    Some((init, min, max, step))
}

fn dump_node(arena: &TreeArena, id: SigId, out: &mut String) {
    let Some(node) = arena.node(id) else {
        write!(out, "<invalid:{}>", id.as_u32()).expect("String write cannot fail");
        return;
    };

    match &node.kind {
        NodeKind::Nil => out.push_str("nil"),
        NodeKind::Cons => {
            out.push_str("cons(");
            if let Some(head) = node.children.get(0) {
                dump_node(arena, head, out);
            } else {
                out.push_str("<missing>");
            }
            out.push_str(", ");
            if let Some(tail) = node.children.get(1) {
                dump_node(arena, tail, out);
            } else {
                out.push_str("<missing>");
            }
            out.push(')');
        }
        NodeKind::Symbol(name) => {
            write!(out, "sym({name:?})").expect("String write cannot fail");
        }
        NodeKind::StringLiteral(value) => {
            write!(out, "str({value:?})").expect("String write cannot fail");
        }
        NodeKind::Int(value) => {
            write!(out, "int({value})").expect("String write cannot fail");
        }
        NodeKind::FloatBits(bits) => {
            write!(out, "float_bits(0x{bits:016x})").expect("String write cannot fail");
        }
        NodeKind::Tag(tag_id) => {
            let tag_name = arena.tag_name(*tag_id).unwrap_or("<unknown-tag>");
            write!(out, "{tag_name}(").expect("String write cannot fail");
            for (idx, child) in node.children.as_slice().iter().enumerate() {
                if idx > 0 {
                    out.push_str(", ");
                }
                dump_node(arena, *child, out);
            }
            out.push(')');
        }
    }
}
