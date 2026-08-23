# Image Facilities

Status: accepted Facility architecture through shared failure policy, Input ownership, Monotonic Clock and Cadence, Entropy, Telemetry, Creator observations, and Event Store host binding. Audio is outside the current version; individual included Driver state machines and numeric bounds remain implementation specifications.

## Failure policy

Every Image Facility owns a typed bounded recovery and reset protocol for the Driver Errors it can contain. A Facility that cannot restore its contract reports a typed outcome to a build-wired supervisor Actor. The Image Constructor must provide the required policy for loss of every selected Facility; neither the runtime nor the Facility silently continues, invents a requester, or automatically labels a contained environmental failure as Panic.

An uncontainable device state, broken ownership invariant, or violated compiler/runtime assumption remains Panic. Recovery attempts, reset counts, resource restoration, reporting, and escalation all fit compiler-admitted bounds.

The flagship Console supplies fail-closed defaults for its required Facilities. Permanent loss of Display, Input, Monotonic Clock, or Event Store requests controlled Shutdown with a typed `FacilityLost` reason after bounded quiescence. Telemetry loss disables further observations and gameplay continues. Entropy loss follows the explicit policy of the selecting Image. Recoverable per-operation outcomes remain locally handleable before a Facility declares permanent loss.

## Deferred Facilities

The current Wrela version has no Audio Facility, Audio authoring model, sound content contract, Audio Driver, or compatibility obligation for future sound support. Audio is deferred indefinitely until its product and authoring domain can be designed from informed experience. Future Audio work begins as a fresh design branch and does not inherit the shelved exploration as accepted terminology or architecture.

## Input

An Image wires the Input Facility to exactly one owning Actor. That Actor pulls a bounded typed `Input Sample` at chosen logical boundaries and translates it into the Image's own semantic messages; the flagship requests one Sample per gameplay Cadence occurrence. A Sample contains current control state, pressed and released transitions accumulated since the preceding accepted Sample, bounded pointer and wheel movement, and explicit focus state. Replay records the admitted Sample rather than device event or interrupt timing.

The guest-visible vocabulary is a finite Wrela-defined set of normalized physical keyboard keys, mouse buttons, wheel movement, pointer movement, and focus state. An ordinary standard-library action mapper turns those controls into game-specific typed actions. Text entry is outside the current version. Controller-to-keyboard-and-mouse mapping remains a host concern and does not expand the guest-visible Input contract. The Facility does not dynamically choose recipients, broadcast raw host events, or expose macOS, QEMU, or Virtio codes.

An Action Map is a compile-time declaration of fixed bindings that derives typed held, pressed, released, and bounded axis values from an Input Sample. It supports finite alternative bindings and chords and diagnoses ambiguous mappings. Runtime rebinding and persistence of binding configuration are outside the current version.

Focus loss produces one admitted Input Sample that marks focus lost, releases every control, and clears movement. Later unfocused Samples remain neutral. The owning Actor, not the launcher or runtime, decides that this means an explicit paused gameplay state; QEMU vCPUs never freeze implicitly. Focus regain cannot synthesize held controls without new admitted input.

## Monotonic Clock

The Monotonic Clock Facility normalizes a sealed architecture counter into typed `Duration` values. Creator code never observes native counter registers, counter frequency, host timestamps, a wall date, or a calendar. Realtime observations cannot influence Replay state unless they are captured as admitted Replay input.

Cadence is not an Image Facility, Console intrinsic, or language construct. It is an ordinary safe standard-library Wrela Module built over Monotonic Clock and scheduler deadline operations. An Image Constructor may instantiate zero, one, or several Cadence Actors with compile-time frequencies and build-known destinations; the flagship Console conventionally uses one for gameplay. Images without recurring logical work have no Update concept.

Because Cadence uses ordinary Actors, messages, and deadlines, its state, Mailbox, placement, frequency, and cost participate in normal whole-Image analysis. It receives no hidden authority and cannot introduce dynamic Actor destinations or an unbounded loop.

Every numbered Cadence occurrence is delivered exactly once, with at most one occurrence outstanding from a given Cadence Actor. When the Image cannot keep pace, logical progression slows relative to wall time and the runtime records a performance fault; Cadence neither skips occurrences nor performs an unbounded catch-up burst. Several Cadence Actors remain ordinary Actors whose simultaneous work is ordered by the deterministic scheduler rather than by a separate clock priority rule.

## Entropy

The initial Entropy Facility is backed by a bounded modern Virtio-RNG protocol shared across Architecture Profiles. CPU-specific random instructions are not source semantics. Gameplay Actors cannot hold its Capability; replayable randomness begins from an explicit seed captured with Replay, while selected non-replayable Facilities may request bounded genuine entropy.

Device-provided lengths, completion state, and control values enter the Driver as Untrusted. Entropy bytes themselves are nondeterministic payload rather than tainted control values.

## Telemetry and Creator observations

Telemetry uses a dedicated generic `virtserialport` with queues independent from mandatory terminal control. Creator and Facility observations enter a bounded lossy guest ring. Host backpressure never delays gameplay; overflow increments a stable dropped-record counter that appears in later observations and build/runtime evidence.

Wrela has no semantic stdout, stderr, `printf`, or Creator-visible console stream. Creator logging is structured Telemetry, and compiler/runtime diagnostics are structured records. Launchers, inspectors, tests, and future editor tools decide how to render those records for humans.

Every Telemetry record has a build-known typed schema and fixed maximum encoded size. The Image carries only compact diagnostic identities and bounded values; its Evidence Bundle relates those identities to Project declarations, field schemas, and human-facing names. Runtime formatting, open-ended strings, and dynamically invented record shapes are outside the guest contract.

## Event Store binding

A Deployment Image admits zero or one Event Store Facility; the flagship admits exactly one. This is a current product and domain rule: one production Store is the Image's authoritative ordered history, Store Identity, and schema lifecycle. It is not an inference that one disk can technically host only one database.

The Event Store Runtime owns storage semantics above a private Store Media Interface. The production Adapter implements that Interface through the selected Virtio-Block Driver. A Test Case Graph may instead construct its own bounded Memory Media Adapter and an independent instance of the same Event Store Runtime. Multiple memory-backed test instances inside one Test Image do not create multiple production Event Store Facilities or expose Store Media to Creators.

Memory Media models committed and uncommitted writes, flush barriers, finite geometry, injected failures, and deterministic power loss and reopen. It proves Event Store behavior at the Store Media Seam, not real physical durability or Virtio-Block conformance. Those obligations use the production Adapter and Driver Conformance Image described in the testing design.

The launcher binds one Store Identity to one exclusive host-managed fixed-size block image and exposes it only as the Event Store Facility's admitted Virtio-Block device. Creator source cannot name a host path, block device, mount, or volume. The launcher validates expected geometry, Store Identity, and schema lifecycle before the Image becomes running.

Only one live Image may hold the binding for a Store Identity at a time. Compatible production Images reopen the same binding through the Store Identity rather than their Image digest. Greenfield storage follows its separate replace-and-archive lifecycle.

The Image Constructor declares Event Store capacity in semantic terms. The whole-Image planner derives and reports the exact block-image geometry, including all storage-engine metadata and safety headroom, rather than asking the Creator to size incidental blocks. If the requested Store Identity is already exclusively bound, launch terminates immediately with the typed `StoreInUse` Boot Failure before the Image runs.
