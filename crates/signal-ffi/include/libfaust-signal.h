#ifndef LIBFAUST_SIGNAL_H
#define LIBFAUST_SIGNAL_H

/*
 * C++ convenience interface for the libfaust Signal API.
 *
 * The reference Faust C++ header exposes named C++ functions over the same tree
 * API. This Rust-port header keeps that shape as thin inline wrappers over
 * `libfaust-signal-c.h`, preserving the C ABI as the single implementation
 * boundary.
 */

#include "libfaust-signal-c.h"

#ifdef __cplusplus
#include <string>

/* Common helpers shared with libfaust-box.h. */
#ifndef LIBFAUST_COMMON_CPP_WRAPPERS_H
#define LIBFAUST_COMMON_CPP_WRAPPERS_H
inline bool isNil(Signal s) { return CisNil(s); }
inline const char* tree2str(Signal s) { return Ctree2str(s); }
inline void* getUserData(Signal s) { return CgetUserData(s); }
#endif

/* Core Signal constructors: constants, inputs, delays, casts, and tables. */
inline Signal sigInt(int n) { return CsigInt(n); }
inline Signal sigInt64(int64_t n) { return CsigInt64(n); }
inline Signal sigReal(double n) { return CsigReal(n); }
inline Signal sigInput(int idx) { return CsigInput(idx); }
inline Signal sigDelay(Signal s, Signal del) { return CsigDelay(s, del); }
inline Signal sigDelay1(Signal s) { return CsigDelay1(s); }
inline Signal sigIntCast(Signal s) { return CsigIntCast(s); }
inline Signal sigFloatCast(Signal s) { return CsigFloatCast(s); }
inline Signal sigReadOnlyTable(Signal n, Signal init, Signal ridx) { return CsigReadOnlyTable(n, init, ridx); }
inline Signal sigWriteReadTable(Signal n, Signal init, Signal widx, Signal wsig, Signal ridx) { return CsigWriteReadTable(n, init, widx, wsig, ridx); }
inline Signal sigWaveform(Signal* wf) { return CsigWaveform(wf); }

/* Soundfile selectors and buffer accessors. */
inline Signal sigSoundfile(const char* label) { return CsigSoundfile(label); }
inline Signal sigSoundfile(const std::string& label) { return CsigSoundfile(label.c_str()); }
inline Signal sigSoundfileLength(Signal sf, Signal part) { return CsigSoundfileLength(sf, part); }
inline Signal sigSoundfileRate(Signal sf, Signal part) { return CsigSoundfileRate(sf, part); }
inline Signal sigSoundfileBuffer(Signal sf, Signal chan, Signal part, Signal ridx) { return CsigSoundfileBuffer(sf, chan, part, ridx); }
inline Signal sigSelect2(Signal selector, Signal s1, Signal s2) { return CsigSelect2(selector, s1, s2); }
inline Signal sigSelect3(Signal selector, Signal s1, Signal s2, Signal s3) { return CsigSelect3(selector, s1, s2, s3); }
inline Signal sigFFun(enum SType rtype, const char** names, enum SType* atypes, const char* incfile, const char* libfile, Signal* largs) { return CsigFFun(rtype, names, atypes, incfile, libfile, largs); }
inline Signal sigFConst(enum SType type, const char* name, const char* file) { return CsigFConst(type, name, file); }
inline Signal sigFVar(enum SType type, const char* name, const char* file) { return CsigFVar(type, name, file); }
inline Signal sigBinOp(enum SOperator op, Signal x, Signal y) { return CsigBinOp(op, x, y); }

/* Unary and binary math wrappers generated from the C API names. */
#define FAUST_SIGNAL_WRAP_1(name) inline Signal name(Signal x) { return C##name(x); }
#define FAUST_SIGNAL_WRAP_2(name) inline Signal name(Signal x, Signal y) { return C##name(x, y); }

FAUST_SIGNAL_WRAP_2(sigAdd)
FAUST_SIGNAL_WRAP_2(sigSub)
FAUST_SIGNAL_WRAP_2(sigMul)
FAUST_SIGNAL_WRAP_2(sigDiv)
FAUST_SIGNAL_WRAP_2(sigRem)
FAUST_SIGNAL_WRAP_2(sigLeftShift)
FAUST_SIGNAL_WRAP_2(sigLRightShift)
FAUST_SIGNAL_WRAP_2(sigARightShift)
FAUST_SIGNAL_WRAP_2(sigGT)
FAUST_SIGNAL_WRAP_2(sigLT)
FAUST_SIGNAL_WRAP_2(sigGE)
FAUST_SIGNAL_WRAP_2(sigLE)
FAUST_SIGNAL_WRAP_2(sigEQ)
FAUST_SIGNAL_WRAP_2(sigNE)
FAUST_SIGNAL_WRAP_2(sigAND)
FAUST_SIGNAL_WRAP_2(sigOR)
FAUST_SIGNAL_WRAP_2(sigXOR)
FAUST_SIGNAL_WRAP_1(sigAbs)
FAUST_SIGNAL_WRAP_1(sigAcos)
FAUST_SIGNAL_WRAP_1(sigTan)
FAUST_SIGNAL_WRAP_1(sigSqrt)
FAUST_SIGNAL_WRAP_1(sigSin)
FAUST_SIGNAL_WRAP_1(sigRint)
FAUST_SIGNAL_WRAP_1(sigLog)
FAUST_SIGNAL_WRAP_1(sigLog10)
FAUST_SIGNAL_WRAP_1(sigFloor)
FAUST_SIGNAL_WRAP_1(sigExp)
FAUST_SIGNAL_WRAP_1(sigExp10)
FAUST_SIGNAL_WRAP_1(sigCos)
FAUST_SIGNAL_WRAP_1(sigCeil)
FAUST_SIGNAL_WRAP_1(sigAtan)
FAUST_SIGNAL_WRAP_1(sigAsin)
FAUST_SIGNAL_WRAP_2(sigRemainder)
FAUST_SIGNAL_WRAP_2(sigPow)
FAUST_SIGNAL_WRAP_2(sigMin)
FAUST_SIGNAL_WRAP_2(sigMax)
FAUST_SIGNAL_WRAP_2(sigFmod)
FAUST_SIGNAL_WRAP_2(sigAtan2)
FAUST_SIGNAL_WRAP_2(sigAttach)

#undef FAUST_SIGNAL_WRAP_1
#undef FAUST_SIGNAL_WRAP_2

/* Recursion, UI, and attachment wrappers. */
inline Signal sigSelf() { return CsigSelf(); }
inline Signal sigSelfN(int id) { return CsigSelfN(id); }
inline Signal sigRecursion(Signal s) { return CsigRecursion(s); }
inline Signal* sigRecursionN(Signal* rf) { return CsigRecursionN(rf); }
inline Signal sigButton(const char* label) { return CsigButton(label); }
inline Signal sigButton(const std::string& label) { return CsigButton(label.c_str()); }
inline Signal sigCheckbox(const char* label) { return CsigCheckbox(label); }
inline Signal sigCheckbox(const std::string& label) { return CsigCheckbox(label.c_str()); }
inline Signal sigVSlider(const char* label, Signal init, Signal min, Signal max, Signal step) { return CsigVSlider(label, init, min, max, step); }
inline Signal sigVSlider(const std::string& label, Signal init, Signal min, Signal max, Signal step) { return CsigVSlider(label.c_str(), init, min, max, step); }
inline Signal sigHSlider(const char* label, Signal init, Signal min, Signal max, Signal step) { return CsigHSlider(label, init, min, max, step); }
inline Signal sigHSlider(const std::string& label, Signal init, Signal min, Signal max, Signal step) { return CsigHSlider(label.c_str(), init, min, max, step); }
inline Signal sigNumEntry(const char* label, Signal init, Signal min, Signal max, Signal step) { return CsigNumEntry(label, init, min, max, step); }
inline Signal sigNumEntry(const std::string& label, Signal init, Signal min, Signal max, Signal step) { return CsigNumEntry(label.c_str(), init, min, max, step); }
inline Signal sigVBargraph(const char* label, Signal min, Signal max, Signal s) { return CsigVBargraph(label, min, max, s); }
inline Signal sigHBargraph(const char* label, Signal min, Signal max, Signal s) { return CsigHBargraph(label, min, max, s); }

/* Structural Signal matchers. Output references receive context-owned handles. */
inline bool isSigInt(Signal s, int& i) { return CisSigInt(s, &i); }
inline bool isSigInt64(Signal s, int64_t& i) { return CisSigInt64(s, &i); }
inline bool isSigReal(Signal s, double& r) { return CisSigReal(s, &r); }
inline bool isSigInput(Signal s, int& i) { return CisSigInput(s, &i); }
inline bool isSigBinOp(Signal s, int& op, Signal& x, Signal& y) { return CisSigBinOp(s, &op, &x, &y); }
inline bool isProj(Signal s, int& i, Signal& rgroup) { return CisProj(s, &i, &rgroup); }
inline bool isRec(Signal s, Signal& var, Signal& body) { return CisRec(s, &var, &body); }

#define FAUST_SIGNAL_IS_1(name) inline bool name(Signal s, Signal& x) { return C##name(s, &x); }
#define FAUST_SIGNAL_IS_2(name, a, b) inline bool name(Signal s, Signal& a, Signal& b) { return C##name(s, &a, &b); }

FAUST_SIGNAL_IS_1(isSigDelay1)
FAUST_SIGNAL_IS_2(isSigDelay, x, y)
FAUST_SIGNAL_IS_2(isSigAttach, x, y)
FAUST_SIGNAL_IS_1(isSigIntCast)
FAUST_SIGNAL_IS_1(isSigFloatCast)
FAUST_SIGNAL_IS_1(isSigButton)
FAUST_SIGNAL_IS_1(isSigCheckbox)

#undef FAUST_SIGNAL_IS_1
#undef FAUST_SIGNAL_IS_2

/* Normal-form and source-generation wrappers. */
inline bool isSigWaveform(Signal s) { return CisSigWaveform(s); }
inline Signal simplifyToNormalForm(Signal s) { return CsimplifyToNormalForm(s); }
inline Signal* simplifyToNormalForm2(Signal* siglist) { return CsimplifyToNormalForm2(siglist); }
inline char* createSourceFromSignals(const char* name_app, Signal* osigs, const char* lang,
                                     int argc, const char* argv[], char* error_msg) {
    return CcreateSourceFromSignals(name_app, osigs, lang, argc, argv, error_msg);
}

#endif

#endif
