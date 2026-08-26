#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
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
    CancellationMaximumLatency,
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
            Self::CancellationMaximumLatency => 22,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FlowStructuredScenarioKind {
    ReversedArrival,
    PreCommitCancellation,
    DurableCommit,
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
    AdmissionCancelled,
    ReplyPathReserved,
    ReplyEndpointClosed,
    ReplyClosed,
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
    owning_group: u128,
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
    capacity: u64,
    fulfillment_capacity_infallible: bool,
    acyclic_wait_requirement: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GroupObligation {
    identity: u128,
    actor: u128,
    handler: u128,
    child_activation_bound: u64,
    cancellation_authority: u128,
    policy: FlowGroupPolicy,
    deadline_class: Option<FlowDeadlineClass>,
    deadline_authority: Option<u128>,
    deadline_slack: Option<u64>,
    return_home: u128,
    moved_resources: Arc<[FlowResourceCustody]>,
    cleanup_actions: Arc<[CleanupAction]>,
    cancellation_checkpoints: Arc<[u128]>,
    maximum_cancellation_latency: u64,
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
struct GroupPolicyLaw {
    policy: FlowGroupPolicy,
    deterministic_result_order: bool,
    cancels_siblings: bool,
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowProposalTemplateObservation {
    identity: u128,
    current_meaning: u128,
    sender: u128,
    destination: u128,
    admission_kind: FlowAdmissionKind,
    owning_group: u128,
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
        Some(self.owning_group)
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
    endpoint: u128,
    return_path: u128,
    response_home: u128,
    capacity: u64,
    fulfillment_capacity_infallible: bool,
    acyclic_wait_requirement: u128,
}

impl FlowReplyObligationObservation {
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowGroupObservation {
    child_activation_bound: u64,
    cancellation_authority: u128,
    return_home: u128,
    maximum_cancellation_latency: u64,
    cleanup_actions: Arc<[u128]>,
    cleanup_execution_order: Arc<[u128]>,
}

impl FlowGroupObservation {
    #[must_use]
    pub const fn child_activation_bound(&self) -> u64 {
        self.child_activation_bound
    }
    #[must_use]
    pub const fn noncopyable_cancellation_authority(&self) -> u128 {
        self.cancellation_authority
    }
    #[must_use]
    pub const fn return_home(&self) -> u128 {
        self.return_home
    }
    #[must_use]
    pub const fn maximum_cancellation_latency(&self) -> u64 {
        self.maximum_cancellation_latency
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
}

impl VerifiedFlowProgram {
    pub(crate) const fn fingerprint(&self) -> u128 {
        self.fingerprint
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
                    endpoint: reply.endpoint,
                    return_path: reply.return_path,
                    response_home: reply.response_home,
                    capacity: reply.capacity,
                    fulfillment_capacity_infallible: reply.fulfillment_capacity_infallible,
                    acyclic_wait_requirement: reply.acyclic_wait_requirement,
                })
                .collect::<Vec<_>>()
                .into(),
            groups: self
                .groups
                .iter()
                .map(|group| FlowGroupObservation {
                    child_activation_bound: group.child_activation_bound,
                    cancellation_authority: group.cancellation_authority,
                    return_home: group.return_home,
                    maximum_cancellation_latency: group.maximum_cancellation_latency,
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
        })
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
        source: crate::SourceRange,
        cycle: Arc<[u128]>,
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
                requirements.push(requirement(input.identity(), None, None, kind, bound));
            }
            for handler in handlers {
                for site in suspension_sites
                    .iter()
                    .filter(|site| site.handler == handler)
                {
                    let requirement = requirement(
                        input.identity(),
                        Some(handler),
                        Some(site.reference_identity),
                        FlowRequirementKind::SuspensionHome,
                        1,
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
        let templates =
            proposal_templates(&actors, &homes, core.message_proposals(), cancellation)?;
        if let Some((source, cycle)) = reply_wait_cycle(&templates) {
            return Err(FlowFailure::Admission { source, cycle });
        }
        let (reply_obligations, groups, group_policy_laws, deadline_laws, structured_requirements) =
            structured_authority(
                &actors,
                &homes,
                &templates,
                &core.cleanup_sites(),
                &core.handler_flow_identities(),
            )?;
        requirements.extend(structured_requirements);
        requirements.sort_by_key(|requirement| requirement.identity);
        let structured_scenarios = produce_structured_scenarios(&templates, &reply_obligations);
        if execute_independent_structured_model(&templates, &reply_obligations)
            != structured_scenarios
        {
            return defect("structured Flow scenarios disagree with independent model");
        }
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
        };
        let model_scenarios =
            produce_bounded_scenarios(&model_contract, &actors, &homes, &templates, cancellation)?;
        let independently_modeled = execute_independent_scenarios(&model_contract, cancellation)?;
        let model = ModelResult {
            agrees: independently_modeled == model_scenarios,
            scenarios: independently_modeled,
        };
        if !model.agrees {
            return defect("Flow graph disagrees with compact independent model");
        }
        let fingerprint = fingerprint(
            FlowFingerprintInput {
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
            fingerprint,
            _verified: Verified,
        };
        verify(&candidate, planning, core, cancellation)?;
        Ok(candidate)
    }
}

fn requirement(
    actor: u128,
    handler: Option<u128>,
    site: Option<u128>,
    kind: FlowRequirementKind,
    bound: u64,
) -> FlowRequirement {
    let identity = requirement_identity(actor, handler, site, kind);
    let mut hash = Xxh3::new();
    hash.update(b"wrela.flow.requirement-meaning\0\x01");
    hash.update(&identity.to_be_bytes());
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

fn group_identity(actor: u128, handler: u128) -> u128 {
    graph_identity(b"group", &[actor, handler])
}

fn policy_laws() -> Arc<[GroupPolicyLaw]> {
    [
        (FlowGroupPolicy::All, true),
        (FlowGroupPolicy::Collect, false),
        (FlowGroupPolicy::Race, true),
        (FlowGroupPolicy::Supervise, false),
    ]
    .into_iter()
    .map(|(policy, cancels_siblings)| GroupPolicyLaw {
        policy,
        deterministic_result_order: true,
        cancels_siblings,
        host_completion_ignored: true,
    })
    .collect::<Vec<_>>()
    .into()
}

fn deadline_laws() -> Arc<[DeadlineLaw]> {
    Arc::from([
        DeadlineLaw {
            class: FlowDeadlineClass::Logical,
            authority: graph_identity(b"logical-deadline-law", &[]),
            deterministic: true,
            replay_capture_required: false,
        },
        DeadlineLaw {
            class: FlowDeadlineClass::Realtime,
            authority: graph_identity(b"monotonic-clock-authority", &[]),
            deterministic: false,
            replay_capture_required: true,
        },
    ])
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
    homes: &[SuspensionHome],
    templates: &[ProposalTemplate],
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
        replies.push(ReplyObligation {
            identity: graph_identity(b"reply", &[template.identity]),
            request_template: template.identity,
            endpoint,
            return_path,
            response_home,
            response_type_identity: template.response_type_identity,
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
    for actor in actors {
        for handler in actor.handlers.iter().copied() {
            let stable_handler =
                handler_flow_identities
                    .get(&handler)
                    .copied()
                    .ok_or_else(|| {
                        FlowFailure::Defect(Arc::from("Group handler has no stable Core identity"))
                    })?;
            let identity = group_identity(actor.identity, stable_handler);
            let group_homes = homes
                .iter()
                .filter(|home| home.actor == actor.identity && home.handler == handler)
                .map(|home| home.identity)
                .collect::<Vec<_>>();
            let group_templates = templates
                .iter()
                .filter(|template| {
                    template.sender == actor.identity && template.sender_handler == handler
                })
                .collect::<Vec<_>>();
            let mut cleanup_actions = cleanup_sites
                .iter()
                .filter(|site| site.handler == handler)
                .map(|site| CleanupAction {
                    identity: site.identity,
                    current_meaning: site.current_meaning,
                    handler: site.handler,
                    program_order: site.program_order,
                    source: site.source.clone(),
                })
                .collect::<Vec<_>>();
            cleanup_actions.sort_by_key(|action| (action.program_order, action.identity));
            let mut resources = group_templates
                .iter()
                .flat_map(|template| template.resource_custody.iter().cloned())
                .collect::<Vec<_>>();
            resources.sort();
            resources.dedup();
            let deadline_class = group_templates
                .iter()
                .any(|template| template.deadline_class.is_some())
                .then_some(FlowDeadlineClass::Logical);
            let child_activation_bound = u64::try_from(
                group_homes
                    .len()
                    .saturating_add(group_templates.len())
                    .max(1),
            )
            .map_err(|_| FlowFailure::Defect(Arc::from("Group activation bound overflows")))?;
            groups.push(GroupObligation {
                identity,
                actor: actor.identity,
                handler,
                child_activation_bound,
                cancellation_authority: graph_identity(b"group-cancel", &[identity]),
                policy: FlowGroupPolicy::All,
                deadline_class,
                deadline_authority: deadline_class
                    .map(|_| graph_identity(b"logical-deadline-authority", &[identity])),
                deadline_slack: deadline_class.map(|_| child_activation_bound.saturating_add(2)),
                return_home: graph_identity(b"group-return-home", &[identity]),
                moved_resources: resources.into(),
                cleanup_actions: cleanup_actions.into(),
                cancellation_checkpoints: group_homes.into(),
                maximum_cancellation_latency: child_activation_bound.saturating_add(1),
            });
        }
    }
    groups.sort_by_key(|group| group.identity);
    let laws = policy_laws();
    let deadline_laws = deadline_laws();
    let mut requirements = Vec::new();
    for reply in &replies {
        let template = templates
            .iter()
            .find(|template| template.identity == reply.request_template)
            .ok_or_else(|| FlowFailure::Defect(Arc::from("Reply has no request template")))?;
        for (kind, site) in [
            (FlowRequirementKind::ReplyEndpoint, reply.endpoint),
            (FlowRequirementKind::ReplyReturnPath, reply.return_path),
            (FlowRequirementKind::ReplyResponseHome, reply.response_home),
            (
                FlowRequirementKind::ReplyAcyclicWait,
                reply.request_template,
            ),
        ] {
            requirements.push(requirement(
                template.sender,
                Some(template.sender_handler),
                Some(site),
                kind,
                1,
            ));
        }
    }
    for group in &groups {
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
            (
                FlowRequirementKind::GroupOutcomePolicy,
                group.identity,
                u64::from(group.policy.tag()),
            ),
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
                FlowRequirementKind::CancellationMaximumLatency,
                group.identity,
                group.maximum_cancellation_latency,
            ),
            (
                FlowRequirementKind::ServiceStorage,
                group.identity,
                group.child_activation_bound,
            ),
            (
                FlowRequirementKind::ActivationStorage,
                group.identity,
                group.child_activation_bound,
            ),
        ] {
            requirements.push(requirement(
                group.actor,
                Some(group.handler),
                Some(site),
                kind,
                bound,
            ));
        }
        for checkpoint_id in group.cancellation_checkpoints.iter().copied() {
            requirements.push(requirement(
                group.actor,
                Some(group.handler),
                Some(checkpoint_id),
                FlowRequirementKind::CancellationCheckpoint,
                group.maximum_cancellation_latency,
            ));
        }
        for (reverse_ordinal, action) in group.cleanup_actions.iter().rev().enumerate() {
            requirements.push(requirement(
                group.actor,
                Some(group.handler),
                Some(action.identity),
                FlowRequirementKind::GroupCleanupOrder,
                u64::try_from(reverse_ordinal).unwrap_or(u64::MAX),
            ));
        }
        if let (Some(class), Some(authority), Some(slack)) = (
            group.deadline_class,
            group.deadline_authority,
            group.deadline_slack,
        ) {
            for (kind, site, bound) in [
                (
                    FlowRequirementKind::DeadlineClass,
                    group.identity,
                    match class {
                        FlowDeadlineClass::Logical => 1,
                        FlowDeadlineClass::Realtime => 2,
                    },
                ),
                (FlowRequirementKind::DeadlineAuthority, authority, 1),
                (FlowRequirementKind::DeadlineSlack, group.identity, slack),
                (FlowRequirementKind::DeadlineFeasibility, group.identity, 1),
            ] {
                requirements.push(requirement(
                    group.actor,
                    Some(group.handler),
                    Some(site),
                    kind,
                    bound,
                ));
            }
        }
    }
    for law in deadline_laws.iter() {
        requirements.push(requirement(
            0,
            None,
            Some(law.authority),
            FlowRequirementKind::DeadlineClass,
            match law.class {
                FlowDeadlineClass::Logical => 1,
                FlowDeadlineClass::Realtime => 2,
            },
        ));
        requirements.push(requirement(
            0,
            None,
            Some(law.authority),
            FlowRequirementKind::DeadlineAuthority,
            1,
        ));
    }
    Ok((
        replies.into(),
        groups.into(),
        laws,
        deadline_laws,
        requirements,
    ))
}

fn reply_wait_cycle(templates: &[ProposalTemplate]) -> Option<(crate::SourceRange, Arc<[u128]>)> {
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
    fn visit(
        node: u128,
        edges: &BTreeMap<u128, Vec<(u128, crate::SourceRange)>>,
        stack: &mut Vec<u128>,
        active: &mut BTreeSet<u128>,
        complete: &mut BTreeSet<u128>,
    ) -> Option<(crate::SourceRange, Arc<[u128]>)> {
        if complete.contains(&node) {
            return None;
        }
        active.insert(node);
        stack.push(node);
        for (target, source) in edges.get(&node).into_iter().flatten() {
            if let Some(start) = stack.iter().position(|member| member == target) {
                let mut cycle = stack[start..].to_vec();
                cycle.push(*target);
                return Some((source.clone(), cycle.into()));
            }
            if !active.contains(target)
                && let Some(cycle) = visit(*target, edges, stack, active, complete)
            {
                return Some(cycle);
            }
        }
        stack.pop();
        active.remove(&node);
        complete.insert(node);
        None
    }
    let mut active = BTreeSet::new();
    let mut complete = BTreeSet::new();
    for node in edges.keys().copied() {
        if let Some(cycle) = visit(node, &edges, &mut Vec::new(), &mut active, &mut complete) {
            return Some(cycle);
        }
    }
    None
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
    }
}

fn produce_structured_scenarios(
    templates: &[ProposalTemplate],
    replies: &[ReplyObligation],
) -> Arc<[StructuredScenario]> {
    let mut scenarios = vec![StructuredScenario {
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
    }];
    if templates
        .iter()
        .any(|template| template.admission_kind == FlowAdmissionKind::WaitingSend)
    {
        scenarios.push(StructuredScenario {
            kind: FlowStructuredScenarioKind::PreCommitCancellation,
            outcome: FlowStructuredOutcome::Cancelled,
            events: Arc::from([
                structured_event(
                    0,
                    FlowEventKind::AdmissionWaiting,
                    Some(FlowCustodian::ProposalHome),
                    false,
                ),
                structured_event(
                    1,
                    FlowEventKind::AdmissionCancelled,
                    Some(FlowCustodian::ProposalHome),
                    false,
                ),
            ]),
            winner_order: Arc::from([]),
            cleanup_order: Arc::from([]),
        });
        scenarios.push(StructuredScenario {
            kind: FlowStructuredScenarioKind::DurableCommit,
            outcome: FlowStructuredOutcome::Completed,
            events: Arc::from([
                structured_event(
                    0,
                    FlowEventKind::AdmissionWaiting,
                    Some(FlowCustodian::ProposalHome),
                    false,
                ),
                structured_event(
                    1,
                    FlowEventKind::MailboxTransferCommitted,
                    Some(FlowCustodian::Mailbox),
                    false,
                ),
                structured_event(
                    2,
                    FlowEventKind::CancellationPropagated,
                    Some(FlowCustodian::Mailbox),
                    false,
                ),
            ]),
            winner_order: Arc::from([]),
            cleanup_order: Arc::from([]),
        });
    }
    if !replies.is_empty() {
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
    scenarios.extend([
        StructuredScenario {
            kind: FlowStructuredScenarioKind::GroupPolicies,
            outcome: FlowStructuredOutcome::Completed,
            events: Arc::from([
                structured_event(0, FlowEventKind::ChildrenQuiesced, None, false),
                structured_event(
                    1,
                    FlowEventKind::ResourceReturned,
                    Some(FlowCustodian::GroupReturnHome),
                    false,
                ),
            ]),
            winner_order: Arc::from([0, 1, 2, 3]),
            cleanup_order: Arc::from([]),
        },
        StructuredScenario {
            kind: FlowStructuredScenarioKind::DeadlineUnmeetable,
            outcome: FlowStructuredOutcome::DeadlineUnmeetable,
            events: Arc::from([structured_event(
                0,
                FlowEventKind::DeadlineUnmeetable,
                Some(FlowCustodian::ProposalHome),
                false,
            )]),
            winner_order: Arc::from([]),
            cleanup_order: Arc::from([]),
        },
        StructuredScenario {
            kind: FlowStructuredScenarioKind::DeadlineExceeded,
            outcome: FlowStructuredOutcome::DeadlineExceeded,
            events: Arc::from([
                structured_event(
                    0,
                    FlowEventKind::DeadlineExceeded,
                    Some(FlowCustodian::GroupReturnHome),
                    false,
                ),
                structured_event(
                    1,
                    FlowEventKind::ResourceReturned,
                    Some(FlowCustodian::GroupReturnHome),
                    false,
                ),
                structured_event(2, FlowEventKind::CleanupRun, None, false),
            ]),
            winner_order: Arc::from([]),
            cleanup_order: Arc::from([2, 1, 0]),
        },
        StructuredScenario {
            kind: FlowStructuredScenarioKind::ReverseCleanup,
            outcome: FlowStructuredOutcome::Cancelled,
            events: Arc::from([
                structured_event(
                    0,
                    FlowEventKind::CancellationAdmissionClosed,
                    Some(FlowCustodian::GroupReturnHome),
                    false,
                ),
                structured_event(
                    1,
                    FlowEventKind::CancellationPropagated,
                    Some(FlowCustodian::GroupReturnHome),
                    false,
                ),
                structured_event(
                    2,
                    FlowEventKind::ChildrenQuiesced,
                    Some(FlowCustodian::GroupReturnHome),
                    false,
                ),
                structured_event(
                    3,
                    FlowEventKind::ResourceReturned,
                    Some(FlowCustodian::GroupReturnHome),
                    false,
                ),
                structured_event(4, FlowEventKind::CleanupRun, None, false),
                structured_event(5, FlowEventKind::CleanupRun, None, false),
                structured_event(6, FlowEventKind::CleanupRun, None, false),
                structured_event(7, FlowEventKind::CancelledPublished, None, false),
            ]),
            winner_order: Arc::from([]),
            cleanup_order: Arc::from([2, 1, 0]),
        },
        StructuredScenario {
            kind: FlowStructuredScenarioKind::CustodyRecovery,
            outcome: FlowStructuredOutcome::Cancelled,
            events: Arc::from([
                structured_event(
                    0,
                    FlowEventKind::AdmissionCancelled,
                    Some(FlowCustodian::ProposalHome),
                    false,
                ),
                structured_event(
                    1,
                    FlowEventKind::ResourceReturned,
                    Some(FlowCustodian::GroupReturnHome),
                    false,
                ),
            ]),
            winner_order: Arc::from([]),
            cleanup_order: Arc::from([]),
        },
        StructuredScenario {
            kind: FlowStructuredScenarioKind::TerminalPanic,
            outcome: FlowStructuredOutcome::Panic,
            events: Arc::from([structured_event(
                0,
                FlowEventKind::TerminalPanic,
                None,
                false,
            )]),
            winner_order: Arc::from([]),
            cleanup_order: Arc::from([]),
        },
    ]);
    scenarios.sort_by_key(|scenario| scenario.kind);
    scenarios.into()
}

fn execute_independent_structured_model(
    templates: &[ProposalTemplate],
    replies: &[ReplyObligation],
) -> Arc<[StructuredScenario]> {
    type ModelEvent = (FlowEventKind, Option<FlowCustodian>, bool);
    type ModelScript = &'static [ModelEvent];
    type ModelOrder = &'static [u32];

    let waiting = templates
        .iter()
        .any(|template| template.admission_kind == FlowAdmissionKind::WaitingSend);
    let mut kinds = vec![FlowStructuredScenarioKind::ReversedArrival];
    if waiting {
        kinds.extend([
            FlowStructuredScenarioKind::PreCommitCancellation,
            FlowStructuredScenarioKind::DurableCommit,
        ]);
    }
    if !replies.is_empty() {
        kinds.push(FlowStructuredScenarioKind::ReplyClosedRecovery);
    }
    kinds.extend([
        FlowStructuredScenarioKind::GroupPolicies,
        FlowStructuredScenarioKind::DeadlineUnmeetable,
        FlowStructuredScenarioKind::DeadlineExceeded,
        FlowStructuredScenarioKind::ReverseCleanup,
        FlowStructuredScenarioKind::CustodyRecovery,
        FlowStructuredScenarioKind::TerminalPanic,
    ]);
    let mut scenarios = Vec::new();
    for kind in kinds {
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
                structured_event(
                    u64::try_from(sequence).unwrap_or(u64::MAX),
                    *event,
                    *custodian,
                    *must_use,
                )
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
    scenarios.sort_by_key(|scenario| scenario.kind);
    scenarios.into()
}

fn proposal_templates(
    actors: &[Actor],
    homes: &[SuspensionHome],
    messages: Vec<crate::core::FlowCoreMessageProposal>,
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
                    (*destination == sender
                        && message.admission_kind == crate::core::FlowCoreAdmissionKind::Request)
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
                let mut meaning = Xxh3::new();
                meaning.update(b"wrela.flow.proposal-template-meaning\0\x01");
                meaning.update(&identity.to_be_bytes());
                meaning.update(&message.operation_current_meaning.to_be_bytes());
                meaning.update(&[match message.admission_kind {
                    crate::core::FlowCoreAdmissionKind::TrySend => 1,
                    crate::core::FlowCoreAdmissionKind::WaitingSend => 2,
                    crate::core::FlowCoreAdmissionKind::Request => 3,
                }]);
                for resource in &resource_custody {
                    meaning.update(&resource.core_reference_current_meaning.to_be_bytes());
                }
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
                    owning_group: group_identity(sender, message.sender_flow_identity),
                    deadline_class: (message.admission_kind
                        != crate::core::FlowCoreAdmissionKind::TrySend)
                        .then_some(FlowDeadlineClass::Logical),
                    response_type_identity: if message.admission_kind
                        == crate::core::FlowCoreAdmissionKind::Request
                    {
                        message.response_type_identity
                    } else {
                        0
                    },
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

fn produce_bounded_scenarios(
    contract: &ModelContract,
    actors: &[Actor],
    homes: &[SuspensionHome],
    templates: &[ProposalTemplate],
    cancellation: &Cancellation,
) -> Result<Arc<[ModelScenario]>, FlowFailure> {
    let selections = produce_path_selections(contract, cancellation)?;
    let mut scenarios = Vec::with_capacity(selections.len());
    for selected_paths in selections {
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
    Ok(scenarios.into())
}

fn produce_path_selections(
    contract: &ModelContract,
    cancellation: &Cancellation,
) -> Result<Vec<SelectedControlPaths>, FlowFailure> {
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
                if expanded.len() > MODEL_SCENARIO_BOUND {
                    return defect("bounded Flow model exceeds its explicit scenario limit");
                }
            }
        }
        selections = expanded;
    }
    if selections.is_empty() {
        selections.push(Vec::new());
    }
    Ok(selections.into_iter().map(Arc::from).collect())
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
    let mut committed = BTreeMap::<u128, u64>::new();
    let mut commit_sequence = 0_u64;
    let mut results = Vec::with_capacity(ordered.len());
    for proposal in ordered {
        checkpoint(cancellation)?;
        let capacity = capacities
            .get(&proposal.key.destination)
            .copied()
            .unwrap_or(0);
        let occupancy = committed.entry(proposal.key.destination).or_default();
        let admitted =
            proposal.admission_kind != FlowAdmissionKind::TrySend || *occupancy < capacity;
        let transfer_commit = admitted.then_some(commit_sequence);
        if admitted {
            if proposal.admission_kind == FlowAdmissionKind::TrySend {
                *occupancy = occupancy.saturating_add(1);
            }
            commit_sequence = commit_sequence.saturating_add(1);
        }
        results.push(FlowProposal {
            template: proposal.template,
            key: proposal.key,
            arrival_ordinal: proposal.arrival_ordinal,
            source: proposal.source.clone(),
            outcome: if admitted {
                FlowSendOutcome::Admitted
            } else {
                FlowSendOutcome::Full
            },
            resource_custody: Arc::clone(&proposal.resource_custody),
            before_commit: FlowCustodian::ProposalHome,
            after_arbitration: if admitted {
                FlowCustodian::Mailbox
            } else {
                FlowCustodian::ProposalHome
            },
            transfer_commit,
        });
    }
    Ok(results.into())
}

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
            return defect("independent Flow model exceeds its explicit scenario limit");
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
        let mut occupancy = BTreeMap::<u128, u64>::new();
        let mut commit = 0_u64;
        let mut proposals = Vec::new();
        for proposal in runtime {
            checkpoint(cancellation)?;
            let current = occupancy.entry(proposal.key.destination).or_default();
            let admitted = proposal.admission_kind != FlowAdmissionKind::TrySend
                || *current
                    < capacities
                        .get(&proposal.key.destination)
                        .copied()
                        .unwrap_or(0);
            let transfer_commit = admitted.then_some(commit);
            if admitted {
                if proposal.admission_kind == FlowAdmissionKind::TrySend {
                    *current = current.saturating_add(1);
                }
                commit = commit.saturating_add(1);
            }
            proposals.push(FlowProposal {
                template: proposal.template,
                key: proposal.key,
                arrival_ordinal: proposal.arrival_ordinal,
                source: proposal.source,
                outcome: if admitted {
                    FlowSendOutcome::Admitted
                } else {
                    FlowSendOutcome::Full
                },
                resource_custody: proposal.resource_custody,
                before_commit: FlowCustodian::ProposalHome,
                after_arbitration: if admitted {
                    FlowCustodian::Mailbox
                } else {
                    FlowCustodian::ProposalHome
                },
                transfer_commit,
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
            for kind in [FlowEventKind::TurnSuspended, FlowEventKind::TurnResumed] {
                causal = append(
                    &mut events,
                    kind,
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
            };
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
            prior = append(
                &mut observations,
                FlowEventKind::TurnResumed,
                actor,
                handler,
                turn,
                Some(message.key),
                Some(template.suspension_home),
                Some(prior),
                None,
                u64::from(template.program_order),
            );
            if message.outcome == FlowSendOutcome::Admitted {
                deliveries.push((message, template));
                prior = append(
                    &mut observations,
                    FlowEventKind::MailboxTransferCommitted,
                    message.key.destination,
                    template.destination_handler,
                    message.transfer_commit.unwrap_or(0),
                    Some(message.key),
                    None,
                    Some(prior),
                    message.transfer_commit,
                    u64::from(template.program_order),
                );
            } else {
                prior = append(
                    &mut observations,
                    FlowEventKind::MessageFull,
                    actor,
                    handler,
                    turn,
                    Some(message.key),
                    None,
                    Some(prior),
                    None,
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
            expected_requirements.push(requirement(actor.identity, None, None, kind, bound));
        }
        for site in suspension_sites
            .iter()
            .filter(|site| actor.handlers.contains(&site.handler))
        {
            expected_requirements.push(requirement(
                actor.identity,
                Some(site.handler),
                Some(site.reference_identity),
                FlowRequirementKind::SuspensionHome,
                1,
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
            || *supplied
                != requirement(
                    supplied.actor,
                    supplied.handler,
                    supplied.site,
                    supplied.kind,
                    supplied.bound,
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
    let expected_templates = proposal_templates(
        &expected_actors,
        &expected_homes,
        core.message_proposals(),
        cancellation,
    )?;
    if reply_wait_cycle(&expected_templates).is_some() {
        return defect("verified Flow retained a statically knowable Reply wait cycle");
    }
    let (
        expected_replies,
        expected_groups,
        expected_policy_laws,
        expected_deadline_laws,
        structured_requirements,
    ) = structured_authority(
        &expected_actors,
        &expected_homes,
        &expected_templates,
        &core.cleanup_sites(),
        &core.handler_flow_identities(),
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
    let expected_structured = produce_structured_scenarios(&expected_templates, &expected_replies);
    if candidate.structured_scenarios != expected_structured
        || execute_independent_structured_model(&expected_templates, &expected_replies)
            != expected_structured
    {
        return defect("Flow structured scenarios or independent model are invalid");
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
    for scenario in candidate.model_scenarios.iter() {
        verify_non_reentrant_trace(&scenario.trace, cancellation)?;
        let expected_trace = produce_trace(
            &candidate.actors,
            &candidate.suspension_homes,
            &candidate.proposal_templates,
            &scenario.proposals,
            &scenario.selected_paths,
            cancellation,
        )?;
        if expected_trace != scenario.trace {
            return defect(
                "Flow model scenario trace is not the canonical static graph projection",
            );
        }
        verify_proposals(candidate, &scenario.proposals, cancellation)?;
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
    };
    if candidate.model_contract != expected_contract {
        return defect("Flow model contract roster or direct relationship is invalid");
    }
    let modeled = execute_independent_scenarios(&candidate.model_contract, cancellation)?;
    if modeled != candidate.model.scenarios
        || candidate.model.scenarios != candidate.model_scenarios
        || !candidate.model.agrees
    {
        return defect("Flow compact model and typed trace disagree");
    }
    let expected_fingerprint = fingerprint(
        FlowFingerprintInput {
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
                for resource in proposal.resource_custody.iter() {
                    let runtime_subject = (proposal.key, resource.core_reference_identity);
                    if !proposal_resources.insert(runtime_subject)
                        || mailbox_resources.contains(&runtime_subject)
                    {
                        return defect("Full Flow proposal duplicated or lost Resource custody");
                    }
                }
            }
        }
    }
    let capacities = candidate
        .actors
        .iter()
        .map(|actor| (actor.identity, actor.mailbox_capacity))
        .collect::<BTreeMap<_, _>>();
    let mut admissions = BTreeMap::<u128, u64>::new();
    for proposal in proposals.iter().filter(|proposal| {
        proposal.outcome == FlowSendOutcome::Admitted
            && candidate
                .proposal_templates
                .iter()
                .find(|template| template.identity == proposal.template)
                .is_some_and(|template| template.admission_kind == FlowAdmissionKind::TrySend)
    }) {
        *admissions.entry(proposal.key.destination).or_default() += 1;
    }
    if admissions
        .iter()
        .any(|(actor, count)| *count > capacities.get(actor).copied().unwrap_or(0))
    {
        return defect("Flow Mailbox logical capacity is oversubscribed");
    }
    Ok(())
}

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
            FlowEventKind::AdmissionWaiting
            | FlowEventKind::AdmissionCancelled
            | FlowEventKind::ReplyPathReserved
            | FlowEventKind::ReplyEndpointClosed
            | FlowEventKind::ReplyClosed
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
    hash.update(b"wrela.verified-flow-program\0\x01");
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
            hash.update(&construction.to_be_bytes());
        }
    }
    for requirement in requirements {
        checkpoint(cancellation)?;
        hash.update(&requirement.identity.to_be_bytes());
        hash.update(&requirement.current_meaning.to_be_bytes());
    }
    for home in homes {
        checkpoint(cancellation)?;
        hash.update(&home.identity.to_be_bytes());
        hash.update(&home.actor.to_be_bytes());
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
    for template in templates {
        checkpoint(cancellation)?;
        hash.update(&template.identity.to_be_bytes());
        hash.update(&template.current_meaning.to_be_bytes());
        hash.update(&template.sender.to_be_bytes());
        hash.update(&template.destination.to_be_bytes());
        hash.update(&[match template.admission_kind {
            FlowAdmissionKind::TrySend => 1,
            FlowAdmissionKind::WaitingSend => 2,
            FlowAdmissionKind::Request => 3,
        }]);
        hash.update(&template.owning_group.to_be_bytes());
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
        hash.update(&home.suspension_reference.to_be_bytes());
        hash.update(&home.requirement.to_be_bytes());
    }
    for template in contract.templates.iter() {
        checkpoint(cancellation)?;
        hash.update(&template.identity.to_be_bytes());
        hash.update(&template.sender.to_be_bytes());
        hash.update(&template.destination.to_be_bytes());
        hash.update(&template.suspension_home.to_be_bytes());
    }
    for (actor, capacity) in contract.mailbox_capacities.iter() {
        checkpoint(cancellation)?;
        hash.update(&actor.to_be_bytes());
        hash.update(&capacity.to_be_bytes());
    }
    for reply in replies {
        hash.update(&reply.identity.to_be_bytes());
        hash.update(&reply.endpoint.to_be_bytes());
        hash.update(&reply.return_path.to_be_bytes());
        hash.update(&reply.response_home.to_be_bytes());
        hash.update(&reply.response_type_identity.to_be_bytes());
    }
    for group in groups {
        hash.update(&group.identity.to_be_bytes());
        hash.update(&group.cancellation_authority.to_be_bytes());
        hash.update(&group.return_home.to_be_bytes());
        hash.update(&group.child_activation_bound.to_be_bytes());
        hash.update(&group.maximum_cancellation_latency.to_be_bytes());
        for resource in group.moved_resources.iter() {
            hash.update(&resource.core_reference_identity.to_be_bytes());
            hash.update(&resource.core_reference_current_meaning.to_be_bytes());
        }
        for checkpoint in group.cancellation_checkpoints.iter() {
            hash.update(&checkpoint.to_be_bytes());
        }
        for action in group.cleanup_actions.iter() {
            hash.update(&action.identity.to_be_bytes());
            hash.update(&action.current_meaning.to_be_bytes());
            hash.update(&action.program_order.to_be_bytes());
        }
    }
    for law in policy_laws {
        hash.update(&[law.policy.tag()]);
        hash.update(&[u8::from(law.deterministic_result_order)]);
        hash.update(&[u8::from(law.cancels_siblings)]);
        hash.update(&[u8::from(law.host_completion_ignored)]);
    }
    for law in deadline_laws {
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

    const REPLY_SOURCE: &[u8] = br#"resource struct Token:
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
        response = await server.exchange(take token)
        consume(take response)

@image
fn build() -> Image:
    server = Server()
    client = Client(server=server)
    return Image.new(server=server, client=client)
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

    fn resign(candidate: &mut VerifiedFlowProgram) {
        candidate.fingerprint = fingerprint(
            FlowFingerprintInput {
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

    #[test]
    fn verifier_rejects_single_fault_logical_order_corruption() {
        let (mut candidate, planning, core) = fixture();
        let scenario = Arc::make_mut(&mut candidate.model_scenarios)
            .iter_mut()
            .find(|scenario| scenario.proposals.len() >= 2)
            .expect("fixture models competing proposals");
        Arc::make_mut(&mut scenario.proposals).swap(0, 1);
        resign(&mut candidate);
        assert!(rejected(&candidate, &planning, &core));
    }

    #[test]
    fn verifier_rejects_single_fault_turn_reentrancy_corruption() {
        let (mut candidate, planning, core) = fixture();
        Arc::make_mut(&mut Arc::make_mut(&mut candidate.model_scenarios)[0].trace)[1].kind =
            FlowEventKind::TurnStarted;
        resign(&mut candidate);
        assert!(rejected(&candidate, &planning, &core));
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
        assert!(rejected(&candidate, &planning, &core));
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

        let mut ownership = candidate.clone();
        Arc::make_mut(&mut ownership.groups)
            .iter_mut()
            .find(|group| !group.moved_resources.is_empty())
            .expect("request Group owns moved custody")
            .moved_resources = Arc::from([]);
        resign(&mut ownership);
        assert!(rejected(&ownership, &planning, &core));

        let mut bound = candidate.clone();
        Arc::make_mut(&mut bound.groups)[0].child_activation_bound = 0;
        resign(&mut bound);
        assert!(rejected(&bound, &planning, &core));

        let mut policy = candidate.clone();
        Arc::make_mut(&mut policy.groups)[0].policy = FlowGroupPolicy::Race;
        resign(&mut policy);
        assert!(rejected(&policy, &planning, &core));

        let mut cleanup = candidate;
        let handler = cleanup.groups[0].handler;
        Arc::make_mut(&mut cleanup.groups)[0].cleanup_actions = Arc::from([CleanupAction {
            identity: 0xfeed,
            current_meaning: 0xbeef,
            handler,
            program_order: 0,
            source: crate::SourceRange::new("src/image.wr", 0, 0),
        }]);
        resign(&mut cleanup);
        assert!(rejected(&cleanup, &planning, &core));
    }

    #[test]
    fn verifier_rejects_deadline_and_cancellation_corruptions() {
        let (candidate, planning, core) = reply_fixture();
        let deadline_group = candidate
            .groups
            .iter()
            .position(|group| group.deadline_class.is_some())
            .expect("request Group has logical deadline");

        let mut class = candidate.clone();
        Arc::make_mut(&mut class.groups)[deadline_group].deadline_class =
            Some(FlowDeadlineClass::Realtime);
        resign(&mut class);
        assert!(rejected(&class, &planning, &core));

        let mut authority = candidate.clone();
        Arc::make_mut(&mut authority.groups)[deadline_group].deadline_authority = None;
        resign(&mut authority);
        assert!(rejected(&authority, &planning, &core));

        let mut slack = candidate.clone();
        Arc::make_mut(&mut slack.groups)[deadline_group].deadline_slack = Some(0);
        resign(&mut slack);
        assert!(rejected(&slack, &planning, &core));

        let mut latency = candidate.clone();
        Arc::make_mut(&mut latency.groups)[deadline_group].maximum_cancellation_latency = 0;
        resign(&mut latency);
        assert!(rejected(&latency, &planning, &core));

        let mut checkpoint = candidate;
        Arc::make_mut(&mut checkpoint.groups)[deadline_group].cancellation_checkpoints =
            Arc::from([]);
        resign(&mut checkpoint);
        assert!(rejected(&checkpoint, &planning, &core));

        let (mut realtime, planning, core) = reply_fixture();
        Arc::make_mut(&mut realtime.deadline_laws)
            .iter_mut()
            .find(|law| law.class == FlowDeadlineClass::Realtime)
            .expect("realtime law")
            .authority ^= 1;
        resign(&mut realtime);
        assert!(rejected(&realtime, &planning, &core));
    }

    #[test]
    fn verifier_rejects_structured_trace_model_and_contract_relationship_corruptions() {
        let (candidate, planning, core) = reply_fixture();

        let mut trace = candidate.clone();
        let scenario = Arc::make_mut(&mut trace.structured_scenarios)
            .iter_mut()
            .find(|scenario| scenario.kind == FlowStructuredScenarioKind::ReverseCleanup)
            .expect("reverse cleanup scenario");
        Arc::make_mut(&mut scenario.events).swap(0, 1);
        resign(&mut trace);
        assert!(rejected(&trace, &planning, &core));

        let mut model = candidate.clone();
        Arc::make_mut(&mut model.structured_scenarios)[0].outcome = FlowStructuredOutcome::Panic;
        resign(&mut model);
        assert!(rejected(&model, &planning, &core));

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
    }
}
