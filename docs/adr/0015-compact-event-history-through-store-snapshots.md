# Compact Event history through Store Snapshots

The Event Store may reclaim an old Event prefix only after durably installing a Store Snapshot for a stable committed sequence. The Store Snapshot is an authoritative schema-locked Wire Layout of the root projected game state, not a disposable cache. Recovery restores it and then applies the retained Event suffix exactly once. System-owned bounded chunks carry a Store Snapshot larger than one Event Slot.

Compaction begins from a build-declared slot-occupancy threshold rather than elapsed time. It writes the new Store Snapshot and retained suffix into the inactive storage bank, durably selects that bank, and only then reuses the old one. New Event Transactions may wait in a bounded queue while compaction proceeds. If compaction cannot create sufficient capacity, further appends return `Full`.
