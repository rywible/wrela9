# Deduplicate retried Event Transactions

Event Transaction submission is at least once across an uncertain acknowledgement boundary, but a retry has exactly one visible effect in the Event history. Every build-known Event Producer has a stable identity and monotonically increasing transaction sequence, and may have only one unacknowledged transaction at a time. The Event Store persists the last durable sequence and digest for each producer, including them in compaction snapshots.

A retry with the same producer, sequence, and digest returns the original durable result. Reusing a sequence with different contents is a recoverable transaction conflict, and skipping a sequence is rejected. This keeps deduplication metadata bounded while permitting the persistor to group transactions from many Actors into one physical commit.
