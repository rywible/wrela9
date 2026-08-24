/Users/ryanwible/.zshenv:.:13: no such file or directory: /Users/ryanwible/.vite-plus/env
/Users/ryanwible/.zshenv:.:13: no such file or directory: /Users/ryanwible/.vite-plus/env
# Wrela8 semantic inventory for Layer 1

Status: resolved research input for the Layer 1 Wayfinder map. This note is
descriptive, not a compatibility promise or a language specification.

## Question

Which Wrela8 language, evaluator, scheduler-adjacent, and compiler tests express
semantic obligations that Wrela9 should adopt, revise, or retire?

## Sources and method

The primary evidence is Wrela8 source and tests at commit
[`40d1d9dff38c6c1dde527a9873108bfaeb8c775d`](https://github.com/rywible/wrela8/tree/40d1d9dff38c6c1dde527a9873108bfaeb8c775d).
The corpus contains 852 golden-case directories. This inventory classifies
public behavior by obligation rather than treating every expected file as an
independent contract. A test is cited when its source states the behavior;
implementation is cited when the obligation is distributed across the corpus
or when it exposes coupling that a fixture alone does not show.

The decisions are evaluated against Wrela9's accepted
[Language Core](../design/language-core.md),
[Compiler](../design/compiler.md), and [Testing](../design/testing.md) designs.
In this note:

- **Adopt** means preserve the public semantic obligation, while freely
  rewriting implementation, diagnostics, spelling, and test harness.
- **Revise** means preserve the motivating scenario but change the public
  contract to the already accepted Wrela9 design.
- **Retire** means the Wrela8 behavior is not a Wrela9 obligation. It may remain
  useful as implementation archaeology or as a negative migration case.

## Answer

Wrela9 should adopt Wrela8's language shape and its strongest safety cases:
significant indentation, explicit visibility, algebraic data and exhaustive
matching, checked numerics, explicit `read`/`mut`/`take`, definite
initialization, path-sensitive Resource ownership, pure bounded build
evaluation, statically wired Actors, non-reentrant Turns, bounded FIFO
Mailboxes, typed awaited results, static async frames, deterministic cleanup,
and validation at representation seams.

It should revise rather than port Wrela8's module closure, generic identity,
Pool identity, loop and recursion rules, compile-time interpreter, Image graph,
admission and Reply protocol, cancellation and deadline outcomes, cross-core
ordering, and scheduler service. Wrela8's implementation encodes many of these
through string identities, per-module body copies, host recursion, fail-fast
`CallError.NotAdmitted`, transport-local capacity, and AArch64 layout facts that
contradict accepted Wrela9 decisions.

It should retire the Wrela8 product and backend surface: `Target`/machine-v1,
Creator-visible device assembly, public MMIO/DMA/IRQ vocabulary, `Pixels`,
generated runtime source, MWIR and FlowWir textual forms, custom AArch64 codegen,
register allocation and relaxation, physical layout snapshots, guest stdout
transcripts, and the golden harness itself.

## Adopted obligations

### Source language and local semantics

| Public obligation | Wrela8 evidence | Layer 1 disposition |
| --- | --- | --- |
| Significant indentation, delimiter-aware layout, comments, doc comments, and source spans are part of the language shape. | The lexer emits `Newline`, `Indent`, and `Dedent` and tracks byte spans ([lexer.rs L1-L27](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/syntax/lexer.rs#L1-L27)); layout islands are explicit ([lexer.rs L95-L121](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/syntax/lexer.rs#L95-L121)); `lex-layout-island` exercises nested indentation inside a closure and call ([input.wr L1-L15](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/lex-layout-island/input.wr#L1-L15)). | Adopt the language shape. Rebuild parsing around lossless syntax; token dump text is not contractual. |
| Text, byte, character, numeric, and interpolated literals have checked lexical forms and escapes. | Wrela8 distinguishes `Str`, `FStr`, `BStr`, and `Char` tokens ([lexer.rs L1-L16](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/syntax/lexer.rs#L1-L16)); `lex-escapes` covers Unicode scalar, byte, and control escapes ([input.wr L1-L6](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/lex-escapes/input.wr#L1-L6)). | Adopt checked literal decoding and the Text/Bytes distinction. Exact token categories and old `Str`/`String` names are not binding. |
| Structures, enums, fixed arrays, tuples, `Option`, `Result`, functions, methods, visibility, and explicit parameter modes compose as ordinary typed language features. | `check-decls` combines Data, Resource, methods, generics, arrays, `Option`, `Result`, and `own[P] T` ([input.wr L3-L40](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/check-decls/input.wr#L3-L40)); the semantic type vocabulary is explicit ([types.rs L13-L51](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/types.rs#L13-L51)). | Adopt the concepts and representative positive/negative cases. Source spelling can change without compatibility. |
| Expression operands and call arguments evaluate exactly once, left to right, with access modes activated in source order. | Wrela8 specifies the sequencing rule and its interaction with `read`, `mut`, and `take` arguments ([02-language.md L549-L566](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/docs/language/02-language.md#L549-L566)); flow checking models ordered access activation and overlap ([flow.rs L279-L350](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/flow.rs#L279-L350)). | Adopt as both evaluator and compiled-execution behavior. Add focused structured tests rather than inheriting pass dumps. |
| Pattern matching must reject non-exhaustive and unreachable arms, and alternatives must bind consistently. | `check-match` covers enum, tuple, boolean, guarded, alternative, wildcard, and `is` patterns ([input.wr L8-L75](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/check-match/input.wr#L8-L75)); the checker reports unreachable rows and the first missing witness ([matches.rs L212-L245](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/matches.rs#L212-L245)). | Adopt as semantic tests with structured diagnostics. |
| Integer arithmetic is checked by default; wrapping operations are explicit; division, shifts, indexing, and narrowing reject invalid values. | The evaluator separates ordinary checked operations from wrapping operations ([value.rs L162-L200](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/eval/value.rs#L162-L200)) and checks division and shift failure ([value.rs L216-L283](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/eval/value.rs#L216-L283)); `err-comptime-overflow` makes overflow observable during evaluation ([input.wr L1-L5](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/err-comptime-overflow/input.wr#L1-L5)). | Adopt and share the cases between the Typed-HIR evaluator and compiled execution. Wrela9's specified float mode supersedes host-float behavior. |
| External control values must be narrowed before they can index, allocate, or dispatch. | `check-untrusted-narrow-index` checks a bound before indexing ([input.wr L1-L6](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/check-untrusted-narrow-index/input.wr#L1-L6)); `err-untrusted-index` rejects the raw value ([input.wr L1-L5](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/err-untrusted-index/input.wr#L1-L5)). | Adopt the `Untrusted` boundary obligation. Device-specific constructors and diagnostics are not binding. |

### Ownership, Resources, and Pools

| Public obligation | Wrela8 evidence | Layer 1 disposition |
| --- | --- | --- |
| Data copies, Resources move only through explicit `take`, and a moved place is unreadable until restored. | `err-move-use-after-take` reads a Resource after moving it ([input.wr L3-L11](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/err-move-use-after-take/input.wr#L3-L11)); flow analysis tracks `Uninit`, `Init`, and `Moved` and merges them across paths ([flow.rs L17-L73](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/flow.rs#L17-L73)). | Adopt. Port cases, not the analysis representation. |
| A `mut` place borrowed by a call must be restored on every exit; overlapping mutable or mutable/read access is rejected. | Exit checking requires full restoration ([flow.rs L99-L122](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/flow.rs#L99-L122)); `err-overlap-two-muts` passes one place twice as `mut` ([input.wr L7-L11](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/err-overlap-two-muts/input.wr#L7-L11)); generic field replacement restores the borrowed aggregate ([check-x-generic-flow-restore L6-L15](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/check-x-generic-flow-restore/input.wr#L6-L15)). | Adopt. Wrela9 narrows borrow lifetimes further by forbidding storage, return, messaging, and suspension. |
| Protocol Resources cannot disappear on a recoverable path; they must be consumed, returned, transferred, or protected by deterministic cleanup. | The checker recursively finds must-consume Resources inside aggregates and rejects live values at exit ([flow.rs L124-L223](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/flow.rs#L124-L223)); `err-manual-resource-dropped` leaves one live ([input.wr L4-L12](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/err-manual-resource-dropped/input.wr#L4-L12)). | Adopt. Wrela9 distinguishes compiler-reclaimable Resources from protocol Resources and excludes Panic from cleanup. |
| `defer` participates in ownership analysis and cleanup observes reverse registration order. | `check-defer-exits` demonstrates restoration before each exit ([input.wr L12-L36](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/check-defer-exits/input.wr#L12-L36)); `boot-cancel-cleanup` expects cleanup trace `21`, never `12` ([input.wr L25-L44](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-cancel-cleanup/input.wr#L25-L44)). | Adopt the ownership and reverse-order obligations. Revise the runtime case to Wrela9's typed Group cancellation and structured observations. |
| Pool keys must not alias across Pools or after slot reuse; stale lookup returns absence. | `check-slotmap-key-discipline` covers foreign Pool identity, stale generations, reinsertion, and exhaustion ([source L6-L103](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/check-slotmap-key-discipline/src/examples/check_slotmap_key_discipline.wr#L6-L103)). | Adopt foreign/stale-miss behavior. Revise `Key` into unforgeable `Key[P,T]`; do not preserve public construction of `map_id`, index, or generation. |

### Pure evaluation and closed construction

| Public obligation | Wrela8 evidence | Layer 1 disposition |
| --- | --- | --- |
| Constants, compile-time branches, assertions, pure tests, and Image construction use ordinary typed Wrela values and functions. | `check-const-eval` calls an ordinary function from a constant ([input.wr L3-L15](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/check-const-eval/input.wr#L3-L15)); `check-comptime-if-stmt` selects ordinary statements ([input.wr L3-L10](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/check-comptime-if-stmt/input.wr#L3-L10)); the evaluator exposes constants, tests, layout assertions, and Image evaluation through the same interpreter ([interp.rs L99-L237](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/eval/interp.rs#L99-L237)). | Adopt one authoritative pure evaluator over Typed HIR. |
| Evaluation is deterministic and bounded; exhaustion is a build failure, not an ambient timeout. | Wrela8 has explicit step, memory, call-depth, and exhaustive-test quotas ([quota.rs L1-L36](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/eval/quota.rs#L1-L36)); `err-comptime-quota` intentionally exhausts the step budget ([input.wr L3-L10](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/err-comptime-quota/input.wr#L3-L10)). | Adopt deterministic fuel and memory limits. Numeric Wrela8 constants are not binding. |
| Compile-time code cannot observe runtime clock, entropy, hardware, or other ambient authority. | Legality is propagated through the call graph with an explanatory call path ([legal.rs L27-L125](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/eval/legal.rs#L27-L125)); `err-now-in-comptime` reaches `now()` transitively ([input.wr L3-L9](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/err-now-in-comptime/input.wr#L3-L9)). | Adopt and extend to all ambient host state listed by Wrela9. |
| One reachable Image Constructor yields a closed, validated system declaration before native lowering. | Wrela8's evaluator requires the `@image` function to seal its graph ([interp.rs L208-L236](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/eval/interp.rs#L208-L236)); graph validation checks the construction DAG, bindings, initialization, failure policy, placement, and renderer declarations ([image_checks.rs L78-L105](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/eval/image_checks.rs#L78-L105)). | Adopt closure and validation. Revise the result from product-specific `ImageGraph` to generic target-neutral `ImagePlan` plus authenticated Facility planners. |

### Actors and scheduler-adjacent behavior

| Public obligation | Wrela8 evidence | Layer 1 disposition |
| --- | --- | --- |
| Actor destinations are statically wired; ordinary messages cannot carry Actor handles or mutable loans, and Resource arguments move explicitly. | The message checker rejects `mut`, closures, Actor handles, and non-`take` Resource arguments ([actor.rs L94-L183](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/actor.rs#L94-L183)); `err-actor-handle-in-message` demonstrates the forbidden dynamic destination transfer ([input.wr L10-L26](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/err-actor-handle-in-message/input.wr#L10-L26)). | Adopt and strengthen to Wrela9's Image-wired named-handle rule. |
| A suspended Actor Turn is non-reentrant and accepted messages remain FIFO. | `boot-actors` admits `slow`, `quick`, and `report`, then requires the trace `123`, never `132` ([input.wr L59-L86](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-actors/input.wr#L59-L86)). | Adopt the scenario as a scheduler-model and QEMU conformance case. Remove compiler-reserved test markers and transcript coupling. |
| Async suspension has explicit state and statically known frame storage; values crossing suspension are checked. | `check-flow-multi-suspend` keeps a local across two awaits ([input.wr L20-L35](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/check-flow-multi-suspend/input.wr#L20-L35)); FlowWir represents frame slots, states, operations, and explicit transitions ([flowwir.rs L8-L116](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/flowwir.rs#L8-L116)). | Adopt the obligation. Replace FlowWir with Wrela9 Flow IR; old temp numbers and dumps are not contracts. |
| Typed same-core and cross-core awaited calls preserve scalar, aggregate, and `Result` payloads completely. | `boot-actor-reply-result` checks sync and async `Result` replies, aggregate fields, operational errors, and a nested awaited call ([input.wr L141-L198](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-actor-reply-result/input.wr#L141-L198)); `boot-cross-core-call` checks the same typed call across fixed core placement ([input.wr L14-L29](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-cross-core-call/input.wr#L14-L29)). | Adopt payload-completeness scenarios. Revise to explicit one-shot Reply Resources and `ReplyClosed` ownership recovery. |
| Mailbox pressure never silently drops a message or loses moved Resources. | `boot-cross-core-ring-full` requires one admitted and one explicit rejection at capacity one ([input.wr L15-L32](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-cross-core-ring-full/input.wr#L15-L32)); `boot-await-rejected` requires the moved argument back on failed admission ([input.wr L31-L49](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-await-rejected/input.wr#L31-L49)). | Adopt no-drop and ownership-recovery invariants. Revise the operation and outcome split as described below. |
| Concurrent cross-core proposals commit as complete messages and one destination observes sequential effects; pressure does not tear, duplicate, or lose admitted work. | Wrela8's `boot-cross-core-two-senders` checks complete sequential effects on one Sink ([input.wr L41-L64](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-cross-core-two-senders/input.wr#L41-L64)); `boot-cross-core-mailbox-depth` preserves both admitted effects through a one-slot destination ([input.wr L47-L85](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-cross-core-mailbox-depth/input.wr#L47-L85)). | Adopt atomicity, no-loss, and sequential-observation scenarios. Wrela9 strengthens their order separately; do not preserve Wrela8 host-arrival freedom. |
| Groups have bounded child activation, join results, parent deadline propagation, cooperative cancellation, and deterministic cleanup. | `boot-group-join` creates two bounded children and joins both results ([input.wr L27-L47](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-group-join/input.wr#L27-L47)); `boot-deadline-inherit` expects an outer deadline to cancel the inner child at a checkpoint ([input.wr L28-L49](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-deadline-inherit/input.wr#L28-L49)). | Adopt the scenarios. Revise deadline authority, outcome types, checkpoint latency, child policies, and scheduler service to Wrela9. |

### Compiler properties

| Public obligation | Wrela8 evidence | Layer 1 disposition |
| --- | --- | --- |
| Explicit module imports respect visibility and resolve a complete deterministic source closure. | The loader anchors declared module paths to files, confines symlinks to the source root, and walks imports in a `BTreeMap` closure ([loader.rs L39-L138](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/loader.rs#L39-L138), [loader.rs L227-L299](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/loader.rs#L227-L299)); imports reject private names and collisions ([imports.rs L124-L207](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/imports.rs#L124-L207)). | Adopt deterministic closure, explicit imports/exports, path confinement, and privacy. Revise cycles and identities. |
| Structural generics are monomorphized and checked after substitution; compile-time Data can participate. | `check-generics` covers explicit and inferred type arguments and fixed-size generic aggregates ([input.wr L12-L33](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/check-generics/input.wr#L12-L33)); Wrela8 substitutes types and const expressions into cloned syntax ([generics.rs L54-L100](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/generics.rs#L54-L100)). | Adopt structural monomorphization. Replace syntax cloning and string keys with closure-wide `DefId`/`InstanceId`; disallow Pool and Facility identities as generic arguments. |
| Whole-closure analyses may use the closed Image to prove bounded operations, but a proof must not silently change ordinary source types. | Wrela8's send proof computes capacities by evaluating the Image and rejects unprovable repeated/static sites ([send_proof.rs L190-L310](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/send_proof.rs#L190-L310)). | Adopt whole-Image proof inputs and diagnostics. Revise the operations to explicit `allocate`/`reserve` versus `try_allocate`, and proof-required send protocols versus `try_send`; do not port the static-site-count algorithm as policy. |
| Important intermediate and final artifacts are structurally verified before serialization, and phase observations can explain a build. | Wrela8 validates section order, overlap, ownership, relocation bounds, and exact function coverage before serialization ([linked.rs L168-L337](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/linked.rs#L168-L337)); its report records compiler, inputs, quotas, graph declarations, and edges ([report.rs L96-L157](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/report.rs#L96-L157)). | Adopt verification at every Wrela9 representation seam and structured phase inspection. Replace Wrela8 physical artifacts with Typed HIR, Core, Flow, World/Transport, `ImagePlan`, Cranelift inputs, ELF validation, and Evidence Bundles. |

## Revised obligations

These Wrela8 tests describe a useful pressure case but assert a contract that
Wrela9 has already changed.

| Wrela8 behavior | Evidence | Wrela9 revision |
| --- | --- | --- |
| Identifiers and source structure are ASCII-only, while string contents may be Unicode. | The lexer explicitly rejects a non-ASCII source byte outside a literal ([lexer.rs L218-L239](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/syntax/lexer.rs#L218-L239)). | Preserve Unicode Text, but Layer 1 still needs an explicit identifier policy. Do not accidentally inherit ASCII-only identifiers merely by porting the lexer. |
| `String[..N]` is UTF-8 bytes with direct integer indexing. | `check-string-bound` treats `"hi"` as a bounded String and indexes bytes `0` and `1` ([input.wr L4-L21](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/check-string-bound/input.wr#L4-L21)). | Wrela9 has `Text` and `Bytes`; Text is not arbitrarily integer-indexed. Preserve capacity failure cases, but route byte indexing to Bytes and specify scalar/grapheme Text APIs. |
| Every Data type receives generated equality, and generic ordering accepts floats. | `check-operators` compares aggregate Data containing floats ([input.wr L34-L40](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/check-operators/input.wr#L34-L40)); `check-generic-order-f64` instantiates a generic ordered operation with `f64` ([input.wr L4-L19](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/check-generic-order-f64/input.wr#L4-L19)). | Require explicit or derived `Eq`, `Order`, and `Hash` structural requirements. Ordinary floats do not satisfy total equality, ordering, or map-key contracts; use the accepted explicit float modes and wrappers instead. |
| Import cycles are accepted, including cycles used during compile-time evaluation. | `import-cycle-accept` has `a -> b -> a` ([a.wr L1-L9](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/import-cycle-accept/src/a.wr#L1-L9), [b.wr L1-L9](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/import-cycle-accept/src/b.wr#L1-L9)). | Wrela9 modules form an acyclic import graph. Convert the old positive cases into cycle diagnostics; retain transitive reachability and recursive-by-value type rejection as separate cases. |
| Runtime loops use Creator-supplied `@budget(bound=N)` guards, and all runtime recursion is rejected. | `check-budget-sync-loop` manually annotates both `while` and `for` ([input.wr L4-L18](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/check-budget-sync-loop/input.wr#L4-L18)); `err-recursion-direct` rejects visibly decreasing recursion ([input.wr L4-L11](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/err-recursion-direct/input.wr#L4-L11)); the send proof's diagnostic recommends rewriting every cycle as a budgeted loop ([send_proof.rs L165-L187](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/send_proof.rs#L165-L187)). | Derive loop maxima from bounded ranges/collections and admit recursion only with a compiler-recognized decreasing measure. Retire `@budget` as a semantic trust assertion. Keep early exit and exhaustion tests against the new proof system. |
| Compile-time evaluation is host-recursive and protected by a 256 MiB host stack plus a recursion counter. | Wrela8 spawns a guarded-stack thread ([interp.rs L36-L49](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/eval/interp.rs#L36-L49)) and retains a host call stack limit ([interp.rs L75-L96](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/eval/interp.rs#L75-L96)). | Execute Typed HIR with explicit control, call, and value stacks; charge frames to deterministic evaluator memory. Preserve results and diagnostics classes, not host-stack behavior. |
| Image construction returns a closed `ImageGraph` whose variants are Device, Driver, Actor, Renderer, Pool, and DMA Pool and whose scalar `target` selects machine-v1. | The enum and graph fields are product-specific ([image.rs L6-L25](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/eval/image.rs#L6-L25), [image.rs L85-L116](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/eval/image.rs#L85-L116)). | Image Constructor selects high-level Facilities and yields a generic target-neutral `ImagePlan`; Architecture Profile is a build input, Drivers are authenticated, and Display replaces Renderer/Pixels declarations. |
| Actor state rooted at `self` may be read again after suspension, while an equivalent non-Actor parameter path is rejected. | `check-await-self-path` reads `self.cache.value` on both sides of `await` ([input.wr L13-L27](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/check-await-self-path/input.wr#L13-L27)); `err-await-external-path` rejects use of an input aggregate after suspension ([input.wr L13-L24](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/err-await-external-path/input.wr#L13-L24)). | Wrela9 permits Actor state to persist but forbids retaining any `read` or `mut` borrow across suspension. Reframe these tests around re-borrowing stable Actor state after resume; reject any loan live through the suspension edge regardless of root. |
| Pool identity and generic identity are source strings; slot-map keys expose constructible numeric identity fields. | Generic keys concatenate rendered names and arguments ([generics.rs L16-L39](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/generics.rs#L16-L39)); the slot-map test constructs `Key(map_id=..., index=..., generation=...)` directly ([source L6-L18](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/check-slotmap-key-discipline/src/examples/check_slotmap_key_discipline.wr#L6-L18)). | Resolve closure-wide stable identities before typing/lowering. Make Pool identity generative and `Key[P,T]` unforgeable. Preserve foreign/stale lookup semantics only. |
| Whole-Image proofs can change an intrinsic's source type: a proven `VirtQueue.reserve` returns a permit where an unproven call returns `Result`. | The intrinsic classifier selects `QueuePermit` or `Result[QueuePermit, QueueFull]` from proof state ([intrinsics.rs L155-L170](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/intrinsics.rs#L155-L170)). | Keep proof-required and ordinary fallible operations as distinct source operations. Optimizations may remove checks after proving them, but proof success must not alter source-level types or control flow. |
| Message Resources are restricted to `own[P] T`; other Resource-bearing messages are rejected as an implementation limitation. | The Actor checker accepts the Pool-owned form after `take`, then emits an explicit not-implemented error for other Resources ([actor.rs L166-L181](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/actor.rs#L166-L181)). | Generalize the move/no-loss rule to all admitted Resource-bearing messages. Resource shape is not a reason to reject a message; admission, Reply, cancellation, and cleanup must preserve ownership. |
| `send` and awaited calls fail immediately with `CallError.NotAdmitted`, which embeds moved arguments. | `CallError` variants include `NotAdmitted` ([actor.rs L84-L91](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/actor.rs#L84-L91)); `boot-await-mailbox-full` requires an awaited call into a full Mailbox to fail immediately ([input.wr L15-L37](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-await-mailbox-full/input.wr#L15-L37)). | Split operations: `try_send` resolves current deterministic arbitration with `Full`; `send` waits under a Group/deadline and is cancellable before admission. Explicit Reply Resources reserve their return path; `ReplyClosed` returns undelivered ownership after waiter cancellation. The no-loss scenarios stay, but the old `CallError` sum does not. |
| Mailbox proof is a count of static send sites and cross-core transport can act as an observable ring capacity. | Wrela8 rejects loop, child, or multi-caller sites and compares site count with the smallest mailbox declaration ([send_proof.rs L259-L340](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/sema/send_proof.rs#L259-L340)); `boot-cross-core-ring-full` names the transport ring as the rejecting object ([input.wr L31-L32](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-cross-core-ring-full/input.wr#L31-L32)). | One global logical Mailbox capacity includes resident, cross-core admitted, and reserved messages. Transport holds proposals only. Derive proof-required capacity from Flow, Replies, deadlines, and service plans; ordinary pressure keeps its explicit fallible type. |
| Cross-core admission may follow host arrival; a conformance test explicitly accepts either intermediate result. | `boot-cross-core-admission-order` accepts the Near Actor observing either `1` or `11` after concurrent work from two cores ([input.wr L43-L68](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-cross-core-admission-order/input.wr#L43-L68)). | Require one logical total order from stable sender Actor, Turn, and send ordinals, independent of host timing. Convert the old either/or assertion into one exact scheduler-model and QEMU observation. |
| Deadline cancellation is observed at convenient await checkpoints and tests bind the exact number of effects before that checkpoint. | The deadline case expects one effect before cancellation at the first post-deadline checkpoint ([boot-deadline-cancel L28-L46](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-deadline-cancel/input.wr#L28-L46)). | Preserve cooperative cancellation and bounded checkpoints, but use compiler-planned cyclic service, maximum observation latency, logical versus realtime deadline authority, and distinct `Cancelled`, `DeadlineExceeded`, and `DeadlineUnmeetable` outcomes. Old effect counts are scenario inputs, not expected results. |
| Private functions may synthesize structural union error sets from their callees. | `check-inferred-error-set` infers the union of `IoFault` and `ParseFault` through `?` ([input.wr L8-L28](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/check-inferred-error-set/input.wr#L8-L28)), while the public form is rejected ([expected/check.txt L1](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/err-inferred-at-pub/expected/check.txt#L1)). | Preserve nominal `Result` and `?`, but do not inherit inferred union error sets as settled policy. Decide the Wrela9 error-inference surface explicitly before porting these cases. |
| Automatic reclaim and `defer` run during fatal "abandonment" as well as recoverable exits. | The Wrela8 language contract includes abandonment in both automatic Resource reclaim and `defer` execution ([02-language.md L144-L153](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/docs/language/02-language.md#L144-L153), [02-language.md L808-L835](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/docs/language/02-language.md#L808-L835)). | Wrela9 Panic ends the Image without source cleanup or a terminal acknowledgement. Keep cleanup obligations only for recoverable return, `?`, cancellation, Reply closure, and orderly Shutdown; Panic tests assert fail-stop behavior instead. |
| `@layout(wire|runtime|dma|mmio)` is one public attribute family and Creator source can name physical layouts and MMIO capabilities. | `check-layout-wire` specifies byte order and offsets ([input.wr L4-L10](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/check-layout-wire/input.wr#L4-L10)); `check-layout-runtime` declares scheduler storage ([input.wr L4-L18](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/check-layout-runtime/input.wr#L4-L18)); `check-capabilities` names MMIO register structure in ordinary source ([input.wr L4-L18](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/check-capabilities/input.wr#L4-L18)). | Keep exact Wire Layout as a public portable schema contract. Move Logical Image Layout to compiler planning and Target ABI Layout to authenticated backend/runtime modules. Creator source cannot inspect addresses, native size/alignment, MMIO, DMA, IRQ, or raw device queues. Driver protocol-layout cases belong to authenticated-module conformance, not Creator language compatibility. |

## Retired obligations

| Wrela8 surface or test family | Why it is retired |
| --- | --- |
| Exact Wrela8 source acceptance, diagnostic wording, error categories, line rendering, and old keyword set. | Wrela9 explicitly makes no source or Image compatibility promise. Preserve concepts and diagnostic provenance, not legacy acceptance or strings. |
| Positive import-cycle cases (`import-cycle-accept`, `check-import-type-cycle`, `check-import-comptime-cycle`). | Wrela9's Project module graph is acyclic. These become negative cycle tests or are decomposed into acyclic reachability and recursive-type tests. |
| Creator-facing `Target`, machine-v1, `Image.device`, `Image.driver`, DMA Pool, IRQ, MMIO, VirtQueue, Receipt, ISR, wake, driver-mode, and raw block tests. | The new vocabulary is Image, Architecture Profile, VM ABI, Image Facility, authenticated Driver, Capability, and Compiler Primitive. Creator concurrency and devices stay behind Facilities. Hardware protocol scenarios migrate later to authenticated Driver/VM ABI suites. |
| `Failure.Halt`, `img.on_failure`, implicit guest-test completion, and runtime console output. | Wrela9 has generated boot, typed Image Result, explicit Shutdown Capability, fail-stop Panic, Telemetry, and structured test observations; it has no Creator-visible stdout/stderr. |
| Wrela8 `Pixels` syntax, `Renderer`, `Field`/Material markers, renderer configuration, Pixels packet intrinsics, and the entire `check-pixels-*`, `err-pixels-*`, and `boot-pixels-*` public contract. | Wrela9's Creator surface is Space, Form, World, View, Material, and Transport; Field is compiler-owned and Display is a Facility. Wrela8 visual cases remain research inputs for later World/Transport acceptance, not Layer 1 obligations or source compatibility. |
| Manual `@budget`, priority/task annotations, exact Wrela8 `CallError`, and transport-ring admission terms. | Accepted Wrela9 boundedness, scheduling, admission, deadline, and Reply semantics replace them. |
| Public construction or numeric inspection of capability tokens, Actor IDs, Pool IDs, key generations, physical addresses, native offsets, and target sizes. | Wrela9 uses unforgeable typed identities and hides Target ABI and machine representation from Creator code. |
| Exact MWIR, FlowWir, CFG, frame, SROA, placement, cost, relaxation, AArch64 instruction, and linked-byte golden files. | Wrela9 owns new semantic Core and Flow IRs but uses Cranelift and LLD below them. Structural and semantic observations replace old representation snapshots. |
| The `tests/golden/<case>/{input.wr,expected/*}` harness, whole stdout boot transcripts, signed-HVF test path, and fixed test census. | Wrela9 tests semantics through structured observations, models, differential execution, and QEMU conformance. Snapshots remain only where exact presentation is intentional. |

## Incidental coupling that must not cross the migration boundary

### MWIR, AArch64, and optimizer coupling

Wrela8's public-looking dumps are implementation products. `MwirProgram` uses
string-keyed functions and carries Pixels-specific packet operations directly
([mwir.rs L16-L24](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/mwir.rs#L16-L24),
[mwir.rs L96-L124](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/mwir.rs#L96-L124)).
The compiler module list includes its own lowering, optimization, register
allocation, encoder, AArch64 code generator, and relaxation stack
([lib.rs L1-L38](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/lib.rs#L1-L38)).
Code generation also uses thread-local mutable switches and counters
([codegen.rs L11-L35](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/codegen.rs#L11-L35)).

The obligation to preserve is narrower: Wrela-owned IRs must make ownership,
Panic, bounds, effects, suspension, Replies, Groups, cleanup, and scheduler
facts explicit and verifiable. No MWIR instruction, temp number, block ID,
register, encoded word, relocation width, or optimization dump is a Wrela9
contract.

### Physical layout and runtime-harness coupling

The old layout object combines emitted bytes, linked code, runtime tables,
Pools, device registers, block queues, IRQ injection, core entries, renderer
workspaces, framebuffers, and stage-1 mappings in one structure
([layout.rs L87-L129](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/layout.rs#L87-L129)).
Its scheduler bookkeeping sizes, Reply slots, ring addresses, boot test harness,
and AArch64 stubs are imported through the same module seam
([layout.rs L23-L61](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/layout.rs#L23-L61)).

Wrela9 must separate Wire Layout, target-neutral Logical Image Layout, and
Target ABI Layout. Only exact Wire bytes and the final admitted VM ABI/ELF
container are representation contracts. Old offsets and memory-map assertions
are test data for archaeological comparison, not expected output.

### Generated-source and orchestration coupling

The loader pretty-prints source to discover implicit time use
([loader.rs L301-L313](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/loader.rs#L301-L313))
and parses generated `__image_runtime` and `__image_pixels` Wrela modules back
into the ordinary source closure
([loader.rs L354-L392](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/loader.rs#L354-L392),
[loader.rs L420-L465](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/loader.rs#L420-L465)).
The CLI owns closure selection and directly sequences evaluator, image checks,
Pixels compilation, layout, report, and backend stages
([wrela.rs L20-L39](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/bin/wrela.rs#L20-L39),
[wrela.rs L316-L336](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/bin/wrela.rs#L316-L336)).

Wrela9 should preserve one authoritative pipeline and inspectable phases, while
representing generated glue as authenticated compiler IR or authenticated
runtime objects. Semantic discovery must use resolved identities and typed
facts rather than re-rendered source text. CLI, editor, inspector, and tests all
call the same compiler service.

### Golden and host coupling

The old harness selects a root marker or `input.wr`, compares stage outputs,
builds a macOS/AArch64 Hypervisor.framework VMM, applies ad-hoc code signing,
and asserts an exact count of HVF tests
([golden.rs L69-L111](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/xtask/src/golden.rs#L69-L111),
[golden.rs L175-L218](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/xtask/src/golden.rs#L175-L218)).
One representative semantic fixture also imports compiler-reserved lane markers
and asserts through a runtime transcript
([boot-actors L1-L7](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/tests/golden/boot-actors/input.wr#L1-L7)).

The semantic scenarios survive, but these seams do not. Wrela9 uses small host
semantic tests, a scheduler model, generated differential cases, structured
test observations, and representative QEMU Images. Text snapshots are reserved
for deliberate presentation such as diagnostics, reports, pretty-printing, and
source rewrites.

## Contradictions and terminology drift against Wrela9

1. **ADR 0005 is stale on two actor details.** It names "fail-fast admission"
   and "deterministic round-robin scheduling per core" as preserved semantics.
   The accepted Language Core instead splits waiting `send` from fail-fast
   `try_send`, and ADR 0029 explicitly replaces Wrela8 drain-first round-robin
   with compiler-planned cyclic service. ADR 0005 should be amended or
   superseded so future implementers do not port Wrela8 admission or fairness.
2. **`Target` and machine-v1 are retired terms.** Use Architecture Profile for
   native-code selection and VM ABI for the Image/VMM interface. The Image
   Constructor does not select a product-specific target enum.
3. **`ImageGraph` is too product-specific.** Its Device/Driver/Renderer variants
   conflict with the generic `ImagePlan`, Image Facility, Device Manifest, and
   authenticated-planner boundaries.
4. **`Pixels` and `Renderer` are retired Creator terms.** Use World/View/
   Material/Transport for authoring and Display/Scanout for execution. Field is
   compiler-owned.
5. **`String` and `Str` drift from `Text`.** Wrela8's integer-indexed bounded
   UTF-8 value cannot be adopted as Text. Bytes retains byte indexing.
6. **`CallError.NotAdmitted` conflates admission and reply.** Wrela9 uses
   `try_send`/`Full`, waiting `send`, one-shot Reply, and `ReplyClosed` with
   explicit ownership recovery.
7. **Wrela8 `@layout` kinds conflate three domains.** Only Wire Layout is a
   portable Creator-visible byte contract. Logical Image and Target ABI layout
   are compiler/backend products.
8. **Import-cycle behavior contradicts the accepted module model.** Wrela8's
   positive cycle cases must not silently return through a reused loader.
9. **`@budget` is not proof.** Wrela8 accepts a stated iteration cap guarded at
   runtime; Wrela9 requires a compiler-derived maximum. The old annotation and
   diagnostics should not define the new boundedness surface.
10. **Failure terminology changed.** `Failure.Halt`, guest test completion, and
    runtime console transcripts do not map to Panic, `BootFailed`, requested
    Shutdown, `ShutdownFailed`, power loss, or structured Telemetry/test
    observations.

## Decisions exposed by the inventory

Only three Layer 1 policy decisions remain genuinely exposed; the other
differences are already resolved by accepted Wrela9 documents.

1. **Identifier character policy.** Wrela8 accepts ASCII identifiers only and
   rejects non-ASCII source structure, while Wrela9 specifies Unicode Text but
   does not yet say whether identifiers are ASCII, Unicode XID, or another
   closed profile. The lossless lexer specification must choose explicitly.
2. **Legacy surface case selection.** ADR 0017 preserves the successful *shape*
   of Wrela8 without compatibility. The first Layer 1 spec should name the
   exact initial syntax subset (especially closures, f-strings, deriving,
   interfaces, and compile-time declarations) rather than interpreting all 852
   old directories as release scope.
3. **Private error inference.** Wrela9 accepts nominal `Result` and explicit
   error values but does not yet say whether a private function may infer a
   closed union of callee errors. Decide whether errors are always explicitly
   named, locally inferred behind a nominal public boundary, or inferred in
   some narrower form before porting Wrela8's inferred-error cases.

The actor admission/fairness wording is not a new product decision; accepted
Language Core and ADR 0029 already resolve it. It is a documentation repair to
ADR 0005.

## Migration rule for implementation tickets

When a Layer 1 ticket draws from Wrela8, it should cite an obligation in this
inventory and create the narrowest Wrela9 test that owns it:

- lexer/parser behavior in lossless syntax tests;
- type, ownership, and evaluation behavior in structured host tests;
- pure evaluator/backend agreement in differential tests;
- scheduler transitions in the host model and bounded generated scenarios;
- VM-observable Actor behavior in QEMU conformance Images;
- exact bytes only for Wire Layout and admitted ABI/container seams;
- snapshots only for intentionally presented diagnostics, reports,
  pretty-printing, and source rewrites.

No implementation ticket should copy an `expected/mwir.txt`, `asm.txt`, layout
offset, whole boot stdout transcript, Wrela8 `ImageGraph` dump, or generated
runtime source merely because a corresponding golden directory exists.

