Fully implement <MILESTONE> on the current local branch. Do not take shortcuts. Do not cheat. Every single thing specified must be implemented fully production grade. World class implementation. Exhaustive.

When you've fully completed the work, launch an independent subagent to review your implementation. They should always be a new subagent and always have fresh context. Send them this prompt exactly:
"Review the current changes against main for the <MILESTONE> scope, including modified tracked files and new/untracked files. Use the pocock spec review skill for this work. Don't launch any subagents. Perform only a spec-axis review yourself.

If there are no substantive discrepancies, report exactly:
NO FINDINGS

It is acceptable to report no findings."

Verify and fix any subagent findings. Loop until you receive "NO FINDINGS". And don't just farm out a shitty implementation and expect the subagent to catch it for you. Do it right the first time. You should be optimizing for needing as few loops as possible to get a clean NO FINDINGS report. Take some pride in your work.
