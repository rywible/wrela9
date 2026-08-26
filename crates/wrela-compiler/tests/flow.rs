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
        try_send receiver.receive(take token)

@actor
struct SenderB:
    pub async fn deliver(self, receiver: Receiver, take token: Token):
        try_send receiver.receive(take token)

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
