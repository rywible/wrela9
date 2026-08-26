use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;
use wrela_compiler::{
    ArchitectureProfile, Cancellation, CompilationOutcome, CompilationRequest, Compiler,
    CompilerInstallation, DiagnosticValue, FacilityBindingAvailability, FacilityEndpointOwnership,
    FacilityFlagshipRule, FacilityKind, FacilityLossPolicy, FacilityReplayAuthority,
    FacilityReplayRule, FacilitySemanticCapacity, FacilitySharedRole, FacilitySharing,
    FacilityShutdown, GeneratedRoleKind, IdentityDomain, InspectSelection, LayoutCostKind,
    LogicalProtection, LogicalRegionKind, PlannerKind, PlanningBinding, PlanningCapability,
    ProjectFile, ProjectSnapshot, RequirementBounds, RequirementCategory, Root,
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

#[test]
fn input_facility_requires_an_explicit_build_wired_owner() {
    let source = br#"from core import facilities

@image
fn build() -> Image:
    input = facilities.Input.new()
    return Image.new(input=input)
"#;
    let compiler = Compiler::open(CompilerInstallation::layer1()).unwrap();
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    );
    let CompilationOutcome::Rejected(rejected) = outcome else {
        panic!("Input without its explicit owning Actor must reject: {outcome:#?}");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.argument_count")
    );
}

#[test]
fn every_facility_requires_explicit_supervisor_and_loss_configuration() {
    let source = br#"from core import facilities

@image
fn build() -> Image:
    display = facilities.Display.new()
    return Image.new(display=display)
"#;
    let compiler = Compiler::open(CompilerInstallation::layer1()).unwrap();
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    );
    let CompilationOutcome::Rejected(rejected) = outcome else {
        panic!("Facility without supervisor/loss configuration must reject: {outcome:#?}");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.argument_count")
    );
}

#[test]
fn entropy_is_rejected_from_replayable_gameplay() {
    let source = br#"from core import facilities

@actor
struct Coordinator implements facilities.FacilityActor:
    pure fn facility_identity(read self) -> u64:
        return 1
    pub async fn run(self):
        pass

@image
fn build() -> Image:
    coordinator = Coordinator()
    entropy = facilities.Entropy.new(supervisor=coordinator, loss=facilities.SELECTING_IMAGE_POLICY, replay=facilities.REPLAYABLE_GAMEPLAY)
    return Image.new(entropy=entropy)
"#;
    let compiler = Compiler::open(CompilerInstallation::layer1()).unwrap();
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    );
    let CompilationOutcome::Rejected(rejected) = outcome else {
        panic!("Entropy cannot enter replayable gameplay: {outcome:#?}");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "admission.facility_replay"),
        "{:#?}",
        (
            rejected.diagnostics(),
            rejected.inspection().constructions()
        )
    );
}

#[test]
fn selected_loss_policy_must_match_the_authenticated_facility_rule() {
    let source = br#"from core import facilities

@actor
struct Coordinator implements facilities.FacilityActor:
    pure fn facility_identity(read self) -> u64:
        return 1
    pub async fn run(self):
        pass

@image
fn build() -> Image:
    coordinator = Coordinator()
    display = facilities.Display.new(supervisor=coordinator, loss=facilities.SELECTING_IMAGE_POLICY, replay=facilities.REPLAYABLE_GAMEPLAY)
    return Image.new(display=display)
"#;
    let outcome = Compiler::open(CompilerInstallation::layer1())
        .unwrap()
        .compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
                Root::Image,
            )
            .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
            .with_inspection(InspectSelection::all()),
            &Cancellation::new(),
        );
    let CompilationOutcome::Rejected(rejected) = outcome else {
        panic!("a source-selected loss policy cannot disagree with its contract: {outcome:#?}");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "admission.facility_loss_policy")
    );
}

#[test]
fn duplicate_facility_instances_are_rejected_during_admission() {
    let source = br#"from core import facilities

@actor
struct Coordinator implements facilities.FacilityActor:
    pure fn facility_identity(read self) -> u64:
        return 1
    pub async fn run(self):
        pass

@image
fn build() -> Image:
    coordinator = Coordinator()
    first = facilities.Display.new(supervisor=coordinator, loss=facilities.CONTROLLED_SHUTDOWN, replay=facilities.REPLAYABLE_GAMEPLAY)
    second = facilities.Display.new(supervisor=coordinator, loss=facilities.CONTROLLED_SHUTDOWN, replay=facilities.REPLAYABLE_GAMEPLAY)
    return Image.new(first=first, second=second)
"#;
    let compiler = Compiler::open(CompilerInstallation::layer1()).unwrap();
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    );
    let CompilationOutcome::Rejected(rejected) = outcome else {
        panic!("duplicate Facility instances reject: {outcome:#?}");
    };
    let diagnostic = rejected
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.code() == "admission.facility_cardinality")
        .expect("typed Facility cardinality diagnostic");
    assert!(
        diagnostic
            .typed_parameters()
            .contains(&("selected".into(), DiagnosticValue::Unsigned(2),))
    );
    assert!(
        diagnostic
            .typed_parameters()
            .contains(&("minimum".into(), DiagnosticValue::Unsigned(0),))
    );
    assert!(
        diagnostic
            .typed_parameters()
            .contains(&("maximum".into(), DiagnosticValue::Unsigned(1),))
    );
    assert_eq!(diagnostic.labels().len(), 1);
    assert_eq!(diagnostic.labels()[0].role(), "related");
    assert!(rejected.inspection().planning_foundation().is_none());
}

#[test]
fn helper_module_facility_cardinality_diagnostic_has_exact_stable_provenance() {
    let root = ProjectFile::new(
        "src/image.wr",
        br#"from core import facilities
from game import display_factory

@actor
struct Coordinator implements facilities.FacilityActor:
    pure fn facility_identity(read self) -> u64:
        return 1
    pub async fn run(self):
        pass

@image
fn build() -> Image:
    coordinator = Coordinator()
    first = display_factory.first(supervisor=coordinator)
    second = display_factory.second(supervisor=coordinator)
    return Image.new(first=first, second=second)
"#,
    );
    let helper = ProjectFile::new(
        "src/game/display_factory.wr",
        br#"from core import facilities

pub pure fn first(supervisor: any facilities.FacilityActor) -> facilities.Display:
    return facilities.Display.new(supervisor=supervisor, loss=facilities.CONTROLLED_SHUTDOWN, replay=facilities.REPLAYABLE_GAMEPLAY)

pub pure fn second(supervisor: any facilities.FacilityActor) -> facilities.Display:
    return facilities.Display.new(supervisor=supervisor, loss=facilities.CONTROLLED_SHUTDOWN, replay=facilities.REPLAYABLE_GAMEPLAY)
"#,
    );
    let compile = |reverse: bool| {
        let files = if reverse {
            vec![helper.clone(), root.clone()]
        } else {
            vec![root.clone(), helper.clone()]
        };
        Compiler::open(CompilerInstallation::layer1())
            .unwrap()
            .compile(
                CompilationRequest::new(ProjectSnapshot::new(files), Root::Image)
                    .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
                    .with_inspection(InspectSelection::all()),
                &Cancellation::new(),
            )
    };
    let extract = |outcome: CompilationOutcome| {
        let CompilationOutcome::Rejected(rejected) = outcome else {
            panic!("helper-created duplicate Display Facilities reject: {outcome:#?}");
        };
        let diagnostic = rejected
            .diagnostics()
            .iter()
            .find(|diagnostic| diagnostic.code() == "admission.facility_cardinality")
            .expect("Facility cardinality diagnostic")
            .clone();
        let mut sites = rejected
            .inspection()
            .constructions()
            .iter()
            .filter(|construction| construction.site().path() == "src/game/display_factory.wr")
            .map(|construction| (construction.identity(), construction.site().clone()))
            .collect::<Vec<_>>();
        sites.sort_by_key(|(identity, _)| *identity);
        (diagnostic, sites)
    };
    let (forward, forward_sites) = extract(compile(false));
    let (reversed, reversed_sites) = extract(compile(true));
    assert_eq!(forward, reversed);
    assert_eq!(forward_sites, reversed_sites);
    assert_eq!(forward_sites.len(), 2);
    assert_eq!(forward.primary(), &forward_sites[0].1);
    assert_eq!(forward.labels().len(), 1);
    assert_eq!(forward.labels()[0].role(), "related");
    assert_eq!(forward.labels()[0].range(), &forward_sites[1].1);
    for (ordinal, (identity, _)) in forward_sites.iter().enumerate() {
        assert!(forward.typed_parameters().contains(&(
            format!("construction_{ordinal}").into(),
            DiagnosticValue::Identity {
                domain: IdentityDomain::Construction,
                digest: *identity,
            },
        )));
    }
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
fn selected_current_facilities_publish_complete_verified_domain_plans() {
    let source = br#"from core import facilities

@actor
struct Coordinator implements facilities.FacilityActor:
    pure fn facility_identity(read self) -> u64:
        return 1
    pub async fn run(self):
        pass

@image
fn build() -> Image:
    coordinator = Coordinator()
    display = facilities.Display.new(supervisor=coordinator, loss=facilities.CONTROLLED_SHUTDOWN, replay=facilities.REPLAYABLE_GAMEPLAY)
    input = facilities.Input.new(owner=coordinator, supervisor=coordinator, loss=facilities.CONTROLLED_SHUTDOWN, replay=facilities.REPLAYABLE_GAMEPLAY)
    events = facilities.EventStore.new(supervisor=coordinator, loss=facilities.CONTROLLED_SHUTDOWN, replay=facilities.REPLAYABLE_GAMEPLAY)
    clock = facilities.MonotonicClock.new(supervisor=coordinator, loss=facilities.CONTROLLED_SHUTDOWN, replay=facilities.REPLAYABLE_GAMEPLAY)
    entropy = facilities.Entropy.new(supervisor=coordinator, loss=facilities.SELECTING_IMAGE_POLICY, replay=facilities.NON_REPLAYABLE_FACILITY)
    telemetry = facilities.Telemetry.new(supervisor=coordinator, loss=facilities.DISABLE_AND_CONTINUE, replay=facilities.REPLAYABLE_GAMEPLAY)
    return Image.new(display=display, input=input, events=events, clock=clock, entropy=entropy, telemetry=telemetry)
"#;
    let accepted = accepted(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::planning()),
    );
    let planning = accepted.inspection().planning_foundation().unwrap();

    assert_eq!(planning.facility_contracts().len(), 6);
    assert_eq!(planning.facility_domain_plans().len(), 6);
    assert!(
        planning
            .facility_contracts()
            .windows(2)
            .all(|contracts| contracts[0].kind() < contracts[1].kind())
    );
    assert!(
        planning
            .facility_domain_plans()
            .windows(2)
            .all(|plans| plans[0].identity() < plans[1].identity())
    );
    assert_eq!(
        planning
            .facility_domain_plans()
            .iter()
            .map(|plan| plan.kind())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            FacilityKind::Display,
            FacilityKind::Input,
            FacilityKind::EventStore,
            FacilityKind::MonotonicClock,
            FacilityKind::Entropy,
            FacilityKind::Telemetry,
        ])
    );
    assert!(planning.facility_contracts().iter().all(|contract| {
        contract.allows_deployment()
            && contract.allows_test()
            && contract.minimum_instances() == 0
            && contract.maximum_instances() == 1
            && contract.maximum_exported_endpoints() > 0
            && !contract.generated_roles().is_empty()
            && !contract.semantic_capacities().is_empty()
            && !contract.required_capabilities().is_empty()
            && contract.external_binding().is_some()
            && contract.current_meaning() != 0
            && contract.fingerprint() != 0
            && contract.identity() != 0
            && contract.context_receipt() != 0
            && contract.maximum_recovery_attempts() > 0
            && contract.ambient_binding_unavailability_is_boot_failure()
    }));
    let contracts = planning
        .facility_contracts()
        .iter()
        .map(|contract| (contract.kind(), contract))
        .collect::<BTreeMap<_, _>>();
    let virtio = BTreeSet::from([
        PlanningCapability::PciVirtioModern,
        PlanningCapability::SplitVirtqueue,
        PlanningCapability::SharedIntx,
        PlanningCapability::DmaOwnership,
    ]);
    for kind in [
        FacilityKind::Display,
        FacilityKind::Input,
        FacilityKind::EventStore,
        FacilityKind::Entropy,
        FacilityKind::Telemetry,
    ] {
        assert_eq!(
            contracts[&kind]
                .required_capabilities()
                .iter()
                .copied()
                .collect::<BTreeSet<_>>(),
            virtio
        );
    }
    assert_eq!(
        contracts[&FacilityKind::MonotonicClock].required_capabilities(),
        &[PlanningCapability::MonotonicCounter]
    );
    assert_eq!(
        contracts[&FacilityKind::Display].external_binding(),
        Some(PlanningBinding::Display)
    );
    assert_eq!(
        contracts[&FacilityKind::Input].external_binding(),
        Some(PlanningBinding::Input)
    );
    assert_eq!(
        contracts[&FacilityKind::EventStore].external_binding(),
        Some(PlanningBinding::EventStore)
    );
    assert_eq!(
        contracts[&FacilityKind::MonotonicClock].external_binding(),
        Some(PlanningBinding::MonotonicClock)
    );
    assert_eq!(
        contracts[&FacilityKind::Entropy].external_binding(),
        Some(PlanningBinding::Entropy)
    );
    assert_eq!(
        contracts[&FacilityKind::Telemetry].external_binding(),
        Some(PlanningBinding::Telemetry)
    );
    assert_eq!(
        contracts[&FacilityKind::Display].semantic_capacities(),
        &[FacilitySemanticCapacity::FrameBuffers(3)]
    );
    assert_eq!(
        contracts[&FacilityKind::Input].semantic_capacities(),
        &[FacilitySemanticCapacity::InputTransitions(256)]
    );
    assert_eq!(
        contracts[&FacilityKind::EventStore].semantic_capacities(),
        &[FacilitySemanticCapacity::EventSlots(65_536)]
    );
    assert_eq!(
        contracts[&FacilityKind::MonotonicClock].semantic_capacities(),
        &[FacilitySemanticCapacity::ClockWaiters(1024)]
    );
    assert_eq!(
        contracts[&FacilityKind::Entropy].semantic_capacities(),
        &[FacilitySemanticCapacity::EntropyRequestBytes(4096)]
    );
    assert_eq!(
        contracts[&FacilityKind::Telemetry].semantic_capacities(),
        &[FacilitySemanticCapacity::TelemetryRingRecords(4096)]
    );
    assert_eq!(
        contracts[&FacilityKind::Input].endpoint_ownership(),
        FacilityEndpointOwnership::BuildWiredActor
    );
    assert!(
        contracts
            .iter()
            .filter(|(kind, _)| **kind != FacilityKind::Input)
            .all(|(_, contract)| contract.endpoint_ownership()
                == FacilityEndpointOwnership::FacilityInstance)
    );
    assert_eq!(
        contracts[&FacilityKind::Display].sharing(),
        FacilitySharing::Exclusive
    );
    assert_eq!(
        contracts[&FacilityKind::MonotonicClock].sharing(),
        FacilitySharing::RegisteredDisjoint {
            role: FacilitySharedRole::MonotonicCounter,
            maximum_units: 1024,
        }
    );
    assert_eq!(
        contracts[&FacilityKind::Entropy].sharing(),
        FacilitySharing::RegisteredDisjoint {
            role: FacilitySharedRole::EntropyQueue,
            maximum_units: 16,
        }
    );
    let flagship = planning
        .facility_contracts()
        .iter()
        .filter(|contract| contract.required_by_flagship())
        .map(|contract| contract.kind())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        flagship,
        BTreeSet::from([
            FacilityKind::Display,
            FacilityKind::Input,
            FacilityKind::EventStore,
            FacilityKind::MonotonicClock,
            FacilityKind::Telemetry,
        ])
    );
    let entropy = planning
        .facility_contracts()
        .iter()
        .find(|contract| contract.kind() == FacilityKind::Entropy)
        .unwrap();
    assert!(!entropy.allowed_in_replayable_gameplay());
    assert_eq!(
        entropy.replay_rule(),
        FacilityReplayRule::ExcludedFromReplayableGameplay
    );
    assert_eq!(
        entropy.flagship_rule(),
        FacilityFlagshipRule::SelectingImageOptional
    );
    assert!(
        planning
            .facility_contracts()
            .iter()
            .filter(|contract| contract.kind() != FacilityKind::Entropy)
            .all(|contract| contract.flagship_rule()
                == FacilityFlagshipRule::Required {
                    loss_policy: contract.loss_policy(),
                })
    );
    assert!(
        planning
            .facility_contracts()
            .iter()
            .all(|contract| { contract.physical_sharing_is_registered_disjoint_or_exclusive() })
    );
    assert_eq!(
        planning
            .facility_contracts()
            .iter()
            .find(|contract| contract.kind() == FacilityKind::Telemetry)
            .unwrap()
            .loss_policy(),
        FacilityLossPolicy::DisableAndContinue
    );
    assert_eq!(
        planning
            .facility_contracts()
            .iter()
            .find(|contract| contract.kind() == FacilityKind::EventStore)
            .unwrap()
            .shutdown(),
        FacilityShutdown::FlushCommittedAndQuiesce
    );
    assert!(planning.facility_domain_plans().iter().all(|plan| {
        plan.instance_identity() != 0
            && plan.contract_fingerprint() != 0
            && plan.contract_identity() != 0
            && plan.contract_current_meaning() != 0
            && plan.generated_role_count() > 0
            && plan.requirement_count() > 0
            && plan.current_meaning() != 0
    }));
    let facility_requirements = planning
        .requirements()
        .iter()
        .filter(|requirement| {
            matches!(
                requirement.subject(),
                wrela_compiler::RequirementSubject::FacilityInstance(_)
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(facility_requirements.len(), 81);
    assert!(facility_requirements.iter().any(|requirement| matches!(
        requirement.bounds(),
        RequirementBounds::FacilityRecovery {
            loss_policy: FacilityLossPolicy::SelectingImagePolicy,
            maximum_attempts: 3,
            ..
        }
    )));
    assert!(facility_requirements.iter().any(|requirement| matches!(
        requirement.bounds(),
        RequirementBounds::FacilityReplay {
            selected: FacilityReplayAuthority::NonReplayableFacility,
            rule: FacilityReplayRule::ExcludedFromReplayableGameplay,
        }
    )));
    assert!(facility_requirements.iter().any(|requirement| matches!(
        requirement.bounds(),
        RequirementBounds::FacilityFlagship(FacilityFlagshipRule::Required {
            loss_policy: FacilityLossPolicy::ControlledShutdown,
        })
    )));
    assert_eq!(
        facility_requirements
            .iter()
            .filter(|requirement| matches!(
                requirement.bounds(),
                RequirementBounds::FacilityEndpoint {
                    ownership: FacilityEndpointOwnership::BuildWiredActor,
                    input_owner: Some(_),
                    ..
                }
            ))
            .count(),
        1
    );
    assert_eq!(
        facility_requirements
            .iter()
            .filter(|requirement| matches!(
                requirement.bounds(),
                RequirementBounds::FacilityRecovery { supervisor, .. }
                    if supervisor.identity() != 0 && supervisor.current_meaning() != 0
            ))
            .count(),
        6
    );
    assert_eq!(
        facility_requirements
            .iter()
            .filter(|requirement| matches!(
                requirement.bounds(),
                RequirementBounds::FacilityBindingAvailability(
                    FacilityBindingAvailability::BootFailure
                )
            ))
            .count(),
        6
    );
    assert_eq!(
        facility_requirements
            .iter()
            .filter(|requirement| matches!(
                requirement.bounds(),
                RequirementBounds::FacilityShutdown(_)
            ))
            .count(),
        6
    );

    let assignment = accepted.inspection().whole_image_assignment().unwrap();
    let service = accepted.inspection().service_plan().unwrap();
    let drivers = service
        .classes()
        .iter()
        .filter(|class| class.kind() == wrela_compiler::ServiceClassKind::Driver)
        .collect::<Vec<_>>();
    assert_eq!(drivers.len(), 7);
    for driver in drivers {
        let requirement = planning
            .requirements()
            .iter()
            .find(|requirement| requirement.reference() == driver.requirement())
            .unwrap();
        let wrela_compiler::RequirementSubject::GeneratedRole(role_identity) =
            requirement.subject()
        else {
            panic!("Driver service is bound to a generated-role Requirement");
        };
        let executable = planning
            .generated_roles()
            .iter()
            .find(|role| role.identity() == role_identity)
            .unwrap()
            .executable();
        assert_eq!(
            driver.core(),
            assignment
                .placements()
                .iter()
                .find(|placement| placement.executable() == executable)
                .unwrap()
                .core()
        );
    }
}

fn input_with_actor_meaning(value: u64) -> CompilationRequest {
    let source = format!(
        r#"from core import facilities

@actor
struct Coordinator implements facilities.FacilityActor:
    pure fn facility_identity(read self) -> u64:
        return {value}
    pub async fn run(self):
        pass

@image
fn build() -> Image:
    coordinator = Coordinator()
    input = facilities.Input.new(owner=coordinator, supervisor=coordinator, loss=facilities.CONTROLLED_SHUTDOWN, replay=facilities.REPLAYABLE_GAMEPLAY)
    return Image.new(input=input)
"#
    );
    CompilationRequest::new(
        ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source.into_bytes())]),
        Root::Image,
    )
    .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
    .with_inspection(InspectSelection::planning())
}

#[test]
fn actor_meaning_edits_rebind_input_facility_planning_meanings_not_identities() {
    let before = accepted(input_with_actor_meaning(1));
    let after = accepted(input_with_actor_meaning(2));
    let before = before.inspection().planning_foundation().unwrap();
    let after = after.inspection().planning_foundation().unwrap();
    let before_plan = &before.facility_domain_plans()[0];
    let after_plan = &after.facility_domain_plans()[0];

    assert_eq!(before_plan.kind(), FacilityKind::Input);
    assert_eq!(before_plan.identity(), after_plan.identity());
    assert_eq!(
        before_plan.instance_identity(),
        after_plan.instance_identity()
    );
    assert_ne!(before_plan.current_meaning(), after_plan.current_meaning());
    let before_planner = before
        .planners()
        .iter()
        .find(|planner| planner.kind() == PlannerKind::Facility(FacilityKind::Input))
        .unwrap();
    let after_planner = after
        .planners()
        .iter()
        .find(|planner| planner.kind() == PlannerKind::Facility(FacilityKind::Input))
        .unwrap();
    assert_eq!(before_planner.identity(), after_planner.identity());
    assert_ne!(
        before_planner.current_meaning(),
        after_planner.current_meaning()
    );

    let facility_requirements = |planning: &wrela_compiler::PlanningFoundationObservation| {
        planning
            .requirements()
            .iter()
            .filter(|requirement| {
                matches!(
                    requirement.subject(),
                    wrela_compiler::RequirementSubject::FacilityInstance(_)
                ) && matches!(
                    requirement.category(),
                    RequirementCategory::FacilityOwnership | RequirementCategory::Recovery
                )
            })
            .map(|requirement| (requirement.reference(), requirement.current_meaning()))
            .collect::<Vec<_>>()
    };
    let before_requirements = facility_requirements(before);
    let after_requirements = facility_requirements(after);
    assert_eq!(
        before_requirements
            .iter()
            .map(|(identity, _)| identity)
            .collect::<Vec<_>>(),
        after_requirements
            .iter()
            .map(|(identity, _)| identity)
            .collect::<Vec<_>>()
    );
    assert_ne!(before_requirements, after_requirements);

    let actor_refs = |planning: &wrela_compiler::PlanningFoundationObservation| {
        planning
            .requirements()
            .iter()
            .filter_map(|requirement| match requirement.bounds() {
                RequirementBounds::FacilityEndpoint {
                    input_owner: Some(owner),
                    ..
                } => Some(*owner),
                RequirementBounds::FacilityRecovery { supervisor, .. } => Some(*supervisor),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let before_refs = actor_refs(before);
    let after_refs = actor_refs(after);
    assert_eq!(before_refs.len(), 2);
    assert_eq!(
        before_refs
            .iter()
            .map(|reference| reference.identity())
            .collect::<Vec<_>>(),
        after_refs
            .iter()
            .map(|reference| reference.identity())
            .collect::<Vec<_>>()
    );
    assert_ne!(
        before_refs
            .iter()
            .map(|reference| reference.current_meaning())
            .collect::<Vec<_>>(),
        after_refs
            .iter()
            .map(|reference| reference.current_meaning())
            .collect::<Vec<_>>()
    );
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
            wrela_compiler::RequirementSubject::Pool(_)
            | wrela_compiler::RequirementSubject::FacilityInstance(_) => return true,
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
    assert_eq!(
        without.whole_image_assignment_fingerprint(),
        first.whole_image_assignment_fingerprint()
    );
    assert_eq!(
        first.whole_image_assignment_fingerprint(),
        repeated.whole_image_assignment_fingerprint()
    );
    assert_eq!(
        without.service_plan_fingerprint(),
        first.service_plan_fingerprint()
    );
    assert_eq!(
        first.service_plan_fingerprint(),
        repeated.service_plan_fingerprint()
    );
    assert_eq!(
        without.logical_image_layout_fingerprint(),
        first.logical_image_layout_fingerprint()
    );
    assert_eq!(
        first.logical_image_layout_fingerprint(),
        repeated.logical_image_layout_fingerprint()
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
            accepted.whole_image_assignment_fingerprint(),
            accepted.service_plan_fingerprint(),
            accepted.logical_image_layout_fingerprint(),
            accepted
                .inspection()
                .planning_foundation()
                .expect("planning selected")
                .clone(),
            accepted
                .inspection()
                .whole_image_assignment()
                .expect("planning assignment selected")
                .clone(),
            accepted
                .inspection()
                .service_plan()
                .expect("planning Service Plan selected")
                .clone(),
            accepted
                .inspection()
                .logical_image_layout()
                .expect("planning Logical Image Layout selected")
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
    assert!(rejected.inspection().whole_image_assignment().is_none());
    assert!(rejected.inspection().logical_image_layout().is_none());

    let cancellation = Cancellation::new();
    cancellation.cancel();
    assert!(matches!(
        compiler.compile(deployment_request(InspectSelection::all()), &cancellation),
        CompilationOutcome::Cancelled
    ));
}

#[test]
fn whole_image_solver_places_every_executable_and_discharges_every_requirement_once() {
    let outcome = Compiler::open(CompilerInstallation::layer1())
        .unwrap()
        .compile(
            deployment_request(InspectSelection::all()),
            &Cancellation::new(),
        );
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("the minimal deployment Image must be admitted: {outcome:#?}");
    };
    let inspection = accepted.inspection();
    let assignment = inspection
        .whole_image_assignment()
        .expect("planning inspection includes the verified canonical assignment");
    let core = inspection.core_program().expect("Core inspection");
    let foundation = inspection
        .planning_foundation()
        .expect("planning foundation inspection");
    let flow = inspection.flow_program().expect("Flow inspection");

    assert_eq!(assignment.placements().len(), core.executables().len());
    assert_eq!(
        assignment.discharges().len(),
        foundation.requirements().len() + flow.requirements().len()
    );
    assert_eq!(
        assignment
            .placements()
            .iter()
            .map(|placement| placement.executable())
            .collect::<BTreeSet<_>>()
            .len(),
        assignment.placements().len(),
        "every executable is placed exactly once"
    );
    assert_eq!(
        assignment
            .discharges()
            .iter()
            .map(|discharge| discharge.requirement())
            .collect::<BTreeSet<_>>()
            .len(),
        assignment.discharges().len(),
        "every requirement is discharged exactly once"
    );
    for role in foundation.generated_roles() {
        assert_eq!(
            assignment
                .placements()
                .iter()
                .filter(|placement| placement.executable() == role.executable())
                .count(),
            1,
            "each generated executable has one placement"
        );
    }
    let required_bindings = foundation
        .requirements()
        .iter()
        .filter_map(|requirement| match requirement.bounds() {
            RequirementBounds::Binding { kind, minimum, .. } if *minimum > 0 => {
                Some((requirement.reference(), *kind))
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(assignment.bindings().len(), required_bindings.len());
    assert!(assignment.bindings().iter().all(|binding| {
        required_bindings.get(&binding.requirement()) == Some(&binding.binding())
    }));
}

#[test]
fn compiler_assignment_enforces_actor_handler_affinity_and_flow_capacity() {
    let source = br#"@actor
struct Receiver:
    pub async fn first(self):
        pass

    pub async fn second(self):
        pass

@image
fn build() -> Image:
    receiver = Receiver()
    return Image.new(receiver=receiver)
"#;
    let outcome = Compiler::open(CompilerInstallation::layer1())
        .unwrap()
        .compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
                Root::Image,
            )
            .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
            .with_inspection(InspectSelection::all()),
            &Cancellation::new(),
        );
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("bounded Actor Image must admit: {outcome:#?}");
    };
    let flow = accepted.inspection().flow_program().unwrap();
    let assignment = accepted.inspection().whole_image_assignment().unwrap();
    let actor = flow
        .actors()
        .iter()
        .find(|actor| actor.handlers().len() == 2)
        .unwrap();
    let handler_cores = actor
        .handlers()
        .iter()
        .map(|handler| {
            assignment
                .placements()
                .iter()
                .find(|placement| placement.executable() == *handler)
                .unwrap()
                .core()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(handler_cores.len(), 1, "one Actor has one permanent core");
    assert!(assignment.discharges().iter().any(|discharge| {
        discharge.requirement() == actor.permanent_core_requirement()
            && discharge.kind() == wrela_compiler::DischargeKind::Placed
    }));
    assert!(
        flow.requirements()
            .iter()
            .filter(|requirement| requirement.actor() == actor.identity())
            .all(|requirement| assignment
                .discharges()
                .iter()
                .any(|discharge| discharge.requirement() == requirement.identity()))
    );
}

fn compile_service_deadline(slack: u64) -> CompilationOutcome {
    let source = format!(
        r#"from core import actor as actors

@actor
struct Worker:
    pub async fn run(self):
        mut group = actors.Group.all(bound=1u64)
        group.logical_deadline(epoch=1u64, slack={slack}u64)
        _ = actors.Group.complete(take group)

@image
fn build() -> Image:
    worker = Worker()
    return Image.new(worker=worker)
"#
    )
    .into_bytes();
    Compiler::open(CompilerInstallation::layer1())
        .unwrap()
        .compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
                Root::Image,
            )
            .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
            .with_inspection(InspectSelection::all()),
            &Cancellation::new(),
        )
}

fn exact_service_deadline_outcome() -> &'static CompilationOutcome {
    static OUTCOME: OnceLock<CompilationOutcome> = OnceLock::new();
    OUTCOME.get_or_init(|| compile_service_deadline(85))
}

#[test]
fn compiler_rejects_a_positive_but_unmeetable_verified_group_deadline() {
    assert!(matches!(
        exact_service_deadline_outcome(),
        CompilationOutcome::Accepted(_)
    ));
    let outcome = compile_service_deadline(84);
    let CompilationOutcome::Rejected(rejected) = outcome else {
        panic!("positive slack below verified Flow work must reject: {outcome:#?}");
    };
    assert_eq!(
        rejected.diagnostics()[0].code(),
        "admission.service_conflict"
    );
    assert!(
        rejected.diagnostics()[0]
            .typed_parameters()
            .contains(&("required_units".into(), DiagnosticValue::Unsigned(85)))
    );
    assert!(
        rejected.diagnostics()[0]
            .typed_parameters()
            .contains(&("available_units".into(), DiagnosticValue::Unsigned(84)))
    );
    assert!(rejected.inspection().flow_program().is_some());
    assert!(rejected.inspection().whole_image_assignment().is_none());
    assert!(rejected.inspection().service_plan().is_none());
}

#[test]
fn compiler_planning_inspection_reports_the_verified_deterministic_service_plan() {
    let outcome = exact_service_deadline_outcome();
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("bounded Actor service must admit: {outcome:#?}");
    };
    let inspection = accepted.inspection();
    let assignment = inspection.whole_image_assignment().unwrap();
    let service = inspection
        .service_plan()
        .expect("planning inspection includes the verified Service Plan");
    assert_eq!(
        service.whole_image_assignment_fingerprint(),
        assignment.fingerprint()
    );
    for kind in [
        wrela_compiler::ServiceClassKind::Ingress,
        wrela_compiler::ServiceClassKind::ActorTurn,
        wrela_compiler::ServiceClassKind::GroupChild,
    ] {
        assert!(service.classes().iter().any(|class| class.kind() == kind));
    }
    assert!(
        service
            .cores()
            .iter()
            .all(|core| core.cycle_units() <= core.maximum_cycle_units())
    );
    assert!(service.classes().iter().all(|class| {
        class.quota() > 0
            && class.maximum_response_units() <= class.maximum_delay_units()
            && class.maximum_cancellation_response_units()
                <= class.maximum_cancellation_delay_units()
    }));
}

#[test]
fn compiler_constructs_the_complete_canonical_logical_image_layout() {
    let accepted = accepted(deployment_request(InspectSelection::planning()));
    let inspection = accepted.inspection();
    let assignment = inspection.whole_image_assignment().unwrap();
    let service = inspection.service_plan().unwrap();
    let layout = inspection
        .logical_image_layout()
        .expect("planning inspection includes the verified Logical Image Layout");

    assert_eq!(layout.phase_schema(), "wrela.logical-image-layout.v1");
    assert_eq!(
        layout.whole_image_assignment_fingerprint(),
        assignment.fingerprint()
    );
    assert_eq!(layout.service_plan_fingerprint(), service.fingerprint());
    assert_eq!(
        layout
            .regions()
            .iter()
            .map(|region| (region.kind(), region.protection()))
            .collect::<Vec<_>>(),
        vec![
            (
                LogicalRegionKind::BootReservation,
                LogicalProtection::Sealed
            ),
            (
                LogicalRegionKind::Executable,
                LogicalProtection::ReadExecute
            ),
            (
                LogicalRegionKind::ImmutableData,
                LogicalProtection::ReadOnlyNoExecute,
            ),
            (
                LogicalRegionKind::PerCoreMutable,
                LogicalProtection::ReadWriteNoExecute,
            ),
            (
                LogicalRegionKind::SharedMutable,
                LogicalProtection::ReadWriteNoExecute,
            ),
            (
                LogicalRegionKind::DmaOwned,
                LogicalProtection::DmaVisibleReadWriteNoExecute,
            ),
        ]
    );
    assert!(
        layout
            .regions()
            .windows(2)
            .all(|pair| pair[0].end() <= pair[1].start())
    );
    assert!(layout.allocations().windows(2).all(|pair| {
        (pair[0].region(), pair[0].requirement(), pair[0].local_key())
            < (pair[1].region(), pair[1].requirement(), pair[1].local_key())
    }));
    assert!(layout.allocations().iter().all(|allocation| {
        allocation.start() % allocation.alignment() == 0
            && allocation.end() - allocation.start() == allocation.envelope_bytes()
    }));
    let requirements = inspection.planning_foundation().unwrap().requirements();
    assert!(
        layout
            .allocations()
            .iter()
            .filter(|allocation| allocation.dma_owned())
            .all(|allocation| requirements.iter().any(|requirement| {
                requirement.reference() == allocation.requirement()
                    && matches!(
                        requirement.bounds(),
                        RequirementBounds::Binding {
                            kind: PlanningBinding::Display
                                | PlanningBinding::Input
                                | PlanningBinding::EventStore
                                | PlanningBinding::Entropy
                                | PlanningBinding::Telemetry
                                | PlanningBinding::Terminal,
                            minimum: 1..,
                            ..
                        }
                    )
            }))
    );
    assert_eq!(
        layout
            .reservations()
            .iter()
            .filter(|reservation| reservation.is_guard())
            .count(),
        17,
        "one null guard and four stack guards for each of four symbolic cores"
    );
    assert_eq!(
        layout
            .reservations()
            .iter()
            .filter(|reservation| reservation.requirement().is_some())
            .count(),
        6,
        "Boot, terminal, and per-core Panic reservations retain their exact RequirementRefs"
    );
    assert!(
        layout
            .ledger()
            .iter()
            .any(|entry| entry.kind() == LayoutCostKind::EnvelopePayload)
    );
    assert!(layout.ledger().iter().any(|entry| {
        entry.kind() == LayoutCostKind::EnvelopePayload
            && entry.requirement().is_some()
            && entry.envelope().is_some()
            && entry.multiplicity() > 1
    }));
    assert!(
        layout
            .ledger()
            .iter()
            .any(|entry| entry.kind() == LayoutCostKind::Guard)
    );
    assert_eq!(
        layout
            .ledger()
            .iter()
            .map(|entry| entry.bytes())
            .sum::<u64>(),
        layout.reserved_bytes()
    );
    assert!(layout.reserved_bytes() <= layout.total_ram_bytes());
    assert_eq!(layout.total_ram_bytes(), 128 * 1024 * 1024);
    assert_eq!(
        accepted.logical_image_layout_fingerprint(),
        Some(layout.fingerprint())
    );
}

#[test]
fn actor_turn_service_cost_is_handler_specific_without_charging_cheap_work_the_expensive_cost() {
    let source = br#"@actor
struct Worker:
    pub async fn light(self):
        pass

    pub async fn heavy(self):
        first = 1 + 2
        second = first * 3
        third = second + first
        _ = third * second

@image
fn build() -> Image:
    worker = Worker()
    return Image.new(worker=worker)
"#;
    let outcome = Compiler::open(CompilerInstallation::layer1())
        .unwrap()
        .compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
                Root::Image,
            )
            .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
            .with_inspection(InspectSelection::all()),
            &Cancellation::new(),
        );
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("bounded handler work admits: {outcome:#?}");
    };
    let service = accepted.inspection().service_plan().unwrap();
    let costs = service
        .classes()
        .iter()
        .filter(|class| class.kind() == wrela_compiler::ServiceClassKind::ActorTurn)
        .map(|class| class.activation_units())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        costs.len(),
        2,
        "one Actor must retain distinct cheap and expensive handler costs"
    );
}
