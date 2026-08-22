# Separate logical and realtime deadlines

Wrela will distinguish replayable deadlines measured in deterministic Cadence occurrences or scheduler epochs from realtime deadlines requiring Monotonic Clock authority. Logical deadlines may directly shape gameplay state; realtime outcomes may do so only when captured as Replay input. Both constrain compiler-planned admission and produce explicit `DeadlineUnmeetable` or `DeadlineExceeded` outcomes rather than causing dynamic runtime priority, preserving real service contracts without silently introducing host time into deterministic behavior.
