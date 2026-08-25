Complete `<MILESTONE>` autonomously on the current branch.

Use `implement`, `tdd`, `code-review`, `codebase-design`, and `diagnosing-bugs` according to their skill instructions. Create a durable goal for completing the milestone; do not set a token budget.

Before editing, fetch the milestone, approved specification, implementation tickets, comments, and native dependencies.

Work the ticket frontier until empty:

1. Select and claim one unblocked ticket.
2. Launch one fresh worker subagent with only the ticket, spec, relevant documentation, and starting commit. Run implementation workers sequentially on the shared worktree.
3. Have the worker implement with TDD, preserve unrelated changes, run `./check`, and commit with the ticket reference.
4. Independently review that ticket’s committed diff against its starting commit using the two-axis `code-review`.
5. Validate every finding. Send confirmed findings back to the worker, require Regression Cases for behavioral defects, and rerun `./check`.
6. Perform at most one targeted re-review after fixes, then post concise evidence and close the ticket.
7. Recalculate the frontier and continue. If one ticket is blocked, work another unblocked ticket.

After all tickets close, run milestone-wide fresh-context reviews for:

- Specification completeness
- Architecture and invariant ownership
- Boundary and failure correctness
- State, identity, cancellation, and determinism
- Cross-feature integration
- Performance only where an explicit budget exists

Run reviewers in parallel waves. Deduplicate and independently validate findings, repair all confirmed issues in one batch, add Regression Cases, and run `./check`. Re-run only affected review axes once. If the same substantive defect survives two materially different fixes, request human direction instead of redesigning indefinitely.

Commits are authorized on the current branch. Do not push, merge, create a PR, or discard unrelated work.

Ask for human input only for contradictory specifications, missing authority, required external access, or a repeatedly unresolved defect. Otherwise continue autonomously.

Complete the goal only when every milestone ticket is closed, no confirmed finding remains, the final spec review is clean, the working tree has no accidental artifacts, and the final `./check` passes. Post milestone completion evidence and close the milestone.
