# Graphical Editor

Status: accepted through product boundary, procedural manipulation, human-agent transactions, preview execution and isolation, workspace state, Source Transaction conflicts and undo, QEMU lifecycle integration, launch storage modes, typed preview fixtures, pure preview JIT, the external agent boundary, Project extensibility, and the release-gating flagship workflow.

## Product boundary

The current version includes a scene-first integrated graphical environment. It combines Project navigation, Wrela source view, diagnostics, Form, World, and View construction, direct manipulation, semantic inspectors, interactive preview, cost and approximation overlays, semantic diffs, and agent activity. It is sophisticated around Wrela's distinctive authoring model but is not required to reproduce every feature of a mature general-purpose IDE, debugger, profiler, or text editor.

Wrela source remains the only canonical Project representation. The editor consumes the same compiler service and lossless syntax artifacts as command-line tools and never writes a private scene graph, generated-source shadow tree, or alternative serialization.

## Direct manipulation

The compiler exposes declared Authoring Parameters with exact source provenance and structured operations. A direct manipulation is available only when it maps unambiguously to such a declaration. Procedurally generated results remain selectable and inspectable, but the editor neither decompiles them into explicit structures nor stores a hidden override. Creators expose meaningful editable controls deliberately when defining procedural abstractions.

Projects may expose typed Authoring Parameters and compose the standard manipulation operations available to their types. They cannot load host editor plugins or execute Project code inside the editor process. New primitive manipulation behavior comes from authenticated compiler or standard-library modules rather than a dynamic Project extension seam.

## Human-agent collaboration

Humans and agents use the same compiler-described Source Transactions. Each transaction produces an exact source patch, semantic diff, diagnostics, preview impact, and undo record before it becomes accepted Project state. Agents may operate under review-required or trusted auto-apply policy, but accepted changes are ordinary Wrela source and repository diffs. UI automation and private agent scene formats are not the collaboration contract; direct external source edits remain interoperable rather than becoming forbidden.

Agents may inspect, preview, check, build, test, and launch QEMU Images without source-mutation approval. Source Transactions require review by default. Trusted auto-apply is an explicit per-session editor choice and cannot become durable Project policy or silently carry into another session.

A Source Transaction names the source revision and stable semantic identities it inspected. The compiler may rebase it only when every intended target still resolves uniquely and retains compatible meaning. Otherwise it returns a structured conflict and the operation must be recomputed; textual resemblance and last-writer-wins behavior are insufficient.

Local undo and redo apply inverse Source Transactions under the same revision, identity, and rebase rules. An external source change invalidates only history entries that can no longer rebase safely. Git remains durable Project history; editor operations do not create an automatic commit stream.

Interactive manipulation uses disposable preview values while a gesture is active. Completing the gesture proposes one atomic Source Transaction; cancellation restores the unchanged source revision, and a value that fails checking cannot commit. Pointer-motion samples never become independent source edits or undo entries.

The compiler and editor expose a provider-neutral local semantic protocol for external host agents. It supports Project queries, inspection, proposed Source Transactions, preview, build, run, and test operations while the editor presents activity and review state. Model credentials, provider SDKs, conversation execution, and agent runtimes remain outside Wrela source and Images. The current tools may adapt this protocol to the builders' chosen agent environment without making that provider part of the language.

## Preview execution

The editor directly previews only effect-free constants, Image-construction fragments, Forms, Worlds, Views, Materials, and explicit bounded test snapshots through compiler-owned presentation paths. Checked World and Transport plans may lower sealed Preview Kernels through Cranelift JIT for interactive execution. Those kernels receive copied Preview Fixture Data and have no Actors, Facilities, Capabilities, host calls, or semantic authority. The Typed-HIR evaluator and scalar presentation semantics remain their differential oracle. Effectful gameplay and complete Image behavior remain AOT-only and authoritative only under pinned QEMU.

Field evaluation, lighting, and Transport remain CPU-executed through the same planned kernels and contracts used by the Console. The host GPU may composite editor chrome and display completed pixels, but it cannot evaluate Fields, shade Materials, execute Transport, or become an alternate renderer.

JIT Preview Kernels execute in a replaceable Preview Worker process rather than the editor address space. A compiler or generated-code defect may terminate that worker and return bounded structured failure evidence without destroying the editor session. This boundary uses ordinary local process isolation and does not introduce a signed sandbox, entitlement policy, Project plugin host, or semantic recovery promise for faulty kernels.

Projects declare typed effect-free Preview Fixtures in Wrela using each View's real snapshot types. They are canonical checked source. Temporary editor adjustments remain disposable workspace state until deliberately accepted through a Source Transaction; JSON fixtures and captures from a running Image are not required for ordinary preview.

## Workspace state

Selections, panel arrangements, temporary cameras, expanded nodes, local undo history, and other editor-only state live in disposable per-user workspace storage outside the Project and source repository. Anything with shared semantic meaning, including a named camera, Authoring Parameter, presentation layout, or test snapshot, belongs in canonical Wrela source. The editor has no private authoritative scene metadata.

## Running Images

The editor may build, launch, stop, observe, and inspect pinned QEMU Images and attach their structured Telemetry and Evidence. It does not hot-patch a running Image. Every accepted source change produces a new Image, and retaining durable state across launch is an explicit `Continue` against a compatible Event Store rather than an editor mutation of guest state.

Each editor launch configuration visibly selects `New`, `Continue`, or `Ephemeral`. `New` refuses to overwrite an existing Store binding, `Continue` requires compatible Store Identity and schema, and `Ephemeral` uses disposable storage. A per-user workspace may remember the choice, but neither the editor nor launcher infers persistence from an Image digest.

## Release-gating workflow

The editor must let a Creator create a Form abstraction, expose Authoring Parameters, instantiate and directly manipulate it, construct a typed Preview Fixture, inspect bounds and cost, review and accept an agent-proposed Source Transaction, build the complete Image, run it under QEMU, and inspect structured runtime evidence. This complete journey gates the current version; isolated feature demonstrations do not.
