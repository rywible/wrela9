# Language Core

Status: accepted language and runtime lifecycle architecture. Detailed compiler, Facility, and tooling protocols remain focused implementation specifications.

## Image and host boundary

Wrela Creator code produces self-contained bootable Images against one generic VM ABI. The Console is the flagship curated kind of Image, but the Image model, Constructor, Facilities, Drivers, and Device Manifest do not assume every system is a game. The first release proves the generic architecture through a Console product rather than requiring a second non-game product.

The compiler, launcher, inspector, and editor are host-native Rust tools. Wrela does not produce macOS or Linux applications and has no planned self-hosting requirement. The host evaluator supports builds and pure semantic tests; a sealed Cranelift JIT executes only effect-free compiler-produced editor Preview Kernels and is not a deployable Wrela target or effectful runtime.

Wrela makes no source or Image compatibility promise for now. Reproducibility comes from pinning the complete compiler, VM, and build inputs. Compatibility may be reconsidered only through a later explicit decision.

The simple evaluator required for constants and effect-free Image construction is authoritative for pure language operations. Native backends share its conformance corpus. Wrela does not build a second complete interpreter for effectful Driver or Actor execution.

## Memory, ownership, and borrowing

Wrela has no implicit general-purpose heap or tracing garbage collector. Fixed values and arrays live inline; dynamic storage comes from explicitly named bounded Pools. Growable vectors, maps, sets, queues, and runtime text builders identify their Pool and are Resources tied to it.

Data copies implicitly. Resources have one owner, move only through explicit `take`, and make the source unreadable until it is reinitialized.

A plain `struct` is Data and may contain only Data fields. A `resource struct` is explicitly single-owner and may contain Data or Resources. The compiler never silently promotes a `struct` to Resource semantics because one field changes; owning a Resource requires an explicit Resource declaration, and the owned field's reclaim or protocol obligations propagate through it. Declaring a `resource struct` selects ownership semantics but does not introduce a destructor, finalizer, or arbitrary implicit cleanup.

A collection may own Resource elements and then propagates their ownership obligations. Compiler-reclaimable elements may be reclaimed automatically. A collection containing protocol Resources must be explicitly drained, consumed, transferred, or protected by bounded cleanup before it can disappear.

Each Pool declaration creates a compiler-known generative identity `P`. Allocation may return an owning `own[P] T` Resource and a copyable unforgeable `Key[P,T]` Data identity for later lookup. A Key cannot access storage without the Pool, and generation checking makes lookup through a stale Key return absence rather than alias a reused slot. Generic functions may abstract over the Pool identity without creator-authored lifetime parameters.

`read` and `mut` borrows are call- or block-scoped. They cannot be stored, returned, placed in messages, or retained across suspension. Long-lived relationships use domain identity or Pool Keys rather than stored references.

Compiler-known ownership such as ordinary Pool allocations may be reclaimed automatically on recoverable exits. Arbitrary user-defined destructors and finalizers do not run implicitly. Protocol Resources must be explicitly consumed, transferred, or protected by bounded cleanup. Panic does not run source-level cleanup because it ends the Image.

Deferred cleanup must complete without returning a recoverable error. Fallible protocol resolution is handled explicitly before scope exit; a deferred invariant failure may Panic but cannot replace or aggregate with the original outcome.

Pools expose distinct allocation intents. `allocate` requires a whole-Image capacity proof and is infallible. `reserve` requires the same proof and returns a one-shot Pool Permit that guarantees a later infallible allocation. `try_allocate` explicitly models expected capacity pressure and returns `PoolFull`. Proving one operation safe never changes its source-visible result type into another operation's type.

Every readable value is definitely initialized. Bindings and fields are immutable unless explicitly mutable. Moving a Resource produces a compiler-tracked uninitialized location, never a readable null or zero value.

## Collections, text, and identity

Fixed-array length is part of the array type. A pooled collection's runtime capacity belongs to its checked construction and compiler metadata rather than silently changing its ordinary source type. Capacity-growing operations remain fallible even when whole-Image proof later removes their runtime branch.

Every iterable collection specifies deterministic order as part of its semantics. Vectors use index order, queues use FIFO order, and ordered or insertion maps state their ordering explicitly. A hash lookup collection does not expose iteration unless it can specify stable semantics.

`Text` and `Bytes` are distinct. Text contains valid Unicode stored as UTF-8 and is not indexed by an arbitrary integer. It provides explicit scalar, grapheme, and normalization operations. Text preserves its exact scalar sequence by default; NFC or NFD normalization is explicit. Bytes carries uninterpreted binary data. Literals are immutable Image Data and runtime builders use explicit Pools.

Data structs and enums automatically receive compiler-standard `Eq`, `Order`, and `Hash` capabilities whenever all of their components support the corresponding capability. Equality is structural, ordering is lexicographic in field and variant declaration order, and hashing is structural and consistent with equality. Resource types receive none automatically. Ordinary floating-point values do not satisfy these total contracts; explicit wrappers may choose bitwise or total-order behavior. Wrela initially has no derivation declaration and does not permit overriding these synthesized standard meanings; custom domain equality, ordering, or hashing uses an explicit wrapper or key projection. Absence uses `Option[T]`; Wrela has no universal or Resource-specific null.

## Functions, generics, interfaces, and modules

Function declarations state parameter modes and types, return type, generic parameters, and any explicit purity or effect ceiling. Local variables and closure details may be inferred.

An unmarked Data parameter is passed by value. Resource parameters explicitly select `read`, `mut`, or `take`; a Resource never moves implicitly. These are parameter and block modes rather than first-class reference types, so borrows cannot be stored, returned, collected, or hidden in generic arguments. Plain `self` means `read self`; mutation and consumption require `mut self` and `take self`.

Functions declared inside a nominal type are associated with that type. A function without `self`, conventionally named `new` when it constructs a value, is called through the type namespace such as `Card.new(...)`; it is not a magic constructor category. Types cannot be reopened through extension or separate implementation blocks. Struct fields have explicit types and optional `pub` and `mut` modifiers but no inline defaults, and every construction initializes every field.

Generics are structurally monomorphized and may accept types plus immutable compile-time Data with a compiler-known total canonical `ConstKey`, such as integers, enums, Text, Bytes, Spaces, units, and layouts. Ordinary `f16`, `f32`, and `f64` values may participate in compile-time evaluation but cannot directly key a generic because they do not have total equality; an explicit bitwise or total-float wrapper may qualify. Generic const Data cannot contain Facilities, host resources, Actors, Capabilities, or other identity-bearing Resources. Pool identity is a separate permitted generic kind rather than const Data. Numeric width, sign, float, unit, narrowing, saturating, and wrapping conversions are explicit.

Functions are statically higher-order and may also form runtime callables from a finite Image-known family. Runtime closures capture bounded Data only; they cannot hide Actor handles, Capabilities, Pools, or other Resources. The compiler represents the family as a closed tagged choice and reports its storage, effect union, and worst-case call cost.

Interfaces primarily constrain monomorphized generics. A nominal type explicitly lists user-defined interfaces that it `implements`, and required methods remain inside that type declaration; Wrela initially has no accidental structural conformance, default interface methods, associated types, or separate implementation blocks. Compiler-standard `Eq`, `Order`, and `Hash` capabilities are the exception because their fixed structural meanings are synthesized automatically. Explicit `any Interface` opts into a finite Image-closed existential family. Such an interface declares Data-only or Resource-owning representation, enforces method effect ceilings, exposes its concrete family in compiler reports, and has no open virtual dispatch, dynamic loading, downcasting, or runtime type reflection. Domain-significant alternatives may still use explicit enums.

Generic parameter lists distinguish bare type parameters from `const` compile-time Data parameters; Pool identities use a compiler-known `Pool` bound. Bounds remain inline without `where` clauses, defaults, specialization, or variadics initially. Type syntax includes nominal and generic types, fixed arrays, tuples, function types, `own[P] T`, and explicit `any Interface`. It has no nullable or union shorthand, raw pointer or first-class reference type, dynamically sized array, or inferred placeholder in declaration signatures. Transparent `type` aliases create no nominal identity or change to ownership, layout, units, or interface behavior.

One Project directory builds one Image and roots Creator source at `src/image.wr`. This is the only root-level source file: every other Project Module is nested beneath at least one directory. A repository may contain several independent Project directories, but Wrela has no manifest, multi-Image Project, package, dependency, or external source model. The selected Project's absolute host path is operational metadata and cannot affect source semantics or Image identity.

Each nested regular `src/**/*.wr` file is a possible importable Module. Its canonical Project-relative path is its complete Module identity: `src/game/player.wr` defines `game.player`, while `src/image.wr` defines the distinguished non-importable root Module `image`. A root-level `.wr` file other than `image.wr` is invalid Project topology rather than a one-segment Module. Source contains no redundant Module declaration, directory Module, index Module, or alternate file mapping. Every Module path segment matches `[a-z][a-z0-9_]*`; doubled and trailing underscores are valid spellings rather than parser-level style errors. Moving a file deliberately changes its Module identity.

The root Module contains exactly one `@image` Image Constructor. Another `@image` in the reachable closure is rejected, while an unreachable source file is not part of that compilation. The filesystem adapter captures every regular `.wr` file beneath `src/` into the immutable Project snapshot, but the compiler analyzes only `image` and its transitive import closure. Unreachable candidate source has no semantic diagnostics and cannot affect the accepted Image.

An import names one absolute complete Module and binds its namespace under the Module's leaf segment or an explicit `as` alias. Imports never copy declarations into the importing Module's unqualified scope. Every `pub` declaration in the imported Module is accessible through that namespace, while declarations and fields remain private to their defining Module unless marked `pub`. Wrela has exactly one import form and initially has no relative, whole-path alternative, wildcard, selective-declaration, implicit-parent, friend, or re-export form. `pub` defines a source Module interface, not a compatibility or binary-ABI promise.

```wrela
from game import player
from core import option as maybe
```

These bind `player` to the complete `game.player` Module and `maybe` to the complete `core.option` Module. Selective declaration imports such as importing `Card` directly are invalid even when that declaration is public.

Project Modules may import authenticated Modules from the sealed Compiler Distribution, using the same source import form while resolution retains their distinct origins. Authenticated Module identities are also hierarchical rather than one segment. Authenticated Modules cannot import Project Modules, and a Project cannot declare or shadow an authenticated Module identity. Trust derives from the distribution registry and content identity rather than source spelling.

The reachable Module import graph is acyclic without type-only or compile-time exceptions. Resolution orders it dependency-first with canonical Module identity as the tie-breaker and preserves source order within a Module; filesystem enumeration and import discovery order are unobservable. Missing imports, path collisions, authenticated-identity collisions, and case or normalization aliases are Creator diagnostics with deterministic Project-relative provenance.

Metaprogramming consists of ordinary effect-free compile-time Wrela, generics, constant evaluation, symbolic visual declarations, and the Image Constructor. Wrela does not initially provide syntax macros, AST reflection, or Project compiler plugins.

Module declarations are restricted to imports, `const`, `fn`, `async fn`, `struct`, `resource struct`, `enum`, `interface`, `pool`, transparent `type` aliases, attributes, and `comptime if`; Modules have no executable statements or mutable `static`. `@actor` classifies a struct as Actor state, while `@image` classifies an ordinary function as the one root Image Constructor. `const`, `comptime if`, and `comptime assert` are the complete initial compile-time syntax: ordinary pure functions execute during constant evaluation and Image construction without a separate `comptime fn` category. Every declaration and member is private unless `pub`, with no restricted visibility or public imports.

Methods, associated constants, and compile-time conditionals may appear inside structs, Resource structs, and enums. Interfaces contain required method signatures and optional associated constants, but no stored fields, default bodies, associated types, or nested declarations. Types and Pools cannot be nested. Effects are inferred, with `pure fn` available as an enforceable ceiling. Only `async fn` may suspend; on an `@actor` struct, its public async methods form the externally callable message interface, while synchronous methods are never remote calls. Pool declarations name generative identities whose capacity and lifetime are selected when instantiated by the Image Constructor or a scoped owner.

## Source text and syntax

A valid Wrela source file is UTF-8 without a byte-order mark. LF and CRLF line endings are accepted, a final line ending is optional, and bare CR is invalid. The lossless syntax representation still preserves arbitrary input bytes, including invalid UTF-8 and malformed tokens, so tools can inspect and repair rejected source without silently changing it. Unicode is available in Text, scalar literals, and comments; identifiers initially use the ASCII form `[A-Za-z_][A-Za-z0-9_]*`.

Wrela uses significant indentation with exactly four spaces per level. Tabs are invalid outside literals and comments, while blank and comment-only lines do not affect indentation. A colon-introduced suite always begins on a following indented line; inline suites and semicolon-separated statements are not part of the language. Ordinary `#` comments and `##` declaration documentation are preserved exactly, and Wrela initially has no block comments.

The language globally reserves only words used by its current grammar. It does not reserve speculative future words and accepts that adding a later keyword may require source changes while Wrela makes no compatibility promise. Attributes use uniform `@name(...)` syntax but form a closed compiler-recognized set: Project code cannot define attributes, derivations, syntax macros, or textual preprocessing. Unknown attributes are semantic errors. Every branch of a compile-time conditional must contain valid ordinary Wrela syntax even when evaluation does not select it.

## Executable syntax

A first assignment introduces an initialized immutable local, with an optional explicit type; prefix `mut` introduces a reassignable local. Wrela has no `let`, uninitialized local, chained assignment, assignment expression, or destructuring assignment. Simple and compound assignment may target only mutable locals, mutable fields, and mutable indexed elements. Compound arithmetic remains checked.

The initial statement set consists of bindings, assignment, expression statements, `if`/`elif`/`else`, exhaustive `match`, bounded `for` and `while`, nearest-loop `break` and `continue`, `return`, `pass`, runtime and compile-time assertions, `defer`, compiler-known `with`, and compile-time conditionals. Wrela has no exceptions, labels, `goto`, fallthrough, or general block expressions. A `while` loop is accepted only when the compiler derives and reports a finite maximum iteration count.

Patterns include wildcard, literal, binding, enum-variant, struct, tuple, fixed-array, explicit `take` binding, and or-pattern forms. Every or-pattern alternative binds the same names with the same modes and types. Guards express ranges and other computed conditions rather than admitting arbitrary expression patterns. `match` is a statement whose cases are considered in source order and must exhaust every closed alternative; guarded cases require an unguarded complete fallback. `value is Pattern` provides single-case conditional destructuring, with bindings scoped only to the successful branch; a negated pattern test cannot bind names. `if` and `match` are not initially expressions.

Direct struct construction uses a type call with every field named, while enum variants use qualified calls. Ordinary functions may accept positional or named arguments, with no positional argument following a named one. Tuple literals and fixed-array literals are built in; `[value; N]` repeats into a fixed array. Wrela has no implicit heap-backed list literal. Module namespaces, associated functions, enum variants, fields, and methods all use `.` and are distinguished by name resolution.

Wrela has a fixed operator vocabulary and precedence table. Arithmetic `+`, `-`, `*`, `/`, and `%`; bitwise `&`, `|`, `^`, `~`, `<<`, and `>>`; comparisons; short-circuit `not`, `and`, and `or`; half-open `..` and inclusive `..=` ranges; unary numeric signs; and postfix `?` are initial forms. Compiler-known operator interfaces may admit Data types, but Creators cannot define operator spellings or precedence and operators never move Resources implicitly. Arithmetic is checked by default; wrapping, saturating, widening, narrowing, fused, and exponentiation operations are explicitly named. Comparisons do not chain. Ranges require both endpoints, and stepping is an explicit bounded operation. Indexing is checked, general slicing is absent, and Text is not integer-indexable.

Prefix `take` moves only from an assignable Resource location and leaves it unreadable until restored. Postfix `?` propagates compatible `Result` and `Option` alternatives as an ownership-checked early return. Request/reply uses `await actor.method(...)`; fire-and-forget admission uses `await send actor.method(...)`, while `try_send actor.method(...)` performs immediate deterministic arbitration. `await` is valid only in async functions, and `read` or `mut` borrows cannot cross it.

Closures use `|parameters| expression` or a colon followed by an indented suite. Their parameter and return types may be inferred from context; runtime closures capture bounded Data only, and async closures and explicit capture lists are absent initially. `defer expression` runs in reverse registration order on recoverable exits, cannot return a recoverable error, and never runs after Panic. `with` is restricted to compiler-known ownership scopes such as borrows, scoped Pools, and Groups rather than a user-defined context-manager protocol.

Runtime `assert` and explicit `panic` remain present in every build, with compile-time Text messages that become compact Image diagnostic sites. `comptime assert` fails during compilation. Expression statements may discard ordinary Data, but never Resources, Replies, `Result`, or other compiler-designated must-use values. `_ = expression` acknowledges intentional Data disposal but cannot erase an unresolved failure or ownership obligation.

Receivers, operands, call arguments, collection elements, and field initializers evaluate exactly once from left to right. Short-circuit Boolean operators are the only ordinary expressions that conditionally skip a later operand. Optimization preserves this order whenever effects, ownership, or Panic make it observable.

## Literals

Integers support decimal, binary, octal, and hexadecimal spelling with validated underscore separators. Signs remain unary operators. Suffixes select `u8`, `u16`, `u32`, `u64`, `i8`, `i16`, `i32`, or `i64`; Wrela exposes no host-sized integer. Unsuffixed integers infer from context and otherwise default to `i64`.

Floating-point literals support decimal fractions and exponents with `f16`, `f32`, or `f64` suffixes. Unsuffixed values infer from context and otherwise default to `f64`. All three are ordinary IEEE-format language scalars; arithmetic retains the selected precision and rounds after each operation rather than silently promoting, even when a backend implements the operation through a wider type. NaN and infinity use explicit standard values rather than special literal spellings. Wrela has no ambiguous `f8`; a later source-visible eight-bit format would receive an exact encoding name, while Display may internally quantize presentation data under the Visual Contract.

Double-quoted literals are immutable UTF-8 Text, single-quoted literals contain exactly one Unicode scalar, and byte strings admit ASCII source characters plus explicit byte escapes. Wrela initially has no raw-string syntax or general runtime interpolation. Triple-quoted Text supports authored multiline content: one newline immediately after the opener is ignored, the closing delimiter appears alone at the surrounding indentation, and exactly that indentation prefix is stripped from each nonblank content line. Runtime formatting uses an explicit bounded Text builder or structured presentation facility so Pool, capacity, failure, and update cost remain visible. Boolean literals are `true` and `false`, unit is `()`, and absence is expressed through `Option.None` rather than a null literal.

Creator code cannot inspect physical or virtual addresses, pointer width, native object size, native alignment, stack layout, or Target ABI offsets through runtime or compile-time operations. It reasons through types, capacities, Pools, and explicit Wire Layouts. Authenticated modules may receive sealed compiler layout tokens and Compiler Primitives without exposing a general `sizeof`, `alignof`, address, pointer, or FFI facility.

## Effects, authority, and purity

External effects require explicit Resources or Capabilities. The compiler infers a closed set of effects including messaging a particular Actor, writing through an Event Producer, reading Input, presenting a View, using Entropy, and operating a Driver protocol. Tooling and diagnostics expose the precise set, while source declarations usually constrain readable families or state `pure` rather than spelling a complete effect row.

Wrela has no semantic stdout, stderr, `printf`, or Creator-visible console. Creator observations use structured bounded Telemetry, while diagnostics remain structured compiler or VM ABI records whose human rendering belongs to host tools.

An effect declaration grants no authority. A caller still needs the corresponding Capability. Root Image Facilities and Capabilities remain wired to selected Actors by the Image Constructor; they may produce bounded protocol Resources and receipts that move through messages, but the root authority itself does not migrate dynamically.

A pure function may create and mutate bounded scratch state owned entirely within the call when no identity, allocation pressure, or mutation escapes. Using caller-owned Pools or mutating caller state is observable and therefore effectful.

Panic is a tracked defect possibility rather than an authority effect. Pure code may Panic after violated invariants such as checked overflow or invalid indexing. A Panic during build evaluation becomes a build diagnostic.

## Actor communication and placement

Every Actor owns one bounded Mailbox. `try_send` attempts immediate admission and returns `Full` when capacity is unavailable. `send` waits for admission under its Group and deadline and is cancellable before admission. Once a message is admitted, cancellation of its sender does not retract it; delivery is guaranteed unless the complete Image Panics.

One global logical Mailbox capacity covers resident messages, admitted cross-core messages, and reserved admissions. Transport queues do not create extra hidden capacity. Cross-core `try_send` may suspend until the destination core arbitrates the current proposals in logical order; it does not wait for future capacity and resumes with admitted or `Full` from that deterministic arbitration.

The compiler derives the minimum Mailbox capacity required by proof-required sends, Reply protocols, deadlines, and service plans. Creators may state deliberate semantic capacities for `try_send` pressure or memory budgets, and the compiler reports any guarantees those bounds reject. Mailboxes never grow at runtime.

Messages may contain Resources moved with explicit `take`. After admission the Mailbox owns the complete message and every contained Resource until the receiving Turn accepts them; sender and receiver never retain shared access.

Concurrent sends to one Mailbox commit in a deterministic total order derived from stable sender identity, Turn sequence, and send ordinal rather than host-core arrival. Each sender's program order is preserved.

Request/reply uses a compiler-managed one-shot Reply Resource. The requester creates the pair, moves the fulfillment end with the request, and waits within a Group. The receiver must fulfill or explicitly cancel it; Replies are not copyable shared futures and do not introduce a dynamically selected Actor destination.

Admitting a request reserves its complete return path and response storage. Reply fulfillment cannot fail for capacity; it either delivers the response or returns `ReplyClosed` with ownership when the waiter has cancelled.

Cancelling the waiting Group closes its Reply endpoint. A later fulfillment returns `ReplyClosed` together with ownership of the undelivered response so the receiver can reclaim, reroute, or explicitly dispose of it. The compiler rejects any possible cycle of Reply waits held across non-reentrant Actor Turns; cyclic workflows must end a Turn and continue through a later message.

Recoverable handler failures have no scheduler-defined meaning. Each handler must resolve them locally or communicate them through an explicit Reply or outgoing protocol message. The runtime does not restart an Actor, retry a message, or infer an original requester.

Every Actor created by the Image Constructor exists for the Image lifetime. Inactive behavior is explicit Actor state; handles never become stale through termination and Actors are not recreated dynamically.

The compiler assigns every Actor a permanent core using admitted cost and communication structure. Creators may declare co-location, separation, or exact-core constraints when semantics or measured evidence requires them. Actors do not migrate and are not stolen at runtime.

Each core follows a compiler-planned deterministic cyclic service plan with bounded quotas for ingress arbitration, ready Actors, Group children, and Driver work. Every ready service class receives a reported maximum delay; host queue depth and timing never create dynamic priority.

The service plan is deterministically work-conserving. When a reserved slot's work is not ready, a compiler-emitted fallback order may use its slack only when doing so cannot violate another class's maximum service delay. Handler-specific admitted costs avoid reserving every Actor slot for its most expensive message while preserving non-preemptive bounded Turns.

Group outcomes distinguish external or parent `Cancelled`, execution that reaches `DeadlineExceeded`, and work rejected as `DeadlineUnmeetable`. Cancellation remains cooperative at compiler-selected safe checkpoints, but the compiler reports and enforces a maximum cancellation-observation latency for every cancellable activation rather than checking only at convenient awaits.

Deadline classes constrain compilation and admission rather than dynamically reprioritizing runtime work. The compiler synthesizes a plan that satisfies admitted classes, and work that cannot meet its current class returns `DeadlineUnmeetable` before admission.

Logical deadlines use deterministic Cadence occurrences or scheduler epochs and may influence replayable state. Realtime deadlines require an explicit Monotonic Clock effect; their outcomes cannot influence replayable gameplay unless captured as Replay input.

Every Actor Turn and bounded Group-child activation uses statically admitted async-frame storage. Suspended work never allocates a hidden future or relies on a runtime heap.

Group children receive copied Data and Resources moved into them. They cannot hold `read` or `mut` borrows of spawning Actor state. A Group declares an explicit bounded outcome policy such as `all`, `collect`, `race`, or `supervise`, including deterministic result ordering and sibling cancellation.

Every bounded child activation site receives compiler-planned fixed core placement and statically admitted frame storage. Cheap children normally remain co-located; independent expensive children may execute on other cores without borrowing their spawning Actor's state or participating in runtime work stealing.

Cancellation authority is a noncopyable Resource retained by the Group owner; parent cancellation and deadlines propagate automatically, while other Actors must use a protocol message to request cancellation. Admission closes before children quiesce, moved Resources return, and cleanup runs in reverse logical registration order independent of host completion timing. Parallel cleanup requires an explicit nested Group.

Wrela8's scheduler implementation will not be ported intact. Wrela9 preserves non-reentrant Turns, FIFO Mailboxes, typed awaited calls, static frames, fixed placement, and the behavioral scenarios that exposed their semantics, while rebuilding cross-core transport, global admission, fairness, cancellation, Reply delivery, and the compiler/runtime seam. Those scenarios move into structured model, conformance, integration, and diagnostic layers rather than preserving the old golden harness.

## Driver and VM boundary

Creator code has no atomics, locks, shared mutable memory, preemptive interrupt handlers, raw Virtio queues, or DMA authority. Mutable authority crosses concurrency boundaries only through Actor messages, Replies, Groups, and Image Facilities. The compiler may physically share immutable Image Data because doing so creates no observable mutable identity.

A sealed interrupt adapter acknowledges hardware, captures bounded Untrusted device state, and wakes compiler-planned Driver or Facility service. Trusted Facility Actors own Drivers and expose bounded high-level Data, Resources, and messages. Extending the Driver set requires an authenticated toolchain module rather than ordinary Project source.

Live external observations are nondeterministic. Facilities validate them before Replay records their admitted Data, logical order, and update boundary. Replay does not store raw QEMU exits, interrupt timing, malformed device traffic, or host timestamps.

Normal Image termination begins through one Image-wired Shutdown Capability. Root Groups cancel and finish cleanup, Event Store work reaches its declared durable boundary, Facilities quiesce, and the VM ABI receives a typed exit reason. Returning from an Actor or receiving a host callback does not implicitly end the Image; Panic remains a separate fail-stop path without source cleanup.

## Boot and termination

The compiler lowers the Image Constructor's closed dependency graph into ordered boot phases for VM ABI state, memory, mandatory terminal control, Drivers, Facilities, scheduler structures, Actors, and initial messages. Components start only after their readiness dependencies. A typed terminal `Ready` handshake separates boot from running; Creators do not reproduce this graph through an imperative boot Actor.

An incompatible VM ABI, absent required device, or failed Driver initialization produces typed `BootFailed` before the Image becomes running. Trusted boot code performs bounded quiescence of anything already initialized; the Image never enters a partial running state.

Shutdown completes every Event Transaction already admitted by the Event Store or ends as `ShutdownFailed`. Unsubmitted Actor memory is not implicitly persisted. Once quiescence begins, failure cannot resume a partially stopped Image: bounded recovery and reset attempts end through a terminal typed result rather than being mislabeled as source Panic.

Panic always ends the current Image without source cleanup or terminal acknowledgment. Its bounded diagnostic distinguishes source invariants, arithmetic or bounds failure, trusted-runtime invariants, and machine faults, and carries a stable Panic-site identity when one exists. A sealed terminal latch makes one preallocated best-effort report and then emits the terminal panic pulse. A build-selected launcher profile may start a completely fresh instance with reset memory and devices and the same durable Store, subject to a finite crash-loop limit. Failed source execution never performs its own restart.

Abrupt host termination is modeled as power loss. Facilities recover from their last promised durable boundary on the next boot; the Event Store exposes only complete committed transactions. The VM ABI and launcher use a typed Image Result distinguishing normal completion, requested Shutdown, `BootFailed`, `ShutdownFailed`, Panic with bounded diagnostic identity, and host power loss when it can be observed.

## Control flow and numeric semantics

Every loop has a compiler-derived maximum from a bounded collection or range. Early exit is permitted; unrestricted `while` is not. Long-lived work proceeds across Actor messages or deterministic updates rather than through one unbounded Turn.

Every recursive cycle must visibly decrease a compiler-recognized bounded measure such as a finite collection remainder, tree depth, or bounded integer. Wrela does not admit recursion through a hidden runtime guard or trusted depth assertion.

Ordinary integer arithmetic is checked and Panics on unexpected overflow. Explicit checked operations return `Result`; wrapping and saturating behavior are named in source. Ordinary indexing expresses an invariant and Panics if a remaining runtime check fails; explicit lookup returns `Option` or `Result`.

General `f32` and `f64` operations use specified IEEE-754 semantics without global fast-math reassociation. Contract-bounded approximation is confined to Form, Material, and Transport compilation and cannot influence authoritative gameplay state.

Ordinary floating-point operations use round-to-nearest ties-to-even, preserve signed zero, canonicalize NaN results, and do not inherit ambient host floating-point modes. Compile-time evaluation and generated code implement the same rules. Transcendental functions require individually specified evaluator and runtime implementations rather than inheriting a host math library.
