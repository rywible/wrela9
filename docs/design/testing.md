# Testing

Status: accepted project-wide testing and release-evidence strategy. Individual subsystem fixtures and numeric performance thresholds are calibrated implementation specifications.

## Principles

Tests preserve semantic obligations rather than incidental implementation output. Existing Wrela8 golden cases are evidence about language and scheduler behavior, not a harness or file format to port intact.

The important inherited scenarios include non-reentrant suspended Actors, FIFO Mailboxes, rejected-admission Resource recovery, deterministic cancellation cleanup, typed same- and cross-core Replies, complete Reply delivery, and logical cross-core admission. Each scenario moves into the narrowest layer that can state its invariant directly.

Migration inventories each relevant Wrela8 case by semantic obligation and records whether Wrela9 adopts, revises, or retires it. Wrela8 source behavior is evidence rather than a compatibility gate, and its MWIR, AArch64, layout, and incidental textual snapshots are not migrated as requirements.

## Structured conformance

Semantic Images emit typed test observations for events such as admission, Turn start, suspension, resumption, fulfillment, cancellation, cleanup, ownership recovery, and Panic. Tests assert relevant fields and order through stable structured schemas rather than comparing a complete stdout transcript.

A compact pure host-side scheduler model defines Mailbox, Turn, Reply, Group, admission, logical commit, deadline, and cancellation transitions. It is not a second Wrela interpreter. Generated scenarios execute against both the model and production scheduler and compare their structured observations.

Model-based generation and bounded exhaustive exploration cover small Actor graphs, Mailbox capacities, cross-core proposals, suspension, cancellation, deadline expiration, and Resource transfer. Every discovered failure is reduced and retained as a named regression. Host timing stress may supplement these tests but cannot define correctness for a logically deterministic scheduler.

## Test layers

Fast host tests cover parsing, type and effect checking, build evaluation, compiler transformations, cost accounting, the scheduler model, and isolated runtime components.

Compiler test and debug builds run structural verification after individual Wrela-owned transformations; release builds verify major representation seams. Generated pure programs compare the Typed-HIR evaluator with optimized compiled execution. These differential checks cover semantic outcomes rather than requiring stable Core IR, Cranelift IR, or machine bytes.

Representative conformance Images boot through pinned QEMU for every Image Facility, VM ABI seam, Driver protocol, backend, scheduler transition family, and Panic/shutdown path. Release gates run the complete applicable suite on every supported architecture. QEMU is required evidence but not the execution environment for every small semantic case.

The Genshin-shaped, Pokémon-shaped, and Yu-Gi-Oh-shaped references are complete conformance Images. Each boots through QEMU, offers a deterministic scripted mode for release gates, permits limited manual interaction, and supplies typed Preview Fixtures for editor workflows. They use only public Wrela and standard-library interfaces but do not carry independent full-game progression obligations.

Their fixed semantic pressure is distinct even though exact population and performance counts are calibrated later. The Genshin-shaped Image covers continuous terrain, an articulated character, lighting, shadows, transparency, and field effects. The Pokémon-shaped Image covers deterministic regional generation, reusable creature families, an overworld-to-battle transition, and menus. The Yu-Gi-Oh-shaped Image covers dense board layout, exact procedural text, cards, nested Form illustrations, overlap, and dramatic effects. Each supplies fixed scripted camera and state sequences for repeatable release gates.

Snapshots are reserved for output whose exact presentation is the behavior under test: diagnostics, formatted reports, pretty-printing, source rewrites, and editor-visible rendering. Language, ownership, scheduler, and protocol semantics use structured assertions. ABI and Wire Layout tests inspect exact bytes only where representation is contractual.

## Performance

Deterministic tests validate cost-model accounting, admitted work bounds, placement plans, cancellation latency, and generated schedule properties on every host. Controlled Reference Console runners measure native microbenchmarks, scheduler service latency, Driver throughput, and the flagship and three reference programs against calibrated gates. Ordinary developer-machine wall time does not decide semantic or performance conformance.

## Repository evidence

Git contains the durable inputs and expectations needed to reproduce a release judgment: Wrela source, typed Preview Fixtures, canonical Replay scripts, Event Schema Locks, semantic expectations, and calibrated thresholds. Generated Images, Evidence Bundles, traces, profiles, screenshots, and other large or host-specific outputs are build artifacts rather than source-controlled files. They are keyed by Image digest and retained by the release runner when a run must be inspected or compared.

No release assertion depends on an unrecorded manual action or an artifact that cannot be regenerated from the committed inputs and the authenticated local toolchain.
