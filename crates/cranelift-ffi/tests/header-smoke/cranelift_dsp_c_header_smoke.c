#include "cranelift-dsp-c.h"

int main(void) {
    cranelift_dsp_factory* factory = 0;
    cranelift_dsp* dsp = 0;
    (void)getCLibFaustVersion();
    (void)startMTDSPFactories();
    stopMTDSPFactories();
    freeCMemory(0);
    (void)getCCraneliftDSPFactoryJSON(factory);
    (void)getCCraneliftDSPFactoryLibraryList(factory);
    (void)createCCraneliftDSPInstance(factory);
    deleteCCraneliftDSPInstance(dsp);
    return 0;
}
