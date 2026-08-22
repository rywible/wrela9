# Release Gates

Status: accepted current-version completion contract. Numeric thresholds are filled by controlled calibration without changing the shape of the gate.

## Authority

One release gate decides whether the current Wrela version is complete. It begins from a fresh checkout on the Apple Silicon macOS development host after the documented external prerequisites have been installed. Every required result must come from committed inputs and the documented local developer environment; an ad hoc demonstration cannot replace a failed or absent gate.

The physical performance authority is the Apple M4 MacBook Air with 16 GB RAM running the AArch64 Architecture Profile through the documented local QEMU version and HVF. Controlled prototypes select the exact virtual CPU count, guest RAM, service budgets, thermal protocol, rendering budgets, and other numeric thresholds before those values become gate inputs.

## Build identity

Two consecutive builds made from identical Project source, compiler executable, unchanged local external tools, Architecture Profile, build mode, and declared inputs must produce byte-identical `.wrela-image` artifacts. Each build also produces a canonical Evidence Bundle keyed by the Image digest. A missing tool, failed invocation, undeclared input, unresolved symbol, unsupported capability, or noncanonical package blocks the release.

## Semantic and runtime conformance

The complete language, evaluator, Wrela-owned IR verifier, optimizer differential, scheduler model, ownership, capacity, cancellation, Driver, VM ABI, boot, shutdown, and Panic suites must pass. Representative conformance Images boot through the documented local QEMU version and exercise every implemented Image Facility and terminal path, including bounded recovery and deliberately injected failures.

Requests for unsupported Facilities, Architecture Profiles, content mechanisms, or host authority must fail at compilation or Image planning with structured diagnostics. The gate tests these negative cases explicitly; an inert stub or runtime fallback is a defect.

## Editor journey

The graphical editor gate performs one complete source-native authoring journey:

1. Create or extend a reusable Form abstraction in canonical Wrela source.
2. Declare Authoring Parameters with source provenance.
3. Manipulate an instance through the graphical scene tools.
4. Run a typed Preview Fixture through the isolated pure Preview Worker.
5. Inspect bounds, approximation choices, and dominant costs.
6. Receive and review an external agent-authored Source Transaction.
7. Build the complete Image.
8. Launch it under QEMU and verify the resulting behavior.

The journey must prove revision-aware conflicts, semantic diff, undo, and invalidation after an incompatible external source edit. Preview success cannot substitute for an AOT Image run.

## Flagship journey

The flagship runs as a silent third-person exploration adventure at the fixed 1280×720, 60 Hz Display contract. Automated Replay and bounded manual checks cover New, Continue, Reset, movement, camera, target lock, light attack, dodge, damage, defeat, checkpoint respawn, all progression milestones, victory, durable completion, save-status UI, pause, Facility-loss UI, and the ending.

After every persistent milestone, terminating and relaunching Continue must reconstruct the authoritative state from the Event Store. Victory becomes authoritative and visibly final only after its Event transaction has been durably acknowledged. Reset appends a new Campaign Epoch rather than deleting the host store.

The gate runs both Event Store lifecycles. Greenfield mode may archive incompatible development history and begin cleanly. Production mode generates and validates the committed Event Schema Lock, proves every released payload readable through admitted upcasts, verifies retry deduplication, and rejects unsafe evolution.

## Reference Images

The three permanent reference Images boot through QEMU, execute their canonical Replay scripts, and supply typed Preview Fixtures:

- The Genshin-shaped Image stresses continuous terrain, articulation, lighting, shadows, transparency, and field effects.
- The Pokémon-shaped Image stresses deterministic regional generation, reusable creature families, overworld-to-battle transition, and menus.
- The Yu-Gi-Oh-shaped Image stresses dense board layout, exact procedural text, cards, nested Form illustrations, overlap, and dramatic effects.

They use only public Wrela and standard-library mechanisms. They are conformance programs rather than independent games, but a shortcut or private compiler hook introduced only to make one pass is a gate failure.

## Replay and performance

The flagship and all three reference Images replay admitted Input Samples, explicit gameplay seeds, and logical boundaries deterministically. Event Store truth and lossy Telemetry remain outside the Replay artifact. Repeated runs must agree on authoritative structured observations even when host timing differs.

Controlled Reference Console runs measure the complete Images and the supporting native microbenchmarks against calibrated admission and real-time thresholds. The flagship and references must sustain their admitted presentation contracts without relying on a host GPU for field evaluation, lighting, or compositing. A missed complete frame retains the previous frame and records the environmental performance fault, but a release fails when its calibrated scenario exceeds the permitted fault budget.

## Retention

Git retains Wrela source, typed Preview Fixtures, canonical Replay scripts, Event Schema Locks, semantic expectations, and calibrated thresholds. Generated Images, Evidence Bundles, traces, profiles, screenshots, and other large or host-specific outputs remain reproducible release artifacts keyed by Image digest. The runner retains enough failed-run evidence to diagnose a gate without turning incidental output into a source compatibility promise.
