# Compiler

Status: accepted compiler architecture through execution, specialization, optimization, linking, diagnostics, reproducibility, and pure editor-preview JIT. Detailed IR instruction sets, individual pass designs, and build-tool packaging are implementation specifications rather than unresolved product policy.

## Compiler boundary

The host-native Rust compiler exposes one deep Module over a deterministic batch pipeline of immutable phase artifacts with explicit inputs. The command-line builder, tests, inspector, and graphical editor use that same Module instead of assembling independent compiler pipelines.

Its external interface has only two operations: `Compiler::open(CompilerInstallation)` seals one authenticated immutable Compiler Distribution, while `Compiler::compile(CompilationRequest, Cancellation)` runs one authoritative compilation. A Compiler instance owns no current Project, mutable workspace, semantic session, query engine, dependency invalidator, or persistent cache. The first implementation may serialize concurrent calls, but request order, thread count, prior cancellation, and private reuse cannot change canonical outcomes.

Every request carries a complete immutable Project snapshot containing every regular `.wr` file beneath the conventional `src/` directory as exact bytes under a canonical Project-relative path, together with recognized declared immutable content, revision identity, and a verified snapshot digest. Filesystem and editor-document adapters construct that snapshot before the compiler seam; the compiler never rereads ambient files during a request. The adapter rejects symlinks along recognized source paths and canonical path aliases, while absolute paths, timestamps, inode numbers, permissions, directory order, Git state, ignored files, environment variables, and unrecognized content never enter compiler semantics. `Analyze` is target-neutral and stops after source semantics. `Plan` carries an Architecture Profile, evaluates the Image Constructor, and validates the closed `ImagePlan`. `Build` additionally carries a build mode and produces the native Image. `Plan` and `Build` rerun their complete authoritative prefixes rather than accepting an Analysis, `ImagePlan`, or another phase artifact as input.

A compilation finishes as exactly one of `Accepted`, `Rejected`, `Cancelled`, `HostFailed`, or `Defect`. `Rejected` contains Creator-correctable structured diagnostics. `HostFailed` contains failures of required native tools, temporary storage, or other host prerequisites. `Defect` contains violated compiler invariants, malformed compiler-produced artifacts, failed verifiers, unknown backend helpers, and bounded reproduction evidence. No unsuccessful outcome contains an Image, and the compiler never disguises its own defect as a Creator error.

Rejected requests may expose only immutable artifacts valid before the hard error gate, such as lossless syntax and locally valid semantic observations. Cancelled requests expose no downgraded intent, resumable token, partial result, or publicly reusable cache entry. Cancellation is one-way operational control polled at compiler-owned bounded checkpoints; it is not a build input and cannot change any request that reaches an accepted or rejected canonical result. The synchronous Rust interface lets a host tool choose its own worker-thread policy without selecting an async runtime for compiler semantics.

An `InspectSelection` changes only which immutable semantic observations appear in the returned `InspectArchive`. Callers may inspect syntax, stable identities, Typed-HIR summaries, behavior facts, plans, costs, layouts, receipts, and Evidence Bundle data, but cannot select or skip passes, advance compilation, mutate artifacts, or feed an artifact into another request. Internal artifact types are valid only for the exact compiler build that produced them; only deliberately persisted contracts receive canonical versioned encodings.

The compiler may use controlled temporary storage and private native-tool adapters, but it accepts no output directory and performs no caller-visible persistence. Accepted artifacts carry their canonical identities and contents; the CLI or editor decides where to retain them. A caught unexpected host panic becomes a bounded `Defect`, poisons that Compiler instance, and requires the caller to reopen it before another request.

The initial Rust workspace exposes one public `wrela-compiler` library Module. Syntax, resolution, Typed HIR, the evaluator, diagnostics, and later IR phases remain private implementation modules. Consumer-level tests cross the same `compile` seam as real tools; narrow phase and verifier tests remain private to the implementation. A new public crate, daemon protocol, session interface, progress stream, inspection pager, or persistent artifact store requires a demonstrated second seam or measured editor need rather than speculative flexibility.

Project source cannot extend the compiler with plugins. The authenticated toolchain supplies a closed set of Image-kind and Facility planners. A Project selects and configures those planners through ordinary checked Wrela declarations and its Image Constructor.

Authenticated Wrela modules come from a compiler-distribution registry keyed by content digest, role, version, and permitted Compiler Primitives. Trust is not a source modifier or reserved pathname, and Project modules cannot shadow or forge registered module identities.

The compiler represents the evaluated declaration graph as a generic target-neutral `ImagePlan`. Trusted planners validate and refine their owned declarations without hard-coding Console, Display, Event Store, or individual Virtio devices into the generic compiler graph.

## Source and identity

Parsing produces an immutable lossless syntax representation suitable for exact source preservation, diagnostics, formatting, and future editor collaboration. Every source byte is owned exactly once by a token, trivia region, or invalid-token node, and an unedited tree prints byte-for-byte identically to its input. Recovery may create explicitly marked zero-width missing nodes but never invented source bytes. Syntax nodes have no durable identity across compilation requests. Semantic analysis uses a separate representation rather than progressively annotating or rewriting syntax nodes.

The Module closure begins at the only root source, `src/image.wr`. Other Modules are hierarchically nested, derive identity from canonical paths rather than source declarations, and are selected through the one absolute whole-namespace `from parent.path import leaf [as alias]` form without ambient filesystem reads. The Project snapshot digest covers all captured candidate source, while a separate semantic closure digest covers only the reachable Project Modules, their authenticated Module dependencies, and their exact origins. Editing unreachable valid source changes the snapshot revision without changing the semantic closure, accepted Image, or Image identity.

Missing roots, malformed Project paths, duplicate or colliding Module identities, declaration visibility failures, unresolved imports, and import cycles are Creator-correctable rejection. A host adapter's inability to capture selected bytes consistently is a host failure. Compiler invariant violations after accepting an immutable snapshot remain defects. Diagnostics and Evidence Bundles name canonical Project-relative paths and never expose absolute host locations.

Resolution assigns closure-wide stable `ModuleId`, `DefId`, `TypeId`, and `InstanceId` identities before typing and lowering. A declaration remains owned by its defining module, and every monomorphized instance has one canonical closure-wide identity. Source spelling, AST addresses, copied imported declarations, and concatenated strings are never semantic identities.

Every semantic value that originated in source retains file-aware provenance. Generated compiler structures retain provenance back to the declaration, planner, or lowering step that produced them.

Generic declarations are resolved and structurally validated before use. A reachable concrete combination of a generic `DefId` and compile-time arguments is demand-specialized, fully checked after substitution, and interned as one closure-wide `InstanceId`. Wrela initially treats generics as checked templates rather than implementing a polymorphic Core IR, dictionary passing, or backend-owned specialization.

The compiler computes the complete reachable `InstanceId` set from the closed Image graph before Core lowering. Only that set participates in effects, ownership, capacity, cost, and code generation. Linker section collection is a defensive validation and size optimization, not the definition of semantic reachability.

## Representation stack

The compiler uses representations with narrow responsibilities:

1. Lossless syntax preserves what the Creator wrote.
2. Typed HIR records resolved identities, types, ownership, effects, and source provenance.
3. Core IR makes monomorphized control flow, values, ownership operations, Panics, and Wrela-specific optimization facts explicit.
4. Flow IR represents suspension, Actor Turns, Replies, Groups, cleanup, cancellation checkpoints, and statically planned async frames.
5. World and Transport IRs represent symbolic visual structure, spatial bounds, approximation contracts, acceleration structures, and field-evaluation plans.
6. `ImagePlan` represents the closed target-neutral system graph, Facility requirements, resource bounds, service obligations, and logical placement constraints.
7. Backend lowering turns admitted synchronous functions and Flow resume functions into Cranelift IR and then target machine code.

Core IR is intentionally Wrela-owned. It is the home for optimizations that depend on Wrela semantics, including ownership and Pool reasoning, bounds and capacity proofs, effect-aware specialization, whole-Image constant propagation, and transformations coordinated with World compilation. World and Transport IRs may perform domain-specific transformations before producing or specializing Core IR. Cranelift remains responsible for generic machine optimization, instruction selection, register allocation, and emission; it is not Wrela's semantic optimizer.

Flow IR is also Wrela-owned and survives until scheduler behavior has been made explicit. Cranelift receives ordinary non-suspending functions and generated resume functions; it does not define Actor, Reply, Group, cancellation, or deterministic scheduling semantics.

Generated runtime glue is represented as authenticated compiler IR or authenticated precompiled runtime modules. The compiler does not generate Wrela source and feed it back through parsing and whole-closure semantic analysis.

## Compile-time evaluation

The authoritative pure evaluator executes Typed HIR directly. It uses explicit control, call, and value stacks rather than host recursion, so evaluation limits and failures are deterministic and cannot require an oversized host thread stack. Wrela does not introduce a separate Eval IR or use JIT-compiled code to define compile-time semantics.

One evaluator implements constants, compile-time branches, compile-time generic values, pure tests, and the Image Constructor. It may observe source values, literals, and authenticated compiler declarations only. Ambient files, environment variables, clocks, entropy, network state, and host process state are unavailable. A future declared immutable-content feature may add explicit build inputs without weakening this rule.

Evaluation is bounded by deterministic instruction fuel and evaluator memory. Explicit frames count against evaluator memory. Exhaustion is a build diagnostic with source provenance, not a host timeout or runtime Panic.

Compile-time and compiled ordinary floating-point operations share one specified mode: round to nearest with ties to even, preserve signed zero, use no ambient host floating-point mode, and canonicalize NaN results. Transcendental functions are not inherited from the host math library; each may be added later only with explicit language semantics and matching evaluator and runtime implementations.

## Behavior facts and optimization

Effects, possible Panic, suspension, ownership transfer and cleanup, allocation behavior, nondeterministic authority, and cost remain separate analyses with their own rules. Compiler consumers may retrieve them through a plain `FunctionFacts` aggregation keyed by stable identity; this aggregation is not a universal effect lattice. Message ordering belongs to Flow analysis, and contextual machine or visual cost may be computed separately rather than forced into every function summary.

Core transformations preserve source-observable authoritative values, effects, ownership, deterministic choices, ordering, and possible Panic outcomes. A proven-impossible check may disappear; a pass may not introduce, defer, or reorder observable failure. Flow transformations additionally preserve Actor and scheduler observations. World and Transport transformations may approximate presentation only through an admitted Visual Contract.

Typed HIR, Core IR, Flow IR, World and Transport plans, `ImagePlan`, and backend inputs each have structural verifiers. Test and debug builds verify after individual transformations; release builds verify at major pipeline seams. Generated pure programs compare evaluator results with optimized compiled results, while scheduler behavior remains checked through its independent model. Wrela does not require formal proofs or an optimizer-wide model checker.

## Layout domains

The compiler keeps three layout domains distinct:

- **Wire Layout** is a portable exact byte contract for persistent Events, Store Snapshots, messages or protocols that explicitly request it, and other schema-governed data.
- **Logical Image Layout** is the target-neutral bounded arrangement of Pools, Mailboxes, async frames, buffers, Facility state, scheduler structures, and memory regions required by an `ImagePlan`.
- **Target ABI Layout** is the target-specific calling convention, stack and register representation, alignment, relocation, and machine encoding used by Cranelift and the selected machine serializer.

Crossing between layout domains requires an explicit checked lowering. A native struct layout never silently becomes a Wire Layout, and a Cranelift ABI choice never changes an Event schema or target-neutral capacity proof.

## Pipeline, diagnostics, and artifacts

Parsing, name resolution, typing, pure evaluation, monomorphization, proof analyses, World compilation, Image planning, cost analysis, and backend lowering consume and produce immutable artifacts through explicit stage interfaces. Global mutable compilation switches and duplicated frontends are forbidden.

Wrela initially implements this as a batch pipeline without a general query engine, dependency invalidator, in-memory incremental-recomputation promise, persistent cache, public progress stream, or revisioned compiler session. Stable identities and explicit inputs preserve the option to add those mechanisms when the editor creates a measured need. Any future reuse must be observationally equivalent to a fresh request with the same compiler executable and declared inputs; the batch build remains authoritative.

Lexing preserves invalid UTF-8, invalid characters, and malformed literals as invalid tokens with diagnostics rather than aborting. Parsing recovers at closing delimiters, line boundaries, dedents, and recognizable declaration starts without inventing indentation structure. Structurally valid declaration islands may lower independently even when other declarations in the same source are malformed. A malformed declaration remains syntax-only, and error values, missing nodes, and placeholder types do not propagate into semantic representations, the evaluator, Core IR, World compilation, Image planning, or backend representations. A hard no-errors gate precedes executable planning. Each file reports at most 64 syntax diagnostics followed by one explicit truncation diagnostic, while its complete syntax remains inspectable. IDE-grade typed holes and cross-file recovery are deferred.

Source ranges use request-local source identity plus authoritative half-open byte offsets. Human line and column locations are derived views; protocol-specific encodings such as UTF-16 editor positions belong in host adapters. Durable semantic declaration identities are defined independently of ephemeral syntax nodes.

Creator diagnostics and compiler defects are disjoint. A violated compiler invariant produces a structured internal compiler error containing the failed phase, stable artifact identities, and bounded reproduction evidence, and no Image is emitted. The compiler never disguises its own defect as a Creator source error.

Every important phase can emit a structured inspect artifact for reports and tests. These observations describe stable semantics and plans rather than promising compatibility for incidental Cranelift IR, register allocation, instruction bytes, or final Image byte offsets.

The Image carries compact stable diagnostic and Panic-site identities. A canonical Evidence Bundle keyed by Image digest holds Project-relative source spans, symbols, layouts, plans, and phase receipts for launchers, tests, and tools. It excludes absolute host paths and need not embed complete source text into the Image. The public `.wrela-image` container holds its Architecture Profile, Device Manifest, admitted RAM layout, bootable ELF, and any required architecture-owned reset member.

## Native backend and linking

The first compiler is AOT-only. It lowers admitted synchronous functions and Flow resume functions to Cranelift and emits standard target ELF object files through Cranelift's object module. Authenticated runtime and target-stub objects use the same link seam.

Each Architecture Profile declares a fixed ISA, CPU-feature baseline, conventional freestanding calling convention, Cranelift version and flags, VM ABI revision, and trusted architecture stubs. Code generation never detects or inherits features from the compiler host. The initial compiler uses the architecture's conventional ABI across generated code and trusted stubs rather than defining a custom Wrela calling convention.

Safe runtime policy, Facilities, and Drivers are implemented primarily as authenticated Wrela modules. Minimal architecture-owned Rust or assembly stubs contain reset, interrupt and fault entry, MMIO, context switching, and operations that cannot be expressed through safe Wrela and Compiler Primitives.

A closed primitive manifest maps each typed Compiler Primitive to its exact Image-internal symbol, calling convention, required Capability-bearing operands, and permitted Architecture Profiles. Neither Creator nor authenticated Wrela source can declare a general FFI or acquire authority merely by naming a linker symbol.

An Image links no libc, libm, host operating-system library, shared library, dynamic library, or other external library. When Cranelift requests a helper operation, the backend must lower it directly or resolve it to a finite allowlisted sealed function owned by the Image runtime. Every requested helper appears in build evidence, an unknown helper is a compiler error, and the final linked result must contain no unresolved symbols.

A locally installed `ld.lld` links those objects under a compiler-generated, architecture-owned linker script. The compiler selects the explicit `ld.lld` executable name rather than a host-default `ld`, resolves it through the developer's `PATH` only when `Build` needs it, records its resolved path and reported version in build evidence, and reports absence or invocation failure as `HostFailed`. The linker resolves Image-internal symbols, applies relocations, places ELF sections, and reports overflows. ELF exists only as a host-side linking and QEMU-loading interchange: a small Wrela packager validates its linked segments against the Logical Image Layout and VM ABI, adds the Architecture Profile, Device Manifest, exact admitted RAM layout, and any architecture-owned reset member, and produces the final `.wrela-image` container. The guest does not contain an ELF loader. Wrela does not initially implement a general linker or private relocatable object format.

The current developer checkout has no external-tool setup operation or lock. It does not hash native tools, traverse their dynamic dependencies, inspect code signatures, bind them to a macOS build, or detect upgrades. Downloading, vendoring, authenticating, signing, and distributing a self-contained toolchain are one later product problem and must be designed together when Wrela is distributed beyond its builders.

Cranelift JIT is admitted only for compiler-produced effect-free Preview Kernels used by the graphical editor. It is not the semantic oracle, a general Wrela host target, deployed Image execution, an Actor or Facility runtime, a Driver environment, or part of scheduler semantics. Preview Kernels expose no host-call seam or ambient authority and are differentially checked against the Typed-HIR evaluator and scalar presentation semantics. They execute in a replaceable child Preview Worker so generated-code failure does not share the editor address space. All complete Images remain AOT-only.

## Reproducibility

Repeated builds using the same compiler executable, declared inputs, Architecture Profile, authenticated planner and runtime objects, Cranelift version and flags, unchanged local `ld.lld` installation, and build mode produce bit-identical Image bytes and canonical structured reports. Artifacts exclude timestamps, absolute host paths, host scheduling order, and unordered iteration.

Rebuilding an identical compiler executable from source across different hosts or across changes to locally installed native tools is a future toolchain-distribution and reproducibility problem. Wrela does not promise byte identity across compiler versions, Cranelift versions, LLD installations, build modes, or target architectures.

Development and optimized build modes preserve identical language, ownership, Panic, floating-point, and scheduler semantics. Neither may bypass resource, ownership, scheduler, deadline, or VM ABI proofs. A development Image may carry instrumentation and lack flagship performance certification, provided the Evidence Bundle says so explicitly; a development-only behavior is never valid evidence for a release-only semantic rule.

## Wrela8 inheritance

Wrela9 preserves the useful semantics and behavioral cases from Wrela8's lexer, grammar, type/access/move/flow analyses, whole-closure proofs, deterministic compile-time values and quotas, explicit async state-machine concepts, final artifact validation, and phase inspection. Each relevant case is inventoried and explicitly adopted, revised, or retired; none is binding merely because Wrela8 implemented it.

Wrela9 rewrites Wrela8's string-based identity, imported-body splicing, per-module generic ownership, recursive compile-time interpreter, closed product-specific `ImageGraph`, generated-runtime-source feedback loop, orchestration monolith, thread-local options, fragmented diagnostics, custom general-purpose optimizer, custom register allocator, AArch64 emitter, and linker relaxation. Wrela8's old intermediate and machine dumps remain migration evidence, not Wrela9 compatibility requirements.
