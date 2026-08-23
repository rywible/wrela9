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

A Project may contain `src/image.wr`, `src/test.wr`, or both. The first roots a Deployment Image and the second roots a Test Image; purpose follows from the root rather than a source flag. A Test-only Project is valid, so the standard library and authenticated runtime Modules can have native suites without inventing a Deployment Image. Each Test Root explicitly imports Test Suite Modules and registers the cases they expose. There is no filesystem discovery. The exact suite export form and the treatment of reachable but unregistered Tests remain open Layer 1 decisions.

A Project with `src/test.wr` produces one consolidated Test Image and Check boots that Image once. The Test Image is a real Image with a Test Facility, ordinary compiled Wrela code, a Device Manifest, and the same VM ABI lifecycle as a Deployment Image. Its Test Runtime activates registered cases and emits one complete typed Test Report. Each case has only `Passed` or `ExpectationFailed`; an unexpected error Panics, and Panic or a broken runtime invariant terminates the complete Image rather than forging case results. The report contains no logs, timings, truncation policy, retries, skipped cases, expected-Panic mode, or flaky status.

Each stateful Wrela Test owns a distinct Test Case Graph: a closed build-known subset containing that case's Actors, Mailboxes, Pools, fake-media state, endpoints, and root Group. Cases may share immutable code and Data, semantically invisible scheduler machinery, and the append-only Test Report, but no reachable mutable state or observable Facility endpoint. Test setup remains local to each declaration even when that duplicates construction, keeping a case understandable without fixture indirection. Wrela has no generic fixture, `beforeEach`, reset hook, or ambient Facility lookup. The exact rule for immutable suite-level inputs remains open.

Test Case Graph storage is admitted with the complete Image and exists for the Image lifetime. Its Actors are initialized to their declared state and remain dormant until the runner activates the case root; they are neither dynamically created nor destroyed. A completed case must quiesce its root Group with no pending work, unresolved Replies, leaked ownership, or later Turns. Its graph then becomes unreachable to later cases rather than being observably reset or reclaimed.

Cases are semantically unordered and isolated. The initial Test Runtime executes them serially, but a later compiler may overlay or reinitialize provably nonoverlapping storage or run provably isolated cases concurrently without changing semantics. Authors receive no `parallel` or `serial` annotation. The Test Report is ordered by a canonical Test Identity rather than registration, execution, placement, or completion order; the exact identity derivation remains open.

The exact source spelling of a Test declaration's build-time construction clause remains unsettled. Whatever spelling is selected must keep construction effect-free, local, and distinct from the runtime body while using the ordinary Build Constructor and graph-sealing rules. Suite export and registration likewise remain explicit; the remaining decisions cover construction spelling and evaluation details, suite export and dependency rules, registration completeness, internal host-step expression, and canonical Test Identity—not whether setup becomes runtime allocation or ambient lookup.

## Storage and Driver evidence

The production Event Store remains a zero-or-one Image Facility and the flagship selects exactly one authoritative history. Native storage tests use the same Event Store Runtime behind a private Store Media Adapter. Production selects Virtio-Block; every Test Case Graph may select an independent Memory Media Adapter. This test multiplicity does not relax production Facility cardinality.

Memory Media is a contract-faithful Adapter rather than a permissive fake Driver. It has bounded capacity, committed and uncommitted writes, flush barriers, injected errors, and deterministic power-loss and reopen behavior. It can prove Event Store semantics over the media contract, but it cannot prove physical disk durability. Production-shaped durability, queue ownership, reset, and device behavior require real Driver conformance.

Drivers are substantial authenticated Wrela Modules over thin architecture primitive Adapters. Creator Projects cannot import or construct Driver Modules, request Compiler Primitives, or enter a trusted mode. Driver protocol and state-machine behavior should still use native Wrela Tests wherever possible. Rust Regression Cases are reserved for compiler behavior and the irreducible primitive floor.

The Compiler Distribution owns private Driver Conformance Projects. They are real Wrela Projects compiled through an internal registry that authenticates version-controlled Module identities and their exact primitive grants; local development adds no source modifier, path convention, certificate, signature, or Creator-visible driver-test mode. Check builds one internal conformance Image containing one real instance and one compact case for each shipped Driver, provided the Check Latency contract remains satisfied.

Conformance cases boot real Drivers against controlled QEMU devices and disposable backends. Guest Wrela requests bounded typed conformance steps; the host runner performs the corresponding QMP or backend action and acknowledges it through a private typed channel. The exact internal Wrela expression of that authority remains open. Irreversible VM termination, broad fault matrices, power-loss lifecycles, and deeper per-Driver launches remain Challenges. A modeled device Adapter is added only when a concrete case cannot be expressed cheaply through pure Wrela logic or controlled QEMU; it must earn the additional Seam.

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
