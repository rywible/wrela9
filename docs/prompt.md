Implement layer 1 fully autonomously. Don't commit anything until the work is fully complete. Work on the current local branch. When you think you have finished, launch an independent subagent to review your work. It should have fresh context. Here is the exact prompt you should use:

"Review the current uncommitted working-tree changes against main, including both modified tracked files and new/untracked files. Nothing from this implementation has been committed yet. Use the pocock spec review skill for this work. Do the work yourself. Don't launch any other subagents and just do a spec axis review.

Independently evaluate whether the implementation fully satisfies the expected Layer 1 outcomes. Identify any concrete gaps, missing behavior, incorrect behavior, incomplete implementation, or requirements that are not actually supported by the current code.

Focus only on substantive discrepancies between the expected Layer 1 outcomes and the implementation; do not suggest optional improvements, stylistic changes, or out-of-scope enhancements.

Do not modify any files.

For each finding, explain the expected behavior, what the implementation currently does, and the relevant file/location.
If there are no substantive gaps, report exactly:
NO FINDINGS

It's ok to have no findings."

Fix any findings that they come back with, and loop until you receive a clean "NO FINDINGS".
