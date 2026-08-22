# Preserve the Wrela8 language surface without compatibility

Wrela9 will preserve the successful shape of Wrela8's Creator-facing language: significant indentation, explicit imports, Data versus Resources, mirrored `read`/`mut`/`take`, bounded Pools, structural monomorphized generics, Actors, `Result`, checked arithmetic, and ordinary-language compile-time evaluation. It may reuse the lexer, parser, pretty-printer, semantic algorithms, and selected golden tests.

Wrela9 makes no source-compatibility promise. Wrela8 programs, Image declarations, diagnostics, and generated artifacts may change wherever the new Console, Cranelift, QEMU, safety, scheduling, Event Store, World language, or Display contracts require a cleaner design. Preserving a concept does not require preserving its implementation.
