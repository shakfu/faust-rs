// cranelift-dsp.cpp — C++ wrapper scaffold bridging to the Cranelift C ABI.
//
// Current status:
// - Wrapper implementation is scaffold-level and targets API/lifecycle smoke.
// - It bridges through `cranelift-dsp-c.h` exported symbols from `cranelift-ffi`.
// - Some families remain intentionally stubbed/deferred (signals/boxes factory creation).

#include <array>
#include <memory>
#include <string>
#include <vector>

// Avoid C typedef/class name collisions: the C API uses `cranelift_dsp` and
// `cranelift_dsp_factory` as opaque typedef names, while the C++ API uses the
// same identifiers for wrapper classes.
#define cranelift_dsp cranelift_dsp_c_api
#define cranelift_dsp_factory cranelift_dsp_factory_c_api
#include "cranelift-dsp-c.h"
#undef cranelift_dsp
#undef cranelift_dsp_factory

#include "cranelift-dsp.h"
#include "faust/gui/CGlue.h"

using c_cranelift_dsp = cranelift_dsp_c_api;
using c_cranelift_dsp_factory = cranelift_dsp_factory_c_api;

namespace {

class cranelift_dsp_factory_impl : public cranelift_dsp_factory {
  public:
    explicit cranelift_dsp_factory_impl(void* c_handle)
        : cranelift_dsp_factory(c_handle)
    {}

    void setMemoryManager(dsp_memory_manager*) override {}

    dsp_memory_manager* getMemoryManager() override
    {
        return nullptr;
    }
};

static c_cranelift_dsp* as_c_dsp(cranelift_dsp* dsp)
{
    return dsp ? reinterpret_cast<c_cranelift_dsp*>(dsp->rawCHandle()) : nullptr;
}

static const c_cranelift_dsp* as_c_dsp(const cranelift_dsp* dsp)
{
    return dsp ? reinterpret_cast<const c_cranelift_dsp*>(dsp->rawCHandle()) : nullptr;
}

static c_cranelift_dsp_factory* as_c_factory(cranelift_dsp_factory* factory)
{
    return factory ? reinterpret_cast<c_cranelift_dsp_factory*>(factory->rawCHandle()) : nullptr;
}

static const c_cranelift_dsp_factory* as_c_factory(const cranelift_dsp_factory* factory)
{
    return factory ? reinterpret_cast<const c_cranelift_dsp_factory*>(factory->rawCHandle()) : nullptr;
}

static std::string from_owned_c_string(char* s)
{
    if (!s) {
        return std::string();
    }
    std::string out(s);
    freeCMemory(reinterpret_cast<void*>(s));
    return out;
}

static std::vector<std::string> from_c_string_array(const char** items)
{
    std::vector<std::string> out;
    if (!items) {
        return out;
    }
    for (const char** p = items; *p != nullptr; ++p) {
        out.emplace_back(*p);
    }
    return out;
}

static std::vector<std::string> from_owned_c_string_array(char** items)
{
    std::vector<std::string> out;
    if (!items) {
        return out;
    }
    for (char** p = items; *p != nullptr; ++p) {
        out.emplace_back(*p);
        freeCMemory(reinterpret_cast<void*>(*p));
    }
    // The current Rust scaffold does not provide a dedicated outer-array free.
    // Keep the behavior explicit here to avoid pretending ownership is complete.
    return out;
}

static void set_error_from_buf(std::string& error_msg, const std::array<char, 4096>& err)
{
    if (err[0] != '\0') {
        error_msg.assign(err.data());
    } else {
        error_msg.clear();
    }
}

} // namespace

// ── cranelift_dsp wrapper ────────────────────────────────────────────────────

cranelift_dsp::cranelift_dsp(void* c_handle)
    : fHandle(c_handle)
{}

void* cranelift_dsp::rawCHandle() const noexcept
{
    return fHandle;
}

cranelift_dsp::~cranelift_dsp() noexcept
{
    if (fHandle) {
        deleteCCraneliftDSPInstance(reinterpret_cast<c_cranelift_dsp*>(fHandle));
        fHandle = nullptr;
    }
}

int cranelift_dsp::getNumInputs()
{
    return getNumInputsCCraneliftDSPInstance(as_c_dsp(this));
}

int cranelift_dsp::getNumOutputs()
{
    return getNumOutputsCCraneliftDSPInstance(as_c_dsp(this));
}

void cranelift_dsp::buildUserInterface(UI* ui_interface)
{
    if (!fHandle || !ui_interface) {
        return;
    }
    UIGlue glue;
    buildUIGlue(&glue, ui_interface, sizeof(FAUSTFLOAT) == sizeof(double));
    buildUserInterfaceCCraneliftDSPInstance(as_c_dsp(this), &glue);
}

int cranelift_dsp::getSampleRate()
{
    return getSampleRateCCraneliftDSPInstance(as_c_dsp(this));
}

void cranelift_dsp::init(int sample_rate)
{
    initCCraneliftDSPInstance(as_c_dsp(this), sample_rate);
}

void cranelift_dsp::instanceInit(int sample_rate)
{
    instanceInitCCraneliftDSPInstance(as_c_dsp(this), sample_rate);
}

void cranelift_dsp::instanceConstants(int sample_rate)
{
    instanceConstantsCCraneliftDSPInstance(as_c_dsp(this), sample_rate);
}

void cranelift_dsp::instanceResetUserInterface()
{
    instanceResetUserInterfaceCCraneliftDSPInstance(as_c_dsp(this));
}

void cranelift_dsp::instanceClear()
{
    instanceClearCCraneliftDSPInstance(as_c_dsp(this));
}

cranelift_dsp* cranelift_dsp::clone()
{
    c_cranelift_dsp* cloned = cloneCCraneliftDSPInstance(as_c_dsp(this));
    return cloned ? new cranelift_dsp(reinterpret_cast<void*>(cloned)) : nullptr;
}

void cranelift_dsp::metadata(Meta* m)
{
    if (!fHandle || !m) {
        return;
    }
    MetaGlue glue;
    buildMetaGlue(&glue, m);
    metadataCCraneliftDSPInstance(as_c_dsp(this), &glue);
}

void cranelift_dsp::compute(int count, FAUSTFLOAT** inputs, FAUSTFLOAT** outputs)
{
    computeCCraneliftDSPInstance(as_c_dsp(this), count, inputs, outputs);
}

// ── cranelift_dsp_factory wrapper ────────────────────────────────────────────

cranelift_dsp_factory::cranelift_dsp_factory(void* c_handle)
    : fHandle(c_handle)
{}

void* cranelift_dsp_factory::rawCHandle() const noexcept
{
    return fHandle;
}

cranelift_dsp_factory::~cranelift_dsp_factory() noexcept
{
    if (fHandle) {
        deleteCCraneliftDSPFactory(reinterpret_cast<c_cranelift_dsp_factory*>(fHandle));
        fHandle = nullptr;
    }
}

std::string cranelift_dsp_factory::getName()
{
    return from_owned_c_string(getCCraneliftDSPFactoryName(as_c_factory(this)));
}

std::string cranelift_dsp_factory::getSHAKey()
{
    return from_owned_c_string(getCCraneliftDSPFactorySHAKey(as_c_factory(this)));
}

std::string cranelift_dsp_factory::getDSPCode()
{
    return from_owned_c_string(getCCraneliftDSPFactoryDSPCode(as_c_factory(this)));
}

std::string cranelift_dsp_factory::getJSON()
{
    return from_owned_c_string(getCCraneliftDSPFactoryJSON(as_c_factory(this)));
}

std::string cranelift_dsp_factory::getCompileOptions()
{
    return from_owned_c_string(getCCraneliftDSPFactoryCompileOptions(as_c_factory(this)));
}

std::vector<std::string> cranelift_dsp_factory::getLibraryList()
{
    return from_c_string_array(getCCraneliftDSPFactoryLibraryList(as_c_factory(this)));
}

std::vector<std::string> cranelift_dsp_factory::getIncludePathnames()
{
    return from_c_string_array(getCCraneliftDSPFactoryIncludePathnames(as_c_factory(this)));
}

std::vector<std::string> cranelift_dsp_factory::getWarningMessages()
{
    return from_c_string_array(getCCraneliftDSPFactoryWarningMessages(as_c_factory(this)));
}

cranelift_dsp* cranelift_dsp_factory::createDSPInstance()
{
    c_cranelift_dsp* handle = createCCraneliftDSPInstance(as_c_factory(this));
    return handle ? new cranelift_dsp(reinterpret_cast<void*>(handle)) : nullptr;
}

// ── C++ free-function wrappers ───────────────────────────────────────────────

cranelift_dsp_factory* getCraneliftDSPFactoryFromSHAKey(const std::string& sha_key)
{
    c_cranelift_dsp_factory* handle = getCCraneliftDSPFactoryFromSHAKey(sha_key.c_str());
    return handle ? new cranelift_dsp_factory_impl(reinterpret_cast<void*>(handle)) : nullptr;
}

cranelift_dsp_factory* createCraneliftDSPFactoryFromFile(const std::string& filename,
                                                         int argc,
                                                         const char* argv[],
                                                         std::string& error_msg,
                                                         int opt_level)
{
    std::array<char, 4096> err{};
    auto* handle = createCCraneliftDSPFactoryFromFile(
        filename.c_str(), argc, argv, err.data(), opt_level);
    set_error_from_buf(error_msg, err);
    return handle ? new cranelift_dsp_factory_impl(reinterpret_cast<void*>(handle)) : nullptr;
}

cranelift_dsp_factory* createCraneliftDSPFactoryFromString(const std::string& name_app,
                                                           const std::string& dsp_content,
                                                           int argc,
                                                           const char* argv[],
                                                           std::string& error_msg,
                                                           int opt_level)
{
    std::array<char, 4096> err{};
    auto* handle = createCCraneliftDSPFactoryFromString(
        name_app.c_str(), dsp_content.c_str(), argc, argv, err.data(), opt_level);
    set_error_from_buf(error_msg, err);
    return handle ? new cranelift_dsp_factory_impl(reinterpret_cast<void*>(handle)) : nullptr;
}

cranelift_dsp_factory* createCraneliftDSPFactoryFromSignals(const std::string&,
                                                            tvec,
                                                            int,
                                                            const char*[],
                                                            std::string& error_msg,
                                                            int)
{
    error_msg = "Cranelift C++ scaffold wrapper does not convert signal vectors yet";
    return nullptr;
}

cranelift_dsp_factory* createCraneliftDSPFactoryFromBoxes(const std::string&,
                                                          Box,
                                                          int,
                                                          const char*[],
                                                          std::string& error_msg,
                                                          int)
{
    error_msg = "Cranelift C++ scaffold wrapper does not convert box expressions yet";
    return nullptr;
}

bool deleteCraneliftDSPFactory(cranelift_dsp_factory* factory)
{
    if (!factory) {
        return false;
    }
    delete factory;
    return true;
}

void deleteAllCraneliftDSPFactories()
{
    deleteAllCCraneliftDSPFactories();
}

std::vector<std::string> getAllCraneliftDSPFactories()
{
    return from_owned_c_string_array(getAllCCraneliftDSPFactories());
}

cranelift_dsp_factory* readCraneliftDSPFactoryFromBitcode(const std::string& bit_code,
                                                          std::string& error_msg)
{
    std::array<char, 4096> err{};
    auto* handle = readCCraneliftDSPFactoryFromBitcode(bit_code.c_str(), err.data());
    set_error_from_buf(error_msg, err);
    return handle ? new cranelift_dsp_factory_impl(reinterpret_cast<void*>(handle)) : nullptr;
}

std::string writeCraneliftDSPFactoryToBitcode(cranelift_dsp_factory* factory)
{
    return from_owned_c_string(writeCCraneliftDSPFactoryToBitcode(as_c_factory(factory)));
}

cranelift_dsp_factory* readCraneliftDSPFactoryFromBitcodeFile(const std::string& bit_code_path,
                                                              std::string& error_msg)
{
    std::array<char, 4096> err{};
    auto* handle = readCCraneliftDSPFactoryFromBitcodeFile(bit_code_path.c_str(), err.data());
    set_error_from_buf(error_msg, err);
    return handle ? new cranelift_dsp_factory_impl(reinterpret_cast<void*>(handle)) : nullptr;
}

bool writeCraneliftDSPFactoryToBitcodeFile(cranelift_dsp_factory* factory,
                                           const std::string& bit_code_path)
{
    return writeCCraneliftDSPFactoryToBitcodeFile(as_c_factory(factory), bit_code_path.c_str());
}
