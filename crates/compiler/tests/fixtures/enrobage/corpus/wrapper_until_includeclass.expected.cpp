/*
 * Fixture architecture wrapper.
 */

// injected_one
#define ENROBAGE_ONE 1
// injected_two
#define ENROBAGE_TWO 2
<<includeIntrinsic>>

static customdsp* g_dsp = new customdsp();
static faust_dsp* g_base = nullptr;
static int dsp_token = 0;
static int customdsp_token = 1;

