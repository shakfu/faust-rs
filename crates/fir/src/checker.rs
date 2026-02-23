//! FIR module verifier — Phase 1, Phase 2, and Phase 3.
//!
//! **Phase 1** validates the top-level shape of a `FirMatch::Module` node and
//! populates [`ModuleSymbols`] (struct fields, globals, declared functions).
//!
//! **Phase 2** traverses every function body and performs scope analysis:
//! variable declarations, accesses, loop structures, return statements, and
//! switch statements.
//!
//! **Phase 3** is implemented on top of the same traversal and adds expression
//! type checks (binops, casts, select, function calls, table accesses, and
//! typed control-flow conditions).
//!
//! # Diagnostic codes implemented
//!
//! ## Phase 1 — module structure
//! | Code | Sev | Check |
//! |---|---|---|
//! | FIR-M01 | E | Root node is not a Module |
//! | FIR-M02 | E | `dsp_struct` is not a `Block` of struct field declarations |
//! | FIR-M03 | E | `globals` is not a Block |
//! | FIR-M04 | E | `declarations` is not a Block |
//! | FIR-M05 | E | Non-`DeclareFun` node in declarations block |
//! | FIR-M06 | W | Duplicate function name in declarations |
//! | FIR-M07 | W | Expected DSP API function is not declared |
//! | FIR-S01 | E | Struct field declaration is not `DeclareVar/DeclareTable(kStruct)` |
//! | FIR-S02 | E | Duplicate struct field name in `dsp_struct` |
//! | FIR-S03 | E | Struct field has `Void` type |
//! | FIR-S04 | W | Struct array field has size 0 |
//! | FIR-G01 | E | Globals block contains a non-`DeclareVar`/`DeclareTable`/`DeclareFun` node |
//! | FIR-G02 | E | Global declaration has wrong access type |
//! | FIR-G03 | E | Duplicate global variable name |
//! | FIR-F01 | E | Function type is not `FirType::Fun` |
//! | FIR-F04 | E | Duplicate parameter name in function |
//! | FIR-F05 | W | `compute` return type is not `Void` |
//! | FIR-F06 | W | `compute` parameter count is not 4 |
//! | FIR-F07 | W | Function has no body (prototype/extern declaration) |
//!
//! ## Phase 2 — per-function scope analysis
//! | Code | Sev | Check |
//! |---|---|---|
//! | FIR-SC01 | E | `LoadVar` of undeclared variable |
//! | FIR-SC02 | E | `LoadVar` access type does not match declaration |
//! | FIR-SC03 | W | `LoadVar` of uninitialized stack variable |
//! | FIR-SC04 | E | `StoreVar` to undeclared variable |
//! | FIR-SC05 | E | `StoreVar` access type does not match declaration |
//! | FIR-SC07 | E | `kFunArgs` variable re-declared inside function body |
//! | FIR-SC09 | W | `kStruct` access name not declared in `dsp_struct` |
//! | FIR-SC10 | E | Local `DeclareVar` uses a non-local access class |
//! | FIR-L01  | E | `ForLoop` init is not a `DeclareVar(kLoop)` |
//! | FIR-L02  | E | `ForLoop` loop variable type is not `Int32`/`Int64` |
//! | FIR-L04  | W | `ForLoop`/`SimpleForLoop` body is empty |
//! | FIR-R02  | W | `Return(None)` in a non-`Void` function |
//! | FIR-R03  | W | Statements after a `Return` in a block (dead code) |
//! | FIR-SW02 | E | Duplicate case value in `Switch` |
//! | FIR-SW03 | W | `Switch` has no cases |
//!
//! ## Phase 3 — type checking and typed conditions
//! | Code | Sev | Check |
//! |---|---|---|
//! | FIR-B01 | E | `BinOp` operand type mismatch (except int/bool mixing) |
//! | FIR-B02 | E | `BinOp` operand is not numeric |
//! | FIR-B03 | W | `BinOp` declared result type inconsistent with operands |
//! | FIR-B04 | W | Division by constant zero |
//! | FIR-U01 | E | `Neg` operand is not numeric |
//! | FIR-U02 | W | `Cast` is a no-op |
//! | FIR-U03 | E | `Cast` between non-numeric types |
//! | FIR-U04 | W | `Bitcast` width mismatch |
//! | FIR-C01 | E | `Select2` condition is not int/bool |
//! | FIR-C02 | W | `Select2` branch type mismatch |
//! | FIR-C03 | W | `Select2` result type inconsistent with branches |
//! | FIR-C04 | E | `If` condition is not int/bool |
//! | FIR-FC01 | E | Call to undeclared function |
//! | FIR-FC02 | E | Function call arity mismatch |
//! | FIR-FC03 | W | Function call argument type mismatch |
//! | FIR-FC04 | W | Function return value type mismatch at use site (partial) |
//! | FIR-L03  | E | `WhileLoop` condition is not int/bool |
//! | FIR-SW01 | E | `Switch` condition is not integer |
//! | FIR-R01  | E | `Return` value type mismatch |
//! | FIR-T01  | E | Table index is not integer |
//! | FIR-T02  | E | `StoreTable` value type mismatch |
//! | FIR-T03  | W | `LoadTable` / `StoreTable` on non-table declaration |
//! | FIR-MA01 | W | Unary math op called with wrong arity |
//! | FIR-MA02 | W | Binary math op called with wrong arity |
//! | FIR-MA03 | W | Floating-point math op called with integer-like argument |
//! | FIR-MA04 | W | `abs` / `fabs` int-vs-float distinction warning |
//!
//! ## Deferred / partial
//! - **SC06/SC08** — naturally enforced by scope-stack pop; SC01 fires for any
//!   out-of-scope access regardless of the access class.
//! - **FC04** — implemented partially (discarded non-void call result + call
//!   node declared-type/signature mismatch), but not yet all assignment/use
//!   sites.
//!
//! # Source provenance
//! - Plan: `porting/fir-module-verifier-plan-en.md`, §7
//! - C++ parity: `FIRTypeChecker`, `FIRCodeChecker`, `FIRVarChecker`
//!   in `compiler/generator/fir_to_fir.hh` and `fir_code_checker.hh`

use std::collections::{HashMap, HashSet};

use crate::{
    AccessType, FirBinOp, FirId, FirMatch, FirMathOp, FirStore, FirType, NamedType, match_fir,
};

// ─── Diagnostic types ─────────────────────────────────────────────────────────

/// Severity of a verifier diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// A blocking FIR invariant violation.
    Error,
    /// A non-blocking but suspicious FIR pattern.
    Warning,
}

/// A single diagnostic produced during FIR verification.
#[derive(Clone, Debug, PartialEq)]
pub struct FirDiagnostic {
    /// Diagnostic severity (`Error` or `Warning`).
    pub severity: Severity,
    /// Short code from the diagnostic registry, e.g. `"FIR-M01"`.
    pub code: &'static str,
    /// Human-readable diagnostic message.
    pub message: String,
    /// The [`FirId`] most closely associated with the problem.
    pub node: FirId,
    /// Optional contextual metadata (current function, variable, ...).
    pub context: DiagContext,
}

/// Contextual location of a diagnostic (enclosing function, variable, etc.).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiagContext {
    /// Enclosing function name when the diagnostic originates in a function body.
    pub function_name: Option<String>,
    /// Variable name when the checker can identify a specific variable symbol.
    pub variable_name: Option<String>,
}

// ─── Verify report ─────────────────────────────────────────────────────────────

/// Result of a FIR verification run.
#[derive(Debug, Default)]
pub struct FirVerifyReport {
    /// All diagnostics emitted during the verification run.
    pub diagnostics: Vec<FirDiagnostic>,
}

impl FirVerifyReport {
    /// Returns `true` if any `Error`-severity diagnostics were emitted.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    /// Iterates over error-severity diagnostics.
    #[must_use]
    pub fn errors(&self) -> impl Iterator<Item = &FirDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
    }

    /// Iterates over warning-severity diagnostics.
    #[must_use]
    pub fn warnings(&self) -> impl Iterator<Item = &FirDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
    }

    /// Panics with a formatted listing of all errors if any are present.
    ///
    /// Intended for use in tests and debug-build assertions.
    pub fn assert_ok(&self) {
        if self.has_errors() {
            let msgs = self
                .errors()
                .map(|d| format!("  [{}] {}", d.code, d.message))
                .collect::<Vec<_>>()
                .join("\n");
            panic!("FIR verification failed:\n{msgs}");
        }
    }
}

// ─── Module symbol tables ──────────────────────────────────────────────────────

/// Signature of a function declared in the module.
#[derive(Clone, Debug)]
pub struct FunctionSig {
    /// Ordered list of `(param_name, param_type)` pairs.
    pub params: Vec<(String, FirType)>,
    /// Return type from the function signature.
    pub return_type: FirType,
    /// `true` when the function has no body (prototype / extern declaration).
    pub is_extern: bool,
}

/// Symbol tables populated during Phase 1 (module-level pass).
///
/// These tables feed into Phase 2 (scope analysis) and Phase 3 (type checking).
#[derive(Clone, Debug, Default)]
pub struct ModuleSymbols {
    /// Logical DSP struct name (currently sourced from `Module.name`).
    pub struct_name: Option<String>,
    /// Ordered field types from declarations in the `dsp_struct` block.
    ///
    /// Field names are tracked separately in [`Self::struct_field_names`].
    pub struct_fields: Vec<FirType>,
    /// Set of names declared in the `dsp_struct` block (vars and tables).
    pub struct_field_names: HashSet<String>,
    /// Global/static variables: name → `(AccessType, FirType)`.
    pub globals: HashMap<String, (AccessType, FirType)>,
    /// Names declared as global/static tables (for T03).
    ///
    /// This is tracked separately because table-ness is not encoded in
    /// [`globals`](Self::globals) (which stores only access + element type).
    pub global_tables: HashSet<String>,
    /// Declared functions: name → [`FunctionSig`].
    pub functions: HashMap<String, FunctionSig>,
}

// ─── DSP API registry ──────────────────────────────────────────────────────────

/// Expected DSP API function names checked by M07.
pub const DSP_API_FUNCTIONS: &[&str] = &[
    "classInit",
    "instanceConstants",
    "instanceResetUserInterface",
    "instanceClear",
    "instanceInit",
    "init",
    "buildUserInterface",
    "getSampleRate",
    "compute",
    "metadata",
];

// ─── Phase 2 — scope analysis types ───────────────────────────────────────────

/// Initialization status of a local variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitStatus {
    /// Variable declared but not yet assigned.
    No,
    /// Variable definitely assigned on all paths so far.
    Yes,
    /// Variable assigned on some but not all control-flow paths.
    Maybe,
}

/// Entry stored in a [`ScopeFrame`] for each declared variable.
#[derive(Clone, Debug)]
struct VarEntry {
    access: AccessType,
    typ: FirType,
    init: InitStatus,
    is_table: bool,
}

/// Kind of a scope frame.
#[derive(Clone, Debug)]
enum FrameKind {
    /// Ordinary `Block`.
    Block,
    /// Loop body (ForLoop / SimpleForLoop / IteratorForLoop).
    Loop,
    /// Top-level function frame (holds kFunArgs pre-populated).
    Function,
}

/// One level of the lexical scope stack.
#[derive(Clone, Debug)]
struct ScopeFrame {
    vars: HashMap<String, VarEntry>,
}

/// Lexical scope stack for Phase 2 traversal.
struct ScopeStack {
    frames: Vec<ScopeFrame>,
}

impl ScopeStack {
    fn new() -> Self {
        Self { frames: Vec::new() }
    }

    fn push(&mut self, _kind: FrameKind) {
        self.frames.push(ScopeFrame {
            vars: HashMap::new(),
        });
    }

    fn pop(&mut self) {
        self.frames.pop();
    }

    /// Declare a variable in the current (top) frame.
    fn declare(&mut self, name: String, typ: FirType, access: AccessType, init: InitStatus) {
        self.declare_with_kind(name, typ, access, init, false);
    }

    fn declare_table(&mut self, name: String, elem_type: FirType, access: AccessType) {
        self.declare_with_kind(name, elem_type, access, InitStatus::Yes, true);
    }

    fn declare_with_kind(
        &mut self,
        name: String,
        typ: FirType,
        access: AccessType,
        init: InitStatus,
        is_table: bool,
    ) {
        if let Some(frame) = self.frames.last_mut() {
            frame.vars.insert(
                name,
                VarEntry {
                    access,
                    typ,
                    init,
                    is_table,
                },
            );
        }
    }

    /// Look up a variable from the top of the stack downward.
    /// Returns `(frame_index, &VarEntry)` or `None`.
    fn lookup(&self, name: &str) -> Option<(usize, &VarEntry)> {
        for (fi, frame) in self.frames.iter().enumerate().rev() {
            if let Some(entry) = frame.vars.get(name) {
                return Some((fi, entry));
            }
        }
        None
    }

    /// Mark a variable as initialized (update the topmost frame that holds it).
    fn mark_initialized(&mut self, name: &str) {
        for frame in self.frames.iter_mut().rev() {
            if let Some(entry) = frame.vars.get_mut(name) {
                entry.init = InitStatus::Yes;
                return;
            }
        }
    }

    // ── Snapshot / restore for If-branch merge ──────────────────────────────

    /// Snapshot the current `InitStatus` of every variable in every frame.
    fn snapshot_inits(&self) -> Vec<(usize, String, InitStatus)> {
        self.frames
            .iter()
            .enumerate()
            .flat_map(|(fi, frame)| {
                frame
                    .vars
                    .iter()
                    .map(move |(name, entry)| (fi, name.clone(), entry.init))
            })
            .collect()
    }

    /// Restore `InitStatus` values from a previous snapshot (does not add or
    /// remove variables, only resets existing init flags).
    fn restore_inits(&mut self, snap: &[(usize, String, InitStatus)]) {
        for (fi, name, status) in snap {
            if let Some(frame) = self.frames.get_mut(*fi) {
                if let Some(entry) = frame.vars.get_mut(name.as_str()) {
                    entry.init = *status;
                }
            }
        }
    }

    /// Returns the set of `(frame_idx, var_name)` pairs whose `InitStatus`
    /// changed compared to `snap` (i.e. were newly initialized in the branch).
    fn diff_inits(&self, snap: &[(usize, String, InitStatus)]) -> Vec<(usize, String)> {
        snap.iter()
            .filter_map(|(fi, name, old)| {
                let cur = self
                    .frames
                    .get(*fi)
                    .and_then(|f| f.vars.get(name.as_str()))
                    .map(|e| e.init);
                if cur != Some(*old) {
                    Some((*fi, name.clone()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Set the `InitStatus` of a specific `(frame_idx, var_name)` pair.
    fn set_init(&mut self, fi: usize, name: &str, status: InitStatus) {
        if let Some(frame) = self.frames.get_mut(fi) {
            if let Some(entry) = frame.vars.get_mut(name) {
                entry.init = status;
            }
        }
    }
}

// ─── Entry points ─────────────────────────────────────────────────────────────

/// Verify the FIR module (Phase 1 + Phase 2 + Phase 3) and return the diagnostic report.
///
/// This is the main verifier entry point used by tests, pass assertions, and
/// compiler integration. It validates the module shape, collects top-level
/// symbols, then walks all function bodies for scope and type checks.
#[must_use]
pub fn verify_fir_module(store: &FirStore, module_id: FirId) -> FirVerifyReport {
    let (report, _symbols) = verify_module_structure(store, module_id);
    report
}

/// Like [`verify_fir_module`] but also returns the [`ModuleSymbols`] collected
/// during Phase 1 for targeted function verification or later passes.
///
/// The returned [`FirVerifyReport`] already includes all diagnostics emitted by
/// phase 1, phase 2, and phase 3.
#[must_use]
pub fn verify_module_structure(
    store: &FirStore,
    module_id: FirId,
) -> (FirVerifyReport, ModuleSymbols) {
    let mut ctx = VerifyCtx::new(store, module_id);
    ctx.check_phase1();
    ctx.check_phase2();
    (
        FirVerifyReport {
            diagnostics: ctx.diags,
        },
        ctx.symbols,
    )
}

/// Verify a single function body using pre-collected module symbols.
///
/// This runs the per-function Phase 2 + Phase 3 semantic checks (no module-shape
/// validation).
///
/// If `fun_id` is not a `DeclareFun` node (or its type is not `FirType::Fun`),
/// a diagnostic is emitted in the returned report.
#[must_use]
pub fn verify_fir_function(
    store: &FirStore,
    fun_id: FirId,
    symbols: &ModuleSymbols,
) -> FirVerifyReport {
    let mut ctx = VerifyCtx::new(store, fun_id);
    ctx.symbols = symbols.clone();

    let FirMatch::DeclareFun {
        name,
        typ,
        args,
        body,
        ..
    } = match_fir(store, fun_id)
    else {
        ctx.error("FIR-M05", "node is not a DeclareFun", fun_id);
        return FirVerifyReport {
            diagnostics: ctx.diags,
        };
    };

    let FirType::Fun { ret, .. } = typ else {
        ctx.error(
            "FIR-F01",
            format!("function '{name}' has type that is not FirType::Fun"),
            fun_id,
        );
        return FirVerifyReport {
            diagnostics: ctx.diags,
        };
    };

    if let Some(body_id) = body {
        ctx.enter_function(
            name,
            *ret,
            args.iter().map(|a| (a.name.clone(), a.typ.clone())),
        );
        ctx.check_stmt(body_id);
        ctx.leave_function();
    }

    FirVerifyReport {
        diagnostics: ctx.diags,
    }
}

// ─── Internal context ──────────────────────────────────────────────────────────

struct VerifyCtx<'s> {
    store: &'s FirStore,
    module_id: FirId,
    diags: Vec<FirDiagnostic>,
    symbols: ModuleSymbols,

    // ── Phase 2 per-function state ─────────────────────────────────────────
    /// Name of the function currently being verified.
    current_function: Option<String>,
    /// Return type of the function currently being verified.
    current_return_type: Option<FirType>,
    /// `kFunArgs` parameters of the current function: name → type.
    current_fun_args: HashMap<String, FirType>,
    /// Lexical scope stack for `kStack` / `kLoop` variables.
    scope_stack: ScopeStack,
}

impl<'s> VerifyCtx<'s> {
    fn new(store: &'s FirStore, module_id: FirId) -> Self {
        Self {
            store,
            module_id,
            diags: Vec::new(),
            symbols: ModuleSymbols::default(),
            current_function: None,
            current_return_type: None,
            current_fun_args: HashMap::new(),
            scope_stack: ScopeStack::new(),
        }
    }

    fn emit(
        &mut self,
        severity: Severity,
        code: &'static str,
        message: impl Into<String>,
        node: FirId,
    ) {
        self.diags.push(FirDiagnostic {
            severity,
            code,
            message: message.into(),
            node,
            context: DiagContext {
                function_name: self.current_function.clone(),
                variable_name: None,
            },
        });
    }

    fn error(&mut self, code: &'static str, message: impl Into<String>, node: FirId) {
        self.emit(Severity::Error, code, message, node);
    }

    fn warn(&mut self, code: &'static str, message: impl Into<String>, node: FirId) {
        self.emit(Severity::Warning, code, message, node);
    }

    // =========================================================================
    // Phase 1 — module structure and symbol collection
    // =========================================================================

    fn check_phase1(&mut self) {
        let id = self.module_id;

        // M01: root must decode as Module
        let FirMatch::Module {
            name,
            dsp_struct,
            globals,
            declarations,
        } = match_fir(self.store, id)
        else {
            self.error("FIR-M01", "root node is not a FirMatch::Module", id);
            return;
        };

        // `Module.name` is the DSP class name and is used as the logical struct
        // name in the checker symbols table.
        self.symbols.struct_name = Some(name.clone());

        // M02: validate and register struct fields
        self.check_dsp_struct(dsp_struct);

        // M03: globals must be a Block
        match match_fir(self.store, globals) {
            FirMatch::Block(stmts) => self.check_globals(globals, stmts),
            _ => self.error("FIR-M03", "globals is not a Block", globals),
        }

        // M04: declarations must be a Block
        match match_fir(self.store, declarations) {
            FirMatch::Block(stmts) => self.check_declarations(declarations, stmts, &name),
            _ => self.error("FIR-M04", "declarations is not a Block", declarations),
        }
    }

    // ── dsp_struct ────────────────────────────────────────────────────────────

    fn check_dsp_struct(&mut self, id: FirId) {
        let FirMatch::Block(stmts) = match_fir(self.store, id) else {
            self.error("FIR-M02", "dsp_struct is not a Block", id);
            return;
        };

        let mut seen = HashSet::new();
        let mut field_types = Vec::new();
        for stmt_id in stmts {
            let (field_name, field_type) = match match_fir(self.store, stmt_id) {
                FirMatch::DeclareVar {
                    name, typ, access, ..
                } => {
                    if access != AccessType::Struct {
                        self.error(
                            "FIR-S01",
                            format!(
                                "dsp_struct field '{name}' has access type {access:?}, expected Struct"
                            ),
                            stmt_id,
                        );
                    }
                    (name, typ)
                }
                FirMatch::DeclareTable {
                    name,
                    access,
                    elem_type,
                    ..
                } => {
                    if access != AccessType::Struct {
                        self.error(
                            "FIR-S01",
                            format!(
                                "dsp_struct table '{name}' has access type {access:?}, expected Struct"
                            ),
                            stmt_id,
                        );
                    }
                    (name, elem_type)
                }
                _ => {
                    self.error(
                        "FIR-S01",
                        "dsp_struct contains a node that is not DeclareVar or DeclareTable",
                        stmt_id,
                    );
                    continue;
                }
            };

            if !seen.insert(field_name.clone()) {
                self.error(
                    "FIR-S02",
                    format!("duplicate dsp_struct field name '{field_name}'"),
                    stmt_id,
                );
            }

            match &field_type {
                FirType::Void => {
                    self.error(
                        "FIR-S03",
                        format!("dsp_struct field '{field_name}' has Void type"),
                        stmt_id,
                    );
                }
                FirType::Array(_, 0) => {
                    self.warn(
                        "FIR-S04",
                        format!("dsp_struct array field '{field_name}' has size 0"),
                        stmt_id,
                    );
                }
                _ => {}
            }
            field_types.push(field_type);
        }

        self.symbols.struct_field_names = seen;
        self.symbols.struct_fields = field_types;
    }

    // ── globals ───────────────────────────────────────────────────────────────

    fn check_globals(&mut self, _block_id: FirId, stmts: Vec<FirId>) {
        let mut seen: HashSet<String> = HashSet::new();

        for stmt_id in stmts {
            match match_fir(self.store, stmt_id) {
                FirMatch::DeclareVar {
                    name, typ, access, ..
                } => {
                    if !matches!(access, AccessType::Static | AccessType::Global) {
                        self.error(
                            "FIR-G02",
                            format!(
                                "global variable '{name}' has access type {access:?}, \
                                 expected Static or Global"
                            ),
                            stmt_id,
                        );
                    }
                    if seen.insert(name.clone()) {
                        self.symbols.globals.insert(name, (access, typ));
                    } else {
                        self.error(
                            "FIR-G03",
                            format!("duplicate global variable name '{name}'"),
                            stmt_id,
                        );
                    }
                }
                FirMatch::DeclareTable {
                    name,
                    access,
                    elem_type,
                    ..
                } => {
                    if !matches!(access, AccessType::Static | AccessType::Global) {
                        self.error(
                            "FIR-G02",
                            format!(
                                "global table '{name}' has access type {access:?}, \
                                 expected Static or Global"
                            ),
                            stmt_id,
                        );
                    }
                    if seen.insert(name.clone()) {
                        self.symbols.global_tables.insert(name.clone());
                        self.symbols.globals.insert(name, (access, elem_type));
                    } else {
                        self.error(
                            "FIR-G03",
                            format!("duplicate global variable name '{name}'"),
                            stmt_id,
                        );
                    }
                }
                FirMatch::DeclareFun {
                    name,
                    typ,
                    args,
                    body,
                    ..
                } => {
                    self.register_function_signature(
                        stmt_id, &name, &typ, &args, body, None, false,
                    );
                }
                _ => {
                    self.error(
                        "FIR-G01",
                        "globals block contains a node that is not DeclareVar, DeclareTable, or DeclareFun",
                        stmt_id,
                    );
                }
            }
        }
    }

    // ── declarations ──────────────────────────────────────────────────────────

    fn check_declarations(&mut self, _block_id: FirId, stmts: Vec<FirId>, _module_name: &str) {
        let mut seen: HashSet<String> = HashSet::new();

        for stmt_id in stmts {
            let FirMatch::DeclareFun {
                name,
                typ,
                args,
                body,
                ..
            } = match_fir(self.store, stmt_id)
            else {
                self.error(
                    "FIR-M05",
                    "declarations block contains a non-DeclareFun node",
                    stmt_id,
                );
                continue;
            };

            self.register_function_signature(
                stmt_id,
                &name,
                &typ,
                &args,
                body,
                Some(&mut seen),
                true,
            );
        }

        for &api_fn in DSP_API_FUNCTIONS {
            if !self.symbols.functions.contains_key(api_fn) {
                self.warn(
                    "FIR-M07",
                    format!("expected DSP API function '{api_fn}' is not declared"),
                    self.module_id,
                );
            }
        }
    }

    fn register_function_signature(
        &mut self,
        stmt_id: FirId,
        name: &str,
        typ: &FirType,
        args: &[NamedType],
        body: Option<FirId>,
        seen_names: Option<&mut HashSet<String>>,
        warn_extern: bool,
    ) {
        if let Some(seen) = seen_names {
            if !seen.insert(name.to_string()) {
                self.warn(
                    "FIR-M06",
                    format!("duplicate function name '{name}'"),
                    stmt_id,
                );
            }
        }

        let FirType::Fun {
            args: param_types,
            ret,
        } = typ
        else {
            self.error(
                "FIR-F01",
                format!("function '{name}' has type that is not FirType::Fun"),
                stmt_id,
            );
            return;
        };

        let mut param_names: HashSet<String> = HashSet::new();
        let mut params_list: Vec<(String, FirType)> = Vec::with_capacity(args.len());
        for arg in args {
            if !param_names.insert(arg.name.clone()) {
                self.error(
                    "FIR-F04",
                    format!(
                        "function '{name}' has duplicate parameter name '{}'",
                        arg.name
                    ),
                    stmt_id,
                );
            }
            params_list.push((arg.name.clone(), arg.typ.clone()));
        }

        if name == "compute" {
            if **ret != FirType::Void {
                self.warn(
                    "FIR-F05",
                    format!("'compute' return type should be Void, got {ret:?}"),
                    stmt_id,
                );
            }
            if param_types.len() != 4 {
                self.warn(
                    "FIR-F06",
                    format!(
                        "'compute' should have 4 parameters \
                         (dsp*, count, inputs, outputs), got {}",
                        param_types.len()
                    ),
                    stmt_id,
                );
            }
        }

        let is_extern = body.is_none();
        if is_extern && warn_extern {
            self.warn(
                "FIR-F07",
                format!("function '{name}' has no body (prototype/extern declaration)"),
                stmt_id,
            );
        }

        self.symbols
            .functions
            .entry(name.to_string())
            .or_insert_with(|| FunctionSig {
                params: params_list,
                return_type: *ret.clone(),
                is_extern,
            });
    }

    // =========================================================================
    // Phase 2 — per-function scope analysis
    // =========================================================================

    fn check_phase2(&mut self) {
        // Bail out if Phase 1 found a broken module skeleton.
        let FirMatch::Module { declarations, .. } = match_fir(self.store, self.module_id) else {
            return;
        };
        let FirMatch::Block(stmts) = match_fir(self.store, declarations) else {
            return;
        };

        for stmt_id in stmts {
            if let FirMatch::DeclareFun {
                name,
                typ,
                args,
                body: Some(body_id),
                ..
            } = match_fir(self.store, stmt_id)
            {
                let FirType::Fun { ret, .. } = typ else {
                    continue;
                };
                self.enter_function(
                    name,
                    *ret,
                    args.iter().map(|a| (a.name.clone(), a.typ.clone())),
                );
                self.check_stmt(body_id);
                self.leave_function();
            }
        }
    }

    fn enter_function(
        &mut self,
        name: String,
        ret: FirType,
        args: impl Iterator<Item = (String, FirType)>,
    ) {
        self.current_function = Some(name);
        self.current_return_type = Some(ret);
        self.current_fun_args.clear();
        for (param_name, param_type) in args {
            self.current_fun_args.insert(param_name, param_type);
        }
        self.scope_stack.push(FrameKind::Function);
    }

    fn leave_function(&mut self) {
        self.scope_stack.pop();
        self.current_function = None;
        self.current_return_type = None;
        self.current_fun_args.clear();
    }

    // ── Scope resolution ──────────────────────────────────────────────────────

    /// Resolve a variable name+access to its declared `VarEntry`, if any.
    ///
    /// Returns `None` only when the variable is genuinely undeclared.
    ///
    /// `kStruct` accesses are validated against names declared in the
    /// `dsp_struct` block. The returned type remains a placeholder because
    /// checker phase 3 still relies on the explicit FIR node `typ` for struct
    /// accesses (name→type mapping is not tracked yet).
    fn resolve(&self, name: &str, access: AccessType) -> Option<VarEntry> {
        match access {
            AccessType::Struct => {
                if !self.symbols.struct_field_names.contains(name) {
                    return None;
                }
                Some(VarEntry {
                    access: AccessType::Struct,
                    typ: FirType::Void, // placeholder; type check is Phase 3
                    init: InitStatus::Yes,
                    is_table: false,
                })
            }
            AccessType::Static | AccessType::Global => {
                let (a, t) = self.symbols.globals.get(name)?;
                Some(VarEntry {
                    access: *a,
                    typ: t.clone(),
                    init: InitStatus::Yes,
                    is_table: self.symbols.global_tables.contains(name),
                })
            }
            AccessType::FunArgs => {
                let t = self.current_fun_args.get(name)?;
                Some(VarEntry {
                    access: AccessType::FunArgs,
                    typ: t.clone(),
                    init: InitStatus::Yes,
                    is_table: false,
                })
            }
            AccessType::Stack | AccessType::Loop => {
                if access == AccessType::Loop && self.is_implicit_compute_loop_index(name) {
                    return Some(VarEntry {
                        access: AccessType::Loop,
                        typ: FirType::Int32,
                        init: InitStatus::Yes,
                        is_table: false,
                    });
                }
                let (_, e) = self.scope_stack.lookup(name)?;
                Some(e.clone())
            }
        }
    }

    /// Resolve a variable by name only (ignoring the requested access class).
    ///
    /// Used to distinguish "undeclared" from "declared in another access space"
    /// so SC02/SC05 can be emitted instead of SC01/SC04.
    fn resolve_any_by_name(&self, name: &str) -> Option<VarEntry> {
        if self.is_implicit_compute_loop_index(name) {
            return Some(VarEntry {
                access: AccessType::Loop,
                typ: FirType::Int32,
                init: InitStatus::Yes,
                is_table: false,
            });
        }
        if let Some((_, entry)) = self.scope_stack.lookup(name) {
            return Some(entry.clone());
        }
        if let Some(t) = self.current_fun_args.get(name) {
            return Some(VarEntry {
                access: AccessType::FunArgs,
                typ: t.clone(),
                init: InitStatus::Yes,
                is_table: false,
            });
        }
        if let Some((access, typ)) = self.symbols.globals.get(name) {
            return Some(VarEntry {
                access: *access,
                typ: typ.clone(),
                init: InitStatus::Yes,
                is_table: self.symbols.global_tables.contains(name),
            });
        }
        if self.symbols.struct_field_names.contains(name) {
            return Some(VarEntry {
                access: AccessType::Struct,
                typ: FirType::Void,
                init: InitStatus::Yes,
                is_table: false,
            });
        }
        None
    }

    // ── Phase 3 helpers (type inference / compatibility) ────────────────────

    fn is_implicit_compute_loop_index(&self, name: &str) -> bool {
        self.current_function.as_deref() == Some("compute") && name == "i0"
    }

    fn is_indexable_container_type(&self, typ: &FirType) -> Option<FirType> {
        match typ {
            FirType::Ptr(inner) => Some((**inner).clone()),
            FirType::Array(inner, _) => Some((**inner).clone()),
            FirType::Vector(inner, _) => Some((**inner).clone()),
            _ => None,
        }
    }

    fn infer_value_type(&self, id: FirId) -> Option<FirType> {
        match match_fir(self.store, id) {
            FirMatch::LoadVar { name, access, typ } => {
                // For `kStruct`, `resolve()` intentionally carries a placeholder
                // type because field names are unavailable at the type level.
                // Prefer the explicit FIR node type in that case.
                if access == AccessType::Struct {
                    Some(typ)
                } else {
                    self.resolve(&name, access).map(|e| e.typ).or(Some(typ))
                }
            }
            FirMatch::LoadTable {
                name, access, typ, ..
            } => {
                if access == AccessType::Struct {
                    Some(typ)
                } else {
                    self.resolve(&name, access)
                        .map(|e| {
                            if e.is_table {
                                e.typ
                            } else {
                                self.is_indexable_container_type(&e.typ).unwrap_or(e.typ)
                            }
                        })
                        .or(Some(typ))
                }
            }
            FirMatch::FunCall { name, typ, .. } => self
                .symbols
                .functions
                .get(&name)
                .map(|sig| sig.return_type.clone())
                .or(Some(typ)),
            _ => self.store.value_type(id),
        }
    }

    fn is_integer_type(&self, typ: &FirType) -> bool {
        matches!(typ, FirType::Int32 | FirType::Int64)
    }

    fn is_numeric_type(&self, typ: &FirType) -> bool {
        matches!(
            typ,
            FirType::Int32
                | FirType::Int64
                | FirType::Float32
                | FirType::Float64
                | FirType::FaustFloat
                | FirType::Quad
                | FirType::FixedPoint
                | FirType::Bool
        )
    }

    fn is_float_like_type(&self, typ: &FirType) -> bool {
        matches!(
            typ,
            FirType::Float32 | FirType::Float64 | FirType::FaustFloat | FirType::Quad
        )
    }

    fn is_int_or_bool_type(&self, typ: &FirType) -> bool {
        self.is_integer_type(typ) || *typ == FirType::Bool
    }

    fn same_or_int_bool_mix(&self, lhs: &FirType, rhs: &FirType) -> bool {
        lhs == rhs
            || matches!(
                (lhs, rhs),
                (FirType::Bool, FirType::Int32)
                    | (FirType::Bool, FirType::Int64)
                    | (FirType::Int32, FirType::Bool)
                    | (FirType::Int64, FirType::Bool)
            )
    }

    fn types_compatible(&self, actual: &FirType, expected: &FirType) -> bool {
        actual == expected || (self.is_numeric_type(actual) && self.is_numeric_type(expected))
    }

    fn bit_width(&self, typ: &FirType) -> Option<u32> {
        match typ {
            FirType::Bool => Some(1),
            FirType::Int32 | FirType::Float32 => Some(32),
            FirType::Int64 | FirType::Float64 => Some(64),
            FirType::Quad => Some(128),
            FirType::Ptr(_) | FirType::Obj | FirType::Sound | FirType::UI | FirType::Meta => {
                Some(64)
            }
            _ => None,
        }
    }

    fn promoted_numeric_type(&self, lhs: &FirType, rhs: &FirType) -> Option<FirType> {
        if !self.is_numeric_type(lhs) || !self.is_numeric_type(rhs) {
            return None;
        }
        if lhs == rhs {
            return Some(lhs.clone());
        }
        if self.same_or_int_bool_mix(lhs, rhs) {
            return Some(
                if matches!(lhs, FirType::Int64) || matches!(rhs, FirType::Int64) {
                    FirType::Int64
                } else {
                    FirType::Int32
                },
            );
        }
        let rank = |t: &FirType| -> i32 {
            match t {
                FirType::Quad => 70,
                FirType::Float64 => 60,
                FirType::FaustFloat => 55,
                FirType::Float32 => 50,
                FirType::FixedPoint => 45,
                FirType::Int64 => 20,
                FirType::Int32 => 10,
                FirType::Bool => 0,
                _ => -1,
            }
        };
        let out = if rank(lhs) >= rank(rhs) { lhs } else { rhs };
        Some(out.clone())
    }

    fn expected_binop_result_type(
        &self,
        op: FirBinOp,
        lhs: &FirType,
        rhs: &FirType,
    ) -> Option<FirType> {
        match op {
            FirBinOp::Eq
            | FirBinOp::Ne
            | FirBinOp::Lt
            | FirBinOp::Le
            | FirBinOp::Gt
            | FirBinOp::Ge => Some(FirType::Bool),
            FirBinOp::And | FirBinOp::Or | FirBinOp::Xor => {
                if *lhs == FirType::Bool && *rhs == FirType::Bool {
                    Some(FirType::Bool)
                } else {
                    self.promoted_numeric_type(lhs, rhs)
                }
            }
            _ => self.promoted_numeric_type(lhs, rhs),
        }
    }

    fn const_is_zero(&self, id: FirId) -> bool {
        match match_fir(self.store, id) {
            FirMatch::Int32 { value, .. } => value == 0,
            FirMatch::Int64 { value, .. } => value == 0,
            FirMatch::Float32 { value, .. } => value == 0.0,
            FirMatch::Float64 { value, .. } => value == 0.0,
            FirMatch::Bool { value, .. } => !value,
            _ => false,
        }
    }

    fn check_int_or_bool_condition(
        &mut self,
        id: FirId,
        cond: FirId,
        code: &'static str,
        what: &str,
    ) {
        if let Some(cond_ty) = self.infer_value_type(cond) {
            if !self.is_int_or_bool_type(&cond_ty) {
                self.error(
                    code,
                    format!("{what} condition should be Int32, Int64, or Bool, got {cond_ty:?}"),
                    id,
                );
            }
        }
    }

    fn check_switch_condition_type(&mut self, id: FirId, cond: FirId) {
        if let Some(cond_ty) = self.infer_value_type(cond) {
            if !self.is_integer_type(&cond_ty) {
                self.error(
                    "FIR-SW01",
                    format!("Switch condition should be Int32 or Int64, got {cond_ty:?}"),
                    id,
                );
            }
        }
    }

    fn check_binop_types(
        &mut self,
        id: FirId,
        op: FirBinOp,
        lhs: FirId,
        rhs: FirId,
        declared: &FirType,
    ) {
        let lhs_ty = self.infer_value_type(lhs);
        let rhs_ty = self.infer_value_type(rhs);
        let (Some(lhs_ty), Some(rhs_ty)) = (lhs_ty, rhs_ty) else {
            return;
        };

        if !self.same_or_int_bool_mix(&lhs_ty, &rhs_ty) {
            self.error(
                "FIR-B01",
                format!("BinOp operands have incompatible types: {lhs_ty:?} vs {rhs_ty:?}"),
                id,
            );
        }
        if !self.is_numeric_type(&lhs_ty) || !self.is_numeric_type(&rhs_ty) {
            self.error(
                "FIR-B02",
                format!("BinOp operands must be numeric, got {lhs_ty:?} and {rhs_ty:?}"),
                id,
            );
        }
        if let Some(expected) = self.expected_binop_result_type(op, &lhs_ty, &rhs_ty) {
            if &expected != declared {
                self.warn(
                    "FIR-B03",
                    format!(
                        "BinOp declared result type {declared:?} is inconsistent with operands \
                         ({lhs_ty:?}, {rhs_ty:?}); expected {expected:?}"
                    ),
                    id,
                );
            }
        }
        if op == FirBinOp::Div && self.const_is_zero(rhs) {
            self.warn("FIR-B04", "division by constant zero in BinOp", id);
        }
    }

    fn check_neg_type(&mut self, id: FirId, value: FirId) {
        if let Some(val_ty) = self.infer_value_type(value) {
            if !self.is_numeric_type(&val_ty) {
                self.error(
                    "FIR-U01",
                    format!("Neg operand must be numeric, got {val_ty:?}"),
                    id,
                );
            }
        }
    }

    fn check_cast_type(&mut self, id: FirId, target: &FirType, value: FirId) {
        if let Some(src_ty) = self.infer_value_type(value) {
            if &src_ty == target {
                self.warn("FIR-U02", format!("Cast is a no-op to {target:?}"), id);
            }
            if !self.is_numeric_type(&src_ty) || !self.is_numeric_type(target) {
                self.error(
                    "FIR-U03",
                    format!(
                        "Cast requires numeric source/target types, got {src_ty:?} -> {target:?}"
                    ),
                    id,
                );
            }
        }
    }

    fn check_bitcast_type(&mut self, id: FirId, target: &FirType, value: FirId) {
        if let Some(src_ty) = self.infer_value_type(value) {
            let src_w = self.bit_width(&src_ty);
            let dst_w = self.bit_width(target);
            if let (Some(sw), Some(dw)) = (src_w, dst_w) {
                if sw != dw {
                    self.warn(
                        "FIR-U04",
                        format!("Bitcast width mismatch: {src_ty:?} ({sw}) -> {target:?} ({dw})"),
                        id,
                    );
                }
            }
        }
    }

    fn check_select2_types(
        &mut self,
        id: FirId,
        cond: FirId,
        then_value: FirId,
        else_value: FirId,
        declared: &FirType,
    ) {
        self.check_int_or_bool_condition(id, cond, "FIR-C01", "Select2");
        let then_ty = self.infer_value_type(then_value);
        let else_ty = self.infer_value_type(else_value);
        if let (Some(tt), Some(et)) = (then_ty, else_ty) {
            if tt != et {
                self.warn(
                    "FIR-C02",
                    format!("Select2 branches have different types: {tt:?} vs {et:?}"),
                    id,
                );
            }
            if &tt != declared && &et != declared {
                self.warn(
                    "FIR-C03",
                    format!(
                        "Select2 declared result type {declared:?} does not match branch types \
                         ({tt:?}, {et:?})"
                    ),
                    id,
                );
            }
        }
    }

    fn check_fun_call_types(&mut self, id: FirId, name: &str, args: &[FirId], declared: &FirType) {
        if let Some(sig) = self.symbols.functions.get(name).cloned() {
            if sig.params.len() != args.len() {
                self.error(
                    "FIR-FC02",
                    format!(
                        "call to '{name}' has {} args, expected {}",
                        args.len(),
                        sig.params.len()
                    ),
                    id,
                );
            }
            for (i, (arg_id, (_pname, pty))) in args.iter().zip(sig.params.iter()).enumerate() {
                if let Some(actual_ty) = self.infer_value_type(*arg_id) {
                    if !self.types_compatible(&actual_ty, pty) {
                        self.warn(
                            "FIR-FC03",
                            format!(
                                "call to '{name}' arg #{i} has type {actual_ty:?}, expected {pty:?}"
                            ),
                            id,
                        );
                    }
                }
            }
            if &sig.return_type != declared {
                self.warn(
                    "FIR-FC04",
                    format!(
                        "call to '{name}' declared result type {declared:?} differs from function \
                         signature return type {:?}",
                        sig.return_type
                    ),
                    id,
                );
            }
        } else {
            self.error(
                "FIR-FC01",
                format!("call to undeclared function '{name}'"),
                id,
            );
        }

        self.check_math_call(id, name, args);
    }

    fn check_math_call(&mut self, id: FirId, name: &str, args: &[FirId]) {
        let raw = name.strip_prefix("std::").unwrap_or(name);
        let Some(op) = FirMathOp::from_symbol(name) else {
            if raw == "abs" {
                if let Some(arg) = args.first().and_then(|arg| self.infer_value_type(*arg)) {
                    if self.is_float_like_type(&arg) {
                        self.warn(
                            "FIR-MA04",
                            "use 'fabs' for floating-point absolute value (got 'abs')",
                            id,
                        );
                    }
                }
            }
            return;
        };

        let expected_arity = match op {
            FirMathOp::Pow
            | FirMathOp::Min
            | FirMathOp::Max
            | FirMathOp::Atan2
            | FirMathOp::Fmod
            | FirMathOp::Remainder => 2,
            _ => 1,
        };
        match expected_arity {
            1 if args.len() != 1 => self.warn(
                "FIR-MA01",
                format!(
                    "math op '{}' expects 1 arg, got {}",
                    op.symbol(),
                    args.len()
                ),
                id,
            ),
            2 if args.len() != 2 => self.warn(
                "FIR-MA02",
                format!(
                    "math op '{}' expects 2 args, got {}",
                    op.symbol(),
                    args.len()
                ),
                id,
            ),
            _ => {}
        }

        let is_float_math = !matches!(op, FirMathOp::Abs);
        if is_float_math && expected_arity == args.len() {
            for (i, arg_id) in args.iter().enumerate() {
                if let Some(arg_ty) = self.infer_value_type(*arg_id) {
                    if self.is_integer_type(&arg_ty) || arg_ty == FirType::Bool {
                        self.warn(
                            "FIR-MA03",
                            format!(
                                "math op '{}' arg #{i} is integer-like ({arg_ty:?}); \
                                 floating-point argument expected",
                                op.symbol()
                            ),
                            id,
                        );
                    }
                }
            }
        }

        if raw == "fabs" {
            if let Some(arg_ty) = args.first().and_then(|arg| self.infer_value_type(*arg)) {
                if self.is_integer_type(&arg_ty) || arg_ty == FirType::Bool {
                    self.warn(
                        "FIR-MA04",
                        format!("'fabs' called with integer-like argument {arg_ty:?}"),
                        id,
                    );
                }
            }
        }
    }

    fn check_load_table_types(
        &mut self,
        id: FirId,
        name: &str,
        access: AccessType,
        index: FirId,
        declared_elem_type: &FirType,
    ) {
        if let Some(index_ty) = self.infer_value_type(index) {
            if !self.is_integer_type(&index_ty) {
                self.error(
                    "FIR-T01",
                    format!("table index must be Int32 or Int64, got {index_ty:?}"),
                    id,
                );
            }
        }
        if access != AccessType::Struct {
            if let Some(entry) = self.resolve(name, access) {
                let effective_elem_type = if entry.is_table {
                    Some(entry.typ.clone())
                } else {
                    self.is_indexable_container_type(&entry.typ)
                };
                if effective_elem_type.is_none() {
                    self.warn(
                        "FIR-T03",
                        format!("LoadTable '{name}' refers to a non-table declaration"),
                        id,
                    );
                }
                if let Some(expected_elem_type) = effective_elem_type
                    && expected_elem_type != *declared_elem_type
                {
                    self.warn(
                        "FIR-T03",
                        format!(
                            "LoadTable '{name}' element type {declared_elem_type:?} differs from \
                             declaration {:?}",
                            expected_elem_type
                        ),
                        id,
                    );
                }
            }
        }
    }

    fn check_store_table_types(
        &mut self,
        id: FirId,
        name: &str,
        access: AccessType,
        index: FirId,
        value: FirId,
    ) {
        if let Some(index_ty) = self.infer_value_type(index) {
            if !self.is_integer_type(&index_ty) {
                self.error(
                    "FIR-T01",
                    format!("table index must be Int32 or Int64, got {index_ty:?}"),
                    id,
                );
            }
        }
        if access == AccessType::Struct {
            return;
        }
        if let Some(entry) = self.resolve(name, access) {
            let effective_elem_type = if entry.is_table {
                Some(entry.typ.clone())
            } else {
                self.is_indexable_container_type(&entry.typ)
            };
            if effective_elem_type.is_none() {
                self.warn(
                    "FIR-T03",
                    format!("StoreTable '{name}' refers to a non-table declaration"),
                    id,
                );
            }
            if let Some(val_ty) = self.infer_value_type(value) {
                let expected_elem_type = effective_elem_type.unwrap_or(entry.typ);
                if val_ty != expected_elem_type {
                    self.error(
                        "FIR-T02",
                        format!(
                            "StoreTable value type {val_ty:?} does not match element type {:?}",
                            expected_elem_type
                        ),
                        id,
                    );
                }
            }
        }
    }

    fn check_fun_call_drop_use(&mut self, id: FirId, value: FirId) {
        if let FirMatch::FunCall { name, typ, .. } = match_fir(self.store, value) {
            if typ != FirType::Void {
                self.warn(
                    "FIR-FC04",
                    format!("discarded non-void return value from '{name}' ({typ:?})"),
                    id,
                );
            }
        }
    }

    // ── Statement traversal ───────────────────────────────────────────────────

    fn check_stmt(&mut self, id: FirId) {
        match match_fir(self.store, id) {
            FirMatch::Block(stmts) => self.check_block(stmts),
            FirMatch::DeclareVar {
                name,
                typ,
                access,
                init,
            } => {
                self.check_declare_var(id, name, typ, access, init);
            }
            FirMatch::DeclareTable {
                name,
                access,
                elem_type,
                values,
            } => {
                for v in values {
                    self.check_value(v);
                }
                self.scope_stack.declare_table(name, elem_type, access);
            }
            FirMatch::StoreVar {
                name,
                access,
                value,
            } => {
                self.check_value(value);
                self.check_store_var(id, &name, access);
            }
            FirMatch::StoreTable {
                name,
                access,
                index,
                value,
            } => {
                self.check_value(index);
                self.check_value(value);
                self.check_store_var(id, &name, access);
                self.check_store_table_types(id, &name, access, index, value);
            }
            FirMatch::ShiftArrayVar { name, access, .. } => {
                // ShiftArrayVar modifies an array variable in-place; treat as a store.
                self.check_store_var(id, &name, access);
            }
            FirMatch::Drop(val) => {
                self.check_value(val);
                self.check_fun_call_drop_use(id, val);
            }
            FirMatch::Return(val) => self.check_return(id, val),
            FirMatch::If {
                cond,
                then_block,
                else_block,
            } => {
                self.check_value(cond);
                self.check_int_or_bool_condition(id, cond, "FIR-C04", "If");
                self.check_if(then_block, else_block);
            }
            FirMatch::ForLoop {
                var,
                init,
                end,
                step,
                body,
                ..
            } => {
                self.check_for_loop(id, &var, init, end, step, body);
            }
            FirMatch::SimpleForLoop {
                var, upper, body, ..
            } => {
                self.check_simple_for_loop(id, &var, upper, body);
            }
            FirMatch::IteratorForLoop {
                iterators, body, ..
            } => {
                self.check_iterator_for_loop(body, &iterators);
            }
            FirMatch::WhileLoop { cond, body } => {
                self.check_value(cond);
                self.check_int_or_bool_condition(id, cond, "FIR-L03", "WhileLoop");
                self.scope_stack.push(FrameKind::Block);
                self.check_stmt(body);
                self.scope_stack.pop();
            }
            FirMatch::Switch {
                cond,
                cases,
                default,
            } => {
                self.check_value(cond);
                self.check_switch_condition_type(id, cond);
                self.check_switch(id, cases, default);
            }
            FirMatch::Control { cond, stmt } => {
                self.check_value(cond);
                self.check_stmt(stmt);
            }
            // UI, meta, null — no scope-relevant content
            _ => {}
        }
    }

    // ── Block ─────────────────────────────────────────────────────────────────

    fn check_block(&mut self, stmts: Vec<FirId>) {
        self.scope_stack.push(FrameKind::Block);
        let mut returned = false;
        for (i, stmt_id) in stmts.iter().enumerate() {
            if returned {
                // R03: dead code after Return
                self.warn("FIR-R03", "unreachable statement after Return", *stmt_id);
                break;
            }
            // Detect a Return statement to set the `returned` flag for the next iteration.
            if matches!(match_fir(self.store, *stmt_id), FirMatch::Return(_)) {
                returned = true;
                // Still check the Return itself.
                let _ = i;
            }
            self.check_stmt(*stmt_id);
        }
        self.scope_stack.pop();
    }

    // ── DeclareVar ────────────────────────────────────────────────────────────

    fn check_declare_var(
        &mut self,
        id: FirId,
        name: String,
        typ: FirType,
        access: AccessType,
        init: Option<FirId>,
    ) {
        // SC07: kFunArgs must not be re-declared inside a function body
        if access == AccessType::FunArgs {
            self.error(
                "FIR-SC07",
                format!("kFunArgs variable '{name}' re-declared inside function body"),
                id,
            );
        }
        if !matches!(
            access,
            AccessType::Stack | AccessType::Loop | AccessType::FunArgs
        ) {
            self.error(
                "FIR-SC10",
                format!(
                    "local DeclareVar '{name}' uses non-local access type {access:?} \
                     (expected Stack or Loop)"
                ),
                id,
            );
        }

        if let Some(init_id) = init {
            self.check_value(init_id);
        }

        let init_status = if init.is_some() {
            InitStatus::Yes
        } else {
            InitStatus::No
        };
        self.scope_stack.declare(name, typ, access, init_status);
    }

    // ── LoadVar (value) ───────────────────────────────────────────────────────

    fn check_load_var(&mut self, id: FirId, name: &str, access: AccessType) {
        if access == AccessType::Struct && !self.symbols.struct_field_names.contains(name) {
            self.warn(
                "FIR-SC09",
                format!("kStruct variable '{name}' is not declared in dsp_struct"),
                id,
            );
            return;
        }
        match self.resolve(name, access) {
            None => {
                if let Some(entry) = self.resolve_any_by_name(name) {
                    self.error(
                        "FIR-SC02",
                        format!(
                            "variable '{name}' accessed as {access:?} \
                             but declared as {:?}",
                            entry.access
                        ),
                        id,
                    );
                } else {
                    // SC01: variable not declared
                    self.error(
                        "FIR-SC01",
                        format!("use of undeclared variable '{name}'"),
                        id,
                    );
                }
            }
            Some(entry) => {
                // SC02: access type must match declaration
                if entry.access != access {
                    self.error(
                        "FIR-SC02",
                        format!(
                            "variable '{name}' accessed as {access:?} \
                             but declared as {:?}",
                            entry.access
                        ),
                        id,
                    );
                }
                // SC03: warn if kStack variable is uninitialized
                if access == AccessType::Stack && entry.init == InitStatus::No {
                    self.warn(
                        "FIR-SC03",
                        format!("variable '{name}' may be used before initialization"),
                        id,
                    );
                }
            }
        }
    }

    // ── StoreVar (statement) ──────────────────────────────────────────────────

    fn check_store_var(&mut self, id: FirId, name: &str, access: AccessType) {
        if access == AccessType::Struct && !self.symbols.struct_field_names.contains(name) {
            self.warn(
                "FIR-SC09",
                format!("kStruct variable '{name}' is not declared in dsp_struct"),
                id,
            );
            return;
        }
        match self.resolve(name, access) {
            None => {
                if let Some(entry) = self.resolve_any_by_name(name) {
                    self.error(
                        "FIR-SC05",
                        format!(
                            "variable '{name}' stored as {access:?} \
                             but declared as {:?}",
                            entry.access
                        ),
                        id,
                    );
                } else {
                    self.error(
                        "FIR-SC04",
                        format!("store to undeclared variable '{name}'"),
                        id,
                    );
                }
            }
            Some(entry) => {
                // SC05: access type must match declaration
                if entry.access != access {
                    self.error(
                        "FIR-SC05",
                        format!(
                            "variable '{name}' stored as {access:?} \
                             but declared as {:?}",
                            entry.access
                        ),
                        id,
                    );
                }
                // Mark as initialized (for kStack / kLoop vars in the scope stack)
                if matches!(access, AccessType::Stack | AccessType::Loop) {
                    self.scope_stack.mark_initialized(name);
                }
            }
        }
    }

    // ── ForLoop ───────────────────────────────────────────────────────────────

    fn check_for_loop(
        &mut self,
        id: FirId,
        var: &str,
        init: FirId,
        end: FirId,
        step: FirId,
        body: FirId,
    ) {
        // L01 / L02: the init node should be DeclareVar(kLoop)
        match match_fir(self.store, init) {
            FirMatch::DeclareVar {
                name: ref decl_name,
                ref typ,
                access,
                ..
            } if decl_name == var => {
                if access != AccessType::Loop {
                    self.error(
                        "FIR-L01",
                        format!(
                            "ForLoop variable '{var}' init is not a kLoop DeclareVar \
                             (got {access:?})"
                        ),
                        init,
                    );
                }
                if !matches!(typ, FirType::Int32 | FirType::Int64) {
                    self.error(
                        "FIR-L02",
                        format!(
                            "ForLoop variable '{var}' type should be Int32 or Int64, \
                             got {typ:?}"
                        ),
                        init,
                    );
                }
            }
            _ => {
                // init is not a DeclareVar for the expected loop variable
                self.error(
                    "FIR-L01",
                    format!("ForLoop '{var}' init is not a DeclareVar for the loop variable"),
                    init,
                );
            }
        }

        // Push a Loop frame containing the loop variable
        self.scope_stack.push(FrameKind::Loop);

        // Process the init statement (registers the loop variable in the loop frame)
        self.check_stmt(init);

        // Traverse end condition and step
        self.check_value(end);
        self.check_stmt(step);

        // L04: body must be non-empty
        if let FirMatch::Block(ref stmts) = match_fir(self.store, body) {
            if stmts.is_empty() {
                self.warn("FIR-L04", format!("ForLoop '{var}' body is empty"), id);
            }
        }
        self.check_stmt(body);

        self.scope_stack.pop();
    }

    // ── SimpleForLoop ─────────────────────────────────────────────────────────

    fn check_simple_for_loop(&mut self, id: FirId, var: &str, upper: FirId, body: FirId) {
        self.scope_stack.push(FrameKind::Loop);

        // Implicit loop variable: kLoop, Int32, initialized
        self.scope_stack.declare(
            var.to_string(),
            FirType::Int32,
            AccessType::Loop,
            InitStatus::Yes,
        );

        self.check_value(upper);

        // L04: body must be non-empty
        if let FirMatch::Block(ref stmts) = match_fir(self.store, body) {
            if stmts.is_empty() {
                self.warn(
                    "FIR-L04",
                    format!("SimpleForLoop '{var}' body is empty"),
                    id,
                );
            }
        }
        self.check_stmt(body);

        self.scope_stack.pop();
    }

    // ── IteratorForLoop ───────────────────────────────────────────────────────

    fn check_iterator_for_loop(&mut self, body: FirId, iterators: &[String]) {
        self.scope_stack.push(FrameKind::Loop);
        for iter in iterators {
            self.scope_stack.declare(
                iter.clone(),
                FirType::Int32,
                AccessType::Loop,
                InitStatus::Yes,
            );
        }
        self.check_stmt(body);
        self.scope_stack.pop();
    }

    // ── If ────────────────────────────────────────────────────────────────────

    fn check_if(&mut self, then_block: FirId, else_block: Option<FirId>) {
        let pre = self.scope_stack.snapshot_inits();

        // Traverse then branch
        self.scope_stack.push(FrameKind::Block);
        self.check_stmt(then_block);
        self.scope_stack.pop();
        let then_changes = self.scope_stack.diff_inits(&pre);
        self.scope_stack.restore_inits(&pre);

        // Traverse else branch
        let else_changes = if let Some(else_id) = else_block {
            self.scope_stack.push(FrameKind::Block);
            self.check_stmt(else_id);
            self.scope_stack.pop();
            let changes = self.scope_stack.diff_inits(&pre);
            self.scope_stack.restore_inits(&pre);
            changes
        } else {
            Vec::new()
        };

        // Merge: both branches initialized the var → Yes; only one → Maybe
        let all: HashSet<(usize, String)> = then_changes
            .iter()
            .chain(else_changes.iter())
            .cloned()
            .collect();
        for (fi, name) in all {
            let in_then = then_changes.contains(&(fi, name.clone()));
            let in_else = else_changes.contains(&(fi, name.clone()));
            let status = if in_then && in_else {
                InitStatus::Yes
            } else {
                InitStatus::Maybe
            };
            self.scope_stack.set_init(fi, &name, status);
        }
    }

    // ── Return ────────────────────────────────────────────────────────────────

    fn check_return(&mut self, id: FirId, value: Option<FirId>) {
        if let Some(val_id) = value {
            self.check_value(val_id);
            if let Some(ret_ty) = &self.current_return_type {
                if let Some(val_ty) = self.infer_value_type(val_id) {
                    if val_ty != *ret_ty {
                        self.error(
                            "FIR-R01",
                            format!(
                                "Return value type {val_ty:?} does not match function return type {ret_ty:?}"
                            ),
                            id,
                        );
                    }
                }
            }
        } else {
            // R02: Return(None) in a non-Void function
            if let Some(ret_ty) = &self.current_return_type {
                if *ret_ty != FirType::Void {
                    self.warn(
                        "FIR-R02",
                        format!(
                            "Return without value in function '{}' whose return type is {:?}",
                            self.current_function.as_deref().unwrap_or("?"),
                            ret_ty
                        ),
                        id,
                    );
                }
            }
        }
        // R03 is handled by check_block which detects statements after a Return.
    }

    // ── Switch ────────────────────────────────────────────────────────────────

    fn check_switch(&mut self, id: FirId, cases: Vec<(i64, FirId)>, default: Option<FirId>) {
        // SW03: at least one case
        if cases.is_empty() {
            self.warn("FIR-SW03", "Switch has no cases", id);
        }

        // SW02: no duplicate case values
        let mut seen_vals: HashSet<i64> = HashSet::new();
        for &(val, case_body) in &cases {
            if !seen_vals.insert(val) {
                self.error(
                    "FIR-SW02",
                    format!("Switch has duplicate case value {val}"),
                    id,
                );
            }
            self.scope_stack.push(FrameKind::Block);
            self.check_stmt(case_body);
            self.scope_stack.pop();
        }

        if let Some(default_body) = default {
            self.scope_stack.push(FrameKind::Block);
            self.check_stmt(default_body);
            self.scope_stack.pop();
        }
    }

    // ── Value traversal ───────────────────────────────────────────────────────

    fn check_value(&mut self, id: FirId) {
        match match_fir(self.store, id) {
            FirMatch::LoadVar { name, access, .. } => {
                self.check_load_var(id, &name, access);
            }
            FirMatch::LoadVarAddress { name, access, .. } => {
                self.check_load_var(id, &name, access);
            }
            FirMatch::TeeVar {
                name,
                access,
                value,
                ..
            } => {
                // TeeVar = store + load: check that the target is declared
                self.check_value(value);
                self.check_store_var(id, &name, access);
            }
            FirMatch::LoadTable {
                name,
                access,
                index,
                typ,
            } => {
                self.check_load_var(id, &name, access);
                self.check_value(index);
                self.check_load_table_types(id, &name, access, index, &typ);
            }
            FirMatch::BinOp { op, lhs, rhs, typ } => {
                self.check_value(lhs);
                self.check_value(rhs);
                self.check_binop_types(id, op, lhs, rhs, &typ);
            }
            FirMatch::Neg { value, .. } => {
                self.check_value(value);
                self.check_neg_type(id, value);
            }
            FirMatch::Cast { typ, value } => {
                self.check_value(value);
                self.check_cast_type(id, &typ, value);
            }
            FirMatch::Bitcast { typ, value } => {
                self.check_value(value);
                self.check_bitcast_type(id, &typ, value);
            }
            FirMatch::Select2 {
                cond,
                then_value,
                else_value,
                typ,
            } => {
                self.check_value(cond);
                self.check_value(then_value);
                self.check_value(else_value);
                self.check_select2_types(id, cond, then_value, else_value, &typ);
            }
            FirMatch::FunCall { name, args, typ } => {
                for &arg in &args {
                    self.check_value(arg);
                }
                self.check_fun_call_types(id, &name, &args, &typ);
            }
            FirMatch::ValueArray { values, .. } => {
                for v in values {
                    self.check_value(v);
                }
            }
            // Leaf value nodes (literals, NullValue, NewDsp, etc.) — nothing to check
            _ => {}
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FirBinOp, FirBuilder, FirStore, NamedType};

    // ══ Phase 1 helpers (unchanged) ═══════════════════════════════════════════

    fn make_dsp_struct(b: &mut FirBuilder<'_>) -> FirId {
        b.block(&[])
    }

    fn make_empty_block(b: &mut FirBuilder<'_>) -> FirId {
        b.block(&[])
    }

    fn make_void_fun(b: &mut FirBuilder<'_>, name: &str) -> FirId {
        let typ = FirType::Fun {
            args: vec![],
            ret: Box::new(FirType::Void),
        };
        let body = b.block(&[]);
        b.declare_fun(name, typ, &[], Some(body), false)
    }

    fn make_full_declarations(b: &mut FirBuilder<'_>) -> FirId {
        let compute = {
            let params = vec![
                FirType::Ptr(Box::new(FirType::Obj)),
                FirType::Int32,
                FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            ];
            let args: Vec<NamedType> = params
                .iter()
                .enumerate()
                .map(|(i, t)| NamedType {
                    name: format!("p{i}"),
                    typ: t.clone(),
                })
                .collect();
            let typ = FirType::Fun {
                args: params,
                ret: Box::new(FirType::Void),
            };
            let body = b.block(&[]);
            b.declare_fun("compute", typ, &args, Some(body), false)
        };
        let mut funs: Vec<FirId> = DSP_API_FUNCTIONS
            .iter()
            .filter(|&&n| n != "compute")
            .map(|&n| make_void_fun(b, n))
            .collect();
        funs.push(compute);
        b.block(&funs)
    }

    fn make_valid_module(store: &mut FirStore) -> FirId {
        let mut b = FirBuilder::new(store);
        let dsp_struct = make_dsp_struct(&mut b);
        let globals = make_empty_block(&mut b);
        let declarations = make_full_declarations(&mut b);
        b.module("dsp", dsp_struct, globals, declarations)
    }

    // ── Helper: build a minimal module with one custom function ───────────────

    /// Wrap a custom function `DeclareFun` node inside a minimal module.
    fn module_with_fun(store: &mut FirStore, fun_id: FirId) -> FirId {
        let mut b = FirBuilder::new(store);
        let dsp_struct = make_dsp_struct(&mut b);
        let globals = make_empty_block(&mut b);
        let decls = b.block(&[fun_id]);
        b.module("dsp", dsp_struct, globals, decls)
    }

    // ══ Phase 1 tests (unchanged) ═════════════════════════════════════════════

    #[test]
    fn valid_module_has_no_errors() {
        let mut store = FirStore::new();
        let module_id = make_valid_module(&mut store);
        let report = verify_fir_module(&store, module_id);
        report.assert_ok();
    }

    #[test]
    fn m01_non_module_root() {
        let mut store = FirStore::new();
        let not_a_module = FirBuilder::new(&mut store).int32(0);
        let report = verify_fir_module(&store, not_a_module);
        assert!(report.diagnostics.iter().any(|d| d.code == "FIR-M01"));
    }

    #[test]
    fn m02_bad_dsp_struct_not_block() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let bad_struct = b.int32(0);
        let globals = make_empty_block(&mut b);
        let decls = make_full_declarations(&mut b);
        let module_id = b.module("dsp", bad_struct, globals, decls);
        let report = verify_fir_module(&store, module_id);
        assert!(report.diagnostics.iter().any(|d| d.code == "FIR-M02"));
    }

    #[test]
    fn m02_bad_dsp_struct_non_struct_type() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let bad_struct = b.declare_struct_type(FirType::Int32);
        let globals = make_empty_block(&mut b);
        let decls = make_full_declarations(&mut b);
        let module_id = b.module("dsp", bad_struct, globals, decls);
        let report = verify_fir_module(&store, module_id);
        assert!(report.diagnostics.iter().any(|d| d.code == "FIR-M02"));
    }

    #[test]
    fn m03_globals_not_block() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = make_dsp_struct(&mut b);
        let bad_globals = b.int32(0);
        let decls = make_full_declarations(&mut b);
        let module_id = b.module("dsp", dsp_struct, bad_globals, decls);
        assert!(
            verify_fir_module(&store, module_id)
                .diagnostics
                .iter()
                .any(|d| d.code == "FIR-M03")
        );
    }

    #[test]
    fn m04_declarations_not_block() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = make_dsp_struct(&mut b);
        let globals = make_empty_block(&mut b);
        let bad_decls = b.int32(0);
        let module_id = b.module("dsp", dsp_struct, globals, bad_decls);
        assert!(
            verify_fir_module(&store, module_id)
                .diagnostics
                .iter()
                .any(|d| d.code == "FIR-M04")
        );
    }

    #[test]
    fn m05_non_declarefun_in_declarations() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = make_dsp_struct(&mut b);
        let globals = make_empty_block(&mut b);
        let intruder = b.int32(99);
        let decls = b.block(&[intruder]);
        let module_id = b.module("dsp", dsp_struct, globals, decls);
        assert!(
            verify_fir_module(&store, module_id)
                .diagnostics
                .iter()
                .any(|d| d.code == "FIR-M05")
        );
    }

    #[test]
    fn m06_duplicate_function_name() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = make_dsp_struct(&mut b);
        let globals = make_empty_block(&mut b);
        let f1 = make_void_fun(&mut b, "myFun");
        let f2 = make_void_fun(&mut b, "myFun");
        let decls = b.block(&[f1, f2]);
        let module_id = b.module("dsp", dsp_struct, globals, decls);
        let report = verify_fir_module(&store, module_id);
        assert!(!report.has_errors());
        assert!(report.diagnostics.iter().any(|d| d.code == "FIR-M06"));
    }

    #[test]
    fn m07_missing_api_function() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = make_dsp_struct(&mut b);
        let globals = make_empty_block(&mut b);
        let decls = make_empty_block(&mut b);
        let module_id = b.module("dsp", dsp_struct, globals, decls);
        let report = verify_fir_module(&store, module_id);
        assert!(!report.has_errors());
        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter(|d| d.code == "FIR-M07")
                .count(),
            DSP_API_FUNCTIONS.len()
        );
    }

    #[test]
    fn s03_void_struct_field() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let bad_field = b.declare_var("f", FirType::Void, AccessType::Struct, None);
        let bad_struct = b.block(&[bad_field]);
        let globals = make_empty_block(&mut b);
        let decls = make_full_declarations(&mut b);
        let module_id = b.module("dsp", bad_struct, globals, decls);
        assert!(
            verify_fir_module(&store, module_id)
                .diagnostics
                .iter()
                .any(|d| d.code == "FIR-S03")
        );
    }

    #[test]
    fn s04_zero_size_array_field() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let bad_field = b.declare_var(
            "arr",
            FirType::Array(Box::new(FirType::Float32), 0),
            AccessType::Struct,
            None,
        );
        let bad_struct = b.block(&[bad_field]);
        let globals = make_empty_block(&mut b);
        let decls = make_full_declarations(&mut b);
        let module_id = b.module("dsp", bad_struct, globals, decls);
        let report = verify_fir_module(&store, module_id);
        assert!(!report.has_errors());
        assert!(report.diagnostics.iter().any(|d| d.code == "FIR-S04"));
    }

    #[test]
    fn struct_fields_registered_in_symbols() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let fields = vec![FirType::Int32, FirType::Float32];
        let f0 = b.declare_var("a", FirType::Int32, AccessType::Struct, None);
        let f1 = b.declare_var("b", FirType::Float32, AccessType::Struct, None);
        let dsp_struct = b.block(&[f0, f1]);
        let globals = make_empty_block(&mut b);
        let decls = make_full_declarations(&mut b);
        let module_id = b.module("dsp", dsp_struct, globals, decls);
        let (_report, symbols) = verify_module_structure(&store, module_id);
        assert_eq!(symbols.struct_name.as_deref(), Some("dsp"));
        assert_eq!(symbols.struct_fields, fields);
        assert!(symbols.struct_field_names.contains("a"));
        assert!(symbols.struct_field_names.contains("b"));
    }

    #[test]
    fn s01_struct_field_wrong_access() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let bad_field = b.declare_var("f", FirType::Int32, AccessType::Stack, None);
        let dsp_struct = b.block(&[bad_field]);
        let globals = make_empty_block(&mut b);
        let decls = make_full_declarations(&mut b);
        let module_id = b.module("dsp", dsp_struct, globals, decls);
        let report = verify_fir_module(&store, module_id);
        assert!(report.has_errors());
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-S01"),
            "{report:?}"
        );
    }

    #[test]
    fn s02_duplicate_struct_field_name() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let f1 = b.declare_var("f", FirType::Int32, AccessType::Struct, None);
        let f2 = b.declare_table("f", AccessType::Struct, FirType::FaustFloat, &[]);
        let dsp_struct = b.block(&[f1, f2]);
        let globals = make_empty_block(&mut b);
        let decls = make_full_declarations(&mut b);
        let module_id = b.module("dsp", dsp_struct, globals, decls);
        let report = verify_fir_module(&store, module_id);
        assert!(report.has_errors());
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-S02"),
            "{report:?}"
        );
    }

    #[test]
    fn g01_non_declarevar_in_globals() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = make_dsp_struct(&mut b);
        let intruder = b.int32(0);
        let globals = b.block(&[intruder]);
        let decls = make_full_declarations(&mut b);
        let module_id = b.module("dsp", dsp_struct, globals, decls);
        assert!(
            verify_fir_module(&store, module_id)
                .diagnostics
                .iter()
                .any(|d| d.code == "FIR-G01")
        );
    }

    #[test]
    fn g02_wrong_access_type_in_globals() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = make_dsp_struct(&mut b);
        let bad_var = b.declare_var("x", FirType::Int32, AccessType::Stack, None);
        let globals = b.block(&[bad_var]);
        let decls = make_full_declarations(&mut b);
        let module_id = b.module("dsp", dsp_struct, globals, decls);
        assert!(
            verify_fir_module(&store, module_id)
                .diagnostics
                .iter()
                .any(|d| d.code == "FIR-G02")
        );
    }

    #[test]
    fn g03_duplicate_global_name() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = make_dsp_struct(&mut b);
        let v1 = b.declare_var("g", FirType::Int32, AccessType::Global, None);
        let v2 = b.declare_var("g", FirType::Int32, AccessType::Global, None);
        let globals = b.block(&[v1, v2]);
        let decls = make_full_declarations(&mut b);
        let module_id = b.module("dsp", dsp_struct, globals, decls);
        assert!(
            verify_fir_module(&store, module_id)
                .diagnostics
                .iter()
                .any(|d| d.code == "FIR-G03")
        );
    }

    #[test]
    fn globals_registered_in_symbols() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = make_dsp_struct(&mut b);
        let var = b.declare_var("gRate", FirType::Int32, AccessType::Global, None);
        let globals = b.block(&[var]);
        let decls = make_full_declarations(&mut b);
        let module_id = b.module("dsp", dsp_struct, globals, decls);
        let (_report, symbols) = verify_module_structure(&store, module_id);
        assert!(symbols.globals.contains_key("gRate"));
    }

    #[test]
    fn f01_non_fun_type() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let body = b.block(&[]);
        let bad_fun = b.declare_fun("bad", FirType::Int32, &[], Some(body), false);
        let module_id = module_with_fun(&mut store, bad_fun);
        assert!(
            verify_fir_module(&store, module_id)
                .diagnostics
                .iter()
                .any(|d| d.code == "FIR-F01")
        );
    }

    #[test]
    fn f04_duplicate_param_name() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dup_args = vec![
            NamedType {
                name: "x".to_string(),
                typ: FirType::Int32,
            },
            NamedType {
                name: "x".to_string(),
                typ: FirType::Int32,
            },
        ];
        let typ = FirType::Fun {
            args: vec![FirType::Int32, FirType::Int32],
            ret: Box::new(FirType::Void),
        };
        let body = b.block(&[]);
        let fun = b.declare_fun("f", typ, &dup_args, Some(body), false);
        let module_id = module_with_fun(&mut store, fun);
        assert!(
            verify_fir_module(&store, module_id)
                .diagnostics
                .iter()
                .any(|d| d.code == "FIR-F04")
        );
    }

    #[test]
    fn f05_compute_non_void_return() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let params = vec![
            FirType::Ptr(Box::new(FirType::Obj)),
            FirType::Int32,
            FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        ];
        let args: Vec<NamedType> = params
            .iter()
            .enumerate()
            .map(|(i, t)| NamedType {
                name: format!("p{i}"),
                typ: t.clone(),
            })
            .collect();
        let typ = FirType::Fun {
            args: params,
            ret: Box::new(FirType::Int32),
        };
        let body = b.block(&[]);
        let compute = b.declare_fun("compute", typ, &args, Some(body), false);
        let module_id = module_with_fun(&mut store, compute);
        let report = verify_fir_module(&store, module_id);
        assert!(!report.has_errors());
        assert!(report.diagnostics.iter().any(|d| d.code == "FIR-F05"));
    }

    #[test]
    fn f06_compute_wrong_arity() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let typ = FirType::Fun {
            args: vec![FirType::Int32],
            ret: Box::new(FirType::Void),
        };
        let args = vec![NamedType {
            name: "n".to_string(),
            typ: FirType::Int32,
        }];
        let body = b.block(&[]);
        let compute = b.declare_fun("compute", typ, &args, Some(body), false);
        let module_id = module_with_fun(&mut store, compute);
        let report = verify_fir_module(&store, module_id);
        assert!(!report.has_errors());
        assert!(report.diagnostics.iter().any(|d| d.code == "FIR-F06"));
    }

    #[test]
    fn f07_prototype_only_function() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let typ = FirType::Fun {
            args: vec![],
            ret: Box::new(FirType::Void),
        };
        let proto = b.declare_fun("proto", typ, &[], None, false);
        let module_id = module_with_fun(&mut store, proto);
        let report = verify_fir_module(&store, module_id);
        assert!(!report.has_errors());
        assert!(report.diagnostics.iter().any(|d| d.code == "FIR-F07"));
    }

    #[test]
    fn functions_registered_in_symbols() {
        let mut store = FirStore::new();
        let module_id = make_valid_module(&mut store);
        let (_report, symbols) = verify_module_structure(&store, module_id);
        for &api_fn in DSP_API_FUNCTIONS {
            assert!(symbols.functions.contains_key(api_fn), "missing '{api_fn}'");
        }
    }

    // ══ Phase 2 helpers ═══════════════════════════════════════════════════════

    /// Build a single-function module with the given body statements.
    /// The function has signature `(x: Int32) -> Void` with param `x`.
    fn module_with_body(store: &mut FirStore, stmts: &[FirId]) -> FirId {
        let body = FirBuilder::new(store).block(stmts);
        let mut b = FirBuilder::new(store);
        let arg = NamedType {
            name: "x".to_string(),
            typ: FirType::Int32,
        };
        let typ = FirType::Fun {
            args: vec![FirType::Int32],
            ret: Box::new(FirType::Void),
        };
        let fun = b.declare_fun("myFun", typ, &[arg], Some(body), false);
        let dsp_struct = make_dsp_struct(&mut b);
        let globals = make_empty_block(&mut b);
        let decls = b.block(&[fun]);
        b.module("dsp", dsp_struct, globals, decls)
    }

    /// Build a single-function module that returns an Int32 value.
    fn module_with_int_body(store: &mut FirStore, stmts: &[FirId]) -> FirId {
        let body = FirBuilder::new(store).block(stmts);
        let mut b = FirBuilder::new(store);
        let typ = FirType::Fun {
            args: vec![],
            ret: Box::new(FirType::Int32),
        };
        let fun = b.declare_fun("getVal", typ, &[], Some(body), false);
        let dsp_struct = make_dsp_struct(&mut b);
        let globals = make_empty_block(&mut b);
        let decls = b.block(&[fun]);
        b.module("dsp", dsp_struct, globals, decls)
    }

    // ══ Phase 2 — SC checks ═══════════════════════════════════════════════════

    #[test]
    fn sc01_undeclared_load() {
        let mut store = FirStore::new();
        let load = FirBuilder::new(&mut store).load_var("z", AccessType::Stack, FirType::Int32);
        let drop = FirBuilder::new(&mut store).drop_(load);
        let module_id = module_with_body(&mut store, &[drop]);
        let report = verify_fir_module(&store, module_id);
        assert!(report.has_errors());
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-SC01"),
            "{report:?}"
        );
    }

    #[test]
    fn sc02_access_type_mismatch_load() {
        let mut store = FirStore::new();
        // Declare as kStack, then load as kGlobal
        let zero = FirBuilder::new(&mut store).int32(0);
        let decl = FirBuilder::new(&mut store).declare_var(
            "v",
            FirType::Int32,
            AccessType::Stack,
            Some(zero),
        );
        let load = FirBuilder::new(&mut store).load_var("v", AccessType::Global, FirType::Int32);
        let drop = FirBuilder::new(&mut store).drop_(load);
        let module_id = module_with_body(&mut store, &[decl, drop]);
        let report = verify_fir_module(&store, module_id);
        assert!(report.has_errors());
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-SC02"),
            "{report:?}"
        );
    }

    #[test]
    fn sc03_load_uninitialized() {
        let mut store = FirStore::new();
        // Declare without initializer, then load
        let decl =
            FirBuilder::new(&mut store).declare_var("v", FirType::Int32, AccessType::Stack, None);
        let load = FirBuilder::new(&mut store).load_var("v", AccessType::Stack, FirType::Int32);
        let drop = FirBuilder::new(&mut store).drop_(load);
        let module_id = module_with_body(&mut store, &[decl, drop]);
        let report = verify_fir_module(&store, module_id);
        assert!(!report.has_errors());
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-SC03"),
            "{report:?}"
        );
    }

    #[test]
    fn sc03_initialized_after_store_no_warning() {
        let mut store = FirStore::new();
        let zero = FirBuilder::new(&mut store).int32(0);
        let decl =
            FirBuilder::new(&mut store).declare_var("v", FirType::Int32, AccessType::Stack, None);
        let store_v = FirBuilder::new(&mut store).store_var("v", AccessType::Stack, zero);
        let load = FirBuilder::new(&mut store).load_var("v", AccessType::Stack, FirType::Int32);
        let drop = FirBuilder::new(&mut store).drop_(load);
        let module_id = module_with_body(&mut store, &[decl, store_v, drop]);
        let report = verify_fir_module(&store, module_id);
        assert!(
            !report.diagnostics.iter().any(|d| d.code == "FIR-SC03"),
            "{report:?}"
        );
    }

    #[test]
    fn sc04_undeclared_store() {
        let mut store = FirStore::new();
        let zero = FirBuilder::new(&mut store).int32(0);
        let store_v = FirBuilder::new(&mut store).store_var("z", AccessType::Stack, zero);
        let module_id = module_with_body(&mut store, &[store_v]);
        let report = verify_fir_module(&store, module_id);
        assert!(report.has_errors());
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-SC04"),
            "{report:?}"
        );
    }

    #[test]
    fn sc05_access_type_mismatch_store() {
        let mut store = FirStore::new();
        // Declare as kStack, store as kGlobal
        let decl =
            FirBuilder::new(&mut store).declare_var("v", FirType::Int32, AccessType::Stack, None);
        let zero = FirBuilder::new(&mut store).int32(0);
        let bad_store = FirBuilder::new(&mut store).store_var("v", AccessType::Global, zero);
        let module_id = module_with_body(&mut store, &[decl, bad_store]);
        let report = verify_fir_module(&store, module_id);
        assert!(report.has_errors());
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-SC05"),
            "{report:?}"
        );
    }

    #[test]
    fn sc07_funargs_redeclared_in_body() {
        let mut store = FirStore::new();
        // Declare a kFunArgs variable inside the function body (x is already a param)
        let redecl =
            FirBuilder::new(&mut store).declare_var("x", FirType::Int32, AccessType::FunArgs, None);
        let module_id = module_with_body(&mut store, &[redecl]);
        let report = verify_fir_module(&store, module_id);
        assert!(report.has_errors());
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-SC07"),
            "{report:?}"
        );
    }

    #[test]
    fn sc10_local_declarevar_with_global_access() {
        let mut store = FirStore::new();
        let bad_decl =
            FirBuilder::new(&mut store).declare_var("g", FirType::Int32, AccessType::Global, None);
        let module_id = module_with_body(&mut store, &[bad_decl]);
        let report = verify_fir_module(&store, module_id);
        assert!(report.has_errors());
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-SC10"),
            "{report:?}"
        );
    }

    #[test]
    fn funarg_load_is_valid() {
        // Loading a kFunArgs parameter should not produce any scope errors
        let mut store = FirStore::new();
        let load_x = FirBuilder::new(&mut store).load_var("x", AccessType::FunArgs, FirType::Int32);
        let drop = FirBuilder::new(&mut store).drop_(load_x);
        let module_id = module_with_body(&mut store, &[drop]);
        let report = verify_fir_module(&store, module_id);
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|d| matches!(d.code, "FIR-SC01" | "FIR-SC02")),
            "{report:?}"
        );
    }

    #[test]
    fn global_load_is_valid() {
        // Loading a kGlobal variable declared in the globals block should be valid
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = make_dsp_struct(&mut b);
        let g = b.declare_var("gRate", FirType::Int32, AccessType::Global, None);
        let globals = b.block(&[g]);
        let load_g = b.load_var("gRate", AccessType::Global, FirType::Int32);
        let drop = b.drop_(load_g);
        let body = b.block(&[drop]);
        let typ = FirType::Fun {
            args: vec![],
            ret: Box::new(FirType::Void),
        };
        let fun = b.declare_fun("f", typ, &[], Some(body), false);
        let decls = b.block(&[fun]);
        let module_id = b.module("dsp", dsp_struct, globals, decls);
        let report = verify_fir_module(&store, module_id);
        assert!(
            !report.diagnostics.iter().any(|d| d.code == "FIR-SC01"),
            "unexpected SC01: {report:?}"
        );
    }

    // ══ Phase 2 — loop checks ════════════════════════════════════════════════

    #[test]
    fn l01_forloop_non_loop_var() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        // init with kStack instead of kLoop
        let init_val = b.int32(0);
        let init_decl = b.declare_var("i", FirType::Int32, AccessType::Stack, Some(init_val));
        let cond = b.int32(10);
        let one = b.int32(1);
        let step = b.store_var("i", AccessType::Stack, one);
        let body_stmt = b.int32(0);
        let body = b.block(&[body_stmt]); // non-empty
        let loop_node = b.for_loop("i", init_decl, cond, step, body, false);
        let module_id = module_with_body(&mut store, &[loop_node]);
        let report = verify_fir_module(&store, module_id);
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-L01"),
            "{report:?}"
        );
    }

    #[test]
    fn l02_forloop_float_var() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        // loop variable is Float32 instead of Int32
        let init_val = b.float32(0.0);
        let init_decl = b.declare_var("f", FirType::Float32, AccessType::Loop, Some(init_val));
        let cond = b.int32(10);
        let one = b.float32(1.0);
        let step = b.store_var("f", AccessType::Loop, one);
        let body_stmt = b.int32(0);
        let body = b.block(&[body_stmt]);
        let loop_node = b.for_loop("f", init_decl, cond, step, body, false);
        let module_id = module_with_body(&mut store, &[loop_node]);
        let report = verify_fir_module(&store, module_id);
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-L02"),
            "{report:?}"
        );
    }

    #[test]
    fn l04_forloop_empty_body() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let init_val = b.int32(0);
        let init_decl = b.declare_var("i", FirType::Int32, AccessType::Loop, Some(init_val));
        let cond = b.int32(10);
        let one = b.int32(1);
        let step = b.store_var("i", AccessType::Loop, one);
        let body = b.block(&[]); // empty
        let loop_node = b.for_loop("i", init_decl, cond, step, body, false);
        let module_id = module_with_body(&mut store, &[loop_node]);
        let report = verify_fir_module(&store, module_id);
        assert!(!report.has_errors());
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-L04"),
            "{report:?}"
        );
    }

    #[test]
    fn l04_simple_forloop_empty_body() {
        let mut store = FirStore::new();
        let upper = FirBuilder::new(&mut store).int32(64);
        let body = FirBuilder::new(&mut store).block(&[]);
        let loop_node = FirBuilder::new(&mut store).simple_for_loop("i", upper, body, false);
        let module_id = module_with_body(&mut store, &[loop_node]);
        let report = verify_fir_module(&store, module_id);
        assert!(!report.has_errors());
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-L04"),
            "{report:?}"
        );
    }

    #[test]
    fn valid_forloop_no_errors() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let init_val = b.int32(0);
        let init_decl = b.declare_var("i", FirType::Int32, AccessType::Loop, Some(init_val));
        let load_i = b.load_var("i", AccessType::Loop, FirType::Int32);
        let limit = b.int32(64);
        let cond = b.binop(FirBinOp::Lt, load_i, limit, FirType::Bool);
        let load_i2 = b.load_var("i", AccessType::Loop, FirType::Int32);
        let one = b.int32(1);
        let step_val = b.binop(FirBinOp::Add, load_i2, one, FirType::Int32);
        let step = b.store_var("i", AccessType::Loop, step_val);
        let body_stmt = b.int32(0);
        let body = b.block(&[body_stmt]); // non-empty
        let loop_node = b.for_loop("i", init_decl, cond, step, body, false);
        let module_id = module_with_body(&mut store, &[loop_node]);
        let report = verify_fir_module(&store, module_id);
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|d| matches!(d.code, "FIR-L01" | "FIR-L02" | "FIR-SC01" | "FIR-SC02")),
            "{report:?}"
        );
    }

    // ══ Phase 2 — return checks ══════════════════════════════════════════════

    #[test]
    fn r02_return_none_in_non_void_function() {
        let mut store = FirStore::new();
        let ret = FirBuilder::new(&mut store).ret(None);
        let module_id = module_with_int_body(&mut store, &[ret]);
        let report = verify_fir_module(&store, module_id);
        assert!(!report.has_errors());
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-R02"),
            "{report:?}"
        );
    }

    #[test]
    fn r03_dead_code_after_return() {
        let mut store = FirStore::new();
        let ret = FirBuilder::new(&mut store).ret(None);
        let dead = FirBuilder::new(&mut store).null_statement();
        let module_id = module_with_body(&mut store, &[ret, dead]);
        let report = verify_fir_module(&store, module_id);
        assert!(!report.has_errors());
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-R03"),
            "{report:?}"
        );
    }

    // ══ Phase 2 — switch checks ═══════════════════════════════════════════════

    #[test]
    fn sw02_duplicate_case_value() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let cond = b.int32(0);
        let case_body = b.block(&[]);
        let sw = b.switch(cond, &[(0, case_body), (0, case_body)], None);
        let module_id = module_with_body(&mut store, &[sw]);
        let report = verify_fir_module(&store, module_id);
        assert!(report.has_errors());
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-SW02"),
            "{report:?}"
        );
    }

    #[test]
    fn sw03_empty_switch() {
        let mut store = FirStore::new();
        let cond = FirBuilder::new(&mut store).int32(0);
        let sw = FirBuilder::new(&mut store).switch(cond, &[], None);
        let module_id = module_with_body(&mut store, &[sw]);
        let report = verify_fir_module(&store, module_id);
        assert!(!report.has_errors());
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-SW03"),
            "{report:?}"
        );
    }

    #[test]
    fn sw03_default_only_still_warns_no_cases() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let cond = b.int32(0);
        let default_body = b.block(&[]);
        let sw = b.switch(cond, &[], Some(default_body));
        let module_id = module_with_body(&mut store, &[sw]);
        let report = verify_fir_module(&store, module_id);
        assert!(!report.has_errors());
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-SW03"),
            "{report:?}"
        );
    }

    // ══ Phase 2 — If / InitStatus merge ══════════════════════════════════════

    #[test]
    fn if_both_branches_initialize_var_marks_yes() {
        // var is uninitialized; both then and else store to it → must be Yes after If
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let zero = b.int32(0);
        let decl = b.declare_var("v", FirType::Int32, AccessType::Stack, None);
        let store_then = b.store_var("v", AccessType::Stack, zero);
        let store_else = b.store_var("v", AccessType::Stack, zero);
        let then_block = b.block(&[store_then]);
        let else_block = b.block(&[store_else]);
        let cond_val = b.bool_(true);
        let if_node = b.if_(cond_val, then_block, Some(else_block));
        // Load after If — should NOT trigger SC03
        let load = b.load_var("v", AccessType::Stack, FirType::Int32);
        let drop = b.drop_(load);
        let module_id = module_with_body(&mut store, &[decl, if_node, drop]);
        let report = verify_fir_module(&store, module_id);
        assert!(
            !report.diagnostics.iter().any(|d| d.code == "FIR-SC03"),
            "unexpected SC03: {report:?}"
        );
    }

    #[test]
    fn if_one_branch_initializes_var_marks_maybe() {
        // var is uninitialized; only then branch stores → Maybe after If.
        // Phase 2 treats `Maybe` as acceptable for SC03 (warning only on `No`).
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let zero = b.int32(0);
        let decl = b.declare_var("v", FirType::Int32, AccessType::Stack, None);
        let store_then = b.store_var("v", AccessType::Stack, zero);
        let then_block = b.block(&[store_then]);
        let cond_val = b.bool_(true);
        let if_node = b.if_(cond_val, then_block, None); // no else
        let load = b.load_var("v", AccessType::Stack, FirType::Int32);
        let drop = b.drop_(load);
        let module_id = module_with_body(&mut store, &[decl, if_node, drop]);
        let report = verify_fir_module(&store, module_id);
        assert!(
            !report.diagnostics.iter().any(|d| d.code == "FIR-SC03"),
            "unexpected SC03: {report:?}"
        );
    }

    #[test]
    fn if_non_block_branch_does_not_leak_scope() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let cond = b.bool_(true);
        let zero = b.int32(0);
        let then_decl = b.declare_var("t", FirType::Int32, AccessType::Stack, Some(zero));
        let if_node = b.if_(cond, then_decl, None);
        let load_t = b.load_var("t", AccessType::Stack, FirType::Int32);
        let drop_t = b.drop_(load_t);
        let module_id = module_with_body(&mut store, &[if_node, drop_t]);
        let report = verify_fir_module(&store, module_id);
        assert!(report.has_errors());
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-SC01"),
            "{report:?}"
        );
    }

    // ══ Phase 3 — type checks ════════════════════════════════════════════════

    #[test]
    fn b01_binop_type_mismatch() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let i = b.int32(1);
        let f = b.float32(2.0);
        let bad = b.binop(FirBinOp::Add, i, f, FirType::Float32);
        let drop = b.drop_(bad);
        let module_id = module_with_body(&mut store, &[drop]);
        let report = verify_fir_module(&store, module_id);
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-B01"),
            "{report:?}"
        );
    }

    #[test]
    fn u02_noop_cast() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let i = b.int32(1);
        let c = b.cast(FirType::Int32, i);
        let drop = b.drop_(c);
        let module_id = module_with_body(&mut store, &[drop]);
        let report = verify_fir_module(&store, module_id);
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-U02"),
            "{report:?}"
        );
    }

    #[test]
    fn u03_cast_non_numeric() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp = b.new_dsp("dsp", FirType::Obj);
        let c = b.cast(FirType::Int32, dsp);
        let drop = b.drop_(c);
        let module_id = module_with_body(&mut store, &[drop]);
        let report = verify_fir_module(&store, module_id);
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-U03"),
            "{report:?}"
        );
    }

    #[test]
    fn c01_select2_bad_condition_type() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let cond = b.float32(0.5);
        let t = b.int32(1);
        let e = b.int32(0);
        let sel = b.select2(cond, t, e, FirType::Int32);
        let drop = b.drop_(sel);
        let module_id = module_with_body(&mut store, &[drop]);
        let report = verify_fir_module(&store, module_id);
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-C01"),
            "{report:?}"
        );
    }

    #[test]
    fn c04_if_bad_condition_type() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let cond = b.float32(1.0);
        let then_block = b.block(&[]);
        let if_ = b.if_(cond, then_block, None);
        let module_id = module_with_body(&mut store, &[if_]);
        let report = verify_fir_module(&store, module_id);
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-C04"),
            "{report:?}"
        );
    }

    #[test]
    fn fc01_call_undeclared_function() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let arg = b.int32(1);
        let call = b.fun_call("missing", &[arg], FirType::Int32);
        let drop = b.drop_(call);
        let module_id = module_with_body(&mut store, &[drop]);
        let report = verify_fir_module(&store, module_id);
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-FC01"),
            "{report:?}"
        );
    }

    #[test]
    fn fc02_fc03_call_arity_and_arg_type_mismatch() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let callee_ty = FirType::Fun {
            args: vec![FirType::Int32],
            ret: Box::new(FirType::Int32),
        };
        let callee_args = vec![NamedType {
            name: "x".to_string(),
            typ: FirType::Int32,
        }];
        let callee = b.declare_fun("foo", callee_ty, &callee_args, None, false);

        let farg = b.new_dsp("tmp", FirType::Obj);
        let extra = b.int32(2);
        let call = b.fun_call("foo", &[farg, extra], FirType::Int32);
        let drop = b.drop_(call);
        let body = b.block(&[drop]);
        let caller_ty = FirType::Fun {
            args: vec![],
            ret: Box::new(FirType::Void),
        };
        let caller = b.declare_fun("caller", caller_ty, &[], Some(body), false);

        let dsp_struct = make_dsp_struct(&mut b);
        let globals = make_empty_block(&mut b);
        let decls = b.block(&[callee, caller]);
        let module_id = b.module("dsp", dsp_struct, globals, decls);
        let report = verify_fir_module(&store, module_id);
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-FC02"),
            "{report:?}"
        );
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-FC03"),
            "{report:?}"
        );
    }

    #[test]
    fn r01_return_value_type_mismatch() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let f = b.float32(1.0);
        let ret = b.ret(Some(f));
        let module_id = module_with_int_body(&mut store, &[ret]);
        let report = verify_fir_module(&store, module_id);
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-R01"),
            "{report:?}"
        );
    }

    #[test]
    fn l03_whileloop_bad_condition_type() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let cond = b.float32(0.0);
        let body = b.block(&[]);
        let w = b.while_loop(cond, body);
        let module_id = module_with_body(&mut store, &[w]);
        let report = verify_fir_module(&store, module_id);
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-L03"),
            "{report:?}"
        );
    }

    #[test]
    fn sw01_switch_bad_condition_type() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let cond = b.bool_(true);
        let body = b.block(&[]);
        let sw = b.switch(cond, &[(0, body)], None);
        let module_id = module_with_body(&mut store, &[sw]);
        let report = verify_fir_module(&store, module_id);
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-SW01"),
            "{report:?}"
        );
    }

    #[test]
    fn t01_t03_loadtable_bad_index_and_non_table_ref() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let decl = b.declare_var("x", FirType::Int32, AccessType::Stack, None);
        let idx = b.float32(1.5);
        let load = b.load_table("x", AccessType::Stack, idx, FirType::Int32);
        let drop = b.drop_(load);
        let module_id = module_with_body(&mut store, &[decl, drop]);
        let report = verify_fir_module(&store, module_id);
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-T01"),
            "{report:?}"
        );
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-T03"),
            "{report:?}"
        );
    }

    #[test]
    fn t02_storetable_value_type_mismatch() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let v0 = b.int32(0);
        let table = b.declare_table("t", AccessType::Stack, FirType::Int32, &[v0]);
        let idx = b.int32(0);
        let val = b.float32(1.0);
        let st = b.store_table("t", AccessType::Stack, idx, val);
        let module_id = module_with_body(&mut store, &[table, st]);
        let report = verify_fir_module(&store, module_id);
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-T02"),
            "{report:?}"
        );
    }

    #[test]
    fn ma03_and_ma04_math_call_warnings() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let proto_ty = FirType::Fun {
            args: vec![FirType::Float64],
            ret: Box::new(FirType::Float64),
        };
        let proto_args = vec![NamedType {
            name: "x".to_string(),
            typ: FirType::Float64,
        }];
        let sin_decl = b.declare_fun("sin", proto_ty.clone(), &proto_args, None, false);
        let fabs_decl = b.declare_fun("fabs", proto_ty, &proto_args, None, false);

        let i = b.int32(1);
        let call_sin = b.fun_call("sin", &[i], FirType::Float64);
        let call_fabs = b.fun_call("fabs", &[i], FirType::Float64);
        let d1 = b.drop_(call_sin);
        let d2 = b.drop_(call_fabs);
        let body = b.block(&[d1, d2]);
        let caller_ty = FirType::Fun {
            args: vec![],
            ret: Box::new(FirType::Void),
        };
        let caller = b.declare_fun("caller", caller_ty, &[], Some(body), false);

        let dsp_struct = make_dsp_struct(&mut b);
        let globals = make_empty_block(&mut b);
        let decls = b.block(&[sin_decl, fabs_decl, caller]);
        let module_id = b.module("dsp", dsp_struct, globals, decls);
        let report = verify_fir_module(&store, module_id);
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-MA03"),
            "{report:?}"
        );
        assert!(
            report.diagnostics.iter().any(|d| d.code == "FIR-MA04"),
            "{report:?}"
        );
    }
}
