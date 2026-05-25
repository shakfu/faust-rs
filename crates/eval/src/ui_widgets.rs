use super::*;

/// Evaluates one label node and re-interns the resulting string literal in the arena.
///
/// Widget/group constructors in box IR still store labels as tree nodes, so the
/// string returned by [`eval_label_node`] must be converted back into a canonical
/// literal node before rebuilding the enclosing widget.
pub(crate) fn evaluated_label_node(
    arena: &mut TreeArena,
    label: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let text = eval_label_node(arena, label, env, loop_detector)?;
    Ok(arena.string_lit(&text))
}

/// Evaluates one `button` label and rebuilds the widget node.
pub(crate) fn eval_button(
    arena: &mut TreeArena,
    label: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let label = evaluated_label_node(arena, label, env, loop_detector)?;
    Ok(BoxBuilder::new(arena).button(label))
}

/// Evaluates one `checkbox` label and rebuilds the widget node.
pub(crate) fn eval_checkbox(
    arena: &mut TreeArena,
    label: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let label = evaluated_label_node(arena, label, env, loop_detector)?;
    Ok(BoxBuilder::new(arena).checkbox(label))
}

pub(crate) fn eval_vslider(
    arena: &mut TreeArena,
    label: TreeId,
    params: [TreeId; 4],
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    eval_slider_like(
        arena,
        SliderKind::VSlider,
        label,
        params,
        env,
        loop_detector,
    )
}

pub(crate) fn eval_hslider(
    arena: &mut TreeArena,
    label: TreeId,
    params: [TreeId; 4],
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    eval_slider_like(
        arena,
        SliderKind::HSlider,
        label,
        params,
        env,
        loop_detector,
    )
}

pub(crate) fn eval_num_entry(
    arena: &mut TreeArena,
    label: TreeId,
    params: [TreeId; 4],
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    eval_slider_like(
        arena,
        SliderKind::NumEntry,
        label,
        params,
        env,
        loop_detector,
    )
}

enum SliderKind {
    VSlider,
    HSlider,
    NumEntry,
}

fn eval_slider_like(
    arena: &mut TreeArena,
    kind: SliderKind,
    label: TreeId,
    params: [TreeId; 4],
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    // C++ eval.cpp: each numeric parameter is reduced via eval2double(…)
    // which calls boxPropagateSig + simplify internally.  We do the same by
    // calling eval_box then simplifying the result to a boxReal literal when
    // possible, matching C++ `tree(eval2double(param, …))`.
    let label = evaluated_label_node(arena, label, env, loop_detector)?;
    let [cur, min, max, step] = params;
    let cur = simplify_slider_param(arena, cur, env, loop_detector)?;
    let min = simplify_slider_param(arena, min, env, loop_detector)?;
    let max = simplify_slider_param(arena, max, env, loop_detector)?;
    let step = simplify_slider_param(arena, step, env, loop_detector)?;
    let mut b = BoxBuilder::new(arena);
    Ok(match kind {
        SliderKind::VSlider => b.vslider(label, cur, min, max, step),
        SliderKind::HSlider => b.hslider(label, cur, min, max, step),
        SliderKind::NumEntry => b.num_entry(label, cur, min, max, step),
    })
}

/// Evaluates a slider/bargraph numeric parameter with the same semantics as
/// C++ `eval2double`: `eval_box` followed by `propagate + simplify → boxReal`.
///
/// If the expression cannot be reduced to a numeric constant at evaluation
/// time, the evaluated (but not simplified) box is returned unchanged so that
/// later passes can still handle it.
///
/// # C++ equivalent
///
/// `tree(eval2double(param, visited, localValEnv))` for slider/bargraph params
/// in `compiler/evaluate/eval.cpp`.
pub(crate) fn simplify_slider_param(
    arena: &mut TreeArena,
    param: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let evaled = eval_box(arena, param, env, loop_detector)?;
    // Try to reduce to f64 constant → boxReal(x).
    if let Ok(x) = eval_box_to_f64(arena, evaled) {
        return Ok(BoxBuilder::new(arena).real(x));
    }
    // Fallback: return the evaluated box as-is (e.g. pattern var, slot).
    Ok(evaled)
}

/// Evaluates one `soundfile` widget.
///
/// Only label interpolation and channel expression evaluation happen here. Full
/// runtime/path semantics are still handled later in `propagate`, just like in
/// the C++ split between evaluation and box-to-signal lowering.
pub(crate) fn eval_soundfile(
    arena: &mut TreeArena,
    label: TreeId,
    chan: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    // C++ eval.cpp: `tree(eval2int(chan, visited, localValEnv))`.
    let label = evaluated_label_node(arena, label, env, loop_detector)?;
    let evaled_chan = eval_box(arena, chan, env, loop_detector)?;
    let chan = if let Ok(n) = eval_box_to_i32(arena, evaled_chan) {
        BoxBuilder::new(arena).int(n)
    } else {
        evaled_chan
    };
    Ok(BoxBuilder::new(arena).soundfile(label, chan))
}

/// Evaluates one vertical UI group by interpolating its label and body.
pub(crate) fn eval_vgroup(
    arena: &mut TreeArena,
    label: TreeId,
    body: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let label = evaluated_label_node(arena, label, env, loop_detector)?;
    let body = eval_box(arena, body, env, loop_detector)?;
    Ok(BoxBuilder::new(arena).vgroup(label, body))
}

/// Evaluates one horizontal UI group by interpolating its label and body.
pub(crate) fn eval_hgroup(
    arena: &mut TreeArena,
    label: TreeId,
    body: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let label = evaluated_label_node(arena, label, env, loop_detector)?;
    let body = eval_box(arena, body, env, loop_detector)?;
    Ok(BoxBuilder::new(arena).hgroup(label, body))
}

/// Evaluates one tab UI group by interpolating its label and body.
pub(crate) fn eval_tgroup(
    arena: &mut TreeArena,
    label: TreeId,
    body: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let label = evaluated_label_node(arena, label, env, loop_detector)?;
    let body = eval_box(arena, body, env, loop_detector)?;
    Ok(BoxBuilder::new(arena).tgroup(label, body))
}

/// Evaluates one vertical bargraph node.
pub(crate) fn eval_vbargraph(
    arena: &mut TreeArena,
    label: TreeId,
    min: TreeId,
    max: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let label = evaluated_label_node(arena, label, env, loop_detector)?;
    // C++ uses eval2double for bargraph min/max.
    let min = simplify_slider_param(arena, min, env, loop_detector)?;
    let max = simplify_slider_param(arena, max, env, loop_detector)?;
    Ok(BoxBuilder::new(arena).vbargraph(label, min, max))
}

/// Evaluates one horizontal bargraph node.
pub(crate) fn eval_hbargraph(
    arena: &mut TreeArena,
    label: TreeId,
    min: TreeId,
    max: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let label = evaluated_label_node(arena, label, env, loop_detector)?;
    // C++ uses eval2double for bargraph min/max.
    let min = simplify_slider_param(arena, min, env, loop_detector)?;
    let max = simplify_slider_param(arena, max, env, loop_detector)?;
    Ok(BoxBuilder::new(arena).hbargraph(label, min, max))
}
