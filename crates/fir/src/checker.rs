//! FIR module verifier — Phase 1: module structure and symbol collection.
//!
//! Validates the top-level shape of a `FirMatch::Module` node and populates
//! the [`ModuleSymbols`] tables used by subsequent verification phases.
//!
//! # Diagnostic codes implemented
//!
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
//! # Notes on S01/S02
//!
//! The plan (§5.2) describes S01 ("each field is a `DeclareVar` with `kStruct`
//! access") and S02 ("no duplicate field names") in terms of named struct
//! fields.  After the `FirType::Struct(String, Vec<FirType>)` refactor, field
//! *names* are not stored in the type — only field *types* are.  S01 and S02
//! therefore cannot be checked at the type level and are deferred to Phase 2,
//! where `kStruct`-access `DeclareVar` nodes encountered inside function bodies
//! are cross-validated against the struct (SC09).
//!
//! # Source provenance
//! - Plan: `porting/fir-module-verifier-plan-en.md`, §7 Phase 1
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
        self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }

    /// Iterates over error-severity diagnostics.
    pub fn errors(&self) -> impl Iterator<Item = &FirDiagnostic> {
        self.diagnostics.iter().filter(|d| d.severity == Severity::Error)
    }

    /// Iterates over warning-severity diagnostics.
    pub fn warnings(&self) -> impl Iterator<Item = &FirDiagnostic> {
        self.diagnostics.iter().filter(|d| d.severity == Severity::Warning)
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
#[derive(Debug, Default)]
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

// ─── Entry points ─────────────────────────────────────────────────────────────

/// Verify the module structure (Phase 1) and return the diagnostic report.
///
/// Checks M01–M07, S03–S04, G01–G03, F01, F04–F07.
/// For the collected symbol tables use [`verify_module_structure`].
pub fn verify_fir_module(store: &FirStore, module_id: FirId) -> FirVerifyReport {
    let (report, _symbols) = verify_module_structure(store, module_id);
    report
}

/// Like [`verify_fir_module`] but also returns the [`ModuleSymbols`] populated
/// during the phase-1 pass, for use by subsequent verification phases.
pub fn verify_module_structure(
    store: &FirStore,
    module_id: FirId,
) -> (FirVerifyReport, ModuleSymbols) {
    let mut ctx = VerifyCtx::new(store, module_id);
    ctx.check_phase1();
    (FirVerifyReport { diagnostics: ctx.diags }, ctx.symbols)
}

// ─── Internal context ──────────────────────────────────────────────────────────

struct VerifyCtx<'s> {
    store: &'s FirStore,
    module_id: FirId,
    diags: Vec<FirDiagnostic>,
    symbols: ModuleSymbols,
}

impl<'s> VerifyCtx<'s> {
    fn new(store: &'s FirStore, module_id: FirId) -> Self {
        Self {
            store,
            module_id,
            diags: Vec::new(),
            symbols: ModuleSymbols::default(),
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
            context: DiagContext::default(),
        });
    }

    fn error(&mut self, code: &'static str, message: impl Into<String>, node: FirId) {
        self.emit(Severity::Error, code, message, node);
    }

    fn warn(&mut self, code: &'static str, message: impl Into<String>, node: FirId) {
        self.emit(Severity::Warning, code, message, node);
    }

    // ─── Phase 1 top-level ────────────────────────────────────────────────────

    fn check_phase1(&mut self) {
        let id = self.module_id;

        // M01: root must decode as Module
        let FirMatch::Module { name, dsp_struct, globals, declarations } =
            match_fir(self.store, id)
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

    // ─── dsp_struct ───────────────────────────────────────────────────────────

    fn check_dsp_struct(&mut self, id: FirId) {
        // M02: must decode as DeclareStructType
        let FirMatch::DeclareStructType { typ } = match_fir(self.store, id) else {
            self.error("FIR-M02", "dsp_struct is not a DeclareStructType", id);
            return;
        };

        // M02: the wrapped type must be FirType::Struct
        let FirType::Struct(struct_name, fields) = typ else {
            self.error(
                "FIR-M02",
                "dsp_struct type is not FirType::Struct",
                id,
            );
            return;
        };

        self.symbols.struct_name = Some(struct_name);

        // S03/S04: validate each field type
        for (i, field_type) in fields.iter().enumerate() {
            match field_type {
                FirType::Void => {
                    // S03: field type must not be Void
                    self.error(
                        "FIR-S03",
                        format!("struct field #{i} has Void type"),
                        id,
                    );
                }
                FirType::Array(_, 0) => {
                    // S04: array fields must have size > 0
                    self.warn(
                        "FIR-S04",
                        format!("struct array field #{i} has size 0"),
                        id,
                    );
                }
                _ => {}
            }
        }

        self.symbols.struct_fields = fields;
    }

    // ─── globals ──────────────────────────────────────────────────────────────

    fn check_globals(&mut self, _block_id: FirId, stmts: Vec<FirId>) {
        let mut seen: HashSet<String> = HashSet::new();

        for stmt_id in stmts {
            match match_fir(self.store, stmt_id) {
                FirMatch::DeclareVar { name, typ, access, .. } => {
                    // G02: access must be Static or Global
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
                    // G03: no duplicate names
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
                FirMatch::DeclareTable { name, access, elem_type, .. } => {
                    // G02
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
                    // G03
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
                    // G01: only DeclareVar or DeclareTable allowed
                    self.error(
                        "FIR-G01",
                        "globals block contains a node that is not DeclareVar or DeclareTable",
                        stmt_id,
                    );
                }
            }
        }
    }

    // ─── declarations ─────────────────────────────────────────────────────────

    fn check_declarations(&mut self, _block_id: FirId, stmts: Vec<FirId>, _module_name: &str) {
        let mut seen: HashSet<String> = HashSet::new();

        for stmt_id in stmts {
            // M05: every node must be DeclareFun
            let FirMatch::DeclareFun { name, typ, args, body, .. } =
                match_fir(self.store, stmt_id)
            else {
                self.error(
                    "FIR-M05",
                    "declarations block contains a non-DeclareFun node",
                    stmt_id,
                );
                continue;
            };

            // M06: no duplicate function names
            if !seen.insert(name.clone()) {
                self.warn(
                    "FIR-M06",
                    format!("duplicate function name '{name}'"),
                    stmt_id,
                );
            }

            // F01: type must be FirType::Fun
            let FirType::Fun { args: param_types, ret } = &typ else {
                self.error(
                    "FIR-F01",
                    format!("function '{name}' has type that is not FirType::Fun"),
                    stmt_id,
                );
                continue;
            };

            // F02/F03: return type and param types are guaranteed valid by successful
            // decode of FirType::Fun (the arena decoder returns None on failure, which
            // would have produced FirMatch::Unknown, not DeclareFun).

            // F04: no duplicate parameter names
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

            // F05/F06: compute-specific signature checks
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

            // F07: body-less function is a prototype / extern declaration
            let is_extern = body.is_none();
            if is_extern {
                self.warn(
                    "FIR-F07",
                    format!("function '{name}' has no body (prototype/extern declaration)"),
                    stmt_id,
                );
            }

            // Register the first declaration in the symbol table (duplicates already warned).
            self.symbols.functions.entry(name).or_insert_with(|| FunctionSig {
                params: params_list,
                return_type: *ret.clone(),
                is_extern,
            });
        }

        // M07: expected DSP API functions must all be declared
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
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FirBuilder, FirStore, NamedType};

    // ── Helpers ──────────────────────────────────────────────────────────────

    /// Build a `DeclareStructType { Struct("dsp", []) }` node.
    fn make_dsp_struct(b: &mut FirBuilder<'_>) -> FirId {
        b.declare_struct_type(FirType::Struct("dsp".to_string(), vec![]))
    }

    /// Build an empty `Block` node (used for globals or declarations).
    fn make_empty_block(b: &mut FirBuilder<'_>) -> FirId {
        b.block(&[])
    }

    /// Build a `DeclareFun` with the given name, `() -> Void` signature, and an
    /// empty body.
    fn make_void_fun(b: &mut FirBuilder<'_>, name: &str) -> FirId {
        let typ = FirType::Fun { args: vec![], ret: Box::new(FirType::Void) };
        let body = b.block(&[]);
        b.declare_fun(name, typ, &[], Some(body), false)
    }

    /// Build a declarations block that contains exactly the 10 expected DSP API
    /// functions, each with a `() -> Void` signature and an empty body.
    ///
    /// This satisfies M05, M06, M07, F01, F04, F05 (Void return), F06 (arity
    /// except for `compute` which gets 4 params), F07 (has body).
    fn make_full_declarations(b: &mut FirBuilder<'_>) -> FirId {
        // compute needs 4 params to avoid F06
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
                .map(|(i, t)| NamedType { name: format!("p{i}"), typ: t.clone() })
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

    /// Build a fully valid minimal module (no errors, no warnings).
    fn make_valid_module(store: &mut FirStore) -> FirId {
        let mut b = FirBuilder::new(store);
        let dsp_struct = make_dsp_struct(&mut b);
        let globals = make_empty_block(&mut b);
        let declarations = make_full_declarations(&mut b);
        b.module("dsp", dsp_struct, globals, declarations)
    }

    // ── Module structure (M01–M07) ────────────────────────────────────────────

    #[test]
    fn valid_module_has_no_errors() {
        let mut store = FirStore::new();
        let module_id = make_valid_module(&mut store);
        let report = verify_fir_module(&store, module_id);
        report.assert_ok();
        assert!(!report.has_errors());
    }

    #[test]
    fn m01_non_module_root() {
        let mut store = FirStore::new();
        let not_a_module = FirBuilder::new(&mut store).int32(0);
        let report = verify_fir_module(&store, not_a_module);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|d| d.code == "FIR-M01"));
    }

    #[test]
    fn m02_bad_dsp_struct_not_declarestruct() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let bad_struct = b.block(&[]); // not a DeclareStructType
        let globals = make_empty_block(&mut b);
        let decls = make_full_declarations(&mut b);
        let module_id = b.module("dsp", bad_struct, globals, decls);
        let report = verify_fir_module(&store, module_id);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|d| d.code == "FIR-M02"));
    }

    #[test]
    fn m02_bad_dsp_struct_non_struct_type() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        // DeclareStructType wrapping a non-Struct type
        let bad_struct = b.declare_struct_type(FirType::Int32);
        let globals = make_empty_block(&mut b);
        let decls = make_full_declarations(&mut b);
        let module_id = b.module("dsp", bad_struct, globals, decls);
        let report = verify_fir_module(&store, module_id);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|d| d.code == "FIR-M02"));
    }

    #[test]
    fn m03_globals_not_block() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = make_dsp_struct(&mut b);
        let bad_globals = b.int32(0); // not a Block
        let decls = make_full_declarations(&mut b);
        let module_id = b.module("dsp", dsp_struct, bad_globals, decls);
        let report = verify_fir_module(&store, module_id);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|d| d.code == "FIR-M03"));
    }

    #[test]
    fn m04_declarations_not_block() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = make_dsp_struct(&mut b);
        let globals = make_empty_block(&mut b);
        let bad_decls = b.int32(0); // not a Block
        let module_id = b.module("dsp", dsp_struct, globals, bad_decls);
        let report = verify_fir_module(&store, module_id);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|d| d.code == "FIR-M04"));
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
        let report = verify_fir_module(&store, module_id);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|d| d.code == "FIR-M05"));
    }

    #[test]
    fn m06_duplicate_function_name() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = make_dsp_struct(&mut b);
        let globals = make_empty_block(&mut b);
        let f1 = make_void_fun(&mut b, "myFun");
        let f2 = make_void_fun(&mut b, "myFun"); // duplicate
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
        let decls = make_empty_block(&mut b); // no functions at all
        let module_id = b.module("dsp", dsp_struct, globals, decls);
        let report = verify_fir_module(&store, module_id);
        assert!(!report.has_errors());
        let m07_count = report.diagnostics.iter().filter(|d| d.code == "FIR-M07").count();
        assert_eq!(m07_count, DSP_API_FUNCTIONS.len());
    }

    // ── Struct fields (S03–S04) ───────────────────────────────────────────────

    #[test]
    fn s03_void_struct_field() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let bad_struct =
            b.declare_struct_type(FirType::Struct("dsp".to_string(), vec![FirType::Void]));
        let globals = make_empty_block(&mut b);
        let decls = make_full_declarations(&mut b);
        let module_id = b.module("dsp", bad_struct, globals, decls);
        let report = verify_fir_module(&store, module_id);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|d| d.code == "FIR-S03"));
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

    // ── Globals (G01–G03) ─────────────────────────────────────────────────────

    #[test]
    fn g01_non_declarevar_in_globals() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = make_dsp_struct(&mut b);
        let intruder = b.int32(0);
        let globals = b.block(&[intruder]);
        let decls = make_full_declarations(&mut b);
        let module_id = b.module("dsp", dsp_struct, globals, decls);
        let report = verify_fir_module(&store, module_id);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|d| d.code == "FIR-G01"));
    }

    #[test]
    fn g02_wrong_access_type_in_globals() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = make_dsp_struct(&mut b);
        // kStack variable in the globals block is wrong
        let bad_var = b.declare_var("x", FirType::Int32, AccessType::Stack, None);
        let globals = b.block(&[bad_var]);
        let decls = make_full_declarations(&mut b);
        let module_id = b.module("dsp", dsp_struct, globals, decls);
        let report = verify_fir_module(&store, module_id);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|d| d.code == "FIR-G02"));
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
        let report = verify_fir_module(&store, module_id);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|d| d.code == "FIR-G03"));
    }

    #[test]
    fn globals_registered_in_symbols() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = make_dsp_struct(&mut b);
        let var = b.declare_var("gSampleRate", FirType::Int32, AccessType::Global, None);
        let globals = b.block(&[var]);
        let decls = make_full_declarations(&mut b);
        let module_id = b.module("dsp", dsp_struct, globals, decls);
        let (_report, symbols) = verify_module_structure(&store, module_id);
        assert!(symbols.globals.contains_key("gSampleRate"));
        let (access, typ) = &symbols.globals["gSampleRate"];
        assert_eq!(*access, AccessType::Global);
        assert_eq!(*typ, FirType::Int32);
    }

    // ── Function declarations (F01, F04–F07) ──────────────────────────────────

    #[test]
    fn f01_non_fun_type() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = make_dsp_struct(&mut b);
        let globals = make_empty_block(&mut b);
        // Declare a function whose FirType is Int32 instead of Fun
        let body = b.block(&[]);
        let bad_fun = b.declare_fun("badFn", FirType::Int32, &[], Some(body), false);
        let decls = b.block(&[bad_fun]);
        let module_id = b.module("dsp", dsp_struct, globals, decls);
        let report = verify_fir_module(&store, module_id);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|d| d.code == "FIR-F01"));
    }

    #[test]
    fn f04_duplicate_param_name() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = make_dsp_struct(&mut b);
        let globals = make_empty_block(&mut b);
        let dup_args = vec![
            NamedType { name: "x".to_string(), typ: FirType::Int32 },
            NamedType { name: "x".to_string(), typ: FirType::Int32 }, // duplicate
        ];
        let typ = FirType::Fun {
            args: vec![FirType::Int32, FirType::Int32],
            ret: Box::new(FirType::Void),
        };
        let body = b.block(&[]);
        let bad_fun = b.declare_fun("dupParams", typ, &dup_args, Some(body), false);
        let decls = b.block(&[bad_fun]);
        let module_id = b.module("dsp", dsp_struct, globals, decls);
        let report = verify_fir_module(&store, module_id);
        assert!(report.has_errors());
        assert!(report.diagnostics.iter().any(|d| d.code == "FIR-F04"));
    }

    #[test]
    fn f05_compute_non_void_return() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = make_dsp_struct(&mut b);
        let globals = make_empty_block(&mut b);
        // compute with Int32 return type
        let params = vec![
            FirType::Ptr(Box::new(FirType::Obj)),
            FirType::Int32,
            FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        ];
        let args: Vec<NamedType> = params
            .iter()
            .enumerate()
            .map(|(i, t)| NamedType { name: format!("p{i}"), typ: t.clone() })
            .collect();
        let typ = FirType::Fun { args: params, ret: Box::new(FirType::Int32) };
        let body = b.block(&[]);
        let compute = b.declare_fun("compute", typ, &args, Some(body), false);
        let decls = b.block(&[compute]);
        let module_id = b.module("dsp", dsp_struct, globals, decls);
        let report = verify_fir_module(&store, module_id);
        assert!(!report.has_errors());
        assert!(report.diagnostics.iter().any(|d| d.code == "FIR-F05"));
    }

    #[test]
    fn f06_compute_wrong_arity() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = make_dsp_struct(&mut b);
        let globals = make_empty_block(&mut b);
        // compute with only 2 params
        let typ = FirType::Fun {
            args: vec![FirType::Int32, FirType::Int32],
            ret: Box::new(FirType::Void),
        };
        let args = vec![
            NamedType { name: "a".to_string(), typ: FirType::Int32 },
            NamedType { name: "b".to_string(), typ: FirType::Int32 },
        ];
        let body = b.block(&[]);
        let compute = b.declare_fun("compute", typ, &args, Some(body), false);
        let decls = b.block(&[compute]);
        let module_id = b.module("dsp", dsp_struct, globals, decls);
        let report = verify_fir_module(&store, module_id);
        assert!(!report.has_errors());
        assert!(report.diagnostics.iter().any(|d| d.code == "FIR-F06"));
    }

    #[test]
    fn f07_prototype_only_function() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = make_dsp_struct(&mut b);
        let globals = make_empty_block(&mut b);
        let typ = FirType::Fun { args: vec![], ret: Box::new(FirType::Void) };
        let proto = b.declare_fun("myProto", typ, &[], None, false); // no body
        let decls = b.block(&[proto]);
        let module_id = b.module("dsp", dsp_struct, globals, decls);
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
            assert!(
                symbols.functions.contains_key(api_fn),
                "missing '{api_fn}' in function symbol table"
            );
        }
    }
}
