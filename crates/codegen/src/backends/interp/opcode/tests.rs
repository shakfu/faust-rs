use super::*;

#[test]
fn opcode_count_matches_table() {
    assert_eq!(FBC_OPCODE_COUNT, FBC_INSTRUCTION_NAMES.len());
}

#[test]
fn first_opcode_is_zero() {
    assert_eq!(FbcOpcode::RealValue as u16, 0);
}

#[test]
fn last_opcode_is_count_minus_one() {
    assert_eq!(FbcOpcode::LoadOutput as u16, (FBC_OPCODE_COUNT - 1) as u16);
}

#[test]
fn from_u16_roundtrips() {
    for v in 0..FBC_OPCODE_COUNT as u16 {
        let op = FbcOpcode::from_u16(v).unwrap_or_else(|| panic!("invalid opcode {v}"));
        assert_eq!(op as u16, v);
    }
}

#[test]
fn from_u16_rejects_out_of_range() {
    assert!(FbcOpcode::from_u16(FBC_OPCODE_COUNT as u16).is_none());
    assert!(FbcOpcode::from_u16(u16::MAX).is_none());
}

#[test]
fn name_table_first_and_last() {
    assert_eq!(FbcOpcode::RealValue.name(), "kRealValue");
    assert_eq!(FbcOpcode::LoadOutput.name(), "kLoadOutput");
}

#[test]
fn name_table_spot_checks() {
    assert_eq!(FbcOpcode::LoadReal.name(), "kLoadReal");
    assert_eq!(FbcOpcode::AddReal.name(), "kAddReal");
    assert_eq!(FbcOpcode::Abs.name(), "kAbs");
    assert_eq!(FbcOpcode::Atan2f.name(), "kAtan2f");
    assert_eq!(FbcOpcode::Loop.name(), "kLoop");
    assert_eq!(FbcOpcode::Return.name(), "kReturn");
    assert_eq!(FbcOpcode::If.name(), "kIf");
    assert_eq!(FbcOpcode::OpenVerticalBox.name(), "kOpenVerticalBox");
    assert_eq!(FbcOpcode::Declare.name(), "kDeclare");
}

#[test]
fn is_math_boundaries() {
    assert!(!FbcOpcode::BitcastReal.is_math());
    assert!(FbcOpcode::AddReal.is_math());
    assert!(FbcOpcode::XORInt.is_math());
    assert!(!FbcOpcode::AddRealHeap.is_math());
}

#[test]
fn is_extended_unary_math_boundaries() {
    assert!(FbcOpcode::Abs.is_extended_unary_math());
    assert!(FbcOpcode::Tanhf.is_extended_unary_math());
    // Isnanf and Isinff are excluded (not optimized in C++).
    assert!(!FbcOpcode::Isnanf.is_extended_unary_math());
    assert!(!FbcOpcode::Isinff.is_extended_unary_math());
}

#[test]
fn is_extended_binary_math_boundaries() {
    assert!(FbcOpcode::Atan2f.is_extended_binary_math());
    assert!(FbcOpcode::Minf.is_extended_binary_math());
    // Copysignf is excluded (not optimized in C++).
    assert!(!FbcOpcode::Copysignf.is_extended_binary_math());
}

#[test]
fn is_choice_matches_cpp() {
    assert!(FbcOpcode::If.is_choice());
    assert!(FbcOpcode::SelectReal.is_choice());
    assert!(FbcOpcode::SelectInt.is_choice());
    assert!(!FbcOpcode::CondBranch.is_choice());
    assert!(!FbcOpcode::Loop.is_choice());
}

#[test]
fn is_real_type_spot_checks() {
    assert!(FbcOpcode::RealValue.is_real_type());
    assert!(FbcOpcode::AddReal.is_real_type());
    assert!(FbcOpcode::Sinf.is_real_type());
    assert!(FbcOpcode::Atan2f.is_real_type());
    assert!(!FbcOpcode::Int32Value.is_real_type());
    assert!(!FbcOpcode::AddInt.is_real_type());
    assert!(!FbcOpcode::Nop.is_real_type());
}

// ── Offset arithmetic helper tests ──────────────────────────────────

#[test]
fn to_heap_standard_math() {
    assert_eq!(FbcOpcode::AddReal.to_heap(), Some(FbcOpcode::AddRealHeap));
    assert_eq!(FbcOpcode::SubInt.to_heap(), Some(FbcOpcode::SubIntHeap));
    assert_eq!(FbcOpcode::XORInt.to_heap(), Some(FbcOpcode::XORIntHeap));
}

#[test]
fn to_heap_extended_unary() {
    assert_eq!(FbcOpcode::Abs.to_heap(), Some(FbcOpcode::AbsHeap));
    assert_eq!(FbcOpcode::Sinf.to_heap(), Some(FbcOpcode::SinfHeap));
    assert_eq!(FbcOpcode::Tanhf.to_heap(), Some(FbcOpcode::TanhfHeap));
}

#[test]
fn to_heap_extended_binary() {
    assert_eq!(FbcOpcode::Atan2f.to_heap(), Some(FbcOpcode::Atan2fHeap));
    assert_eq!(FbcOpcode::Minf.to_heap(), Some(FbcOpcode::MinfHeap));
}

#[test]
fn to_stack_standard_math() {
    assert_eq!(FbcOpcode::AddReal.to_stack(), Some(FbcOpcode::AddRealStack));
    assert_eq!(FbcOpcode::MultInt.to_stack(), Some(FbcOpcode::MultIntStack));
}

#[test]
fn to_stack_extended_binary() {
    assert_eq!(FbcOpcode::Atan2f.to_stack(), Some(FbcOpcode::Atan2fStack));
}

#[test]
fn to_stack_returns_none_for_unary() {
    assert!(FbcOpcode::Sinf.to_stack().is_none());
}

#[test]
fn to_stack_value_standard() {
    assert_eq!(
        FbcOpcode::MultReal.to_stack_value(),
        Some(FbcOpcode::MultRealStackValue)
    );
}

#[test]
fn to_value_standard() {
    assert_eq!(FbcOpcode::AddReal.to_value(), Some(FbcOpcode::AddRealValue));
    assert_eq!(FbcOpcode::SubInt.to_value(), Some(FbcOpcode::SubIntValue));
}

#[test]
fn to_value_invert_non_commutative() {
    assert_eq!(
        FbcOpcode::SubReal.to_value_invert(),
        Some(FbcOpcode::SubRealValueInvert)
    );
    assert_eq!(
        FbcOpcode::DivInt.to_value_invert(),
        Some(FbcOpcode::DivIntValueInvert)
    );
    assert_eq!(
        FbcOpcode::GTReal.to_value_invert(),
        Some(FbcOpcode::GTRealValueInvert)
    );
    assert_eq!(
        FbcOpcode::Atan2f.to_value_invert(),
        Some(FbcOpcode::Atan2fValueInvert)
    );
}

#[test]
fn to_value_invert_commutative_falls_through() {
    // Commutative ops: value_invert == value
    assert_eq!(
        FbcOpcode::AddReal.to_value_invert(),
        Some(FbcOpcode::AddRealValue)
    );
    assert_eq!(FbcOpcode::Max.to_value_invert(), Some(FbcOpcode::MaxValue));
}

#[test]
fn is_commutative_spot_checks() {
    assert!(FbcOpcode::AddReal.is_commutative());
    assert!(FbcOpcode::MultInt.is_commutative());
    assert!(FbcOpcode::EQReal.is_commutative());
    assert!(FbcOpcode::Maxf.is_commutative());
    assert!(!FbcOpcode::SubReal.is_commutative());
    assert!(!FbcOpcode::DivInt.is_commutative());
    assert!(!FbcOpcode::GTReal.is_commutative());
    assert!(!FbcOpcode::Atan2f.is_commutative());
}

#[test]
fn to_heap_rejects_non_math() {
    assert!(FbcOpcode::LoadReal.to_heap().is_none());
    assert!(FbcOpcode::Return.to_heap().is_none());
    assert!(FbcOpcode::Nop.to_heap().is_none());
}

#[test]
fn name_table_matches_enum_names() {
    assert_eq!(
        FBC_INSTRUCTION_NAMES[FbcOpcode::GEIntValueInvert as usize],
        "kGEIntValueInvert"
    );
    assert_eq!(
        FBC_INSTRUCTION_NAMES[FbcOpcode::MaxfStackValue as usize],
        "kMaxfStackValue"
    );
    assert_eq!(
        FBC_INSTRUCTION_NAMES[FbcOpcode::AddCheckButton as usize],
        "kAddCheckButton"
    );
}
