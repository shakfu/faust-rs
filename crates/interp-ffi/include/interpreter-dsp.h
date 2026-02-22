/************************** BEGIN interpreter-dsp.h **********************
 * C++ wrapper for the Faust FBC interpreter — Rust port
 * (mirrors faust/architecture/faust/dsp/interpreter-dsp.h)
 *
 * This header provides C++ classes that wrap the C API defined in
 * interpreter-dsp-c.h.  Include this file from C++ projects.
 * C projects should include interpreter-dsp-c.h directly.
 ************************************************************************/

#ifndef INTERPRETER_DSP_H
#define INTERPRETER_DSP_H

#include <string>
#include <vector>
#include <cstring>    // strdup, strnlen

// Pull in the C API declarations and the UIGlue / MetaGlue types.
#include "interpreter-dsp-c.h"

// ── Compatibility shim ────────────────────────────────────────────────────────
// The Rust-generated header uses snake_case field names (open_tab_box, etc.)
// to match Rust conventions.  The original CInterface.h uses camelCase.
// Both layouts are binary-identical (same order, same types), so code using
// the Faust CInterface.h UIGlue is directly compatible.
// ─────────────────────────────────────────────────────────────────────────────

#ifdef __cplusplus

// ── interpreter_dsp_factory ───────────────────────────────────────────────────

/**
 * C++ wrapper for the FBC DSP factory.
 *
 * Instances are obtained via the free functions below (readInterpreterDSP...).
 * The factory owns its memory; call deleteInterpreterDSPFactory() to free it.
 */
class interpreter_dsp_factory {
public:
    explicit interpreter_dsp_factory(::interpreter_dsp_factory* impl)
        : impl_(impl) {}

    ~interpreter_dsp_factory() = default;

    // Non-copyable: ownership managed explicitly by the C API.
    interpreter_dsp_factory(const interpreter_dsp_factory&) = delete;
    interpreter_dsp_factory& operator=(const interpreter_dsp_factory&) = delete;

    /// Return the underlying C pointer (for passing to C API functions).
    ::interpreter_dsp_factory* get() const { return impl_; }

    /// Return factory SHA key.
    std::string getSHAKey() const {
        // SHA key is embedded in the JSON; extract it from getJSON for now.
        // (A dedicated getCInterpreterDSPFactoryName function is not in the
        // initial C API — use the JSON field.)
        return "";  // TODO: expose getSHAKey via C API
    }

    /// Return factory JSON description.
    std::string getJSON() const {
        char* raw = getCInterpreterDSPFactoryJSON(impl_);
        if (!raw) return "{}";
        std::string result(raw);
        freeCMemory(raw);
        return result;
    }

    /// Serialize factory to a bitcode string (in-memory .fbc format).
    std::string writeToMemory() const {
        char* raw = writeCInterpreterDSPFactoryToBitcode(impl_);
        if (!raw) return "";
        std::string result(raw);
        freeCMemory(raw);
        return result;
    }

    /// Write factory to a .fbc file.
    bool writeToFile(const std::string& path) const {
        return writeCInterpreterDSPFactoryToBitcodeFile(impl_, path.c_str());
    }

    /// Create a new DSP instance from this factory.
    ///
    /// The caller owns the returned object and is responsible for deletion.
    class interpreter_dsp* createDSPInstance();

private:
    ::interpreter_dsp_factory* impl_;
};

// ── interpreter_dsp ───────────────────────────────────────────────────────────

/**
 * C++ wrapper for a FBC DSP instance.
 *
 * Instances are created via `interpreter_dsp_factory::createDSPInstance()`.
 * The caller owns the instance; call `delete` or `deleteCInterpreterDSPInstance`.
 */
class interpreter_dsp {
public:
    explicit interpreter_dsp(::interpreter_dsp* impl) : impl_(impl) {}

    ~interpreter_dsp() {
        if (impl_) {
            deleteCInterpreterDSPInstance(impl_);
            impl_ = nullptr;
        }
    }

    // Non-copyable.
    interpreter_dsp(const interpreter_dsp&) = delete;
    interpreter_dsp& operator=(const interpreter_dsp&) = delete;

    int getNumInputs() const {
        return getNumInputsCInterpreterDSPInstance(impl_);
    }

    int getNumOutputs() const {
        return getNumOutputsCInterpreterDSPInstance(impl_);
    }

    int getSampleRate() const {
        return getSampleRateCInterpreterDSPInstance(impl_);
    }

    void init(int sample_rate) {
        initCInterpreterDSPInstance(impl_, sample_rate);
    }

    void instanceInit(int sample_rate) {
        instanceInitCInterpreterDSPInstance(impl_, sample_rate);
    }

    void instanceConstants(int sample_rate) {
        instanceConstantsCInterpreterDSPInstance(impl_, sample_rate);
    }

    void instanceResetUserInterface() {
        instanceResetUserInterfaceCInterpreterDSPInstance(impl_);
    }

    void instanceClear() {
        instanceClearCInterpreterDSPInstance(impl_);
    }

    interpreter_dsp* clone() const {
        ::interpreter_dsp* c = cloneCInterpreterDSPInstance(impl_);
        return c ? new interpreter_dsp(c) : nullptr;
    }

    /**
     * Build user interface using a `UIGlue` callback table.
     *
     * The UIGlue struct matches `faust/gui/CInterface.h` exactly except
     * that field names use snake_case (same binary layout).
     */
    void buildUserInterface(UIGlue* glue) {
        buildUserInterfaceCInterpreterDSPInstance(impl_, glue);
    }

    void metadata(MetaGlue* meta) {
        metadataCInterpreterDSPInstance(impl_, meta);
    }

    /**
     * Process one buffer of audio.
     *
     * @param count   number of frames
     * @param inputs  array of num_inputs non-interleaved float* buffers
     * @param outputs array of num_outputs non-interleaved float* buffers
     */
    void compute(int count, FAUSTFLOAT** inputs, FAUSTFLOAT** outputs) {
        computeCInterpreterDSPInstance(impl_, count, inputs, outputs);
    }

    ::interpreter_dsp* get() const { return impl_; }

private:
    ::interpreter_dsp* impl_;
};

// ── interpreter_dsp_factory::createDSPInstance (needs interpreter_dsp defined) ─

inline interpreter_dsp* interpreter_dsp_factory::createDSPInstance() {
    ::interpreter_dsp* c = createCInterpreterDSPInstance(impl_);
    return c ? new interpreter_dsp(c) : nullptr;
}

// ── Free functions ────────────────────────────────────────────────────────────

/**
 * Create a factory from a bitcode string (.fbc in-memory format).
 *
 * @param bit_code  the bitcode string
 * @param error_msg output error message (filled on failure)
 * @return a factory on success, nullptr on failure
 */
inline interpreter_dsp_factory* readInterpreterDSPFactoryFromBitcode(
    const std::string& bit_code,
    std::string& error_msg)
{
    char buf[4096] = {};
    ::interpreter_dsp_factory* raw =
        readCInterpreterDSPFactoryFromBitcode(bit_code.c_str(), buf);
    if (!raw) {
        error_msg = buf;
        return nullptr;
    }
    return new interpreter_dsp_factory(raw);
}

/**
 * Serialize a factory to a bitcode string.
 *
 * @param factory the factory to serialize
 * @return the bitcode as a std::string
 */
inline std::string writeInterpreterDSPFactoryToBitcode(
    interpreter_dsp_factory* factory)
{
    return factory ? factory->writeToMemory() : "";
}

/**
 * Create a factory from a .fbc file.
 *
 * @param path      path to the .fbc file
 * @param error_msg output error message (filled on failure)
 * @return a factory on success, nullptr on failure
 */
inline interpreter_dsp_factory* readInterpreterDSPFactoryFromBitcodeFile(
    const std::string& path,
    std::string& error_msg)
{
    char buf[4096] = {};
    ::interpreter_dsp_factory* raw =
        readCInterpreterDSPFactoryFromBitcodeFile(path.c_str(), buf);
    if (!raw) {
        error_msg = buf;
        return nullptr;
    }
    return new interpreter_dsp_factory(raw);
}

/**
 * Write a factory to a .fbc file.
 *
 * @param factory the factory to write
 * @param path    destination file path
 * @return true on success
 */
inline bool writeInterpreterDSPFactoryToBitcodeFile(
    interpreter_dsp_factory* factory,
    const std::string& path)
{
    return factory && factory->writeToFile(path);
}

/**
 * Look up a factory in the cache by SHA key.
 *
 * @param sha_key the SHA key
 * @return a (non-owning) factory pointer, or nullptr if not found
 */
inline interpreter_dsp_factory* getInterpreterDSPFactoryFromSHAKey(
    const std::string& sha_key)
{
    ::interpreter_dsp_factory* raw =
        getCInterpreterDSPFactoryFromSHAKey(sha_key.c_str());
    return raw ? new interpreter_dsp_factory(raw) : nullptr;
}

/**
 * Delete a factory and remove it from the cache.
 *
 * @param factory the factory to delete
 * @return true if the internal allocation was freed
 */
inline bool deleteInterpreterDSPFactory(interpreter_dsp_factory* factory) {
    if (!factory) return false;
    bool result = deleteCInterpreterDSPFactory(factory->get());
    delete factory;
    return result;
}

/**
 * Delete all factories in the cache.
 */
inline void deleteAllInterpreterDSPFactories() {
    deleteAllCInterpreterDSPFactories();
}

/**
 * Return all SHA keys in the cache.
 */
inline std::vector<std::string> getAllInterpreterDSPFactories() {
    const char** raw = reinterpret_cast<const char**>(getAllCInterpreterDSPFactories());
    std::vector<std::string> result;
    if (!raw) return result;
    for (int i = 0; raw[i]; ++i) {
        result.push_back(raw[i]);
        freeCMemory(const_cast<void*>(reinterpret_cast<const void*>(raw[i])));
    }
    freeCMemory(raw);
    return result;
}

// ── Top-level using declarations (optional — matches original Faust API) ──────
// Uncomment if you want to use interpreter_dsp / interpreter_dsp_factory
// without the faust_interp:: prefix.
//
// using faust_interp::interpreter_dsp;
// using faust_interp::interpreter_dsp_factory;
// using faust_interp::readInterpreterDSPFactoryFromBitcode;
// etc.

#endif // __cplusplus
#endif // INTERPRETER_DSP_H

/************************** END interpreter-dsp.h **************************/
