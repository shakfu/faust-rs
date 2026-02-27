#include "../../include/libfaust-box.h"

int main()
{
    createLibContext();
    Box gain = boxReal(0.5);
    Box wire = boxWire();
    Box mul = CboxMulAux(wire, gain);
    std::string txt = printBox(mul, true, 2048);
    (void)txt;
    destroyLibContext();
    return 0;
}
