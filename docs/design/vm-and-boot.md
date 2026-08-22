# VM and Boot

Status: accepted architecture through Image packaging, QEMU launch, architecture boot, static memory, restricted Virtio-PCI transport, DMA ownership, and terminal lifecycle. The current version implements and checks only AArch64; x86-64 is a preserved later design. Exact numeric limits and individual Image Facility protocols remain measured implementation specifications.

## Image and launcher

The public build artifact is one versioned `.wrela-image` container with a minimal canonical binary encoding. Its fixed header and sorted member table record the format version, member kinds, offsets, lengths, and cryptographic digests. It contains the selected Architecture Profile, Device Manifest, exact admitted RAM size and Logical Image Layout, a bootable architecture ELF, and any architecture-owned reset artifact required by that profile. The first format is uncompressed; archive paths, timestamps, host metadata, optional encodings, and equivalent byte representations are forbidden.

Every member is hashed and one digest identifies the complete canonical Image. Mandatory distribution signatures are not part of the first format. The current developer checkout makes no external-tool authentication claim, while a later publishing design may ship an authenticated toolchain and sign Image digests without changing guest semantics. The canonical Evidence Bundle is a separate host-side artifact keyed by the complete Image digest.

ELF is a host-side link and load envelope, not the Wrela Image abstraction. The Wrela launcher validates the container and supplies its members to a locally installed unmodified QEMU. Once QEMU has populated guest memory, the guest executes the VM ABI layout directly and never parses ELF or the `.wrela-image` container.

The Architecture Profile fixes the supported QEMU release, versioned machine model, device order, virtual CPU count, exact RAM size, CPU-feature baseline, and canonical launch flags. The current developer launcher resolves `qemu-system-aarch64` from `PATH` when launching, records its resolved path and reported version, and rejects an absent or incompatible command. It does not accept an unversioned machine alias, change RAM, or add devices outside the Device Manifest. Exact QEMU binary authentication belongs to later toolchain distribution.

HVF, KVM, and TCG are launcher execution mechanisms rather than Image semantics. An Image is identical across compatible accelerators for its Architecture Profile. The launcher may use a host CPU model when required by hardware acceleration, but sealed boot validates the fixed required baseline and optional host features cannot change generated code or become Creator-visible.

## AArch64 profile

The first Architecture Profile uses a versioned QEMU Arm `virt` machine and boots a self-contained AArch64 ELF through QEMU's `-kernel` ELF path. QEMU loads its segments and enters its ELF entry as bare-metal code without UEFI, a BIOS, or a Wrela-specific QEMU patch. A raw `-kernel` image and its Linux boot convention are not used.

The launch configuration disables secure and nested-virtualization modes unless a later profile requires them, selects the admitted GIC version, and disables DTB randomness. QEMU's generated DTB area at the RAM base is reserved so load segments cannot overlap it, but the guest does not parse the DTB: memory, CPUs, and devices come from the pinned Architecture Profile and Device Manifest.

Secondary AArch64 CPUs begin powered off. A sealed architecture stub starts them with the QEMU/HVF-provided PSCI HVC `CPU_ON` operation and normalizes their entry into the VM ABI. PSCI remains hidden below authenticated runtime modules and is not a Creator Facility or general firmware interface.

The AArch64 code generator always uses its fixed profile baseline even when HVF requires QEMU to expose the host CPU. The same Image may run under AArch64 HVF, KVM, or TCG when the launcher validates that the accelerator satisfies the profile.

## Later x86-64 profile

The later x86-64 Architecture Profile uses a versioned QEMU `pc-q35` machine so the Console retains its PCI Virtio device model. QEMU's generic loader places the self-contained x86-64 ELF without overriding the architectural reset vector.

A tiny authenticated Wrela-owned reset ROM is supplied through QEMU's `-bios` slot. It begins at the x86 reset vector, establishes protected and long mode, installs the initial paging and descriptor state, and jumps to the fixed Wrela entry. It is not UEFI, SeaBIOS, a general BIOS, Multiboot, PVH, or an external firmware dependency. The ELF contains the admitted low-memory application-processor trampoline, and the runtime starts secondary CPUs through the normal LAPIC SIPI protocol.

The x86 ELF and reset ROM are members of the same `.wrela-image` container. On an Apple Silicon development host, this profile may run under TCG rather than HVF; that affects speed, not the Image or VM ABI.

## Boot Contract

Architecture reset and secondary-core stubs normalize their native entry state into one sealed Boot Contract before entering authenticated runtime Wrela. It identifies the current core and its primary or secondary role, total admitted core count, Image layout root, per-core storage, Device Manifest, and VM ABI communication state. Raw reset registers, DTB contents, PSCI arguments, x86 reset state, and loader conventions never cross this seam.

The launcher and Image require exact VM ABI equality. There is no boot negotiation, forward-compatibility probe, or fallback path. A mismatch produces `BootFailed` before any Actor or Facility becomes running.

## Memory and admission

The closed ImagePlan determines one exact guest RAM size and complete bounded layout for code, immutable Data, Pools, Mailboxes, async frames, Display buffers, Driver state, scheduler structures, stacks, boot reservations, and architecture-required low memory. The Architecture Profile states the permitted range, but the launcher cannot add opportunistic RAM and Creator code cannot discover or allocate unplanned memory at runtime.

Every launch member, load segment, reserved region, and entry point is checked against that layout before packaging. A missing or overlapping member, unsupported CPU baseline, mismatched QEMU identity, unexpected device, or different launch configuration fails before the Image becomes running.

The first virtual-address scheme is a fixed identity mapping for admitted RAM and MMIO, with deliberate unmapped null and guard regions. Compiler-generated page tables map executable code RX, immutable Data RO and NX, mutable state RW and NX, and device MMIO RW and NX. Only authenticated Driver primitives can materialize or use MMIO addresses in valid code. Wrela does not initially implement a higher-half mapping, runtime address-space randomization, demand paging, or a dynamic virtual-memory manager.

Every core receives fixed normal and interrupt stacks. The compiler derives their maxima from final Cranelift frame sizes, the complete bounded call graph, finite callable families, recursion measures, architecture entry frames, and admitted interrupt nesting. Guard pages surround stacks; stack growth and fault-driven recovery do not exist.

Sealed boot explicitly establishes the canonical initial contents of all mutable regions, Pools, stacks, scheduler structures, and buffers before runtime Wrela may observe them. Immutable segments are loaded and validated separately. Correct initialization does not depend on incidental QEMU RAM contents or accelerator behavior.

## PCI and Virtio transport

Both Architecture Profiles expose one flat restricted modern Virtio-PCI topology. Each profile defines a finite table of usable root-bus slots, MMIO windows, and interrupt routes. The compiler assigns selected devices to those resources in stable order and records the exact BDF, BAR allocation, and route in the Device Manifest. Creator source and QEMU command order never allocate physical resources.

The launcher disables QEMU default and user-added devices, attaches every admitted device directly to the root PCIe bus at its manifest BDF, disables legacy Virtio, and initially disables MSI-X. Root ports, bridges, secondary buses, hotplug, general bus enumeration, legacy interfaces, packed queues, IOMMU, and SR-IOV are absent.

The Device Manifest replaces PCI discovery but not PCI configuration. Sealed boot accesses only each declared function, verifies the expected Virtio vendor and modern device identity, sizes and validates its BARs, assigns manifest-planned non-overlapping MMIO ranges, and enables PCI memory access and bus mastering. It walks a bounded capability list to locate the modern Virtio common, notification, ISR, and device-specific regions and rejects malformed, missing, duplicated, cyclic, out-of-range, or unsupported capabilities. Drivers do not assume capability order, BAR selection, register offset, or notification multiplier.

Every device requires `VIRTIO_F_VERSION_1` and requests one compiler-selected fixed feature set. Extra offered bits are ignored and a missing required bit produces `BootFailed`; offered features cannot create runtime behavioral variants or change memory and cost planning. The initial profiles use split queues and legacy INTx. Reading the Virtio ISR capability identifies and deasserts an event; sealed architecture adapters establish the fixed Arm GIC or x86 IOAPIC route declared by the Architecture Profile. MSI-X remains a later profile change rather than a runtime negotiation.

When several devices share one INTx line, its sealed interrupt adapter inspects every manifest device on that route in stable manifest order, reads each Virtio ISR capability, records a bounded set of causes, and schedules the corresponding Driver services. Device arrival timing cannot reorder service arbitrarily.

Interrupt and completion service obey compiler-planned quotas. Repeated interrupts without valid bounded progress cause the Driver to mask or otherwise contain the source, validate all device-controlled completions as Untrusted, and execute its bounded reset policy. A contained condition becomes a Driver Error with restored ownership; an uncontainable state or violated runtime invariant causes terminal failure or Panic according to the existing Driver contract. Interrupt traffic never acquires unlimited scheduler time.

Facility planners reserve fixed DMA Pools for descriptor tables, available and used rings, request records, and device-visible payload buffers. Their physical ranges, alignments, queue depths, maximum in-flight ownership, and interrupt servicing costs participate in Image admission. Ordinary Creator Pool storage cannot be handed to a device. A sealed protocol moves ownership from runtime to Driver to device and back, and device-written control values return wrapped as Untrusted until validated. The first profiles have no IOMMU or runtime DMA allocator.

## Terminal channel

Terminal control is mandatory sealed VM ABI infrastructure rather than a selectable Image Facility. It is initialized before other devices and never exposed to Creator code. Its generic non-console `virtserialport`, queues, DMA buffers, and preallocated frames have dedicated statically admitted capacity that cannot be consumed by console output, Telemetry, or Creator messages.

The VM ABI transports canonical bounded terminal frames over that port. A frame is an exact Wire Layout containing protocol version, Image and VM ABI identities, lifecycle or result kind, sequence, payload length, integrity field, and a fixed-capacity typed payload with canonical unused bytes. It never contains a native struct, pointer, raw memory dump, unbounded text, or host path.

A separate `pvpanic-pci` device supplies the terminal QEMU event after the record attempt: its panic event marks Panic, and its guest-shutdown event marks controlled completion, Shutdown, or `BootFailed` according to the launcher's observed lifecycle state. Console text, semihosting, ISA debug-exit devices, firmware interfaces, and parsed process output are not semantic result channels.

After sealed boot has validated memory, cores, terminal transport, and the remaining prerequisites required to cross into running, it sends a typed `Ready` frame and waits for a bounded launcher acknowledgment. Selectable Facilities and Actors cannot become running before that ACK. A recoverable failure before `Ready` attempts a `BootFailed` frame and emits the guest-shutdown pulse; the launcher classifies any pre-Ready shutdown as `BootFailed` even if no payload arrived. A violated boot invariant instead emits the panic pulse and remains Panic.

Controlled completion and structured Shutdown send their final typed frame and wait for a bounded valid launcher ACK before emitting the guest-shutdown pulse. Missing, invalid, or late acknowledgment converts the guest outcome to `ShutdownFailed`; the terminal protocol never asks the host to issue QMP `quit` and relabel a host action as guest completion.

Every ACK is Untrusted input and must echo the terminal protocol version, Image identity, frame sequence, frame kind, and integrity value. The Architecture Profile fixes finite queue-poll and retry budgets for `Ready` and controlled-finalization handshakes; guest progress never waits on a host wall-clock timeout or waits indefinitely.

If the final controlled frame is not validly acknowledged, the guest may make one bounded best-effort attempt to submit a corrected `ShutdownFailed` frame and then emits the guest-shutdown pulse. The launcher knows whether it acknowledged the original frame and therefore reports `ShutdownFailed` even when the correction is lost. Reporting failure cannot recursively start another reporting protocol.

Panic uses a preallocated frame and one bounded best-effort submission that performs no allocation, cleanup, scheduler wait, or acknowledgment wait. It then emits the panic pulse regardless of whether the payload reached the launcher. A pulse without a frame remains a valid bounded Panic observation.

One sealed terminal latch owns lifecycle finalization. The single Shutdown authority may request controlled termination, while Panic preempts any state that has not yet emitted a terminal pulse. Once a terminal pulse has been emitted, subsequent completion, Shutdown, failure, or Panic attempts have no effect.

Every core has one preallocated Panic slot. On a terminal defect, a sealed coordinator issues bounded architecture stop interrupts, scans completed slots by logical execution stamp and then core identity, selects one primary diagnostic, and includes a bounded set of other observed sites or nonresponsive cores as secondary evidence. Host arrival timing does not choose among concurrent observed Panics.

The typed record and terminal pulse have different jobs. The serial record carries the Image identity, VM ABI identity, result kind, and bounded diagnostic payload; the pulse makes termination observable through QEMU's control protocol even when a Panic prevents a complete record. Exact numeric capacities, integrity algorithm, retry counts, and timeout values remain implementation-profile decisions.
