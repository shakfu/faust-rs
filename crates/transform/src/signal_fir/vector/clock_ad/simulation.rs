//! Executable reference simulators for clock steps and reverse-AD windows.
//! These are reference models, not lowering helpers.

use super::model::*;

/// Runtime clock value used by the executable `ClockStep` reference model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockValue {
    /// Boolean clock driving a `BooleanOnDemand` guard.
    Boolean(bool),
    /// Integer clock driving counted or downsample-modulo guards.
    Integer(i64),
}
/// Minimal concrete state needed to test fire-time and held-output semantics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClockRuntime<S, O> {
    /// Domain state advanced only by inner transitions.
    pub state: S,
    /// Last output produced by a fire; preserved across zero-fire samples.
    pub held_output: O,
    /// Running modulo counter for `DownsampleModulo` guards.
    pub downsample_counter: u64,
}
/// Applies `Step_c` exactly `fires(c,i)` times for one outer sample.
///
/// The held output is changed only by an inner transition. Consequently zero
/// fires preserve both domain state and the previous output.
pub fn simulate_clock_step<S, O, F>(
    guard: ClockGuard,
    clock: ClockValue,
    runtime: &mut ClockRuntime<S, O>,
    mut transition: F,
) -> Result<u64, VectorClockAdError>
where
    F: FnMut(&S, u64) -> (S, O),
{
    let fires = match (guard, clock) {
        (ClockGuard::BooleanOnDemand, ClockValue::Boolean(active)) => u64::from(active),
        (
            ClockGuard::CountedOnDemand | ClockGuard::CountedUpsampling,
            ClockValue::Integer(count),
        ) => u64::try_from(count).unwrap_or(0),
        (ClockGuard::DownsampleModulo, ClockValue::Integer(factor)) => {
            if factor <= 0 {
                return Err(VectorClockAdError::InvalidDownsampleFactor { factor });
            }
            let fires = u64::from(runtime.downsample_counter == 0);
            let factor = u64::try_from(factor).expect("positive i64 fits u64");
            runtime.downsample_counter = (runtime.downsample_counter + 1) % factor;
            fires
        }
        (guard, _) => return Err(VectorClockAdError::ClockValueKindMismatch { guard }),
    };
    for fire in 0..fires {
        let (next_state, output) = transition(&runtime.state, fire);
        runtime.state = next_state;
        runtime.held_output = output;
    }
    Ok(fires)
}
/// Executes one scalar reverse window with immutable `Forward < Reverse` order.
pub fn execute_reverse_ad_window<S, P, T, A, Forward, Reverse>(
    initial_state: S,
    forward: Forward,
    reverse: Reverse,
) -> (S, P, A)
where
    Forward: FnOnce(S) -> (S, P, T),
    Reverse: FnOnce(S, T) -> (S, A),
{
    let (forward_state, primal, tape) = forward(initial_state);
    let (reverse_state, adjoints) = reverse(forward_state, tape);
    (reverse_state, primal, adjoints)
}
