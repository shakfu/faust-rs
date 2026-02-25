/* cranelift-dsp-c.h — Phase 1 scaffold placeholder
 *
 * Planned role:
 * - C API for the `cranelift_dsp` runtime/factory family.
 * - V1 parity target: same exported function-set strategy and cache/factory
 *   lifecycle strategy as `llvm_dsp` / `interpreter_dsp`.
 *
 * This header is intentionally incomplete in Phase 1. The exact function list
 * will be filled from the mandatory Phase-0 export parity matrix.
 */

#ifndef FAUST_CRANELIFT_DSP_C_H
#define FAUST_CRANELIFT_DSP_C_H

#ifdef __cplusplus
extern "C" {
#endif

typedef struct cranelift_dsp_factory cranelift_dsp_factory;
typedef struct cranelift_dsp cranelift_dsp;

/* Placeholder version accessor for scaffold smoke wiring. */
const char* getCLibFaustVersion(void);

#ifdef __cplusplus
}
#endif

#endif /* FAUST_CRANELIFT_DSP_C_H */
