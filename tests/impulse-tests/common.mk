#
# Shared configuration for the faust-rs impulse-response test machinery.
#
# Every variable is overridable from the environment or the make command line,
# e.g. `make interp FAUST_RS=/path/to/faust-rs`.
#

# --- faust-rs side (the system under test) ---------------------------------
# The faust-rs compiler binary and the interpreter impulse runner. Built from
# the workspace with `cargo build --release -p compiler -p impulse-runner`.
FAUST_RS  ?= ../../target/release/faust-rs
RUNNER    ?= ../../target/release/impulse-runner

# --- C++ reference oracle ---------------------------------------------------
# The reference `.ir` files are produced by the genuine C++ Faust compiler
# wrapped in the original 4-pass impulse architecture. That architecture pulls
# headers from a C++ Faust checkout, so reference generation and the native
# C/C++ test paths depend on it. Point these at your Faust source tree.
FAUST_CPP  ?= faust
CPP_TESTS  ?= /Users/letz/Developpements/RUST/faust/tests/impulse-tests
FAUST_ARCH ?= /Users/letz/Developpements/RUST/faust/architecture
IMPULSE_ARCH ?= $(CPP_TESTS)/archs/impulsearch.cpp
# The C backend emits C functions wrapped by a C++ `Cdsp` adaptor, so it uses a
# dedicated impulse architecture.
IMPULSE_ARCH_C ?= $(CPP_TESTS)/archs/impulsearch2.cpp

# Faust standard libraries (auto-resolved by the C++ compiler, must be passed
# explicitly to faust-rs which does not add system paths when -I is given).
FAUSTLIBS ?= /usr/local/share/faust

# --- native build / comparison ---------------------------------------------
CXX      ?= c++
CXXFLAGS ?= -O3 -I$(FAUST_ARCH) -I$(CPP_TESTS)/archs -pthread -std=c++11
COMPARE  ?= ./tools/filesCompare

# Total reference frames (4 passes of 15000) and the scalar-only prefix the
# faust-rs interpreter/JIT runners can reproduce today.
NFRAMES      ?= 60000
SCALARFRAMES ?= 15000

# filesCompare tolerance override (empty -> default 2e-06).
precision ?=

dspfiles := $(wildcard dsp/*.dsp)

# Per-DSP tolerance overrides and known-failure lists.
include known.mk
