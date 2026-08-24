# Wrela8 evidence for Layer 2 whole-Image planning

## Question

What useful evidence does Wrela8 provide for Core IR, control flow, ownership,
Pools, Actors, scheduling, capacity analysis, whole-Image planning, memory
accounting, diagnostics, and conformance—and which mechanisms should Wrela9
adopt, revise, or retire under its accepted domain model and Layer 1
foundations?

## Scope and method

This finding examines Wrela8 source at revision
[`40d1d9df`](https://github.com/rywible/wrela8/tree/40d1d9dff38c6c1dde527a9873108bfaeb8c775d)
and Wrela9's accepted design at revision
[`9f13393b`](https://github.com/rywible/wrela9/tree/9f13393be13693cd58de0bc23bc1789736c4b100).
The evidence comes from first-party compiler and standard-library source,
accepted Wrela9 design records, and executable Wrela8 cases. The earlier
`wrela8-semantic-inventory.md` was used only as a navigation aid; every claim
below links to the primary source that owns it.

"Adopt" means preserve the semantic obligation or architectural pattern,
"revise" means retain the lesson behind a different Wrela9 interface, and
"retire" means deliberately provide no compatibility obligation.

## Executive answer

Wrela8 proves that Wrela needs semantic IRs above a machine backend and that a
closed Image can drive async-frame sizing, mailbox and Pool storage, placement,
runtime wiring, and admission evidence. Its strongest reusable mechanisms are:

- a closed instruction vocabulary with centralized use/definition/effect facts;
- explicit async states, suspension transitions, liveness, frame-home planning,
  deterministic slot coloring, and independent plan validation;
- path-sensitive initialization and ownership state joined across branches and
  loops;
- generation-checked Pool keys, exact declared backing, and fail-closed
  whole-Image checks;
- a pure Image-construction step followed by graph checks;
- executable cases for Actor non-reentrancy, no-drop admission, ownership
  recovery, typed Replies, Groups, cancellation, and deadlines.

Those are evidence for obligations, not data structures to port. Wrela8 also
demonstrates why Wrela9's accepted boundaries are necessary: semantic identity
is string/index based; MWIR mixes semantic, machine, Driver, and product
operations; FlowWir embeds MWIR operations; proofs can rewrite source-visible
types; scheduler capacity is confused with transport rings; planning generates
Wrela source and re-runs semantic analysis; placement reports unproved work;
diagnostics are mostly strings; and conformance relies heavily on private dumps
and textual goldens. Wrela9 already rejects those mechanisms in favor of stable
semantic identities, immutable verified artifacts, target-neutral
`ImagePlan`/Logical Image Layout, explicit proof-required operations, logical
Actor ordering, and structured observations
([compiler representation stack](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/design/compiler.md#L93-L122),
[artifact verification](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/design/compiler.md#L154-L166),
[layout domains](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/design/compiler.md#L170-L182)).

The Layer 2 specification should therefore translate Wrela8's successful
semantic obligations into Wrela9-owned proof artifacts and verifiers. It should
not begin by renaming MWIR, FlowWir, `ImageGraph`, or `ImageLayout`.

## Evidence and disposition

### 1. Core IR and semantic optimization

#### Adopt

Wrela8's MWIR has a finite operation vocabulary, typed temporaries, explicit
control transfer, calls, checked operations, Panics/aborts, and mutation. More
importantly, instruction dataflow and removability facts are centralized:
`mwir_facts` distinguishes uses, definitions, address escape, `MayTrap`, and
`Observable` instead of allowing every optimization to reinterpret an
operation
([MWIR program shape](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/mwir.rs#L17-L34),
[central facts](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/mwir_facts.rs#L1-L55),
[effectful cases](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/mwir_facts.rs#L147-L220)).
Layer 2 should preserve the single authoritative operation-law catalogue:
typing, ownership modes, evaluation order, possible Panic, effects, dataflow,
and legal transformations should be exhaustive over one closed Core operation
set.

Wrela8 also separates a certified range-proof result from an ordinary optimized
program and fails closed when proof application fails
([checked optimization result](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/mwir_opt.rs#L79-L112),
[proof application](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/mwir_opt.rs#L112-L180)).
The reusable principle is that proof-bearing rewrites cross an explicit checked
boundary.

#### Revise

Core IR must be monomorphized and keyed by Wrela9 `SpecializationId`, `TypeId`,
and provenance rather than function-name strings and positional `Temp`s. Its
completed artifact must record phase schema, catalog revision, input receipts,
and fingerprint, and an independent verifier must validate it before any proof
or optimization influences admission. This follows Wrela9's established
identity flow and verifier contract
([identity flow](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/design/compiler.md#L61-L87),
[Core responsibility](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/design/compiler.md#L118-L120),
[verification](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/design/compiler.md#L154-L164)).

Facts such as ownership, possible Panic, suspension, allocation, boundedness,
and cost should remain separate typed analyses rather than collapse into
Wrela8's three-valued removability effect. Wrela9 has already accepted that
separation and requires Core transformations to preserve observable failure and
ordering
([behavior facts](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/design/compiler.md#L154-L160)).

#### Retire

Do not port MWIR as Wrela9 Core IR. Its operation enum contains machine and
product substrate alongside language semantics, and the optimizer is controlled
through thread-local global switches
([global optimization switches](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/mwir_opt.rs#L1-L77)).
Retire string function keys, product-specific packet instructions, backend
register/storage assumptions, the custom general-purpose optimizer, and all
private dump formats as contracts. Cranelift owns generic machine optimization;
Wrela Core owns only Wrela semantics
([accepted backend boundary](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/adr/0034-keep-wrela-semantic-irs-above-cranelift.md#L1-L3)).

### 2. Control flow and ownership

#### Adopt

Wrela8's flow checker models every storage path as `Uninit`, `Init`, or `Moved`,
joins states deterministically across alternatives, forbids reads and repeated
moves, prevents overlapping exclusive arguments in source order, rejects
overwriting a live Resource, and checks that borrowed `mut` storage is restored
and protocol Resources are consumed on every recoverable exit
([path-state lattice and joins](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/flow.rs#L15-L73),
[exit obligations](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/flow.rs#L99-L224),
[read/move/overlap checks](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/flow.rs#L279-L390)).
These are direct behavioral obligations for Wrela9 ownership analysis.

Defer processing is integrated with normal completion, `return`, `break`, and
`continue`, rather than being an afterthought
([exit handling](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/flow.rs#L858-L900),
[statement exits](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/flow.rs#L1023-L1066)).
Preserve that integration for every recoverable exit, including cancellation
and Reply closure.

#### Revise

Move the reusable dataflow algorithm behind a deep ownership Module over
verified concrete Typed HIR/Core operations. Its result should be an immutable,
identity-keyed proof artifact with bounded provenance paths, not mutable maps
whose only public outcome is the first `SemaError`. Ownership before and after
suspension must be stated explicitly at Flow boundaries, and the verifier must
confirm that lowering did not invent, lose, duplicate, or implicitly destroy a
Resource.

Wrela8 iterates loops with a fixed cap of four passes and silently accepts the
last candidate if no equality was observed
([loop iteration](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/flow.rs#L1128-L1166)).
Wrela9 should solve its finite lattice to convergence with a proven bound or
reject a violated internal invariant; a magic analysis cap must not weaken
ownership correctness.

#### Retire

Retire duplicate AST/Typed-AST scans as the authority for ownership, capacity,
and cleanup. Retire Wrela8's abandonment cleanup rule: Wrela9 Panic ends the
Image without source cleanup, while cancellation is recoverable and must return
moved Resources and run cleanup in reverse order
([accepted cancellation/Panic split](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/adr/0006-separate-cancellation-cleanup-from-panic.md#L1-L3)).

### 3. Flow IR, suspension, and async frames

#### Adopt

Wrela8 FlowWir represents an async body as explicit states containing operations
and one transition. Await transitions identify the requested operation, resume
state, and result definition; Group creation/start/close are explicit
([FlowWir shape](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/flowwir.rs#L8-L116)).
That is strong evidence that suspension must survive in a Wrela-owned artifact
until scheduler semantics and storage are explicit.

The liveness pass gives operations, transitions, and resume definitions
first-class points, rejects malformed jumps and temporaries, and computes the
exact set saved across each suspension
([liveness model](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/flow_liveness.rs#L1-L62),
[fail-closed analysis entry](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/flow_liveness.rs#L196-L208),
[suspension facts](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/flow_liveness.rs#L518-L559)).
Frame planning classifies persistent, boundary-live, resume-result, escaped,
pinned, and local values; assigns durable homes; validates that every live value
has storage; and independently checks register interference
([frame planning and validation](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/frame_plan.rs#L332-L518)).
Deterministic frame-slot coloring then validates the resulting interference
assignment again
([frame coloring](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/frame_color.rs#L94-L240)).
Adopt this plan/verify separation and exact async-frame accounting.

#### Revise

Wrela9 Flow IR must name Actor Turns, Mailboxes, explicit Reply Resources,
logical admission, deadlines, cancellation checkpoints, cleanup obligations,
and generated resume roles through typed identities and closed operations.
Frame layout in Layer 2 is target-neutral logical storage; register caches,
calling conventions, and native offsets belong to Target ABI lowering. A Flow
proof should state both semantic preservation and the storage/service facts
consumed by `ImagePlan`.

#### Retire

Retire `FlowInst::Mwir`, string `method_key`/`callee_key`, shared positional
`Temp`s, source-field-name paths, and a frame representation that already
assumes native scalar sizes and registers. Do not make Flow text dumps or state
numbers stable public contracts. Wrela9's accepted contract is that Flow IR
survives until scheduler behavior is explicit, then lowers to ordinary and
generated resume functions
([Flow boundary](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/design/compiler.md#L118-L122)).

### 4. Pools, Keys, and capacity

#### Adopt

Wrela8's `SlotMap` demonstrates the necessary stale-key behavior: a key carries
map identity, slot index, and generation; foreign, empty, out-of-range, and
stale keys miss; reclaim advances generation; and exhausted generations retire
rather than wrap
([implementation](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/stdlib/core/slotmap.wr#L9-L126),
[executable discipline cases](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/check-slotmap-key-discipline/src/examples/check_slotmap_key_discipline.wr#L6-L103)).
Wrela8 also rejects omitted, non-integer, zero, and oversized backing rather
than guessing Pool capacity
([exact Pool backing](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/eval/image_checks.rs#L482-L750)).

#### Revise

Make Pool identity generative and typed: `PoolId`, `own[P] T`, and unforgeable
`Key[P,T]`. `try_allocate` must return `PoolFull[T]` with the rejected value;
`lookup` is copy-only; `reclaim` consumes ownership. Capacity is the maximum
simultaneously live allocation count, and ordinary-exit reclamation participates
in the same ownership proof. These requirements are already accepted for Layer
2
([Pool language semantics](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/design/language-core.md#L15-L35),
[scoped Pool handoff](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/adr/0052-open-scoped-pools-through-an-authenticated-factory.md#L1-L5)).

Wrela8's reserve proof usefully emits occupancy arithmetic and a why-chain, but
its proof is static-site counting with broad exclusions for loops, closures,
multiple call paths, and Group children
([reserve proof](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/reserve_proof.rs#L61-L188),
[diagnostic evidence](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/reserve_proof.rs#L201-L231)).
Wrela9 should retain explainable demand, supply, concurrency, and ownership
evidence while deriving maximum live allocation from verified Core/Flow paths,
closed call families, Replies, Groups, cleanup, and service plans.

#### Retire

Retire publicly constructible numeric `Key` fields, string Pool identity,
Wrela8's fixed `u8` generation choice as a language contract, and separate
product Pool/DMA-Pool identity mechanisms. Most importantly, retire proof-
conditioned source typing: Wrela8 turns `VirtQueue.reserve` from
`Result[QueuePermit, CapacityError]` into `QueuePermit` when its proof holds
([intrinsic rule](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/intrinsics.rs#L162-L165)).
Wrela9 instead has distinct `allocate`, `reserve`, and `try_allocate` operations;
proof success can erase checks but never change source-visible types
([capacity/type boundary](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/adr/0019-keep-capacity-proofs-out-of-source-types.md#L1-L5),
[allocation intents](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/adr/0027-separate-required-allocation-from-capacity-pressure.md#L1-L3)).

### 5. Actors, admission, scheduling, and service

#### Adopt

The following Wrela8 executable scenarios are durable semantic evidence:

- a suspended Actor Turn is non-reentrant and FIFO admission yields `123`, not
  `132`
  ([case](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-actors/input.wr#L59-L94));
- a full Mailbox rejects explicitly and returns a moved argument rather than
  dropping it
  ([case](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-await-rejected/input.wr#L16-L56));
- same-core and cross-core calls preserve typed scalar, aggregate, and
  `Result` Replies
  ([wide Reply case](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-actor-reply-result/input.wr#L141-L207),
  [cross-core case](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-cross-core-call/input.wr#L14-L29));
- bounded Groups join every child, inherit an earlier parent deadline, and run
  cancellation cleanup in reverse order
  ([join](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-group-join/input.wr#L27-L48),
  [deadline inheritance](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-deadline-inherit/input.wr#L28-L56),
  [cleanup](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-cancel-cleanup/input.wr#L25-L51)).

Static Actor wiring remains a useful simplifier for closed message topology and
capacity analysis, and Wrela9 has explicitly accepted it
([static destinations](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/adr/0020-keep-actor-message-destinations-static.md#L1-L5)).

#### Revise

Wrela8's bare-send proof counts static sites against the smallest Mailbox for an
Actor type and rejects sites inside loops, closures, Groups, recursive or
multi-caller paths
([capacity derivation](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/send_proof.rs#L190-L240),
[site proof](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/send_proof.rs#L259-L340)).
Retain the fail-closed and explainable proof discipline, but derive Wrela9
admission from concrete Flow executions, stable logical send ordinals, Reply
reservations, deadlines, cancellation, and the scheduler service plan.

Wrela9 needs one global logical destination capacity covering resident,
cross-core admitted, and reserved messages. Transport queues hold proposals,
not additional admitted messages. Actors remain compiler-placed, and concurrent
effects commit in a stable total order
([logical ordering](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/adr/0028-order-actor-effects-logically-and-fix-core-placement.md#L1-L3)).
The plan must also report bounded service and cancellation-observation delay for
each ready class through a compiler-planned cyclic service schedule
([scheduler service](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/adr/0029-use-compiler-planned-cyclic-scheduler-service.md#L1-L3)).

#### Retire

Retire Wrela8 `CallError.NotAdmitted`, transport-ring capacity as observable
Mailbox capacity, and immediate fallibility as the only admission operation.
Layer 2 must specify ordinary fallible/waiting operations and explicit one-shot
Reply ownership under the accepted Wrela9 vocabulary.

Retire host-arrival nondeterminism: Wrela8 explicitly accepts either `1` or
`11` for an intermediate cross-core result
([nondeterministic case](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-cross-core-admission-order/input.wr#L43-L70)).
The Wrela9 version must have one exact logical observation.

Also retire drain-first round-robin scheduling, runtime queue depth as dynamic
priority, Actor migration/work stealing, and any service decision derived from
host timing. These are explicitly superseded by the cyclic service ADR.

### 6. Whole-Image construction and admission

#### Adopt

Wrela8 evaluates one `@image` function, requires it to seal the result, and then
runs whole-graph checks
([evaluation boundary](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/eval/interp.rs#L208-L236),
[check entry](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/eval/image_checks.rs#L78-L106)).
The checks cover a construction DAG, declared/bound Pools, device uniqueness,
initialization, failure policy, Driver configuration, vectors, placement, and
product declarations. Cycle diagnostics report the concrete edge path
([construction proof](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/eval/image_checks.rs#L449-L480)).
Preserve the separation between local Build Constructor validation and a later
generic whole-graph sealer.

#### Revise

Wrela8's mutable `ImageGraph` directly stores product variants—Device, Driver,
Actor, Renderer, Pool, and DMA Pool—under construction-order indexes and names
([graph shape](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/eval/image.rs#L6-L205)).
Wrela9 should instead seal immutable typed symbolic nodes with `ConstructionId`,
same-root handles, cycle-safe reachability, ownership, legal wiring, and complete
construction-only Resources. Facility planners consume that sealed graph after
evaluation and produce target-neutral plan contributions; they never run as
evaluator callbacks
([accepted sealer boundary](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/design/compiler.md#L134-L140)).

Admission should have a closed structured verdict separating Creator
`Rejected`, compiler `Defect`, cancellation, host inability, and successful
verified `ImagePlan`. Each rejected proof should identify demand, available
capacity/service, the blocking path, and actionable source provenance.

#### Retire

Retire the product-specific `ImageGraph`, mutable `sealed: bool`, raw declaration
indices and names, target selection inside pure Image construction, and one
monolithic `check_sealed` function as the public artifact. Do not allow Facility
planning or generated runtime details to mutate the sealed semantic graph.

### 7. Logical Image Layout and memory accounting

#### Adopt

Wrela8 demonstrates that the compiler can account separately for Actor/Driver
state, Mailboxes, Pool storage, async frames, runtime tables, cross-core rings,
Group arenas, and product buffers. Placement uses deterministic ordering and
reports the component byte contributions
([placement facts](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/placement.rs#L26-L44),
[deterministic placement](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/placement.rs#L98-L227)).
Its physical layout verifies section sizes, ordering, overlaps, Pool windows,
device windows, ring windows, and final linked coverage before publishing bytes
([ImageLayout shape](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/layout.rs#L66-L102),
[layout verification call sites](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/layout.rs#L2548-L2557)).
Adopt checked arithmetic, exact component accounting, deterministic placement,
and independent overlap/coverage verification.

#### Revise

Layer 2's result must be Logical Image Layout: target-neutral bounded
arrangements and quantities for Pools, Mailboxes, async frames, buffers,
Facility state, scheduler structures, regions, and service obligations. It may
use semantic alignment/packing policies defined by the logical schema, but it
must not contain AArch64 addresses, registers, calling convention, ELF sections,
or relocation choices. Target ABI Layout later lowers and verifies that logical
plan without altering its semantic capacity facts
([three layout domains](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/design/compiler.md#L170-L178)).

Every total needs a typed ledger row: owner identity, category, count/bound,
unit size, checked subtotal, provenance, proof receipt, and whether the bytes are
resident, mutually exclusive, or reusable. A scalar `total_bytes` is not enough
to diagnose admission or independently recompute it.

#### Retire

Wrela8 planning performs a preliminary Flow lowering and frame derivation,
generates live runtime Wrela source, rechecks the whole closure, lowers Flow and
MWIR again, and then computes final analyses
([derive pass](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/layout.rs#L3897-L3931),
[generated-source recheck and final lowering](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/layout.rs#L4003-L4119)).
Retire this generated-runtime-source feedback loop. Generated runtime glue must
be authenticated compiler IR or authenticated precompiled runtime Modules
([accepted rule](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/design/compiler.md#L120-L122)).

Retire Wrela8 `ImageLayout` as Layer 2 output: it combines blob bytes, linked
machine program, entry address, native sections, MMIO windows, IRQ injection,
runtime tables, product renderer storage, and semantic Pool placement. Also
retire placement whose `work_source` is literally `unproved`
([reported placement](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/placement.rs#L201-L247)).

### 8. Diagnostics and inspection

#### Adopt

Wrela8 shows the value of naming an error category, primary site, additional
evidence, and explicit arithmetic/edge paths. Whole-Image failures such as a
construction cycle and failed reserve proof already contain useful why-chain
material. Its report also demonstrates that graph, placement, layout, frame,
and cost facts can be emitted from one build.

#### Revise

Wrela8 `SemaError` is primarily a category string, message string, line/column,
and arbitrary `extra_lines`
([error shape](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/mod.rs#L31-L69)).
Layer 2 needs typed diagnostic codes and parameters, half-open byte ranges,
ordered labels, semantic identities, canonical bounded proof paths, recovery or
repair guidance, and explicit classification as Creator rejection or Defect.
Human prose is a renderer, not the evidence schema
([Wrela9 diagnostic evidence](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/design/testing.md#L55-L67)).

Inspection should project stable semantic observations—Core/Flow identity and
fingerprint, ownership/capacity facts, logical ordering, service bounds,
ImagePlan nodes, layout ledger, proof receipts, and accepted/rejected outcome—
without serializing private arenas, indexes, instruction variants, solver state,
or reusable IR files.

#### Retire

Retire full MWIR/FlowWir/CFG/frame/assembly/report snapshots as the default
conformance interface, arbitrary string `extra_lines`, output offsets as
identity, and report parsers as verifier authority. Preserve exact textual
snapshots only when presentation itself is the contract.

### 9. Conformance and Regression Cases

#### Adopt

Preserve the semantic scenarios, not their harness: Actor non-reentrancy and
FIFO, admission rejection without Resource loss, typed Replies, bounded Groups,
deadline inheritance, cancellation cleanup, stale Pool keys, exact Pool
capacity, suspension liveness, and frame-plan validity. Wrela8's
`check-flow-multi-suspend` is a compact source case for a local that must survive
two awaits
([case](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/check-flow-multi-suspend/input.wr#L20-L35)).

Adopt narrow private malformed-artifact tests for Core, Flow, ownership proofs,
ImagePlan, and Logical Image Layout verifiers. Those cases should be the only
ones that bypass the public compile/inspection Seam.

#### Revise

Translate each scenario into the narrowest deterministic oracle:

- Core transformations: evaluator-versus-optimized compiled differential over
  canonical value, alternative/payload, Panic kind/site, and failure order.
- Flow and scheduler: a compact executable host model compared to production
  typed observations for admission, Turn start, suspension, resumption, Reply,
  cancellation, cleanup, and ownership recovery.
- Capacity and planning: source-to-`Plan` cases asserting typed admission or
  rejection evidence and independently recomputed proof/layout ledgers.
- Backend-independent Layer 2: verified artifact properties, not Cranelift IR,
  native offsets, or emitted bytes.
- A representative subset later crosses the QEMU boundary; broad schedules and
  state spaces remain named Challenges whose Findings reduce into Check.

This is the accepted Wrela9 evidence strategy
([semantic evidence](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/design/testing.md#L29-L37),
[planning Seam](https://github.com/rywible/wrela9/blob/9f13393be13693cd58de0bc23bc1789736c4b100/docs/design/testing.md#L69-L78)).

#### Retire

Retire Wrela8's snapshot-heavy golden harness, compiler-reserved test markers,
accepted either/or cross-core observations, direct dependency on internal stage
dumps, and any claim that matching Wrela8 machine layout or AArch64 bytes proves
Wrela9 semantics.

## Required consequences for the Layer 2 specification

The evidence makes the following requirements sharp enough to constrain the
remaining Wayfinder decisions:

1. **Core has one operation-law authority.** Every operation exhaustively states
   types, ownership, ordering, Panic/effects, dataflow, evaluation, lowering,
   inspection, and differential evidence. A pass cannot carry a private
   alternate semantics table.
2. **Core and Flow are immutable verified artifacts, not shared mutable IR.**
   Builders are private. Each completed artifact carries stable semantic
   identities, schema/fingerprint/input receipts, provenance, and an unforgeable
   verified marker.
3. **Ownership is a proof consumed across seams.** Typed HIR establishes source
   ownership, Core makes transfers and cleanup explicit, Flow accounts for
   suspension/cancellation/Reply ownership, and Image planning aggregates
   lifetime/capacity. Each downstream verifier independently checks the
   obligations it consumes.
4. **Capacity proofs are typed ledgers.** Pool, Mailbox, Reply, Group, async-frame,
   scheduler, Facility, and memory bounds need named demand/supply/concurrency
   evidence and canonical provenance. Proof success never changes source types.
5. **Flow owns logical scheduling semantics.** Transport does not define
   admission order or add capacity. Logical send and Turn identities, Reply
   reservation, deadlines, cancellation, cleanup, and service quotas remain
   explicit until resume functions and a scheduler plan are generated.
6. **The sealed construction graph precedes planners.** Evaluation creates typed
   symbolic nodes; a generic verifier seals them; Facility and whole-Image
   planners consume the verified graph without callbacks or graph mutation.
7. **`ImagePlan` is the admission authority.** It aggregates the verified closed
   graph, reachable concrete code family, ownership/capacity proofs, service
   obligations, logical placement, and Logical Image Layout. It records a
   deterministic accepted or rejected outcome with recomputable evidence.
8. **Logical and target layouts never collapse.** Layer 2 reports target-neutral
   quantities and relationships. Layer 3 chooses ABI sizes, addresses,
   relocations, executable sections, and machine representation and proves that
   lowering conforms to the logical plan.
9. **Conformance observes semantics, not IR spelling.** Public cases enter through
   compile/Plan inspection; private cases inject malformed compiler-produced
   artifacts only to test verifiers. Scheduler-model and evaluator differentials
   replace broad internal dumps.

## Newly sharp questions for the map

The historical evidence does not decide these questions, but it makes them
precise enough to ticket after the artifact-graph decision establishes their
owners:

1. **Define the whole-Image proof ledger and admission verdict.** Which closed
   proof kinds contribute to admission, what typed demand/supply/provenance does
   each expose, how are failures canonically prioritized, and what exactly does
   the `ImagePlan` verifier recompute independently?
2. **Define logical capacity and lifetime accounting.** How do Core ownership
   paths and Flow suspension/Reply/Group/cancellation paths combine into maximum
   simultaneous live Pool allocations, Mailbox occupancy, Reply reservations,
   async-frame multiplicity, and cleanup bounds without reducing to Wrela8
   static-site counting?
3. **Define the Layer 2 scheduler-model and observation schema.** What is the
   smallest executable transition model and typed observation vocabulary that
   jointly cover admission, logical commit order, Turns, Replies, Groups,
   deadlines, cancellation, cleanup, service quotas, and ownership recovery?
4. **Define Logical Image Layout's recomputable ledger.** Which target-neutral
   categories, alignment/overlay rules, multiplicities, ownership identities,
   and proof receipts make totals exact and independently verifiable while
   excluding every Target ABI fact?

These should graduate from fog only after **Define the Layer 2 artifact graph
and verifier authority** fixes which Module owns the sealed graph, proof tables,
`ImagePlan`, and Logical Image Layout. No additional historical-research ticket
is required before that decision.
