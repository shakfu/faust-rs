/* cranelift-dsp-c.h — Phase 1 scaffold placeholder
 *
 * Planned role:
 * - C API for the `cranelift_dsp` runtime/factory family.
 * - V1 parity target: same exported function-set strategy and cache/factory
 *   lifecycle strategy as `llvm_dsp` / `interpreter_dsp`.
 *
 * This header is intentionally incomplete in Phase 1. The exact function list
 * will be filled from the mandatory Phase-0 export parity matrix
 * (`porting/cranelift-dsp-ffi-parity-matrix-en.md`).
 *
 * Locked naming convention (user decision):
 * - backend-prefixed C API names, e.g.
 *   `createCCraneliftDSPFactoryFromFile`, `createCCraneliftDSPInstance`.
 */

#ifndef FAUST_CRANELIFT_DSP_C_H
#define FAUST_CRANELIFT_DSP_C_H

#include <stdbool.h>

/* Matches the Faust C API callback glue definitions (`UIGlue`, `MetaGlue`,
 * `MemoryManagerGlue`, `FAUSTFLOAT`).
 */
#include "faust/gui/CInterface.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct cranelift_dsp_factory cranelift_dsp_factory;
typedef struct cranelift_dsp cranelift_dsp;

/* Version (scaffold implementation returns a process-lifetime static string). */
const char* getCLibFaustVersion(void);

/* Factory creation (source path/string wired to scaffold runtime shape only).
 *
 * User-locked signature policy for Cranelift source-creation APIs:
 * - keep `opt_level` when Cranelift optimization levels are exposed
 * - do not carry LLVM-specific `target` string parameter
 */
cranelift_dsp_factory* createCCraneliftDSPFactoryFromFile(const char* filename,
                                                          int argc,
                                                          const char* argv[],
                                                          char* error_msg,
                                                          int opt_level);
cranelift_dsp_factory* createCCraneliftDSPFactoryFromString(const char* name_app,
                                                            const char* dsp_content,
                                                            int argc,
                                                            const char* argv[],
                                                            char* error_msg,
                                                            int opt_level);
/* Present in V1 surface, not implemented yet (returns null + error message). */
cranelift_dsp_factory* createCCraneliftDSPFactoryFromSignals(const char* name_app,
                                                             void* signals,
                                                             int argc,
                                                             const char* argv[],
                                                             char* error_msg,
                                                             int opt_level);
cranelift_dsp_factory* createCCraneliftDSPFactoryFromBoxes(const char* name_app,
                                                           void* box_expr,
                                                           int argc,
                                                           const char* argv[],
                                                           char* error_msg,
                                                           int opt_level);

/* Factory cache / lifecycle (minimal scaffold cache is wired). */
cranelift_dsp_factory* getCCraneliftDSPFactoryFromSHAKey(const char* sha_key);
bool deleteCCraneliftDSPFactory(cranelift_dsp_factory* factory);
void deleteAllCCraneliftDSPFactories(void);
char** getAllCCraneliftDSPFactories(void);

/* Factory queries (scaffold values for now, caller frees strings with freeCMemory). */
char* getCCraneliftDSPFactoryName(cranelift_dsp_factory* factory);
char* getCCraneliftDSPFactorySHAKey(cranelift_dsp_factory* factory);
char* getCCraneliftDSPFactoryDSPCode(cranelift_dsp_factory* factory);
char* getCCraneliftDSPFactoryJSON(cranelift_dsp_factory* factory);
char* getCCraneliftDSPFactoryCompileOptions(cranelift_dsp_factory* factory);
const char** getCCraneliftDSPFactoryLibraryList(cranelift_dsp_factory* factory);
const char** getCCraneliftDSPFactoryIncludePathnames(cranelift_dsp_factory* factory);
const char** getCCraneliftDSPFactoryWarningMessages(cranelift_dsp_factory* factory);

/* Cranelift backend bitcode family (temporary scaffold format implemented).
 *
 * The current Rust implementation uses a temporary text format marker
 * (`CRANELIFT_FFI_SCAFFOLD_V1`) for API-family validation only. It is not the
 * final backend serialization format.
 */
cranelift_dsp_factory* readCCraneliftDSPFactoryFromBitcode(const char* bit_code,
                                                           char* error_msg);
char* writeCCraneliftDSPFactoryToBitcode(cranelift_dsp_factory* factory);
cranelift_dsp_factory* readCCraneliftDSPFactoryFromBitcodeFile(const char* bit_code_path,
                                                               char* error_msg);
bool writeCCraneliftDSPFactoryToBitcodeFile(cranelift_dsp_factory* factory,
                                            const char* bit_code_path);

/* Multi-thread mode compatibility flag (cache guarded independently in scaffold). */
bool startMTDSPFactories(void);
void stopMTDSPFactories(void);

/* Foreign-function registration for DSPs using `ffunction(...)`. */
void registerCCraneliftForeignFunction(const char* name, void* fn_ptr);
void unregisterCCraneliftForeignFunction(const char* name);
void clearCCraneliftForeignFunctions(void);

/* Memory allocated by this library (currently strings; array ownership still scaffold-level). */
void freeCMemory(void* ptr);

/* DSP instance lifecycle and DSP methods (scaffold behavior, symbol set present). */
cranelift_dsp* createCCraneliftDSPInstance(cranelift_dsp_factory* factory);
void deleteCCraneliftDSPInstance(cranelift_dsp* dsp);
cranelift_dsp* cloneCCraneliftDSPInstance(cranelift_dsp* dsp);
int getNumInputsCCraneliftDSPInstance(cranelift_dsp* dsp);
int getNumOutputsCCraneliftDSPInstance(cranelift_dsp* dsp);
int getSampleRateCCraneliftDSPInstance(cranelift_dsp* dsp);
void initCCraneliftDSPInstance(cranelift_dsp* dsp, int sample_rate);
void instanceInitCCraneliftDSPInstance(cranelift_dsp* dsp, int sample_rate);
void instanceConstantsCCraneliftDSPInstance(cranelift_dsp* dsp, int sample_rate);
void instanceResetUserInterfaceCCraneliftDSPInstance(cranelift_dsp* dsp);
void instanceClearCCraneliftDSPInstance(cranelift_dsp* dsp);
void buildUserInterfaceCCraneliftDSPInstance(cranelift_dsp* dsp, UIGlue* ui);
void metadataCCraneliftDSPInstance(cranelift_dsp* dsp, MetaGlue* meta);
void computeCCraneliftDSPInstance(cranelift_dsp* dsp,
                                  int count,
                                  FAUSTFLOAT** inputs,
                                  FAUSTFLOAT** outputs);

/* Explicitly omitted from this header in V1 (deferred without symbols):
 * - LLVM-only IR/machine/object serialization families
 * - target getter/query functions (`...FactoryTarget`, `...MachineTarget`)
 * - memory-manager registration families
 */

#ifdef __cplusplus
}
#endif

#endif /* FAUST_CRANELIFT_DSP_C_H */
