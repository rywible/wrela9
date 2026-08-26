use std::collections::BTreeSet;
use wrela_compiler::{
    ArchitectureProfile, Cancellation, CompilationOutcome, CompilationRequest, Compiler,
    CompilerInstallation, GeneratedRoleKind, InspectSelection, PlannerKind, ProjectFile,
    ProjectSnapshot, RequirementBounds, RequirementCategory, Root,
};

fn deployment_request(inspection: InspectSelection) -> CompilationRequest {
    CompilationRequest::new(
        ProjectSnapshot::new(vec![ProjectFile::new(
            "src/image.wr",
            b"@image\nfn build() -> Image:\n    return Image.new()\n",
        )]),
        Root::Image,
    )
    .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
    .with_inspection(inspection)
}

fn test_request(inspection: InspectSelection) -> CompilationRequest {
    let source = br#"pub suite smoke:
    test passes():
        expect true

@image
fn build() -> Image:
    tests = Test.new(cases=[smoke.passes()])
    return Image.new(tests=tests)
"#;
    CompilationRequest::new(
        ProjectSnapshot::new(vec![ProjectFile::new("src/test.wr", source)]),
        Root::Test,
    )
    .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
    .with_inspection(inspection)
}

fn accepted(request: CompilationRequest) -> wrela_compiler::AcceptedCompilation {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(request, &Cancellation::new());
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("planning fixture must accept: {outcome:#?}");
    };
    accepted
}

#[test]
fn deployment_image_derives_exact_mandatory_planning_closure() {
    let accepted = accepted(deployment_request(InspectSelection::planning()));
    let planning = accepted
        .inspection()
        .planning_foundation()
        .expect("planning inspection is selected");

    assert_eq!(
        planning.phase_schema(),
        "wrela.image-planning-foundation.v1"
    );
    assert_eq!(planning.planners().len(), 1);
    assert_eq!(planning.planners()[0].kind(), PlannerKind::ImageKind);
    assert_eq!(planning.domain_plans().len(), 1);
    assert_eq!(
        planning
            .generated_roles()
            .iter()
            .map(|role| role.kind())
            .collect::<Vec<_>>(),
        [
            GeneratedRoleKind::Boot,
            GeneratedRoleKind::Scheduler,
            GeneratedRoleKind::Terminal,
            GeneratedRoleKind::Panic,
            GeneratedRoleKind::Shutdown,
        ]
    );
    assert_eq!(planning.requirements().len(), 21);
    assert_eq!(
        planning
            .requirements()
            .iter()
            .map(|requirement| requirement.reference())
            .collect::<BTreeSet<_>>()
            .len(),
        planning.requirements().len(),
        "every exact requirement has one unique stable Requirement Reference"
    );
    assert!(planning.requirements().iter().all(|requirement| {
        let wrela_compiler::RequirementSubject::GeneratedRole(role) = requirement.subject();
        requirement.reference() != 0
            && requirement.owner() == planning.planners()[0].identity()
            && planning
                .generated_roles()
                .iter()
                .any(|generated| generated.identity() == role)
            && requirement.provenance().domain_plan() == planning.domain_plans()[0].identity()
            && requirement.provenance().generated_role() == role
            && requirement.provenance().local_site() > 0
            && requirement.current_meaning() != 0
    }));
    assert!(planning.requirements().iter().any(|requirement| {
        requirement.category() == RequirementCategory::ArchitectureCapability
            && matches!(requirement.bounds(), RequirementBounds::Capability(_))
    }));
    assert_eq!(planning.executable_demand().generated_executable_count(), 5);
    assert_eq!(
        planning.executable_demand().exact_executable_count(),
        planning.executable_demand().source_executable_count() + 5
    );
}

#[test]
fn test_image_adds_only_the_mandatory_test_runtime_closure() {
    let deployment = accepted(deployment_request(InspectSelection::planning()));
    let test = accepted(test_request(InspectSelection::planning()));
    let deployment = deployment.inspection().planning_foundation().unwrap();
    let test = test.inspection().planning_foundation().unwrap();

    assert_eq!(test.planners().len(), 1);
    assert_eq!(test.domain_plans().len(), 1);
    assert_eq!(test.generated_roles().len(), 6);
    assert_eq!(
        test.generated_roles().last().unwrap().kind(),
        GeneratedRoleKind::TestRuntime
    );
    assert_eq!(test.requirements().len(), 24);
    assert_eq!(
        test.executable_demand().generated_executable_count(),
        deployment.executable_demand().generated_executable_count() + 1
    );
    assert!(test.generated_roles().iter().all(|role| {
        role.identity() != 0
            && role.owner() == test.planners()[0].identity()
            && role.generator() == test.planners()[0].identity()
            && role.current_meaning() != 0
    }));
}

#[test]
fn planning_inspection_is_output_only_and_compiler_use_is_deterministic() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let compile = |selection| {
        let CompilationOutcome::Accepted(accepted) =
            compiler.compile(deployment_request(selection), &Cancellation::new())
        else {
            panic!("planning fixture accepts");
        };
        accepted
    };

    let without = compile(InspectSelection::none());
    let first = compile(InspectSelection::planning());
    let repeated = compile(InspectSelection::all());
    assert_eq!(
        without.planning_foundation_fingerprint(),
        first.planning_foundation_fingerprint()
    );
    assert_eq!(
        first.planning_foundation_fingerprint(),
        repeated.planning_foundation_fingerprint()
    );
    assert!(without.inspection().planning_foundation().is_none());
    assert_eq!(
        first
            .inspection()
            .planning_foundation()
            .unwrap()
            .fingerprint(),
        first.planning_foundation_fingerprint().unwrap()
    );
}

#[test]
fn rejected_and_cancelled_compiles_publish_no_planning_foundation() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let malformed = CompilationRequest::new(
        ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", b"fn broken(\n")]),
        Root::Image,
    )
    .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
    .with_inspection(InspectSelection::all());
    let CompilationOutcome::Rejected(rejected) = compiler.compile(malformed, &Cancellation::new())
    else {
        panic!("malformed source rejects");
    };
    assert!(rejected.inspection().planning_foundation().is_none());

    let cancellation = Cancellation::new();
    cancellation.cancel();
    assert!(matches!(
        compiler.compile(deployment_request(InspectSelection::all()), &cancellation),
        CompilationOutcome::Cancelled
    ));
}
