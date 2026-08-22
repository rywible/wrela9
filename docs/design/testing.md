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
