//! Box construction helpers backed by `tlib::TreeArena`.
//!
//! # Source provenance (C++)
//! - `compiler/boxes/boxes.hh`
//! - `compiler/boxes/boxes.cpp`
//!
//! # Public API mapping status
//! - `1:1`: `box_ident`, `box_int`, `box_real`, `box_wire`, `box_cut`,
//!   `box_seq`, `box_par`, `box_rec`, `box_split`, `box_merge`,
//!   `box_appl`, `box_access`,
//!   `box_add`, `box_sub`, `box_mul`, `box_div`, `box_rem`,
//!   `box_and`, `box_or`, `box_xor`, `box_lsh`, `box_rsh`,
//!   `box_lt`, `box_le`, `box_gt`, `box_ge`, `box_eq`, `box_ne`,
//!   `box_pow`, `box_delay`, `box_delay1`, `box_min`, `box_max`,
//!   `box_prefix`, `box_int_cast`, `box_float_cast`,
//!   `box_read_only_table`, `box_write_read_table`,
//!   `box_select2`, `box_select3`,
//!   `box_assert_bounds`, `box_lowest`, `box_highest`,
//!   `box_attach`, `box_enable`, `box_control`,
//!   `box_ipar`, `box_iseq`, `box_isum`, `box_iprod`,
//!   `box_with_local_def`, `box_environment`, `box_component`, `box_library`,
//!   `box_waveform`, `box_route`,
//!   `ffunction`, `box_ffun`, `box_fconst`, `box_fvar`,
//!   `box_case`, `box_pattern_var`,
//!   `box_abstr`, `box_modulation`, `build_box_abstr`, `build_box_modulation`,
//!   `box_inputs`, `box_outputs`, `box_ondemand`, `box_upsampling`, `box_downsampling`,
//!   `box_button`, `box_checkbox`, `box_vslider`, `box_hslider`,
//!   `box_num_entry`, `box_vgroup`, `box_hgroup`, `box_tgroup`,
//!   `box_vbargraph`, `box_hbargraph`, `box_soundfile`
//! - `adapted`: `box_with_rec_def` (see function-level note)
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

/// Typed construction facade over [`TreeArena`] for box nodes.
///
/// This is the canonical write-side API for new passes. Existing `box_*` free
/// functions remain available as compatibility wrappers.
pub struct BoxBuilder<'a> {
    arena: &'a mut TreeArena,
}

impl<'a> BoxBuilder<'a> {
    /// Creates a builder bound to one arena.
    #[must_use]
    pub fn new(arena: &'a mut TreeArena) -> Self {
        Self { arena }
    }

    /// Equivalent to C++ `boxIdent(const char*)`.
    #[must_use]
    pub fn ident(&mut self, name: &str) -> BoxId {
        let sym = self.arena.symbol(name);
        intern_tag(self.arena, BOX_IDENT_TAG, &[sym])
    }

    /// Equivalent to C++ `boxInt`.
    #[must_use]
    pub fn int(&mut self, value: i64) -> BoxId {
        self.arena.int(value)
    }

    /// Equivalent to C++ `boxReal`.
    #[must_use]
    pub fn real(&mut self, value: f64) -> BoxId {
        self.arena.float(value)
    }

    /// Equivalent to C++ `boxWire`.
    #[must_use]
    pub fn wire(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_WIRE_TAG, &[])
    }

    /// Equivalent to C++ `boxCut`.
    #[must_use]
    pub fn cut(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_CUT_TAG, &[])
    }

    /// Equivalent to C++ `boxAdd`.
    #[must_use]
    pub fn add(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_ADD_TAG, &[])
    }

    /// Equivalent to C++ `boxSub`.
    #[must_use]
    pub fn sub(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_SUB_TAG, &[])
    }

    /// Equivalent to C++ `boxMul`.
    #[must_use]
    pub fn mul(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_MUL_TAG, &[])
    }

    /// Equivalent to C++ `boxDiv`.
    #[must_use]
    pub fn div(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_DIV_TAG, &[])
    }

    /// Equivalent to C++ `boxRem`.
    #[must_use]
    pub fn rem(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_REM_TAG, &[])
    }

    /// Equivalent to C++ `boxAND`.
    #[must_use]
    pub fn and(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_AND_TAG, &[])
    }

    /// Equivalent to C++ `boxOR`.
    #[must_use]
    pub fn or(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_OR_TAG, &[])
    }

    /// Equivalent to C++ `boxXOR`.
    #[must_use]
    pub fn xor(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_XOR_TAG, &[])
    }

    /// Equivalent to C++ `boxLeftShift`.
    #[must_use]
    pub fn lsh(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_LSH_TAG, &[])
    }

    /// Equivalent to C++ `boxARightShift`.
    #[must_use]
    pub fn rsh(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_RSH_TAG, &[])
    }

    /// Equivalent to C++ `boxLT`.
    #[must_use]
    pub fn lt(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_LT_TAG, &[])
    }

    /// Equivalent to C++ `boxLE`.
    #[must_use]
    pub fn le(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_LE_TAG, &[])
    }

    /// Equivalent to C++ `boxGT`.
    #[must_use]
    pub fn gt(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_GT_TAG, &[])
    }

    /// Equivalent to C++ `boxGE`.
    #[must_use]
    pub fn ge(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_GE_TAG, &[])
    }

    /// Equivalent to C++ `boxEQ`.
    #[must_use]
    pub fn eq(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_EQ_TAG, &[])
    }

    /// Equivalent to C++ `boxNE`.
    #[must_use]
    pub fn ne(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_NE_TAG, &[])
    }

    /// Equivalent to C++ `boxPow`.
    #[must_use]
    pub fn pow(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_POW_TAG, &[])
    }

    /// Equivalent to C++ `boxDelay`.
    #[must_use]
    pub fn delay(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_DELAY_TAG, &[])
    }

    /// Equivalent to C++ `boxDelay1`.
    #[must_use]
    pub fn delay1(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_DELAY1_TAG, &[])
    }

    /// Equivalent to C++ `boxMin`.
    #[must_use]
    pub fn min(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_MIN_TAG, &[])
    }

    /// Equivalent to C++ `boxMax`.
    #[must_use]
    pub fn max(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_MAX_TAG, &[])
    }

    /// Equivalent to C++ `boxPrefix`.
    #[must_use]
    pub fn prefix(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_PREFIX_TAG, &[])
    }

    /// Equivalent to C++ `boxIntCast`.
    #[must_use]
    pub fn int_cast(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_INT_CAST_TAG, &[])
    }

    /// Equivalent to C++ `boxFloatCast`.
    #[must_use]
    pub fn float_cast(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_FLOAT_CAST_TAG, &[])
    }

    /// Equivalent to C++ `boxReadOnlyTable`.
    #[must_use]
    pub fn read_only_table(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_READ_ONLY_TABLE_TAG, &[])
    }

    /// Equivalent to C++ `boxWriteReadTable`.
    #[must_use]
    pub fn write_read_table(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_WRITE_READ_TABLE_TAG, &[])
    }

    /// Equivalent to C++ `boxSelect2`.
    #[must_use]
    pub fn select2(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_SELECT2_TAG, &[])
    }

    /// Equivalent to C++ `boxSelect3`.
    #[must_use]
    pub fn select3(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_SELECT3_TAG, &[])
    }

    /// Equivalent to C++ `boxAssertBound`.
    #[must_use]
    pub fn assert_bounds(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_ASSERT_BOUNDS_TAG, &[])
    }

    /// Equivalent to C++ `boxLowest`.
    #[must_use]
    pub fn lowest(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_LOWEST_TAG, &[])
    }

    /// Equivalent to C++ `boxHighest`.
    #[must_use]
    pub fn highest(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_HIGHEST_TAG, &[])
    }

    /// Equivalent to C++ `boxAttach`.
    #[must_use]
    pub fn attach(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_ATTACH_TAG, &[])
    }

    /// Equivalent to C++ `boxEnable`.
    #[must_use]
    pub fn enable(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_ENABLE_TAG, &[])
    }

    /// Equivalent to C++ `boxControl`.
    #[must_use]
    pub fn control(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_CONTROL_TAG, &[])
    }

    /// Equivalent to C++ `boxSeq`.
    #[must_use]
    pub fn seq(&mut self, left: BoxId, right: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_SEQ_TAG, &[left, right])
    }

    /// Equivalent to C++ `boxPar`.
    #[must_use]
    pub fn par(&mut self, left: BoxId, right: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_PAR_TAG, &[left, right])
    }

    /// Equivalent to C++ `boxRec`.
    #[must_use]
    pub fn rec(&mut self, left: BoxId, right: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_REC_TAG, &[left, right])
    }

    /// Equivalent to C++ `boxSplit`.
    #[must_use]
    pub fn split(&mut self, left: BoxId, right: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_SPLIT_TAG, &[left, right])
    }

    /// Equivalent to C++ `boxMerge`.
    #[must_use]
    pub fn merge(&mut self, left: BoxId, right: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_MERGE_TAG, &[left, right])
    }

    /// Equivalent to C++ `boxAppl`.
    #[must_use]
    pub fn appl(&mut self, fun: BoxId, arglist: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_APPL_TAG, &[fun, arglist])
    }

    /// Equivalent to C++ `boxAccess`.
    #[must_use]
    pub fn access(&mut self, expr: BoxId, ident: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_ACCESS_TAG, &[expr, ident])
    }

    /// Equivalent to C++ `boxIPar`.
    #[must_use]
    pub fn ipar(&mut self, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_IPAR_TAG, &[index, count, body])
    }

    /// Equivalent to C++ `boxISeq`.
    #[must_use]
    pub fn iseq(&mut self, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_ISEQ_TAG, &[index, count, body])
    }

    /// Equivalent to C++ `boxISum`.
    #[must_use]
    pub fn isum(&mut self, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_ISUM_TAG, &[index, count, body])
    }

    /// Equivalent to C++ `boxIProd`.
    #[must_use]
    pub fn iprod(&mut self, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_IPROD_TAG, &[index, count, body])
    }

    /// Equivalent to C++ `boxWithLocalDef`.
    #[must_use]
    pub fn with_local_def(&mut self, body: BoxId, ldef: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_WITH_LOCAL_DEF_TAG, &[body, ldef])
    }

    /// Equivalent to C++ `boxWithRecDef` (adapted representation).
    #[must_use]
    pub fn with_rec_def(&mut self, body: BoxId, ldef: BoxId, ldef2: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_WITH_REC_DEF_TAG, &[body, ldef, ldef2])
    }

    /// Equivalent to C++ `boxEnvironment`.
    #[must_use]
    pub fn environment(&mut self) -> BoxId {
        intern_tag(self.arena, BOX_ENVIRONMENT_TAG, &[])
    }

    /// Equivalent to C++ `boxComponent`.
    #[must_use]
    pub fn component(&mut self, filename: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_COMPONENT_TAG, &[filename])
    }

    /// Equivalent to C++ `boxLibrary`.
    #[must_use]
    pub fn library(&mut self, filename: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_LIBRARY_TAG, &[filename])
    }

    /// Equivalent to C++ `boxWaveform`.
    #[must_use]
    pub fn waveform(&mut self, values: &[BoxId]) -> BoxId {
        let mut list = self.arena.nil();
        for value in values.iter().rev() {
            list = self.arena.cons(*value, list);
        }
        intern_tag(self.arena, BOX_WAVEFORM_TAG, &[list])
    }

    /// Equivalent to C++ `boxRoute`.
    #[must_use]
    pub fn route(&mut self, n: BoxId, m: BoxId, route_spec: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_ROUTE_TAG, &[n, m, route_spec])
    }

    /// Equivalent to C++ `ffunction(signature, incfile, libfile)`.
    #[must_use]
    pub fn ffunction(&mut self, signature: BoxId, incfile: BoxId, libfile: BoxId) -> BoxId {
        intern_tag(self.arena, FFUN_TAG, &[signature, incfile, libfile])
    }

    /// Equivalent to C++ `boxFFun`.
    #[must_use]
    pub fn ffun(&mut self, ff: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_FFUN_TAG, &[ff])
    }

    /// Equivalent to C++ `boxFConst`.
    #[must_use]
    pub fn fconst(&mut self, ty: BoxId, name: BoxId, file: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_FCONST_TAG, &[ty, name, file])
    }

    /// Equivalent to C++ `boxFVar`.
    #[must_use]
    pub fn fvar(&mut self, ty: BoxId, name: BoxId, file: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_FVAR_TAG, &[ty, name, file])
    }

    /// Equivalent to C++ `boxCase`.
    #[must_use]
    pub fn case(&mut self, rules: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_CASE_TAG, &[rules])
    }

    /// Equivalent to C++ `boxPatternVar`.
    #[must_use]
    pub fn pattern_var(&mut self, ident: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_PATTERN_VAR_TAG, &[ident])
    }

    /// Equivalent to C++ `boxAbstr`.
    #[must_use]
    pub fn abstr(&mut self, arg: BoxId, body: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_ABSTR_TAG, &[arg, body])
    }

    /// Equivalent to C++ `boxModulation`.
    #[must_use]
    pub fn modulation(&mut self, arg: BoxId, body: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_MODULATION_TAG, &[arg, body])
    }

    /// Equivalent to C++ `boxInputs`.
    #[must_use]
    pub fn inputs(&mut self, expr: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_INPUTS_TAG, &[expr])
    }

    /// Equivalent to C++ `boxOutputs`.
    #[must_use]
    pub fn outputs(&mut self, expr: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_OUTPUTS_TAG, &[expr])
    }

    /// Equivalent to C++ `boxOndemand`.
    #[must_use]
    pub fn ondemand(&mut self, expr: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_ONDEMAND_TAG, &[expr])
    }

    /// Equivalent to C++ `boxUpsampling`.
    #[must_use]
    pub fn upsampling(&mut self, expr: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_UPSAMPLING_TAG, &[expr])
    }

    /// Equivalent to C++ `boxDownsampling`.
    #[must_use]
    pub fn downsampling(&mut self, expr: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_DOWNSAMPLING_TAG, &[expr])
    }

    /// Equivalent to C++ `boxButton`.
    #[must_use]
    pub fn button(&mut self, label: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_BUTTON_TAG, &[label])
    }

    /// Equivalent to C++ `boxCheckbox`.
    #[must_use]
    pub fn checkbox(&mut self, label: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_CHECKBOX_TAG, &[label])
    }

    /// Equivalent to C++ `boxVSlider`.
    #[must_use]
    pub fn vslider(
        &mut self,
        label: BoxId,
        cur: BoxId,
        min: BoxId,
        max: BoxId,
        step: BoxId,
    ) -> BoxId {
        let params = list4(self.arena, cur, min, max, step);
        intern_tag(self.arena, BOX_VSLIDER_TAG, &[label, params])
    }

    /// Equivalent to C++ `boxHSlider`.
    #[must_use]
    pub fn hslider(
        &mut self,
        label: BoxId,
        cur: BoxId,
        min: BoxId,
        max: BoxId,
        step: BoxId,
    ) -> BoxId {
        let params = list4(self.arena, cur, min, max, step);
        intern_tag(self.arena, BOX_HSLIDER_TAG, &[label, params])
    }

    /// Equivalent to C++ `boxNumEntry`.
    #[must_use]
    pub fn num_entry(
        &mut self,
        label: BoxId,
        cur: BoxId,
        min: BoxId,
        max: BoxId,
        step: BoxId,
    ) -> BoxId {
        let params = list4(self.arena, cur, min, max, step);
        intern_tag(self.arena, BOX_NUM_ENTRY_TAG, &[label, params])
    }

    /// Equivalent to C++ `boxVGroup`.
    #[must_use]
    pub fn vgroup(&mut self, label: BoxId, expr: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_VGROUP_TAG, &[label, expr])
    }

    /// Equivalent to C++ `boxHGroup`.
    #[must_use]
    pub fn hgroup(&mut self, label: BoxId, expr: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_HGROUP_TAG, &[label, expr])
    }

    /// Equivalent to C++ `boxTGroup`.
    #[must_use]
    pub fn tgroup(&mut self, label: BoxId, expr: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_TGROUP_TAG, &[label, expr])
    }

    /// Equivalent to C++ `boxVBargraph`.
    #[must_use]
    pub fn vbargraph(&mut self, label: BoxId, min: BoxId, max: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_VBARGRAPH_TAG, &[label, min, max])
    }

    /// Equivalent to C++ `boxHBargraph`.
    #[must_use]
    pub fn hbargraph(&mut self, label: BoxId, min: BoxId, max: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_HBARGRAPH_TAG, &[label, min, max])
    }

    /// Equivalent to C++ `boxSoundfile`.
    #[must_use]
    pub fn soundfile(&mut self, label: BoxId, chan: BoxId) -> BoxId {
        intern_tag(self.arena, BOX_SOUNDFILE_TAG, &[label, chan])
    }

    /// Equivalent to C++ `buildBoxAbstr(largs, body)`.
    #[must_use]
    pub fn build_abstr(&mut self, args: BoxId, body: BoxId) -> BoxId {
        if self.arena.is_nil(args) {
            return body;
        }
        let Some(head) = self.arena.hd(args) else {
            return body;
        };
        let Some(tail) = self.arena.tl(args) else {
            return body;
        };
        let nested = self.build_abstr(tail, body);
        self.abstr(head, nested)
    }

    /// Equivalent to C++ `buildBoxModulation(largs, body)`.
    #[must_use]
    pub fn build_modulation(&mut self, args: BoxId, body: BoxId) -> BoxId {
        if self.arena.is_nil(args) {
            return body;
        }
        let Some(head) = self.arena.hd(args) else {
            return body;
        };
        let Some(tail) = self.arena.tl(args) else {
            return body;
        };
        let nested = self.modulation(head, body);
        self.build_modulation(tail, nested)
    }
}

/// Canonical read-side dispatch for box nodes.
///
/// This first tranche covers core constructors used by parser/eval/propagate
/// bootstrap paths.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BoxMatch<'a> {
    Int(i64),
    Real(f64),
    Ident(&'a str),
    Wire,
    Cut,
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
    Seq(BoxId, BoxId),
    Par(BoxId, BoxId),
    Rec(BoxId, BoxId),
    Split(BoxId, BoxId),
    Merge(BoxId, BoxId),
    Appl(BoxId, BoxId),
    Access(BoxId, BoxId),
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
    FFunction(BoxId, BoxId, BoxId),
    FFun(BoxId),
    FConst(BoxId, BoxId, BoxId),
    FVar(BoxId, BoxId, BoxId),
    Case(BoxId),
    PatternVar(BoxId),
    Abstr(BoxId, BoxId),
    Modulation(BoxId, BoxId),
    Inputs(BoxId),
    Outputs(BoxId),
    OnDemand(BoxId),
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
    Unknown,
}

/// Matches one box node to its canonical variant.
#[must_use]
pub fn match_box<'a>(arena: &'a TreeArena, id: BoxId) -> BoxMatch<'a> {
    match arena.kind(id) {
        Some(NodeKind::Int(value)) => BoxMatch::Int(*value),
        Some(NodeKind::FloatBits(bits)) => BoxMatch::Real(f64::from_bits(*bits)),
        Some(NodeKind::Tag(tag_id)) => match arena.tag_name(*tag_id).unwrap_or("") {
            BOX_IDENT_TAG => {
                let Some([sym]) = match_tag_arity(arena, id, BOX_IDENT_TAG, 1) else {
                    return BoxMatch::Unknown;
                };
                match arena.kind(*sym) {
                    Some(NodeKind::Symbol(name)) => BoxMatch::Ident(name.as_ref()),
                    _ => BoxMatch::Unknown,
                }
            }
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
            BOX_SEQ_TAG => match match_binary(arena, id, BOX_SEQ_TAG) {
                Some((a, b)) => BoxMatch::Seq(a, b),
                None => BoxMatch::Unknown,
            },
            BOX_PAR_TAG => match match_binary(arena, id, BOX_PAR_TAG) {
                Some((a, b)) => BoxMatch::Par(a, b),
                None => BoxMatch::Unknown,
            },
            BOX_REC_TAG => match match_binary(arena, id, BOX_REC_TAG) {
                Some((a, b)) => BoxMatch::Rec(a, b),
                None => BoxMatch::Unknown,
            },
            BOX_SPLIT_TAG => match match_binary(arena, id, BOX_SPLIT_TAG) {
                Some((a, b)) => BoxMatch::Split(a, b),
                None => BoxMatch::Unknown,
            },
            BOX_MERGE_TAG => match match_binary(arena, id, BOX_MERGE_TAG) {
                Some((a, b)) => BoxMatch::Merge(a, b),
                None => BoxMatch::Unknown,
            },
            BOX_APPL_TAG => match match_binary(arena, id, BOX_APPL_TAG) {
                Some((a, b)) => BoxMatch::Appl(a, b),
                None => BoxMatch::Unknown,
            },
            BOX_ACCESS_TAG => match match_binary(arena, id, BOX_ACCESS_TAG) {
                Some((a, b)) => BoxMatch::Access(a, b),
                None => BoxMatch::Unknown,
            },
            BOX_IPAR_TAG => match match_ternary(arena, id, BOX_IPAR_TAG) {
                Some((a, b, c)) => BoxMatch::IPar(a, b, c),
                None => BoxMatch::Unknown,
            },
            BOX_ISEQ_TAG => match match_ternary(arena, id, BOX_ISEQ_TAG) {
                Some((a, b, c)) => BoxMatch::ISeq(a, b, c),
                None => BoxMatch::Unknown,
            },
            BOX_ISUM_TAG => match match_ternary(arena, id, BOX_ISUM_TAG) {
                Some((a, b, c)) => BoxMatch::ISum(a, b, c),
                None => BoxMatch::Unknown,
            },
            BOX_IPROD_TAG => match match_ternary(arena, id, BOX_IPROD_TAG) {
                Some((a, b, c)) => BoxMatch::IProd(a, b, c),
                None => BoxMatch::Unknown,
            },
            BOX_WITH_LOCAL_DEF_TAG => match match_binary(arena, id, BOX_WITH_LOCAL_DEF_TAG) {
                Some((a, b)) => BoxMatch::WithLocalDef(a, b),
                None => BoxMatch::Unknown,
            },
            BOX_WITH_REC_DEF_TAG => match match_ternary(arena, id, BOX_WITH_REC_DEF_TAG) {
                Some((a, b, c)) => BoxMatch::WithRecDef(a, b, c),
                None => BoxMatch::Unknown,
            },
            BOX_ENVIRONMENT_TAG => BoxMatch::Environment,
            BOX_COMPONENT_TAG => match match_unary(arena, id, BOX_COMPONENT_TAG) {
                Some(a) => BoxMatch::Component(a),
                None => BoxMatch::Unknown,
            },
            BOX_LIBRARY_TAG => match match_unary(arena, id, BOX_LIBRARY_TAG) {
                Some(a) => BoxMatch::Library(a),
                None => BoxMatch::Unknown,
            },
            BOX_WAVEFORM_TAG => match match_unary(arena, id, BOX_WAVEFORM_TAG) {
                Some(a) => BoxMatch::Waveform(a),
                None => BoxMatch::Unknown,
            },
            BOX_ROUTE_TAG => match match_ternary(arena, id, BOX_ROUTE_TAG) {
                Some((a, b, c)) => BoxMatch::Route(a, b, c),
                None => BoxMatch::Unknown,
            },
            FFUN_TAG => match match_ternary(arena, id, FFUN_TAG) {
                Some((a, b, c)) => BoxMatch::FFunction(a, b, c),
                None => BoxMatch::Unknown,
            },
            BOX_FFUN_TAG => match match_unary(arena, id, BOX_FFUN_TAG) {
                Some(a) => BoxMatch::FFun(a),
                None => BoxMatch::Unknown,
            },
            BOX_FCONST_TAG => match match_ternary(arena, id, BOX_FCONST_TAG) {
                Some((a, b, c)) => BoxMatch::FConst(a, b, c),
                None => BoxMatch::Unknown,
            },
            BOX_FVAR_TAG => match match_ternary(arena, id, BOX_FVAR_TAG) {
                Some((a, b, c)) => BoxMatch::FVar(a, b, c),
                None => BoxMatch::Unknown,
            },
            BOX_CASE_TAG => match match_unary(arena, id, BOX_CASE_TAG) {
                Some(a) => BoxMatch::Case(a),
                None => BoxMatch::Unknown,
            },
            BOX_PATTERN_VAR_TAG => match match_unary(arena, id, BOX_PATTERN_VAR_TAG) {
                Some(a) => BoxMatch::PatternVar(a),
                None => BoxMatch::Unknown,
            },
            BOX_ABSTR_TAG => match match_binary(arena, id, BOX_ABSTR_TAG) {
                Some((a, b)) => BoxMatch::Abstr(a, b),
                None => BoxMatch::Unknown,
            },
            BOX_MODULATION_TAG => match match_binary(arena, id, BOX_MODULATION_TAG) {
                Some((a, b)) => BoxMatch::Modulation(a, b),
                None => BoxMatch::Unknown,
            },
            BOX_INPUTS_TAG => match match_unary(arena, id, BOX_INPUTS_TAG) {
                Some(a) => BoxMatch::Inputs(a),
                None => BoxMatch::Unknown,
            },
            BOX_OUTPUTS_TAG => match match_unary(arena, id, BOX_OUTPUTS_TAG) {
                Some(a) => BoxMatch::Outputs(a),
                None => BoxMatch::Unknown,
            },
            BOX_ONDEMAND_TAG => match match_unary(arena, id, BOX_ONDEMAND_TAG) {
                Some(a) => BoxMatch::OnDemand(a),
                None => BoxMatch::Unknown,
            },
            BOX_UPSAMPLING_TAG => match match_unary(arena, id, BOX_UPSAMPLING_TAG) {
                Some(a) => BoxMatch::Upsampling(a),
                None => BoxMatch::Unknown,
            },
            BOX_DOWNSAMPLING_TAG => match match_unary(arena, id, BOX_DOWNSAMPLING_TAG) {
                Some(a) => BoxMatch::Downsampling(a),
                None => BoxMatch::Unknown,
            },
            BOX_BUTTON_TAG => match match_unary(arena, id, BOX_BUTTON_TAG) {
                Some(a) => BoxMatch::Button(a),
                None => BoxMatch::Unknown,
            },
            BOX_CHECKBOX_TAG => match match_unary(arena, id, BOX_CHECKBOX_TAG) {
                Some(a) => BoxMatch::Checkbox(a),
                None => BoxMatch::Unknown,
            },
            BOX_VSLIDER_TAG => match match_slider(arena, id, BOX_VSLIDER_TAG) {
                Some((a, b, c, d, e)) => BoxMatch::VSlider(a, b, c, d, e),
                None => BoxMatch::Unknown,
            },
            BOX_HSLIDER_TAG => match match_slider(arena, id, BOX_HSLIDER_TAG) {
                Some((a, b, c, d, e)) => BoxMatch::HSlider(a, b, c, d, e),
                None => BoxMatch::Unknown,
            },
            BOX_NUM_ENTRY_TAG => match match_slider(arena, id, BOX_NUM_ENTRY_TAG) {
                Some((a, b, c, d, e)) => BoxMatch::NumEntry(a, b, c, d, e),
                None => BoxMatch::Unknown,
            },
            BOX_VGROUP_TAG => match match_binary(arena, id, BOX_VGROUP_TAG) {
                Some((a, b)) => BoxMatch::VGroup(a, b),
                None => BoxMatch::Unknown,
            },
            BOX_HGROUP_TAG => match match_binary(arena, id, BOX_HGROUP_TAG) {
                Some((a, b)) => BoxMatch::HGroup(a, b),
                None => BoxMatch::Unknown,
            },
            BOX_TGROUP_TAG => match match_binary(arena, id, BOX_TGROUP_TAG) {
                Some((a, b)) => BoxMatch::TGroup(a, b),
                None => BoxMatch::Unknown,
            },
            BOX_VBARGRAPH_TAG => match match_ternary(arena, id, BOX_VBARGRAPH_TAG) {
                Some((a, b, c)) => BoxMatch::VBargraph(a, b, c),
                None => BoxMatch::Unknown,
            },
            BOX_HBARGRAPH_TAG => match match_ternary(arena, id, BOX_HBARGRAPH_TAG) {
                Some((a, b, c)) => BoxMatch::HBargraph(a, b, c),
                None => BoxMatch::Unknown,
            },
            BOX_SOUNDFILE_TAG => match match_binary(arena, id, BOX_SOUNDFILE_TAG) {
                Some((a, b)) => BoxMatch::Soundfile(a, b),
                None => BoxMatch::Unknown,
            },
            _ => BoxMatch::Unknown,
        },
        _ => BoxMatch::Unknown,
    }
}

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

/// Equivalent to C++ `boxIdent(const char*)`.
#[must_use]
pub fn box_ident(arena: &mut TreeArena, name: &str) -> BoxId {
    BoxBuilder::new(arena).ident(name)
}

/// Returns identifier symbol name when `b` is `box_ident`.
#[must_use]
pub fn box_ident_name(arena: &TreeArena, b: BoxId) -> Option<&str> {
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
pub fn box_int(arena: &mut TreeArena, value: i64) -> BoxId {
    BoxBuilder::new(arena).int(value)
}

/// Equivalent to C++ `boxReal`.
#[must_use]
pub fn box_real(arena: &mut TreeArena, value: f64) -> BoxId {
    BoxBuilder::new(arena).real(value)
}

/// Predicate equivalent to C++ `isBoxInt`.
#[must_use]
pub fn is_box_int(arena: &TreeArena, b: BoxId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Int(_))
}

/// Predicate equivalent to C++ `isBoxReal`.
#[must_use]
pub fn is_box_real(arena: &TreeArena, b: BoxId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Real(_))
}

/// Equivalent to C++ `boxWire`.
#[must_use]
pub fn box_wire(arena: &mut TreeArena) -> BoxId {
    BoxBuilder::new(arena).wire()
}

/// Equivalent to C++ `boxCut`.
#[must_use]
pub fn box_cut(arena: &mut TreeArena) -> BoxId {
    BoxBuilder::new(arena).cut()
}

/// Predicate equivalent to C++ `isBoxWire`.
#[must_use]
pub fn is_box_wire(arena: &TreeArena, b: BoxId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Wire)
}

/// Predicate equivalent to C++ `isBoxCut`.
#[must_use]
pub fn is_box_cut(arena: &TreeArena, b: BoxId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Cut)
}

/// Equivalent to C++ `boxSeq`.
#[must_use]
pub fn box_seq(arena: &mut TreeArena, left: BoxId, right: BoxId) -> BoxId {
    BoxBuilder::new(arena).seq(left, right)
}

/// Equivalent to C++ `boxPar`.
#[must_use]
pub fn box_par(arena: &mut TreeArena, left: BoxId, right: BoxId) -> BoxId {
    BoxBuilder::new(arena).par(left, right)
}

/// Equivalent to C++ `boxRec`.
#[must_use]
pub fn box_rec(arena: &mut TreeArena, left: BoxId, right: BoxId) -> BoxId {
    BoxBuilder::new(arena).rec(left, right)
}

/// Equivalent to C++ `boxSplit`.
#[must_use]
pub fn box_split(arena: &mut TreeArena, left: BoxId, right: BoxId) -> BoxId {
    BoxBuilder::new(arena).split(left, right)
}

/// Equivalent to C++ `boxMerge`.
#[must_use]
pub fn box_merge(arena: &mut TreeArena, left: BoxId, right: BoxId) -> BoxId {
    BoxBuilder::new(arena).merge(left, right)
}

/// Equivalent to C++ `boxAppl`.
#[must_use]
pub fn box_appl(arena: &mut TreeArena, fun: BoxId, arglist: BoxId) -> BoxId {
    BoxBuilder::new(arena).appl(fun, arglist)
}

/// Returns `(fun, arglist)` when `b` is `box_appl`.
#[must_use]
pub fn is_box_appl(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_APPL_TAG)
}

/// Equivalent to C++ `boxAccess`.
#[must_use]
pub fn box_access(arena: &mut TreeArena, expr: BoxId, ident: BoxId) -> BoxId {
    BoxBuilder::new(arena).access(expr, ident)
}

/// Returns `(expr, ident)` when `b` is `box_access`.
#[must_use]
pub fn is_box_access(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_ACCESS_TAG)
}

/// Returns `(left, right)` when `b` is `box_seq`.
#[must_use]
pub fn is_box_seq(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match match_box(arena, b) {
        BoxMatch::Seq(a, c) => Some((a, c)),
        _ => None,
    }
}

/// Returns `(left, right)` when `b` is `box_par`.
#[must_use]
pub fn is_box_par(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match match_box(arena, b) {
        BoxMatch::Par(a, c) => Some((a, c)),
        _ => None,
    }
}

/// Returns `(left, right)` when `b` is `box_rec`.
#[must_use]
pub fn is_box_rec(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match match_box(arena, b) {
        BoxMatch::Rec(a, c) => Some((a, c)),
        _ => None,
    }
}

/// Returns `(left, right)` when `b` is `box_split`.
#[must_use]
pub fn is_box_split(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match match_box(arena, b) {
        BoxMatch::Split(a, c) => Some((a, c)),
        _ => None,
    }
}

/// Returns `(left, right)` when `b` is `box_merge`.
#[must_use]
pub fn is_box_merge(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match match_box(arena, b) {
        BoxMatch::Merge(a, c) => Some((a, c)),
        _ => None,
    }
}

/// Equivalent to C++ `boxAdd`.
#[must_use]
pub fn box_add(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_ADD_TAG, &[])
}

/// Equivalent to C++ `boxSub`.
#[must_use]
pub fn box_sub(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_SUB_TAG, &[])
}

/// Equivalent to C++ `boxMul`.
#[must_use]
pub fn box_mul(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_MUL_TAG, &[])
}

/// Equivalent to C++ `boxDiv`.
#[must_use]
pub fn box_div(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_DIV_TAG, &[])
}

/// Equivalent to C++ `boxRem`.
#[must_use]
pub fn box_rem(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_REM_TAG, &[])
}

/// Equivalent to C++ `boxAND`.
#[must_use]
pub fn box_and(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_AND_TAG, &[])
}

/// Equivalent to C++ `boxOR`.
#[must_use]
pub fn box_or(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_OR_TAG, &[])
}

/// Equivalent to C++ `boxXOR`.
#[must_use]
pub fn box_xor(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_XOR_TAG, &[])
}

/// Equivalent to C++ `boxLeftShift`.
#[must_use]
pub fn box_lsh(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_LSH_TAG, &[])
}

/// Equivalent to C++ `boxARightShift`.
#[must_use]
pub fn box_rsh(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_RSH_TAG, &[])
}

/// Equivalent to C++ `boxLT`.
#[must_use]
pub fn box_lt(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_LT_TAG, &[])
}

/// Equivalent to C++ `boxLE`.
#[must_use]
pub fn box_le(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_LE_TAG, &[])
}

/// Equivalent to C++ `boxGT`.
#[must_use]
pub fn box_gt(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_GT_TAG, &[])
}

/// Equivalent to C++ `boxGE`.
#[must_use]
pub fn box_ge(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_GE_TAG, &[])
}

/// Equivalent to C++ `boxEQ`.
#[must_use]
pub fn box_eq(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_EQ_TAG, &[])
}

/// Equivalent to C++ `boxNE`.
#[must_use]
pub fn box_ne(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_NE_TAG, &[])
}

/// Equivalent to C++ `boxPow`.
#[must_use]
pub fn box_pow(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_POW_TAG, &[])
}

/// Equivalent to C++ `boxDelay`.
#[must_use]
pub fn box_delay(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_DELAY_TAG, &[])
}

/// Equivalent to C++ `boxDelay1`.
#[must_use]
pub fn box_delay1(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_DELAY1_TAG, &[])
}

/// Equivalent to C++ `boxMin`.
#[must_use]
pub fn box_min(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_MIN_TAG, &[])
}

/// Equivalent to C++ `boxMax`.
#[must_use]
pub fn box_max(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_MAX_TAG, &[])
}

/// Equivalent to C++ `boxPrefix`.
#[must_use]
pub fn box_prefix(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_PREFIX_TAG, &[])
}

/// Equivalent to C++ `boxIntCast`.
#[must_use]
pub fn box_int_cast(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_INT_CAST_TAG, &[])
}

/// Equivalent to C++ `boxFloatCast`.
#[must_use]
pub fn box_float_cast(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_FLOAT_CAST_TAG, &[])
}

/// Equivalent to C++ `boxReadOnlyTable`.
#[must_use]
pub fn box_read_only_table(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_READ_ONLY_TABLE_TAG, &[])
}

/// Equivalent to C++ `boxWriteReadTable`.
#[must_use]
pub fn box_write_read_table(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_WRITE_READ_TABLE_TAG, &[])
}

/// Equivalent to C++ `boxSelect2`.
#[must_use]
pub fn box_select2(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_SELECT2_TAG, &[])
}

/// Equivalent to C++ `boxSelect3`.
#[must_use]
pub fn box_select3(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_SELECT3_TAG, &[])
}

/// Equivalent to C++ `boxAssertBound`.
#[must_use]
pub fn box_assert_bounds(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_ASSERT_BOUNDS_TAG, &[])
}

/// Equivalent to C++ `boxLowest`.
#[must_use]
pub fn box_lowest(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_LOWEST_TAG, &[])
}

/// Equivalent to C++ `boxHighest`.
#[must_use]
pub fn box_highest(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_HIGHEST_TAG, &[])
}

/// Equivalent to C++ `boxAttach`.
#[must_use]
pub fn box_attach(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_ATTACH_TAG, &[])
}

/// Equivalent to C++ `boxEnable`.
#[must_use]
pub fn box_enable(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_ENABLE_TAG, &[])
}

/// Equivalent to C++ `boxControl`.
#[must_use]
pub fn box_control(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_CONTROL_TAG, &[])
}

macro_rules! define_is_prim {
    ($fn_name:ident, $tag:ident) => {
        #[must_use]
        pub fn $fn_name(arena: &TreeArena, b: BoxId) -> bool {
            match_tag_arity(arena, b, $tag, 0).is_some()
        }
    };
}

define_is_prim!(is_box_add, BOX_ADD_TAG);
define_is_prim!(is_box_sub, BOX_SUB_TAG);
define_is_prim!(is_box_mul, BOX_MUL_TAG);
define_is_prim!(is_box_div, BOX_DIV_TAG);
define_is_prim!(is_box_rem, BOX_REM_TAG);
define_is_prim!(is_box_and, BOX_AND_TAG);
define_is_prim!(is_box_or, BOX_OR_TAG);
define_is_prim!(is_box_xor, BOX_XOR_TAG);
define_is_prim!(is_box_lsh, BOX_LSH_TAG);
define_is_prim!(is_box_rsh, BOX_RSH_TAG);
define_is_prim!(is_box_lt, BOX_LT_TAG);
define_is_prim!(is_box_le, BOX_LE_TAG);
define_is_prim!(is_box_gt, BOX_GT_TAG);
define_is_prim!(is_box_ge, BOX_GE_TAG);
define_is_prim!(is_box_eq, BOX_EQ_TAG);
define_is_prim!(is_box_ne, BOX_NE_TAG);
define_is_prim!(is_box_pow, BOX_POW_TAG);
define_is_prim!(is_box_delay, BOX_DELAY_TAG);
define_is_prim!(is_box_delay1, BOX_DELAY1_TAG);
define_is_prim!(is_box_min, BOX_MIN_TAG);
define_is_prim!(is_box_max, BOX_MAX_TAG);
define_is_prim!(is_box_prefix, BOX_PREFIX_TAG);
define_is_prim!(is_box_int_cast, BOX_INT_CAST_TAG);
define_is_prim!(is_box_float_cast, BOX_FLOAT_CAST_TAG);
define_is_prim!(is_box_read_only_table, BOX_READ_ONLY_TABLE_TAG);
define_is_prim!(is_box_write_read_table, BOX_WRITE_READ_TABLE_TAG);
define_is_prim!(is_box_select2, BOX_SELECT2_TAG);
define_is_prim!(is_box_select3, BOX_SELECT3_TAG);
define_is_prim!(is_box_assert_bounds, BOX_ASSERT_BOUNDS_TAG);
define_is_prim!(is_box_lowest, BOX_LOWEST_TAG);
define_is_prim!(is_box_highest, BOX_HIGHEST_TAG);
define_is_prim!(is_box_attach, BOX_ATTACH_TAG);
define_is_prim!(is_box_enable, BOX_ENABLE_TAG);
define_is_prim!(is_box_control, BOX_CONTROL_TAG);

/// Equivalent to C++ `boxIPar`.
#[must_use]
pub fn box_ipar(arena: &mut TreeArena, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
    intern_tag(arena, BOX_IPAR_TAG, &[index, count, body])
}

/// Returns `(index, count, body)` when `b` is `box_ipar`.
#[must_use]
pub fn is_box_ipar(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_IPAR_TAG)
}

/// Equivalent to C++ `boxISeq`.
#[must_use]
pub fn box_iseq(arena: &mut TreeArena, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
    intern_tag(arena, BOX_ISEQ_TAG, &[index, count, body])
}

/// Returns `(index, count, body)` when `b` is `box_iseq`.
#[must_use]
pub fn is_box_iseq(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_ISEQ_TAG)
}

/// Equivalent to C++ `boxISum`.
#[must_use]
pub fn box_isum(arena: &mut TreeArena, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
    intern_tag(arena, BOX_ISUM_TAG, &[index, count, body])
}

/// Returns `(index, count, body)` when `b` is `box_isum`.
#[must_use]
pub fn is_box_isum(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_ISUM_TAG)
}

/// Equivalent to C++ `boxIProd`.
#[must_use]
pub fn box_iprod(arena: &mut TreeArena, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
    intern_tag(arena, BOX_IPROD_TAG, &[index, count, body])
}

/// Returns `(index, count, body)` when `b` is `box_iprod`.
#[must_use]
pub fn is_box_iprod(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_IPROD_TAG)
}

/// Equivalent to C++ `boxWithLocalDef`.
#[must_use]
pub fn box_with_local_def(arena: &mut TreeArena, body: BoxId, ldef: BoxId) -> BoxId {
    intern_tag(arena, BOX_WITH_LOCAL_DEF_TAG, &[body, ldef])
}

/// Returns `(body, ldef)` when `b` is `box_with_local_def`.
#[must_use]
pub fn is_box_with_local_def(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_WITH_LOCAL_DEF_TAG)
}

/// Adapted representation for C++ `boxWithRecDef`.
///
/// C++ performs an immediate lowering/expansion into a local-definition structure.
/// For the current parser prototype, Rust stores an explicit node preserving the three
/// inputs `(body, ldef, ldef2)`. This keeps parser output deterministic and lets later
/// phases choose where lowering happens.
#[must_use]
pub fn box_with_rec_def(arena: &mut TreeArena, body: BoxId, ldef: BoxId, ldef2: BoxId) -> BoxId {
    intern_tag(arena, BOX_WITH_REC_DEF_TAG, &[body, ldef, ldef2])
}

/// Returns `(body, ldef, ldef2)` when `b` is `box_with_rec_def`.
#[must_use]
pub fn is_box_with_rec_def(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    let [body, ldef, ldef2] = match_tag_arity(arena, b, BOX_WITH_REC_DEF_TAG, 3)? else {
        return None;
    };
    Some((*body, *ldef, *ldef2))
}

/// Equivalent to C++ `boxEnvironment`.
#[must_use]
pub fn box_environment(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_ENVIRONMENT_TAG, &[])
}

/// Predicate equivalent to C++ `isBoxEnvironment`.
#[must_use]
pub fn is_box_environment(arena: &TreeArena, b: BoxId) -> bool {
    match_tag_arity(arena, b, BOX_ENVIRONMENT_TAG, 0).is_some()
}

/// Equivalent to C++ `boxComponent`.
#[must_use]
pub fn box_component(arena: &mut TreeArena, filename: BoxId) -> BoxId {
    intern_tag(arena, BOX_COMPONENT_TAG, &[filename])
}

/// Returns `filename` when `b` is `box_component`.
#[must_use]
pub fn is_box_component(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_COMPONENT_TAG)
}

/// Equivalent to C++ `boxLibrary`.
#[must_use]
pub fn box_library(arena: &mut TreeArena, filename: BoxId) -> BoxId {
    intern_tag(arena, BOX_LIBRARY_TAG, &[filename])
}

/// Returns `filename` when `b` is `box_library`.
#[must_use]
pub fn is_box_library(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_LIBRARY_TAG)
}

/// Equivalent to C++ `boxWaveform`.
///
/// Rust keeps a deterministic list payload in one child:
/// `tree(BOXWAVEFORM, cons(v0, cons(v1, ...)))`.
#[must_use]
pub fn box_waveform(arena: &mut TreeArena, values: &[BoxId]) -> BoxId {
    let mut list = arena.nil();
    for value in values.iter().rev() {
        list = arena.cons(*value, list);
    }
    intern_tag(arena, BOX_WAVEFORM_TAG, &[list])
}

/// Returns waveform list payload when `b` is `box_waveform`.
#[must_use]
pub fn is_box_waveform(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_WAVEFORM_TAG)
}

/// Equivalent to C++ `boxRoute`.
#[must_use]
pub fn box_route(arena: &mut TreeArena, n: BoxId, m: BoxId, route_spec: BoxId) -> BoxId {
    intern_tag(arena, BOX_ROUTE_TAG, &[n, m, route_spec])
}

/// Returns `(n, m, route_spec)` when `b` is `box_route`.
#[must_use]
pub fn is_box_route(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_ROUTE_TAG)
}

/// Equivalent to C++ `ffunction(signature, incfile, libfile)`.
#[must_use]
pub fn ffunction(arena: &mut TreeArena, signature: BoxId, incfile: BoxId, libfile: BoxId) -> BoxId {
    intern_tag(arena, FFUN_TAG, &[signature, incfile, libfile])
}

/// Returns `(signature, incfile, libfile)` when `b` is `ffunction(...)`.
#[must_use]
pub fn is_ffunction(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, FFUN_TAG)
}

/// Equivalent to C++ `boxFFun`.
#[must_use]
pub fn box_ffun(arena: &mut TreeArena, ff: BoxId) -> BoxId {
    intern_tag(arena, BOX_FFUN_TAG, &[ff])
}

/// Returns wrapped foreign-function descriptor when `b` is `box_ffun`.
#[must_use]
pub fn is_box_ffun(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_FFUN_TAG)
}

/// Equivalent to C++ `boxFConst`.
#[must_use]
pub fn box_fconst(arena: &mut TreeArena, ty: BoxId, name: BoxId, file: BoxId) -> BoxId {
    intern_tag(arena, BOX_FCONST_TAG, &[ty, name, file])
}

/// Returns `(ty, name, file)` when `b` is `box_fconst`.
#[must_use]
pub fn is_box_fconst(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_FCONST_TAG)
}

/// Equivalent to C++ `boxFVar`.
#[must_use]
pub fn box_fvar(arena: &mut TreeArena, ty: BoxId, name: BoxId, file: BoxId) -> BoxId {
    intern_tag(arena, BOX_FVAR_TAG, &[ty, name, file])
}

/// Returns `(ty, name, file)` when `b` is `box_fvar`.
#[must_use]
pub fn is_box_fvar(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_FVAR_TAG)
}

/// Equivalent to C++ `boxCase`.
#[must_use]
pub fn box_case(arena: &mut TreeArena, rules: BoxId) -> BoxId {
    intern_tag(arena, BOX_CASE_TAG, &[rules])
}

/// Returns `rules` when `b` is `box_case`.
#[must_use]
pub fn is_box_case(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_CASE_TAG)
}

/// Equivalent to C++ `boxPatternVar`.
#[must_use]
pub fn box_pattern_var(arena: &mut TreeArena, ident: BoxId) -> BoxId {
    intern_tag(arena, BOX_PATTERN_VAR_TAG, &[ident])
}

/// Returns wrapped identifier when `b` is `box_pattern_var`.
#[must_use]
pub fn is_box_pattern_var(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_PATTERN_VAR_TAG)
}

/// Equivalent to C++ `boxAbstr`.
#[must_use]
pub fn box_abstr(arena: &mut TreeArena, arg: BoxId, body: BoxId) -> BoxId {
    BoxBuilder::new(arena).abstr(arg, body)
}

/// Returns `(arg, body)` when `b` is `box_abstr`.
#[must_use]
pub fn is_box_abstr(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match match_box(arena, b) {
        BoxMatch::Abstr(a, c) => Some((a, c)),
        _ => None,
    }
}

/// Equivalent to C++ `boxModulation`.
#[must_use]
pub fn box_modulation(arena: &mut TreeArena, arg: BoxId, body: BoxId) -> BoxId {
    BoxBuilder::new(arena).modulation(arg, body)
}

/// Returns `(arg, body)` when `b` is `box_modulation`.
#[must_use]
pub fn is_box_modulation(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match match_box(arena, b) {
        BoxMatch::Modulation(a, c) => Some((a, c)),
        _ => None,
    }
}

/// Equivalent to C++ `buildBoxAbstr(largs, body)` using parser-built arg list.
///
/// This preserves C++ nesting order by consuming list tail first.
#[must_use]
pub fn build_box_abstr(arena: &mut TreeArena, args: BoxId, body: BoxId) -> BoxId {
    BoxBuilder::new(arena).build_abstr(args, body)
}

/// Equivalent to C++ `buildBoxModulation(largs, body)` using parser-built arg list.
///
/// This preserves C++ nesting order by applying each list head to the current body,
/// then recursing on the tail.
#[must_use]
pub fn build_box_modulation(arena: &mut TreeArena, args: BoxId, body: BoxId) -> BoxId {
    BoxBuilder::new(arena).build_modulation(args, body)
}

/// Equivalent to C++ `boxInputs`.
#[must_use]
pub fn box_inputs(arena: &mut TreeArena, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_INPUTS_TAG, &[expr])
}

/// Returns wrapped expression when `b` is `box_inputs`.
#[must_use]
pub fn is_box_inputs(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_INPUTS_TAG)
}

/// Equivalent to C++ `boxOutputs`.
#[must_use]
pub fn box_outputs(arena: &mut TreeArena, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_OUTPUTS_TAG, &[expr])
}

/// Returns wrapped expression when `b` is `box_outputs`.
#[must_use]
pub fn is_box_outputs(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_OUTPUTS_TAG)
}

/// Equivalent to C++ `boxOndemand`.
#[must_use]
pub fn box_ondemand(arena: &mut TreeArena, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_ONDEMAND_TAG, &[expr])
}

/// Returns wrapped expression when `b` is `box_ondemand`.
#[must_use]
pub fn is_box_ondemand(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_ONDEMAND_TAG)
}

/// Equivalent to C++ `boxUpsampling`.
#[must_use]
pub fn box_upsampling(arena: &mut TreeArena, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_UPSAMPLING_TAG, &[expr])
}

/// Returns wrapped expression when `b` is `box_upsampling`.
#[must_use]
pub fn is_box_upsampling(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_UPSAMPLING_TAG)
}

/// Equivalent to C++ `boxDownsampling`.
#[must_use]
pub fn box_downsampling(arena: &mut TreeArena, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_DOWNSAMPLING_TAG, &[expr])
}

/// Returns wrapped expression when `b` is `box_downsampling`.
#[must_use]
pub fn is_box_downsampling(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_DOWNSAMPLING_TAG)
}

/// Equivalent to C++ `boxButton`.
#[must_use]
pub fn box_button(arena: &mut TreeArena, label: BoxId) -> BoxId {
    intern_tag(arena, BOX_BUTTON_TAG, &[label])
}

/// Returns `label` when `b` is `box_button`.
#[must_use]
pub fn is_box_button(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_BUTTON_TAG)
}

/// Equivalent to C++ `boxCheckbox`.
#[must_use]
pub fn box_checkbox(arena: &mut TreeArena, label: BoxId) -> BoxId {
    intern_tag(arena, BOX_CHECKBOX_TAG, &[label])
}

/// Returns `label` when `b` is `box_checkbox`.
#[must_use]
pub fn is_box_checkbox(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_CHECKBOX_TAG)
}

/// Equivalent to C++ `boxVSlider`.
///
/// C++ payload encoding is preserved:
/// `tree(BOXVSLIDER, label, list4(cur,min,max,step))`.
#[must_use]
pub fn box_vslider(
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

/// Returns `(label, cur, min, max, step)` when `b` is `box_vslider`.
#[must_use]
pub fn is_box_vslider(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId, BoxId, BoxId)> {
    match_slider(arena, b, BOX_VSLIDER_TAG)
}

/// Equivalent to C++ `boxHSlider`.
///
/// C++ payload encoding is preserved:
/// `tree(BOXHSLIDER, label, list4(cur,min,max,step))`.
#[must_use]
pub fn box_hslider(
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

/// Returns `(label, cur, min, max, step)` when `b` is `box_hslider`.
#[must_use]
pub fn is_box_hslider(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId, BoxId, BoxId)> {
    match_slider(arena, b, BOX_HSLIDER_TAG)
}

/// Equivalent to C++ `boxNumEntry`.
///
/// C++ payload encoding is preserved:
/// `tree(BOXNUMENTRY, label, list4(cur,min,max,step))`.
#[must_use]
pub fn box_num_entry(
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

/// Returns `(label, cur, min, max, step)` when `b` is `box_num_entry`.
#[must_use]
pub fn is_box_num_entry(
    arena: &TreeArena,
    b: BoxId,
) -> Option<(BoxId, BoxId, BoxId, BoxId, BoxId)> {
    match_slider(arena, b, BOX_NUM_ENTRY_TAG)
}

/// Equivalent to C++ `boxVGroup`.
#[must_use]
pub fn box_vgroup(arena: &mut TreeArena, label: BoxId, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_VGROUP_TAG, &[label, expr])
}

/// Returns `(label, expr)` when `b` is `box_vgroup`.
#[must_use]
pub fn is_box_vgroup(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_VGROUP_TAG)
}

/// Equivalent to C++ `boxHGroup`.
#[must_use]
pub fn box_hgroup(arena: &mut TreeArena, label: BoxId, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_HGROUP_TAG, &[label, expr])
}

/// Returns `(label, expr)` when `b` is `box_hgroup`.
#[must_use]
pub fn is_box_hgroup(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_HGROUP_TAG)
}

/// Equivalent to C++ `boxTGroup`.
#[must_use]
pub fn box_tgroup(arena: &mut TreeArena, label: BoxId, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_TGROUP_TAG, &[label, expr])
}

/// Returns `(label, expr)` when `b` is `box_tgroup`.
#[must_use]
pub fn is_box_tgroup(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_TGROUP_TAG)
}

/// Equivalent to C++ `boxVBargraph`.
#[must_use]
pub fn box_vbargraph(arena: &mut TreeArena, label: BoxId, min: BoxId, max: BoxId) -> BoxId {
    intern_tag(arena, BOX_VBARGRAPH_TAG, &[label, min, max])
}

/// Returns `(label, min, max)` when `b` is `box_vbargraph`.
#[must_use]
pub fn is_box_vbargraph(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_VBARGRAPH_TAG)
}

/// Equivalent to C++ `boxHBargraph`.
#[must_use]
pub fn box_hbargraph(arena: &mut TreeArena, label: BoxId, min: BoxId, max: BoxId) -> BoxId {
    intern_tag(arena, BOX_HBARGRAPH_TAG, &[label, min, max])
}

/// Returns `(label, min, max)` when `b` is `box_hbargraph`.
#[must_use]
pub fn is_box_hbargraph(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_HBARGRAPH_TAG)
}

/// Equivalent to C++ `boxSoundfile`.
#[must_use]
pub fn box_soundfile(arena: &mut TreeArena, label: BoxId, chan: BoxId) -> BoxId {
    intern_tag(arena, BOX_SOUNDFILE_TAG, &[label, chan])
}

/// Returns `(label, chan)` when `b` is `box_soundfile`.
#[must_use]
pub fn is_box_soundfile(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
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
    let tag_id = arena.intern_tag(tag);
    arena.intern(NodeKind::Tag(tag_id), children)
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
        Some(NodeKind::Tag(actual_id)) if arena.tag_name(*actual_id) == Some(tag) => {
            Some(children)
        }
        _ => None,
    }
}

fn match_binary(arena: &TreeArena, b: BoxId, tag: &str) -> Option<(BoxId, BoxId)> {
    let [left, right] = match_tag_arity(arena, b, tag, 2)? else {
        return None;
    };
    Some((*left, *right))
}

fn match_ternary(arena: &TreeArena, b: BoxId, tag: &str) -> Option<(BoxId, BoxId, BoxId)> {
    let [a, b, c] = match_tag_arity(arena, b, tag, 3)? else {
        return None;
    };
    Some((*a, *b, *c))
}

fn match_unary(arena: &TreeArena, b: BoxId, tag: &str) -> Option<BoxId> {
    let [child] = match_tag_arity(arena, b, tag, 1)? else {
        return None;
    };
    Some(*child)
}

fn match_slider(
    arena: &TreeArena,
    b: BoxId,
    tag: &str,
) -> Option<(BoxId, BoxId, BoxId, BoxId, BoxId)> {
    let [label, params] = match_tag_arity(arena, b, tag, 2)? else {
        return None;
    };
    let cur = list_nth(arena, *params, 0)?;
    let min = list_nth(arena, *params, 1)?;
    let max = list_nth(arena, *params, 2)?;
    let step = list_nth(arena, *params, 3)?;
    Some((*label, cur, min, max, step))
}

fn list4(arena: &mut TreeArena, a: BoxId, b: BoxId, c: BoxId, d: BoxId) -> BoxId {
    let nil = arena.nil();
    let l3 = arena.cons(d, nil);
    let l2 = arena.cons(c, l3);
    let l1 = arena.cons(b, l2);
    arena.cons(a, l1)
}

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
        NodeKind::Tag(tag_id) => {
            let name = arena.tag_name(*tag_id).unwrap_or("?");
            write!(out, "{name}(").expect("String write cannot fail");
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
