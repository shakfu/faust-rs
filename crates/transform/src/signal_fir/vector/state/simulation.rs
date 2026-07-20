//! Executable reference state machines used by bounded simulation checks
//! (copy/ring delay lines, recursion step commit).

use super::model::*;

/// Abstract newest-first history transition from the formal port plan.
pub fn history_step<T: Clone>(history: &mut Vec<T>, current: T) {
    if history.is_empty() {
        return;
    }
    history.pop();
    history.insert(0, current);
}

/// Abstract `delayRead`: delay zero is current, delay `n>0` is history `n-1`.
#[must_use]
pub fn delay_read<'a, T>(history: &'a [T], current: &'a T, delay: usize) -> Option<&'a T> {
    if delay == 0 {
        Some(current)
    } else {
        history.get(delay - 1)
    }
}

/// C++ short-delay concrete state used by bounded `DelaySim` checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyDelayState<T> {
    max_delay: usize,
    vec_size: usize,
    permanent: Vec<T>,
}

impl<T: Clone> CopyDelayState<T> {
    pub fn new(
        storage: &VectorDelayStorage,
        max_delay: usize,
        fill: T,
    ) -> Result<Self, VectorStateError> {
        let VectorDelayStorage::Copy {
            history_length,
            temporary_length,
            ..
        } = storage
        else {
            return Err(VectorStateError::SimulationGeometryMismatch);
        };
        let history_length = usize::try_from(*history_length)
            .map_err(|_| VectorStateError::SimulationGeometryMismatch)?;
        let temporary_length = usize::try_from(*temporary_length)
            .map_err(|_| VectorStateError::SimulationGeometryMismatch)?;
        let vec_size = temporary_length
            .checked_sub(history_length)
            .ok_or(VectorStateError::SimulationGeometryMismatch)?;
        if history_length < max_delay {
            return Err(VectorStateError::SimulationGeometryMismatch);
        }
        Ok(Self {
            max_delay,
            vec_size,
            permanent: vec![fill; history_length],
        })
    }

    pub fn process_chunk(
        &mut self,
        input: &[T],
        delays: &[usize],
    ) -> Result<Vec<Vec<T>>, VectorStateError> {
        validate_simulation_request(input.len(), self.vec_size, delays, self.max_delay)?;
        let history_length = self.permanent.len();
        let mut temporary = self.permanent.clone();
        if let Some(fill) = self.permanent.first().cloned() {
            temporary.resize(history_length + self.vec_size, fill);
        } else {
            return Err(VectorStateError::SimulationGeometryMismatch);
        }
        let mut output = Vec::with_capacity(input.len());
        for (sample, value) in input.iter().enumerate() {
            let write = history_length + sample;
            temporary[write] = value.clone();
            output.push(
                delays
                    .iter()
                    .map(|delay| temporary[write - delay].clone())
                    .collect(),
            );
        }
        self.permanent
            .clone_from_slice(&temporary[input.len()..input.len() + history_length]);
        Ok(output)
    }

    /// Abstraction function `alpha`: newest-first semantic history.
    #[must_use]
    pub fn abstract_history(&self) -> Vec<T> {
        self.permanent[self.permanent.len() - self.max_delay..]
            .iter()
            .rev()
            .cloned()
            .collect()
    }
}

/// C++ long-delay concrete state used by bounded `DelaySim` checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RingDelayState<T> {
    max_delay: usize,
    vec_size: usize,
    memory: Vec<T>,
    index: usize,
    index_save: usize,
}

impl<T: Clone> RingDelayState<T> {
    pub fn new(
        storage: &VectorDelayStorage,
        max_delay: usize,
        vec_size: usize,
        fill: T,
    ) -> Result<Self, VectorStateError> {
        let VectorDelayStorage::Ring { capacity, mask, .. } = storage else {
            return Err(VectorStateError::SimulationGeometryMismatch);
        };
        let capacity =
            usize::try_from(*capacity).map_err(|_| VectorStateError::SimulationGeometryMismatch)?;
        if capacity == 0
            || !capacity.is_power_of_two()
            || *mask != u64::try_from(capacity - 1).expect("capacity fits u64")
            || capacity < max_delay.saturating_add(vec_size)
        {
            return Err(VectorStateError::SimulationGeometryMismatch);
        }
        Ok(Self {
            max_delay,
            vec_size,
            memory: vec![fill; capacity],
            index: 0,
            index_save: 0,
        })
    }

    pub fn process_chunk(
        &mut self,
        input: &[T],
        delays: &[usize],
    ) -> Result<Vec<Vec<T>>, VectorStateError> {
        validate_simulation_request(input.len(), self.vec_size, delays, self.max_delay)?;
        let mask = self.memory.len() - 1;
        self.index = (self.index + self.index_save) & mask;
        let mut output = Vec::with_capacity(input.len());
        for (sample, value) in input.iter().enumerate() {
            let write = (self.index + sample) & mask;
            self.memory[write] = value.clone();
            output.push(
                delays
                    .iter()
                    .map(|delay| self.memory[write.wrapping_sub(*delay) & mask].clone())
                    .collect(),
            );
        }
        self.index_save = input.len();
        Ok(output)
    }

    /// Abstraction function `alpha`: newest-first semantic history.
    #[must_use]
    pub fn abstract_history(&self) -> Vec<T> {
        let mask = self.memory.len() - 1;
        let next = (self.index + self.index_save) & mask;
        (1..=self.max_delay)
            .map(|delay| self.memory[next.wrapping_sub(delay) & mask].clone())
            .collect()
    }
}

pub(super) fn validate_simulation_request(
    count: usize,
    vec_size: usize,
    delays: &[usize],
    max_delay: usize,
) -> Result<(), VectorStateError> {
    if count > vec_size {
        return Err(VectorStateError::SimulationChunkTooLarge { count, vec_size });
    }
    if let Some(&delay) = delays.iter().find(|&&delay| delay > max_delay) {
        return Err(VectorStateError::SimulationDelayOutOfRange { delay, max_delay });
    }
    Ok(())
}

/// Commits one simultaneous recursion transition and returns the old tuple.
pub fn commit_recursion_step<T>(
    state: &mut Vec<T>,
    next: Vec<T>,
) -> Result<Vec<T>, VectorStateError> {
    if state.len() != next.len() {
        return Err(VectorStateError::RecursionArityMismatch {
            state: state.len(),
            next: next.len(),
        });
    }
    Ok(std::mem::replace(state, next))
}
