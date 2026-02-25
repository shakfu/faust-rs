#include "cranelift-dsp.h"

void smoke(cranelift_dsp* dsp, cranelift_dsp_factory* factory, UI* ui, Meta* meta) {
    if (dsp) {
        dsp->getNumInputs();
        dsp->getNumOutputs();
        dsp->buildUserInterface(ui);
        dsp->metadata(meta);
    }
    if (factory) {
        factory->getName();
        factory->getJSON();
        factory->getCompileOptions();
        factory->createDSPInstance();
    }
}

int main() {
    return getCLibFaustVersion() ? 0 : 0;
}
