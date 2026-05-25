use super::*;

impl<'a> SignalToFirLower<'a> {
    /// Creates a fresh lowering state for one [`build_module`] call.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        arena: &'a TreeArena,
        ui_program: &'a UiProgram,
        types: &'a HashMap<SigId, SimpleSigType>,
        sig_types: &'a HashMap<SigId, SigType>,
        num_inputs: usize,
        real_ty: FirType,
        sig_ref_counts: HashMap<SigId, usize>,
        sig_at_boundary: HashSet<SigId>,
        konst_escapes: HashSet<SigId>,
        delay_opts: DelayOptions,
    ) -> Self {
        Self {
            arena,
            ui_program,
            types,
            sig_types,
            num_inputs,
            real_ty,
            store: FirStore::new(),
            cache: HashMap::new(),
            struct_declarations: Vec::new(),
            static_declarations: Vec::new(),
            global_declarations: Vec::new(),
            constants_statements: Vec::new(),
            reset_statements: Vec::new(),
            clear_statements: Vec::new(),
            control_statements: Vec::new(),
            sample_phases: SamplePhases::default(),
            state_name_by_node: HashMap::new(),
            recursion: RecursionState::default(),
            scheduled_state_updates: HashSet::new(),
            delay: DelayManager::new(delay_opts),
            uses_iota: false,
            ui_controls: HashMap::new(),
            soundfiles: HashMap::new(),
            waveform_tables: HashMap::new(),
            waveform_table_len: HashMap::new(),
            table_access_by_sig: HashMap::new(),
            ui_statements: Vec::new(),
            named_struct_vars: HashSet::new(),
            reset_init_seen: HashSet::new(),
            clear_init_seen: HashSet::new(),
            input_ptr_aliases: HashMap::new(),
            used_math_ops: HashSet::new(),
            used_int_fun_names: HashSet::new(),
            used_foreign_fun_protos: BTreeMap::new(),
            used_foreign_vars: BTreeMap::new(),
            next_loop_var_id: 0,
            fconst_counter: 0,
            iconst_counter: 0,
            fslow_counter: 0,
            islow_counter: 0,
            sig_ref_counts,
            sig_at_boundary,
            konst_escapes,
            forward_output_by_sig: HashMap::new(),
            forward_output_by_sig_key: HashMap::new(),
            lowering_reverse_loop: false,
            bra_state_scheduled: HashSet::new(),
            bra_grad_cache: HashMap::new(),
            bra_delay1_carry_vars: HashMap::new(),
            bra_delay_array_carry_vars: HashMap::new(),
            bra_tape_store_var: HashMap::new(),
        }
    }

    /// Ensures the canonical DSP sample-rate field is present in the FIR struct.
    ///
    /// Backends should consume this field directly instead of synthesizing their
    /// own `fSampleRate` side channel.
    pub(super) fn ensure_sample_rate_var(&mut self) {
        self.ensure_named_struct_var("fSampleRate", FirType::Int32, None);
    }

    /// Pre-scans the output signal forest and allocates all delay lines before
    /// lowering begins.
    ///
    /// This preparation step now has two phases:
    ///
    /// - [`DelayManager::analyze_signals`] computes read-only accumulated delay
    ///   metadata for reachable signals and recursion outputs
    /// - [`DelayManager::scan_signals`] collects the concrete non-recursive
    ///   carried signals that still need standalone delay-line allocation
    ///
    /// Multiple `SIGDELAY(x, n)` nodes sharing the same carried signal `x`
    /// reuse one delay line sized to the largest delay seen. Standalone
    /// `Delay1(x)` nodes that use the shift strategy are included in the same
    /// pre-pass so delay-line geometry is decided exactly once up front.
    ///
    /// Recursion carriers are not allocated here directly; their size is
    /// planned by the accumulated delay analysis and consumed later by
    /// `ensure_recursion_array_for_group`.
    ///
    /// This pre-pass ensures all resource-sizing decisions are registered
    /// before reads are emitted during lowering.
    pub(super) fn prepare_delay_lines(&mut self, outputs: &[SigId]) -> Result<(), SignalFirError> {
        self.delay
            .analyze_signals(self.arena, self.sig_types, outputs)?;
        let max_delays = self
            .delay
            .scan_signals(self.arena, self.sig_types, outputs)?;
        for (carried, delay) in max_delays {
            self.ensure_delay_line_decl(carried, delay)?;
        }
        Ok(())
    }

    /// Emits the BRA reverse update for a supported unary math node.
    ///
    /// Unlike the pure Signal RAD path, BRA cannot freely rebuild every
    /// operand expression during the reverse sweep: operands may be temporal,
    /// recursive, or otherwise already materialized in forward storage. This
    /// method therefore performs the tape-aware loads first, then delegates
    /// only the pointwise algebra to `ad_rules`. For formulas that can reuse the
    /// forward node output (`exp`, `sqrt`, `abs`), `sig` is loaded as `primal`
    /// so the local transpose uses the recorded forward value rather than a
    /// second computation.
    pub(super) fn propagate_bra_unary_math_adj(
        &mut self,
        rule: RadUnaryMathRule,
        sig: SigId,
        x: SigId,
        y_bar: FirId,
        adj: &mut std::collections::HashMap<SigId, FirId>,
    ) -> Result<(), SignalFirError> {
        let real_ty = self.real_ty.clone();
        let x_fir = self.load_bra_fwd_value(x)?;
        // The shared formula only sees values. For rules whose derivative can
        // reuse the forward output, pass the tape-loaded current node value so
        // the reverse sweep does not recompute non-trivial temporal operands.
        let primal = match rule {
            RadUnaryMathRule::Exp | RadUnaryMathRule::Sqrt | RadUnaryMathRule::Abs => {
                self.load_bra_fwd_value(sig)?
            }
            _ => x_fir,
        };
        let mut b = FirRadFormulaBuilder::new(self, real_ty.clone());
        let x_adj = rad_unary_contribution(&mut b, rule, x_fir, primal, y_bar);
        Self::add_to_adjoint(&mut self.store, adj, x, x_adj, real_ty);
        Ok(())
    }

    /// Emits the BRA reverse updates for a supported binary math node.
    ///
    /// This method is the FIR/BRA counterpart of `propagate_binary_math`: it
    /// loads both forward operand values from BRA storage, lets the shared
    /// `ad_rules` formula build the two local cotangents in FIR, then
    /// accumulates them into the reverse adjoint map. `pow` additionally needs
    /// the stored forward result of `sig` for its exponent contribution; other
    /// binary math rules depend only on the loaded operands and ignore the
    /// `primal` placeholder.
    pub(super) fn propagate_bra_binary_math_adj(
        &mut self,
        rule: RadBinaryMathRule,
        lhs: SigId,
        rhs: SigId,
        sig: SigId,
        y_bar: FirId,
        adj: &mut std::collections::HashMap<SigId, FirId>,
    ) -> Result<(), SignalFirError> {
        let real_ty = self.real_ty.clone();
        let lhs_fir = self.load_bra_fwd_value(lhs)?;
        let rhs_fir = self.load_bra_fwd_value(rhs)?;
        // `pow` needs its forward output for the exponent derivative. Other
        // binary rules compute their local transpose from operand values only,
        // so the placeholder is intentionally ignored by the shared helper.
        let primal = match rule {
            RadBinaryMathRule::Pow => self.load_bra_fwd_value(sig)?,
            _ => lhs_fir,
        };
        let mut b = FirRadFormulaBuilder::new(self, real_ty.clone());
        let (lhs_adj, rhs_adj) =
            rad_binary_contributions(&mut b, rule, lhs_fir, rhs_fir, primal, y_bar);
        Self::add_to_adjoint(&mut self.store, adj, lhs, lhs_adj, real_ty.clone());
        Self::add_to_adjoint(&mut self.store, adj, rhs, rhs_adj, real_ty);
        Ok(())
    }

    /// Returns a clone of the internal real computation type.
    ///
    /// Use this whenever a FIR node must carry the internal scalar precision
    /// (arithmetic result, state slot, math call, real constant, …).
    /// For external interface points (audio buffer samples, UI zone variables)
    /// use `FirType::FaustFloat` directly instead.
    pub(super) fn real_ty(&self) -> FirType {
        self.real_ty.clone()
    }

    // ── Variability-driven statement placement (Phase 1) ──────────────────

    /// Returns the signal-level variability for a node, if type info exists.
    ///
    /// Variability drives the execution-tier placement of the resulting FIR
    /// expression:
    /// - [`Variability::Konst`] → `constants_statements` (once at init)
    /// - [`Variability::Block`] → `control_statements` (once per `compute()`)
    /// - [`Variability::Samp`]  → sample-loop immediate phase
    pub(super) fn variability_of(&self, sig: SigId) -> Option<Variability> {
        self.sig_types.get(&sig).map(|t| t.variability())
    }

    /// Returns `true` when a hoisted `Konst` value must remain persistent
    /// beyond `instanceConstants()`.
    pub(super) fn konst_escapes(&self, sig: SigId) -> bool {
        self.konst_escapes.contains(&sig)
    }

    /// Returns the typed prefix used for one materialized scalar value.
    pub(super) fn typed_prefix_for(bucket: Bucket, typ: &FirType) -> &'static str {
        let is_int_like = matches!(typ, FirType::Int32 | FirType::Int64 | FirType::Bool);
        match (bucket, is_int_like) {
            (Bucket::Constants, true) => "iConst",
            (Bucket::Constants, false) => "fConst",
            (Bucket::Control, true) => "iSlow",
            (Bucket::Control, false) => "fSlow",
        }
    }

    /// Returns the next numeric suffix for one typed materialization prefix.
    pub(super) fn next_materialized_counter(&mut self, prefix: &str) -> u32 {
        match prefix {
            "fConst" => {
                let n = self.fconst_counter;
                self.fconst_counter += 1;
                n
            }
            "iConst" => {
                let n = self.iconst_counter;
                self.iconst_counter += 1;
                n
            }
            "fSlow" => {
                let n = self.fslow_counter;
                self.fslow_counter += 1;
                n
            }
            "iSlow" => {
                let n = self.islow_counter;
                self.islow_counter += 1;
                n
            }
            other => panic!("unsupported materialized prefix `{other}`"),
        }
    }

    /// Returns `true` when the signal is a direct `Proj(i, SYMREC)` read.
    ///
    /// The type system (after the `update_rec_types` variability-join fix)
    /// guarantees that such nodes always carry at least `Samp` variability, so
    /// they would not be hoisted by the placement logic anyway.  This guard is
    /// kept as a defensive check against future regressions.
    pub(super) fn is_recursive_projection(&self, sig: SigId) -> bool {
        if let SigMatch::Proj(_, group) = match_sig(self.arena, sig) {
            let group = match match_sig(self.arena, group) {
                SigMatch::ReverseTimeRec(body) => body,
                _ => group,
            };
            match_sym_rec(self.arena, group).is_some()
                || match_sym_ref(self.arena, group).is_some()
                || tlib::match_de_bruijn_ref(self.arena, group).is_some()
        } else {
            false
        }
    }

    /// Materializes a FIR value expression into a named variable in the
    /// given execution-tier bucket.
    ///
    /// Returns a [`FirId`] for the `LoadVar` that reads the materialized
    /// variable.  The corresponding `DeclareVar` (with initializer) is
    /// appended to the appropriate lifecycle accumulator:
    ///
    /// | Bucket | Prefix | Access | Lifecycle section |
    /// |--------|--------|--------|-------------------|
    /// | `Constants` | `iConst` / `fConst` | [`AccessType::Stack`] for init-local, [`AccessType::Struct`] for escaping values | `instanceConstants` |
    /// | `Control` | `iSlow` / `fSlow` | [`AccessType::Stack`] | `compute` preamble |
    ///
    /// `Konst` variables that feed `compute()` use struct storage because they
    /// are written in `instanceConstants()` and read later; init-only `Konst`
    /// temporaries and all `Block` variables stay stack-local.
    pub(super) fn materialize_in_bucket(
        &mut self,
        sig: SigId,
        value: FirId,
        bucket: Bucket,
    ) -> FirId {
        let typ = self
            .store
            .value_type(value)
            .unwrap_or_else(|| self.real_ty());
        let prefix = Self::typed_prefix_for(bucket, &typ);
        let n = self.next_materialized_counter(prefix);
        let access = match bucket {
            Bucket::Constants if self.konst_escapes(sig) => AccessType::Struct,
            Bucket::Constants | Bucket::Control => AccessType::Stack,
        };
        let name = format!("{prefix}{n}");

        match bucket {
            Bucket::Constants if access == AccessType::Struct => {
                self.ensure_named_struct_var(&name, typ.clone(), None);
                let mut b = FirBuilder::new(&mut self.store);
                self.constants_statements
                    .push(b.store_var(&name, AccessType::Struct, value));
            }
            Bucket::Constants => {
                let mut b = FirBuilder::new(&mut self.store);
                self.constants_statements.push(b.declare_var(
                    &name,
                    typ.clone(),
                    AccessType::Stack,
                    Some(value),
                ));
            }
            Bucket::Control => {
                let mut b = FirBuilder::new(&mut self.store);
                self.control_statements.push(b.declare_var(
                    &name,
                    typ.clone(),
                    AccessType::Stack,
                    Some(value),
                ));
            }
        }

        let mut b = FirBuilder::new(&mut self.store);
        b.load_var(name, access, typ)
    }

    /// Returns the reduced prepared signal type attached to one signal node.
    ///
    /// The fast-lane relies on the pre-FIR `signal_prepare` boundary to decide
    /// whether one value/state/table should stay integer or use the internal
    /// real computation type, mirroring the reduced
    /// `deBruijn2Sym -> typeAnnotation -> signalPromote` contract.
    pub(super) fn simple_type(&self, sig: SigId) -> Result<SimpleSigType, SignalFirError> {
        self.types.get(&sig).copied().ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("missing prepared type for signal {}", sig.as_u32()),
            )
        })
    }

    /// Maps one prepared signal type to the FIR value type used by lowering.
    pub(super) fn signal_fir_type(&self, sig: SigId) -> Result<FirType, SignalFirError> {
        match self.simple_type(sig)? {
            SimpleSigType::Int => Ok(FirType::Int32),
            SimpleSigType::Real => Ok(self.real_ty()),
            SimpleSigType::Sound => Ok(FirType::Sound),
        }
    }

    /// Returns the typed zero initializer used for state slots and table
    /// declarations.
    pub(super) fn zero_value_for_signal(&mut self, sig: SigId) -> Result<FirId, SignalFirError> {
        match self.simple_type(sig)? {
            SimpleSigType::Int => Ok(self.lower_int32_const(0)),
            SimpleSigType::Real => Ok(self.float_const(0.0)),
            SimpleSigType::Sound => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "signal {} cannot use a soundfile handle as delay/table state",
                    sig.as_u32()
                ),
            )),
        }
    }
}
