//! `signal-ffi` — C/C++ export layer for Faust signal construction/matching.
//!
//! This crate owns the `Csig*` and `CisSig*` symbols that map directly onto
//! [`signals::SigBuilder`] and [`signals::match_sig`]. It intentionally shares
//! the process-global [`tree_ffi::FfiTreeContext`] with `box-ffi`, so Signal
//! handles can be printed and freed by the common libfaust helpers.
//!
//! Source provenance: C++ reference header
//! `architecture/faust/dsp/libfaust-signal-c.h` in Faust commit `8eebea429`.

#![allow(unsafe_code)]
#![allow(non_snake_case)] // FFI parity requires preserving C API symbol names.

use std::ffi::{c_int, c_void};

use signals::{BinOp, SigBuilder, SigMatch, match_sig};
use tlib::TreeId;
use tree_ffi::{
    FfiTreeContext, SOperator, with_global_context as with_ctx,
    write_out_handle as unsafe_write_out_signal, write_out_int as unsafe_write_out_int,
    write_out_real as unsafe_write_out_real,
};

fn null_signal() -> *mut c_void {
    std::ptr::null_mut()
}

fn decode_signal(ctx: &FfiTreeContext, signal: *mut c_void) -> Option<TreeId> {
    ctx.decode(signal)
}

fn encode_signal(ctx: &mut FfiTreeContext, signal: TreeId) -> *mut c_void {
    ctx.encode(signal)
}

fn write_out_signal(ctx: &mut FfiTreeContext, out: *mut *mut c_void, value: TreeId) {
    // SAFETY: exported predicate functions receive optional out-pointers from C.
    unsafe { unsafe_write_out_signal(ctx, out, value) }
}

fn write_out_int(out: *mut c_int, value: i32) {
    // SAFETY: exported predicate functions receive optional out-pointers from C.
    unsafe { unsafe_write_out_int(out, value) }
}

fn write_out_real(out: *mut f64, value: f64) {
    // SAFETY: exported predicate functions receive optional out-pointers from C.
    unsafe { unsafe_write_out_real(out, value) }
}

fn soperator_to_binop(op: SOperator) -> BinOp {
    match op {
        SOperator::kAdd => BinOp::Add,
        SOperator::kSub => BinOp::Sub,
        SOperator::kMul => BinOp::Mul,
        SOperator::kDiv => BinOp::Div,
        SOperator::kRem => BinOp::Rem,
        SOperator::kLsh => BinOp::Lsh,
        SOperator::kARsh => BinOp::ARsh,
        SOperator::kLRsh => BinOp::LRsh,
        SOperator::kGT => BinOp::Gt,
        SOperator::kLT => BinOp::Lt,
        SOperator::kGE => BinOp::Ge,
        SOperator::kLE => BinOp::Le,
        SOperator::kEQ => BinOp::Eq,
        SOperator::kNE => BinOp::Ne,
        SOperator::kAND => BinOp::And,
        SOperator::kOR => BinOp::Or,
        SOperator::kXOR => BinOp::Xor,
    }
}

fn binop_to_raw(op: BinOp) -> c_int {
    c_int::try_from(op as i64).unwrap_or_default()
}

fn unary_signal(
    input: *mut c_void,
    build: impl FnOnce(&mut SigBuilder<'_>, TreeId) -> TreeId,
) -> *mut c_void {
    with_ctx(|ctx| {
        let Some(input) = decode_signal(ctx, input) else {
            return null_signal();
        };
        let output = build(&mut SigBuilder::new(&mut ctx.arena), input);
        encode_signal(ctx, output)
    })
}

fn binary_signal(
    left: *mut c_void,
    right: *mut c_void,
    build: impl FnOnce(&mut SigBuilder<'_>, TreeId, TreeId) -> TreeId,
) -> *mut c_void {
    with_ctx(|ctx| {
        let (Some(left), Some(right)) = (decode_signal(ctx, left), decode_signal(ctx, right))
        else {
            return null_signal();
        };
        let output = build(&mut SigBuilder::new(&mut ctx.arena), left, right);
        encode_signal(ctx, output)
    })
}

macro_rules! binary_export {
    ($name:ident, $builder:ident) => {
        #[unsafe(no_mangle)]
        /// Builds one binary Signal node.
        pub extern "C" fn $name(x: *mut c_void, y: *mut c_void) -> *mut c_void {
            binary_signal(x, y, |b, x, y| b.$builder(x, y))
        }
    };
}

macro_rules! unary_export {
    ($name:ident, $builder:ident) => {
        #[unsafe(no_mangle)]
        /// Builds one unary Signal node.
        pub extern "C" fn $name(x: *mut c_void) -> *mut c_void {
            unary_signal(x, |b, x| b.$builder(x))
        }
    };
}

#[unsafe(no_mangle)]
/// Builds an integer Signal constant.
pub extern "C" fn CsigInt(n: c_int) -> *mut c_void {
    with_ctx(|ctx| {
        let output = SigBuilder::new(&mut ctx.arena).int(n);
        encode_signal(ctx, output)
    })
}

#[unsafe(no_mangle)]
/// Builds a 64-bit integer Signal constant.
pub extern "C" fn CsigInt64(n: i64) -> *mut c_void {
    with_ctx(|ctx| {
        let output = ctx.arena.int(n);
        encode_signal(ctx, output)
    })
}

#[unsafe(no_mangle)]
/// Builds a real Signal constant.
pub extern "C" fn CsigReal(n: f64) -> *mut c_void {
    with_ctx(|ctx| {
        let output = SigBuilder::new(&mut ctx.arena).real(n);
        encode_signal(ctx, output)
    })
}

#[unsafe(no_mangle)]
/// Builds one input Signal.
pub extern "C" fn CsigInput(idx: c_int) -> *mut c_void {
    if idx < 0 {
        return null_signal();
    }
    with_ctx(|ctx| {
        let output = SigBuilder::new(&mut ctx.arena).input(idx);
        encode_signal(ctx, output)
    })
}

#[unsafe(no_mangle)]
/// Builds one arbitrary binary operator Signal.
pub extern "C" fn CsigBinOp(op: SOperator, x: *mut c_void, y: *mut c_void) -> *mut c_void {
    binary_signal(x, y, |b, x, y| b.binop(soperator_to_binop(op), x, y))
}

binary_export!(CsigAdd, add);
binary_export!(CsigSub, sub);
binary_export!(CsigMul, mul);
binary_export!(CsigDiv, div);
binary_export!(CsigRem, rem);
binary_export!(CsigLeftShift, lsh);
binary_export!(CsigLRightShift, lrsh);
binary_export!(CsigARightShift, arsh);
binary_export!(CsigGT, gt);
binary_export!(CsigLT, lt);
binary_export!(CsigGE, ge);
binary_export!(CsigLE, le);
binary_export!(CsigEQ, eq);
binary_export!(CsigNE, ne);
binary_export!(CsigAND, and);
binary_export!(CsigOR, or);
binary_export!(CsigXOR, xor);
binary_export!(CsigRemainder, remainder);
binary_export!(CsigPow, pow);
binary_export!(CsigMin, min);
binary_export!(CsigMax, max);
binary_export!(CsigFmod, fmod);
binary_export!(CsigAtan2, atan2);

unary_export!(CsigAbs, abs);
unary_export!(CsigAcos, acos);
unary_export!(CsigTan, tan);
unary_export!(CsigSqrt, sqrt);
unary_export!(CsigSin, sin);
unary_export!(CsigRint, rint);
unary_export!(CsigLog, log);
unary_export!(CsigLog10, log10);
unary_export!(CsigFloor, floor);
unary_export!(CsigExp, exp);
unary_export!(CsigExp10, exp10);
unary_export!(CsigCos, cos);
unary_export!(CsigCeil, ceil);
unary_export!(CsigAtan, atan);
unary_export!(CsigAsin, asin);
unary_export!(CsigDelay1, delay1);
unary_export!(CsigIntCast, int_cast);
unary_export!(CsigFloatCast, float_cast);

#[unsafe(no_mangle)]
/// Builds one explicit delay Signal.
pub extern "C" fn CsigDelay(s: *mut c_void, del: *mut c_void) -> *mut c_void {
    binary_signal(s, del, |b, s, del| b.delay(s, del))
}

#[unsafe(no_mangle)]
/// Matches an integer Signal constant.
pub extern "C" fn CisSigInt(t: *mut c_void, i: *mut c_int) -> bool {
    with_ctx(|ctx| {
        let Some(t) = decode_signal(ctx, t) else {
            return false;
        };
        match match_sig(&ctx.arena, t) {
            SigMatch::Int(value) => {
                write_out_int(i, value);
                true
            }
            _ => false,
        }
    })
}

#[unsafe(no_mangle)]
/// Matches a real Signal constant.
pub extern "C" fn CisSigReal(t: *mut c_void, r: *mut f64) -> bool {
    with_ctx(|ctx| {
        let Some(t) = decode_signal(ctx, t) else {
            return false;
        };
        match match_sig(&ctx.arena, t) {
            SigMatch::Real(value) => {
                write_out_real(r, value);
                true
            }
            _ => false,
        }
    })
}

#[unsafe(no_mangle)]
/// Matches an input Signal.
pub extern "C" fn CisSigInput(t: *mut c_void, i: *mut c_int) -> bool {
    with_ctx(|ctx| {
        let Some(t) = decode_signal(ctx, t) else {
            return false;
        };
        match match_sig(&ctx.arena, t) {
            SigMatch::Input(value) => {
                write_out_int(i, value);
                true
            }
            _ => false,
        }
    })
}

#[unsafe(no_mangle)]
/// Matches a one-sample delay Signal.
pub extern "C" fn CisSigDelay1(t: *mut c_void, t0: *mut *mut c_void) -> bool {
    with_ctx(|ctx| {
        let Some(t) = decode_signal(ctx, t) else {
            return false;
        };
        match match_sig(&ctx.arena, t) {
            SigMatch::Delay1(value) => {
                write_out_signal(ctx, t0, value);
                true
            }
            _ => false,
        }
    })
}

#[unsafe(no_mangle)]
/// Matches an explicit delay Signal.
pub extern "C" fn CisSigDelay(t: *mut c_void, t0: *mut *mut c_void, t1: *mut *mut c_void) -> bool {
    with_ctx(|ctx| {
        let Some(t) = decode_signal(ctx, t) else {
            return false;
        };
        match match_sig(&ctx.arena, t) {
            SigMatch::Delay(left, right) => {
                write_out_signal(ctx, t0, left);
                write_out_signal(ctx, t1, right);
                true
            }
            _ => false,
        }
    })
}

#[unsafe(no_mangle)]
/// Matches a binary operator Signal.
pub extern "C" fn CisSigBinOp(
    s: *mut c_void,
    op: *mut c_int,
    x: *mut *mut c_void,
    y: *mut *mut c_void,
) -> bool {
    with_ctx(|ctx| {
        let Some(s) = decode_signal(ctx, s) else {
            return false;
        };
        match match_sig(&ctx.arena, s) {
            SigMatch::BinOp(binop, left, right) => {
                write_out_int(op, binop_to_raw(binop));
                write_out_signal(ctx, x, left);
                write_out_signal(ctx, y, right);
                true
            }
            _ => false,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;
    use std::ptr;
    use std::sync::{Mutex, MutexGuard};
    use tree_ffi::reset_global_context;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn lock_context() -> MutexGuard<'static, ()> {
        TEST_LOCK.lock().expect("test mutex poisoned")
    }

    unsafe fn print_signal(signal: *mut c_void) -> String {
        let ptr = faust_box::CprintSignal(signal, false, 4096);
        assert!(!ptr.is_null());
        let text = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();
        unsafe { faust_box::freeCMemory(ptr.cast()) };
        text
    }

    #[test]
    fn builds_basic_signals_and_matches_them() {
        let _guard = lock_context();
        reset_global_context();

        let int_sig = CsigInt(7);
        assert!(!int_sig.is_null());
        let mut int_out = 0;
        assert!(CisSigInt(int_sig, &mut int_out));
        assert_eq!(int_out, 7);

        let real_sig = CsigReal(0.5);
        let mut real_out = 0.0;
        assert!(CisSigReal(real_sig, &mut real_out));
        assert_eq!(real_out, 0.5);

        let input_sig = CsigInput(0);
        let mut input_out = -1;
        assert!(CisSigInput(input_sig, &mut input_out));
        assert_eq!(input_out, 0);

        let add_sig = CsigAdd(int_sig, input_sig);
        let mut op = -1;
        let mut lhs = ptr::null_mut();
        let mut rhs = ptr::null_mut();
        assert!(CisSigBinOp(add_sig, &mut op, &mut lhs, &mut rhs));
        assert_eq!(op, SOperator::kAdd as c_int);
        assert_eq!(lhs, int_sig);
        assert_eq!(rhs, input_sig);

        let delay1 = CsigDelay1(input_sig);
        let mut delayed = ptr::null_mut();
        assert!(CisSigDelay1(delay1, &mut delayed));
        assert_eq!(delayed, input_sig);

        let delay = CsigDelay(input_sig, CsigInt(2));
        let mut signal_out = ptr::null_mut();
        let mut amount_out = ptr::null_mut();
        assert!(CisSigDelay(delay, &mut signal_out, &mut amount_out));
        assert_eq!(signal_out, input_sig);
        assert!(CisSigInt(amount_out, &mut int_out));
        assert_eq!(int_out, 2);

        let rendered = unsafe { print_signal(CsigExp10(input_sig)) };
        assert!(rendered.contains("SIGEXP10"));
    }

    #[test]
    fn rejects_null_and_invalid_signal_inputs() {
        let _guard = lock_context();
        reset_global_context();

        assert!(CsigInput(-1).is_null());
        assert!(CsigAdd(ptr::null_mut(), CsigInt(1)).is_null());
        assert!(!CisSigInt(ptr::null_mut(), ptr::null_mut()));
        assert!(!CisSigBinOp(
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut()
        ));
    }
}
