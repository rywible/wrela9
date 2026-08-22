# Wrela 9 backend and proof strategy

Status: historical research input, not the accepted design. Later decisions in
[`docs/design/compiler.md`](../design/compiler.md) replace the proposed custom
image linker with Cranelift ELF objects plus pinned LLD, and the project has
removed Lean entirely. The evidence and risk analysis below remain useful, but
its recommendations do not override accepted design or ADRs.

Research date: 2026-08-20. Wrela 8 evidence is pinned to preservation commit
[`40d1d9d`](https://github.com/rywible/wrela8/tree/40d1d9dff38c6c1dde527a9873108bfaeb8c775d).

## Recommendation

1. **Adopt Cranelift for ordinary native code generation on x86-64 and
   AArch64.** This directly removes Wrela 8's one-ISA development trap. Treat
   x86-64 and AArch64 as two Console target profiles with the same virtual
   devices and source semantics. Cranelift cross-compilation does not make an
   AArch64 guest executable by an x86 KVM host; the VMM still needs a guest ISA
   the host can virtualize.
2. **Do not outsource the Image to Cranelift.** Use
   `cranelift-codegen` as a library to produce per-function bytes and
   relocations, then retain a small Wrela-owned image planner/linker for the
   Boot Contract, fixed virtual-device layout, sections, entry stubs, and final
   evidence. Cranelift's object layer emits relocatable `.o` files, not a final
   bootable image.
3. **Retain Lean, but reduce and delay its scope.** Freeze the Wrela 8 formal
   project as reference material. The first Wrela 9 opaque-frame milestone
   should not be blocked on porting 3,873 lines of proofs. When certified
   rendering returns, port a compact proof kernel for interval containment,
   root/certificate soundness, ordering, coverage/error bounds, and exact
   display-byte admission. Do not port theorem count as a goal.
4. **Keep claims precise.** Lean should prove the generic certificate verifier's
   mathematics. Rust should construct concrete records; guest code should
   decode and check them; differential/golden tests should establish that the
   implementations correspond. Until the decoder and checker are themselves
   connected to Lean, do not claim an end-to-end verified renderer or image.

## 1. Cranelift feasibility

### What it supports

Cranelift is a low-level, non-WebAssembly-specific code generator. Its official
status page lists x86-64, AArch64, RISC-V 64, and s390x backends; x86-64 and
AArch64 both have SIMD support. It is explicitly usable by non-Wasm languages,
though its APIs are not yet considered stable. The same page describes code
quality competitive with optimizing browser JITs and much faster compilation
than the LLVM comparison it cites; those numbers are encouraging, not a Wrela
performance guarantee. ([Cranelift README](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/README.md))

The code generator is a cross-compiler selected from a target triple at run
time. `Context::compile` returns unrelocated machine code; external relocations
remain available from the compiled buffer. This is the narrow API Wrela needs.
([ISA construction](https://docs.rs/cranelift-codegen/0.134.3/cranelift_codegen/isa/index.html),
[`Context::compile`](https://docs.rs/cranelift-codegen/0.134.3/cranelift_codegen/struct.Context.html#method.compile))

Cranelift offers both JIT and AOT-shaped integrations. `cranelift-jit` places
code in host memory, while `cranelift-object` implements `Module` by emitting
relocatable object files. `ObjectModule::finish` still yields an object, not a
Image. Wrela should use direct codegen for production and optionally
emit an object as a debugging/interoperability artifact.
([crate map](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/index.md),
[`cranelift_object`](https://docs.rs/cranelift-object/0.134.3/cranelift_object/))

### Freestanding/custom-OS conclusion

**Yes: the needed AOT/freestanding setup is feasible, with qualifications.**
Cranelift runs inside the host compiler; it does not place Wasmtime, an OS, or
a standard library in generated code. It may, however, introduce a finite set
of runtime libcalls when an operation has no direct expansion: floating-point
rounding/FMA, `memcpy`/`memset`/`memmove`/`memcmp`, stack probing, and an x86
SIMD fallback are current examples. Wrela must implement every admitted
libcall inside the Image or reject code that would require it. The
libcall set can grow when Cranelift is upgraded, so the backend receipt must
list it and the build must fail on an unknown symbol.
([`LibCall`](https://docs.rs/cranelift-codegen/0.134.3/cranelift_codegen/ir/enum.LibCall.html),
[module symbol mapping](https://docs.rs/cranelift-module/0.134.3/src/cranelift_module/lib.rs.html#37-54))

Cranelift's normal calling conventions are sufficient for compiled functions,
but they are not an arbitrary Wrela ABI mechanism. `Fast` is explicitly not
ABI-stable. Start with `SystemV` at every hand-written boundary; place reset,
interrupt/fault entry, CPU-control, and other privileged sequences in tiny
target-owned stubs. Internal calls may use `Fast` later only if the entire
image is rebuilt together and measurements justify it.
([`CallConv`](https://docs.rs/cranelift-codegen/0.134.3/cranelift_codegen/isa/enum.CallConv.html))

Wrela's sealed machine primitives also need care. Cranelift has atomics, a
sequentially consistent fence, and a compiler `sequence_point`, but it still
has an open request for first-class volatile memory flags. MMIO and privileged
instructions should therefore remain compiler-defined primitive calls to
sealed target stubs, not ordinary optimizable loads/stores or inline assembly.
Calls are memory-fence points in Cranelift's own instruction analysis.
([atomic/fence operations](https://docs.rs/cranelift-codegen/0.134.3/cranelift_codegen/ir/struct.ReplaceBuilder.html),
[volatile gap](https://github.com/bytecodealliance/wasmtime/issues/1598),
[fence classification](https://docs.rs/cranelift-codegen/0.134.3/src/cranelift_codegen/inst_predicates.rs.html#141-154))

Wrela does not need GC stack maps for Pool ownership. Cranelift nevertheless
exposes source locations, traps, frame layouts, unwind data, and user-defined
stack maps. If Wrela later needs precise managed roots, the IR producer—not
Cranelift—must spill and identify them. For now, use source locations plus
frame-pointer/unwind policy for Panic reports and record the backend's frame
layout in the image report.
([source locations](https://docs.rs/cranelift-codegen/0.134.3/cranelift_codegen/ir/struct.SourceLoc.html),
[frame layout](https://docs.rs/cranelift-codegen/0.134.3/cranelift_codegen/struct.FrameLayout.html),
[user stack maps](https://docs.rs/cranelift-codegen/0.134.3/src/cranelift_codegen/ir/user_stack_maps.rs.html#1-29))

### The deep-module seam

Cranelift must sit below Wrela semantics and above final image placement:

```text
typed source -> Semantic/Flow WIR + proof facts
             -> target-neutral Lowered WIR
             -> NativeBackend::compile(target profile)
             -> BackendArtifact
             -> Wrela image planner/linker
             -> Image + evidence report
```

`BackendArtifact` should contain only stable Wrela-owned data: function bytes,
alignment, symbolic relocations, frame size/layout, source ranges, trap sites,
required libcalls, target/feature flags, Cranelift version, and CLIF/machine-code
digests. The adapter is the only module allowed to mention Cranelift types.
The image linker owns section addresses, range checking/trampolines, boot and
primitive stubs, immutable data, capability roots, relocation application, and
the final manifest.

This corrects Wrela 8's coupling. Its normative pipeline makes MachineWir,
the AArch64 ABI, instruction selection, linker, and Cortex-A76 cost evidence
one obligation, while the implementation concentrates 14,620 lines in
`codegen.rs` and carries AArch64 words through its linked representation.
([Wrela 8 compiler contract](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/docs/language/04-compiler.md),
[`codegen.rs`](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/codegen.rs),
[`linked.rs`](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/wrela-compiler/src/linked.rs))

### Evidence Wrela can preserve

Keep semantic proof facts above the backend. Add two receipts below it:

- **Backend receipt:** normalized Lowered-WIR/CLIF digest, exact Cranelift
  version and flags, CPU feature baseline, emitted bytes, relocations, frame
  layout, source/trap ranges, and complete libcall inventory.
- **Link receipt:** deterministic symbol/section order, addresses and
  protection, non-overlap/alignment proofs, resolved relocations and ranges,
  entry/Boot Contract mapping, primitive-stub hashes, and final image digest.

Cranelift normally uses an empty control plane whose decisions are fixed; its
upstream CI also has a deterministic-codegen check. Wrela must still pin the
crate version, flags, CPU features, and input order. Because Cranelift's API is
unstable and output can legitimately change between releases, machine-byte
goldens are toolchain-versioned evidence, not permanent language semantics.
([control plane](https://docs.rs/cranelift-control/0.134.2/cranelift_control/),
[upstream deterministic check](https://github.com/bytecodealliance/wasmtime/blob/main/.github/workflows/main.yml))

Do not try to preserve Wrela 8's single-A76, per-emitted-word cost model as a
cross-target theorem. Preserve semantic operation IDs through `SourceLoc`, then
add target-specific machine-code census/performance gates after correctness.
Cranelift exposes emitted relocations, traps, source ranges, and stack maps,
but not Wrela 8's `CostRule` tag on every final instruction.
([compiled buffer metadata](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/codegen/src/machinst/buffer.rs))

### Migration risks and gates

- **Relocations remain our problem.** Direct codegen avoids a general object
  linker but the adapter/linker must support every emitted relocation kind and
  enforce branch ranges. Cranelift distinguishes near/far references; AArch64
  near calls imply roughly a +/-128 MiB range.
  ([relocations](https://docs.rs/cranelift-codegen/0.134.3/src/cranelift_codegen/binemit/mod.rs.html#19-35),
  [`RelocDistance`](https://docs.rs/cranelift-codegen/0.134.3/cranelift_codegen/enum.RelocDistance.html))
- **Trap semantics are not Wrela Panic semantics.** Cranelift documents trap
  behavior as ISA/OS-dependent. Lower checked failures to an explicit Wrela
  Panic routine; reserve hardware traps for genuine faults handled by Console
  vectors. ([CLIF traps](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/ir.md#control-flow))
- **Renderer performance is unproved.** Cranelift's SIMD support makes Pixels
  plausible on both primary ISAs, but packet intrinsics need explicit mappings
  and two-target benchmarks. Never silently scalarize an operation carrying a
  target instruction obligation.
- **Backend correctness is trusted, not proven by Wrela.** Pin patched releases,
  run Cranelift's verifier, fuzz Lowered-WIR generation, and differentially run
  the same semantic fixture corpus on both target profiles.
- **Migration gate:** boot the same minimal Console source as an x86-64 KVM
  guest and an AArch64 KVM/HVF guest; require identical semantic transcript and
  device-visible output, target-specific reproducible image digests, zero
  unknown libcalls, and complete relocation/link receipts. Keep Wrela 8's
  backend as a differential oracle until this gate passes.

## 2. What Lean proves in Wrela 8 Pixels

The formal project is substantial but bounded: 26 Lean modules, 3,873 lines,
205 theorem/lemma declarations, and a 78-row implementation correspondence
manifest. It proves generic mathematics for interval and dyadic containment,
Bernstein sign/root arguments, bounded root-isolation preservation,
projective primitive equivalences, CSG evaluation, run-certificate/root/order
consequences, fixed-q error bounds, coverage/compositing/transparency error
bounds, capacity arithmetic, normals/material bounds, exact display-byte
admission, and kinetic slack. The Wrela 8 invariant matrix assigns Lean to 8
of 14 renderer invariants; placement, snapshot schema, device submission,
host evidence/replay, and error routing are explicitly outside Lean.
([formal project](https://github.com/rywible/wrela8/tree/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/formal/pixels),
[invariant matrix](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/docs/designs/WRELA_PIXELS_INVARIANT_MATRIX.md))

There are no project-defined `axiom`, `sorry`, or `admit` declarations in the
formal sources. Wrela pins `#print axioms` output; the listed dependencies are
only Lean's standard `propext`, `Classical.choice`, and `Quot.sound` (or none).
Lean's own reference explains both that it kernel-checks proofs and that these
standard axioms are tracked; absence of `sorryAx` is the relevant completeness
signal, not literal absence of every axiom.
([Wrela axiom manifest](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/formal/pixels/EXPECTED_AXIOMS.txt),
[Lean axiom reference](https://lean-lang.org/doc/reference/latest/Axioms/))

### What is not theorem-proved

The main `renderer_trust_boundary` theorem is an honest implication over a
`RendererVerifierFacts` bundle. Its difficult concrete premises—continuity,
derivative sign, complete feature accounting, omitted-feature exclusion,
crossing/visibility correspondence, strict order, numeric error bounds, and
display containment—are supplied as fields. Lean proves that those premises
compose to the advertised root, visibility, ordering, error, display, and
kinetic conclusions. It does **not** prove that a particular Rust compiler or
guest verifier produced those fields correctly.
([trust boundary](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/formal/pixels/Pixels/TrustBoundary.lean),
[run certificate](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/formal/pixels/Pixels/RunCertificate.lean))

The 78-row `KERNELS.txt` manifest connects a theorem name, a Rust function, a
Wrela function, and differential vector families. The gate checks that the
symbols exist and that each kernel has an exact Lean alias to the named
theorem; Lean type-checks that alias. This is useful traceability, but it is not
a refinement proof from either implementation's source to the Lean model.
Concrete correspondence comes from Rust/Wrela/guest differential vectors and
goldens.
([kernel manifest](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/formal/pixels/KERNELS.txt),
[manifest validator](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/xtask/src/pixels_vectors.rs),
[formal gate](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/crates/xtask/src/pixels_formal.rs))

Other non-Lean obligations include FieldGraph lowering, FrameProgram byte
encoding/decoding, guest control flow, compiler/codegen correctness, final
Image layout, driver protocol, replay, and fail-closed routing. Wrela
8's own normative contract says exactly that Lean proves generic kernel
mathematics while Rust constructs facts and guest verifiers check records; it
explicitly rejects an end-to-end verification claim.
([Pixels proof ownership](https://github.com/rywible/wrela8/blob/40d1d9dff38c6c1dde527a9873108bfaeb8c775d/docs/language/07-pixels.md#15-quality-and-proof-ownership))

### Recommended Wrela 9 Lean scope

Retain Lean where a small local theorem justifies a fail-closed certificate:

- outward interval/dyadic containment and overflow preconditions;
- Bernstein/root-exclusion and unique-root certificate rules;
- complete event accounting plus strict front-order composition;
- coverage, compositing, transparency, and material error enclosure;
- endpoint-singleton display encoding.

Reduce or postpone broad catalogs of algebraic restatements, target capacity
arithmetic, and features not present in the first opaque renderer. More
important than restoring all 205 theorems is strengthening one bridge: define
a versioned certificate decoder and checker model in Lean, prove that `accept`
implies the renderer property, and run the Rust and Wrela implementations
against the same generated records. Keep Lean pinned and in the deep
verification lane rather than ordinary Creator builds, as Wrela 8 already
does. This preserves the genuinely valuable mathematics without making Lean a
second implementation of the entire renderer.
