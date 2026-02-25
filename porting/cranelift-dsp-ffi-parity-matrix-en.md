# Cranelift DSP FFI Parity Matrix (Phase 0)

**Date:** 2026-02-25  
**Status:** Draft (started during Phase 0)  
**Primary reference:** `llvm_dsp` / `llvm_dsp_factory` (C + C++)  
**Secondary cross-check:** `interpreter_dsp` / `interpreter_dsp_factory` (C + C++)  
**Target surface:** `cranelift_dsp` / `cranelift_dsp_factory` (C + C++)

---

## 1. Purpose

This document is the **Phase 0 function-by-function parity matrix** required by:

- `porting/cranelift-backend-plan-en.md`

It defines the target exported function set and naming strategy for the future
`cranelift_dsp` FFI layer before implementation deepens.

---

## 2. Locked Decisions

## 2.1 Reference priority

- Primary reference: `llvm_dsp`
- Secondary cross-check: `interpreter_dsp`
- If they diverge:
  - default to `llvm_dsp`
  - document the divergence and rationale before implementation

## 2.2 C API naming convention

Use the **interpreter-style backend-prefixed** naming for Cranelift C functions.

Examples:

- `createCCraneliftDSPFactoryFromFile`
- `createCCraneliftDSPFactoryFromString`
- `createCCraneliftDSPInstance`

Rule:

- Preserve the **function family/semantics** parity of `llvm_dsp`
- Replace the backend-specific naming prefix with `Cranelift`

---

## 3. C API Parity Matrix (Initial Draft)

Legend:

- `Target`: Cranelift C API target symbol name
- `Status`: `v1-required`, `v1-deferred`, `n/a`, `decision-pending`
- `Ref`: primary/secondary reference note

## 3.1 Global / shared functions

| Family | LLVM C API | Interpreter C API | Target (Cranelift C API) | Status | Notes |
|---|---|---|---|---|---|
| Version | `getCLibFaustVersion` | `getCLibFaustVersion` | `getCLibFaustVersion` | `v1-required` | Shared global symbol (same name) |
| MT start | `startMTDSPFactories` | `startMTDSPFactories` | `startMTDSPFactories` | `v1-required` | Shared cache/threading strategy |
| MT stop | `stopMTDSPFactories` | `stopMTDSPFactories` | `stopMTDSPFactories` | `v1-required` | Shared cache/threading strategy |

## 3.2 Factory cache lifecycle

| Family | LLVM C API | Interpreter C API | Target (Cranelift C API) | Status | Notes |
|---|---|---|---|---|---|
| Cache get by SHA | `getCDSPFactoryFromSHAKey` | `getCInterpreterDSPFactoryFromSHAKey` | `getCCraneliftDSPFactoryFromSHAKey` | `v1-required` | Backend-prefixed naming, cache parity required |
| Delete all factories | `deleteAllCDSPFactories` | `deleteAllCInterpreterDSPFactories` | `deleteAllCCraneliftDSPFactories` | `v1-required` | |
| List all factories | `getAllCDSPFactories` | `getAllCInterpreterDSPFactories` | `getAllCCraneliftDSPFactories` | `v1-required` | Array/freeing semantics parity |

## 3.3 Factory creation from Faust source

| Family | LLVM C API | Interpreter C API | Target (Cranelift C API) | Status | Notes |
|---|---|---|---|---|---|
| From file | `createCDSPFactoryFromFile` | `createCInterpreterDSPFactoryFromFile` | `createCCraneliftDSPFactoryFromFile` | `v1-required` | User-locked naming |
| From string | `createCDSPFactoryFromString` | `createCInterpreterDSPFactoryFromString` | `createCCraneliftDSPFactoryFromString` | `v1-required` | |
| From signals | `createCDSPFactoryFromSignals` | `createCInterpreterDSPFactoryFromSignals` | `createCCraneliftDSPFactoryFromSignals` | `v1-required` | May return typed unsupported initially, but symbol present |
| From boxes | `createCDSPFactoryFromBoxes` | `createCInterpreterDSPFactoryFromBoxes` | `createCCraneliftDSPFactoryFromBoxes` | `v1-required` | May return typed unsupported initially, but symbol present |

## 3.4 Factory deletion and metadata/query surface

| Family | LLVM C API | Interpreter C API | Target (Cranelift C API) | Status | Notes |
|---|---|---|---|---|---|
| Delete factory | `deleteCDSPFactory` | `deleteCInterpreterDSPFactory` | `deleteCCraneliftDSPFactory` | `v1-required` | |
| JSON | `getCDSPFactoryJSON` | `getCInterpreterDSPFactoryJSON` | `getCCraneliftDSPFactoryJSON` | `v1-required` | |
| Library list | `getCDSPFactoryLibraryList` | `getCInterpreterDSPFactoryLibraryList` | `getCCraneliftDSPFactoryLibraryList` | `v1-required` | |
| Name | `getCName` | C++ method only helper equivalent | `getCCraneliftDSPFactoryName` (or parity alias decision pending) | `decision-pending` | LLVM has generic naming here; exact C name needs final convention decision |
| SHA key | `getCSHAKey` | no same C symbol in interpreter C header | `getCCraneliftDSPFactorySHAKey` (or alias decision pending) | `decision-pending` | |
| Expanded code | `getCDSPCode` | not present in interpreter C header | `getCCraneliftDSPFactoryDSPCode` (or alias decision pending) | `decision-pending` | likely `v1-required` if following LLVM full surface |
| Compile options | `getCDSPFactoryCompileOptions` | not present in interpreter C header | `getCCraneliftDSPFactoryCompileOptions` | `decision-pending` | |
| Target string | `getCTarget` | not present | `getCCraneliftDSPFactoryTarget` | `decision-pending` | Cranelift target semantics need Phase 0 ABI decision |
| Include pathnames | `getCDSPFactoryIncludePathnames` | not present in interpreter C header | `getCCraneliftDSPFactoryIncludePathnames` | `decision-pending` | |
| Warning messages | `getCWarningMessages` | not present in interpreter C header | `getCCraneliftDSPFactoryWarningMessages` | `decision-pending` | |

## 3.5 Factory serialization / code import-export families

| Family | LLVM C API | Interpreter C API | Target (Cranelift C API) | Status | Notes |
|---|---|---|---|---|---|
| Bitcode read/write (backend bytecode) | `readCDSPFactoryFromBitcode`, `writeCDSPFactoryToBitcode`, `...File` | `readCInterpreterDSPFactoryFromBitcode`, `writeCInterpreterDSPFactoryToBitcode`, `...File` | `readCCraneliftDSPFactoryFromBitcode`, `writeCCraneliftDSPFactoryToBitcode`, `...File` | `v1-required` | Exact backend payload format TBD (likely Cranelift-specific compiled snapshot or deferred stub with typed error) |
| IR read/write | `readCDSPFactoryFromIR`, `writeCDSPFactoryToIR`, `...File` | n/a | `readCCraneliftDSPFactoryFromIR`, `writeCCraneliftDSPFactoryToIR`, `...File` | `v1-deferred` | May exist as symbols returning typed unsupported if strict parity set is required |
| Machine read/write | `readCDSPFactoryFromMachine`, `writeCDSPFactoryToMachine`, `...File` | n/a | `readCCraneliftDSPFactoryFromMachine`, `writeCCraneliftDSPFactoryToMachine`, `...File` | `v1-deferred` | Depends on object/machine snapshot strategy |

## 3.6 Factory runtime/global configuration

| Family | LLVM C API | Interpreter C API | Target (Cranelift C API) | Status | Notes |
|---|---|---|---|---|---|
| Class init on factory | `classCInit` | n/a | `classCCraneliftInit` (naming pending) | `decision-pending` | LLVM-specific pattern; may be represented via instance APIs only |
| Memory manager setter | `setCMemoryManager` | n/a (in current interpreter C header) | `setCCraneliftMemoryManager` (or parity alias) | `decision-pending` | Important for `llvm_dsp` parity; exact support strategy needs user decision |
| Register foreign function | `registerCForeignFunction` | n/a | `registerCCraneliftForeignFunction` (or parity alias) | `decision-pending` | Depends on Cranelift host-call policy |
| Machine target getter | `getCDSPMachineTarget` | n/a | `getCCraneliftDSPMachineTarget` or shared alias | `decision-pending` | Cranelift target exposure policy needs decision |

## 3.7 DSP instance functions (C API)

| Family | LLVM C API | Interpreter C API | Target (Cranelift C API) | Status | Notes |
|---|---|---|---|---|---|
| Create instance | `createCDSPInstance` | `createCInterpreterDSPInstance` | `createCCraneliftDSPInstance` | `v1-required` | User-locked naming |
| Delete instance | `deleteCDSPInstance` | `deleteCInterpreterDSPInstance` | `deleteCCraneliftDSPInstance` | `v1-required` | |
| Clone | `cloneCDSPInstance` | `cloneCInterpreterDSPInstance` | `cloneCCraneliftDSPInstance` | `v1-required` | May return typed unsupported initially only if absolutely necessary (prefer implement) |
| Num inputs | `getNumInputsCDSPInstance` | `getNumInputsCInterpreterDSPInstance` | `getNumInputsCCraneliftDSPInstance` | `v1-required` | |
| Num outputs | `getNumOutputsCDSPInstance` | `getNumOutputsCInterpreterDSPInstance` | `getNumOutputsCCraneliftDSPInstance` | `v1-required` | |
| Build UI | `buildUserInterfaceCDSPInstance` | `buildUserInterfaceCInterpreterDSPInstance` | `buildUserInterfaceCCraneliftDSPInstance` | `v1-required` | UI callbacks mandatory in V1 |
| Sample rate | `getSampleRateCDSPInstance` | `getSampleRateCInterpreterDSPInstance` | `getSampleRateCCraneliftDSPInstance` | `v1-required` | |
| Init | `initCDSPInstance` | `initCInterpreterDSPInstance` | `initCCraneliftDSPInstance` | `v1-required` | |
| instanceInit | `instanceInitCDSPInstance` | `instanceInitCInterpreterDSPInstance` | `instanceInitCCraneliftDSPInstance` | `v1-required` | |
| instanceConstants | `instanceConstantsCDSPInstance` | `instanceConstantsCInterpreterDSPInstance` | `instanceConstantsCCraneliftDSPInstance` | `v1-required` | |
| instanceResetUserInterface | `instanceResetUserInterfaceCDSPInstance` | `instanceResetUserInterfaceCInterpreterDSPInstance` | `instanceResetUserInterfaceCCraneliftDSPInstance` | `v1-required` | |
| instanceClear | `instanceClearCDSPInstance` | `instanceClearCInterpreterDSPInstance` | `instanceClearCCraneliftDSPInstance` | `v1-required` | |
| Metadata | `metadataCDSPInstance` | `metadataCInterpreterDSPInstance` | `metadataCCraneliftDSPInstance` | `v1-required` | Meta callbacks mandatory in V1 |
| Compute | `computeCDSPInstance` | `computeCInterpreterDSPInstance` | `computeCCraneliftDSPInstance` | `v1-required` | |

## 3.8 Memory helpers / shared utilities

| Family | LLVM C API | Interpreter C API | Target (Cranelift C API) | Status | Notes |
|---|---|---|---|---|---|
| `freeCMemory` | Present in LLVM C API family support stack | Present in interpreter C API implementation/export | `freeCMemory` | `v1-required` | Shared utility name (same symbol strategy) |

---

## 4. C++ Wrapper Parity Matrix (Initial Draft)

Phase 0 requirement is to mirror the **usage strategy** and class roles of:

- `llvm_dsp` / `llvm_dsp_factory`
- `interpreter_dsp` / `interpreter_dsp_factory`

Initial V1 target classes:

- `cranelift_dsp` (inherits/behaves as `dsp`)
- `cranelift_dsp_factory` (inherits/behaves as `dsp_factory`)

### Required V1 wrapper capabilities

- create/delete factory wrappers over C API
- create/delete DSP instances
- `init`, `compute`
- `buildUserInterface`, `metadata`
- cache-aware factory acquisition/release strategy

### Phase 0 action item (still pending)

Add a method-by-method matrix (C++ wrappers) mapped from:

- `llvm-dsp.h` methods/functions (primary)
- `interpreter-dsp.h` methods/functions (secondary cross-check)

This submatrix is not complete yet and must be finalized before Phase 5.

---

## 5. Open Decisions (Must be Resolved Before Deep Implementation)

1. **Exact Cranelift C names for LLVM-only generic C helpers**
   - examples: `getCName`, `getCSHAKey`, `getCDSPCode`, `getCTarget`
   - choose whether to keep LLVM generic names for strict set parity, or use
     backend-prefixed Cranelift names + aliases
2. **V1 policy for LLVM-only serialization families (IR/machine)**
   - real implementation vs typed unsupported stubs with exported symbols
3. **Memory manager and foreign-function registration C APIs**
   - full V1 implementation vs exported typed unsupported stubs
4. **Machine target getter naming and semantics**
   - shared/global symbol vs backend-prefixed symbol

Per repository collaboration rule, these require explicit requester confirmation
before implementation proceeds on the affected function families.
