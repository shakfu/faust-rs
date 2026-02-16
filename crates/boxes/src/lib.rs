//! Box construction helpers backed by `tlib::TreeArena`.
//!
//! # Source provenance (C++)
//! - `compiler/boxes/boxes.hh`
//! - `compiler/boxes/boxes.cpp`
//!
//! # Public API mapping status
//! - Public construction API is `BoxBuilder`, which keeps 1:1 semantic mapping with
//!   the C++ box families (`box*` constructors in `compiler/boxes/boxes.hh/.cpp`).
//! - Public inspection API is `match_box` + `BoxMatch`.
//! - Legacy `node_*` / `is_node_*` helpers are kept internal to this crate.
//!
//! # Parity invariants
//! - Box nodes are represented as tagged trees with deterministic child order.
//! - Labels/identifiers are carried as `NodeKind::Symbol`.
//! - UI slider parameter payload keeps Faust list encoding (`list4(cur,min,max,step)`).

use std::fmt::Write;

use tlib::{NodeKind, TreeArena, TreeId};

pub const CRATE_NAME: &str = "boxes";

/// Box node identifier in `TreeArena`.
pub type BoxId = TreeId;

const BOX_IDENT_TAG: &str = "BOXIDENT";
const BOX_WIRE_TAG: &str = "BOXWIRE";
const BOX_CUT_TAG: &str = "BOXCUT";
const BOX_SEQ_TAG: &str = "BOXSEQ";
const BOX_PAR_TAG: &str = "BOXPAR";
const BOX_REC_TAG: &str = "BOXREC";
const BOX_SPLIT_TAG: &str = "BOXSPLIT";
const BOX_MERGE_TAG: &str = "BOXMERGE";
const BOX_APPL_TAG: &str = "BOXAPPL";
const BOX_ACCESS_TAG: &str = "BOXACCESS";
const BOX_ADD_TAG: &str = "BOXADD";
const BOX_SUB_TAG: &str = "BOXSUB";
const BOX_MUL_TAG: &str = "BOXMUL";
const BOX_DIV_TAG: &str = "BOXDIV";
const BOX_REM_TAG: &str = "BOXREM";
const BOX_AND_TAG: &str = "BOXAND";
const BOX_OR_TAG: &str = "BOXOR";
const BOX_XOR_TAG: &str = "BOXXOR";
const BOX_LSH_TAG: &str = "BOXLSH";
const BOX_RSH_TAG: &str = "BOXRSH";
const BOX_LT_TAG: &str = "BOXLT";
const BOX_LE_TAG: &str = "BOXLE";
const BOX_GT_TAG: &str = "BOXGT";
const BOX_GE_TAG: &str = "BOXGE";
const BOX_EQ_TAG: &str = "BOXEQ";
const BOX_NE_TAG: &str = "BOXNE";
const BOX_POW_TAG: &str = "BOXPOW";
const BOX_DELAY_TAG: &str = "BOXDELAY";
const BOX_DELAY1_TAG: &str = "BOXDELAY1";
const BOX_MIN_TAG: &str = "BOXMIN";
const BOX_MAX_TAG: &str = "BOXMAX";
const BOX_PREFIX_TAG: &str = "BOXPREFIX";
const BOX_INT_CAST_TAG: &str = "BOXINTCAST";
const BOX_FLOAT_CAST_TAG: &str = "BOXFLOATCAST";
const BOX_READ_ONLY_TABLE_TAG: &str = "BOXRDTBL";
const BOX_WRITE_READ_TABLE_TAG: &str = "BOXRWTBL";
const BOX_SELECT2_TAG: &str = "BOXSELECT2";
const BOX_SELECT3_TAG: &str = "BOXSELECT3";
const BOX_ASSERT_BOUNDS_TAG: &str = "BOXASSERTBOUNDS";
const BOX_LOWEST_TAG: &str = "BOXLOWEST";
const BOX_HIGHEST_TAG: &str = "BOXHIGHEST";
const BOX_ATTACH_TAG: &str = "BOXATTACH";
const BOX_ENABLE_TAG: &str = "BOXENABLE";
const BOX_CONTROL_TAG: &str = "BOXCONTROL";
const BOX_IPAR_TAG: &str = "BOXIPAR";
const BOX_ISEQ_TAG: &str = "BOXISEQ";
const BOX_ISUM_TAG: &str = "BOXISUM";
const BOX_IPROD_TAG: &str = "BOXIPROD";
const BOX_WITH_LOCAL_DEF_TAG: &str = "BOXWITHLOCALDEF";
const BOX_WITH_REC_DEF_TAG: &str = "BOXWITHRECDEF";
const BOX_ENVIRONMENT_TAG: &str = "BOXENVIRONMENT";
const BOX_COMPONENT_TAG: &str = "BOXCOMPONENT";
const BOX_LIBRARY_TAG: &str = "BOXLIBRARY";
const BOX_WAVEFORM_TAG: &str = "BOXWAVEFORM";
const BOX_ROUTE_TAG: &str = "BOXROUTE";
const FFUN_TAG: &str = "FFUN";
const BOX_FFUN_TAG: &str = "BOXFFUN";
const BOX_FCONST_TAG: &str = "BOXFCONST";
const BOX_FVAR_TAG: &str = "BOXFVAR";
const BOX_CASE_TAG: &str = "BOXCASE";
const BOX_PATTERN_VAR_TAG: &str = "BOXPATVAR";
const BOX_ABSTR_TAG: &str = "BOXABSTR";
const BOX_MODULATION_TAG: &str = "BOXMODULATION";
const BOX_INPUTS_TAG: &str = "BOXINPUTS";
const BOX_OUTPUTS_TAG: &str = "BOXOUTPUTS";
const BOX_ONDEMAND_TAG: &str = "BOXONDEMAND";
const BOX_UPSAMPLING_TAG: &str = "BOXUPSAMPLING";
const BOX_DOWNSAMPLING_TAG: &str = "BOXDOWNSAMPLING";
const BOX_BUTTON_TAG: &str = "BOXBUTTON";
const BOX_CHECKBOX_TAG: &str = "BOXCHECKBOX";
const BOX_VSLIDER_TAG: &str = "BOXVSLIDER";
const BOX_HSLIDER_TAG: &str = "BOXHSLIDER";
const BOX_NUM_ENTRY_TAG: &str = "BOXNUMENTRY";
const BOX_VGROUP_TAG: &str = "BOXVGROUP";
const BOX_HGROUP_TAG: &str = "BOXHGROUP";
const BOX_TGROUP_TAG: &str = "BOXTGROUP";
const BOX_VBARGRAPH_TAG: &str = "BOXVBARGRAPH";
const BOX_HBARGRAPH_TAG: &str = "BOXHBARGRAPH";
const BOX_SOUNDFILE_TAG: &str = "BOXSOUNDFILE";

/// Stable crate identifier used in workspace-level tooling and diagnostics.
#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}

/// Canonical builder API for constructing box nodes.
///
/// This is the preferred Rust API for new code.
pub struct BoxBuilder<'a> {
    arena: &'a mut TreeArena,
}

impl<'a> BoxBuilder<'a> {
    #[must_use]
    pub fn new(arena: &'a mut TreeArena) -> Self {
        Self { arena }
    }

    #[must_use]
    pub fn ident(&mut self, name: &str) -> BoxId {
        node_ident(self.arena, name)
    }

    #[must_use]
    pub fn int(&mut self, value: i64) -> BoxId {
        node_int(self.arena, value)
    }

    #[must_use]
    pub fn real(&mut self, value: f64) -> BoxId {
        node_real(self.arena, value)
    }

    #[must_use]
    pub fn wire(&mut self) -> BoxId {
        node_wire(self.arena)
    }

    #[must_use]
    pub fn cut(&mut self) -> BoxId {
        node_cut(self.arena)
    }

    #[must_use]
    pub fn seq(&mut self, left: BoxId, right: BoxId) -> BoxId {
        node_seq(self.arena, left, right)
    }

    #[must_use]
    pub fn par(&mut self, left: BoxId, right: BoxId) -> BoxId {
        node_par(self.arena, left, right)
    }

    #[must_use]
    pub fn rec(&mut self, left: BoxId, right: BoxId) -> BoxId {
        node_rec(self.arena, left, right)
    }

    #[must_use]
    pub fn split(&mut self, left: BoxId, right: BoxId) -> BoxId {
        node_split(self.arena, left, right)
    }

    #[must_use]
    pub fn merge(&mut self, left: BoxId, right: BoxId) -> BoxId {
        node_merge(self.arena, left, right)
    }

    #[must_use]
    pub fn appl(&mut self, fun: BoxId, arglist: BoxId) -> BoxId {
        node_appl(self.arena, fun, arglist)
    }

    #[must_use]
    pub fn access(&mut self, expr: BoxId, ident: BoxId) -> BoxId {
        node_access(self.arena, expr, ident)
    }

    #[must_use]
    pub fn add(&mut self) -> BoxId {
        node_add(self.arena)
    }

    #[must_use]
    pub fn sub(&mut self) -> BoxId {
        node_sub(self.arena)
    }

    #[must_use]
    pub fn mul(&mut self) -> BoxId {
        node_mul(self.arena)
    }

    #[must_use]
    pub fn div(&mut self) -> BoxId {
        node_div(self.arena)
    }

    #[must_use]
    pub fn rem(&mut self) -> BoxId {
        node_rem(self.arena)
    }

    #[must_use]
    pub fn and(&mut self) -> BoxId {
        node_and(self.arena)
    }

    #[must_use]
    pub fn or(&mut self) -> BoxId {
        node_or(self.arena)
    }

    #[must_use]
    pub fn xor(&mut self) -> BoxId {
        node_xor(self.arena)
    }

    #[must_use]
    pub fn lsh(&mut self) -> BoxId {
        node_lsh(self.arena)
    }

    #[must_use]
    pub fn rsh(&mut self) -> BoxId {
        node_rsh(self.arena)
    }

    #[must_use]
    pub fn lt(&mut self) -> BoxId {
        node_lt(self.arena)
    }

    #[must_use]
    pub fn le(&mut self) -> BoxId {
        node_le(self.arena)
    }

    #[must_use]
    pub fn gt(&mut self) -> BoxId {
        node_gt(self.arena)
    }

    #[must_use]
    pub fn ge(&mut self) -> BoxId {
        node_ge(self.arena)
    }

    #[must_use]
    pub fn eq(&mut self) -> BoxId {
        node_eq(self.arena)
    }

    #[must_use]
    pub fn ne(&mut self) -> BoxId {
        node_ne(self.arena)
    }

    #[must_use]
    pub fn pow(&mut self) -> BoxId {
        node_pow(self.arena)
    }

    #[must_use]
    pub fn delay(&mut self) -> BoxId {
        node_delay(self.arena)
    }

    #[must_use]
    pub fn delay1(&mut self) -> BoxId {
        node_delay1(self.arena)
    }

    #[must_use]
    pub fn min(&mut self) -> BoxId {
        node_min(self.arena)
    }

    #[must_use]
    pub fn max(&mut self) -> BoxId {
        node_max(self.arena)
    }

    #[must_use]
    pub fn prefix(&mut self) -> BoxId {
        node_prefix(self.arena)
    }

    #[must_use]
    pub fn int_cast(&mut self) -> BoxId {
        node_int_cast(self.arena)
    }

    #[must_use]
    pub fn float_cast(&mut self) -> BoxId {
        node_float_cast(self.arena)
    }

    #[must_use]
    pub fn read_only_table(&mut self) -> BoxId {
        node_read_only_table(self.arena)
    }

    #[must_use]
    pub fn write_read_table(&mut self) -> BoxId {
        node_write_read_table(self.arena)
    }

    #[must_use]
    pub fn select2(&mut self) -> BoxId {
        node_select2(self.arena)
    }

    #[must_use]
    pub fn select3(&mut self) -> BoxId {
        node_select3(self.arena)
    }

    #[must_use]
    pub fn assert_bounds(&mut self) -> BoxId {
        node_assert_bounds(self.arena)
    }

    #[must_use]
    pub fn lowest(&mut self) -> BoxId {
        node_lowest(self.arena)
    }

    #[must_use]
    pub fn highest(&mut self) -> BoxId {
        node_highest(self.arena)
    }

    #[must_use]
    pub fn attach(&mut self) -> BoxId {
        node_attach(self.arena)
    }

    #[must_use]
    pub fn enable(&mut self) -> BoxId {
        node_enable(self.arena)
    }

    #[must_use]
    pub fn control(&mut self) -> BoxId {
        node_control(self.arena)
    }

    #[must_use]
    pub fn ipar(&mut self, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
        node_ipar(self.arena, index, count, body)
    }

    #[must_use]
    pub fn iseq(&mut self, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
        node_iseq(self.arena, index, count, body)
    }

    #[must_use]
    pub fn isum(&mut self, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
        node_isum(self.arena, index, count, body)
    }

    #[must_use]
    pub fn iprod(&mut self, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
        node_iprod(self.arena, index, count, body)
    }

    #[must_use]
    pub fn with_local_def(&mut self, body: BoxId, ldef: BoxId) -> BoxId {
        node_with_local_def(self.arena, body, ldef)
    }

    #[must_use]
    pub fn with_rec_def(&mut self, body: BoxId, ldef: BoxId, ldef2: BoxId) -> BoxId {
        node_with_rec_def(self.arena, body, ldef, ldef2)
    }

    #[must_use]
    pub fn environment(&mut self) -> BoxId {
        node_environment(self.arena)
    }

    #[must_use]
    pub fn component(&mut self, filename: BoxId) -> BoxId {
        node_component(self.arena, filename)
    }

    #[must_use]
    pub fn library(&mut self, filename: BoxId) -> BoxId {
        node_library(self.arena, filename)
    }

    #[must_use]
    pub fn waveform(&mut self, values: &[BoxId]) -> BoxId {
        node_waveform(self.arena, values)
    }

    #[must_use]
    pub fn route(&mut self, n: BoxId, m: BoxId, route_spec: BoxId) -> BoxId {
        node_route(self.arena, n, m, route_spec)
    }

    #[must_use]
    pub fn ffunction(&mut self, signature: BoxId, incfile: BoxId, libfile: BoxId) -> BoxId {
        ffunction(self.arena, signature, incfile, libfile)
    }

    #[must_use]
    pub fn ffun(&mut self, ff: BoxId) -> BoxId {
        node_ffun(self.arena, ff)
    }

    #[must_use]
    pub fn fconst(&mut self, ty: BoxId, name: BoxId, file: BoxId) -> BoxId {
        node_fconst(self.arena, ty, name, file)
    }

    #[must_use]
    pub fn fvar(&mut self, ty: BoxId, name: BoxId, file: BoxId) -> BoxId {
        node_fvar(self.arena, ty, name, file)
    }

    #[must_use]
    pub fn case(&mut self, rules: BoxId) -> BoxId {
        node_case(self.arena, rules)
    }

    #[must_use]
    pub fn pattern_var(&mut self, ident: BoxId) -> BoxId {
        node_pattern_var(self.arena, ident)
    }

    #[must_use]
    pub fn abstr(&mut self, arg: BoxId, body: BoxId) -> BoxId {
        node_abstr(self.arena, arg, body)
    }

    #[must_use]
    pub fn modulation(&mut self, arg: BoxId, body: BoxId) -> BoxId {
        node_modulation(self.arena, arg, body)
    }

    #[must_use]
    pub fn build_abstr(&mut self, args: BoxId, body: BoxId) -> BoxId {
        build_box_abstr(self.arena, args, body)
    }

    #[must_use]
    pub fn build_modulation(&mut self, args: BoxId, body: BoxId) -> BoxId {
        build_box_modulation(self.arena, args, body)
    }

    #[must_use]
    pub fn inputs(&mut self, expr: BoxId) -> BoxId {
        node_inputs(self.arena, expr)
    }

    #[must_use]
    pub fn outputs(&mut self, expr: BoxId) -> BoxId {
        node_outputs(self.arena, expr)
    }

    #[must_use]
    pub fn ondemand(&mut self, expr: BoxId) -> BoxId {
        node_ondemand(self.arena, expr)
    }

    #[must_use]
    pub fn upsampling(&mut self, expr: BoxId) -> BoxId {
        node_upsampling(self.arena, expr)
    }

    #[must_use]
    pub fn downsampling(&mut self, expr: BoxId) -> BoxId {
        node_downsampling(self.arena, expr)
    }

    #[must_use]
    pub fn button(&mut self, label: BoxId) -> BoxId {
        node_button(self.arena, label)
    }

    #[must_use]
    pub fn checkbox(&mut self, label: BoxId) -> BoxId {
        node_checkbox(self.arena, label)
    }

    #[must_use]
    pub fn vslider(
        &mut self,
        label: BoxId,
        cur: BoxId,
        min: BoxId,
        max: BoxId,
        step: BoxId,
    ) -> BoxId {
        node_vslider(self.arena, label, cur, min, max, step)
    }

    #[must_use]
    pub fn hslider(
        &mut self,
        label: BoxId,
        cur: BoxId,
        min: BoxId,
        max: BoxId,
        step: BoxId,
    ) -> BoxId {
        node_hslider(self.arena, label, cur, min, max, step)
    }

    #[must_use]
    pub fn num_entry(
        &mut self,
        label: BoxId,
        cur: BoxId,
        min: BoxId,
        max: BoxId,
        step: BoxId,
    ) -> BoxId {
        node_num_entry(self.arena, label, cur, min, max, step)
    }

    #[must_use]
    pub fn vgroup(&mut self, label: BoxId, expr: BoxId) -> BoxId {
        node_vgroup(self.arena, label, expr)
    }

    #[must_use]
    pub fn hgroup(&mut self, label: BoxId, expr: BoxId) -> BoxId {
        node_hgroup(self.arena, label, expr)
    }

    #[must_use]
    pub fn tgroup(&mut self, label: BoxId, expr: BoxId) -> BoxId {
        node_tgroup(self.arena, label, expr)
    }

    #[must_use]
    pub fn vbargraph(&mut self, label: BoxId, min: BoxId, max: BoxId) -> BoxId {
        node_vbargraph(self.arena, label, min, max)
    }

    #[must_use]
    pub fn hbargraph(&mut self, label: BoxId, min: BoxId, max: BoxId) -> BoxId {
        node_hbargraph(self.arena, label, min, max)
    }

    #[must_use]
    pub fn soundfile(&mut self, label: BoxId, chan: BoxId) -> BoxId {
        node_soundfile(self.arena, label, chan)
    }
}

/// Box structural matcher result.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BoxMatch<'a> {
    Unknown,
    Ident(&'a str),
    Int(i64),
    Real(f64),
    Wire,
    Cut,
    Seq(BoxId, BoxId),
    Par(BoxId, BoxId),
    Rec(BoxId, BoxId),
    Split(BoxId, BoxId),
    Merge(BoxId, BoxId),
    Appl(BoxId, BoxId),
    Access(BoxId, BoxId),
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    And,
    Or,
    Xor,
    Lsh,
    Rsh,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    Pow,
    Delay,
    Delay1,
    Min,
    Max,
    Prefix,
    IntCast,
    FloatCast,
    ReadOnlyTable,
    WriteReadTable,
    Select2,
    Select3,
    AssertBounds,
    Lowest,
    Highest,
    Attach,
    Enable,
    Control,
    IPar(BoxId, BoxId, BoxId),
    ISeq(BoxId, BoxId, BoxId),
    ISum(BoxId, BoxId, BoxId),
    IProd(BoxId, BoxId, BoxId),
    WithLocalDef(BoxId, BoxId),
    WithRecDef(BoxId, BoxId, BoxId),
    Environment,
    Component(BoxId),
    Library(BoxId),
    Waveform(BoxId),
    Route(BoxId, BoxId, BoxId),
    Ffunction(BoxId, BoxId, BoxId),
    FFun(BoxId),
    FConst(BoxId, BoxId, BoxId),
    FVar(BoxId, BoxId, BoxId),
    Case(BoxId),
    PatternVar(BoxId),
    Abstr(BoxId, BoxId),
    Modulation(BoxId, BoxId),
    Inputs(BoxId),
    Outputs(BoxId),
    Ondemand(BoxId),
    Upsampling(BoxId),
    Downsampling(BoxId),
    Button(BoxId),
    Checkbox(BoxId),
    VSlider(BoxId, BoxId, BoxId, BoxId, BoxId),
    HSlider(BoxId, BoxId, BoxId, BoxId, BoxId),
    NumEntry(BoxId, BoxId, BoxId, BoxId, BoxId),
    VGroup(BoxId, BoxId),
    HGroup(BoxId, BoxId),
    TGroup(BoxId, BoxId),
    VBargraph(BoxId, BoxId, BoxId),
    HBargraph(BoxId, BoxId, BoxId),
    Soundfile(BoxId, BoxId),
}

#[must_use]
pub fn match_box<'a>(arena: &'a TreeArena, b: BoxId) -> BoxMatch<'a> {
    let Some(node) = arena.node(b) else {
        return BoxMatch::Unknown;
    };
    match &node.kind {
        NodeKind::Int(v) => BoxMatch::Int(*v),
        NodeKind::FloatBits(bits) => BoxMatch::Real(f64::from_bits(*bits)),
        NodeKind::Tag(tag) => {
            let children = node.children.as_slice();
            match children.len() {
                0 => match tag.as_ref() {
                    BOX_WIRE_TAG => BoxMatch::Wire,
                    BOX_CUT_TAG => BoxMatch::Cut,
                    BOX_ADD_TAG => BoxMatch::Add,
                    BOX_SUB_TAG => BoxMatch::Sub,
                    BOX_MUL_TAG => BoxMatch::Mul,
                    BOX_DIV_TAG => BoxMatch::Div,
                    BOX_REM_TAG => BoxMatch::Rem,
                    BOX_AND_TAG => BoxMatch::And,
                    BOX_OR_TAG => BoxMatch::Or,
                    BOX_XOR_TAG => BoxMatch::Xor,
                    BOX_LSH_TAG => BoxMatch::Lsh,
                    BOX_RSH_TAG => BoxMatch::Rsh,
                    BOX_LT_TAG => BoxMatch::Lt,
                    BOX_LE_TAG => BoxMatch::Le,
                    BOX_GT_TAG => BoxMatch::Gt,
                    BOX_GE_TAG => BoxMatch::Ge,
                    BOX_EQ_TAG => BoxMatch::Eq,
                    BOX_NE_TAG => BoxMatch::Ne,
                    BOX_POW_TAG => BoxMatch::Pow,
                    BOX_DELAY_TAG => BoxMatch::Delay,
                    BOX_DELAY1_TAG => BoxMatch::Delay1,
                    BOX_MIN_TAG => BoxMatch::Min,
                    BOX_MAX_TAG => BoxMatch::Max,
                    BOX_PREFIX_TAG => BoxMatch::Prefix,
                    BOX_INT_CAST_TAG => BoxMatch::IntCast,
                    BOX_FLOAT_CAST_TAG => BoxMatch::FloatCast,
                    BOX_READ_ONLY_TABLE_TAG => BoxMatch::ReadOnlyTable,
                    BOX_WRITE_READ_TABLE_TAG => BoxMatch::WriteReadTable,
                    BOX_SELECT2_TAG => BoxMatch::Select2,
                    BOX_SELECT3_TAG => BoxMatch::Select3,
                    BOX_ASSERT_BOUNDS_TAG => BoxMatch::AssertBounds,
                    BOX_LOWEST_TAG => BoxMatch::Lowest,
                    BOX_HIGHEST_TAG => BoxMatch::Highest,
                    BOX_ATTACH_TAG => BoxMatch::Attach,
                    BOX_ENABLE_TAG => BoxMatch::Enable,
                    BOX_CONTROL_TAG => BoxMatch::Control,
                    BOX_ENVIRONMENT_TAG => BoxMatch::Environment,
                    _ => BoxMatch::Unknown,
                },
                1 => {
                    let c0 = children[0];
                    match tag.as_ref() {
                        BOX_IDENT_TAG => match arena.kind(c0) {
                            Some(NodeKind::Symbol(name)) => BoxMatch::Ident(name.as_ref()),
                            _ => BoxMatch::Unknown,
                        },
                        BOX_COMPONENT_TAG => BoxMatch::Component(c0),
                        BOX_LIBRARY_TAG => BoxMatch::Library(c0),
                        BOX_WAVEFORM_TAG => BoxMatch::Waveform(c0),
                        BOX_FFUN_TAG => BoxMatch::FFun(c0),
                        BOX_CASE_TAG => BoxMatch::Case(c0),
                        BOX_PATTERN_VAR_TAG => BoxMatch::PatternVar(c0),
                        BOX_INPUTS_TAG => BoxMatch::Inputs(c0),
                        BOX_OUTPUTS_TAG => BoxMatch::Outputs(c0),
                        BOX_ONDEMAND_TAG => BoxMatch::Ondemand(c0),
                        BOX_UPSAMPLING_TAG => BoxMatch::Upsampling(c0),
                        BOX_DOWNSAMPLING_TAG => BoxMatch::Downsampling(c0),
                        BOX_BUTTON_TAG => BoxMatch::Button(c0),
                        BOX_CHECKBOX_TAG => BoxMatch::Checkbox(c0),
                        _ => BoxMatch::Unknown,
                    }
                }
                2 => {
                    let c0 = children[0];
                    let c1 = children[1];
                    match tag.as_ref() {
                        BOX_SEQ_TAG => BoxMatch::Seq(c0, c1),
                        BOX_PAR_TAG => BoxMatch::Par(c0, c1),
                        BOX_REC_TAG => BoxMatch::Rec(c0, c1),
                        BOX_SPLIT_TAG => BoxMatch::Split(c0, c1),
                        BOX_MERGE_TAG => BoxMatch::Merge(c0, c1),
                        BOX_APPL_TAG => BoxMatch::Appl(c0, c1),
                        BOX_ACCESS_TAG => BoxMatch::Access(c0, c1),
                        BOX_WITH_LOCAL_DEF_TAG => BoxMatch::WithLocalDef(c0, c1),
                        BOX_ABSTR_TAG => BoxMatch::Abstr(c0, c1),
                        BOX_MODULATION_TAG => BoxMatch::Modulation(c0, c1),
                        BOX_VGROUP_TAG => BoxMatch::VGroup(c0, c1),
                        BOX_HGROUP_TAG => BoxMatch::HGroup(c0, c1),
                        BOX_TGROUP_TAG => BoxMatch::TGroup(c0, c1),
                        BOX_SOUNDFILE_TAG => BoxMatch::Soundfile(c0, c1),
                        BOX_VSLIDER_TAG => {
                            let Some((cur, min, max, step)) = slider_params4(arena, c1) else {
                                return BoxMatch::Unknown;
                            };
                            BoxMatch::VSlider(c0, cur, min, max, step)
                        }
                        BOX_HSLIDER_TAG => {
                            let Some((cur, min, max, step)) = slider_params4(arena, c1) else {
                                return BoxMatch::Unknown;
                            };
                            BoxMatch::HSlider(c0, cur, min, max, step)
                        }
                        BOX_NUM_ENTRY_TAG => {
                            let Some((cur, min, max, step)) = slider_params4(arena, c1) else {
                                return BoxMatch::Unknown;
                            };
                            BoxMatch::NumEntry(c0, cur, min, max, step)
                        }
                        _ => BoxMatch::Unknown,
                    }
                }
                3 => {
                    let c0 = children[0];
                    let c1 = children[1];
                    let c2 = children[2];
                    match tag.as_ref() {
                        BOX_IPAR_TAG => BoxMatch::IPar(c0, c1, c2),
                        BOX_ISEQ_TAG => BoxMatch::ISeq(c0, c1, c2),
                        BOX_ISUM_TAG => BoxMatch::ISum(c0, c1, c2),
                        BOX_IPROD_TAG => BoxMatch::IProd(c0, c1, c2),
                        BOX_WITH_REC_DEF_TAG => BoxMatch::WithRecDef(c0, c1, c2),
                        BOX_ROUTE_TAG => BoxMatch::Route(c0, c1, c2),
                        FFUN_TAG => BoxMatch::Ffunction(c0, c1, c2),
                        BOX_FCONST_TAG => BoxMatch::FConst(c0, c1, c2),
                        BOX_FVAR_TAG => BoxMatch::FVar(c0, c1, c2),
                        BOX_VBARGRAPH_TAG => BoxMatch::VBargraph(c0, c1, c2),
                        BOX_HBARGRAPH_TAG => BoxMatch::HBargraph(c0, c1, c2),
                        _ => BoxMatch::Unknown,
                    }
                }
                _ => BoxMatch::Unknown,
            }
        }
        _ => BoxMatch::Unknown,
    }
}

/// Equivalent to C++ `boxIdent(const char*)`.
#[must_use]
fn node_ident(arena: &mut TreeArena, name: &str) -> BoxId {
    let sym = arena.symbol(name);
    intern_tag(arena, BOX_IDENT_TAG, &[sym])
}

/// Returns identifier symbol name when `b` is `node_ident`.
#[must_use]
#[allow(dead_code)]
fn node_ident_name(arena: &TreeArena, b: BoxId) -> Option<&str> {
    let [sym] = match_tag_arity(arena, b, BOX_IDENT_TAG, 1)? else {
        return None;
    };
    match arena.kind(*sym) {
        Some(NodeKind::Symbol(name)) => Some(name.as_ref()),
        _ => None,
    }
}

/// Equivalent to C++ `boxInt`.
#[must_use]
fn node_int(arena: &mut TreeArena, value: i64) -> BoxId {
    arena.int(value)
}

/// Equivalent to C++ `boxReal`.
#[must_use]
fn node_real(arena: &mut TreeArena, value: f64) -> BoxId {
    arena.float(value)
}

/// Equivalent to C++ `boxWire`.
#[must_use]
fn node_wire(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_WIRE_TAG, &[])
}

/// Equivalent to C++ `boxCut`.
#[must_use]
fn node_cut(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_CUT_TAG, &[])
}

/// Predicate equivalent to C++ `isBoxWire`.
#[must_use]
#[allow(dead_code)]
fn is_node_wire(arena: &TreeArena, b: BoxId) -> bool {
    match_tag_arity(arena, b, BOX_WIRE_TAG, 0).is_some()
}

/// Predicate equivalent to C++ `isBoxCut`.
#[must_use]
#[allow(dead_code)]
fn is_node_cut(arena: &TreeArena, b: BoxId) -> bool {
    match_tag_arity(arena, b, BOX_CUT_TAG, 0).is_some()
}

/// Equivalent to C++ `boxSeq`.
#[must_use]
fn node_seq(arena: &mut TreeArena, left: BoxId, right: BoxId) -> BoxId {
    intern_tag(arena, BOX_SEQ_TAG, &[left, right])
}

/// Equivalent to C++ `boxPar`.
#[must_use]
fn node_par(arena: &mut TreeArena, left: BoxId, right: BoxId) -> BoxId {
    intern_tag(arena, BOX_PAR_TAG, &[left, right])
}

/// Equivalent to C++ `boxRec`.
#[must_use]
fn node_rec(arena: &mut TreeArena, left: BoxId, right: BoxId) -> BoxId {
    intern_tag(arena, BOX_REC_TAG, &[left, right])
}

/// Equivalent to C++ `boxSplit`.
#[must_use]
fn node_split(arena: &mut TreeArena, left: BoxId, right: BoxId) -> BoxId {
    intern_tag(arena, BOX_SPLIT_TAG, &[left, right])
}

/// Equivalent to C++ `boxMerge`.
#[must_use]
fn node_merge(arena: &mut TreeArena, left: BoxId, right: BoxId) -> BoxId {
    intern_tag(arena, BOX_MERGE_TAG, &[left, right])
}

/// Equivalent to C++ `boxAppl`.
#[must_use]
fn node_appl(arena: &mut TreeArena, fun: BoxId, arglist: BoxId) -> BoxId {
    intern_tag(arena, BOX_APPL_TAG, &[fun, arglist])
}

/// Returns `(fun, arglist)` when `b` is `node_appl`.
#[must_use]
#[allow(dead_code)]
fn is_node_appl(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_APPL_TAG)
}

/// Equivalent to C++ `boxAccess`.
#[must_use]
fn node_access(arena: &mut TreeArena, expr: BoxId, ident: BoxId) -> BoxId {
    intern_tag(arena, BOX_ACCESS_TAG, &[expr, ident])
}

/// Returns `(expr, ident)` when `b` is `node_access`.
#[must_use]
#[allow(dead_code)]
fn is_node_access(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_ACCESS_TAG)
}

/// Returns `(left, right)` when `b` is `node_seq`.
#[must_use]
#[allow(dead_code)]
fn is_node_seq(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_SEQ_TAG)
}

/// Returns `(left, right)` when `b` is `node_par`.
#[must_use]
#[allow(dead_code)]
fn is_node_par(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_PAR_TAG)
}

/// Returns `(left, right)` when `b` is `node_rec`.
#[must_use]
#[allow(dead_code)]
fn is_node_rec(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_REC_TAG)
}

/// Returns `(left, right)` when `b` is `node_split`.
#[must_use]
#[allow(dead_code)]
fn is_node_split(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_SPLIT_TAG)
}

/// Returns `(left, right)` when `b` is `node_merge`.
#[must_use]
#[allow(dead_code)]
fn is_node_merge(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_MERGE_TAG)
}

/// Equivalent to C++ `boxAdd`.
#[must_use]
fn node_add(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_ADD_TAG, &[])
}

/// Equivalent to C++ `boxSub`.
#[must_use]
fn node_sub(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_SUB_TAG, &[])
}

/// Equivalent to C++ `boxMul`.
#[must_use]
fn node_mul(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_MUL_TAG, &[])
}

/// Equivalent to C++ `boxDiv`.
#[must_use]
fn node_div(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_DIV_TAG, &[])
}

/// Equivalent to C++ `boxRem`.
#[must_use]
fn node_rem(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_REM_TAG, &[])
}

/// Equivalent to C++ `boxAND`.
#[must_use]
fn node_and(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_AND_TAG, &[])
}

/// Equivalent to C++ `boxOR`.
#[must_use]
fn node_or(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_OR_TAG, &[])
}

/// Equivalent to C++ `boxXOR`.
#[must_use]
fn node_xor(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_XOR_TAG, &[])
}

/// Equivalent to C++ `boxLeftShift`.
#[must_use]
fn node_lsh(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_LSH_TAG, &[])
}

/// Equivalent to C++ `boxARightShift`.
#[must_use]
fn node_rsh(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_RSH_TAG, &[])
}

/// Equivalent to C++ `boxLT`.
#[must_use]
fn node_lt(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_LT_TAG, &[])
}

/// Equivalent to C++ `boxLE`.
#[must_use]
fn node_le(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_LE_TAG, &[])
}

/// Equivalent to C++ `boxGT`.
#[must_use]
fn node_gt(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_GT_TAG, &[])
}

/// Equivalent to C++ `boxGE`.
#[must_use]
fn node_ge(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_GE_TAG, &[])
}

/// Equivalent to C++ `boxEQ`.
#[must_use]
fn node_eq(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_EQ_TAG, &[])
}

/// Equivalent to C++ `boxNE`.
#[must_use]
fn node_ne(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_NE_TAG, &[])
}

/// Equivalent to C++ `boxPow`.
#[must_use]
fn node_pow(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_POW_TAG, &[])
}

/// Equivalent to C++ `boxDelay`.
#[must_use]
fn node_delay(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_DELAY_TAG, &[])
}

/// Equivalent to C++ `boxDelay1`.
#[must_use]
fn node_delay1(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_DELAY1_TAG, &[])
}

/// Equivalent to C++ `boxMin`.
#[must_use]
fn node_min(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_MIN_TAG, &[])
}

/// Equivalent to C++ `boxMax`.
#[must_use]
fn node_max(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_MAX_TAG, &[])
}

/// Equivalent to C++ `boxPrefix`.
#[must_use]
fn node_prefix(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_PREFIX_TAG, &[])
}

/// Equivalent to C++ `boxIntCast`.
#[must_use]
fn node_int_cast(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_INT_CAST_TAG, &[])
}

/// Equivalent to C++ `boxFloatCast`.
#[must_use]
fn node_float_cast(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_FLOAT_CAST_TAG, &[])
}

/// Equivalent to C++ `boxReadOnlyTable`.
#[must_use]
fn node_read_only_table(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_READ_ONLY_TABLE_TAG, &[])
}

/// Equivalent to C++ `boxWriteReadTable`.
#[must_use]
fn node_write_read_table(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_WRITE_READ_TABLE_TAG, &[])
}

/// Equivalent to C++ `boxSelect2`.
#[must_use]
fn node_select2(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_SELECT2_TAG, &[])
}

/// Equivalent to C++ `boxSelect3`.
#[must_use]
fn node_select3(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_SELECT3_TAG, &[])
}

/// Equivalent to C++ `boxAssertBound`.
#[must_use]
fn node_assert_bounds(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_ASSERT_BOUNDS_TAG, &[])
}

/// Equivalent to C++ `boxLowest`.
#[must_use]
fn node_lowest(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_LOWEST_TAG, &[])
}

/// Equivalent to C++ `boxHighest`.
#[must_use]
fn node_highest(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_HIGHEST_TAG, &[])
}

/// Equivalent to C++ `boxAttach`.
#[must_use]
fn node_attach(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_ATTACH_TAG, &[])
}

/// Equivalent to C++ `boxEnable`.
#[must_use]
fn node_enable(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_ENABLE_TAG, &[])
}

/// Equivalent to C++ `boxControl`.
#[must_use]
fn node_control(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_CONTROL_TAG, &[])
}

macro_rules! define_is_prim {
    ($fn_name:ident, $tag:ident) => {
        #[allow(dead_code)]
        #[must_use]
        pub fn $fn_name(arena: &TreeArena, b: BoxId) -> bool {
            match_tag_arity(arena, b, $tag, 0).is_some()
        }
    };
}

define_is_prim!(is_node_add, BOX_ADD_TAG);
define_is_prim!(is_node_sub, BOX_SUB_TAG);
define_is_prim!(is_node_mul, BOX_MUL_TAG);
define_is_prim!(is_node_div, BOX_DIV_TAG);
define_is_prim!(is_node_rem, BOX_REM_TAG);
define_is_prim!(is_node_and, BOX_AND_TAG);
define_is_prim!(is_node_or, BOX_OR_TAG);
define_is_prim!(is_node_xor, BOX_XOR_TAG);
define_is_prim!(is_node_lsh, BOX_LSH_TAG);
define_is_prim!(is_node_rsh, BOX_RSH_TAG);
define_is_prim!(is_node_lt, BOX_LT_TAG);
define_is_prim!(is_node_le, BOX_LE_TAG);
define_is_prim!(is_node_gt, BOX_GT_TAG);
define_is_prim!(is_node_ge, BOX_GE_TAG);
define_is_prim!(is_node_eq, BOX_EQ_TAG);
define_is_prim!(is_node_ne, BOX_NE_TAG);
define_is_prim!(is_node_pow, BOX_POW_TAG);
define_is_prim!(is_node_delay, BOX_DELAY_TAG);
define_is_prim!(is_node_delay1, BOX_DELAY1_TAG);
define_is_prim!(is_node_min, BOX_MIN_TAG);
define_is_prim!(is_node_max, BOX_MAX_TAG);
define_is_prim!(is_node_prefix, BOX_PREFIX_TAG);
define_is_prim!(is_node_int_cast, BOX_INT_CAST_TAG);
define_is_prim!(is_node_float_cast, BOX_FLOAT_CAST_TAG);
define_is_prim!(is_node_read_only_table, BOX_READ_ONLY_TABLE_TAG);
define_is_prim!(is_node_write_read_table, BOX_WRITE_READ_TABLE_TAG);
define_is_prim!(is_node_select2, BOX_SELECT2_TAG);
define_is_prim!(is_node_select3, BOX_SELECT3_TAG);
define_is_prim!(is_node_assert_bounds, BOX_ASSERT_BOUNDS_TAG);
define_is_prim!(is_node_lowest, BOX_LOWEST_TAG);
define_is_prim!(is_node_highest, BOX_HIGHEST_TAG);
define_is_prim!(is_node_attach, BOX_ATTACH_TAG);
define_is_prim!(is_node_enable, BOX_ENABLE_TAG);
define_is_prim!(is_node_control, BOX_CONTROL_TAG);

/// Equivalent to C++ `boxIPar`.
#[must_use]
fn node_ipar(arena: &mut TreeArena, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
    intern_tag(arena, BOX_IPAR_TAG, &[index, count, body])
}

/// Returns `(index, count, body)` when `b` is `node_ipar`.
#[must_use]
#[allow(dead_code)]
fn is_node_ipar(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_IPAR_TAG)
}

/// Equivalent to C++ `boxISeq`.
#[must_use]
fn node_iseq(arena: &mut TreeArena, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
    intern_tag(arena, BOX_ISEQ_TAG, &[index, count, body])
}

/// Returns `(index, count, body)` when `b` is `node_iseq`.
#[must_use]
#[allow(dead_code)]
fn is_node_iseq(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_ISEQ_TAG)
}

/// Equivalent to C++ `boxISum`.
#[must_use]
fn node_isum(arena: &mut TreeArena, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
    intern_tag(arena, BOX_ISUM_TAG, &[index, count, body])
}

/// Returns `(index, count, body)` when `b` is `node_isum`.
#[must_use]
#[allow(dead_code)]
fn is_node_isum(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_ISUM_TAG)
}

/// Equivalent to C++ `boxIProd`.
#[must_use]
fn node_iprod(arena: &mut TreeArena, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
    intern_tag(arena, BOX_IPROD_TAG, &[index, count, body])
}

/// Returns `(index, count, body)` when `b` is `node_iprod`.
#[must_use]
#[allow(dead_code)]
fn is_node_iprod(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_IPROD_TAG)
}

/// Equivalent to C++ `boxWithLocalDef`.
#[must_use]
fn node_with_local_def(arena: &mut TreeArena, body: BoxId, ldef: BoxId) -> BoxId {
    intern_tag(arena, BOX_WITH_LOCAL_DEF_TAG, &[body, ldef])
}

/// Returns `(body, ldef)` when `b` is `node_with_local_def`.
#[must_use]
#[allow(dead_code)]
fn is_node_with_local_def(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_WITH_LOCAL_DEF_TAG)
}

/// Adapted representation for C++ `boxWithRecDef`.
///
/// C++ performs an immediate lowering/expansion into a local-definition structure.
/// For the current parser prototype, Rust stores an explicit node preserving the three
/// inputs `(body, ldef, ldef2)`. This keeps parser output deterministic and lets later
/// phases choose where lowering happens.
#[must_use]
fn node_with_rec_def(arena: &mut TreeArena, body: BoxId, ldef: BoxId, ldef2: BoxId) -> BoxId {
    intern_tag(arena, BOX_WITH_REC_DEF_TAG, &[body, ldef, ldef2])
}

/// Returns `(body, ldef, ldef2)` when `b` is `node_with_rec_def`.
#[must_use]
#[allow(dead_code)]
fn is_node_with_rec_def(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    let [body, ldef, ldef2] = match_tag_arity(arena, b, BOX_WITH_REC_DEF_TAG, 3)? else {
        return None;
    };
    Some((*body, *ldef, *ldef2))
}

/// Equivalent to C++ `boxEnvironment`.
#[must_use]
fn node_environment(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_ENVIRONMENT_TAG, &[])
}

/// Predicate equivalent to C++ `isBoxEnvironment`.
#[must_use]
#[allow(dead_code)]
fn is_node_environment(arena: &TreeArena, b: BoxId) -> bool {
    match_tag_arity(arena, b, BOX_ENVIRONMENT_TAG, 0).is_some()
}

/// Equivalent to C++ `boxComponent`.
#[must_use]
fn node_component(arena: &mut TreeArena, filename: BoxId) -> BoxId {
    intern_tag(arena, BOX_COMPONENT_TAG, &[filename])
}

/// Returns `filename` when `b` is `node_component`.
#[must_use]
#[allow(dead_code)]
fn is_node_component(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_COMPONENT_TAG)
}

/// Equivalent to C++ `boxLibrary`.
#[must_use]
fn node_library(arena: &mut TreeArena, filename: BoxId) -> BoxId {
    intern_tag(arena, BOX_LIBRARY_TAG, &[filename])
}

/// Returns `filename` when `b` is `node_library`.
#[must_use]
#[allow(dead_code)]
fn is_node_library(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_LIBRARY_TAG)
}

/// Equivalent to C++ `boxWaveform`.
///
/// Rust keeps a deterministic list payload in one child:
/// `tree(BOXWAVEFORM, cons(v0, cons(v1, ...)))`.
#[must_use]
fn node_waveform(arena: &mut TreeArena, values: &[BoxId]) -> BoxId {
    let mut list = arena.nil();
    for value in values.iter().rev() {
        list = arena.cons(*value, list);
    }
    intern_tag(arena, BOX_WAVEFORM_TAG, &[list])
}

/// Returns waveform list payload when `b` is `node_waveform`.
#[must_use]
#[allow(dead_code)]
fn is_node_waveform(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_WAVEFORM_TAG)
}

/// Equivalent to C++ `boxRoute`.
#[must_use]
fn node_route(arena: &mut TreeArena, n: BoxId, m: BoxId, route_spec: BoxId) -> BoxId {
    intern_tag(arena, BOX_ROUTE_TAG, &[n, m, route_spec])
}

/// Returns `(n, m, route_spec)` when `b` is `node_route`.
#[must_use]
#[allow(dead_code)]
fn is_node_route(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_ROUTE_TAG)
}

/// Equivalent to C++ `ffunction(signature, incfile, libfile)`.
#[must_use]
fn ffunction(arena: &mut TreeArena, signature: BoxId, incfile: BoxId, libfile: BoxId) -> BoxId {
    intern_tag(arena, FFUN_TAG, &[signature, incfile, libfile])
}

/// Returns `(signature, incfile, libfile)` when `b` is `ffunction(...)`.
#[must_use]
#[allow(dead_code)]
fn is_ffunction(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, FFUN_TAG)
}

/// Equivalent to C++ `boxFFun`.
#[must_use]
fn node_ffun(arena: &mut TreeArena, ff: BoxId) -> BoxId {
    intern_tag(arena, BOX_FFUN_TAG, &[ff])
}

/// Returns wrapped foreign-function descriptor when `b` is `node_ffun`.
#[must_use]
#[allow(dead_code)]
fn is_node_ffun(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_FFUN_TAG)
}

/// Equivalent to C++ `boxFConst`.
#[must_use]
fn node_fconst(arena: &mut TreeArena, ty: BoxId, name: BoxId, file: BoxId) -> BoxId {
    intern_tag(arena, BOX_FCONST_TAG, &[ty, name, file])
}

/// Returns `(ty, name, file)` when `b` is `node_fconst`.
#[must_use]
#[allow(dead_code)]
fn is_node_fconst(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_FCONST_TAG)
}

/// Equivalent to C++ `boxFVar`.
#[must_use]
fn node_fvar(arena: &mut TreeArena, ty: BoxId, name: BoxId, file: BoxId) -> BoxId {
    intern_tag(arena, BOX_FVAR_TAG, &[ty, name, file])
}

/// Returns `(ty, name, file)` when `b` is `node_fvar`.
#[must_use]
#[allow(dead_code)]
fn is_node_fvar(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_FVAR_TAG)
}

/// Equivalent to C++ `boxCase`.
#[must_use]
fn node_case(arena: &mut TreeArena, rules: BoxId) -> BoxId {
    intern_tag(arena, BOX_CASE_TAG, &[rules])
}

/// Returns `rules` when `b` is `node_case`.
#[must_use]
#[allow(dead_code)]
fn is_node_case(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_CASE_TAG)
}

/// Equivalent to C++ `boxPatternVar`.
#[must_use]
fn node_pattern_var(arena: &mut TreeArena, ident: BoxId) -> BoxId {
    intern_tag(arena, BOX_PATTERN_VAR_TAG, &[ident])
}

/// Returns wrapped identifier when `b` is `node_pattern_var`.
#[must_use]
#[allow(dead_code)]
fn is_node_pattern_var(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_PATTERN_VAR_TAG)
}

/// Equivalent to C++ `boxAbstr`.
#[must_use]
fn node_abstr(arena: &mut TreeArena, arg: BoxId, body: BoxId) -> BoxId {
    intern_tag(arena, BOX_ABSTR_TAG, &[arg, body])
}

/// Returns `(arg, body)` when `b` is `node_abstr`.
#[must_use]
#[allow(dead_code)]
fn is_node_abstr(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_ABSTR_TAG)
}

/// Equivalent to C++ `boxModulation`.
#[must_use]
fn node_modulation(arena: &mut TreeArena, arg: BoxId, body: BoxId) -> BoxId {
    intern_tag(arena, BOX_MODULATION_TAG, &[arg, body])
}

/// Returns `(arg, body)` when `b` is `node_modulation`.
#[must_use]
#[allow(dead_code)]
fn is_node_modulation(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_MODULATION_TAG)
}

/// Equivalent to C++ `buildBoxAbstr(largs, body)` using parser-built arg list.
///
/// This preserves C++ nesting order by consuming list tail first.
#[must_use]
fn build_box_abstr(arena: &mut TreeArena, args: BoxId, body: BoxId) -> BoxId {
    if arena.is_nil(args) {
        return body;
    }
    let Some(head) = arena.hd(args) else {
        return body;
    };
    let Some(tail) = arena.tl(args) else {
        return body;
    };
    let nested = build_box_abstr(arena, tail, body);
    node_abstr(arena, head, nested)
}

/// Equivalent to C++ `buildBoxModulation(largs, body)` using parser-built arg list.
///
/// This preserves C++ nesting order by applying each list head to the current body,
/// then recursing on the tail.
#[must_use]
fn build_box_modulation(arena: &mut TreeArena, args: BoxId, body: BoxId) -> BoxId {
    if arena.is_nil(args) {
        return body;
    }
    let Some(head) = arena.hd(args) else {
        return body;
    };
    let Some(tail) = arena.tl(args) else {
        return body;
    };
    let nested = node_modulation(arena, head, body);
    build_box_modulation(arena, tail, nested)
}

/// Equivalent to C++ `boxInputs`.
#[must_use]
fn node_inputs(arena: &mut TreeArena, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_INPUTS_TAG, &[expr])
}

/// Returns wrapped expression when `b` is `node_inputs`.
#[must_use]
#[allow(dead_code)]
fn is_node_inputs(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_INPUTS_TAG)
}

/// Equivalent to C++ `boxOutputs`.
#[must_use]
fn node_outputs(arena: &mut TreeArena, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_OUTPUTS_TAG, &[expr])
}

/// Returns wrapped expression when `b` is `node_outputs`.
#[must_use]
#[allow(dead_code)]
fn is_node_outputs(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_OUTPUTS_TAG)
}

/// Equivalent to C++ `boxOndemand`.
#[must_use]
fn node_ondemand(arena: &mut TreeArena, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_ONDEMAND_TAG, &[expr])
}

/// Returns wrapped expression when `b` is `node_ondemand`.
#[must_use]
#[allow(dead_code)]
fn is_node_ondemand(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_ONDEMAND_TAG)
}

/// Equivalent to C++ `boxUpsampling`.
#[must_use]
fn node_upsampling(arena: &mut TreeArena, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_UPSAMPLING_TAG, &[expr])
}

/// Returns wrapped expression when `b` is `node_upsampling`.
#[must_use]
#[allow(dead_code)]
fn is_node_upsampling(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_UPSAMPLING_TAG)
}

/// Equivalent to C++ `boxDownsampling`.
#[must_use]
fn node_downsampling(arena: &mut TreeArena, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_DOWNSAMPLING_TAG, &[expr])
}

/// Returns wrapped expression when `b` is `node_downsampling`.
#[must_use]
#[allow(dead_code)]
fn is_node_downsampling(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_DOWNSAMPLING_TAG)
}

/// Equivalent to C++ `boxButton`.
#[must_use]
fn node_button(arena: &mut TreeArena, label: BoxId) -> BoxId {
    intern_tag(arena, BOX_BUTTON_TAG, &[label])
}

/// Returns `label` when `b` is `node_button`.
#[must_use]
#[allow(dead_code)]
fn is_node_button(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_BUTTON_TAG)
}

/// Equivalent to C++ `boxCheckbox`.
#[must_use]
fn node_checkbox(arena: &mut TreeArena, label: BoxId) -> BoxId {
    intern_tag(arena, BOX_CHECKBOX_TAG, &[label])
}

/// Returns `label` when `b` is `node_checkbox`.
#[must_use]
#[allow(dead_code)]
fn is_node_checkbox(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_CHECKBOX_TAG)
}

/// Equivalent to C++ `boxVSlider`.
///
/// C++ payload encoding is preserved:
/// `tree(BOXVSLIDER, label, list4(cur,min,max,step))`.
#[must_use]
fn node_vslider(
    arena: &mut TreeArena,
    label: BoxId,
    cur: BoxId,
    min: BoxId,
    max: BoxId,
    step: BoxId,
) -> BoxId {
    let params = list4(arena, cur, min, max, step);
    intern_tag(arena, BOX_VSLIDER_TAG, &[label, params])
}

/// Returns `(label, cur, min, max, step)` when `b` is `node_vslider`.
#[must_use]
#[allow(dead_code)]
fn is_node_vslider(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId, BoxId, BoxId)> {
    match_slider(arena, b, BOX_VSLIDER_TAG)
}

/// Equivalent to C++ `boxHSlider`.
///
/// C++ payload encoding is preserved:
/// `tree(BOXHSLIDER, label, list4(cur,min,max,step))`.
#[must_use]
fn node_hslider(
    arena: &mut TreeArena,
    label: BoxId,
    cur: BoxId,
    min: BoxId,
    max: BoxId,
    step: BoxId,
) -> BoxId {
    let params = list4(arena, cur, min, max, step);
    intern_tag(arena, BOX_HSLIDER_TAG, &[label, params])
}

/// Returns `(label, cur, min, max, step)` when `b` is `node_hslider`.
#[must_use]
#[allow(dead_code)]
fn is_node_hslider(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId, BoxId, BoxId)> {
    match_slider(arena, b, BOX_HSLIDER_TAG)
}

/// Equivalent to C++ `boxNumEntry`.
///
/// C++ payload encoding is preserved:
/// `tree(BOXNUMENTRY, label, list4(cur,min,max,step))`.
#[must_use]
fn node_num_entry(
    arena: &mut TreeArena,
    label: BoxId,
    cur: BoxId,
    min: BoxId,
    max: BoxId,
    step: BoxId,
) -> BoxId {
    let params = list4(arena, cur, min, max, step);
    intern_tag(arena, BOX_NUM_ENTRY_TAG, &[label, params])
}

/// Returns `(label, cur, min, max, step)` when `b` is `node_num_entry`.
#[must_use]
#[allow(dead_code)]
fn is_node_num_entry(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId, BoxId, BoxId)> {
    match_slider(arena, b, BOX_NUM_ENTRY_TAG)
}

/// Equivalent to C++ `boxVGroup`.
#[must_use]
fn node_vgroup(arena: &mut TreeArena, label: BoxId, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_VGROUP_TAG, &[label, expr])
}

/// Returns `(label, expr)` when `b` is `node_vgroup`.
#[must_use]
#[allow(dead_code)]
fn is_node_vgroup(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_VGROUP_TAG)
}

/// Equivalent to C++ `boxHGroup`.
#[must_use]
fn node_hgroup(arena: &mut TreeArena, label: BoxId, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_HGROUP_TAG, &[label, expr])
}

/// Returns `(label, expr)` when `b` is `node_hgroup`.
#[must_use]
#[allow(dead_code)]
fn is_node_hgroup(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_HGROUP_TAG)
}

/// Equivalent to C++ `boxTGroup`.
#[must_use]
fn node_tgroup(arena: &mut TreeArena, label: BoxId, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_TGROUP_TAG, &[label, expr])
}

/// Returns `(label, expr)` when `b` is `node_tgroup`.
#[must_use]
#[allow(dead_code)]
fn is_node_tgroup(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_TGROUP_TAG)
}

/// Equivalent to C++ `boxVBargraph`.
#[must_use]
fn node_vbargraph(arena: &mut TreeArena, label: BoxId, min: BoxId, max: BoxId) -> BoxId {
    intern_tag(arena, BOX_VBARGRAPH_TAG, &[label, min, max])
}

/// Returns `(label, min, max)` when `b` is `node_vbargraph`.
#[must_use]
#[allow(dead_code)]
fn is_node_vbargraph(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_VBARGRAPH_TAG)
}

/// Equivalent to C++ `boxHBargraph`.
#[must_use]
fn node_hbargraph(arena: &mut TreeArena, label: BoxId, min: BoxId, max: BoxId) -> BoxId {
    intern_tag(arena, BOX_HBARGRAPH_TAG, &[label, min, max])
}

/// Returns `(label, min, max)` when `b` is `node_hbargraph`.
#[must_use]
#[allow(dead_code)]
fn is_node_hbargraph(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_HBARGRAPH_TAG)
}

/// Equivalent to C++ `boxSoundfile`.
#[must_use]
fn node_soundfile(arena: &mut TreeArena, label: BoxId, chan: BoxId) -> BoxId {
    intern_tag(arena, BOX_SOUNDFILE_TAG, &[label, chan])
}

/// Returns `(label, chan)` when `b` is `node_soundfile`.
#[must_use]
#[allow(dead_code)]
fn is_node_soundfile(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_SOUNDFILE_TAG)
}

/// Deterministic structural dump helper for parser differential checks.
///
/// Output is shape-and-label based and intentionally excludes arena addresses.
#[must_use]
pub fn dump_box(arena: &TreeArena, root: BoxId) -> String {
    let mut out = String::new();
    dump_node(arena, root, &mut out);
    out
}

fn intern_tag(arena: &mut TreeArena, tag: &str, children: &[BoxId]) -> BoxId {
    arena.intern(NodeKind::Tag(tag.into()), children)
}

fn match_tag_arity<'a>(
    arena: &'a TreeArena,
    b: BoxId,
    tag: &str,
    arity: usize,
) -> Option<&'a [BoxId]> {
    let children = arena.children(b)?;
    if children.len() != arity {
        return None;
    }
    match arena.kind(b) {
        Some(NodeKind::Tag(actual)) if actual.as_ref() == tag => Some(children),
        _ => None,
    }
}

#[allow(dead_code)]
fn match_binary(arena: &TreeArena, b: BoxId, tag: &str) -> Option<(BoxId, BoxId)> {
    let [left, right] = match_tag_arity(arena, b, tag, 2)? else {
        return None;
    };
    Some((*left, *right))
}

#[allow(dead_code)]
fn match_ternary(arena: &TreeArena, b: BoxId, tag: &str) -> Option<(BoxId, BoxId, BoxId)> {
    let [a, b, c] = match_tag_arity(arena, b, tag, 3)? else {
        return None;
    };
    Some((*a, *b, *c))
}

#[allow(dead_code)]
fn match_unary(arena: &TreeArena, b: BoxId, tag: &str) -> Option<BoxId> {
    let [child] = match_tag_arity(arena, b, tag, 1)? else {
        return None;
    };
    Some(*child)
}

#[allow(dead_code)]
fn match_slider(
    arena: &TreeArena,
    b: BoxId,
    tag: &str,
) -> Option<(BoxId, BoxId, BoxId, BoxId, BoxId)> {
    let [label, params] = match_tag_arity(arena, b, tag, 2)? else {
        return None;
    };
    let (cur, min, max, step) = slider_params4(arena, *params)?;
    Some((*label, cur, min, max, step))
}

fn list4(arena: &mut TreeArena, a: BoxId, b: BoxId, c: BoxId, d: BoxId) -> BoxId {
    let nil = arena.nil();
    let l3 = arena.cons(d, nil);
    let l2 = arena.cons(c, l3);
    let l1 = arena.cons(b, l2);
    arena.cons(a, l1)
}

fn slider_params4(arena: &TreeArena, params: BoxId) -> Option<(BoxId, BoxId, BoxId, BoxId)> {
    let node0 = arena.node(params)?;
    if !matches!(node0.kind, NodeKind::Cons) || node0.children.len() != 2 {
        return None;
    }
    let cur = node0.children.get(0)?;

    let node1 = arena.node(node0.children.get(1)?)?;
    if !matches!(node1.kind, NodeKind::Cons) || node1.children.len() != 2 {
        return None;
    }
    let min = node1.children.get(0)?;

    let node2 = arena.node(node1.children.get(1)?)?;
    if !matches!(node2.kind, NodeKind::Cons) || node2.children.len() != 2 {
        return None;
    }
    let max = node2.children.get(0)?;

    let node3 = arena.node(node2.children.get(1)?)?;
    if !matches!(node3.kind, NodeKind::Cons) || node3.children.len() != 2 {
        return None;
    }
    let step = node3.children.get(0)?;

    Some((cur, min, max, step))
}

#[allow(dead_code)]
fn list_nth(arena: &TreeArena, mut list: BoxId, mut n: usize) -> Option<BoxId> {
    loop {
        if arena.is_nil(list) {
            return None;
        }
        let node = arena.node(list)?;
        if !matches!(node.kind, NodeKind::Cons) || node.children.len() != 2 {
            return None;
        }
        let head = node.children.get(0)?;
        let tail = node.children.get(1)?;
        if n == 0 {
            return Some(head);
        }
        n -= 1;
        list = tail;
    }
}

fn dump_node(arena: &TreeArena, id: BoxId, out: &mut String) {
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
        NodeKind::Tag(tag) => {
            write!(out, "{tag}(").expect("String write cannot fail");
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
