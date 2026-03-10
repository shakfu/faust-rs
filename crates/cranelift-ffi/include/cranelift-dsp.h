/* cranelift-dsp.h — C++ wrapper for the Cranelift backend
 *
 * This header now mirrors the interpreter wrapper strategy: the C++ classes are
 * thin inline adapters built directly on top of `cranelift-dsp-c.h`.
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
#include "faust/gui/CGlue.h"
#include "faust/gui/meta.h"

// Avoid C typedef/class name collisions. The C API uses `cranelift_dsp` and
// `cranelift_dsp_factory` for opaque handles, while the C++ API uses the same
// identifiers for wrapper classes.
#define cranelift_dsp cranelift_dsp_c_api
#define cranelift_dsp_factory cranelift_dsp_factory_c_api
#include "cranelift-dsp-c.h"
#undef cranelift_dsp
#undef cranelift_dsp_factory

using ccranelift_dsp = cranelift_dsp_c_api;
using ccranelift_dsp_factory = cranelift_dsp_factory_c_api;

namespace cranelift_dsp_detail {

inline std::string from_owned_c_string(char* raw)
{
    if (!raw) {
        return {};
    }
    std::string result(raw);
    freeCMemory(raw);
    return result;
}

inline std::vector<std::string> from_c_string_array(const char** items)
{
    std::vector<std::string> result;
    if (!items) {
        return result;
    }
    for (const char** it = items; *it != nullptr; ++it) {
        result.emplace_back(*it);
    }
    return result;
}

inline std::vector<std::string> from_owned_c_string_array(char** items)
{
    std::vector<std::string> result;
    if (!items) {
        return result;
    }
    for (char** it = items; *it != nullptr; ++it) {
        result.emplace_back(*it);
        freeCMemory(*it);
    }
    return result;
}

inline void copy_error_message(std::string& error_msg, const char* buffer)
{
    error_msg = buffer ? buffer : "";
}

} // namespace cranelift_dsp_detail

/*!
 \addtogroup craneliftcpp C++ interface for compiling Faust code with the Cranelift backend.
 Note that the API is not thread safe: use `startMTDSPFactories/stopMTDSPFactories`
 to coordinate global factory-cache access.
 @{
 */

extern "C" LIBFAUST_API const char* getCLibFaustVersion();

class cranelift_dsp;

class LIBFAUST_API cranelift_dsp_factory : public dsp_factory {
public:
    explicit cranelift_dsp_factory(ccranelift_dsp_factory* impl)
        : impl_(impl)
    {}

    ~cranelift_dsp_factory() override = default;

    cranelift_dsp_factory(const cranelift_dsp_factory&) = delete;
    cranelift_dsp_factory& operator=(const cranelift_dsp_factory&) = delete;

    ccranelift_dsp_factory* get() const { return impl_; }

    std::string getName() override
    {
        return cranelift_dsp_detail::from_owned_c_string(
            getCCraneliftDSPFactoryName(impl_));
    }

    std::string getSHAKey() override
    {
        return cranelift_dsp_detail::from_owned_c_string(
            getCCraneliftDSPFactorySHAKey(impl_));
    }

    std::string getDSPCode() override
    {
        return cranelift_dsp_detail::from_owned_c_string(
            getCCraneliftDSPFactoryDSPCode(impl_));
    }

    std::string getJSON() override
    {
        return cranelift_dsp_detail::from_owned_c_string(
            getCCraneliftDSPFactoryJSON(impl_));
    }

    std::string getCompileOptions() override
    {
        return cranelift_dsp_detail::from_owned_c_string(
            getCCraneliftDSPFactoryCompileOptions(impl_));
    }

    std::vector<std::string> getLibraryList() override
    {
        return cranelift_dsp_detail::from_c_string_array(
            getCCraneliftDSPFactoryLibraryList(impl_));
    }

    std::vector<std::string> getIncludePathnames() override
    {
        return cranelift_dsp_detail::from_c_string_array(
            getCCraneliftDSPFactoryIncludePathnames(impl_));
    }

    std::vector<std::string> getWarningMessages() override
    {
        return cranelift_dsp_detail::from_c_string_array(
            getCCraneliftDSPFactoryWarningMessages(impl_));
    }

    ::dsp* createDSPInstance() override;

    void setMemoryManager(dsp_memory_manager* /*manager*/) override {}

    dsp_memory_manager* getMemoryManager() override { return nullptr; }

private:
    ccranelift_dsp_factory* impl_;
};

class LIBFAUST_API cranelift_dsp : public dsp {
public:
    explicit cranelift_dsp(ccranelift_dsp* impl)
        : impl_(impl)
    {}

    ~cranelift_dsp() override
    {
        if (impl_) {
            deleteCCraneliftDSPInstance(impl_);
            impl_ = nullptr;
        }
    }

    cranelift_dsp(const cranelift_dsp&) = delete;
    cranelift_dsp& operator=(const cranelift_dsp&) = delete;

    int getNumInputs() override
    {
        return getNumInputsCCraneliftDSPInstance(impl_);
    }

    int getNumOutputs() override
    {
        return getNumOutputsCCraneliftDSPInstance(impl_);
    }

    int getSampleRate() override
    {
        return getSampleRateCCraneliftDSPInstance(impl_);
    }

    void init(int sample_rate) override
    {
        initCCraneliftDSPInstance(impl_, sample_rate);
    }

    void instanceInit(int sample_rate) override
    {
        instanceInitCCraneliftDSPInstance(impl_, sample_rate);
    }

    void instanceConstants(int sample_rate) override
    {
        instanceConstantsCCraneliftDSPInstance(impl_, sample_rate);
    }

    void instanceResetUserInterface() override
    {
        instanceResetUserInterfaceCCraneliftDSPInstance(impl_);
    }

    void instanceClear() override
    {
        instanceClearCCraneliftDSPInstance(impl_);
    }

    cranelift_dsp* clone() override
    {
        ccranelift_dsp* cloned = cloneCCraneliftDSPInstance(impl_);
        return cloned ? new cranelift_dsp(cloned) : nullptr;
    }

    void buildUserInterface(UI* ui_interface) override
    {
        if (!ui_interface) {
            return;
        }
        UIGlue glue;
        buildUIGlue(&glue, ui_interface, sizeof(FAUSTFLOAT) == sizeof(double));
        buildUserInterfaceCCraneliftDSPInstance(impl_, &glue);
    }

    void metadata(Meta* meta) override
    {
        if (!meta) {
            return;
        }
        MetaGlue glue;
        buildMetaGlue(&glue, meta);
        metadataCCraneliftDSPInstance(impl_, &glue);
    }

    void buildUserInterface(UIGlue* glue)
    {
        buildUserInterfaceCCraneliftDSPInstance(impl_, glue);
    }

    void metadata(MetaGlue* meta)
    {
        metadataCCraneliftDSPInstance(impl_, meta);
    }

    void compute(int count, FAUSTFLOAT** inputs, FAUSTFLOAT** outputs) override
    {
        computeCCraneliftDSPInstance(impl_, count, inputs, outputs);
    }

    ccranelift_dsp* get() const { return impl_; }

private:
    ccranelift_dsp* impl_;
};

inline ::dsp* cranelift_dsp_factory::createDSPInstance()
{
    ccranelift_dsp* impl = createCCraneliftDSPInstance(impl_);
    return impl ? new cranelift_dsp(impl) : nullptr;
}

inline cranelift_dsp_factory* getCraneliftDSPFactoryFromSHAKey(
    const std::string& sha_key)
{
    ccranelift_dsp_factory* impl =
        getCCraneliftDSPFactoryFromSHAKey(sha_key.c_str());
    return impl ? new cranelift_dsp_factory(impl) : nullptr;
}

inline cranelift_dsp_factory* createCraneliftDSPFactoryFromFile(
    const std::string& filename,
    int argc,
    const char* argv[],
    std::string& error_msg,
    int opt_level = 0)
{
    char buffer[4096] = {};
    ccranelift_dsp_factory* impl = createCCraneliftDSPFactoryFromFile(
        filename.c_str(), argc, argv, buffer, opt_level);
    if (!impl) {
        cranelift_dsp_detail::copy_error_message(error_msg, buffer);
        return nullptr;
    }
    error_msg.clear();
    return new cranelift_dsp_factory(impl);
}

inline cranelift_dsp_factory* createCraneliftDSPFactoryFromString(
    const std::string& name_app,
    const std::string& dsp_content,
    int argc,
    const char* argv[],
    std::string& error_msg,
    int opt_level = 0)
{
    char buffer[4096] = {};
    ccranelift_dsp_factory* impl = createCCraneliftDSPFactoryFromString(
        name_app.c_str(), dsp_content.c_str(), argc, argv, buffer, opt_level);
    if (!impl) {
        cranelift_dsp_detail::copy_error_message(error_msg, buffer);
        return nullptr;
    }
    error_msg.clear();
    return new cranelift_dsp_factory(impl);
}

inline cranelift_dsp_factory* createCraneliftDSPFactoryFromSignals(
    const std::string&,
    tvec,
    int,
    const char*[],
    std::string& error_msg,
    int = 0)
{
    error_msg = "Cranelift C++ wrapper does not expose signal-vector bridging yet";
    return nullptr;
}

inline cranelift_dsp_factory* createCraneliftDSPFactoryFromBoxes(
    const std::string&,
    Box,
    int,
    const char*[],
    std::string& error_msg,
    int = 0)
{
    error_msg = "Cranelift C++ wrapper does not expose box-expression bridging yet";
    return nullptr;
}

inline bool deleteCraneliftDSPFactory(cranelift_dsp_factory* factory)
{
    if (!factory) {
        return false;
    }
    bool result = deleteCCraneliftDSPFactory(factory->get());
    delete factory;
    return result;
}

inline void deleteAllCraneliftDSPFactories()
{
    deleteAllCCraneliftDSPFactories();
}

inline std::vector<std::string> getAllCraneliftDSPFactories()
{
    return cranelift_dsp_detail::from_owned_c_string_array(
        getAllCCraneliftDSPFactories());
}

inline cranelift_dsp_factory* readCraneliftDSPFactoryFromBitcode(
    const std::string& bit_code,
    std::string& error_msg)
{
    char buffer[4096] = {};
    ccranelift_dsp_factory* impl =
        readCCraneliftDSPFactoryFromBitcode(bit_code.c_str(), buffer);
    if (!impl) {
        cranelift_dsp_detail::copy_error_message(error_msg, buffer);
        return nullptr;
    }
    error_msg.clear();
    return new cranelift_dsp_factory(impl);
}

inline std::string writeCraneliftDSPFactoryToBitcode(
    cranelift_dsp_factory* factory)
{
    return factory
        ? cranelift_dsp_detail::from_owned_c_string(
              writeCCraneliftDSPFactoryToBitcode(factory->get()))
        : std::string();
}

inline cranelift_dsp_factory* readCraneliftDSPFactoryFromBitcodeFile(
    const std::string& bit_code_path,
    std::string& error_msg)
{
    char buffer[4096] = {};
    ccranelift_dsp_factory* impl =
        readCCraneliftDSPFactoryFromBitcodeFile(bit_code_path.c_str(), buffer);
    if (!impl) {
        cranelift_dsp_detail::copy_error_message(error_msg, buffer);
        return nullptr;
    }
    error_msg.clear();
    return new cranelift_dsp_factory(impl);
}

inline bool writeCraneliftDSPFactoryToBitcodeFile(
    cranelift_dsp_factory* factory,
    const std::string& bit_code_path)
{
    return factory
        ? writeCCraneliftDSPFactoryToBitcodeFile(factory->get(), bit_code_path.c_str())
        : false;
}

/*!
 @}
 */

#endif /* FAUST_CRANELIFT_DSP_H */
