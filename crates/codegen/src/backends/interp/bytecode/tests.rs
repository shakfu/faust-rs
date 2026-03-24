use super::*;

#[test]
/// Verifies that block arena allocation preserves ids and instruction payloads.
fn block_arena_alloc_and_get() {
    let mut arena = FbcBlockArena::<f32>::new();
    assert!(arena.is_empty());

    let mut block = FbcBlock::new();
    block.push(FbcInstruction::new(FbcOpcode::Int32Value));
    block.push(FbcInstruction::new(FbcOpcode::Return));

    let id = arena.alloc(block);
    assert_eq!(id.as_u32(), 0);
    assert_eq!(arena.len(), 1);
    assert!(!arena.is_empty());

    let retrieved = arena.get(id);
    assert_eq!(retrieved.len(), 2);
    assert_eq!(retrieved.instructions[0].opcode, FbcOpcode::Int32Value);
    assert_eq!(retrieved.instructions[1].opcode, FbcOpcode::Return);
}

#[test]
/// Verifies the minimal well-formedness rule (`Return` must terminate the block).
fn block_well_formed() {
    let mut block = FbcBlock::<f64>::new();
    assert!(!block.is_well_formed());

    block.push(FbcInstruction::new(FbcOpcode::AddReal));
    assert!(!block.is_well_formed());

    block.push(FbcInstruction::new(FbcOpcode::Return));
    assert!(block.is_well_formed());
}

#[test]
/// Verifies `with_values` fills the numeric payload fields only.
fn instruction_with_values() {
    let instr = FbcInstruction::<f32>::with_values(FbcOpcode::RealValue, 0, 3.125);
    assert_eq!(instr.opcode, FbcOpcode::RealValue);
    assert_eq!(instr.int_value, 0);
    assert!((instr.real_value - 3.125).abs() < 1e-6);
    assert_eq!(instr.offset1, -1);
    assert_eq!(instr.offset2, -1);
    assert!(instr.branch1.is_none());
    assert!(instr.branch2.is_none());
}

#[test]
/// Verifies `with_values_and_offsets` fills both payload and offset fields.
fn instruction_with_values_and_offsets() {
    let instr = FbcInstruction::<f64>::with_values_and_offsets(FbcOpcode::LoadReal, 0, 0.0, 42, -1);
    assert_eq!(instr.opcode, FbcOpcode::LoadReal);
    assert_eq!(instr.offset1, 42);
    assert_eq!(instr.offset2, -1);
}

#[test]
/// Verifies the fully-specified constructor preserves both branch targets.
fn instruction_full() {
    let instr = FbcInstruction::<f32>::full(
        FbcOpcode::If,
        "",
        0,
        0.0,
        -1,
        -1,
        Some(BlockId(1)),
        Some(BlockId(2)),
    );
    assert_eq!(instr.opcode, FbcOpcode::If);
    assert_eq!(instr.branch1, Some(BlockId(1)));
    assert_eq!(instr.branch2, Some(BlockId(2)));
}

#[test]
/// Verifies `CondBranch` hides its loop-back pointer from `get_branch1`.
fn get_branch1_cond_branch_returns_none() {
    let instr = FbcInstruction::<f32>::full(
        FbcOpcode::CondBranch,
        "",
        0,
        0.0,
        -1,
        -1,
        Some(BlockId(0)),
        None,
    );
    // CondBranch's branch1 is the loop-back pointer, not semantically owned.
    assert!(instr.get_branch1().is_none());
}

#[test]
/// Verifies `is_real_inst` reflects the first instruction family.
fn block_is_real_inst() {
    let mut block = FbcBlock::<f32>::new();
    block.push(FbcInstruction::new(FbcOpcode::AddReal));
    assert!(block.is_real_inst());

    let mut block2 = FbcBlock::<f32>::new();
    block2.push(FbcInstruction::new(FbcOpcode::AddInt));
    assert!(!block2.is_real_inst());
}

#[test]
/// Verifies block-store payloads stay attached to the owning instruction.
fn block_store_data() {
    let mut block = FbcBlock::<f32>::new();
    let instr = FbcInstruction::with_values_and_offsets(FbcOpcode::BlockStoreReal, 0, 0.0, 0, 4);
    let data = BlockStoreData::Real(vec![1.0, 2.0, 3.0, 4.0]);
    block.push_block_store(instr, data);
    block.push(FbcInstruction::new(FbcOpcode::Return));

    assert_eq!(block.len(), 2);
    assert!(matches!(
        block.instructions[0].block_store,
        Some(BlockStoreData::Real(_))
    ));
}

#[test]
/// Verifies the UI helper constructors populate the expected fields.
fn ui_instruction_constructors() {
    let open = FbcUiInstruction::<f32>::open_box(FbcOpcode::OpenVerticalBox, "main");
    assert_eq!(open.opcode, FbcOpcode::OpenVerticalBox);
    assert_eq!(open.label, "main");

    let slider = FbcUiInstruction::<f64>::widget(
        FbcOpcode::AddHorizontalSlider,
        10,
        "gain",
        0.5,
        0.0,
        1.0,
        0.01,
    );
    assert_eq!(slider.opcode, FbcOpcode::AddHorizontalSlider);
    assert_eq!(slider.offset, 10);
    assert_eq!(slider.label, "gain");
    assert!((slider.init - 0.5).abs() < 1e-10);

    let meta = FbcUiInstruction::<f32>::declare(5, "unit", "dB");
    assert_eq!(meta.opcode, FbcOpcode::Declare);
    assert_eq!(meta.key, "unit");
    assert_eq!(meta.value, "dB");
}

#[test]
/// Verifies metadata instructions preserve key/value pairs.
fn meta_instruction() {
    let meta = FbcMetaInstruction::new("name", "sine");
    assert_eq!(meta.key, "name");
    assert_eq!(meta.value, "sine");
}

#[test]
/// Verifies multiple allocated blocks remain independently addressable.
fn multiple_blocks_in_arena() {
    let mut arena = FbcBlockArena::<f32>::new();

    let mut b1 = FbcBlock::new();
    b1.push(FbcInstruction::new(FbcOpcode::Return));
    let id1 = arena.alloc(b1);

    let mut b2 = FbcBlock::new();
    b2.push(FbcInstruction::with_values(FbcOpcode::Int32Value, 42, 0.0));
    b2.push(FbcInstruction::new(FbcOpcode::Return));
    let id2 = arena.alloc(b2);

    assert_eq!(arena.len(), 2);
    assert_eq!(arena.get(id1).len(), 1);
    assert_eq!(arena.get(id2).len(), 2);
    assert_eq!(arena.get(id2).instructions[0].int_value, 42);
}
