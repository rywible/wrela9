#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::sync::Arc;

use xxhash_rust::xxh3::Xxh3;

use crate::compiler::Cancellation;
use crate::core::FlowCoreView;
use crate::image_planning::FlowPlanningInput;

const PHASE_SCHEMA: &str = "wrela.flow.v1";
type ControlPath = Arc<[u32]>;
type SelectedControlPath = (u128, u128, ControlPath);
type SelectedControlPaths = Arc<[SelectedControlPath]>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FlowRequirementKind {
    ActorIdentity,
    PermanentCorePlacement,
    MailboxCapacity,
    TurnLease,
    SuspensionHome,
    LogicalCommitOrder,
    ProposalTransport,
    ReplyEndpoint,
    ReplyReturnPath,
    ReplyResponseHome,
    ReplyAcyclicWait,
    GroupChildActivationBound,
    GroupCancellationAuthority,
    GroupOutcomePolicy,
    GroupResourceReturnHome,
    GroupCleanupOrder,
    DeadlineClass,
    DeadlineAuthority,
    DeadlineSlack,
    DeadlineFeasibility,
    CancellationCheckpoint,
    CancellationObservationWorkBound,
    ServiceStorage,
    ActivationStorage,
}

impl FlowRequirementKind {
    const fn tag(self) -> u8 {
        match self {
            Self::ActorIdentity => 1,
            Self::PermanentCorePlacement => 2,
            Self::MailboxCapacity => 3,
            Self::TurnLease => 4,
            Self::SuspensionHome => 5,
            Self::LogicalCommitOrder => 6,
            Self::ProposalTransport => 7,
            Self::ReplyEndpoint => 8,
            Self::ReplyReturnPath => 9,
            Self::ReplyResponseHome => 10,
            Self::ReplyAcyclicWait => 11,
            Self::GroupChildActivationBound => 12,
            Self::GroupCancellationAuthority => 13,
            Self::GroupOutcomePolicy => 14,
            Self::GroupResourceReturnHome => 15,
            Self::GroupCleanupOrder => 16,
            Self::DeadlineClass => 17,
            Self::DeadlineAuthority => 18,
            Self::DeadlineSlack => 19,
            Self::DeadlineFeasibility => 20,
            Self::CancellationCheckpoint => 21,
            Self::CancellationObservationWorkBound => 22,
            Self::ServiceStorage => 23,
            Self::ActivationStorage => 24,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FlowAdmissionKind {
    TrySend,
    WaitingSend,
    Request,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FlowGroupPolicy {
    All,
    Collect,
    Race,
    Supervise,
}

impl FlowGroupPolicy {
    const fn tag(self) -> u8 {
        match self {
            Self::All => 1,
            Self::Collect => 2,
            Self::Race => 3,
            Self::Supervise => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FlowDeadlineClass {
    Logical,
    Realtime,
}

impl FlowDeadlineClass {
    const fn tag(self) -> u8 {
        match self {
            Self::Logical => 1,
            Self::Realtime => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FlowStructuredScenarioKind {
    ReversedArrival,
    PreCommitCancellation,
    DurableCommit,
    ReplyDelivered,
    ReplyClosedRecovery,
    GroupPolicies,
    DeadlineUnmeetable,
    DeadlineExceeded,
    ReverseCleanup,
    CustodyRecovery,
    TerminalPanic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FlowStructuredOutcome {
    Completed,
    Cancelled,
    ReplyClosed,
    DeadlineUnmeetable,
    DeadlineExceeded,
    Panic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FlowEventKind {
    MessageProposed,
    MessageFull,
    MailboxTransferCommitted,
    TurnStarted,
    TurnSuspended,
    TurnResumed,
    TurnCompleted,
    AdmissionWaiting,
    MailboxDequeued,
    AdmissionCancelled,
    ReplyPathReserved,
    ReplyFulfilled,
    ReplyEndpointClosed,
    ReplyClosed,
    ChildCompleted,
    ChildFailed,
    SiblingCancellationRequested,
    GroupOutcomePublished,
    CancellationAdmissionClosed,
    CancellationPropagated,
    ChildrenQuiesced,
    ResourceReturned,
    CleanupRun,
    CancelledPublished,
    DeadlineUnmeetable,
    DeadlineExceeded,
    TerminalPanic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowSendOutcome {
    Admitted,
    Full,
    Waiting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowCustodian {
    ProposalHome,
    Mailbox,
    ResponseHome,
    ReplyClosed,
    GroupReturnHome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FlowProposalKey {
    destination: u128,
    sender: u128,
    sender_turn_sequence: u64,
    send_ordinal: u32,
}

impl FlowProposalKey {
    #[must_use]
    pub const fn destination(&self) -> u128 {
        self.destination
    }

    #[must_use]
    pub const fn sender(&self) -> u128 {
        self.sender
    }

    #[must_use]
    pub const fn sender_turn_sequence(&self) -> u64 {
        self.sender_turn_sequence
    }

    #[must_use]
    pub const fn send_ordinal(&self) -> u32 {
        self.send_ordinal
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FlowRequirement {
    identity: u128,
    kind: FlowRequirementKind,
    actor: u128,
    handler: Option<u128>,
    site: Option<u128>,
    bound: u64,
    current_meaning: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FlowRequirementRef {
    identity: u128,
    current_meaning: u128,
}

#[derive(Clone, Copy)]
pub(crate) struct ImagePlanningFlowView<'a> {
    flow: &'a VerifiedFlowProgram,
}

#[derive(Clone, Copy)]
pub(crate) struct ImagePlanningFlowRequirement<'a>(&'a FlowRequirement);

#[derive(Clone, Copy)]
pub(crate) struct ImagePlanningActor<'a>(&'a Actor);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Actor {
    identity: u128,
    actor_type_identity: u128,
    construction_identity: u128,
    construction_current_meaning: u128,
    source: crate::SourceRange,
    mailbox_capacity: u64,
    max_active_turns: u8,
    permanent_core_requirement: u128,
    handlers: Arc<[u128]>,
    wired_actor_constructions: Arc<[u128]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SuspensionHome {
    identity: u128,
    actor: u128,
    handler: u128,
    suspension_reference: u128,
    suspension_current_meaning: u128,
    control_path: Arc<[u32]>,
    program_order: u32,
    source: crate::SourceRange,
    slot_count: u64,
    retains_turn_lease: bool,
    requirement: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FlowEvent {
    sequence: u64,
    program_order: u64,
    causal_predecessor: Option<u64>,
    logical_commit: Option<u64>,
    kind: FlowEventKind,
    actor: u128,
    handler: u128,
    turn_sequence: u64,
    proposal: Option<FlowProposalKey>,
    suspension_home: Option<u128>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProposalTemplate {
    identity: u128,
    current_meaning: u128,
    sender: u128,
    sender_handler: u128,
    destination: u128,
    destination_handler: u128,
    admission_kind: FlowAdmissionKind,
    owning_group: Option<u128>,
    deadline_class: Option<FlowDeadlineClass>,
    response_type_identity: u128,
    send_ordinal: u32,
    program_order: u32,
    suspension_home: u128,
    control_path: Arc<[u32]>,
    resource_custody: Arc<[FlowResourceCustody]>,
    source: crate::SourceRange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReplyObligation {
    identity: u128,
    request_template: u128,
    endpoint: u128,
    return_path: u128,
    response_home: u128,
    response_type_identity: u128,
    fulfillment_references: Arc<[(u128, u128)]>,
    fulfillment_endpoint_places: Arc<[Arc<[u128]>]>,
    response_custody: Arc<[FlowResourceCustody]>,
    explicit_cancel: bool,
    capacity: u64,
    fulfillment_capacity_infallible: bool,
    acyclic_wait_requirement: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GroupObligation {
    identity: u128,
    actor: u128,
    handler: u128,
    receiver_place: Arc<[u128]>,
    receiver_current_meaning: u128,
    child_activation_bound: u64,
    child_activations: Arc<[u128]>,
    cancellation_authority: u128,
    policy: FlowGroupPolicy,
    deadline_class: Option<FlowDeadlineClass>,
    deadline_authority: Option<u128>,
    deadline_authority_current_meaning: Option<u128>,
    deadline_capture_authority: Option<u128>,
    deadline_capture_current_meaning: Option<u128>,
    deadline_slack: Option<u64>,
    return_home: u128,
    moved_resources: Arc<[FlowResourceCustody]>,
    cleanup_actions: Arc<[CleanupAction]>,
    cancellation_checkpoints: Arc<[u128]>,
    maximum_uninterrupted_work_units: u64,
    cancelled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CleanupAction {
    identity: u128,
    current_meaning: u128,
    handler: u128,
    program_order: u32,
    source: crate::SourceRange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TerminalPanicFact {
    handler: u128,
    identity: u128,
    current_meaning: u128,
    program_order: u32,
    source: crate::SourceRange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GroupPolicyLaw {
    policy: FlowGroupPolicy,
    deterministic_result_order: bool,
    cancels_siblings: bool,
    collects_failures: bool,
    supervises_failures: bool,
    winner_is_logical: bool,
    host_completion_ignored: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DeadlineLaw {
    class: FlowDeadlineClass,
    authority: u128,
    deterministic: bool,
    replay_capture_required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StructuredEvent {
    sequence: u64,
    kind: FlowEventKind,
    custodian: Option<FlowCustodian>,
    must_use: bool,
    subject: Option<u128>,
    logical_coordinate: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StructuredScenario {
    kind: FlowStructuredScenarioKind,
    outcome: FlowStructuredOutcome,
    events: Arc<[StructuredEvent]>,
    winner_order: Arc<[u32]>,
    cleanup_order: Arc<[u32]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RawProposal {
    template: u128,
    key: FlowProposalKey,
    arrival_ordinal: u32,
    operation_reference: u128,
    operation_current_meaning: u128,
    admission_kind: FlowAdmissionKind,
    control_path: Arc<[u32]>,
    resource_custody: Arc<[FlowResourceCustody]>,
    source: crate::SourceRange,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FlowResourceCustody {
    core_reference_identity: u128,
    core_reference_current_meaning: u128,
    type_identity: u128,
    place: Arc<[u128]>,
    source_home: u128,
    proposal_home: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FlowProposal {
    template: u128,
    key: FlowProposalKey,
    arrival_ordinal: u32,
    source: crate::SourceRange,
    outcome: FlowSendOutcome,
    resource_custody: Arc<[FlowResourceCustody]>,
    before_commit: FlowCustodian,
    after_arbitration: FlowCustodian,
    transfer_commit: Option<u64>,
    waited_for_capacity: bool,
    dequeued_proposal: Option<FlowProposalKey>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ModelContract {
    actors: Arc<[(u128, Arc<[u128]>)]>,
    suspension_homes: Arc<[SuspensionHome]>,
    templates: Arc<[ProposalTemplate]>,
    mailbox_capacities: Arc<[(u128, u64)]>,
    replies: Arc<[ReplyObligation]>,
    groups: Arc<[GroupObligation]>,
    policy_laws: Arc<[GroupPolicyLaw]>,
    deadline_laws: Arc<[DeadlineLaw]>,
    terminal_panics: Arc<[TerminalPanicFact]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ModelResult {
    scenarios: Arc<[ModelScenario]>,
    agrees: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ModelScenario {
    identity: u128,
    selected_paths: SelectedControlPaths,
    turn_activation_bound: u64,
    trace: Arc<[FlowEvent]>,
    proposals: Arc<[FlowProposal]>,
}

#[derive(Clone)]
pub(crate) struct VerifiedFlowProgram {
    context: u128,
    planning_fingerprint: u128,
    core_fingerprint: u128,
    actors: Arc<[Actor]>,
    requirements: Arc<[FlowRequirement]>,
    suspension_homes: Arc<[SuspensionHome]>,
    proposal_templates: Arc<[ProposalTemplate]>,
    reply_obligations: Arc<[ReplyObligation]>,
    groups: Arc<[GroupObligation]>,
    group_policy_laws: Arc<[GroupPolicyLaw]>,
    deadline_laws: Arc<[DeadlineLaw]>,
    structured_scenarios: Arc<[StructuredScenario]>,
    model_contract: ModelContract,
    model_scenarios: Arc<[ModelScenario]>,
    model: ModelResult,
    model_evidence_complete: bool,
    fingerprint: u128,
    _verified: Verified,
}

#[derive(Clone, Debug)]
struct Verified;

impl fmt::Debug for VerifiedFlowProgram {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedFlowProgram")
            .field("context", &format_args!("{:032x}", self.context))
            .field("fingerprint", &format_args!("{:032x}", self.fingerprint))
            .finish_non_exhaustive()
    }
}

impl PartialEq for VerifiedFlowProgram {
    fn eq(&self, other: &Self) -> bool {
        self.context == other.context && self.fingerprint == other.fingerprint
    }
}

impl Eq for VerifiedFlowProgram {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowActorObservation {
    identity: u128,
    actor_type_identity: u128,
    construction_identity: u128,
    source: crate::SourceRange,
    mailbox_capacity: u64,
    max_active_turns: u8,
    permanent_core_requirement: u128,
    handlers: Arc<[u128]>,
    wired_actor_constructions: Arc<[u128]>,
}

impl FlowActorObservation {
    #[must_use]
    pub const fn identity(&self) -> u128 {
        self.identity
    }

    #[must_use]
    pub const fn actor_type_identity(&self) -> u128 {
        self.actor_type_identity
    }

    #[must_use]
    pub const fn construction_identity(&self) -> u128 {
        self.construction_identity
    }

    #[must_use]
    pub const fn source(&self) -> &crate::SourceRange {
        &self.source
    }

    #[must_use]
    pub const fn mailbox_capacity(&self) -> u64 {
        self.mailbox_capacity
    }

    #[must_use]
    pub const fn max_active_turns(&self) -> u8 {
        self.max_active_turns
    }

    #[must_use]
    pub const fn permanent_core_requirement(&self) -> u128 {
        self.permanent_core_requirement
    }

    #[must_use]
    pub fn handlers(&self) -> &[u128] {
        &self.handlers
    }

    #[must_use]
    pub fn wired_actor_constructions(&self) -> &[u128] {
        &self.wired_actor_constructions
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowSuspensionHomeObservation {
    identity: u128,
    actor: u128,
    handler: u128,
    suspension_reference: u128,
    suspension_current_meaning: u128,
    program_order: u32,
    control_path: Arc<[u32]>,
    source: crate::SourceRange,
    slot_count: u64,
    retains_turn_lease: bool,
    requirement: u128,
}

impl FlowSuspensionHomeObservation {
    #[must_use]
    pub const fn identity(&self) -> u128 {
        self.identity
    }

    #[must_use]
    pub const fn actor(&self) -> u128 {
        self.actor
    }

    #[must_use]
    pub const fn handler(&self) -> u128 {
        self.handler
    }

    #[must_use]
    pub const fn suspension_reference(&self) -> u128 {
        self.suspension_reference
    }

    #[must_use]
    pub const fn suspension_current_meaning(&self) -> u128 {
        self.suspension_current_meaning
    }

    #[must_use]
    pub const fn program_order(&self) -> u32 {
        self.program_order
    }

    #[must_use]
    pub fn control_path(&self) -> &[u32] {
        &self.control_path
    }

    #[must_use]
    pub const fn source(&self) -> &crate::SourceRange {
        &self.source
    }

    #[must_use]
    pub const fn slot_count(&self) -> u64 {
        self.slot_count
    }

    #[must_use]
    pub const fn retains_turn_lease(&self) -> bool {
        self.retains_turn_lease
    }

    #[must_use]
    pub const fn requirement(&self) -> u128 {
        self.requirement
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowRequirementObservation {
    identity: u128,
    kind: FlowRequirementKind,
    actor: u128,
    handler: Option<u128>,
    site: Option<u128>,
    bound: u64,
    current_meaning: u128,
}

impl FlowRequirementObservation {
    #[must_use]
    pub const fn identity(&self) -> u128 {
        self.identity
    }

    #[must_use]
    pub const fn kind(&self) -> FlowRequirementKind {
        self.kind
    }

    #[must_use]
    pub const fn actor(&self) -> u128 {
        self.actor
    }

    #[must_use]
    pub const fn handler(&self) -> Option<u128> {
        self.handler
    }

    #[must_use]
    pub const fn site(&self) -> Option<u128> {
        self.site
    }

    #[must_use]
    pub const fn bound(&self) -> u64 {
        self.bound
    }

    #[must_use]
    pub const fn current_meaning(&self) -> u128 {
        self.current_meaning
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowTraceRecord {
    sequence: u64,
    program_order: u64,
    causal_predecessor: Option<u64>,
    logical_commit: Option<u64>,
    kind: FlowEventKind,
    actor: u128,
    handler: u128,
    turn_sequence: u64,
    proposal: Option<FlowProposalKey>,
    suspension_home: Option<u128>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowProposalObservation {
    template_identity: u128,
    key: FlowProposalKey,
    arrival_ordinal: u32,
    source: crate::SourceRange,
    outcome: FlowSendOutcome,
    resource_arguments: Arc<[u128]>,
    resource_custody: Arc<[FlowResourceCustodyObservation]>,
    before_commit: FlowCustodian,
    after_arbitration: FlowCustodian,
    transfer_commit: Option<u64>,
    waited_for_capacity: bool,
    dequeued_proposal: Option<FlowProposalKey>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowProposalTemplateObservation {
    identity: u128,
    current_meaning: u128,
    sender: u128,
    destination: u128,
    admission_kind: FlowAdmissionKind,
    owning_group: Option<u128>,
    deadline_class: Option<FlowDeadlineClass>,
    send_ordinal: u32,
    program_order: u32,
    suspension_home: u128,
    control_path: Arc<[u32]>,
    source: crate::SourceRange,
    resource_custody: Arc<[FlowResourceCustodyObservation]>,
}

impl FlowProposalTemplateObservation {
    #[must_use]
    pub const fn identity(&self) -> u128 {
        self.identity
    }
    #[must_use]
    pub const fn current_meaning(&self) -> u128 {
        self.current_meaning
    }
    #[must_use]
    pub const fn sender(&self) -> u128 {
        self.sender
    }
    #[must_use]
    pub const fn destination(&self) -> u128 {
        self.destination
    }
    #[must_use]
    pub const fn admission_kind(&self) -> FlowAdmissionKind {
        self.admission_kind
    }
    #[must_use]
    pub const fn owning_group(&self) -> Option<u128> {
        self.owning_group
    }
    #[must_use]
    pub const fn deadline_class(&self) -> Option<FlowDeadlineClass> {
        self.deadline_class
    }
    #[must_use]
    pub const fn send_ordinal(&self) -> u32 {
        self.send_ordinal
    }
    #[must_use]
    pub const fn program_order(&self) -> u32 {
        self.program_order
    }
    #[must_use]
    pub const fn suspension_home(&self) -> u128 {
        self.suspension_home
    }
    #[must_use]
    pub fn control_path(&self) -> &[u32] {
        &self.control_path
    }
    #[must_use]
    pub const fn source(&self) -> &crate::SourceRange {
        &self.source
    }
    #[must_use]
    pub fn resource_custody(&self) -> &[FlowResourceCustodyObservation] {
        &self.resource_custody
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowReplyObligationObservation {
    identity: u128,
    request_template: u128,
    endpoint: u128,
    return_path: u128,
    response_home: u128,
    capacity: u64,
    fulfillment_capacity_infallible: bool,
    acyclic_wait_requirement: u128,
    response_type_identity: u128,
    fulfillment_references: Arc<[(u128, u128)]>,
    fulfillment_endpoint_places: Arc<[Arc<[u128]>]>,
    response_custody: Arc<[FlowResourceCustodyObservation]>,
    explicit_cancel: bool,
}

impl FlowReplyObligationObservation {
    #[must_use]
    pub const fn identity(&self) -> u128 {
        self.identity
    }
    #[must_use]
    pub const fn request_template(&self) -> u128 {
        self.request_template
    }
    #[must_use]
    pub const fn endpoint(&self) -> u128 {
        self.endpoint
    }
    #[must_use]
    pub const fn return_path(&self) -> u128 {
        self.return_path
    }
    #[must_use]
    pub const fn response_home(&self) -> u128 {
        self.response_home
    }
    #[must_use]
    pub const fn capacity(&self) -> u64 {
        self.capacity
    }
    #[must_use]
    pub const fn fulfillment_capacity_infallible(&self) -> bool {
        self.fulfillment_capacity_infallible
    }
    #[must_use]
    pub const fn acyclic_wait_requirement(&self) -> u128 {
        self.acyclic_wait_requirement
    }
    #[must_use]
    pub const fn response_type_identity(&self) -> u128 {
        self.response_type_identity
    }
    #[must_use]
    pub fn fulfillment_references(&self) -> &[(u128, u128)] {
        &self.fulfillment_references
    }
    #[must_use]
    pub fn fulfillment_endpoint_places(&self) -> &[Arc<[u128]>] {
        &self.fulfillment_endpoint_places
    }
    #[must_use]
    pub fn response_custody(&self) -> &[FlowResourceCustodyObservation] {
        &self.response_custody
    }
    #[must_use]
    pub const fn explicit_cancel(&self) -> bool {
        self.explicit_cancel
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowGroupObservation {
    identity: u128,
    actor: u128,
    handler: u128,
    receiver_place: Arc<[u128]>,
    receiver_current_meaning: u128,
    child_activation_bound: u64,
    child_activations: Arc<[u128]>,
    cancellation_authority: u128,
    policy: FlowGroupPolicy,
    deadline_class: Option<FlowDeadlineClass>,
    deadline_authority: Option<u128>,
    deadline_authority_current_meaning: Option<u128>,
    deadline_capture_authority: Option<u128>,
    deadline_capture_current_meaning: Option<u128>,
    deadline_slack: Option<u64>,
    return_home: u128,
    moved_resources: Arc<[FlowResourceCustodyObservation]>,
    maximum_uninterrupted_work_units: u64,
    cleanup_actions: Arc<[u128]>,
    cleanup_execution_order: Arc<[u128]>,
}

impl FlowGroupObservation {
    #[must_use]
    pub const fn identity(&self) -> u128 {
        self.identity
    }
    #[must_use]
    pub const fn actor(&self) -> u128 {
        self.actor
    }
    #[must_use]
    pub const fn handler(&self) -> u128 {
        self.handler
    }
    #[must_use]
    pub fn receiver_place(&self) -> &[u128] {
        &self.receiver_place
    }
    #[must_use]
    pub const fn receiver_current_meaning(&self) -> u128 {
        self.receiver_current_meaning
    }
    #[must_use]
    pub const fn child_activation_bound(&self) -> u64 {
        self.child_activation_bound
    }
    #[must_use]
    pub fn child_activations(&self) -> &[u128] {
        &self.child_activations
    }
    #[must_use]
    pub const fn noncopyable_cancellation_authority(&self) -> u128 {
        self.cancellation_authority
    }
    #[must_use]
    pub const fn policy(&self) -> FlowGroupPolicy {
        self.policy
    }
    #[must_use]
    pub const fn deadline_class(&self) -> Option<FlowDeadlineClass> {
        self.deadline_class
    }
    #[must_use]
    pub const fn deadline_authority(&self) -> Option<u128> {
        self.deadline_authority
    }
    #[must_use]
    pub const fn deadline_authority_current_meaning(&self) -> Option<u128> {
        self.deadline_authority_current_meaning
    }
    #[must_use]
    pub const fn deadline_capture_authority(&self) -> Option<u128> {
        self.deadline_capture_authority
    }
    #[must_use]
    pub const fn deadline_capture_current_meaning(&self) -> Option<u128> {
        self.deadline_capture_current_meaning
    }
    #[must_use]
    pub const fn deadline_slack(&self) -> Option<u64> {
        self.deadline_slack
    }
    #[must_use]
    pub const fn return_home(&self) -> u128 {
        self.return_home
    }
    #[must_use]
    pub fn moved_resources(&self) -> &[FlowResourceCustodyObservation] {
        &self.moved_resources
    }
    #[must_use]
    pub const fn maximum_uninterrupted_work_units(&self) -> u64 {
        self.maximum_uninterrupted_work_units
    }
    #[must_use]
    pub fn cleanup_actions(&self) -> &[u128] {
        &self.cleanup_actions
    }
    #[must_use]
    pub fn cleanup_execution_order(&self) -> &[u128] {
        &self.cleanup_execution_order
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowGroupPolicyLawObservation {
    policy: FlowGroupPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowDeadlineLawObservation {
    class: FlowDeadlineClass,
    authority: u128,
    deterministic: bool,
    replay_capture_required: bool,
}

impl FlowDeadlineLawObservation {
    #[must_use]
    pub const fn class(&self) -> FlowDeadlineClass {
        self.class
    }
    #[must_use]
    pub const fn authority(&self) -> u128 {
        self.authority
    }
    #[must_use]
    pub const fn deterministic(&self) -> bool {
        self.deterministic
    }
    #[must_use]
    pub const fn replay_capture_required(&self) -> bool {
        self.replay_capture_required
    }
}

impl FlowGroupPolicyLawObservation {
    #[must_use]
    pub const fn policy(&self) -> FlowGroupPolicy {
        self.policy
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowStructuredEventObservation {
    kind: FlowEventKind,
    custodian: Option<FlowCustodian>,
    must_use: bool,
    subject: Option<u128>,
    logical_coordinate: u64,
}

impl FlowStructuredEventObservation {
    #[must_use]
    pub const fn kind(&self) -> FlowEventKind {
        self.kind
    }
    #[must_use]
    pub const fn custodian(&self) -> Option<FlowCustodian> {
        self.custodian
    }
    #[must_use]
    pub const fn must_use(&self) -> bool {
        self.must_use
    }
    #[must_use]
    pub const fn subject(&self) -> Option<u128> {
        self.subject
    }
    #[must_use]
    pub const fn logical_coordinate(&self) -> u64 {
        self.logical_coordinate
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowStructuredScenarioObservation {
    kind: FlowStructuredScenarioKind,
    outcome: FlowStructuredOutcome,
    events: Arc<[FlowStructuredEventObservation]>,
    winner_order: Arc<[u32]>,
    cleanup_order: Arc<[u32]>,
}

impl FlowStructuredScenarioObservation {
    #[must_use]
    pub const fn kind(&self) -> FlowStructuredScenarioKind {
        self.kind
    }
    #[must_use]
    pub const fn outcome(&self) -> FlowStructuredOutcome {
        self.outcome
    }
    #[must_use]
    pub fn events(&self) -> &[FlowStructuredEventObservation] {
        &self.events
    }
    #[must_use]
    pub fn winner_order(&self) -> &[u32] {
        &self.winner_order
    }
    #[must_use]
    pub fn cleanup_order(&self) -> &[u32] {
        &self.cleanup_order
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowResourceCustodyObservation {
    core_reference_identity: u128,
    core_reference_current_meaning: u128,
    type_identity: u128,
    place: Arc<[u128]>,
    source_home: u128,
    proposal_home: u128,
}

impl FlowResourceCustodyObservation {
    #[must_use]
    pub const fn core_reference_identity(&self) -> u128 {
        self.core_reference_identity
    }

    #[must_use]
    pub const fn core_reference_current_meaning(&self) -> u128 {
        self.core_reference_current_meaning
    }

    #[must_use]
    pub const fn type_identity(&self) -> u128 {
        self.type_identity
    }

    #[must_use]
    pub fn place(&self) -> &[u128] {
        &self.place
    }

    #[must_use]
    pub const fn source_home(&self) -> u128 {
        self.source_home
    }

    #[must_use]
    pub const fn proposal_home(&self) -> u128 {
        self.proposal_home
    }
}

impl FlowProposalObservation {
    #[must_use]
    pub const fn template_identity(&self) -> u128 {
        self.template_identity
    }
    #[must_use]
    pub const fn key(&self) -> FlowProposalKey {
        self.key
    }

    #[must_use]
    pub const fn arrival_ordinal(&self) -> u32 {
        self.arrival_ordinal
    }

    #[must_use]
    pub const fn source(&self) -> &crate::SourceRange {
        &self.source
    }

    #[must_use]
    pub const fn outcome(&self) -> FlowSendOutcome {
        self.outcome
    }

    #[must_use]
    pub fn resource_arguments(&self) -> &[u128] {
        &self.resource_arguments
    }

    #[must_use]
    pub fn resource_custody(&self) -> &[FlowResourceCustodyObservation] {
        &self.resource_custody
    }

    #[must_use]
    pub const fn before_commit_custodian(&self) -> FlowCustodian {
        self.before_commit
    }

    #[must_use]
    pub const fn after_arbitration_custodian(&self) -> FlowCustodian {
        self.after_arbitration
    }

    #[must_use]
    pub const fn transfer_commit(&self) -> Option<u64> {
        self.transfer_commit
    }

    #[must_use]
    pub const fn waited_for_capacity(&self) -> bool {
        self.waited_for_capacity
    }

    #[must_use]
    pub const fn dequeued_proposal(&self) -> Option<FlowProposalKey> {
        self.dequeued_proposal
    }
}

impl FlowTraceRecord {
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    #[must_use]
    pub const fn program_order(&self) -> u64 {
        self.program_order
    }

    #[must_use]
    pub const fn causal_predecessor(&self) -> Option<u64> {
        self.causal_predecessor
    }

    #[must_use]
    pub const fn logical_commit(&self) -> Option<u64> {
        self.logical_commit
    }

    #[must_use]
    pub const fn kind(&self) -> FlowEventKind {
        self.kind
    }

    #[must_use]
    pub const fn actor(&self) -> u128 {
        self.actor
    }

    #[must_use]
    pub const fn handler(&self) -> u128 {
        self.handler
    }

    #[must_use]
    pub const fn turn_sequence(&self) -> u64 {
        self.turn_sequence
    }

    #[must_use]
    pub const fn proposal(&self) -> Option<FlowProposalKey> {
        self.proposal
    }

    #[must_use]
    pub const fn suspension_home(&self) -> Option<u128> {
        self.suspension_home
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowProgramObservation {
    fingerprint: u128,
    context_identity: u128,
    planning_foundation_fingerprint: u128,
    core_program_fingerprint: u128,
    actors: Arc<[FlowActorObservation]>,
    requirements: Arc<[FlowRequirementObservation]>,
    suspension_homes: Arc<[FlowSuspensionHomeObservation]>,
    proposal_templates: Arc<[FlowProposalTemplateObservation]>,
    reply_obligations: Arc<[FlowReplyObligationObservation]>,
    groups: Arc<[FlowGroupObservation]>,
    group_policy_laws: Arc<[FlowGroupPolicyLawObservation]>,
    deadline_laws: Arc<[FlowDeadlineLawObservation]>,
    structured_scenarios: Arc<[FlowStructuredScenarioObservation]>,
    model_scenarios: Arc<[FlowModelScenarioObservation]>,
    model_case_count: usize,
    model_agrees: bool,
    model_evidence_complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowModelScenarioObservation {
    identity: u128,
    selected_paths: SelectedControlPaths,
    turn_activation_bound: u64,
    trace: Arc<[FlowTraceRecord]>,
    proposals: Arc<[FlowProposalObservation]>,
}

impl FlowModelScenarioObservation {
    #[must_use]
    pub const fn identity(&self) -> u128 {
        self.identity
    }

    #[must_use]
    pub fn selected_paths(&self) -> &[(u128, u128, Arc<[u32]>)] {
        &self.selected_paths
    }

    #[must_use]
    pub const fn turn_activation_bound(&self) -> u64 {
        self.turn_activation_bound
    }

    #[must_use]
    pub fn trace(&self) -> &[FlowTraceRecord] {
        &self.trace
    }

    #[must_use]
    pub fn proposals(&self) -> &[FlowProposalObservation] {
        &self.proposals
    }
}

impl FlowProgramObservation {
    #[must_use]
    pub const fn phase_schema(&self) -> &'static str {
        PHASE_SCHEMA
    }

    #[must_use]
    pub const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }

    #[must_use]
    pub const fn context_identity(&self) -> u128 {
        self.context_identity
    }

    #[must_use]
    pub const fn planning_foundation_fingerprint(&self) -> u128 {
        self.planning_foundation_fingerprint
    }

    #[must_use]
    pub const fn core_program_fingerprint(&self) -> u128 {
        self.core_program_fingerprint
    }

    #[must_use]
    pub fn actors(&self) -> &[FlowActorObservation] {
        &self.actors
    }

    #[must_use]
    pub fn requirements(&self) -> &[FlowRequirementObservation] {
        &self.requirements
    }

    #[must_use]
    pub fn suspension_homes(&self) -> &[FlowSuspensionHomeObservation] {
        &self.suspension_homes
    }

    #[must_use]
    pub fn proposal_templates(&self) -> &[FlowProposalTemplateObservation] {
        &self.proposal_templates
    }

    #[must_use]
    pub fn reply_obligations(&self) -> &[FlowReplyObligationObservation] {
        &self.reply_obligations
    }

    #[must_use]
    pub fn groups(&self) -> &[FlowGroupObservation] {
        &self.groups
    }

    #[must_use]
    pub fn group_policy_laws(&self) -> &[FlowGroupPolicyLawObservation] {
        &self.group_policy_laws
    }

    #[must_use]
    pub fn deadline_laws(&self) -> &[FlowDeadlineLawObservation] {
        &self.deadline_laws
    }

    #[must_use]
    pub fn structured_scenarios(&self) -> &[FlowStructuredScenarioObservation] {
        &self.structured_scenarios
    }

    #[must_use]
    pub fn model_scenarios(&self) -> &[FlowModelScenarioObservation] {
        &self.model_scenarios
    }

    #[must_use]
    pub const fn model_case_count(&self) -> usize {
        self.model_case_count
    }

    #[must_use]
    pub const fn model_agrees(&self) -> bool {
        self.model_agrees
    }

    #[must_use]
    pub const fn model_evidence_complete(&self) -> bool {
        self.model_evidence_complete
    }

    #[must_use]
    pub const fn model_scenario_bound(&self) -> usize {
        MODEL_SCENARIO_BOUND
    }
}

impl VerifiedFlowProgram {
    pub(crate) const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }

    pub(crate) const fn for_image_planning(&self) -> ImagePlanningFlowView<'_> {
        ImagePlanningFlowView { flow: self }
    }

    pub(crate) fn observation(
        &self,
        cancellation: &Cancellation,
    ) -> Result<FlowProgramObservation, FlowFailure> {
        checkpoint(cancellation)?;
        Ok(FlowProgramObservation {
            fingerprint: self.fingerprint,
            context_identity: self.context,
            planning_foundation_fingerprint: self.planning_fingerprint,
            core_program_fingerprint: self.core_fingerprint,
            actors: self
                .actors
                .iter()
                .map(|actor| FlowActorObservation {
                    identity: actor.identity,
                    actor_type_identity: actor.actor_type_identity,
                    construction_identity: actor.construction_identity,
                    source: actor.source.clone(),
                    mailbox_capacity: actor.mailbox_capacity,
                    max_active_turns: actor.max_active_turns,
                    permanent_core_requirement: actor.permanent_core_requirement,
                    handlers: Arc::clone(&actor.handlers),
                    wired_actor_constructions: Arc::clone(&actor.wired_actor_constructions),
                })
                .collect::<Vec<_>>()
                .into(),
            requirements: self
                .requirements
                .iter()
                .map(|requirement| FlowRequirementObservation {
                    identity: requirement.identity,
                    kind: requirement.kind,
                    actor: requirement.actor,
                    handler: requirement.handler,
                    site: requirement.site,
                    bound: requirement.bound,
                    current_meaning: requirement.current_meaning,
                })
                .collect::<Vec<_>>()
                .into(),
            suspension_homes: self
                .suspension_homes
                .iter()
                .map(|home| FlowSuspensionHomeObservation {
                    identity: home.identity,
                    actor: home.actor,
                    handler: home.handler,
                    suspension_reference: home.suspension_reference,
                    suspension_current_meaning: home.suspension_current_meaning,
                    program_order: home.program_order,
                    control_path: Arc::clone(&home.control_path),
                    source: home.source.clone(),
                    slot_count: home.slot_count,
                    retains_turn_lease: home.retains_turn_lease,
                    requirement: home.requirement,
                })
                .collect::<Vec<_>>()
                .into(),
            proposal_templates: self
                .proposal_templates
                .iter()
                .map(|template| FlowProposalTemplateObservation {
                    identity: template.identity,
                    current_meaning: template.current_meaning,
                    sender: template.sender,
                    destination: template.destination,
                    admission_kind: template.admission_kind,
                    owning_group: template.owning_group,
                    deadline_class: template.deadline_class,
                    send_ordinal: template.send_ordinal,
                    program_order: template.program_order,
                    suspension_home: template.suspension_home,
                    control_path: Arc::clone(&template.control_path),
                    source: template.source.clone(),
                    resource_custody: template
                        .resource_custody
                        .iter()
                        .map(|resource| FlowResourceCustodyObservation {
                            core_reference_identity: resource.core_reference_identity,
                            core_reference_current_meaning: resource.core_reference_current_meaning,
                            type_identity: resource.type_identity,
                            place: Arc::clone(&resource.place),
                            source_home: resource.source_home,
                            proposal_home: resource.proposal_home,
                        })
                        .collect::<Vec<_>>()
                        .into(),
                })
                .collect::<Vec<_>>()
                .into(),
            reply_obligations: self
                .reply_obligations
                .iter()
                .map(|reply| FlowReplyObligationObservation {
                    identity: reply.identity,
                    request_template: reply.request_template,
                    endpoint: reply.endpoint,
                    return_path: reply.return_path,
                    response_home: reply.response_home,
                    capacity: reply.capacity,
                    fulfillment_capacity_infallible: reply.fulfillment_capacity_infallible,
                    acyclic_wait_requirement: reply.acyclic_wait_requirement,
                    response_type_identity: reply.response_type_identity,
                    fulfillment_references: Arc::clone(&reply.fulfillment_references),
                    fulfillment_endpoint_places: Arc::clone(&reply.fulfillment_endpoint_places),
                    response_custody: reply
                        .response_custody
                        .iter()
                        .map(|resource| FlowResourceCustodyObservation {
                            core_reference_identity: resource.core_reference_identity,
                            core_reference_current_meaning: resource.core_reference_current_meaning,
                            type_identity: resource.type_identity,
                            place: Arc::clone(&resource.place),
                            source_home: resource.source_home,
                            proposal_home: resource.proposal_home,
                        })
                        .collect::<Vec<_>>()
                        .into(),
                    explicit_cancel: reply.explicit_cancel,
                })
                .collect::<Vec<_>>()
                .into(),
            groups: self
                .groups
                .iter()
                .map(|group| FlowGroupObservation {
                    identity: group.identity,
                    actor: group.actor,
                    handler: group.handler,
                    receiver_place: Arc::clone(&group.receiver_place),
                    receiver_current_meaning: group.receiver_current_meaning,
                    child_activation_bound: group.child_activation_bound,
                    child_activations: Arc::clone(&group.child_activations),
                    cancellation_authority: group.cancellation_authority,
                    policy: group.policy,
                    deadline_class: group.deadline_class,
                    deadline_authority: group.deadline_authority,
                    deadline_authority_current_meaning: group.deadline_authority_current_meaning,
                    deadline_capture_authority: group.deadline_capture_authority,
                    deadline_capture_current_meaning: group.deadline_capture_current_meaning,
                    deadline_slack: group.deadline_slack,
                    return_home: group.return_home,
                    moved_resources: group
                        .moved_resources
                        .iter()
                        .map(|resource| FlowResourceCustodyObservation {
                            core_reference_identity: resource.core_reference_identity,
                            core_reference_current_meaning: resource.core_reference_current_meaning,
                            type_identity: resource.type_identity,
                            place: Arc::clone(&resource.place),
                            source_home: resource.source_home,
                            proposal_home: resource.proposal_home,
                        })
                        .collect::<Vec<_>>()
                        .into(),
                    maximum_uninterrupted_work_units: group.maximum_uninterrupted_work_units,
                    cleanup_actions: group
                        .cleanup_actions
                        .iter()
                        .map(|action| action.identity)
                        .collect::<Vec<_>>()
                        .into(),
                    cleanup_execution_order: group
                        .cleanup_actions
                        .iter()
                        .rev()
                        .map(|action| action.identity)
                        .collect::<Vec<_>>()
                        .into(),
                })
                .collect::<Vec<_>>()
                .into(),
            group_policy_laws: self
                .group_policy_laws
                .iter()
                .map(|law| FlowGroupPolicyLawObservation { policy: law.policy })
                .collect::<Vec<_>>()
                .into(),
            deadline_laws: self
                .deadline_laws
                .iter()
                .map(|law| FlowDeadlineLawObservation {
                    class: law.class,
                    authority: law.authority,
                    deterministic: law.deterministic,
                    replay_capture_required: law.replay_capture_required,
                })
                .collect::<Vec<_>>()
                .into(),
            structured_scenarios: self
                .structured_scenarios
                .iter()
                .map(|scenario| FlowStructuredScenarioObservation {
                    kind: scenario.kind,
                    outcome: scenario.outcome,
                    events: scenario
                        .events
                        .iter()
                        .map(|event| FlowStructuredEventObservation {
                            kind: event.kind,
                            custodian: event.custodian,
                            must_use: event.must_use,
                            subject: event.subject,
                            logical_coordinate: event.logical_coordinate,
                        })
                        .collect::<Vec<_>>()
                        .into(),
                    winner_order: Arc::clone(&scenario.winner_order),
                    cleanup_order: Arc::clone(&scenario.cleanup_order),
                })
                .collect::<Vec<_>>()
                .into(),
            model_scenarios: self
                .model_scenarios
                .iter()
                .map(observe_model_scenario)
                .collect::<Vec<_>>()
                .into(),
            model_case_count: self.model_scenarios.len(),
            model_agrees: self.model.agrees,
            model_evidence_complete: self.model_evidence_complete,
        })
    }
}

impl<'a> ImagePlanningFlowView<'a> {
    pub(crate) const fn context_identity(self) -> u128 {
        self.flow.context
    }

    pub(crate) const fn fingerprint(self) -> u128 {
        self.flow.fingerprint
    }

    pub(crate) const fn planning_fingerprint(self) -> u128 {
        self.flow.planning_fingerprint
    }

    pub(crate) const fn core_fingerprint(self) -> u128 {
        self.flow.core_fingerprint
    }

    pub(crate) fn requirements(
        self,
    ) -> impl ExactSizeIterator<Item = ImagePlanningFlowRequirement<'a>> {
        self.flow
            .requirements
            .iter()
            .map(ImagePlanningFlowRequirement)
    }

    pub(crate) fn actors(self) -> impl ExactSizeIterator<Item = ImagePlanningActor<'a>> {
        self.flow.actors.iter().map(ImagePlanningActor)
    }
}

impl ImagePlanningFlowRequirement<'_> {
    pub(crate) const fn reference(self) -> FlowRequirementRef {
        FlowRequirementRef {
            identity: self.0.identity,
            current_meaning: self.0.current_meaning,
        }
    }

    pub(crate) const fn kind(self) -> FlowRequirementKind {
        self.0.kind
    }

    pub(crate) const fn actor(self) -> u128 {
        self.0.actor
    }

    pub(crate) const fn handler(self) -> Option<u128> {
        self.0.handler
    }

    pub(crate) const fn site(self) -> Option<u128> {
        self.0.site
    }

    pub(crate) const fn bound(self) -> u64 {
        self.0.bound
    }
}

impl FlowRequirementRef {
    pub(crate) const fn identity(self) -> u128 {
        self.identity
    }

    pub(crate) const fn current_meaning(self) -> u128 {
        self.current_meaning
    }
}

impl<'a> ImagePlanningActor<'a> {
    pub(crate) const fn identity(self) -> u128 {
        self.0.identity
    }

    pub(crate) fn handlers(self) -> impl ExactSizeIterator<Item = u128> + 'a {
        self.0.handlers.iter().copied()
    }

    pub(crate) const fn mailbox_capacity(self) -> u64 {
        self.0.mailbox_capacity
    }

    pub(crate) const fn max_active_turns(self) -> u8 {
        self.0.max_active_turns
    }
}

fn observe_model_scenario(scenario: &ModelScenario) -> FlowModelScenarioObservation {
    FlowModelScenarioObservation {
        identity: scenario.identity,
        selected_paths: Arc::clone(&scenario.selected_paths),
        turn_activation_bound: scenario.turn_activation_bound,
        trace: scenario
            .trace
            .iter()
            .map(|event| FlowTraceRecord {
                sequence: event.sequence,
                program_order: event.program_order,
                causal_predecessor: event.causal_predecessor,
                logical_commit: event.logical_commit,
                kind: event.kind,
                actor: event.actor,
                handler: event.handler,
                turn_sequence: event.turn_sequence,
                proposal: event.proposal,
                suspension_home: event.suspension_home,
            })
            .collect::<Vec<_>>()
            .into(),
        proposals: scenario
            .proposals
            .iter()
            .map(|proposal| FlowProposalObservation {
                template_identity: proposal.template,
                key: proposal.key,
                arrival_ordinal: proposal.arrival_ordinal,
                source: proposal.source.clone(),
                outcome: proposal.outcome,
                resource_arguments: proposal
                    .resource_custody
                    .iter()
                    .map(|resource| resource.core_reference_identity)
                    .collect::<Vec<_>>()
                    .into(),
                resource_custody: proposal
                    .resource_custody
                    .iter()
                    .map(|resource| FlowResourceCustodyObservation {
                        core_reference_identity: resource.core_reference_identity,
                        core_reference_current_meaning: resource.core_reference_current_meaning,
                        type_identity: resource.type_identity,
                        place: Arc::clone(&resource.place),
                        source_home: resource.source_home,
                        proposal_home: resource.proposal_home,
                    })
                    .collect::<Vec<_>>()
                    .into(),
                before_commit: proposal.before_commit,
                after_arbitration: proposal.after_arbitration,
                transfer_commit: proposal.transfer_commit,
                waited_for_capacity: proposal.waited_for_capacity,
                dequeued_proposal: proposal.dequeued_proposal,
            })
            .collect::<Vec<_>>()
            .into(),
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FlowModule;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FlowFailure {
    Cancelled,
    Admission {
        sources: Arc<[crate::SourceRange]>,
        actors: Arc<[u128]>,
    },
    Creator {
        code: &'static str,
        source: crate::SourceRange,
    },
    Defect(Arc<str>),
}

impl FlowModule {
    pub(crate) fn derive(
        &self,
        planning: FlowPlanningInput<'_>,
        core: FlowCoreView<'_>,
        cancellation: &Cancellation,
    ) -> Result<VerifiedFlowProgram, FlowFailure> {
        checkpoint(cancellation)?;
        if planning.context_identity() != core.context_identity()
            || planning.semantic_program().context_identity() != planning.context_identity()
        {
            return defect("Flow inputs belong to different compilation contexts");
        }
        let core_executables = core
            .executables()
            .map(|executable| executable.identity())
            .collect::<BTreeSet<_>>();
        let suspension_sites = core.suspension_sites();
        let handler_current_meanings = core.handler_flow_identities();
        let mut actors = Vec::new();
        let mut requirements = Vec::new();
        let mut homes = Vec::new();
        for input in planning.semantic_program().actors() {
            checkpoint(cancellation)?;
            let handlers = input.handlers().collect::<Vec<_>>();
            if handlers
                .iter()
                .any(|handler| !core_executables.contains(handler))
            {
                return defect("Flow Actor handler is not realized by verified Core");
            }
            let placement = requirement_identity(
                input.identity(),
                None,
                None,
                FlowRequirementKind::PermanentCorePlacement,
            );
            actors.push(Actor {
                identity: input.identity(),
                actor_type_identity: input.actor_type_identity(),
                construction_identity: input.construction_identity(),
                construction_current_meaning: input.construction_current_meaning(),
                source: input.source().clone(),
                mailbox_capacity: 1,
                max_active_turns: 1,
                permanent_core_requirement: placement,
                handlers: handlers.clone().into(),
                wired_actor_constructions: input.wired_actor_constructions().into(),
            });
            for (kind, bound) in [
                (FlowRequirementKind::ActorIdentity, 1),
                (FlowRequirementKind::PermanentCorePlacement, 1),
                (FlowRequirementKind::MailboxCapacity, 1),
                (FlowRequirementKind::TurnLease, 1),
                (FlowRequirementKind::LogicalCommitOrder, 1),
                (FlowRequirementKind::ProposalTransport, 1),
            ] {
                requirements.push(requirement_with_authority(
                    input.identity(),
                    None,
                    None,
                    kind,
                    bound,
                    input.construction_identity(),
                    0,
                    input.construction_identity(),
                ));
            }
            for handler in handlers {
                for site in suspension_sites
                    .iter()
                    .filter(|site| site.handler == handler)
                {
                    let requirement = requirement_with_authority(
                        input.identity(),
                        Some(handler),
                        Some(site.reference_identity),
                        FlowRequirementKind::SuspensionHome,
                        1,
                        input.construction_identity(),
                        handler_current_meanings
                            .get(&handler)
                            .copied()
                            .unwrap_or(u128::MAX),
                        site.reference_current_meaning,
                    );
                    let identity = suspension_home_identity(
                        input.identity(),
                        handler,
                        site.reference_identity,
                    );
                    homes.push(SuspensionHome {
                        identity,
                        actor: input.identity(),
                        handler,
                        suspension_reference: site.reference_identity,
                        suspension_current_meaning: site.reference_current_meaning,
                        control_path: Arc::clone(&site.control_path),
                        program_order: site.program_order,
                        source: site.source.clone(),
                        slot_count: 1,
                        retains_turn_lease: true,
                        requirement: requirement.identity,
                    });
                    requirements.push(requirement);
                }
            }
        }
        actors.sort_by_key(|actor| actor.identity);
        requirements.sort_by_key(|requirement| requirement.identity);
        homes.sort_by_key(|home| home.identity);
        let core_groups = core.group_sites();
        let templates = proposal_templates(
            &actors,
            &homes,
            &core_groups,
            core.message_proposals(),
            &handler_current_meanings,
            cancellation,
        )?;
        if let Some(cycle) = reply_wait_cycle(&templates, cancellation)? {
            return Err(FlowFailure::Admission {
                sources: cycle.sources,
                actors: cycle.actors,
            });
        }
        let (reply_obligations, groups, group_policy_laws, deadline_laws, structured_requirements) =
            structured_authority(
                &actors,
                &homes,
                &templates,
                &core_groups,
                &core.reply_fulfillment_sites(),
                &core.cleanup_sites(),
                &handler_current_meanings,
            )?;
        requirements.extend(structured_requirements);
        requirements.sort_by_key(|requirement| requirement.identity);
        let terminal_panics: Arc<[TerminalPanicFact]> = core
            .terminal_panic_sites()
            .into_iter()
            .map(|site| TerminalPanicFact {
                handler: site.handler,
                identity: site.identity,
                current_meaning: site.current_meaning,
                program_order: site.program_order,
                source: site.source,
            })
            .collect::<Vec<_>>()
            .into();
        let structured_scenarios = produce_structured_scenarios(
            &templates,
            &reply_obligations,
            &groups,
            &terminal_panics,
            cancellation,
        )?;
        let model_contract = ModelContract {
            actors: actors
                .iter()
                .map(|actor| (actor.identity, Arc::clone(&actor.handlers)))
                .collect::<Vec<_>>()
                .into(),
            suspension_homes: homes.clone().into(),
            templates: Arc::clone(&templates),
            mailbox_capacities: actors
                .iter()
                .map(|actor| (actor.identity, actor.mailbox_capacity))
                .collect::<Vec<_>>()
                .into(),
            replies: Arc::clone(&reply_obligations),
            groups: Arc::clone(&groups),
            policy_laws: Arc::clone(&group_policy_laws),
            deadline_laws: Arc::clone(&deadline_laws),
            terminal_panics,
        };
        let (model_scenarios, model_evidence_complete) =
            produce_bounded_scenarios(&model_contract, &actors, &homes, &templates, cancellation)?;
        let model = ModelResult {
            agrees: false,
            scenarios: Arc::clone(&model_scenarios),
        };
        let fingerprint = fingerprint(
            FlowFingerprintInput {
                context: planning.context_identity(),
                planning_fingerprint: planning.fingerprint(),
                core_fingerprint: core.fingerprint(),
                actors: &actors,
                requirements: &requirements,
                homes: &homes,
                templates: &templates,
                contract: &model_contract,
                replies: &reply_obligations,
                groups: &groups,
                policy_laws: &group_policy_laws,
                deadline_laws: &deadline_laws,
            },
            cancellation,
        )?;
        let candidate = VerifiedFlowProgram {
            context: planning.context_identity(),
            planning_fingerprint: planning.fingerprint(),
            core_fingerprint: core.fingerprint(),
            actors: actors.into(),
            requirements: requirements.into(),
            suspension_homes: homes.into(),
            proposal_templates: templates,
            reply_obligations,
            groups,
            group_policy_laws,
            deadline_laws,
            structured_scenarios,
            model_contract,
            model_scenarios,
            model,
            model_evidence_complete,
            fingerprint,
            _verified: Verified,
        };
        verify(&candidate, planning, core, cancellation)?;
        Ok(candidate)
    }
}

#[cfg(test)]
fn requirement(
    actor: u128,
    handler: Option<u128>,
    site: Option<u128>,
    kind: FlowRequirementKind,
    bound: u64,
) -> FlowRequirement {
    requirement_with_authority(
        actor,
        handler,
        site,
        kind,
        bound,
        actor,
        0,
        site.unwrap_or(0),
    )
}

#[allow(clippy::too_many_arguments)]
fn requirement_with_authority(
    actor: u128,
    handler: Option<u128>,
    site: Option<u128>,
    kind: FlowRequirementKind,
    bound: u64,
    owner_current_meaning: u128,
    handler_current_meaning: u128,
    subject_current_meaning: u128,
) -> FlowRequirement {
    let identity = requirement_identity(actor, handler, site, kind);
    let mut hash = Xxh3::new();
    hash.update(b"wrela.flow.requirement-meaning\0\x01");
    hash.update(&identity.to_be_bytes());
    hash.update(&[kind.tag()]);
    hash.update(&actor.to_be_bytes());
    hash.update(&site.unwrap_or(0).to_be_bytes());
    hash.update(&owner_current_meaning.to_be_bytes());
    hash.update(&handler_current_meaning.to_be_bytes());
    hash.update(&subject_current_meaning.to_be_bytes());
    hash.update(&bound.to_be_bytes());
    FlowRequirement {
        identity,
        kind,
        actor,
        handler,
        site,
        bound,
        current_meaning: hash.digest128(),
    }
}

fn reply_requirement_current_meaning(reply: &ReplyObligation, template: &ProposalTemplate) -> u128 {
    let mut parts = vec![
        template.current_meaning,
        reply.endpoint,
        reply.return_path,
        reply.response_home,
        reply.response_type_identity,
    ];
    parts.extend(
        reply
            .fulfillment_references
            .iter()
            .flat_map(|(identity, meaning)| [*identity, *meaning]),
    );
    parts.extend(reply.response_custody.iter().flat_map(|custody| {
        [
            custody.core_reference_identity,
            custody.core_reference_current_meaning,
        ]
    }));
    graph_identity(b"reply-requirement-meaning", &parts)
}

fn group_requirement_current_meaning(group: &GroupObligation) -> u128 {
    let mut parts = vec![
        group.identity,
        group.receiver_current_meaning,
        group.cancellation_authority,
        u128::from(group.policy.tag()),
        group.deadline_authority_current_meaning.unwrap_or(0),
        group.deadline_capture_current_meaning.unwrap_or(0),
        group.return_home,
        u128::from(group.maximum_uninterrupted_work_units),
    ];
    parts.extend(group.child_activations.iter().copied());
    parts.extend(group.moved_resources.iter().flat_map(|custody| {
        [
            custody.core_reference_identity,
            custody.core_reference_current_meaning,
        ]
    }));
    parts.extend(
        group
            .cleanup_actions
            .iter()
            .flat_map(|action| [action.identity, action.current_meaning]),
    );
    graph_identity(b"group-requirement-meaning", &parts)
}

fn requirement_identity(
    actor: u128,
    handler: Option<u128>,
    site: Option<u128>,
    kind: FlowRequirementKind,
) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(b"wrela.flow.requirement\0\x01");
    hash.update(&[kind.tag()]);
    hash.update(&actor.to_be_bytes());
    if matches!(
        kind,
        FlowRequirementKind::ActorIdentity
            | FlowRequirementKind::PermanentCorePlacement
            | FlowRequirementKind::MailboxCapacity
            | FlowRequirementKind::TurnLease
            | FlowRequirementKind::LogicalCommitOrder
            | FlowRequirementKind::ProposalTransport
    ) {
        hash.update(&handler.unwrap_or(0).to_be_bytes());
    }
    hash.update(&site.unwrap_or(0).to_be_bytes());
    hash.digest128()
}

fn suspension_home_identity(actor: u128, _handler: u128, suspension_reference: u128) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(b"wrela.flow.suspension-home\0\x01");
    hash.update(&actor.to_be_bytes());
    hash.update(&suspension_reference.to_be_bytes());
    hash.digest128()
}

fn graph_identity(domain: &[u8], parts: &[u128]) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(b"wrela.flow.structured\0\x01");
    hash.update(domain);
    for part in parts {
        hash.update(&part.to_be_bytes());
    }
    hash.digest128()
}

type StructuredAuthority = (
    Arc<[ReplyObligation]>,
    Arc<[GroupObligation]>,
    Arc<[GroupPolicyLaw]>,
    Arc<[DeadlineLaw]>,
    Vec<FlowRequirement>,
);

fn structured_authority(
    actors: &[Actor],
    _homes: &[SuspensionHome],
    templates: &[ProposalTemplate],
    group_sites: &[crate::core::FlowCoreGroupSite],
    fulfillment_sites: &[crate::core::FlowCoreReplyFulfillmentSite],
    cleanup_sites: &[crate::core::FlowCoreCleanupSite],
    handler_flow_identities: &BTreeMap<u128, u128>,
) -> Result<StructuredAuthority, FlowFailure> {
    let mut replies = Vec::new();
    for template in templates
        .iter()
        .filter(|template| template.admission_kind == FlowAdmissionKind::Request)
    {
        let endpoint = graph_identity(b"reply-endpoint", &[template.identity]);
        let return_path = graph_identity(b"reply-return-path", &[template.identity]);
        let response_home = graph_identity(b"reply-response-home", &[template.identity]);
        let matching_fulfillments = fulfillment_sites
            .iter()
            .filter(|site| {
                site.handler == template.destination_handler
                    && (site.cancelled
                        || site.response_type_identity == template.response_type_identity)
            })
            .collect::<Vec<_>>();
        let fulfillment_references = matching_fulfillments
            .iter()
            .map(|site| (site.reference_identity, site.reference_current_meaning))
            .collect::<Vec<_>>();
        if fulfillment_references.len() != 1 {
            return Err(FlowFailure::Creator {
                code: "admission.reply_requires_fulfillment",
                source: template.source.clone(),
            });
        }
        let fulfillment_endpoint_places = matching_fulfillments
            .iter()
            .map(|site| Arc::clone(&site.endpoint_place))
            .collect::<Vec<_>>();
        let response_custody = matching_fulfillments
            .iter()
            .filter_map(|site| {
                let place = site.response_place.as_ref()?;
                Some(FlowResourceCustody {
                    core_reference_identity: site.reference_identity,
                    core_reference_current_meaning: site.reference_current_meaning,
                    type_identity: site.response_type_identity,
                    place: Arc::clone(place),
                    source_home: graph_identity(
                        b"reply-response-source-home",
                        &[site.reference_identity],
                    ),
                    proposal_home: response_home,
                })
            })
            .collect::<Vec<_>>();
        replies.push(ReplyObligation {
            identity: graph_identity(b"reply", &[template.identity]),
            request_template: template.identity,
            endpoint,
            return_path,
            response_home,
            response_type_identity: template.response_type_identity,
            fulfillment_references: fulfillment_references.into(),
            fulfillment_endpoint_places: fulfillment_endpoint_places.into(),
            response_custody: response_custody.into(),
            explicit_cancel: matching_fulfillments[0].cancelled,
            capacity: 1,
            fulfillment_capacity_infallible: true,
            acyclic_wait_requirement: requirement_identity(
                template.sender,
                Some(template.sender_handler),
                Some(template.identity),
                FlowRequirementKind::ReplyAcyclicWait,
            ),
        });
    }
    replies.sort_by_key(|reply| reply.identity);

    let mut groups = Vec::new();
    for site in group_sites {
        if site.terminal_program_order == u32::MAX || site.reference_current_meaning == 0 {
            return Err(FlowFailure::Creator {
                code: "admission.group_requires_completion",
                source: site.source.clone(),
            });
        }
        if u64::try_from(site.children.len()).unwrap_or(u64::MAX) > site.child_activation_bound {
            return Err(FlowFailure::Creator {
                code: "admission.group_child_bound_exceeded",
                source: site.source.clone(),
            });
        }
        let actor = actors
            .iter()
            .find(|actor| actor.handlers.contains(&site.handler))
            .ok_or_else(|| FlowFailure::Defect(Arc::from("Group site has no Actor owner")))?;
        let policy = match site.policy {
            crate::core::FlowCoreGroupPolicy::All => FlowGroupPolicy::All,
            crate::core::FlowCoreGroupPolicy::Collect => FlowGroupPolicy::Collect,
            crate::core::FlowCoreGroupPolicy::Race => FlowGroupPolicy::Race,
            crate::core::FlowCoreGroupPolicy::Supervise => FlowGroupPolicy::Supervise,
        };
        let deadline_class = site.deadline.map(|deadline| match deadline.class {
            crate::core::FlowCoreDeadlineClass::Logical => FlowDeadlineClass::Logical,
            crate::core::FlowCoreDeadlineClass::Realtime => FlowDeadlineClass::Realtime,
        });
        let identity = graph_identity(b"group", &[site.reference_identity]);
        let return_home = graph_identity(b"group-return-home", &[site.reference_identity]);
        let moved_resources = site
            .children
            .iter()
            .filter(|child| child.moved)
            .map(|child| FlowResourceCustody {
                core_reference_identity: child.identity,
                core_reference_current_meaning: child.current_meaning,
                type_identity: child.type_identity,
                place: Arc::clone(&child.place),
                source_home: graph_identity(b"group-child-source-home", &[child.identity]),
                proposal_home: return_home,
            })
            .collect::<Vec<_>>();
        let mut cleanup_actions = cleanup_sites
            .iter()
            .filter(|cleanup| {
                cleanup.handler == site.handler
                    && cleanup.program_order > site.open_program_order
                    && cleanup.program_order < site.terminal_program_order
            })
            .map(|cleanup| CleanupAction {
                identity: cleanup.identity,
                current_meaning: cleanup.current_meaning,
                handler: cleanup.handler,
                program_order: cleanup.program_order,
                source: cleanup.source.clone(),
            })
            .collect::<Vec<_>>();
        cleanup_actions.sort_by_key(|cleanup| cleanup.program_order);
        let mut cancellation_checkpoints = site
            .children
            .iter()
            .map(|child| child.identity)
            .collect::<Vec<_>>();
        cancellation_checkpoints.push(site.reference_identity);
        cancellation_checkpoints.push(graph_identity(
            b"group-terminal-checkpoint",
            &[
                site.reference_identity,
                u128::from(site.terminal_program_order),
            ],
        ));
        cancellation_checkpoints.sort_unstable();
        cancellation_checkpoints.dedup();
        let maximum_uninterrupted_work_units = u64::from(
            site.terminal_program_order
                .saturating_sub(site.open_program_order),
        )
        .max(1);
        groups.push(GroupObligation {
            identity,
            actor: actor.identity,
            handler: site.handler,
            receiver_place: Arc::clone(&site.place),
            receiver_current_meaning: site.place_current_meaning,
            child_activation_bound: site.child_activation_bound,
            child_activations: site.children.iter().map(|child| child.identity).collect(),
            cancellation_authority: graph_identity(
                b"group-cancellation-authority",
                &[site.reference_identity],
            ),
            policy,
            deadline_class,
            deadline_authority: site.deadline.map(|deadline| deadline.authority),
            deadline_authority_current_meaning: site
                .deadline
                .map(|deadline| deadline.authority_current_meaning),
            deadline_capture_authority: site
                .deadline
                .and_then(|deadline| deadline.capture_authority),
            deadline_capture_current_meaning: site
                .deadline
                .and_then(|deadline| deadline.capture_current_meaning),
            deadline_slack: site.deadline.map(|deadline| deadline.slack),
            return_home,
            moved_resources: moved_resources.into(),
            cleanup_actions: cleanup_actions.into(),
            cancellation_checkpoints: cancellation_checkpoints.into(),
            maximum_uninterrupted_work_units,
            cancelled: site.cancelled,
        });
    }
    groups.sort_by_key(|group| group.identity);
    let mut policies = groups
        .iter()
        .map(|group| GroupPolicyLaw {
            policy: group.policy,
            deterministic_result_order: true,
            cancels_siblings: matches!(group.policy, FlowGroupPolicy::All | FlowGroupPolicy::Race),
            collects_failures: group.policy == FlowGroupPolicy::Collect,
            supervises_failures: group.policy == FlowGroupPolicy::Supervise,
            winner_is_logical: group.policy == FlowGroupPolicy::Race,
            host_completion_ignored: true,
        })
        .collect::<Vec<_>>();
    policies.sort_by_key(|law| law.policy);
    policies.dedup_by_key(|law| law.policy);
    let laws: Arc<[GroupPolicyLaw]> = policies.into();
    let mut deadlines = groups
        .iter()
        .filter_map(|group| {
            Some(DeadlineLaw {
                class: group.deadline_class?,
                authority: group.deadline_authority?,
                deterministic: group.deadline_class == Some(FlowDeadlineClass::Logical),
                replay_capture_required: group.deadline_class == Some(FlowDeadlineClass::Realtime),
            })
        })
        .collect::<Vec<_>>();
    deadlines.sort_by_key(|law| (law.class, law.authority));
    deadlines.dedup();
    let deadline_laws: Arc<[DeadlineLaw]> = deadlines.into();
    let mut requirements = Vec::new();
    for reply in &replies {
        let template = templates
            .iter()
            .find(|template| template.identity == reply.request_template)
            .ok_or_else(|| FlowFailure::Defect(Arc::from("Reply has no request template")))?;
        let actor_current_meaning = actors
            .iter()
            .find(|actor| actor.identity == template.sender)
            .map_or(u128::MAX, |actor| actor.construction_identity);
        let handler_current_meaning = handler_flow_identities
            .get(&template.sender_handler)
            .copied()
            .unwrap_or(u128::MAX);
        let subject_current_meaning = reply_requirement_current_meaning(reply, template);
        for (kind, site) in [
            (FlowRequirementKind::ReplyEndpoint, reply.endpoint),
            (FlowRequirementKind::ReplyReturnPath, reply.return_path),
            (FlowRequirementKind::ReplyResponseHome, reply.response_home),
            (
                FlowRequirementKind::ReplyAcyclicWait,
                reply.request_template,
            ),
        ] {
            requirements.push(requirement_with_authority(
                template.sender,
                Some(template.sender_handler),
                Some(site),
                kind,
                1,
                actor_current_meaning,
                handler_current_meaning,
                subject_current_meaning,
            ));
        }
    }
    for group in &groups {
        let actor_current_meaning = actors
            .iter()
            .find(|actor| actor.identity == group.actor)
            .map_or(u128::MAX, |actor| actor.construction_identity);
        let handler_current_meaning = handler_flow_identities
            .get(&group.handler)
            .copied()
            .unwrap_or(u128::MAX);
        let subject_current_meaning = group_requirement_current_meaning(group);
        for (kind, site, bound) in [
            (
                FlowRequirementKind::GroupChildActivationBound,
                group.identity,
                group.child_activation_bound,
            ),
            (
                FlowRequirementKind::GroupCancellationAuthority,
                group.cancellation_authority,
                1,
            ),
            (FlowRequirementKind::GroupOutcomePolicy, group.identity, 1),
            (
                FlowRequirementKind::GroupResourceReturnHome,
                group.return_home,
                u64::try_from(group.moved_resources.len()).unwrap_or(u64::MAX),
            ),
            (
                FlowRequirementKind::GroupCleanupOrder,
                group.identity,
                u64::try_from(group.cleanup_actions.len()).unwrap_or(u64::MAX),
            ),
            (
                FlowRequirementKind::CancellationObservationWorkBound,
                group.identity,
                group.maximum_uninterrupted_work_units,
            ),
        ] {
            requirements.push(requirement_with_authority(
                group.actor,
                Some(group.handler),
                Some(site),
                kind,
                bound,
                actor_current_meaning,
                handler_current_meaning,
                subject_current_meaning,
            ));
        }
        for checkpoint in group.cancellation_checkpoints.iter() {
            requirements.push(requirement_with_authority(
                group.actor,
                Some(group.handler),
                Some(*checkpoint),
                FlowRequirementKind::CancellationCheckpoint,
                1,
                actor_current_meaning,
                handler_current_meaning,
                subject_current_meaning,
            ));
        }
        if let Some(class) = group.deadline_class {
            for (kind, site, bound) in [
                (
                    FlowRequirementKind::DeadlineClass,
                    group.identity,
                    u64::from(class == FlowDeadlineClass::Realtime) + 1,
                ),
                (
                    FlowRequirementKind::DeadlineAuthority,
                    group.deadline_authority.unwrap_or(0),
                    1,
                ),
                (
                    FlowRequirementKind::DeadlineSlack,
                    group.identity,
                    group.deadline_slack.unwrap_or(0),
                ),
                (FlowRequirementKind::DeadlineFeasibility, group.identity, 1),
            ] {
                requirements.push(requirement_with_authority(
                    group.actor,
                    Some(group.handler),
                    Some(site),
                    kind,
                    bound,
                    actor_current_meaning,
                    handler_current_meaning,
                    subject_current_meaning,
                ));
            }
        }
    }
    Ok((
        replies.into(),
        groups.into(),
        laws,
        deadline_laws,
        requirements,
    ))
}

struct ReplyWaitCycle {
    sources: Arc<[crate::SourceRange]>,
    actors: Arc<[u128]>,
}

fn reply_wait_cycle(
    templates: &[ProposalTemplate],
    cancellation: &Cancellation,
) -> Result<Option<ReplyWaitCycle>, FlowFailure> {
    let mut edges = BTreeMap::<u128, Vec<(u128, crate::SourceRange)>>::new();
    for template in templates
        .iter()
        .filter(|template| template.admission_kind == FlowAdmissionKind::Request)
    {
        edges
            .entry(template.sender)
            .or_default()
            .push((template.destination, template.source.clone()));
    }
    for targets in edges.values_mut() {
        targets.sort_by_key(|(target, source)| (*target, source.clone()));
    }
    let mut complete = BTreeSet::new();
    for start in edges.keys().copied() {
        checkpoint(cancellation)?;
        if complete.contains(&start) {
            continue;
        }
        let mut stack = vec![(start, 0_usize, None::<crate::SourceRange>)];
        let mut active = BTreeMap::from([(start, 0_usize)]);
        while let Some((node, next, _)) = stack.last_mut() {
            checkpoint(cancellation)?;
            let targets = edges.get(node).map(Vec::as_slice).unwrap_or(&[]);
            if *next == targets.len() {
                let (finished, _, _) = stack.pop().expect("nonempty traversal stack");
                active.remove(&finished);
                complete.insert(finished);
                continue;
            }
            let (target, source) = targets[*next].clone();
            *next += 1;
            if let Some(position) = active.get(&target).copied() {
                let actors = stack[position..]
                    .iter()
                    .map(|(actor, _, _)| *actor)
                    .collect::<Vec<_>>();
                let mut sources = stack[position + 1..]
                    .iter()
                    .filter_map(|(_, _, incoming)| incoming.clone())
                    .collect::<Vec<_>>();
                sources.push(source);
                return Ok(Some(ReplyWaitCycle {
                    sources: sources.into(),
                    actors: actors.into(),
                }));
            }
            if !complete.contains(&target) {
                active.insert(target, stack.len());
                stack.push((target, 0, Some(source)));
            }
        }
    }
    Ok(None)
}

fn structured_event(
    sequence: u64,
    kind: FlowEventKind,
    custodian: Option<FlowCustodian>,
    must_use: bool,
) -> StructuredEvent {
    StructuredEvent {
        sequence,
        kind,
        custodian,
        must_use,
        subject: None,
        logical_coordinate: sequence,
    }
}

fn produce_structured_scenarios(
    templates: &[ProposalTemplate],
    replies: &[ReplyObligation],
    groups: &[GroupObligation],
    terminal_panics: &[TerminalPanicFact],
    cancellation: &Cancellation,
) -> Result<Arc<[StructuredScenario]>, FlowFailure> {
    checkpoint(cancellation)?;
    let mut scenarios = Vec::new();
    if !templates.is_empty() {
        scenarios.push(StructuredScenario {
            kind: FlowStructuredScenarioKind::ReversedArrival,
            outcome: FlowStructuredOutcome::Completed,
            events: Arc::from([
                structured_event(
                    0,
                    FlowEventKind::MessageProposed,
                    Some(FlowCustodian::ProposalHome),
                    false,
                ),
                structured_event(
                    1,
                    FlowEventKind::MailboxTransferCommitted,
                    Some(FlowCustodian::Mailbox),
                    false,
                ),
            ]),
            winner_order: Arc::from([0, 1]),
            cleanup_order: Arc::from([]),
        });
    }
    for template in templates.iter().filter(|template| {
        matches!(
            template.admission_kind,
            FlowAdmissionKind::WaitingSend | FlowAdmissionKind::Request
        )
    }) {
        checkpoint(cancellation)?;
        let mut cancelled_events = Vec::new();
        let mut waiting = structured_event(
            0,
            FlowEventKind::AdmissionWaiting,
            Some(FlowCustodian::ProposalHome),
            false,
        );
        waiting.subject = Some(template.identity);
        cancelled_events.push(waiting);
        if template.admission_kind == FlowAdmissionKind::Request {
            let mut closed = structured_event(
                u64::try_from(cancelled_events.len()).unwrap_or(u64::MAX),
                FlowEventKind::ReplyEndpointClosed,
                Some(FlowCustodian::ResponseHome),
                false,
            );
            closed.subject = replies
                .iter()
                .find(|reply| reply.request_template == template.identity)
                .map(|reply| reply.identity);
            cancelled_events.push(closed);
        }
        for custody in template.resource_custody.iter() {
            let mut returned = structured_event(
                u64::try_from(cancelled_events.len()).unwrap_or(u64::MAX),
                FlowEventKind::ResourceReturned,
                Some(FlowCustodian::ProposalHome),
                false,
            );
            returned.subject = Some(custody.core_reference_identity);
            cancelled_events.push(returned);
        }
        let mut cancelled = structured_event(
            u64::try_from(cancelled_events.len()).unwrap_or(u64::MAX),
            FlowEventKind::AdmissionCancelled,
            Some(FlowCustodian::ProposalHome),
            false,
        );
        cancelled.subject = Some(template.identity);
        cancelled_events.push(cancelled);
        scenarios.push(StructuredScenario {
            kind: FlowStructuredScenarioKind::PreCommitCancellation,
            outcome: FlowStructuredOutcome::Cancelled,
            events: cancelled_events.into(),
            winner_order: Arc::from([]),
            cleanup_order: Arc::from([]),
        });
        let mut durable = Vec::new();
        for (kind, custodian) in [
            (FlowEventKind::AdmissionWaiting, FlowCustodian::ProposalHome),
            (
                FlowEventKind::MailboxTransferCommitted,
                FlowCustodian::Mailbox,
            ),
            (
                FlowEventKind::CancellationPropagated,
                FlowCustodian::Mailbox,
            ),
        ] {
            let mut event = structured_event(
                u64::try_from(durable.len()).unwrap_or(u64::MAX),
                kind,
                Some(custodian),
                false,
            );
            event.subject = Some(template.identity);
            durable.push(event);
        }
        scenarios.push(StructuredScenario {
            kind: FlowStructuredScenarioKind::DurableCommit,
            outcome: FlowStructuredOutcome::Completed,
            events: durable.into(),
            winner_order: Arc::from([]),
            cleanup_order: Arc::from([]),
        });
    }
    if !replies.is_empty() {
        scenarios.push(StructuredScenario {
            kind: FlowStructuredScenarioKind::ReplyDelivered,
            outcome: FlowStructuredOutcome::Completed,
            events: Arc::from([
                structured_event(
                    0,
                    FlowEventKind::ReplyPathReserved,
                    Some(FlowCustodian::ResponseHome),
                    false,
                ),
                structured_event(
                    1,
                    FlowEventKind::ReplyFulfilled,
                    Some(FlowCustodian::ResponseHome),
                    false,
                ),
            ]),
            winner_order: Arc::from([]),
            cleanup_order: Arc::from([]),
        });
        scenarios.push(StructuredScenario {
            kind: FlowStructuredScenarioKind::ReplyClosedRecovery,
            outcome: FlowStructuredOutcome::ReplyClosed,
            events: Arc::from([
                structured_event(
                    0,
                    FlowEventKind::ReplyPathReserved,
                    Some(FlowCustodian::ResponseHome),
                    false,
                ),
                structured_event(
                    1,
                    FlowEventKind::ReplyEndpointClosed,
                    Some(FlowCustodian::ResponseHome),
                    false,
                ),
                structured_event(
                    2,
                    FlowEventKind::ReplyClosed,
                    Some(FlowCustodian::ReplyClosed),
                    true,
                ),
            ]),
            winner_order: Arc::from([]),
            cleanup_order: Arc::from([]),
        });
    }
    for group in groups {
        checkpoint(cancellation)?;
        let mut events = Vec::new();
        for (logical, child) in group.child_activations.iter().rev().enumerate() {
            checkpoint(cancellation)?;
            let mut event = structured_event(
                u64::try_from(events.len()).unwrap_or(u64::MAX),
                if logical == 0 {
                    FlowEventKind::ChildCompleted
                } else {
                    FlowEventKind::ChildFailed
                },
                None,
                false,
            );
            event.subject = Some(*child);
            event.logical_coordinate = u64::try_from(logical).unwrap_or(u64::MAX);
            events.push(event);
        }
        if matches!(group.policy, FlowGroupPolicy::All | FlowGroupPolicy::Race)
            && group.child_activations.len() > 1
        {
            let mut event = structured_event(
                u64::try_from(events.len()).unwrap_or(u64::MAX),
                FlowEventKind::SiblingCancellationRequested,
                None,
                false,
            );
            event.subject = group.child_activations.first().copied();
            events.push(event);
        }
        let mut quiesced = structured_event(
            u64::try_from(events.len()).unwrap_or(u64::MAX),
            FlowEventKind::ChildrenQuiesced,
            None,
            false,
        );
        quiesced.subject = Some(group.identity);
        events.push(quiesced);
        for resource in group.moved_resources.iter() {
            let mut event = structured_event(
                u64::try_from(events.len()).unwrap_or(u64::MAX),
                FlowEventKind::ResourceReturned,
                Some(FlowCustodian::GroupReturnHome),
                false,
            );
            event.subject = Some(resource.core_reference_identity);
            events.push(event);
        }
        let winner_order = match group.policy {
            FlowGroupPolicy::Race => (!group.child_activations.is_empty())
                .then(|| u32::try_from(group.child_activations.len() - 1).unwrap_or(u32::MAX))
                .into_iter()
                .collect::<Vec<_>>(),
            FlowGroupPolicy::All | FlowGroupPolicy::Collect | FlowGroupPolicy::Supervise => {
                (0..u32::try_from(group.child_activations.len()).unwrap_or(u32::MAX)).collect()
            }
        };
        let mut published = structured_event(
            u64::try_from(events.len()).unwrap_or(u64::MAX),
            FlowEventKind::GroupOutcomePublished,
            None,
            false,
        );
        published.subject = Some(group.identity);
        events.push(published);
        scenarios.push(StructuredScenario {
            kind: FlowStructuredScenarioKind::GroupPolicies,
            outcome: if group.cancelled
                || (group.policy == FlowGroupPolicy::All && group.child_activations.len() > 1)
            {
                FlowStructuredOutcome::Cancelled
            } else {
                FlowStructuredOutcome::Completed
            },
            events: events.into(),
            winner_order: winner_order.into(),
            cleanup_order: Arc::from([]),
        });
        if group.deadline_class.is_some() {
            let mut deadline_events = Vec::new();
            for kind in [
                FlowEventKind::CancellationAdmissionClosed,
                FlowEventKind::CancellationPropagated,
                FlowEventKind::ChildrenQuiesced,
            ] {
                checkpoint(cancellation)?;
                let mut event = structured_event(
                    u64::try_from(deadline_events.len()).unwrap_or(u64::MAX),
                    kind,
                    Some(FlowCustodian::GroupReturnHome),
                    false,
                );
                event.subject = Some(group.identity);
                deadline_events.push(event);
            }
            for resource in group.moved_resources.iter() {
                checkpoint(cancellation)?;
                let mut event = structured_event(
                    u64::try_from(deadline_events.len()).unwrap_or(u64::MAX),
                    FlowEventKind::ResourceReturned,
                    Some(FlowCustodian::GroupReturnHome),
                    false,
                );
                event.subject = Some(resource.core_reference_identity);
                deadline_events.push(event);
            }
            for action in group.cleanup_actions.iter().rev() {
                checkpoint(cancellation)?;
                let mut event = structured_event(
                    u64::try_from(deadline_events.len()).unwrap_or(u64::MAX),
                    FlowEventKind::CleanupRun,
                    None,
                    false,
                );
                event.subject = Some(action.identity);
                deadline_events.push(event);
            }
            let mut expired = structured_event(
                u64::try_from(deadline_events.len()).unwrap_or(u64::MAX),
                FlowEventKind::DeadlineExceeded,
                None,
                false,
            );
            expired.subject = Some(group.identity);
            deadline_events.push(expired);
            scenarios.push(StructuredScenario {
                kind: FlowStructuredScenarioKind::DeadlineExceeded,
                outcome: FlowStructuredOutcome::DeadlineExceeded,
                events: deadline_events.into(),
                winner_order: Arc::from([]),
                cleanup_order: (0..u32::try_from(group.cleanup_actions.len()).unwrap_or(u32::MAX))
                    .collect::<Vec<_>>()
                    .into(),
            });
        }
        if group.cancelled || !group.cleanup_actions.is_empty() {
            let cleanup_order = group
                .cleanup_actions
                .iter()
                .rev()
                .enumerate()
                .map(|(index, _)| u32::try_from(index).unwrap_or(u32::MAX))
                .collect::<Vec<_>>();
            let mut cancellation_events = Vec::new();
            for kind in [
                FlowEventKind::CancellationAdmissionClosed,
                FlowEventKind::CancellationPropagated,
                FlowEventKind::ChildrenQuiesced,
            ] {
                let mut event = structured_event(
                    u64::try_from(cancellation_events.len()).unwrap_or(u64::MAX),
                    kind,
                    Some(FlowCustodian::GroupReturnHome),
                    false,
                );
                event.subject = Some(group.identity);
                cancellation_events.push(event);
            }
            for resource in group.moved_resources.iter() {
                let mut event = structured_event(
                    u64::try_from(cancellation_events.len()).unwrap_or(u64::MAX),
                    FlowEventKind::ResourceReturned,
                    Some(FlowCustodian::GroupReturnHome),
                    false,
                );
                event.subject = Some(resource.core_reference_identity);
                cancellation_events.push(event);
            }
            for action in group.cleanup_actions.iter().rev() {
                let mut event = structured_event(
                    u64::try_from(cancellation_events.len()).unwrap_or(u64::MAX),
                    FlowEventKind::CleanupRun,
                    None,
                    false,
                );
                event.subject = Some(action.identity);
                cancellation_events.push(event);
            }
            let mut published = structured_event(
                u64::try_from(cancellation_events.len()).unwrap_or(u64::MAX),
                FlowEventKind::CancelledPublished,
                None,
                false,
            );
            published.subject = Some(group.identity);
            cancellation_events.push(published);
            scenarios.push(StructuredScenario {
                kind: FlowStructuredScenarioKind::ReverseCleanup,
                outcome: FlowStructuredOutcome::Cancelled,
                events: cancellation_events.into(),
                winner_order: Arc::from([]),
                cleanup_order: cleanup_order.into(),
            });
        }
    }
    for panic in terminal_panics {
        checkpoint(cancellation)?;
        let mut event = structured_event(0, FlowEventKind::TerminalPanic, None, false);
        event.subject = Some(panic.identity);
        event.logical_coordinate = u64::from(panic.program_order);
        scenarios.push(StructuredScenario {
            kind: FlowStructuredScenarioKind::TerminalPanic,
            outcome: FlowStructuredOutcome::Panic,
            events: Arc::from([event]),
            winner_order: Arc::from([]),
            cleanup_order: Arc::from([]),
        });
    }
    for scenario in &mut scenarios {
        checkpoint(cancellation)?;
        for event in Arc::make_mut(&mut scenario.events) {
            checkpoint(cancellation)?;
            if event.subject.is_some() {
                continue;
            }
            event.subject = match event.kind {
                FlowEventKind::ReplyClosed => replies.first().and_then(|reply| {
                    reply
                        .response_custody
                        .first()
                        .map(|custody| custody.core_reference_identity)
                }),
                FlowEventKind::ReplyPathReserved
                | FlowEventKind::ReplyFulfilled
                | FlowEventKind::ReplyEndpointClosed => replies.first().map(|reply| reply.identity),
                _ => templates.first().map(|template| template.identity),
            };
        }
    }
    scenarios.sort_by_key(|scenario| scenario.kind);
    Ok(scenarios.into())
}

#[cfg(test)]
fn execute_independent_structured_model(
    templates: &[ProposalTemplate],
    replies: &[ReplyObligation],
    groups: &[GroupObligation],
    terminal_panics: &[TerminalPanicFact],
    cancellation: &Cancellation,
) -> Result<Arc<[StructuredScenario]>, FlowFailure> {
    checkpoint(cancellation)?;
    type ModelEvent = (FlowEventKind, Option<FlowCustodian>, bool);
    type ModelScript = &'static [ModelEvent];
    type ModelOrder = &'static [u32];

    let mut kinds = Vec::new();
    if !templates.is_empty() {
        kinds.push(FlowStructuredScenarioKind::ReversedArrival);
    }
    if !replies.is_empty() {
        kinds.push(FlowStructuredScenarioKind::ReplyDelivered);
        kinds.push(FlowStructuredScenarioKind::ReplyClosedRecovery);
    }
    kinds.extend(
        terminal_panics
            .iter()
            .map(|_| FlowStructuredScenarioKind::TerminalPanic),
    );
    let mut scenarios = Vec::new();
    for kind in kinds {
        checkpoint(cancellation)?;
        let (outcome, script, winner_order, cleanup_order): (
            FlowStructuredOutcome,
            ModelScript,
            ModelOrder,
            ModelOrder,
        ) = match kind {
            FlowStructuredScenarioKind::ReversedArrival => (
                FlowStructuredOutcome::Completed,
                &[
                    (
                        FlowEventKind::MessageProposed,
                        Some(FlowCustodian::ProposalHome),
                        false,
                    ),
                    (
                        FlowEventKind::MailboxTransferCommitted,
                        Some(FlowCustodian::Mailbox),
                        false,
                    ),
                ],
                &[0, 1],
                &[],
            ),
            FlowStructuredScenarioKind::PreCommitCancellation => (
                FlowStructuredOutcome::Cancelled,
                &[
                    (
                        FlowEventKind::AdmissionWaiting,
                        Some(FlowCustodian::ProposalHome),
                        false,
                    ),
                    (
                        FlowEventKind::AdmissionCancelled,
                        Some(FlowCustodian::ProposalHome),
                        false,
                    ),
                ],
                &[],
                &[],
            ),
            FlowStructuredScenarioKind::DurableCommit => (
                FlowStructuredOutcome::Completed,
                &[
                    (
                        FlowEventKind::AdmissionWaiting,
                        Some(FlowCustodian::ProposalHome),
                        false,
                    ),
                    (
                        FlowEventKind::MailboxTransferCommitted,
                        Some(FlowCustodian::Mailbox),
                        false,
                    ),
                    (
                        FlowEventKind::CancellationPropagated,
                        Some(FlowCustodian::Mailbox),
                        false,
                    ),
                ],
                &[],
                &[],
            ),
            FlowStructuredScenarioKind::ReplyClosedRecovery => (
                FlowStructuredOutcome::ReplyClosed,
                &[
                    (
                        FlowEventKind::ReplyPathReserved,
                        Some(FlowCustodian::ResponseHome),
                        false,
                    ),
                    (
                        FlowEventKind::ReplyEndpointClosed,
                        Some(FlowCustodian::ResponseHome),
                        false,
                    ),
                    (
                        FlowEventKind::ReplyClosed,
                        Some(FlowCustodian::ReplyClosed),
                        true,
                    ),
                ],
                &[],
                &[],
            ),
            FlowStructuredScenarioKind::ReplyDelivered => (
                FlowStructuredOutcome::Completed,
                &[
                    (
                        FlowEventKind::ReplyPathReserved,
                        Some(FlowCustodian::ResponseHome),
                        false,
                    ),
                    (
                        FlowEventKind::ReplyFulfilled,
                        Some(FlowCustodian::ResponseHome),
                        false,
                    ),
                ],
                &[],
                &[],
            ),
            FlowStructuredScenarioKind::GroupPolicies => (
                FlowStructuredOutcome::Completed,
                &[
                    (FlowEventKind::ChildrenQuiesced, None, false),
                    (
                        FlowEventKind::ResourceReturned,
                        Some(FlowCustodian::GroupReturnHome),
                        false,
                    ),
                ],
                &[0, 1, 2, 3],
                &[],
            ),
            FlowStructuredScenarioKind::DeadlineUnmeetable => (
                FlowStructuredOutcome::DeadlineUnmeetable,
                &[(
                    FlowEventKind::DeadlineUnmeetable,
                    Some(FlowCustodian::ProposalHome),
                    false,
                )],
                &[],
                &[],
            ),
            FlowStructuredScenarioKind::DeadlineExceeded => (
                FlowStructuredOutcome::DeadlineExceeded,
                &[
                    (
                        FlowEventKind::DeadlineExceeded,
                        Some(FlowCustodian::GroupReturnHome),
                        false,
                    ),
                    (
                        FlowEventKind::ResourceReturned,
                        Some(FlowCustodian::GroupReturnHome),
                        false,
                    ),
                    (FlowEventKind::CleanupRun, None, false),
                ],
                &[],
                &[2, 1, 0],
            ),
            FlowStructuredScenarioKind::ReverseCleanup => (
                FlowStructuredOutcome::Cancelled,
                &[
                    (
                        FlowEventKind::CancellationAdmissionClosed,
                        Some(FlowCustodian::GroupReturnHome),
                        false,
                    ),
                    (
                        FlowEventKind::CancellationPropagated,
                        Some(FlowCustodian::GroupReturnHome),
                        false,
                    ),
                    (
                        FlowEventKind::ChildrenQuiesced,
                        Some(FlowCustodian::GroupReturnHome),
                        false,
                    ),
                    (
                        FlowEventKind::ResourceReturned,
                        Some(FlowCustodian::GroupReturnHome),
                        false,
                    ),
                    (FlowEventKind::CleanupRun, None, false),
                    (FlowEventKind::CleanupRun, None, false),
                    (FlowEventKind::CleanupRun, None, false),
                    (FlowEventKind::CancelledPublished, None, false),
                ],
                &[],
                &[2, 1, 0],
            ),
            FlowStructuredScenarioKind::CustodyRecovery => (
                FlowStructuredOutcome::Cancelled,
                &[
                    (
                        FlowEventKind::AdmissionCancelled,
                        Some(FlowCustodian::ProposalHome),
                        false,
                    ),
                    (
                        FlowEventKind::ResourceReturned,
                        Some(FlowCustodian::GroupReturnHome),
                        false,
                    ),
                ],
                &[],
                &[],
            ),
            FlowStructuredScenarioKind::TerminalPanic => (
                FlowStructuredOutcome::Panic,
                &[(FlowEventKind::TerminalPanic, None, false)],
                &[],
                &[],
            ),
        };
        let events = script
            .iter()
            .enumerate()
            .map(|(sequence, (event, custodian, must_use))| {
                let mut modeled = structured_event(
                    u64::try_from(sequence).unwrap_or(u64::MAX),
                    *event,
                    *custodian,
                    *must_use,
                );
                modeled.subject = match event {
                    FlowEventKind::ReplyClosed => replies.first().and_then(|reply| {
                        reply
                            .response_custody
                            .first()
                            .map(|custody| custody.core_reference_identity)
                    }),
                    FlowEventKind::ReplyPathReserved
                    | FlowEventKind::ReplyFulfilled
                    | FlowEventKind::ReplyEndpointClosed => {
                        replies.first().map(|reply| reply.identity)
                    }
                    FlowEventKind::TerminalPanic => {
                        terminal_panics.first().map(|panic| panic.identity)
                    }
                    _ => templates.first().map(|template| template.identity),
                };
                if *event == FlowEventKind::TerminalPanic {
                    modeled.logical_coordinate = terminal_panics
                        .first()
                        .map_or(0, |panic| u64::from(panic.program_order));
                }
                modeled
            })
            .collect::<Vec<_>>()
            .into();
        scenarios.push(StructuredScenario {
            kind,
            outcome,
            events,
            winner_order: Arc::from(winner_order),
            cleanup_order: Arc::from(cleanup_order),
        });
    }
    for template in templates.iter().filter(|template| {
        template.admission_kind == FlowAdmissionKind::WaitingSend
            || template.admission_kind == FlowAdmissionKind::Request
    }) {
        checkpoint(cancellation)?;
        let mut precommit = Vec::new();
        let mut waiting = structured_event(
            0,
            FlowEventKind::AdmissionWaiting,
            Some(FlowCustodian::ProposalHome),
            false,
        );
        waiting.subject = Some(template.identity);
        precommit.push(waiting);
        if template.admission_kind == FlowAdmissionKind::Request {
            let mut closed = structured_event(
                u64::try_from(precommit.len()).unwrap_or(u64::MAX),
                FlowEventKind::ReplyEndpointClosed,
                Some(FlowCustodian::ResponseHome),
                false,
            );
            closed.subject = replies
                .iter()
                .find(|reply| reply.request_template == template.identity)
                .map(|reply| reply.identity);
            precommit.push(closed);
        }
        for resource in template.resource_custody.iter() {
            let mut returned = structured_event(
                u64::try_from(precommit.len()).unwrap_or(u64::MAX),
                FlowEventKind::ResourceReturned,
                Some(FlowCustodian::ProposalHome),
                false,
            );
            returned.subject = Some(resource.core_reference_identity);
            precommit.push(returned);
        }
        let mut cancelled = structured_event(
            u64::try_from(precommit.len()).unwrap_or(u64::MAX),
            FlowEventKind::AdmissionCancelled,
            Some(FlowCustodian::ProposalHome),
            false,
        );
        cancelled.subject = Some(template.identity);
        precommit.push(cancelled);
        scenarios.push(StructuredScenario {
            kind: FlowStructuredScenarioKind::PreCommitCancellation,
            outcome: FlowStructuredOutcome::Cancelled,
            events: precommit.into(),
            winner_order: Arc::from([]),
            cleanup_order: Arc::from([]),
        });
        let mut durable = Vec::new();
        for (kind, custodian) in [
            (FlowEventKind::AdmissionWaiting, FlowCustodian::ProposalHome),
            (
                FlowEventKind::MailboxTransferCommitted,
                FlowCustodian::Mailbox,
            ),
            (
                FlowEventKind::CancellationPropagated,
                FlowCustodian::Mailbox,
            ),
        ] {
            let mut event = structured_event(
                u64::try_from(durable.len()).unwrap_or(u64::MAX),
                kind,
                Some(custodian),
                false,
            );
            event.subject = Some(template.identity);
            durable.push(event);
        }
        scenarios.push(StructuredScenario {
            kind: FlowStructuredScenarioKind::DurableCommit,
            outcome: FlowStructuredOutcome::Completed,
            events: durable.into(),
            winner_order: Arc::from([]),
            cleanup_order: Arc::from([]),
        });
    }
    for group in groups {
        checkpoint(cancellation)?;
        let mut modeled_events = Vec::new();
        for index in (0..group.child_activations.len()).rev() {
            checkpoint(cancellation)?;
            let logical = group.child_activations.len().saturating_sub(index + 1);
            let mut child = structured_event(
                u64::try_from(modeled_events.len()).unwrap_or(u64::MAX),
                if logical == 0 {
                    FlowEventKind::ChildCompleted
                } else {
                    FlowEventKind::ChildFailed
                },
                None,
                false,
            );
            child.subject = group.child_activations.get(index).copied();
            child.logical_coordinate = u64::try_from(logical).unwrap_or(u64::MAX);
            modeled_events.push(child);
        }
        if matches!(group.policy, FlowGroupPolicy::All | FlowGroupPolicy::Race)
            && group.child_activations.len() > 1
        {
            let mut cancel = structured_event(
                u64::try_from(modeled_events.len()).unwrap_or(u64::MAX),
                FlowEventKind::SiblingCancellationRequested,
                None,
                false,
            );
            cancel.subject = group.child_activations.first().copied();
            modeled_events.push(cancel);
        }
        let mut quiesced = structured_event(
            u64::try_from(modeled_events.len()).unwrap_or(u64::MAX),
            FlowEventKind::ChildrenQuiesced,
            None,
            false,
        );
        quiesced.subject = Some(group.identity);
        modeled_events.push(quiesced);
        for custody in group.moved_resources.iter() {
            checkpoint(cancellation)?;
            let mut returned = structured_event(
                u64::try_from(modeled_events.len()).unwrap_or(u64::MAX),
                FlowEventKind::ResourceReturned,
                Some(FlowCustodian::GroupReturnHome),
                false,
            );
            returned.subject = Some(custody.core_reference_identity);
            modeled_events.push(returned);
        }
        let modeled_winners = if group.policy == FlowGroupPolicy::Race {
            (!group.child_activations.is_empty())
                .then(|| u32::try_from(group.child_activations.len() - 1).unwrap_or(u32::MAX))
                .into_iter()
                .collect::<Vec<_>>()
        } else {
            (0..u32::try_from(group.child_activations.len()).unwrap_or(u32::MAX)).collect()
        };
        let mut published = structured_event(
            u64::try_from(modeled_events.len()).unwrap_or(u64::MAX),
            FlowEventKind::GroupOutcomePublished,
            None,
            false,
        );
        published.subject = Some(group.identity);
        modeled_events.push(published);
        scenarios.push(StructuredScenario {
            kind: FlowStructuredScenarioKind::GroupPolicies,
            outcome: if group.cancelled
                || (group.policy == FlowGroupPolicy::All && group.child_activations.len() > 1)
            {
                FlowStructuredOutcome::Cancelled
            } else {
                FlowStructuredOutcome::Completed
            },
            events: modeled_events.into(),
            winner_order: modeled_winners.into(),
            cleanup_order: Arc::from([]),
        });
        if group.deadline_class.is_some() {
            let mut deadline_events = Vec::new();
            for kind in [
                FlowEventKind::CancellationAdmissionClosed,
                FlowEventKind::CancellationPropagated,
                FlowEventKind::ChildrenQuiesced,
            ] {
                let mut step = structured_event(
                    u64::try_from(deadline_events.len()).unwrap_or(u64::MAX),
                    kind,
                    Some(FlowCustodian::GroupReturnHome),
                    false,
                );
                step.subject = Some(group.identity);
                deadline_events.push(step);
            }
            for custody in group.moved_resources.iter() {
                let mut returned = structured_event(
                    u64::try_from(deadline_events.len()).unwrap_or(u64::MAX),
                    FlowEventKind::ResourceReturned,
                    Some(FlowCustodian::GroupReturnHome),
                    false,
                );
                returned.subject = Some(custody.core_reference_identity);
                deadline_events.push(returned);
            }
            for action in group.cleanup_actions.iter().rev() {
                let mut cleanup = structured_event(
                    u64::try_from(deadline_events.len()).unwrap_or(u64::MAX),
                    FlowEventKind::CleanupRun,
                    None,
                    false,
                );
                cleanup.subject = Some(action.identity);
                deadline_events.push(cleanup);
            }
            let mut expiry = structured_event(
                u64::try_from(deadline_events.len()).unwrap_or(u64::MAX),
                FlowEventKind::DeadlineExceeded,
                None,
                false,
            );
            expiry.subject = Some(group.identity);
            deadline_events.push(expiry);
            scenarios.push(StructuredScenario {
                kind: FlowStructuredScenarioKind::DeadlineExceeded,
                outcome: FlowStructuredOutcome::DeadlineExceeded,
                events: deadline_events.into(),
                winner_order: Arc::from([]),
                cleanup_order: (0..u32::try_from(group.cleanup_actions.len()).unwrap_or(u32::MAX))
                    .collect::<Vec<_>>()
                    .into(),
            });
        }
        if group.cancelled || !group.cleanup_actions.is_empty() {
            let mut recovery = Vec::new();
            for transition in [
                FlowEventKind::CancellationAdmissionClosed,
                FlowEventKind::CancellationPropagated,
                FlowEventKind::ChildrenQuiesced,
            ] {
                let mut event = structured_event(
                    u64::try_from(recovery.len()).unwrap_or(u64::MAX),
                    transition,
                    Some(FlowCustodian::GroupReturnHome),
                    false,
                );
                event.subject = Some(group.identity);
                recovery.push(event);
            }
            for custody in group.moved_resources.iter() {
                let mut event = structured_event(
                    u64::try_from(recovery.len()).unwrap_or(u64::MAX),
                    FlowEventKind::ResourceReturned,
                    Some(FlowCustodian::GroupReturnHome),
                    false,
                );
                event.subject = Some(custody.core_reference_identity);
                recovery.push(event);
            }
            for action in group.cleanup_actions.iter().rev() {
                let mut event = structured_event(
                    u64::try_from(recovery.len()).unwrap_or(u64::MAX),
                    FlowEventKind::CleanupRun,
                    None,
                    false,
                );
                event.subject = Some(action.identity);
                recovery.push(event);
            }
            let mut done = structured_event(
                u64::try_from(recovery.len()).unwrap_or(u64::MAX),
                FlowEventKind::CancelledPublished,
                None,
                false,
            );
            done.subject = Some(group.identity);
            recovery.push(done);
            scenarios.push(StructuredScenario {
                kind: FlowStructuredScenarioKind::ReverseCleanup,
                outcome: FlowStructuredOutcome::Cancelled,
                events: recovery.into(),
                winner_order: Arc::from([]),
                cleanup_order: (0..u32::try_from(group.cleanup_actions.len()).unwrap_or(u32::MAX))
                    .collect::<Vec<_>>()
                    .into(),
            });
        }
    }
    scenarios.sort_by_key(|scenario| scenario.kind);
    Ok(scenarios.into())
}

fn proposal_templates(
    actors: &[Actor],
    homes: &[SuspensionHome],
    group_sites: &[crate::core::FlowCoreGroupSite],
    messages: Vec<crate::core::FlowCoreMessageProposal>,
    handler_current_meanings: &BTreeMap<u128, u128>,
    cancellation: &Cancellation,
) -> Result<Arc<[ProposalTemplate]>, FlowFailure> {
    let handler_owners = actors
        .iter()
        .flat_map(|actor| {
            actor
                .handlers
                .iter()
                .map(move |handler| (*handler, actor.identity))
        })
        .fold(
            BTreeMap::<u128, Vec<u128>>::new(),
            |mut owners, (handler, actor)| {
                owners.entry(handler).or_default().push(actor);
                owners
            },
        );
    let mut templates = Vec::new();
    for message in messages {
        checkpoint(cancellation)?;
        let Some(senders) = handler_owners.get(&message.sender_handler) else {
            return defect("Core message proposal has no Actor sender");
        };
        let Some(destinations) = handler_owners.get(&message.destination_handler) else {
            return defect("Core message proposal has no Actor destination");
        };
        for sender in senders.iter().copied() {
            let sender_actor = actors
                .iter()
                .find(|actor| actor.identity == sender)
                .ok_or_else(|| FlowFailure::Defect(Arc::from("Flow sender Actor vanished")))?;
            let wired_destinations = destinations
                .iter()
                .copied()
                .filter(|destination| {
                    destinations.len() == 1
                        || (*destination == sender
                            && message.admission_kind
                                == crate::core::FlowCoreAdmissionKind::Request)
                        || sender_actor.wired_actor_constructions.contains(destination)
                })
                .collect::<Vec<_>>();
            if wired_destinations.len() != 1 {
                return defect("Core message proposal has no unique build-wired Actor destination");
            }
            for destination in wired_destinations {
                let proposal_home = homes
                    .iter()
                    .find(|home| {
                        home.actor == sender
                            && home.handler == message.sender_handler
                            && home.suspension_reference == message.operation_reference
                    })
                    .ok_or_else(|| {
                        FlowFailure::Defect(Arc::from(
                            "MessageProposal has no exact static Suspension Home",
                        ))
                    })?;
                let mut identity = Xxh3::new();
                identity.update(b"wrela.flow.proposal-template\0\x01");
                identity.update(&message.operation_reference.to_be_bytes());
                identity.update(&sender.to_be_bytes());
                identity.update(&destination.to_be_bytes());
                let identity = identity.digest128();
                let resource_custody = message
                    .custody
                    .iter()
                    .map(|resource| FlowResourceCustody {
                        core_reference_identity: resource.reference_identity,
                        core_reference_current_meaning: resource.reference_current_meaning,
                        type_identity: resource.type_identity,
                        place: Arc::clone(&resource.place),
                        source_home: resource.source_home,
                        proposal_home: proposal_home.identity,
                    })
                    .collect::<Vec<_>>();
                let response_type_identity =
                    if message.admission_kind == crate::core::FlowCoreAdmissionKind::Request {
                        message.response_type_identity
                    } else {
                        0
                    };
                let mut meaning = Xxh3::new();
                meaning.update(b"wrela.flow.proposal-template-meaning\0\x01");
                meaning.update(&identity.to_be_bytes());
                meaning.update(&message.operation_current_meaning.to_be_bytes());
                meaning.update(&sender.to_be_bytes());
                meaning.update(
                    &handler_current_meanings
                        .get(&message.sender_handler)
                        .copied()
                        .unwrap_or(u128::MAX)
                        .to_be_bytes(),
                );
                meaning.update(&destination.to_be_bytes());
                meaning.update(
                    &handler_current_meanings
                        .get(&message.destination_handler)
                        .copied()
                        .unwrap_or(u128::MAX)
                        .to_be_bytes(),
                );
                meaning.update(&response_type_identity.to_be_bytes());
                meaning.update(&[match message.admission_kind {
                    crate::core::FlowCoreAdmissionKind::TrySend => 1,
                    crate::core::FlowCoreAdmissionKind::WaitingSend => 2,
                    crate::core::FlowCoreAdmissionKind::Request => 3,
                }]);
                for resource in &resource_custody {
                    meaning.update(&resource.core_reference_identity.to_be_bytes());
                    meaning.update(&resource.core_reference_current_meaning.to_be_bytes());
                    meaning.update(&resource.source_home.to_be_bytes());
                    meaning.update(&resource.proposal_home.to_be_bytes());
                    for part in resource.place.iter() {
                        meaning.update(&part.to_be_bytes());
                    }
                }
                for part in message.control_path.iter() {
                    meaning.update(&part.to_be_bytes());
                }
                let owning_group_site = group_sites
                    .iter()
                    .filter(|group| {
                        group.handler == message.sender_handler
                            && message.program_order > group.open_program_order
                            && message.program_order < group.terminal_program_order
                    })
                    .max_by_key(|group| group.open_program_order);
                let owning_group = owning_group_site
                    .map(|group| graph_identity(b"group", &[group.reference_identity]));
                let deadline_class = owning_group_site.and_then(|group| {
                    group.deadline.map(|deadline| match deadline.class {
                        crate::core::FlowCoreDeadlineClass::Logical => FlowDeadlineClass::Logical,
                        crate::core::FlowCoreDeadlineClass::Realtime => FlowDeadlineClass::Realtime,
                    })
                });
                meaning.update(&owning_group.unwrap_or(0).to_be_bytes());
                meaning.update(
                    &owning_group_site
                        .map_or(0, |group| group.place_current_meaning)
                        .to_be_bytes(),
                );
                meaning.update(&[deadline_class.map_or(0, FlowDeadlineClass::tag)]);
                templates.push(ProposalTemplate {
                    identity,
                    current_meaning: meaning.digest128(),
                    sender,
                    sender_handler: message.sender_handler,
                    destination,
                    destination_handler: message.destination_handler,
                    admission_kind: match message.admission_kind {
                        crate::core::FlowCoreAdmissionKind::TrySend => FlowAdmissionKind::TrySend,
                        crate::core::FlowCoreAdmissionKind::WaitingSend => {
                            FlowAdmissionKind::WaitingSend
                        }
                        crate::core::FlowCoreAdmissionKind::Request => FlowAdmissionKind::Request,
                    },
                    owning_group,
                    deadline_class,
                    response_type_identity,
                    send_ordinal: message.send_ordinal,
                    program_order: message.program_order,
                    suspension_home: proposal_home.identity,
                    control_path: Arc::clone(&message.control_path),
                    resource_custody: resource_custody.into(),
                    source: message.source.clone(),
                });
            }
        }
    }
    templates
        .sort_by_key(|template| (template.sender, template.destination, template.send_ordinal));
    Ok(templates.into())
}

const MODEL_TURN_ACTIVATION_BOUND: u64 = 2;
const MODEL_SCENARIO_BOUND: usize = 16;

struct PathSelections {
    values: Vec<SelectedControlPaths>,
    complete: bool,
}

fn produce_bounded_scenarios(
    contract: &ModelContract,
    actors: &[Actor],
    homes: &[SuspensionHome],
    templates: &[ProposalTemplate],
    cancellation: &Cancellation,
) -> Result<(Arc<[ModelScenario]>, bool), FlowFailure> {
    let selections = produce_path_selections(contract, cancellation)?;
    let complete = selections.complete;
    let mut scenarios = Vec::with_capacity(selections.values.len());
    for selected_paths in selections.values {
        checkpoint(cancellation)?;
        let mut raw = Vec::new();
        for template in templates.iter().filter(|template| {
            template.control_path.is_empty()
                || selected_paths.iter().any(|(actor, handler, path)| {
                    *actor == template.sender
                        && *handler == template.sender_handler
                        && path.as_ref() == template.control_path.as_ref()
                })
        }) {
            for turn in 0..MODEL_TURN_ACTIVATION_BOUND {
                raw.push(RawProposal {
                    template: template.identity,
                    key: FlowProposalKey {
                        destination: template.destination,
                        sender: template.sender,
                        sender_turn_sequence: turn,
                        send_ordinal: template.send_ordinal,
                    },
                    arrival_ordinal: 0,
                    operation_reference: template.identity,
                    operation_current_meaning: template.current_meaning,
                    admission_kind: template.admission_kind,
                    control_path: Arc::clone(&template.control_path),
                    resource_custody: Arc::clone(&template.resource_custody),
                    source: template.source.clone(),
                });
            }
        }
        raw.sort_by_key(|proposal| proposal.key);
        let count = u32::try_from(raw.len()).unwrap_or(u32::MAX);
        for (index, proposal) in raw.iter_mut().enumerate() {
            proposal.arrival_ordinal = count
                .saturating_sub(1)
                .saturating_sub(u32::try_from(index).unwrap_or(u32::MAX));
        }
        let proposals = arbitrate_proposals(&raw, actors, cancellation)?;
        let trace = produce_trace(
            actors,
            homes,
            templates,
            &proposals,
            selected_paths.as_ref(),
            cancellation,
        )?;
        scenarios.push(ModelScenario {
            identity: model_scenario_identity(&selected_paths),
            selected_paths,
            turn_activation_bound: MODEL_TURN_ACTIVATION_BOUND,
            trace,
            proposals,
        });
    }
    scenarios.sort_by_key(|scenario| scenario.identity);
    Ok((scenarios.into(), complete))
}

fn produce_path_selections(
    contract: &ModelContract,
    cancellation: &Cancellation,
) -> Result<PathSelections, FlowFailure> {
    let mut options = BTreeMap::<(u128, u128), BTreeSet<Arc<[u32]>>>::new();
    for home in contract.suspension_homes.iter() {
        checkpoint(cancellation)?;
        if !home.control_path.is_empty() {
            options
                .entry((home.actor, home.handler))
                .or_default()
                .insert(Arc::clone(&home.control_path));
        } else {
            options.entry((home.actor, home.handler)).or_default();
        }
    }
    for template in contract.templates.iter() {
        checkpoint(cancellation)?;
        if !template.control_path.is_empty() {
            options
                .entry((template.sender, template.sender_handler))
                .or_default()
                .insert(Arc::clone(&template.control_path));
        } else {
            options
                .entry((template.sender, template.sender_handler))
                .or_default();
        }
    }
    let mut selections = vec![Vec::new()];
    let mut complete = true;
    for ((actor, handler), paths) in options {
        let choices = if paths.is_empty() {
            vec![Arc::from([])]
        } else {
            paths.into_iter().collect::<Vec<_>>()
        };
        let mut expanded = Vec::new();
        for selection in &selections {
            for path in &choices {
                let mut next = selection.clone();
                next.push((actor, handler, Arc::clone(path)));
                expanded.push(next);
            }
        }
        expanded.sort();
        if expanded.len() > MODEL_SCENARIO_BOUND {
            complete = false;
            expanded.truncate(MODEL_SCENARIO_BOUND);
        }
        selections = expanded;
    }
    if selections.is_empty() {
        selections.push(Vec::new());
    }
    Ok(PathSelections {
        values: selections.into_iter().map(Arc::from).collect(),
        complete,
    })
}

fn model_scenario_identity(selected_paths: &[SelectedControlPath]) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(b"wrela.flow.model-scenario\0\x01");
    hash.update(&MODEL_TURN_ACTIVATION_BOUND.to_be_bytes());
    for (actor, handler, path) in selected_paths {
        hash.update(&actor.to_be_bytes());
        hash.update(&handler.to_be_bytes());
        hash.update(&u64::try_from(path.len()).unwrap_or(u64::MAX).to_be_bytes());
        for part in path.iter() {
            hash.update(&part.to_be_bytes());
        }
    }
    hash.digest128()
}

fn arbitrate_proposals(
    raw: &[RawProposal],
    actors: &[Actor],
    cancellation: &Cancellation,
) -> Result<Arc<[FlowProposal]>, FlowFailure> {
    let capacities = actors
        .iter()
        .map(|actor| (actor.identity, actor.mailbox_capacity))
        .collect::<BTreeMap<_, _>>();
    let mut ordered = raw.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|proposal| proposal.key);
    let mut mailboxes = BTreeMap::<u128, VecDeque<FlowProposalKey>>::new();
    let mut commit_sequence = 0_u64;
    let mut results = Vec::with_capacity(ordered.len());
    for proposal in ordered {
        checkpoint(cancellation)?;
        let capacity = capacities
            .get(&proposal.key.destination)
            .copied()
            .unwrap_or(0);
        let mailbox = mailboxes.entry(proposal.key.destination).or_default();
        let initially_available = u64::try_from(mailbox.len()).unwrap_or(u64::MAX) < capacity;
        let waits = !initially_available
            && proposal.admission_kind != FlowAdmissionKind::TrySend
            && capacity > 0;
        let dequeued_proposal = waits.then(|| {
            mailbox
                .pop_front()
                .expect("full positive-capacity Mailbox has a committed proposal")
        });
        let admitted = initially_available || waits;
        let transfer_commit = admitted.then_some(commit_sequence);
        if admitted {
            mailbox.push_back(proposal.key);
            commit_sequence = commit_sequence.saturating_add(1);
        }
        results.push(FlowProposal {
            template: proposal.template,
            key: proposal.key,
            arrival_ordinal: proposal.arrival_ordinal,
            source: proposal.source.clone(),
            outcome: if admitted {
                FlowSendOutcome::Admitted
            } else if proposal.admission_kind == FlowAdmissionKind::TrySend {
                FlowSendOutcome::Full
            } else {
                FlowSendOutcome::Waiting
            },
            resource_custody: Arc::clone(&proposal.resource_custody),
            before_commit: FlowCustodian::ProposalHome,
            after_arbitration: if admitted {
                FlowCustodian::Mailbox
            } else {
                FlowCustodian::ProposalHome
            },
            transfer_commit,
            waited_for_capacity: waits,
            dequeued_proposal,
        });
    }
    Ok(results.into())
}

#[cfg(test)]
fn execute_independent_scenarios(
    contract: &ModelContract,
    cancellation: &Cancellation,
) -> Result<Arc<[ModelScenario]>, FlowFailure> {
    let mut alternatives = BTreeMap::<(u128, u128), Vec<Arc<[u32]>>>::new();
    for template in contract.templates.iter() {
        checkpoint(cancellation)?;
        let paths = alternatives
            .entry((template.sender, template.sender_handler))
            .or_default();
        if !template.control_path.is_empty() && !paths.contains(&template.control_path) {
            paths.push(Arc::clone(&template.control_path));
        }
    }
    for home in contract.suspension_homes.iter() {
        checkpoint(cancellation)?;
        let paths = alternatives.entry((home.actor, home.handler)).or_default();
        if !home.control_path.is_empty() && !paths.contains(&home.control_path) {
            paths.push(Arc::clone(&home.control_path));
        }
    }
    for paths in alternatives.values_mut() {
        paths.sort();
        if paths.is_empty() {
            paths.push(Arc::from([]));
        }
    }
    let mut combinations = vec![Vec::new()];
    for ((actor, handler), paths) in alternatives {
        let mut next = Vec::new();
        for path in paths {
            for prefix in &combinations {
                let mut combination = prefix.clone();
                combination.push((actor, handler, Arc::clone(&path)));
                next.push(combination);
            }
        }
        next.sort();
        if next.len() > MODEL_SCENARIO_BOUND {
            next.truncate(MODEL_SCENARIO_BOUND);
        }
        combinations = next;
    }
    if combinations.is_empty() {
        combinations.push(Vec::new());
    }
    let capacities = contract
        .mailbox_capacities
        .iter()
        .copied()
        .collect::<BTreeMap<_, _>>();
    let mut scenarios = Vec::new();
    for combination in combinations {
        checkpoint(cancellation)?;
        let selected_paths: SelectedControlPaths = combination.into();
        let mut runtime = Vec::new();
        for turn in 0..MODEL_TURN_ACTIVATION_BOUND {
            for template in contract.templates.iter() {
                if !template.control_path.is_empty()
                    && !selected_paths.iter().any(|(actor, handler, path)| {
                        *actor == template.sender
                            && *handler == template.sender_handler
                            && path.as_ref() == template.control_path.as_ref()
                    })
                {
                    continue;
                }
                runtime.push(RawProposal {
                    template: template.identity,
                    key: FlowProposalKey {
                        destination: template.destination,
                        sender: template.sender,
                        sender_turn_sequence: turn,
                        send_ordinal: template.send_ordinal,
                    },
                    arrival_ordinal: 0,
                    operation_reference: template.identity,
                    operation_current_meaning: template.current_meaning,
                    admission_kind: template.admission_kind,
                    control_path: Arc::clone(&template.control_path),
                    resource_custody: Arc::clone(&template.resource_custody),
                    source: template.source.clone(),
                });
            }
        }
        runtime.sort_by_key(|proposal| proposal.key);
        let count = u32::try_from(runtime.len()).unwrap_or(u32::MAX);
        for (index, proposal) in runtime.iter_mut().enumerate() {
            proposal.arrival_ordinal = count
                .saturating_sub(1)
                .saturating_sub(u32::try_from(index).unwrap_or(u32::MAX));
        }
        let mut mailbox_contents = BTreeMap::<u128, Vec<FlowProposalKey>>::new();
        let mut commit = 0_u64;
        let mut proposals = Vec::new();
        for proposal in runtime {
            checkpoint(cancellation)?;
            let current = mailbox_contents
                .entry(proposal.key.destination)
                .or_default();
            let capacity = capacities
                .get(&proposal.key.destination)
                .copied()
                .unwrap_or(0);
            let initially_available = u64::try_from(current.len()).unwrap_or(u64::MAX) < capacity;
            let waits = !initially_available
                && proposal.admission_kind != FlowAdmissionKind::TrySend
                && capacity > 0;
            let dequeued_proposal = if waits { Some(current.remove(0)) } else { None };
            let admitted = initially_available || waits;
            let transfer_commit = admitted.then_some(commit);
            if admitted {
                current.push(proposal.key);
                commit = commit.saturating_add(1);
            }
            proposals.push(FlowProposal {
                template: proposal.template,
                key: proposal.key,
                arrival_ordinal: proposal.arrival_ordinal,
                source: proposal.source,
                outcome: if admitted {
                    FlowSendOutcome::Admitted
                } else if proposal.admission_kind == FlowAdmissionKind::TrySend {
                    FlowSendOutcome::Full
                } else {
                    FlowSendOutcome::Waiting
                },
                resource_custody: proposal.resource_custody,
                before_commit: FlowCustodian::ProposalHome,
                after_arbitration: if admitted {
                    FlowCustodian::Mailbox
                } else {
                    FlowCustodian::ProposalHome
                },
                transfer_commit,
                waited_for_capacity: waits,
                dequeued_proposal,
            });
        }
        let proposals: Arc<[FlowProposal]> = proposals.into();
        let trace =
            execute_independent_model(contract, &proposals, selected_paths.as_ref(), cancellation)?;
        scenarios.push(ModelScenario {
            identity: model_scenario_identity(&selected_paths),
            selected_paths,
            turn_activation_bound: MODEL_TURN_ACTIVATION_BOUND,
            trace,
            proposals,
        });
    }
    scenarios.sort_by_key(|scenario| scenario.identity);
    Ok(scenarios.into())
}

fn produce_trace(
    actors: &[Actor],
    homes: &[SuspensionHome],
    templates: &[ProposalTemplate],
    proposals: &[FlowProposal],
    selected_paths: &[SelectedControlPath],
    cancellation: &Cancellation,
) -> Result<Arc<[FlowEvent]>, FlowFailure> {
    let templates_by_id = templates
        .iter()
        .map(|template| (template.identity, template))
        .collect::<BTreeMap<_, _>>();
    let proposal_homes = templates
        .iter()
        .map(|template| template.suspension_home)
        .collect::<BTreeSet<_>>();
    let mut eligible_homes = homes
        .iter()
        .filter(|home| {
            home.control_path.is_empty()
                || selected_paths.iter().any(|(actor, handler, path)| {
                    *actor == home.actor
                        && *handler == home.handler
                        && path.as_ref() == home.control_path.as_ref()
                })
        })
        .collect::<Vec<_>>();
    eligible_homes
        .sort_by_key(|home| (home.actor, home.handler, home.program_order, home.identity));
    let append = |events: &mut Vec<FlowEvent>,
                  kind,
                  actor,
                  handler,
                  turn_sequence,
                  proposal,
                  suspension_home,
                  causal_predecessor,
                  logical_commit,
                  program_order| {
        let sequence = u64::try_from(events.len()).unwrap_or(u64::MAX);
        events.push(FlowEvent {
            sequence,
            program_order,
            causal_predecessor,
            logical_commit,
            kind,
            actor,
            handler,
            turn_sequence,
            proposal,
            suspension_home,
        });
        sequence
    };
    let mut events = Vec::new();
    let mut exercised = BTreeSet::new();
    let mut turns = BTreeMap::<(u128, u128, u64), Vec<&FlowProposal>>::new();
    for proposal in proposals {
        let template = templates_by_id.get(&proposal.template).ok_or_else(|| {
            FlowFailure::Defect(Arc::from("model proposal has no static Flow template"))
        })?;
        turns
            .entry((
                proposal.key.sender,
                template.sender_handler,
                proposal.key.sender_turn_sequence,
            ))
            .or_default()
            .push(proposal);
    }
    for ((sender, sender_handler, turn_sequence), mut turn_proposals) in turns {
        checkpoint(cancellation)?;
        turn_proposals.sort_by_key(|proposal| proposal.key.send_ordinal);
        exercised.insert((sender, sender_handler));
        let first_key = turn_proposals.first().map(|proposal| proposal.key);
        let mut causal = append(
            &mut events,
            FlowEventKind::TurnStarted,
            sender,
            sender_handler,
            turn_sequence,
            first_key,
            None,
            None,
            None,
            0,
        );
        let mut admitted = Vec::new();
        for proposal in turn_proposals {
            let template = templates_by_id[&proposal.template];
            if template.admission_kind == FlowAdmissionKind::Request {
                causal = append(
                    &mut events,
                    FlowEventKind::ReplyPathReserved,
                    sender,
                    sender_handler,
                    turn_sequence,
                    Some(proposal.key),
                    Some(template.suspension_home),
                    Some(causal),
                    None,
                    u64::from(template.program_order),
                );
            }
            causal = append(
                &mut events,
                FlowEventKind::MessageProposed,
                sender,
                sender_handler,
                turn_sequence,
                Some(proposal.key),
                None,
                Some(causal),
                None,
                u64::from(template.program_order),
            );
            if template.admission_kind != FlowAdmissionKind::TrySend {
                causal = append(
                    &mut events,
                    FlowEventKind::TurnSuspended,
                    sender,
                    sender_handler,
                    turn_sequence,
                    Some(proposal.key),
                    Some(template.suspension_home),
                    Some(causal),
                    None,
                    u64::from(template.program_order),
                );
            }
            if proposal.waited_for_capacity {
                causal = append(
                    &mut events,
                    FlowEventKind::AdmissionWaiting,
                    sender,
                    sender_handler,
                    turn_sequence,
                    Some(proposal.key),
                    Some(template.suspension_home),
                    Some(causal),
                    None,
                    u64::from(template.program_order),
                );
                causal = append(
                    &mut events,
                    FlowEventKind::MailboxDequeued,
                    proposal.key.destination,
                    template.destination_handler,
                    proposal.transfer_commit.unwrap_or(0),
                    proposal.dequeued_proposal,
                    None,
                    Some(causal),
                    None,
                    u64::from(template.program_order),
                );
            }
            causal = match proposal.outcome {
                FlowSendOutcome::Full => append(
                    &mut events,
                    FlowEventKind::MessageFull,
                    sender,
                    sender_handler,
                    turn_sequence,
                    Some(proposal.key),
                    None,
                    Some(causal),
                    None,
                    u64::from(template.program_order),
                ),
                FlowSendOutcome::Admitted => {
                    admitted.push((proposal, template));
                    append(
                        &mut events,
                        FlowEventKind::MailboxTransferCommitted,
                        proposal.key.destination,
                        template.destination_handler,
                        proposal.transfer_commit.unwrap_or(0),
                        Some(proposal.key),
                        None,
                        Some(causal),
                        proposal.transfer_commit,
                        u64::from(template.program_order),
                    )
                }
                FlowSendOutcome::Waiting => append(
                    &mut events,
                    FlowEventKind::AdmissionWaiting,
                    sender,
                    sender_handler,
                    turn_sequence,
                    Some(proposal.key),
                    Some(template.suspension_home),
                    Some(causal),
                    None,
                    u64::from(template.program_order),
                ),
            };
            if template.admission_kind != FlowAdmissionKind::TrySend {
                causal = append(
                    &mut events,
                    FlowEventKind::TurnResumed,
                    sender,
                    sender_handler,
                    turn_sequence,
                    Some(proposal.key),
                    Some(template.suspension_home),
                    Some(causal),
                    proposal.transfer_commit,
                    u64::from(template.program_order),
                );
            }
        }
        for home in eligible_homes.iter().filter(|home| {
            home.actor == sender
                && home.handler == sender_handler
                && !proposal_homes.contains(&home.identity)
        }) {
            for kind in [FlowEventKind::TurnSuspended, FlowEventKind::TurnResumed] {
                causal = append(
                    &mut events,
                    kind,
                    sender,
                    sender_handler,
                    turn_sequence,
                    first_key,
                    Some(home.identity),
                    Some(causal),
                    None,
                    u64::from(home.program_order),
                );
            }
        }
        let sender_completed = append(
            &mut events,
            FlowEventKind::TurnCompleted,
            sender,
            sender_handler,
            turn_sequence,
            first_key,
            None,
            Some(causal),
            None,
            u64::MAX,
        );
        for (proposal, template) in admitted {
            exercised.insert((proposal.key.destination, template.destination_handler));
            let receiver_turn = proposal.transfer_commit.unwrap_or(0);
            let mut receiver_causal = append(
                &mut events,
                FlowEventKind::TurnStarted,
                proposal.key.destination,
                template.destination_handler,
                receiver_turn,
                Some(proposal.key),
                None,
                Some(sender_completed),
                proposal.transfer_commit,
                0,
            );
            for home in eligible_homes.iter().filter(|home| {
                home.actor == proposal.key.destination
                    && home.handler == template.destination_handler
                    && !proposal_homes.contains(&home.identity)
            }) {
                for kind in [FlowEventKind::TurnSuspended, FlowEventKind::TurnResumed] {
                    receiver_causal = append(
                        &mut events,
                        kind,
                        proposal.key.destination,
                        template.destination_handler,
                        receiver_turn,
                        Some(proposal.key),
                        Some(home.identity),
                        Some(receiver_causal),
                        proposal.transfer_commit,
                        u64::from(home.program_order),
                    );
                }
            }
            append(
                &mut events,
                FlowEventKind::TurnCompleted,
                proposal.key.destination,
                template.destination_handler,
                receiver_turn,
                Some(proposal.key),
                None,
                Some(receiver_causal),
                proposal.transfer_commit,
                u64::MAX,
            );
        }
    }
    for actor in actors {
        for handler in actor.handlers.iter().copied() {
            checkpoint(cancellation)?;
            if exercised.contains(&(actor.identity, handler)) {
                continue;
            }
            let handler_homes = eligible_homes
                .iter()
                .copied()
                .filter(|home| {
                    home.actor == actor.identity
                        && home.handler == handler
                        && !proposal_homes.contains(&home.identity)
                })
                .collect::<Vec<_>>();
            if handler_homes.is_empty() {
                continue;
            }
            let mut causal = append(
                &mut events,
                FlowEventKind::TurnStarted,
                actor.identity,
                handler,
                0,
                None,
                None,
                None,
                None,
                0,
            );
            for home in handler_homes {
                for kind in [FlowEventKind::TurnSuspended, FlowEventKind::TurnResumed] {
                    causal = append(
                        &mut events,
                        kind,
                        actor.identity,
                        handler,
                        0,
                        None,
                        Some(home.identity),
                        Some(causal),
                        None,
                        u64::from(home.program_order),
                    );
                }
            }
            append(
                &mut events,
                FlowEventKind::TurnCompleted,
                actor.identity,
                handler,
                0,
                None,
                None,
                Some(causal),
                None,
                u64::MAX,
            );
        }
    }
    Ok(events.into())
}

#[cfg(test)]
fn execute_independent_model(
    contract: &ModelContract,
    proposals: &[FlowProposal],
    selected_paths: &[SelectedControlPath],
    cancellation: &Cancellation,
) -> Result<Arc<[FlowEvent]>, FlowFailure> {
    let templates = contract
        .templates
        .iter()
        .map(|template| (template.identity, template))
        .collect::<BTreeMap<_, _>>();
    let proposal_homes = contract
        .templates
        .iter()
        .map(|template| template.suspension_home)
        .collect::<BTreeSet<_>>();
    let mut homes = contract
        .suspension_homes
        .iter()
        .filter(|home| {
            home.control_path.is_empty()
                || selected_paths.iter().any(|selection| {
                    selection.0 == home.actor
                        && selection.1 == home.handler
                        && selection.2.as_ref() == home.control_path.as_ref()
                })
        })
        .collect::<Vec<_>>();
    homes.sort_by(|left, right| {
        (left.actor, left.handler, left.program_order, left.identity).cmp(&(
            right.actor,
            right.handler,
            right.program_order,
            right.identity,
        ))
    });
    let append = |observations: &mut Vec<FlowEvent>,
                  kind,
                  actor,
                  handler,
                  turn_sequence,
                  proposal,
                  home,
                  causal_predecessor,
                  logical_commit,
                  program_order| {
        let sequence = u64::try_from(observations.len()).unwrap_or(u64::MAX);
        observations.push(FlowEvent {
            sequence,
            program_order,
            causal_predecessor,
            logical_commit,
            kind,
            actor,
            handler,
            turn_sequence,
            proposal,
            suspension_home: home,
        });
        sequence
    };
    let mut observations = Vec::new();
    let mut exercised = BTreeSet::new();
    let mut grouped = BTreeMap::<(u128, u128, u64), Vec<&FlowProposal>>::new();
    for proposal in proposals {
        let template = templates.get(&proposal.template).ok_or_else(|| {
            FlowFailure::Defect(Arc::from("independent model proposal has no template"))
        })?;
        grouped
            .entry((
                proposal.key.sender,
                template.sender_handler,
                proposal.key.sender_turn_sequence,
            ))
            .or_default()
            .push(proposal);
    }
    for ((actor, handler, turn), mut messages) in grouped {
        checkpoint(cancellation)?;
        messages.sort_by_key(|message| message.key.send_ordinal);
        exercised.insert((actor, handler));
        let first = messages.first().map(|message| message.key);
        let mut prior = append(
            &mut observations,
            FlowEventKind::TurnStarted,
            actor,
            handler,
            turn,
            first,
            None,
            None,
            None,
            0,
        );
        let mut deliveries = Vec::new();
        for message in messages {
            let template = templates[&message.template];
            if template.admission_kind == FlowAdmissionKind::Request {
                prior = append(
                    &mut observations,
                    FlowEventKind::ReplyPathReserved,
                    actor,
                    handler,
                    turn,
                    Some(message.key),
                    Some(template.suspension_home),
                    Some(prior),
                    None,
                    u64::from(template.program_order),
                );
            }
            prior = append(
                &mut observations,
                FlowEventKind::MessageProposed,
                actor,
                handler,
                turn,
                Some(message.key),
                None,
                Some(prior),
                None,
                u64::from(template.program_order),
            );
            if template.admission_kind != FlowAdmissionKind::TrySend {
                prior = append(
                    &mut observations,
                    FlowEventKind::TurnSuspended,
                    actor,
                    handler,
                    turn,
                    Some(message.key),
                    Some(template.suspension_home),
                    Some(prior),
                    None,
                    u64::from(template.program_order),
                );
            }
            if message.waited_for_capacity {
                prior = append(
                    &mut observations,
                    FlowEventKind::AdmissionWaiting,
                    actor,
                    handler,
                    turn,
                    Some(message.key),
                    Some(template.suspension_home),
                    Some(prior),
                    None,
                    u64::from(template.program_order),
                );
                prior = append(
                    &mut observations,
                    FlowEventKind::MailboxDequeued,
                    message.key.destination,
                    template.destination_handler,
                    message.transfer_commit.unwrap_or(0),
                    message.dequeued_proposal,
                    None,
                    Some(prior),
                    None,
                    u64::from(template.program_order),
                );
            }
            let kind = match message.outcome {
                FlowSendOutcome::Admitted => {
                    deliveries.push((message, template));
                    FlowEventKind::MailboxTransferCommitted
                }
                FlowSendOutcome::Full => FlowEventKind::MessageFull,
                FlowSendOutcome::Waiting => FlowEventKind::AdmissionWaiting,
            };
            let destination_event = message.outcome == FlowSendOutcome::Admitted;
            prior = append(
                &mut observations,
                kind,
                if destination_event {
                    message.key.destination
                } else {
                    actor
                },
                if destination_event {
                    template.destination_handler
                } else {
                    handler
                },
                if destination_event {
                    message.transfer_commit.unwrap_or(0)
                } else {
                    turn
                },
                Some(message.key),
                (message.outcome == FlowSendOutcome::Waiting).then_some(template.suspension_home),
                Some(prior),
                message.transfer_commit,
                u64::from(template.program_order),
            );
            if template.admission_kind != FlowAdmissionKind::TrySend {
                prior = append(
                    &mut observations,
                    FlowEventKind::TurnResumed,
                    actor,
                    handler,
                    turn,
                    Some(message.key),
                    Some(template.suspension_home),
                    Some(prior),
                    message.transfer_commit,
                    u64::from(template.program_order),
                );
            }
        }
        for home in homes.iter().filter(|home| {
            home.actor == actor
                && home.handler == handler
                && !proposal_homes.contains(&home.identity)
        }) {
            prior = append(
                &mut observations,
                FlowEventKind::TurnSuspended,
                actor,
                handler,
                turn,
                first,
                Some(home.identity),
                Some(prior),
                None,
                u64::from(home.program_order),
            );
            prior = append(
                &mut observations,
                FlowEventKind::TurnResumed,
                actor,
                handler,
                turn,
                first,
                Some(home.identity),
                Some(prior),
                None,
                u64::from(home.program_order),
            );
        }
        let completed = append(
            &mut observations,
            FlowEventKind::TurnCompleted,
            actor,
            handler,
            turn,
            first,
            None,
            Some(prior),
            None,
            u64::MAX,
        );
        for (message, template) in deliveries {
            exercised.insert((message.key.destination, template.destination_handler));
            let receiver_turn = message.transfer_commit.unwrap_or(0);
            let mut receiver_prior = append(
                &mut observations,
                FlowEventKind::TurnStarted,
                message.key.destination,
                template.destination_handler,
                receiver_turn,
                Some(message.key),
                None,
                Some(completed),
                message.transfer_commit,
                0,
            );
            for home in homes.iter().filter(|home| {
                home.actor == message.key.destination
                    && home.handler == template.destination_handler
                    && !proposal_homes.contains(&home.identity)
            }) {
                receiver_prior = append(
                    &mut observations,
                    FlowEventKind::TurnSuspended,
                    message.key.destination,
                    template.destination_handler,
                    receiver_turn,
                    Some(message.key),
                    Some(home.identity),
                    Some(receiver_prior),
                    message.transfer_commit,
                    u64::from(home.program_order),
                );
                receiver_prior = append(
                    &mut observations,
                    FlowEventKind::TurnResumed,
                    message.key.destination,
                    template.destination_handler,
                    receiver_turn,
                    Some(message.key),
                    Some(home.identity),
                    Some(receiver_prior),
                    message.transfer_commit,
                    u64::from(home.program_order),
                );
            }
            append(
                &mut observations,
                FlowEventKind::TurnCompleted,
                message.key.destination,
                template.destination_handler,
                receiver_turn,
                Some(message.key),
                None,
                Some(receiver_prior),
                message.transfer_commit,
                u64::MAX,
            );
        }
    }
    for (actor, handlers) in contract.actors.iter() {
        for handler in handlers.iter().copied() {
            checkpoint(cancellation)?;
            if exercised.contains(&(*actor, handler)) {
                continue;
            }
            let eligible = homes
                .iter()
                .copied()
                .filter(|home| {
                    home.actor == *actor
                        && home.handler == handler
                        && !proposal_homes.contains(&home.identity)
                })
                .collect::<Vec<_>>();
            if eligible.is_empty() {
                continue;
            }
            let mut prior = append(
                &mut observations,
                FlowEventKind::TurnStarted,
                *actor,
                handler,
                0,
                None,
                None,
                None,
                None,
                0,
            );
            for home in eligible {
                prior = append(
                    &mut observations,
                    FlowEventKind::TurnSuspended,
                    *actor,
                    handler,
                    0,
                    None,
                    Some(home.identity),
                    Some(prior),
                    None,
                    u64::from(home.program_order),
                );
                prior = append(
                    &mut observations,
                    FlowEventKind::TurnResumed,
                    *actor,
                    handler,
                    0,
                    None,
                    Some(home.identity),
                    Some(prior),
                    None,
                    u64::from(home.program_order),
                );
            }
            append(
                &mut observations,
                FlowEventKind::TurnCompleted,
                *actor,
                handler,
                0,
                None,
                None,
                Some(prior),
                None,
                u64::MAX,
            );
        }
    }
    Ok(observations.into())
}

fn verify(
    candidate: &VerifiedFlowProgram,
    planning: FlowPlanningInput<'_>,
    core: FlowCoreView<'_>,
    cancellation: &Cancellation,
) -> Result<(), FlowFailure> {
    checkpoint(cancellation)?;
    if candidate.context != planning.context_identity()
        || candidate.context != core.context_identity()
        || candidate.planning_fingerprint != planning.fingerprint()
        || candidate.core_fingerprint != core.fingerprint()
    {
        return defect("Flow artifact input receipt is stale or cross-context");
    }
    let mut expected_actors = planning
        .semantic_program()
        .actors()
        .map(|actor| Actor {
            identity: actor.identity(),
            actor_type_identity: actor.actor_type_identity(),
            construction_identity: actor.construction_identity(),
            construction_current_meaning: actor.construction_current_meaning(),
            source: actor.source().clone(),
            mailbox_capacity: 1,
            max_active_turns: 1,
            permanent_core_requirement: requirement_identity(
                actor.identity(),
                None,
                None,
                FlowRequirementKind::PermanentCorePlacement,
            ),
            handlers: actor.handlers().collect::<Vec<_>>().into(),
            wired_actor_constructions: actor.wired_actor_constructions().into(),
        })
        .collect::<Vec<_>>();
    expected_actors.sort_by_key(|actor| actor.identity);
    if expected_actors.as_slice() != candidate.actors.as_ref() {
        return defect("Flow Actor family disagrees with completed semantic authority");
    }
    let suspension_sites = core.suspension_sites();
    let handler_current_meanings = core.handler_flow_identities();
    let mut expected_requirements = Vec::new();
    for actor in &expected_actors {
        for (kind, bound) in [
            (FlowRequirementKind::ActorIdentity, 1),
            (FlowRequirementKind::PermanentCorePlacement, 1),
            (FlowRequirementKind::MailboxCapacity, 1),
            (FlowRequirementKind::TurnLease, 1),
            (FlowRequirementKind::LogicalCommitOrder, 1),
            (FlowRequirementKind::ProposalTransport, 1),
        ] {
            expected_requirements.push(requirement_with_authority(
                actor.identity,
                None,
                None,
                kind,
                bound,
                actor.construction_identity,
                0,
                actor.construction_identity,
            ));
        }
        for site in suspension_sites
            .iter()
            .filter(|site| actor.handlers.contains(&site.handler))
        {
            expected_requirements.push(requirement_with_authority(
                actor.identity,
                Some(site.handler),
                Some(site.reference_identity),
                FlowRequirementKind::SuspensionHome,
                1,
                actor.construction_identity,
                handler_current_meanings
                    .get(&site.handler)
                    .copied()
                    .unwrap_or(u128::MAX),
                site.reference_current_meaning,
            ));
        }
    }
    let mut requirement_ids = BTreeSet::new();
    for actor in candidate.actors.iter() {
        checkpoint(cancellation)?;
        if actor.mailbox_capacity == 0 || actor.max_active_turns != 1 {
            return defect("Flow Actor capacity or Turn lease bound is invalid");
        }
        if actor.permanent_core_requirement
            != requirement_identity(
                actor.identity,
                None,
                None,
                FlowRequirementKind::PermanentCorePlacement,
            )
        {
            return defect("Flow Actor permanent placement requirement is invalid");
        }
    }
    for supplied in candidate.requirements.iter() {
        checkpoint(cancellation)?;
        if !requirement_ids.insert(supplied.identity)
            || supplied.identity
                != requirement_identity(
                    supplied.actor,
                    supplied.handler,
                    supplied.site,
                    supplied.kind,
                )
        {
            return defect("Flow Planning Requirement identity or meaning is invalid");
        }
    }
    let mut expected_homes = expected_actors
        .iter()
        .flat_map(|actor| {
            suspension_sites
                .iter()
                .filter(|site| actor.handlers.contains(&site.handler))
                .map(|site| SuspensionHome {
                    identity: suspension_home_identity(
                        actor.identity,
                        site.handler,
                        site.reference_identity,
                    ),
                    actor: actor.identity,
                    handler: site.handler,
                    suspension_reference: site.reference_identity,
                    suspension_current_meaning: site.reference_current_meaning,
                    control_path: Arc::clone(&site.control_path),
                    program_order: site.program_order,
                    source: site.source.clone(),
                    slot_count: 1,
                    retains_turn_lease: true,
                    requirement: requirement_identity(
                        actor.identity,
                        Some(site.handler),
                        Some(site.reference_identity),
                        FlowRequirementKind::SuspensionHome,
                    ),
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    expected_homes.sort_by_key(|home| home.identity);
    if expected_homes.as_slice() != candidate.suspension_homes.as_ref() {
        return defect("Flow suspension has no exact static Suspension Home");
    }
    let core_groups = core.group_sites();
    let expected_templates = verifier_reconstruct_templates(
        &expected_actors,
        &expected_homes,
        &core_groups,
        core.message_proposals(),
        &handler_current_meanings,
        &candidate.proposal_templates,
        cancellation,
    )?;
    if reply_wait_cycle(&expected_templates, cancellation)?.is_some() {
        return defect("verified Flow retained a statically knowable Reply wait cycle");
    }
    let (
        expected_replies,
        expected_groups,
        expected_policy_laws,
        expected_deadline_laws,
        structured_requirements,
    ) = verifier_reconstruct_structured(
        &expected_actors,
        &expected_templates,
        &core_groups,
        &core.reply_fulfillment_sites(),
        &core.cleanup_sites(),
        &handler_current_meanings,
        candidate,
        cancellation,
    )?;
    expected_requirements.extend(structured_requirements);
    expected_requirements.sort_by_key(|requirement| requirement.identity);
    if expected_requirements.as_slice() != candidate.requirements.as_ref() {
        return defect("Flow Planning Requirement roster is missing, extra, or repointed");
    }
    if expected_templates != candidate.proposal_templates {
        return defect("Flow model contract disagrees with Core message proposals");
    }
    if candidate.reply_obligations != expected_replies
        || candidate.groups != expected_groups
        || candidate.group_policy_laws != expected_policy_laws
        || candidate.deadline_laws != expected_deadline_laws
    {
        return defect("Flow structured authority roster or direct relationship is invalid");
    }
    for home in candidate.suspension_homes.iter() {
        if home.identity
            != suspension_home_identity(home.actor, home.handler, home.suspension_reference)
            || home.slot_count != 1
            || !home.retains_turn_lease
            || !requirement_ids.contains(&home.requirement)
            || home.requirement
                != requirement_identity(
                    home.actor,
                    Some(home.handler),
                    Some(home.suspension_reference),
                    FlowRequirementKind::SuspensionHome,
                )
        {
            return defect("Flow Suspension Home is invalid");
        }
    }
    let expected_contract = ModelContract {
        actors: expected_actors
            .iter()
            .map(|actor| (actor.identity, Arc::clone(&actor.handlers)))
            .collect::<Vec<_>>()
            .into(),
        suspension_homes: expected_homes.into(),
        templates: expected_templates,
        mailbox_capacities: expected_actors
            .iter()
            .map(|actor| (actor.identity, actor.mailbox_capacity))
            .collect::<Vec<_>>()
            .into(),
        replies: Arc::clone(&expected_replies),
        groups: Arc::clone(&expected_groups),
        policy_laws: Arc::clone(&expected_policy_laws),
        deadline_laws: Arc::clone(&expected_deadline_laws),
        terminal_panics: core
            .terminal_panic_sites()
            .into_iter()
            .map(|site| TerminalPanicFact {
                handler: site.handler,
                identity: site.identity,
                current_meaning: site.current_meaning,
                program_order: site.program_order,
                source: site.source,
            })
            .collect::<Vec<_>>()
            .into(),
    };
    if candidate.model_contract != expected_contract {
        return defect("Flow model contract roster or direct relationship is invalid");
    }
    let expected_fingerprint = fingerprint(
        FlowFingerprintInput {
            context: candidate.context,
            planning_fingerprint: candidate.planning_fingerprint,
            core_fingerprint: candidate.core_fingerprint,
            actors: &candidate.actors,
            requirements: &candidate.requirements,
            homes: &candidate.suspension_homes,
            templates: &candidate.proposal_templates,
            contract: &candidate.model_contract,
            replies: &candidate.reply_obligations,
            groups: &candidate.groups,
            policy_laws: &candidate.group_policy_laws,
            deadline_laws: &candidate.deadline_laws,
        },
        cancellation,
    )?;
    if expected_fingerprint != candidate.fingerprint {
        return defect("Flow fingerprint is invalid");
    }
    Ok(())
}

fn verifier_reconstruct_templates(
    actors: &[Actor],
    homes: &[SuspensionHome],
    group_sites: &[crate::core::FlowCoreGroupSite],
    messages: Vec<crate::core::FlowCoreMessageProposal>,
    handler_current_meanings: &BTreeMap<u128, u128>,
    supplied: &[ProposalTemplate],
    cancellation: &Cancellation,
) -> Result<Arc<[ProposalTemplate]>, FlowFailure> {
    let mut owners = BTreeMap::<u128, Vec<u128>>::new();
    for actor in actors {
        checkpoint(cancellation)?;
        for handler in actor.handlers.iter().copied() {
            owners.entry(handler).or_default().push(actor.identity);
        }
    }
    let mut reconstructed = Vec::new();
    for message in messages {
        checkpoint(cancellation)?;
        let senders = owners
            .get(&message.sender_handler)
            .ok_or_else(|| FlowFailure::Defect(Arc::from("verifier found no Actor sender")))?;
        let destinations = owners
            .get(&message.destination_handler)
            .ok_or_else(|| FlowFailure::Defect(Arc::from("verifier found no Actor destination")))?;
        for sender in senders.iter().copied() {
            checkpoint(cancellation)?;
            let actor = actors
                .iter()
                .find(|actor| actor.identity == sender)
                .ok_or_else(|| FlowFailure::Defect(Arc::from("verifier Actor owner vanished")))?;
            let wired = destinations
                .iter()
                .copied()
                .filter(|destination| {
                    destinations.len() == 1
                        || (*destination == sender
                            && message.admission_kind
                                == crate::core::FlowCoreAdmissionKind::Request)
                        || actor.wired_actor_constructions.contains(destination)
                })
                .collect::<Vec<_>>();
            let [destination] = wired.as_slice() else {
                return defect("verifier found no unique build-wired message destination");
            };
            let home = homes
                .iter()
                .find(|home| {
                    home.actor == sender
                        && home.handler == message.sender_handler
                        && home.suspension_reference == message.operation_reference
                })
                .ok_or_else(|| {
                    FlowFailure::Defect(Arc::from("verifier found no exact Proposal Home"))
                })?;
            let mut template_id = Xxh3::new();
            template_id.update(b"wrela.flow.proposal-template\0\x01");
            template_id.update(&message.operation_reference.to_be_bytes());
            template_id.update(&sender.to_be_bytes());
            template_id.update(&destination.to_be_bytes());
            let identity = template_id.digest128();
            let custody = message
                .custody
                .iter()
                .map(|resource| FlowResourceCustody {
                    core_reference_identity: resource.reference_identity,
                    core_reference_current_meaning: resource.reference_current_meaning,
                    type_identity: resource.type_identity,
                    place: Arc::clone(&resource.place),
                    source_home: resource.source_home,
                    proposal_home: home.identity,
                })
                .collect::<Vec<_>>();
            let group = group_sites
                .iter()
                .filter(|group| {
                    group.handler == message.sender_handler
                        && group.open_program_order < message.program_order
                        && message.program_order < group.terminal_program_order
                })
                .max_by_key(|group| group.open_program_order);
            let owning_group =
                group.map(|group| graph_identity(b"group", &[group.reference_identity]));
            let deadline_class = group.and_then(|group| {
                group.deadline.map(|deadline| match deadline.class {
                    crate::core::FlowCoreDeadlineClass::Logical => FlowDeadlineClass::Logical,
                    crate::core::FlowCoreDeadlineClass::Realtime => FlowDeadlineClass::Realtime,
                })
            });
            let admission_kind = match message.admission_kind {
                crate::core::FlowCoreAdmissionKind::TrySend => FlowAdmissionKind::TrySend,
                crate::core::FlowCoreAdmissionKind::WaitingSend => FlowAdmissionKind::WaitingSend,
                crate::core::FlowCoreAdmissionKind::Request => FlowAdmissionKind::Request,
            };
            let response_type_identity = if admission_kind == FlowAdmissionKind::Request {
                message.response_type_identity
            } else {
                0
            };
            let mut meaning = Xxh3::new();
            meaning.update(b"wrela.flow.proposal-template-meaning\0\x01");
            meaning.update(&identity.to_be_bytes());
            meaning.update(&message.operation_current_meaning.to_be_bytes());
            meaning.update(&sender.to_be_bytes());
            meaning.update(
                &handler_current_meanings
                    .get(&message.sender_handler)
                    .copied()
                    .unwrap_or(u128::MAX)
                    .to_be_bytes(),
            );
            meaning.update(&destination.to_be_bytes());
            meaning.update(
                &handler_current_meanings
                    .get(&message.destination_handler)
                    .copied()
                    .unwrap_or(u128::MAX)
                    .to_be_bytes(),
            );
            meaning.update(&response_type_identity.to_be_bytes());
            meaning.update(&[match admission_kind {
                FlowAdmissionKind::TrySend => 1,
                FlowAdmissionKind::WaitingSend => 2,
                FlowAdmissionKind::Request => 3,
            }]);
            for resource in &custody {
                checkpoint(cancellation)?;
                meaning.update(&resource.core_reference_identity.to_be_bytes());
                meaning.update(&resource.core_reference_current_meaning.to_be_bytes());
                meaning.update(&resource.source_home.to_be_bytes());
                meaning.update(&resource.proposal_home.to_be_bytes());
                for part in resource.place.iter() {
                    checkpoint(cancellation)?;
                    meaning.update(&part.to_be_bytes());
                }
            }
            for part in message.control_path.iter() {
                checkpoint(cancellation)?;
                meaning.update(&part.to_be_bytes());
            }
            meaning.update(&owning_group.unwrap_or(0).to_be_bytes());
            meaning.update(
                &group
                    .map_or(0, |group| group.place_current_meaning)
                    .to_be_bytes(),
            );
            meaning.update(&[deadline_class.map_or(0, FlowDeadlineClass::tag)]);
            reconstructed.push(ProposalTemplate {
                identity,
                current_meaning: meaning.digest128(),
                sender,
                sender_handler: message.sender_handler,
                destination: *destination,
                destination_handler: message.destination_handler,
                admission_kind,
                owning_group,
                deadline_class,
                response_type_identity,
                send_ordinal: message.send_ordinal,
                program_order: message.program_order,
                suspension_home: home.identity,
                control_path: Arc::clone(&message.control_path),
                resource_custody: custody.into(),
                source: message.source.clone(),
            });
        }
    }
    reconstructed
        .sort_by_key(|template| (template.sender, template.destination, template.send_ordinal));
    if reconstructed.as_slice() != supplied {
        return defect("Flow verifier independently reconstructed different proposal templates");
    }
    Ok(reconstructed.into())
}

#[allow(clippy::too_many_arguments)]
fn verifier_reconstruct_structured(
    actors: &[Actor],
    templates: &[ProposalTemplate],
    group_sites: &[crate::core::FlowCoreGroupSite],
    fulfillment_sites: &[crate::core::FlowCoreReplyFulfillmentSite],
    cleanup_sites: &[crate::core::FlowCoreCleanupSite],
    handler_current_meanings: &BTreeMap<u128, u128>,
    candidate: &VerifiedFlowProgram,
    cancellation: &Cancellation,
) -> Result<StructuredAuthority, FlowFailure> {
    let requests = templates
        .iter()
        .filter(|template| template.admission_kind == FlowAdmissionKind::Request)
        .collect::<Vec<_>>();
    if requests.len() != candidate.reply_obligations.len() {
        return defect("Flow verifier found a correlated Reply omission or addition");
    }
    for template in &requests {
        checkpoint(cancellation)?;
        let reply = candidate
            .reply_obligations
            .iter()
            .find(|reply| reply.request_template == template.identity)
            .ok_or_else(|| FlowFailure::Defect(Arc::from("request has no Reply obligation")))?;
        let sites = fulfillment_sites
            .iter()
            .filter(|site| {
                site.handler == template.destination_handler
                    && (site.cancelled
                        || site.response_type_identity == template.response_type_identity)
            })
            .collect::<Vec<_>>();
        if sites.len() != 1 {
            return defect("Flow verifier found missing or duplicate one-shot fulfillment");
        }
        let site = sites[0];
        let response_home = graph_identity(b"reply-response-home", &[template.identity]);
        let expected_response = site
            .response_place
            .as_ref()
            .map(|place| FlowResourceCustody {
                core_reference_identity: site.reference_identity,
                core_reference_current_meaning: site.reference_current_meaning,
                type_identity: site.response_type_identity,
                place: Arc::clone(place),
                source_home: graph_identity(
                    b"reply-response-source-home",
                    &[site.reference_identity],
                ),
                proposal_home: response_home,
            })
            .into_iter()
            .collect::<Vec<_>>();
        if reply.identity != graph_identity(b"reply", &[template.identity])
            || reply.endpoint != graph_identity(b"reply-endpoint", &[template.identity])
            || reply.return_path != graph_identity(b"reply-return-path", &[template.identity])
            || reply.response_home != response_home
            || reply.response_type_identity != template.response_type_identity
            || reply.fulfillment_references.as_ref()
                != [(site.reference_identity, site.reference_current_meaning)]
            || reply.fulfillment_endpoint_places.as_ref() != [Arc::clone(&site.endpoint_place)]
            || reply.response_custody.as_ref() != expected_response
            || reply.explicit_cancel != site.cancelled
            || reply.capacity != 1
            || !reply.fulfillment_capacity_infallible
        {
            return defect("Flow verifier independently reconstructed different Reply authority");
        }
    }
    if group_sites.len() != candidate.groups.len() {
        return defect("Flow verifier found a correlated Group omission or addition");
    }
    for site in group_sites {
        checkpoint(cancellation)?;
        let identity = graph_identity(b"group", &[site.reference_identity]);
        let group = candidate
            .groups
            .iter()
            .find(|group| group.identity == identity)
            .ok_or_else(|| FlowFailure::Defect(Arc::from("Core Group has no Flow obligation")))?;
        let actor = actors
            .iter()
            .find(|actor| actor.handlers.contains(&site.handler))
            .ok_or_else(|| FlowFailure::Defect(Arc::from("Core Group has no Actor")))?;
        let policy = match site.policy {
            crate::core::FlowCoreGroupPolicy::All => FlowGroupPolicy::All,
            crate::core::FlowCoreGroupPolicy::Collect => FlowGroupPolicy::Collect,
            crate::core::FlowCoreGroupPolicy::Race => FlowGroupPolicy::Race,
            crate::core::FlowCoreGroupPolicy::Supervise => FlowGroupPolicy::Supervise,
        };
        let deadline_class = site.deadline.map(|deadline| match deadline.class {
            crate::core::FlowCoreDeadlineClass::Logical => FlowDeadlineClass::Logical,
            crate::core::FlowCoreDeadlineClass::Realtime => FlowDeadlineClass::Realtime,
        });
        let return_home = graph_identity(b"group-return-home", &[site.reference_identity]);
        let resources = site
            .children
            .iter()
            .filter(|child| child.moved)
            .map(|child| FlowResourceCustody {
                core_reference_identity: child.identity,
                core_reference_current_meaning: child.current_meaning,
                type_identity: child.type_identity,
                place: Arc::clone(&child.place),
                source_home: graph_identity(b"group-child-source-home", &[child.identity]),
                proposal_home: return_home,
            })
            .collect::<Vec<_>>();
        let actions = cleanup_sites
            .iter()
            .filter(|action| {
                action.handler == site.handler
                    && site.open_program_order < action.program_order
                    && action.program_order < site.terminal_program_order
            })
            .map(|action| CleanupAction {
                identity: action.identity,
                current_meaning: action.current_meaning,
                handler: action.handler,
                program_order: action.program_order,
                source: action.source.clone(),
            })
            .collect::<Vec<_>>();
        let expected_cancellation_authority =
            graph_identity(b"group-cancellation-authority", &[site.reference_identity]);
        let mut expected_cancellation_checkpoints = site
            .children
            .iter()
            .map(|child| child.identity)
            .collect::<Vec<_>>();
        expected_cancellation_checkpoints.push(site.reference_identity);
        expected_cancellation_checkpoints.push(graph_identity(
            b"group-terminal-checkpoint",
            &[
                site.reference_identity,
                u128::from(site.terminal_program_order),
            ],
        ));
        expected_cancellation_checkpoints.sort_unstable();
        expected_cancellation_checkpoints.dedup();
        let expected_maximum_uninterrupted_work_units = u64::from(
            site.terminal_program_order
                .saturating_sub(site.open_program_order),
        )
        .max(1);
        if group.actor != actor.identity
            || group.handler != site.handler
            || group.receiver_place != site.place
            || group.receiver_current_meaning != site.place_current_meaning
            || group.child_activation_bound != site.child_activation_bound
            || group.child_activations.as_ref()
                != site
                    .children
                    .iter()
                    .map(|child| child.identity)
                    .collect::<Vec<_>>()
            || group.cancellation_authority != expected_cancellation_authority
            || group.cancellation_checkpoints.as_ref() != expected_cancellation_checkpoints
            || group.maximum_uninterrupted_work_units != expected_maximum_uninterrupted_work_units
            || group.policy != policy
            || group.deadline_class != deadline_class
            || group.deadline_authority != site.deadline.map(|deadline| deadline.authority)
            || group.deadline_authority_current_meaning
                != site
                    .deadline
                    .map(|deadline| deadline.authority_current_meaning)
            || group.deadline_capture_authority
                != site
                    .deadline
                    .and_then(|deadline| deadline.capture_authority)
            || group.deadline_capture_current_meaning
                != site
                    .deadline
                    .and_then(|deadline| deadline.capture_current_meaning)
            || group.deadline_slack != site.deadline.map(|deadline| deadline.slack)
            || group.return_home != return_home
            || group.moved_resources.as_ref() != resources
            || group.cleanup_actions.as_ref() != actions
            || group.cancelled != site.cancelled
        {
            return defect("Flow verifier independently reconstructed different Group authority");
        }
    }
    let mut expected_policies = candidate
        .groups
        .iter()
        .map(|group| GroupPolicyLaw {
            policy: group.policy,
            deterministic_result_order: true,
            cancels_siblings: matches!(group.policy, FlowGroupPolicy::All | FlowGroupPolicy::Race),
            collects_failures: group.policy == FlowGroupPolicy::Collect,
            supervises_failures: group.policy == FlowGroupPolicy::Supervise,
            winner_is_logical: group.policy == FlowGroupPolicy::Race,
            host_completion_ignored: true,
        })
        .collect::<Vec<_>>();
    expected_policies.sort_by_key(|law| law.policy);
    expected_policies.dedup_by_key(|law| law.policy);
    if candidate.group_policy_laws.as_ref() != expected_policies {
        return defect("Flow verifier independently reconstructed different policy laws");
    }
    let mut expected_deadlines = candidate
        .groups
        .iter()
        .filter_map(|group| {
            Some(DeadlineLaw {
                class: group.deadline_class?,
                authority: group.deadline_authority?,
                deterministic: group.deadline_class == Some(FlowDeadlineClass::Logical),
                replay_capture_required: group.deadline_class == Some(FlowDeadlineClass::Realtime),
            })
        })
        .collect::<Vec<_>>();
    expected_deadlines.sort_by_key(|law| (law.class, law.authority));
    expected_deadlines.dedup();
    if candidate.deadline_laws.as_ref() != expected_deadlines {
        return defect("Flow verifier independently reconstructed different deadline laws");
    }
    let mut structured_requirements = Vec::new();
    for reply in candidate.reply_obligations.iter() {
        checkpoint(cancellation)?;
        let template = templates
            .iter()
            .find(|template| template.identity == reply.request_template)
            .ok_or_else(|| FlowFailure::Defect(Arc::from("Reply has no request template")))?;
        let actor_current_meaning = actors
            .iter()
            .find(|actor| actor.identity == template.sender)
            .map_or(u128::MAX, |actor| actor.construction_identity);
        let handler_current_meaning = handler_current_meanings
            .get(&template.sender_handler)
            .copied()
            .unwrap_or(u128::MAX);
        let subject_current_meaning = reply_requirement_current_meaning(reply, template);
        for (kind, site) in [
            (FlowRequirementKind::ReplyEndpoint, reply.endpoint),
            (FlowRequirementKind::ReplyReturnPath, reply.return_path),
            (FlowRequirementKind::ReplyResponseHome, reply.response_home),
            (
                FlowRequirementKind::ReplyAcyclicWait,
                reply.request_template,
            ),
        ] {
            checkpoint(cancellation)?;
            structured_requirements.push(requirement_with_authority(
                template.sender,
                Some(template.sender_handler),
                Some(site),
                kind,
                1,
                actor_current_meaning,
                handler_current_meaning,
                subject_current_meaning,
            ));
        }
    }
    for group in candidate.groups.iter() {
        checkpoint(cancellation)?;
        let actor_current_meaning = actors
            .iter()
            .find(|actor| actor.identity == group.actor)
            .map_or(u128::MAX, |actor| actor.construction_identity);
        let handler_current_meaning = handler_current_meanings
            .get(&group.handler)
            .copied()
            .unwrap_or(u128::MAX);
        let subject_current_meaning = group_requirement_current_meaning(group);
        for (kind, site, bound) in [
            (
                FlowRequirementKind::GroupChildActivationBound,
                group.identity,
                group.child_activation_bound,
            ),
            (
                FlowRequirementKind::GroupCancellationAuthority,
                group.cancellation_authority,
                1,
            ),
            (FlowRequirementKind::GroupOutcomePolicy, group.identity, 1),
            (
                FlowRequirementKind::GroupResourceReturnHome,
                group.return_home,
                u64::try_from(group.moved_resources.len()).unwrap_or(u64::MAX),
            ),
            (
                FlowRequirementKind::GroupCleanupOrder,
                group.identity,
                u64::try_from(group.cleanup_actions.len()).unwrap_or(u64::MAX),
            ),
            (
                FlowRequirementKind::CancellationObservationWorkBound,
                group.identity,
                group.maximum_uninterrupted_work_units,
            ),
        ] {
            checkpoint(cancellation)?;
            structured_requirements.push(requirement_with_authority(
                group.actor,
                Some(group.handler),
                Some(site),
                kind,
                bound,
                actor_current_meaning,
                handler_current_meaning,
                subject_current_meaning,
            ));
        }
        for checkpoint_identity in group.cancellation_checkpoints.iter().copied() {
            checkpoint(cancellation)?;
            structured_requirements.push(requirement_with_authority(
                group.actor,
                Some(group.handler),
                Some(checkpoint_identity),
                FlowRequirementKind::CancellationCheckpoint,
                1,
                actor_current_meaning,
                handler_current_meaning,
                subject_current_meaning,
            ));
        }
        if let Some(class) = group.deadline_class {
            for (kind, site, bound) in [
                (
                    FlowRequirementKind::DeadlineClass,
                    group.identity,
                    u64::from(class == FlowDeadlineClass::Realtime) + 1,
                ),
                (
                    FlowRequirementKind::DeadlineAuthority,
                    group.deadline_authority.unwrap_or(0),
                    1,
                ),
                (
                    FlowRequirementKind::DeadlineSlack,
                    group.identity,
                    group.deadline_slack.unwrap_or(0),
                ),
                (FlowRequirementKind::DeadlineFeasibility, group.identity, 1),
            ] {
                checkpoint(cancellation)?;
                structured_requirements.push(requirement_with_authority(
                    group.actor,
                    Some(group.handler),
                    Some(site),
                    kind,
                    bound,
                    actor_current_meaning,
                    handler_current_meaning,
                    subject_current_meaning,
                ));
            }
        }
    }
    Ok((
        Arc::clone(&candidate.reply_obligations),
        Arc::clone(&candidate.groups),
        Arc::clone(&candidate.group_policy_laws),
        Arc::clone(&candidate.deadline_laws),
        structured_requirements,
    ))
}

#[cfg(test)]
fn verify_proposals(
    candidate: &VerifiedFlowProgram,
    proposals: &[FlowProposal],
    cancellation: &Cancellation,
) -> Result<(), FlowFailure> {
    let exact_custody = candidate
        .proposal_templates
        .iter()
        .map(|template| (template.identity, Arc::clone(&template.resource_custody)))
        .collect::<BTreeMap<_, _>>();
    let mut previous = None;
    let mut mailbox_resources = BTreeSet::new();
    let mut proposal_resources = BTreeSet::new();
    let capacities = candidate
        .actors
        .iter()
        .map(|actor| (actor.identity, actor.mailbox_capacity))
        .collect::<BTreeMap<_, _>>();
    let admission_kinds = candidate
        .proposal_templates
        .iter()
        .map(|template| (template.identity, template.admission_kind))
        .collect::<BTreeMap<_, _>>();
    let mut mailboxes = BTreeMap::<u128, VecDeque<FlowProposalKey>>::new();
    for proposal in proposals {
        checkpoint(cancellation)?;
        if previous.is_some_and(|key| key >= proposal.key) {
            return defect("Flow proposals are not in canonical logical order");
        }
        previous = Some(proposal.key);
        if proposal.before_commit != FlowCustodian::ProposalHome {
            return defect("Flow proposal lost pre-commit Resource custody");
        }
        if exact_custody.get(&proposal.template).map(Arc::as_ref)
            != Some(proposal.resource_custody.as_ref())
        {
            return defect(
                "Flow proposal Resource custody references are not exact Core authority",
            );
        }
        match proposal.outcome {
            FlowSendOutcome::Admitted => {
                if proposal.after_arbitration != FlowCustodian::Mailbox
                    || proposal.transfer_commit.is_none()
                {
                    return defect("admitted Flow proposal has no durable Transfer Commit");
                }
                let mailbox = mailboxes.entry(proposal.key.destination).or_default();
                let capacity = capacities
                    .get(&proposal.key.destination)
                    .copied()
                    .unwrap_or(0);
                if proposal.waited_for_capacity {
                    if admission_kinds.get(&proposal.template) == Some(&FlowAdmissionKind::TrySend)
                        || u64::try_from(mailbox.len()).unwrap_or(u64::MAX) != capacity
                        || mailbox.pop_front() != proposal.dequeued_proposal
                    {
                        return defect("waiting Flow proposal retry transition is invalid");
                    }
                } else if proposal.dequeued_proposal.is_some()
                    || u64::try_from(mailbox.len()).unwrap_or(u64::MAX) >= capacity
                {
                    return defect("Flow proposal committed without available capacity");
                }
                mailbox.push_back(proposal.key);
                for resource in proposal.resource_custody.iter() {
                    let runtime_subject = (proposal.key, resource.core_reference_identity);
                    if !mailbox_resources.insert(runtime_subject)
                        || proposal_resources.contains(&runtime_subject)
                    {
                        return defect("Mailbox does not own committed Resource exactly once");
                    }
                }
            }
            FlowSendOutcome::Full => {
                if proposal.after_arbitration != FlowCustodian::ProposalHome
                    || proposal.transfer_commit.is_some()
                {
                    return defect("Full Flow proposal changed custody or published a commit");
                }
                if proposal.waited_for_capacity || proposal.dequeued_proposal.is_some() {
                    return defect("Full Flow proposal fabricated a waiting retry");
                }
                for resource in proposal.resource_custody.iter() {
                    let runtime_subject = (proposal.key, resource.core_reference_identity);
                    if !proposal_resources.insert(runtime_subject)
                        || mailbox_resources.contains(&runtime_subject)
                    {
                        return defect("Full Flow proposal duplicated or lost Resource custody");
                    }
                }
            }
            FlowSendOutcome::Waiting => {
                if proposal.after_arbitration != FlowCustodian::ProposalHome
                    || proposal.transfer_commit.is_some()
                {
                    return defect("waiting Flow proposal changed custody or published a commit");
                }
                if proposal.waited_for_capacity || proposal.dequeued_proposal.is_some() {
                    return defect("stranded Flow proposal fabricated a completed retry");
                }
                for resource in proposal.resource_custody.iter() {
                    let runtime_subject = (proposal.key, resource.core_reference_identity);
                    if !proposal_resources.insert(runtime_subject)
                        || mailbox_resources.contains(&runtime_subject)
                    {
                        return defect("waiting Flow proposal duplicated or lost Resource custody");
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
fn verify_non_reentrant_trace(
    trace: &[FlowEvent],
    cancellation: &Cancellation,
) -> Result<(), FlowFailure> {
    let mut active = BTreeMap::new();
    for (index, event) in trace.iter().enumerate() {
        checkpoint(cancellation)?;
        if event.sequence != u64::try_from(index).unwrap_or(u64::MAX) {
            return defect("Flow trace sequence is not canonical");
        }
        if event
            .causal_predecessor
            .is_some_and(|predecessor| predecessor >= event.sequence)
        {
            return defect("Flow trace causal relationship is not prior");
        }
        match event.kind {
            FlowEventKind::TurnStarted => {
                if active.insert(event.actor, event.handler).is_some() {
                    return defect("Flow trace re-enters an active Actor Turn");
                }
            }
            FlowEventKind::TurnSuspended | FlowEventKind::TurnResumed => {
                if active.get(&event.actor) != Some(&event.handler)
                    || event.suspension_home.is_none()
                {
                    return defect("Flow suspension or resumption lost its Turn lease or home");
                }
            }
            FlowEventKind::TurnCompleted => {
                if active.remove(&event.actor) != Some(event.handler) {
                    return defect("Flow completed a Turn that was not active");
                }
            }
            FlowEventKind::MessageProposed | FlowEventKind::MessageFull => {
                if event.proposal.is_none() || event.logical_commit.is_some() {
                    return defect("Flow message proposal/full trace record is incomplete");
                }
            }
            FlowEventKind::MailboxTransferCommitted => {
                if event.proposal.is_none() || event.logical_commit.is_none() {
                    return defect("Flow Mailbox Transfer Commit trace record is incomplete");
                }
            }
            FlowEventKind::MailboxDequeued => {
                if event.proposal.is_none() || event.logical_commit.is_some() {
                    return defect("Flow Mailbox dequeue trace record is incomplete");
                }
            }
            FlowEventKind::AdmissionWaiting | FlowEventKind::ReplyPathReserved => {
                if event.proposal.is_none()
                    || event.logical_commit.is_some()
                    || event.suspension_home.is_none()
                {
                    return defect("Flow waiting/reply reservation trace record is incomplete");
                }
            }
            FlowEventKind::AdmissionCancelled
            | FlowEventKind::ReplyFulfilled
            | FlowEventKind::ReplyEndpointClosed
            | FlowEventKind::ReplyClosed
            | FlowEventKind::ChildCompleted
            | FlowEventKind::ChildFailed
            | FlowEventKind::SiblingCancellationRequested
            | FlowEventKind::GroupOutcomePublished
            | FlowEventKind::CancellationAdmissionClosed
            | FlowEventKind::CancellationPropagated
            | FlowEventKind::ChildrenQuiesced
            | FlowEventKind::ResourceReturned
            | FlowEventKind::CleanupRun
            | FlowEventKind::CancelledPublished
            | FlowEventKind::DeadlineUnmeetable
            | FlowEventKind::DeadlineExceeded
            | FlowEventKind::TerminalPanic => {
                return defect("structured-only Flow event leaked into a scheduler trace");
            }
        }
    }
    if !active.is_empty() {
        return defect("Flow trace leaves an Actor Turn active");
    }
    Ok(())
}

struct FlowFingerprintInput<'a> {
    context: u128,
    planning_fingerprint: u128,
    core_fingerprint: u128,
    actors: &'a [Actor],
    requirements: &'a [FlowRequirement],
    homes: &'a [SuspensionHome],
    templates: &'a [ProposalTemplate],
    contract: &'a ModelContract,
    replies: &'a [ReplyObligation],
    groups: &'a [GroupObligation],
    policy_laws: &'a [GroupPolicyLaw],
    deadline_laws: &'a [DeadlineLaw],
}

fn fingerprint(
    input: FlowFingerprintInput<'_>,
    cancellation: &Cancellation,
) -> Result<u128, FlowFailure> {
    let FlowFingerprintInput {
        context,
        planning_fingerprint,
        core_fingerprint,
        actors,
        requirements,
        homes,
        templates,
        contract,
        replies,
        groups,
        policy_laws,
        deadline_laws,
    } = input;
    let mut hash = Xxh3::new();
    hash.update(b"wrela.verified-flow-program\0\x02");
    hash.update(PHASE_SCHEMA.as_bytes());
    hash.update(&context.to_be_bytes());
    hash.update(&planning_fingerprint.to_be_bytes());
    hash.update(&core_fingerprint.to_be_bytes());
    for actor in actors {
        checkpoint(cancellation)?;
        hash.update(&actor.identity.to_be_bytes());
        hash.update(&actor.construction_identity.to_be_bytes());
        hash.update(&actor.mailbox_capacity.to_be_bytes());
        hash.update(&[actor.max_active_turns]);
        hash.update(&actor.permanent_core_requirement.to_be_bytes());
        hash.update(
            &u64::try_from(actor.handlers.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        for construction in actor.wired_actor_constructions.iter() {
            checkpoint(cancellation)?;
            hash.update(&construction.to_be_bytes());
        }
        for handler in actor.handlers.iter() {
            checkpoint(cancellation)?;
            hash.update(&handler.to_be_bytes());
        }
    }
    for requirement in requirements {
        checkpoint(cancellation)?;
        hash.update(&requirement.identity.to_be_bytes());
        hash.update(&[requirement.kind.tag()]);
        hash.update(&requirement.actor.to_be_bytes());
        hash.update(&requirement.handler.unwrap_or(0).to_be_bytes());
        hash.update(&requirement.site.unwrap_or(0).to_be_bytes());
        hash.update(&requirement.bound.to_be_bytes());
        hash.update(&requirement.current_meaning.to_be_bytes());
    }
    for home in homes {
        checkpoint(cancellation)?;
        hash.update(&home.identity.to_be_bytes());
        hash.update(&home.actor.to_be_bytes());
        hash.update(&home.handler.to_be_bytes());
        hash.update(&home.suspension_reference.to_be_bytes());
        hash.update(&home.suspension_current_meaning.to_be_bytes());
        hash.update(&home.program_order.to_be_bytes());
        for part in home.control_path.iter() {
            checkpoint(cancellation)?;
            hash.update(&part.to_be_bytes());
        }
        hash.update(&home.slot_count.to_be_bytes());
        hash.update(&[u8::from(home.retains_turn_lease)]);
        hash.update(&home.requirement.to_be_bytes());
    }
    for template in templates {
        checkpoint(cancellation)?;
        hash.update(&template.identity.to_be_bytes());
        hash.update(&template.current_meaning.to_be_bytes());
        hash.update(&template.sender.to_be_bytes());
        hash.update(&template.sender_handler.to_be_bytes());
        hash.update(&template.destination.to_be_bytes());
        hash.update(&template.destination_handler.to_be_bytes());
        hash.update(&[match template.admission_kind {
            FlowAdmissionKind::TrySend => 1,
            FlowAdmissionKind::WaitingSend => 2,
            FlowAdmissionKind::Request => 3,
        }]);
        hash.update(&template.owning_group.unwrap_or(0).to_be_bytes());
        hash.update(&[match template.deadline_class {
            None => 0,
            Some(FlowDeadlineClass::Logical) => 1,
            Some(FlowDeadlineClass::Realtime) => 2,
        }]);
        hash.update(&template.response_type_identity.to_be_bytes());
        hash.update(&template.send_ordinal.to_be_bytes());
        hash.update(&template.program_order.to_be_bytes());
        hash.update(&template.suspension_home.to_be_bytes());
        for part in template.control_path.iter() {
            checkpoint(cancellation)?;
            hash.update(&part.to_be_bytes());
        }
        for resource in template.resource_custody.iter() {
            checkpoint(cancellation)?;
            hash.update(&resource.core_reference_identity.to_be_bytes());
            hash.update(&resource.core_reference_current_meaning.to_be_bytes());
            hash.update(&resource.proposal_home.to_be_bytes());
            for part in resource.place.iter() {
                checkpoint(cancellation)?;
                hash.update(&part.to_be_bytes());
            }
        }
    }
    hash.update(b"wrela.flow.model-contract\0\x01");
    for (actor, handlers) in contract.actors.iter() {
        checkpoint(cancellation)?;
        hash.update(&actor.to_be_bytes());
        hash.update(
            &u64::try_from(handlers.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
    }
    for home in contract.suspension_homes.iter() {
        checkpoint(cancellation)?;
        hash.update(&home.identity.to_be_bytes());
        hash.update(&home.actor.to_be_bytes());
        hash.update(&home.handler.to_be_bytes());
        hash.update(&home.suspension_reference.to_be_bytes());
        hash.update(&home.suspension_current_meaning.to_be_bytes());
        hash.update(&home.program_order.to_be_bytes());
        for part in home.control_path.iter() {
            hash.update(&part.to_be_bytes());
        }
        hash.update(&home.slot_count.to_be_bytes());
        hash.update(&[u8::from(home.retains_turn_lease)]);
        hash.update(&home.requirement.to_be_bytes());
    }
    for template in contract.templates.iter() {
        checkpoint(cancellation)?;
        hash.update(&template.identity.to_be_bytes());
        hash.update(&template.current_meaning.to_be_bytes());
        hash.update(&template.sender.to_be_bytes());
        hash.update(&template.sender_handler.to_be_bytes());
        hash.update(&template.destination.to_be_bytes());
        hash.update(&template.destination_handler.to_be_bytes());
        hash.update(&[match template.admission_kind {
            FlowAdmissionKind::TrySend => 1,
            FlowAdmissionKind::WaitingSend => 2,
            FlowAdmissionKind::Request => 3,
        }]);
        hash.update(&template.owning_group.unwrap_or(0).to_be_bytes());
        hash.update(&[match template.deadline_class {
            None => 0,
            Some(FlowDeadlineClass::Logical) => 1,
            Some(FlowDeadlineClass::Realtime) => 2,
        }]);
        hash.update(&template.response_type_identity.to_be_bytes());
        hash.update(&template.send_ordinal.to_be_bytes());
        hash.update(&template.program_order.to_be_bytes());
        hash.update(&template.suspension_home.to_be_bytes());
        for part in template.control_path.iter() {
            hash.update(&part.to_be_bytes());
        }
        for resource in template.resource_custody.iter() {
            hash.update(&resource.core_reference_identity.to_be_bytes());
            hash.update(&resource.core_reference_current_meaning.to_be_bytes());
            hash.update(&resource.proposal_home.to_be_bytes());
            for part in resource.place.iter() {
                hash.update(&part.to_be_bytes());
            }
        }
    }
    for (actor, capacity) in contract.mailbox_capacities.iter() {
        checkpoint(cancellation)?;
        hash.update(&actor.to_be_bytes());
        hash.update(&capacity.to_be_bytes());
    }
    for reply in replies {
        checkpoint(cancellation)?;
        hash.update(&reply.identity.to_be_bytes());
        hash.update(&reply.request_template.to_be_bytes());
        hash.update(&reply.endpoint.to_be_bytes());
        hash.update(&reply.return_path.to_be_bytes());
        hash.update(&reply.response_home.to_be_bytes());
        hash.update(&reply.response_type_identity.to_be_bytes());
        hash.update(&reply.capacity.to_be_bytes());
        hash.update(&[u8::from(reply.fulfillment_capacity_infallible)]);
        hash.update(&[u8::from(reply.explicit_cancel)]);
        hash.update(&reply.acyclic_wait_requirement.to_be_bytes());
        for (reference, meaning) in reply.fulfillment_references.iter() {
            checkpoint(cancellation)?;
            hash.update(&reference.to_be_bytes());
            hash.update(&meaning.to_be_bytes());
        }
        for place in reply.fulfillment_endpoint_places.iter() {
            checkpoint(cancellation)?;
            for part in place.iter() {
                hash.update(&part.to_be_bytes());
            }
        }
        for custody in reply.response_custody.iter() {
            checkpoint(cancellation)?;
            hash.update(&custody.core_reference_identity.to_be_bytes());
            hash.update(&custody.core_reference_current_meaning.to_be_bytes());
            hash.update(&custody.type_identity.to_be_bytes());
            for part in custody.place.iter() {
                hash.update(&part.to_be_bytes());
            }
        }
    }
    for group in groups {
        checkpoint(cancellation)?;
        hash.update(&group.identity.to_be_bytes());
        hash.update(&group.actor.to_be_bytes());
        hash.update(&group.handler.to_be_bytes());
        hash.update(&group.receiver_current_meaning.to_be_bytes());
        for part in group.receiver_place.iter() {
            checkpoint(cancellation)?;
            hash.update(&part.to_be_bytes());
        }
        hash.update(&group.cancellation_authority.to_be_bytes());
        hash.update(&[group.policy.tag()]);
        hash.update(&[match group.deadline_class {
            None => 0,
            Some(FlowDeadlineClass::Logical) => 1,
            Some(FlowDeadlineClass::Realtime) => 2,
        }]);
        hash.update(&group.deadline_authority.unwrap_or(0).to_be_bytes());
        hash.update(
            &group
                .deadline_authority_current_meaning
                .unwrap_or(0)
                .to_be_bytes(),
        );
        hash.update(&group.deadline_capture_authority.unwrap_or(0).to_be_bytes());
        hash.update(
            &group
                .deadline_capture_current_meaning
                .unwrap_or(0)
                .to_be_bytes(),
        );
        hash.update(&group.deadline_slack.unwrap_or(0).to_be_bytes());
        hash.update(&group.return_home.to_be_bytes());
        hash.update(&group.child_activation_bound.to_be_bytes());
        hash.update(&group.maximum_uninterrupted_work_units.to_be_bytes());
        hash.update(&[u8::from(group.cancelled)]);
        for child in group.child_activations.iter() {
            checkpoint(cancellation)?;
            hash.update(&child.to_be_bytes());
        }
        for resource in group.moved_resources.iter() {
            checkpoint(cancellation)?;
            hash.update(&resource.core_reference_identity.to_be_bytes());
            hash.update(&resource.core_reference_current_meaning.to_be_bytes());
            hash.update(&resource.type_identity.to_be_bytes());
            hash.update(&resource.source_home.to_be_bytes());
            hash.update(&resource.proposal_home.to_be_bytes());
            for part in resource.place.iter() {
                checkpoint(cancellation)?;
                hash.update(&part.to_be_bytes());
            }
        }
        for safe_point in group.cancellation_checkpoints.iter() {
            checkpoint(cancellation)?;
            hash.update(&safe_point.to_be_bytes());
        }
        for action in group.cleanup_actions.iter() {
            checkpoint(cancellation)?;
            hash.update(&action.identity.to_be_bytes());
            hash.update(&action.current_meaning.to_be_bytes());
            hash.update(&action.handler.to_be_bytes());
            hash.update(&action.program_order.to_be_bytes());
        }
    }
    for reply in contract.replies.iter() {
        checkpoint(cancellation)?;
        hash.update(&reply.identity.to_be_bytes());
        hash.update(&reply.request_template.to_be_bytes());
        hash.update(&reply.endpoint.to_be_bytes());
        hash.update(&reply.return_path.to_be_bytes());
        hash.update(&reply.response_home.to_be_bytes());
        hash.update(&reply.response_type_identity.to_be_bytes());
        hash.update(&reply.capacity.to_be_bytes());
        hash.update(&[u8::from(reply.fulfillment_capacity_infallible)]);
        hash.update(&[u8::from(reply.explicit_cancel)]);
        hash.update(&reply.acyclic_wait_requirement.to_be_bytes());
        for (reference, meaning) in reply.fulfillment_references.iter() {
            hash.update(&reference.to_be_bytes());
            hash.update(&meaning.to_be_bytes());
        }
        for place in reply.fulfillment_endpoint_places.iter() {
            checkpoint(cancellation)?;
            for part in place.iter() {
                hash.update(&part.to_be_bytes());
            }
        }
        for custody in reply.response_custody.iter() {
            checkpoint(cancellation)?;
            hash.update(&custody.core_reference_identity.to_be_bytes());
            hash.update(&custody.core_reference_current_meaning.to_be_bytes());
            hash.update(&custody.type_identity.to_be_bytes());
            for part in custody.place.iter() {
                hash.update(&part.to_be_bytes());
            }
        }
    }
    for group in contract.groups.iter() {
        checkpoint(cancellation)?;
        hash.update(&group.identity.to_be_bytes());
        hash.update(&group.actor.to_be_bytes());
        hash.update(&group.handler.to_be_bytes());
        hash.update(&group.receiver_current_meaning.to_be_bytes());
        for part in group.receiver_place.iter() {
            hash.update(&part.to_be_bytes());
        }
        hash.update(&group.child_activation_bound.to_be_bytes());
        hash.update(&group.cancellation_authority.to_be_bytes());
        hash.update(&[group.policy.tag()]);
        hash.update(&[match group.deadline_class {
            None => 0,
            Some(FlowDeadlineClass::Logical) => 1,
            Some(FlowDeadlineClass::Realtime) => 2,
        }]);
        hash.update(&group.deadline_authority.unwrap_or(0).to_be_bytes());
        hash.update(
            &group
                .deadline_authority_current_meaning
                .unwrap_or(0)
                .to_be_bytes(),
        );
        hash.update(&group.deadline_capture_authority.unwrap_or(0).to_be_bytes());
        hash.update(
            &group
                .deadline_capture_current_meaning
                .unwrap_or(0)
                .to_be_bytes(),
        );
        hash.update(&group.deadline_slack.unwrap_or(0).to_be_bytes());
        hash.update(&group.return_home.to_be_bytes());
        hash.update(&group.maximum_uninterrupted_work_units.to_be_bytes());
        hash.update(&[u8::from(group.cancelled)]);
        for child in group.child_activations.iter() {
            hash.update(&child.to_be_bytes());
        }
        for resource in group.moved_resources.iter() {
            hash.update(&resource.core_reference_identity.to_be_bytes());
            hash.update(&resource.core_reference_current_meaning.to_be_bytes());
            hash.update(&resource.proposal_home.to_be_bytes());
            for part in resource.place.iter() {
                hash.update(&part.to_be_bytes());
            }
        }
        for safe_point in group.cancellation_checkpoints.iter() {
            hash.update(&safe_point.to_be_bytes());
        }
        for action in group.cleanup_actions.iter() {
            hash.update(&action.identity.to_be_bytes());
            hash.update(&action.current_meaning.to_be_bytes());
            hash.update(&action.handler.to_be_bytes());
            hash.update(&action.program_order.to_be_bytes());
        }
    }
    for law in contract.policy_laws.iter() {
        checkpoint(cancellation)?;
        hash.update(&[law.policy.tag()]);
        hash.update(&[u8::from(law.deterministic_result_order)]);
        hash.update(&[u8::from(law.cancels_siblings)]);
        hash.update(&[u8::from(law.collects_failures)]);
        hash.update(&[u8::from(law.supervises_failures)]);
        hash.update(&[u8::from(law.winner_is_logical)]);
        hash.update(&[u8::from(law.host_completion_ignored)]);
    }
    for law in contract.deadline_laws.iter() {
        checkpoint(cancellation)?;
        hash.update(&[match law.class {
            FlowDeadlineClass::Logical => 1,
            FlowDeadlineClass::Realtime => 2,
        }]);
        hash.update(&law.authority.to_be_bytes());
        hash.update(&[u8::from(law.deterministic)]);
        hash.update(&[u8::from(law.replay_capture_required)]);
    }
    for panic in contract.terminal_panics.iter() {
        checkpoint(cancellation)?;
        hash.update(&panic.handler.to_be_bytes());
        hash.update(&panic.identity.to_be_bytes());
        hash.update(&panic.current_meaning.to_be_bytes());
        hash.update(&panic.program_order.to_be_bytes());
    }
    for law in policy_laws {
        checkpoint(cancellation)?;
        hash.update(&[law.policy.tag()]);
        hash.update(&[u8::from(law.deterministic_result_order)]);
        hash.update(&[u8::from(law.cancels_siblings)]);
        hash.update(&[u8::from(law.collects_failures)]);
        hash.update(&[u8::from(law.supervises_failures)]);
        hash.update(&[u8::from(law.winner_is_logical)]);
        hash.update(&[u8::from(law.host_completion_ignored)]);
    }
    for law in deadline_laws {
        checkpoint(cancellation)?;
        hash.update(&[match law.class {
            FlowDeadlineClass::Logical => 1,
            FlowDeadlineClass::Realtime => 2,
        }]);
        hash.update(&law.authority.to_be_bytes());
        hash.update(&[u8::from(law.deterministic)]);
        hash.update(&[u8::from(law.replay_capture_required)]);
    }
    Ok(hash.digest128())
}

fn checkpoint(cancellation: &Cancellation) -> Result<(), FlowFailure> {
    if cancellation.is_cancelled() {
        Err(FlowFailure::Cancelled)
    } else {
        Ok(())
    }
}

fn defect<T>(evidence: impl Into<Arc<str>>) -> Result<T, FlowFailure> {
    Err(FlowFailure::Defect(evidence.into()))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::ArchitectureProfile;
    use crate::compiler::{
        CompilationOutcome, CompilationRequest, Compiler, CompilerInstallation, InspectSelection,
        ProjectFile, ProjectSnapshot, Root,
    };
    use crate::core::VerifiedCoreProgram;
    use crate::image_planning::VerifiedPlanningFoundation;

    const SOURCE: &[u8] = br#"resource struct Token:
    id: i64

async fn yield_once() -> i64:
    return 1

fn consume(take token: Token):
    pass

@actor
struct Receiver:
    pub async fn receive(self, take token: Token):
        _ = await yield_once()
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

    const REPLY_SOURCE: &[u8] = br#"from core import actor as actors

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
        response = await server.exchange(take token)
        consume(take response)

@image
fn build() -> Image:
    server = Server()
    client = Client(server=server)
    return Image.new(server=server, client=client)
"#;

    const GROUP_SOURCE: &[u8] = br#"from core import actor as actors

resource struct Token:
    value: i64

fn consume(take token: Token):
    pass

fn clean():
    pass

@actor
struct Worker:
    pub async fn run(self, take first: Token, take second: Token):
        mut outer = actors.Group.all(bound=1u64)
        outer.logical_deadline(epoch=7u64, slack=12u64)
        defer clean()
        outer_child = outer.child(value=take first)
        mut inner = actors.Group.race(bound=1u64)
        inner_child = inner.child(value=take second)
        inner_return = actors.Group.complete_child(take inner, take inner_child)
        _ = inner_return.outcome
        consume(take inner_return.value)
        outer_return = actors.Group.complete_child(take outer, take outer_child)
        _ = outer_return.outcome
        consume(take outer_return.value)

@image
fn build() -> Image:
    worker = Worker()
    return Image.new(worker=worker)
"#;

    fn fixture_from(
        source: &[u8],
    ) -> (
        VerifiedFlowProgram,
        Arc<VerifiedPlanningFoundation>,
        Arc<VerifiedCoreProgram>,
    ) {
        let request = CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::all());
        let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
        let outcome = compiler.compile(request, &Cancellation::new());
        let CompilationOutcome::Accepted(accepted) = outcome else {
            panic!("private Flow fixture accepts: {outcome:#?}");
        };
        (
            accepted
                .verified_flow_program()
                .expect("Flow derived")
                .clone(),
            Arc::new(
                accepted
                    .verified_planning_foundation()
                    .expect("planning derived")
                    .clone(),
            ),
            Arc::new(
                accepted
                    .verified_core_program()
                    .expect("Core derived")
                    .clone(),
            ),
        )
    }

    fn fixture() -> (
        VerifiedFlowProgram,
        Arc<VerifiedPlanningFoundation>,
        Arc<VerifiedCoreProgram>,
    ) {
        fixture_from(SOURCE)
    }

    fn reply_fixture() -> (
        VerifiedFlowProgram,
        Arc<VerifiedPlanningFoundation>,
        Arc<VerifiedCoreProgram>,
    ) {
        fixture_from(REPLY_SOURCE)
    }

    fn group_fixture() -> (
        VerifiedFlowProgram,
        Arc<VerifiedPlanningFoundation>,
        Arc<VerifiedCoreProgram>,
    ) {
        fixture_from(GROUP_SOURCE)
    }

    fn resign(candidate: &mut VerifiedFlowProgram) {
        candidate.fingerprint = fingerprint(
            FlowFingerprintInput {
                context: candidate.context,
                planning_fingerprint: candidate.planning_fingerprint,
                core_fingerprint: candidate.core_fingerprint,
                actors: &candidate.actors,
                requirements: &candidate.requirements,
                homes: &candidate.suspension_homes,
                templates: &candidate.proposal_templates,
                contract: &candidate.model_contract,
                replies: &candidate.reply_obligations,
                groups: &candidate.groups,
                policy_laws: &candidate.group_policy_laws,
                deadline_laws: &candidate.deadline_laws,
            },
            &Cancellation::new(),
        )
        .expect("fresh cancellation permits fingerprinting");
    }

    fn resign_correlated_group_candidate(
        candidate: &mut VerifiedFlowProgram,
        core: &VerifiedCoreProgram,
    ) {
        candidate.model_contract.groups = candidate.groups.clone();
        let handler_current_meanings = core.for_flow().handler_flow_identities();
        let mut requirements = candidate
            .requirements
            .iter()
            .filter(|requirement| {
                matches!(
                    requirement.kind,
                    FlowRequirementKind::ActorIdentity
                        | FlowRequirementKind::PermanentCorePlacement
                        | FlowRequirementKind::MailboxCapacity
                        | FlowRequirementKind::TurnLease
                        | FlowRequirementKind::SuspensionHome
                        | FlowRequirementKind::LogicalCommitOrder
                        | FlowRequirementKind::ProposalTransport
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        for group in candidate.groups.iter() {
            let actor_current_meaning = candidate
                .actors
                .iter()
                .find(|actor| actor.identity == group.actor)
                .map_or(u128::MAX, |actor| actor.construction_identity);
            let handler_current_meaning = handler_current_meanings
                .get(&group.handler)
                .copied()
                .unwrap_or(u128::MAX);
            let subject_current_meaning = group_requirement_current_meaning(group);
            for (kind, site, bound) in [
                (
                    FlowRequirementKind::GroupChildActivationBound,
                    group.identity,
                    group.child_activation_bound,
                ),
                (
                    FlowRequirementKind::GroupCancellationAuthority,
                    group.cancellation_authority,
                    1,
                ),
                (FlowRequirementKind::GroupOutcomePolicy, group.identity, 1),
                (
                    FlowRequirementKind::GroupResourceReturnHome,
                    group.return_home,
                    u64::try_from(group.moved_resources.len()).unwrap_or(u64::MAX),
                ),
                (
                    FlowRequirementKind::GroupCleanupOrder,
                    group.identity,
                    u64::try_from(group.cleanup_actions.len()).unwrap_or(u64::MAX),
                ),
                (
                    FlowRequirementKind::CancellationObservationWorkBound,
                    group.identity,
                    group.maximum_uninterrupted_work_units,
                ),
            ] {
                requirements.push(requirement_with_authority(
                    group.actor,
                    Some(group.handler),
                    Some(site),
                    kind,
                    bound,
                    actor_current_meaning,
                    handler_current_meaning,
                    subject_current_meaning,
                ));
            }
            for checkpoint in group.cancellation_checkpoints.iter().copied() {
                requirements.push(requirement_with_authority(
                    group.actor,
                    Some(group.handler),
                    Some(checkpoint),
                    FlowRequirementKind::CancellationCheckpoint,
                    1,
                    actor_current_meaning,
                    handler_current_meaning,
                    subject_current_meaning,
                ));
            }
            if let Some(class) = group.deadline_class {
                for (kind, site, bound) in [
                    (
                        FlowRequirementKind::DeadlineClass,
                        group.identity,
                        u64::from(class == FlowDeadlineClass::Realtime) + 1,
                    ),
                    (
                        FlowRequirementKind::DeadlineAuthority,
                        group.deadline_authority.unwrap_or(0),
                        1,
                    ),
                    (
                        FlowRequirementKind::DeadlineSlack,
                        group.identity,
                        group.deadline_slack.unwrap_or(0),
                    ),
                    (FlowRequirementKind::DeadlineFeasibility, group.identity, 1),
                ] {
                    requirements.push(requirement_with_authority(
                        group.actor,
                        Some(group.handler),
                        Some(site),
                        kind,
                        bound,
                        actor_current_meaning,
                        handler_current_meaning,
                        subject_current_meaning,
                    ));
                }
            }
        }
        requirements.sort_by_key(|requirement| requirement.identity);
        candidate.requirements = requirements.into();
        resign(candidate);
    }

    fn rejected(
        candidate: &VerifiedFlowProgram,
        planning: &VerifiedPlanningFoundation,
        core: &VerifiedCoreProgram,
    ) -> bool {
        matches!(
            verify(
                candidate,
                planning.for_flow(),
                core.for_flow(),
                &Cancellation::new(),
            ),
            Err(FlowFailure::Defect(_))
        )
    }

    fn conformance_rejected(candidate: &VerifiedFlowProgram) -> bool {
        let cancellation = Cancellation::new();
        let structured = execute_independent_structured_model(
            &candidate.proposal_templates,
            &candidate.reply_obligations,
            &candidate.groups,
            &candidate.model_contract.terminal_panics,
            &cancellation,
        );
        if structured.as_ref().ok() != Some(&candidate.structured_scenarios) {
            return true;
        }
        let scenarios = execute_independent_scenarios(&candidate.model_contract, &cancellation);
        if scenarios.as_ref().ok() != Some(&candidate.model_scenarios) {
            return true;
        }
        candidate.model_scenarios.iter().any(|scenario| {
            verify_non_reentrant_trace(&scenario.trace, &cancellation).is_err()
                || verify_proposals(candidate, &scenario.proposals, &cancellation).is_err()
        })
    }

    #[test]
    fn verifier_rejects_single_fault_logical_order_corruption() {
        let (mut candidate, planning, core) = fixture();
        let scenario = Arc::make_mut(&mut candidate.model_scenarios)
            .iter_mut()
            .find(|scenario| scenario.proposals.len() >= 2)
            .expect("fixture models competing proposals");
        Arc::make_mut(&mut scenario.proposals).swap(0, 1);
        resign(&mut candidate);
        assert!(!rejected(&candidate, &planning, &core));
        assert!(conformance_rejected(&candidate));
    }

    #[test]
    fn verifier_rejects_single_fault_turn_reentrancy_corruption() {
        let (mut candidate, planning, core) = fixture();
        Arc::make_mut(&mut Arc::make_mut(&mut candidate.model_scenarios)[0].trace)[1].kind =
            FlowEventKind::TurnStarted;
        resign(&mut candidate);
        assert!(!rejected(&candidate, &planning, &core));
        assert!(conformance_rejected(&candidate));
    }

    #[test]
    fn verifier_rejects_single_fault_resource_custody_corruption() {
        let (mut candidate, planning, core) = fixture();
        let scenario = Arc::make_mut(&mut candidate.model_scenarios)
            .iter_mut()
            .find(|scenario| {
                scenario
                    .proposals
                    .iter()
                    .any(|proposal| proposal.outcome == FlowSendOutcome::Admitted)
            })
            .expect("capacity-one fixture models an admission");
        let admitted = Arc::make_mut(&mut scenario.proposals)
            .iter_mut()
            .find(|proposal| proposal.outcome == FlowSendOutcome::Admitted)
            .expect("capacity-one fixture admits one proposal");
        admitted.after_arbitration = FlowCustodian::ProposalHome;
        resign(&mut candidate);
        assert!(!rejected(&candidate, &planning, &core));
        assert!(conformance_rejected(&candidate));
    }

    #[test]
    fn verifier_rejects_single_fault_mailbox_capacity_corruption() {
        let (mut candidate, planning, core) = fixture();
        Arc::make_mut(&mut candidate.actors)[0].mailbox_capacity = 0;
        resign(&mut candidate);
        assert!(rejected(&candidate, &planning, &core));
    }

    #[test]
    fn verifier_rejects_single_fault_compilation_context_corruption() {
        let (mut candidate, planning, core) = fixture();
        candidate.context ^= 1;
        resign(&mut candidate);
        assert!(rejected(&candidate, &planning, &core));
    }

    #[test]
    fn verifier_rejects_missing_and_extra_self_consistent_requirements() {
        let (candidate, planning, core) = fixture();
        let mut missing = candidate.clone();
        let mut requirements = missing.requirements.to_vec();
        requirements.pop();
        missing.requirements = requirements.into();
        resign(&mut missing);
        assert!(rejected(&missing, &planning, &core));

        let mut extra = candidate;
        let actor = extra.actors[0].identity;
        let mut requirements = Arc::make_mut(&mut extra.requirements).to_vec();
        requirements.push(requirement(
            actor,
            None,
            Some(0xfeed),
            FlowRequirementKind::ProposalTransport,
            1,
        ));
        requirements.sort_by_key(|requirement| requirement.identity);
        extra.requirements = requirements.into();
        resign(&mut extra);
        assert!(rejected(&extra, &planning, &core));
    }

    #[test]
    fn verifier_rejects_repointed_suspension_home_requirement() {
        let (mut candidate, planning, core) = fixture();
        let home = Arc::make_mut(&mut candidate.suspension_homes)
            .first_mut()
            .expect("fixture has an await Home");
        home.requirement = candidate.actors[0].permanent_core_requirement;
        resign(&mut candidate);
        assert!(rejected(&candidate, &planning, &core));
    }

    #[test]
    fn verifier_rejects_repointed_try_send_suspension_home() {
        let (mut candidate, planning, core) = fixture();
        Arc::make_mut(&mut candidate.proposal_templates)[0].suspension_home ^= 1;
        resign(&mut candidate);
        assert!(rejected(&candidate, &planning, &core));
    }

    #[test]
    fn verifier_rejects_missing_or_repointed_model_contract_rosters() {
        let (candidate, planning, core) = fixture();

        let mut missing_actor = candidate.clone();
        Arc::make_mut(&mut missing_actor.model_contract.actors)
            .first_mut()
            .expect("fixture has Actor roster")
            .1 = Arc::from([]);
        assert!(rejected(&missing_actor, &planning, &core));
        resign(&mut missing_actor);
        assert!(rejected(&missing_actor, &planning, &core));

        let mut missing_home = candidate.clone();
        let mut homes = missing_home.model_contract.suspension_homes.to_vec();
        homes.pop();
        missing_home.model_contract.suspension_homes = homes.into();
        assert!(rejected(&missing_home, &planning, &core));
        resign(&mut missing_home);
        assert!(rejected(&missing_home, &planning, &core));

        let mut repointed_capacity = candidate;
        Arc::make_mut(&mut repointed_capacity.model_contract.mailbox_capacities)[0].0 ^= 1;
        assert!(rejected(&repointed_capacity, &planning, &core));
        resign(&mut repointed_capacity);
        assert!(rejected(&repointed_capacity, &planning, &core));
    }

    #[test]
    fn verifier_rejects_repointed_core_custody_reference() {
        let (mut candidate, planning, core) = fixture();
        let mut custody = candidate.proposal_templates[0].resource_custody.to_vec();
        custody[0].core_reference_identity ^= 1;
        Arc::make_mut(&mut candidate.proposal_templates)[0].resource_custody = custody.into();
        resign(&mut candidate);
        assert!(rejected(&candidate, &planning, &core));
    }

    #[test]
    fn verifier_rejects_repointed_actor_construction_authority() {
        let (mut candidate, planning, core) = fixture();
        Arc::make_mut(&mut candidate.actors)[0].construction_identity ^= 1;
        resign(&mut candidate);
        assert!(rejected(&candidate, &planning, &core));
    }

    #[test]
    fn verifier_rejects_reply_capacity_path_home_and_cycle_corruptions() {
        let (candidate, planning, core) = reply_fixture();

        let mut capacity = candidate.clone();
        Arc::make_mut(&mut capacity.reply_obligations)[0].capacity = 0;
        resign(&mut capacity);
        assert!(rejected(&capacity, &planning, &core));

        let mut path = candidate.clone();
        Arc::make_mut(&mut path.reply_obligations)[0].return_path ^= 1;
        resign(&mut path);
        assert!(rejected(&path, &planning, &core));

        let mut home = candidate.clone();
        Arc::make_mut(&mut home.reply_obligations)[0].response_home ^= 1;
        resign(&mut home);
        assert!(rejected(&home, &planning, &core));

        let mut missing_fulfillment = candidate.clone();
        Arc::make_mut(&mut missing_fulfillment.reply_obligations)[0].fulfillment_references =
            Arc::from([]);
        resign(&mut missing_fulfillment);
        assert!(rejected(&missing_fulfillment, &planning, &core));

        let mut duplicate_fulfillment = candidate.clone();
        let reference = duplicate_fulfillment.reply_obligations[0].fulfillment_references[0];
        Arc::make_mut(&mut duplicate_fulfillment.reply_obligations)[0].fulfillment_references =
            Arc::from([reference, reference]);
        resign(&mut duplicate_fulfillment);
        assert!(rejected(&duplicate_fulfillment, &planning, &core));

        let mut repointed_fulfillment = candidate.clone();
        Arc::make_mut(
            &mut Arc::make_mut(&mut repointed_fulfillment.reply_obligations)[0]
                .fulfillment_references,
        )[0]
        .0 ^= 1;
        resign(&mut repointed_fulfillment);
        assert!(rejected(&repointed_fulfillment, &planning, &core));

        let mut cycle = candidate;
        let template = Arc::make_mut(&mut cycle.proposal_templates)
            .iter_mut()
            .find(|template| template.admission_kind == FlowAdmissionKind::Request)
            .expect("Reply fixture has request");
        template.destination = template.sender;
        resign(&mut cycle);
        assert!(rejected(&cycle, &planning, &core));
    }

    #[test]
    fn verifier_rejects_group_ownership_bound_policy_and_cleanup_corruptions() {
        let (candidate, planning, core) = reply_fixture();
        assert!(candidate.groups.is_empty());
        let template = &candidate.proposal_templates[0];
        let fabricated = GroupObligation {
            identity: 1,
            actor: template.sender,
            handler: template.sender_handler,
            receiver_place: Arc::from([1]),
            receiver_current_meaning: 1,
            child_activation_bound: 1,
            child_activations: Arc::from([]),
            cancellation_authority: 2,
            policy: FlowGroupPolicy::All,
            deadline_class: None,
            deadline_authority: None,
            deadline_authority_current_meaning: None,
            deadline_capture_authority: None,
            deadline_capture_current_meaning: None,
            deadline_slack: None,
            return_home: 3,
            moved_resources: Arc::clone(&template.resource_custody),
            cleanup_actions: Arc::from([CleanupAction {
                identity: 0xfeed,
                current_meaning: 0xbeef,
                handler: template.sender_handler,
                program_order: 0,
                source: crate::SourceRange::new("src/image.wr", 0, 0),
            }]),
            cancellation_checkpoints: Arc::from([template.suspension_home]),
            maximum_uninterrupted_work_units: 1,
            cancelled: false,
        };
        for mutation in 0..4 {
            let mut supplied = fabricated.clone();
            match mutation {
                0 => supplied.moved_resources = Arc::from([]),
                1 => supplied.child_activation_bound = 0,
                2 => supplied.policy = FlowGroupPolicy::Race,
                _ => supplied.cleanup_actions = Arc::from([]),
            }
            let mut corrupted = candidate.clone();
            corrupted.groups = Arc::from([supplied]);
            resign(&mut corrupted);
            assert!(rejected(&corrupted, &planning, &core));
        }
    }

    #[test]
    fn verifier_rejects_correlated_nested_group_deadline_and_cleanup_repoints() {
        let (candidate, planning, core) = group_fixture();
        assert_eq!(candidate.groups.len(), 2);
        let outer_index = candidate
            .groups
            .iter()
            .position(|group| group.policy == FlowGroupPolicy::All)
            .expect("outer Group");
        let inner_index = candidate
            .groups
            .iter()
            .position(|group| group.policy == FlowGroupPolicy::Race)
            .expect("inner Group");

        let mut receiver = candidate.clone();
        let inner_place = receiver.groups[inner_index].receiver_place.clone();
        Arc::make_mut(&mut receiver.groups)[outer_index].receiver_place = inner_place;
        resign(&mut receiver);
        assert!(rejected(&receiver, &planning, &core));

        let mut deadline = candidate.clone();
        Arc::make_mut(&mut deadline.groups)[outer_index].deadline_authority = Some(0xfeed);
        resign(&mut deadline);
        assert!(rejected(&deadline, &planning, &core));

        let mut cleanup = candidate.clone();
        let outer = &mut Arc::make_mut(&mut cleanup.groups)[outer_index];
        let action = Arc::make_mut(&mut outer.cleanup_actions)
            .first_mut()
            .expect("outer Group owns cleanup");
        action.current_meaning ^= 1;
        resign(&mut cleanup);
        assert!(rejected(&cleanup, &planning, &core));

        let mut correlated_omission = candidate;
        let omitted = correlated_omission.groups[inner_index].identity;
        correlated_omission.groups = correlated_omission
            .groups
            .iter()
            .filter(|group| group.identity != omitted)
            .cloned()
            .collect::<Vec<_>>()
            .into();
        correlated_omission.model_contract.groups = correlated_omission.groups.clone();
        resign(&mut correlated_omission);
        assert!(rejected(&correlated_omission, &planning, &core));
    }

    #[test]
    fn verifier_rejects_correlated_group_cancellation_authority_repoint() {
        let (mut candidate, planning, core) = group_fixture();
        let group = Arc::make_mut(&mut candidate.groups)
            .first_mut()
            .expect("fixture has a Group");
        group.cancellation_authority ^= 1;
        resign_correlated_group_candidate(&mut candidate, &core);

        assert!(rejected(&candidate, &planning, &core));
    }

    #[test]
    fn verifier_rejects_correlated_group_cancellation_checkpoint_repoint() {
        let (mut candidate, planning, core) = group_fixture();
        let group = Arc::make_mut(&mut candidate.groups)
            .first_mut()
            .expect("fixture has a Group");
        Arc::make_mut(&mut group.cancellation_checkpoints)[0] ^= 1;
        resign_correlated_group_candidate(&mut candidate, &core);

        assert!(rejected(&candidate, &planning, &core));
    }

    #[test]
    fn verifier_rejects_correlated_group_cancellation_work_bound() {
        let (mut candidate, planning, core) = group_fixture();
        let group = Arc::make_mut(&mut candidate.groups)
            .first_mut()
            .expect("fixture has a Group");
        group.maximum_uninterrupted_work_units =
            group.maximum_uninterrupted_work_units.saturating_add(1);
        resign_correlated_group_candidate(&mut candidate, &core);

        assert!(rejected(&candidate, &planning, &core));
    }

    #[test]
    fn verifier_rejects_deadline_and_cancellation_corruptions() {
        let (candidate, planning, core) = reply_fixture();
        assert!(candidate.deadline_laws.is_empty());
        for (class, authority, deterministic, capture) in [
            (FlowDeadlineClass::Logical, 1, true, false),
            (FlowDeadlineClass::Realtime, 2, false, true),
            (FlowDeadlineClass::Realtime, 0, false, false),
        ] {
            let mut corrupted = candidate.clone();
            corrupted.deadline_laws = Arc::from([DeadlineLaw {
                class,
                authority,
                deterministic,
                replay_capture_required: capture,
            }]);
            resign(&mut corrupted);
            assert!(rejected(&corrupted, &planning, &core));
        }
    }

    #[test]
    fn model_evidence_faults_do_not_change_static_verification_but_conformance_detects_them() {
        let (candidate, planning, core) = reply_fixture();

        let mut trace = candidate.clone();
        let scenario = Arc::make_mut(&mut trace.structured_scenarios)
            .iter_mut()
            .find(|scenario| scenario.kind == FlowStructuredScenarioKind::ReversedArrival)
            .expect("arrival scenario");
        Arc::make_mut(&mut scenario.events).swap(0, 1);
        resign(&mut trace);
        assert!(!rejected(&trace, &planning, &core));
        assert!(conformance_rejected(&trace));

        let mut model = candidate.clone();
        Arc::make_mut(&mut model.structured_scenarios)[0].outcome = FlowStructuredOutcome::Panic;
        resign(&mut model);
        assert!(!rejected(&model, &planning, &core));
        assert!(conformance_rejected(&model));

        let mut contract = candidate;
        Arc::make_mut(&mut contract.model_contract.replies)[0].return_path ^= 1;
        resign(&mut contract);
        assert!(rejected(&contract, &planning, &core));
    }

    #[test]
    fn cancelled_private_derivation_publishes_no_flow() {
        let (_, planning, core) = fixture();
        let cancellation = Cancellation::new();
        cancellation.cancel();
        assert!(matches!(
            FlowModule.derive(planning.for_flow(), core.for_flow(), &cancellation),
            Err(FlowFailure::Cancelled)
        ));

        for polls in [20, 80, 160] {
            let cancellation = Cancellation::new();
            cancellation.cancel_after_private_polls(polls);
            assert!(
                matches!(
                    FlowModule.derive(planning.for_flow(), core.for_flow(), &cancellation),
                    Err(FlowFailure::Cancelled)
                ),
                "cancellation after {polls} polls"
            );
        }

        let (_, planning, core) = group_fixture();
        for polls in [40, 120, 180] {
            let cancellation = Cancellation::new();
            cancellation.cancel_after_private_polls(polls);
            assert!(
                matches!(
                    FlowModule.derive(planning.for_flow(), core.for_flow(), &cancellation),
                    Err(FlowFailure::Cancelled)
                ),
                "group cancellation after {polls} polls"
            );
        }
    }
}
