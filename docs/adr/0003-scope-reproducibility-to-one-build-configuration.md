# Scope reproducibility to one build configuration

Wrela guarantees reproducible Images and Replay only for the same architecture, compiler version, VM configuration, and build inputs. Every supported architecture must pass the same language conformance suite, but Wrela does not require Images, transcripts, floating-point results, or rendered frames to match across architectures. This avoids a pairwise equivalence ledger while preserving deterministic evidence for every artifact that Creators actually run.
