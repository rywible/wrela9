# Use fixed-width Event Slots

Every Event in one Event Store occupies exactly one fixed-width Event Slot. The slot contains a canonical system envelope, one typed wire payload, and zero-filled unused payload bytes. An Event larger than one slot must be decomposed into several Events inside one atomic Event Transaction; system-owned compaction snapshots use their own bounded chunk format.

An Event Store's slot width is selected during Greenfield Mode and frozen when its Event Schema Lock is created. Fixed-width slots trade storage density for constant-time addressing, bounded DMA buffers, simple capacity accounting and recovery, contiguous transaction batches, and the absence of record fragmentation.
