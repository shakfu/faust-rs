//! UI control lowering, metadata emission, and `buildUserInterface` assembly.
//!
//! Manages the UI side of the Faust lifecycle:
//! - zone variable declaration for buttons, checkboxes, sliders, bargraphs,
//!   and soundfile controls;
//! - `addMetaDeclare` emission for per-control metadata key/value pairs;
//! - `emit_ui_program` — the recursive walk that emits the full
//!   `buildUserInterface(ui)` body from the [`UiProgram`] tree.
//!
//! All methods operate on the shared [`SignalToFirLower`] state and write
//! directly into `self.ui.ui_statements` for final assembly by `build_module`.

use super::*;

/// UI and table lowering accumulators.
#[derive(Default)]
pub(super) struct UiLoweringState {
    /// Maps each `ControlId` to its generated `FaustFloat` zone variable name.
    pub(super) ui_controls: HashMap<ControlId, String>,
    /// Maps each soundfile `ControlId` to its generated opaque zone variable name.
    pub(super) soundfiles: HashMap<ControlId, String>,
    /// Maps each waveform/table signal to its generated table variable name.
    pub(super) waveform_tables: HashMap<SigId, String>,
    /// Maps each waveform/table signal to its element count.
    pub(super) waveform_table_len: HashMap<SigId, usize>,
    /// Maps each waveform/table signal to the FIR storage class used for access.
    pub(super) table_access_by_sig: HashMap<SigId, AccessType>,
    /// `buildUserInterface` body: open/close box and add-control calls.
    pub(super) ui_statements: Vec<FirId>,
}

impl<'a> SignalToFirLower<'a> {
    /// Declares the `FaustFloat` struct zone variable for a button or checkbox, idempotent.
    pub(super) fn ensure_button_zone(
        &mut self,
        control: ControlId,
        typ: ButtonType,
    ) -> Result<String, SignalFirError> {
        if let Some(var) = self.ui.ui_controls.get(&control).cloned() {
            return Ok(var);
        }
        let spec = self.control_spec(control)?;
        let expected_kind = match typ {
            ButtonType::Button => ControlKind::Button,
            ButtonType::Checkbox => ControlKind::Checkbox,
        };
        if spec.kind != expected_kind {
            return Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "control id {control} kind mismatch: expected {expected_kind:?}, got {:?}",
                    spec.kind
                ),
            ));
        }
        let var = self.ui_control_var_name(
            control,
            match typ {
                ButtonType::Button => "fButton",
                ButtonType::Checkbox => "fCheckbox",
            },
        );
        let init = self.float_const(0.0);
        self.ensure_named_struct_var(&var, FirType::FaustFloat, Some(init));
        self.ui.ui_controls.insert(control, var.clone());
        Ok(var)
    }

    /// Lowers button/checkbox UI controls as zone-backed struct variables.
    pub(super) fn lower_button(
        &mut self,
        control: ControlId,
        typ: ButtonType,
    ) -> Result<FirId, SignalFirError> {
        let var = self.ensure_button_zone(control, typ)?;
        if self.ui.ui_controls.contains_key(&control) {
            // UI zone variable is FaustFloat (external); cast to real_ty for computation.
            let real_ty = self.real_ty();
            let mut b = FirBuilder::new(&mut self.store);
            let load = b.load_var(var, AccessType::Struct, FirType::FaustFloat);
            return Ok(b.cast(real_ty, load));
        }
        unreachable!("button zone should be inserted before loading")
    }

    /// Lowers slider-style UI controls and records metadata in
    /// `buildUserInterface`.
    pub(super) fn lower_slider(
        &mut self,
        control: ControlId,
        typ: SliderType,
    ) -> Result<FirId, SignalFirError> {
        let var = self.ensure_slider_zone(control, typ)?;
        if self.ui.ui_controls.contains_key(&control) {
            // UI zone variable is FaustFloat (external); cast to real_ty for computation.
            let real_ty = self.real_ty();
            let mut b = FirBuilder::new(&mut self.store);
            let load = b.load_var(var, AccessType::Struct, FirType::FaustFloat);
            return Ok(b.cast(real_ty, load));
        }
        unreachable!("slider zone should be inserted before loading")
    }

    /// Lowers bargraph UI nodes by creating UI descriptors and storing incoming
    /// runtime value in a dedicated control zone.
    pub(super) fn lower_bargraph(
        &mut self,
        control: ControlId,
        value: SigId,
        typ: BargraphType,
    ) -> Result<FirId, SignalFirError> {
        let _ = self.ensure_bargraph_zone(control, typ)?;
        // The incoming signal value is computed at internal real precision; cast
        // it to FaustFloat before writing to the external zone variable.
        let value = self.lower_signal(value)?;
        let var = self
            .ui
            .ui_controls
            .get(&control)
            .cloned()
            .expect("bargraph variable should exist after declaration");
        let mut b = FirBuilder::new(&mut self.store);
        let faust_value = b.cast(FirType::FaustFloat, value);
        self.sample_phases
            .immediate
            .push(b.store_var(var, AccessType::Struct, faust_value));
        Ok(value)
    }

    /// Lowers a soundfile declaration into UI-only registration and an opaque
    /// struct-backed runtime handle.
    pub(super) fn lower_soundfile(&mut self, control: ControlId) -> Result<FirId, SignalFirError> {
        let var = self.ensure_soundfile_zone(control)?;
        if self.ui.soundfiles.contains_key(&control) {
            let mut b = FirBuilder::new(&mut self.store);
            return Ok(b.load_var(var, AccessType::Struct, FirType::Sound));
        }
        unreachable!("soundfile zone should be inserted before loading")
    }

    /// Extracts the var name from a `SIGSOUNDFILE` signal node.
    pub(super) fn soundfile_var_from_signal(
        &mut self,
        sf: SigId,
    ) -> Result<String, SignalFirError> {
        match match_sig(self.arena, sf) {
            SigMatch::Soundfile(control) => self.ensure_soundfile_zone(control),
            _ => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "expected SIGSOUNDFILE node, got {}",
                    dump_sig_readable(self.arena, sf)
                ),
            )),
        }
    }

    /// Lowers `SIGSOUNDFILELENGTH(sf, part)` → `fSoundN->fLength[part]`.
    pub(super) fn lower_soundfile_length(
        &mut self,
        sf: SigId,
        part: SigId,
    ) -> Result<FirId, SignalFirError> {
        let var = self.soundfile_var_from_signal(sf)?;
        let part = self.lower_signal(part)?;
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.load_soundfile_length(var, part))
    }

    /// Lowers `SIGSOUNDFILERATE(sf, part)` → `fSoundN->fSR[part]`.
    pub(super) fn lower_soundfile_rate(
        &mut self,
        sf: SigId,
        part: SigId,
    ) -> Result<FirId, SignalFirError> {
        let var = self.soundfile_var_from_signal(sf)?;
        let part = self.lower_signal(part)?;
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.load_soundfile_rate(var, part))
    }

    /// Lowers `SIGSOUNDFILEBUFFER(sf, chan, part, ridx)` →
    /// `((FAUSTFLOAT**)fSoundN->fBuffers)[chan][fSoundN->fOffset[part] + ridx]`.
    pub(super) fn lower_soundfile_buffer(
        &mut self,
        node: SigId,
        sf: SigId,
        chan: SigId,
        part: SigId,
        ridx: SigId,
    ) -> Result<FirId, SignalFirError> {
        let var = self.soundfile_var_from_signal(sf)?;
        let chan = self.lower_signal(chan)?;
        let part = self.lower_signal(part)?;
        let idx = self.lower_signal(ridx)?;
        let typ = self.signal_fir_type(node)?;
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.load_soundfile_buffer(var, chan, part, idx, typ))
    }

    /// Converts a label signal node to UTF-8 text fallback used by foreign refs.
    pub(super) fn label_text(&self, label: SigId) -> String {
        match self.arena.kind(label) {
            Some(NodeKind::Symbol(s)) => s.to_string(),
            Some(NodeKind::StringLiteral(s)) => s.to_string(),
            Some(NodeKind::Int(v)) => v.to_string(),
            Some(NodeKind::FloatBits(bits)) => f64::from_bits(*bits).to_string(),
            _ => "ui".to_owned(),
        }
    }

    /// Stable generated UI zone variable naming policy.
    pub(super) fn ui_control_var_name(&self, control: ControlId, prefix: &str) -> String {
        format!("{prefix}{control}")
    }

    /// Looks up the `ControlSpec` for `control`, returning an error if missing.
    pub(super) fn control_spec(
        &self,
        control: ControlId,
    ) -> Result<&ui::ControlSpec, SignalFirError> {
        self.ui_program.control(control).ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("missing UiProgram control spec for control id {control}"),
            )
        })
    }

    /// Returns the numeric range for `control`, returning an error if absent.
    ///
    /// `kind_name` is included in the error message for diagnostics only.
    pub(super) fn control_range(
        &self,
        control: ControlId,
        kind_name: &str,
    ) -> Result<ui::ControlRange, SignalFirError> {
        self.control_spec(control)?.range.ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("missing UI range for {kind_name} control id {control}"),
            )
        })
    }

    /// Emits `addMetaDeclare(var, key, value)` calls for each metadata pair.
    pub(super) fn emit_ui_metadata_for_target(&mut self, var: &str, metadata: &[(String, String)]) {
        for (key, value) in metadata {
            let mut b = FirBuilder::new(&mut self.store);
            self.ui
                .ui_statements
                .push(b.add_meta_declare(var, key.clone(), value.clone()));
        }
    }

    /// Looks up one metadata value by key for the given control, if present.
    pub(super) fn control_metadata_value(
        &self,
        control: ControlId,
        key: &str,
    ) -> Result<Option<String>, SignalFirError> {
        Ok(self
            .control_spec(control)?
            .metadata
            .iter()
            .find_map(|(entry_key, entry_value)| (entry_key == key).then(|| entry_value.clone())))
    }

    /// Emits `addMetaDeclare` calls for every metadata entry attached to `control`.
    pub(super) fn emit_control_ui_metadata(
        &mut self,
        control: ControlId,
        var: &str,
    ) -> Result<(), SignalFirError> {
        let metadata = self.control_spec(control)?.metadata.clone();
        self.emit_ui_metadata_for_target(var, &metadata);
        Ok(())
    }

    /// Declares the `FaustFloat` struct zone variable for a slider or numentry, idempotent.
    pub(super) fn ensure_slider_zone(
        &mut self,
        control: ControlId,
        typ: SliderType,
    ) -> Result<String, SignalFirError> {
        if let Some(var) = self.ui.ui_controls.get(&control).cloned() {
            return Ok(var);
        }
        let spec = self.control_spec(control)?;
        let expected_kind = match typ {
            SliderType::Horizontal => ControlKind::HSlider,
            SliderType::Vertical => ControlKind::VSlider,
            SliderType::NumEntry => ControlKind::NumEntry,
        };
        if spec.kind != expected_kind {
            return Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "control id {control} kind mismatch: expected {expected_kind:?}, got {:?}",
                    spec.kind
                ),
            ));
        }
        let var = self.ui_control_var_name(
            control,
            match typ {
                SliderType::Horizontal => "fHslider",
                SliderType::Vertical => "fVslider",
                SliderType::NumEntry => "fEntry",
            },
        );
        let range = self.control_range(
            control,
            match typ {
                SliderType::Horizontal => "hslider",
                SliderType::Vertical => "vslider",
                SliderType::NumEntry => "numentry",
            },
        )?;
        let init = self.float_const(range.init);
        self.ensure_named_struct_var(&var, FirType::FaustFloat, Some(init));
        self.ui.ui_controls.insert(control, var.clone());
        Ok(var)
    }

    /// Declares the `FaustFloat` struct zone variable for a bargraph, idempotent.
    pub(super) fn ensure_bargraph_zone(
        &mut self,
        control: ControlId,
        typ: BargraphType,
    ) -> Result<String, SignalFirError> {
        if let Some(var) = self.ui.ui_controls.get(&control).cloned() {
            return Ok(var);
        }
        let spec = self.control_spec(control)?;
        let expected_kind = match typ {
            BargraphType::Horizontal => ControlKind::HBargraph,
            BargraphType::Vertical => ControlKind::VBargraph,
        };
        if spec.kind != expected_kind {
            return Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "control id {control} kind mismatch: expected {expected_kind:?}, got {:?}",
                    spec.kind
                ),
            ));
        }
        let var = self.ui_control_var_name(
            control,
            match typ {
                BargraphType::Horizontal => "fHbargraph",
                BargraphType::Vertical => "fVbargraph",
            },
        );
        let init = self.float_const(0.0);
        self.ensure_named_struct_var(&var, FirType::FaustFloat, Some(init));
        self.ui.ui_controls.insert(control, var.clone());
        Ok(var)
    }

    /// Declares the opaque `Sound` struct zone variable for a soundfile, idempotent.
    pub(super) fn ensure_soundfile_zone(
        &mut self,
        control: ControlId,
    ) -> Result<String, SignalFirError> {
        if let Some(var) = self.ui.soundfiles.get(&control).cloned() {
            return Ok(var);
        }
        let spec = self.control_spec(control)?;
        if spec.kind != ControlKind::Soundfile {
            return Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "control id {control} kind mismatch: expected {:?}, got {:?}",
                    ControlKind::Soundfile,
                    spec.kind
                ),
            ));
        }
        let var = format!("fSound{control}");
        self.ensure_named_struct_var(&var, FirType::Sound, None);
        self.ui.soundfiles.insert(control, var.clone());
        Ok(var)
    }

    /// Drives emission of the entire `buildUserInterface` body from the root UI node.
    ///
    /// Clears any previous `ui_statements` accumulator before walking the tree.
    pub(super) fn emit_ui_program(&mut self) -> Result<(), SignalFirError> {
        if self.ui_program.is_empty() {
            self.ui.ui_statements.clear();
            return Ok(());
        }
        self.ui.ui_statements.clear();
        self.emit_ui_node(self.ui_program.root)
    }

    /// Recursively emits FIR UI calls for one UI tree node.
    ///
    /// Dispatches on group containers (open/close box), input controls
    /// (button, checkbox, slider, numentry), output controls (bargraph),
    /// and soundfile declarations.
    pub(super) fn emit_ui_node(&mut self, node: ui::UiId) -> Result<(), SignalFirError> {
        match match_ui(&self.ui_program.arena, node) {
            UiMatch::Group {
                kind,
                label,
                metadata,
                children,
            } => {
                let typ = match kind {
                    UiGroupKind::Vertical => UiBoxType::Vertical,
                    UiGroupKind::Horizontal => UiBoxType::Horizontal,
                    UiGroupKind::Tab => UiBoxType::Tab,
                };
                self.emit_ui_metadata_for_target("0", &metadata);
                let mut b = FirBuilder::new(&mut self.store);
                self.ui.ui_statements.push(b.open_box(typ, label));
                for child in children {
                    self.emit_ui_node(child)?;
                }
                let mut b = FirBuilder::new(&mut self.store);
                self.ui.ui_statements.push(b.close_box());
                Ok(())
            }
            UiMatch::InputControl(control) => {
                let spec = self.control_spec(control)?;
                let kind = spec.kind;
                let label = spec.label.clone();
                match kind {
                    ControlKind::Button => {
                        let var = self.ensure_button_zone(control, ButtonType::Button)?;
                        self.emit_control_ui_metadata(control, &var)?;
                        let mut b = FirBuilder::new(&mut self.store);
                        self.ui
                            .ui_statements
                            .push(b.add_button(ButtonType::Button, label, var));
                    }
                    ControlKind::Checkbox => {
                        let var = self.ensure_button_zone(control, ButtonType::Checkbox)?;
                        self.emit_control_ui_metadata(control, &var)?;
                        let mut b = FirBuilder::new(&mut self.store);
                        self.ui
                            .ui_statements
                            .push(b.add_button(ButtonType::Checkbox, label, var));
                    }
                    ControlKind::VSlider => {
                        let range = self.control_range(control, "vslider")?;
                        let var = self.ensure_slider_zone(control, SliderType::Vertical)?;
                        self.emit_control_ui_metadata(control, &var)?;
                        let mut b = FirBuilder::new(&mut self.store);
                        self.ui.ui_statements.push(b.add_slider(
                            SliderType::Vertical,
                            label,
                            var,
                            SliderRange {
                                init: range.init,
                                lo: range.min,
                                hi: range.max,
                                step: range.step,
                            },
                        ));
                    }
                    ControlKind::HSlider => {
                        let range = self.control_range(control, "hslider")?;
                        let var = self.ensure_slider_zone(control, SliderType::Horizontal)?;
                        self.emit_control_ui_metadata(control, &var)?;
                        let mut b = FirBuilder::new(&mut self.store);
                        self.ui.ui_statements.push(b.add_slider(
                            SliderType::Horizontal,
                            label,
                            var,
                            SliderRange {
                                init: range.init,
                                lo: range.min,
                                hi: range.max,
                                step: range.step,
                            },
                        ));
                    }
                    ControlKind::NumEntry => {
                        let range = self.control_range(control, "numentry")?;
                        let var = self.ensure_slider_zone(control, SliderType::NumEntry)?;
                        self.emit_control_ui_metadata(control, &var)?;
                        let mut b = FirBuilder::new(&mut self.store);
                        self.ui.ui_statements.push(b.add_slider(
                            SliderType::NumEntry,
                            label,
                            var,
                            SliderRange {
                                init: range.init,
                                lo: range.min,
                                hi: range.max,
                                step: range.step,
                            },
                        ));
                    }
                    other => {
                        return Err(SignalFirError::new(
                            SignalFirErrorCode::UnsupportedSignalNode,
                            format!("input UI leaf points to non-input control kind {other:?}"),
                        ));
                    }
                }
                Ok(())
            }
            UiMatch::OutputControl(control) => {
                let spec = self.control_spec(control)?;
                let kind = spec.kind;
                let label = spec.label.clone();
                match kind {
                    ControlKind::VBargraph => {
                        let range = self.control_range(control, "vbargraph")?;
                        let var = self.ensure_bargraph_zone(control, BargraphType::Vertical)?;
                        self.emit_control_ui_metadata(control, &var)?;
                        let mut b = FirBuilder::new(&mut self.store);
                        self.ui.ui_statements.push(b.add_bargraph(
                            BargraphType::Vertical,
                            label,
                            var,
                            range.min,
                            range.max,
                        ));
                    }
                    ControlKind::HBargraph => {
                        let range = self.control_range(control, "hbargraph")?;
                        let var = self.ensure_bargraph_zone(control, BargraphType::Horizontal)?;
                        self.emit_control_ui_metadata(control, &var)?;
                        let mut b = FirBuilder::new(&mut self.store);
                        self.ui.ui_statements.push(b.add_bargraph(
                            BargraphType::Horizontal,
                            label,
                            var,
                            range.min,
                            range.max,
                        ));
                    }
                    other => {
                        return Err(SignalFirError::new(
                            SignalFirErrorCode::UnsupportedSignalNode,
                            format!("output UI leaf points to non-bargraph control kind {other:?}"),
                        ));
                    }
                }
                Ok(())
            }
            UiMatch::Soundfile(control) => {
                let label = self.control_spec(control)?.label.clone();
                let url = self
                    .control_metadata_value(control, "url")?
                    .unwrap_or_default();
                let var = self.ensure_soundfile_zone(control)?;
                let mut b = FirBuilder::new(&mut self.store);
                self.ui
                    .ui_statements
                    .push(b.add_soundfile_with_url(label, url, var));
                Ok(())
            }
            UiMatch::Unknown => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                "malformed UiProgram node".to_owned(),
            )),
        }
    }
}
