# Keep Creator concurrency message-based

Creator code will not expose atomics, locks, shared mutable memory, raw interrupt handlers, or device queues. Actors, Replies, Groups, and Image Facility protocols are the only mutable concurrency boundaries; sealed runtime and Driver modules alone may use Compiler Primitives for cross-core and interrupt mechanics. This gives up low-level shared-memory escape hatches so whole-Image ownership, effect ordering, cancellation, Replay, and service analysis remain tractable.
