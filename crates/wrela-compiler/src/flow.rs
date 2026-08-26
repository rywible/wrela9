#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use xxhash_rust::xxh3::Xxh3;

use crate::compiler::Cancellation;
use crate::core::FlowCoreView;
use crate::image_planning::FlowPlanningInput;

const PHASE_SCHEMA: &str = "wrela.flow.v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FlowRequirementKind {
    ActorIdentity,
    PermanentCorePlacement,
    MailboxCapacity,
    TurnLease,
    SuspensionHome,
    LogicalCommitOrder,
    ProposalTransport,
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
        }
    }
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

impl FlowEventKind {
    const fn tag(self) -> u8 {
        match self {
            Self::TurnStarted => 1,
            Self::TurnSuspended => 2,
            Self::TurnResumed => 3,
            Self::TurnCompleted => 4,
            Self::MessageProposed => 5,
            Self::MessageFull => 6,
            Self::MailboxTransferCommitted => 7,
        }
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SuspensionHome {
    identity: u128,
    actor: u128,
    handler: u128,
    suspension_reference: u128,
    suspension_current_meaning: u128,
    control_path: Arc<[u32]>,
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
    send_ordinal: u32,
    control_path: Arc<[u32]>,
    resource_custody: Arc<[FlowResourceCustody]>,
    source: crate::SourceRange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RawProposal {
    template: u128,
    key: FlowProposalKey,
    arrival_ordinal: u32,
    operation_reference: u128,
    operation_current_meaning: u128,
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ModelResult {
    trace: Arc<[FlowEvent]>,
    proposals: Arc<[FlowProposal]>,
    agrees: bool,
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
    trace: Arc<[FlowEvent]>,
    proposals: Arc<[FlowProposal]>,
    model_contract: ModelContract,
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowSuspensionHomeObservation {
    identity: u128,
    actor: u128,
    handler: u128,
    suspension_reference: u128,
    suspension_current_meaning: u128,
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
    send_ordinal: u32,
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
    pub const fn send_ordinal(&self) -> u32 {
        self.send_ordinal
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
    trace: Arc<[FlowTraceRecord]>,
    proposals: Arc<[FlowProposalObservation]>,
    model_case_count: usize,
    model_agrees: bool,
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
    pub fn trace(&self) -> &[FlowTraceRecord] {
        &self.trace
    }

    #[must_use]
    pub fn proposals(&self) -> &[FlowProposalObservation] {
        &self.proposals
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
                    send_ordinal: template.send_ordinal,
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
            trace: self
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
            proposals: self
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
            model_case_count: self.model.trace.len() + self.model.proposals.len(),
            model_agrees: self.model.agrees,
        })
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FlowModule;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FlowFailure {
    Cancelled,
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
        let templates = proposal_templates(&actors, core.message_proposals(), cancellation)?;
        let raw_proposals = representative_proposals(&templates, cancellation)?;
        let proposals = arbitrate_proposals(&raw_proposals, &actors, cancellation)?;
        let trace = produce_trace(&actors, &homes, &templates, &proposals, cancellation)?;
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
        };
        let model_proposals = execute_independent_arbitration(&model_contract, cancellation)?;
        let model_trace =
            execute_independent_model(&model_contract, &model_proposals, cancellation)?;
        let model = ModelResult {
            agrees: model_trace.as_ref() == trace.as_ref()
                && model_proposals.as_ref() == proposals.as_ref(),
            trace: model_trace,
            proposals: model_proposals,
        };
        if !model.agrees {
            return defect("Flow graph disagrees with compact independent model");
        }
        let fingerprint = fingerprint(
            FlowFingerprintInput {
                context: planning.context_identity(),
                planning: planning.fingerprint(),
                core: core.fingerprint(),
                actors: &actors,
                requirements: &requirements,
                homes: &homes,
                templates: &templates,
                trace: &trace,
                proposals: &proposals,
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
            trace,
            proposals,
            model_contract,
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
    hash.update(&handler.unwrap_or(0).to_be_bytes());
    hash.update(&site.unwrap_or(0).to_be_bytes());
    hash.digest128()
}

fn suspension_home_identity(actor: u128, handler: u128, suspension_reference: u128) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(b"wrela.flow.suspension-home\0\x01");
    hash.update(&actor.to_be_bytes());
    hash.update(&handler.to_be_bytes());
    hash.update(&suspension_reference.to_be_bytes());
    hash.digest128()
}

fn proposal_templates(
    actors: &[Actor],
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
            for destination in destinations.iter().copied() {
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
                        proposal_home: resource.proposal_home,
                    })
                    .collect::<Vec<_>>();
                let mut meaning = Xxh3::new();
                meaning.update(b"wrela.flow.proposal-template-meaning\0\x01");
                meaning.update(&identity.to_be_bytes());
                meaning.update(&message.operation_current_meaning.to_be_bytes());
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
                    send_ordinal: message.send_ordinal,
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

fn representative_proposals(
    templates: &[ProposalTemplate],
    cancellation: &Cancellation,
) -> Result<Arc<[RawProposal]>, FlowFailure> {
    let mut eligible_paths = BTreeMap::<u128, Arc<[u32]>>::new();
    for template in templates {
        checkpoint(cancellation)?;
        if !template.control_path.is_empty() {
            eligible_paths
                .entry(template.sender)
                .and_modify(|path| {
                    if template.control_path.as_ref() < path.as_ref() {
                        *path = Arc::clone(&template.control_path);
                    }
                })
                .or_insert_with(|| Arc::clone(&template.control_path));
        }
    }
    let mut proposals = templates
        .iter()
        .filter(|template| {
            template.control_path.is_empty()
                || eligible_paths.get(&template.sender) == Some(&template.control_path)
        })
        .map(|template| RawProposal {
            template: template.identity,
            key: FlowProposalKey {
                destination: template.destination,
                sender: template.sender,
                sender_turn_sequence: 0,
                send_ordinal: template.send_ordinal,
            },
            arrival_ordinal: 0,
            operation_reference: template.identity,
            operation_current_meaning: template.current_meaning,
            control_path: Arc::clone(&template.control_path),
            resource_custody: Arc::clone(&template.resource_custody),
            source: template.source.clone(),
        })
        .collect::<Vec<_>>();
    if proposals.len() == 1 {
        let mut next_turn = proposals[0].clone();
        next_turn.key.sender_turn_sequence = 1;
        proposals.push(next_turn);
    }
    proposals.sort_by_key(|proposal| proposal.key);
    let count = u32::try_from(proposals.len()).unwrap_or(u32::MAX);
    for (index, proposal) in proposals.iter_mut().enumerate() {
        proposal.arrival_ordinal = count
            .saturating_sub(1)
            .saturating_sub(u32::try_from(index).unwrap_or(u32::MAX));
    }
    Ok(proposals.into())
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
        let admitted = *occupancy < capacity;
        let transfer_commit = admitted.then_some(commit_sequence);
        if admitted {
            *occupancy = occupancy.saturating_add(1);
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

fn execute_independent_arbitration(
    contract: &ModelContract,
    cancellation: &Cancellation,
) -> Result<Arc<[FlowProposal]>, FlowFailure> {
    let capacities = contract
        .mailbox_capacities
        .iter()
        .copied()
        .collect::<BTreeMap<_, _>>();
    let mut branch_choice = BTreeMap::<u128, Arc<[u32]>>::new();
    for template in contract.templates.iter() {
        if !template.control_path.is_empty() {
            let choice = branch_choice
                .entry(template.sender)
                .or_insert_with(|| Arc::clone(&template.control_path));
            if template.control_path.as_ref() < choice.as_ref() {
                *choice = Arc::clone(&template.control_path);
            }
        }
    }
    let mut runtime = contract
        .templates
        .iter()
        .filter(|template| {
            template.control_path.is_empty()
                || branch_choice.get(&template.sender) == Some(&template.control_path)
        })
        .map(|template| RawProposal {
            template: template.identity,
            key: FlowProposalKey {
                destination: template.destination,
                sender: template.sender,
                sender_turn_sequence: 0,
                send_ordinal: template.send_ordinal,
            },
            arrival_ordinal: 0,
            operation_reference: template.identity,
            operation_current_meaning: template.current_meaning,
            control_path: Arc::clone(&template.control_path),
            resource_custody: Arc::clone(&template.resource_custody),
            source: template.source.clone(),
        })
        .collect::<Vec<_>>();
    if runtime.len() == 1 {
        let mut activation = runtime[0].clone();
        activation.key.sender_turn_sequence = 1;
        runtime.push(activation);
    }
    runtime.sort_by_key(|proposal| proposal.key);
    let runtime_count = u32::try_from(runtime.len()).unwrap_or(u32::MAX);
    for (index, proposal) in runtime.iter_mut().enumerate() {
        proposal.arrival_ordinal = runtime_count
            .saturating_sub(1)
            .saturating_sub(u32::try_from(index).unwrap_or(u32::MAX));
    }
    let mut remaining = runtime.iter().collect::<Vec<_>>();
    let mut occupancy = BTreeMap::<u128, u64>::new();
    let mut results = Vec::with_capacity(remaining.len());
    let mut commit = 0_u64;
    while !remaining.is_empty() {
        checkpoint(cancellation)?;
        let (index, proposal) = remaining
            .iter()
            .enumerate()
            .min_by_key(|(_, proposal)| proposal.key)
            .map(|(index, proposal)| (index, *proposal))
            .ok_or_else(|| FlowFailure::Defect(Arc::from("model proposal set vanished")))?;
        remaining.remove(index);
        let current = occupancy.entry(proposal.key.destination).or_default();
        let admitted = *current
            < capacities
                .get(&proposal.key.destination)
                .copied()
                .unwrap_or(0);
        let transfer_commit = admitted.then_some(commit);
        if admitted {
            *current = current.saturating_add(1);
            commit = commit.saturating_add(1);
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

fn produce_trace(
    actors: &[Actor],
    homes: &[SuspensionHome],
    templates: &[ProposalTemplate],
    proposals: &[FlowProposal],
    cancellation: &Cancellation,
) -> Result<Arc<[FlowEvent]>, FlowFailure> {
    let homes = homes.iter().fold(BTreeMap::new(), |mut selected, home| {
        selected
            .entry((home.actor, home.handler))
            .and_modify(|current: &mut &SuspensionHome| {
                if (&home.control_path, home.identity) < (&current.control_path, current.identity) {
                    *current = home;
                }
            })
            .or_insert(home);
        selected
    });
    let templates = templates
        .iter()
        .map(|template| (template.identity, template))
        .collect::<BTreeMap<_, _>>();
    let mut events = Vec::new();
    let mut exercised = BTreeSet::new();
    for proposal in proposals {
        checkpoint(cancellation)?;
        let template = templates.get(&proposal.template).ok_or_else(|| {
            FlowFailure::Defect(Arc::from("runtime proposal has no static template"))
        })?;
        let started = u64::try_from(events.len()).unwrap_or(u64::MAX);
        events.push(FlowEvent {
            sequence: started,
            program_order: 0,
            causal_predecessor: None,
            logical_commit: None,
            kind: FlowEventKind::TurnStarted,
            actor: proposal.key.sender,
            handler: template.sender_handler,
            turn_sequence: proposal.key.sender_turn_sequence,
            proposal: Some(proposal.key),
            suspension_home: None,
        });
        exercised.insert((proposal.key.sender, template.sender_handler));
        let proposed = u64::try_from(events.len()).unwrap_or(u64::MAX);
        events.push(FlowEvent {
            sequence: proposed,
            program_order: u64::from(proposal.key.send_ordinal),
            causal_predecessor: Some(started),
            logical_commit: None,
            kind: FlowEventKind::MessageProposed,
            actor: proposal.key.sender,
            handler: template.sender_handler,
            turn_sequence: proposal.key.sender_turn_sequence,
            proposal: Some(proposal.key),
            suspension_home: None,
        });
        let outcome = u64::try_from(events.len()).unwrap_or(u64::MAX);
        match proposal.outcome {
            FlowSendOutcome::Full => events.push(FlowEvent {
                sequence: outcome,
                program_order: u64::from(proposal.key.send_ordinal),
                causal_predecessor: Some(proposed),
                logical_commit: None,
                kind: FlowEventKind::MessageFull,
                actor: proposal.key.sender,
                handler: template.sender_handler,
                turn_sequence: proposal.key.sender_turn_sequence,
                proposal: Some(proposal.key),
                suspension_home: None,
            }),
            FlowSendOutcome::Admitted => {
                events.push(FlowEvent {
                    sequence: outcome,
                    program_order: u64::from(proposal.key.send_ordinal),
                    causal_predecessor: Some(proposed),
                    logical_commit: proposal.transfer_commit,
                    kind: FlowEventKind::MailboxTransferCommitted,
                    actor: proposal.key.destination,
                    handler: template.destination_handler,
                    turn_sequence: proposal.transfer_commit.unwrap_or(0),
                    proposal: Some(proposal.key),
                    suspension_home: None,
                });
                let receiver_started = u64::try_from(events.len()).unwrap_or(u64::MAX);
                events.push(FlowEvent {
                    sequence: receiver_started,
                    program_order: 0,
                    causal_predecessor: Some(outcome),
                    logical_commit: proposal.transfer_commit,
                    kind: FlowEventKind::TurnStarted,
                    actor: proposal.key.destination,
                    handler: template.destination_handler,
                    turn_sequence: proposal.transfer_commit.unwrap_or(0),
                    proposal: Some(proposal.key),
                    suspension_home: None,
                });
                exercised.insert((proposal.key.destination, template.destination_handler));
                let mut previous = receiver_started;
                if let Some(home) =
                    homes.get(&(proposal.key.destination, template.destination_handler))
                {
                    for kind in [FlowEventKind::TurnSuspended, FlowEventKind::TurnResumed] {
                        let sequence = u64::try_from(events.len()).unwrap_or(u64::MAX);
                        events.push(FlowEvent {
                            sequence,
                            program_order: 0,
                            causal_predecessor: Some(previous),
                            logical_commit: proposal.transfer_commit,
                            kind,
                            actor: proposal.key.destination,
                            handler: template.destination_handler,
                            turn_sequence: proposal.transfer_commit.unwrap_or(0),
                            proposal: Some(proposal.key),
                            suspension_home: Some(home.identity),
                        });
                        previous = sequence;
                    }
                }
                let sequence = u64::try_from(events.len()).unwrap_or(u64::MAX);
                events.push(FlowEvent {
                    sequence,
                    program_order: 0,
                    causal_predecessor: Some(previous),
                    logical_commit: proposal.transfer_commit,
                    kind: FlowEventKind::TurnCompleted,
                    actor: proposal.key.destination,
                    handler: template.destination_handler,
                    turn_sequence: proposal.transfer_commit.unwrap_or(0),
                    proposal: Some(proposal.key),
                    suspension_home: None,
                });
            }
        }
        let completed = u64::try_from(events.len()).unwrap_or(u64::MAX);
        events.push(FlowEvent {
            sequence: completed,
            program_order: u64::from(proposal.key.send_ordinal).saturating_add(1),
            causal_predecessor: Some(outcome),
            logical_commit: proposal.transfer_commit,
            kind: FlowEventKind::TurnCompleted,
            actor: proposal.key.sender,
            handler: template.sender_handler,
            turn_sequence: proposal.key.sender_turn_sequence,
            proposal: Some(proposal.key),
            suspension_home: None,
        });
    }
    for actor in actors {
        for handler in actor.handlers.iter().copied() {
            checkpoint(cancellation)?;
            if exercised.contains(&(actor.identity, handler)) {
                continue;
            }
            let Some(home) = homes
                .get(&(actor.identity, handler))
                .map(|home| home.identity)
            else {
                continue;
            };
            for (kind, suspension_home) in [
                (FlowEventKind::TurnStarted, None),
                (FlowEventKind::TurnSuspended, Some(home)),
                (FlowEventKind::TurnResumed, Some(home)),
                (FlowEventKind::TurnCompleted, None),
            ] {
                events.push(FlowEvent {
                    sequence: u64::try_from(events.len()).unwrap_or(u64::MAX),
                    program_order: 0,
                    causal_predecessor: events.last().map(|event| event.sequence),
                    logical_commit: None,
                    kind,
                    actor: actor.identity,
                    handler,
                    turn_sequence: 0,
                    proposal: None,
                    suspension_home,
                });
            }
        }
    }
    Ok(events.into())
}

fn execute_independent_model(
    contract: &ModelContract,
    proposals: &[FlowProposal],
    cancellation: &Cancellation,
) -> Result<Arc<[FlowEvent]>, FlowFailure> {
    let selected_homes = contract.suspension_homes.iter().fold(
        BTreeMap::<(u128, u128), &SuspensionHome>::new(),
        |mut selected, home| {
            selected
                .entry((home.actor, home.handler))
                .and_modify(|current| {
                    if (&home.control_path, home.identity)
                        < (&current.control_path, current.identity)
                    {
                        *current = home;
                    }
                })
                .or_insert(home);
            selected
        },
    );
    let templates = contract
        .templates
        .iter()
        .map(|template| (template.identity, template))
        .collect::<BTreeMap<_, _>>();
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
    for proposal in proposals {
        checkpoint(cancellation)?;
        let template = templates.get(&proposal.template).ok_or_else(|| {
            FlowFailure::Defect(Arc::from("model runtime proposal has no template"))
        })?;
        let start = append(
            &mut observations,
            FlowEventKind::TurnStarted,
            proposal.key.sender,
            template.sender_handler,
            proposal.key.sender_turn_sequence,
            Some(proposal.key),
            None,
            None,
            None,
            0,
        );
        exercised.insert((proposal.key.sender, template.sender_handler));
        let proposed = append(
            &mut observations,
            FlowEventKind::MessageProposed,
            proposal.key.sender,
            template.sender_handler,
            proposal.key.sender_turn_sequence,
            Some(proposal.key),
            None,
            Some(start),
            None,
            u64::from(proposal.key.send_ordinal),
        );
        let outcome = match proposal.outcome {
            FlowSendOutcome::Full => append(
                &mut observations,
                FlowEventKind::MessageFull,
                proposal.key.sender,
                template.sender_handler,
                proposal.key.sender_turn_sequence,
                Some(proposal.key),
                None,
                Some(proposed),
                None,
                u64::from(proposal.key.send_ordinal),
            ),
            FlowSendOutcome::Admitted => {
                let committed = append(
                    &mut observations,
                    FlowEventKind::MailboxTransferCommitted,
                    proposal.key.destination,
                    template.destination_handler,
                    proposal.transfer_commit.unwrap_or(0),
                    Some(proposal.key),
                    None,
                    Some(proposed),
                    proposal.transfer_commit,
                    u64::from(proposal.key.send_ordinal),
                );
                let receiver_start = append(
                    &mut observations,
                    FlowEventKind::TurnStarted,
                    proposal.key.destination,
                    template.destination_handler,
                    proposal.transfer_commit.unwrap_or(0),
                    Some(proposal.key),
                    None,
                    Some(committed),
                    proposal.transfer_commit,
                    0,
                );
                exercised.insert((proposal.key.destination, template.destination_handler));
                let mut causal = receiver_start;
                if let Some(home) =
                    selected_homes.get(&(proposal.key.destination, template.destination_handler))
                {
                    causal = append(
                        &mut observations,
                        FlowEventKind::TurnSuspended,
                        proposal.key.destination,
                        template.destination_handler,
                        proposal.transfer_commit.unwrap_or(0),
                        Some(proposal.key),
                        Some(home.identity),
                        Some(causal),
                        proposal.transfer_commit,
                        0,
                    );
                    causal = append(
                        &mut observations,
                        FlowEventKind::TurnResumed,
                        proposal.key.destination,
                        template.destination_handler,
                        proposal.transfer_commit.unwrap_or(0),
                        Some(proposal.key),
                        Some(home.identity),
                        Some(causal),
                        proposal.transfer_commit,
                        0,
                    );
                }
                append(
                    &mut observations,
                    FlowEventKind::TurnCompleted,
                    proposal.key.destination,
                    template.destination_handler,
                    proposal.transfer_commit.unwrap_or(0),
                    Some(proposal.key),
                    None,
                    Some(causal),
                    proposal.transfer_commit,
                    0,
                );
                committed
            }
        };
        append(
            &mut observations,
            FlowEventKind::TurnCompleted,
            proposal.key.sender,
            template.sender_handler,
            proposal.key.sender_turn_sequence,
            Some(proposal.key),
            None,
            Some(outcome),
            proposal.transfer_commit,
            u64::from(proposal.key.send_ordinal).saturating_add(1),
        );
    }
    for (actor, handlers) in contract.actors.iter() {
        for handler in handlers
            .iter()
            .copied()
            .filter(|handler| selected_homes.contains_key(&(*actor, *handler)))
        {
            checkpoint(cancellation)?;
            if exercised.contains(&(*actor, handler)) {
                continue;
            }
            let home = selected_homes
                .get(&(*actor, handler))
                .map(|home| home.identity)
                .ok_or_else(|| FlowFailure::Defect(Arc::from("model suspension has no Home")))?;
            for (kind, suspension_home) in [
                (FlowEventKind::TurnStarted, None),
                (FlowEventKind::TurnSuspended, Some(home)),
                (FlowEventKind::TurnResumed, Some(home)),
                (FlowEventKind::TurnCompleted, None),
            ] {
                let causal = observations.last().map(|event| event.sequence);
                append(
                    &mut observations,
                    kind,
                    *actor,
                    handler,
                    0,
                    None,
                    suspension_home,
                    causal,
                    None,
                    0,
                );
            }
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
    expected_requirements.sort_by_key(|requirement| requirement.identity);
    if expected_requirements.as_slice() != candidate.requirements.as_ref() {
        return defect("Flow Planning Requirement roster is missing, extra, or repointed");
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
    let expected_homes = expected_actors
        .iter()
        .flat_map(|actor| {
            suspension_sites
                .iter()
                .filter(|site| actor.handlers.contains(&site.handler))
                .map(|site| {
                    (
                        actor.identity,
                        site.handler,
                        site.reference_identity,
                        site.reference_current_meaning,
                        Arc::clone(&site.control_path),
                        site.source.clone(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect::<BTreeSet<_>>();
    let supplied_homes = candidate
        .suspension_homes
        .iter()
        .map(|home| {
            (
                home.actor,
                home.handler,
                home.suspension_reference,
                home.suspension_current_meaning,
                Arc::clone(&home.control_path),
                home.source.clone(),
            )
        })
        .collect::<BTreeSet<_>>();
    if expected_homes != supplied_homes || expected_homes.len() != candidate.suspension_homes.len()
    {
        return defect("Flow suspension has no exact static Suspension Home");
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
    verify_non_reentrant_trace(&candidate.trace, cancellation)?;
    let expected_trace = produce_trace(
        &candidate.actors,
        &candidate.suspension_homes,
        &candidate.proposal_templates,
        &candidate.proposals,
        cancellation,
    )?;
    if expected_trace != candidate.trace {
        return defect("Flow typed trace is not the canonical graph projection");
    }
    let expected_templates =
        proposal_templates(&expected_actors, core.message_proposals(), cancellation)?;
    if expected_templates != candidate.model_contract.templates
        || expected_templates != candidate.proposal_templates
    {
        return defect("Flow model contract disagrees with Core message proposals");
    }
    let model_proposals = execute_independent_arbitration(&candidate.model_contract, cancellation)?;
    let model_trace =
        execute_independent_model(&candidate.model_contract, &model_proposals, cancellation)?;
    if model_trace != candidate.model.trace
        || model_proposals != candidate.model.proposals
        || candidate.model.trace != candidate.trace
        || candidate.model.proposals != candidate.proposals
        || !candidate.model.agrees
    {
        return defect("Flow compact model and typed trace disagree");
    }
    verify_proposals(candidate, cancellation)?;
    let expected_fingerprint = fingerprint(
        FlowFingerprintInput {
            context: candidate.context,
            planning: candidate.planning_fingerprint,
            core: candidate.core_fingerprint,
            actors: &candidate.actors,
            requirements: &candidate.requirements,
            homes: &candidate.suspension_homes,
            templates: &candidate.proposal_templates,
            trace: &candidate.trace,
            proposals: &candidate.proposals,
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
    cancellation: &Cancellation,
) -> Result<(), FlowFailure> {
    let exact_custody =
        representative_proposals(&candidate.model_contract.templates, cancellation)?
            .iter()
            .map(|proposal| (proposal.key, Arc::clone(&proposal.resource_custody)))
            .collect::<BTreeMap<_, _>>();
    let mut previous = None;
    let mut mailbox_resources = BTreeSet::new();
    let mut proposal_resources = BTreeSet::new();
    for proposal in candidate.proposals.iter() {
        checkpoint(cancellation)?;
        if previous.is_some_and(|key| key >= proposal.key) {
            return defect("Flow proposals are not in canonical logical order");
        }
        previous = Some(proposal.key);
        if proposal.before_commit != FlowCustodian::ProposalHome {
            return defect("Flow proposal lost pre-commit Resource custody");
        }
        if exact_custody.get(&proposal.key).map(Arc::as_ref)
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
    for proposal in candidate
        .proposals
        .iter()
        .filter(|proposal| proposal.outcome == FlowSendOutcome::Admitted)
    {
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
        }
    }
    if !active.is_empty() {
        return defect("Flow trace leaves an Actor Turn active");
    }
    Ok(())
}

struct FlowFingerprintInput<'a> {
    context: u128,
    planning: u128,
    core: u128,
    actors: &'a [Actor],
    requirements: &'a [FlowRequirement],
    homes: &'a [SuspensionHome],
    templates: &'a [ProposalTemplate],
    trace: &'a [FlowEvent],
    proposals: &'a [FlowProposal],
}

fn fingerprint(
    input: FlowFingerprintInput<'_>,
    cancellation: &Cancellation,
) -> Result<u128, FlowFailure> {
    let FlowFingerprintInput {
        context,
        planning,
        core,
        actors,
        requirements,
        homes,
        templates,
        trace,
        proposals,
    } = input;
    let mut hash = Xxh3::new();
    hash.update(b"wrela.verified-flow-program\0\x01");
    hash.update(&context.to_be_bytes());
    hash.update(&planning.to_be_bytes());
    hash.update(&core.to_be_bytes());
    for actor in actors {
        checkpoint(cancellation)?;
        hash.update(&actor.identity.to_be_bytes());
        hash.update(&actor.actor_type_identity.to_be_bytes());
        hash.update(&actor.construction_identity.to_be_bytes());
        hash.update(&actor.construction_current_meaning.to_be_bytes());
        hash.update(&actor.mailbox_capacity.to_be_bytes());
        hash.update(&[actor.max_active_turns]);
        hash.update(&actor.permanent_core_requirement.to_be_bytes());
        for handler in actor.handlers.iter() {
            hash.update(&handler.to_be_bytes());
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
        hash.update(&home.handler.to_be_bytes());
        hash.update(&home.suspension_reference.to_be_bytes());
        hash.update(&home.suspension_current_meaning.to_be_bytes());
        hash.update(home.source.path().as_bytes());
        hash.update(&home.source.start().to_be_bytes());
        hash.update(&home.source.end().to_be_bytes());
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
        hash.update(&template.sender_handler.to_be_bytes());
        hash.update(&template.destination.to_be_bytes());
        hash.update(&template.destination_handler.to_be_bytes());
        hash.update(&template.send_ordinal.to_be_bytes());
        for part in template.control_path.iter() {
            hash.update(&part.to_be_bytes());
        }
        for resource in template.resource_custody.iter() {
            hash.update(&resource.core_reference_identity.to_be_bytes());
            hash.update(&resource.core_reference_current_meaning.to_be_bytes());
            hash.update(&resource.type_identity.to_be_bytes());
            hash.update(&resource.source_home.to_be_bytes());
            hash.update(&resource.proposal_home.to_be_bytes());
            for part in resource.place.iter() {
                hash.update(&part.to_be_bytes());
            }
        }
    }
    for event in trace {
        checkpoint(cancellation)?;
        hash.update(&event.sequence.to_be_bytes());
        hash.update(&event.program_order.to_be_bytes());
        hash.update(&event.causal_predecessor.unwrap_or(u64::MAX).to_be_bytes());
        hash.update(&event.logical_commit.unwrap_or(u64::MAX).to_be_bytes());
        hash.update(&[event.kind.tag()]);
        hash.update(&event.actor.to_be_bytes());
        hash.update(&event.handler.to_be_bytes());
        hash.update(&event.turn_sequence.to_be_bytes());
        if let Some(proposal) = event.proposal {
            hash.update(&proposal.destination.to_be_bytes());
            hash.update(&proposal.sender.to_be_bytes());
            hash.update(&proposal.sender_turn_sequence.to_be_bytes());
            hash.update(&proposal.send_ordinal.to_be_bytes());
        }
        hash.update(&event.suspension_home.unwrap_or(0).to_be_bytes());
    }
    for proposal in proposals {
        checkpoint(cancellation)?;
        hash.update(&proposal.key.destination.to_be_bytes());
        hash.update(&proposal.key.sender.to_be_bytes());
        hash.update(&proposal.key.sender_turn_sequence.to_be_bytes());
        hash.update(&proposal.key.send_ordinal.to_be_bytes());
        hash.update(&proposal.arrival_ordinal.to_be_bytes());
        hash.update(&[match proposal.outcome {
            FlowSendOutcome::Admitted => 1,
            FlowSendOutcome::Full => 2,
        }]);
        hash.update(&[match proposal.after_arbitration {
            FlowCustodian::ProposalHome => 1,
            FlowCustodian::Mailbox => 2,
        }]);
        hash.update(&proposal.transfer_commit.unwrap_or(u64::MAX).to_be_bytes());
        for resource in proposal.resource_custody.iter() {
            hash.update(&resource.core_reference_identity.to_be_bytes());
            hash.update(&resource.core_reference_current_meaning.to_be_bytes());
            hash.update(&resource.type_identity.to_be_bytes());
            hash.update(&resource.source_home.to_be_bytes());
            hash.update(&resource.proposal_home.to_be_bytes());
            for part in resource.place.iter() {
                hash.update(&part.to_be_bytes());
            }
        }
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

    fn fixture() -> (
        VerifiedFlowProgram,
        Arc<VerifiedPlanningFoundation>,
        Arc<VerifiedCoreProgram>,
    ) {
        let request = CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", SOURCE)]),
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

    fn resign(candidate: &mut VerifiedFlowProgram) {
        candidate.fingerprint = fingerprint(
            FlowFingerprintInput {
                context: candidate.context,
                planning: candidate.planning_fingerprint,
                core: candidate.core_fingerprint,
                actors: &candidate.actors,
                requirements: &candidate.requirements,
                homes: &candidate.suspension_homes,
                templates: &candidate.proposal_templates,
                trace: &candidate.trace,
                proposals: &candidate.proposals,
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
        Arc::make_mut(&mut candidate.proposals).swap(0, 1);
        resign(&mut candidate);
        assert!(rejected(&candidate, &planning, &core));
    }

    #[test]
    fn verifier_rejects_single_fault_turn_reentrancy_corruption() {
        let (mut candidate, planning, core) = fixture();
        Arc::make_mut(&mut candidate.trace)[1].kind = FlowEventKind::TurnStarted;
        resign(&mut candidate);
        assert!(rejected(&candidate, &planning, &core));
    }

    #[test]
    fn verifier_rejects_single_fault_resource_custody_corruption() {
        let (mut candidate, planning, core) = fixture();
        let admitted = Arc::make_mut(&mut candidate.proposals)
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
