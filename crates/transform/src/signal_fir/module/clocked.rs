//! Guarded-block lowering for clocked wrappers (roadmap P3.1, first slice).
//!
//! # Source provenance (C++)
//! - `compiler/generator/compile_scal.cpp` (`generateOD`, `generateTempVar`,
//!   `generatePermVar`, `sigSeq`/`sigClocked` cases, 2181-2284; branch
//!   `master-dev-ocpp-od-fir-2-FIR19`, commit `8eebea429`)
//! - `compiler/parallelize/loop.cpp` (`CodeIFblock`)
//!
//! # Scope of this slice
//! **Boolean `ondemand` only**: the wrapper clock must be provably boolean
//! (type interval ⊆ [0, 1]), and the block is emitted as one guarded `If`
//! region (`if (clock != 0) { … }`) — the C++ `CodeIFblock` shape, reusing
//! the generic FIR `If`/`Block` statements per the P2.1 vocabulary decision.
//! Counted-loop integer `ondemand`, `upsampling`, and `downsampling` keep the
//! structured `FRS-SFIR-0007` rejection, as do delay lines inside a clocked
//! block that would need the per-domain `IOTA`
//! (`delay/domain_counters.rs`) — only state whose updates are emitted
//! *inside* the guarded region (shift-strategy delays, scalar recursion
//! carriers) is allowed, because it advances exactly when the block fires.
//!
//! # Emission model (adaptation note)
//! C++ drives emission from the `Hsched` schedule. This port keeps the
//! fast-lane's demand-driven lowering and obtains the same ordering
//! guarantees from the **region redirection** rule: when lowering reaches a
//! signal whose inferred domain is a strict ancestor of the domain of the
//! open guarded block, emission is redirected to the ancestor's region
//! (statements land *before* the guard statement, which is appended when the
//! block closes). `Hgraph`/`Hsched` (P1.2) are still built by the clocked
//! entry point as a pre-lowering validation pass (partition + causality).
//!
//! # Node generators (plan §3.8)
//! - `Seq(od, y)` → `CS(od); return CS(y)`;
//! - `Clocked(env, y)` → passthrough (annotation only);
//! - `TempVar(x)` → passthrough: evaluated inside the guard, the expression
//!   reads the outer value exactly at the fire tick;
//! - `PermVar(Clocked(env, v))` → persistent struct field `fPerm<i>` cleared
//!   to 0, assigned inside the block, read as a plain field load;
//! - `OnDemand([Clocked(env, h), holds…])` → guarded `If` region.

use super::*;

/// Per-compilation clocked-lowering state (present only when the program
/// actually contains clocked wrappers).
pub(super) struct ClockedState<'a> {
    /// Propagation-owned clock-domain table (P0.2).
    pub(super) domains: &'a propagate::ClockDomainTable,
    /// Clock-environment side map from `clk_env::annotate` (P1.1).
    pub(super) envs: crate::clk_env::ClkEnvMap,
    /// Domains of the open guarded blocks, innermost last (parallel to
    /// `RegionTree::child_depth`).
    pub(super) open_domains: Vec<propagate::ClockDomainId>,
    /// Counted-loop context per open block (parallel to `open_domains`):
    /// `Some` for integer-OD/US blocks, `None` for If-shaped blocks.
    pub(super) open_loops: Vec<Option<LoopCtx>>,
    /// `PermVar` node → hold-field name (registered by the wrapper emission).
    pub(super) perm_fields: HashMap<SigId, String>,
    /// Wrapper nodes whose guarded block has been emitted.
    pub(super) emitted_blocks: HashSet<SigId>,
    /// Monotonic counter for `fPerm<i>` hold-field names.
    pub(super) next_perm_id: usize,
}

/// Analysis products handed to `build_module` for clocked programs.
pub(crate) struct ClockedPlan<'a> {
    pub(crate) domains: &'a propagate::ClockDomainTable,
    pub(crate) envs: crate::clk_env::ClkEnvMap,
}

impl<'a> ClockedState<'a> {
    pub(super) fn new(plan: ClockedPlan<'a>) -> Self {
        Self {
            domains: plan.domains,
            envs: plan.envs,
            open_domains: Vec::new(),
            open_loops: Vec::new(),
            perm_fields: HashMap::new(),
            emitted_blocks: HashSet::new(),
            next_perm_id: 0,
        }
    }
}

/// Counted-loop context of one open guarded block (integer OD / US).
#[derive(Clone)]
pub(super) struct LoopCtx {
    /// Inner loop variable name (`lOd<i>`), readable by `ZeroPad` lowering.
    pub(super) var: String,
    /// Lowered loop bound (the integer clock value at this tick).
    pub(super) bound: FirId,
}

/// Guard statement shape selected from the wrapper kind and clock type
/// (plan §3.8: boolean OD `if`, integer OD / US counted loop, DS modulo).
enum GuardShape {
    /// `if (clock != 0) { body }`
    BoolIf,
    /// `for (l = 0; l < clock; l++) { body }`
    CountedLoop,
    /// `if (fDSCounter_d<i> == 0) { body }` + post `counter = (counter+1) % clock`
    DsModulo,
}

fn clocked_not_lowered(message: impl Into<String>) -> SignalFirError {
    SignalFirError::new(SignalFirErrorCode::ClockedNotLowered, message)
}

impl<'a> SignalToFirLower<'a> {
    /// Computes the append-redirection depth for `sig`, when its inferred
    /// domain is a strict ancestor of the effective (append-target) domain.
    ///
    /// Returns `None` when no redirection is needed: not a clocked program,
    /// same domain, unknown node, or a *deeper* domain (deeper values are
    /// only reachable through their wrapper / hold fields, which have their
    /// own arms).
    pub(super) fn clocked_redirect_target(&self, sig: SigId) -> Option<usize> {
        let clocked = self.clocked.as_ref()?;
        let sig_env = self.clocked_env_of(sig)?;

        let effective_depth = self
            .regions
            .redirect_depth()
            .unwrap_or_else(|| self.regions.child_depth());
        let effective_env: crate::clk_env::ClkEnv = if effective_depth == 0 {
            None
        } else {
            Some(clocked.open_domains[effective_depth - 1])
        };
        if sig_env == effective_env {
            return None;
        }
        if !crate::clk_env::is_ancestor_clk_env(clocked.domains, sig_env, effective_env) {
            return None;
        }
        match sig_env {
            None => Some(0),
            Some(id) => clocked
                .open_domains
                .iter()
                .position(|&d| d == id)
                .map(|index| index + 1),
        }
    }

    /// Inferred clock environment of one prepared node, when known.
    fn clocked_env_of(&self, sig: SigId) -> Option<crate::clk_env::ClkEnv> {
        self.clocked.as_ref()?.envs.env(sig)
    }

    /// Rejects clocked programs whose planned delay lines live inside a
    /// clocked domain with a strategy whose time advance is *global*
    /// (`CircularPow2` shares `fIOTA`; `IfWrapping` counters bump at the top
    /// sample end). Inner state must advance only when its block fires, so
    /// until the per-domain `IOTA` integration (`delay/domain_counters.rs`)
    /// is wired into these strategies, only shift-strategy lines are legal
    /// inside a domain (their updates are emitted inside the guarded
    /// region).
    pub(super) fn reject_unsupported_clocked_delay_lines(&mut self) -> Result<(), SignalFirError> {
        let Some(clocked) = self.clocked.as_ref() else {
            return Ok(());
        };
        for (&carried, info) in self.delay.lines() {
            let inner = matches!(clocked.envs.env(carried), Some(Some(_)));
            if inner && !matches!(info.strategy, super::super::delay::DelayKind::Shift) {
                return Err(clocked_not_lowered(format!(
                    "delay line on signal {} inside a clocked block uses a \
                     globally-advanced strategy ({:?}); per-domain IOTA lowering \
                     has not landed yet (P3 follow-up)",
                    carried.as_u32(),
                    info.strategy
                )));
            }
        }
        Ok(())
    }

    /// Lowers `ZeroPad(x, h)` inside a counted upsampling block:
    /// `((loop_idx == bound - 1) ? x : 0)` — the input value is exposed on
    /// the **last** inner iteration only (plan §3.8 `generateZeroPad`).
    pub(super) fn lower_zero_pad_clocked(
        &mut self,
        sig: SigId,
        value: SigId,
    ) -> Result<FirId, SignalFirError> {
        let effective_depth = self
            .regions
            .redirect_depth()
            .unwrap_or_else(|| self.regions.child_depth());
        let loop_ctx = self
            .clocked
            .as_ref()
            .and_then(|c| {
                effective_depth
                    .checked_sub(1)
                    .and_then(|index| c.open_loops.get(index))
            })
            .and_then(Clone::clone);
        let Some(loop_ctx) = loop_ctx else {
            return Err(clocked_not_lowered(
                "ZeroPad outside a counted upsampling block (zero-stuffed inputs \
                 are only legal inside integer-clock blocks)",
            ));
        };
        let value_fir = self.lower_signal(value)?;
        let ty = self.signal_fir_type(sig)?;
        let zero = if matches!(ty, FirType::Int32) {
            self.lower_int32_const(0)
        } else {
            self.float_const(0.0)
        };
        let one = self.lower_int32_const(1);
        let mut b = FirBuilder::new(&mut self.store);
        let idx = b.load_var(loop_ctx.var.clone(), AccessType::Stack, FirType::Int32);
        let last = b.binop(FirBinOp::Sub, loop_ctx.bound, one, FirType::Int32);
        let is_last = b.binop(FirBinOp::Eq, idx, last, FirType::Int32);
        Ok(b.select2(is_last, value_fir, zero, ty))
    }

    /// Reads one registered hold field (`PermVar` outside its block).
    pub(super) fn lower_perm_var_read(&mut self, sig: SigId) -> Result<FirId, SignalFirError> {
        let Some(name) = self
            .clocked
            .as_ref()
            .and_then(|c| c.perm_fields.get(&sig).cloned())
        else {
            return Err(clocked_not_lowered(
                "PermVar hold read before its clocked wrapper block was emitted \
                 (expected Seq(wrapper, hold) shape from propagation)",
            ));
        };
        let ty = self.signal_fir_type(sig)?;
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.load_var(name, AccessType::Struct, ty))
    }

    /// Emits the guarded block of one `ondemand` wrapper, idempotent.
    ///
    /// Returns a dummy value: the wrapper "returns no expression" (C++);
    /// consumers read the hold fields through `Seq`.
    pub(super) fn ensure_guarded_block(&mut self, wrapper: SigId) -> Result<FirId, SignalFirError> {
        let dummy = self.lower_int32_const(0);
        if self
            .clocked
            .as_ref()
            .is_some_and(|c| c.emitted_blocks.contains(&wrapper))
        {
            return Ok(dummy);
        }

        // ── Decode the wrapper payload ───────────────────────────────────
        let (children, kind): (Vec<SigId>, propagate::ClockDomainKind) =
            match match_sig(self.arena, wrapper) {
                SigMatch::OnDemand(children) => {
                    (children.to_vec(), propagate::ClockDomainKind::OnDemand)
                }
                SigMatch::Upsampling(children) => {
                    (children.to_vec(), propagate::ClockDomainKind::Upsampling)
                }
                SigMatch::Downsampling(children) => {
                    (children.to_vec(), propagate::ClockDomainKind::Downsampling)
                }
                _ => {
                    return Err(clocked_not_lowered(
                        "ensure_guarded_block called on a non-wrapper signal",
                    ));
                }
            };
        let Some((&first, holds)) = children.split_first() else {
            return Err(clocked_not_lowered("clocked wrapper without children"));
        };
        let SigMatch::Clocked(env_tok, clock) = match_sig(self.arena, first) else {
            return Err(clocked_not_lowered(
                "clocked wrapper first child must be Clocked(env, clock)",
            ));
        };
        let SigMatch::ClockEnvToken(domain_id) = match_sig(self.arena, env_tok) else {
            return Err(clocked_not_lowered(
                "clocked wrapper carries a malformed clock-env token",
            ));
        };
        let domain = propagate::ClockDomainId::from_u32(domain_id);

        // ── Guard shape selection (plan §3.8) ────────────────────────────
        let clock_is_int = matches!(self.types.get(&clock), Some(SimpleSigType::Int));
        let shape = match kind {
            propagate::ClockDomainKind::OnDemand => {
                let clock_interval = self
                    .sig_types
                    .get(&clock)
                    .map(sigtype::SigType::interval)
                    .ok_or_else(|| clocked_not_lowered("clock signal has no type annotation"))?;
                let boolean = clock_interval.is_valid()
                    && clock_interval.lo() >= 0.0
                    && clock_interval.hi() <= 1.0;
                if boolean {
                    GuardShape::BoolIf
                } else if clock_is_int {
                    GuardShape::CountedLoop
                } else {
                    return Err(clocked_not_lowered(
                        "ondemand with a real-valued non-boolean clock is not supported \
                         (use a boolean gate or an integer repetition count)",
                    ));
                }
            }
            propagate::ClockDomainKind::Upsampling => {
                if !clock_is_int {
                    return Err(clocked_not_lowered(
                        "upsampling requires an integer clock (inner iteration count)",
                    ));
                }
                GuardShape::CountedLoop
            }
            propagate::ClockDomainKind::Downsampling => {
                if !clock_is_int {
                    return Err(clocked_not_lowered(
                        "downsampling requires an integer clock (decimation factor)",
                    ));
                }
                GuardShape::DsModulo
            }
        };

        // ── Clock and guard precondition (outer region) ──────────────────
        let clock_fir = self.lower_signal(clock)?;
        let guard_cond = match shape {
            GuardShape::BoolIf => {
                let zero = if clock_is_int {
                    self.lower_int32_const(0)
                } else {
                    self.float_const(0.0)
                };
                let mut b = FirBuilder::new(&mut self.store);
                Some(b.binop(FirBinOp::Ne, clock_fir, zero, FirType::Int32))
            }
            GuardShape::CountedLoop => None,
            GuardShape::DsModulo => {
                // C++ declareRetrieveDSName: per-domain modulo counter,
                // fires when the counter is 0.
                let counter = {
                    let mut ctx = DelayFirCtx {
                        store: &mut self.store,
                        real_ty: self.real_ty.clone(),
                        types: self.types,
                        struct_declarations: &mut self.sections.struct_declarations,
                        clear_statements: &mut self.sections.clear_statements,
                        clear_init_seen: &mut self.sections.clear_init_seen,
                        next_loop_var_id: &mut self.name_gen.next_loop_var_id,
                        uses_iota: &mut self.uses_iota,
                    };
                    self.domain_counters
                        .declare_retrieve_ds_counter(domain, &mut ctx)
                };
                let zero = self.lower_int32_const(0);
                let mut b = FirBuilder::new(&mut self.store);
                let counter_value = b.load_var(counter, AccessType::Struct, FirType::Int32);
                Some(b.binop(FirBinOp::Eq, counter_value, zero, FirType::Int32))
            }
        };
        let loop_ctx = match shape {
            GuardShape::CountedLoop => Some(LoopCtx {
                var: self.fresh_loop_var("lOd"),
                bound: clock_fir,
            }),
            GuardShape::BoolIf | GuardShape::DsModulo => None,
        };

        // ── Hold fields: persistent struct fields cleared to 0 ───────────
        let mut hold_stores: Vec<(SigId, String)> = Vec::with_capacity(holds.len());
        for &hold in holds {
            let SigMatch::PermVar(inner) = match_sig(self.arena, hold) else {
                return Err(clocked_not_lowered(
                    "clocked wrapper output is not a PermVar hold",
                ));
            };
            let ty = self.signal_fir_type(hold)?;
            let clocked_state = self
                .clocked
                .as_mut()
                .expect("guarded blocks only emitted for clocked programs");
            let name = format!("fPerm{}", clocked_state.next_perm_id);
            clocked_state.next_perm_id += 1;
            clocked_state.perm_fields.insert(hold, name.clone());
            let is_int = matches!(ty, FirType::Int32);
            let decl = {
                let mut b = FirBuilder::new(&mut self.store);
                b.declare_var(name.clone(), ty, AccessType::Struct, None)
            };
            self.sections.struct_declarations.push(decl);
            if self.sections.clear_init_seen.insert(name.clone()) {
                let zero = if is_int {
                    self.lower_int32_const(0)
                } else {
                    self.float_const(0.0)
                };
                let mut b = FirBuilder::new(&mut self.store);
                let clear = b.store_var(name.clone(), AccessType::Struct, zero);
                self.sections.clear_statements.push(clear);
            }
            hold_stores.push((inner, name));
        }

        // ── Body: lower the held values inside the child region ──────────
        let uses_iota_before = self.uses_iota;
        self.regions.open_child();
        {
            let clocked_state = self.clocked.as_mut().expect("clocked state present");
            clocked_state.open_domains.push(domain);
            clocked_state.open_loops.push(loop_ctx.clone());
        }
        let prev_redirect = self.regions.set_redirect(None);

        let mut body_result: Result<(), SignalFirError> = Ok(());
        for (value, field) in &hold_stores {
            // Propagation wraps every held value as Clocked(env, v); the
            // Clocked arm strips the annotation.
            let lowered = match self.lower_signal(*value) {
                Ok(id) => id,
                Err(err) => {
                    body_result = Err(err);
                    break;
                }
            };
            let store_stmt = {
                let mut b = FirBuilder::new(&mut self.store);
                b.store_var(field.clone(), AccessType::Struct, lowered)
            };
            self.regions.current_phases_mut().immediate.push(store_stmt);
        }

        {
            let clocked_state = self.clocked.as_mut().expect("clocked state present");
            clocked_state.open_domains.pop();
            clocked_state.open_loops.pop();
        }
        let body_phases = self.regions.close_child();
        self.regions.set_redirect(prev_redirect);
        body_result?;

        // Delay/recursion state inside the block must advance only when the
        // block fires. Shift-strategy delays and scalar recursion carriers
        // emit their updates inside the child region (correct); anything
        // that required the *global* circular cursor would advance every
        // sample — reject until per-domain IOTA lands (P3 follow-up).
        if !uses_iota_before && self.uses_iota {
            return Err(clocked_not_lowered(
                "state inside this ondemand block needs the shared circular cursor \
                 (fIOTA); per-domain IOTA lowering has not landed yet (P3 follow-up)",
            ));
        }

        // ── Wrap the body in the guard and append to the outer region ────
        let body_stmts = body_phases.flattened();
        let guard = {
            let mut b = FirBuilder::new(&mut self.store);
            let block = b.block(&body_stmts);
            match (&shape, guard_cond, &loop_ctx) {
                (GuardShape::BoolIf | GuardShape::DsModulo, Some(cond), _) => {
                    b.if_(cond, block, None)
                }
                (GuardShape::CountedLoop, _, Some(ctx)) => {
                    b.simple_for_loop(ctx.var.clone(), ctx.bound, block, false)
                }
                _ => unreachable!("guard shape and condition are built together"),
            }
        };
        self.regions.current_phases_mut().immediate.push(guard);
        if matches!(shape, GuardShape::DsModulo) {
            // Post-code, every outer tick: counter = (counter + 1) % clock.
            let counter = self
                .domain_counters
                .ds_counter_name(domain)
                .expect("DS counter declared above")
                .to_owned();
            let bump =
                DomainCounters::emit_wrapping_increment(&mut self.store, &counter, clock_fir);
            self.regions.current_phases_mut().immediate.push(bump);
        }

        self.clocked
            .as_mut()
            .expect("clocked state present")
            .emitted_blocks
            .insert(wrapper);
        Ok(dummy)
    }
}
