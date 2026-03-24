use super::*;
use crate::backends::interp::bytecode::{FbcBlock, FbcInstruction};
use std::time::{Duration, Instant};

/// Helper: build a block from a list of instructions, appending Return.
fn make_block(instrs: Vec<FbcInstruction<f32>>) -> FbcBlock<f32> {
    let mut block = FbcBlock::new();
    for i in instrs {
        block.push(i);
    }
    block.push(FbcInstruction::new(FbcOpcode::Return));
    block
}

#[test]
fn push_and_store_real() {
    let mut arena = FbcBlockArena::<f32>::new();
    let block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::RealValue, 0, 3.125),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 0, -1),
    ]);
    let bid = arena.alloc(block);

    let mut exec = FbcExecutor::new(0, 4);
    exec.execute_block(&arena, bid);
    assert!((exec.real_heap[0] - 3.125).abs() < 1e-6);
}

#[test]
fn push_and_store_int() {
    let mut arena = FbcBlockArena::<f32>::new();
    let block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::Int32Value, 42, 0.0),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreInt, 0, 0.0, 0, -1),
    ]);
    let bid = arena.alloc(block);

    let mut exec = FbcExecutor::new(4, 0);
    exec.execute_block(&arena, bid);
    assert_eq!(exec.int_heap[0], 42);
}

#[test]
fn add_real() {
    let mut arena = FbcBlockArena::<f32>::new();
    let block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::RealValue, 0, 1.5),
        FbcInstruction::with_values(FbcOpcode::RealValue, 0, 2.5),
        FbcInstruction::new(FbcOpcode::AddReal),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 0, -1),
    ]);
    let bid = arena.alloc(block);

    let mut exec = FbcExecutor::new(0, 4);
    exec.execute_block(&arena, bid);
    assert!((exec.real_heap[0] - 4.0).abs() < 1e-6);
}

#[test]
fn sub_int() {
    let mut arena = FbcBlockArena::<f32>::new();
    let block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::Int32Value, 10, 0.0),
        FbcInstruction::with_values(FbcOpcode::Int32Value, 3, 0.0),
        FbcInstruction::new(FbcOpcode::SubInt),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreInt, 0, 0.0, 0, -1),
    ]);
    let bid = arena.alloc(block);

    let mut exec = FbcExecutor::new(4, 0);
    exec.execute_block(&arena, bid);
    // v1=3 (top), v2=10 (second), result = v1 - v2 = 3 - 10 = -7
    assert_eq!(exec.int_heap[0], -7);
}

#[test]
fn heap_load_store() {
    let mut arena = FbcBlockArena::<f32>::new();
    // Load real from heap[0], store to heap[1]
    let block = make_block(vec![
        FbcInstruction::with_values_and_offsets(FbcOpcode::LoadReal, 0, 0.0, 0, -1),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 1, -1),
    ]);
    let bid = arena.alloc(block);

    let mut exec = FbcExecutor::new(0, 4);
    exec.real_heap[0] = 99.0;
    exec.execute_block(&arena, bid);
    assert!((exec.real_heap[1] - 99.0).abs() < 1e-6);
}

#[test]
fn heap_op_heap() {
    let mut arena = FbcBlockArena::<f32>::new();
    // AddRealHeap: push real_heap[0] + real_heap[1], store to heap[2]
    let block = make_block(vec![
        FbcInstruction::with_values_and_offsets(FbcOpcode::AddRealHeap, 0, 0.0, 0, 1),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 2, -1),
    ]);
    let bid = arena.alloc(block);

    let mut exec = FbcExecutor::new(0, 4);
    exec.real_heap[0] = 10.0;
    exec.real_heap[1] = 20.0;
    exec.execute_block(&arena, bid);
    assert!((exec.real_heap[2] - 30.0).abs() < 1e-6);
}

#[test]
fn comparison_real() {
    let mut arena = FbcBlockArena::<f32>::new();
    // Push 5.0, push 3.0, GTReal → 3.0 > 5.0 = false (0)
    let block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::RealValue, 0, 5.0),
        FbcInstruction::with_values(FbcOpcode::RealValue, 0, 3.0),
        FbcInstruction::new(FbcOpcode::GTReal),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreInt, 0, 0.0, 0, -1),
    ]);
    let bid = arena.alloc(block);

    let mut exec = FbcExecutor::new(4, 0);
    exec.execute_block(&arena, bid);
    // v1=3.0 (top), v2=5.0, v1 > v2 = 3.0 > 5.0 = false = 0
    assert_eq!(exec.int_heap[0], 0);
}

#[test]
fn cast_real_to_int() {
    let mut arena = FbcBlockArena::<f32>::new();
    let block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::RealValue, 0, 3.7),
        FbcInstruction::new(FbcOpcode::CastInt),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreInt, 0, 0.0, 0, -1),
    ]);
    let bid = arena.alloc(block);

    let mut exec = FbcExecutor::new(4, 0);
    exec.execute_block(&arena, bid);
    assert_eq!(exec.int_heap[0], 3); // truncation
}

#[test]
fn cast_int_to_real() {
    let mut arena = FbcBlockArena::<f32>::new();
    let block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::Int32Value, 7, 0.0),
        FbcInstruction::new(FbcOpcode::CastReal),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 0, -1),
    ]);
    let bid = arena.alloc(block);

    let mut exec = FbcExecutor::new(0, 4);
    exec.execute_block(&arena, bid);
    assert!((exec.real_heap[0] - 7.0).abs() < 1e-6);
}

#[test]
fn extended_unary_sin() {
    let mut arena = FbcBlockArena::<f32>::new();
    let block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::RealValue, 0, std::f32::consts::FRAC_PI_2),
        FbcInstruction::new(FbcOpcode::Sinf),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 0, -1),
    ]);
    let bid = arena.alloc(block);

    let mut exec = FbcExecutor::new(0, 4);
    exec.execute_block(&arena, bid);
    assert!((exec.real_heap[0] - 1.0).abs() < 1e-6);
}

#[test]
fn if_branch_true() {
    let mut arena = FbcBlockArena::<f32>::new();

    // then-block: store 1.0 to heap[0]
    let then_block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::RealValue, 0, 1.0),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 0, -1),
    ]);
    let then_id = arena.alloc(then_block);

    // else-block: store 2.0 to heap[0]
    let else_block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::RealValue, 0, 2.0),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 0, -1),
    ]);
    let else_id = arena.alloc(else_block);

    // main block: push 1 (true), If
    let main_block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::Int32Value, 1, 0.0),
        FbcInstruction::full(
            FbcOpcode::If,
            "",
            0,
            0.0,
            -1,
            -1,
            Some(then_id),
            Some(else_id),
        ),
    ]);
    let main_id = arena.alloc(main_block);

    let mut exec = FbcExecutor::new(0, 4);
    exec.execute_block(&arena, main_id);
    assert!((exec.real_heap[0] - 1.0).abs() < 1e-6);
}

#[test]
fn if_branch_false() {
    let mut arena = FbcBlockArena::<f32>::new();

    let then_block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::RealValue, 0, 1.0),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 0, -1),
    ]);
    let then_id = arena.alloc(then_block);

    let else_block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::RealValue, 0, 2.0),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 0, -1),
    ]);
    let else_id = arena.alloc(else_block);

    let main_block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::Int32Value, 0, 0.0), // false
        FbcInstruction::full(
            FbcOpcode::If,
            "",
            0,
            0.0,
            -1,
            -1,
            Some(then_id),
            Some(else_id),
        ),
    ]);
    let main_id = arena.alloc(main_block);

    let mut exec = FbcExecutor::new(0, 4);
    exec.execute_block(&arena, main_id);
    assert!((exec.real_heap[0] - 2.0).abs() < 1e-6);
}

#[test]
fn simple_loop() {
    // Implement: for (i = 0; i < 5; i++) { heap[1] += 10; }
    let mut arena = FbcBlockArena::<f32>::new();

    // Init block: set int_heap[0] = 0  (loop counter)
    let init_block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::Int32Value, 0, 0.0),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreInt, 0, 0.0, 0, -1),
    ]);
    let init_id = arena.alloc(init_block);

    // Body block: heap[1] += 10; i++; CondBranch(i < 5)
    // We need to allocate first to get the ID for CondBranch's branch1.
    let body_placeholder = FbcBlock::new(); // placeholder
    let body_id = arena.alloc(body_placeholder);

    // Now build the real body
    let mut body = FbcBlock::new();
    // heap[1] += 10
    body.push(FbcInstruction::with_values_and_offsets(
        FbcOpcode::LoadInt,
        0,
        0.0,
        1,
        -1,
    ));
    body.push(FbcInstruction::with_values(FbcOpcode::Int32Value, 10, 0.0));
    body.push(FbcInstruction::new(FbcOpcode::AddInt));
    body.push(FbcInstruction::with_values_and_offsets(
        FbcOpcode::StoreInt,
        0,
        0.0,
        1,
        -1,
    ));
    // i++
    body.push(FbcInstruction::with_values_and_offsets(
        FbcOpcode::LoadInt,
        0,
        0.0,
        0,
        -1,
    ));
    body.push(FbcInstruction::with_values(FbcOpcode::Int32Value, 1, 0.0));
    body.push(FbcInstruction::new(FbcOpcode::AddInt));
    body.push(FbcInstruction::with_values_and_offsets(
        FbcOpcode::StoreInt,
        0,
        0.0,
        0,
        -1,
    ));
    // condition: i < 5
    // Stack convention: LTInt pops v1 (TOS), v2 (second), computes v1 < v2.
    // To get i < 5: push 5 first (v2), then i (v1 = TOS), so v1 < v2 = i < 5.
    body.push(FbcInstruction::with_values(FbcOpcode::Int32Value, 5, 0.0));
    body.push(FbcInstruction::with_values_and_offsets(
        FbcOpcode::LoadInt,
        0,
        0.0,
        0,
        -1,
    ));
    body.push(FbcInstruction::new(FbcOpcode::LTInt));
    // CondBranch: if true → loop back (branch1 = body_id)
    body.push(FbcInstruction::full(
        FbcOpcode::CondBranch,
        "",
        0,
        0.0,
        -1,
        -1,
        Some(body_id),
        None,
    ));
    body.push(FbcInstruction::new(FbcOpcode::Return));
    // Replace placeholder with real body
    *arena.get_mut(body_id) = body;

    // Main block: Loop with init and body
    let main_block = make_block(vec![FbcInstruction::full(
        FbcOpcode::Loop,
        "",
        0,
        0.0,
        -1,
        -1,
        Some(init_id),
        Some(body_id),
    )]);
    let main_id = arena.alloc(main_block);

    let mut exec = FbcExecutor::new(4, 0);
    exec.execute_block(&arena, main_id);
    // After 5 iterations: i=5, heap[1] = 50
    assert_eq!(exec.int_heap[0], 5);
    assert_eq!(exec.int_heap[1], 50);
}

#[test]
fn move_real() {
    let mut arena = FbcBlockArena::<f32>::new();
    let block = make_block(vec![FbcInstruction::with_values_and_offsets(
        FbcOpcode::MoveReal,
        0,
        0.0,
        1,
        0,
    )]);
    let bid = arena.alloc(block);

    let mut exec = FbcExecutor::new(0, 4);
    exec.real_heap[0] = 42.0;
    exec.execute_block(&arena, bid);
    assert!((exec.real_heap[1] - 42.0).abs() < 1e-6);
}

#[test]
fn store_real_value() {
    let mut arena = FbcBlockArena::<f32>::new();
    let block = make_block(vec![FbcInstruction::with_values_and_offsets(
        FbcOpcode::StoreRealValue,
        0,
        7.77,
        2,
        -1,
    )]);
    let bid = arena.alloc(block);

    let mut exec = FbcExecutor::new(0, 4);
    exec.execute_block(&arena, bid);
    assert!((exec.real_heap[2] - 7.77).abs() < 1e-5);
}

#[test]
fn io_load_store() {
    let mut arena = FbcBlockArena::<f32>::new();
    // Load sample 0 from input channel 0, store to output channel 0 at index 0
    let block = make_block(vec![
        // Push sample index 0
        FbcInstruction::with_values(FbcOpcode::Int32Value, 0, 0.0),
        // LoadInput channel 0
        FbcInstruction::with_values_and_offsets(FbcOpcode::LoadInput, 0, 0.0, 0, -1),
        // Push sample index 0 for output
        FbcInstruction::with_values(FbcOpcode::Int32Value, 0, 0.0),
        // StoreOutput channel 0 (pops index then value)
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreOutput, 0, 0.0, 0, -1),
    ]);
    let bid = arena.alloc(block);

    let input_data = [1.0_f32, 2.0, 3.0];
    let inputs: &[&[f32]] = &[&input_data];
    let mut output_data = [0.0_f32; 3];
    let mut exec = FbcExecutor::new(0, 0);
    exec.execute_block_io(&arena, bid, inputs, &mut [&mut output_data]);
    assert!((output_data[0] - 1.0).abs() < 1e-6);
}

#[test]
fn load_input_oob_returns_structured_io_error() {
    let mut arena = FbcBlockArena::<f32>::new();
    let block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::Int32Value, 0, 0.0),
        // Request input channel 1 while only channel 0 is provided.
        FbcInstruction::with_values_and_offsets(FbcOpcode::LoadInput, 0, 0.0, 1, -1),
    ]);
    let bid = arena.alloc(block);

    let input_data = [1.0_f32];
    let inputs: &[&[f32]] = &[&input_data];
    let mut exec = FbcExecutor::new(0, 0);
    let err = exec
        .try_execute_block_io(&arena, bid, inputs, &mut [])
        .expect_err("LoadInput with missing channel should return io_oob");

    assert_eq!(err.kind, "io_oob");
    assert_eq!(err.opcode, FbcOpcode::LoadInput);
    assert_eq!(err.channel, Some(1));
    assert_eq!(err.sample, Some(0));
}

#[test]
fn store_output_stack_underflow_returns_structured_error() {
    let mut arena = FbcBlockArena::<f32>::new();
    // Push only an int sample index, then attempt StoreOutput. This leaves
    // no value on the real stack and should report a structured underflow.
    let block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::Int32Value, 0, 0.0),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreOutput, 0, 0.0, 0, -1),
    ]);
    let bid = arena.alloc(block);

    let mut output_data = [0.0_f32; 1];
    let mut exec = FbcExecutor::new(0, 0);
    let err = exec
        .try_execute_block_io(&arena, bid, &[], &mut [&mut output_data])
        .expect_err("StoreOutput with empty real stack should not panic");

    assert_eq!(err.kind, "stack_underflow");
    assert_eq!(err.opcode, FbcOpcode::StoreOutput);
    assert_eq!(err.block_id, bid);
    assert_eq!(err.pc, 1);
    assert_eq!(err.stack, Some(FbcStackKind::Real));
}

#[test]
fn unchecked_heap_oob_is_reported_as_structured_heap_error_in_try_mode() {
    let mut arena = FbcBlockArena::<f32>::new();
    // StoreReal into heap[4] while heap size is 1 -> indexing panic in the
    // current unchecked fast-style implementation path, which try-mode must
    // trap and report structurally.
    let block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::RealValue, 0, 1.0),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 4, -1),
    ]);
    let bid = arena.alloc(block);

    let mut exec = FbcExecutor::new(0, 1);
    let err = exec
        .try_execute_block(&arena, bid)
        .expect_err("heap OOB should be trapped as a structured runtime error");

    assert_eq!(err.kind, "heap_oob");
    assert_eq!(err.opcode, FbcOpcode::StoreReal);
    assert_eq!(err.block_id, bid);
    assert_eq!(err.pc, 1);
}

#[test]
fn invalid_block_id_returns_structured_error() {
    let mut arena = FbcBlockArena::<f32>::new();
    let valid = make_block(vec![]);
    let _ = arena.alloc(valid);
    let invalid = BlockId::from_raw(42);

    let mut exec = FbcExecutor::new(0, 0);
    let err = exec
        .try_execute_block(&arena, invalid)
        .expect_err("invalid block id should return a structured error");

    assert_eq!(err.kind, "invalid_block_id");
    assert_eq!(err.block_id, invalid);
}

#[test]
fn invalid_block_pc_returns_structured_error() {
    let mut arena = FbcBlockArena::<f32>::new();
    let empty_block = FbcBlock::new(); // no Return, invalid runtime block shape on purpose
    let bid = arena.alloc(empty_block);

    let mut exec = FbcExecutor::new(0, 0);
    let err = exec
        .try_execute_block(&arena, bid)
        .expect_err("invalid pc fetch should return a structured error");

    assert_eq!(err.kind, "invalid_block_pc");
    assert_eq!(err.block_id, bid);
    assert_eq!(err.pc, 0);
}

#[test]
fn div_int_by_zero() {
    let mut arena = FbcBlockArena::<f32>::new();
    let block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::Int32Value, 10, 0.0),
        FbcInstruction::with_values(FbcOpcode::Int32Value, 0, 0.0),
        FbcInstruction::new(FbcOpcode::DivInt),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreInt, 0, 0.0, 0, -1),
    ]);
    let bid = arena.alloc(block);

    let mut exec = FbcExecutor::new(4, 0);
    exec.execute_block(&arena, bid);
    // Division by zero → 0
    assert_eq!(exec.int_heap[0], 0);
}

#[test]
fn bitwise_and() {
    let mut arena = FbcBlockArena::<f32>::new();
    let block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::Int32Value, 0b1100, 0.0),
        FbcInstruction::with_values(FbcOpcode::Int32Value, 0b1010, 0.0),
        FbcInstruction::new(FbcOpcode::ANDInt),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreInt, 0, 0.0, 0, -1),
    ]);
    let bid = arena.alloc(block);

    let mut exec = FbcExecutor::new(4, 0);
    exec.execute_block(&arena, bid);
    // v1=0b1010, v2=0b1100, result = 0b1010 & 0b1100 = 0b1000 = 8
    assert_eq!(exec.int_heap[0], 0b1000);
}

#[test]
fn select_real() {
    let mut arena = FbcBlockArena::<f32>::new();

    // Branch1: push 100.0
    let b1 = make_block(vec![FbcInstruction::with_values(
        FbcOpcode::RealValue,
        0,
        100.0,
    )]);
    let b1_id = arena.alloc(b1);

    // Branch2: push 200.0
    let b2 = make_block(vec![FbcInstruction::with_values(
        FbcOpcode::RealValue,
        0,
        200.0,
    )]);
    let b2_id = arena.alloc(b2);

    // Main: push cond=1, SelectReal, StoreReal
    let main_block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::Int32Value, 1, 0.0),
        FbcInstruction::full(
            FbcOpcode::SelectReal,
            "",
            0,
            0.0,
            -1,
            -1,
            Some(b1_id),
            Some(b2_id),
        ),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 0, -1),
    ]);
    let main_id = arena.alloc(main_block);

    let mut exec = FbcExecutor::new(0, 4);
    exec.execute_block(&arena, main_id);
    assert!((exec.real_heap[0] - 100.0).abs() < 1e-6);
}

#[test]
fn nop_and_return() {
    let mut arena = FbcBlockArena::<f32>::new();
    let block = make_block(vec![
        FbcInstruction::new(FbcOpcode::Nop),
        FbcInstruction::new(FbcOpcode::Nop),
    ]);
    let bid = arena.alloc(block);

    let mut exec = FbcExecutor::new(0, 0);
    exec.execute_block(&arena, bid);
    // Just verify it doesn't crash.
}

#[test]
fn extended_binary_max_min() {
    let mut arena = FbcBlockArena::<f32>::new();
    let block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::Int32Value, 3, 0.0),
        FbcInstruction::with_values(FbcOpcode::Int32Value, 7, 0.0),
        FbcInstruction::new(FbcOpcode::Max),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreInt, 0, 0.0, 0, -1),
        FbcInstruction::with_values(FbcOpcode::Int32Value, 3, 0.0),
        FbcInstruction::with_values(FbcOpcode::Int32Value, 7, 0.0),
        FbcInstruction::new(FbcOpcode::Min),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreInt, 0, 0.0, 1, -1),
    ]);
    let bid = arena.alloc(block);

    let mut exec = FbcExecutor::new(4, 0);
    exec.execute_block(&arena, bid);
    // v1=7 (top), v2=3, max(7,3)=7, min(7,3)=3
    assert_eq!(exec.int_heap[0], 7);
    assert_eq!(exec.int_heap[1], 3);
}

fn run_bench_case<F>(name: &str, iters: usize, mut f: F) -> Duration
where
    F: FnMut(),
{
    let start = Instant::now();
    for _ in 0..iters {
        f();
        std::hint::black_box(());
    }
    let elapsed = start.elapsed();
    let ns_per_iter = elapsed.as_nanos() as f64 / iters as f64;
    eprintln!("{name}: {elapsed:?} total ({ns_per_iter:.1} ns/iter)");
    elapsed
}

#[test]
#[ignore = "manual benchmark: run in release with -- --ignored --nocapture"]
fn benchmark_fast_vs_try_executor_paths() {
    // Case 1: tiny no-IO arithmetic/control block (dispatch-heavy)
    let mut arena1 = FbcBlockArena::<f32>::new();
    let block1 = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::RealValue, 0, 1.0),
        FbcInstruction::with_values(FbcOpcode::RealValue, 0, 2.0),
        FbcInstruction::new(FbcOpcode::AddReal),
        FbcInstruction::with_values(FbcOpcode::RealValue, 0, 3.0),
        FbcInstruction::new(FbcOpcode::MultReal),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 0, -1),
    ]);
    let bid1 = arena1.alloc(block1);
    let mut exec1_fast = FbcExecutor::new(0, 4);
    let mut exec1_try = FbcExecutor::new(0, 4);

    // Case 2: IO roundtrip for one sample (exercise IO + stacks)
    let mut arena2 = FbcBlockArena::<f32>::new();
    let block2 = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::Int32Value, 0, 0.0),
        FbcInstruction::with_values_and_offsets(FbcOpcode::LoadInput, 0, 0.0, 0, -1),
        FbcInstruction::with_values(FbcOpcode::Int32Value, 0, 0.0),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreOutput, 0, 0.0, 0, -1),
    ]);
    let bid2 = arena2.alloc(block2);
    let input_data = [1.0_f32];
    let inputs: &[&[f32]] = &[&input_data];
    let mut out_fast = [0.0_f32; 1];
    let mut out_try = [0.0_f32; 1];
    let mut exec2_fast = FbcExecutor::new(0, 0);
    let mut exec2_try = FbcExecutor::new(0, 0);

    // Case 3: simple integer loop (control-flow heavy)
    let mut arena3 = FbcBlockArena::<f32>::new();
    let init_block = make_block(vec![
        FbcInstruction::with_values(FbcOpcode::Int32Value, 0, 0.0),
        FbcInstruction::with_values_and_offsets(FbcOpcode::StoreInt, 0, 0.0, 0, -1),
    ]);
    let init_id = arena3.alloc(init_block);
    let body_placeholder = FbcBlock::new();
    let body_id = arena3.alloc(body_placeholder);
    let mut body = FbcBlock::new();
    body.push(FbcInstruction::with_values_and_offsets(
        FbcOpcode::LoadInt,
        0,
        0.0,
        1,
        -1,
    ));
    body.push(FbcInstruction::with_values(FbcOpcode::Int32Value, 1, 0.0));
    body.push(FbcInstruction::new(FbcOpcode::AddInt));
    body.push(FbcInstruction::with_values_and_offsets(
        FbcOpcode::StoreInt,
        0,
        0.0,
        1,
        -1,
    ));
    body.push(FbcInstruction::with_values_and_offsets(
        FbcOpcode::LoadInt,
        0,
        0.0,
        0,
        -1,
    ));
    body.push(FbcInstruction::with_values(FbcOpcode::Int32Value, 1, 0.0));
    body.push(FbcInstruction::new(FbcOpcode::AddInt));
    body.push(FbcInstruction::with_values_and_offsets(
        FbcOpcode::StoreInt,
        0,
        0.0,
        0,
        -1,
    ));
    body.push(FbcInstruction::with_values(FbcOpcode::Int32Value, 8, 0.0));
    body.push(FbcInstruction::with_values_and_offsets(
        FbcOpcode::LoadInt,
        0,
        0.0,
        0,
        -1,
    ));
    body.push(FbcInstruction::new(FbcOpcode::LTInt));
    body.push(FbcInstruction::full(
        FbcOpcode::CondBranch,
        "",
        0,
        0.0,
        -1,
        -1,
        Some(body_id),
        None,
    ));
    body.push(FbcInstruction::new(FbcOpcode::Return));
    *arena3.get_mut(body_id) = body;
    let main_block = make_block(vec![FbcInstruction::full(
        FbcOpcode::Loop,
        "",
        0,
        0.0,
        -1,
        -1,
        Some(init_id),
        Some(body_id),
    )]);
    let bid3 = arena3.alloc(main_block);
    let mut exec3_fast = FbcExecutor::new(4, 0);
    let mut exec3_try = FbcExecutor::new(4, 0);

    // Warmup
    for _ in 0..1000 {
        exec1_fast.execute_block(&arena1, bid1);
        exec1_try.try_execute_block(&arena1, bid1).unwrap();
        exec2_fast.execute_block_io(&arena2, bid2, inputs, &mut [&mut out_fast]);
        exec2_try
            .try_execute_block_io(&arena2, bid2, inputs, &mut [&mut out_try])
            .unwrap();
        exec3_fast.execute_block(&arena3, bid3);
        exec3_try.try_execute_block(&arena3, bid3).unwrap();
    }

    let iters_small = 200_000;
    let iters_loop = 60_000;

    eprintln!("\n== bench: arithmetic block ==");
    let fast1 = run_bench_case("fast execute_block", iters_small, || {
        exec1_fast.execute_block(&arena1, bid1);
    });
    let try1 = run_bench_case("try  execute_block", iters_small, || {
        exec1_try.try_execute_block(&arena1, bid1).unwrap();
    });
    eprintln!(
        "ratio try/fast = {:.3}x\n",
        try1.as_secs_f64() / fast1.as_secs_f64()
    );

    eprintln!("== bench: single-sample IO block ==");
    let fast2 = run_bench_case("fast execute_block_io", iters_small, || {
        exec2_fast.execute_block_io(&arena2, bid2, inputs, &mut [&mut out_fast]);
    });
    let try2 = run_bench_case("try  execute_block_io", iters_small, || {
        exec2_try
            .try_execute_block_io(&arena2, bid2, inputs, &mut [&mut out_try])
            .unwrap();
    });
    eprintln!(
        "ratio try/fast = {:.3}x\n",
        try2.as_secs_f64() / fast2.as_secs_f64()
    );

    eprintln!("== bench: loop/control-flow block ==");
    let fast3 = run_bench_case("fast execute_block(loop)", iters_loop, || {
        exec3_fast.execute_block(&arena3, bid3);
    });
    let try3 = run_bench_case("try  execute_block(loop)", iters_loop, || {
        exec3_try.try_execute_block(&arena3, bid3).unwrap();
    });
    eprintln!(
        "ratio try/fast = {:.3}x\n",
        try3.as_secs_f64() / fast3.as_secs_f64()
    );
}
