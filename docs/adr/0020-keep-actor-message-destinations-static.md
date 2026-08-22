# Keep Actor message destinations static

The initial language permits an Actor to use only the explicitly named Actor handles wired into it by the Image Constructor. Actor handles cannot be placed in runtime collections or selected dynamically, even when every possible destination was created by the Image. Actors remain fixed and no handle can be forged or minted at runtime.

This makes the complete message graph literal and keeps admission, mailbox, placement, cancellation, and capacity analysis simple while the first Console is developed. Runtime handle selection may be reconsidered if concrete game code demonstrates that explicit routing Actors or messages are too restrictive.
