/*
 * Fixture architecture wrapper.
 */

// injected_one
#define ENROBAGE_ONE 1
// injected_two
#define ENROBAGE_TWO 2

static customdsp* g_dsp = new customdsp();
static faust_dsp* g_base = nullptr;
static int dsp_token = 0;
static int customdsp_token = 1;

// GENERATED CLASS
class customdsp : public faust_dsp {};

customdsp* build_instance() { return new customdsp(); }
