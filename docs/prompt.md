Implement Layer 1 fully and autonomously on the current local branch.

Do not commit before every phase below is complete. Preserve unrelated working-tree changes. Treat modified tracked files and new/untracked files as part of the implementation. Add regression coverage for every confirmed behavioral defect.

Use `./check` as the sole completion gate. Run it after each group of fixes and once more at the end. Use a named Challenge only for a stated investigative question, and keep all Challenge work within the repository’s documented time budget.

Do not commit unless separately authorized after completion.

## Workflow

Complete these phases in order:

1. Spec completeness
2. Architecture and maintainability
3. Correctness and adversarial bug hunting
4. Performance
5. Final integration

Keep every review narrow. Do not ask one reviewer to perform a general review combining multiple axes.

For every review invocation:

1. Check the applicable review counter and total review counter.
2. Increment both counters.
3. Launch one independent subagent with fresh context.
4. Give it the applicable prompt below.
5. The reviewer must inspect the current implementation itself.
6. The reviewer must not modify files.
7. The reviewer must not launch other subagents.
8. Independently verify every reported finding.
9. Fix every substantiated finding.
10. Add a regression case for every confirmed behavioral defect.
11. Run `./check`.
12. Use a new fresh-context reviewer for any subsequent review.

Do not retain a reviewer across iterations.

Do not change code solely to satisfy an unsupported finding. If a finding is invalid, preserve the implementation and continue based on the specification, accepted design documentation, tests, and concrete evidence.

## Review counters and convergence limits

Track only these five integer counters in the active conversation:

- Spec reviews
- Architecture reviews
- Correctness reviews
- Performance reviews
- Total reviews

Do not create a review ledger, tracking document, or repository file.

Counters never reset when a phase is reopened.

After each review, report one compact status line:

`Review count: spec N, architecture N, correctness N, performance N, total N`

No more than 18 independent review invocations may occur across the entire task.

Apply these phase limits:

- Initial spec phase: at most 3 reviewer invocations.
- Architecture phase: at most 2 discovery/fix invocations followed by 1 verification invocation.
- Initial correctness phase: at least 3 distinct risk-focused invocations and at most 4 total invocations.
- Performance phase: at most 1 discovery invocation followed, if needed, by 1 verification invocation.
- Final integration: 1 final spec review and 1 final correctness review.
- If a final review finds a defect, allow at most 1 additional fresh review of that axis after fixing it.
- A downstream phase may reopen an earlier phase at most once.
- A reopened phase may use at most 2 reviewer invocations: one discovery invocation and, if fixes are required, one verification invocation.

These are maximums, not targets, except for the required 3 distinct correctness risk areas. Stop reviewing every other axis as soon as its exit condition is satisfied.

If a limit is reached while a confirmed substantive finding remains unresolved, stop autonomous iteration. Do not waive the finding or declare completion. Report:

- the unresolved finding;
- the attempted resolutions;
- the current evidence;
- why progress stopped; and
- the decision or authority needed to continue.

If substantially the same finding remains after two materially different attempted fixes, treat the task as blocked instead of beginning another redesign.

If reviewers propose mutually exclusive architectures and both satisfy the specification and accepted design decisions, preserve the current implementation. Preference for another valid design is not a finding.

Do not revisit an architectural decision already resolved consistently with the specification and accepted design unless a later reviewer presents new concrete evidence of a release-blocking defect.

## Phase 0: Implementation

Before beginning independent review:

1. Read the originating Layer 1 issue or specification.
2. Read the relevant accepted design and domain documentation.
3. Inspect the current implementation and tests.
4. Implement the complete expected Layer 1 behavior.
5. Add direct tests for the required outcomes.
6. Run `./check`.
7. Do not begin the review phases while `./check` is failing.

## Phase 1: Spec completeness

The purpose of this phase is only to determine whether the implementation satisfies the expected Layer 1 outcomes.

Use this reviewer prompt:

"Review the current uncommitted working-tree changes against main, including modified tracked files and new/untracked files. Use the pocock spec review skill for this work. Do the work yourself. Do not launch any subagents. Perform only a spec-axis review.

Independently determine the expected Layer 1 outcomes from the originating issue or specification and the repository's accepted design documentation. Evaluate whether the current implementation fully supports those outcomes.

Identify concrete missing behavior, incorrect behavior, incomplete implementation, unsupported requirements, or behavior that contradicts the expected outcomes.

Focus only on substantive discrepancies between the Layer 1 specification and the implementation. Do not report optional improvements, stylistic concerns, architectural preferences, performance opportunities, or out-of-scope enhancements.

Do not modify any files.

For each finding, explain:

- the expected behavior;
- what the implementation currently does;
- why that is a substantive discrepancy; and
- the relevant file and location.

If there are no substantive discrepancies, report exactly:

NO FINDINGS

It is acceptable to report no findings."

The phase exits when a reviewer reports `NO FINDINGS`.

If the third spec reviewer still reports a substantiated finding:

1. Fix it if the resolution is clear.
2. Add regression coverage when appropriate.
3. Run `./check`.
4. Stop as blocked because the phase no longer has an independent clean verification available within its limit.

Do not declare spec completion without a clean independent review.

## Phase 2: Architecture and maintainability

Begin this phase only after the spec phase is clean.

The purpose of this phase is to ensure the completed behavior is represented by sound module boundaries, centralized invariants, and maintainable abstractions.

It is not a style review and does not seek the best imaginable architecture.

Use this reviewer prompt:

"Review the current uncommitted working-tree changes against main, including modified tracked files, new/untracked files, and adjacent code directly affected by the implementation. Use the codebase-design skill for this work. Do the work yourself. Do not launch any subagents. Perform only an architecture-axis review.

Evaluate whether the Layer 1 implementation contains release-blocking architectural defects that make required behavior difficult to preserve, reason about, test, or extend safely.

A finding is release-blocking only when it demonstrates at least one of the following:

- a contradiction with an accepted design decision;
- loss of information required by Layer 1;
- competing sources of semantic authority that currently permit inconsistent behavior;
- an important invariant enforced inconsistently or at the wrong ownership boundary;
- a module seam that cannot preserve a required invariant;
- invalid dependency direction that creates a concrete correctness or maintenance failure;
- partial artifacts crossing a seam that requires completed immutable artifacts;
- hidden state or reconstruction that makes required identity, ownership, failure, or determinism semantics unreliable; or
- a representation that cannot support required behavior without ambiguity or information loss.

Do not report:

- alternative decompositions that are also valid;
- naming, formatting, or style preferences;
- optional abstraction;
- local cleanup;
- speculative future extensibility;
- theoretical purity concerns without a concrete consequence; or
- architecture that is merely different from what you would personally choose.

Do not modify any files.

For each finding, explain:

- the invariant or responsibility involved;
- the current structure;
- the concrete failure or maintenance risk;
- why it blocks release rather than representing a preference;
- the required architectural direction; and
- the relevant file and location.

If there are no release-blocking architectural findings, report exactly:

NO BLOCKING FINDINGS

It is acceptable to report no findings."

The architecture phase exits when a reviewer reports `NO BLOCKING FINDINGS`.

After any architectural change:

1. Run `./check`.
2. Perform one spec recheck.
3. If the spec recheck finds a substantive discrepancy, reopen the spec phase.
4. The reopened spec phase may use at most 2 reviewer invocations.
5. The spec phase may be reopened only once.

If the architecture verification reviewer still reports a substantiated blocking finding:

1. Fix it if the resolution is clear.
2. Run `./check`.
3. Stop as blocked because the architecture phase has exhausted its verification budget.

## Phase 3: Correctness and adversarial bug hunting

Begin this phase only after spec and architecture are clean.

The purpose of this phase is to find implementation defects independently of whether the feature appears complete.

Rust already provides strong memory-safety, borrowing, type-consistency, and data-race protections for safe Rust. Do not spend correctness-review capacity searching for hypothetical pointer corruption or memory unsafety when the implementation contains no relevant unsafe boundary.

Rust does not prove that the compiler implements the correct language semantics. Concentrate correctness reviews on semantic state machines, compiler invariants, determinism, identity, ownership in the implemented language, failure behavior, and consistency between validation and execution.

Complete at least 3 separate correctness reviews using the required risk focuses below. Each invocation must remain within its assigned focus.

### Correctness focus 1: Inputs, boundaries, and operational limits

Use this risk focus:

`Malformed and boundary inputs, empty and maximum cases, numeric conversion, overflow and underflow, recursion and loop bounds, fuel and memory limits, exhaustion behavior, and bounded failure evidence.`

### Correctness focus 2: State, identity, and determinism

Use this risk focus:

`Repeated operations, compiler-instance reuse, cancellation and partial failure, state leakage between roots or requests, deterministic ordering, stable semantic identities, collision handling, loop and recursive construction coordinates, and inspection-selection invariance.`

### Correctness focus 3: Semantic consistency and feature interaction

Use this risk focus:

`Ownership transitions in the implemented language, move and must-use behavior, validation-versus-evaluation consistency, symbolic graph sealing, generic specialization, existential dispatch, public signature rules, Result propagation, and combinations of otherwise valid features not exercised together.`

For each focus, use this reviewer prompt, replacing `<RISK FOCUS>` with the assigned focus:

"Perform an adversarial correctness review of the current Layer 1 implementation focused exclusively on:

<RISK FOCUS>

Review all current uncommitted working-tree changes against main, including modified tracked files, new/untracked files, and directly affected adjacent code. Use the diagnosing-bugs workflow where useful. Do the work yourself. Do not launch any subagents.

Do not perform a spec-completeness, architecture, style, or performance review. Focus only on concrete implementation bugs within the stated risk area.

The implementation is written in safe Rust and forbids unsafe code in relevant compiler modules. Do not report hypothetical memory-unsafety concerns that Rust's type and ownership system already prevents. Rust safety does not establish semantic correctness: look for valid Rust code that implements the wrong compiler behavior.

Investigate concrete cases involving the assigned focus, including where relevant:

- boundary and empty cases;
- repeated operations and compiler-instance reuse;
- invalid or adversarial source inputs;
- cancellation and partial failure;
- state leakage between roots or requests;
- ordering and determinism;
- overflow, underflow, truncation, and limit handling;
- incorrect source-language ownership transitions;
- malformed or information-losing intermediate artifacts;
- errors that panic, lose evidence, or return the wrong outcome;
- inconsistencies between validation and execution;
- recursion, loops, and identity coordinates; and
- combinations of valid features that existing tests may not cover.

You may run read-only diagnostics and existing tests. Do not modify any files. Use a named Challenge only for a stated investigative question and remain within its documented time budget.

Report only reproducible or directly demonstrable defects. Do not report hypothetical risks without a concrete failing path.

For each finding, explain:

- the triggering scenario;
- the expected behavior;
- the actual behavior;
- the underlying implementation cause;
- the relevant file and location; and
- the regression case that would expose it.

If there are no concrete correctness defects within the stated focus, report exactly:

NO FINDINGS

It is acceptable to report no findings."

Complete all 3 required risk focuses, even if an earlier focus reports `NO FINDINGS`.

If a correctness reviewer reports findings:

1. Independently confirm each finding.
2. Fix every substantiated defect.
3. Add a durable regression case.
4. Run `./check`.
5. Continue with the remaining required risk focuses.

A fourth initial correctness review is allowed only when independent verification is needed after fixes from the third required focus. Its focus must be limited to verifying those fixes and their interactions with the previously reviewed correctness areas.

The initial correctness phase exits when:

- all 3 required risk areas have been reviewed;
- the most recent applicable correctness review reports `NO FINDINGS`;
- every confirmed defect has a regression case; and
- `./check` passes.

Do not continue searching merely because additional hypothetical bug-hunt themes could be invented.

If the fourth correctness reviewer reports another substantiated defect:

1. Fix it if the resolution is clear.
2. Add a regression case.
3. Run `./check`.
4. Reserve further independent correctness verification for final integration.

If substantially the same defect persists after two attempted fixes, stop as blocked.

After correctness fixes:

- Run `./check`.
- Reopen spec only if externally observable behavior or a specified invariant changed.
- Reopen architecture only if module responsibilities, artifact representations, or important interfaces changed.
- Each earlier phase may be reopened at most once.
- Each reopened phase may use at most 2 reviewer invocations.

## Phase 4: Performance

Begin this phase only after spec, architecture, and correctness are clean.

Performance work must be evidence-based. Do not optimize code merely because it appears theoretically inefficient.

Use this reviewer prompt:

"Review the current Layer 1 implementation for concrete performance failures. Review all current uncommitted working-tree changes against main. Do the work yourself. Do not launch any subagents. Perform only a performance-axis review.

Determine the relevant workloads, limits, and performance expectations from the specification, accepted design documents, release gates, and existing benchmark or test infrastructure.

Look only for demonstrable violations such as:

- asymptotic behavior incompatible with expected input sizes;
- repeated whole-program work where the design requires closure-wide interning or reuse;
- unbounded memory retention;
- reconstruction on a critical path that violates an explicit design or budget;
- budget accounting that does not reflect actual work;
- pathological behavior on valid adversarial inputs; or
- measured regressions against an existing benchmark or explicit budget.

Do not report micro-optimizations, speculative improvements, or preferences unsupported by measurements or an explicit performance requirement.

Do not modify any files. If measurement is needed, state the investigative question first. Use a named Challenge only when appropriate and keep it within the repository's documented time budget.

For each finding, explain:

- the relevant workload or budget;
- the measured or mechanically demonstrated behavior;
- why it violates the expected performance outcome;
- the underlying cause; and
- the relevant file and location.

If every applicable explicit workload and budget is satisfied, or the repository defines no applicable performance requirement, report exactly:

NO FINDINGS

It is acceptable to report no findings."

The performance phase exits immediately when the reviewer reports `NO FINDINGS`. Do not continue searching for additional optimization opportunities.

If the first reviewer reports a substantiated performance violation:

1. Fix it.
2. Add or update durable performance regression evidence when supported by the repository.
3. Run `./check`.
4. Launch one fresh verification reviewer using the same prompt.

If the verification reviewer still reports a substantiated violation, stop as blocked after documenting the measurements and attempted fix.

After a performance change:

- Always perform a correctness recheck.
- Reopen architecture only if representations, module seams, or ownership changed.
- Reopen spec only if observable semantics changed.
- Each earlier phase may be reopened at most once.
- Revalidation consumes the existing counters and total review budget.

## Phase 5: Final integration

After all four axes have met their exit conditions:

1. Run `./check`.
2. Launch one final fresh-context spec reviewer using the Phase 1 prompt.
3. Launch one final fresh-context correctness reviewer using the Phase 3 reviewer prompt with this risk focus:

   `Cross-feature interactions, repeated compiler use, cancellation, failure paths, and inconsistencies between validation, immutable artifacts, specialization, symbolic graph construction, and evaluation.`

4. Inspect the working tree for accidental generated files, debug instrumentation, temporary benchmarks, or unrelated modifications.
5. Run `./check` one final time.

If a final reviewer reports a substantiated finding:

1. Fix it.
2. Add a regression case when behavioral.
3. Run `./check`.
4. Reopen the relevant earlier phase at most once.
5. Launch at most one additional fresh reviewer for that final axis.

If the additional final reviewer still reports a substantiated finding, or any phase or global review limit has been exhausted, stop as blocked. Do not begin another recursive review cycle.

## Completion criteria

Do not declare the implementation complete unless:

- the final spec review reports `NO FINDINGS`;
- all 3 required correctness risk areas were independently reviewed;
- the final correctness review reports `NO FINDINGS`;
- architecture reports `NO BLOCKING FINDINGS`;
- every applicable explicit performance budget is satisfied;
- every confirmed behavioral defect has durable regression coverage;
- no review finding remains unresolved;
- no review limit was exceeded;
- the working tree contains no accidental artifacts; and
- the final `./check` passes.

When complete, report:

- the Layer 1 outcomes implemented;
- the substantive architectural decisions made;
- the regression coverage added;
- the final review counters;
- the final working-tree state; and
- the final `./check` result.

Do not commit unless separately authorized.
