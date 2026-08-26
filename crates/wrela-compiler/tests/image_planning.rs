use std::collections::{BTreeMap, BTreeSet};
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

fn valued_deployment_request(value: i64, inspection: InspectSelection) -> CompilationRequest {
    CompilationRequest::new(
        ProjectSnapshot::new(vec![ProjectFile::new(
            "src/image.wr",
            format!(
                "const VALUE: i64 = {value}\n\n@image\nfn build() -> Image:\n    return Image.new(value=VALUE)\n"
            )
            .into_bytes(),
        )]),
        Root::Image,
    )
    .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
    .with_inspection(inspection)
}

fn multi_file_request(reverse: bool, inspection: InspectSelection) -> CompilationRequest {
    let root = ProjectFile::new(
        "src/image.wr",
        b"from game import values\n\n@image\nfn build() -> Image:\n    return Image.new(value=values.VALUE)\n",
    );
    let values = ProjectFile::new("src/game/values.wr", b"pub const VALUE: i64 = 42\n");
    let files = if reverse {
        vec![values, root]
    } else {
        vec![root, values]
    };
    CompilationRequest::new(ProjectSnapshot::new(files), Root::Image)
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
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            GeneratedRoleKind::Boot,
            GeneratedRoleKind::Scheduler,
            GeneratedRoleKind::Terminal,
            GeneratedRoleKind::Panic,
            GeneratedRoleKind::Shutdown,
        ])
    );
    assert!(
        planning
            .generated_roles()
            .windows(2)
            .all(|roles| roles[0].identity() < roles[1].identity()),
        "Generated Roles publish in stable identity order"
    );
    let role_kinds = planning
        .generated_roles()
        .iter()
        .map(|role| (role.identity(), role.kind()))
        .collect::<BTreeMap<_, _>>();
    for role in planning
        .generated_roles()
        .iter()
        .filter(|role| role.dependencies().len() > 1)
    {
        assert!(
            role.dependencies()
                .windows(2)
                .all(|dependencies| dependencies[0] < dependencies[1]),
            "multi-dependency Generated Roles publish direct references in identity order"
        );
        let actual = role
            .dependencies()
            .iter()
            .map(|identity| role_kinds[identity])
            .collect::<BTreeSet<_>>();
        let expected = match role.kind() {
            GeneratedRoleKind::Shutdown => {
                BTreeSet::from([GeneratedRoleKind::Scheduler, GeneratedRoleKind::Terminal])
            }
            GeneratedRoleKind::TestRuntime => BTreeSet::from([
                GeneratedRoleKind::Scheduler,
                GeneratedRoleKind::Terminal,
                GeneratedRoleKind::Shutdown,
            ]),
            kind => panic!("unexpected multi-dependency role: {kind:?}"),
        };
        assert_eq!(actual, expected);
    }
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
        let role = match requirement.subject() {
            wrela_compiler::RequirementSubject::GeneratedRole(role) => role,
            wrela_compiler::RequirementSubject::Pool(_) => return true,
        };
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
    assert!(
        planning
            .executable_demand()
            .generated_executables()
            .windows(2)
            .all(|executables| executables[0] < executables[1]),
        "generated executable additions publish in stable identity order"
    );
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
    assert!(
        test.generated_roles()
            .iter()
            .any(|role| role.kind() == GeneratedRoleKind::TestRuntime)
    );
    let role_kinds = test
        .generated_roles()
        .iter()
        .map(|role| (role.identity(), role.kind()))
        .collect::<BTreeMap<_, _>>();
    let runtime = test
        .generated_roles()
        .iter()
        .find(|role| role.kind() == GeneratedRoleKind::TestRuntime)
        .unwrap();
    assert!(
        runtime
            .dependencies()
            .windows(2)
            .all(|dependencies| dependencies[0] < dependencies[1])
    );
    assert_eq!(
        runtime
            .dependencies()
            .iter()
            .map(|identity| role_kinds[identity])
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            GeneratedRoleKind::Scheduler,
            GeneratedRoleKind::Terminal,
            GeneratedRoleKind::Shutdown,
        ])
    );
    assert_ne!(
        deployment.planners()[0].identity(),
        test.planners()[0].identity(),
        "Image-kind planner identity includes Deployment versus Test kind"
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

    let reused_forward = compiler.compile(
        multi_file_request(false, InspectSelection::planning()),
        &Cancellation::new(),
    );
    let reused_reversed = compiler.compile(
        multi_file_request(true, InspectSelection::all()),
        &Cancellation::new(),
    );
    let reopened = Compiler::open(CompilerInstallation::layer1())
        .expect("distribution reopens")
        .compile(
            multi_file_request(false, InspectSelection::planning()),
            &Cancellation::new(),
        );
    let canonical_outcome = |outcome: &CompilationOutcome| {
        let CompilationOutcome::Accepted(accepted) = outcome else {
            panic!("multi-file fixture accepts: {outcome:#?}");
        };
        (
            accepted.diagnostics().to_vec(),
            accepted.semantic_program_fingerprint(),
            accepted.planning_foundation_fingerprint(),
            accepted
                .inspection()
                .planning_foundation()
                .expect("planning selected")
                .clone(),
        )
    };
    assert_eq!(
        canonical_outcome(&reused_forward),
        canonical_outcome(&reused_reversed)
    );
    assert_eq!(
        canonical_outcome(&reused_forward),
        canonical_outcome(&reopened)
    );
}

#[test]
fn meaning_only_edits_preserve_planning_identities_and_change_current_meaning() {
    let before = accepted(valued_deployment_request(41, InspectSelection::planning()));
    let after = accepted(valued_deployment_request(42, InspectSelection::planning()));
    let before = before.inspection().planning_foundation().unwrap();
    let after = after.inspection().planning_foundation().unwrap();

    assert_ne!(before.context_identity(), after.context_identity());
    assert_ne!(before.fingerprint(), after.fingerprint());
    assert_eq!(
        before
            .planners()
            .iter()
            .map(|planner| planner.identity())
            .collect::<Vec<_>>(),
        after
            .planners()
            .iter()
            .map(|planner| planner.identity())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        before
            .domain_plans()
            .iter()
            .map(|plan| plan.identity())
            .collect::<Vec<_>>(),
        after
            .domain_plans()
            .iter()
            .map(|plan| plan.identity())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        before
            .generated_roles()
            .iter()
            .map(|role| (role.kind(), role.identity(), role.executable()))
            .collect::<Vec<_>>(),
        after
            .generated_roles()
            .iter()
            .map(|role| (role.kind(), role.identity(), role.executable()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        before
            .requirements()
            .iter()
            .map(|requirement| requirement.reference())
            .collect::<Vec<_>>(),
        after
            .requirements()
            .iter()
            .map(|requirement| requirement.reference())
            .collect::<Vec<_>>()
    );
    assert_ne!(
        before.planners()[0].current_meaning(),
        after.planners()[0].current_meaning()
    );
    assert_ne!(
        before.domain_plans()[0].current_meaning(),
        after.domain_plans()[0].current_meaning()
    );
    assert_ne!(
        before
            .generated_roles()
            .iter()
            .map(|role| role.current_meaning())
            .collect::<Vec<_>>(),
        after
            .generated_roles()
            .iter()
            .map(|role| role.current_meaning())
            .collect::<Vec<_>>()
    );
    assert_ne!(
        before
            .requirements()
            .iter()
            .map(|requirement| requirement.current_meaning())
            .collect::<Vec<_>>(),
        after
            .requirements()
            .iter()
            .map(|requirement| requirement.current_meaning())
            .collect::<Vec<_>>()
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
