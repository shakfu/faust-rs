//! `FbcDspInstance<R>` — runtime DSP instance with heaps and compute loop.
//!
//! # Source provenance (C++)
//! - `interpreter_dsp_aux<REAL, TRACE>` in `interpreter_dsp_aux.hh`
//!
//! # Design notes
//! - Holds a reference to its parent [`FbcDspFactory`] and owns an
//!   [`FbcExecutor`] with heaps sized from the factory.
//! - Lifecycle: `new()` → `init(sr)` → `compute()` loop.
//! - The factory must be optimized before creating an instance; this is
//!   enforced by requiring `&mut FbcDspFactory` in `new()`.
//! - No `TRACE` template parameter — tracing is a future runtime option.

use super::executor::{FbcExecError, FbcExecutor};
use super::factory::FbcDspFactory;
use super::real::FbcReal;

/// Runtime DSP instance with its own heaps.
///
/// # Source provenance (C++)
/// - `interpreter_dsp_aux<REAL, TRACE>` in `interpreter_dsp_aux.hh`
///
/// # Lifetime
/// The instance borrows the factory for the duration of its lifetime.
/// The factory must outlive all its instances.
///
/// The split between [`FbcDspFactory`] and [`FbcExecutor`] mirrors the C++
/// interpreter design:
/// - the factory owns immutable bytecode and metadata,
/// - the instance owns mutable heaps and lifecycle state.
pub struct FbcDspInstance<'a, R: FbcReal> {
    factory: &'a FbcDspFactory<R>,
    executor: FbcExecutor<R>,
    initialized: bool,
    cycle: usize,
}

/// Structured runtime error returned by [`FbcDspInstance::try_compute`].
///
/// This currently wraps executor failures only, but keeping a dedicated
/// instance-level error type leaves room for future lifecycle or host-I/O
/// validation errors without changing the public compute signature.
#[derive(Debug)]
pub enum FbcDspRuntimeError {
    /// Bytecode executor reported a structured execution failure.
    Exec(FbcExecError),
}

impl std::fmt::Display for FbcDspRuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exec(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for FbcDspRuntimeError {}

impl From<FbcExecError> for FbcDspRuntimeError {
    fn from(value: FbcExecError) -> Self {
        Self::Exec(value)
    }
}

impl<'a, R: FbcReal> FbcDspInstance<'a, R> {
    /// Creates a new DSP instance from a factory.
    ///
    /// The factory is optimized (if not already) before the instance is created.
    /// The executor is allocated with heap sizes from the factory.
    ///
    /// Requiring `&mut FbcDspFactory` preserves the one-shot optimization
    /// contract before instances start borrowing immutable factory state.
    ///
    /// # Source provenance (C++)
    /// - `interpreter_dsp_aux(interpreter_dsp_factory_aux*)` constructor
    ///   in `interpreter_dsp_aux.hh` (lines 536–545).
    #[must_use]
    pub fn new(factory: &'a mut FbcDspFactory<R>) -> Self {
        // Done before createFBCExecutor that may compile blocks...
        factory.optimize();

        let executor = FbcExecutor::new(
            factory.int_heap_size as usize,
            factory.real_heap_size as usize,
        );

        Self {
            factory,
            executor,
            initialized: false,
            cycle: 0,
        }
    }

    /// Full initialization entrypoint used by the public DSP lifecycle.
    ///
    /// This marks the instance initialized, then runs the canonical
    /// `instance_init` sequence (`classInit`, constants, UI reset, clear).
    ///
    /// # Source provenance (C++)
    /// - `interpreter_dsp_aux::init()` in `interpreter_dsp_aux.hh` (lines 653–668).
    pub fn init(&mut self, sample_rate: i32) {
        self.initialized = true;
        self.instance_init(sample_rate);
    }

    /// Instance initialization: `class_init + constants + reset UI + clear`.
    ///
    /// This matches the Faust DSP lifecycle contract rather than merely setting
    /// heap defaults: stateful tables, controls, and delay memory are all
    /// re-established here.
    ///
    /// # Source provenance (C++)
    /// - `interpreter_dsp_aux::instanceInit()` in `interpreter_dsp_aux.hh`
    ///   (lines 637–651).
    pub fn instance_init(&mut self, sample_rate: i32) {
        // classInit has to be called for each instance since the tables are
        // actually not shared between instances.
        self.class_init(sample_rate);
        self.instance_constants(sample_rate);
        self.instance_reset_user_interface();
        self.instance_clear();
    }

    /// Executes the static init block.
    ///
    /// # Source provenance (C++)
    /// - `interpreter_dsp_aux::classInit()` in `interpreter_dsp_aux.hh`
    ///   (lines 570–584).
    pub fn class_init(&mut self, _sample_rate: i32) {
        self.executor
            .execute_block(&self.factory.arena, self.factory.static_init_block);
    }

    /// Sets sample rate and executes the init (constants) block.
    ///
    /// # Source provenance (C++)
    /// - `interpreter_dsp_aux::instanceConstants()` in `interpreter_dsp_aux.hh`
    ///   (lines 586–603).
    pub fn instance_constants(&mut self, sample_rate: i32) {
        // Store sample_rate in 'fSampleRate' variable at correct offset in fIntHeap.
        self.executor.int_heap[self.factory.sr_offset as usize] = sample_rate;

        self.executor
            .execute_block(&self.factory.arena, self.factory.init_block);
    }

    /// Executes the reset UI block (sets UI controls to default values).
    ///
    /// # Source provenance (C++)
    /// - `interpreter_dsp_aux::instanceResetUserInterface()` in
    ///   `interpreter_dsp_aux.hh` (lines 605–619).
    pub fn instance_reset_user_interface(&mut self) {
        self.executor
            .execute_block(&self.factory.arena, self.factory.reset_ui_block);
    }

    /// Executes the clear block (zeros delay lines, state variables).
    ///
    /// # Source provenance (C++)
    /// - `interpreter_dsp_aux::instanceClear()` in `interpreter_dsp_aux.hh`
    ///   (lines 621–635).
    pub fn instance_clear(&mut self) {
        self.executor
            .execute_block(&self.factory.arena, self.factory.clear_block);
    }

    /// Processes one buffer of audio samples.
    ///
    /// # Arguments
    /// - `count`: number of frames to compute.
    /// - `inputs`: audio input buffers (`inputs[channel][frame]`).
    /// - `outputs`: audio output buffers (`outputs[channel][frame]`).
    ///
    /// # Source provenance (C++)
    /// - `interpreter_dsp_aux::compute()` in `interpreter_dsp_aux.hh`
    ///   (lines 706–790).
    ///
    /// The interpreter keeps the historical two-stage compute split:
    /// - a per-buffer control block,
    /// - then the DSP/sample-loop block with actual audio I/O.
    pub fn compute(&mut self, count: i32, inputs: &[&[R]], outputs: &mut [&mut [R]]) {
        if count == 0 {
            return; // Beware: compiled loop does not work with an index of 0.
        }

        // Set count in 'count' variable at the correct offset in fIntHeap.
        self.executor.int_heap[self.factory.count_offset as usize] = count;

        // Executes the 'control' block.
        self.executor
            .execute_block(&self.factory.arena, self.factory.compute_block);

        // Executes the 'DSP' block (with audio I/O).
        self.executor.execute_block_io(
            &self.factory.arena,
            self.factory.compute_dsp_block,
            inputs,
            outputs,
        );

        self.cycle += 1;
    }

    /// Processes one buffer of audio samples and returns a structured runtime
    /// error for detected execution failures.
    ///
    /// Unlike [`Self::compute`], this surface is intended for tests and hosts
    /// that want to handle malformed/generated-invalid bytecode without panic.
    pub fn try_compute(
        &mut self,
        count: i32,
        inputs: &[&[R]],
        outputs: &mut [&mut [R]],
    ) -> Result<(), FbcDspRuntimeError> {
        if count == 0 {
            return Ok(()); // Beware: compiled loop does not work with an index of 0.
        }

        // Set count in 'count' variable at the correct offset in fIntHeap.
        self.executor.int_heap[self.factory.count_offset as usize] = count;

        // Executes the 'control' block.
        self.executor
            .execute_block(&self.factory.arena, self.factory.compute_block);

        // Executes the 'DSP' block (with audio I/O).
        self.executor.try_execute_block_io(
            &self.factory.arena,
            self.factory.compute_dsp_block,
            inputs,
            outputs,
        )?;

        self.cycle += 1;
        Ok(())
    }

    /// Returns the current sample rate.
    ///
    /// # Source provenance (C++)
    /// - `interpreter_dsp_aux::getSampleRate()` in `interpreter_dsp_aux.hh`.
    #[must_use]
    pub fn get_sample_rate(&self) -> i32 {
        self.executor.int_heap[self.factory.sr_offset as usize]
    }

    /// Returns the number of audio inputs.
    #[must_use]
    pub fn get_num_inputs(&self) -> i32 {
        self.factory.num_inputs
    }

    /// Returns the number of audio outputs.
    #[must_use]
    pub fn get_num_outputs(&self) -> i32 {
        self.factory.num_outputs
    }

    /// Returns whether `init()` has been called.
    #[must_use]
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Returns the compute cycle counter.
    #[must_use]
    pub fn cycle(&self) -> usize {
        self.cycle
    }
}


#[cfg(test)]
mod tests;
