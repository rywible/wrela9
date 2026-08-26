# Check and Challenges

Status: accepted technical release authority and development-loop contract.

## Authority

Check is the sole technical authority for merge and release. It is one fast deterministic command: `Passed` means the revision is Releasable, `Failed` means a Release Obligation is violated, and `Unable` means no judgment was produced. There are no separate edit, merge, CI, full, or release checks.

Releasable means that Check establishes every currently encoded Release Obligation for the revision. It does not mean that no undiscovered defect or unencoded obligation exists, and it does not automatically approve a product or design choice. Running Challenges cannot strengthen, replace, or override this status.

Main is kept Releasable. An implementation is not declared complete until Check passes, and releases are built from an exact passing revision of `main` without a second technical qualification ceremony. Packaging and later distribution may produce artifacts from that revision but cannot introduce another definition of correctness.

## Check envelope

Check always selects the same evidence. Correct incremental computation may make repeated execution faster, while missing external prerequisites may make it `Unable`; neither condition changes the release claim. Initial tool installation and host-toolchain compilation are setup cost rather than a separate Check mode.

The Reference Development Host is the builders' Apple Silicon Mac profile, identified by model and memory class. A representative warm local change targets subsecond Check Latency and may not exceed five seconds on that host. Other hosts receive the same semantic result but do not become latency authorities.

The Check envelope includes every deterministic Regression Case and one minimal production-shaped path through compilation, Image packaging, QEMU boot, typed terminal observation, and shutdown. When Check approaches its latency ceiling, evidence must be made deeper or cheaper. It is not split into another tier.

## Challenge discipline

A Challenge is a bounded exploratory instrument, not a gate. Before running one, a human or agent states the concrete question it is meant to answer. A Challenge may run only when the user requests it, the task creates or changes that Challenge, or an observed failure or suspected weakness supplies a concrete investigative question. Extra confidence is not a sufficient reason.

Each named Challenge targets about thirty seconds and terminates within sixty seconds on the Reference Development Host. Ordinary work has one sixty-second aggregate Challenge budget unless the user explicitly authorizes more. There is no aggregate Challenge command, scheduled Challenge requirement, CI invocation, merge hook, release invocation, or automatic discovery of Challenge names.

The canonical agent rule is:

> Run Check before declaring implementation complete. Do not run Challenges for routine confidence. Run a named Challenge only to answer a stated investigative question, within a total one-minute budget unless the user authorizes more.

## Product and performance Challenges

The complete graphical-editor authoring journey, flagship progression and persistence journey, Greenfield and Production Event Store lifecycles, Facility fault campaigns, and complete Replay runs are named Challenges. The Genshin-shaped, Pokémon-shaped, and Yu-Gi-Oh-shaped reference Images are also permanent Challenges. These programs remain important product and architecture pressure without becoming a shadow full suite.

Physical Reference Console runs, native microbenchmarks, scheduler and Driver measurements, full-frame Display workloads, profiles, fuzzers, and bounded model exploration are Performance Challenges. They calibrate cost models and seek weaknesses in deterministic Check evidence. A benchmark records measurements; it does not pass a revision.

When a Challenge exposes a reproducible violation of a Release Obligation, the Finding must enter Check before the next merge or release. Its narrow Regression Case, not the complete Challenge execution or raw output, becomes durable release evidence.
