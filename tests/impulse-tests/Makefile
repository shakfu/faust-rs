#
# faust-rs impulse-response test suite — top-level driver.
#
# Mirrors the C++ Faust `tests/impulse-tests` machinery: build reference impulse
# responses with the genuine C++ Faust compiler, then check each faust-rs
# backend against them with `filesCompare`.
#
# Typical workflow:
#   make build          # cargo build the faust-rs compiler + impulse runner
#   make reference      # generate the C++ reference .ir files (the oracle)
#   make interp         # check the faust-rs interpreter backend
#   make cpp            # check the faust-rs C++ backend
#   make c              # check the faust-rs C backend
#   make wasm           # check the faust-rs WASM backend
#   make all            # interp + c + cpp + cranelift
#
# Add `-k` to keep going past failing cases and `-j N` for parallelism, e.g.
#   make -k -j8 cpp
#
# There is no destructive clean of references: `make clean` only removes the
# generated `ir/` and `build/` trees. Use `make distclean` to also drop
# `reference/`.
#

include common.mk

MAKE ?= make

.PHONY: all reference interp c cpp cranelift wasm build tools help clean distclean

all: cpp c interp cranelift

reference:
	$(MAKE) -f Make.ref reference

interp:
	$(MAKE) -f Make.interp all outdir=interp

cpp:
	$(MAKE) -f Make.gcc all outdir=cpp lang=cpp

c:
	$(MAKE) -f Make.gcc all outdir=c lang=c

cranelift:
	$(MAKE) -f Make.cranelift all outdir=cranelift

wasm:
	$(MAKE) -f Make.wasm all outdir=wasm

# Build the faust-rs binaries the harness drives.
build:
	cd ../.. && cargo build --release -p compiler -p impulse-runner \
		-p cranelift-ffi --bin impulse_cranelift

tools: $(COMPARE)

$(COMPARE): tools/filesCompare.cpp
	$(CXX) -O2 -std=c++11 $< -o $@

clean:
	rm -rf ir build

distclean: clean
	rm -rf reference

help:
	@echo "-------- faust-rs impulse-response tests --------"
	@echo "Targets:"
	@echo "  build      : cargo build the faust-rs compiler + impulse runner"
	@echo "  reference  : generate the genuine C++ reference .ir files (oracle)"
	@echo "  interp     : check the faust-rs interpreter backend (scalar prefix)"
	@echo "  cranelift  : check the faust-rs Cranelift JIT backend (scalar prefix, 64-bit)"
	@echo "  wasm       : check the faust-rs WASM backend (scalar prefix, 64-bit, Node)"
	@echo "  cpp        : check the faust-rs C++ backend (full 4-pass)"
	@echo "  c          : check the faust-rs C backend (full 4-pass)"
	@echo "  all        : cpp + c + interp + cranelift"
	@echo "  tools      : build the filesCompare comparator"
	@echo "  clean      : remove ir/ and build/"
	@echo "  distclean  : also remove reference/"
	@echo ""
	@echo "Key variables (see common.mk, all overridable):"
	@echo "  FAUST_CPP=$(FAUST_CPP)   CPP_TESTS=$(CPP_TESTS)"
	@echo "  FAUST_ARCH=$(FAUST_ARCH)"
	@echo "  FAUST_RS=$(FAUST_RS)   RUNNER=$(RUNNER)"
	@echo "  precision=<tol>  (filesCompare tolerance override)"
	@echo ""
	@echo "Run 'make reference' once before the backend targets."
