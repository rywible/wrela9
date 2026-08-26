use wrela_compiler::{
    ArchitectureProfile, Cancellation, CompilationOutcome, CompilationRequest, Compiler,
    CompilerInstallation, FlowAdmissionKind, FlowCustodian, FlowEventKind, FlowGroupPolicy,
    FlowRequirementKind, FlowSendOutcome, FlowStructuredOutcome, FlowStructuredScenarioKind,
    InspectSelection, ProjectFile, ProjectSnapshot, Root,
};

fn actor_request(inspection: InspectSelection) -> CompilationRequest {
    CompilationRequest::new(
        ProjectSnapshot::new(vec![ProjectFile::new(
            "src/image.wr",
            br#"async fn yield_once() -> i64:
    return 1

@actor
struct Receiver:
    value: i64

    pub async fn receive(read self, value: i64):
        resumed = await yield_once()
        _ = value + resumed

@actor
struct Sender:
    sequence: i64

@image
fn build() -> Image:
    receiver = Receiver(value=0)
    sender = Sender(sequence=0)
    return Image.new(receiver=receiver, sender=sender)
"#,
        )]),
        Root::Image,
    )
    .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
    .with_inspection(inspection)
}

#[test]
fn authenticated_groups_derive_all_four_exact_policies_and_child_bounds() {
    let source = br#"from core import actor as actors

resource struct Token:
    value: i64

fn consume(take token: Token):
    pass

fn oldest():
    pass

fn newest():
    pass

@actor
struct Worker:
    pub async fn run(self, take first: Token, take second: Token, take third: Token, take fourth: Token, take fifth: Token, take sixth: Token, take seventh: Token, take eighth: Token):
        mut all = actors.Group.all(bound=2u64)
        all.logical_deadline(epoch=5u64, slack=1000u64)
        defer oldest()
        defer newest()
        first_child = all.child(value=take first)
        second_child = all.child(value=take second)
        first_return = actors.Group.complete_pair(take all, take first_child, take second_child)
        _ = first_return.outcome
        consume(take first_return.first)
        consume(take first_return.second)

        mut collect = actors.Group.collect(bound=2u64)
        third_child = collect.child(value=take third)
        fourth_child = collect.child(value=take fourth)
        second_return = actors.Group.complete_pair(take collect, take third_child, take fourth_child)
        _ = second_return.outcome
        consume(take second_return.first)
        consume(take second_return.second)

        mut race = actors.Group.race(bound=2u64)
        fifth_child = race.child(value=take fifth)
        sixth_child = race.child(value=take sixth)
        third_return = actors.Group.complete_pair(take race, take fifth_child, take sixth_child)
        _ = third_return.outcome
        consume(take third_return.first)
        consume(take third_return.second)

        mut supervise = actors.Group.supervise(bound=2u64)
        seventh_child = supervise.child(value=take seventh)
        eighth_child = supervise.child(value=take eighth)
        fourth_return = actors.Group.complete_pair(take supervise, take seventh_child, take eighth_child)
        _ = fourth_return.outcome
        consume(take fourth_return.first)
        consume(take fourth_return.second)

@image
fn build() -> Image:
    worker = Worker()
    return Image.new(worker=worker)
"#;
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    );
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("authenticated Group policies accept: {outcome:#?}");
    };
    let flow = accepted.inspection().flow_program().expect("Flow selected");
    let assignment = accepted
        .inspection()
        .whole_image_assignment()
        .expect("Assignment selected");
    let service = accepted
        .inspection()
        .service_plan()
        .expect("verified Service Plan selected");
    assert_eq!(
        service.whole_image_assignment_fingerprint(),
        assignment.fingerprint()
    );
    for kind in [
        wrela_compiler::ServiceClassKind::Ingress,
        wrela_compiler::ServiceClassKind::ActorTurn,
        wrela_compiler::ServiceClassKind::GroupChild,
        wrela_compiler::ServiceClassKind::Cleanup,
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
    assert_eq!(flow.groups().len(), 4);
    let policies = flow
        .groups()
        .iter()
        .map(|group| {
            assert_eq!(group.child_activation_bound(), 2);
            assert_eq!(group.child_activations().len(), 2);
            assert_eq!(group.moved_resources().len(), 2);
            assert_ne!(group.noncopyable_cancellation_authority(), 0);
            assert_ne!(group.return_home(), 0);
            group.policy()
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        policies,
        std::collections::BTreeSet::from([
            FlowGroupPolicy::All,
            FlowGroupPolicy::Collect,
            FlowGroupPolicy::Race,
            FlowGroupPolicy::Supervise,
        ])
    );
    assert_eq!(flow.group_policy_laws().len(), 4);
    for group in flow.groups() {
        let scenario = flow
            .structured_scenarios()
            .iter()
            .find(|scenario| {
                scenario.kind() == FlowStructuredScenarioKind::GroupPolicies
                    && scenario.events().iter().any(|event| {
                        event.kind() == FlowEventKind::GroupOutcomePublished
                            && event.subject() == Some(group.identity())
                    })
            })
            .expect("policy transition scenario");
        assert!(
            scenario
                .events()
                .iter()
                .any(|event| event.kind() == FlowEventKind::ChildCompleted)
        );
        assert!(
            scenario
                .events()
                .iter()
                .any(|event| event.kind() == FlowEventKind::ChildFailed)
        );
        assert_eq!(
            scenario
                .events()
                .iter()
                .any(|event| { event.kind() == FlowEventKind::SiblingCancellationRequested }),
            matches!(group.policy(), FlowGroupPolicy::All | FlowGroupPolicy::Race)
        );
        match group.policy() {
            FlowGroupPolicy::All => {
                assert_eq!(scenario.outcome(), FlowStructuredOutcome::Cancelled);
                assert_eq!(scenario.winner_order(), [0, 1]);
            }
            FlowGroupPolicy::Race => {
                assert_eq!(scenario.outcome(), FlowStructuredOutcome::Completed);
                assert_eq!(scenario.winner_order(), [1]);
            }
            FlowGroupPolicy::Collect | FlowGroupPolicy::Supervise => {
                assert_eq!(scenario.outcome(), FlowStructuredOutcome::Completed);
                assert_eq!(scenario.winner_order(), [0, 1]);
            }
        }
    }
    let logical = flow
        .groups()
        .iter()
        .find(|group| group.policy() == FlowGroupPolicy::All)
        .expect("logical All Group");
    assert_eq!(
        logical.deadline_class(),
        Some(wrela_compiler::FlowDeadlineClass::Logical)
    );
    assert_eq!(logical.deadline_slack(), Some(1000));
    assert_ne!(logical.deadline_authority(), Some(0));
    assert!(logical.maximum_uninterrupted_work_units() > 4);
    assert_eq!(logical.cleanup_actions().len(), 2);
    assert_eq!(
        logical.cleanup_execution_order(),
        [logical.cleanup_actions()[1], logical.cleanup_actions()[0]]
    );
    assert_eq!(flow.deadline_laws().len(), 1);
    assert!(flow.structured_scenarios().iter().any(|scenario| {
        scenario.kind() == FlowStructuredScenarioKind::DeadlineExceeded
            && scenario.outcome() == FlowStructuredOutcome::DeadlineExceeded
            && scenario
                .events()
                .iter()
                .all(|event| event.subject().is_some())
    }));
    assert!(flow.structured_scenarios().iter().any(|scenario| {
        scenario.kind() == FlowStructuredScenarioKind::ReverseCleanup
            && scenario
                .events()
                .windows(2)
                .all(|events| events[0].logical_coordinate() < events[1].logical_coordinate())
    }));
    assert!(!flow.model_agrees());
}

#[test]
fn nested_groups_keep_exact_receiver_places_and_innermost_message_ownership() {
    let source = br#"from core import actor as actors

resource struct Token:
    value: i64

fn consume(take token: Token):
    pass

@actor
struct Receiver:
    pub async fn receive(self, value: i64):
        pass

@actor
struct Worker:
    pub async fn run(self, receiver: Receiver, take first: Token, take second: Token):
        mut outer = actors.Group.all(bound=1u64)
        outer_child = outer.child(value=take first)
        mut inner = actors.Group.race(bound=1u64)
        inner_child = inner.child(value=take second)
        await send receiver.receive(1)
        inner_return = actors.Group.complete_child(take inner, take inner_child)
        _ = inner_return.outcome
        consume(take inner_return.value)
        await send receiver.receive(2)
        outer_return = actors.Group.complete_child(take outer, take outer_child)
        _ = outer_return.outcome
        consume(take outer_return.value)

@image
fn build() -> Image:
    receiver = Receiver()
    worker = Worker()
    return Image.new(receiver=receiver, worker=worker)
"#;
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    );
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("nested Group fixture accepts: {outcome:#?}");
    };
    let flow = accepted.inspection().flow_program().expect("Flow selected");
    assert_eq!(flow.groups().len(), 2);
    let outer = flow
        .groups()
        .iter()
        .find(|group| group.policy() == FlowGroupPolicy::All)
        .expect("outer Group");
    let inner = flow
        .groups()
        .iter()
        .find(|group| group.policy() == FlowGroupPolicy::Race)
        .expect("inner Group");
    assert!(!outer.receiver_place().is_empty());
    assert!(!inner.receiver_place().is_empty());
    assert_ne!(outer.receiver_place(), inner.receiver_place());
    assert_ne!(
        outer.receiver_current_meaning(),
        inner.receiver_current_meaning()
    );
    assert_ne!(
        outer.moved_resources()[0].place(),
        inner.moved_resources()[0].place()
    );
    assert_eq!(flow.proposal_templates().len(), 2);
    assert_eq!(
        flow.proposal_templates()[0].owning_group(),
        Some(inner.identity())
    );
    assert_eq!(
        flow.proposal_templates()[1].owning_group(),
        Some(outer.identity())
    );
}

#[test]
fn awaited_send_inherits_exact_source_group_and_logical_deadline() {
    let source = br#"from core import actor as actors

@actor
struct Receiver:
    pub async fn receive(self):
        pass

@actor
struct Sender:
    receiver: Receiver

    pub async fn deliver(self, receiver: Receiver):
        mut group = actors.Group.all(bound=1u64)
        group.logical_deadline(epoch=7u64, slack=1000u64)
        await send receiver.receive()
        _ = actors.Group.complete(take group)

@image
fn build() -> Image:
    receiver = Receiver()
    sender = Sender(receiver=receiver)
    return Image.new(receiver=receiver, sender=sender)
"#;
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    );
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("Group-owned await send accepts: {outcome:#?}");
    };
    let flow = accepted.inspection().flow_program().expect("Flow selected");
    let group = flow.groups().first().expect("source Group");
    let waiting = flow
        .proposal_templates()
        .iter()
        .find(|template| template.admission_kind() == FlowAdmissionKind::WaitingSend)
        .expect("waiting send template");
    assert_eq!(waiting.owning_group(), Some(group.identity()));
    assert_eq!(
        waiting.deadline_class(),
        Some(wrela_compiler::FlowDeadlineClass::Logical)
    );
    assert_eq!(group.deadline_slack(), Some(1000));
}

#[test]
fn impossible_group_deadline_and_missing_realtime_authority_reject_before_flow() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let compile = |source: &'static [u8]| {
        compiler.compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
                Root::Image,
            )
            .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
            .with_inspection(InspectSelection::all()),
            &Cancellation::new(),
        )
    };
    let impossible = compile(
        br#"from core import actor as actors

@actor
struct Worker:
    pub async fn run(self):
        mut group = actors.Group.all(bound=1u64)
        group.logical_deadline(epoch=1u64, slack=0u64)
        _ = actors.Group.complete(take group)

@image
fn build() -> Image:
    worker = Worker()
    return Image.new(worker=worker)
"#,
    );
    let CompilationOutcome::Rejected(impossible) = impossible else {
        panic!("zero deadline slack rejects: {impossible:#?}");
    };
    assert_eq!(
        impossible.diagnostics()[0].code(),
        "admission.deadline_unmeetable"
    );
    assert!(impossible.inspection().flow_program().is_none());

    let missing_authority = compile(
        br#"from core import actor as actors

@actor
struct Worker:
    pub async fn run(self):
        mut group = actors.Group.all(bound=1u64)
        group.realtime_deadline(slack=5u64)
        _ = actors.Group.complete(take group)

@image
fn build() -> Image:
    worker = Worker()
    return Image.new(worker=worker)
"#,
    );
    let CompilationOutcome::Rejected(missing_authority) = missing_authority else {
        panic!("realtime without clock/capture rejects: {missing_authority:#?}");
    };
    assert_eq!(
        missing_authority.diagnostics()[0].code(),
        "semantic.argument_count"
    );
    assert!(missing_authority.inspection().flow_program().is_none());

    let bound = compile(
        br#"from core import actor as actors

resource struct Token:
    value: i64

fn consume(take token: Token):
    pass

@actor
struct Worker:
    pub async fn run(self, take token: Token):
        mut group = actors.Group.all(bound=0u64)
        child = group.child(value=take token)
        returned = actors.Group.complete_child(take group, take child)
        _ = returned.outcome
        consume(take returned.value)

@image
fn build() -> Image:
    worker = Worker()
    return Image.new(worker=worker)
"#,
    );
    let CompilationOutcome::Rejected(bound) = bound else {
        panic!("Group child beyond bound rejects: {bound:#?}");
    };
    assert_eq!(
        bound.diagnostics()[0].code(),
        "admission.group_child_bound_exceeded"
    );
    assert!(bound.inspection().flow_program().is_none());
}

const ONE_WAY_SOURCE: &[u8] = br#"resource struct Token:
    id: i64

fn consume(take token: Token):
    pass

@actor
struct Receiver:
    pub async fn receive(self, take token: Token):
        consume(take token)

@actor
struct SenderA:
    receiver: Receiver

    pub async fn deliver(self, receiver: Receiver, take token: Token):
        admission = try_send receiver.receive(take token)
        match admission:
            case Result.Ok(_):
                pass
            case Result.Err(take full):
                consume(take full.arguments)
        pass

@actor
struct SenderB:
    receiver: Receiver

    pub async fn deliver(self, receiver: Receiver, take token: Token):
        admission = try_send receiver.receive(take token)
        match admission:
            case Result.Ok(_):
                pass
            case Result.Err(take full):
                consume(take full.arguments)
        pass

@image
fn build() -> Image:
    receiver = Receiver()
    left = SenderA(receiver=receiver)
    right = SenderB(receiver=receiver)
    return Image.new(receiver=receiver, left=left, right=right)
"#;

fn one_way_request(inspection: InspectSelection) -> CompilationRequest {
    CompilationRequest::new(
        ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", ONE_WAY_SOURCE)]),
        Root::Image,
    )
    .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
    .with_inspection(inspection)
}

#[test]
fn actor_flow_has_bounded_non_reentrant_turns_and_static_homes() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(actor_request(InspectSelection::all()), &Cancellation::new());
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("Actor Flow fixture accepts: {outcome:#?}");
    };
    let flow = accepted
        .inspection()
        .flow_program()
        .expect("Flow inspection requested");

    assert_eq!(flow.phase_schema(), "wrela.flow.v1");
    assert_eq!(
        accepted.flow_program_fingerprint(),
        Some(flow.fingerprint())
    );
    assert_eq!(flow.actors().len(), 2);
    assert!(flow.actors().iter().all(|actor| {
        actor.mailbox_capacity() == 1
            && actor.max_active_turns() == 1
            && actor.permanent_core_requirement() != 0
    }));
    assert_eq!(flow.suspension_homes().len(), 1);
    assert_eq!(flow.suspension_homes()[0].slot_count(), 1);
    assert!(flow.suspension_homes()[0].retains_turn_lease());
    assert!(!flow.model_agrees());
}

#[test]
fn try_send_arbitrates_canonically_and_preserves_resource_custody() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(
        one_way_request(InspectSelection::all()),
        &Cancellation::new(),
    );
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("one-way Actor fixture accepts: {outcome:#?}");
    };
    let flow = accepted.inspection().flow_program().expect("Flow selected");
    let scenario = flow.model_scenarios().first().expect("bounded scenario");
    assert_eq!(scenario.proposals().len(), 4);

    let mut canonical = scenario.proposals().iter().collect::<Vec<_>>();
    canonical.sort_by_key(|proposal| proposal.key());
    assert!(canonical[0].arrival_ordinal() > canonical[1].arrival_ordinal());
    assert_eq!(canonical[0].outcome(), FlowSendOutcome::Admitted);
    assert_eq!(canonical[1].outcome(), FlowSendOutcome::Full);
    assert_eq!(
        canonical[0].before_commit_custodian(),
        FlowCustodian::ProposalHome
    );
    assert_eq!(
        canonical[0].after_arbitration_custodian(),
        FlowCustodian::Mailbox
    );
    assert_eq!(
        canonical[1].before_commit_custodian(),
        FlowCustodian::ProposalHome
    );
    assert_eq!(
        canonical[1].after_arbitration_custodian(),
        FlowCustodian::ProposalHome
    );
    assert_eq!(canonical[0].resource_arguments().len(), 1);
    assert_eq!(canonical[1].resource_arguments().len(), 1);
    assert_ne!(canonical[0].key(), canonical[1].key());
    assert!(canonical[0].transfer_commit().is_some());
    assert!(canonical[1].transfer_commit().is_none());
    assert!(!flow.model_agrees());
}

#[test]
fn flow_fingerprint_ignores_inspection_reuse_reopen_and_file_enumeration() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let CompilationOutcome::Accepted(without) = compiler.compile(
        one_way_request(InspectSelection::none()),
        &Cancellation::new(),
    ) else {
        panic!("fixture accepts without inspection");
    };
    let CompilationOutcome::Accepted(with) = compiler.compile(
        one_way_request(InspectSelection::all()),
        &Cancellation::new(),
    ) else {
        panic!("fixture accepts with inspection");
    };
    let CompilationOutcome::Accepted(repeated) = compiler.compile(
        one_way_request(InspectSelection::all()),
        &Cancellation::new(),
    ) else {
        panic!("fixture accepts repeatedly");
    };
    let reopened = Compiler::open(CompilerInstallation::layer1()).expect("distribution reopens");
    let CompilationOutcome::Accepted(reopened_result) = reopened.compile(
        one_way_request(InspectSelection::all()),
        &Cancellation::new(),
    ) else {
        panic!("fixture accepts after reopen");
    };
    let reordered = CompilationRequest::new(
        ProjectSnapshot::new(vec![
            ProjectFile::new("src/unused/module.wr", b"const UNUSED: i64 = 1\n"),
            ProjectFile::new("src/image.wr", ONE_WAY_SOURCE),
        ]),
        Root::Image,
    )
    .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
    .with_inspection(InspectSelection::all());
    let CompilationOutcome::Accepted(reordered) = reopened.compile(reordered, &Cancellation::new())
    else {
        panic!("fixture with reordered Project enumeration accepts");
    };

    assert!(without.inspection().flow_program().is_none());
    assert_eq!(
        without.flow_program_fingerprint(),
        with.flow_program_fingerprint()
    );
    assert_eq!(
        with.flow_program_fingerprint(),
        repeated.flow_program_fingerprint()
    );
    assert_eq!(
        repeated.flow_program_fingerprint(),
        reopened_result.flow_program_fingerprint()
    );
    assert_eq!(
        reopened_result.flow_program_fingerprint(),
        reordered.flow_program_fingerprint()
    );
}

#[test]
fn flow_semantic_identities_ignore_blank_lines_and_comments() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let compile = |path: &str, source: Vec<u8>| {
        let request = CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new(path, source)]),
            Root::Image,
        )
        .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::all());
        let outcome = compiler.compile(request, &Cancellation::new());
        let CompilationOutcome::Accepted(accepted) = outcome else {
            panic!("semantic-identity fixture {path} accepts: {outcome:#?}");
        };
        accepted
    };
    let baseline = compile("src/image.wr", ONE_WAY_SOURCE.to_vec());
    let with_trivia = compile(
        "src/image.wr",
        [
            b"# semantic identities ignore trivia\n\n".as_slice(),
            ONE_WAY_SOURCE,
        ]
        .concat(),
    );
    let projection = |accepted: &wrela_compiler::AcceptedCompilation| {
        let flow = accepted.inspection().flow_program().expect("Flow selected");
        (
            flow.actors()
                .iter()
                .map(|actor| (actor.identity(), actor.construction_identity()))
                .collect::<Vec<_>>(),
            flow.suspension_homes()
                .iter()
                .map(|home| (home.identity(), home.suspension_reference()))
                .collect::<Vec<_>>(),
            flow.proposal_templates()
                .iter()
                .map(|template| {
                    (
                        template.identity(),
                        template.current_meaning(),
                        template.suspension_home(),
                        template
                            .resource_custody()
                            .iter()
                            .map(|custody| custody.core_reference_identity())
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>(),
        )
    };

    assert_eq!(projection(&baseline), projection(&with_trivia));
    assert_ne!(
        baseline
            .inspection()
            .flow_program()
            .expect("Flow selected")
            .fingerprint(),
        with_trivia
            .inspection()
            .flow_program()
            .expect("Flow selected")
            .fingerprint()
    );
}

#[test]
fn flow_semantic_identities_ignore_an_actor_module_file_move() {
    const ACTORS: &[u8] = br#"pub resource struct Token:
    id: i64

fn consume(take token: Token):
    pass

@actor
pub struct Receiver:
    pub async fn receive(self, take token: Token):
        consume(take token)

@actor
pub struct Sender:
    pub receiver: Receiver

    pub async fn deliver(self, receiver: Receiver, take token: Token):
        admission = try_send receiver.receive(take token)
        match admission:
            case Result.Ok(_):
                pass
            case Result.Err(take full):
                consume(take full.arguments)
        pass
"#;
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let compile = |directory: &str| {
        let image = format!(
            "from {directory} import actors\n\n@image\nfn build() -> Image:\n    receiver = actors.Receiver()\n    sender = actors.Sender(receiver=receiver)\n    return Image.new(receiver=receiver, sender=sender)\n"
        );
        let request = CompilationRequest::new(
            ProjectSnapshot::new(vec![
                ProjectFile::new("src/image.wr", image.into_bytes()),
                ProjectFile::new(format!("src/{directory}/actors.wr"), ACTORS),
            ]),
            Root::Image,
        )
        .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::all());
        let outcome = compiler.compile(request, &Cancellation::new());
        let CompilationOutcome::Accepted(accepted) = outcome else {
            panic!("moved Actor module fixture accepts: {outcome:#?}");
        };
        accepted
    };
    let baseline = compile("game");
    let moved = compile("moved");
    let projection = |accepted: &wrela_compiler::AcceptedCompilation| {
        let flow = accepted.inspection().flow_program().expect("Flow selected");
        (
            flow.actors()
                .iter()
                .map(|actor| {
                    (
                        actor.identity(),
                        actor.construction_identity(),
                        actor.permanent_core_requirement(),
                        actor.handlers().len(),
                        actor.wired_actor_constructions().to_vec(),
                    )
                })
                .collect::<Vec<_>>(),
            flow.requirements()
                .iter()
                .map(|requirement| (requirement.identity(), requirement.current_meaning()))
                .collect::<Vec<_>>(),
            flow.suspension_homes()
                .iter()
                .map(|home| {
                    (
                        home.identity(),
                        home.suspension_reference(),
                        home.suspension_current_meaning(),
                        home.program_order(),
                        home.control_path().to_vec(),
                        home.requirement(),
                    )
                })
                .collect::<Vec<_>>(),
            flow.proposal_templates()
                .iter()
                .map(|template| {
                    (
                        template.identity(),
                        template.current_meaning(),
                        template.suspension_home(),
                        template
                            .resource_custody()
                            .iter()
                            .map(|custody| {
                                (
                                    custody.core_reference_identity(),
                                    custody.core_reference_current_meaning(),
                                    custody.place().to_vec(),
                                    custody.proposal_home(),
                                )
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>(),
        )
    };

    assert_eq!(projection(&baseline), projection(&moved));
    assert_ne!(
        baseline
            .inspection()
            .flow_program()
            .expect("Flow selected")
            .fingerprint(),
        moved
            .inspection()
            .flow_program()
            .expect("Flow selected")
            .fingerprint()
    );
}

#[test]
fn cancelled_requests_publish_no_partial_flow() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let cancellation = Cancellation::new();
    cancellation.cancel();
    assert!(matches!(
        compiler.compile(one_way_request(InspectSelection::all()), &cancellation),
        CompilationOutcome::Cancelled
    ));
}

#[test]
fn flow_actors_are_image_instances_not_actor_type_declarations() {
    let source = br#"@actor
struct Worker:
    id: i64

    pub async fn run(read self):
        pass

@actor
struct Unconstructed:
    pub async fn run(self):
        pass

@image
fn build() -> Image:
    first = Worker(id=1)
    second = Worker(id=2)
    return Image.new(first=first, second=second)
"#;
    let request = CompilationRequest::new(
        ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
        Root::Image,
    )
    .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
    .with_inspection(InspectSelection::all());
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(request, &Cancellation::new());
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("two Actor instances accept: {outcome:#?}");
    };
    let flow = accepted.inspection().flow_program().expect("Flow selected");

    assert_eq!(flow.actors().len(), 2);
    assert_ne!(flow.actors()[0].identity(), flow.actors()[1].identity());
    assert_eq!(
        flow.actors()[0].actor_type_identity(),
        flow.actors()[1].actor_type_identity()
    );
    assert_ne!(
        flow.actors()[0].permanent_core_requirement(),
        flow.actors()[1].permanent_core_requirement()
    );
}

#[test]
fn one_actor_construction_shared_by_two_image_fields_has_one_authority() {
    let source = br#"@actor
struct Worker:
    id: i64

    pub async fn run(read self):
        pass

@image
fn build() -> Image:
    shared = Worker(id=1)
    return Image.new(left=shared, right=shared)
"#;
    let request = CompilationRequest::new(
        ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
        Root::Image,
    )
    .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
    .with_inspection(InspectSelection::all());
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(request, &Cancellation::new());
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("shared Actor construction accepts: {outcome:#?}");
    };
    let flow = accepted.inspection().flow_program().expect("Flow selected");

    assert_eq!(flow.actors().len(), 1);
    assert_eq!(
        flow.actors()[0].identity(),
        flow.actors()[0].construction_identity()
    );
}

#[test]
fn try_send_rejects_a_non_actor_destination_before_flow() {
    let source = br#"fn ordinary():
    pass

@actor
struct Sender:
    pub async fn deliver(self):
        result = try_send ordinary()
        match result:
            case Result.Ok(_):
                pass
            case Result.Err(_):
                pass

@image
fn build() -> Image:
    return Image.new(sender=Sender())
"#;
    let request = CompilationRequest::new(
        ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
        Root::Image,
    )
    .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
    .with_inspection(InspectSelection::all());
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(request, &Cancellation::new());
    let CompilationOutcome::Rejected(rejected) = outcome else {
        panic!("non-Actor try_send is Creator Rejected: {outcome:#?}");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.try_send_requires_actor_handler")
    );
    assert!(rejected.inspection().flow_program().is_none());
}

#[test]
fn try_send_outside_an_actor_handler_is_creator_rejected() {
    let source = br#"async fn helper(receiver: Receiver, value: i64):
    admission = try_send receiver.receive(value)
    match admission:
        case Result.Ok(_):
            pass
        case Result.Err(_):
            pass

@actor
struct Receiver:
    pub async fn receive(self, value: i64):
        pass

@actor
struct Sender:
    pub async fn deliver(self, receiver: Receiver, value: i64):
        _ = await helper(receiver, value)

@image
fn build() -> Image:
    return Image.new(receiver=Receiver(), sender=Sender())
"#;
    let request = CompilationRequest::new(
        ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
        Root::Image,
    )
    .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
    .with_inspection(InspectSelection::all());
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(request, &Cancellation::new());
    let CompilationOutcome::Rejected(rejected) = outcome else {
        panic!("helper-mediated try_send is Creator Rejected: {outcome:#?}");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.try_send_requires_actor_context")
    );
}

#[test]
fn build_wiring_selects_one_exact_actor_destination() {
    let source = br#"resource struct Token:
    id: i64

fn consume(take token: Token):
    pass

@actor
struct Receiver:
    id: i64

    pub async fn receive(self, take token: Token):
        consume(take token)

@actor
struct Sender:
    receiver: Receiver

    pub async fn deliver(read self, receiver: Receiver, take token: Token):
        admission = try_send receiver.receive(take token)
        match admission:
            case Result.Ok(_):
                pass
            case Result.Err(take full):
                consume(take full.arguments)
        pass

@image
fn build() -> Image:
    selected = Receiver(id=1)
    other = Receiver(id=2)
    sender = Sender(receiver=selected)
    return Image.new(selected=selected, other=other, sender=sender)
"#;
    let request = CompilationRequest::new(
        ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
        Root::Image,
    )
    .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
    .with_inspection(InspectSelection::all());
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(request, &Cancellation::new());
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("build-wired destination accepts: {outcome:#?}");
    };
    let flow = accepted.inspection().flow_program().expect("Flow selected");
    let sender = flow
        .actors()
        .iter()
        .find(|actor| !actor.wired_actor_constructions().is_empty())
        .expect("Sender instance");
    assert_eq!(sender.wired_actor_constructions().len(), 1);
    assert_eq!(flow.proposal_templates().len(), 1);
    assert_eq!(flow.proposal_templates()[0].sender(), sender.identity());
    assert_eq!(
        flow.proposal_templates()[0].destination(),
        sender.wired_actor_constructions()[0]
    );
    assert_eq!(flow.suspension_homes().len(), 1);
    let home = &flow.suspension_homes()[0];
    assert_eq!(home.actor(), sender.identity());
    assert_eq!(
        flow.proposal_templates()[0].resource_custody()[0].proposal_home(),
        home.identity()
    );

    let rewired_source = String::from_utf8(source.to_vec())
        .expect("fixture is UTF-8")
        .replace("Sender(receiver=selected)", "Sender(receiver=other)");
    let rewired_request = CompilationRequest::new(
        ProjectSnapshot::new(vec![ProjectFile::new(
            "src/image.wr",
            rewired_source.into_bytes(),
        )]),
        Root::Image,
    )
    .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
    .with_inspection(InspectSelection::all());
    let CompilationOutcome::Accepted(rewired) =
        compiler.compile(rewired_request, &Cancellation::new())
    else {
        panic!("rewired destination accepts");
    };
    let rewired_flow = rewired.inspection().flow_program().expect("Flow selected");
    assert_ne!(
        flow.proposal_templates()[0].destination(),
        rewired_flow.proposal_templates()[0].destination()
    );
    assert_ne!(flow.fingerprint(), rewired_flow.fingerprint());
}

#[test]
fn try_send_full_preserves_each_nested_resource_as_exact_core_custody() {
    let source = br#"resource struct Token:
    id: i64

resource struct Envelope:
    left: Token
    right: Token

fn consume(take token: Token):
    pass

fn consume_envelope(take envelope: Envelope):
    consume(take envelope.left)
    consume(take envelope.right)

@actor
struct Receiver:
    pub async fn receive(self, take envelope: Envelope):
        consume_envelope(take envelope)

@actor
struct Sender:
    receiver: Receiver

    pub async fn deliver(self, receiver: Receiver, take envelope: Envelope):
        admission = try_send receiver.receive(take envelope)
        match admission:
            case Result.Ok(_):
                pass
            case Result.Err(take full):
                consume_envelope(take full.arguments)
        pass

@image
fn build() -> Image:
    receiver = Receiver()
    sender = Sender(receiver=receiver)
    return Image.new(receiver=receiver, sender=sender)
"#;
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let request = CompilationRequest::new(
        ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
        Root::Image,
    )
    .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
    .with_inspection(InspectSelection::all());
    let outcome = compiler.compile(request, &Cancellation::new());
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("nested custody fixture accepts: {outcome:#?}");
    };
    let core = accepted.inspection().core_program().expect("Core selected");
    let flow = accepted.inspection().flow_program().expect("Flow selected");
    let template = flow
        .proposal_templates()
        .first()
        .expect("proposal template");
    assert_eq!(template.resource_custody().len(), 2);
    assert_ne!(
        template.resource_custody()[0].place(),
        template.resource_custody()[1].place()
    );
    let core_references = core
        .executables()
        .iter()
        .flat_map(|executable| executable.custody_effects())
        .map(|effect| {
            (
                effect.reference_identity(),
                effect.reference_current_meaning(),
                effect.type_identity(),
                effect.place().to_vec(),
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    for resource in template.resource_custody() {
        assert!(core_references.contains(&(
            resource.core_reference_identity(),
            resource.core_reference_current_meaning(),
            Some(resource.type_identity()),
            resource.place().to_vec(),
        )));
    }
}

#[test]
fn each_await_site_has_an_exact_static_suspension_home() {
    let source = br#"async fn yield_once() -> i64:
    return 1

@actor
struct Worker:
    pub async fn run(self):
        first = await yield_once()
        second = await yield_once()
        _ = first + second

@image
fn build() -> Image:
    worker = Worker()
    return Image.new(worker=worker)
"#;
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let request = CompilationRequest::new(
        ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
        Root::Image,
    )
    .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
    .with_inspection(InspectSelection::all());
    let outcome = compiler.compile(request, &Cancellation::new());
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("two-await fixture accepts: {outcome:#?}");
    };
    let flow = accepted.inspection().flow_program().expect("Flow selected");
    assert_eq!(flow.suspension_homes().len(), 2);
    assert_ne!(
        flow.suspension_homes()[0].suspension_reference(),
        flow.suspension_homes()[1].suspension_reference()
    );
    assert_ne!(
        flow.suspension_homes()[0].requirement(),
        flow.suspension_homes()[1].requirement()
    );
    assert_eq!(
        flow.model_scenarios()[0]
            .trace()
            .iter()
            .filter(|record| record.kind() == wrela_compiler::FlowEventKind::TurnSuspended)
            .count(),
        2
    );
}

#[test]
fn proposal_templates_separate_branch_sites_from_runtime_turn_activations() {
    let source = br#"resource struct Token:
    id: i64

fn consume(take token: Token):
    pass

@actor
struct Receiver:
    pub async fn receive(self, take token: Token):
        consume(take token)

@actor
struct Sender:
    receiver: Receiver

    pub async fn deliver(self, receiver: Receiver, choose: bool, take token: Token):
        if choose:
            match try_send receiver.receive(take token):
                case Result.Ok(_):
                    pass
                case Result.Err(take full):
                    consume(take full.arguments)
        else:
            match try_send receiver.receive(take token):
                case Result.Ok(_):
                    pass
                case Result.Err(take full):
                    consume(take full.arguments)
        pass

@image
fn build() -> Image:
    receiver = Receiver()
    sender = Sender(receiver=receiver)
    return Image.new(receiver=receiver, sender=sender)
"#;
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let request = CompilationRequest::new(
        ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
        Root::Image,
    )
    .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
    .with_inspection(InspectSelection::all());
    let outcome = compiler.compile(request, &Cancellation::new());
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("branch proposal fixture accepts: {outcome:#?}");
    };
    let flow = accepted.inspection().flow_program().expect("Flow selected");
    assert_eq!(flow.proposal_templates().len(), 2);
    assert_eq!(
        flow.proposal_templates()
            .iter()
            .map(|template| template.send_ordinal())
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_ne!(
        flow.proposal_templates()[0].control_path(),
        flow.proposal_templates()[1].control_path()
    );
    assert_eq!(flow.model_scenarios().len(), 2);
    for scenario in flow.model_scenarios() {
        assert_eq!(scenario.proposals().len(), 2);
        assert!(scenario.proposals().iter().all(|proposal| {
            proposal.template_identity() == scenario.proposals()[0].template_identity()
        }));
        assert_eq!(scenario.proposals()[0].key().sender_turn_sequence(), 0);
        assert_eq!(scenario.proposals()[1].key().sender_turn_sequence(), 1);
        assert_eq!(scenario.proposals()[0].outcome(), FlowSendOutcome::Admitted);
        assert_eq!(scenario.proposals()[1].outcome(), FlowSendOutcome::Full);
        for kind in [
            wrela_compiler::FlowEventKind::MessageProposed,
            wrela_compiler::FlowEventKind::MessageFull,
            wrela_compiler::FlowEventKind::MailboxTransferCommitted,
        ] {
            assert!(scenario.trace().iter().any(|record| record.kind() == kind));
        }
    }
    assert!(!flow.model_agrees());
}

#[test]
fn sequential_send_sites_share_a_turn_sequence_and_keep_program_ordinals() {
    let source = br#"resource struct Token:
    id: i64

fn consume(take token: Token):
    pass

@actor
struct Receiver:
    pub async fn receive(self, take token: Token):
        consume(take token)

@actor
struct Sender:
    receiver: Receiver

    pub async fn deliver(self, receiver: Receiver, take first: Token, take second: Token):
        first_admission = try_send receiver.receive(take first)
        match first_admission:
            case Result.Ok(_):
                pass
            case Result.Err(take full):
                consume(take full.arguments)
        second_admission = try_send receiver.receive(take second)
        match second_admission:
            case Result.Ok(_):
                pass
            case Result.Err(take full):
                consume(take full.arguments)
        pass

@image
fn build() -> Image:
    receiver = Receiver()
    sender = Sender(receiver=receiver)
    return Image.new(receiver=receiver, sender=sender)
"#;
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let request = CompilationRequest::new(
        ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
        Root::Image,
    )
    .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
    .with_inspection(InspectSelection::all());
    let outcome = compiler.compile(request, &Cancellation::new());
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("sequential send fixture accepts: {outcome:#?}");
    };
    let flow = accepted.inspection().flow_program().expect("Flow selected");
    assert_eq!(
        flow.proposal_templates()
            .iter()
            .map(|template| template.send_ordinal())
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    let scenario = &flow.model_scenarios()[0];
    for turn in 0..2 {
        let proposals = scenario
            .proposals()
            .iter()
            .filter(|proposal| proposal.key().sender_turn_sequence() == turn)
            .collect::<Vec<_>>();
        assert_eq!(proposals.len(), 2);
        assert_eq!(proposals[0].key().send_ordinal(), 0);
        assert_eq!(proposals[1].key().send_ordinal(), 1);
    }
    let sender = flow.proposal_templates()[0].sender();
    assert_eq!(
        scenario
            .trace()
            .iter()
            .filter(|record| {
                record.actor() == sender
                    && record.kind() == wrela_compiler::FlowEventKind::TurnStarted
            })
            .count(),
        2
    );
}

#[test]
fn waiting_send_owns_future_admission_cancellation_and_durable_commit_obligations() {
    let source = br#"resource struct Token:
    id: i64

fn consume(take token: Token):
    pass

@actor
struct Receiver:
    pub async fn receive(self, take token: Token):
        consume(take token)

@actor
struct Sender:
    receiver: Receiver

    pub async fn deliver(self, receiver: Receiver, take token: Token):
        await send receiver.receive(take token)

@image
fn build() -> Image:
    receiver = Receiver()
    sender = Sender(receiver=receiver)
    return Image.new(receiver=receiver, sender=sender)
"#;
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    );
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("waiting send fixture accepts: {outcome:#?}");
    };
    let flow = accepted.inspection().flow_program().expect("Flow selected");
    let template = flow.proposal_templates().first().expect("send template");
    assert_eq!(template.admission_kind(), FlowAdmissionKind::WaitingSend);
    assert_eq!(template.owning_group(), None);
    assert_eq!(template.deadline_class(), None);

    let pre_commit = flow
        .structured_scenarios()
        .iter()
        .find(|scenario| scenario.kind() == FlowStructuredScenarioKind::PreCommitCancellation)
        .expect("pre-commit cancellation scenario");
    assert_eq!(pre_commit.outcome(), FlowStructuredOutcome::Cancelled);
    assert!(pre_commit.events().iter().any(|event| {
        event.kind() == FlowEventKind::AdmissionCancelled
            && event.custodian() == Some(FlowCustodian::ProposalHome)
    }));

    let committed = flow
        .structured_scenarios()
        .iter()
        .find(|scenario| scenario.kind() == FlowStructuredScenarioKind::DurableCommit)
        .expect("durable commit scenario");
    assert!(committed.events().iter().any(|event| {
        event.kind() == FlowEventKind::MailboxTransferCommitted
            && event.custodian() == Some(FlowCustodian::Mailbox)
    }));
    assert!(!flow.model_agrees());
}

#[test]
fn waiting_send_source_mistakes_are_rejected_before_flow() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let compile = |source: &'static [u8]| {
        compiler.compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
                Root::Image,
            )
            .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
            .with_inspection(InspectSelection::all()),
            &Cancellation::new(),
        )
    };
    let non_handler = compile(
        br#"async fn helper():
    pass

@actor
struct Sender:
    pub async fn deliver(self):
        await send helper()

@image
fn build() -> Image:
    sender = Sender()
    return Image.new(sender=sender)
"#,
    );
    let CompilationOutcome::Rejected(non_handler) = non_handler else {
        panic!("send to non-handler rejects: {non_handler:#?}");
    };
    assert_eq!(
        non_handler.diagnostics()[0].code(),
        "semantic.send_requires_actor_handler"
    );
    assert!(non_handler.inspection().flow_program().is_none());

    let outside_actor = compile(
        br#"@actor
struct Receiver:
    pub async fn receive(self):
        pass

async fn helper(receiver: Receiver):
    await send receiver.receive()

@image
fn build() -> Image:
    receiver = Receiver()
    return Image.new(receiver=receiver)
"#,
    );
    let CompilationOutcome::Rejected(outside_actor) = outside_actor else {
        panic!("send outside Actor rejects: {outside_actor:#?}");
    };
    assert_eq!(
        outside_actor.diagnostics()[0].code(),
        "semantic.send_requires_actor_context"
    );
    assert!(outside_actor.inspection().flow_program().is_none());
}

#[test]
fn awaited_actor_request_reserves_reply_and_recovers_late_reply_closed_custody() {
    let source = br#"from core import actor as actors

resource struct Token:
    id: i64

fn consume(take token: Token):
    pass

fn finish(take fulfillment: Result[bool, actors.ReplyClosed[Token]]):
    match fulfillment:
        case Result.Ok(_):
            return
        case Result.Err(take closed):
            consume(take closed.response)
            return

@actor
struct Server:
    pub async fn exchange(self, take token: Token, take reply: actors.Reply[Token]):
        fulfillment = actors.Reply.fulfill(take reply, take token)
        finish(take fulfillment)

@actor
struct Client:
    server: Server

    pub async fn request(self, server: Server, take token: Token):
        answer = await server.exchange(take token)
        consume(take answer)

@image
fn build() -> Image:
    server = Server()
    client = Client(server=server)
    return Image.new(server=server, client=client)
"#;
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    );
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("request fixture accepts: {outcome:#?}");
    };
    let flow = accepted.inspection().flow_program().expect("Flow selected");
    assert_eq!(flow.reply_obligations().len(), 1);
    let reply = &flow.reply_obligations()[0];
    assert!(reply.endpoint() != 0 && reply.return_path() != 0 && reply.response_home() != 0);
    assert_eq!(reply.capacity(), 1);
    assert!(reply.fulfillment_capacity_infallible());
    assert!(reply.acyclic_wait_requirement() != 0);
    assert!(
        !reply.fulfillment_references().is_empty()
            && reply
                .fulfillment_references()
                .iter()
                .all(|(reference, meaning)| *reference != 0 && *meaning != 0)
    );
    assert_eq!(reply.fulfillment_endpoint_places().len(), 1);
    assert!(!reply.fulfillment_endpoint_places()[0].is_empty());
    assert_eq!(reply.response_custody().len(), 1);
    assert!(!reply.response_custody()[0].place().is_empty());
    assert!(!reply.explicit_cancel());
    assert!(flow.requirements().iter().any(|requirement| {
        requirement.kind() == FlowRequirementKind::ReplyResponseHome
            && requirement.site() == Some(reply.response_home())
    }));
    let request_template = flow
        .proposal_templates()
        .iter()
        .find(|template| template.admission_kind() == FlowAdmissionKind::Request)
        .expect("request template");
    let cancelled_wait = flow
        .structured_scenarios()
        .iter()
        .find(|scenario| {
            scenario.kind() == FlowStructuredScenarioKind::PreCommitCancellation
                && scenario.events().iter().any(|event| {
                    event.kind() == FlowEventKind::ReplyEndpointClosed
                        && event.subject() == Some(reply.identity())
                })
        })
        .expect("waiting Request cancellation");
    assert!(cancelled_wait.events().iter().any(|event| {
        event.kind() == FlowEventKind::AdmissionCancelled
            && event.subject() == Some(request_template.identity())
    }));
    assert!(request_template.resource_custody().iter().all(|custody| {
        cancelled_wait.events().iter().any(|event| {
            event.kind() == FlowEventKind::ResourceReturned
                && event.subject() == Some(custody.core_reference_identity())
                && event.custodian() == Some(FlowCustodian::ProposalHome)
        })
    }));

    let delivered = flow
        .structured_scenarios()
        .iter()
        .find(|scenario| scenario.kind() == FlowStructuredScenarioKind::ReplyDelivered)
        .expect("delivered Reply scenario");
    assert!(delivered.events().iter().any(|event| {
        event.kind() == FlowEventKind::ReplyFulfilled
            && event.subject() == Some(reply.identity())
            && event.custodian() == Some(FlowCustodian::ResponseHome)
    }));

    let closed = flow
        .structured_scenarios()
        .iter()
        .find(|scenario| scenario.kind() == FlowStructuredScenarioKind::ReplyClosedRecovery)
        .expect("ReplyClosed scenario");
    assert_eq!(closed.outcome(), FlowStructuredOutcome::ReplyClosed);
    assert!(closed.events().iter().any(|event| {
        event.kind() == FlowEventKind::ReplyClosed
            && event.custodian() == Some(FlowCustodian::ReplyClosed)
            && event.must_use()
            && event.subject() == Some(reply.response_custody()[0].core_reference_identity())
    }));
    assert!(!flow.model_agrees());
}

#[test]
fn reply_endpoint_must_be_fulfilled_once_with_a_checked_exact_response() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let bodies = [
        "        pass",
        "        first = actors.Reply.fulfill_copy(take reply, 1)\n        match first:\n            case Result.Ok(_):\n                pass\n            case Result.Err(take closed):\n                actors.ReplyClosed.discard_copy(take closed)\n        second = actors.Reply.fulfill_copy(take reply, 2)",
        "        actors.Reply.fulfill_copy(take reply, 1)",
        "        result = actors.Reply.fulfill_copy(take reply, false)",
    ];
    for body in bodies {
        let source = format!(
            "from core import actor as actors\n\n@actor\nstruct Receiver:\n    pub async fn ask(self, take reply: actors.Reply[i64]):\n{body}\n\n@actor\nstruct Sender:\n    pub async fn run(self, receiver: Receiver):\n        value = await receiver.ask()\n        _ = value\n\n@image\nfn build() -> Image:\n    receiver = Receiver()\n    sender = Sender()\n    return Image.new(receiver=receiver, sender=sender)\n"
        );
        let outcome = compiler.compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source.into_bytes())]),
                Root::Image,
            )
            .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
            .with_inspection(InspectSelection::all()),
            &Cancellation::new(),
        );
        let CompilationOutcome::Rejected(rejected) = outcome else {
            panic!("invalid Reply endpoint program is Creator-rejected: {outcome:#?}");
        };
        assert!(rejected.inspection().flow_program().is_none());
    }
}

#[test]
fn absent_group_deadline_and_panic_facts_emit_no_fabricated_flow_authority() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(actor_request(InspectSelection::all()), &Cancellation::new());
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("Actor Flow fixture accepts: {outcome:#?}");
    };
    let flow = accepted.inspection().flow_program().expect("Flow selected");
    assert!(flow.groups().is_empty());
    assert!(flow.group_policy_laws().is_empty());
    assert!(flow.deadline_laws().is_empty());
    for kind in [
        FlowStructuredScenarioKind::GroupPolicies,
        FlowStructuredScenarioKind::DeadlineUnmeetable,
        FlowStructuredScenarioKind::DeadlineExceeded,
        FlowStructuredScenarioKind::ReverseCleanup,
        FlowStructuredScenarioKind::CustodyRecovery,
        FlowStructuredScenarioKind::TerminalPanic,
    ] {
        assert!(
            !flow
                .structured_scenarios()
                .iter()
                .any(|scenario| scenario.kind() == kind)
        );
    }
}

#[test]
fn terminal_panic_scenario_is_derived_from_the_exact_core_site() {
    let source = br#"@actor
struct Worker:
    pub async fn crash(self):
        panic "boom"

@image
fn build() -> Image:
    worker = Worker()
    return Image.new(worker=worker)
"#;
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    );
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("terminal Panic fixture accepts: {outcome:#?}");
    };
    let flow = accepted.inspection().flow_program().expect("Flow selected");
    let scenarios = flow
        .structured_scenarios()
        .iter()
        .filter(|scenario| scenario.kind() == FlowStructuredScenarioKind::TerminalPanic)
        .collect::<Vec<_>>();
    assert_eq!(scenarios.len(), 1);
    assert_eq!(scenarios[0].outcome(), FlowStructuredOutcome::Panic);
    assert_eq!(scenarios[0].events().len(), 1);
    let event = &scenarios[0].events()[0];
    assert_eq!(event.kind(), FlowEventKind::TerminalPanic);
    assert!(event.subject().is_some_and(|subject| subject != 0));
    assert_ne!(event.logical_coordinate(), 0);
}

#[test]
fn ordinary_defer_cleanup_is_not_fabricated_into_a_group() {
    let source = br#"fn oldest():
    pass

fn newest():
    pass

async fn yield_once() -> i64:
    return 1

@actor
struct Worker:
    pub async fn run(self):
        defer oldest()
        defer newest()
        _ = await yield_once()

@image
fn build() -> Image:
    worker = Worker()
    return Image.new(worker=worker)
"#;
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    );
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("cleanup Group fixture accepts: {outcome:#?}");
    };
    let flow = accepted.inspection().flow_program().expect("Flow selected");
    assert!(flow.groups().is_empty());
    assert!(
        !flow
            .requirements()
            .iter()
            .any(|requirement| requirement.kind() == FlowRequirementKind::GroupCleanupOrder)
    );
}

#[test]
fn statically_knowable_reply_wait_cycle_is_creator_rejected_with_exact_evidence() {
    let source = br#"@actor
struct Left:
    pub async fn first(self, right: Right, left: Left):
        await right.second(left, right)

@actor
struct Right:
    pub async fn second(self, left: Left, right: Right):
        await left.first(right, left)

@image
fn build() -> Image:
    left = Left()
    right = Right()
    return Image.new(left=left, right=right)
"#;
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
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
        panic!("Reply wait cycle is Creator-rejected, not Defect: {outcome:#?}");
    };
    let Some(diagnostic) = rejected
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.code() == "admission.reply_wait_cycle")
    else {
        panic!("exact cycle diagnostic: {:#?}", rejected.diagnostics());
    };
    assert_eq!(diagnostic.labels().len(), 1);
    assert!(diagnostic.typed_parameters().iter().any(|(name, value)| {
        name.as_ref() == "cycle_length" && *value == wrela_compiler::DiagnosticValue::Unsigned(2)
    }));
    assert_eq!(
        diagnostic
            .typed_parameters()
            .iter()
            .filter(|(name, _)| name.as_ref() == "actor")
            .count(),
        2
    );
    assert!(rejected.inspection().flow_program().is_none());
}

#[test]
fn actor_message_admission_rejects_payload_and_receiver_loans_before_core_or_flow() {
    let cases: &[(&str, &[u8])] = &[
        (
            "read-payload",
            br#"from core import actor as actors

resource struct Token:
    value: i64

@actor
struct Worker:
    pub async fn accept(self, read token: Token):
        pass

    pub async fn start(self, take token: Token):
        admission = try_send self.accept(token)
        match admission:
            case Result.Ok(_):
                pass
            case Result.Err(_):
                pass

@image
fn build() -> Image:
    worker = Worker()
    return Image.new(worker=worker)
"#,
        ),
        (
            "mut-payload",
            br#"resource struct Token:
    value: i64

@actor
struct Worker:
    pub async fn accept(self, mut token: Token):
        pass

    pub async fn start(self, take token: Token):
        await send self.accept(mut token)

@image
fn build() -> Image:
    worker = Worker()
    return Image.new(worker=worker)
"#,
        ),
        (
            "request-payload",
            br#"from core import actor as actors

resource struct Token:
    value: i64

@actor
struct Worker:
    pub async fn accept(self, read token: Token, take reply: actors.Reply[i64]):
        fulfillment = actors.Reply.fulfill_copy(take reply, 1)
        match fulfillment:
            case Result.Ok(_):
                pass
            case Result.Err(take closed):
                actors.ReplyClosed.discard_copy(take closed)

    pub async fn start(self, take token: Token):
        _ = await self.accept(token)

@image
fn build() -> Image:
    worker = Worker()
    return Image.new(worker=worker)
"#,
        ),
        (
            "implicit-receiver",
            br#"@actor
struct Worker:
    pub async fn ping(read self):
        pass

    pub async fn start(self):
        admission = try_send self.ping()
        match admission:
            case Result.Ok(_):
                pass
            case Result.Err(_):
                pass

@image
fn build() -> Image:
    worker = Worker()
    return Image.new(worker=worker)
"#,
        ),
    ];
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    for (name, source) in cases {
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
            panic!("{name} message loan must be Creator-rejected: {outcome:#?}");
        };
        assert!(
            rejected
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code() == "semantic.loan_across_suspension"),
            "{name}: {:#?}",
            rejected.diagnostics()
        );
        assert!(rejected.inspection().core_program().is_none());
        assert!(rejected.inspection().flow_program().is_none());
    }
}

#[test]
fn send_is_only_valid_as_the_direct_operand_of_await() {
    let cases: &[(&str, &[u8])] = &[
        (
            "standalone",
            br#"@actor
struct Worker:
    pub async fn ping(self):
        pass

    pub async fn start(self):
        send self.ping()

@image
fn build() -> Image:
    worker = Worker()
    return Image.new(worker=worker)
"#,
        ),
        (
            "nested",
            br#"fn consume(value: Unit):
    pass

@actor
struct Worker:
    pub async fn ping(self):
        pass

    pub async fn start(self):
        consume(send self.ping())

@image
fn build() -> Image:
    worker = Worker()
    return Image.new(worker=worker)
"#,
        ),
    ];
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    for (name, source) in cases {
        let outcome = compiler.compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
                Root::Image,
            ),
            &Cancellation::new(),
        );
        let CompilationOutcome::Rejected(rejected) = outcome else {
            panic!("{name} send must reject: {outcome:#?}");
        };
        assert!(
            rejected
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code() == "semantic.send_requires_await"),
            "{name}: {:#?}",
            rejected.diagnostics()
        );
        assert!(rejected.inspection().core_program().is_none());
    }
}

#[test]
fn bounded_model_sampling_never_rejects_a_valid_static_flow_graph() {
    let source = br#"@actor
struct Sink:
    pub async fn sink(self):
        pass

@actor
struct WorkerOne:
    sink: Sink

    pub async fn one(self, sink: Sink, flag: bool):
        if flag:
            match try_send sink.sink():
                case Result.Ok(_):
                    pass
                case Result.Err(_):
                    pass
        else:
            match try_send sink.sink():
                case Result.Ok(_):
                    pass
                case Result.Err(_):
                    pass
        pass

@actor
struct WorkerTwo:
    sink: Sink

    pub async fn two(self, sink: Sink, flag: bool):
        if flag:
            match try_send sink.sink():
                case Result.Ok(_):
                    pass
                case Result.Err(_):
                    pass
        else:
            match try_send sink.sink():
                case Result.Ok(_):
                    pass
                case Result.Err(_):
                    pass
        pass

@actor
struct WorkerThree:
    sink: Sink

    pub async fn three(self, sink: Sink, flag: bool):
        if flag:
            match try_send sink.sink():
                case Result.Ok(_):
                    pass
                case Result.Err(_):
                    pass
        else:
            match try_send sink.sink():
                case Result.Ok(_):
                    pass
                case Result.Err(_):
                    pass
        pass

@actor
struct WorkerFour:
    sink: Sink

    pub async fn four(self, sink: Sink, flag: bool):
        if flag:
            match try_send sink.sink():
                case Result.Ok(_):
                    pass
                case Result.Err(_):
                    pass
        else:
            match try_send sink.sink():
                case Result.Ok(_):
                    pass
                case Result.Err(_):
                    pass
        pass

@actor
struct WorkerFive:
    sink: Sink

    pub async fn five(self, sink: Sink, flag: bool):
        if flag:
            match try_send sink.sink():
                case Result.Ok(_):
                    pass
                case Result.Err(_):
                    pass
        else:
            match try_send sink.sink():
                case Result.Ok(_):
                    pass
                case Result.Err(_):
                    pass
        pass

@image
fn build() -> Image:
    sink = Sink()
    one = WorkerOne(sink=sink)
    two = WorkerTwo(sink=sink)
    three = WorkerThree(sink=sink)
    four = WorkerFour(sink=sink)
    five = WorkerFive(sink=sink)
    return Image.new(sink=sink, one=one, two=two, three=three, four=four, five=five)
"#;
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    );
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("valid static graph is independent of bounded model sample count: {outcome:#?}");
    };
    let flow = accepted.inspection().flow_program().expect("Flow selected");
    assert!(!flow.model_evidence_complete());
    assert_eq!(flow.model_scenario_bound(), 16);
    assert_eq!(flow.model_scenarios().len(), 16);
}

#[test]
fn mailbox_capacity_is_global_for_waiting_sends_and_requests() {
    let source = br#"from core import actor as actors

resource struct Token:
    value: i64

fn consume(take token: Token):
    pass

fn finish_i64(take fulfillment: Result[bool, actors.ReplyClosed[i64]]):
    match fulfillment:
        case Result.Ok(_):
            return
        case Result.Err(take closed):
            actors.ReplyClosed.discard_copy(take closed)
            return

@actor
struct Receiver:
    pub async fn receive(self, take token: Token):
        consume(take token)

    pub async fn request(self, value: i64, take reply: actors.Reply[i64]):
        fulfillment = actors.Reply.fulfill_copy(take reply, value)
        finish_i64(take fulfillment)

@actor
struct SendOwner:
    receiver: Receiver

    pub async fn run(self, receiver: Receiver, take first: Token, take second: Token):
        await send receiver.receive(take first)
        await send receiver.receive(take second)

@actor
struct RequestOwner:
    receiver: Receiver

    pub async fn run(self, receiver: Receiver):
        _ = await receiver.request(1)
        _ = await receiver.request(2)

@image
fn build() -> Image:
    receiver = Receiver()
    sends = SendOwner(receiver=receiver)
    requests = RequestOwner(receiver=receiver)
    return Image.new(receiver=receiver, sends=sends, requests=requests)
"#;
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    );
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("global Mailbox fixture accepts: {outcome:#?}");
    };
    let flow = accepted.inspection().flow_program().expect("Flow selected");
    let receiver = flow
        .actors()
        .iter()
        .find(|actor| actor.handlers().len() == 2)
        .expect("Receiver Actor");
    for scenario in flow.model_scenarios() {
        let destination = scenario
            .proposals()
            .iter()
            .filter(|proposal| proposal.key().destination() == receiver.identity())
            .collect::<Vec<_>>();
        assert!(
            destination
                .iter()
                .all(|proposal| proposal.outcome() == FlowSendOutcome::Admitted)
        );
        assert_eq!(
            destination
                .iter()
                .filter(|proposal| proposal.waited_for_capacity())
                .count(),
            destination.len() - 1
        );
        for proposal in destination
            .iter()
            .filter(|proposal| proposal.waited_for_capacity())
        {
            assert!(proposal.dequeued_proposal().is_some());
            assert_eq!(
                proposal.before_commit_custodian(),
                FlowCustodian::ProposalHome
            );
            assert_eq!(
                proposal.after_arbitration_custodian(),
                FlowCustodian::Mailbox
            );
            let records = scenario.trace().iter().enumerate().collect::<Vec<_>>();
            let waiting = records.iter().position(|(_, record)| {
                record.proposal() == Some(proposal.key())
                    && record.kind() == FlowEventKind::AdmissionWaiting
            });
            let dequeue = records.iter().position(|(_, record)| {
                record.proposal() == proposal.dequeued_proposal()
                    && record.kind() == FlowEventKind::MailboxDequeued
            });
            let commit = records.iter().position(|(_, record)| {
                record.proposal() == Some(proposal.key())
                    && record.kind() == FlowEventKind::MailboxTransferCommitted
            });
            let resume = records.iter().position(|(_, record)| {
                record.proposal() == Some(proposal.key())
                    && record.kind() == FlowEventKind::TurnResumed
            });
            assert!(waiting.zip(dequeue).zip(commit).zip(resume).is_some_and(
                |(((waiting, dequeue), commit), resume)| {
                    waiting < dequeue && dequeue < commit && commit < resume
                }
            ));
        }
        assert!(
            destination
                .iter()
                .filter(|proposal| {
                    flow.proposal_templates()
                        .iter()
                        .find(|template| template.identity() == proposal.template_identity())
                        .is_some_and(|template| {
                            template.admission_kind() == FlowAdmissionKind::Request
                        })
                })
                .all(|proposal| {
                    let records = scenario
                        .trace()
                        .iter()
                        .filter(|record| record.proposal() == Some(proposal.key()))
                        .collect::<Vec<_>>();
                    let reserved = records
                        .iter()
                        .position(|record| record.kind() == FlowEventKind::ReplyPathReserved);
                    let proposed = records
                        .iter()
                        .position(|record| record.kind() == FlowEventKind::MessageProposed);
                    reserved
                        .zip(proposed)
                        .is_some_and(|(reserved, proposed)| reserved < proposed)
                })
        );
    }
}
