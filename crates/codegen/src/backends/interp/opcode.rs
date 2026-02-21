//! FBC opcode definitions and instruction name table.
//!
//! # Source provenance (C++)
//! - `compiler/generator/interpreter/fbc_opcode.hh`
//!
//! # Parity invariants
//! - Enum discriminant values match the C++ enum ordering exactly, as they are
//!   used as integer keys in the `.fbc` serialization format.
//! - [`FBC_INSTRUCTION_NAMES`] replicates `gFBCInstructionTable[]` from C++
//!   byte-for-byte (including known typos) for cross-compiler compatibility.
//!
//! # API mapping status
//! - `FBCInstruction::Opcode` (C++) → [`FbcOpcode`] (Rust): 1:1 discriminant mapping.
//! - `gFBCInstructionTable[]` (C++) → [`FBC_INSTRUCTION_NAMES`] (Rust): 1:1.
//! - `INTERP_FILE_VERSION` (C++) → [`INTERP_FILE_VERSION`] (Rust): 1:1.

/// Interpreter file format version.
///
/// Must match `INTERP_FILE_VERSION` in C++ `fbc_opcode.hh`.
pub const INTERP_FILE_VERSION: u32 = 8;

/// FBC opcode — complete instruction set for the Faust interpreter.
///
/// Uses `#[repr(u16)]` to guarantee dense integer discriminants suitable for
/// jump-table dispatch. The ordering matches the C++ enum exactly; do **not**
/// reorder variants without updating [`FBC_INSTRUCTION_NAMES`] and the `.fbc`
/// format.
///
/// # Source provenance (C++)
/// - `FBCInstruction::Opcode` in `fbc_opcode.hh`
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum FbcOpcode {
    // ── Numbers ──────────────────────────────────────────────────────────
    RealValue = 0,
    Int32Value = 1,

    // ── Memory ───────────────────────────────────────────────────────────
    LoadReal = 2,
    LoadInt = 3,
    LoadSoundFieldInt = 4,
    LoadSoundFieldReal = 5,
    StoreReal = 6,
    StoreInt = 7,
    StoreRealValue = 8,
    StoreIntValue = 9,
    LoadIndexedReal = 10,
    LoadIndexedInt = 11,
    StoreIndexedReal = 12,
    StoreIndexedInt = 13,
    BlockStoreReal = 14,
    BlockStoreInt = 15,
    MoveReal = 16,
    MoveInt = 17,
    PairMoveReal = 18,
    PairMoveInt = 19,
    BlockPairMoveReal = 20,
    BlockPairMoveInt = 21,
    BlockShiftReal = 22,
    BlockShiftInt = 23,
    LoadInput = 24,
    StoreOutput = 25,

    // ── Cast / Bitcast ───────────────────────────────────────────────────
    CastReal = 26,
    CastInt = 27,
    CastRealHeap = 28,
    CastIntHeap = 29,
    BitcastInt = 30,
    BitcastReal = 31,

    // ── Standard math (stack OP stack) ────────────────────────────────────
    AddReal = 32,
    AddInt = 33,
    SubReal = 34,
    SubInt = 35,
    MultReal = 36,
    MultInt = 37,
    DivReal = 38,
    DivInt = 39,
    RemReal = 40,
    RemInt = 41,
    LshInt = 42,
    ARshInt = 43,
    LRshInt = 44,
    GTInt = 45,
    LTInt = 46,
    GEInt = 47,
    LEInt = 48,
    EQInt = 49,
    NEInt = 50,
    GTReal = 51,
    LTReal = 52,
    GEReal = 53,
    LEReal = 54,
    EQReal = 55,
    NEReal = 56,
    ANDInt = 57,
    ORInt = 58,
    XORInt = 59,

    // ── Standard math (heap OP heap) ─────────────────────────────────────
    AddRealHeap = 60,
    AddIntHeap = 61,
    SubRealHeap = 62,
    SubIntHeap = 63,
    MultRealHeap = 64,
    MultIntHeap = 65,
    DivRealHeap = 66,
    DivIntHeap = 67,
    RemRealHeap = 68,
    RemIntHeap = 69,
    LshIntHeap = 70,
    ARshIntHeap = 71,
    LRshIntHeap = 72,
    GTIntHeap = 73,
    LTIntHeap = 74,
    GEIntHeap = 75,
    LEIntHeap = 76,
    EQIntHeap = 77,
    NEIntHeap = 78,
    GTRealHeap = 79,
    LTRealHeap = 80,
    GERealHeap = 81,
    LERealHeap = 82,
    EQRealHeap = 83,
    NERealHeap = 84,
    ANDIntHeap = 85,
    ORIntHeap = 86,
    XORIntHeap = 87,

    // ── Standard math (heap OP stack) ────────────────────────────────────
    AddRealStack = 88,
    AddIntStack = 89,
    SubRealStack = 90,
    SubIntStack = 91,
    MultRealStack = 92,
    MultIntStack = 93,
    DivRealStack = 94,
    DivIntStack = 95,
    RemRealStack = 96,
    RemIntStack = 97,
    LshIntStack = 98,
    ARshIntStack = 99,
    LRshIntStack = 100,
    GTIntStack = 101,
    LTIntStack = 102,
    GEIntStack = 103,
    LEIntStack = 104,
    EQIntStack = 105,
    NEIntStack = 106,
    GTRealStack = 107,
    LTRealStack = 108,
    GERealStack = 109,
    LERealStack = 110,
    EQRealStack = 111,
    NERealStack = 112,
    ANDIntStack = 113,
    ORIntStack = 114,
    XORIntStack = 115,

    // ── Standard math (value OP stack) ───────────────────────────────────
    AddRealStackValue = 116,
    AddIntStackValue = 117,
    SubRealStackValue = 118,
    SubIntStackValue = 119,
    MultRealStackValue = 120,
    MultIntStackValue = 121,
    DivRealStackValue = 122,
    DivIntStackValue = 123,
    RemRealStackValue = 124,
    RemIntStackValue = 125,
    LshIntStackValue = 126,
    ARshIntStackValue = 127,
    LRshIntStackValue = 128,
    GTIntStackValue = 129,
    LTIntStackValue = 130,
    GEIntStackValue = 131,
    LEIntStackValue = 132,
    EQIntStackValue = 133,
    NEIntStackValue = 134,
    GTRealStackValue = 135,
    LTRealStackValue = 136,
    GERealStackValue = 137,
    LERealStackValue = 138,
    EQRealStackValue = 139,
    NERealStackValue = 140,
    ANDIntStackValue = 141,
    ORIntStackValue = 142,
    XORIntStackValue = 143,

    // ── Standard math (value OP heap) ────────────────────────────────────
    AddRealValue = 144,
    AddIntValue = 145,
    SubRealValue = 146,
    SubIntValue = 147,
    MultRealValue = 148,
    MultIntValue = 149,
    DivRealValue = 150,
    DivIntValue = 151,
    RemRealValue = 152,
    RemIntValue = 153,
    LshIntValue = 154,
    ARshIntValue = 155,
    LRshIntValue = 156,
    GTIntValue = 157,
    LTIntValue = 158,
    GEIntValue = 159,
    LEIntValue = 160,
    EQIntValue = 161,
    NEIntValue = 162,
    GTRealValue = 163,
    LTRealValue = 164,
    GERealValue = 165,
    LERealValue = 166,
    EQRealValue = 167,
    NERealValue = 168,
    ANDIntValue = 169,
    ORIntValue = 170,
    XORIntValue = 171,

    // ── Standard math (value OP heap) : non-commutative inverted ─────────
    SubRealValueInvert = 172,
    SubIntValueInvert = 173,
    DivRealValueInvert = 174,
    DivIntValueInvert = 175,
    RemRealValueInvert = 176,
    RemIntValueInvert = 177,
    LshIntValueInvert = 178,
    ARshIntValueInvert = 179,
    LRshIntValueInvert = 180,
    GTIntValueInvert = 181,
    LTIntValueInvert = 182,
    GEIntValueInvert = 183,
    LEIntValueInvert = 184,
    GTRealValueInvert = 185,
    LTRealValueInvert = 186,
    GERealValueInvert = 187,
    LERealValueInvert = 188,

    // ── Extended unary math (stack) ──────────────────────────────────────
    Abs = 189,
    Absf = 190,
    Acosf = 191,
    Acoshf = 192,
    Asinf = 193,
    Asinhf = 194,
    Atanf = 195,
    Atanhf = 196,
    Ceilf = 197,
    Cosf = 198,
    Coshf = 199,
    Expf = 200,
    Floorf = 201,
    Logf = 202,
    Log10f = 203,
    Rintf = 204,
    Roundf = 205,
    Sinf = 206,
    Sinhf = 207,
    Sqrtf = 208,
    Tanf = 209,
    Tanhf = 210,
    Isnanf = 211,
    Isinff = 212,

    // ── Extended unary math (heap OP) ────────────────────────────────────
    AbsHeap = 213,
    AbsfHeap = 214,
    AcosfHeap = 215,
    AcoshfHeap = 216,
    AsinfHeap = 217,
    AsinhfHeap = 218,
    AtanfHeap = 219,
    AtanhfHeap = 220,
    CeilfHeap = 221,
    CosfHeap = 222,
    CoshfHeap = 223,
    ExpfHeap = 224,
    FloorfHeap = 225,
    LogfHeap = 226,
    Log10fHeap = 227,
    RintfHeap = 228,
    RoundfHeap = 229,
    SinfHeap = 230,
    SinhfHeap = 231,
    SqrtfHeap = 232,
    TanfHeap = 233,
    TanhfHeap = 234,

    // ── Extended binary math (stack OP stack) ────────────────────────────
    Atan2f = 235,
    Fmodf = 236,
    Powf = 237,
    Max = 238,
    Maxf = 239,
    Min = 240,
    Minf = 241,
    Copysignf = 242,

    // ── Extended binary math (heap OP heap) ──────────────────────────────
    Atan2fHeap = 243,
    FmodfHeap = 244,
    PowfHeap = 245,
    MaxHeap = 246,
    MaxfHeap = 247,
    MinHeap = 248,
    MinfHeap = 249,

    // ── Extended binary math (heap OP stack) ─────────────────────────────
    Atan2fStack = 250,
    FmodfStack = 251,
    PowfStack = 252,
    MaxStack = 253,
    MaxfStack = 254,
    MinStack = 255,
    MinfStack = 256,

    // ── Extended binary math (value OP stack) ────────────────────────────
    Atan2fStackValue = 257,
    FmodfStackValue = 258,
    PowfStackValue = 259,
    MaxStackValue = 260,
    MaxfStackValue = 261,
    MinStackValue = 262,
    MinfStackValue = 263,

    // ── Extended binary math (value OP heap) ─────────────────────────────
    Atan2fValue = 264,
    FmodfValue = 265,
    PowfValue = 266,
    MaxValue = 267,
    MaxfValue = 268,
    MinValue = 269,
    MinfValue = 270,

    // ── Extended binary math (value OP heap) : non-commutative inverted ──
    Atan2fValueInvert = 271,
    FmodfValueInvert = 272,
    PowfValueInvert = 273,

    // ── Control ──────────────────────────────────────────────────────────
    Loop = 274,
    Return = 275,

    // ── Select / if ──────────────────────────────────────────────────────
    If = 276,
    SelectReal = 277,
    SelectInt = 278,
    CondBranch = 279,

    // ── User Interface ───────────────────────────────────────────────────
    OpenVerticalBox = 280,
    OpenHorizontalBox = 281,
    OpenTabBox = 282,
    CloseBox = 283,
    AddButton = 284,
    AddCheckButton = 285,
    AddHorizontalSlider = 286,
    AddVerticalSlider = 287,
    AddNumEntry = 288,
    AddSoundfile = 289,
    AddHorizontalBargraph = 290,
    AddVerticalBargraph = 291,
    Declare = 292,

    // ── Misc ─────────────────────────────────────────────────────────────
    Nop = 293,
}

/// Total number of opcodes in the FBC instruction set.
pub const FBC_OPCODE_COUNT: usize = 294;

/// Lookup table for converting `u16` discriminants to [`FbcOpcode`] without
/// `unsafe` transmute.
///
/// This avoids the need for a 294-arm match expression while staying safe.
const FROM_U16_TABLE: [FbcOpcode; FBC_OPCODE_COUNT] = {
    use FbcOpcode::*;
    [
        RealValue,
        Int32Value,
        LoadReal,
        LoadInt,
        LoadSoundFieldInt,
        LoadSoundFieldReal,
        StoreReal,
        StoreInt,
        StoreRealValue,
        StoreIntValue,
        LoadIndexedReal,
        LoadIndexedInt,
        StoreIndexedReal,
        StoreIndexedInt,
        BlockStoreReal,
        BlockStoreInt,
        MoveReal,
        MoveInt,
        PairMoveReal,
        PairMoveInt,
        BlockPairMoveReal,
        BlockPairMoveInt,
        BlockShiftReal,
        BlockShiftInt,
        LoadInput,
        StoreOutput,
        CastReal,
        CastInt,
        CastRealHeap,
        CastIntHeap,
        BitcastInt,
        BitcastReal,
        AddReal,
        AddInt,
        SubReal,
        SubInt,
        MultReal,
        MultInt,
        DivReal,
        DivInt,
        RemReal,
        RemInt,
        LshInt,
        ARshInt,
        LRshInt,
        GTInt,
        LTInt,
        GEInt,
        LEInt,
        EQInt,
        NEInt,
        GTReal,
        LTReal,
        GEReal,
        LEReal,
        EQReal,
        NEReal,
        ANDInt,
        ORInt,
        XORInt,
        AddRealHeap,
        AddIntHeap,
        SubRealHeap,
        SubIntHeap,
        MultRealHeap,
        MultIntHeap,
        DivRealHeap,
        DivIntHeap,
        RemRealHeap,
        RemIntHeap,
        LshIntHeap,
        ARshIntHeap,
        LRshIntHeap,
        GTIntHeap,
        LTIntHeap,
        GEIntHeap,
        LEIntHeap,
        EQIntHeap,
        NEIntHeap,
        GTRealHeap,
        LTRealHeap,
        GERealHeap,
        LERealHeap,
        EQRealHeap,
        NERealHeap,
        ANDIntHeap,
        ORIntHeap,
        XORIntHeap,
        AddRealStack,
        AddIntStack,
        SubRealStack,
        SubIntStack,
        MultRealStack,
        MultIntStack,
        DivRealStack,
        DivIntStack,
        RemRealStack,
        RemIntStack,
        LshIntStack,
        ARshIntStack,
        LRshIntStack,
        GTIntStack,
        LTIntStack,
        GEIntStack,
        LEIntStack,
        EQIntStack,
        NEIntStack,
        GTRealStack,
        LTRealStack,
        GERealStack,
        LERealStack,
        EQRealStack,
        NERealStack,
        ANDIntStack,
        ORIntStack,
        XORIntStack,
        AddRealStackValue,
        AddIntStackValue,
        SubRealStackValue,
        SubIntStackValue,
        MultRealStackValue,
        MultIntStackValue,
        DivRealStackValue,
        DivIntStackValue,
        RemRealStackValue,
        RemIntStackValue,
        LshIntStackValue,
        ARshIntStackValue,
        LRshIntStackValue,
        GTIntStackValue,
        LTIntStackValue,
        GEIntStackValue,
        LEIntStackValue,
        EQIntStackValue,
        NEIntStackValue,
        GTRealStackValue,
        LTRealStackValue,
        GERealStackValue,
        LERealStackValue,
        EQRealStackValue,
        NERealStackValue,
        ANDIntStackValue,
        ORIntStackValue,
        XORIntStackValue,
        AddRealValue,
        AddIntValue,
        SubRealValue,
        SubIntValue,
        MultRealValue,
        MultIntValue,
        DivRealValue,
        DivIntValue,
        RemRealValue,
        RemIntValue,
        LshIntValue,
        ARshIntValue,
        LRshIntValue,
        GTIntValue,
        LTIntValue,
        GEIntValue,
        LEIntValue,
        EQIntValue,
        NEIntValue,
        GTRealValue,
        LTRealValue,
        GERealValue,
        LERealValue,
        EQRealValue,
        NERealValue,
        ANDIntValue,
        ORIntValue,
        XORIntValue,
        SubRealValueInvert,
        SubIntValueInvert,
        DivRealValueInvert,
        DivIntValueInvert,
        RemRealValueInvert,
        RemIntValueInvert,
        LshIntValueInvert,
        ARshIntValueInvert,
        LRshIntValueInvert,
        GTIntValueInvert,
        LTIntValueInvert,
        GEIntValueInvert,
        LEIntValueInvert,
        GTRealValueInvert,
        LTRealValueInvert,
        GERealValueInvert,
        LERealValueInvert,
        Abs,
        Absf,
        Acosf,
        Acoshf,
        Asinf,
        Asinhf,
        Atanf,
        Atanhf,
        Ceilf,
        Cosf,
        Coshf,
        Expf,
        Floorf,
        Logf,
        Log10f,
        Rintf,
        Roundf,
        Sinf,
        Sinhf,
        Sqrtf,
        Tanf,
        Tanhf,
        Isnanf,
        Isinff,
        AbsHeap,
        AbsfHeap,
        AcosfHeap,
        AcoshfHeap,
        AsinfHeap,
        AsinhfHeap,
        AtanfHeap,
        AtanhfHeap,
        CeilfHeap,
        CosfHeap,
        CoshfHeap,
        ExpfHeap,
        FloorfHeap,
        LogfHeap,
        Log10fHeap,
        RintfHeap,
        RoundfHeap,
        SinfHeap,
        SinhfHeap,
        SqrtfHeap,
        TanfHeap,
        TanhfHeap,
        Atan2f,
        Fmodf,
        Powf,
        Max,
        Maxf,
        Min,
        Minf,
        Copysignf,
        Atan2fHeap,
        FmodfHeap,
        PowfHeap,
        MaxHeap,
        MaxfHeap,
        MinHeap,
        MinfHeap,
        Atan2fStack,
        FmodfStack,
        PowfStack,
        MaxStack,
        MaxfStack,
        MinStack,
        MinfStack,
        Atan2fStackValue,
        FmodfStackValue,
        PowfStackValue,
        MaxStackValue,
        MaxfStackValue,
        MinStackValue,
        MinfStackValue,
        Atan2fValue,
        FmodfValue,
        PowfValue,
        MaxValue,
        MaxfValue,
        MinValue,
        MinfValue,
        Atan2fValueInvert,
        FmodfValueInvert,
        PowfValueInvert,
        Loop,
        Return,
        If,
        SelectReal,
        SelectInt,
        CondBranch,
        OpenVerticalBox,
        OpenHorizontalBox,
        OpenTabBox,
        CloseBox,
        AddButton,
        AddCheckButton,
        AddHorizontalSlider,
        AddVerticalSlider,
        AddNumEntry,
        AddSoundfile,
        AddHorizontalBargraph,
        AddVerticalBargraph,
        Declare,
        Nop,
    ]
};

impl FbcOpcode {
    /// Returns the opcode name as it appears in the `.fbc` text format.
    ///
    /// Uses [`FBC_INSTRUCTION_NAMES`] for C++ parity.
    #[must_use]
    pub fn name(self) -> &'static str {
        FBC_INSTRUCTION_NAMES[self as usize]
    }

    /// Converts a raw `u16` discriminant to an opcode, if in range.
    #[must_use]
    pub fn from_u16(v: u16) -> Option<Self> {
        // Lookup table generated at compile time (no unsafe transmute).
        FROM_U16_TABLE.get(v as usize).copied()
    }

    /// Returns `true` if this opcode operates on real (floating-point) values.
    ///
    /// # Source provenance (C++)
    /// - `FBCInstruction::isRealType()` in `fbc_opcode.hh`
    #[must_use]
    pub fn is_real_type(self) -> bool {
        matches!(
            self,
            Self::RealValue
                | Self::LoadReal
                | Self::LoadIndexedReal
                | Self::LoadSoundFieldReal
                | Self::LoadInput
                | Self::CastReal
                | Self::BitcastReal
                | Self::SelectReal
                | Self::AddReal
                | Self::SubReal
                | Self::MultReal
                | Self::DivReal
                | Self::RemReal
                | Self::Absf
                | Self::Acosf
                | Self::Acoshf
                | Self::Asinf
                | Self::Asinhf
                | Self::Atanf
                | Self::Atanhf
                | Self::Ceilf
                | Self::Cosf
                | Self::Coshf
                | Self::Expf
                | Self::Floorf
                | Self::Logf
                | Self::Log10f
                | Self::Rintf
                | Self::Roundf
                | Self::Sinf
                | Self::Sinhf
                | Self::Sqrtf
                | Self::Tanf
                | Self::Tanhf
                | Self::Atan2f
                | Self::Fmodf
                | Self::Powf
                | Self::Maxf
                | Self::Minf
                | Self::Copysignf
        )
    }

    /// Returns `true` if this is a standard binary math opcode (stack OP stack).
    ///
    /// # Source provenance (C++)
    /// - `FBCInstruction::isMath()` in `fbc_opcode.hh`
    #[must_use]
    pub fn is_math(self) -> bool {
        let v = self as u16;
        v >= Self::AddReal as u16 && v <= Self::XORInt as u16
    }

    /// Returns `true` if this is an extended unary math opcode (stack version).
    ///
    /// Note: `Isnanf` and `Isinff` are excluded (not optimized in C++).
    ///
    /// # Source provenance (C++)
    /// - `FBCInstruction::isExtendedUnaryMath()` in `fbc_opcode.hh`
    #[must_use]
    pub fn is_extended_unary_math(self) -> bool {
        let v = self as u16;
        v >= Self::Abs as u16 && v <= Self::Tanhf as u16
    }

    /// Returns `true` if this is an extended binary math opcode (stack version).
    ///
    /// Note: `Copysignf` is excluded (not optimized in C++).
    ///
    /// # Source provenance (C++)
    /// - `FBCInstruction::isExtendedBinaryMath()` in `fbc_opcode.hh`
    #[must_use]
    pub fn is_extended_binary_math(self) -> bool {
        let v = self as u16;
        v >= Self::Atan2f as u16 && v <= Self::Minf as u16
    }

    /// Returns `true` if this is a choice/control opcode.
    ///
    /// # Source provenance (C++)
    /// - `FBCInstruction::isChoice()` in `fbc_opcode.hh`
    #[must_use]
    pub fn is_choice(self) -> bool {
        matches!(self, Self::If | Self::SelectReal | Self::SelectInt)
    }
}

/// Instruction name table for `.fbc` text format serialization.
///
/// This table replicates the C++ `gFBCInstructionTable[]` from `fbc_opcode.hh`
/// **exactly**, including known typos in the C++ source (marked with comments).
/// This is required for cross-compiler `.fbc` format compatibility.
///
/// # Known C++ typos replicated here
/// - Index 182: C++ has `"kLTIntValueInvert"` for enum `kGEIntValueInvert`
/// - Index 261: C++ has `"kMaxStackfValue"` for enum `kMaxfStackValue`
/// - Index 285: C++ has `"kAddChecButton"` for enum `kAddCheckButton`
pub static FBC_INSTRUCTION_NAMES: [&str; FBC_OPCODE_COUNT] = [
    // ── Numbers ──
    "kRealValue",  // 0
    "kInt32Value", // 1
    // ── Memory ──
    "kLoadReal",           // 2
    "kLoadInt",            // 3
    "kLoadSoundFieldInt",  // 4
    "kLoadSoundFieldReal", // 5
    "kStoreReal",          // 6
    "kStoreInt",           // 7
    "kStoreRealValue",     // 8
    "kStoreIntValue",      // 9
    "kLoadIndexedReal",    // 10
    "kLoadIndexedInt",     // 11
    "kStoreIndexedReal",   // 12
    "kStoreIndexedInt",    // 13
    "kBlockStoreReal",     // 14
    "kBlockStoreInt",      // 15
    "kMoveReal",           // 16
    "kMoveInt",            // 17
    "kPairMoveReal",       // 18
    "kPairMoveInt",        // 19
    "kBlockPairMoveReal",  // 20
    "kBlockPairMoveInt",   // 21
    "kBlockShiftReal",     // 22
    "kBlockShiftInt",      // 23
    "kLoadInput",          // 24
    "kStoreOutput",        // 25
    // ── Cast / Bitcast ──
    "kCastReal",     // 26
    "kCastInt",      // 27
    "kCastRealHeap", // 28
    "kCastIntHeap",  // 29
    "kBitcastInt",   // 30
    "kBitcastReal",  // 31
    // ── Standard math (stack OP stack) ──
    "kAddReal",  // 32
    "kAddInt",   // 33
    "kSubReal",  // 34
    "kSubInt",   // 35
    "kMultReal", // 36
    "kMultInt",  // 37
    "kDivReal",  // 38
    "kDivInt",   // 39
    "kRemReal",  // 40
    "kRemInt",   // 41
    "kLshInt",   // 42
    "kARshInt",  // 43
    "kLRshInt",  // 44
    "kGTInt",    // 45
    "kLTInt",    // 46
    "kGEInt",    // 47
    "kLEInt",    // 48
    "kEQInt",    // 49
    "kNEInt",    // 50
    "kGTReal",   // 51
    "kLTReal",   // 52
    "kGEReal",   // 53
    "kLEReal",   // 54
    "kEQReal",   // 55
    "kNEReal",   // 56
    "kANDInt",   // 57
    "kORInt",    // 58
    "kXORInt",   // 59
    // ── Standard math (heap OP heap) ──
    "kAddRealHeap",  // 60
    "kAddIntHeap",   // 61
    "kSubRealHeap",  // 62
    "kSubIntHeap",   // 63
    "kMultRealHeap", // 64
    "kMultIntHeap",  // 65
    "kDivRealHeap",  // 66
    "kDivIntHeap",   // 67
    "kRemRealHeap",  // 68
    "kRemIntHeap",   // 69
    "kLshIntHeap",   // 70
    "kARshIntHeap",  // 71
    "kLRshIntHeap",  // 72
    "kGTIntHeap",    // 73
    "kLTIntHeap",    // 74
    "kGEIntHeap",    // 75
    "kLEIntHeap",    // 76
    "kEQIntHeap",    // 77
    "kNEIntHeap",    // 78
    "kGTRealHeap",   // 79
    "kLTRealHeap",   // 80
    "kGERealHeap",   // 81
    "kLERealHeap",   // 82
    "kEQRealHeap",   // 83
    "kNERealHeap",   // 84
    "kANDIntHeap",   // 85
    "kORIntHeap",    // 86
    "kXORIntHeap",   // 87
    // ── Standard math (heap OP stack) ──
    "kAddRealStack",  // 88
    "kAddIntStack",   // 89
    "kSubRealStack",  // 90
    "kSubIntStack",   // 91
    "kMultRealStack", // 92
    "kMultIntStack",  // 93
    "kDivRealStack",  // 94
    "kDivIntStack",   // 95
    "kRemRealStack",  // 96
    "kRemIntStack",   // 97
    "kLshIntStack",   // 98
    "kARshIntStack",  // 99
    "kLRshIntStack",  // 100
    "kGTIntStack",    // 101
    "kLTIntStack",    // 102
    "kGEIntStack",    // 103
    "kLEIntStack",    // 104
    "kEQIntStack",    // 105
    "kNEIntStack",    // 106
    "kGTRealStack",   // 107
    "kLTRealStack",   // 108
    "kGERealStack",   // 109
    "kLERealStack",   // 110
    "kEQRealStack",   // 111
    "kNERealStack",   // 112
    "kANDIntStack",   // 113
    "kORIntStack",    // 114
    "kXORIntStack",   // 115
    // ── Standard math (value OP stack) ──
    "kAddRealStackValue",  // 116
    "kAddIntStackValue",   // 117
    "kSubRealStackValue",  // 118
    "kSubIntStackValue",   // 119
    "kMultRealStackValue", // 120
    "kMultIntStackValue",  // 121
    "kDivRealStackValue",  // 122
    "kDivIntStackValue",   // 123
    "kRemRealStackValue",  // 124
    "kRemIntStackValue",   // 125
    "kLshIntStackValue",   // 126
    "kARshIntStackValue",  // 127
    "kLRshIntStackValue",  // 128
    "kGTIntStackValue",    // 129
    "kLTIntStackValue",    // 130
    "kGEIntStackValue",    // 131
    "kLEIntStackValue",    // 132
    "kEQIntStackValue",    // 133
    "kNEIntStackValue",    // 134
    "kGTRealStackValue",   // 135
    "kLTRealStackValue",   // 136
    "kGERealStackValue",   // 137
    "kLERealStackValue",   // 138
    "kEQRealStackValue",   // 139
    "kNERealStackValue",   // 140
    "kANDIntStackValue",   // 141
    "kORIntStackValue",    // 142
    "kXORIntStackValue",   // 143
    // ── Standard math (value OP heap) ──
    "kAddRealValue",  // 144
    "kAddIntValue",   // 145
    "kSubRealValue",  // 146
    "kSubIntValue",   // 147
    "kMultRealValue", // 148
    "kMultIntValue",  // 149
    "kDivRealValue",  // 150
    "kDivIntValue",   // 151
    "kRemRealValue",  // 152
    "kRemIntValue",   // 153
    "kLshIntValue",   // 154
    "kARshIntValue",  // 155
    "kLRshIntValue",  // 156
    "kGTIntValue",    // 157
    "kLTIntValue",    // 158
    "kGEIntValue",    // 159
    "kLEIntValue",    // 160
    "kEQIntValue",    // 161
    "kNEIntValue",    // 162
    "kGTRealValue",   // 163
    "kLTRealValue",   // 164
    "kGERealValue",   // 165
    "kLERealValue",   // 166
    "kEQRealValue",   // 167
    "kNERealValue",   // 168
    "kANDIntValue",   // 169
    "kORIntValue",    // 170
    "kXORIntValue",   // 171
    // ── Standard math (value OP heap) : non-commutative inverted ──
    "kSubRealValueInvert", // 172
    "kSubIntValueInvert",  // 173
    "kDivRealValueInvert", // 174
    "kDivIntValueInvert",  // 175
    "kRemRealValueInvert", // 176
    "kRemIntValueInvert",  // 177
    "kLshIntValueInvert",  // 178
    "kARshIntValueInvert", // 179
    "kLRshIntValueInvert", // 180
    "kGTIntValueInvert",   // 181
    "kLTIntValueInvert",   // 182  (C++ typo: says "kLTIntValueInvert" for kGEIntValueInvert)
    "kLTIntValueInvert",   // 183  ← C++ bug: duplicates index 182 instead of "kGEIntValueInvert"
    "kLEIntValueInvert",   // 184
    "kGTRealValueInvert",  // 185
    "kLTRealValueInvert",  // 186
    "kGERealValueInvert",  // 187
    "kLERealValueInvert",  // 188
    // ── Extended unary math (stack) ──
    "kAbs",    // 189
    "kAbsf",   // 190
    "kAcosf",  // 191
    "kAcoshf", // 192
    "kAsinf",  // 193
    "kAsinhf", // 194
    "kAtanf",  // 195
    "kAtanhf", // 196
    "kCeilf",  // 197
    "kCosf",   // 198
    "kCoshf",  // 199
    "kExpf",   // 200
    "kFloorf", // 201
    "kLogf",   // 202
    "kLog10f", // 203
    "kRintf",  // 204
    "kRoundf", // 205
    "kSinf",   // 206
    "kSinhf",  // 207
    "kSqrtf",  // 208
    "kTanf",   // 209
    "kTanhf",  // 210
    "kIsnanf", // 211
    "kIsinff", // 212
    // ── Extended unary math (heap OP) ──
    "kAbsHeap",    // 213
    "kAbsfHeap",   // 214
    "kAcosfHeap",  // 215
    "kAcoshfHeap", // 216
    "kAsinfHeap",  // 217
    "kAsinhfHeap", // 218
    "kAtanfHeap",  // 219
    "kAtanhfHeap", // 220
    "kCeilfHeap",  // 221
    "kCosfHeap",   // 222
    "kCoshfHeap",  // 223
    "kExpfHeap",   // 224
    "kFloorfHeap", // 225
    "kLogfHeap",   // 226
    "kLog10fHeap", // 227
    "kRintfHeap",  // 228
    "kRoundfHeap", // 229
    "kSinfHeap",   // 230
    "kSinhfHeap",  // 231
    "kSqrtfHeap",  // 232
    "kTanfHeap",   // 233
    "kTanhfHeap",  // 234
    // ── Extended binary math (stack OP stack) ──
    "kAtan2f",    // 235
    "kFmodf",     // 236
    "kPowf",      // 237
    "kMax",       // 238
    "kMaxf",      // 239
    "kMin",       // 240
    "kMinf",      // 241
    "kCopysignf", // 242
    // ── Extended binary math (heap OP heap) ──
    "kAtan2fHeap", // 243
    "kFmodfHeap",  // 244
    "kPowfHeap",   // 245
    "kMaxHeap",    // 246
    "kMaxfHeap",   // 247
    "kMinHeap",    // 248
    "kMinfHeap",   // 249
    // ── Extended binary math (heap OP stack) ──
    "kAtan2fStack", // 250
    "kFmodfStack",  // 251
    "kPowfStack",   // 252
    "kMaxStack",    // 253
    "kMaxfStack",   // 254
    "kMinStack",    // 255
    "kMinfStack",   // 256
    // ── Extended binary math (value OP stack) ──
    "kAtan2fStackValue", // 257
    "kFmodfStackValue",  // 258
    "kPowfStackValue",   // 259
    "kMaxStackValue",    // 260
    "kMaxStackfValue",   // 261  ← C++ bug: should be "kMaxfStackValue"
    "kMinStackValue",    // 262
    "kMinfStackValue",   // 263
    // ── Extended binary math (value OP heap) ──
    "kAtan2fValue", // 264
    "kFmodfValue",  // 265
    "kPowfValue",   // 266
    "kMaxValue",    // 267
    "kMaxfValue",   // 268
    "kMinValue",    // 269
    "kMinfValue",   // 270
    // ── Extended binary math (value OP heap) : non-commutative inverted ──
    "kAtan2fValueInvert", // 271
    "kFmodfValueInvert",  // 272
    "kPowfValueInvert",   // 273
    // ── Control ──
    "kLoop",   // 274
    "kReturn", // 275
    // ── Select / if ──
    "kIf",         // 276
    "kSelectReal", // 277
    "kSelectInt",  // 278
    "kCondBranch", // 279
    // ── User Interface ──
    "kOpenVerticalBox",       // 280
    "kOpenHorizontalBox",     // 281
    "kOpenTabBox",            // 282
    "kCloseBox",              // 283
    "kAddButton",             // 284
    "kAddChecButton",         // 285  ← C++ bug: should be "kAddCheckButton"
    "kAddHorizontalSlider",   // 286
    "kAddVerticalSlider",     // 287
    "kAddNumEntry",           // 288
    "kAddSoundfile",          // 289
    "kAddHorizontalBargraph", // 290
    "kAddVerticalBargraph",   // 291
    "kDeclare",               // 292
    // ── Misc ──
    "kNop", // 293
];

#[cfg(test)]
mod tests {
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
        assert_eq!(FbcOpcode::Nop as u16, (FBC_OPCODE_COUNT - 1) as u16);
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
        assert_eq!(FbcOpcode::Nop.name(), "kNop");
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

    /// Verify name table replicates known C++ typos for format compatibility.
    #[test]
    fn cpp_typos_replicated() {
        // kGEIntValueInvert (index 183) → "kLTIntValueInvert" in C++
        assert_eq!(
            FBC_INSTRUCTION_NAMES[FbcOpcode::GEIntValueInvert as usize],
            "kLTIntValueInvert"
        );
        // kMaxfStackValue (index 261) → "kMaxStackfValue" in C++
        assert_eq!(
            FBC_INSTRUCTION_NAMES[FbcOpcode::MaxfStackValue as usize],
            "kMaxStackfValue"
        );
        // kAddCheckButton (index 285) → "kAddChecButton" in C++
        assert_eq!(
            FBC_INSTRUCTION_NAMES[FbcOpcode::AddCheckButton as usize],
            "kAddChecButton"
        );
    }
}
