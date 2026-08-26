use std::collections::BTreeSet;

use wrela_compiler::{
    Cancellation, CompilationOutcome, CompilationRequest, Compiler, CompilerInstallation,
    CoreAccessLaw, CoreCustodianEffect, CoreCustodyOperation, CoreExecutableKind,
    CoreInitializationEffect, CoreLoanEffect, CoreObligationEffect, CoreOperationKind,
    CoreRewriteKind, InspectSelection, ProjectFile, ProjectSnapshot, Root,
};

fn compile_core(source: &[u8], root: Root) -> wrela_compiler::CoreProgramObservation {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let path = if root == Root::Test {
        "src/test.wr"
    } else {
        "src/image.wr"
    };
    let request = CompilationRequest::new(
        ProjectSnapshot::new(vec![ProjectFile::new(path, source)]),
        root,
    )
    .with_architecture_profile(wrela_compiler::ArchitectureProfile::CurrentAarch64)
    .with_inspection(InspectSelection::all());
    let outcome = compiler.compile(request, &Cancellation::new());
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("Core conformance fixture accepts: {outcome:#?}");
    };
    accepted
        .inspection()
        .core_program()
        .expect("Core inspection requested")
        .clone()
}

fn core_request(selection: InspectSelection) -> CompilationRequest {
    CompilationRequest::new(
        ProjectSnapshot::new(vec![ProjectFile::new(
            "src/image.wr",
            br#"pure fn answer() -> i64:
    left = 6
    right = 7
    if true:
        return left * right
    return left + right

pure fn guarded() -> i64:
    if false:
        panic "unreachable"
    return 3

@image
fn build() -> Image:
    return Image.new(value=answer() + guarded())
"#,
        )]),
        Root::Image,
    )
    .with_architecture_profile(wrela_compiler::ArchitectureProfile::CurrentAarch64)
    .with_inspection(selection)
}

#[test]
fn core_realizes_exact_executable_demand_and_preserves_semantic_operations() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let CompilationOutcome::Accepted(accepted) =
        compiler.compile(core_request(InspectSelection::all()), &Cancellation::new())
    else {
        panic!("Core fixture accepts");
    };
    let core = accepted
        .inspection()
        .core_program()
        .expect("Core inspection requested");
    let planning = accepted
        .inspection()
        .planning_foundation()
        .expect("planning inspection requested");

    assert_eq!(core.phase_schema(), "wrela.core.v3");
    assert_eq!(
        accepted.core_program_fingerprint(),
        Some(core.fingerprint())
    );
    assert_eq!(
        core.executables().len(),
        planning.executable_demand().exact_executable_count()
    );
    assert_eq!(
        core.executables()
            .iter()
            .map(|executable| executable.identity())
            .collect::<BTreeSet<_>>()
            .len(),
        core.executables().len()
    );
    assert!(core.executables().iter().any(|executable| {
        executable.kind() == CoreExecutableKind::SourceSpecialization
            && executable.operations().contains(&CoreOperationKind::Call)
    }));
    assert!(core.executables().iter().any(|executable| {
        executable.kind() == CoreExecutableKind::SourceSpecialization
            && executable
                .operations()
                .contains(&CoreOperationKind::CheckedArithmetic)
            && executable.operations().contains(&CoreOperationKind::Branch)
            && executable.operations().contains(&CoreOperationKind::Return)
    }));
    assert!(core.executables().iter().any(|executable| {
        executable.kind() == CoreExecutableKind::Generated
            && executable.operations() == [CoreOperationKind::GeneratedRole]
    }));
    assert!(core.executables().iter().any(|executable| {
        executable
            .operations()
            .contains(&CoreOperationKind::TerminalPanic)
    }));
    assert!(core.oracle_case_count() >= 2);
    assert!(core.oracle_agrees());
}

#[test]
fn core_identity_is_independent_of_inspection_reuse_reopen_and_file_enumeration() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let CompilationOutcome::Accepted(without) =
        compiler.compile(core_request(InspectSelection::none()), &Cancellation::new())
    else {
        panic!("fixture accepts without inspection");
    };
    let CompilationOutcome::Accepted(with) =
        compiler.compile(core_request(InspectSelection::all()), &Cancellation::new())
    else {
        panic!("fixture accepts with inspection");
    };
    let CompilationOutcome::Accepted(repeated) =
        compiler.compile(core_request(InspectSelection::all()), &Cancellation::new())
    else {
        panic!("fixture accepts repeatedly");
    };
    let reopened_compiler =
        Compiler::open(CompilerInstallation::layer1()).expect("distribution reopens");
    let CompilationOutcome::Accepted(reopened) =
        reopened_compiler.compile(core_request(InspectSelection::all()), &Cancellation::new())
    else {
        panic!("fixture accepts after reopen");
    };
    let reordered = CompilationRequest::new(
        ProjectSnapshot::new(vec![
            ProjectFile::new("src/unused/module.wr", b"const UNUSED: i64 = 1\n"),
            ProjectFile::new(
                "src/image.wr",
                br#"pure fn answer() -> i64:
    left = 6
    right = 7
    if true:
        return left * right
    return left + right

pure fn guarded() -> i64:
    if false:
        panic "unreachable"
    return 3

@image
fn build() -> Image:
    return Image.new(value=answer() + guarded())
"#,
            ),
        ]),
        Root::Image,
    )
    .with_architecture_profile(wrela_compiler::ArchitectureProfile::CurrentAarch64)
    .with_inspection(InspectSelection::all());
    let CompilationOutcome::Accepted(reordered) =
        reopened_compiler.compile(reordered, &Cancellation::new())
    else {
        panic!("fixture with unreachable source accepts");
    };

    assert!(without.inspection().core_program().is_none());
    assert_eq!(
        without.core_program_fingerprint(),
        with.core_program_fingerprint()
    );
    assert_eq!(
        with.core_program_fingerprint(),
        repeated.core_program_fingerprint()
    );
    assert_eq!(
        repeated.core_program_fingerprint(),
        reopened.core_program_fingerprint()
    );
    assert_eq!(
        reopened.core_program_fingerprint(),
        reordered.core_program_fingerprint()
    );
}

#[test]
fn rejected_and_cancelled_requests_publish_no_core() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let CompilationOutcome::Rejected(rejected) = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", b"fn broken(\n")]),
            Root::Image,
        )
        .with_architecture_profile(wrela_compiler::ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    ) else {
        panic!("malformed source rejects");
    };
    assert!(rejected.inspection().core_program().is_none());

    let cancellation = Cancellation::new();
    cancellation.cancel();
    assert!(matches!(
        compiler.compile(core_request(InspectSelection::all()), &cancellation),
        CompilationOutcome::Cancelled
    ));
}

#[test]
fn core_retains_exact_typed_signatures_and_all_access_laws() {
    let test_core = compile_core(
        br#"resource struct Token:
    value: i64

pub suite behavior:
    test consumes(take token: Token):
        expect token.value == 1
        consume(take token)

fn consume(take token: Token):
    pass

@image
fn build() -> Image:
    tests = Test.new(cases=[behavior.consumes(Token(value=1))])
    return Image.new(tests=tests)
"#,
        Root::Test,
    );

    let core = compile_core(
        br#"resource struct Counter:
    mut value: i64

fn increment(mut counter: Counter):
    counter.value += 1

fn observe(read counter: Counter) -> i64:
    return counter.value

fn consume(take counter: Counter):
    pass

fn compute() -> i64:
    mut counter = Counter(value=2)
    before = observe(counter)
    increment(mut counter)
    consume(take counter)
    return before

@image
fn build() -> Image:
    callback = |value: i64| value + 1
    return Image.new(value=compute(), callback=callback)
"#,
        Root::Image,
    );

    let functions = core
        .executables()
        .iter()
        .filter(|executable| executable.parameters().len() == 1)
        .collect::<Vec<_>>();
    assert!(functions.iter().all(|function| {
        function.parameters()[0].local_identity() != u32::MAX
            && function.parameters()[0].type_identity() != 0
            && function.return_type_identity() != 0
    }));
    assert!(functions.iter().any(|function| {
        function.parameters()[0].access() == CoreAccessLaw::CopyValue
            && function.parameters()[0].type_identity() == function.return_type_identity()
    }));
    let signature_accesses = functions
        .iter()
        .map(|function| function.parameters()[0].access())
        .collect::<BTreeSet<_>>();
    assert!(signature_accesses.is_superset(&BTreeSet::from([
        CoreAccessLaw::CopyValue,
        CoreAccessLaw::SharedLoan,
        CoreAccessLaw::ExclusiveLoan,
        CoreAccessLaw::Move,
    ])));

    let test = test_core
        .executables()
        .iter()
        .find(|executable| executable.kind() == CoreExecutableKind::SourceTestBody)
        .expect("applied Test realization");
    assert_eq!(test.parameters().len(), 1);
    assert_eq!(test.parameters()[0].access(), CoreAccessLaw::Move);

    let closure = core
        .executables()
        .iter()
        .find(|executable| executable.kind() == CoreExecutableKind::SourceClosureBody)
        .expect("retained Closure realization");
    assert_eq!(closure.parameters().len(), 1);
    assert_eq!(closure.parameters()[0].access(), CoreAccessLaw::CopyValue);
    assert_eq!(
        closure.parameters()[0].type_identity(),
        closure.return_type_identity()
    );

    let accesses = core
        .executables()
        .iter()
        .flat_map(|executable| executable.access_laws().iter().copied())
        .collect::<BTreeSet<_>>();
    assert!(accesses.is_superset(&BTreeSet::from([
        CoreAccessLaw::CopyValue,
        CoreAccessLaw::SharedLoan,
        CoreAccessLaw::ExclusiveLoan,
        CoreAccessLaw::Move,
    ])));
}

#[test]
fn core_signature_type_ids_join_authoritative_body_value_type_ids() {
    let core = compile_core(
        br#"struct Marker:
    value: i64

pure fn echo_marker(value: Marker) -> Marker:
    return value

pure fn echo_integer(value: i64) -> i64:
    return value

@image
fn build() -> Image:
    marker = echo_marker(Marker(value=1))
    return Image.new(marker=marker, integer=echo_integer(marker.value))
"#,
        Root::Image,
    );

    let identity_functions = core
        .executables()
        .iter()
        .filter(|executable| {
            executable.kind() == CoreExecutableKind::SourceSpecialization
                && executable.parameters().len() == 1
                && executable.parameters()[0].type_identity() == executable.return_type_identity()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        identity_functions.len(),
        2,
        "nominal and primitive identity functions"
    );
    assert!(identity_functions.iter().all(|executable| {
        executable
            .operation_type_identities()
            .contains(&executable.parameters()[0].type_identity())
    }));
    assert_ne!(
        identity_functions[0].return_type_identity(),
        identity_functions[1].return_type_identity(),
        "nominal and primitive TypeIds remain distinct catalog authority"
    );
}

#[test]
fn core_oracle_covers_calls_integer_bits_floats_and_panic_order() {
    let core = compile_core(
        br#"pure fn bits(value: i64) -> i64:
    return ((value & 15) << 2) | (value >> 1)

pure fn bit_wrapper() -> i64:
    return bits(10)

pure fn float_wrapper() -> bool:
    return 1.5f64 + 2.25f64 > 3.0f64

pure fn fail() -> i64:
    panic "first"

pure fn panic_wrapper() -> i64:
    return fail() + (1 / 0)

pure fn panic_guard() -> i64:
    if false:
        return panic_wrapper()
    return 0

@image
fn build() -> Image:
    return Image.new(bits=bit_wrapper() + panic_guard(), float=float_wrapper())
"#,
        Root::Image,
    );

    assert!(core.oracle_agrees());
    assert!(
        core.oracle_case_count() >= 5,
        "the eligible callees and wrappers, including the first Panic, are compared; got {}",
        core.oracle_case_count()
    );
}

#[test]
fn core_oracle_binds_named_call_arguments_by_parameter_without_reordering_evaluation() {
    let core = compile_core(
        br#"pure fn subtract(left: i64, right: i64) -> i64:
    return left - right

pure fn reordered() -> i64:
    return subtract(right=2, left=9)

pure fn first_panic() -> i64:
    panic "first"

pure fn nested_panic_order() -> i64:
    return subtract(right=first_panic(), left=(1 / 0))

pure fn retain_nested_panic() -> i64:
    if false:
        return nested_panic_order()
    return 0

@image
fn build() -> Image:
    return Image.new(value=reordered() + retain_nested_panic())
"#,
        Root::Image,
    );

    assert!(core.oracle_agrees());
    assert!(core.oracle_case_count() >= 4);
}

#[test]
fn core_oracle_exactly_covers_current_pure_scalar_and_aggregate_laws() {
    let core = compile_core(
        br#"pure fn float_laws() -> bool:
    return -1.5f64 + 4.0f64 == 2.5f64 and 1.0f32 < 2.0f32

pure fn unsigned_not() -> bool:
    return ~0u8 == 255u8

pure fn text_order() -> bool:
    return "alpha" < "beta"

pure fn bytes_order() -> bool:
    return b"ab" < b"ac"

pure fn scalar_order() -> bool:
    return 'a' < 'b'

pure fn aggregate_equality() -> bool:
    return [1, 2] == [1, 2] and (1, false) != (1, true)

@image
fn build() -> Image:
    return Image.new(float=float_laws(), inverted=unsigned_not(), text=text_order(), bytes=bytes_order(), scalar=scalar_order(), aggregate=aggregate_equality())
"#,
        Root::Image,
    );

    assert!(core.oracle_agrees());
    assert!(
        core.oracle_case_count() >= 6,
        "each representative pure law participates in conformance"
    );
}

#[test]
fn core_oracle_covers_bounded_for_and_evaluator_sized_thousand_iteration_while() {
    let core = compile_core(
        br#"pure fn bounded_for() -> i64:
    mut total = 0
    for value in [1, 2, 3, 4]:
        total += value
    return total

pure fn thousand_iterations() -> i64:
    mut index = 0
    while index < 1000:
        index += 1
    return index

@image
fn build() -> Image:
    return Image.new(value=bounded_for() + thousand_iterations())
"#,
        Root::Image,
    );

    assert!(core.oracle_agrees());
    assert!(core.oracle_case_count() >= 2);
}

#[test]
fn core_oracle_re_evaluates_bounded_while_conditions() {
    let core = compile_core(
        br#"pure fn total() -> i64:
    mut index = 0
    mut sum = 0
    while index < 4:
        index += 1
        if index == 2:
            continue
        sum += index
    return sum

@image
fn build() -> Image:
    return Image.new(value=total())
"#,
        Root::Image,
    );

    assert!(core.oracle_agrees());
    assert!(core.oracle_case_count() >= 1);
    assert!(core.executables().iter().any(|executable| {
        executable.operations().contains(&CoreOperationKind::Loop)
            && executable
                .operations()
                .contains(&CoreOperationKind::LoopBack)
    }));
}

#[test]
fn false_expect_records_test_failure_without_certifying_panic() {
    let core = compile_core(
        br#"pub suite behavior:
    test records_failure(take value: i64):
        expect false
        value
        expect true

@image
fn build() -> Image:
    tests = Test.new(cases=[behavior.records_failure(1)])
    return Image.new(tests=tests)
"#,
        Root::Test,
    );
    let test = core
        .executables()
        .iter()
        .find(|executable| executable.kind() == CoreExecutableKind::SourceTestBody)
        .expect("Test body realization");
    assert!(!test.may_panic());
    assert_eq!(
        test.operations()
            .iter()
            .filter(|kind| **kind == CoreOperationKind::Expect)
            .count(),
        2
    );
}

#[test]
fn core_panic_facts_distinguish_local_and_recoverable_laws_from_terminal_checks() {
    let core = compile_core(
        br#"pure fn assertion_failure() -> i64:
    assert false
    return 0

pure fn arithmetic_failure() -> i8:
    return 127i8 + 1i8

pure fn index_failure() -> i64:
    return [1, 2][3]

pure fn retain_failures() -> i64:
    if false:
        return assertion_failure() + index_failure()
    if false:
        arithmetic_failure()
    return 0

@image
fn build() -> Image:
    safe = |value: i64| value
    checked = |value: i8| value + 1i8
    return Image.new(value=retain_failures(), safe=safe, checked=checked)
"#,
        Root::Image,
    );

    for operation in [
        CoreOperationKind::Assert,
        CoreOperationKind::CheckedArithmetic,
        CoreOperationKind::Index,
    ] {
        assert!(core.executables().iter().any(|executable| {
            executable.operations().contains(&operation) && executable.may_panic()
        }));
    }
    let closures = core
        .executables()
        .iter()
        .filter(|executable| executable.kind() == CoreExecutableKind::SourceClosureBody)
        .collect::<Vec<_>>();
    assert_eq!(closures.len(), 2);
    assert!(closures.iter().any(|closure| {
        closure
            .operations()
            .contains(&CoreOperationKind::CheckedArithmetic)
            && closure.may_panic()
    }));
    assert!(closures.iter().any(|closure| {
        !closure
            .operations()
            .contains(&CoreOperationKind::CheckedArithmetic)
            && !closure.may_panic()
    }));
}

#[test]
fn core_oracle_reports_evaluated_integer_negation_overflow_as_terminal_panic() {
    let core = compile_core(
        br#"pure fn minimum() -> i8:
    return -128i8

pure fn negate_minimum() -> i8:
    value = minimum()
    return -value

pure fn retain_overflow() -> i64:
    if false:
        negate_minimum()
    return 0

@image
fn build() -> Image:
    return Image.new(value=retain_overflow())
"#,
        Root::Image,
    );

    assert!(core.oracle_agrees());
    assert!(core.oracle_case_count() >= 3);
}

#[test]
fn redundant_pass_is_canonically_eliminated_with_a_witness() {
    let core = compile_core(
        br#"pure fn answer() -> i64:
    pass
    return 42

@image
fn build() -> Image:
    return Image.new(value=answer())
"#,
        Root::Image,
    );
    let rewritten = core
        .executables()
        .iter()
        .find(|executable| !executable.rewrites().is_empty())
        .expect("pass elimination witness");
    assert_eq!(rewritten.rewrites(), [CoreRewriteKind::EliminatedPass]);
    assert!(!rewritten.operations().contains(&CoreOperationKind::Pass));
    assert!(core.oracle_agrees());
}

#[test]
fn core_preserves_resource_custody_as_four_orthogonal_dimensions() {
    let core = compile_core(
        br#"resource struct Ticket:
    mut id: i64

resource struct Envelope:
    ticket: Ticket

fn inspect(read ticket: Ticket) -> i64:
    return ticket.id

fn edit(mut ticket: Ticket):
    ticket.id = 7

fn consume(take ticket: Ticket):
    pass

fn run() -> i64:
    mut ticket = Ticket(id=1)
    before = inspect(ticket)
    edit(mut ticket)
    moved = take ticket
    ticket = Ticket(id=2)
    envelope = Envelope(ticket=take moved)
    consume(take envelope.ticket)
    consume(take ticket)
    return before

@image
fn build() -> Image:
    return Image.new(value=run())
"#,
        Root::Image,
    );

    let effects = core
        .executables()
        .iter()
        .flat_map(|executable| executable.custody_effects())
        .collect::<Vec<_>>();
    let kinds = effects
        .iter()
        .map(|effect| effect.operation())
        .collect::<BTreeSet<_>>();
    assert!(
        kinds.is_superset(&BTreeSet::from([
            CoreCustodyOperation::Construct,
            CoreCustodyOperation::Move,
            CoreCustodyOperation::SharedLoan,
            CoreCustodyOperation::ExclusiveLoan,
            CoreCustodyOperation::Reinitialize,
            CoreCustodyOperation::TransferCommit,
            CoreCustodyOperation::Discharge,
        ])),
        "missing custody law: {kinds:?}"
    );
    assert!(effects.iter().any(|effect| {
        effect.operation() == CoreCustodyOperation::Move
            && effect.place_projection_count() > 0
            && effect.initialization() == CoreInitializationEffect::Uninitialize
            && effect.custodian() == CoreCustodianEffect::Transfer
            && effect.loan() == CoreLoanEffect::None
            && effect.obligation() == CoreObligationEffect::Transfer
    }));
    assert!(effects.iter().any(|effect| {
        effect.operation() == CoreCustodyOperation::SharedLoan
            && effect.loan() == CoreLoanEffect::Shared
            && effect.custodian() == CoreCustodianEffect::None
    }));
    assert!(effects.iter().any(|effect| {
        effect.operation() == CoreCustodyOperation::ExclusiveLoan
            && effect.loan() == CoreLoanEffect::Exclusive
            && effect.custodian() == CoreCustodianEffect::None
    }));
    assert!(core.custody_oracle_case_count() >= 1);
    assert!(core.custody_oracle_agrees());
}

#[test]
fn recoverable_exits_preserve_cleanup_proof_and_fixpoint_laws_while_panic_does_not_clean_up() {
    let core = compile_core(
        br#"enum Failure:
    Failed

fn fail() -> Result[i64, Failure]:
    return Result.Err(Failure.Failed)

fn oldest():
    pass

fn newest():
    pass

fn run(flag: bool) -> Result[i64, Failure]:
    defer oldest()
    defer newest()
    mut count = 0
    while count < 1:
        count += 1
    if flag:
        return fail()?
    return Result.Ok(count)

fn terminal():
    defer oldest()
    panic "terminal"

@image
fn build() -> Image:
    if false:
        terminal()
    return Image.new(value=run(true))
"#,
        Root::Image,
    );

    let effects = core
        .executables()
        .iter()
        .flat_map(|executable| executable.custody_effects())
        .collect::<Vec<_>>();
    let registrations = effects
        .iter()
        .filter(|effect| effect.operation() == CoreCustodyOperation::CleanupRegister)
        .filter_map(|effect| effect.cleanup_identity())
        .collect::<Vec<_>>();
    assert!(registrations.len() >= 3);

    let cleanup_runs = effects
        .iter()
        .filter(|effect| effect.operation() == CoreCustodyOperation::CleanupRun)
        .count();
    assert!(cleanup_runs >= 2);
    assert!(core.executables().iter().any(|executable| {
        executable
            .operations()
            .contains(&CoreOperationKind::CleanupRun)
    }));

    assert!(
        effects
            .iter()
            .all(|effect| effect.operation() != CoreCustodyOperation::ProofCondition),
        "ordinary propagation has no planning Requirement authority"
    );
    assert!(
        effects
            .iter()
            .any(|effect| effect.operation() == CoreCustodyOperation::Join)
    );
    assert!(
        effects
            .iter()
            .any(|effect| effect.operation() == CoreCustodyOperation::LoopFixpoint)
    );

    assert!(
        registrations
            .iter()
            .any(|registration| !effects.iter().any(|effect| {
                effect.operation() == CoreCustodyOperation::CleanupRun
                    && effect.cleanup_identity() == Some(*registration)
            })),
        "Panic leaves at least its source cleanup registration unexecuted"
    );
}

#[test]
fn stored_resource_custody_is_continuous_through_take_reinitialize_and_consume() {
    let core = compile_core(
        br#"resource struct Ticket:
    id: i64

fn consume(take ticket: Ticket):
    pass

fn run():
    ticket = Ticket(id=1)
    first = take ticket
    ticket = Ticket(id=2)
    consume(take first)
    consume(take ticket)

@image
fn build() -> Image:
    run()
    return Image.new(value=0)
"#,
        Root::Image,
    );
    let effects = core
        .executables()
        .iter()
        .flat_map(|executable| executable.custody_effects())
        .collect::<Vec<_>>();
    let reinitialize = effects
        .iter()
        .find(|effect| effect.operation() == CoreCustodyOperation::Reinitialize)
        .expect("take then assign is explicit reinitialization");
    assert!(reinitialize.custody_continuous());
    assert!(
        effects
            .iter()
            .filter(|effect| matches!(
                effect.operation(),
                CoreCustodyOperation::Move
                    | CoreCustodyOperation::Reinitialize
                    | CoreCustodyOperation::TransferCommit
            ))
            .all(|effect| effect.custody_continuous())
    );
    assert!(core.custody_oracle_agrees());
}

#[test]
fn normally_completing_while_and_match_scopes_run_their_deferred_cleanup() {
    let core = compile_core(
        br#"fn clean():
    pass

fn run():
    mut count = 0
    while count < 1:
        count += 1
        defer clean()
    match count:
        case 1:
            defer clean()
        case _:
            pass

@image
fn build() -> Image:
    run()
    return Image.new(value=0)
"#,
        Root::Image,
    );
    let cleanup_runs = core
        .executables()
        .iter()
        .flat_map(|executable| executable.custody_effects())
        .filter(|effect| effect.operation() == CoreCustodyOperation::CleanupRun)
        .count();
    assert_eq!(cleanup_runs, 2);
    assert!(core.custody_oracle_agrees());
}

#[test]
fn panicking_newer_cleanup_has_only_a_success_continuation_to_the_older_cleanup() {
    let core = compile_core(
        br#"fn older():
    pass

fn newer():
    panic "newer cleanup"

fn run():
    defer older()
    defer newer()

@image
fn build() -> Image:
    if false:
        run()
    return Image.new(value=0)
"#,
        Root::Image,
    );
    let cleanup_runs = core
        .executables()
        .iter()
        .flat_map(|executable| executable.custody_effects())
        .filter(|effect| effect.operation() == CoreCustodyOperation::CleanupRun)
        .collect::<Vec<_>>();
    assert_eq!(cleanup_runs.len(), 2);
    assert!(cleanup_runs.iter().any(|effect| {
        effect.cleanup_action_may_panic() && effect.cleanup_has_success_continuation()
    }));
    assert!(cleanup_runs.iter().any(|effect| {
        !effect.cleanup_action_may_panic() && !effect.cleanup_has_success_continuation()
    }));
}

#[test]
fn unrelated_earlier_operation_does_not_renumber_custody_observations() {
    let without = compile_core(
        br#"resource struct Ticket:
    id: i64

fn consume(take ticket: Ticket):
    pass

fn run():
    ticket = Ticket(id=1)
    consume(take ticket)

@image
fn build() -> Image:
    run()
    return Image.new(value=0)
"#,
        Root::Image,
    );
    let with = compile_core(
        br#"resource struct Ticket:
    id: i64

fn consume(take ticket: Ticket):
    pass

fn run():
    pass
    ticket = Ticket(id=1)
    consume(take ticket)

@image
fn build() -> Image:
    run()
    return Image.new(value=0)
"#,
        Root::Image,
    );
    let custody_identities = |core: &wrela_compiler::CoreProgramObservation| {
        core.executables()
            .iter()
            .find(|executable| {
                executable
                    .custody_effects()
                    .iter()
                    .any(|effect| effect.operation() == CoreCustodyOperation::Move)
            })
            .expect("run executable")
            .custody_effects()
            .iter()
            .map(|effect| effect.semantic_identity())
            .collect::<Vec<_>>()
    };
    assert_eq!(custody_identities(&without), custody_identities(&with));
}

#[test]
fn nested_recoverable_exit_runs_inner_then_outer_cleanup_in_reverse_registration_order() {
    let core = compile_core(
        br#"fn outer():
    pass

fn inner_oldest():
    pass

fn inner_newest():
    pass

fn run(flag: bool) -> i64:
    defer outer()
    if flag:
        defer inner_oldest()
        defer inner_newest()
        return 1
    return 0

@image
fn build() -> Image:
    return Image.new(value=run(true))
"#,
        Root::Image,
    );

    let cleanup_runs = core
        .executables()
        .iter()
        .flat_map(|executable| executable.custody_effects())
        .filter(|effect| effect.operation() == CoreCustodyOperation::CleanupRun)
        .count();
    assert!(cleanup_runs >= 3);
}

#[test]
fn compiler_owned_pool_is_transferred_into_its_scope_and_reclaimed_after_nested_scope() {
    let core = compile_core(
        br#"from core import pool as pools

fn run(flag: bool):
    with pools.scoped(capacity=1) as scratch:
        if flag:
            pass
        pass

@image
fn build() -> Image:
    run(true)
    return Image.new()
"#,
        Root::Image,
    );

    let run = core
        .executables()
        .iter()
        .find(|executable| {
            executable
                .operations()
                .contains(&CoreOperationKind::PoolScope)
        })
        .expect("pool scope executable");
    let custody = run.custody_effects();
    assert!(custody.iter().any(|effect| {
        effect.operation() == CoreCustodyOperation::TransferCommit
            && effect.initialization() == CoreInitializationEffect::Initialize
    }));
    assert!(custody.iter().any(|effect| {
        effect.operation() == CoreCustodyOperation::Discharge && effect.custody_continuous()
    }));
}
