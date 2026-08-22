# Audio authoring strategy for Wrela

Status: shelved research, not current design. Audio is outside the current Wrela
version and deferred indefinitely. None of this note's terminology or
recommendations is accepted; any future Audio work begins as a fresh design
branch.

Research date: 2026-08-21.

## Historical recommendation (not adopted)

This investigation recommended that a future Wrela Audio design should **not
make field-based audio a foundational constraint**. It proposed a hybrid, typed
authoring model that orchestrates both immutable
recorded `Sample`s and compiled procedural generators. The Creator-facing
language can be ordinary Wrela standard-library declarations, but those
declarations should lower through an authenticated Audio Facility planner to a
closed, compiler-owned **Audio Plan**.

The first useful model has three distinct layers:

1. **Content:** immutable `Sample`s plus a small library of procedural sources.
2. **Meaning and orchestration:** typed cues, scores, parameters, transitions,
   envelopes, variation, spatial placement, and finite voice policies.
3. **Signal execution:** a fixed routing/DSP graph of sources, buses, effects,
   and one stereo master, evaluated in bounded 48 kHz blocks.

This separation matters. A language that exposes only oscillators and filters
does not by itself make game audio sound good. High-quality voices, performed
music, environmental recordings, and many authored effects start with recorded
material; the authoring system then makes that material responsive to gameplay.
Procedural synthesis remains valuable for UI tones, stylized instruments,
ambience, variation, and effects that respond continuously to game parameters.

### Direct answers

- **Must `Sample` be present on day one?** Yes, for the first flagship-quality
  audio slice. This need not imply a filesystem, streaming asset API, dependency
  system, or general codec framework. The compiler can accept one explicitly
  declared source format, validate and resample it at build time, and embed
  canonical PCM in the Image. Runtime compression and streaming can wait for
  measured Image-size pressure.
- **Is a dedicated authoring language warranted?** A dedicated *domain model*
  is warranted; a second parser or general-purpose language is not. Start with
  typed Wrela combinators and declarations, then let the future graphical editor
  generate the same declarations.
- **Is an Audio IR warranted?** Yes, if kept narrow. A closed Audio Plan is the
  representation that makes graph validation, fixed memory, latency, voice
  admission, rate analysis, buffer reuse, and DSP lowering compiler-visible.
  It is not a second runtime, plugin ABI, or dynamic patching system.
- **Is field-based audio useful?** Potentially later for acoustic propagation,
  but not as the source, score, mixer, or initial spatialization model. Treat it
  as an optional specialized planner that supplies attenuation, occlusion,
  diffraction, and reverberation parameters to ordinary voices.

## What established systems actually expose

The mature systems surveyed are hybrids. They do not ask a game to choose
between “samples” and “procedural audio,” nor do they represent all authoring as
arbitrary per-sample code.

### Gameplay events and orchestration

FMOD Studio makes an event the game-triggered unit. An event contains tracks,
instruments, action/parameter/timeline sheets, parameter-controlled properties,
and routing into a project mixer. Its tracks form local submixes and effect
chains; parameters can drive automation and instrument conditions.
([FMOD Studio concepts](https://www.fmod.com/docs/2.03/studio/fmod-studio-concepts.html),
[FMOD parameters](https://www.fmod.com/docs/2.03/studio/parameters.html))

Wwise similarly defines author-created Events as actions over authored sound
objects, while game objects carry positions, orientations, parameters, states,
switches, and environmental properties.
([Wwise Events](https://www.audiokinetic.com/en/public-library/2024.1.7_8863/?id=concept_events.html&source=SDK),
[Wwise game objects](https://www.audiokinetic.com/library/2024.1.7_8863/?id=what_are_game_objects&source=WwiseFundamentalApproach))

**Inference for Wrela:** gameplay should send typed, finite cue commands such as
“start this cue with these bounded parameters,” not manipulate decoders,
oscillators, or mixer nodes directly. Cue parameter types and every possible
route remain known when the Image is planned.

### Recorded and procedural sources coexist

FMOD banks contain both event metadata and referenced sample data. Its
instruments trigger audio content and event behavior.
([FMOD banks](https://www.fmod.com/docs/2.03/studio/getting-events-into-your-game.html),
[FMOD instruments](https://www.fmod.com/docs/2.03/studio/working-with-instruments.html))

Unreal MetaSounds is a procedural DSP graph, but its standard Wave Player is a
first-class node with seeking, looping, cue points, pitch modulation, and
sample-accurate concatenation. Epic explicitly describes mixing recorded and
synthetic sources in the same system.
([MetaSounds overview](https://dev.epicgames.com/documentation/unreal-engine/metasounds-the-next-generation-sound-sources-in-unreal-engine?lang=en-US))

Even code-first ChucK combines oscillators, envelopes, filters, delays, and
noise with `SndBuf` sample playback.
([ChucK basic unit generators](https://chuck.cs.princeton.edu/doc/reference/ugens-basic.html))

**Inference for Wrela:** deferring `Sample` while attempting polished game audio
would force the procedural system to reproduce vocals, performance, and
recorded timbre before Wrela has basic orchestration. That is the wrong risk
order. A minimal immutable `Sample` feature is smaller and more enabling than a
large synthesis library.

### Static graphs are fertile compiler input

MetaSound graphs are sample-accurate DSP flow graphs. Epic says each graph is
converted to an optimized static C++ object, avoiding interpreted bytecode,
virtual dispatch, and data copies.
([MetaSounds overview](https://dev.epicgames.com/documentation/unreal-engine/metasounds-the-next-generation-sound-sources-in-unreal-engine?lang=en-US),
[MetaSounds reference](https://dev.epicgames.com/documentation/en-us/unreal-engine/metasounds-reference-guide-in-unreal-engine))

Faust treats DSP as an algebra of signal-processor block diagrams. Its compiler
fully compiles those specifications, emits self-contained sample-level code,
and emphasizes deterministic behavior and constant memory footprint.
([Faust introduction](https://faustdoc.grame.fr/manual/introduction/))

SuperCollider's compiled `SynthDef` format is a list of unit generators and
connections with explicit scalar, control, and audio calculation rates. It
forbids direct graph cycles and recommends topological ordering that allows
connection-buffer reuse.
([SuperCollider Synth Definition format](https://doc.sccode.org/Reference/Synth-Definition-File-Format.html),
[unit generators and Synths](https://doc.sccode.org/Guides/UGens-and-Synths.html))

The Web Audio specification likewise renders an audio graph in fixed-size
sample-frame quanta; the default render quantum is 128 frames.
([Web Audio API 1.1, graph rendering](https://www.w3.org/TR/webaudio-1.1/#rendering-loop))

**Inference for Wrela:** an Audio Plan gives the compiler materially better
facts than an opaque “produce the next sample block” callback. It can classify
nodes by event, control, or audio rate; topologically order them; constant-fold
parameters; fuse kernels; reuse scratch buffers; plan delay state; vectorize
straight-line DSP; and produce a fixed worst-case memory and work report before
the Image boots.

### Voice bounds are product behavior, not an implementation detail

FMOD exposes per-instrument polyphony, event/bus instance limits, and explicit
stealing or virtualization behavior. Wwise also uses playback limits,
priorities, and virtual voices to prevent inaudible sounds from consuming
physical-voice processing.
([FMOD instrument polyphony](https://www.fmod.com/docs/2.03/studio/instrument-reference.html),
[FMOD stealing and virtualization](https://www.fmod.com/docs/2.03/studio/advanced-topics.html),
[Wwise virtual voices](https://www.audiokinetic.com/en/library/edge/?id=concept_virtualvoices.html&source=SDK))

**Inference for Wrela:** every cue and bus should declare a fixed concurrency
bound and a deterministic overflow policy. The whole Image should reserve the
voice states and DSP memory. A stable policy—reject the new voice, replace the
oldest, or replace the lowest statically ordered priority—fits Wrela better than
an implicit host-dependent heuristic. “Quietest” and “furthest” can be added
only after their numeric and ordering semantics are fixed.

### Mixing and spatialization are ordinary signal operations

FMOD routes events through group/return/master buses, effect chains, and mix
snapshots. Its standard spatializer pans by listener-relative angle and
attenuates by distance.
([FMOD mixing](https://www.fmod.com/docs/2.03/studio/mixing.html),
[FMOD spatialization](https://www.fmod.com/docs/2.03/studio/advanced-topics.html#spatialization-options))

XAudio2's graph uses source, submix, and mastering voices; voices apply gain,
effects, channel matrices, decoding, sample-rate conversion, and final mixing.
([XAudio2 voices](https://learn.microsoft.com/en-us/windows/win32/xaudio2/xaudio2-voices),
[XAudio2 audio graph](https://learn.microsoft.com/en-us/windows/win32/xaudio2/xaudio2-audio-graph))

**Inference for Wrela:** the initial stereo spatial model can be typed emitter
position plus listener position/orientation, a declared attenuation curve, and
deterministic stereo panning. This is useful for games without importing wave
propagation, HRTFs, room geometry, or a general effect-plugin ABI.

### Timing and score deserve first-class domain concepts

ChucK makes time and duration native and advances a precise logical `now` in a
strongly timed concurrent model. SuperCollider separates compiled synthesis
graphs from higher-level Patterns that describe and execute musical sequences.
([ChucK time](https://chuck.cs.princeton.edu/doc/language/time.html),
[SuperCollider Pattern guide](https://doc.sccode.org/Tutorials/A-Practical-Guide/PG_01_Introduction.html))

**Inference for Wrela:** do not tie musical timing to the Console's gameplay
Cadence. The Audio Plan needs its own bounded sample timeline and typed musical
units (frames, duration, beat, tempo), while gameplay supplies timestamped or
next-boundary commands through a finite queue. Scores should be finite or have
statically bounded repeating structure.

## Why not field-based audio first

“Field-based audio” can mean two very different things:

1. treating a waveform as a continuous function of time; or
2. simulating the acoustic pressure field through a spatial world.

The first adds little. Audio DSP already has the more useful abstraction: a
causal signal graph with state, delay, multiple calculation rates, and fixed
block evaluation. Unlike a visual Form, a filter or reverberator cannot in
general be treated as a pure value at an independent coordinate because its
output depends on prior samples.

The second is real, but it is a specialized propagation problem. Microsoft
Research's Project Triton models wave effects such as diffraction and
reverberation, but describes full wave simulation as very expensive. Its
production approach precomputes static scene geometry in a bake and compresses
the result for runtime lookup and signal processing.
([Project Triton](https://www.microsoft.com/en-us/research/project/project-triton/))

Research on dynamic scenes further separates a game thread, a wave-simulation
thread updating at 10 Hz, and an audio thread at 44.1 kHz; the paper notes that
real-time 3D wave approaches require significant resources and demonstrates a
2D single-core approximation instead.
([Interactive sound propagation for dynamic scenes using 2D wave simulation](https://www.microsoft.com/en-us/research/wp-content/uploads/2020/08/Planeverb_CameraReady_wFonts.pdf))

**Inference for Wrela:** making spatial pressure fields the Audio foundation
would combine acoustics research, world-to-acoustic geometry, precomputation or
heavy simulation, perceptual compression, and DSP before the system can play a
footstep. It also conflicts with a strict “no baking” interpretation. This work
may later become a distinctive feature, but it should consume ordinary sources
and produce propagation parameters; it should not define what an Audio cue is.

## Proposed first Audio Plan

The initial authenticated planner should accept only a closed declaration graph:

- `Sample`: explicitly declared immutable Project content, normalized by the
  compiler to the Audio Facility format;
- `Generator`: a curated finite set of oscillators/noise, envelopes, and basic
  filters, with later custom pure DSP nodes admitted through a checked seam;
- `Cue`: a typed game-facing command with fixed parameters, sources, variation,
  and start/stop behavior;
- `Score`: finite or bounded-repeating sample-timed sequences and transitions;
- `Voice Class`: fixed capacity, priority, and deterministic overflow policy;
- `Bus`: fixed acyclic routing, gain, sends, and a finite effect chain;
- `Emitter` and `Listener`: optional typed spatial inputs for attenuation and
  stereo pan; and
- `Master`: the one bounded stereo output path, including final limiting and
  signed-16-bit conversion.

Planning should reject dynamic node creation, unbounded score generation,
runtime graph rewiring, runtime file access, unknown effects, unbounded delay,
and a voice count without admitted storage and work. It should report:

- maximum simultaneous voices and commands;
- persistent state and scratch-buffer bytes;
- maximum work per audio block and estimated headroom on the Reference Console;
- end-to-end planned latency;
- embedded Sample bytes; and
- the deterministic behavior for command overflow, voice pressure, and underrun.

The Audio Plan should lower its DSP kernels through Wrela's semantic Core IR and
then Cranelift, so ownership, bounds, effects, and deterministic rules remain
Wrela semantics. The plan should not expose Cranelift, host callbacks, or device
buffers to Creators.

## Authoring-tool minimum

Textual declarations alone are insufficient for fast sound design. MetaSounds'
official workflow includes live preview, meters, parameter controls, reusable
patches, and presets; FMOD exposes meters, a mixer, live adjustment, and
profiling.
([MetaSounds overview](https://dev.epicgames.com/documentation/unreal-engine/metasounds-the-next-generation-sound-sources-in-unreal-engine?lang=en-US),
[FMOD mixing](https://www.fmod.com/docs/2.03/studio/mixing.html))

**Inference for Wrela:** before a sophisticated graphical editor, provide a
small host-side audition loop that compiles the same Wrela declarations, renders
a deterministic excerpt, plays or exports it, and displays plan cost, peaks,
clipping, and voice usage. That keeps the Image contract small while making the
authoring language usable. A later human-and-agent graphical editor can edit the
same source model rather than introduce a second Audio format.

## Suggested scope boundary

The first milestone should prove one hybrid vertical capability—not “all game
audio”:

- embed and play a mono or stereo Sample;
- define one typed parameterized cue that layers a Sample and procedural tone;
- sequence it sample-accurately;
- route it through gain, pan, one filter, and the stereo master;
- enforce a small deterministic voice budget; and
- audition the identical Audio Plan on the host and in a QEMU Image.

Defer streaming, compressed runtime decoding, arbitrary creator DSP callbacks,
dynamic patching, HRTF/binaural output, convolution reverb, acoustic wave
simulation, plugin loading, recording, MIDI, and a full graphical workstation
until concrete flagship content demonstrates which of them is necessary.
