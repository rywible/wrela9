use wrela_compiler::{
    ArchitectureProfile, Cancellation, CompilationOutcome, CompilationRequest, Compiler,
    CompilerInstallation, FlowAdmissionKind, FlowCustodian, FlowDeadlineClass, FlowEventKind,
    FlowGroupPolicy, FlowRequirementKind, FlowSendOutcome, FlowStructuredOutcome,
    FlowStructuredScenarioKind, InspectSelection, ProjectFile, ProjectSnapshot, Root,
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
    assert!(flow.model_agrees());
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
    assert!(flow.model_agrees());
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
            flow.fingerprint(),
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
            flow.fingerprint(),
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
    assert!(flow.model_agrees());
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
    assert!(template.owning_group().is_some());
    assert_eq!(template.deadline_class(), Some(FlowDeadlineClass::Logical));
    assert!(flow.requirements().iter().any(|requirement| {
        requirement.kind() == FlowRequirementKind::CancellationMaximumLatency
            && requirement.bound() > 0
    }));

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
    assert!(flow.model_agrees());
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
    let source = br#"resource struct Token:
    id: i64

fn consume(take token: Token):
    pass

@actor
struct Server:
    pub async fn exchange(self, take token: Token) -> Token:
        return take token

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
    assert!(flow.requirements().iter().any(|requirement| {
        requirement.kind() == FlowRequirementKind::ReplyResponseHome
            && requirement.site() == Some(reply.response_home())
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
    }));
    assert!(flow.model_agrees());
}

#[test]
fn structured_group_deadline_cleanup_and_panic_scenarios_are_typed_and_deterministic() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(actor_request(InspectSelection::all()), &Cancellation::new());
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("Actor Flow fixture accepts: {outcome:#?}");
    };
    let flow = accepted.inspection().flow_program().expect("Flow selected");
    assert!(flow.groups().iter().all(|group| {
        group.child_activation_bound() > 0
            && group.noncopyable_cancellation_authority() != 0
            && group.return_home() != 0
            && group.maximum_cancellation_latency() > 0
    }));
    assert_eq!(
        flow.group_policy_laws()
            .iter()
            .map(|law| law.policy())
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([
            FlowGroupPolicy::All,
            FlowGroupPolicy::Collect,
            FlowGroupPolicy::Race,
            FlowGroupPolicy::Supervise,
        ])
    );
    let logical = flow
        .deadline_laws()
        .iter()
        .find(|law| law.class() == FlowDeadlineClass::Logical)
        .expect("logical deadline law");
    assert!(logical.deterministic() && !logical.replay_capture_required());
    let realtime = flow
        .deadline_laws()
        .iter()
        .find(|law| law.class() == FlowDeadlineClass::Realtime)
        .expect("realtime deadline law");
    assert!(realtime.authority() != 0 && realtime.replay_capture_required());
    for kind in [
        FlowStructuredScenarioKind::GroupPolicies,
        FlowStructuredScenarioKind::DeadlineUnmeetable,
        FlowStructuredScenarioKind::DeadlineExceeded,
        FlowStructuredScenarioKind::ReverseCleanup,
        FlowStructuredScenarioKind::CustodyRecovery,
        FlowStructuredScenarioKind::TerminalPanic,
    ] {
        assert!(
            flow.structured_scenarios()
                .iter()
                .any(|scenario| scenario.kind() == kind)
        );
    }
    let cleanup = flow
        .structured_scenarios()
        .iter()
        .find(|scenario| scenario.kind() == FlowStructuredScenarioKind::ReverseCleanup)
        .expect("cleanup scenario");
    assert_eq!(cleanup.cleanup_order(), &[2, 1, 0]);
    let panic = flow
        .structured_scenarios()
        .iter()
        .find(|scenario| scenario.kind() == FlowStructuredScenarioKind::TerminalPanic)
        .expect("Panic scenario");
    assert_eq!(panic.outcome(), FlowStructuredOutcome::Panic);
    assert!(panic.cleanup_order().is_empty());
}

#[test]
fn group_static_cleanup_actions_are_exact_and_execute_in_reverse_registration_order() {
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
    let group = flow
        .groups()
        .iter()
        .find(|group| group.cleanup_actions().len() == 2)
        .expect("handler Group owns both exact cleanup actions");
    assert_eq!(
        group.cleanup_execution_order(),
        &group
            .cleanup_actions()
            .iter()
            .rev()
            .copied()
            .collect::<Vec<_>>()
    );
}

#[test]
fn statically_knowable_reply_wait_cycle_is_creator_rejected_with_exact_evidence() {
    let source = br#"@actor
struct Loop:
    pub async fn ping(self):
        await self.ping()

@image
fn build() -> Image:
    loop_actor = Loop()
    return Image.new(loop_actor=loop_actor)
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
    assert!(rejected.inspection().flow_program().is_none());
}
