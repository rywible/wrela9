# Current Version

Status: accepted product and architecture scope. The release-gate shape is fixed; numeric VM, renderer, and performance thresholds are calibrated from controlled prototypes rather than chosen as product policy.

## Outcome

The current Wrela version is complete only when it provides a real compiler, launcher, runtime, selected Facilities, and field renderer that build and boot Images through pinned unmodified QEMU. It must produce one deliberately small but genuinely playable silent flagship game and keep the three permanent Genshin-shaped, Pokémon-shaped, and Yu-Gi-Oh-shaped reference programs passing. The flagship is a compact exploration adventure: one continuous 3D region, an articulated player character, a small set of creatures or mechanisms, persistent collectible and interaction state, and at least one distinct encounter or puzzle state. A compiler-only foundation, disconnected architecture demonstration, objective-free sandbox, or disposable vertical slice does not satisfy this outcome.

## Audience

The supported audience is initially the Wrela system builders themselves. The repository is public, but this version makes no third-party onboarding, installer, source compatibility, polished documentation, publishing, or support promise. Creator-facing designs must still be coherent because the builders use the same language and tools; external release work does not gate the architecture.

## Architecture scope

The current release gates only the AArch64 Architecture Profile used for Apple Silicon development. The accepted later x86-64 profile remains documented so the architecture does not accidentally become AArch64-specific, but its boot path, Drivers, conformance, and performance are not implemented or tested in this version.

The supported development host is Apple Silicon macOS. The compiler, linker integration, QEMU launcher, tests, and editor are gated only there; behavior on another host creates no current support obligation.

## Included Facilities

The implemented Facility set is closed to Display, Input, Event Store, Monotonic Clock, Entropy, and Telemetry. The flagship selects Display, Input, Event Store, Monotonic Clock, and Telemetry. Entropy is exercised by a conformance Image and remains unavailable to replayable gameplay. Audio, Network, Filesystem, USB, native controller input, and runtime content loading are outside the current version; host-side controller mapping may still synthesize configured keyboard and mouse bindings.

Replay is an optional current-version tool capability rather than an Image Facility. The launcher and editor capture and play host-managed artifacts containing admitted Input Samples, explicit gameplay seeds, and their logical boundaries. Replay remains separate from Event Store truth and lossy Telemetry; the flagship and all three reference Images use it for deterministic regression and scripted release gates.

The current Event Store implementation supports both Greenfield and Production modes. Development normally uses Greenfield. A release gate generates the Event Schema Lock, rebuilds the flagship in Production mode, proves every locked schema readable through admitted upcasts, and rejects unsafe schema evolution.

The implemented capability set is closed. A Project that requests Audio, Network, Filesystem, USB, native controller input, runtime content loading, an unavailable Architecture Profile, or another unsupported capability fails during compilation or Image planning with a precise diagnostic. Wrela does not emit an Image containing an inert stub, silent fallback, partial implementation, or experimental escape hatch.

## Creator tools

The current version includes a sophisticated graphical editor for human-and-agent collaboration rather than deferring it until after the flagship. The editor operates on canonical Wrela source and uses the same compiler service as command-line tools. It must support source navigation and construction, direct scene manipulation, interactive Form and View preview, diagnostics, semantic inspection, cost and approximation overlays, and agent-authored changes without introducing a second scene format.

The same repository also provides the coherent developer operations needed to check, plan, build, run, test, and inspect Images. Exact command spelling and editor interaction design remain downstream decisions.

## Distribution

The current version is delivered as a developer checkout for its builders. It produces real `.wrela-image` artifacts but has no signed installer, automatic updater, publishing service, catalog, supported third-party binary distribution, or cross-machine toolchain-reproducibility promise.

QEMU and LLD are ordinary locally installed developer prerequisites. When their roles are needed, Wrela resolves `qemu-system-aarch64` or `ld.lld` through the current process `PATH`, records the resolved path and reported version in evidence, and produces a direct host diagnostic if the command is absent or fails. There is no setup operation, lockfile, executable hash, dynamic-dependency inventory, signature check, macOS-build binding, automatic download, or vendored copy. Selecting, authenticating, and shipping a self-contained toolchain are deferred together until Wrela has an actual distribution product.

The current version manages no Developer ID certificate, notarization, App Sandbox policy, Hypervisor entitlement, signing identity, or package-acceptance suite. Incidental ad-hoc signatures produced by local build tools carry no Wrela product meaning.

## Physical Reference Console

The initial physical performance authority is the builders' Apple M4 MacBook Air with 16 GB RAM running the AArch64 Image through the documented local QEMU version and HVF. Exact virtual CPU count, guest RAM, service budgets, thermal protocol, and rendering cost calibration will be selected from controlled measurements rather than guessed. Supporting an older Apple Silicon floor is a later measured product decision.

## Completion

The current version is complete only when one reproducible release gate passes from a fresh checkout after its documented external prerequisites are installed. That gate covers byte-identical repeated builds from identical inputs, the complete graphical-editor authoring journey including an agent-authored Source Transaction, QEMU execution of the flagship and all three reference Images, Replay, Greenfield and Production Event Store behavior, Facility conformance and failure paths, and calibrated performance on the physical Reference Console.

Passing isolated subsystem tests, booting an architecture demonstration, or presenting a visually impressive but incomplete scene does not substitute for the complete gate. The exact fixtures, measurements, and retained evidence are defined in [Testing](./testing.md) and [Release Gates](./release-gates.md).
