#include "cranelift-dsp.h"

void smoke(cranelift_dsp* dsp, cranelift_dsp_factory* factory, UI* ui, Meta* meta) {
    if (dsp) {
        dsp->getNumInputs();
        dsp->getNumOutputs();
        dsp->getSampleRate();
        dsp->init(48000);
        dsp->instanceInit(48000);
        dsp->instanceConstants(48000);
        dsp->instanceResetUserInterface();
        dsp->instanceClear();
        dsp->buildUserInterface(ui);
        dsp->metadata(meta);
        FAUSTFLOAT* inputs[1] = {nullptr};
        FAUSTFLOAT* outputs[1] = {nullptr};
        dsp->compute(0, inputs, outputs);
        cranelift_dsp* cloned = dsp->clone();
        delete cloned;
    }
    if (factory) {
        factory->getName();
        factory->getSHAKey();
        factory->getDSPCode();
        factory->getJSON();
        factory->getCompileOptions();
        factory->getLibraryList();
        factory->getIncludePathnames();
        factory->getWarningMessages();
        factory->createDSPInstance();
    }
}

int main() {
    std::string error_msg;
    const char* argv[] = {"-I.", nullptr};
    createCraneliftDSPFactoryFromFile("x.dsp", 1, argv, error_msg, 0);
    createCraneliftDSPFactoryFromString("x", "process=_,_;", 0, nullptr, error_msg, 1);
    getCraneliftDSPFactoryFromSHAKey("dummy");
    getAllCraneliftDSPFactories();
    startMTDSPFactories();
    stopMTDSPFactories();
    readCraneliftDSPFactoryFromBitcode("dummy", error_msg);
    readCraneliftDSPFactoryFromBitcodeFile("dummy.fbc", error_msg);
    writeCraneliftDSPFactoryToBitcode(nullptr);
    writeCraneliftDSPFactoryToBitcodeFile(nullptr, "dummy.fbc");
    deleteAllCraneliftDSPFactories();
    return getCLibFaustVersion() ? 0 : 0;
}
