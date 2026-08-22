# Use compiler-planned cyclic scheduler service

Each Image core will run a deterministic compiler-planned service cycle with bounded quotas for ingress arbitration, ready Actor Turns, Group children, and Driver work. Admission reports a maximum service and cancellation-observation delay for every ready class; runtime host timing, queue depth, and dynamic priority do not reorder the plan. This replaces Wrela8's drain-first round-robin, which can starve Actors or child activations, while preserving predictable fixed placement and parallel execution.
