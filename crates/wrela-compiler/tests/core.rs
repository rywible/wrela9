use std::collections::BTreeSet;

use wrela_compiler::{
    Cancellation, CompilationOutcome, CompilationRequest, Compiler, CompilerInstallation,
    CoreExecutableKind, CoreOperationKind, InspectSelection, ProjectFile, ProjectSnapshot, Root,
};

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
