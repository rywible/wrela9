# Keep Wrela semantic IRs above Cranelift

Wrela will retain its own typed Core IR, scheduler-aware Flow IR, and domain-specific World and Transport IRs before lowering ordinary functions and async resume functions to Cranelift. These representations carry ownership, boundedness, effects, deterministic scheduling, visual contracts, and whole-Image facts that a general machine backend cannot preserve as Wrela semantics. Cranelift replaces the custom instruction selection, register allocation, and machine optimization stack; it does not replace Wrela-specific optimization or proof passes.
