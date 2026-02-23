//! FIR module verifier — Phase 1 and Phase 2.
//!
//! **Phase 1** validates the top-level shape of a `FirMatch::Module` node and
//! populates [`ModuleSymbols`] (struct fields, globals, declared functions).
//!
//! **Phase 2** traverses every function body and performs scope analysis:
//! variable declarations, accesses, loop structures, return statements, and
//! switch statements.
//!
//! # Diagnostic codes implemented
//!
//! ## Phase 1 — module structure
//! | Code | Sev | Check |
//! |---|---|---|
//! | FIR-M01 | E | Root node is not a Module |
//! | FIR-M02 | E | `dsp_struct` is not a valid `DeclareStructType` |
//! | FIR-M03 | E | `globals` is not a Block |
//! | FIR-M04 | E | `declarations` is not a Block |
//! | FIR-M05 | E | Non-`DeclareFun` node in declarations block |
//! | FIR-M06 | W | Duplicate function name in declarations |
//! | FIR-M07 | W | Expected DSP API function is not declared |
//! | FIR-S03 | E | Struct field has `Void` type |
//! | FIR-S04 | W | Struct array field has size 0 |
//! | FIR-G01 | E | Globals block contains a non-`DeclareVar`/`DeclareTable` node |
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
//! | FIR-SC09 | W | `kStruct` access on a name not seen in any struct field |
//! | FIR-SC10 | E | Local `DeclareVar` uses a non-local access class |
//! | FIR-L01  | E | `ForLoop` init is not a `DeclareVar(kLoop)` |
//! | FIR-L02  | E | `ForLoop` loop variable type is not `Int32`/`Int64` |
//! | FIR-L04  | W | `ForLoop`/`SimpleForLoop` body is empty |
//! | FIR-R02  | W | `Return(None)` in a non-`Void` function |
//! | FIR-R03  | W | Statements after a `Return` in a block (dead code) |
//! | FIR-SW02 | E | Duplicate case value in `Switch` |
//! | FIR-SW03 | W | `Switch` has no cases |
//!
//! ## Deferred
//! - **S01/S02** — struct field names not available in `FirType::Struct(_, Vec<FirType>)`.
//! - **SC06/SC08** — naturally enforced by scope-stack pop; SC01 fires for any
//!   out-of-scope access regardless of the access class.
//! - **SC09** — field names not available; kStruct accesses are assumed valid.
//! - **L03** — WhileLoop condition type requires Phase 3 type inference.
//! - **R01** — return value type matching requires Phase 3 type inference.
//! - **SW01** — switch condition type requires Phase 3 type inference.
//!
//! # Source provenance
//! - Plan: `porting/fir-module-verifier-plan-en.md`, §7
//! - C++ parity: `FIRTypeChecker`, `FIRCodeChecker`, `FIRVarChecker`
//!   in `compiler/generator/fir_to_fir.hh` and `fir_code_checker.hh`

use std::collections::{HashMap, HashSet};

use crate::{AccessType, FirId, FirMatch, FirStore, FirType, match_fir};

// ─── Diagnostic types ─────────────────────────────────────────────────────────

/// Severity of a verifier diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// A single diagnostic produced during FIR verification.
#[derive(Clone, Debug, PartialEq)]
pub struct FirDiagnostic {
    pub severity: Severity,
    /// Short code from the diagnostic registry, e.g. `"FIR-M01"`.
    pub code: &'static str,
    pub message: String,
    /// The [`FirId`] most closely associated with the problem.
    pub node: FirId,
    pub context: DiagContext,
}

/// Contextual location of a diagnostic (enclosing function, variable, etc.).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiagContext {
    pub function_name: Option<String>,
    pub variable_name: Option<String>,
}

// ─── Verify report ─────────────────────────────────────────────────────────────

/// Result of a FIR verification run.
#[derive(Debug, Default)]
pub struct FirVerifyReport {
    pub diagnostics: Vec<FirDiagnostic>,
}

impl FirVerifyReport {
    /// Returns `true` if any `Error`-severity diagnostics were emitted.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    /// Iterates over error-severity diagnostics.
    pub fn errors(&self) -> impl Iterator<Item = &FirDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
    }

    /// Iterates over warning-severity diagnostics.
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
    pub return_type: FirType,
    /// `true` when the function has no body (prototype / extern declaration).
    pub is_extern: bool,
}

/// Symbol tables populated during Phase 1 (module-level pass).
///
/// These tables feed into Phase 2 (scope analysis) and Phase 3 (type checking).
#[derive(Clone, Debug, Default)]
pub struct ModuleSymbols {
    /// Struct name from `FirType::Struct(name, _)`.
    pub struct_name: Option<String>,
    /// Ordered field types from `FirType::Struct(_, fields)`.
    ///
    /// Field names are not available at the type level; they are gathered from
    /// `kStruct`-access `DeclareVar` nodes in function bodies during Phase 2.
    pub struct_fields: Vec<FirType>,
    /// Global/static variables: name → `(AccessType, FirType)`.
    pub globals: HashMap<String, (AccessType, FirType)>,
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
}

/// Kind of a scope frame.
#[derive(Clone, Debug)]
enum FrameKind {
    /// Ordinary `Block`.
    Block,
    /// Loop body (ForLoop / SimpleForLoop / IteratorForLoop).
    Loop { var_name: String },
    /// Top-level function frame (holds kFunArgs pre-populated).
    Function,
}

/// One level of the lexical scope stack.
#[derive(Clone, Debug)]
struct ScopeFrame {
    kind: FrameKind,
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

    fn push(&mut self, kind: FrameKind) {
        self.frames.push(ScopeFrame {
            kind,
            vars: HashMap::new(),
        });
    }

    fn pop(&mut self) {
        self.frames.pop();
    }

    /// Declare a variable in the current (top) frame.
    fn declare(&mut self, name: String, typ: FirType, access: AccessType, init: InitStatus) {
        if let Some(frame) = self.frames.last_mut() {
            frame.vars.insert(name, VarEntry { access, typ, init });
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

    /// Returns `true` if the current stack has at least one Loop frame.
    fn is_in_loop(&self) -> bool {
        self.frames
            .iter()
            .any(|f| matches!(f.kind, FrameKind::Loop { .. }))
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

/// Verify the FIR module (Phase 1 + Phase 2) and return the diagnostic report.
pub fn verify_fir_module(store: &FirStore, module_id: FirId) -> FirVerifyReport {
    let (report, _symbols) = verify_module_structure(store, module_id);
    report
}

/// Like [`verify_fir_module`] but also returns the [`ModuleSymbols`] collected
/// during Phase 1, for use by Phase 3 (type checking, not yet implemented).
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
/// This runs the Phase 2 scope checks only (no module-shape validation, no
/// Phase 3 type inference).
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
        let FirMatch::DeclareStructType { typ } = match_fir(self.store, id) else {
            self.error("FIR-M02", "dsp_struct is not a DeclareStructType", id);
            return;
        };

        let FirType::Struct(struct_name, fields) = typ else {
            self.error("FIR-M02", "dsp_struct type is not FirType::Struct", id);
            return;
        };

        self.symbols.struct_name = Some(struct_name);

        for (i, field_type) in fields.iter().enumerate() {
            match field_type {
                FirType::Void => {
                    self.error("FIR-S03", format!("struct field #{i} has Void type"), id);
                }
                FirType::Array(_, 0) => {
                    self.warn("FIR-S04", format!("struct array field #{i} has size 0"), id);
                }
                _ => {}
            }
        }

        self.symbols.struct_fields = fields;
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
                        self.symbols.globals.insert(name, (access, elem_type));
                    } else {
                        self.error(
                            "FIR-G03",
                            format!("duplicate global variable name '{name}'"),
                            stmt_id,
                        );
                    }
                }
                _ => {
                    self.error(
                        "FIR-G01",
                        "globals block contains a node that is not DeclareVar or DeclareTable",
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

            if !seen.insert(name.clone()) {
                self.warn(
                    "FIR-M06",
                    format!("duplicate function name '{name}'"),
                    stmt_id,
                );
            }

            let FirType::Fun {
                args: param_types,
                ret,
            } = &typ
            else {
                self.error(
                    "FIR-F01",
                    format!("function '{name}' has type that is not FirType::Fun"),
                    stmt_id,
                );
                continue;
            };

            let mut param_names: HashSet<String> = HashSet::new();
            let mut params_list: Vec<(String, FirType)> = Vec::with_capacity(args.len());
            for arg in &args {
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
            if is_extern {
                self.warn(
                    "FIR-F07",
                    format!("function '{name}' has no body (prototype/extern declaration)"),
                    stmt_id,
                );
            }

            self.symbols
                .functions
                .entry(name)
                .or_insert_with(|| FunctionSig {
                    params: params_list,
                    return_type: *ret.clone(),
                    is_extern,
                });
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
    /// For `kStruct` accesses, always returns a dummy `Ok` entry (SC09 deferred).
    fn resolve(&self, name: &str, access: AccessType) -> Option<VarEntry> {
        match access {
            AccessType::Struct => {
                // SC09 would validate the name against struct field names, but
                // FirType::Struct(_, Vec<FirType>) does not store names.
                // Treat kStruct accesses as always valid.
                Some(VarEntry {
                    access: AccessType::Struct,
                    typ: FirType::Void, // placeholder; type check is Phase 3
                    init: InitStatus::Yes,
                })
            }
            AccessType::Static | AccessType::Global => {
                let (a, t) = self.symbols.globals.get(name)?;
                Some(VarEntry {
                    access: *a,
                    typ: t.clone(),
                    init: InitStatus::Yes,
                })
            }
            AccessType::FunArgs => {
                let t = self.current_fun_args.get(name)?;
                Some(VarEntry {
                    access: AccessType::FunArgs,
                    typ: t.clone(),
                    init: InitStatus::Yes,
                })
            }
            AccessType::Stack | AccessType::Loop => {
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
        if let Some((_, entry)) = self.scope_stack.lookup(name) {
            return Some(entry.clone());
        }
        if let Some(t) = self.current_fun_args.get(name) {
            return Some(VarEntry {
                access: AccessType::FunArgs,
                typ: t.clone(),
                init: InitStatus::Yes,
            });
        }
        if let Some((access, typ)) = self.symbols.globals.get(name) {
            return Some(VarEntry {
                access: *access,
                typ: typ.clone(),
                init: InitStatus::Yes,
            });
        }
        // kStruct names cannot be validated precisely in Phase 2 because field
        // names are not carried in `FirType::Struct(_, Vec<FirType>)`.
        None
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
                self.scope_stack
                    .declare(name, elem_type, access, InitStatus::Yes);
            }
            FirMatch::StoreVar {
                name,
                access,
                value,
            } => {
                self.check_value(value);
                self.check_store_var(id, &name, access);
            }
            FirMatch::StoreTable { index, value, .. } => {
                self.check_value(index);
                self.check_value(value);
            }
            FirMatch::ShiftArrayVar { name, access, .. } => {
                // ShiftArrayVar modifies an array variable in-place; treat as a store.
                self.check_store_var(id, &name, access);
            }
            FirMatch::Drop(val) => self.check_value(val),
            FirMatch::Return(val) => self.check_return(id, val),
            FirMatch::If {
                cond,
                then_block,
                else_block,
            } => {
                self.check_value(cond);
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
        self.scope_stack.push(FrameKind::Loop {
            var_name: var.to_string(),
        });

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
        self.scope_stack.push(FrameKind::Loop {
            var_name: var.to_string(),
        });

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
        let var_name = iterators.first().cloned().unwrap_or_default();
        self.scope_stack.push(FrameKind::Loop { var_name });
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
                ..
            } => {
                self.check_load_var(id, &name, access);
                self.check_value(index);
            }
            FirMatch::BinOp { lhs, rhs, .. } => {
                self.check_value(lhs);
                self.check_value(rhs);
            }
            FirMatch::Neg { value, .. }
            | FirMatch::Cast { value, .. }
            | FirMatch::Bitcast { value, .. } => {
                self.check_value(value);
            }
            FirMatch::Select2 {
                cond,
                then_value,
                else_value,
                ..
            } => {
                self.check_value(cond);
                self.check_value(then_value);
                self.check_value(else_value);
            }
            FirMatch::FunCall { args, .. } => {
                for arg in args {
                    self.check_value(arg);
                }
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
        b.declare_struct_type(FirType::Struct("dsp".to_string(), vec![]))
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
    fn m02_bad_dsp_struct_not_declarestruct() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let bad_struct = b.block(&[]);
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
        let bad_struct =
            b.declare_struct_type(FirType::Struct("dsp".to_string(), vec![FirType::Void]));
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
        let bad_struct = b.declare_struct_type(FirType::Struct(
            "dsp".to_string(),
            vec![FirType::Array(Box::new(FirType::Float32), 0)],
        ));
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
        let dsp_struct = b.declare_struct_type(FirType::Struct("dsp".to_string(), fields.clone()));
        let globals = make_empty_block(&mut b);
        let decls = make_full_declarations(&mut b);
        let module_id = b.module("dsp", dsp_struct, globals, decls);
        let (_report, symbols) = verify_module_structure(&store, module_id);
        assert_eq!(symbols.struct_name.as_deref(), Some("dsp"));
        assert_eq!(symbols.struct_fields, fields);
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
}
