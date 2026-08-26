use wrela_compiler::{
    ArchitectureProfile, Cancellation, CompilationOutcome, CompilationRequest, Compiler,
    CompilerInstallation, FlowCustodian, FlowSendOutcome, InspectSelection, ProjectFile,
    ProjectSnapshot, Root,
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
    left = SenderA()
    right = SenderB()
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
    assert_eq!(flow.proposals().len(), 2);

    let mut canonical = flow.proposals().iter().collect::<Vec<_>>();
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
    assert_ne!(
        canonical[0].resource_arguments(),
        canonical[1].resource_arguments()
    );
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
    sender = Sender()
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
        flow.trace()
            .iter()
            .filter(|record| record.kind() == wrela_compiler::FlowEventKind::TurnSuspended)
            .count(),
        1
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
    sender = Sender()
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
    assert_eq!(flow.proposals().len(), 2);
    assert!(flow
        .proposals()
        .iter()
        .all(|proposal| proposal.template_identity() == flow.proposals()[0].template_identity()));
    assert_eq!(flow.proposals()[0].key().sender_turn_sequence(), 0);
    assert_eq!(flow.proposals()[1].key().sender_turn_sequence(), 1);
    assert_eq!(flow.proposals()[0].outcome(), FlowSendOutcome::Admitted);
    assert_eq!(flow.proposals()[1].outcome(), FlowSendOutcome::Full);
    for kind in [
        wrela_compiler::FlowEventKind::MessageProposed,
        wrela_compiler::FlowEventKind::MessageFull,
        wrela_compiler::FlowEventKind::MailboxTransferCommitted,
    ] {
        assert!(flow.trace().iter().any(|record| record.kind() == kind));
    }
    let admitted = flow.proposals()[0].key();
    assert!(flow.trace().iter().any(|record| {
        record.kind() == wrela_compiler::FlowEventKind::TurnStarted
            && record.actor() == admitted.destination()
            && record.logical_commit().is_some()
    }));
    assert!(flow.model_agrees());
}
