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
//!
//! # Integer convention
//! - Public box integer constructors/matchers are `i32`-based (`boxInt` parity).
//! - Storage remains `tlib::NodeKind::Int(i64)` internally; boundary conversion
//!   is explicit in this crate.

use std::fmt::Write;

use tlib::{NodeKind, TreeArena, TreeId};

pub const CRATE_NAME: &str = "boxes";

/// Box node identifier in `TreeArena`.
pub type BoxId = TreeId;

const BOX_IDENT_TAG: &str = "BOXIDENT";
const BOX_WIRE_TAG: &str = "BOXWIRE";
const BOX_CUT_TAG: &str = "BOXCUT";
const BOX_SLOT_TAG: &str = "BOXSLOT";
const BOX_SYMBOLIC_TAG: &str = "BOXSYMBOLIC";
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
const BOX_ACOS_TAG: &str = "BOXACOS";
const BOX_ASIN_TAG: &str = "BOXASIN";
const BOX_ATAN_TAG: &str = "BOXATAN";
const BOX_ATAN2_TAG: &str = "BOXATAN2";
const BOX_COS_TAG: &str = "BOXCOS";
const BOX_SIN_TAG: &str = "BOXSIN";
const BOX_TAN_TAG: &str = "BOXTAN";
const BOX_EXP_TAG: &str = "BOXEXP";
const BOX_LOG_TAG: &str = "BOXLOG";
const BOX_LOG10_TAG: &str = "BOXLOG10";
const BOX_SQRT_TAG: &str = "BOXSQRT";
const BOX_ABS_TAG: &str = "BOXABS";
const BOX_FMOD_TAG: &str = "BOXFMOD";
const BOX_REMAINDER_TAG: &str = "BOXREMAINDER";
const BOX_FLOOR_TAG: &str = "BOXFLOOR";
const BOX_CEIL_TAG: &str = "BOXCEIL";
const BOX_RINT_TAG: &str = "BOXRINT";
const BOX_ROUND_TAG: &str = "BOXROUND";
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
const BOX_MODIF_LOCAL_DEF_TAG: &str = "BOXMODIFLOCALDEF";
const BOX_WITH_REC_DEF_TAG: &str = "BOXWITHRECDEF";
const BOX_METADATA_TAG: &str = "BOXMETADATA";
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
const BOX_PATTERN_MATCHER_TAG: &str = "BOXPATMATCHER";
const BOX_CLOSURE_TAG: &str = "BOXCLOSURE";
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
    /// Creates a `BoxBuilder` bound to one mutable `TreeArena`.
    pub fn new(arena: &'a mut TreeArena) -> Self {
        Self { arena }
    }

    #[must_use]
    /// Builds one box node for `ident` and returns its `BoxId`.
    pub fn ident(&mut self, name: &str) -> BoxId {
        node_ident(self.arena, name)
    }

    #[must_use]
    /// Builds one box node for `int` and returns its `BoxId`.
    pub fn int(&mut self, value: i32) -> BoxId {
        node_int(self.arena, value)
    }

    #[must_use]
    /// Builds one box node for `real` and returns its `BoxId`.
    pub fn real(&mut self, value: f64) -> BoxId {
        node_real(self.arena, value)
    }

    #[must_use]
    /// Builds one box node for `wire` and returns its `BoxId`.
    pub fn wire(&mut self) -> BoxId {
        node_wire(self.arena)
    }

    #[must_use]
    /// Builds one box node for `cut` and returns its `BoxId`.
    pub fn cut(&mut self) -> BoxId {
        node_cut(self.arena)
    }

    #[must_use]
    /// Builds one box node for `seq` and returns its `BoxId`.
    pub fn seq(&mut self, left: BoxId, right: BoxId) -> BoxId {
        node_seq(self.arena, left, right)
    }

    #[must_use]
    /// Builds one box node for `par` and returns its `BoxId`.
    pub fn par(&mut self, left: BoxId, right: BoxId) -> BoxId {
        node_par(self.arena, left, right)
    }

    #[must_use]
    /// Builds one box node for `rec` and returns its `BoxId`.
    pub fn rec(&mut self, left: BoxId, right: BoxId) -> BoxId {
        node_rec(self.arena, left, right)
    }

    #[must_use]
    /// Builds one box node for `split` and returns its `BoxId`.
    pub fn split(&mut self, left: BoxId, right: BoxId) -> BoxId {
        node_split(self.arena, left, right)
    }

    #[must_use]
    /// Builds one box node for `merge` and returns its `BoxId`.
    pub fn merge(&mut self, left: BoxId, right: BoxId) -> BoxId {
        node_merge(self.arena, left, right)
    }

    #[must_use]
    /// Builds one box node for `appl` and returns its `BoxId`.
    pub fn appl(&mut self, fun: BoxId, arglist: BoxId) -> BoxId {
        node_appl(self.arena, fun, arglist)
    }

    #[must_use]
    /// Builds one box node for `access` and returns its `BoxId`.
    pub fn access(&mut self, expr: BoxId, ident: BoxId) -> BoxId {
        node_access(self.arena, expr, ident)
    }

    #[must_use]
    /// Builds one box node for `add` and returns its `BoxId`.
    pub fn add(&mut self) -> BoxId {
        node_add(self.arena)
    }

    #[must_use]
    /// Builds one box node for `sub` and returns its `BoxId`.
    pub fn sub(&mut self) -> BoxId {
        node_sub(self.arena)
    }

    #[must_use]
    /// Builds one box node for `mul` and returns its `BoxId`.
    pub fn mul(&mut self) -> BoxId {
        node_mul(self.arena)
    }

    #[must_use]
    /// Builds one box node for `div` and returns its `BoxId`.
    pub fn div(&mut self) -> BoxId {
        node_div(self.arena)
    }

    #[must_use]
    /// Builds one box node for `rem` and returns its `BoxId`.
    pub fn rem(&mut self) -> BoxId {
        node_rem(self.arena)
    }

    #[must_use]
    /// Builds one box node for `and` and returns its `BoxId`.
    pub fn and(&mut self) -> BoxId {
        node_and(self.arena)
    }

    #[must_use]
    /// Builds one box node for `or` and returns its `BoxId`.
    pub fn or(&mut self) -> BoxId {
        node_or(self.arena)
    }

    #[must_use]
    /// Builds one box node for `xor` and returns its `BoxId`.
    pub fn xor(&mut self) -> BoxId {
        node_xor(self.arena)
    }

    #[must_use]
    /// Builds one box node for `lsh` and returns its `BoxId`.
    pub fn lsh(&mut self) -> BoxId {
        node_lsh(self.arena)
    }

    #[must_use]
    /// Builds one box node for `rsh` and returns its `BoxId`.
    pub fn rsh(&mut self) -> BoxId {
        node_rsh(self.arena)
    }

    #[must_use]
    /// Builds one box node for `lt` and returns its `BoxId`.
    pub fn lt(&mut self) -> BoxId {
        node_lt(self.arena)
    }

    #[must_use]
    /// Builds one box node for `le` and returns its `BoxId`.
    pub fn le(&mut self) -> BoxId {
        node_le(self.arena)
    }

    #[must_use]
    /// Builds one box node for `gt` and returns its `BoxId`.
    pub fn gt(&mut self) -> BoxId {
        node_gt(self.arena)
    }

    #[must_use]
    /// Builds one box node for `ge` and returns its `BoxId`.
    pub fn ge(&mut self) -> BoxId {
        node_ge(self.arena)
    }

    #[must_use]
    /// Builds one box node for `eq` and returns its `BoxId`.
    pub fn eq(&mut self) -> BoxId {
        node_eq(self.arena)
    }

    #[must_use]
    /// Builds one box node for `ne` and returns its `BoxId`.
    pub fn ne(&mut self) -> BoxId {
        node_ne(self.arena)
    }

    #[must_use]
    /// Builds one box node for `pow` and returns its `BoxId`.
    pub fn pow(&mut self) -> BoxId {
        node_pow(self.arena)
    }

    #[must_use]
    /// Builds one box node for `acos` and returns its `BoxId`.
    pub fn acos(&mut self) -> BoxId {
        node_acos(self.arena)
    }

    #[must_use]
    /// Builds one box node for `asin` and returns its `BoxId`.
    pub fn asin(&mut self) -> BoxId {
        node_asin(self.arena)
    }

    #[must_use]
    /// Builds one box node for `atan` and returns its `BoxId`.
    pub fn atan(&mut self) -> BoxId {
        node_atan(self.arena)
    }

    #[must_use]
    /// Builds one box node for `atan2` and returns its `BoxId`.
    pub fn atan2(&mut self) -> BoxId {
        node_atan2(self.arena)
    }

    #[must_use]
    /// Builds one box node for `cos` and returns its `BoxId`.
    pub fn cos(&mut self) -> BoxId {
        node_cos(self.arena)
    }

    #[must_use]
    /// Builds one box node for `sin` and returns its `BoxId`.
    pub fn sin(&mut self) -> BoxId {
        node_sin(self.arena)
    }

    #[must_use]
    /// Builds one box node for `tan` and returns its `BoxId`.
    pub fn tan(&mut self) -> BoxId {
        node_tan(self.arena)
    }

    #[must_use]
    /// Builds one box node for `exp` and returns its `BoxId`.
    pub fn exp(&mut self) -> BoxId {
        node_exp(self.arena)
    }

    #[must_use]
    /// Builds one box node for `log` and returns its `BoxId`.
    pub fn log(&mut self) -> BoxId {
        node_log(self.arena)
    }

    #[must_use]
    /// Builds one box node for `log10` and returns its `BoxId`.
    pub fn log10(&mut self) -> BoxId {
        node_log10(self.arena)
    }

    #[must_use]
    /// Builds one box node for `sqrt` and returns its `BoxId`.
    pub fn sqrt(&mut self) -> BoxId {
        node_sqrt(self.arena)
    }

    #[must_use]
    /// Builds one box node for `abs` and returns its `BoxId`.
    pub fn abs(&mut self) -> BoxId {
        node_abs(self.arena)
    }

    #[must_use]
    /// Builds one box node for `fmod` and returns its `BoxId`.
    pub fn fmod(&mut self) -> BoxId {
        node_fmod(self.arena)
    }

    #[must_use]
    /// Builds one box node for `remainder` and returns its `BoxId`.
    pub fn remainder(&mut self) -> BoxId {
        node_remainder(self.arena)
    }

    #[must_use]
    /// Builds one box node for `floor` and returns its `BoxId`.
    pub fn floor(&mut self) -> BoxId {
        node_floor(self.arena)
    }

    #[must_use]
    /// Builds one box node for `ceil` and returns its `BoxId`.
    pub fn ceil(&mut self) -> BoxId {
        node_ceil(self.arena)
    }

    #[must_use]
    /// Builds one box node for `rint` and returns its `BoxId`.
    pub fn rint(&mut self) -> BoxId {
        node_rint(self.arena)
    }

    #[must_use]
    /// Builds one box node for `round` and returns its `BoxId`.
    pub fn round(&mut self) -> BoxId {
        node_round(self.arena)
    }

    #[must_use]
    /// Builds one box node for `delay` and returns its `BoxId`.
    pub fn delay(&mut self) -> BoxId {
        node_delay(self.arena)
    }

    #[must_use]
    /// Builds one box node for `delay1` and returns its `BoxId`.
    pub fn delay1(&mut self) -> BoxId {
        node_delay1(self.arena)
    }

    #[must_use]
    /// Builds one box node for `min` and returns its `BoxId`.
    pub fn min(&mut self) -> BoxId {
        node_min(self.arena)
    }

    #[must_use]
    /// Builds one box node for `max` and returns its `BoxId`.
    pub fn max(&mut self) -> BoxId {
        node_max(self.arena)
    }

    #[must_use]
    /// Builds one box node for `prefix` and returns its `BoxId`.
    pub fn prefix(&mut self) -> BoxId {
        node_prefix(self.arena)
    }

    #[must_use]
    /// Builds one box node for `int_cast` and returns its `BoxId`.
    pub fn int_cast(&mut self) -> BoxId {
        node_int_cast(self.arena)
    }

    #[must_use]
    /// Builds one box node for `float_cast` and returns its `BoxId`.
    pub fn float_cast(&mut self) -> BoxId {
        node_float_cast(self.arena)
    }

    #[must_use]
    /// Builds one box node for `read_only_table` and returns its `BoxId`.
    pub fn read_only_table(&mut self) -> BoxId {
        node_read_only_table(self.arena)
    }

    #[must_use]
    /// Builds one box node for `write_read_table` and returns its `BoxId`.
    pub fn write_read_table(&mut self) -> BoxId {
        node_write_read_table(self.arena)
    }

    #[must_use]
    /// Builds one box node for `select2` and returns its `BoxId`.
    pub fn select2(&mut self) -> BoxId {
        node_select2(self.arena)
    }

    #[must_use]
    /// Builds one box node for `select3` and returns its `BoxId`.
    pub fn select3(&mut self) -> BoxId {
        node_select3(self.arena)
    }

    #[must_use]
    /// Builds one box node for `assert_bounds` and returns its `BoxId`.
    pub fn assert_bounds(&mut self) -> BoxId {
        node_assert_bounds(self.arena)
    }

    #[must_use]
    /// Builds one box node for `lowest` and returns its `BoxId`.
    pub fn lowest(&mut self) -> BoxId {
        node_lowest(self.arena)
    }

    #[must_use]
    /// Builds one box node for `highest` and returns its `BoxId`.
    pub fn highest(&mut self) -> BoxId {
        node_highest(self.arena)
    }

    #[must_use]
    /// Builds one box node for `attach` and returns its `BoxId`.
    pub fn attach(&mut self) -> BoxId {
        node_attach(self.arena)
    }

    #[must_use]
    /// Builds one box node for `enable` and returns its `BoxId`.
    pub fn enable(&mut self) -> BoxId {
        node_enable(self.arena)
    }

    #[must_use]
    /// Builds one box node for `control` and returns its `BoxId`.
    pub fn control(&mut self) -> BoxId {
        node_control(self.arena)
    }

    #[must_use]
    /// Builds one box node for `slot` and returns its `BoxId`.
    pub fn slot(&mut self, id: i32) -> BoxId {
        node_slot(self.arena, id)
    }

    #[must_use]
    /// Builds one box node for `symbolic` and returns its `BoxId`.
    pub fn symbolic(&mut self, slot: BoxId, body: BoxId) -> BoxId {
        node_symbolic(self.arena, slot, body)
    }

    #[must_use]
    /// Builds one box node for `ipar` and returns its `BoxId`.
    pub fn ipar(&mut self, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
        node_ipar(self.arena, index, count, body)
    }

    #[must_use]
    /// Builds one box node for `iseq` and returns its `BoxId`.
    pub fn iseq(&mut self, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
        node_iseq(self.arena, index, count, body)
    }

    #[must_use]
    /// Builds one box node for `isum` and returns its `BoxId`.
    pub fn isum(&mut self, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
        node_isum(self.arena, index, count, body)
    }

    #[must_use]
    /// Builds one box node for `iprod` and returns its `BoxId`.
    pub fn iprod(&mut self, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
        node_iprod(self.arena, index, count, body)
    }

    #[must_use]
    /// Builds one box node for `with_local_def` and returns its `BoxId`.
    pub fn with_local_def(&mut self, body: BoxId, ldef: BoxId) -> BoxId {
        node_with_local_def(self.arena, body, ldef)
    }

    #[must_use]
    /// Builds one box node for `modif_local_def` and returns its `BoxId`.
    pub fn modif_local_def(&mut self, body: BoxId, ldef: BoxId) -> BoxId {
        node_modif_local_def(self.arena, body, ldef)
    }

    #[must_use]
    /// Builds one box node for `with_rec_def` and returns its `BoxId`.
    pub fn with_rec_def(&mut self, body: BoxId, ldef: BoxId, ldef2: BoxId) -> BoxId {
        node_with_rec_def(self.arena, body, ldef, ldef2)
    }

    #[must_use]
    /// Builds one box node for `metadata` and returns its `BoxId`.
    ///
    /// Source provenance (C++):
    /// - `compiler/boxes/boxes.cpp`
    /// - `boxMetadata`
    /// - `isBoxMetadata`
    ///
    /// Mapping status: `1:1` semantics.
    ///
    /// `mdlist` keeps the C++ pair encoding used by the parser/evaluator stack:
    /// `cons(key, value)`.
    pub fn metadata(&mut self, expr: BoxId, mdlist: BoxId) -> BoxId {
        node_metadata(self.arena, expr, mdlist)
    }

    #[must_use]
    /// Builds one box node for `environment` and returns its `BoxId`.
    pub fn environment(&mut self) -> BoxId {
        node_environment(self.arena)
    }

    #[must_use]
    /// Builds one box node for `component` and returns its `BoxId`.
    pub fn component(&mut self, filename: BoxId) -> BoxId {
        node_component(self.arena, filename)
    }

    #[must_use]
    /// Builds one box node for `library` and returns its `BoxId`.
    pub fn library(&mut self, filename: BoxId) -> BoxId {
        node_library(self.arena, filename)
    }

    #[must_use]
    /// Builds one box node for `waveform` and returns its `BoxId`.
    pub fn waveform(&mut self, values: &[BoxId]) -> BoxId {
        node_waveform(self.arena, values)
    }

    #[must_use]
    /// Builds one box node for `route` and returns its `BoxId`.
    pub fn route(&mut self, n: BoxId, m: BoxId, route_spec: BoxId) -> BoxId {
        node_route(self.arena, n, m, route_spec)
    }

    #[must_use]
    /// Builds one box node for `ffunction` and returns its `BoxId`.
    pub fn ffunction(&mut self, signature: BoxId, incfile: BoxId, libfile: BoxId) -> BoxId {
        ffunction(self.arena, signature, incfile, libfile)
    }

    #[must_use]
    /// Builds one box node for `ffun` and returns its `BoxId`.
    pub fn ffun(&mut self, ff: BoxId) -> BoxId {
        node_ffun(self.arena, ff)
    }

    #[must_use]
    /// Builds one box node for `fconst` and returns its `BoxId`.
    pub fn fconst(&mut self, ty: BoxId, name: BoxId, file: BoxId) -> BoxId {
        node_fconst(self.arena, ty, name, file)
    }

    #[must_use]
    /// Builds one box node for `fvar` and returns its `BoxId`.
    pub fn fvar(&mut self, ty: BoxId, name: BoxId, file: BoxId) -> BoxId {
        node_fvar(self.arena, ty, name, file)
    }

    #[must_use]
    /// Builds one box node for `case` and returns its `BoxId`.
    pub fn case(&mut self, rules: BoxId) -> BoxId {
        node_case(self.arena, rules)
    }

    #[must_use]
    /// Builds a `boxPatternMatcher` node — a handle to a partially-applied PM in
    /// the evaluator's side-table.
    ///
    /// `key` is a `boxInt(index)` referencing the evaluator's PM store.
    pub fn pattern_matcher(&mut self, key: BoxId) -> BoxId {
        node_pattern_matcher(self.arena, key)
    }

    /// Builds a `boxClosure` node — a handle to a closure in the evaluator's
    /// side-table.
    ///
    /// `key` is a `boxInt(index)` referencing the evaluator's closure store.
    pub fn closure_node(&mut self, key: BoxId) -> BoxId {
        node_closure(self.arena, key)
    }

    #[must_use]
    /// Builds one box node for `pattern_var` and returns its `BoxId`.
    pub fn pattern_var(&mut self, ident: BoxId) -> BoxId {
        node_pattern_var(self.arena, ident)
    }

    #[must_use]
    /// Builds one box node for `abstr` and returns its `BoxId`.
    pub fn abstr(&mut self, arg: BoxId, body: BoxId) -> BoxId {
        node_abstr(self.arena, arg, body)
    }

    #[must_use]
    /// Builds one box node for `modulation` and returns its `BoxId`.
    pub fn modulation(&mut self, arg: BoxId, body: BoxId) -> BoxId {
        node_modulation(self.arena, arg, body)
    }

    #[must_use]
    /// Builds one box node for `build_abstr` and returns its `BoxId`.
    pub fn build_abstr(&mut self, args: BoxId, body: BoxId) -> BoxId {
        build_box_abstr(self.arena, args, body)
    }

    #[must_use]
    /// Builds one box node for `build_modulation` and returns its `BoxId`.
    pub fn build_modulation(&mut self, args: BoxId, body: BoxId) -> BoxId {
        build_box_modulation(self.arena, args, body)
    }

    #[must_use]
    /// Builds one box node for `inputs` and returns its `BoxId`.
    pub fn inputs(&mut self, expr: BoxId) -> BoxId {
        node_inputs(self.arena, expr)
    }

    #[must_use]
    /// Builds one box node for `outputs` and returns its `BoxId`.
    pub fn outputs(&mut self, expr: BoxId) -> BoxId {
        node_outputs(self.arena, expr)
    }

    #[must_use]
    /// Builds one box node for `ondemand` and returns its `BoxId`.
    pub fn ondemand(&mut self, expr: BoxId) -> BoxId {
        node_ondemand(self.arena, expr)
    }

    #[must_use]
    /// Builds one box node for `upsampling` and returns its `BoxId`.
    pub fn upsampling(&mut self, expr: BoxId) -> BoxId {
        node_upsampling(self.arena, expr)
    }

    #[must_use]
    /// Builds one box node for `downsampling` and returns its `BoxId`.
    pub fn downsampling(&mut self, expr: BoxId) -> BoxId {
        node_downsampling(self.arena, expr)
    }

    #[must_use]
    /// Builds one box node for `button` and returns its `BoxId`.
    pub fn button(&mut self, label: BoxId) -> BoxId {
        node_button(self.arena, label)
    }

    #[must_use]
    /// Builds one box node for `checkbox` and returns its `BoxId`.
    pub fn checkbox(&mut self, label: BoxId) -> BoxId {
        node_checkbox(self.arena, label)
    }

    #[must_use]
    /// Builds one box node for `vslider` and returns its `BoxId`.
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
    /// Builds one box node for `hslider` and returns its `BoxId`.
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
    /// Builds one box node for `num_entry` and returns its `BoxId`.
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
    /// Builds one box node for `vgroup` and returns its `BoxId`.
    pub fn vgroup(&mut self, label: BoxId, expr: BoxId) -> BoxId {
        node_vgroup(self.arena, label, expr)
    }

    #[must_use]
    /// Builds one box node for `hgroup` and returns its `BoxId`.
    pub fn hgroup(&mut self, label: BoxId, expr: BoxId) -> BoxId {
        node_hgroup(self.arena, label, expr)
    }

    #[must_use]
    /// Builds one box node for `tgroup` and returns its `BoxId`.
    pub fn tgroup(&mut self, label: BoxId, expr: BoxId) -> BoxId {
        node_tgroup(self.arena, label, expr)
    }

    #[must_use]
    /// Builds one box node for `vbargraph` and returns its `BoxId`.
    pub fn vbargraph(&mut self, label: BoxId, min: BoxId, max: BoxId) -> BoxId {
        node_vbargraph(self.arena, label, min, max)
    }

    #[must_use]
    /// Builds one box node for `hbargraph` and returns its `BoxId`.
    pub fn hbargraph(&mut self, label: BoxId, min: BoxId, max: BoxId) -> BoxId {
        node_hbargraph(self.arena, label, min, max)
    }

    #[must_use]
    /// Builds one box node for `soundfile` and returns its `BoxId`.
    pub fn soundfile(&mut self, label: BoxId, chan: BoxId) -> BoxId {
        node_soundfile(self.arena, label, chan)
    }
}

/// Canonical structural view returned by [`match_box`].
///
/// This enum is the box-layer counterpart of the signal/FIR match enums used in
/// later phases: it decodes the raw tree-encoded representation into one
/// stable shape vocabulary that callers can pattern-match on without depending
/// on tag strings or child ordering.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BoxMatch<'a> {
    Unknown,
    Ident(&'a str),
    Int(i32),
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
    Acos,
    Asin,
    Atan,
    Atan2,
    Cos,
    Sin,
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
    Slot(i32),
    Symbolic(BoxId, BoxId),
    IPar(BoxId, BoxId, BoxId),
    ISeq(BoxId, BoxId, BoxId),
    ISum(BoxId, BoxId, BoxId),
    IProd(BoxId, BoxId, BoxId),
    WithLocalDef(BoxId, BoxId),
    ModifLocalDef(BoxId, BoxId),
    WithRecDef(BoxId, BoxId, BoxId),
    Metadata(BoxId, BoxId),
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
    /// Partially-applied pattern matcher stored in an evaluator side-table.
    ///
    /// The single child is a `boxInt(key)` indexing into the evaluator's PM store.
    /// This node exists so that `force_value_to_box` can return a `TreeId` for a
    /// partially-applied `case` expression without re-entering the evaluator.
    ///
    /// # C++ equivalent
    /// `boxPatternMatcher(Automaton*, int state, Tree env, Tree orig, Tree revParList)`
    /// — C++ stores all PM state inline in the tree; Rust keeps it in a side-table.
    PatternMatcher(BoxId),
    /// Closure stored in an evaluator side-table.
    ///
    /// The single child is a `boxInt(key)` indexing into the evaluator's closure store.
    /// This node exists so that `force_value_to_box` can return a `TreeId` for a
    /// closure (abstraction + captured environment) without losing the environment.
    ///
    /// # C++ equivalent
    /// `closure(expr, genv, visited, lenv)` — C++ stores closures inline in the
    /// tree; Rust keeps them in a side-table.
    Closure(BoxId),
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

/// Decodes one `BoxId` into canonical [`BoxMatch`] shape.
///
/// Performance note:
/// - The current hot path uses arity-first dispatch (`children.len()`) then tag matching.
/// - A tag-id-only (`u32`) dispatch prototype was benchmarked on this branch and did not
///   improve end-to-end throughput under `match_box_bench` workloads, so this version keeps
///   the best measured implementation for now.
#[must_use]
pub fn match_box<'a>(arena: &'a TreeArena, b: BoxId) -> BoxMatch<'a> {
    let Some(node) = arena.node(b) else {
        return BoxMatch::Unknown;
    };
    match &node.kind {
        NodeKind::Int(v) => match i32::try_from(*v) {
            Ok(v) => BoxMatch::Int(v),
            Err(_) => BoxMatch::Unknown,
        },
        NodeKind::FloatBits(bits) => BoxMatch::Real(f64::from_bits(*bits)),
        NodeKind::Tag(tag) => {
            let tag = arena.tag_name(*tag).unwrap_or("");
            let children = node.children.as_slice();
            match children.len() {
                0 => match tag {
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
                    BOX_ACOS_TAG => BoxMatch::Acos,
                    BOX_ASIN_TAG => BoxMatch::Asin,
                    BOX_ATAN_TAG => BoxMatch::Atan,
                    BOX_ATAN2_TAG => BoxMatch::Atan2,
                    BOX_COS_TAG => BoxMatch::Cos,
                    BOX_SIN_TAG => BoxMatch::Sin,
                    BOX_TAN_TAG => BoxMatch::Tan,
                    BOX_EXP_TAG => BoxMatch::Exp,
                    BOX_LOG_TAG => BoxMatch::Log,
                    BOX_LOG10_TAG => BoxMatch::Log10,
                    BOX_SQRT_TAG => BoxMatch::Sqrt,
                    BOX_ABS_TAG => BoxMatch::Abs,
                    BOX_FMOD_TAG => BoxMatch::Fmod,
                    BOX_REMAINDER_TAG => BoxMatch::Remainder,
                    BOX_FLOOR_TAG => BoxMatch::Floor,
                    BOX_CEIL_TAG => BoxMatch::Ceil,
                    BOX_RINT_TAG => BoxMatch::Rint,
                    BOX_ROUND_TAG => BoxMatch::Round,
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
                    match tag {
                        BOX_IDENT_TAG => match arena.kind(c0) {
                            Some(NodeKind::Symbol(name)) => BoxMatch::Ident(name.as_ref()),
                            _ => BoxMatch::Unknown,
                        },
                        BOX_SLOT_TAG => match arena.kind(c0) {
                            Some(NodeKind::Int(v)) => match i32::try_from(*v) {
                                Ok(v) => BoxMatch::Slot(v),
                                Err(_) => BoxMatch::Unknown,
                            },
                            _ => BoxMatch::Unknown,
                        },
                        BOX_COMPONENT_TAG => BoxMatch::Component(c0),
                        BOX_LIBRARY_TAG => BoxMatch::Library(c0),
                        BOX_WAVEFORM_TAG => BoxMatch::Waveform(c0),
                        BOX_FFUN_TAG => BoxMatch::FFun(c0),
                        BOX_CASE_TAG => BoxMatch::Case(c0),
                        BOX_PATTERN_MATCHER_TAG => BoxMatch::PatternMatcher(c0),
                        BOX_CLOSURE_TAG => BoxMatch::Closure(c0),
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
                    match tag {
                        BOX_SEQ_TAG => BoxMatch::Seq(c0, c1),
                        BOX_PAR_TAG => BoxMatch::Par(c0, c1),
                        BOX_REC_TAG => BoxMatch::Rec(c0, c1),
                        BOX_SPLIT_TAG => BoxMatch::Split(c0, c1),
                        BOX_MERGE_TAG => BoxMatch::Merge(c0, c1),
                        BOX_APPL_TAG => BoxMatch::Appl(c0, c1),
                        BOX_ACCESS_TAG => BoxMatch::Access(c0, c1),
                        BOX_SYMBOLIC_TAG => BoxMatch::Symbolic(c0, c1),
                        BOX_WITH_LOCAL_DEF_TAG => BoxMatch::WithLocalDef(c0, c1),
                        BOX_MODIF_LOCAL_DEF_TAG => BoxMatch::ModifLocalDef(c0, c1),
                        BOX_METADATA_TAG => BoxMatch::Metadata(c0, c1),
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
                    match tag {
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

/// Equivalent to C++ `boxSlot`.
#[must_use]
fn node_slot(arena: &mut TreeArena, id: i32) -> BoxId {
    let raw = arena.int(i64::from(id));
    intern_tag(arena, BOX_SLOT_TAG, &[raw])
}

/// Equivalent to C++ `boxSymbolic`.
#[must_use]
fn node_symbolic(arena: &mut TreeArena, slot: BoxId, body: BoxId) -> BoxId {
    intern_tag(arena, BOX_SYMBOLIC_TAG, &[slot, body])
}

/// Equivalent to C++ `boxInt`.
#[must_use]
fn node_int(arena: &mut TreeArena, value: i32) -> BoxId {
    arena.int(i64::from(value))
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

/// Equivalent to C++ `boxAccess`.
#[must_use]
fn node_access(arena: &mut TreeArena, expr: BoxId, ident: BoxId) -> BoxId {
    intern_tag(arena, BOX_ACCESS_TAG, &[expr, ident])
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

/// Equivalent to C++ `gAcosPrim->box()`.
#[must_use]
fn node_acos(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_ACOS_TAG, &[])
}

/// Equivalent to C++ `gAsinPrim->box()`.
#[must_use]
fn node_asin(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_ASIN_TAG, &[])
}

/// Equivalent to C++ `gAtanPrim->box()`.
#[must_use]
fn node_atan(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_ATAN_TAG, &[])
}

/// Equivalent to C++ `gAtan2Prim->box()`.
#[must_use]
fn node_atan2(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_ATAN2_TAG, &[])
}

/// Equivalent to C++ `gCosPrim->box()`.
#[must_use]
fn node_cos(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_COS_TAG, &[])
}

/// Equivalent to C++ `gSinPrim->box()`.
#[must_use]
fn node_sin(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_SIN_TAG, &[])
}

/// Equivalent to C++ `gTanPrim->box()`.
#[must_use]
fn node_tan(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_TAN_TAG, &[])
}

/// Equivalent to C++ `gExpPrim->box()`.
#[must_use]
fn node_exp(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_EXP_TAG, &[])
}

/// Equivalent to C++ `gLogPrim->box()`.
#[must_use]
fn node_log(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_LOG_TAG, &[])
}

/// Equivalent to C++ `gLog10Prim->box()`.
#[must_use]
fn node_log10(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_LOG10_TAG, &[])
}

/// Equivalent to C++ `gSqrtPrim->box()`.
#[must_use]
fn node_sqrt(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_SQRT_TAG, &[])
}

/// Equivalent to C++ `gAbsPrim->box()`.
#[must_use]
fn node_abs(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_ABS_TAG, &[])
}

/// Equivalent to C++ `gFmodPrim->box()`.
#[must_use]
fn node_fmod(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_FMOD_TAG, &[])
}

/// Equivalent to C++ `gRemainderPrim->box()`.
#[must_use]
fn node_remainder(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_REMAINDER_TAG, &[])
}

/// Equivalent to C++ `gFloorPrim->box()`.
#[must_use]
fn node_floor(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_FLOOR_TAG, &[])
}

/// Equivalent to C++ `gCeilPrim->box()`.
#[must_use]
fn node_ceil(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_CEIL_TAG, &[])
}

/// Equivalent to C++ `gRintPrim->box()`.
#[must_use]
fn node_rint(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_RINT_TAG, &[])
}

/// Equivalent to C++ `gRoundPrim->box()`.
#[must_use]
fn node_round(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_ROUND_TAG, &[])
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

/// Equivalent to C++ `boxIPar`.
#[must_use]
fn node_ipar(arena: &mut TreeArena, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
    intern_tag(arena, BOX_IPAR_TAG, &[index, count, body])
}

/// Equivalent to C++ `boxISeq`.
#[must_use]
fn node_iseq(arena: &mut TreeArena, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
    intern_tag(arena, BOX_ISEQ_TAG, &[index, count, body])
}

/// Equivalent to C++ `boxISum`.
#[must_use]
fn node_isum(arena: &mut TreeArena, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
    intern_tag(arena, BOX_ISUM_TAG, &[index, count, body])
}

/// Equivalent to C++ `boxIProd`.
#[must_use]
fn node_iprod(arena: &mut TreeArena, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
    intern_tag(arena, BOX_IPROD_TAG, &[index, count, body])
}

/// Equivalent to C++ `boxWithLocalDef`.
#[must_use]
fn node_with_local_def(arena: &mut TreeArena, body: BoxId, ldef: BoxId) -> BoxId {
    intern_tag(arena, BOX_WITH_LOCAL_DEF_TAG, &[body, ldef])
}

/// Equivalent to C++ `boxModifLocalDef`.
///
/// Source provenance (C++):
/// - `compiler/boxes/boxes.cpp`
/// - `boxModifLocalDef`
/// - `isBoxModifLocalDef`
#[must_use]
fn node_modif_local_def(arena: &mut TreeArena, body: BoxId, ldef: BoxId) -> BoxId {
    intern_tag(arena, BOX_MODIF_LOCAL_DEF_TAG, &[body, ldef])
}

/// Equivalent to C++ `boxWithRecDef`.
///
/// Source provenance (C++):
/// - `compiler/boxes/boxes.cpp`
/// - `boxWithRecDef`
/// - `buildRecursiveBodyDef`
/// - `makeRecProjectionsList`
///
/// Mapping status: `adapted` representation, `1:1` semantics.
///
/// Rust still exposes [`BoxMatch::WithRecDef`] for compatibility with manually
/// constructed trees, but the builder now performs the same eager lowering as
/// C++: `letrec` is expanded immediately into a `with_local_def(...)`
/// containing one synthetic `LETRECBODY = rec(...)` definition followed by one
/// projection definition per recursive name.
///
/// This is the production-path parity point for `letrec`:
/// parser code calls this builder, so downstream phases should no longer see
/// `BOXWITHRECDEF` for normal source programs.
#[must_use]
fn node_with_rec_def(arena: &mut TreeArena, body: BoxId, ldef: BoxId, ldef2: BoxId) -> BoxId {
    box_with_rec_def_expanded(arena, body, ldef, ldef2)
}

/// Eagerly lowers parser-form `letrec` definitions to the same `with`-based
/// structure as C++ `boxWithRecDef(...)`.
///
/// Rust parser definitions arrive here in normalized parser shape
/// `cons(name, cons(args, expr))`, after `parser::ParseState::format_definitions`.
/// This keeps parser/boxes parity with C++ while allowing [`BoxMatch::WithRecDef`]
/// to remain decodable for manually constructed legacy trees.
///
/// Important limitation/invariant:
/// this helper expects parser-normalized definition lists, not arbitrary
/// internal box lists. In particular, each definition cell must follow the
/// parser contract `cons(name, cons(args, expr))`.
fn box_with_rec_def_expanded(
    arena: &mut TreeArena,
    body: BoxId,
    ldef: BoxId,
    ldef2: BoxId,
) -> BoxId {
    let names = def2names(arena, ldef);
    let exprs = def2exp(arena, ldef);
    let n = list_len(arena, ldef);
    let recursive_body_def = make_recursive_body_def(arena, n, names, exprs, ldef2);
    let projections = make_rec_projections_list(arena, n, 0, names, arena.nil());
    let defs = arena.cons(recursive_body_def, projections);
    node_with_local_def(arena, body, defs)
}

fn list_len(arena: &TreeArena, mut list: BoxId) -> usize {
    let mut n = 0usize;
    while !arena.is_nil(list) {
        n += 1;
        list = arena
            .tl(list)
            .expect("definition list should be a well-formed cons/nil list");
    }
    n
}

fn def2names(arena: &mut TreeArena, ldef: BoxId) -> BoxId {
    if arena.is_nil(ldef) {
        arena.nil()
    } else {
        let def = arena.hd(ldef).expect("definition list head");
        let name = arena.hd(def).expect("definition name");
        let rest = arena.tl(ldef).expect("definition list tail");
        let tail = def2names(arena, rest);
        arena.cons(name, tail)
    }
}

fn def2exp(arena: &mut TreeArena, ldef: BoxId) -> BoxId {
    if arena.is_nil(ldef) {
        arena.nil()
    } else {
        let def = arena.hd(ldef).expect("definition list head");
        let payload = arena.tl(def).expect("definition payload");
        let args = arena.hd(payload).expect("definition args");
        let body = arena.tl(payload).expect("definition body");
        // Rust parser definitions are normalized as `cons(name, cons(args, expr))`.
        // `boxWithRecDef` is called after `format_definitions`, so `args` is
        // usually `nil` already. The fallback abstraction rebuild keeps the
        // helper robust for manually constructed parser-shape nodes.
        let expr = if arena.is_nil(args) {
            body
        } else {
            build_box_abstr(arena, args, body)
        };
        let rest = arena.tl(ldef).expect("definition list tail");
        let tail = def2exp(arena, rest);
        arena.cons(expr, tail)
    }
}

fn make_bus(arena: &mut TreeArena, n: usize) -> BoxId {
    if n <= 1 {
        node_wire(arena)
    } else {
        let left = node_wire(arena);
        let right = make_bus(arena, n - 1);
        node_par(arena, left, right)
    }
}

fn make_par_list(arena: &mut TreeArena, lexp: BoxId) -> BoxId {
    let l2 = arena.tl(lexp).expect("expression list tail");
    if arena.is_nil(l2) {
        arena.hd(lexp).expect("expression list head")
    } else {
        let head = arena.hd(lexp).expect("expression list head");
        let tail = make_par_list(arena, l2);
        node_par(arena, head, tail)
    }
}

fn make_box_abstr(arena: &mut TreeArena, largs: BoxId, body: BoxId) -> BoxId {
    if arena.is_nil(largs) {
        body
    } else {
        let arg = arena.hd(largs).expect("abstraction arg");
        let tail = arena.tl(largs).expect("abstraction arg tail");
        let nested = make_box_abstr(arena, tail, body);
        node_abstr(arena, arg, nested)
    }
}

fn make_selector(arena: &mut TreeArena, n: usize, i: i32) -> BoxId {
    let op = if i == 0 {
        node_wire(arena)
    } else {
        node_cut(arena)
    };
    if n <= 1 {
        op
    } else {
        let tail = make_selector(arena, n - 1, i - 1);
        node_par(arena, op, tail)
    }
}

fn make_rec_projections_list(
    arena: &mut TreeArena,
    n: usize,
    i: usize,
    lnames: BoxId,
    ldef: BoxId,
) -> BoxId {
    if i == n {
        ldef
    } else {
        let letrecbody = node_ident(arena, "LETRECBODY");
        let selector = make_selector(arena, n, i as i32);
        let sel = node_seq(arena, letrecbody, selector);
        let name = arena.hd(lnames).expect("recursive projection name");
        let def = make_parser_definition(arena, name, arena.nil(), sel);
        let tail_names = arena.tl(lnames).expect("recursive projection name tail");
        let tail_defs = make_rec_projections_list(arena, n, i + 1, tail_names, ldef);
        arena.cons(def, tail_defs)
    }
}

fn make_recursive_body_def(
    arena: &mut TreeArena,
    n: usize,
    lnames: BoxId,
    lexp: BoxId,
    ldef2: BoxId,
) -> BoxId {
    let body = make_par_list(arena, lexp);
    let body = if arena.is_nil(ldef2) {
        body
    } else {
        node_with_local_def(arena, body, ldef2)
    };
    let abstr = make_box_abstr(arena, lnames, body);
    let bus = make_bus(arena, n);
    let rec = node_rec(arena, abstr, bus);
    let letrecbody = node_ident(arena, "LETRECBODY");
    let nil = arena.nil();
    make_parser_definition(arena, letrecbody, nil, rec)
}

fn make_parser_definition(arena: &mut TreeArena, name: BoxId, args: BoxId, expr: BoxId) -> BoxId {
    let payload = arena.cons(args, expr);
    arena.cons(name, payload)
}

/// Equivalent to C++ `boxMetadata`.
///
/// The metadata payload keeps the tree-pair convention used by the C++ parser:
/// `cons(key, value)`.
#[must_use]
fn node_metadata(arena: &mut TreeArena, expr: BoxId, mdlist: BoxId) -> BoxId {
    intern_tag(arena, BOX_METADATA_TAG, &[expr, mdlist])
}

/// Equivalent to C++ `boxEnvironment`.
#[must_use]
fn node_environment(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_ENVIRONMENT_TAG, &[])
}

/// Equivalent to C++ `boxComponent`.
#[must_use]
fn node_component(arena: &mut TreeArena, filename: BoxId) -> BoxId {
    intern_tag(arena, BOX_COMPONENT_TAG, &[filename])
}

/// Equivalent to C++ `boxLibrary`.
#[must_use]
fn node_library(arena: &mut TreeArena, filename: BoxId) -> BoxId {
    intern_tag(arena, BOX_LIBRARY_TAG, &[filename])
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

/// Equivalent to C++ `boxRoute`.
#[must_use]
fn node_route(arena: &mut TreeArena, n: BoxId, m: BoxId, route_spec: BoxId) -> BoxId {
    intern_tag(arena, BOX_ROUTE_TAG, &[n, m, route_spec])
}

/// Equivalent to C++ `ffunction(signature, incfile, libfile)`.
#[must_use]
fn ffunction(arena: &mut TreeArena, signature: BoxId, incfile: BoxId, libfile: BoxId) -> BoxId {
    intern_tag(arena, FFUN_TAG, &[signature, incfile, libfile])
}

/// Equivalent to C++ `boxFFun`.
#[must_use]
fn node_ffun(arena: &mut TreeArena, ff: BoxId) -> BoxId {
    intern_tag(arena, BOX_FFUN_TAG, &[ff])
}

/// Equivalent to C++ `boxFConst`.
#[must_use]
fn node_fconst(arena: &mut TreeArena, ty: BoxId, name: BoxId, file: BoxId) -> BoxId {
    intern_tag(arena, BOX_FCONST_TAG, &[ty, name, file])
}

/// Equivalent to C++ `boxFVar`.
#[must_use]
fn node_fvar(arena: &mut TreeArena, ty: BoxId, name: BoxId, file: BoxId) -> BoxId {
    intern_tag(arena, BOX_FVAR_TAG, &[ty, name, file])
}

/// Equivalent to C++ `boxCase`.
#[must_use]
fn node_case(arena: &mut TreeArena, rules: BoxId) -> BoxId {
    intern_tag(arena, BOX_CASE_TAG, &[rules])
}

/// Builds a `boxPatternMatcher(key)` node referencing the evaluator PM store.
#[must_use]
fn node_pattern_matcher(arena: &mut TreeArena, key: BoxId) -> BoxId {
    intern_tag(arena, BOX_PATTERN_MATCHER_TAG, &[key])
}

/// Builds a `boxClosure(key)` node referencing the evaluator closure store.
#[must_use]
fn node_closure(arena: &mut TreeArena, key: BoxId) -> BoxId {
    intern_tag(arena, BOX_CLOSURE_TAG, &[key])
}

/// Equivalent to C++ `boxPatternVar`.
#[must_use]
fn node_pattern_var(arena: &mut TreeArena, ident: BoxId) -> BoxId {
    intern_tag(arena, BOX_PATTERN_VAR_TAG, &[ident])
}

/// Equivalent to C++ `boxAbstr`.
#[must_use]
fn node_abstr(arena: &mut TreeArena, arg: BoxId, body: BoxId) -> BoxId {
    intern_tag(arena, BOX_ABSTR_TAG, &[arg, body])
}

/// Equivalent to C++ `boxModulation`.
#[must_use]
fn node_modulation(arena: &mut TreeArena, arg: BoxId, body: BoxId) -> BoxId {
    intern_tag(arena, BOX_MODULATION_TAG, &[arg, body])
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
    let nested = node_abstr(arena, head, body);
    build_box_abstr(arena, tail, nested)
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

/// Equivalent to C++ `boxOutputs`.
#[must_use]
fn node_outputs(arena: &mut TreeArena, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_OUTPUTS_TAG, &[expr])
}

/// Equivalent to C++ `boxOndemand`.
#[must_use]
fn node_ondemand(arena: &mut TreeArena, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_ONDEMAND_TAG, &[expr])
}

/// Equivalent to C++ `boxUpsampling`.
#[must_use]
fn node_upsampling(arena: &mut TreeArena, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_UPSAMPLING_TAG, &[expr])
}

/// Equivalent to C++ `boxDownsampling`.
#[must_use]
fn node_downsampling(arena: &mut TreeArena, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_DOWNSAMPLING_TAG, &[expr])
}

/// Equivalent to C++ `boxButton`.
#[must_use]
fn node_button(arena: &mut TreeArena, label: BoxId) -> BoxId {
    intern_tag(arena, BOX_BUTTON_TAG, &[label])
}

/// Equivalent to C++ `boxCheckbox`.
#[must_use]
fn node_checkbox(arena: &mut TreeArena, label: BoxId) -> BoxId {
    intern_tag(arena, BOX_CHECKBOX_TAG, &[label])
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

/// Equivalent to C++ `boxVGroup`.
#[must_use]
fn node_vgroup(arena: &mut TreeArena, label: BoxId, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_VGROUP_TAG, &[label, expr])
}

/// Equivalent to C++ `boxHGroup`.
#[must_use]
fn node_hgroup(arena: &mut TreeArena, label: BoxId, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_HGROUP_TAG, &[label, expr])
}

/// Equivalent to C++ `boxTGroup`.
#[must_use]
fn node_tgroup(arena: &mut TreeArena, label: BoxId, expr: BoxId) -> BoxId {
    intern_tag(arena, BOX_TGROUP_TAG, &[label, expr])
}

/// Equivalent to C++ `boxVBargraph`.
#[must_use]
fn node_vbargraph(arena: &mut TreeArena, label: BoxId, min: BoxId, max: BoxId) -> BoxId {
    intern_tag(arena, BOX_VBARGRAPH_TAG, &[label, min, max])
}

/// Equivalent to C++ `boxHBargraph`.
#[must_use]
fn node_hbargraph(arena: &mut TreeArena, label: BoxId, min: BoxId, max: BoxId) -> BoxId {
    intern_tag(arena, BOX_HBARGRAPH_TAG, &[label, min, max])
}

/// Equivalent to C++ `boxSoundfile`.
#[must_use]
fn node_soundfile(arena: &mut TreeArena, label: BoxId, chan: BoxId) -> BoxId {
    intern_tag(arena, BOX_SOUNDFILE_TAG, &[label, chan])
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

/// Interns a tagged box node with deterministic child ordering.
///
/// Shared low-level constructor for internal `node_*` helpers. Mirrors the C++
/// `tree(tag, ...)` construction idiom while using arena tag interning and
/// hash-consing (`TreeArena::intern`) for canonicalization.
fn intern_tag(arena: &mut TreeArena, tag: &str, children: &[BoxId]) -> BoxId {
    let tag_id = arena.intern_tag(tag);
    arena.intern(NodeKind::Tag(tag_id), children)
}

/// Builds a canonical 4-element Faust list payload (`cons(a, cons(b, ...)))`.
///
/// Used for slider parameter encoding to preserve C++/Faust list shape exactly.
fn list4(arena: &mut TreeArena, a: BoxId, b: BoxId, c: BoxId, d: BoxId) -> BoxId {
    let nil = arena.nil();
    let l3 = arena.cons(d, nil);
    let l2 = arena.cons(c, l3);
    let l1 = arena.cons(b, l2);
    arena.cons(a, l1)
}

/// Decodes a canonical `list4` slider payload into `(cur, min, max, step)`.
///
/// Returns `None` when `params` is not the expected nested `Cons` shape.
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

/// Recursive structural dumper used by [`dump_box`].
///
/// Emits a deterministic, shape-oriented textual representation suitable for
/// parser differential tests and snapshots. Arena addresses / node ids are not
/// embedded, except for explicit `<invalid:id>` placeholders when a child id
/// cannot be resolved.
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
            match arena.tag_name(*tag) {
                Some(name) => out.push_str(name),
                None => write!(out, "<tag:{tag}>").expect("String write cannot fail"),
            }
            out.push('(');
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
