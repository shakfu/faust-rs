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
            perm_fields: HashMap::new(),
            emitted_blocks: HashSet::new(),
            next_perm_id: 0,
        }
    }
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
        let children: Vec<SigId> = match match_sig(self.arena, wrapper) {
            SigMatch::OnDemand(children) => children.to_vec(),
            SigMatch::Upsampling(_) => {
                return Err(clocked_not_lowered(
                    "upsampling (counted inner loop) is not lowered yet — this slice \
                     covers boolean ondemand only (roadmap P3 follow-up)",
                ));
            }
            SigMatch::Downsampling(_) => {
                return Err(clocked_not_lowered(
                    "downsampling (modulo firing guard) is not lowered yet — this slice \
                     covers boolean ondemand only (roadmap P3 follow-up)",
                ));
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

        // ── Boolean-clock requirement (this slice) ───────────────────────
        let clock_interval = self
            .sig_types
            .get(&clock)
            .map(sigtype::SigType::interval)
            .ok_or_else(|| clocked_not_lowered("clock signal has no type annotation"))?;
        if !(clock_interval.is_valid() && clock_interval.lo() >= 0.0 && clock_interval.hi() <= 1.0)
        {
            return Err(clocked_not_lowered(format!(
                "integer ondemand (clock interval [{}, {}]) needs the counted-loop \
                 OD block — not lowered yet, this slice covers boolean clocks only",
                clock_interval.lo(),
                clock_interval.hi()
            )));
        }

        // ── Clock and guard condition (outer region) ─────────────────────
        let clock_fir = self.lower_signal(clock)?;
        let clock_is_int = matches!(self.types.get(&clock), Some(SimpleSigType::Int));
        let zero = if clock_is_int {
            self.lower_int32_const(0)
        } else {
            self.float_const(0.0)
        };
        let cond = {
            let mut b = FirBuilder::new(&mut self.store);
            b.binop(FirBinOp::Ne, clock_fir, zero, FirType::Int32)
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
        self.clocked
            .as_mut()
            .expect("clocked state present")
            .open_domains
            .push(domain);
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

        self.clocked
            .as_mut()
            .expect("clocked state present")
            .open_domains
            .pop();
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
            b.if_(cond, block, None)
        };
        self.regions.current_phases_mut().immediate.push(guard);

        self.clocked
            .as_mut()
            .expect("clocked state present")
            .emitted_blocks
            .insert(wrapper);
        Ok(dummy)
    }
}
