# Keep capacity proofs out of source-visible types

An operation's source-visible type will not change when whole-Image capacity analysis succeeds or fails. Capacity proofs may erase runtime checks, remove unreachable error paths from generated code, and improve the Image report, but changing a mailbox, Pool, or Actor placement will not silently change an expression from `Result` to an unconditional success type.

If Wrela later exposes proof-required operations, they will use an explicit source form rather than overloading the type of the ordinary operation. This keeps local APIs and error handling stable while retaining whole-Image optimization.
