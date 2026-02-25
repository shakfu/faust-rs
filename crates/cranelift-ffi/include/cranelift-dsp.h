/* cranelift-dsp.h — C++ API scaffold for the Cranelift backend
 *
 * Planned role:
 * - C++ wrapper API for `cranelift_dsp` / `cranelift_dsp_factory`.
 * - V1 parity target: same usage strategy as `llvm_dsp` / `interpreter_dsp`,
 *   with Cranelift-specific naming and the V1 deferred families documented in:
 *   `porting/cranelift-dsp-ffi-parity-matrix-en.md`
 *
 * Current status:
 * - Declaration-only scaffold (no C++ implementation in this phase).
 * - The executable scaffold currently exists in Rust as the C ABI layer
 *   (`cranelift-dsp-c.h` + `cranelift-ffi` exports).
 *
 * Important design note:
 * - This header intentionally does NOT include `cranelift-dsp-c.h`.
 *   The C API opaque type names (`cranelift_dsp`, `cranelift_dsp_factory`) would
 *   collide in C++ with the wrapper class names of the same identifiers.
 */

#ifndef FAUST_CRANELIFT_DSP_H
#define FAUST_CRANELIFT_DSP_H

#ifdef _WIN32
#define DEPRECATED(fun) __declspec(deprecated) fun
#else
#define DEPRECATED(fun) fun __attribute__((deprecated));
#endif

#include <string>
#include <vector>

#include "faust/dsp/dsp.h"
#include "faust/dsp/libfaust-box.h"
#include "faust/dsp/libfaust-signal.h"
#include "faust/gui/meta.h"

/*!
 \addtogroup craneliftcpp C++ interface scaffold for compiling Faust code with a Cranelift backend.
 Note that the API is not thread safe: use `startMTDSPFactories/stopMTDSPFactories`
 to coordinate global factory-cache access (same usage strategy as existing backends).
 @{
 */

/**
 * Get the library version.
 *
 * The current Rust scaffold exports a backend-specific placeholder version
 * string through the shared `getCLibFaustVersion` symbol.
 */
extern "C" LIBFAUST_API const char* getCLibFaustVersion();

class cranelift_dsp_factory;

/**
 * DSP instance class (declaration-only scaffold).
 *
 * V1 target surface mirrors `interpreter_dsp` / `llvm_dsp` instance methods:
 * lifecycle, UI, metadata, and compute.
 */
class LIBFAUST_API cranelift_dsp : public dsp {
    private:
        // Opaque C handle (`cranelift_dsp*` from the C API), stored as `void*`
        // to avoid including `cranelift-dsp-c.h` in this header.
        void* fHandle;

        // Wrapper objects are created by factory/wrapper helper functions.
        explicit cranelift_dsp(void* c_handle);
        friend class cranelift_dsp_factory;
        friend LIBFAUST_API cranelift_dsp_factory* createCraneliftDSPFactoryFromFile(
            const std::string&, int, const char*[], std::string&, int);
        friend LIBFAUST_API cranelift_dsp_factory* createCraneliftDSPFactoryFromString(
            const std::string&, const std::string&, int, const char*[], std::string&, int);
        friend LIBFAUST_API cranelift_dsp_factory* readCraneliftDSPFactoryFromBitcode(
            const std::string&, std::string&);
        friend LIBFAUST_API cranelift_dsp_factory* readCraneliftDSPFactoryFromBitcodeFile(
            const std::string&, std::string&);

    public:
        virtual ~cranelift_dsp() noexcept;
        cranelift_dsp(const cranelift_dsp&) = delete;
        cranelift_dsp& operator=(const cranelift_dsp&) = delete;
        // Internal scaffold bridge helper (C++ wrapper implementation only).
        void* rawCHandle() const noexcept;

        int getNumInputs();
        int getNumOutputs();
        void buildUserInterface(UI* ui_interface);
        int getSampleRate();
        void init(int sample_rate);
        void instanceInit(int sample_rate);
        void instanceConstants(int sample_rate);
        void instanceResetUserInterface();
        void instanceClear();
        cranelift_dsp* clone();
        void metadata(Meta* m);
        void compute(int count, FAUSTFLOAT** inputs, FAUSTFLOAT** outputs);
};

/**
 * DSP factory class (declaration-only scaffold).
 *
 * V1 target surface keeps the common factory query/create APIs and excludes
 * families explicitly deferred in V1 (target getters, memory manager hooks,
 * foreign-function registration).
 */
class LIBFAUST_API cranelift_dsp_factory : public dsp_factory {
    protected:
        // Opaque C handle (`cranelift_dsp_factory*` from the C API), stored as
        // `void*` to avoid including `cranelift-dsp-c.h` in this header.
        void* fHandle;

        explicit cranelift_dsp_factory(void* c_handle);
        friend LIBFAUST_API cranelift_dsp_factory* getCraneliftDSPFactoryFromSHAKey(
            const std::string&);
        friend LIBFAUST_API cranelift_dsp_factory* createCraneliftDSPFactoryFromFile(
            const std::string&, int, const char*[], std::string&, int);
        friend LIBFAUST_API cranelift_dsp_factory* createCraneliftDSPFactoryFromString(
            const std::string&, const std::string&, int, const char*[], std::string&, int);
        friend LIBFAUST_API cranelift_dsp_factory* createCraneliftDSPFactoryFromSignals(
            const std::string&, tvec, int, const char*[], std::string&, int);
        friend LIBFAUST_API cranelift_dsp_factory* createCraneliftDSPFactoryFromBoxes(
            const std::string&, Box, int, const char*[], std::string&, int);
        friend LIBFAUST_API cranelift_dsp_factory* readCraneliftDSPFactoryFromBitcode(
            const std::string&, std::string&);
        friend LIBFAUST_API cranelift_dsp_factory* readCraneliftDSPFactoryFromBitcodeFile(
            const std::string&, std::string&);

    public:
        virtual ~cranelift_dsp_factory() noexcept;
        cranelift_dsp_factory(const cranelift_dsp_factory&) = delete;
        cranelift_dsp_factory& operator=(const cranelift_dsp_factory&) = delete;
        // Internal scaffold bridge helper (C++ wrapper implementation only).
        void* rawCHandle() const noexcept;

        std::string getName();
        std::string getSHAKey();
        std::string getDSPCode();
        std::string getJSON();
        std::string getCompileOptions();
        std::vector<std::string> getLibraryList();
        std::vector<std::string> getIncludePathnames();
        std::vector<std::string> getWarningMessages();

        cranelift_dsp* createDSPInstance();
};

// ── Factory cache / lifecycle (V1 target families) ──────────────────────────

LIBFAUST_API cranelift_dsp_factory* getCraneliftDSPFactoryFromSHAKey(
    const std::string& sha_key);

LIBFAUST_API cranelift_dsp_factory* createCraneliftDSPFactoryFromFile(
    const std::string& filename,
    int argc,
    const char* argv[],
    std::string& error_msg,
    int opt_level = 0);

LIBFAUST_API cranelift_dsp_factory* createCraneliftDSPFactoryFromString(
    const std::string& name_app,
    const std::string& dsp_content,
    int argc,
    const char* argv[],
    std::string& error_msg,
    int opt_level = 0);

LIBFAUST_API cranelift_dsp_factory* createCraneliftDSPFactoryFromSignals(
    const std::string& name_app,
    tvec signals,
    int argc,
    const char* argv[],
    std::string& error_msg,
    int opt_level = 0);

LIBFAUST_API cranelift_dsp_factory* createCraneliftDSPFactoryFromBoxes(
    const std::string& name_app,
    Box box,
    int argc,
    const char* argv[],
    std::string& error_msg,
    int opt_level = 0);

LIBFAUST_API bool deleteCraneliftDSPFactory(cranelift_dsp_factory* factory);
LIBFAUST_API void deleteAllCraneliftDSPFactories();
LIBFAUST_API std::vector<std::string> getAllCraneliftDSPFactories();

// ── Multi-thread cache mode compatibility ────────────────────────────────────

extern "C" LIBFAUST_API bool startMTDSPFactories();
extern "C" LIBFAUST_API void stopMTDSPFactories();

// ── Cranelift backend bitcode family (V1 symbols present, scaffold impl) ────

LIBFAUST_API cranelift_dsp_factory* readCraneliftDSPFactoryFromBitcode(
    const std::string& bit_code,
    std::string& error_msg);

LIBFAUST_API std::string writeCraneliftDSPFactoryToBitcode(
    cranelift_dsp_factory* factory);

LIBFAUST_API cranelift_dsp_factory* readCraneliftDSPFactoryFromBitcodeFile(
    const std::string& bit_code_path,
    std::string& error_msg);

LIBFAUST_API bool writeCraneliftDSPFactoryToBitcodeFile(
    cranelift_dsp_factory* factory,
    const std::string& bit_code_path);

// ── Explicit V1 omissions (deferred without symbols) ────────────────────────

/* Intentionally omitted from the V1 Cranelift C++ API declaration scaffold:
 * - target getters (`getDSPMachineTarget`, factory `getTarget`)
 * - LLVM-only IR/machine/object serialization families
 * - memory manager hooks (`setMemoryManager/getMemoryManager`)
 * - foreign-function registration
 * - LLVM-only `classInit` factory method (decision postponed)
 */

/*!
 @}
 */

#endif /* FAUST_CRANELIFT_DSP_H */
