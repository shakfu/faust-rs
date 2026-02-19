/*
 * Fixture architecture wrapper.
 */

#include <faust/injected_one.inc>
#include "faust/injected_two.inc"
<<includeIntrinsic>>

static mydsp* g_dsp = new mydsp();
static dsp* g_base = nullptr;
static int dsp_token = 0;
static int mydsp_token = 1;

<<includeclass>>

mydsp* build_instance() { return new mydsp(); }
