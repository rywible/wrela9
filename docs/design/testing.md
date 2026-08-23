# Testing and Evidence

Status: accepted project-wide evidence strategy. One fast Check determines whether a revision is Releasable; bounded Challenges discover missing Release Obligations and Regression Cases.

## Principles

Tests preserve semantic obligations rather than incidental implementation output. Existing Wrela8 golden cases are evidence about language and scheduler behavior, not a harness or file format to port intact.

The important inherited scenarios include non-reentrant suspended Actors, FIFO Mailboxes, rejected-admission Resource recovery, deterministic cancellation cleanup, typed same- and cross-core Replies, complete Reply delivery, and logical cross-core admission. Each scenario moves into the narrowest layer that can state its invariant directly. Wrela8 source behavior is evidence rather than a compatibility requirement, and its MWIR, AArch64 implementation, layout, and incidental textual snapshots do not become Wrela9 contracts.

Every accepted observable property that would block release is a Release Obligation. Every Release Obligation has deterministic evidence in Check. Evidence may be optimized, consolidated, or moved to a deeper verifier, but an obligation is never silently discarded to preserve Check Latency. Subjective product and design judgment may reject a change without becoming a second technical release tier.

## Check

Check is one command with one evidence set and no fast, full, CI, release, changed-file, or exhaustive mode. It returns:

- `Passed`: every currently encoded Release Obligation holds, so the revision is Releasable.
- `Failed`: at least one Release Obligation is violated.
- `Unable`: prerequisites or Check machinery prevented a judgment.

Only `Passed` permits a merge or release. An `Unable` result is never converted into success by a silent retry. Check may use correct content-addressed and incremental caches, but caches cannot select different cases or change the claim. Installing prerequisites and initially building the host toolchain are setup cost rather than Check Latency.

On the Reference Development Host, representative warm local changes target a Check Latency below one second and have a hard two-second ceiling. Check measures its own latency. Crossing the ceiling fails on the reference host and warns on other hosts. When evidence growth threatens the envelope, cases are consolidated behind stronger invariants, deeper verifiers, or cheaper Regression Cases rather than moved into a slower routine tier.

Check incrementally builds changed host code, checks formatting and static rules that encode Release Obligations, runs every Regression Case, takes a minimal Image through the real compiler and packaging pipeline, and boots one tiny Image under QEMU to a deterministic typed sentinel before exiting. The minimal Image is built twice from identical inputs to protect canonical byte identity, and its Evidence Bundle remains keyed by the resulting Image digest. That end-to-end case must fit within the same latency envelope. Unsupported Facilities, Architecture Profiles, authority, and content mechanisms have narrow negative Regression Cases proving that they fail explicitly rather than producing inert stubs or fallbacks.

Every Check case is deterministic. There is no retry policy, accepted flake rate, quarantine, or second-attempt pass. A nondeterministic case is a defect in Check and must be repaired, narrowed, or removed without losing its Release Obligation. Each failure names the obligation and the narrow case that observed it; Check does not produce dashboards or historical administrative reports.

## Semantic evidence

Semantic Images emit typed observations for events such as admission, Turn start, suspension, resumption, fulfillment, cancellation, cleanup, ownership recovery, and Panic. Cases assert relevant fields and order through stable structured schemas rather than complete stdout transcripts.

A compact pure host-side scheduler model defines Mailbox, Turn, Reply, Group, admission, logical commit, deadline, and cancellation transitions. It is not a second Wrela interpreter. Generated scenarios execute against both the model and production scheduler and compare their structured observations. Compiler transformation verification and Typed-HIR evaluator differentials similarly protect semantic outcomes without requiring stable Wrela-owned IR, Cranelift IR, or machine bytes.

Bounded generated cases may live in Check when they are fixed, deterministic, inexpensive, and protect a concrete Release Obligation. Open-ended fuzzing, model exploration, timing stress, and large state-space searches are Challenges. A useful discovery is minimized before it becomes a permanent Regression Case.

Broad golden-file testing is not a strategy. Explicit assertions and typed structured observations are preferred. A snapshot is appropriate only when its exact presentation or serialized bytes are themselves the contract, such as a user-facing diagnostic, formatted report, source rewrite, Wire Layout, or ABI record.

## Native Wrela tests

Wrela behavior should be protected in Wrela whenever the behavior can be expressed through the language and its authenticated standard/runtime Modules. A Wrela Test is a dedicated language declaration, not an ordinary callable function. It executes compiled guest code; the Typed-HIR evaluator may supply differential evidence for eligible pure behavior but can never substitute for that execution.

A Project may contain `src/image.wr`, `src/test.wr`, or both. The first roots a Deployment Image and the second roots a Test Image; purpose follows from the root rather than a source flag. A Test-only Project is valid, so the standard library and authenticated runtime Modules can have native Suites without inventing a Deployment Image. There is no filesystem discovery.

A `suite` is a source declaration that owns nested `test` and `async test` declarations. It has no parameters and creates no runtime class, value, fixture, or isolation scope. Tests use ordinary Wrela parameter modes. Their bodies may construct ordinary runtime Data and Resources, but only the Test Image Constructor and its ordinary native Wrela helper call chain may invoke Build Constructors that create Actors, Facilities, Pools, Mailboxes, or graph wiring.

The Test Image Constructor builds one ordinary closed Image graph and supplies one explicit ordered `cases` list to the Test Facility. Each entry is a Test Application: a build-known binding of one nested Test to values from that graph. The call-like spelling records a statically dispatched invocation for runtime and does not execute the Test during Image construction. Every Wrela Test in the selected Test root's reachable Module closure must be applied exactly once; missing and duplicate applications are compilation errors, while a completely unimported Module remains outside the Image.

The Test Image has exactly the same Actor, Resource, Image Facility, cardinality, Device Manifest, boot, shutdown, and graph-sealing rules as a Deployment Image. Wrela adds no Test Case Graph, automatic isolation, reset, rollback, fixture lifecycle, dormant-case activation, or special mutable-reachability restriction. Distinct state exists only when the Image Constructor creates distinct ordinary state. Sharing an Actor, Facility endpoint, or other mutable authority between Tests is legal and observable.

The Test Runtime executes Test Applications serially in exact `cases` order, which is semantic and also determines Test Report order. A later compiler may not reorder or parallelize them. Each Test runs in an ordinary root Group and becomes `Passed` only after its body returns and that Group quiesces. A false `expect` records structured failure and execution continues through the body and later Tests; one or more mismatches produce `ExpectationFailed`. State changes are never rolled back. Unexpected errors Panic, and Panic or a broken runtime invariant terminates the complete Image rather than fabricating a partial report.

A Project with `src/test.wr` produces one consolidated Test Image and Check boots that Image once. The complete typed Test Report contains no logs, timings, truncation policy, retries, skipped cases, expected-Panic mode, or flaky status. There is no filter, alternate order, case selection, `beforeEach`, generic fixture, or ambient Facility lookup. Setup may be factored through ordinary native Wrela helpers called by the Image Constructor; runtime scenario logic remains in the Test declaration.

## Layer 1 Regression Case Seam contract

Consumer-level compiler Regression Cases cross the same `Compiler::compile` Seam as the CLI, inspector, and graphical editor. The compiler does not expose phases merely to test them. Narrow private Regression Cases are justified only for defensive states that credible source cannot produce or for containment mechanics whose Interface is intentionally private: forced Identity Catalog collisions, malformed compiler-produced verified artifacts, evaluator tariff and exhaustion machinery, and the parser's low-level recovery invariants.

Focused cases assert structured observations rather than complete phase dumps. A diagnostic's canonical evidence is its code, primary half-open byte range, ordered labeled ranges, typed parameters, recovery action, and relevant semantic identities. A focused rejected request asserts the exact canonically ordered diagnostic set. Human wording is a host-rendered presentation and receives only a small representative snapshot corpus; changing wording never changes the underlying diagnostic identity.

Parser Regression Cases protect exact source-byte round trips, token and node kinds and ranges, trivia ownership, invalid and zero-width missing nodes, structured diagnostic and recovery actions, structurally valid declaration islands that remain eligible for semantic lowering, and the per-file diagnostic truncation rule. They do not snapshot a printed private tree or parser-library representation.

Resolution and type cases enter through source and assert `Accepted` or `Rejected`, exact root diagnostics, related provenance and identities, canonical ordering, and locally valid observations available before the hard no-errors gate. Invalid syntax nodes, placeholder types, and error values never cross into semantic artifacts merely to enable further testing.

Evaluator evidence through the compile Seam covers canonical completed Data, returned alternative and payload, Panic kind and site, eligibility rejection, deterministic limit outcomes and contributor summaries, and agreement between eligible pure evaluation and compiled execution. Private evaluator cases cover tariff accounting, containment, and impossible internal states without making evaluator stacks or arenas contractual.

Determinism Regression Cases repeat representative requests on one Compiler instance and after reopening it, vary candidate-file enumeration order, and vary `InspectSelection`. These changes must not alter the base outcome or canonical artifacts. Check does not mechanically repeat every case; later Build evidence separately constructs the minimal Image twice from identical inputs.

| Behavior family | Observing Interface | Canonical evidence | Narrow private exception |
|---|---|---|---|
| Lossless syntax and recovery | `Compiler::compile` plus requested syntax inspection | exact bytes, structured tree observations, diagnostics, recovery, valid islands | recovery mechanics that cannot be induced precisely through source |
| Project closure, resolution, and typing | `Compiler::compile` | outcome, structured diagnostics, identities, locally valid observations | none by default |
| Identity derivation | `Compiler::compile` plus identity inspection | typed identities, fingerprints, provenance, canonical ordering | forced digest collision and interner invariants |
| Pure evaluation | `Compiler::compile` plus evaluation inspection | canonical outcome and receipt; compiled differential | tariffs, containment, impossible evaluator state |
| Image construction and planning | `Compiler::compile` with `Plan` intent | structured rejection or sealed plan observations | malformed compiler-produced graph or verifier artifact |
| Native Wrela behavior | compiled Test Image through the VM ABI | complete typed Test Report in registration order | none; evaluator execution cannot substitute |

The Wrela8 inventory remains translation provenance, not a permanent Check dimension. Every adopted or revised behavior becomes an ordinary Wrela9 Release Obligation and Regression Case; retired behavior receives no compatibility case. Check has no Wrela8 mode, harness, label, or live migration ledger.

Layer 1 is specified for implementation when each behavior family names its observing Interface, canonical result, justified private exception, and representative Regression Case shape. This concise Seam contract plus the accepted design documents is the exit evidence; an exhaustive speculative fixture catalog is not required before implementation.

## Storage and Driver evidence

The Event Store remains a zero-or-one Image Facility in every Image, including a Test Image, and the flagship selects exactly one authoritative history. Native storage tests use the same Event Store Runtime behind the private Store Media Interface. A Test Image may select a bounded Memory Media Adapter for its one Store; it receives no multiplicity exception. Several Tests may deliberately form one ordered scenario over that Store. A scenario requiring a completely fresh Store belongs in another Test Project/Image or in a bounded Challenge.

Memory Media is a contract-faithful Adapter rather than a permissive fake Driver. It has bounded capacity, committed and uncommitted writes, flush barriers, injected errors, and deterministic power-loss and reopen behavior. It can prove Event Store semantics over the media contract, but it cannot prove physical disk durability. Production-shaped durability, queue ownership, reset, and device behavior require real Driver conformance.

Drivers are substantial authenticated Wrela Modules over thin architecture primitive Adapters. Creator Projects cannot import or construct Driver Modules, request Compiler Primitives, or enter a trusted mode. Driver protocol and state-machine behavior should still use native Wrela Tests wherever possible. Rust Regression Cases are reserved for compiler behavior and the irreducible primitive floor.

The Compiler Distribution owns private Driver Conformance Projects. They are real Wrela Projects compiled through an internal registry that authenticates version-controlled Module identities and their exact primitive grants; local development adds no source modifier, path convention, certificate, signature, or Creator-visible driver-test mode. Check builds one internal conformance Image containing one real instance and one compact case for each shipped Driver, provided the Check Latency contract remains satisfied.

Conformance cases boot real Drivers against controlled QEMU devices and disposable backends. The internal Test Image receives a private non-Creator-importable Driver Conformance Capability. Guest Wrela submits one of its bounded typed step requests; the host runner performs the corresponding QMP or backend action and returns a bounded typed acknowledgement through the private VM channel. The Capability and its request and acknowledgement types are authenticated Compiler Distribution Modules, not language syntax or ambient authority. Irreversible VM termination, broad fault matrices, power-loss lifecycles, and deeper per-Driver launches remain Challenges. A modeled device Adapter is added only when a concrete case cannot be expressed cheaply through pure Wrela logic or controlled QEMU; it must earn the additional Seam.

## Challenges and Findings

A Challenge is a named exploratory activity that answers a stated investigative question. It may exercise realistic Images, broad workflows, fuzzers, state spaces, physical timings, profiles, or failure injection, but it cannot make a revision Releasable and cannot grant additional release credit after Check passes.

Challenges never run from Check, CI, merge hooks, release scripts, or routine agent instructions. There is no `challenge all`, automatic enumeration, or default Challenge. Each Challenge targets about thirty seconds and must terminate within sixty seconds on the Reference Development Host. During ordinary work, all Challenge invocations share a sixty-second aggregate budget unless the user explicitly authorizes more.

A reproducible Challenge observation becomes a Finding only when it violates an accepted Release Obligation. Interesting profiles, slower measurements, unusual behavior, and speculative concerns are not automatically Findings. An accepted Finding follows one path:

1. Isolate the smallest input and observation that credibly preserve the defect.
2. Add that failing Regression Case to Check.
3. Fix the defect.
4. Retain the case while its Release Obligation remains valid.

An unreduced reproducer may enter Check temporarily when necessary to block the next merge, but reduction is immediate work. Large traces, corpora, profiles, screenshots, and reproductions are discarded unless independently useful. A Regression Case may be removed or replaced when its obligation is retired, a cheaper case subsumes it, or a stronger invariant makes the behavior impossible.

The flagship, graphical-editor journey, and Genshin-shaped, Pokémon-shaped, and Yu-Gi-Oh-shaped reference Images are permanent high-value Challenges rather than routine Check suites. Their deterministic Replay scripts and typed Preview Fixtures make them effective investigative instruments. Their Findings become narrow Check evidence; their full executions do not become release ceremonies.

## Performance evidence

Benchmarks, profiles, native microbenchmarks, complete-frame runs, and realistic Image measurements are Performance Challenges. A benchmark defines its workload, environment, metrics, and protocol but has no pass status by itself. There is no coverage target or benchmark score that independently grants release status.

Check ordinarily protects Compilation Performance, Generated-Code Performance, and Image Performance through deterministic evidence: logical work counters, allocation bounds, cost-model invariants, admitted plans, IR properties, vectorization decisions, reduced kernels, cancellation bounds, and stable micro-thresholds. Physical Performance Challenges calibrate and attack those proxies when a concrete investigation calls for it; no calendar cadence is required. Check's own physical latency is the sole routine elapsed-time threshold.

The Reference Console remains the authority for Image Performance claims. The distinct Reference Development Host is the authority for Compilation Performance and Check Latency. The current host profile records the builders' Apple Silicon model and memory class without freezing irrelevant macOS patch details.

## Repository evidence

Git retains the durable inputs and expectations needed by Check and useful Challenges: Wrela source, typed Preview Fixtures, Replay scripts, Event Schema Locks, semantic expectations, and deterministic thresholds. Generated Images, Evidence Bundles, traces, profiles, screenshots, and other large or host-specific outputs are disposable artifacts keyed by Image digest rather than permanent proof ledgers.
