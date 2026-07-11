/-
  Lean 4 specification for:

    vector-mode-signal-level-analysis-cpp-port-plan-2026-07-10-en.md

  Scope
  -----
  This file mechanizes the finite structural core of section 5:

  * signal decorations and representative typing rules;
  * dependency graphs with consumer -> dependency edges;
  * valid schedule certificates and an executable certificate checker;
  * the four public scheduling strategy tags;
  * the ordered C++ needSeparateLoop decision;
  * effects, placement, epochs, and typed transports;
  * region-aware lowering and fission/simulation proof obligations;
  * abstract delay and repeated-transition semantics.

  It deliberately does not claim a proof of the complete Faust compiler.
  FissionSafe, schedule independence, and scalar/vector simulation are exposed
  as propositions that later implementation-specific developments must prove.

  This file uses only Lean's bundled Std library. It contains no `sorry` and no
  axioms. Validate it with:

      lean porting/vector-mode-scheduling-formal-spec.lean

  How to read this file
  ---------------------
  Lean distinguishes programs from propositions, but both are ordinary terms:

  * definitions returning `Bool`, such as `verifySchedule`, are executable;
  * definitions returning `Prop`, such as `ValidSchedule`, state contracts;
  * a `theorem` supplies a checked proof of a proposition;
  * a proof-valued structure field, such as `Scheduler.sound`, is an obligation
    that every future implementation must fill before Lean accepts it.

  Consequently, the file proves the algebraic and finite checker lemmas that do
  not depend on faust-rs, while representing compiler-specific correctness as
  explicit interfaces. Merely compiling this file does not prove the current
  Rust compiler correct; it proves that this specification is well typed, has
  no admitted facts, and that every theorem body is accepted by Lean's kernel.

  Naming conventions
  ------------------
  Names ending in `B` return `Bool` and can be evaluated. Their proposition-level
  wrappers use equality with `true`. Graph edges always point from a consumer to
  one of its dependencies. Thus a dependency must occur before its consumer in
  a valid execution order.
-/

import Std

namespace Faust.VectorScheduling

abbrev SigId := Nat
abbrev LoopId := Nat
abbrev EpochId := Nat
abbrev ClockId := Nat

/-
  Identifiers are natural numbers because the porting plan requires stable,
  deterministic allocation. They are aliases, not distinct runtime wrappers:
  the model focuses on semantics rather than Rust representation details.
-/

/-! ## Analysis domains -/

/-
  `Rate` is the three-point variability lattice used by the signal analysis:

      Konst < Block < Samp

  `join` computes the least upper bound. The three following theorems establish
  the semilattice laws needed to combine operand analyses independently of
  traversal grouping.
-/

inductive Rate where
  | konst
  | block
  | samp
  deriving Repr, DecidableEq, BEq

namespace Rate

def join : Rate -> Rate -> Rate
  | .samp, _ => .samp
  | _, .samp => .samp
  | .block, _ => .block
  | _, .block => .block
  | .konst, .konst => .konst

def isSlow : Rate -> Bool
  | .samp => false
  | _ => true

theorem join_comm (a b : Rate) : join a b = join b a := by
  cases a <;> cases b <;> rfl

theorem join_assoc (a b c : Rate) : join (join a b) c = join a (join b c) := by
  cases a <;> cases b <;> cases c <;> rfl

theorem join_idem (a : Rate) : join a a = a := by
  cases a <;> rfl

end Rate

inductive Vectorability where
  | vect
  | scal
  | trueScal
  deriving Repr, DecidableEq, BEq

namespace Vectorability

/-
  Vectorability is ordered from least to most restrictive:

      Vect < Scal < TrueScal

  A parent inherits the strongest restriction of its children. `TrueScal`
  denotes an operation whose semantics intrinsically require scalar execution;
  `Scal` may still participate in a serial island inside a vector plan.
-/

def join : Vectorability -> Vectorability -> Vectorability
  | .trueScal, _ => .trueScal
  | _, .trueScal => .trueScal
  | .scal, _ => .scal
  | _, .scal => .scal
  | .vect, .vect => .vect

theorem join_comm (a b : Vectorability) : join a b = join b a := by
  cases a <;> cases b <;> rfl

theorem join_assoc (a b c : Vectorability) :
    join (join a b) c = join a (join b c) := by
  cases a <;> cases b <;> cases c <;> rfl

end Vectorability

inductive ValueTy where
  | int
  | real
  | tuple (components : List ValueTy)
  deriving Repr

def promoteNumeric : ValueTy -> ValueTy -> Option ValueTy
  | .int, .int => some .int
  | .int, .real => some .real
  | .real, .int => some .real
  | .real, .real => some .real
  | _, _ => none

/-
  This deliberately small value-type universe is sufficient to state the
  scheduling invariants. `none` makes an invalid numeric combination explicit
  instead of silently inventing a coercion. Backend-specific widths and
  `FaustFloat` lowering belong to the FIR refinement of this model.
-/

inductive Purity where
  | pure
  | impure
  | unknown
  deriving Repr, DecidableEq, BEq

inductive Resource where
  | state (id : Nat)
  | table (id : Nat)
  | ui (id : Nat)
  | output (channel : Nat)
  | foreign (name : String)
  deriving Repr, DecidableEq, BEq

inductive Effect where
  | read (resource : Resource)
  | write (resource : Resource)
  | foreignCall (name : String) (purity : Purity)
  deriving Repr, DecidableEq, BEq

namespace Effect

/-
  `conflicts a b` is a conservative dependence test. Two reads commute. Accesses
  to different resources commute. A write conflicts with another access to the
  same resource. Pure foreign calls add no ordering edge, while impure or unknown
  calls conflict conservatively because the model cannot inspect their effects.
-/

def conflicts : Effect -> Effect -> Bool
  | .foreignCall _ .pure, _ => false
  | _, .foreignCall _ .pure => false
  | .foreignCall _ _, _ => true
  | _, .foreignCall _ _ => true
  | .read _, .read _ => false
  | .read a, .write b => a == b
  | .write a, .read b => a == b
  | .write a, .write b => a == b

end Effect

def effectsCommute (left right : List Effect) : Bool :=
  left.all fun a => right.all fun b => !(Effect.conflicts a b)

/-
  Pairwise commutation is the condition for leaving two incomparable loops
  unordered. If this check is false, planning must add an effect edge, merge the
  operations, or place them in a serial island.
-/

example : effectsCommute
    [.read (.table 0)] [.read (.table 0)] = true := by
  decide

example : effectsCommute
    [.read (.table 0)] [.write (.table 0)] = false := by
  decide

structure Decoration where
  valueTy : ValueTy
  rate : Rate
  vectorability : Vectorability
  clock : ClockId
  effects : List Effect
  deriving Repr

/-
  A decoration is the product judgment attached to a prepared signal. Keeping
  type, rate, vectorability, clock, and effects together prevents later passes
  from making locally inconsistent scheduling decisions.
-/

/-! ## Representative signal typing judgment -/

/-
  `HasType expression decoration` is an inductively generated judgment. A value
  exists only when one constructor proves it. These constructors are a compact,
  representative subset of Faust signals rather than a replacement AST:

  * literals and inputs introduce base facts;
  * `bin` joins rates and vectorability after checking clocks and promotion;
  * `delay` records both the state read and state write;
  * `proj` preserves recursion-group effects but forces scalar treatment;
  * `clocked` changes only the explicit clock domain.

  Adding a real signal node requires adding a constructor whose conclusion
  states all five decoration components, making omissions visible to Lean.
-/

inductive Expr where
  | intLit (value : Int)
  | realLit (value : Float)
  | input (channel : Nat)
  | bin (left right : Expr)
  | delay (value amount : Expr) (state : Resource)
  | proj (index : Nat) (group : Expr)
  | clocked (clock : ClockId) (value : Expr)
  deriving Repr

inductive HasType : Expr -> Decoration -> Prop where
  | intLit (value : Int) (clock : ClockId) :
      HasType (.intLit value)
        { valueTy := .int
          rate := .konst
          vectorability := .vect
          clock := clock
          effects := [] }
  | realLit (value : Float) (clock : ClockId) :
      HasType (.realLit value)
        { valueTy := .real
          rate := .konst
          vectorability := .vect
          clock := clock
          effects := [] }
  | input (channel : Nat) (clock : ClockId) :
      HasType (.input channel)
        { valueTy := .real
          rate := .samp
          vectorability := .vect
          clock := clock
          effects := [] }
  | bin {left right : Expr} {dl dr : Decoration} {resultTy : ValueTy}
      (leftTyped : HasType left dl)
      (rightTyped : HasType right dr)
      (sameClock : dl.clock = dr.clock)
      (promoted : promoteNumeric dl.valueTy dr.valueTy = some resultTy) :
      HasType (.bin left right)
        { valueTy := resultTy
          rate := Rate.join dl.rate dr.rate
          vectorability := Vectorability.join dl.vectorability dr.vectorability
          clock := dl.clock
          effects := dl.effects ++ dr.effects }
  | delay {value amount : Expr} {dv da : Decoration} (state : Resource)
      (valueTyped : HasType value dv)
      (amountTyped : HasType amount da)
      (amountIsInt : da.valueTy = .int)
      (sameClock : dv.clock = da.clock) :
      HasType (.delay value amount state)
        { valueTy := dv.valueTy
          rate := Rate.join dv.rate da.rate
          vectorability := Vectorability.join dv.vectorability da.vectorability
          clock := dv.clock
          effects := dv.effects ++ da.effects ++ [.read state, .write state] }
  | proj {index : Nat} {group : Expr} {dg : Decoration}
      {components : List ValueTy} {component : ValueTy}
      (groupTyped : HasType group dg)
      (groupIsTuple : dg.valueTy = .tuple components)
      (componentAt : components[index]? = some component) :
      HasType (.proj index group)
        { valueTy := component
          rate := .samp
          vectorability := .scal
          clock := dg.clock
          effects := dg.effects }
  | clocked {value : Expr} {d : Decoration} (clock : ClockId)
      (valueTyped : HasType value d) :
      HasType (.clocked clock value) { d with clock := clock }

/-! ## Dependency graph and schedule certificates -/

/-
  `DependencyGraph Node` is finite because `nodes` is a list. For a consumer
  `u`, `dependencies u` lists every `v` required by `u`; this is the C++ Faust
  edge convention `u -> v`. A well-formed adapter must list every dependency in
  `nodes`. The schedule checker detects an omitted endpoint because such a node
  cannot be found in the proposed order.

  `BEq` provides executable equality and `LawfulBEq` proves that this Boolean
  equality agrees with Lean equality. Requiring both avoids certificates built
  with a pathological comparison function.
-/

structure DependencyGraph (Node : Type) where
  nodes : List Node
  dependencies : Node -> List Node

def position? [BEq Node] [LawfulBEq Node] (needle : Node) : List Node -> Option Nat
  | [] => none
  | item :: rest =>
      if item == needle then
        some 0
      else
        match position? needle rest with
        | none => none
        | some index => some (index + 1)

/- `position?` is intentionally total: an absent node returns `none`. -/

def beforeB [BEq Node] [LawfulBEq Node]
    (order : List Node) (first second : Node) : Bool :=
  match position? first order, position? second order with
  | some firstIndex, some secondIndex => firstIndex < secondIndex
  | _, _ => false

def Before [BEq Node] [LawfulBEq Node]
    (order : List Node) (first second : Node) : Prop :=
  beforeB order first second = true

def noDuplicatesB [BEq Node] [LawfulBEq Node] : List Node -> Bool
  | [] => true
  | node :: rest => !rest.contains node && noDuplicatesB rest

/-
  A schedule covers a graph exactly when it is a duplicate-free permutation of
  `graph.nodes`. The two `all` clauses check both inclusions instead of assuming
  that equal list lengths imply equality of finite sets.
-/

def coversB [BEq Node] [LawfulBEq Node]
    (graph : DependencyGraph Node) (order : List Node) : Bool :=
  noDuplicatesB order
    && graph.nodes.all (fun node => order.contains node)
    && order.all (fun node => graph.nodes.contains node)

def respectsDependenciesB [BEq Node] [LawfulBEq Node] (graph : DependencyGraph Node)
    (order : List Node) : Bool :=
  graph.nodes.all fun consumer =>
    (graph.dependencies consumer).all fun dependency =>
      beforeB order dependency consumer

/-
  This is the executable form of

      forall consumer, dependency in deps(consumer),
        position(dependency) < position(consumer).

  `validScheduleB` combines that causal condition with permutation coverage.
-/

def validScheduleB [BEq Node] [LawfulBEq Node] (graph : DependencyGraph Node)
    (order : List Node) : Bool :=
  coversB graph order && respectsDependenciesB graph order

def ValidSchedule [BEq Node] [LawfulBEq Node] (graph : DependencyGraph Node)
    (order : List Node) : Prop :=
  validScheduleB graph order = true

def verifySchedule [BEq Node] [LawfulBEq Node] (graph : DependencyGraph Node)
    (order : List Node) : Bool :=
  validScheduleB graph order

theorem verifySchedule_eq_true_iff [BEq Node] [LawfulBEq Node]
    (graph : DependencyGraph Node) (order : List Node) :
    verifySchedule graph order = true ↔ ValidSchedule graph order := by
  rfl

/-
  The theorem is reflexive because the executable checker is the definition of
  the proposition. This small trusted checker can validate orders produced by
  a more complicated scheduler without reusing that scheduler's algorithm.
-/

inductive SchedulingStrategy where
  | depthFirst
  | breadthFirst
  | special
  | reverseBreadthFirst
  deriving Repr, DecidableEq, BEq

/-
  The constructors are the public `-ss` contract:

  * `depthFirst`          = `-ss 0`;
  * `breadthFirst`        = `-ss 1`;
  * `special`             = `-ss 2`;
  * `reverseBreadthFirst` = `-ss n` for every `n >= 3`.

  Strategy-specific order generation is intentionally behind `Scheduler.run`.
  All strategies share the same independently checked validity predicate.
-/

def decodeStrategy : Nat -> SchedulingStrategy
  | 0 => .depthFirst
  | 1 => .breadthFirst
  | 2 => .special
  | _ => .reverseBreadthFirst

example : decodeStrategy 0 = .depthFirst := rfl
example : decodeStrategy 1 = .breadthFirst := rfl
example : decodeStrategy 2 = .special := rfl
example : decodeStrategy 3 = .reverseBreadthFirst := rfl
example : decodeStrategy 42 = .reverseBreadthFirst := rfl

structure ScheduleCertificate [BEq Node] [LawfulBEq Node]
    (graph : DependencyGraph Node) where
  strategy : SchedulingStrategy
  orderedNodes : List Node
  edgeHash : Nat
  valid : ValidSchedule graph orderedNodes

/-
  A certificate couples the candidate order to the exact graph (`edgeHash`) and
  records the requested strategy for diagnostics. Its `valid` field is proof,
  not metadata: a certificate cannot be constructed without satisfying the
  checker. In production, `edgeHash` must be derived deterministically from the
  graph snapshot rather than supplied by an untrusted caller.
-/

def certify? [BEq Node] [LawfulBEq Node] (graph : DependencyGraph Node)
    (strategy : SchedulingStrategy) (edgeHash : Nat) (order : List Node) :
    Option (ScheduleCertificate graph) :=
  if valid : verifySchedule graph order = true then
    some {
      strategy
      orderedNodes := order
      edgeHash
      valid := (verifySchedule_eq_true_iff graph order).mp valid }
  else
    none

/- `certify?` is the trust boundary: invalid candidate orders become `none`. -/

theorem certificate_is_valid [BEq Node] [LawfulBEq Node]
    {graph : DependencyGraph Node}
    (certificate : ScheduleCertificate graph) :
    ValidSchedule graph certificate.orderedNodes :=
  certificate.valid

def HasValidSchedule [BEq Node] [LawfulBEq Node]
    (graph : DependencyGraph Node) : Prop :=
  ∃ order, ValidSchedule graph order

inductive ScheduleError (Node : Type) where
  | cycle (witness : List Node)
  | invalidGraph (message : String)
  deriving Repr

/-
  A scheduler must terminate with either a checked order or a typed error. A
  cycle carries nodes useful for diagnostics; malformed adapters use the second
  case. The full port may strengthen the cycle witness to prove adjacency.
-/

structure Scheduler (Node : Type) [BEq Node] [LawfulBEq Node] where
  run : SchedulingStrategy -> DependencyGraph Node ->
    Except (ScheduleError Node) (List Node)
  sound : ∀ strategy graph order,
    run strategy graph = .ok order -> ValidSchedule graph order
  complete : ∀ strategy graph,
    HasValidSchedule graph -> ∃ order, run strategy graph = .ok order

/-
  The `run` field is a total Lean function, so termination is part of the
  implementation boundary. `sound` and `complete` are the S-Sound and
  S-Complete obligations. Determinism follows from `run` being a function.
-/

def diamondGraph : DependencyGraph Nat where
  nodes := [0, 1, 2, 3]
  dependencies
    | 3 => [1, 2]
    | 1 => [0]
    | 2 => [0]
    | _ => []

/-
  The diamond is a minimal nontrivial example. Both `[0,1,2,3]` and
  `[0,2,1,3]` are valid because nodes 1 and 2 are incomparable. The order
  `[1,0,2,3]` is rejected because consumer 1 precedes dependency 0.
-/

example : ValidSchedule diamondGraph [0, 1, 2, 3] := by
  rfl

example : ValidSchedule diamondGraph [0, 2, 1, 3] := by
  rfl

example : verifySchedule diamondGraph [1, 0, 2, 3] = false := by
  decide

def diamondCertificate : ScheduleCertificate diamondGraph where
  strategy := .depthFirst
  orderedNodes := [0, 1, 2, 3]
  edgeHash := 0
  valid := by rfl

/-! ## Exact loop-separation decision -/

/-
  `SignalFacts` contains only the inputs used by the C++ `needSeparateLoop`
  decision. The nested `if` expression is normative: it preserves first-match
  priority rather than treating the conditions as an unordered Boolean formula.
  In particular, a positive delayed use always creates a loop, even for a very
  simple or slow signal.
-/

structure SignalFacts where
  maxDelay : Nat
  verySimple : Bool
  rate : Rate
  delayRead : Bool
  recursiveProjection : Bool
  multipleOccurrences : Bool
  deriving Repr, DecidableEq

def separateLoop (facts : SignalFacts) : Bool :=
  if 0 < facts.maxDelay then
    true
  else if facts.verySimple || facts.rate.isSlow then
    false
  else if facts.delayRead then
    false
  else if facts.recursiveProjection then
    true
  else if facts.multipleOccurrences then
    true
  else
    false

theorem maxDelay_dominates (facts : SignalFacts) (hasDelay : 0 < facts.maxDelay) :
    separateLoop facts = true := by
  simp [separateLoop, hasDelay]

theorem verySimple_without_delay_is_inline (facts : SignalFacts)
    (noDelay : facts.maxDelay = 0) (simple : facts.verySimple = true) :
    separateLoop facts = false := by
  simp [separateLoop, noDelay, simple]

/-
  These two theorems pin the most regression-prone precedence cases. Additional
  truth-table cases can be added as `example`s and discharged by `decide`.
-/

example : separateLoop
    { maxDelay := 1
      verySimple := true
      rate := .block
      delayRead := true
      recursiveProjection := false
      multipleOccurrences := false } = true := by
  decide

/-! ## Placement, epochs, transports, and vector-plan certificates -/

/-
  Placement separates ownership from execution order:

  * `control` is materialized in a fixed slower/lifecycle region;
  * `inline` may be rebuilt in several loops, but only when duplicable;
  * `owned loop` has one materialized producer loop.

  Loop kind records whether a loop may use vector FIR, must preserve a recursive
  group's serial semantics, or is a serial clock/effect island.
-/

inductive Placement where
  | control
  | inline
  | owned (loop : LoopId)
  deriving Repr, DecidableEq, BEq

inductive LoopKind where
  | vectorizable
  | recursive (group : Nat)
  | island (clock : ClockId)
  deriving Repr, DecidableEq, BEq

structure ExecutionEpoch where
  id : EpochId
  rank : Nat
  loops : List LoopId
  deriving Repr, DecidableEq

/-
  Epochs are fixed semantic phases, for example forward and reverse AD passes.
  `id` is stable identity; `rank` is execution order. Scheduling may reorder
  loops inside an epoch but must never reorder or interleave epochs.
-/

structure Transport where
  signal : SigId
  producer : LoopId
  consumer : LoopId
  elementTy : ValueTy
  length : Nat
  deriving Repr

/-
  A transport is the explicit chunk-local storage required when an owned signal
  is consumed by another loop. Its element type must match the signal type and
  its array length must equal the vector chunk size.
-/

def Transport.WellTyped (typeOf : SigId -> ValueTy) (vecSize : Nat)
    (transport : Transport) : Prop :=
  transport.producer ≠ transport.consumer
    ∧ transport.elementTy = typeOf transport.signal
    ∧ transport.length = vecSize

def chunkIndex (i0 vindex : Nat) : Nat := i0 - vindex

theorem chunkIndex_lt (i0 vindex vecSize : Nat)
    (lower : vindex ≤ i0) (upper : i0 < vindex + vecSize) :
    chunkIndex i0 vindex < vecSize := by
  simp [chunkIndex]
  omega

/-
  `chunkIndex_lt` proves the array-safety part of `j = i0 - vindex`: when `i0`
  lies in the current half-open chunk `[vindex, vindex + vecSize)`, `j` is a
  valid transport index. The lower bound also prevents truncated subtraction
  from hiding an invalid sample position.
-/

structure VectorPlan where
  signals : List SigId
  loops : List LoopId
  vecSize : Nat
  vecSizePositive : 0 < vecSize
  signalType : SigId -> ValueTy
  placement : SigId -> Placement
  duplicable : SigId -> Prop
  roots : LoopId -> List SigId
  loopKind : LoopId -> LoopKind
  vectorizationSafe : LoopId -> Prop
  epochs : List ExecutionEpoch
  transports : List Transport
  dataEdges : List (LoopId × LoopId)
  effectEdges : List (LoopId × LoopId)
  stableName : LoopId -> String
  placementDuplicate : ∀ signal,
    placement signal = .inline -> duplicable signal
  rootsNodup : ∀ loop, (roots loop).Nodup
  ownedHasRoot : ∀ signal loop,
    placement signal = .owned loop -> signal ∈ roots loop
  rootHasOwner : ∀ signal loop,
    signal ∈ roots loop -> placement signal = .owned loop

/-
  `VectorPlan` is the strategy-independent result of signal-level analysis.
  Proof fields enforce local construction invariants immediately:

  * an inline signal is duplicable;
  * roots contain no duplicates within a loop;
  * ownership and root membership agree in both directions.

  Functions such as `signalType`, `placement`, and `loopKind` stand for immutable
  tables in the Rust implementation. The absence of `SchedulingStrategy` is
  intentional and makes P-Strategy visible in the type itself.
-/

def VectorPlan.allEdges (plan : VectorPlan) : List (LoopId × LoopId) :=
  plan.dataEdges ++ plan.effectEdges

def VectorPlan.epochGraph (plan : VectorPlan)
    (epoch : ExecutionEpoch) : DependencyGraph LoopId where
  nodes := epoch.loops
  dependencies consumer :=
    plan.allEdges.filterMap fun edge =>
      if edge.1 == consumer && epoch.loops.contains edge.2 then
        some edge.2
      else
        none

/-
  `epochGraph` is the induced graph scheduled by `-ss`: it keeps only edges whose
  consumer and dependency belong to the selected epoch. Edges crossing epoch
  boundaries are enforced by barriers, not passed to the local scheduler.
-/

/-
  SchedulingStrategy is intentionally absent from VectorPlan. This type-level
  boundary encodes P-Strategy: changing -ss cannot alter ownership, epochs,
  edges, transports, loop ids, or stable names.
-/

def epochRankOf? (epochs : List ExecutionEpoch) (loop : LoopId) : Option Nat :=
  match epochs.find? (fun epoch => loop ∈ epoch.loops) with
  | none => none
  | some epoch => some epoch.rank

def EpochBarriersValid (epochs : List ExecutionEpoch) :
    List (LoopId × LoopId) -> Prop
  | [] => True
  | edge :: rest =>
      (match epochRankOf? epochs edge.1, epochRankOf? epochs edge.2 with
       | some consumerRank, some dependencyRank => dependencyRank ≤ consumerRank
       | _, _ => False)
      ∧ EpochBarriersValid epochs rest

/-
  Since edges are `consumer -> dependency`, every dependency epoch must have a
  rank no greater than its consumer epoch. Equality permits an intra-epoch edge;
  a smaller rank represents an already completed semantic phase. Unique ranks
  in `VectorPlanCertificate` prevent two distinct epochs from masquerading as
  the same phase.
-/

def AllTransportsWellTyped (typeOf : SigId -> ValueTy) (vecSize : Nat) :
    List Transport -> Prop
  | [] => True
  | transport :: rest =>
      transport.WellTyped typeOf vecSize
        ∧ AllTransportsWellTyped typeOf vecSize rest

def AllEdgesHaveEndpoints (loops : List LoopId) :
    List (LoopId × LoopId) -> Prop
  | [] => True
  | edge :: rest =>
      edge.1 ∈ loops ∧ edge.2 ∈ loops
        ∧ AllEdgesHaveEndpoints loops rest

structure VectorPlanCertificate (plan : VectorPlan) where
  loopsNodup : plan.loops.Nodup
  signalsNodup : plan.signals.Nodup
  epochIdsNodup : (plan.epochs.map (fun epoch => epoch.id)).Nodup
  epochRanksNodup : (plan.epochs.map (fun epoch => epoch.rank)).Nodup
  epochLoopsNodup : (plan.epochs.flatMap (fun epoch => epoch.loops)).Nodup
  epochLoopsCover : ∀ loop,
    loop ∈ plan.loops ↔ loop ∈ plan.epochs.flatMap (fun epoch => epoch.loops)
  edgeEndpoints : AllEdgesHaveEndpoints plan.loops plan.allEdges
  epochDag : ∀ epoch,
    epoch ∈ plan.epochs -> HasValidSchedule (plan.epochGraph epoch)
  transportTyped : AllTransportsWellTyped plan.signalType plan.vecSize plan.transports
  barriersValid : EpochBarriersValid plan.epochs plan.allEdges
  vectorizableSafe : ∀ loop,
    plan.loopKind loop = .vectorizable -> plan.vectorizationSafe loop

/-
  This certificate is the finite gate between planning and FIR lowering:

  * ids and epoch membership are unique and cover all loops;
  * every edge endpoint exists;
  * each induced epoch graph admits a valid topological schedule;
  * transports have the expected signal type and chunk size;
  * all edges respect fixed epoch order;
  * every loop labelled vectorizable carries its `VecSafe` witness.

  The fields are propositions because this file specifies required evidence.
  The Rust port should implement an executable `verify_vector_plan` whose
  successful result constructs the corresponding evidence or mirrors it in
  differential/exhaustive tests.
-/

/-! ## Region-aware lowering and fission obligations -/

/-
  The four lowering rules correspond directly to the mathematical rewrite
  rules R-INLINE, R-LOCAL, R-CROSS, and R-CONTROL. `LoweringWitness` records the
  selected case and the two semantic obligations common to every case: effects
  are emitted exactly once and storage does not alter value bits. A cross-loop
  rule additionally carries its producer and transport.
-/

inductive LoweringRule where
  | inline
  | local
  | cross
  | control
  deriving Repr, DecidableEq, BEq

structure LoweringWitness where
  rule : LoweringRule
  signal : SigId
  region : LoopId
  firType : ValueTy
  producer : Option LoopId
  transport : Option Transport
  effectsEmittedExactlyOnce : Prop
  valueBitsPreserved : Prop

structure Event where
  sample : Nat
  loop : LoopId
  statement : Nat
  deriving Repr, DecidableEq, BEq

def FissionSafe : List (Event × Event) ->
    (Event -> Event -> Prop) -> (Event -> Event -> Prop) -> Prop
  | [], _, _ => True
  | edge :: rest, scalarBefore, vectorBefore =>
      (scalarBefore edge.1 edge.2 -> vectorBefore edge.1 edge.2)
        ∧ FissionSafe rest scalarBefore vectorBefore

/-
  An event identifies a dynamic sample, loop, and statement. `FissionSafe`
  states that every true dependence ordered by scalar execution remains ordered
  after loop fission. The dependence list is an abstract semantic object; the
  compiler checks the finite sufficient facts below instead of enumerating all
  runtime events.
-/

structure StaticFissionFacts where
  loopDag : Prop
  transportsComplete : Prop
  effectsOrdered : Prop
  barriersOrdered : Prop
  carriedDependenciesInternalToSerialLoops : Prop

def StaticFissionSafe (facts : StaticFissionFacts) : Prop :=
  facts.loopDag
    ∧ facts.transportsComplete
    ∧ facts.effectsOrdered
    ∧ facts.barriersOrdered
    ∧ facts.carriedDependenciesInternalToSerialLoops

/-
  The five static facts summarize the implementable legality test. Their
  implication to dynamic `FissionSafe` is deliberately not asserted globally:
  it must be proved for the concrete faust-rs plan, dependence extraction, and
  execution relations.
-/

def StaticImpliesDynamicFission
    (Plan : Type) (staticSafe dynamicSafe : Plan -> Prop) : Prop :=
  ∀ plan, staticSafe plan -> dynamicSafe plan

/-
  The faust-rs port must instantiate `Plan`, `staticSafe`, and `dynamicSafe`,
  then prove `StaticImpliesDynamicFission`. It is intentionally a proof
  obligation here, not an assumed theorem.
-/

/-! ## Delay and transition semantics -/

/-
  The remaining definitions are polymorphic in the actual value and state
  representations. This keeps the specification usable for the interpreter,
  Cranelift, Wasm, AssemblyScript, and future backends.

  History is ordered newest first: `history[0]` is the previous sample. Delay
  zero reads the current value directly; delay `n > 0` reads slot `n - 1`.
  Out-of-range reads return `none`, forcing the concrete compiler proof to show
  that declared maximum delay bounds are respected.
-/

def delayRead (current : Value) (history : List Value) (delay : Nat) : Option Value :=
  if delay = 0 then some current else history[delay - 1]?

def historyStep (current : Value) (history : List Value) : List Value :=
  (current :: history).take history.length

/-
  `historyStep` inserts the current value and drops the oldest slot. The length
  theorem proves that one transition preserves the abstract delay-state shape.
-/

theorem delayRead_zero (current : Value) (history : List Value) :
    delayRead current history 0 = some current := by
  simp [delayRead]

theorem historyStep_length (current : Value) (history : List Value) :
    (historyStep current history).length = history.length := by
  simp [historyStep]

def iterate (count : Nat) (step : State -> State) (state : State) : State :=
  match count with
  | 0 => state
  | count + 1 => iterate count step (step state)

/-
  `iterate n step initial` is the abstract scalar `Run(n, ...)`. Defining it by
  structural recursion gives Lean an immediate termination proof and exposes
  zero/successor equations for later simulations.
-/

theorem iterate_zero (step : State -> State) (state : State) :
    iterate 0 step state = state := rfl

theorem iterate_succ (count : Nat) (step : State -> State) (state : State) :
    iterate (count + 1) step state = iterate count step (step state) := rfl

structure ExecutionResult (State Output Observation : Type) where
  finalState : State
  outputs : List Output
  observations : List Observation
  deriving Repr, DecidableEq

/-
  Equality of execution results covers persistent state, all output samples,
  and externally visible observations. Instantiations should include tables and
  UI zones in `State` or `Observation`, so simulation cannot ignore them.
-/

inductive LoopVariant where
  | fastest
  | simple
  deriving Repr, DecidableEq, BEq

def VSimulation
    (State Input Output Observation : Type)
    (scalarRun : State -> List Input -> ExecutionResult State Output Observation)
    (vectorRun : Nat -> LoopVariant -> List (List LoopId) -> State -> List Input ->
      ExecutionResult State Output Observation) : Prop :=
  ∀ vecSize variant epochSchedules initialState inputs,
    0 < vecSize ->
    vectorRun vecSize variant epochSchedules initialState inputs =
      scalarRun initialState inputs

/-
  `VSimulation` quantifies over both loop variants (`-lv 0` and `-lv 1`), every
  positive chunk size, every selected per-epoch schedule, initial state, and
  input sequence. A concrete theorem must additionally restrict
  `epochSchedules` to schedules certified valid for the program's epoch graphs;
  that program-indexed refinement belongs with the faust-rs execution model.
-/

def ScheduleIndependent
    (Schedule Observation : Type)
    (valid : Schedule -> Prop)
    (execute : Schedule -> Observation) : Prop :=
  ∀ left right, valid left -> valid right -> execute left = execute right

/-
  Schedule independence is the semantic requirement behind exposing `-ss`:
  any two valid orders must produce the same observation. Topological validity
  alone is insufficient; the vector-plan certificate's transport, effect, and
  barrier obligations are the premises needed by the eventual proof.
-/

/-! ## Executable smoke checks -/

/-
  `#eval` commands run while Lean elaborates this file. Expected output is:

      true
      false
      Faust.VectorScheduling.SchedulingStrategy.depthFirst
      Faust.VectorScheduling.SchedulingStrategy.reverseBreadthFirst
      true

  These checks are examples, not substitutes for the propositions above. They
  make accidental changes to edge direction, option decoding, or separation
  precedence immediately visible during standalone validation.
-/

#eval verifySchedule diamondGraph [0, 1, 2, 3]
#eval verifySchedule diamondGraph [1, 0, 2, 3]
#eval decodeStrategy 0
#eval decodeStrategy 3
#eval separateLoop
  { maxDelay := 0
    verySimple := false
    rate := .samp
    delayRead := false
    recursiveProjection := false
    multipleOccurrences := true }

end Faust.VectorScheduling
