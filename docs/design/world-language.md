# World Language

Status: accepted architecture and product design. The flagship and three permanent reference Images supply the fixed acceptance scenarios; numeric workload and performance thresholds are calibrated during implementation. Audio is outside the current version.

## Purpose

Wrela compiles complete field-rendered game Worlds for a CPU-only virtual Console. Creators author continuous spatial intent rather than imported meshes, textures, sprites, raster fonts, or conventional draw calls. The compiler must retain enough structure to admit and specialize complete presentations against a concrete Reference Console while supporting games shaped like a third-person field-world adventure, a creature RPG, or a card game.

Compiler-derived acceleration is allowed. The visual source of truth may not become a conventional baked asset.

## Public model

Wrela has intrinsic declaration kinds for:

- `form`: a symbolic continuous spatial subject.
- `world`: a finite or regionally generated arrangement of admitted Form families.
- `view`: a pure mapping from bounded presentation Data to Worlds and Screen-space Forms.
- `material`: analyzable local appearance and response to incoming light.
- `transport`: bounded movement and evaluation of light.

`Space`, `Point`, `Vector`, `Transform`, `Projection`, `Embedding`, `Region`, `Form`, `World`, and `View` have compiler-known spatial meaning. Physical and presentation quantities distinguish length, angle, duration, update count, and pixels. Color types distinguish linear light, display encoding, perceptual comparison, emission, opacity, and palette indices.

Initial Spaces cover Euclidean 2D and 3D, Screen 2D, spherical and toroidal surfaces, and bounded grids. Forms are kinded so Solids, Surfaces, Media, Patterns, and Regions retain one composition language without pretending to share identical evaluation rules.

Characters, Creatures, Terrain, Cards, Effects, layouts, and procedural glyph families are public standard-library abstractions implemented through the same mechanisms available to Projects. They receive no secret compiler privilege.

## Form semantics and authoring

A Form semantically describes inside, outside, boundary, parts, and appearance. The compiler derives certified conservative distance information rather than requiring every composition or deformation to remain an exact signed-distance function.

Forms are compile-time first-class symbolic values. They may be passed, returned, collected in bounded symbolic structures, selected through static alternatives, and composed by generic visual code. Runtime state changes admitted parameters, transforms, articulation, deformations, Materials, and active bounded instances, but never creates a new structural alternative.

Restricted pure Wrela is legal inside visual declarations. Bounded iteration and structurally terminating recursion are accepted; effects, opaque calls, unbounded recursion, runtime structural construction, and creator-supplied trusted distance or bound claims are rejected. A sealed primitive floor preserves the algebraic facts used by the compiler, while user-defined combinators remain visible for analysis.

Creators may author source-native curves, control points, palettes, proportions, rules, literal structures, and procedural algorithms. Certified bends, twists, tapers, waves, blends, and bounded morphs support expressive animation. Imported meshes, textures, sprites, and raster fonts do not.

All composition is explicit: union, intersection, subtraction, masking, layering, priority, and smooth blending determine overlap. Materials consume explicit typed local, world, surface, screen, or named coordinates rather than an ambient position. Fine detail uses analyzable Patterns, contours, displacement, normal perturbation, and analytic noise.

Each gameplay subject may carry one Image-wide Mark across Worlds and Views. Its Form supplies typed local part labels. Gameplay Bodies and Regions remain explicit authoritative geometry, may share dimensions and poses with Forms, and never inherit compiler-selected visual detail.

## Worlds, Views, and state

A World may be regionally infinite when a pure deterministic generator maps an explicit seed and stable regional key to bounded local structure. Persistent changes are bounded inputs supplied from authoritative game state, not hidden renderer mutations. Procedural randomness derives only from explicit seeds and stable structural paths.

An Image may contain a finite set of named Worlds and Views for overworlds, battles, boards, menus, cinematics, split views, portals, and transitions. A View is the only bridge from game state into presentation: an owning Actor explicitly pushes a bounded read-only snapshot to Display. Forms cannot read Actors, input, clocks, entropy, or Event Store state directly.

Display retains disposable spatial, lighting, reconstruction, and temporal caches. Losing those caches may change an approximation only within the same deterministic Visual Contract; it can never change authoritative game state. Gameplay updates and presentation frames use independent deterministic fixed rates, with typed creator-controlled interpolation for continuous values and explicit treatment of discrete transitions.

Screen interfaces use first-class Screen 2D Forms. Procedural glyph Forms, exact text content, card layouts, menus, and HUDs do not pass through a separate raster UI renderer.

## Materials, light, and media

Materials describe bounded local response to incoming light. Transport controls global propagation through compiler-known operations for direct lights, shadows, finite diffuse bounces, ambient occlusion, reflection, refraction, portals, sampling, deterministic temporal reuse, and reconstruction. Creators compose bounded Transport graphs; arbitrary recursive integrators are not legal.

Transparent Surfaces and participating Media are supported through statically bounded layer, distance, intersection, and integration work. Secondary views have fixed recursion and coverage budgets.

The compiler may derive light and caster indexes, influence regions, visibility and transmittance bounds, sampling distributions, symbolic reductions, work schedules, and certified Transport operators that continue to consume live Material and light inputs. It may not store solved appearance such as lightmaps, sampled static radiance, or pre-rendered shadows in the Image. Runtime evaluated caches are allowed because they are disposable.

## Contracts, compilation, and execution

Visual priority classes such as exact, primary, secondary, and decorative carry Console-defined obligations for semantic preservation, silhouette and color error, temporal age, and permitted simplification. Exact presentation preserves content, topology, identity, ordering, and required layout; it does not require cross-architecture bit-identical edge samples.

The compiler normally infers value frequency, numeric bounds, population capacities, mutual exclusion, and presentation cost from ordinary types and control flow. Checked declarations may constrain frequency or ranges without turning proof success into a different source-visible value type. Bounded presentation Groups express relationships that path analysis cannot recover without exposing instruction-cycle accounting to Creators.

The compiler may select certified execution plans by game state, visibility, projected size, quantized camera state, and Visual Contract. It may use scalar or analytic evaluation, conservative stepping, spatial hierarchies, procedural regional expansion, SIMD, multicore tiling, mixed precision, deterministic detail representations, reconstruction, and bounded cache reuse. It may never adapt to measured wall-clock time.

Admission uses an explainable static cost model calibrated against a concrete minimum supported Apple Silicon Reference Console, VM core count, memory configuration, and instruction baseline. Cost reports attribute work to Worlds, Views, Form families, Transport operations, visibility cases, and presentation Groups. The current Display contract is one fixed 1280×720, 60 Hz, BGRA8-sRGB scanout; host scaling and fullscreen are cosmetic and invisible to the Image. Display submits only complete frames, and a missed real deadline preserves the previous complete frame while recording an environmental performance fault. A later 1920×1080 at 60 Hz profile is the next Display profile.

Gameplay and pure regional generation remain cross-target deterministic. Rendered pixels need only remain deterministic for a pinned compiler and target and satisfy the same Visual Contract across targets.

## Tooling and product pressure

The compiler and tools expose Form and View preview, bounds, spatial partitions, priority and detail choices, dominant costs, and scalar-versus-native comparison. This semantic inspection and source mapping foundation begins with the compiler. A sophisticated graphical editor for human-and-agent collaboration is part of the current version and grows alongside the flagship as structured operations stabilize; Wrela source remains canonical and the editor never creates a second scene format.

Development is breadth-first through the final architecture. The implementation keeps an end-to-end Image running without introducing a disposable visual interface, fallback raster renderer, or narrow prototype that silently defines the architecture. Unsupported capabilities fail explicitly while each subsystem deepens in passes. Every new visual operator receives reference semantics, cost semantics, diagnostics, and a named native Performance Challenge when introduced; that Challenge runs only for a concrete investigation.

One deliberately scoped but shippable third-person field-world adventure is the flagship game. Three smaller permanent conformance programs provide independent pressure:

- A Genshin-shaped scene stresses a continuous 3D World, articulated character, terrain, lighting, and effects.
- A Pokémon-shaped program stresses regional generation, reusable creatures, overworld-to-battle transitions, and menus.
- A Yu-Gi-Oh-shaped program stresses exact text, cards, dense Screen layouts, nested Form illustrations, and dramatic effects.

Language and compiler changes must preserve the reference programs' accepted semantic and cost obligations through narrow Check evidence unless an explicit design decision revises those contracts. Complete reference runs remain Challenges. The flagship proves the complete product; the references prevent the language from overfitting to it.
