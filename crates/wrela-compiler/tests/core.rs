use std::collections::BTreeSet;

use wrela_compiler::{
    Cancellation, CompilationOutcome, CompilationRequest, Compiler, CompilerInstallation,
    CoreAccessLaw, CoreExecutableKind, CoreOperationKind, CoreRewriteKind, InspectSelection,
    ProjectFile, ProjectSnapshot, Root,
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

    assert_eq!(core.phase_schema(), "wrela.core.v1");
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
    test records_failure():
        expect false
        expect true

@image
fn build() -> Image:
    tests = Test.new(cases=[behavior.records_failure()])
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
