# Facility instance architecture

Status: research input, not accepted design. This note investigates an
architectural ambiguity exposed by Facility testing. It does not override the
current glossary, design documents, or ADRs.

Research date: 2026-08-22.

## Conclusion

Wrela's current design has **partially conflated four different things in its
prose**, although the compiler and VM design already contain most of the
mechanisms needed to separate them. The first proposed category also needs an
internal split:

1. **mandatory Image infrastructure** such as boot, terminal control, memory,
   and scheduling is distinct from a selectable **Facility kind**, whose
   authenticated implementation and planning rules define Input or Event Store;
2. a **Facility instance**: one logical service declared in one closed Image;
3. an **external binding**: a VM-visible device or subdevice plus the host
   resource connected to it; and
4. a **static component identity**: the identity of one constructed Actor,
   Pool, endpoint, Driver, or other graph node.

The recommended correction is not “make every Facility plural.” It is:

- make every selected Facility an explicit, statically identified instance;
- give each Facility kind an explicit cardinality and binding policy;
- let one Facility instance aggregate several devices, one device expose
  several binding endpoints, or several client endpoints share one Facility;
- keep Actor and resource construction identity independent from Facility and
  external-binding identity; and
- preserve a separate Image as the only initial hard-failure and restart
  realm, while allowing ordinary statically owned state to be isolated inside
  one Image.

For the current version, Display, Input, Monotonic Clock, Entropy, and Telemetry
can all plausibly remain **zero-or-one Facility instances per Image**. That is a
current-version semantic constraint, not a reason to make the Facility kind
itself the singleton identity. An Architecture Profile may have tighter
physical slot or memory admission limits, but it should not silently redefine
Facility semantics.

Event Store is the important unresolved exception. The current documents call
it both the machine-backed Facility and the Image's one logical database. Test
composition exposes the cost of that collapse. A stronger candidate model is
one zero-or-one **persistence substrate** per Image, over which the closed graph
may construct zero or more bounded **Event Store instances**. Each store would
still have one ordered history, one owning Actor, explicit Producers, and one
schema lifecycle; the substrate could place several stores on one block binding
without exposing blocks to Creators. The flagship could still select exactly
one. This is a Wrela design hypothesis rather than a conclusion supplied by the
external sources.

## The four layers

| Layer | Meaning | Lifetime and identity | Examples |
|---|---|---|---|
| Mandatory infrastructure instance | Sealed substrate present independently of Creator Facility selection | one closed Image | Boot Contract state, terminal control, scheduler |
| Facility kind | Authenticated type-level contract, planner, expansion, and cardinality rules | Compiler Distribution | Input, Event Store, Telemetry |
| Facility instance | One Image-construction declaration with configuration, capacity, exported authority, and failure policy | Image lifetime; revision-scoped `ConstructionId` | the selected flagship store service |
| Device/binding instance | One VM-visible controller, function, port, scanout, or other device endpoint, plus its launcher-side backend | Image/launch; manifest identity and binding role | one Virtio-Block function bound to one host block image |
| Static component instance | One closed graph node with state, placement, storage, and wiring | Image lifetime; revision-scoped `ConstructionId` | store-owning Actor, Event Producer endpoint, DMA Pool |

A Facility instance is itself one kind of constructed graph node, so the
Facility-instance and static-component rows may use the same `ConstructionId`
mechanism. That common mechanism does not make a Facility instance
interchangeable with its internal Actor, Driver, or endpoint nodes; typed node
kinds and owner edges must preserve the distinction.

Mandatory infrastructure may own device and resource instances without becoming
a global Facility. The current terminal serial port and `pvpanic-pci` function
are examples. Likewise, the architecture counter is sealed infrastructure that
the selected Monotonic Clock Facility normalizes. Sharing a physical controller
with infrastructure does not share its semantic capacity or expose its authority.

These identities must not be substituted for one another:

- `Store Identity` is persistent lineage across compatible Image revisions; it
  is not the Event Store Facility instance's construction identity.
- A PCI BDF identifies a manifest-planned device function in one Image layout;
  it is not a Facility identity.
- An Actor's `ConstructionId` identifies that Actor node; it does not make the
  Actor the Facility.
- A Facility kind name such as `Input` states semantics; it does not identify a
  runtime object.

## Primary-source evidence

### QEMU and Virtio separate type, device instance, and backend

**Fact.** QEMU's device model constructs a device object and then realizes that
specific `DeviceState`; the device class and the device instance are distinct.
QMP's `device_add` takes a driver class and an optional instance `id`, which
must be unique.
([QEMU qdev API](https://www.qemu.org/docs/master/devel/qdev-api),
[QMP `device_add`](https://www.qemu.org/docs/master/interop/qemu-qmp-ref.html#command-device_add))

**Fact.** QEMU explicitly distinguishes the guest-visible block device from
its backend: `-device` defines what the guest sees, while `-blockdev` describes
how QEMU handles the data. A block backend has its own unique `node-name`, which
the device's `drive` property references.
([QEMU block device options](https://www.qemu.org/docs/master/system/qemu-manpage.html#block-device-options),
[QEMU system-emulation introduction](https://www.qemu.org/docs/master/system/introduction.html))

**Fact.** Repeated device classes are ordinary QEMU instances, not new device
types. QMP input injection can route an event to a specific device when several
input devices of the same kind exist, and can additionally select a head on a
multi-scanout display.
([QMP `input-send-event`](https://www.qemu.org/docs/master/interop/qemu-qmp-ref.html#command-input-send-event))

**Fact.** Virtio itself has non-uniform cardinality relationships:

- one Virtio Input instance represents one input device;
- one Virtio Console device may expose multiple ports, each with its own receive
  and transmit queues;
- one Virtio GPU device may expose multiple scanouts and multiple framebuffers;
  and
- one Virtio Block instance exposes one capacity-bearing virtual disk.

([Virtio 1.3, §§5.2, 5.3, 5.7, and 5.8](https://docs.oasis-open.org/virtio/virtio/v1.3/virtio-v1.3.html))

**Inference for Wrela.** There cannot be one universal mapping such as “one
Facility kind equals one Virtio device.” Input naturally aggregates multiple
input device instances. Terminal and Telemetry can occupy distinct ports on
one Virtio Console controller or distinct controllers. Display may intentionally
admit only one scanout even though the transport can expose more. Event Store
naturally has a dedicated block device and host backend.

**Wrela decision still required.** QEMU's support for multiple instances and
hotplug does not require Wrela to expose either. Wrela can keep its cold-plugged
fixed Device Manifest and simply key its entries by concrete device instance
rather than by device kind.

### CAmkES and Microkit make static instances and authority explicit

**Fact.** CAmkES formally distinguishes a component type from a component
instance (`component foo f` creates instance `f` of type `foo`) and a connector
type from a connection instance. A composition instantiates named components
and connections; a configuration assigns attributes to specific instances.
([CAmkES Manual, terminology and application construction](https://docs.sel4.systems/projects/camkes/manual.html))

**Fact.** CAmkES components communicate only through explicitly declared
interfaces and static connections. Hardware component instances expose MMIO,
interrupt, or I/O-port interfaces, and the assembly explicitly connects those
interfaces to a Driver component instance. Per-instance configuration supplies
the physical ranges and interrupt details.
([CAmkES introduction](https://docs.sel4.systems/Tutorials/hello-camkes-1.html),
[CAmkES hardware components](https://docs.sel4.systems/projects/camkes/manual.html#hardware-components))

**Fact.** CAmkES normally gives each component instance its own address space;
explicit groups may colocate instances, trading isolation for cheaper direct
calls. The seL4 Microkit similarly takes a static system description, assigns
resources up front, gives each protection domain only its explicitly granted
capabilities, and can reject resource exhaustion while building the image
rather than during system initialization.
([CAmkES single-address-space groups](https://docs.sel4.systems/projects/camkes/manual.html#single-address-space-components-groups),
[Microkit manual](https://docs.sel4.systems/projects/microkit/manual/latest/),
[Microkit capability tutorial](https://docs.sel4.systems/projects/microkit/tutorial/part1.html))

**Inference for Wrela.** The useful pattern is the type/instance/connection
separation, not CAmkES syntax or its process model. Wrela's Image Constructor,
Build Constructors, static Capability wiring, and exact Logical Image Layout
already provide an equivalent place to identify each Facility instance,
exported endpoint, internal Actor, and resource allocation.

**Wrela decision still required.** CAmkES-style address-space isolation is not
compatible with Wrela's current one identity-mapped guest address space and
global Panic semantics without a major architectural revision. Static wiring
does not by itself imply protection domains.

### MirageOS separates required interfaces from selected implementations

**Fact.** MirageOS application logic is written as OCaml functors parameterized
over device module signatures. Its typed `config.ml` graph declares which
devices a job requires and applies the job to concrete implementations; the
configuration tool generates the final application graph and links the selected
libraries into one unikernel.
([Mirage eDSL source documentation](https://github.com/mirage/mirage/blob/main/lib/mirage.mli),
[MirageOS hello-world configuration](https://mirage.io/docs/hello-world),
[Functoria introduction](https://mirage.io/blog/introducing-functoria))

**Fact.** Mirage can select different implementations of the same logical
interface for different deployment targets. Its documentation explicitly
recommends developing and testing with the Unix backend before adapting the
same code to `hvt` or Xen.
([Learning about Mirage](https://mirage.io/docs/learning),
[MirageOS installation and backends](https://mirage.io/docs/install))

**Inference for Wrela.** Facility-facing Creator logic should depend on typed
instance endpoints, while an authenticated planner chooses Drivers and VM
bindings. This makes the semantic Facility contract testable independently of
a particular BDF or host path.

**Wrela decision still required.** Wrela has deliberately rejected a host
application target and a second effectful runtime. It should not copy Mirage's
Unix deployment backend. The transferable idea is typed implementation
selection; Wrela's effectful end-to-end authority remains the real QEMU Image,
with pure models and controlled QEMU backends used as test instruments.

## Diagnosis of the current Wrela design

### What is already separated correctly

**Fact from current design.** The compiler's Build Constructors create symbolic
graph nodes with deterministic revision-scoped `ConstructionId`s. Facility
planners run only after the generic graph is sealed, and the Device Manifest
records exact BDF, BAR, and interrupt-route assignments. This already separates
source construction, Facility planning, and physical device placement.
([compiler design](../design/compiler.md),
[VM and boot design](../design/vm-and-boot.md))

**Fact from current design.** `Store Identity` is deliberately independent of
Image digest, while a launcher binds it to one exclusive host block image. The
current Event Store has many statically known Event Producers but one ordering
Actor and one history.
([ADR-0011](../adr/0011-keep-game-content-in-the-image-and-use-an-event-store.md),
[ADR-0016](../adr/0016-identify-production-event-stores-independently-of-images.md))

**Fact from current design.** Cadence is an ordinary Actor-based standard
library module, not a Monotonic Clock Facility instance. Multiple Cadences can
consume the one selected clock service.
([Facility design](../design/facilities.md))

### What remains ambiguous or conflated

**Fact from current prose.** `Image Facility` is defined as a high-level
declaration that expands into Drivers, Actors, Pools, interrupts, and manifest
entries, but no general text defines a Facility instance identity, per-kind
cardinality, exported endpoints, or how device requirements relate to one
instance. The prose therefore slides between “the Facility kind,” “the selected
service,” and “the device.”

**Fact from current design.** Input is singular at the logical level and has one
owning Actor, yet its implementation requires keyboard and mouse device
instances. Telemetry and mandatory terminal control are distinct semantic
channels that both use generic Virtio serial ports. These are already examples
where logical and physical multiplicity differ.

**Inference.** The missing abstraction is not a new runtime object model. It is
an explicit planning schema connecting one constructed Facility instance to its
exported endpoints, sealed internal graph, and concrete device/binding roles.

### The Event Store fork

The sources establish that Wrela should distinguish a logical instance from a
physical binding. They cannot decide which side of that distinction deserves
the name `Event Store`. Two internally coherent designs remain:

1. **Event Store is the Facility.** One Image admits one Event Store instance,
   one history, and one block binding. Any scenario needing a fresh independent
   Store needs a fresh Image launch. This preserves the current glossary and
   ADRs, but it makes the Image both the deployment unit and the smallest unit
   of mutable-state isolation.
2. **Persistence is the Facility; Event Store is a constructible service.** One
   authenticated persistence substrate owns the Driver, queues, flush policy,
   and block binding. Build Constructors create statically bounded Event Store
   instances over disjoint admitted extents. Each has its own identity, history,
   owning Actor, Producers, schema, capacity, and endpoints. The stores share a
   device failure domain but not logical mutable state.

The second design is analogous to the current Monotonic Clock/Cadence split:
one machine-backed time source supports several ordinary statically constructed
Cadence Actors. It does not imply a raw block Facility, a filesystem, runtime
database creation, tenants, or dynamic lookup. The Creator may see only the
typed Event Store constructor while the compiler and authenticated substrate
own physical placement.

Testing is useful architectural pressure here rather than a reason for a
test-only exception. A consolidated Test Image needs many independently owned
mutable systems but does not normally need separate hardware-failure domains.
If two static systems with disjoint state cannot coexist because a logical
service is accidentally tied to one physical binding, the same composition
limit applies to non-test Images. Conversely, Panic, power-loss, exclusive-host-
binding, and device-reset scenarios genuinely require separate launches and
should not be simulated by logical Store multiplicity.

**Recommendation for further design.** Treat the second model as the stronger
working hypothesis and stress its costs before retaining the current singleton:
stable identity for several production Stores, schema-lock ownership, total
capacity planning, batching across isolated histories, and failure propagation.
Do not accept it merely to reduce test boot count. If those semantics cannot be
made honest, the consequence is explicit: Store-bearing cases require separate
Image launches and the one-consolidated-Test-Image decision must be revised.

## Alternatives under stress

### A. One global singleton per Facility kind

This is the smallest implementation and matches much of the current prose.
However, if the kind name is also the instance identity, authority becomes
implicitly global, instance-specific configuration has no natural owner, and
the compiler cannot uniformly represent a repeated kind even when QEMU can.

**Assessment:** retain explicit at-most-one cardinality where appropriate, but
reject kind-as-instance identity. Even a singleton should be a constructed node
with explicit endpoints and one place in the plan.

### B. Arbitrarily many independent Facility instances

This provides clear isolation, independent bindings, and uniform tests, but it
duplicates Drivers, DMA Pools, service quotas, failure policy, and shutdown
work. It also invents questionable semantics: two “Monotonic Clocks” over one
counter are not independent clocks.

**Assessment:** reject unlimited generic multiplicity. Each Facility kind must
state its admissible instance range and sharing policy. Multiple logical Event
Stores over one persistence substrate, if accepted, are not multiple independent
hardware Facilities and should not duplicate the complete Driver stack.

### C. One physical service hosting logical tenants

This is useful when a device naturally multiplexes subresources or when many
clients need the same service. Examples include multiple Event Producer
endpoints on one Event Store, several Telemetry producers on one collector, and
multiple serial ports on one Virtio Console controller.

But tenants share a fault domain, scheduling budget, buffers, and often ordering.
Calling them “independent Facility instances” would make false isolation claims.

**Assessment:** use explicit exported endpoints and, only when semantically
needed, named partitions with declared capacity and failure coupling. Do not use
“tenant” as a synonym for client or Actor.

### D. Image partitions or realms

Static realms could provide separate Facility namespaces, memory protection,
fault containment, and perhaps independent restart. CAmkES and Microkit show
that such systems are feasible when protection domains are foundational.

Wrela currently has one guest address space, one compiler-planned scheduler,
one static Actor graph, one Shutdown authority, one terminal lifecycle, one
Image Result, and Panic ends the complete Image. A real realm therefore requires
new address spaces or memory-protection domains, fault-routing semantics,
cross-realm message contracts, resource partitioning, persistence rules, and
multi-result lifecycle policy.

**Assessment:** reject realms as a testing convenience. Separate Images already
provide the hard failure, reset, and host-binding boundary. A logical Actor group
may help reporting or placement, but should not be called an isolation realm.

## Implications by current Facility

| Kind | Recommended current instance cardinality | Device/binding relationship | Client/construction multiplicity |
|---|---:|---|---|
| Display | `0..1` | one admitted Scanout on one Virtio-GPU device; transport support for more heads does not expand the current contract | many compiled Views and presentation snapshots are content/requests, not Display instances; presenter authority remains an explicit design choice |
| Input | `0..1` | one logical instance aggregates the admitted keyboard and mouse Virtio Input device instances | exactly one owning Actor; Action Maps are ordinary values/modules, not Facility tenants |
| Event Store (current model) | `0..1` | one instance, one Virtio-Block device, one exclusive host block binding, one persistent `Store Identity` | one store Actor and many bounded Event Producer endpoints; player profiles remain Events inside one history |
| Persistence substrate + Event Stores (candidate model) | substrate `0..1`; stores `0..N` under a compiler-proven bound | one substrate may partition one block binding into disjoint admitted extents; a planner could require more bindings later without changing Creator authority | every Store has one history, owning Actor, schema, capacity, and Producer set; the flagship declares one Store while a Test Image may declare several isolated Stores |
| Monotonic Clock | `0..1` | no Virtio device; one normalized architecture counter source | multiple authorized consumers and multiple ordinary Cadence Actors; Cadences are not clock instances |
| Entropy | `0..1` | one bounded Virtio-RNG device instance | one service may export separately budgeted endpoints to admitted non-gameplay clients; exact client cardinality remains open |
| Telemetry | `0..1` | one generic serial port binding; whether it shares one multiport Virtio Console controller with terminal control or uses a separate controller should be explicit in the manifest profile | many typed producer endpoints may feed one bounded collector/ring; they are not independent Telemetry Facilities |
| Creator Actor | not a Facility | no external binding unless given a Facility endpoint | many Actor instances of the same Actor type are ordinary; each has its own `ConstructionId`, Mailbox, state, storage, and fixed placement |

These are **recommended current constraints**, not facts imposed by QEMU. A
future multi-display Image, for example, must decide whether it has one Display
instance with multiple Scanouts or several independent Display instances. The
Virtio GPU transport permits both shapes to be represented but cannot decide
the Wrela semantics.

## Recommended planning model

Each authenticated Facility kind should define a closed schema containing:

- permitted Facility-instance cardinality for the current language/product
  version, independently from an Architecture Profile's physical admission
  limits;
- typed instance configuration and capacity declarations;
- exported endpoint kinds, rights, client cardinalities, and quotas;
- a sealed internal graph recipe for Actors, Pools, Drivers, DMA, interrupts,
  boot dependencies, failure reporting, and shutdown;
- a multiset of device requirements identified by semantic role rather than BDF;
- whether roles require dedicated devices, aggregate several devices, or occupy
  subdevices such as ports or scanouts;
- launcher binding requirements and validation; and
- the failure domain shared by clients, subdevices, and backends.

The Image Constructor should create one symbolic Facility-instance node and
receive only its typed exported endpoints. After graph sealing, the Facility
planner expands that node. Global device planning then assigns concrete device
instances, ports, queues, BDFs, MMIO, interrupts, and host-binding slots in
stable order or rejects an Architecture Profile whose finite resources cannot
host the semantically valid graph.

The resulting plan should make these relations inspectable:

```text
Facility kind
  -> constructed Facility instance
       -> exported typed endpoints -> client Actor instances
       -> sealed internal Actor/Pool/Driver instances
       -> device requirements by role
            -> manifest device or subdevice instances
                 -> launcher external bindings
```

This does not introduce runtime discovery, dynamic Actor creation, handle
collections, hotplug, a service locator, or ambient global authority. Every
node and edge remains in the one closed Image graph.

## Testing implications

The architecture should make the following test seams distinct:

1. **Kind/planner tests:** construct symbolic Facility instances, verify
   cardinality rejection, endpoint wiring, deterministic expansion, exact
   resource accounting, and manifest requirements without booting QEMU.
2. **Protocol-model tests:** exercise each Driver/Facility state machine against
   bounded inputs and compare typed observations. A model is an oracle, not a
   second deployable Facility.
3. **Binding-adapter tests:** keep the production guest-visible device while
   controlling its external backend. QEMU supplies useful primary examples:
   `blkdebug` injects block errors behind Virtio-Block, QMP injects typed keyboard
   and pointer events, and character backends can use sockets, files, or bounded
   ring buffers.
   ([QEMU `blkdebug`](https://www.qemu.org/docs/master/devel/testing/blkdebug.html),
   [QMP input](https://www.qemu.org/docs/master/interop/qemu-qmp-ref.html#command-input-send-event),
   [QEMU character backends](https://www.qemu.org/docs/master/system/invocation.html#character-device-options))
4. **Conformance Images:** boot the production Facility instance and Driver
   under the pinned QEMU configuration with ephemeral external bindings and
   assert structured results.
5. **Isolation/failure tests:** use a fresh Image launch when the scenario needs
   independent boot, Panic, power loss, Store-binding exclusivity, or device
   ownership. Distinct statically owned logical state inside one Image does not
   require a separate launch merely for test-order independence.

Multiplicity tests should follow product semantics:

- for a kind with `0..1` cardinality, requesting two Facility instances is a
  compiler or planning rejection Regression Case;
- for a Facility with many endpoints, tests should create many endpoints on one
  instance and prove their capacity and ordering rules;
- for a Facility aggregating devices, tests should vary those device instances
  behind the same logical service; and
- only a kind that promises independent Facility instances should be tested for
  cross-instance failure isolation; logical Stores sharing one persistence
  substrate promise state isolation, not independent device-failure domains.

**Inference.** Adding fake Facility tenants or realms solely to pack more cases
into one QEMU boot would change the product model to serve the harness. Multiple
Event Stores are justified only if they are honest statically constructed
logical services over a shared persistence substrate. The current evidence
strategy still supports cheap planner and model cases in Check, one minimal
production-shaped QEMU boot, and broader fault campaigns as bounded Challenges.

## Decisions still open for Wrela

1. The exact Creator-facing Build Constructor surface for naming or binding a
   Facility instance while keeping the Image Constructor ordinary Wrela.
2. Whether the term `Facility instance` belongs in the public glossary or only
   in compiler planning documents.
3. The endpoint-sharing rule for Display and the exact client cardinality for
   Entropy and Telemetry.
4. Whether mandatory terminal control and Telemetry are two ports on one
   Virtio Console controller or two dedicated controller instances in the first
   Device Manifest profile.
5. The Device Manifest representation of controllers versus subdevices such as
   Virtio serial ports and GPU scanouts.
6. Whether Event Store remains the zero-or-one machine Facility, or becomes a
   zero-or-many logical service over a zero-or-one persistence substrate.
7. If Event Stores become plural, how stable Store Identity, Event Schema Locks,
   capacity, batching, and failure coupling compose without introducing ambient
   lookup or test-only semantics.
8. Whether any future Facility kind genuinely needs more than one independent
   machine-backed instance inside one Image. No current-version requirement
   demonstrates this.

The first load-bearing choice is modest: **model Facility instances, endpoints,
and bindings separately now, and give every kind an explicit cardinality.** The
second is not modest: **decide whether Event Store is itself that machine-backed
Facility or a repeatable logical service over one persistence substrate.** The
test architecture has made that unresolved domain seam visible.
