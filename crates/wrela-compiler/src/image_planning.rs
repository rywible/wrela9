#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use xxhash_rust::xxh3::Xxh3;

use crate::architecture_planning::{
    BindingKind, FacilitySharedRoleKind, ReservationKind, ReservationMultiplicity,
    VerifiedArchitecturePlanningContract, VmAbiCapability,
};
use crate::completed_semantic::{
    CompletedSemanticProgram, CorePlanningSemanticProgram, CoreSourceExecutableBody,
    CoreSourceExecutableRef, PlanningConstructionValueRef,
};
use crate::core::VerifiedCoreProgram;
use crate::flow::{FlowRequirementKind, FlowRequirementRef, VerifiedFlowProgram};
use crate::model::{BuildKind, SpecializationId};
use crate::typed_hir::{
    AccessMode, CallTarget, Expression, ExpressionKind, HirMatchCase, Literal, LocalId,
    PoolOperation, Statement, VerifiedProgram, root_place,
};
use crate::{Cancellation, Root, SourceRange};

pub(crate) const PHASE_SCHEMA: &str = "wrela.image-planning-foundation.v1";
pub(crate) const WHOLE_IMAGE_ASSIGNMENT_SCHEMA: &str = "wrela.whole-image-assignment.v1";
const SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlannerKind {
    ImageKind,
    Facility(FacilityKind),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DomainPlanKind {
    MandatoryImage,
    Facility(FacilityKind),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FacilityKind {
    Display,
    Input,
    EventStore,
    MonotonicClock,
    Entropy,
    Telemetry,
}

impl FacilityKind {
    const fn tag(self) -> u8 {
        match self {
            Self::Display => 1,
            Self::Input => 2,
            Self::EventStore => 3,
            Self::MonotonicClock => 4,
            Self::Entropy => 5,
            Self::Telemetry => 6,
        }
    }

    const fn source_name(self) -> &'static str {
        match self {
            Self::Display => "Display",
            Self::Input => "Input",
            Self::EventStore => "EventStore",
            Self::MonotonicClock => "MonotonicClock",
            Self::Entropy => "Entropy",
            Self::Telemetry => "Telemetry",
        }
    }
}

const FACILITY_KINDS: [FacilityKind; 6] = [
    FacilityKind::Display,
    FacilityKind::Input,
    FacilityKind::EventStore,
    FacilityKind::MonotonicClock,
    FacilityKind::Entropy,
    FacilityKind::Telemetry,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FacilityLossPolicy {
    ControlledShutdown,
    DisableAndContinue,
    SelectingImagePolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FacilityReplayAuthority {
    ReplayableGameplay,
    NonReplayableFacility,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FacilityReplayRule {
    ReplayableGameplay,
    ExcludedFromReplayableGameplay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FacilityFlagshipRule {
    Required { loss_policy: FacilityLossPolicy },
    SelectingImageOptional,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FacilityBindingAvailability {
    BootFailure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FacilityShutdown {
    Quiesce,
    FlushCommittedAndQuiesce,
    StopSampling,
    StopWakeups,
    DiscardPending,
    DropObservations,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FacilitySemanticCapacity {
    FrameBuffers(u32),
    InputTransitions(u32),
    EventSlots(u32),
    ClockWaiters(u32),
    EntropyRequestBytes(u32),
    TelemetryRingRecords(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FacilitySharing {
    Exclusive,
    RegisteredDisjoint {
        role: FacilitySharedRole,
        maximum_units: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FacilitySharedRole {
    MonotonicCounter,
    EntropyQueue,
}

impl FacilitySharedRole {
    const fn architecture_kind(self) -> FacilitySharedRoleKind {
        match self {
            Self::MonotonicCounter => FacilitySharedRoleKind::MonotonicCounter,
            Self::EntropyQueue => FacilitySharedRoleKind::EntropyQueue,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FacilityEndpointOwnership {
    FacilityInstance,
    BuildWiredActor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GeneratedRoleKind {
    Boot,
    Scheduler,
    Terminal,
    Panic,
    Shutdown,
    TestRuntime,
    DisplayDriver,
    InputDriver,
    EventStoreRuntime,
    EventStoreDriver,
    MonotonicClockDriver,
    EntropyDriver,
    TelemetryDriver,
}

impl GeneratedRoleKind {
    const fn tag(self) -> u8 {
        match self {
            Self::Boot => 1,
            Self::Scheduler => 2,
            Self::Terminal => 3,
            Self::Panic => 4,
            Self::Shutdown => 5,
            Self::TestRuntime => 6,
            Self::DisplayDriver => 7,
            Self::InputDriver => 8,
            Self::EventStoreRuntime => 9,
            Self::EventStoreDriver => 10,
            Self::MonotonicClockDriver => 11,
            Self::EntropyDriver => 12,
            Self::TelemetryDriver => 13,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequirementCategory {
    GeneratedRoleRealization,
    Lifetime,
    ArchitectureCapability,
    Cardinality,
    Service,
    Binding,
    LogicalLayout,
    CapacityPressure,
    FacilityOwnership,
    Recovery,
    Shutdown,
    Replay,
    Flagship,
    BootAvailability,
}

impl RequirementCategory {
    const fn tag(self) -> u8 {
        match self {
            Self::GeneratedRoleRealization => 1,
            Self::Lifetime => 2,
            Self::ArchitectureCapability => 3,
            Self::Cardinality => 4,
            Self::Service => 5,
            Self::Binding => 6,
            Self::LogicalLayout => 7,
            Self::CapacityPressure => 8,
            Self::FacilityOwnership => 9,
            Self::Recovery => 10,
            Self::Shutdown => 11,
            Self::Replay => 12,
            Self::Flagship => 13,
            Self::BootAvailability => 14,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlanningCapability {
    TypedTerminalLifecycle,
    PanicPulse,
    GuestShutdownPulse,
    SecondaryCoreStartup,
    PciVirtioModern,
    SplitVirtqueue,
    SharedIntx,
    DmaOwnership,
    MonotonicCounter,
}

impl PlanningCapability {
    const fn tag(self) -> u8 {
        match self {
            Self::TypedTerminalLifecycle => 1,
            Self::PanicPulse => 2,
            Self::GuestShutdownPulse => 3,
            Self::SecondaryCoreStartup => 4,
            Self::PciVirtioModern => 5,
            Self::SplitVirtqueue => 6,
            Self::SharedIntx => 7,
            Self::DmaOwnership => 8,
            Self::MonotonicCounter => 9,
        }
    }

    const fn contract_kind(self) -> VmAbiCapability {
        match self {
            Self::TypedTerminalLifecycle => VmAbiCapability::TypedTerminalLifecycle,
            Self::PanicPulse => VmAbiCapability::PanicPulse,
            Self::GuestShutdownPulse => VmAbiCapability::GuestShutdownPulse,
            Self::SecondaryCoreStartup => VmAbiCapability::SecondaryCoreStartup,
            Self::PciVirtioModern => VmAbiCapability::PciVirtioModern,
            Self::SplitVirtqueue => VmAbiCapability::SplitVirtqueue,
            Self::SharedIntx => VmAbiCapability::SharedIntx,
            Self::DmaOwnership => VmAbiCapability::DmaOwnership,
            Self::MonotonicCounter => VmAbiCapability::MonotonicCounter,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlanningBinding {
    Display,
    Input,
    EventStore,
    MonotonicClock,
    Entropy,
    Telemetry,
    Terminal,
    Panic,
}

impl PlanningBinding {
    const fn tag(self) -> u8 {
        match self {
            Self::Display => 1,
            Self::Input => 2,
            Self::EventStore => 3,
            Self::MonotonicClock => 4,
            Self::Entropy => 5,
            Self::Telemetry => 6,
            Self::Terminal => 7,
            Self::Panic => 8,
        }
    }

    const fn contract_kind(self) -> BindingKind {
        match self {
            Self::Display => BindingKind::Display,
            Self::Input => BindingKind::Input,
            Self::EventStore => BindingKind::EventStore,
            Self::MonotonicClock => BindingKind::MonotonicClock,
            Self::Entropy => BindingKind::Entropy,
            Self::Telemetry => BindingKind::Telemetry,
            Self::Terminal => BindingKind::Terminal,
            Self::Panic => BindingKind::Panic,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlanningReservation {
    BootState,
    TerminalTransport,
    PanicState,
}

impl PlanningReservation {
    const fn tag(self) -> u8 {
        match self {
            Self::BootState => 1,
            Self::TerminalTransport => 2,
            Self::PanicState => 3,
        }
    }

    const fn contract_kind(self) -> ReservationKind {
        match self {
            Self::BootState => ReservationKind::BootState,
            Self::TerminalTransport => ReservationKind::TerminalTransport,
            Self::PanicState => ReservationKind::PanicState,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlanningMultiplicity {
    Once,
    PerCore,
}

impl PlanningMultiplicity {
    const fn tag(self) -> u8 {
        match self {
            Self::Once => 1,
            Self::PerCore => 2,
        }
    }

    const fn contract_kind(self) -> ReservationMultiplicity {
        match self {
            Self::Once => ReservationMultiplicity::Once,
            Self::PerCore => ReservationMultiplicity::PerCore,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequirementBounds {
    RealizeExactlyOnce {
        executable: u128,
    },
    ImageLifetime,
    Capability(PlanningCapability),
    Cardinality {
        minimum: u32,
        maximum: u32,
    },
    MaximumServiceUnits(u32),
    Binding {
        kind: PlanningBinding,
        minimum: u32,
        maximum: u32,
    },
    Reservation {
        kind: PlanningReservation,
        multiplicity: PlanningMultiplicity,
    },
    PoolCapacity {
        declared: u64,
        usable: u64,
        peak_live: u64,
        peak_reserved: u64,
        peak_committed: u64,
    },
    FacilityCapacity(FacilitySemanticCapacity),
    FacilitySharing(FacilitySharing),
    FacilityEndpoint {
        maximum: u32,
        ownership: FacilityEndpointOwnership,
        input_owner: Option<FacilityActorRef>,
    },
    FacilityRecovery {
        supervisor: FacilityActorRef,
        loss_policy: FacilityLossPolicy,
        maximum_attempts: u16,
    },
    FacilityShutdown(FacilityShutdown),
    FacilityReplay {
        selected: FacilityReplayAuthority,
        rule: FacilityReplayRule,
    },
    FacilityFlagship(FacilityFlagshipRule),
    FacilityBindingAvailability(FacilityBindingAvailability),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequirementSubject {
    GeneratedRole(u128),
    Pool(u128),
    FacilityInstance(u128),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RequirementProvenance {
    domain_plan: u128,
    generated_role: u128,
    local_site: u16,
}

impl RequirementProvenance {
    #[must_use]
    pub const fn domain_plan(self) -> u128 {
        self.domain_plan
    }

    #[must_use]
    pub const fn generated_role(self) -> u128 {
        self.generated_role
    }

    #[must_use]
    pub const fn local_site(self) -> u16 {
        self.local_site
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FacilityContractObservation {
    kind: FacilityKind,
    identity: u128,
    context_receipt: u128,
    fingerprint: u128,
    current_meaning: u128,
    allows_deployment: bool,
    allows_test: bool,
    minimum_instances: u32,
    maximum_instances: u32,
    maximum_exported_endpoints: u32,
    endpoint_ownership: FacilityEndpointOwnership,
    flagship_rule: FacilityFlagshipRule,
    replay_rule: FacilityReplayRule,
    generated_roles: Arc<[GeneratedRoleKind]>,
    semantic_capacities: Arc<[FacilitySemanticCapacity]>,
    required_capabilities: Arc<[PlanningCapability]>,
    external_binding: Option<PlanningBinding>,
    sharing: FacilitySharing,
    loss_policy: FacilityLossPolicy,
    shutdown: FacilityShutdown,
    maximum_recovery_attempts: u16,
    binding_availability: FacilityBindingAvailability,
}

impl FacilityContractObservation {
    pub const fn kind(&self) -> FacilityKind {
        self.kind
    }
    pub const fn identity(&self) -> u128 {
        self.identity
    }
    pub const fn context_receipt(&self) -> u128 {
        self.context_receipt
    }
    pub const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }
    pub const fn current_meaning(&self) -> u128 {
        self.current_meaning
    }
    pub const fn allows_deployment(&self) -> bool {
        self.allows_deployment
    }
    pub const fn allows_test(&self) -> bool {
        self.allows_test
    }
    pub const fn minimum_instances(&self) -> u32 {
        self.minimum_instances
    }
    pub const fn maximum_instances(&self) -> u32 {
        self.maximum_instances
    }
    pub const fn maximum_exported_endpoints(&self) -> u32 {
        self.maximum_exported_endpoints
    }
    pub const fn endpoint_ownership(&self) -> FacilityEndpointOwnership {
        self.endpoint_ownership
    }
    pub const fn required_by_flagship(&self) -> bool {
        matches!(self.flagship_rule, FacilityFlagshipRule::Required { .. })
    }
    pub const fn allowed_in_replayable_gameplay(&self) -> bool {
        matches!(self.replay_rule, FacilityReplayRule::ReplayableGameplay)
    }
    pub const fn flagship_rule(&self) -> FacilityFlagshipRule {
        self.flagship_rule
    }
    pub const fn replay_rule(&self) -> FacilityReplayRule {
        self.replay_rule
    }
    pub fn generated_roles(&self) -> &[GeneratedRoleKind] {
        &self.generated_roles
    }
    pub fn semantic_capacities(&self) -> &[FacilitySemanticCapacity] {
        &self.semantic_capacities
    }
    pub fn required_capabilities(&self) -> &[PlanningCapability] {
        &self.required_capabilities
    }
    pub const fn external_binding(&self) -> Option<PlanningBinding> {
        self.external_binding
    }
    pub const fn sharing(&self) -> FacilitySharing {
        self.sharing
    }
    pub const fn physical_sharing_is_registered_disjoint_or_exclusive(&self) -> bool {
        match self.sharing {
            FacilitySharing::Exclusive => true,
            FacilitySharing::RegisteredDisjoint { maximum_units, .. } => maximum_units > 0,
        }
    }
    pub const fn loss_policy(&self) -> FacilityLossPolicy {
        self.loss_policy
    }
    pub const fn shutdown(&self) -> FacilityShutdown {
        self.shutdown
    }
    pub const fn maximum_recovery_attempts(&self) -> u16 {
        self.maximum_recovery_attempts
    }
    pub const fn ambient_binding_unavailability_is_boot_failure(&self) -> bool {
        matches!(
            self.binding_availability,
            FacilityBindingAvailability::BootFailure
        )
    }
    pub const fn binding_availability(&self) -> FacilityBindingAvailability {
        self.binding_availability
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FacilityDomainPlanObservation {
    identity: u128,
    kind: FacilityKind,
    instance_identity: u128,
    contract_fingerprint: u128,
    contract_identity: u128,
    contract_current_meaning: u128,
    current_meaning: u128,
    generated_role_count: usize,
    requirement_count: usize,
}

impl FacilityDomainPlanObservation {
    pub const fn identity(&self) -> u128 {
        self.identity
    }
    pub const fn kind(&self) -> FacilityKind {
        self.kind
    }
    pub const fn instance_identity(&self) -> u128 {
        self.instance_identity
    }
    pub const fn contract_fingerprint(&self) -> u128 {
        self.contract_fingerprint
    }
    pub const fn contract_identity(&self) -> u128 {
        self.contract_identity
    }
    pub const fn contract_current_meaning(&self) -> u128 {
        self.contract_current_meaning
    }
    pub const fn current_meaning(&self) -> u128 {
        self.current_meaning
    }
    pub const fn generated_role_count(&self) -> usize {
        self.generated_role_count
    }
    pub const fn requirement_count(&self) -> usize {
        self.requirement_count
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FacilityContract {
    reference: FacilityContractRef,
    observation: FacilityContractObservation,
    authentication: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FacilityContractRef {
    context: u128,
    identity: u128,
    current_meaning: u128,
    fingerprint: u128,
    kind: FacilityKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FacilityActorRef {
    context: u128,
    identity: u128,
    current_meaning: u128,
}

impl FacilityActorRef {
    #[must_use]
    pub const fn context(self) -> u128 {
        self.context
    }

    #[must_use]
    pub const fn identity(self) -> u128 {
        self.identity
    }

    #[must_use]
    pub const fn current_meaning(self) -> u128 {
        self.current_meaning
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FacilitySubjectRef {
    context: u128,
    identity: u128,
    current_meaning: u128,
    kind: FacilityKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FacilityInstanceRef {
    context: u128,
    identity: u128,
    current_meaning: u128,
    kind: FacilityKind,
    source: SourceRange,
    supervisor: FacilityActorRef,
    input_owner: Option<FacilityActorRef>,
    selected_loss_policy: FacilityLossPolicy,
    replay_authority: FacilityReplayAuthority,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannerObservation {
    identity: u128,
    kind: PlannerKind,
    current_meaning: u128,
}

impl PlannerObservation {
    #[must_use]
    pub const fn identity(&self) -> u128 {
        self.identity
    }

    #[must_use]
    pub const fn kind(&self) -> PlannerKind {
        self.kind
    }

    #[must_use]
    pub const fn current_meaning(&self) -> u128 {
        self.current_meaning
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainPlanObservation {
    identity: u128,
    planner: u128,
    kind: DomainPlanKind,
    current_meaning: u128,
    generated_role_count: usize,
    requirement_count: usize,
}

impl DomainPlanObservation {
    #[must_use]
    pub const fn identity(&self) -> u128 {
        self.identity
    }

    #[must_use]
    pub const fn planner(&self) -> u128 {
        self.planner
    }

    #[must_use]
    pub const fn kind(&self) -> DomainPlanKind {
        self.kind
    }

    #[must_use]
    pub const fn current_meaning(&self) -> u128 {
        self.current_meaning
    }

    #[must_use]
    pub const fn generated_role_count(&self) -> usize {
        self.generated_role_count
    }

    #[must_use]
    pub const fn requirement_count(&self) -> usize {
        self.requirement_count
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedRoleObservation {
    identity: u128,
    executable: u128,
    owner: u128,
    generator: u128,
    kind: GeneratedRoleKind,
    local_key: u16,
    dependencies: Arc<[u128]>,
    provenance: u128,
    current_meaning: u128,
}

impl GeneratedRoleObservation {
    #[must_use]
    pub const fn identity(&self) -> u128 {
        self.identity
    }

    #[must_use]
    pub const fn executable(&self) -> u128 {
        self.executable
    }

    #[must_use]
    pub const fn owner(&self) -> u128 {
        self.owner
    }

    #[must_use]
    pub const fn generator(&self) -> u128 {
        self.generator
    }

    #[must_use]
    pub const fn kind(&self) -> GeneratedRoleKind {
        self.kind
    }

    #[must_use]
    pub const fn local_key(&self) -> u16 {
        self.local_key
    }

    #[must_use]
    pub fn dependencies(&self) -> &[u128] {
        &self.dependencies
    }

    #[must_use]
    pub const fn provenance(&self) -> u128 {
        self.provenance
    }

    #[must_use]
    pub const fn current_meaning(&self) -> u128 {
        self.current_meaning
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequirementObservation {
    reference: u128,
    owner: u128,
    subject: RequirementSubject,
    provenance: RequirementProvenance,
    category: RequirementCategory,
    bounds: RequirementBounds,
    current_meaning: u128,
}

impl RequirementObservation {
    #[must_use]
    pub const fn reference(&self) -> u128 {
        self.reference
    }

    #[must_use]
    pub const fn owner(&self) -> u128 {
        self.owner
    }

    #[must_use]
    pub const fn subject(&self) -> RequirementSubject {
        self.subject
    }

    #[must_use]
    pub const fn provenance(&self) -> RequirementProvenance {
        self.provenance
    }

    #[must_use]
    pub const fn category(&self) -> RequirementCategory {
        self.category
    }

    #[must_use]
    pub const fn bounds(&self) -> &RequirementBounds {
        &self.bounds
    }

    #[must_use]
    pub const fn current_meaning(&self) -> u128 {
        self.current_meaning
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoolObservation {
    identity: u128,
    current_meaning: u128,
    source: SourceRange,
    declared_capacity: u64,
    usable_slots: u64,
    peak_live_allocations: u64,
    peak_outstanding_permits: u64,
    peak_commitment: u64,
}

impl PoolObservation {
    #[must_use]
    pub const fn identity(&self) -> u128 {
        self.identity
    }

    #[must_use]
    pub const fn current_meaning(&self) -> u128 {
        self.current_meaning
    }

    #[must_use]
    pub const fn source(&self) -> &SourceRange {
        &self.source
    }

    #[must_use]
    pub const fn declared_capacity(&self) -> u64 {
        self.declared_capacity
    }

    #[must_use]
    pub const fn usable_slots(&self) -> u64 {
        self.usable_slots
    }

    #[must_use]
    pub const fn peak_live_allocations(&self) -> u64 {
        self.peak_live_allocations
    }

    #[must_use]
    pub const fn peak_outstanding_permits(&self) -> u64 {
        self.peak_outstanding_permits
    }

    #[must_use]
    pub const fn peak_commitment(&self) -> u64 {
        self.peak_commitment
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoolAdmissionEvidenceObservation {
    operation: PoolOperation,
    source: SourceRange,
    requirement_identity: u128,
    requirement_current_meaning: u128,
}

impl PoolAdmissionEvidenceObservation {
    #[must_use]
    pub const fn operation(&self) -> PoolOperation {
        self.operation
    }

    #[must_use]
    pub const fn source(&self) -> &SourceRange {
        &self.source
    }

    #[must_use]
    pub const fn requirement_identity(&self) -> u128 {
        self.requirement_identity
    }

    #[must_use]
    pub const fn requirement_current_meaning(&self) -> u128 {
        self.requirement_current_meaning
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoolModelObservation {
    cases: usize,
    agrees: bool,
    accepted: bool,
    full: bool,
    released: bool,
    reserved: bool,
    stale: bool,
    retired: bool,
}

impl PoolModelObservation {
    #[must_use]
    pub const fn cases(&self) -> usize {
        self.cases
    }

    #[must_use]
    pub const fn agrees(&self) -> bool {
        self.agrees
    }

    #[must_use]
    pub const fn covers_accepted(&self) -> bool {
        self.accepted
    }

    #[must_use]
    pub const fn covers_full(&self) -> bool {
        self.full
    }

    #[must_use]
    pub const fn covers_released(&self) -> bool {
        self.released
    }

    #[must_use]
    pub const fn covers_reserved(&self) -> bool {
        self.reserved
    }

    #[must_use]
    pub const fn covers_stale(&self) -> bool {
        self.stale
    }

    #[must_use]
    pub const fn covers_retired(&self) -> bool {
        self.retired
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutableDemandObservation {
    source_fingerprint: u128,
    fingerprint: u128,
    source_executable_count: usize,
    generated_executables: Arc<[u128]>,
}

impl ExecutableDemandObservation {
    #[must_use]
    pub const fn source_fingerprint(&self) -> u128 {
        self.source_fingerprint
    }

    #[must_use]
    pub const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }

    #[must_use]
    pub const fn source_executable_count(&self) -> usize {
        self.source_executable_count
    }

    #[must_use]
    pub fn generated_executables(&self) -> &[u128] {
        &self.generated_executables
    }

    #[must_use]
    pub fn generated_executable_count(&self) -> usize {
        self.generated_executables.len()
    }

    #[must_use]
    pub fn exact_executable_count(&self) -> usize {
        self.source_executable_count + self.generated_executables.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanningFoundationObservation {
    fingerprint: u128,
    context_identity: u128,
    completed_semantic_program_fingerprint: u128,
    architecture_contract_fingerprint: u128,
    planners: Arc<[PlannerObservation]>,
    domain_plans: Arc<[DomainPlanObservation]>,
    generated_roles: Arc<[GeneratedRoleObservation]>,
    requirements: Arc<[RequirementObservation]>,
    pools: Arc<[PoolObservation]>,
    pool_admission_evidence: Arc<[PoolAdmissionEvidenceObservation]>,
    pool_model: PoolModelObservation,
    executable_demand: ExecutableDemandObservation,
    facility_contracts: Arc<[FacilityContractObservation]>,
    facility_domain_plans: Arc<[FacilityDomainPlanObservation]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequirementSource {
    Domain,
    Flow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DischargeKind {
    ExecutableRealized,
    Placed,
    Bound,
    CapacityProved,
    CapabilityPresent,
    CardinalityProved,
    LifetimeProved,
    ContractValidated,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutablePlacementObservation {
    executable: u128,
    executable_current_meaning: u128,
    core: u16,
}

impl ExecutablePlacementObservation {
    #[must_use]
    pub const fn executable(&self) -> u128 {
        self.executable
    }

    #[must_use]
    pub const fn executable_current_meaning(&self) -> u128 {
        self.executable_current_meaning
    }

    #[must_use]
    pub const fn core(&self) -> u16 {
        self.core
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingAssignmentObservation {
    requirement: u128,
    binding: PlanningBinding,
    slot: u8,
}

impl BindingAssignmentObservation {
    #[must_use]
    pub const fn requirement(&self) -> u128 {
        self.requirement
    }

    #[must_use]
    pub const fn binding(&self) -> PlanningBinding {
        self.binding
    }

    #[must_use]
    pub const fn slot(&self) -> u8 {
        self.slot
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequirementDischargeObservation {
    source: RequirementSource,
    requirement: u128,
    requirement_current_meaning: u128,
    kind: DischargeKind,
}

impl RequirementDischargeObservation {
    #[must_use]
    pub const fn source(&self) -> RequirementSource {
        self.source
    }

    #[must_use]
    pub const fn requirement(&self) -> u128 {
        self.requirement
    }

    #[must_use]
    pub const fn requirement_current_meaning(&self) -> u128 {
        self.requirement_current_meaning
    }

    #[must_use]
    pub const fn kind(&self) -> DischargeKind {
        self.kind
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WholeImageAssignmentObservation {
    fingerprint: u128,
    requirement_set_fingerprint: u128,
    placements: Arc<[ExecutablePlacementObservation]>,
    bindings: Arc<[BindingAssignmentObservation]>,
    discharges: Arc<[RequirementDischargeObservation]>,
}

impl WholeImageAssignmentObservation {
    #[must_use]
    pub const fn phase_schema(&self) -> &'static str {
        WHOLE_IMAGE_ASSIGNMENT_SCHEMA
    }

    #[must_use]
    pub const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }

    #[must_use]
    pub const fn requirement_set_fingerprint(&self) -> u128 {
        self.requirement_set_fingerprint
    }

    #[must_use]
    pub fn placements(&self) -> &[ExecutablePlacementObservation] {
        &self.placements
    }

    #[must_use]
    pub fn bindings(&self) -> &[BindingAssignmentObservation] {
        &self.bindings
    }

    #[must_use]
    pub fn discharges(&self) -> &[RequirementDischargeObservation] {
        &self.discharges
    }
}

impl PlanningFoundationObservation {
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
    pub const fn completed_semantic_program_fingerprint(&self) -> u128 {
        self.completed_semantic_program_fingerprint
    }

    #[must_use]
    pub const fn architecture_contract_fingerprint(&self) -> u128 {
        self.architecture_contract_fingerprint
    }

    #[must_use]
    pub fn planners(&self) -> &[PlannerObservation] {
        &self.planners
    }

    #[must_use]
    pub fn domain_plans(&self) -> &[DomainPlanObservation] {
        &self.domain_plans
    }

    #[must_use]
    pub fn generated_roles(&self) -> &[GeneratedRoleObservation] {
        &self.generated_roles
    }

    #[must_use]
    pub fn requirements(&self) -> &[RequirementObservation] {
        &self.requirements
    }

    #[must_use]
    pub fn pools(&self) -> &[PoolObservation] {
        &self.pools
    }

    #[must_use]
    pub fn pool_admission_evidence(&self) -> &[PoolAdmissionEvidenceObservation] {
        &self.pool_admission_evidence
    }

    #[must_use]
    pub const fn pool_model(&self) -> &PoolModelObservation {
        &self.pool_model
    }

    #[must_use]
    pub const fn executable_demand(&self) -> &ExecutableDemandObservation {
        &self.executable_demand
    }

    #[must_use]
    pub fn facility_contracts(&self) -> &[FacilityContractObservation] {
        &self.facility_contracts
    }

    #[must_use]
    pub fn facility_domain_plans(&self) -> &[FacilityDomainPlanObservation] {
        &self.facility_domain_plans
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PlannerRef {
    context: u128,
    identity: u128,
    current_meaning: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DomainPlanRef {
    context: u128,
    identity: u128,
    current_meaning: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RoleRef {
    context: u128,
    identity: u128,
    current_meaning: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ExecutableRef {
    context: u128,
    identity: u128,
    current_meaning: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DemandInputRef {
    context: u128,
    identity: u128,
    current_meaning: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Planner {
    reference: PlannerRef,
    kind: PlannerKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DomainPlan {
    reference: DomainPlanRef,
    planner: PlannerRef,
    kind: DomainPlanKind,
    generated_roles: Arc<[RoleRef]>,
    requirements: Arc<[RequirementRef]>,
    facility_instance: Option<FacilityInstanceRef>,
    facility_contract: Option<FacilityContractRef>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GeneratedRole {
    reference: RoleRef,
    executable: ExecutableRef,
    owner: PlannerRef,
    generator: PlannerRef,
    kind: GeneratedRoleKind,
    local_key: u16,
    dependencies: Arc<[RoleRef]>,
    provenance: DomainPlanRef,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Requirement {
    context: u128,
    reference: RequirementRef,
    owner: PlannerRef,
    subject: RequirementOwner,
    provenance: RequirementProvenance,
    category: RequirementCategory,
    bounds: RequirementBounds,
    facility_contract: Option<FacilityContractRef>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RequirementOwner {
    GeneratedRole(RoleRef),
    Pool(PoolRef),
    Facility(FacilitySubjectRef),
}

impl RequirementOwner {
    const fn context(self) -> u128 {
        match self {
            Self::GeneratedRole(reference) => reference.context,
            Self::Pool(reference) => reference.context,
            Self::Facility(reference) => reference.context,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PoolRef {
    context: u128,
    identity: u128,
    current_meaning: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PoolAdmissionSite {
    executable_identity: u128,
    operation: PoolOperation,
    source: SourceRange,
    source_type_identity: u128,
    requirement: RequirementRef,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PoolPlan {
    reference: PoolRef,
    executable: CoreSourceExecutableRef,
    source: SourceRange,
    declared_capacity: u64,
    usable_slots: u64,
    peak_live: u64,
    peak_reserved: u64,
    peak_committed: u64,
    admission_sites: Arc<[PoolAdmissionSite]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RequirementRef {
    context: u128,
    identity: u128,
    current_meaning: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ExactExecutableDemand {
    source: DemandInputRef,
    source_count: usize,
    additions: Arc<[ExecutableRef]>,
    fingerprint: u128,
}

macro_rules! typed_reference_accessors {
    ($reference:ty) => {
        #[allow(dead_code)]
        impl $reference {
            pub(crate) const fn context(self) -> u128 {
                self.context
            }

            pub(crate) const fn identity(self) -> u128 {
                self.identity
            }

            pub(crate) const fn current_meaning(self) -> u128 {
                self.current_meaning
            }
        }
    };
}

typed_reference_accessors!(PlannerRef);
typed_reference_accessors!(DomainPlanRef);
typed_reference_accessors!(RoleRef);
typed_reference_accessors!(ExecutableRef);
typed_reference_accessors!(DemandInputRef);
typed_reference_accessors!(RequirementRef);

#[allow(dead_code)]
impl DomainPlan {
    pub(crate) const fn reference(&self) -> DomainPlanRef {
        self.reference
    }

    pub(crate) fn generated_roles(&self) -> &[RoleRef] {
        &self.generated_roles
    }

    pub(crate) fn requirements(&self) -> &[RequirementRef] {
        &self.requirements
    }
}

#[allow(dead_code)]
impl GeneratedRole {
    pub(crate) const fn reference(&self) -> RoleRef {
        self.reference
    }

    pub(crate) const fn executable(&self) -> ExecutableRef {
        self.executable
    }

    pub(crate) const fn owner(&self) -> PlannerRef {
        self.owner
    }

    pub(crate) const fn generator(&self) -> PlannerRef {
        self.generator
    }

    pub(crate) const fn kind(&self) -> GeneratedRoleKind {
        self.kind
    }

    pub(crate) const fn local_key(&self) -> u16 {
        self.local_key
    }

    pub(crate) fn dependencies(&self) -> &[RoleRef] {
        &self.dependencies
    }

    pub(crate) const fn provenance(&self) -> DomainPlanRef {
        self.provenance
    }
}

#[allow(dead_code)]
impl Requirement {
    pub(crate) const fn reference(&self) -> RequirementRef {
        self.reference
    }

    const fn subject(&self) -> RequirementOwner {
        self.subject
    }

    pub(crate) const fn owner(&self) -> PlannerRef {
        self.owner
    }

    pub(crate) const fn provenance(&self) -> RequirementProvenance {
        self.provenance
    }

    pub(crate) const fn category(&self) -> RequirementCategory {
        self.category
    }

    pub(crate) const fn bounds(&self) -> &RequirementBounds {
        &self.bounds
    }
}

#[derive(Clone)]
pub(crate) struct VerifiedPlanningFoundation {
    semantic_program: Arc<CompletedSemanticProgram>,
    architecture_contract: Arc<VerifiedArchitecturePlanningContract>,
    context: u128,
    planner_roster: Arc<[Planner]>,
    domain_plans: Arc<[DomainPlan]>,
    generated_roles: Arc<[GeneratedRole]>,
    requirements: Arc<[Requirement]>,
    pools: Arc<[PoolPlan]>,
    pool_model: PoolModelObservation,
    executable_demand: ExactExecutableDemand,
    facility_contracts: Arc<[FacilityContract]>,
    fingerprint: u128,
    _verified: Verified,
}

impl fmt::Debug for VerifiedPlanningFoundation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedPlanningFoundation")
            .field("context", &self.context)
            .field("fingerprint", &format_args!("{:032x}", self.fingerprint))
            .finish_non_exhaustive()
    }
}

impl PartialEq for VerifiedPlanningFoundation {
    fn eq(&self, other: &Self) -> bool {
        self.context == other.context && self.fingerprint == other.fingerprint
    }
}

impl Eq for VerifiedPlanningFoundation {}

#[derive(Clone, Debug)]
struct Verified;

impl VerifiedPlanningFoundation {
    pub(crate) const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }

    pub(crate) fn observation(&self) -> PlanningFoundationObservation {
        PlanningFoundationObservation {
            fingerprint: self.fingerprint,
            context_identity: self.context,
            completed_semantic_program_fingerprint: self.semantic_program.fingerprint(),
            architecture_contract_fingerprint: self
                .architecture_contract
                .for_image_planning()
                .fingerprint(),
            planners: self
                .planner_roster
                .iter()
                .map(|planner| PlannerObservation {
                    identity: planner.reference.identity,
                    kind: planner.kind,
                    current_meaning: planner.reference.current_meaning,
                })
                .collect::<Vec<_>>()
                .into(),
            domain_plans: self
                .domain_plans
                .iter()
                .map(|plan| DomainPlanObservation {
                    identity: plan.reference.identity,
                    planner: plan.planner.identity,
                    kind: plan.kind,
                    current_meaning: plan.reference.current_meaning,
                    generated_role_count: plan.generated_roles.len(),
                    requirement_count: plan.requirements.len(),
                })
                .collect::<Vec<_>>()
                .into(),
            generated_roles: self
                .generated_roles
                .iter()
                .map(|role| GeneratedRoleObservation {
                    identity: role.reference.identity,
                    executable: role.executable.identity,
                    owner: role.owner.identity,
                    generator: role.generator.identity,
                    kind: role.kind,
                    local_key: role.local_key,
                    dependencies: role
                        .dependencies
                        .iter()
                        .map(|dependency| dependency.identity)
                        .collect::<Vec<_>>()
                        .into(),
                    provenance: role.provenance.identity,
                    current_meaning: role.reference.current_meaning,
                })
                .collect::<Vec<_>>()
                .into(),
            requirements: self
                .requirements
                .iter()
                .map(|requirement| RequirementObservation {
                    reference: requirement.reference.identity,
                    owner: requirement.owner.identity,
                    subject: match requirement.subject {
                        RequirementOwner::GeneratedRole(reference) => {
                            RequirementSubject::GeneratedRole(reference.identity)
                        }
                        RequirementOwner::Pool(reference) => {
                            RequirementSubject::Pool(reference.identity)
                        }
                        RequirementOwner::Facility(reference) => {
                            RequirementSubject::FacilityInstance(reference.identity)
                        }
                    },
                    provenance: requirement.provenance,
                    category: requirement.category,
                    bounds: requirement.bounds.clone(),
                    current_meaning: requirement.reference.current_meaning,
                })
                .collect::<Vec<_>>()
                .into(),
            pools: self
                .pools
                .iter()
                .map(|pool| PoolObservation {
                    identity: pool.reference.identity,
                    current_meaning: pool.reference.current_meaning,
                    source: pool.source.clone(),
                    declared_capacity: pool.declared_capacity,
                    usable_slots: pool.usable_slots,
                    peak_live_allocations: pool.peak_live,
                    peak_outstanding_permits: pool.peak_reserved,
                    peak_commitment: pool.peak_committed,
                })
                .collect::<Vec<_>>()
                .into(),
            pool_admission_evidence: self
                .pools
                .iter()
                .flat_map(|pool| pool.admission_sites.iter())
                .map(|site| PoolAdmissionEvidenceObservation {
                    operation: site.operation,
                    source: site.source.clone(),
                    requirement_identity: site.requirement.identity,
                    requirement_current_meaning: site.requirement.current_meaning,
                })
                .collect::<Vec<_>>()
                .into(),
            pool_model: self.pool_model.clone(),
            executable_demand: ExecutableDemandObservation {
                source_fingerprint: self.executable_demand.source.identity,
                fingerprint: self.executable_demand.fingerprint,
                source_executable_count: self.executable_demand.source_count,
                generated_executables: self
                    .executable_demand
                    .additions
                    .iter()
                    .map(|executable| executable.identity)
                    .collect::<Vec<_>>()
                    .into(),
            },
            facility_contracts: self
                .facility_contracts
                .iter()
                .map(|contract| contract.observation.clone())
                .collect::<Vec<_>>()
                .into(),
            facility_domain_plans: self
                .domain_plans
                .iter()
                .filter_map(|plan| {
                    let instance = plan.facility_instance.clone()?;
                    Some(FacilityDomainPlanObservation {
                        identity: plan.reference.identity,
                        kind: instance.kind,
                        instance_identity: instance.identity,
                        contract_fingerprint: plan.facility_contract?.fingerprint,
                        contract_identity: plan.facility_contract?.identity,
                        contract_current_meaning: plan.facility_contract?.current_meaning,
                        current_meaning: plan.reference.current_meaning,
                        generated_role_count: plan.generated_roles.len(),
                        requirement_count: plan.requirements.len(),
                    })
                })
                .collect::<Vec<_>>()
                .into(),
        }
    }

    #[allow(
        dead_code,
        reason = "crate-private handoff reserved for the Core planner"
    )]
    pub(crate) const fn for_core(&self) -> CorePlanningInput<'_> {
        CorePlanningInput { foundation: self }
    }

    pub(crate) const fn for_flow(&self) -> FlowPlanningInput<'_> {
        FlowPlanningInput { foundation: self }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum WholeRequirementRef {
    Domain(RequirementRef),
    Flow(FlowRequirementRef),
}

impl WholeRequirementRef {
    const fn source(self) -> RequirementSource {
        match self {
            Self::Domain(_) => RequirementSource::Domain,
            Self::Flow(_) => RequirementSource::Flow,
        }
    }

    const fn identity(self) -> u128 {
        match self {
            Self::Domain(reference) => reference.identity,
            Self::Flow(reference) => reference.identity(),
        }
    }

    const fn current_meaning(self) -> u128 {
        match self {
            Self::Domain(reference) => reference.current_meaning,
            Self::Flow(reference) => reference.current_meaning(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct VerifiedWholeImageAssignment {
    planning_foundation: Arc<VerifiedPlanningFoundation>,
    core_program: Arc<VerifiedCoreProgram>,
    flow_program: Arc<VerifiedFlowProgram>,
    requirements: Arc<[WholeRequirementRef]>,
    requirement_set_fingerprint: u128,
    placements: Arc<[ExecutablePlacementObservation]>,
    bindings: Arc<[BindingAssignmentObservation]>,
    discharges: Arc<[RequirementDischargeObservation]>,
    fingerprint: u128,
    _verified: Verified,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WholeImageSolveOutcome {
    Assignment(VerifiedWholeImageAssignment),
    Conflict(VerifiedPrivateConflict),
}

impl fmt::Debug for VerifiedWholeImageAssignment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedWholeImageAssignment")
            .field("fingerprint", &format_args!("{:032x}", self.fingerprint))
            .finish_non_exhaustive()
    }
}

impl PartialEq for VerifiedWholeImageAssignment {
    fn eq(&self, other: &Self) -> bool {
        self.fingerprint == other.fingerprint
            && self.requirement_set_fingerprint == other.requirement_set_fingerprint
    }
}

impl Eq for VerifiedWholeImageAssignment {}

impl VerifiedWholeImageAssignment {
    pub(crate) const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }

    pub(crate) fn observation(&self) -> WholeImageAssignmentObservation {
        WholeImageAssignmentObservation {
            fingerprint: self.fingerprint,
            requirement_set_fingerprint: self.requirement_set_fingerprint,
            placements: Arc::clone(&self.placements),
            bindings: Arc::clone(&self.bindings),
            discharges: Arc::clone(&self.discharges),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct FlowPlanningInput<'a> {
    foundation: &'a VerifiedPlanningFoundation,
}

impl<'a> FlowPlanningInput<'a> {
    pub(crate) const fn fingerprint(self) -> u128 {
        self.foundation.fingerprint
    }

    pub(crate) const fn context_identity(self) -> u128 {
        self.foundation.context
    }

    pub(crate) fn semantic_program(
        self,
    ) -> crate::completed_semantic::FlowPlanningSemanticProgram<'a> {
        self.foundation.semantic_program.for_flow_planning()
    }
}

#[derive(Clone, Copy)]
pub(crate) struct CorePlanningInput<'a> {
    foundation: &'a VerifiedPlanningFoundation,
}

fn producer_facility_contracts(distribution_digest: u128) -> Arc<[FacilityContract]> {
    FACILITY_KINDS
        .into_iter()
        .map(|kind| producer_facility_contract(kind, distribution_digest))
        .collect::<Vec<_>>()
        .into()
}

fn producer_facility_contract(kind: FacilityKind, distribution_digest: u128) -> FacilityContract {
    producer_facility_contract_with_cardinality(kind, distribution_digest, 0, 1)
}

fn producer_facility_contract_with_cardinality(
    kind: FacilityKind,
    distribution_digest: u128,
    minimum_instances: u32,
    maximum_instances: u32,
) -> FacilityContract {
    let (roles, capacities, capabilities, binding, sharing, loss_policy, shutdown) =
        producer_facility_facts(kind);
    let flagship_rule = if kind == FacilityKind::Entropy {
        FacilityFlagshipRule::SelectingImageOptional
    } else {
        FacilityFlagshipRule::Required { loss_policy }
    };
    let replay_rule = if kind == FacilityKind::Entropy {
        FacilityReplayRule::ExcludedFromReplayableGameplay
    } else {
        FacilityReplayRule::ReplayableGameplay
    };
    let identity = producer_hash(
        b"wrela.facility-contract.identity.v1",
        &[u128::from(kind.tag())],
    );
    let context_receipt = producer_hash(
        b"wrela.facility-contract.context.v1",
        &[distribution_digest],
    );
    let endpoint_ownership = if kind == FacilityKind::Input {
        FacilityEndpointOwnership::BuildWiredActor
    } else {
        FacilityEndpointOwnership::FacilityInstance
    };
    let fingerprint = producer_hash(
        b"wrela.facility-contract.v1",
        &[
            u128::from(kind.tag()),
            identity,
            context_receipt,
            distribution_digest,
            1,
            1,
            u128::from(minimum_instances),
            u128::from(maximum_instances),
            1,
            match endpoint_ownership {
                FacilityEndpointOwnership::FacilityInstance => 1,
                FacilityEndpointOwnership::BuildWiredActor => 2,
            },
            facility_flagship_tag(flagship_rule),
            facility_replay_rule_tag(replay_rule),
            if kind == FacilityKind::Telemetry {
                1
            } else {
                3
            },
            producer_facility_facts_fingerprint(
                &roles,
                &capacities,
                &capabilities,
                binding,
                sharing,
                loss_policy,
                shutdown,
            ),
        ],
    );
    FacilityContract {
        reference: FacilityContractRef {
            context: context_receipt,
            identity,
            current_meaning: fingerprint,
            fingerprint,
            kind,
        },
        observation: FacilityContractObservation {
            kind,
            identity,
            context_receipt,
            fingerprint,
            current_meaning: fingerprint,
            allows_deployment: true,
            allows_test: true,
            minimum_instances,
            maximum_instances,
            maximum_exported_endpoints: 1,
            endpoint_ownership,
            flagship_rule,
            replay_rule,
            generated_roles: roles,
            semantic_capacities: capacities,
            required_capabilities: capabilities,
            external_binding: Some(binding),
            sharing,
            loss_policy,
            shutdown,
            maximum_recovery_attempts: if kind == FacilityKind::Telemetry {
                1
            } else {
                3
            },
            binding_availability: FacilityBindingAvailability::BootFailure,
        },
        authentication: producer_hash(
            b"wrela.facility-contract.authentication.v1",
            &[distribution_digest, identity, context_receipt, fingerprint],
        ),
    }
}

type FacilityFacts = (
    Arc<[GeneratedRoleKind]>,
    Arc<[FacilitySemanticCapacity]>,
    Arc<[PlanningCapability]>,
    PlanningBinding,
    FacilitySharing,
    FacilityLossPolicy,
    FacilityShutdown,
);

fn producer_facility_facts(kind: FacilityKind) -> FacilityFacts {
    let virtio = Arc::from([
        PlanningCapability::PciVirtioModern,
        PlanningCapability::SplitVirtqueue,
        PlanningCapability::SharedIntx,
        PlanningCapability::DmaOwnership,
    ]);
    match kind {
        FacilityKind::Display => (
            Arc::from([GeneratedRoleKind::DisplayDriver]),
            Arc::from([FacilitySemanticCapacity::FrameBuffers(3)]),
            virtio,
            PlanningBinding::Display,
            FacilitySharing::Exclusive,
            FacilityLossPolicy::ControlledShutdown,
            FacilityShutdown::Quiesce,
        ),
        FacilityKind::Input => (
            Arc::from([GeneratedRoleKind::InputDriver]),
            Arc::from([FacilitySemanticCapacity::InputTransitions(256)]),
            virtio,
            PlanningBinding::Input,
            FacilitySharing::Exclusive,
            FacilityLossPolicy::ControlledShutdown,
            FacilityShutdown::StopSampling,
        ),
        FacilityKind::EventStore => (
            Arc::from([
                GeneratedRoleKind::EventStoreRuntime,
                GeneratedRoleKind::EventStoreDriver,
            ]),
            Arc::from([FacilitySemanticCapacity::EventSlots(65_536)]),
            virtio,
            PlanningBinding::EventStore,
            FacilitySharing::Exclusive,
            FacilityLossPolicy::ControlledShutdown,
            FacilityShutdown::FlushCommittedAndQuiesce,
        ),
        FacilityKind::MonotonicClock => (
            Arc::from([GeneratedRoleKind::MonotonicClockDriver]),
            Arc::from([FacilitySemanticCapacity::ClockWaiters(1024)]),
            Arc::from([PlanningCapability::MonotonicCounter]),
            PlanningBinding::MonotonicClock,
            FacilitySharing::RegisteredDisjoint {
                role: FacilitySharedRole::MonotonicCounter,
                maximum_units: 1024,
            },
            FacilityLossPolicy::ControlledShutdown,
            FacilityShutdown::StopWakeups,
        ),
        FacilityKind::Entropy => (
            Arc::from([GeneratedRoleKind::EntropyDriver]),
            Arc::from([FacilitySemanticCapacity::EntropyRequestBytes(4096)]),
            virtio,
            PlanningBinding::Entropy,
            FacilitySharing::RegisteredDisjoint {
                role: FacilitySharedRole::EntropyQueue,
                maximum_units: 16,
            },
            FacilityLossPolicy::SelectingImagePolicy,
            FacilityShutdown::DiscardPending,
        ),
        FacilityKind::Telemetry => (
            Arc::from([GeneratedRoleKind::TelemetryDriver]),
            Arc::from([FacilitySemanticCapacity::TelemetryRingRecords(4096)]),
            virtio,
            PlanningBinding::Telemetry,
            FacilitySharing::Exclusive,
            FacilityLossPolicy::DisableAndContinue,
            FacilityShutdown::DropObservations,
        ),
    }
}

fn producer_facility_facts_fingerprint(
    roles: &[GeneratedRoleKind],
    capacities: &[FacilitySemanticCapacity],
    capabilities: &[PlanningCapability],
    binding: PlanningBinding,
    sharing: FacilitySharing,
    loss: FacilityLossPolicy,
    shutdown: FacilityShutdown,
) -> u128 {
    let mut values = vec![
        u128::from(binding.tag()),
        facility_sharing_tag(sharing),
        facility_loss_tag(loss),
        facility_shutdown_tag(shutdown),
    ];
    values.extend(roles.iter().map(|role| u128::from(role.tag())));
    values.extend(
        capacities
            .iter()
            .map(|capacity| facility_capacity_tag(*capacity)),
    );
    values.extend(
        capabilities
            .iter()
            .map(|capability| u128::from(capability.tag())),
    );
    producer_hash(b"wrela.facility-contract.facts.v1", &values)
}

const fn facility_capacity_tag(capacity: FacilitySemanticCapacity) -> u128 {
    match capacity {
        FacilitySemanticCapacity::FrameBuffers(value) => (1_u128 << 64) | value as u128,
        FacilitySemanticCapacity::InputTransitions(value) => (2_u128 << 64) | value as u128,
        FacilitySemanticCapacity::EventSlots(value) => (3_u128 << 64) | value as u128,
        FacilitySemanticCapacity::ClockWaiters(value) => (4_u128 << 64) | value as u128,
        FacilitySemanticCapacity::EntropyRequestBytes(value) => (5_u128 << 64) | value as u128,
        FacilitySemanticCapacity::TelemetryRingRecords(value) => (6_u128 << 64) | value as u128,
    }
}

const fn facility_sharing_tag(sharing: FacilitySharing) -> u128 {
    match sharing {
        FacilitySharing::Exclusive => 1,
        FacilitySharing::RegisteredDisjoint {
            role,
            maximum_units,
        } => {
            let role = match role {
                FacilitySharedRole::MonotonicCounter => 1_u128,
                FacilitySharedRole::EntropyQueue => 2_u128,
            };
            (2_u128 << 96) | (role << 64) | maximum_units as u128
        }
    }
}

const fn facility_loss_tag(loss: FacilityLossPolicy) -> u128 {
    match loss {
        FacilityLossPolicy::ControlledShutdown => 1,
        FacilityLossPolicy::DisableAndContinue => 2,
        FacilityLossPolicy::SelectingImagePolicy => 3,
    }
}

const fn facility_flagship_tag(rule: FacilityFlagshipRule) -> u128 {
    match rule {
        FacilityFlagshipRule::Required { loss_policy } => 0x100 | facility_loss_tag(loss_policy),
        FacilityFlagshipRule::SelectingImageOptional => 2,
    }
}

const fn facility_replay_rule_tag(rule: FacilityReplayRule) -> u128 {
    match rule {
        FacilityReplayRule::ReplayableGameplay => 1,
        FacilityReplayRule::ExcludedFromReplayableGameplay => 2,
    }
}

const fn facility_replay_authority_tag(authority: FacilityReplayAuthority) -> u8 {
    match authority {
        FacilityReplayAuthority::ReplayableGameplay => 1,
        FacilityReplayAuthority::NonReplayableFacility => 2,
    }
}

const fn facility_endpoint_ownership_tag(ownership: FacilityEndpointOwnership) -> u8 {
    match ownership {
        FacilityEndpointOwnership::FacilityInstance => 1,
        FacilityEndpointOwnership::BuildWiredActor => 2,
    }
}

const fn facility_binding_availability_tag(availability: FacilityBindingAvailability) -> u8 {
    match availability {
        FacilityBindingAvailability::BootFailure => 1,
    }
}

const fn facility_shutdown_tag(shutdown: FacilityShutdown) -> u128 {
    match shutdown {
        FacilityShutdown::Quiesce => 1,
        FacilityShutdown::FlushCommittedAndQuiesce => 2,
        FacilityShutdown::StopSampling => 3,
        FacilityShutdown::StopWakeups => 4,
        FacilityShutdown::DiscardPending => 5,
        FacilityShutdown::DropObservations => 6,
    }
}

fn discover_facility_instances(
    semantic: crate::completed_semantic::ImagePlanningSemanticProgram<'_>,
    cancellation: &Cancellation,
) -> Result<Vec<FacilityInstanceRef>, PlanningFailure> {
    let type_kinds = FACILITY_KINDS
        .into_iter()
        .filter_map(|kind| {
            semantic
                .authenticated_nominal_type("src/core/facilities.wr", kind.source_name())
                .map(|identity| (identity, kind))
        })
        .collect::<BTreeMap<_, _>>();
    let loss_variants = [
        ("ControlledShutdown", FacilityLossPolicy::ControlledShutdown),
        ("DisableAndContinue", FacilityLossPolicy::DisableAndContinue),
        (
            "SelectingImagePolicy",
            FacilityLossPolicy::SelectingImagePolicy,
        ),
    ]
    .into_iter()
    .filter_map(|(name, policy)| {
        semantic
            .authenticated_variant("src/core/facilities.wr", "FacilityLossSelection", name)
            .map(|identity| (identity, policy))
    })
    .collect::<BTreeMap<_, _>>();
    let replay_variants = [
        (
            "ReplayableGameplay",
            FacilityReplayAuthority::ReplayableGameplay,
        ),
        (
            "NonReplayableFacility",
            FacilityReplayAuthority::NonReplayableFacility,
        ),
    ]
    .into_iter()
    .filter_map(|(name, authority)| {
        semantic
            .authenticated_variant("src/core/facilities.wr", "FacilityReplayAuthority", name)
            .map(|identity| (identity, authority))
    })
    .collect::<BTreeMap<_, _>>();
    let mut instances = Vec::new();
    for node in semantic.construction_nodes() {
        checkpoint(cancellation)?;
        let BuildKind::Node { type_identity, .. } = node.kind() else {
            continue;
        };
        let Some(kind) = type_kinds.get(&type_identity.0).copied() else {
            continue;
        };
        let supervisor = facility_actor_operand(semantic, node, "supervisor").ok_or_else(|| {
            PlanningFailure::FacilityConfiguration {
                kind,
                instance: node.identity(),
                source: node.source().clone(),
                failure: FacilityConfigurationFailure::SupervisorNotActor,
            }
        })?;
        let input_owner = if kind == FacilityKind::Input {
            Some(
                facility_actor_operand(semantic, node, "owner").ok_or_else(|| {
                    PlanningFailure::FacilityConfiguration {
                        kind,
                        instance: node.identity(),
                        source: node.source().clone(),
                        failure: FacilityConfigurationFailure::InputOwnerNotActor,
                    }
                })?,
            )
        } else {
            None
        };
        let selected_loss_policy = facility_variant_operand(node, "loss", &loss_variants)
            .ok_or_else(|| PlanningFailure::Defect(Arc::from("Facility loss fact is malformed")))?;
        let replay_authority = facility_variant_operand(node, "replay", &replay_variants)
            .ok_or_else(|| {
                PlanningFailure::Defect(Arc::from("Facility replay fact is malformed"))
            })?;
        let current_meaning = facility_instance_current_meaning(
            node,
            supervisor,
            input_owner,
            selected_loss_policy,
            replay_authority,
        );
        instances.push(FacilityInstanceRef {
            context: node.context(),
            identity: node.identity(),
            current_meaning,
            kind,
            source: node.source().clone(),
            supervisor,
            input_owner,
            selected_loss_policy,
            replay_authority,
        });
    }
    instances.sort_by_key(|instance| instance.identity);
    Ok(instances)
}

fn facility_instance_current_meaning(
    node: crate::completed_semantic::PlanningConstructionNodeRef<'_>,
    supervisor: FacilityActorRef,
    input_owner: Option<FacilityActorRef>,
    selected_loss_policy: FacilityLossPolicy,
    replay_authority: FacilityReplayAuthority,
) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(b"wrela.facility-instance.meaning.v1");
    for value in [
        node.context(),
        node.identity(),
        node.current_meaning(),
        supervisor.context,
        supervisor.identity,
        supervisor.current_meaning,
    ] {
        hash.update(&value.to_le_bytes());
    }
    hash.update(&[u8::from(input_owner.is_some())]);
    if let Some(owner) = input_owner {
        for value in [owner.context, owner.identity, owner.current_meaning] {
            hash.update(&value.to_le_bytes());
        }
    }
    hash.update(&facility_loss_tag(selected_loss_policy).to_le_bytes());
    hash.update(&[facility_replay_authority_tag(replay_authority)]);
    hash.digest128()
}

fn facility_actor_operand(
    semantic: crate::completed_semantic::ImagePlanningSemanticProgram<'_>,
    node: crate::completed_semantic::PlanningConstructionNodeRef<'_>,
    label: &str,
) -> Option<FacilityActorRef> {
    let PlanningConstructionValueRef::Construction { identity, .. } = node.operand(label)? else {
        return None;
    };
    let actor = semantic.actor_construction(identity)?;
    Some(FacilityActorRef {
        context: actor.context(),
        identity: actor.identity(),
        current_meaning: actor.current_meaning(),
    })
}

fn facility_variant_operand<T: Copy>(
    node: crate::completed_semantic::PlanningConstructionNodeRef<'_>,
    label: &str,
    variants: &BTreeMap<(u128, u128), T>,
) -> Option<T> {
    let PlanningConstructionValueRef::AuthenticatedVariant { owner, variant } =
        node.operand(label)?
    else {
        return None;
    };
    variants.get(&(owner, variant)).copied()
}

#[allow(dead_code)]
impl<'a> CorePlanningInput<'a> {
    pub(crate) const fn fingerprint(&self) -> u128 {
        self.foundation.fingerprint
    }

    pub(crate) const fn context_identity(&self) -> u128 {
        self.foundation.context
    }

    pub(crate) fn completed_semantic_program(self) -> CorePlanningSemanticProgram<'a> {
        self.foundation.semantic_program.for_core_planning()
    }

    pub(crate) const fn source_executable_demand(&self) -> DemandInputRef {
        self.foundation.executable_demand.source
    }

    pub(crate) fn exact_source_executables(
        self,
    ) -> impl ExactSizeIterator<Item = CoreSourceExecutableRef> + 'a {
        self.completed_semantic_program().exact_source_executables()
    }

    pub(crate) fn domain_plans(&self) -> &[DomainPlan] {
        &self.foundation.domain_plans
    }

    pub(crate) fn generated_roles(&self) -> &[GeneratedRole] {
        &self.foundation.generated_roles
    }

    pub(crate) fn requirements(&self) -> &[Requirement] {
        &self.foundation.requirements
    }

    pub(crate) fn pool_admission_site(
        self,
        executable_identity: u128,
        operation: PoolOperation,
        source: &SourceRange,
    ) -> Option<(RequirementRef, u128)> {
        self.foundation
            .pools
            .iter()
            .flat_map(|pool| pool.admission_sites.iter())
            .find(|site| {
                site.executable_identity == executable_identity
                    && site.operation == operation
                    && site.source == *source
            })
            .map(|site| (site.requirement, site.source_type_identity))
    }

    pub(crate) fn generated_executable_additions(&self) -> &[ExecutableRef] {
        &self.foundation.executable_demand.additions
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ImagePlanningModule;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PlanningFailure {
    Cancelled,
    PoolAdmission {
        source: SourceRange,
        declared: u64,
        required: u64,
    },
    FacilityCardinality {
        kind: FacilityKind,
        selected: u32,
        minimum: u32,
        maximum: u32,
        sites: Arc<[FacilityConflictSite]>,
    },
    FacilityConfiguration {
        kind: FacilityKind,
        instance: u128,
        source: SourceRange,
        failure: FacilityConfigurationFailure,
    },
    FacilityCompatibility {
        kind: FacilityKind,
        instance: u128,
        source: SourceRange,
        requirement: RequirementRef,
        missing: FacilityCompatibilityMissing,
    },
    Defect(Arc<str>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FacilityConflictSite {
    pub(crate) identity: u128,
    pub(crate) source: SourceRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FacilityCompatibilityMissing {
    Capability(PlanningCapability),
    Binding(PlanningBinding),
    SharedRole {
        role: FacilitySharedRole,
        required_units: u32,
        available_units: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FacilityConfigurationFailure {
    SupervisorNotActor,
    InputOwnerNotActor,
    LossPolicy {
        selected: FacilityLossPolicy,
        required: FacilityLossPolicy,
    },
    ReplayAuthority {
        selected: FacilityReplayAuthority,
        required: FacilityReplayAuthority,
    },
}

fn translate_whole_requirement_set(
    planning_foundation: &VerifiedPlanningFoundation,
    core: crate::core::ImagePlanningCoreView<'_>,
    flow: crate::flow::ImagePlanningFlowView<'_>,
    cancellation: &Cancellation,
) -> Result<CanonicalProblem, PlanningFailure> {
    let architecture = planning_foundation
        .architecture_contract
        .for_image_planning();
    let executables = core
        .executables()
        .map(|executable| executable.identity())
        .collect::<Vec<_>>();
    let capabilities = [
        PlanningCapability::TypedTerminalLifecycle,
        PlanningCapability::PanicPulse,
        PlanningCapability::GuestShutdownPulse,
        PlanningCapability::SecondaryCoreStartup,
        PlanningCapability::PciVirtioModern,
        PlanningCapability::SplitVirtqueue,
        PlanningCapability::SharedIntx,
        PlanningCapability::DmaOwnership,
        PlanningCapability::MonotonicCounter,
    ]
    .into_iter()
    .filter(|capability| architecture.has_capability(capability.contract_kind()))
    .map(PlanningCapability::tag)
    .collect::<BTreeSet<_>>();
    let actors = flow
        .actors()
        .map(|actor| {
            (
                actor.identity(),
                (
                    actor.mailbox_capacity(),
                    actor.max_active_turns(),
                    actor.handlers().collect::<Vec<_>>(),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let total_service_units = architecture.cores().fold(0_u64, |total, core| {
        total.saturating_add(u64::from(core.maximum_service_units))
    });
    let mut requirements =
        Vec::with_capacity(planning_foundation.requirements.len() + flow.requirements().len());
    for requirement in planning_foundation.requirements.iter() {
        checkpoint(cancellation)?;
        let constraint = match requirement.bounds {
            RequirementBounds::RealizeExactlyOnce { executable } => {
                SolverConstraint::Realize { executable }
            }
            RequirementBounds::ImageLifetime => {
                let RequirementOwner::GeneratedRole(role_reference) = requirement.subject else {
                    return defect("Image-lifetime Requirement is not owned by a generated role");
                };
                let executable = planning_foundation
                    .generated_roles
                    .iter()
                    .find(|role| role.reference == role_reference)
                    .map(|role| role.executable.identity)
                    .ok_or_else(|| {
                        PlanningFailure::Defect(Arc::from(
                            "Image-lifetime Requirement names an unknown generated role",
                        ))
                    })?;
                SolverConstraint::Activation {
                    executable,
                    units: 1,
                    start: 0,
                    end: 1,
                }
            }
            RequirementBounds::Capability(capability) => SolverConstraint::Capability {
                capability: capability.tag(),
            },
            RequirementBounds::Cardinality { minimum, maximum } => SolverConstraint::Cardinality {
                selected: u64::from(selected_cardinality(
                    planning_foundation,
                    requirement,
                    architecture,
                )),
                minimum: u64::from(minimum),
                maximum: u64::from(maximum),
            },
            RequirementBounds::MaximumServiceUnits(required) => SolverConstraint::Capacity {
                required: u64::from(required),
                available: total_service_units,
            },
            RequirementBounds::Binding {
                kind,
                minimum,
                maximum,
            } => SolverConstraint::Binding {
                subject: requirement.reference.identity,
                kind: kind.tag(),
                minimum: u16::try_from(minimum).map_err(|_| {
                    PlanningFailure::Defect(Arc::from(
                        "binding minimum exceeds canonical solver width",
                    ))
                })?,
                maximum: u16::try_from(maximum).map_err(|_| {
                    PlanningFailure::Defect(Arc::from(
                        "binding maximum exceeds canonical solver width",
                    ))
                })?,
                allow_sharing: false,
            },
            RequirementBounds::PoolCapacity {
                usable,
                peak_committed,
                ..
            } => SolverConstraint::Capacity {
                required: peak_committed,
                available: usable,
            },
            RequirementBounds::FacilitySharing(FacilitySharing::RegisteredDisjoint {
                role,
                maximum_units,
            }) => SolverConstraint::Capacity {
                required: u64::from(maximum_units),
                available: architecture
                    .facility_share(role.architecture_kind())
                    .map_or(0, |registration| u64::from(registration.maximum_units)),
            },
            RequirementBounds::Reservation { kind, .. } => SolverConstraint::Static {
                satisfied: architecture.has_reservation(kind.contract_kind()),
            },
            RequirementBounds::FacilityCapacity(_)
            | RequirementBounds::FacilitySharing(FacilitySharing::Exclusive)
            | RequirementBounds::FacilityEndpoint { .. }
            | RequirementBounds::FacilityRecovery { .. }
            | RequirementBounds::FacilityShutdown(_)
            | RequirementBounds::FacilityReplay { .. }
            | RequirementBounds::FacilityFlagship(_)
            | RequirementBounds::FacilityBindingAvailability(_) => {
                SolverConstraint::Static { satisfied: true }
            }
        };
        requirements.push(SolverRequirement {
            identity: requirement.reference.identity,
            source: RequirementSource::Domain,
            current_meaning: requirement.reference.current_meaning,
            category: solver_domain_category(requirement.category),
            constraint,
        });
    }
    for requirement in flow.requirements() {
        checkpoint(cancellation)?;
        let actor = actors.get(&requirement.actor()).ok_or_else(|| {
            PlanningFailure::Defect(Arc::from(
                "Flow Requirement names an unknown Actor during translation",
            ))
        })?;
        let constraint = match requirement.kind() {
            FlowRequirementKind::PermanentCorePlacement if actor.2.is_empty() => {
                SolverConstraint::Static { satisfied: true }
            }
            FlowRequirementKind::PermanentCorePlacement => SolverConstraint::AffinityGroup {
                executables: actor.2.clone().into(),
            },
            FlowRequirementKind::MailboxCapacity => SolverConstraint::Capacity {
                required: requirement.bound(),
                available: actor.0,
            },
            FlowRequirementKind::TurnLease => SolverConstraint::Cardinality {
                selected: u64::from(actor.1),
                minimum: 1,
                maximum: requirement.bound(),
            },
            FlowRequirementKind::SuspensionHome | FlowRequirementKind::ActivationStorage => {
                let executable = requirement.handler().ok_or_else(|| {
                    PlanningFailure::Defect(Arc::from(
                        "activation Requirement has no handler subject",
                    ))
                })?;
                SolverConstraint::Activation {
                    executable,
                    units: u16::try_from(requirement.bound()).map_err(|_| {
                        PlanningFailure::Defect(Arc::from(
                            "activation Requirement exceeds canonical solver width",
                        ))
                    })?,
                    start: 0,
                    end: 1,
                }
            }
            FlowRequirementKind::ServiceStorage => SolverConstraint::Capacity {
                required: requirement.bound(),
                available: u64::from(architecture.capacity().maximum_activation_homes),
            },
            _ => SolverConstraint::Static { satisfied: true },
        };
        requirements.push(SolverRequirement {
            identity: requirement.reference().identity(),
            source: RequirementSource::Flow,
            current_meaning: requirement.reference().current_meaning(),
            category: solver_flow_category(requirement.kind()),
            constraint,
        });
    }
    let binding_subjects = requirements
        .iter()
        .filter_map(|requirement| match requirement.constraint {
            SolverConstraint::Binding { subject, .. } => Some(subject),
            _ => None,
        })
        .collect();
    Ok(CanonicalProblem {
        name: "whole_image",
        cores: architecture
            .cores()
            .map(|core| CoreResource {
                identity: core.ordinal,
            })
            .collect(),
        executables,
        bindings: architecture
            .binding_slots()
            .map(|slot| BindingResource {
                identity: slot.ordinal,
                kind: architecture_binding_tag(slot.kind),
                shareable: false,
            })
            .collect(),
        binding_subjects,
        capabilities,
        requirements,
    })
}

const fn solver_domain_category(category: RequirementCategory) -> SolverRequirementCategory {
    match category {
        RequirementCategory::GeneratedRoleRealization => SolverRequirementCategory::RoleRealization,
        RequirementCategory::ArchitectureCapability => {
            SolverRequirementCategory::RequiredCapability
        }
        RequirementCategory::Cardinality => SolverRequirementCategory::Cardinality,
        RequirementCategory::Binding => SolverRequirementCategory::Binding,
        RequirementCategory::CapacityPressure | RequirementCategory::Service => {
            SolverRequirementCategory::Capacity
        }
        RequirementCategory::Lifetime => SolverRequirementCategory::ActivationLifetime,
        RequirementCategory::LogicalLayout
        | RequirementCategory::FacilityOwnership
        | RequirementCategory::Recovery
        | RequirementCategory::Shutdown
        | RequirementCategory::Replay
        | RequirementCategory::Flagship
        | RequirementCategory::BootAvailability => SolverRequirementCategory::Placement,
    }
}

const fn solver_flow_category(kind: FlowRequirementKind) -> SolverRequirementCategory {
    match kind {
        FlowRequirementKind::PermanentCorePlacement => SolverRequirementCategory::Affinity,
        FlowRequirementKind::MailboxCapacity
        | FlowRequirementKind::ServiceStorage
        | FlowRequirementKind::ActivationStorage => SolverRequirementCategory::Capacity,
        FlowRequirementKind::TurnLease
        | FlowRequirementKind::SuspensionHome
        | FlowRequirementKind::GroupChildActivationBound
        | FlowRequirementKind::GroupCancellationAuthority
        | FlowRequirementKind::GroupOutcomePolicy
        | FlowRequirementKind::GroupResourceReturnHome
        | FlowRequirementKind::GroupCleanupOrder
        | FlowRequirementKind::DeadlineClass
        | FlowRequirementKind::DeadlineAuthority
        | FlowRequirementKind::DeadlineSlack
        | FlowRequirementKind::DeadlineFeasibility
        | FlowRequirementKind::CancellationCheckpoint
        | FlowRequirementKind::CancellationObservationWorkBound => {
            SolverRequirementCategory::ActivationLifetime
        }
        _ => SolverRequirementCategory::Placement,
    }
}

const fn architecture_binding_tag(kind: BindingKind) -> u8 {
    match kind {
        BindingKind::Display => PlanningBinding::Display.tag(),
        BindingKind::Input => PlanningBinding::Input.tag(),
        BindingKind::EventStore => PlanningBinding::EventStore.tag(),
        BindingKind::MonotonicClock => PlanningBinding::MonotonicClock.tag(),
        BindingKind::Entropy => PlanningBinding::Entropy.tag(),
        BindingKind::Telemetry => PlanningBinding::Telemetry.tag(),
        BindingKind::Terminal => PlanningBinding::Terminal.tag(),
        BindingKind::Panic => PlanningBinding::Panic.tag(),
    }
}

impl ImagePlanningModule {
    pub(crate) fn plan(
        &self,
        semantic_program: Arc<CompletedSemanticProgram>,
        architecture_contract: Arc<VerifiedArchitecturePlanningContract>,
        cancellation: &Cancellation,
    ) -> Result<VerifiedPlanningFoundation, PlanningFailure> {
        checkpoint(cancellation)?;
        let semantic = semantic_program.for_image_planning();
        let architecture = architecture_contract.for_image_planning();
        if semantic.distribution_digest() != architecture.distribution_digest() {
            return defect("planning inputs belong to different Compiler Distributions");
        }
        let context = semantic.context_identity();
        let planner = produce_planner(
            context,
            semantic.root(),
            semantic.fingerprint(),
            architecture.fingerprint(),
        );
        checkpoint(cancellation)?;
        let mut plan = produce_domain_plan(
            context,
            planner.reference,
            semantic.fingerprint(),
            architecture.fingerprint(),
        );
        let mut generated_roles = produce_roles(
            context,
            semantic.root(),
            planner.reference,
            plan.reference,
            semantic.fingerprint(),
            architecture.fingerprint(),
            cancellation,
        )?;
        let mut requirements = produce_requirements(
            context,
            &generated_roles,
            planner.reference,
            plan.reference,
            semantic.test_application_count(),
            architecture.core_count(),
            architecture.service().maximum_cycle_units,
            cancellation,
        )?;
        let facility_contracts = producer_facility_contracts(semantic.distribution_digest());
        let facility_instances = discover_facility_instances(semantic, cancellation)?;
        validate_facility_cardinality(&facility_instances, &facility_contracts, cancellation)?;
        let mut planners = vec![planner.clone()];
        let mut plans = vec![plan.clone()];
        for instance in &facility_instances {
            checkpoint(cancellation)?;
            let contract = facility_contracts
                .iter()
                .find(|contract| contract.observation.kind == instance.kind)
                .ok_or_else(|| {
                    PlanningFailure::Defect(Arc::from(
                        "selected Facility has no authenticated contract",
                    ))
                })?;
            if instance.selected_loss_policy != contract.observation.loss_policy {
                return Err(PlanningFailure::FacilityConfiguration {
                    kind: instance.kind,
                    instance: instance.identity,
                    source: instance.source.clone(),
                    failure: FacilityConfigurationFailure::LossPolicy {
                        selected: instance.selected_loss_policy,
                        required: contract.observation.loss_policy,
                    },
                });
            }
            let required_replay = if contract.observation.allowed_in_replayable_gameplay() {
                FacilityReplayAuthority::ReplayableGameplay
            } else {
                FacilityReplayAuthority::NonReplayableFacility
            };
            if instance.replay_authority != required_replay {
                return Err(PlanningFailure::FacilityConfiguration {
                    kind: instance.kind,
                    instance: instance.identity,
                    source: instance.source.clone(),
                    failure: FacilityConfigurationFailure::ReplayAuthority {
                        selected: instance.replay_authority,
                        required: required_replay,
                    },
                });
            }
            let facility_planner = produce_facility_planner(
                context,
                instance.clone(),
                contract,
                architecture.fingerprint(),
            );
            let mut facility_plan = produce_facility_domain_plan(
                context,
                instance.clone(),
                facility_planner.reference,
                contract,
            );
            let facility_roles = produce_facility_roles(
                context,
                facility_planner.reference,
                facility_plan.reference,
                contract,
                architecture.fingerprint(),
                cancellation,
            )?;
            let facility_requirements = produce_facility_requirements(
                context,
                facility_planner.reference,
                facility_plan.reference,
                instance,
                &facility_roles,
                contract,
                cancellation,
            )?;
            check_facility_compatibility(
                architecture,
                instance,
                &facility_requirements,
                cancellation,
            )?;
            facility_plan.generated_roles = facility_roles
                .iter()
                .map(|role| role.reference)
                .collect::<Vec<_>>()
                .into();
            facility_plan.requirements = facility_requirements
                .iter()
                .map(|requirement| requirement.reference)
                .collect::<Vec<_>>()
                .into();
            planners.push(facility_planner);
            plans.push(facility_plan);
            generated_roles.extend(facility_roles);
            requirements.extend(facility_requirements);
        }
        let (pools, mut pool_requirements) = produce_pool_plans(
            context,
            semantic_program.for_core_planning(),
            planner.reference,
            plan.reference,
            cancellation,
        )?;
        requirements.append(&mut pool_requirements);
        requirements.sort_by_key(|requirement| requirement.reference.identity);
        generated_roles.sort_by_key(|role| role.reference.identity);
        if let Some(pool) = pools
            .iter()
            .find(|pool| pool.peak_committed > pool.usable_slots)
        {
            return Err(PlanningFailure::PoolAdmission {
                source: pool.source.clone(),
                declared: pool.declared_capacity,
                required: pool.peak_committed,
            });
        }
        let pool_model = run_pool_model();
        if !pool_model.agrees
            || !pool_model.accepted
            || !pool_model.full
            || !pool_model.released
            || !pool_model.reserved
            || !pool_model.stale
            || !pool_model.retired
        {
            return defect("bounded Pool model does not cover the accepted state machine");
        }
        let source = DemandInputRef {
            context,
            identity: semantic.executable_demand_fingerprint(),
            current_meaning: semantic.executable_demand_fingerprint(),
        };
        let mut additions = generated_roles
            .iter()
            .map(|role| role.executable)
            .collect::<Vec<_>>();
        additions.sort_by_key(|executable| executable.identity);
        plan.generated_roles = generated_roles
            .iter()
            .filter(|role| role.owner == planner.reference)
            .map(|role| role.reference)
            .collect::<Vec<_>>()
            .into();
        plan.requirements = requirements
            .iter()
            .filter(|requirement| requirement.owner == planner.reference)
            .map(|requirement| requirement.reference)
            .collect::<Vec<_>>()
            .into();
        let executable_demand = ExactExecutableDemand {
            source,
            source_count: semantic.source_executable_count(),
            fingerprint: produce_demand_fingerprint(
                source,
                semantic.source_executable_count(),
                &additions,
            ),
            additions: additions.into(),
        };
        plans[0] = plan;
        planners.sort_by_key(|planner| planner.reference.identity);
        plans.sort_by_key(|plan| plan.reference.identity);
        let planner_roster: Arc<[Planner]> = planners.into();
        let domain_plans: Arc<[DomainPlan]> = plans.into();
        let fingerprint = produce_foundation_fingerprint(
            context,
            semantic.fingerprint(),
            semantic.construction_graph_fingerprint(),
            semantic.custody_fingerprint(),
            architecture.identity(),
            architecture.fingerprint(),
            architecture.distribution_input_receipt(),
            &planner_roster,
            &domain_plans,
            &generated_roles,
            &requirements,
            &pools,
            &pool_model,
            &executable_demand,
        );
        let candidate = VerifiedPlanningFoundation {
            semantic_program,
            architecture_contract,
            context,
            planner_roster,
            domain_plans,
            generated_roles: generated_roles.into(),
            requirements: requirements.into(),
            pools: pools.into(),
            pool_model,
            executable_demand,
            facility_contracts,
            fingerprint,
            _verified: Verified,
        };
        verify(&candidate, cancellation)?;
        Ok(candidate)
    }
}

impl ImagePlanningModule {
    pub(crate) fn solve(
        &self,
        planning_foundation: Arc<VerifiedPlanningFoundation>,
        core_program: Arc<VerifiedCoreProgram>,
        flow_program: Arc<VerifiedFlowProgram>,
        cancellation: &Cancellation,
    ) -> Result<WholeImageSolveOutcome, PlanningFailure> {
        checkpoint(cancellation)?;
        let core = core_program.for_image_planning();
        let flow = flow_program.for_image_planning();
        if planning_foundation.context != core.context_identity()
            || planning_foundation.context != flow.context_identity()
            || planning_foundation.fingerprint != flow.planning_fingerprint()
            || core.fingerprint() != flow.core_fingerprint()
        {
            return defect("whole-Image solver inputs do not share one verified context");
        }
        let architecture = planning_foundation
            .architecture_contract
            .for_image_planning();
        if architecture.core_count() == 0 {
            return defect("whole-Image solver received no symbolic cores");
        }

        let mut requirements = planning_foundation
            .requirements
            .iter()
            .map(|requirement| WholeRequirementRef::Domain(requirement.reference))
            .chain(
                flow.requirements()
                    .map(|requirement| WholeRequirementRef::Flow(requirement.reference())),
            )
            .collect::<Vec<_>>();
        requirements.sort_by_key(|reference| (reference.identity(), reference.source()));
        if requirements
            .windows(2)
            .any(|pair| pair[0].identity() == pair[1].identity())
        {
            return defect("complete Requirement Set contains a duplicate identity");
        }
        if requirements.len()
            > usize::try_from(architecture.capacity().maximum_requirements).unwrap_or(usize::MAX)
        {
            return defect("complete Requirement Set exceeds its authenticated finite bound");
        }

        let executable_meanings = core
            .executables()
            .map(|executable| (executable.identity(), executable.current_meaning()))
            .collect::<BTreeMap<_, _>>();
        let placement_problem =
            translate_whole_requirement_set(&planning_foundation, core, flow, cancellation)?;
        let (solved_placements, solved_bindings, solved_discharges) =
            match solve_canonical_problem(&placement_problem, cancellation)? {
                CanonicalSolveOutcome::Assignment {
                    placements,
                    bindings,
                    discharges,
                } => (placements, bindings, discharges),
                CanonicalSolveOutcome::Conflict(conflict) => {
                    return Ok(WholeImageSolveOutcome::Conflict(conflict));
                }
            };
        let placements = solved_placements
            .iter()
            .map(|(executable, core)| ExecutablePlacementObservation {
                executable: *executable,
                executable_current_meaning: executable_meanings[executable],
                core: *core,
            })
            .collect::<Vec<_>>();
        if placements
            .windows(2)
            .any(|pair| pair[0].executable == pair[1].executable)
        {
            return defect("verified Core contains a duplicate executable identity");
        }

        let executable_ids = placements
            .iter()
            .map(|placement| placement.executable)
            .collect::<BTreeSet<_>>();
        let mut domain_kinds = BTreeMap::new();
        for requirement in planning_foundation.requirements.iter() {
            checkpoint(cancellation)?;
            let kind = discharge_domain_requirement(
                requirement,
                architecture,
                &executable_ids,
                selected_cardinality(&planning_foundation, requirement, architecture),
            )?;
            domain_kinds.insert(requirement.reference.identity, kind);
        }
        let binding_kinds = planning_foundation
            .requirements
            .iter()
            .filter_map(|requirement| match requirement.bounds {
                RequirementBounds::Binding { kind, .. } => {
                    Some((requirement.reference.identity, kind))
                }
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        let bindings = solved_bindings
            .iter()
            .map(|(requirement, slot)| {
                Ok(BindingAssignmentObservation {
                    requirement: *requirement,
                    binding: binding_kinds.get(requirement).copied().ok_or_else(|| {
                        PlanningFailure::Defect(Arc::from(
                            "solver binding does not name a binding Requirement",
                        ))
                    })?,
                    slot: *slot,
                })
            })
            .collect::<Result<Vec<_>, PlanningFailure>>()?;

        let actor_placements = flow
            .actors()
            .map(|actor| {
                if actor.max_active_turns() != 1 || actor.mailbox_capacity() == 0 {
                    return defect("Flow Actor lifetime or Mailbox bound is not realizable");
                }
                let mut handler_cores = actor
                    .handlers()
                    .map(|handler| {
                        placements
                            .iter()
                            .find(|placement| placement.executable == handler)
                            .map(|placement| placement.core)
                            .ok_or_else(|| {
                                PlanningFailure::Defect(Arc::from(
                                    "Flow Actor handler is absent from exact executable demand",
                                ))
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                handler_cores.sort_unstable();
                handler_cores.dedup();
                if handler_cores.len() > 1 {
                    return defect("one Actor's handlers received different permanent cores");
                }
                Ok((
                    actor.identity(),
                    handler_cores.first().copied().unwrap_or(0),
                ))
            })
            .collect::<Result<BTreeMap<_, _>, PlanningFailure>>()?;

        let mut flow_kinds = BTreeMap::new();
        for requirement in flow.requirements() {
            checkpoint(cancellation)?;
            if !actor_placements.contains_key(&requirement.actor()) {
                return defect("Flow Planning Requirement names an unknown Actor");
            }
            if let Some(handler) = requirement.handler()
                && !executable_ids.contains(&handler)
            {
                return defect("Flow Planning Requirement names an unrealized handler");
            }
            if requirement.site().is_some() && requirement.handler().is_none() {
                return defect("Flow site Requirement is not owned by a handler");
            }
            flow_kinds.insert(
                requirement.reference().identity(),
                discharge_flow_requirement(requirement.kind()),
            );
        }

        let discharges = requirements
            .iter()
            .map(|reference| RequirementDischargeObservation {
                source: reference.source(),
                requirement: reference.identity(),
                requirement_current_meaning: reference.current_meaning(),
                kind: match reference {
                    WholeRequirementRef::Domain(_) => domain_kinds[&reference.identity()],
                    WholeRequirementRef::Flow(_) => flow_kinds[&reference.identity()],
                },
            })
            .collect::<Vec<_>>();
        if solved_discharges.as_ref()
            != requirements
                .iter()
                .map(|requirement| requirement.identity())
                .collect::<Vec<_>>()
        {
            return defect("canonical solver did not discharge the exact Requirement Set");
        }
        let requirement_set_fingerprint = whole_requirement_set_fingerprint(
            planning_foundation.fingerprint,
            core.fingerprint(),
            flow.fingerprint(),
            &requirements,
        );
        let fingerprint = whole_assignment_fingerprint(
            requirement_set_fingerprint,
            &placements,
            &bindings,
            &discharges,
        );
        let candidate = VerifiedWholeImageAssignment {
            planning_foundation,
            core_program,
            flow_program,
            requirements: requirements.into(),
            requirement_set_fingerprint,
            placements: placements.into(),
            bindings: bindings.into(),
            discharges: discharges.into(),
            fingerprint,
            _verified: Verified,
        };
        verify_whole_image_assignment(&candidate, cancellation)?;
        Ok(WholeImageSolveOutcome::Assignment(candidate))
    }
}

fn selected_cardinality(
    planning_foundation: &VerifiedPlanningFoundation,
    requirement: &Requirement,
    architecture: crate::architecture_planning::ImagePlanningArchitecture<'_>,
) -> u32 {
    match requirement.subject {
        RequirementOwner::GeneratedRole(reference) => planning_foundation
            .generated_roles
            .iter()
            .find(|role| role.reference == reference)
            .map_or(1, |role| match role.kind {
                GeneratedRoleKind::Scheduler => {
                    u32::try_from(architecture.core_count()).unwrap_or(u32::MAX)
                }
                GeneratedRoleKind::TestRuntime => u32::try_from(
                    planning_foundation
                        .semantic_program
                        .for_image_planning()
                        .test_application_count(),
                )
                .unwrap_or(u32::MAX),
                _ => 1,
            }),
        RequirementOwner::Facility(subject) => u32::try_from(
            planning_foundation
                .domain_plans
                .iter()
                .filter(|plan| {
                    plan.facility_instance
                        .as_ref()
                        .is_some_and(|instance| instance.kind == subject.kind)
                })
                .count(),
        )
        .unwrap_or(u32::MAX),
        RequirementOwner::Pool(_) => 1,
    }
}

fn discharge_domain_requirement(
    requirement: &Requirement,
    architecture: crate::architecture_planning::ImagePlanningArchitecture<'_>,
    executable_ids: &BTreeSet<u128>,
    selected_cardinality: u32,
) -> Result<DischargeKind, PlanningFailure> {
    Ok(match &requirement.bounds {
        RequirementBounds::RealizeExactlyOnce { executable } => {
            if !executable_ids.contains(executable) {
                return defect("role-realization Requirement names an absent executable");
            }
            DischargeKind::ExecutableRealized
        }
        RequirementBounds::ImageLifetime => DischargeKind::LifetimeProved,
        RequirementBounds::Capability(capability) => {
            if !architecture.has_capability(capability.contract_kind()) {
                return defect("admitted capability Requirement is not present");
            }
            DischargeKind::CapabilityPresent
        }
        RequirementBounds::Cardinality { minimum, maximum } => {
            if *minimum > *maximum || !(*minimum..=*maximum).contains(&selected_cardinality) {
                return defect("admitted cardinality Requirement is not realized exactly");
            }
            DischargeKind::CardinalityProved
        }
        RequirementBounds::Binding {
            kind,
            minimum,
            maximum,
        } => {
            if minimum > maximum
                || (*minimum > 0 && architecture.binding_ordinal(kind.contract_kind()).is_none())
            {
                return defect("admitted binding Requirement cannot be realized");
            }
            DischargeKind::Bound
        }
        RequirementBounds::PoolCapacity {
            usable,
            peak_live,
            peak_reserved,
            peak_committed,
            ..
        } => {
            if peak_live.saturating_add(*peak_reserved) < *peak_committed || peak_committed > usable
            {
                return defect("admitted Pool capacity Requirement is inconsistent");
            }
            DischargeKind::CapacityProved
        }
        RequirementBounds::MaximumServiceUnits(_)
        | RequirementBounds::FacilityCapacity(_)
        | RequirementBounds::FacilitySharing(_) => DischargeKind::CapacityProved,
        RequirementBounds::Reservation { .. } => DischargeKind::ContractValidated,
        RequirementBounds::FacilityEndpoint { .. }
        | RequirementBounds::FacilityRecovery { .. }
        | RequirementBounds::FacilityShutdown(_)
        | RequirementBounds::FacilityReplay { .. }
        | RequirementBounds::FacilityFlagship(_)
        | RequirementBounds::FacilityBindingAvailability(_) => DischargeKind::ContractValidated,
    })
}

const fn discharge_flow_requirement(kind: FlowRequirementKind) -> DischargeKind {
    match kind {
        FlowRequirementKind::PermanentCorePlacement => DischargeKind::Placed,
        FlowRequirementKind::MailboxCapacity
        | FlowRequirementKind::ServiceStorage
        | FlowRequirementKind::ActivationStorage => DischargeKind::CapacityProved,
        FlowRequirementKind::ActorIdentity
        | FlowRequirementKind::TurnLease
        | FlowRequirementKind::SuspensionHome
        | FlowRequirementKind::ReplyEndpoint
        | FlowRequirementKind::ReplyReturnPath
        | FlowRequirementKind::ReplyResponseHome
        | FlowRequirementKind::ReplyAcyclicWait
        | FlowRequirementKind::GroupChildActivationBound
        | FlowRequirementKind::GroupCancellationAuthority
        | FlowRequirementKind::GroupOutcomePolicy
        | FlowRequirementKind::GroupResourceReturnHome
        | FlowRequirementKind::GroupCleanupOrder
        | FlowRequirementKind::DeadlineClass
        | FlowRequirementKind::DeadlineAuthority
        | FlowRequirementKind::DeadlineSlack
        | FlowRequirementKind::DeadlineFeasibility
        | FlowRequirementKind::CancellationCheckpoint
        | FlowRequirementKind::CancellationObservationWorkBound => DischargeKind::LifetimeProved,
        FlowRequirementKind::LogicalCommitOrder | FlowRequirementKind::ProposalTransport => {
            DischargeKind::ContractValidated
        }
    }
}

fn whole_requirement_set_fingerprint(
    planning_fingerprint: u128,
    core_fingerprint: u128,
    flow_fingerprint: u128,
    requirements: &[WholeRequirementRef],
) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(b"wrela.complete-requirement-set\0\x01");
    hash.update(&planning_fingerprint.to_be_bytes());
    hash.update(&core_fingerprint.to_be_bytes());
    hash.update(&flow_fingerprint.to_be_bytes());
    for requirement in requirements {
        hash.update(&[match requirement.source() {
            RequirementSource::Domain => 1,
            RequirementSource::Flow => 2,
        }]);
        hash.update(&requirement.identity().to_be_bytes());
        hash.update(&requirement.current_meaning().to_be_bytes());
    }
    hash.digest128()
}

fn whole_assignment_fingerprint(
    requirement_set_fingerprint: u128,
    placements: &[ExecutablePlacementObservation],
    bindings: &[BindingAssignmentObservation],
    discharges: &[RequirementDischargeObservation],
) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(b"wrela.whole-image-assignment\0\x01");
    hash.update(&requirement_set_fingerprint.to_be_bytes());
    for placement in placements {
        hash.update(&placement.executable.to_be_bytes());
        hash.update(&placement.executable_current_meaning.to_be_bytes());
        hash.update(&placement.core.to_be_bytes());
    }
    for binding in bindings {
        hash.update(&binding.requirement.to_be_bytes());
        hash.update(&[binding.binding.tag(), binding.slot]);
    }
    for discharge in discharges {
        hash.update(&[match discharge.source {
            RequirementSource::Domain => 1,
            RequirementSource::Flow => 2,
        }]);
        hash.update(&discharge.requirement.to_be_bytes());
        hash.update(&discharge.requirement_current_meaning.to_be_bytes());
        hash.update(&[match discharge.kind {
            DischargeKind::ExecutableRealized => 1,
            DischargeKind::Placed => 2,
            DischargeKind::Bound => 3,
            DischargeKind::CapacityProved => 4,
            DischargeKind::CapabilityPresent => 5,
            DischargeKind::CardinalityProved => 6,
            DischargeKind::LifetimeProved => 7,
            DischargeKind::ContractValidated => 8,
        }]);
    }
    hash.digest128()
}

fn verify_whole_image_assignment(
    candidate: &VerifiedWholeImageAssignment,
    cancellation: &Cancellation,
) -> Result<(), PlanningFailure> {
    checkpoint(cancellation)?;
    let core = candidate.core_program.for_image_planning();
    let flow = candidate.flow_program.for_image_planning();
    let architecture = candidate
        .planning_foundation
        .architecture_contract
        .for_image_planning();
    if candidate.planning_foundation.context != core.context_identity()
        || candidate.planning_foundation.context != flow.context_identity()
        || candidate.planning_foundation.fingerprint != flow.planning_fingerprint()
        || core.fingerprint() != flow.core_fingerprint()
    {
        return defect("whole-Image assignment verifier found mixed contexts");
    }
    let mut expected_requirements = candidate
        .planning_foundation
        .requirements
        .iter()
        .map(|requirement| WholeRequirementRef::Domain(requirement.reference))
        .chain(
            flow.requirements()
                .map(|requirement| WholeRequirementRef::Flow(requirement.reference())),
        )
        .collect::<Vec<_>>();
    expected_requirements.sort_by_key(|reference| (reference.identity(), reference.source()));
    if expected_requirements
        .windows(2)
        .any(|pair| pair[0].identity() == pair[1].identity())
        || expected_requirements.len()
            > usize::try_from(architecture.capacity().maximum_requirements).unwrap_or(usize::MAX)
        || candidate.requirements.as_ref() != expected_requirements
    {
        return defect("whole-Image assignment does not retain the complete Requirement Set");
    }
    let mut expected_executables = core
        .executables()
        .map(|executable| (executable.identity(), executable.current_meaning()))
        .collect::<Vec<_>>();
    expected_executables.sort_unstable();
    let actual_executables = candidate
        .placements
        .iter()
        .map(|placement| (placement.executable, placement.executable_current_meaning))
        .collect::<Vec<_>>();
    let core_ordinals = architecture
        .cores()
        .map(|core| core.ordinal)
        .collect::<BTreeSet<_>>();
    let canonical_core = core_ordinals.iter().next().copied().ok_or_else(|| {
        PlanningFailure::Defect(Arc::from("assignment verifier found no symbolic core"))
    })?;
    if actual_executables != expected_executables
        || candidate
            .placements
            .iter()
            .any(|placement| !core_ordinals.contains(&placement.core))
        || candidate
            .placements
            .iter()
            .any(|placement| placement.core != canonical_core)
    {
        return defect("whole-Image assignment is not the canonical exact executable placement");
    }
    let executable_ids = expected_executables
        .iter()
        .map(|(identity, _)| *identity)
        .collect::<BTreeSet<_>>();
    let expected_bindings =
        verifier_binding_assignments(&candidate.planning_foundation, architecture)?;
    if candidate.bindings.as_ref() != expected_bindings {
        return defect("whole-Image assignment does not contain the canonical exact bindings");
    }
    let mut expected_discharges = candidate
        .planning_foundation
        .requirements
        .iter()
        .map(|requirement| {
            Ok(RequirementDischargeObservation {
                source: RequirementSource::Domain,
                requirement: requirement.reference.identity,
                requirement_current_meaning: requirement.reference.current_meaning,
                kind: verifier_domain_discharge(
                    &candidate.planning_foundation,
                    requirement,
                    architecture,
                    &executable_ids,
                    &candidate.bindings,
                )?,
            })
        })
        .collect::<Result<Vec<_>, PlanningFailure>>()?;
    for requirement in flow.requirements() {
        expected_discharges.push(RequirementDischargeObservation {
            source: RequirementSource::Flow,
            requirement: requirement.reference().identity(),
            requirement_current_meaning: requirement.reference().current_meaning(),
            kind: verifier_flow_discharge(flow, requirement, architecture, &candidate.placements)?,
        });
    }
    expected_discharges.sort_by_key(|discharge| (discharge.requirement, discharge.source));
    if candidate.discharges.as_ref() != expected_discharges {
        return defect("whole-Image assignment does not discharge every Requirement exactly once");
    }
    let expected_set = verifier_requirement_set_fingerprint(
        candidate.planning_foundation.fingerprint,
        core.fingerprint(),
        flow.fingerprint(),
        &expected_requirements,
    );
    if expected_set != candidate.requirement_set_fingerprint
        || verifier_assignment_fingerprint(
            expected_set,
            &candidate.placements,
            &candidate.bindings,
            &candidate.discharges,
        ) != candidate.fingerprint
    {
        return defect("whole-Image assignment fingerprint or Requirement Set disagrees");
    }
    Ok(())
}

fn verifier_binding_assignments(
    foundation: &VerifiedPlanningFoundation,
    architecture: crate::architecture_planning::ImagePlanningArchitecture<'_>,
) -> Result<Vec<BindingAssignmentObservation>, PlanningFailure> {
    let mut requirements = foundation
        .requirements
        .iter()
        .filter_map(|requirement| match requirement.bounds {
            RequirementBounds::Binding {
                kind,
                minimum,
                maximum,
            } => Some((requirement.reference.identity, kind, minimum, maximum)),
            _ => None,
        })
        .collect::<Vec<_>>();
    requirements.sort_by_key(|requirement| requirement.0);
    let mut occupied = BTreeSet::new();
    let mut assignments = Vec::new();
    for (requirement, kind, minimum, maximum) in requirements {
        if minimum > maximum {
            return defect("assignment verifier found inverted binding cardinality");
        }
        let mut slots = architecture
            .binding_slots()
            .filter(|slot| slot.kind == kind.contract_kind())
            .map(|slot| slot.ordinal)
            .collect::<Vec<_>>();
        slots.sort_unstable();
        let selected = slots
            .into_iter()
            .filter(|slot| !occupied.contains(slot))
            .take(usize::try_from(minimum).unwrap_or(usize::MAX))
            .collect::<Vec<_>>();
        if selected.len() != usize::try_from(minimum).unwrap_or(usize::MAX) {
            return defect("assignment verifier cannot realize exact binding cardinality");
        }
        for slot in selected {
            occupied.insert(slot);
            assignments.push(BindingAssignmentObservation {
                requirement,
                binding: kind,
                slot,
            });
        }
    }
    assignments.sort_by_key(|binding| (binding.requirement, binding.slot));
    Ok(assignments)
}

fn verifier_selected_cardinality(
    foundation: &VerifiedPlanningFoundation,
    requirement: &Requirement,
    architecture: crate::architecture_planning::ImagePlanningArchitecture<'_>,
) -> u32 {
    match requirement.subject {
        RequirementOwner::GeneratedRole(reference) => foundation
            .generated_roles
            .iter()
            .find(|role| role.reference == reference)
            .map_or(1, |role| match role.kind {
                GeneratedRoleKind::Scheduler => {
                    u32::try_from(architecture.cores().count()).unwrap_or(u32::MAX)
                }
                GeneratedRoleKind::TestRuntime => u32::try_from(
                    foundation
                        .semantic_program
                        .for_image_planning()
                        .test_application_count(),
                )
                .unwrap_or(u32::MAX),
                _ => 1,
            }),
        RequirementOwner::Facility(subject) => u32::try_from(
            foundation
                .domain_plans
                .iter()
                .filter(|plan| {
                    plan.facility_instance
                        .as_ref()
                        .is_some_and(|instance| instance.kind == subject.kind)
                })
                .count(),
        )
        .unwrap_or(u32::MAX),
        RequirementOwner::Pool(_) => 1,
    }
}

fn verifier_domain_discharge(
    foundation: &VerifiedPlanningFoundation,
    requirement: &Requirement,
    architecture: crate::architecture_planning::ImagePlanningArchitecture<'_>,
    executable_ids: &BTreeSet<u128>,
    bindings: &[BindingAssignmentObservation],
) -> Result<DischargeKind, PlanningFailure> {
    match &requirement.bounds {
        RequirementBounds::RealizeExactlyOnce { executable } => executable_ids
            .contains(executable)
            .then_some(DischargeKind::ExecutableRealized)
            .ok_or_else(|| PlanningFailure::Defect(Arc::from("verifier found unrealized role"))),
        RequirementBounds::ImageLifetime => {
            let RequirementOwner::GeneratedRole(reference) = requirement.subject else {
                return defect("verifier found an unowned Image lifetime");
            };
            foundation
                .generated_roles
                .iter()
                .any(|role| {
                    role.reference == reference
                        && executable_ids.contains(&role.executable.identity)
                })
                .then_some(DischargeKind::LifetimeProved)
                .ok_or_else(|| {
                    PlanningFailure::Defect(Arc::from("verifier found false lifetime evidence"))
                })
        }
        RequirementBounds::Capability(capability) => architecture
            .has_capability(capability.contract_kind())
            .then_some(DischargeKind::CapabilityPresent)
            .ok_or_else(|| {
                PlanningFailure::Defect(Arc::from("verifier found a missing capability"))
            }),
        RequirementBounds::Cardinality { minimum, maximum } => {
            let selected = verifier_selected_cardinality(foundation, requirement, architecture);
            (*minimum <= *maximum && (*minimum..=*maximum).contains(&selected))
                .then_some(DischargeKind::CardinalityProved)
                .ok_or_else(|| {
                    PlanningFailure::Defect(Arc::from("verifier found false cardinality evidence"))
                })
        }
        RequirementBounds::MaximumServiceUnits(required) => {
            let available = architecture.cores().fold(0_u64, |total, core| {
                total.saturating_add(u64::from(core.maximum_service_units))
            });
            (u64::from(*required) <= available)
                .then_some(DischargeKind::CapacityProved)
                .ok_or_else(|| {
                    PlanningFailure::Defect(Arc::from(
                        "verifier found false service capacity evidence",
                    ))
                })
        }
        RequirementBounds::Binding {
            kind,
            minimum,
            maximum,
        } => {
            let selected = bindings
                .iter()
                .filter(|binding| {
                    binding.requirement == requirement.reference.identity
                        && binding.binding == *kind
                })
                .count();
            (*minimum <= *maximum
                && selected >= usize::try_from(*minimum).unwrap_or(usize::MAX)
                && selected <= usize::try_from(*maximum).unwrap_or(0)
                && bindings
                    .iter()
                    .filter(|binding| binding.requirement == requirement.reference.identity)
                    .all(|binding| {
                        architecture.binding_slots().any(|slot| {
                            slot.ordinal == binding.slot && slot.kind == kind.contract_kind()
                        })
                    }))
            .then_some(DischargeKind::Bound)
            .ok_or_else(|| {
                PlanningFailure::Defect(Arc::from("verifier found false binding evidence"))
            })
        }
        RequirementBounds::Reservation { kind, .. } => architecture
            .has_reservation(kind.contract_kind())
            .then_some(DischargeKind::ContractValidated)
            .ok_or_else(|| {
                PlanningFailure::Defect(Arc::from(
                    "verifier found a missing reservation prerequisite",
                ))
            }),
        RequirementBounds::PoolCapacity {
            usable,
            peak_live,
            peak_reserved,
            peak_committed,
            ..
        } => (peak_live.saturating_add(*peak_reserved) >= *peak_committed
            && peak_committed <= usable)
            .then_some(DischargeKind::CapacityProved)
            .ok_or_else(|| {
                PlanningFailure::Defect(Arc::from("verifier found false Pool capacity evidence"))
            }),
        RequirementBounds::FacilitySharing(FacilitySharing::RegisteredDisjoint {
            role,
            maximum_units,
        }) => architecture
            .facility_share(role.architecture_kind())
            .is_some_and(|registration| registration.maximum_units >= *maximum_units)
            .then_some(DischargeKind::CapacityProved)
            .ok_or_else(|| {
                PlanningFailure::Defect(Arc::from("verifier found false Facility sharing evidence"))
            }),
        RequirementBounds::FacilityCapacity(_)
        | RequirementBounds::FacilitySharing(FacilitySharing::Exclusive) => {
            Ok(DischargeKind::CapacityProved)
        }
        RequirementBounds::FacilityEndpoint { .. }
        | RequirementBounds::FacilityRecovery { .. }
        | RequirementBounds::FacilityShutdown(_)
        | RequirementBounds::FacilityReplay { .. }
        | RequirementBounds::FacilityFlagship(_)
        | RequirementBounds::FacilityBindingAvailability(_) => Ok(DischargeKind::ContractValidated),
    }
}

fn verifier_flow_discharge(
    flow: crate::flow::ImagePlanningFlowView<'_>,
    requirement: crate::flow::ImagePlanningFlowRequirement<'_>,
    architecture: crate::architecture_planning::ImagePlanningArchitecture<'_>,
    placements: &[ExecutablePlacementObservation],
) -> Result<DischargeKind, PlanningFailure> {
    let actor = flow
        .actors()
        .find(|actor| actor.identity() == requirement.actor())
        .ok_or_else(|| PlanningFailure::Defect(Arc::from("verifier found a foreign Flow Actor")))?;
    if let Some(handler) = requirement.handler()
        && !placements
            .iter()
            .any(|placement| placement.executable == handler)
    {
        return defect("verifier found a Flow Requirement with an unrealized handler");
    }
    match requirement.kind() {
        FlowRequirementKind::PermanentCorePlacement => {
            let mut cores = actor.handlers().filter_map(|handler| {
                placements
                    .iter()
                    .find(|placement| placement.executable == handler)
                    .map(|placement| placement.core)
            });
            let first = cores.next();
            if cores.any(|core| Some(core) != first) {
                return defect("verifier found false Actor affinity evidence");
            }
            Ok(DischargeKind::Placed)
        }
        FlowRequirementKind::MailboxCapacity => (requirement.bound() <= actor.mailbox_capacity())
            .then_some(DischargeKind::CapacityProved)
            .ok_or_else(|| {
                PlanningFailure::Defect(Arc::from("verifier found false Mailbox capacity evidence"))
            }),
        FlowRequirementKind::TurnLease => (actor.max_active_turns() == 1
            && u64::from(actor.max_active_turns()) <= requirement.bound())
        .then_some(DischargeKind::LifetimeProved)
        .ok_or_else(|| {
            PlanningFailure::Defect(Arc::from("verifier found false Turn lifetime evidence"))
        }),
        FlowRequirementKind::SuspensionHome | FlowRequirementKind::ActivationStorage => {
            (requirement.handler().is_some()
                && requirement.bound()
                    <= u64::from(architecture.capacity().maximum_activation_homes))
            .then_some(
                if requirement.kind() == FlowRequirementKind::ActivationStorage {
                    DischargeKind::CapacityProved
                } else {
                    DischargeKind::LifetimeProved
                },
            )
            .ok_or_else(|| {
                PlanningFailure::Defect(Arc::from("verifier found false activation evidence"))
            })
        }
        FlowRequirementKind::ServiceStorage => (requirement.bound()
            <= u64::from(architecture.capacity().maximum_activation_homes))
        .then_some(DischargeKind::CapacityProved)
        .ok_or_else(|| PlanningFailure::Defect(Arc::from("verifier found false storage evidence"))),
        FlowRequirementKind::ActorIdentity
        | FlowRequirementKind::ReplyEndpoint
        | FlowRequirementKind::ReplyReturnPath
        | FlowRequirementKind::ReplyResponseHome
        | FlowRequirementKind::ReplyAcyclicWait
        | FlowRequirementKind::GroupChildActivationBound
        | FlowRequirementKind::GroupCancellationAuthority
        | FlowRequirementKind::GroupOutcomePolicy
        | FlowRequirementKind::GroupResourceReturnHome
        | FlowRequirementKind::GroupCleanupOrder
        | FlowRequirementKind::DeadlineClass
        | FlowRequirementKind::DeadlineAuthority
        | FlowRequirementKind::DeadlineSlack
        | FlowRequirementKind::DeadlineFeasibility
        | FlowRequirementKind::CancellationCheckpoint
        | FlowRequirementKind::CancellationObservationWorkBound => {
            Ok(DischargeKind::LifetimeProved)
        }
        FlowRequirementKind::LogicalCommitOrder | FlowRequirementKind::ProposalTransport => {
            Ok(DischargeKind::ContractValidated)
        }
    }
}

fn verifier_requirement_set_fingerprint(
    planning_fingerprint: u128,
    core_fingerprint: u128,
    flow_fingerprint: u128,
    requirements: &[WholeRequirementRef],
) -> u128 {
    let mut verifier = Xxh3::new();
    verifier.update(b"wrela.complete-requirement-set\0\x01");
    verifier.update(&planning_fingerprint.to_be_bytes());
    verifier.update(&core_fingerprint.to_be_bytes());
    verifier.update(&flow_fingerprint.to_be_bytes());
    for requirement in requirements {
        verifier.update(&[if requirement.source() == RequirementSource::Domain {
            1
        } else {
            2
        }]);
        verifier.update(&requirement.identity().to_be_bytes());
        verifier.update(&requirement.current_meaning().to_be_bytes());
    }
    verifier.digest128()
}

fn verifier_assignment_fingerprint(
    requirement_set_fingerprint: u128,
    placements: &[ExecutablePlacementObservation],
    bindings: &[BindingAssignmentObservation],
    discharges: &[RequirementDischargeObservation],
) -> u128 {
    let mut verifier = Xxh3::new();
    verifier.update(b"wrela.whole-image-assignment\0\x01");
    verifier.update(&requirement_set_fingerprint.to_be_bytes());
    for placement in placements {
        verifier.update(&placement.executable.to_be_bytes());
        verifier.update(&placement.executable_current_meaning.to_be_bytes());
        verifier.update(&placement.core.to_be_bytes());
    }
    for binding in bindings {
        verifier.update(&binding.requirement.to_be_bytes());
        verifier.update(&[binding.binding.tag(), binding.slot]);
    }
    for discharge in discharges {
        verifier.update(&[if discharge.source == RequirementSource::Domain {
            1
        } else {
            2
        }]);
        verifier.update(&discharge.requirement.to_be_bytes());
        verifier.update(&discharge.requirement_current_meaning.to_be_bytes());
        verifier.update(&[match discharge.kind {
            DischargeKind::ExecutableRealized => 1,
            DischargeKind::Placed => 2,
            DischargeKind::Bound => 3,
            DischargeKind::CapacityProved => 4,
            DischargeKind::CapabilityPresent => 5,
            DischargeKind::CardinalityProved => 6,
            DischargeKind::LifetimeProved => 7,
            DischargeKind::ContractValidated => 8,
        }]);
    }
    verifier.digest128()
}

fn validate_facility_cardinality(
    instances: &[FacilityInstanceRef],
    contracts: &[FacilityContract],
    cancellation: &Cancellation,
) -> Result<(), PlanningFailure> {
    for contract in contracts {
        checkpoint(cancellation)?;
        let kind = contract.observation.kind;
        let selected = checked_u32(
            instances
                .iter()
                .filter(|instance| instance.kind == kind)
                .count(),
        )?;
        if selected < contract.observation.minimum_instances
            || selected > contract.observation.maximum_instances
        {
            return Err(PlanningFailure::FacilityCardinality {
                kind,
                selected,
                minimum: contract.observation.minimum_instances,
                maximum: contract.observation.maximum_instances,
                sites: instances
                    .iter()
                    .filter(|instance| instance.kind == kind)
                    .map(|instance| FacilityConflictSite {
                        identity: instance.identity,
                        source: instance.source.clone(),
                    })
                    .collect::<Vec<_>>()
                    .into(),
            });
        }
    }
    Ok(())
}

fn check_facility_compatibility(
    architecture: crate::architecture_planning::ImagePlanningArchitecture<'_>,
    instance: &FacilityInstanceRef,
    requirements: &[Requirement],
    cancellation: &Cancellation,
) -> Result<(), PlanningFailure> {
    for requirement in requirements {
        checkpoint(cancellation)?;
        let missing = match requirement.bounds {
            RequirementBounds::Capability(capability)
                if !architecture.has_capability(capability.contract_kind()) =>
            {
                Some(FacilityCompatibilityMissing::Capability(capability))
            }
            RequirementBounds::Binding { kind, .. }
                if !architecture.has_binding(kind.contract_kind()) =>
            {
                Some(FacilityCompatibilityMissing::Binding(kind))
            }
            RequirementBounds::FacilitySharing(FacilitySharing::RegisteredDisjoint {
                role,
                maximum_units,
            }) => {
                let available = architecture
                    .facility_share(role.architecture_kind())
                    .map_or(0, |registration| registration.maximum_units);
                (available < maximum_units).then_some(FacilityCompatibilityMissing::SharedRole {
                    role,
                    required_units: maximum_units,
                    available_units: available,
                })
            }
            _ => None,
        };
        if let Some(missing) = missing {
            return Err(PlanningFailure::FacilityCompatibility {
                kind: instance.kind,
                instance: instance.identity,
                source: instance.source.clone(),
                requirement: requirement.reference,
                missing,
            });
        }
    }
    Ok(())
}

fn produce_planner(
    context: u128,
    root: Root,
    semantic_fingerprint: u128,
    architecture_fingerprint: u128,
) -> Planner {
    let identity = producer_hash(b"wrela.planner.image-kind.v1", &[root_tag(root).into()]);
    Planner {
        reference: PlannerRef {
            context,
            identity,
            current_meaning: producer_hash(
                b"wrela.planner.image-kind.meaning.v1",
                &[
                    identity,
                    root_tag(root).into(),
                    semantic_fingerprint,
                    architecture_fingerprint,
                ],
            ),
        },
        kind: PlannerKind::ImageKind,
    }
}

fn produce_domain_plan(
    context: u128,
    planner: PlannerRef,
    semantic_fingerprint: u128,
    architecture_fingerprint: u128,
) -> DomainPlan {
    let identity = producer_hash(
        b"wrela.domain-plan.mandatory-image.v1",
        &[planner.identity, 1],
    );
    DomainPlan {
        reference: DomainPlanRef {
            context,
            identity,
            current_meaning: producer_hash(
                b"wrela.domain-plan.mandatory-image.meaning.v1",
                &[
                    identity,
                    planner.current_meaning,
                    semantic_fingerprint,
                    architecture_fingerprint,
                ],
            ),
        },
        planner,
        kind: DomainPlanKind::MandatoryImage,
        generated_roles: Arc::from([]),
        requirements: Arc::from([]),
        facility_instance: None,
        facility_contract: None,
    }
}

fn produce_facility_planner(
    context: u128,
    instance: FacilityInstanceRef,
    contract: &FacilityContract,
    architecture_fingerprint: u128,
) -> Planner {
    let identity = producer_hash(
        b"wrela.planner.facility.v1",
        &[u128::from(instance.kind.tag()), instance.identity],
    );
    Planner {
        reference: PlannerRef {
            context,
            identity,
            current_meaning: producer_hash(
                b"wrela.planner.facility.meaning.v1",
                &[
                    identity,
                    instance.current_meaning,
                    contract.observation.fingerprint,
                    architecture_fingerprint,
                ],
            ),
        },
        kind: PlannerKind::Facility(instance.kind),
    }
}

fn produce_facility_domain_plan(
    context: u128,
    instance: FacilityInstanceRef,
    planner: PlannerRef,
    contract: &FacilityContract,
) -> DomainPlan {
    let identity = producer_hash(
        b"wrela.domain-plan.facility.v1",
        &[
            planner.identity,
            instance.identity,
            u128::from(instance.kind.tag()),
        ],
    );
    DomainPlan {
        reference: DomainPlanRef {
            context,
            identity,
            current_meaning: producer_hash(
                b"wrela.domain-plan.facility.meaning.v1",
                &[
                    identity,
                    planner.current_meaning,
                    instance.current_meaning,
                    contract.observation.fingerprint,
                ],
            ),
        },
        planner,
        kind: DomainPlanKind::Facility(instance.kind),
        generated_roles: Arc::from([]),
        requirements: Arc::from([]),
        facility_instance: Some(instance),
        facility_contract: Some(contract.reference),
    }
}

fn produce_facility_roles(
    context: u128,
    planner: PlannerRef,
    plan: DomainPlanRef,
    contract: &FacilityContract,
    architecture_fingerprint: u128,
    cancellation: &Cancellation,
) -> Result<Vec<GeneratedRole>, PlanningFailure> {
    let mut roles = Vec::new();
    for (ordinal, kind) in contract
        .observation
        .generated_roles
        .iter()
        .copied()
        .enumerate()
    {
        checkpoint(cancellation)?;
        let mut dependencies = Vec::new();
        if kind == GeneratedRoleKind::EventStoreDriver {
            let runtime = roles
                .iter()
                .find(|role: &&GeneratedRole| role.kind == GeneratedRoleKind::EventStoreRuntime)
                .map(|role| role.reference)
                .ok_or_else(|| {
                    PlanningFailure::Defect(Arc::from("Event Store runtime role is missing"))
                })?;
            dependencies.push(runtime);
        }
        dependencies.sort_by_key(|reference| reference.identity);
        let local_key = u16::try_from(ordinal + 1)
            .map_err(|_| PlanningFailure::Defect(Arc::from("Facility role key overflow")))?;
        let identity = produce_role_identity(planner, kind, local_key);
        let current_meaning = produce_role_current_meaning(
            identity,
            plan,
            &dependencies,
            contract.observation.current_meaning,
            architecture_fingerprint,
        );
        roles.push(GeneratedRole {
            reference: RoleRef {
                context,
                identity,
                current_meaning,
            },
            executable: ExecutableRef {
                context,
                identity: producer_hash(b"wrela.generated-executable.v1", &[identity]),
                current_meaning,
            },
            owner: planner,
            generator: planner,
            kind,
            local_key,
            dependencies: dependencies.into(),
            provenance: plan,
        });
    }
    roles.sort_by_key(|role| role.reference.identity);
    Ok(roles)
}

fn produce_facility_requirements(
    context: u128,
    planner: PlannerRef,
    plan: DomainPlanRef,
    instance: &FacilityInstanceRef,
    roles: &[GeneratedRole],
    contract: &FacilityContract,
    cancellation: &Cancellation,
) -> Result<Vec<Requirement>, PlanningFailure> {
    let mut requirements = Vec::new();
    for (role_ordinal, kind) in contract.observation.generated_roles.iter().enumerate() {
        let role = roles
            .iter()
            .find(|role| role.kind == *kind)
            .ok_or_else(|| PlanningFailure::Defect(Arc::from("Facility role is missing")))?;
        for spec in [
            RequirementSpec {
                category: RequirementCategory::GeneratedRoleRealization,
                bounds: RequirementBounds::RealizeExactlyOnce {
                    executable: role.executable.identity,
                },
            },
            RequirementSpec {
                category: RequirementCategory::Lifetime,
                bounds: RequirementBounds::ImageLifetime,
            },
        ] {
            let local_site = u16::try_from(100 + role_ordinal * 2 + requirements.len() % 2)
                .map_err(|_| {
                    PlanningFailure::Defect(Arc::from("Facility requirement site overflow"))
                })?;
            requirements.push(produce_requirement(
                context,
                planner,
                plan,
                role.reference,
                local_site,
                spec,
            ));
        }
    }
    let subject = FacilitySubjectRef {
        context: instance.context,
        identity: instance.identity,
        current_meaning: instance.current_meaning,
        kind: instance.kind,
    };
    let mut specs = vec![
        (
            1,
            RequirementSpec {
                category: RequirementCategory::Cardinality,
                bounds: RequirementBounds::Cardinality {
                    minimum: contract.observation.minimum_instances,
                    maximum: contract.observation.maximum_instances,
                },
            },
        ),
        (
            2,
            RequirementSpec {
                category: RequirementCategory::Binding,
                bounds: RequirementBounds::Binding {
                    kind: contract
                        .observation
                        .external_binding
                        .expect("verified Facility binding"),
                    minimum: 1,
                    maximum: contract.observation.maximum_exported_endpoints,
                },
            },
        ),
        (
            3,
            RequirementSpec {
                category: RequirementCategory::Binding,
                bounds: RequirementBounds::FacilitySharing(contract.observation.sharing),
            },
        ),
    ];
    specs.extend(
        contract
            .observation
            .required_capabilities
            .iter()
            .copied()
            .enumerate()
            .map(|(ordinal, capability)| {
                (
                    10 + u16::try_from(ordinal).unwrap_or(u16::MAX),
                    RequirementSpec {
                        category: RequirementCategory::ArchitectureCapability,
                        bounds: RequirementBounds::Capability(capability),
                    },
                )
            }),
    );
    specs.extend(
        contract
            .observation
            .semantic_capacities
            .iter()
            .copied()
            .enumerate()
            .map(|(ordinal, capacity)| {
                (
                    30 + u16::try_from(ordinal).unwrap_or(u16::MAX),
                    RequirementSpec {
                        category: RequirementCategory::CapacityPressure,
                        bounds: RequirementBounds::FacilityCapacity(capacity),
                    },
                )
            }),
    );
    specs.extend([
        (
            50,
            RequirementSpec {
                category: RequirementCategory::FacilityOwnership,
                bounds: RequirementBounds::FacilityEndpoint {
                    maximum: contract.observation.maximum_exported_endpoints,
                    ownership: contract.observation.endpoint_ownership,
                    input_owner: instance.input_owner,
                },
            },
        ),
        (
            51,
            RequirementSpec {
                category: RequirementCategory::Recovery,
                bounds: RequirementBounds::FacilityRecovery {
                    supervisor: instance.supervisor,
                    loss_policy: instance.selected_loss_policy,
                    maximum_attempts: contract.observation.maximum_recovery_attempts,
                },
            },
        ),
        (
            52,
            RequirementSpec {
                category: RequirementCategory::Shutdown,
                bounds: RequirementBounds::FacilityShutdown(contract.observation.shutdown),
            },
        ),
        (
            53,
            RequirementSpec {
                category: RequirementCategory::Replay,
                bounds: RequirementBounds::FacilityReplay {
                    selected: instance.replay_authority,
                    rule: contract.observation.replay_rule,
                },
            },
        ),
        (
            54,
            RequirementSpec {
                category: RequirementCategory::Flagship,
                bounds: RequirementBounds::FacilityFlagship(contract.observation.flagship_rule),
            },
        ),
        (
            55,
            RequirementSpec {
                category: RequirementCategory::BootAvailability,
                bounds: RequirementBounds::FacilityBindingAvailability(
                    contract.observation.binding_availability,
                ),
            },
        ),
    ]);
    for (local_site, spec) in specs {
        checkpoint(cancellation)?;
        requirements.push(produce_facility_requirement(
            context,
            planner,
            plan,
            subject,
            contract.reference,
            local_site,
            spec,
        ));
    }
    requirements.sort_by_key(|requirement| requirement.reference.identity);
    Ok(requirements)
}

fn produce_facility_requirement(
    context: u128,
    owner: PlannerRef,
    plan: DomainPlanRef,
    subject: FacilitySubjectRef,
    contract: FacilityContractRef,
    local_site: u16,
    spec: RequirementSpec,
) -> Requirement {
    let identity =
        produce_requirement_identity(owner.identity, subject.identity, spec.category, local_site);
    let current_meaning = produce_requirement_current_meaning(
        identity,
        owner.current_meaning,
        subject.current_meaning,
        plan.current_meaning,
        spec.category,
        &spec.bounds,
    );
    Requirement {
        context,
        reference: RequirementRef {
            context,
            identity,
            current_meaning,
        },
        owner,
        subject: RequirementOwner::Facility(subject),
        provenance: RequirementProvenance {
            domain_plan: plan.identity,
            generated_role: subject.identity,
            local_site,
        },
        category: spec.category,
        bounds: spec.bounds,
        facility_contract: Some(contract),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RoleSpec {
    kind: GeneratedRoleKind,
    dependencies: Arc<[GeneratedRoleKind]>,
}

fn producer_role_specs(root: Root) -> Vec<RoleSpec> {
    let mut roles = vec![
        RoleSpec {
            kind: GeneratedRoleKind::Boot,
            dependencies: Arc::from([]),
        },
        RoleSpec {
            kind: GeneratedRoleKind::Scheduler,
            dependencies: Arc::from([GeneratedRoleKind::Boot]),
        },
        RoleSpec {
            kind: GeneratedRoleKind::Terminal,
            dependencies: Arc::from([GeneratedRoleKind::Boot]),
        },
        RoleSpec {
            kind: GeneratedRoleKind::Panic,
            dependencies: Arc::from([GeneratedRoleKind::Terminal]),
        },
        RoleSpec {
            kind: GeneratedRoleKind::Shutdown,
            dependencies: Arc::from([GeneratedRoleKind::Scheduler, GeneratedRoleKind::Terminal]),
        },
    ];
    if root == Root::Test {
        roles.push(RoleSpec {
            kind: GeneratedRoleKind::TestRuntime,
            dependencies: Arc::from([
                GeneratedRoleKind::Scheduler,
                GeneratedRoleKind::Terminal,
                GeneratedRoleKind::Shutdown,
            ]),
        });
    }
    roles
}

fn produce_roles(
    context: u128,
    root: Root,
    planner: PlannerRef,
    plan: DomainPlanRef,
    semantic_fingerprint: u128,
    architecture_fingerprint: u128,
    cancellation: &Cancellation,
) -> Result<Vec<GeneratedRole>, PlanningFailure> {
    let mut roles = Vec::<GeneratedRole>::new();
    for (ordinal, spec) in producer_role_specs(root).into_iter().enumerate() {
        checkpoint(cancellation)?;
        let mut dependencies = spec
            .dependencies
            .iter()
            .map(|kind| {
                roles
                    .iter()
                    .find(|role| role.kind == *kind)
                    .map(|role| role.reference)
                    .ok_or_else(|| {
                        PlanningFailure::Defect(Arc::from(
                            "generated role dependency is not closed",
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        dependencies.sort_by_key(|reference| reference.identity);
        let local_key = u16::try_from(ordinal + 1)
            .map_err(|_| PlanningFailure::Defect(Arc::from("generated role local key overflow")))?;
        let identity = produce_role_identity(planner, spec.kind, local_key);
        let current_meaning = produce_role_current_meaning(
            identity,
            plan,
            &dependencies,
            semantic_fingerprint,
            architecture_fingerprint,
        );
        roles.push(GeneratedRole {
            reference: RoleRef {
                context,
                identity,
                current_meaning,
            },
            executable: ExecutableRef {
                context,
                identity: producer_hash(b"wrela.generated-executable.v1", &[identity]),
                current_meaning,
            },
            owner: planner,
            generator: planner,
            kind: spec.kind,
            local_key,
            dependencies: dependencies.into(),
            provenance: plan,
        });
    }
    roles.sort_by_key(|role| role.reference.identity);
    Ok(roles)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RequirementSpec {
    category: RequirementCategory,
    bounds: RequirementBounds,
}

fn producer_requirement_specs(
    role: &GeneratedRole,
    test_application_count: usize,
    core_count: usize,
    maximum_cycle_units: u32,
) -> Result<Vec<RequirementSpec>, PlanningFailure> {
    let mut specs = vec![
        RequirementSpec {
            category: RequirementCategory::GeneratedRoleRealization,
            bounds: RequirementBounds::RealizeExactlyOnce {
                executable: role.executable.identity,
            },
        },
        RequirementSpec {
            category: RequirementCategory::Lifetime,
            bounds: RequirementBounds::ImageLifetime,
        },
    ];
    match role.kind {
        GeneratedRoleKind::Boot => specs.extend([
            RequirementSpec {
                category: RequirementCategory::ArchitectureCapability,
                bounds: RequirementBounds::Capability(PlanningCapability::SecondaryCoreStartup),
            },
            RequirementSpec {
                category: RequirementCategory::LogicalLayout,
                bounds: RequirementBounds::Reservation {
                    kind: PlanningReservation::BootState,
                    multiplicity: PlanningMultiplicity::Once,
                },
            },
        ]),
        GeneratedRoleKind::Scheduler => specs.extend([
            RequirementSpec {
                category: RequirementCategory::Cardinality,
                bounds: RequirementBounds::Cardinality {
                    minimum: checked_u32(core_count)?,
                    maximum: checked_u32(core_count)?,
                },
            },
            RequirementSpec {
                category: RequirementCategory::Service,
                bounds: RequirementBounds::MaximumServiceUnits(maximum_cycle_units),
            },
        ]),
        GeneratedRoleKind::Terminal => specs.extend([
            RequirementSpec {
                category: RequirementCategory::ArchitectureCapability,
                bounds: RequirementBounds::Capability(PlanningCapability::TypedTerminalLifecycle),
            },
            RequirementSpec {
                category: RequirementCategory::Binding,
                bounds: RequirementBounds::Binding {
                    kind: PlanningBinding::Terminal,
                    minimum: 1,
                    maximum: 1,
                },
            },
            RequirementSpec {
                category: RequirementCategory::LogicalLayout,
                bounds: RequirementBounds::Reservation {
                    kind: PlanningReservation::TerminalTransport,
                    multiplicity: PlanningMultiplicity::Once,
                },
            },
        ]),
        GeneratedRoleKind::Panic => specs.extend([
            RequirementSpec {
                category: RequirementCategory::ArchitectureCapability,
                bounds: RequirementBounds::Capability(PlanningCapability::PanicPulse),
            },
            RequirementSpec {
                category: RequirementCategory::Binding,
                bounds: RequirementBounds::Binding {
                    kind: PlanningBinding::Panic,
                    minimum: 1,
                    maximum: 1,
                },
            },
            RequirementSpec {
                category: RequirementCategory::LogicalLayout,
                bounds: RequirementBounds::Reservation {
                    kind: PlanningReservation::PanicState,
                    multiplicity: PlanningMultiplicity::PerCore,
                },
            },
        ]),
        GeneratedRoleKind::Shutdown => specs.push(RequirementSpec {
            category: RequirementCategory::ArchitectureCapability,
            bounds: RequirementBounds::Capability(PlanningCapability::GuestShutdownPulse),
        }),
        GeneratedRoleKind::TestRuntime => specs.push(RequirementSpec {
            category: RequirementCategory::Cardinality,
            bounds: RequirementBounds::Cardinality {
                minimum: checked_u32(test_application_count)?,
                maximum: checked_u32(test_application_count)?,
            },
        }),
        GeneratedRoleKind::DisplayDriver
        | GeneratedRoleKind::InputDriver
        | GeneratedRoleKind::EventStoreRuntime
        | GeneratedRoleKind::EventStoreDriver
        | GeneratedRoleKind::MonotonicClockDriver
        | GeneratedRoleKind::EntropyDriver
        | GeneratedRoleKind::TelemetryDriver => {}
    }
    Ok(specs)
}

#[allow(clippy::too_many_arguments)]
fn produce_requirements(
    context: u128,
    roles: &[GeneratedRole],
    planner: PlannerRef,
    plan: DomainPlanRef,
    test_application_count: usize,
    core_count: usize,
    maximum_cycle_units: u32,
    cancellation: &Cancellation,
) -> Result<Vec<Requirement>, PlanningFailure> {
    let mut requirements = Vec::new();
    for role in roles {
        for (site, spec) in producer_requirement_specs(
            role,
            test_application_count,
            core_count,
            maximum_cycle_units,
        )?
        .into_iter()
        .enumerate()
        {
            checkpoint(cancellation)?;
            requirements.push(produce_requirement(
                context,
                planner,
                plan,
                role.reference,
                u16::try_from(site + 1)
                    .map_err(|_| PlanningFailure::Defect(Arc::from("requirement site overflow")))?,
                spec,
            ));
        }
    }
    requirements.sort_by_key(|requirement| requirement.reference.identity);
    Ok(requirements)
}

fn produce_requirement(
    context: u128,
    owner: PlannerRef,
    plan: DomainPlanRef,
    subject: RoleRef,
    local_site: u16,
    spec: RequirementSpec,
) -> Requirement {
    let reference =
        produce_requirement_identity(owner.identity, subject.identity, spec.category, local_site);
    let provenance = RequirementProvenance {
        domain_plan: plan.identity,
        generated_role: subject.identity,
        local_site,
    };
    let current_meaning = produce_requirement_current_meaning(
        reference,
        owner.current_meaning,
        subject.current_meaning,
        plan.current_meaning,
        spec.category,
        &spec.bounds,
    );
    Requirement {
        context,
        reference: RequirementRef {
            context,
            identity: reference,
            current_meaning,
        },
        owner,
        subject: RequirementOwner::GeneratedRole(subject),
        provenance,
        category: spec.category,
        bounds: spec.bounds,
        facility_contract: None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PoolFlowState {
    live: u64,
    reserved: u64,
    transient_permits: u64,
    permits: BTreeMap<LocalId, u64>,
}

#[derive(Default)]
struct PoolFlowSummary {
    peak_live: u64,
    peak_reserved: u64,
    peak_committed: u64,
    admission_sites: Vec<(u128, PoolOperation, SourceRange, u128)>,
    active_calls: BTreeMap<SpecializationId, u64>,
    current_executable: u128,
}

fn produce_pool_plans(
    context: u128,
    semantic: CorePlanningSemanticProgram<'_>,
    planner: PlannerRef,
    plan: DomainPlanRef,
    cancellation: &Cancellation,
) -> Result<(Vec<PoolPlan>, Vec<Requirement>), PlanningFailure> {
    let mut pools = Vec::new();
    let mut requirements = Vec::new();
    for reference in semantic.exact_source_executables() {
        checkpoint(cancellation)?;
        let input = semantic.executable_input(reference).ok_or_else(|| {
            PlanningFailure::Defect(Arc::from(
                "Pool planning demand names a missing source body",
            ))
        })?;
        let statements = match input.body {
            CoreSourceExecutableBody::Specialization(function) => Some(function.body.as_ref()),
            CoreSourceExecutableBody::Test(test) => Some(test.body.as_ref()),
            CoreSourceExecutableBody::Closure(_) => None,
        };
        if let Some(statements) = statements {
            collect_pool_plans(
                context,
                reference,
                statements,
                semantic.verified_program(),
                planner,
                plan,
                cancellation,
                &mut pools,
                &mut requirements,
            )?;
        }
    }
    pools.sort_by_key(|pool| pool.reference.identity);
    requirements.sort_by_key(|requirement| requirement.reference.identity);
    Ok((pools, requirements))
}

#[allow(clippy::too_many_arguments)]
fn collect_pool_plans(
    context: u128,
    executable: CoreSourceExecutableRef,
    statements: &[Statement],
    program: &VerifiedProgram,
    planner: PlannerRef,
    plan: DomainPlanRef,
    cancellation: &Cancellation,
    pools: &mut Vec<PoolPlan>,
    requirements: &mut Vec<Requirement>,
) -> Result<(), PlanningFailure> {
    for statement in statements {
        checkpoint(cancellation)?;
        match statement {
            Statement::WithPool {
                pool,
                binding,
                scope,
                body,
                source,
            } => {
                let declared = pool_declared_capacity(scope).ok_or_else(|| {
                    PlanningFailure::Defect(Arc::from(
                        "authenticated Pool capacity is not an exact u64 value",
                    ))
                })?;
                let mut summary = PoolFlowSummary {
                    current_executable: executable.identity(),
                    ..PoolFlowSummary::default()
                };
                let states = BTreeSet::from([PoolFlowState {
                    live: 0,
                    reserved: 0,
                    transient_permits: 0,
                    permits: BTreeMap::new(),
                }]);
                let _ = pool_flow_statements(
                    body,
                    states,
                    binding.local,
                    declared,
                    program,
                    &mut summary,
                    cancellation,
                )?;
                let identity = pool.0;
                let current_meaning = producer_hash(
                    b"wrela.pool-plan.meaning.v1",
                    &[
                        identity,
                        executable.current_meaning(),
                        u128::from(declared),
                        u128::from(summary.peak_live),
                        u128::from(summary.peak_reserved),
                        u128::from(summary.peak_committed),
                    ],
                );
                let pool_ref = PoolRef {
                    context,
                    identity,
                    current_meaning,
                };
                let mut admission_sites = Vec::new();
                for (ordinal, (site_executable, operation, source, source_type_identity)) in
                    summary.admission_sites.into_iter().enumerate()
                {
                    let local_site = u16::try_from(ordinal + 1).map_err(|_| {
                        PlanningFailure::Defect(Arc::from("Pool admission site overflow"))
                    })?;
                    let bounds = RequirementBounds::PoolCapacity {
                        declared,
                        usable: declared,
                        peak_live: summary.peak_live,
                        peak_reserved: summary.peak_reserved,
                        peak_committed: summary.peak_committed,
                    };
                    let reference = produce_requirement_identity(
                        planner.identity,
                        pool_ref.identity,
                        RequirementCategory::CapacityPressure,
                        local_site,
                    );
                    let current_meaning = produce_requirement_current_meaning(
                        reference,
                        planner.current_meaning,
                        pool_ref.current_meaning,
                        plan.current_meaning,
                        RequirementCategory::CapacityPressure,
                        &bounds,
                    );
                    let requirement_ref = RequirementRef {
                        context,
                        identity: reference,
                        current_meaning,
                    };
                    let provenance = RequirementProvenance {
                        domain_plan: plan.identity,
                        generated_role: 0,
                        local_site,
                    };
                    requirements.push(Requirement {
                        context,
                        reference: requirement_ref,
                        owner: planner,
                        subject: RequirementOwner::Pool(pool_ref),
                        provenance,
                        category: RequirementCategory::CapacityPressure,
                        bounds,
                        facility_contract: None,
                    });
                    admission_sites.push(PoolAdmissionSite {
                        executable_identity: site_executable,
                        operation,
                        source,
                        source_type_identity,
                        requirement: requirement_ref,
                    });
                }
                pools.push(PoolPlan {
                    reference: pool_ref,
                    executable,
                    source: source.clone(),
                    declared_capacity: declared,
                    usable_slots: declared,
                    peak_live: summary.peak_live,
                    peak_reserved: summary.peak_reserved,
                    peak_committed: summary.peak_committed,
                    admission_sites: admission_sites.into(),
                });
                collect_pool_plans(
                    context,
                    executable,
                    body,
                    program,
                    planner,
                    plan,
                    cancellation,
                    pools,
                    requirements,
                )?;
            }
            Statement::If {
                then_branch,
                else_branch,
                ..
            }
            | Statement::IfPattern {
                then_branch,
                else_branch,
                ..
            } => {
                collect_pool_plans(
                    context,
                    executable,
                    then_branch,
                    program,
                    planner,
                    plan,
                    cancellation,
                    pools,
                    requirements,
                )?;
                collect_pool_plans(
                    context,
                    executable,
                    else_branch,
                    program,
                    planner,
                    plan,
                    cancellation,
                    pools,
                    requirements,
                )?;
            }
            Statement::For { body, .. } | Statement::While { body, .. } => collect_pool_plans(
                context,
                executable,
                body,
                program,
                planner,
                plan,
                cancellation,
                pools,
                requirements,
            )?,
            Statement::Match { cases, .. } => {
                for case in cases.iter() {
                    collect_pool_plans(
                        context,
                        executable,
                        &case.body,
                        program,
                        planner,
                        plan,
                        cancellation,
                        pools,
                        requirements,
                    )?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn pool_declared_capacity(scope: &Expression) -> Option<u64> {
    let ExpressionKind::Call { arguments, .. } = &scope.kind else {
        return None;
    };
    arguments.iter().find_map(|argument| match argument.kind {
        ExpressionKind::Literal(Literal::Integer { value, .. }) => u64::try_from(value).ok(),
        _ => None,
    })
}

#[allow(clippy::too_many_lines)]
fn pool_flow_statements(
    statements: &[Statement],
    mut states: BTreeSet<PoolFlowState>,
    binding: LocalId,
    capacity: u64,
    program: &VerifiedProgram,
    summary: &mut PoolFlowSummary,
    cancellation: &Cancellation,
) -> Result<BTreeSet<PoolFlowState>, PlanningFailure> {
    for statement in statements {
        checkpoint(cancellation)?;
        states = match statement {
            Statement::Return { value, .. } => match value {
                Some(value) => pool_flow_expression(
                    value,
                    states,
                    binding,
                    capacity,
                    program,
                    summary,
                    cancellation,
                )?,
                None => states,
            },
            Statement::Panic { value, .. }
            | Statement::Assert {
                condition: value, ..
            }
            | Statement::Expect {
                condition: value, ..
            }
            | Statement::Assign { value, .. }
            | Statement::Evaluate(value) => pool_flow_expression(
                value,
                states,
                binding,
                capacity,
                program,
                summary,
                cancellation,
            )?,
            Statement::Initialize { place, value, .. } => {
                let next = pool_flow_expression(
                    value,
                    states,
                    binding,
                    capacity,
                    program,
                    summary,
                    cancellation,
                )?;
                next.into_iter()
                    .map(|mut state| {
                        if state.transient_permits > 0 {
                            *state.permits.entry(place.local).or_default() +=
                                state.transient_permits;
                            state.transient_permits = 0;
                        }
                        state
                    })
                    .collect()
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
                ..
            }
            | Statement::IfPattern {
                value: condition,
                then_branch,
                else_branch,
                ..
            } => {
                let entered = pool_flow_expression(
                    condition,
                    states,
                    binding,
                    capacity,
                    program,
                    summary,
                    cancellation,
                )?;
                let mut joined = pool_flow_statements(
                    then_branch,
                    entered.clone(),
                    binding,
                    capacity,
                    program,
                    summary,
                    cancellation,
                )?;
                joined.extend(pool_flow_statements(
                    else_branch,
                    entered,
                    binding,
                    capacity,
                    program,
                    summary,
                    cancellation,
                )?);
                joined
            }
            Statement::Match { value, cases, .. } => {
                let entered = pool_flow_expression(
                    value,
                    states,
                    binding,
                    capacity,
                    program,
                    summary,
                    cancellation,
                )?;
                let mut joined = BTreeSet::new();
                for HirMatchCase { guard, body, .. } in cases.iter() {
                    let guarded = guard.as_ref().map_or(Ok(entered.clone()), |guard| {
                        pool_flow_expression(
                            guard,
                            entered.clone(),
                            binding,
                            capacity,
                            program,
                            summary,
                            cancellation,
                        )
                    })?;
                    joined.extend(pool_flow_statements(
                        body,
                        guarded,
                        binding,
                        capacity,
                        program,
                        summary,
                        cancellation,
                    )?);
                }
                joined
            }
            Statement::While {
                condition,
                body,
                max_iterations,
                ..
            } => {
                let mut joined = states.clone();
                let mut frontier = states;
                for _ in 0..*max_iterations {
                    let entered = pool_flow_expression(
                        condition,
                        frontier,
                        binding,
                        capacity,
                        program,
                        summary,
                        cancellation,
                    )?;
                    let next = pool_flow_statements(
                        body,
                        entered,
                        binding,
                        capacity,
                        program,
                        summary,
                        cancellation,
                    )?;
                    let before = joined.len();
                    joined.extend(next.iter().cloned());
                    if joined.len() == before {
                        break;
                    }
                    frontier = next;
                }
                joined
            }
            Statement::For { iterable, body, .. } => {
                let entered = pool_flow_expression(
                    iterable,
                    states,
                    binding,
                    capacity,
                    program,
                    summary,
                    cancellation,
                )?;
                let mut joined = entered.clone();
                joined.extend(pool_flow_statements(
                    body,
                    entered,
                    binding,
                    capacity,
                    program,
                    summary,
                    cancellation,
                )?);
                joined
            }
            Statement::WithPool { scope, body, .. } => {
                let entered = pool_flow_expression(
                    scope,
                    states,
                    binding,
                    capacity,
                    program,
                    summary,
                    cancellation,
                )?;
                pool_flow_statements(
                    body,
                    entered,
                    binding,
                    capacity,
                    program,
                    summary,
                    cancellation,
                )?
            }
            Statement::Defer { action, .. } => pool_flow_expression(
                action.expression(),
                states,
                binding,
                capacity,
                program,
                summary,
                cancellation,
            )?,
            Statement::Break(_) | Statement::Continue(_) | Statement::Pass(_) => states,
        };
    }
    let locals = statements
        .iter()
        .filter_map(|statement| match statement {
            Statement::Initialize { place, .. } => Some(place.local),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    Ok(states
        .into_iter()
        .map(|mut state| {
            let released = locals
                .iter()
                .filter_map(|local| state.permits.remove(local))
                .sum::<u64>();
            state.reserved = state.reserved.saturating_sub(released);
            state
        })
        .collect())
}

fn pool_flow_expression(
    expression: &Expression,
    mut states: BTreeSet<PoolFlowState>,
    binding: LocalId,
    capacity: u64,
    program: &VerifiedProgram,
    summary: &mut PoolFlowSummary,
    cancellation: &Cancellation,
) -> Result<BTreeSet<PoolFlowState>, PlanningFailure> {
    checkpoint(cancellation)?;
    let mut children = Vec::new();
    expression.visit_children(&mut |child| children.push(child.clone()));
    for child in children {
        states = pool_flow_expression(
            &child,
            states,
            binding,
            capacity,
            program,
            summary,
            cancellation,
        )?;
    }
    let ExpressionKind::Call { target, arguments } = &expression.kind else {
        if expression.access == AccessMode::Move
            && let Some(place) = root_place(expression)
        {
            states = states
                .into_iter()
                .map(|mut state| {
                    let available = state.permits.get(&place.local).copied().unwrap_or(0);
                    let moved = if place.projections.is_empty() {
                        available
                    } else {
                        available.min(1)
                    };
                    if moved > 0 {
                        if moved == available {
                            state.permits.remove(&place.local);
                        } else {
                            state.permits.insert(place.local, available - moved);
                        }
                        state.transient_permits = state.transient_permits.saturating_add(moved);
                    }
                    state
                })
                .collect();
        }
        return Ok(states);
    };
    let CallTarget::Function {
        specialization,
        argument_order,
        ..
    } = target
    else {
        return Ok(states);
    };
    let Some(function) = program.specialization_function(*specialization) else {
        return Err(PlanningFailure::Defect(Arc::from(
            "Pool flow call names a missing exact Specialization",
        )));
    };
    let Some(operation) = function.pool_operation else {
        let receiver_parameter =
            arguments
                .iter()
                .enumerate()
                .find_map(|(source_index, argument)| {
                    matches!(&argument.kind, ExpressionKind::Read(place) if place.local == binding)
                        .then(|| {
                            argument_order
                                .get(source_index)
                                .and_then(|parameter| {
                                    function.parameters.get(usize::from(*parameter))
                                })
                                .map(|(local, _, _)| *local)
                        })
                        .flatten()
                });
        let Some(receiver_parameter) = receiver_parameter else {
            return Ok(states);
        };
        let active_remaining = summary.active_calls.get(specialization).copied();
        let exact_remaining = arguments.iter().find_map(|argument| match argument.kind {
            ExpressionKind::Literal(Literal::Integer { value, .. }) => u64::try_from(value).ok(),
            _ => None,
        });
        let remaining = active_remaining
            .map(|remaining| remaining.saturating_sub(1))
            .or(exact_remaining)
            .unwrap_or_else(|| capacity.saturating_add(1));
        if remaining == 0 {
            return Ok(states);
        }
        let previous_remaining = summary.active_calls.insert(*specialization, remaining);
        let caller_executable =
            std::mem::replace(&mut summary.current_executable, specialization.0);
        let result = pool_flow_statements(
            &function.body,
            states,
            receiver_parameter,
            capacity,
            program,
            summary,
            cancellation,
        );
        summary.current_executable = caller_executable;
        if let Some(previous) = previous_remaining {
            summary.active_calls.insert(*specialization, previous);
        } else {
            summary.active_calls.remove(specialization);
        }
        return result;
    };
    let receiver_matches = arguments.first().is_some_and(
        |receiver| matches!(&receiver.kind, ExpressionKind::Read(place) if place.local == binding),
    );
    if !receiver_matches {
        return Ok(states);
    }
    if matches!(operation, PoolOperation::Allocate | PoolOperation::Reserve) {
        let site = (
            summary.current_executable,
            operation,
            expression.source.clone(),
            expression.type_id.0,
        );
        if !summary.admission_sites.contains(&site) {
            summary.admission_sites.push(site);
        }
    }
    let limit = capacity.saturating_add(1);
    let mut next = BTreeSet::new();
    for state in states {
        let state = match operation {
            PoolOperation::TryAllocate if state.live.saturating_add(state.reserved) < capacity => {
                PoolFlowState {
                    live: state.live.saturating_add(1).min(limit),
                    ..state
                }
            }
            PoolOperation::TryAllocate | PoolOperation::Lookup => state,
            PoolOperation::Allocate => PoolFlowState {
                live: state.live.saturating_add(1).min(limit),
                ..state
            },
            PoolOperation::Reserve => PoolFlowState {
                reserved: state.reserved.saturating_add(1).min(limit),
                transient_permits: state.transient_permits.saturating_add(1).min(limit),
                ..state
            },
            PoolOperation::Consume if state.reserved > 0 => PoolFlowState {
                live: state.live.saturating_add(1).min(limit),
                reserved: state.reserved - 1,
                transient_permits: state.transient_permits.saturating_sub(1),
                permits: state.permits,
            },
            PoolOperation::Consume => state,
            PoolOperation::Reclaim if state.live > 0 => PoolFlowState {
                live: state.live - 1,
                ..state
            },
            PoolOperation::Reclaim => state,
            PoolOperation::Release if state.reserved > 0 => PoolFlowState {
                reserved: state.reserved - 1,
                transient_permits: state.transient_permits.saturating_sub(1),
                ..state
            },
            PoolOperation::Release => state,
        };
        summary.peak_live = summary.peak_live.max(state.live);
        summary.peak_reserved = summary.peak_reserved.max(state.reserved);
        summary.peak_committed = summary
            .peak_committed
            .max(state.live.saturating_add(state.reserved));
        next.insert(state);
    }
    Ok(next)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ModelKey {
    generation: u64,
    type_identity: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ModelPermit {
    generation: u64,
    type_identity: u8,
    identity: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModelSlot {
    Free {
        generation: u64,
    },
    Live {
        generation: u64,
        type_identity: u8,
        value: u8,
    },
    Reserved(ModelPermit),
    Retired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BoundedPoolModel {
    slot: ModelSlot,
    next_permit: u8,
}

impl BoundedPoolModel {
    const fn new() -> Self {
        Self {
            slot: ModelSlot::Free { generation: 0 },
            next_permit: 1,
        }
    }

    fn try_allocate(&mut self, value: u8, type_identity: u8) -> Result<ModelKey, u8> {
        let ModelSlot::Free { generation } = self.slot else {
            return Err(value);
        };
        self.slot = ModelSlot::Live {
            generation,
            type_identity,
            value,
        };
        Ok(ModelKey {
            generation,
            type_identity,
        })
    }

    fn lookup(self, key: ModelKey) -> Option<u8> {
        match self.slot {
            ModelSlot::Live {
                generation,
                type_identity,
                value,
            } if generation == key.generation && type_identity == key.type_identity => Some(value),
            _ => None,
        }
    }

    fn reclaim(&mut self, key: ModelKey) -> Option<u8> {
        let ModelSlot::Live {
            generation,
            type_identity,
            value,
        } = self.slot
        else {
            return None;
        };
        if generation != key.generation || type_identity != key.type_identity {
            return None;
        }
        self.slot = generation
            .checked_add(1)
            .map_or(ModelSlot::Retired, |generation| ModelSlot::Free {
                generation,
            });
        Some(value)
    }

    fn reserve(&mut self, type_identity: u8) -> Option<ModelPermit> {
        let ModelSlot::Free { generation } = self.slot else {
            return None;
        };
        let permit = ModelPermit {
            generation,
            type_identity,
            identity: self.next_permit,
        };
        self.next_permit = self.next_permit.checked_add(1)?;
        self.slot = ModelSlot::Reserved(permit);
        Some(permit)
    }

    fn consume(&mut self, permit: ModelPermit, value: u8) -> Option<ModelKey> {
        if self.slot != ModelSlot::Reserved(permit) {
            return None;
        }
        self.slot = ModelSlot::Live {
            generation: permit.generation,
            type_identity: permit.type_identity,
            value,
        };
        Some(ModelKey {
            generation: permit.generation,
            type_identity: permit.type_identity,
        })
    }

    fn release(&mut self, permit: ModelPermit) -> bool {
        if self.slot != ModelSlot::Reserved(permit) {
            return false;
        }
        self.slot = ModelSlot::Free {
            generation: permit.generation,
        };
        true
    }

    const fn commitment(self) -> u8 {
        match self.slot {
            ModelSlot::Live { .. } | ModelSlot::Reserved(_) => 1,
            ModelSlot::Free { .. } | ModelSlot::Retired => 0,
        }
    }
}

fn run_pool_model() -> PoolModelObservation {
    // This deliberately small reference machine shares no transition code with evaluator or
    // source-flow planning. Verification recomputes every accepted-state witness from scratch.
    let mut allocation_model = BoundedPoolModel::new();
    let key = allocation_model.try_allocate(7, 11).ok();
    let accepted = key.is_some()
        && allocation_model.commitment() == 1
        && key.and_then(|key| allocation_model.lookup(key)) == Some(7);
    let full = allocation_model.try_allocate(9, 11) == Err(9) && allocation_model.commitment() == 1;
    let reclaimed = key.and_then(|key| allocation_model.reclaim(key));
    let stale =
        reclaimed == Some(7) && key.is_some_and(|key| allocation_model.lookup(key).is_none());

    let mut reservation_model = BoundedPoolModel::new();
    let permit = reservation_model.reserve(11);
    let reserved = permit.is_some() && reservation_model.commitment() == 1;
    let consumed = permit.and_then(|permit| reservation_model.consume(permit, 7));
    let reserved = reserved && consumed.is_some() && reservation_model.commitment() == 1;

    let mut release_model = BoundedPoolModel::new();
    let release_permit = release_model.reserve(11);
    let released = release_permit.is_some_and(|permit| release_model.release(permit))
        && release_model.commitment() == 0
        && release_permit.is_some_and(|permit| !release_model.release(permit));

    let mut retirement_model = BoundedPoolModel {
        slot: ModelSlot::Live {
            generation: u64::MAX,
            type_identity: 11,
            value: 7,
        },
        next_permit: 1,
    };
    let retired = retirement_model.reclaim(ModelKey {
        generation: u64::MAX,
        type_identity: 11,
    }) == Some(7)
        && retirement_model.slot == ModelSlot::Retired;
    let wrong_key_misses = allocation_model
        .lookup(ModelKey {
            generation: 0,
            type_identity: 99,
        })
        .is_none();
    let precommit_custody = full
        && allocation_model
            .lookup(ModelKey {
                generation: 0,
                type_identity: 11,
            })
            .is_none();
    let permit_reuse_rejected = permit.is_some_and(|permit| {
        reservation_model.consume(permit, 8).is_none() && !reservation_model.release(permit)
    });
    let retired_rejects_reuse = retirement_model.try_allocate(8, 11) == Err(8);
    let agrees = accepted
        && full
        && reserved
        && released
        && stale
        && retired
        && wrong_key_misses
        && precommit_custody
        && permit_reuse_rejected
        && retired_rejects_reuse;
    PoolModelObservation {
        cases: 10,
        agrees,
        accepted,
        full,
        released,
        reserved,
        stale,
        retired,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum OraclePoolSlot {
    Free(u64),
    Live {
        generation: u64,
        type_identity: u8,
        value: u8,
    },
    Reserved {
        generation: u64,
        type_identity: u8,
        permit: u8,
    },
    Retired,
}

#[derive(Clone, Copy)]
enum OraclePoolAction {
    Allocate { value: u8, type_identity: u8 },
    Reserve { type_identity: u8, permit: u8 },
    Lookup(ModelKey),
    Reclaim(ModelKey),
    Consume { permit: ModelPermit, value: u8 },
    Release(ModelPermit),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OraclePoolEffect {
    Key(ModelKey),
    Value(Option<u8>),
    Full(u8),
    Permit(Option<ModelPermit>),
    Released(bool),
}

fn oracle_pool_transition(
    slot: OraclePoolSlot,
    action: OraclePoolAction,
) -> (OraclePoolSlot, OraclePoolEffect) {
    match (slot, action) {
        (
            OraclePoolSlot::Free(generation),
            OraclePoolAction::Allocate {
                value,
                type_identity,
            },
        ) => (
            OraclePoolSlot::Live {
                generation,
                type_identity,
                value,
            },
            OraclePoolEffect::Key(ModelKey {
                generation,
                type_identity,
            }),
        ),
        (slot, OraclePoolAction::Allocate { value, .. }) => (slot, OraclePoolEffect::Full(value)),
        (
            OraclePoolSlot::Free(generation),
            OraclePoolAction::Reserve {
                type_identity,
                permit,
            },
        ) => {
            let permit = ModelPermit {
                generation,
                type_identity,
                identity: permit,
            };
            (
                OraclePoolSlot::Reserved {
                    generation,
                    type_identity,
                    permit: permit.identity,
                },
                OraclePoolEffect::Permit(Some(permit)),
            )
        }
        (slot, OraclePoolAction::Reserve { .. }) => (slot, OraclePoolEffect::Permit(None)),
        (
            slot @ OraclePoolSlot::Live {
                generation,
                type_identity,
                value,
            },
            OraclePoolAction::Lookup(key),
        ) if key.generation == generation && key.type_identity == type_identity => {
            (slot, OraclePoolEffect::Value(Some(value)))
        }
        (slot, OraclePoolAction::Lookup(_)) => (slot, OraclePoolEffect::Value(None)),
        (
            OraclePoolSlot::Live {
                generation,
                type_identity,
                value,
            },
            OraclePoolAction::Reclaim(key),
        ) if key.generation == generation && key.type_identity == type_identity => (
            generation
                .checked_add(1)
                .map_or(OraclePoolSlot::Retired, OraclePoolSlot::Free),
            OraclePoolEffect::Value(Some(value)),
        ),
        (slot, OraclePoolAction::Reclaim(_)) => (slot, OraclePoolEffect::Value(None)),
        (
            OraclePoolSlot::Reserved {
                generation,
                type_identity,
                permit,
            },
            OraclePoolAction::Consume {
                permit: supplied,
                value,
            },
        ) if supplied.generation == generation
            && supplied.type_identity == type_identity
            && supplied.identity == permit =>
        {
            (
                OraclePoolSlot::Live {
                    generation,
                    type_identity,
                    value,
                },
                OraclePoolEffect::Key(ModelKey {
                    generation,
                    type_identity,
                }),
            )
        }
        (slot, OraclePoolAction::Consume { .. }) => (slot, OraclePoolEffect::Value(None)),
        (
            OraclePoolSlot::Reserved {
                generation,
                type_identity,
                permit,
            },
            OraclePoolAction::Release(supplied),
        ) if supplied.generation == generation
            && supplied.type_identity == type_identity
            && supplied.identity == permit =>
        {
            (
                OraclePoolSlot::Free(generation),
                OraclePoolEffect::Released(true),
            )
        }
        (slot, OraclePoolAction::Release(_)) => (slot, OraclePoolEffect::Released(false)),
    }
}

fn independently_verify_pool_model() -> PoolModelObservation {
    let key = ModelKey {
        generation: 0,
        type_identity: 11,
    };
    let permit = ModelPermit {
        generation: 0,
        type_identity: 11,
        identity: 1,
    };
    let actions = [
        OraclePoolAction::Allocate {
            value: 7,
            type_identity: 11,
        },
        OraclePoolAction::Allocate {
            value: 9,
            type_identity: 11,
        },
        OraclePoolAction::Reserve {
            type_identity: 11,
            permit: 1,
        },
        OraclePoolAction::Lookup(key),
        OraclePoolAction::Lookup(ModelKey {
            generation: 0,
            type_identity: 99,
        }),
        OraclePoolAction::Reclaim(key),
        OraclePoolAction::Consume { permit, value: 7 },
        OraclePoolAction::Release(permit),
    ];
    let mut reachable = BTreeSet::from([OraclePoolSlot::Free(0)]);
    for _ in 0..4 {
        let frontier = reachable.iter().copied().collect::<Vec<_>>();
        for state in frontier {
            for action in actions {
                reachable.insert(oracle_pool_transition(state, action).0);
            }
        }
    }
    let accepted = reachable.iter().any(|state| {
        matches!(
            state,
            OraclePoolSlot::Live {
                generation: 0,
                type_identity: 11,
                value: 7
            }
        )
    });
    let live = OraclePoolSlot::Live {
        generation: 0,
        type_identity: 11,
        value: 7,
    };
    let (unchanged, full_effect) = oracle_pool_transition(
        live,
        OraclePoolAction::Allocate {
            value: 9,
            type_identity: 11,
        },
    );
    let full = unchanged == live && full_effect == OraclePoolEffect::Full(9);
    let (reserved_slot, reserved_effect) = oracle_pool_transition(
        OraclePoolSlot::Free(0),
        OraclePoolAction::Reserve {
            type_identity: 11,
            permit: 1,
        },
    );
    let (consumed_slot, consumed_effect) = oracle_pool_transition(
        reserved_slot,
        OraclePoolAction::Consume { permit, value: 7 },
    );
    let reserved = reserved_effect == OraclePoolEffect::Permit(Some(permit))
        && consumed_slot == live
        && consumed_effect == OraclePoolEffect::Key(key);
    let (released_slot, released_effect) =
        oracle_pool_transition(reserved_slot, OraclePoolAction::Release(permit));
    let (_, reused_release) =
        oracle_pool_transition(released_slot, OraclePoolAction::Release(permit));
    let released = released_slot == OraclePoolSlot::Free(0)
        && released_effect == OraclePoolEffect::Released(true)
        && reused_release == OraclePoolEffect::Released(false);
    let (reclaimed_slot, reclaimed) = oracle_pool_transition(live, OraclePoolAction::Reclaim(key));
    let (_, stale_lookup) = oracle_pool_transition(reclaimed_slot, OraclePoolAction::Lookup(key));
    let stale = reclaimed == OraclePoolEffect::Value(Some(7))
        && stale_lookup == OraclePoolEffect::Value(None);
    let retiring = OraclePoolSlot::Live {
        generation: u64::MAX,
        type_identity: 11,
        value: 7,
    };
    let (retired_slot, retired_value) = oracle_pool_transition(
        retiring,
        OraclePoolAction::Reclaim(ModelKey {
            generation: u64::MAX,
            type_identity: 11,
        }),
    );
    let (retired_after_allocate, retired_full) = oracle_pool_transition(
        retired_slot,
        OraclePoolAction::Allocate {
            value: 8,
            type_identity: 11,
        },
    );
    let retired = retired_slot == OraclePoolSlot::Retired
        && retired_value == OraclePoolEffect::Value(Some(7))
        && retired_after_allocate == OraclePoolSlot::Retired
        && retired_full == OraclePoolEffect::Full(8);
    let agrees = accepted && full && released && reserved && stale && retired;
    PoolModelObservation {
        cases: 10,
        agrees,
        accepted,
        full,
        released,
        reserved,
        stale,
        retired,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct AuditedPoolState {
    allocations: u64,
    reservations: u64,
    moving_reservations: u64,
    owners: BTreeMap<LocalId, u64>,
}

#[derive(Default)]
struct AuditedPoolFlow {
    maximum_allocations: u64,
    maximum_reservations: u64,
    maximum_commitment: u64,
    sites: Vec<(u128, PoolOperation, SourceRange, u128)>,
    activations: BTreeMap<SpecializationId, u64>,
    executable: u128,
}

struct AuditedPoolDeclaration {
    identity: u128,
    executable: CoreSourceExecutableRef,
    source: SourceRange,
    declared: u64,
    peak_live: u64,
    peak_reserved: u64,
    peak_committed: u64,
    sites: Vec<(u128, PoolOperation, SourceRange, u128)>,
}

fn audit_pool_expression(
    expression: &Expression,
    mut paths: BTreeSet<AuditedPoolState>,
    scope: LocalId,
    capacity: u64,
    program: &VerifiedProgram,
    flow: &mut AuditedPoolFlow,
    cancellation: &Cancellation,
) -> Result<BTreeSet<AuditedPoolState>, PlanningFailure> {
    checkpoint(cancellation)?;
    let mut operands = Vec::new();
    expression.visit_children(&mut |child| operands.push(child.clone()));
    for operand in operands {
        paths = audit_pool_expression(
            &operand,
            paths,
            scope,
            capacity,
            program,
            flow,
            cancellation,
        )?;
    }
    let ExpressionKind::Call { target, arguments } = &expression.kind else {
        if expression.access == AccessMode::Move
            && let Some(place) = root_place(expression)
        {
            paths = paths
                .into_iter()
                .map(|mut path| {
                    let owned = path.owners.get(&place.local).copied().unwrap_or(0);
                    let moved = if place.projections.is_empty() {
                        owned
                    } else {
                        owned.min(1)
                    };
                    if moved == owned {
                        path.owners.remove(&place.local);
                    } else if moved > 0 {
                        path.owners.insert(place.local, owned - moved);
                    }
                    path.moving_reservations = path.moving_reservations.saturating_add(moved);
                    path
                })
                .collect();
        }
        return Ok(paths);
    };
    let CallTarget::Function {
        specialization,
        argument_order,
        ..
    } = target
    else {
        return Ok(paths);
    };
    let function = program
        .specialization_function(*specialization)
        .ok_or_else(|| {
            PlanningFailure::Defect(Arc::from(
                "Pool audit call names a missing exact Specialization",
            ))
        })?;
    let Some(operation) = function.pool_operation else {
        let scope_parameter = arguments
            .iter()
            .enumerate()
            .find_map(|(source_index, argument)| {
                matches!(&argument.kind, ExpressionKind::Read(place) if place.local == scope)
                    .then(|| {
                        argument_order
                            .get(source_index)
                            .and_then(|parameter| function.parameters.get(usize::from(*parameter)))
                            .map(|(local, _, _)| *local)
                    })
                    .flatten()
            });
        let Some(scope_parameter) = scope_parameter else {
            return Ok(paths);
        };
        let active = flow.activations.get(specialization).copied();
        let authored = arguments.iter().find_map(|argument| match argument.kind {
            ExpressionKind::Literal(Literal::Integer { value, .. }) => u64::try_from(value).ok(),
            _ => None,
        });
        let remaining = active
            .map(|remaining| remaining.saturating_sub(1))
            .or(authored)
            .unwrap_or_else(|| capacity.saturating_add(1));
        if remaining == 0 {
            return Ok(paths);
        }
        let prior = flow.activations.insert(*specialization, remaining);
        let caller = std::mem::replace(&mut flow.executable, specialization.0);
        let result = audit_pool_statements(
            &function.body,
            paths,
            scope_parameter,
            capacity,
            program,
            flow,
            cancellation,
        );
        flow.executable = caller;
        if let Some(prior) = prior {
            flow.activations.insert(*specialization, prior);
        } else {
            flow.activations.remove(specialization);
        }
        return result;
    };
    if !arguments.first().is_some_and(
        |receiver| matches!(&receiver.kind, ExpressionKind::Read(place) if place.local == scope),
    ) {
        return Ok(paths);
    }
    if matches!(operation, PoolOperation::Allocate | PoolOperation::Reserve) {
        let site = (
            flow.executable,
            operation,
            expression.source.clone(),
            expression.type_id.0,
        );
        if !flow.sites.contains(&site) {
            flow.sites.push(site);
        }
    }
    let ceiling = capacity.saturating_add(1);
    paths = paths
        .into_iter()
        .flat_map(|path| {
            let can_try = path.allocations.saturating_add(path.reservations) < capacity;
            let mut alternatives = Vec::new();
            if operation == PoolOperation::TryAllocate {
                alternatives.push(path.clone());
                if can_try {
                    let mut accepted = path;
                    accepted.allocations = accepted.allocations.saturating_add(1).min(ceiling);
                    alternatives.push(accepted);
                }
            } else {
                let mut next = path;
                match operation {
                    PoolOperation::Allocate => {
                        next.allocations = next.allocations.saturating_add(1).min(ceiling);
                    }
                    PoolOperation::Reserve => {
                        next.reservations = next.reservations.saturating_add(1).min(ceiling);
                        next.moving_reservations =
                            next.moving_reservations.saturating_add(1).min(ceiling);
                    }
                    PoolOperation::Consume if next.reservations > 0 => {
                        next.reservations -= 1;
                        next.allocations = next.allocations.saturating_add(1).min(ceiling);
                        next.moving_reservations = next.moving_reservations.saturating_sub(1);
                    }
                    PoolOperation::Reclaim if next.allocations > 0 => next.allocations -= 1,
                    PoolOperation::Release if next.reservations > 0 => {
                        next.reservations -= 1;
                        next.moving_reservations = next.moving_reservations.saturating_sub(1);
                    }
                    PoolOperation::TryAllocate
                    | PoolOperation::Lookup
                    | PoolOperation::Consume
                    | PoolOperation::Reclaim
                    | PoolOperation::Release => {}
                }
                alternatives.push(next);
            }
            alternatives
        })
        .collect();
    for path in &paths {
        flow.maximum_allocations = flow.maximum_allocations.max(path.allocations);
        flow.maximum_reservations = flow.maximum_reservations.max(path.reservations);
        flow.maximum_commitment = flow
            .maximum_commitment
            .max(path.allocations.saturating_add(path.reservations));
    }
    Ok(paths)
}

#[allow(clippy::too_many_lines)]
fn audit_pool_statements(
    statements: &[Statement],
    mut paths: BTreeSet<AuditedPoolState>,
    scope: LocalId,
    capacity: u64,
    program: &VerifiedProgram,
    flow: &mut AuditedPoolFlow,
    cancellation: &Cancellation,
) -> Result<BTreeSet<AuditedPoolState>, PlanningFailure> {
    for statement in statements {
        checkpoint(cancellation)?;
        paths = match statement {
            Statement::Return {
                value: Some(value), ..
            }
            | Statement::Panic { value, .. }
            | Statement::Assert {
                condition: value, ..
            }
            | Statement::Expect {
                condition: value, ..
            }
            | Statement::Assign { value, .. }
            | Statement::Evaluate(value) => {
                audit_pool_expression(value, paths, scope, capacity, program, flow, cancellation)?
            }
            Statement::Return { value: None, .. }
            | Statement::Break(_)
            | Statement::Continue(_)
            | Statement::Pass(_) => paths,
            Statement::Initialize { place, value, .. } => {
                audit_pool_expression(value, paths, scope, capacity, program, flow, cancellation)?
                    .into_iter()
                    .map(|mut path| {
                        if path.moving_reservations > 0 {
                            *path.owners.entry(place.local).or_default() +=
                                path.moving_reservations;
                            path.moving_reservations = 0;
                        }
                        path
                    })
                    .collect()
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
                ..
            }
            | Statement::IfPattern {
                value: condition,
                then_branch,
                else_branch,
                ..
            } => {
                let entered = audit_pool_expression(
                    condition,
                    paths,
                    scope,
                    capacity,
                    program,
                    flow,
                    cancellation,
                )?;
                let mut exits = audit_pool_statements(
                    then_branch,
                    entered.clone(),
                    scope,
                    capacity,
                    program,
                    flow,
                    cancellation,
                )?;
                exits.extend(audit_pool_statements(
                    else_branch,
                    entered,
                    scope,
                    capacity,
                    program,
                    flow,
                    cancellation,
                )?);
                exits
            }
            Statement::Match { value, cases, .. } => {
                let entered = audit_pool_expression(
                    value,
                    paths,
                    scope,
                    capacity,
                    program,
                    flow,
                    cancellation,
                )?;
                let mut exits = BTreeSet::new();
                for case in cases.iter() {
                    let guarded = if let Some(guard) = &case.guard {
                        audit_pool_expression(
                            guard,
                            entered.clone(),
                            scope,
                            capacity,
                            program,
                            flow,
                            cancellation,
                        )?
                    } else {
                        entered.clone()
                    };
                    exits.extend(audit_pool_statements(
                        &case.body,
                        guarded,
                        scope,
                        capacity,
                        program,
                        flow,
                        cancellation,
                    )?);
                }
                exits
            }
            Statement::While {
                condition,
                body,
                max_iterations,
                ..
            } => {
                let mut exits = paths.clone();
                let mut frontier = paths;
                for _ in 0..*max_iterations {
                    let entered = audit_pool_expression(
                        condition,
                        frontier,
                        scope,
                        capacity,
                        program,
                        flow,
                        cancellation,
                    )?;
                    frontier = audit_pool_statements(
                        body,
                        entered,
                        scope,
                        capacity,
                        program,
                        flow,
                        cancellation,
                    )?;
                    exits.extend(frontier.iter().cloned());
                }
                exits
            }
            Statement::For { iterable, body, .. } => {
                let entered = audit_pool_expression(
                    iterable,
                    paths,
                    scope,
                    capacity,
                    program,
                    flow,
                    cancellation,
                )?;
                let mut exits = entered.clone();
                exits.extend(audit_pool_statements(
                    body,
                    entered,
                    scope,
                    capacity,
                    program,
                    flow,
                    cancellation,
                )?);
                exits
            }
            Statement::WithPool {
                scope: nested,
                body,
                ..
            } => {
                let entered = audit_pool_expression(
                    nested,
                    paths,
                    scope,
                    capacity,
                    program,
                    flow,
                    cancellation,
                )?;
                audit_pool_statements(body, entered, scope, capacity, program, flow, cancellation)?
            }
            Statement::Defer { action, .. } => audit_pool_expression(
                action.expression(),
                paths,
                scope,
                capacity,
                program,
                flow,
                cancellation,
            )?,
        };
    }
    let locals = statements
        .iter()
        .filter_map(|statement| match statement {
            Statement::Initialize { place, .. } => Some(place.local),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    Ok(paths
        .into_iter()
        .map(|mut path| {
            let released = locals
                .iter()
                .filter_map(|local| path.owners.remove(local))
                .sum::<u64>();
            path.reservations = path.reservations.saturating_sub(released);
            path
        })
        .collect())
}

fn audit_pool_foundation(
    candidate: &VerifiedPlanningFoundation,
    planner: PlannerRef,
    plan: DomainPlanRef,
    cancellation: &Cancellation,
) -> Result<Vec<Requirement>, PlanningFailure> {
    let semantic = candidate.semantic_program.for_core_planning();
    let mut declarations = Vec::new();
    let program = semantic.verified_program();
    for executable in semantic.exact_source_executables() {
        checkpoint(cancellation)?;
        let input = semantic.executable_input(executable).ok_or_else(|| {
            PlanningFailure::Defect(Arc::from("Pool audit names a missing executable"))
        })?;
        let statements = match input.body {
            CoreSourceExecutableBody::Specialization(function) => function.body.as_ref(),
            CoreSourceExecutableBody::Test(test) => test.body.as_ref(),
            CoreSourceExecutableBody::Closure(_) => continue,
        };
        audit_pool_declarations(
            executable,
            statements,
            program,
            cancellation,
            &mut declarations,
        )?;
    }
    declarations.sort_by_key(|declaration| declaration.identity);
    let mut pools = candidate.pools.iter().collect::<Vec<_>>();
    pools.sort_by_key(|pool| pool.reference.identity);
    if pools.len() != declarations.len() {
        return defect("Pool Plans do not match the independently audited scoped Pool roster");
    }
    let mut requirements = Vec::new();
    for (pool, declaration) in pools.into_iter().zip(declarations) {
        checkpoint(cancellation)?;
        let identity = declaration.identity;
        let executable = declaration.executable;
        let source = declaration.source;
        let declared = declaration.declared;
        if pool.reference.context != candidate.context
            || pool.reference.identity != identity
            || pool.executable != executable
            || pool.source != source
            || pool.declared_capacity != declared
            || pool.usable_slots != declared
            || pool.peak_live != declaration.peak_live
            || pool.peak_reserved != declaration.peak_reserved
            || pool.peak_committed != declaration.peak_committed
            || pool.peak_committed > pool.usable_slots
        {
            return defect(
                "Pool Plan contradicts independent identity, capacity, or commitment laws",
            );
        }
        let expected_meaning = producer_hash(
            b"wrela.pool-plan.meaning.v1",
            &[
                identity,
                executable.current_meaning(),
                u128::from(declared),
                u128::from(pool.peak_live),
                u128::from(pool.peak_reserved),
                u128::from(pool.peak_committed),
            ],
        );
        if pool.reference.current_meaning != expected_meaning {
            return defect("Pool Plan current meaning is stale or fabricated");
        }
        let observed_sites = pool
            .admission_sites
            .iter()
            .map(|site| {
                (
                    site.executable_identity,
                    site.operation,
                    site.source.clone(),
                    site.source_type_identity,
                )
            })
            .collect::<Vec<_>>();
        if observed_sites != declaration.sites {
            return defect(
                "Pool admission sites disagree with independently audited Specializations",
            );
        }
        for (ordinal, site) in pool.admission_sites.iter().enumerate() {
            let local_site = u16::try_from(ordinal + 1).map_err(|_| {
                PlanningFailure::Defect(Arc::from("Pool audit admission site overflow"))
            })?;
            let bounds = RequirementBounds::PoolCapacity {
                declared,
                usable: declared,
                peak_live: pool.peak_live,
                peak_reserved: pool.peak_reserved,
                peak_committed: pool.peak_committed,
            };
            let identity = produce_requirement_identity(
                planner.identity,
                pool.reference.identity,
                RequirementCategory::CapacityPressure,
                local_site,
            );
            let current_meaning = produce_requirement_current_meaning(
                identity,
                planner.current_meaning,
                pool.reference.current_meaning,
                plan.current_meaning,
                RequirementCategory::CapacityPressure,
                &bounds,
            );
            let reference = RequirementRef {
                context: candidate.context,
                identity,
                current_meaning,
            };
            if site.requirement != reference {
                return defect("Pool admission site carries false Requirement Evidence");
            }
            requirements.push(Requirement {
                context: candidate.context,
                reference,
                owner: planner,
                subject: RequirementOwner::Pool(pool.reference),
                provenance: RequirementProvenance {
                    domain_plan: plan.identity,
                    generated_role: 0,
                    local_site,
                },
                category: RequirementCategory::CapacityPressure,
                bounds,
                facility_contract: None,
            });
        }
    }
    requirements.sort_by_key(|requirement| requirement.reference.identity);
    Ok(requirements)
}

fn audit_pool_declarations(
    executable: CoreSourceExecutableRef,
    statements: &[Statement],
    program: &VerifiedProgram,
    cancellation: &Cancellation,
    output: &mut Vec<AuditedPoolDeclaration>,
) -> Result<(), PlanningFailure> {
    for statement in statements {
        checkpoint(cancellation)?;
        match statement {
            Statement::WithPool {
                pool,
                binding,
                scope,
                body,
                source,
                ..
            } => {
                let declared = pool_declared_capacity(scope).ok_or_else(|| {
                    PlanningFailure::Defect(Arc::from("Pool audit found an inexact capacity"))
                })?;
                let mut flow = AuditedPoolFlow {
                    executable: executable.identity(),
                    ..AuditedPoolFlow::default()
                };
                let initial = BTreeSet::from([AuditedPoolState {
                    allocations: 0,
                    reservations: 0,
                    moving_reservations: 0,
                    owners: BTreeMap::new(),
                }]);
                let _ = audit_pool_statements(
                    body,
                    initial,
                    binding.local,
                    declared,
                    program,
                    &mut flow,
                    cancellation,
                )?;
                output.push(AuditedPoolDeclaration {
                    identity: pool.0,
                    executable,
                    source: source.clone(),
                    declared,
                    peak_live: flow.maximum_allocations,
                    peak_reserved: flow.maximum_reservations,
                    peak_committed: flow.maximum_commitment,
                    sites: flow.sites,
                });
                audit_pool_declarations(executable, body, program, cancellation, output)?;
            }
            Statement::If {
                then_branch,
                else_branch,
                ..
            }
            | Statement::IfPattern {
                then_branch,
                else_branch,
                ..
            } => {
                audit_pool_declarations(executable, then_branch, program, cancellation, output)?;
                audit_pool_declarations(executable, else_branch, program, cancellation, output)?;
            }
            Statement::For { body, .. } | Statement::While { body, .. } => {
                audit_pool_declarations(executable, body, program, cancellation, output)?;
            }
            Statement::Match { cases, .. } => {
                for case in cases.iter() {
                    audit_pool_declarations(executable, &case.body, program, cancellation, output)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn verify(
    candidate: &VerifiedPlanningFoundation,
    cancellation: &Cancellation,
) -> Result<(), PlanningFailure> {
    checkpoint(cancellation)?;
    let semantic = candidate.semantic_program.for_image_planning();
    let architecture = candidate.architecture_contract.for_image_planning();
    if candidate.context != semantic.context_identity()
        || semantic.distribution_digest() != architecture.distribution_digest()
    {
        return defect("planning foundation has mixed compilation contexts");
    }
    let expected_contracts = verify_facility_contracts(semantic.distribution_digest());
    if candidate.facility_contracts != expected_contracts {
        return defect(
            "Facility Contract catalog is unauthenticated, malformed, stale, or incompatible",
        );
    }
    let expected_planner = verify_planner(
        candidate.context,
        semantic.root(),
        semantic.fingerprint(),
        architecture.fingerprint(),
    );
    let mut expected_plan = verify_domain_plan(
        candidate.context,
        expected_planner.reference,
        semantic.fingerprint(),
        architecture.fingerprint(),
    );
    let mut expected_roles = verify_roles(
        candidate.context,
        semantic.root(),
        expected_planner.reference,
        expected_plan.reference,
        semantic.fingerprint(),
        architecture.fingerprint(),
        cancellation,
    )?;
    let mut expected_requirements = verify_requirements(
        candidate.context,
        &expected_roles,
        expected_planner.reference,
        expected_plan.reference,
        semantic.test_application_count(),
        architecture.core_count(),
        architecture.service().maximum_cycle_units,
        cancellation,
    )?;
    let instances = discover_facility_instances(semantic, cancellation)?;
    for contract in expected_contracts.iter() {
        checkpoint(cancellation)?;
        let selected = checked_u32(
            instances
                .iter()
                .filter(|instance| instance.kind == contract.observation.kind)
                .count(),
        )?;
        if selected < contract.observation.minimum_instances
            || selected > contract.observation.maximum_instances
        {
            return defect("Facility cardinality violates its authenticated contract");
        }
    }
    let mut expected_planners = vec![expected_planner.clone()];
    let mut expected_plans = vec![expected_plan.clone()];
    for instance in &instances {
        let contract = expected_contracts
            .iter()
            .find(|contract| contract.observation.kind == instance.kind)
            .ok_or_else(|| {
                PlanningFailure::Defect(Arc::from("verifier selected Facility has no contract"))
            })?;
        let required_replay = match contract.observation.replay_rule {
            FacilityReplayRule::ReplayableGameplay => FacilityReplayAuthority::ReplayableGameplay,
            FacilityReplayRule::ExcludedFromReplayableGameplay => {
                FacilityReplayAuthority::NonReplayableFacility
            }
        };
        if instance.selected_loss_policy != contract.observation.loss_policy
            || instance.replay_authority != required_replay
        {
            return defect("Facility instance configuration disagrees with its contract");
        }
        let planner = verify_facility_planner(
            candidate.context,
            instance.clone(),
            contract,
            architecture.fingerprint(),
        );
        let mut plan = verify_facility_domain_plan(
            candidate.context,
            instance.clone(),
            planner.reference,
            contract,
        );
        let roles = verify_facility_roles(
            candidate.context,
            planner.reference,
            plan.reference,
            contract,
            architecture.fingerprint(),
            cancellation,
        )?;
        let requirements = verify_facility_requirements(
            candidate.context,
            planner.reference,
            plan.reference,
            instance,
            &roles,
            contract,
            cancellation,
        )?;
        plan.generated_roles = roles
            .iter()
            .map(|role| role.reference)
            .collect::<Vec<_>>()
            .into();
        plan.requirements = requirements
            .iter()
            .map(|requirement| requirement.reference)
            .collect::<Vec<_>>()
            .into();
        expected_planners.push(planner);
        expected_plans.push(plan);
        expected_roles.extend(roles);
        expected_requirements.extend(requirements);
    }
    let mut expected_pool_requirements = audit_pool_foundation(
        candidate,
        expected_planner.reference,
        expected_plan.reference,
        cancellation,
    )?;
    expected_requirements.append(&mut expected_pool_requirements);
    expected_requirements.sort_by_key(|requirement| requirement.reference.identity);
    expected_roles.sort_by_key(|role| role.reference.identity);
    expected_plan.generated_roles = expected_roles
        .iter()
        .filter(|role| role.owner == expected_planner.reference)
        .map(|role| role.reference)
        .collect::<Vec<_>>()
        .into();
    expected_plan.requirements = expected_requirements
        .iter()
        .filter(|requirement| requirement.owner == expected_planner.reference)
        .map(|requirement| requirement.reference)
        .collect::<Vec<_>>()
        .into();
    expected_plans[0] = expected_plan;
    expected_planners.sort_by_key(|planner| planner.reference.identity);
    expected_plans.sort_by_key(|plan| plan.reference.identity);
    if candidate.planner_roster.as_ref() != expected_planners.as_slice() {
        return defect("planner roster is missing, extra, duplicated, wrong-kind, or stale");
    }
    if candidate.generated_roles.as_ref() != expected_roles.as_slice() {
        return defect(
            "Generated Roles are missing, extra, dangling, wrong-owner, wrong-role, wrong-generator, or stale",
        );
    }
    if candidate.domain_plans.as_ref() != expected_plans.as_slice() {
        return defect(
            "Domain Plans are missing, extra, wrong-owner, wrong-contract, wrong-instance, or stale",
        );
    }
    verify_role_graph(candidate, cancellation)?;
    verify_architecture_evidence(candidate, cancellation)?;
    let expected_pool_model = independently_verify_pool_model();
    if candidate.pool_model != expected_pool_model || !candidate.pool_model.agrees {
        return defect("bounded Pool authority model disagrees");
    }
    if candidate.requirements.as_ref() != expected_requirements.as_slice() {
        return defect(
            "Requirement Set is missing, extra, duplicate, dangling, wrong-owner, wrong-role, wrong-provenance, or stale",
        );
    }
    verify_requirement_bounds(candidate, cancellation)?;
    let mut references = BTreeSet::new();
    for requirement in candidate.requirements.iter() {
        checkpoint(cancellation)?;
        if !references.insert(requirement.reference.identity) {
            return defect("Requirement Set contains a duplicate Requirement Reference");
        }
    }
    if candidate.requirements.len() > architecture.capacity().maximum_requirements as usize {
        return defect("Requirement Set exceeds authenticated architecture capacity");
    }
    if candidate.generated_roles.len() > architecture.capacity().maximum_generated_roles as usize {
        return defect("Generated Role closure exceeds authenticated architecture capacity");
    }
    verify_executable_demand(candidate, &expected_roles)?;
    let expected_fingerprint = verify_foundation_fingerprint(
        candidate.context,
        semantic.fingerprint(),
        semantic.construction_graph_fingerprint(),
        semantic.custody_fingerprint(),
        architecture.identity(),
        architecture.fingerprint(),
        architecture.distribution_input_receipt(),
        &candidate.planner_roster,
        &candidate.domain_plans,
        &candidate.generated_roles,
        &candidate.requirements,
        &candidate.pools,
        &candidate.pool_model,
        &candidate.executable_demand,
    );
    if candidate.fingerprint != expected_fingerprint {
        return defect("planning foundation fingerprint is false");
    }
    Ok(())
}

fn verify_planner(
    context: u128,
    root: Root,
    semantic_fingerprint: u128,
    architecture_fingerprint: u128,
) -> Planner {
    let identity = verifier_hash(b"wrela.planner.image-kind.v1", &[root_tag(root).into()]);
    let current_meaning = verifier_hash(
        b"wrela.planner.image-kind.meaning.v1",
        &[
            identity,
            root_tag(root).into(),
            semantic_fingerprint,
            architecture_fingerprint,
        ],
    );
    Planner {
        reference: PlannerRef {
            context,
            identity,
            current_meaning,
        },
        kind: PlannerKind::ImageKind,
    }
}

fn verify_facility_contracts(distribution_digest: u128) -> Arc<[FacilityContract]> {
    FACILITY_KINDS
        .into_iter()
        .map(|kind| verifier_facility_contract(kind, distribution_digest, 0, 1))
        .collect::<Vec<_>>()
        .into()
}

fn verifier_facility_contract(
    kind: FacilityKind,
    distribution_digest: u128,
    minimum_instances: u32,
    maximum_instances: u32,
) -> FacilityContract {
    let (roles, capacities, capabilities, binding, sharing, loss_policy, shutdown) =
        verifier_facility_facts(kind);
    let flagship_rule = if kind == FacilityKind::Entropy {
        FacilityFlagshipRule::SelectingImageOptional
    } else {
        FacilityFlagshipRule::Required { loss_policy }
    };
    let replay_rule = if kind == FacilityKind::Entropy {
        FacilityReplayRule::ExcludedFromReplayableGameplay
    } else {
        FacilityReplayRule::ReplayableGameplay
    };
    let identity = verifier_hash(
        b"wrela.facility-contract.identity.v1",
        &[u128::from(kind.tag())],
    );
    let context_receipt = verifier_hash(
        b"wrela.facility-contract.context.v1",
        &[distribution_digest],
    );
    let endpoint_ownership = if kind == FacilityKind::Input {
        FacilityEndpointOwnership::BuildWiredActor
    } else {
        FacilityEndpointOwnership::FacilityInstance
    };
    let facts = verifier_facility_facts_fingerprint(
        &roles,
        &capacities,
        &capabilities,
        binding,
        sharing,
        loss_policy,
        shutdown,
    );
    let fingerprint = verifier_hash(
        b"wrela.facility-contract.v1",
        &[
            u128::from(kind.tag()),
            identity,
            context_receipt,
            distribution_digest,
            1,
            1,
            u128::from(minimum_instances),
            u128::from(maximum_instances),
            1,
            match endpoint_ownership {
                FacilityEndpointOwnership::FacilityInstance => 1,
                FacilityEndpointOwnership::BuildWiredActor => 2,
            },
            facility_flagship_tag(flagship_rule),
            facility_replay_rule_tag(replay_rule),
            if kind == FacilityKind::Telemetry {
                1
            } else {
                3
            },
            facts,
        ],
    );
    FacilityContract {
        reference: FacilityContractRef {
            context: context_receipt,
            identity,
            current_meaning: fingerprint,
            fingerprint,
            kind,
        },
        observation: FacilityContractObservation {
            kind,
            identity,
            context_receipt,
            fingerprint,
            current_meaning: fingerprint,
            allows_deployment: true,
            allows_test: true,
            minimum_instances,
            maximum_instances,
            maximum_exported_endpoints: 1,
            endpoint_ownership,
            flagship_rule,
            replay_rule,
            generated_roles: roles,
            semantic_capacities: capacities,
            required_capabilities: capabilities,
            external_binding: Some(binding),
            sharing,
            loss_policy,
            shutdown,
            maximum_recovery_attempts: if kind == FacilityKind::Telemetry {
                1
            } else {
                3
            },
            binding_availability: FacilityBindingAvailability::BootFailure,
        },
        authentication: verifier_hash(
            b"wrela.facility-contract.authentication.v1",
            &[distribution_digest, identity, context_receipt, fingerprint],
        ),
    }
}

fn verifier_facility_facts(kind: FacilityKind) -> FacilityFacts {
    let virtio = Arc::from([
        PlanningCapability::PciVirtioModern,
        PlanningCapability::SplitVirtqueue,
        PlanningCapability::SharedIntx,
        PlanningCapability::DmaOwnership,
    ]);
    match kind {
        FacilityKind::Display => (
            Arc::from([GeneratedRoleKind::DisplayDriver]),
            Arc::from([FacilitySemanticCapacity::FrameBuffers(3)]),
            virtio,
            PlanningBinding::Display,
            FacilitySharing::Exclusive,
            FacilityLossPolicy::ControlledShutdown,
            FacilityShutdown::Quiesce,
        ),
        FacilityKind::Input => (
            Arc::from([GeneratedRoleKind::InputDriver]),
            Arc::from([FacilitySemanticCapacity::InputTransitions(256)]),
            virtio,
            PlanningBinding::Input,
            FacilitySharing::Exclusive,
            FacilityLossPolicy::ControlledShutdown,
            FacilityShutdown::StopSampling,
        ),
        FacilityKind::EventStore => (
            Arc::from([
                GeneratedRoleKind::EventStoreRuntime,
                GeneratedRoleKind::EventStoreDriver,
            ]),
            Arc::from([FacilitySemanticCapacity::EventSlots(65_536)]),
            virtio,
            PlanningBinding::EventStore,
            FacilitySharing::Exclusive,
            FacilityLossPolicy::ControlledShutdown,
            FacilityShutdown::FlushCommittedAndQuiesce,
        ),
        FacilityKind::MonotonicClock => (
            Arc::from([GeneratedRoleKind::MonotonicClockDriver]),
            Arc::from([FacilitySemanticCapacity::ClockWaiters(1024)]),
            Arc::from([PlanningCapability::MonotonicCounter]),
            PlanningBinding::MonotonicClock,
            FacilitySharing::RegisteredDisjoint {
                role: FacilitySharedRole::MonotonicCounter,
                maximum_units: 1024,
            },
            FacilityLossPolicy::ControlledShutdown,
            FacilityShutdown::StopWakeups,
        ),
        FacilityKind::Entropy => (
            Arc::from([GeneratedRoleKind::EntropyDriver]),
            Arc::from([FacilitySemanticCapacity::EntropyRequestBytes(4096)]),
            virtio,
            PlanningBinding::Entropy,
            FacilitySharing::RegisteredDisjoint {
                role: FacilitySharedRole::EntropyQueue,
                maximum_units: 16,
            },
            FacilityLossPolicy::SelectingImagePolicy,
            FacilityShutdown::DiscardPending,
        ),
        FacilityKind::Telemetry => (
            Arc::from([GeneratedRoleKind::TelemetryDriver]),
            Arc::from([FacilitySemanticCapacity::TelemetryRingRecords(4096)]),
            virtio,
            PlanningBinding::Telemetry,
            FacilitySharing::Exclusive,
            FacilityLossPolicy::DisableAndContinue,
            FacilityShutdown::DropObservations,
        ),
    }
}

fn verifier_facility_facts_fingerprint(
    roles: &[GeneratedRoleKind],
    capacities: &[FacilitySemanticCapacity],
    capabilities: &[PlanningCapability],
    binding: PlanningBinding,
    sharing: FacilitySharing,
    loss: FacilityLossPolicy,
    shutdown: FacilityShutdown,
) -> u128 {
    let mut values = vec![
        u128::from(binding.tag()),
        facility_sharing_tag(sharing),
        facility_loss_tag(loss),
        facility_shutdown_tag(shutdown),
    ];
    values.extend(roles.iter().map(|role| u128::from(role.tag())));
    values.extend(
        capacities
            .iter()
            .map(|capacity| facility_capacity_tag(*capacity)),
    );
    values.extend(
        capabilities
            .iter()
            .map(|capability| u128::from(capability.tag())),
    );
    verifier_hash(b"wrela.facility-contract.facts.v1", &values)
}

fn verify_facility_planner(
    context: u128,
    instance: FacilityInstanceRef,
    contract: &FacilityContract,
    architecture_fingerprint: u128,
) -> Planner {
    let identity = verifier_hash(
        b"wrela.planner.facility.v1",
        &[u128::from(instance.kind.tag()), instance.identity],
    );
    Planner {
        reference: PlannerRef {
            context,
            identity,
            current_meaning: verifier_hash(
                b"wrela.planner.facility.meaning.v1",
                &[
                    identity,
                    instance.current_meaning,
                    contract.observation.fingerprint,
                    architecture_fingerprint,
                ],
            ),
        },
        kind: PlannerKind::Facility(instance.kind),
    }
}

fn verify_facility_domain_plan(
    context: u128,
    instance: FacilityInstanceRef,
    planner: PlannerRef,
    contract: &FacilityContract,
) -> DomainPlan {
    let identity = verifier_hash(
        b"wrela.domain-plan.facility.v1",
        &[
            planner.identity,
            instance.identity,
            u128::from(instance.kind.tag()),
        ],
    );
    DomainPlan {
        reference: DomainPlanRef {
            context,
            identity,
            current_meaning: verifier_hash(
                b"wrela.domain-plan.facility.meaning.v1",
                &[
                    identity,
                    planner.current_meaning,
                    instance.current_meaning,
                    contract.observation.fingerprint,
                ],
            ),
        },
        planner,
        kind: DomainPlanKind::Facility(instance.kind),
        generated_roles: Arc::from([]),
        requirements: Arc::from([]),
        facility_instance: Some(instance),
        facility_contract: Some(contract.reference),
    }
}

fn verify_facility_roles(
    context: u128,
    planner: PlannerRef,
    plan: DomainPlanRef,
    contract: &FacilityContract,
    architecture_fingerprint: u128,
    cancellation: &Cancellation,
) -> Result<Vec<GeneratedRole>, PlanningFailure> {
    let mut roles = Vec::<GeneratedRole>::new();
    for (ordinal, kind) in contract
        .observation
        .generated_roles
        .iter()
        .copied()
        .enumerate()
    {
        checkpoint(cancellation)?;
        let mut dependencies = Vec::new();
        if kind == GeneratedRoleKind::EventStoreDriver {
            dependencies.push(
                roles
                    .iter()
                    .find(|role| role.kind == GeneratedRoleKind::EventStoreRuntime)
                    .map(|role| role.reference)
                    .ok_or_else(|| {
                        PlanningFailure::Defect(Arc::from(
                            "verifier Event Store role closure is malformed",
                        ))
                    })?,
            );
        }
        dependencies.sort_by_key(|reference| reference.identity);
        let local_key = u16::try_from(ordinal + 1).map_err(|_| {
            PlanningFailure::Defect(Arc::from("verifier Facility role key overflow"))
        })?;
        let identity = verify_role_identity(planner, kind, local_key);
        let current_meaning = verify_role_current_meaning(
            identity,
            plan,
            &dependencies,
            contract.observation.current_meaning,
            architecture_fingerprint,
        );
        roles.push(GeneratedRole {
            reference: RoleRef {
                context,
                identity,
                current_meaning,
            },
            executable: ExecutableRef {
                context,
                identity: verifier_hash(b"wrela.generated-executable.v1", &[identity]),
                current_meaning,
            },
            owner: planner,
            generator: planner,
            kind,
            local_key,
            dependencies: dependencies.into(),
            provenance: plan,
        });
    }
    roles.sort_by_key(|role| role.reference.identity);
    Ok(roles)
}

fn verify_facility_requirements(
    context: u128,
    planner: PlannerRef,
    plan: DomainPlanRef,
    instance: &FacilityInstanceRef,
    roles: &[GeneratedRole],
    contract: &FacilityContract,
    cancellation: &Cancellation,
) -> Result<Vec<Requirement>, PlanningFailure> {
    let mut verified = Vec::new();
    for (role_ordinal, kind) in contract.observation.generated_roles.iter().enumerate() {
        checkpoint(cancellation)?;
        let role = roles
            .iter()
            .find(|role| role.kind == *kind)
            .ok_or_else(|| PlanningFailure::Defect(Arc::from("verifier Facility role missing")))?;
        for (offset, (category, bounds)) in [
            (
                RequirementCategory::GeneratedRoleRealization,
                RequirementBounds::RealizeExactlyOnce {
                    executable: role.executable.identity,
                },
            ),
            (
                RequirementCategory::Lifetime,
                RequirementBounds::ImageLifetime,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let local_site = u16::try_from(100 + role_ordinal * 2 + offset).map_err(|_| {
                PlanningFailure::Defect(Arc::from("verifier Facility requirement site overflow"))
            })?;
            verified.push(verify_requirement(
                context,
                planner,
                plan,
                role.reference,
                local_site,
                category,
                bounds,
            ));
        }
    }
    let subject = FacilitySubjectRef {
        context: instance.context,
        identity: instance.identity,
        current_meaning: instance.current_meaning,
        kind: instance.kind,
    };
    let binding = contract
        .observation
        .external_binding
        .ok_or_else(|| PlanningFailure::Defect(Arc::from("verifier Facility binding missing")))?;
    let mut specs = vec![
        (
            1,
            RequirementCategory::Cardinality,
            RequirementBounds::Cardinality {
                minimum: contract.observation.minimum_instances,
                maximum: contract.observation.maximum_instances,
            },
        ),
        (
            2,
            RequirementCategory::Binding,
            RequirementBounds::Binding {
                kind: binding,
                minimum: 1,
                maximum: contract.observation.maximum_exported_endpoints,
            },
        ),
        (
            3,
            RequirementCategory::Binding,
            RequirementBounds::FacilitySharing(contract.observation.sharing),
        ),
    ];
    for (ordinal, capability) in contract
        .observation
        .required_capabilities
        .iter()
        .copied()
        .enumerate()
    {
        checkpoint(cancellation)?;
        specs.push((
            10 + u16::try_from(ordinal).map_err(|_| {
                PlanningFailure::Defect(Arc::from("verifier capability ordinal overflow"))
            })?,
            RequirementCategory::ArchitectureCapability,
            RequirementBounds::Capability(capability),
        ));
    }
    for (ordinal, capacity) in contract
        .observation
        .semantic_capacities
        .iter()
        .copied()
        .enumerate()
    {
        checkpoint(cancellation)?;
        specs.push((
            30 + u16::try_from(ordinal).map_err(|_| {
                PlanningFailure::Defect(Arc::from("verifier capacity ordinal overflow"))
            })?,
            RequirementCategory::CapacityPressure,
            RequirementBounds::FacilityCapacity(capacity),
        ));
    }
    specs.extend([
        (
            50,
            RequirementCategory::FacilityOwnership,
            RequirementBounds::FacilityEndpoint {
                maximum: contract.observation.maximum_exported_endpoints,
                ownership: contract.observation.endpoint_ownership,
                input_owner: instance.input_owner,
            },
        ),
        (
            51,
            RequirementCategory::Recovery,
            RequirementBounds::FacilityRecovery {
                supervisor: instance.supervisor,
                loss_policy: instance.selected_loss_policy,
                maximum_attempts: contract.observation.maximum_recovery_attempts,
            },
        ),
        (
            52,
            RequirementCategory::Shutdown,
            RequirementBounds::FacilityShutdown(contract.observation.shutdown),
        ),
        (
            53,
            RequirementCategory::Replay,
            RequirementBounds::FacilityReplay {
                selected: instance.replay_authority,
                rule: contract.observation.replay_rule,
            },
        ),
        (
            54,
            RequirementCategory::Flagship,
            RequirementBounds::FacilityFlagship(contract.observation.flagship_rule),
        ),
        (
            55,
            RequirementCategory::BootAvailability,
            RequirementBounds::FacilityBindingAvailability(
                contract.observation.binding_availability,
            ),
        ),
    ]);
    for (local_site, category, bounds) in specs {
        checkpoint(cancellation)?;
        verified.push(verify_facility_requirement(
            context,
            planner,
            plan,
            subject,
            contract.reference,
            local_site,
            category,
            bounds,
        ));
    }
    verified.sort_by_key(|requirement| requirement.reference.identity);
    Ok(verified)
}

#[allow(clippy::too_many_arguments)]
fn verify_facility_requirement(
    context: u128,
    owner: PlannerRef,
    plan: DomainPlanRef,
    subject: FacilitySubjectRef,
    contract: FacilityContractRef,
    local_site: u16,
    category: RequirementCategory,
    bounds: RequirementBounds,
) -> Requirement {
    let identity =
        verify_requirement_identity(owner.identity, subject.identity, category, local_site);
    let current_meaning = verify_requirement_current_meaning(
        identity,
        owner.current_meaning,
        subject.current_meaning,
        plan.current_meaning,
        category,
        &bounds,
    );
    Requirement {
        context,
        reference: RequirementRef {
            context,
            identity,
            current_meaning,
        },
        owner,
        subject: RequirementOwner::Facility(subject),
        provenance: RequirementProvenance {
            domain_plan: plan.identity,
            generated_role: subject.identity,
            local_site,
        },
        category,
        bounds,
        facility_contract: Some(contract),
    }
}

fn verify_domain_plan(
    context: u128,
    planner: PlannerRef,
    semantic_fingerprint: u128,
    architecture_fingerprint: u128,
) -> DomainPlan {
    let identity = verifier_hash(
        b"wrela.domain-plan.mandatory-image.v1",
        &[planner.identity, 1],
    );
    let current_meaning = verifier_hash(
        b"wrela.domain-plan.mandatory-image.meaning.v1",
        &[
            identity,
            planner.current_meaning,
            semantic_fingerprint,
            architecture_fingerprint,
        ],
    );
    DomainPlan {
        reference: DomainPlanRef {
            context,
            identity,
            current_meaning,
        },
        planner,
        kind: DomainPlanKind::MandatoryImage,
        generated_roles: Arc::from([]),
        requirements: Arc::from([]),
        facility_instance: None,
        facility_contract: None,
    }
}

fn verifier_role_specs(root: Root) -> Vec<(GeneratedRoleKind, Vec<GeneratedRoleKind>)> {
    let mut expected = vec![
        (GeneratedRoleKind::Boot, vec![]),
        (GeneratedRoleKind::Scheduler, vec![GeneratedRoleKind::Boot]),
        (GeneratedRoleKind::Terminal, vec![GeneratedRoleKind::Boot]),
        (GeneratedRoleKind::Panic, vec![GeneratedRoleKind::Terminal]),
        (
            GeneratedRoleKind::Shutdown,
            vec![GeneratedRoleKind::Scheduler, GeneratedRoleKind::Terminal],
        ),
    ];
    if root == Root::Test {
        expected.push((
            GeneratedRoleKind::TestRuntime,
            vec![
                GeneratedRoleKind::Scheduler,
                GeneratedRoleKind::Terminal,
                GeneratedRoleKind::Shutdown,
            ],
        ));
    }
    expected
}

fn verify_roles(
    context: u128,
    root: Root,
    planner: PlannerRef,
    plan: DomainPlanRef,
    semantic_fingerprint: u128,
    architecture_fingerprint: u128,
    cancellation: &Cancellation,
) -> Result<Vec<GeneratedRole>, PlanningFailure> {
    let mut expected = Vec::<GeneratedRole>::new();
    for (ordinal, (kind, dependency_kinds)) in verifier_role_specs(root).into_iter().enumerate() {
        checkpoint(cancellation)?;
        let mut dependencies = dependency_kinds
            .iter()
            .map(|dependency_kind| {
                expected
                    .iter()
                    .find(|role| role.kind == *dependency_kind)
                    .map(|role| role.reference)
                    .ok_or_else(|| {
                        PlanningFailure::Defect(Arc::from(
                            "verifier role graph is not topologically closed",
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        dependencies.sort_by_key(|reference| reference.identity);
        let local_key = u16::try_from(ordinal + 1)
            .map_err(|_| PlanningFailure::Defect(Arc::from("verifier role key overflow")))?;
        let identity = verify_role_identity(planner, kind, local_key);
        let current_meaning = verify_role_current_meaning(
            identity,
            plan,
            &dependencies,
            semantic_fingerprint,
            architecture_fingerprint,
        );
        expected.push(GeneratedRole {
            reference: RoleRef {
                context,
                identity,
                current_meaning,
            },
            executable: ExecutableRef {
                context,
                identity: verifier_hash(b"wrela.generated-executable.v1", &[identity]),
                current_meaning,
            },
            owner: planner,
            generator: planner,
            kind,
            local_key,
            dependencies: dependencies.into(),
            provenance: plan,
        });
    }
    expected.sort_by_key(|role| role.reference.identity);
    Ok(expected)
}

fn verifier_requirement_specs(
    role: &GeneratedRole,
    test_application_count: usize,
    core_count: usize,
    maximum_cycle_units: u32,
) -> Result<Vec<(RequirementCategory, RequirementBounds)>, PlanningFailure> {
    let mut result = vec![
        (
            RequirementCategory::GeneratedRoleRealization,
            RequirementBounds::RealizeExactlyOnce {
                executable: role.executable.identity,
            },
        ),
        (
            RequirementCategory::Lifetime,
            RequirementBounds::ImageLifetime,
        ),
    ];
    match role.kind {
        GeneratedRoleKind::Boot => {
            result.push((
                RequirementCategory::ArchitectureCapability,
                RequirementBounds::Capability(PlanningCapability::SecondaryCoreStartup),
            ));
            result.push((
                RequirementCategory::LogicalLayout,
                RequirementBounds::Reservation {
                    kind: PlanningReservation::BootState,
                    multiplicity: PlanningMultiplicity::Once,
                },
            ));
        }
        GeneratedRoleKind::Scheduler => {
            let cores = checked_u32(core_count)?;
            result.push((
                RequirementCategory::Cardinality,
                RequirementBounds::Cardinality {
                    minimum: cores,
                    maximum: cores,
                },
            ));
            result.push((
                RequirementCategory::Service,
                RequirementBounds::MaximumServiceUnits(maximum_cycle_units),
            ));
        }
        GeneratedRoleKind::Terminal => {
            result.push((
                RequirementCategory::ArchitectureCapability,
                RequirementBounds::Capability(PlanningCapability::TypedTerminalLifecycle),
            ));
            result.push((
                RequirementCategory::Binding,
                RequirementBounds::Binding {
                    kind: PlanningBinding::Terminal,
                    minimum: 1,
                    maximum: 1,
                },
            ));
            result.push((
                RequirementCategory::LogicalLayout,
                RequirementBounds::Reservation {
                    kind: PlanningReservation::TerminalTransport,
                    multiplicity: PlanningMultiplicity::Once,
                },
            ));
        }
        GeneratedRoleKind::Panic => {
            result.push((
                RequirementCategory::ArchitectureCapability,
                RequirementBounds::Capability(PlanningCapability::PanicPulse),
            ));
            result.push((
                RequirementCategory::Binding,
                RequirementBounds::Binding {
                    kind: PlanningBinding::Panic,
                    minimum: 1,
                    maximum: 1,
                },
            ));
            result.push((
                RequirementCategory::LogicalLayout,
                RequirementBounds::Reservation {
                    kind: PlanningReservation::PanicState,
                    multiplicity: PlanningMultiplicity::PerCore,
                },
            ));
        }
        GeneratedRoleKind::Shutdown => result.push((
            RequirementCategory::ArchitectureCapability,
            RequirementBounds::Capability(PlanningCapability::GuestShutdownPulse),
        )),
        GeneratedRoleKind::TestRuntime => {
            let tests = checked_u32(test_application_count)?;
            result.push((
                RequirementCategory::Cardinality,
                RequirementBounds::Cardinality {
                    minimum: tests,
                    maximum: tests,
                },
            ));
        }
        GeneratedRoleKind::DisplayDriver
        | GeneratedRoleKind::InputDriver
        | GeneratedRoleKind::EventStoreRuntime
        | GeneratedRoleKind::EventStoreDriver
        | GeneratedRoleKind::MonotonicClockDriver
        | GeneratedRoleKind::EntropyDriver
        | GeneratedRoleKind::TelemetryDriver => {}
    }
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn verify_requirements(
    context: u128,
    roles: &[GeneratedRole],
    planner: PlannerRef,
    plan: DomainPlanRef,
    test_application_count: usize,
    core_count: usize,
    maximum_cycle_units: u32,
    cancellation: &Cancellation,
) -> Result<Vec<Requirement>, PlanningFailure> {
    let mut expected = Vec::new();
    for role in roles {
        for (offset, (category, bounds)) in verifier_requirement_specs(
            role,
            test_application_count,
            core_count,
            maximum_cycle_units,
        )?
        .into_iter()
        .enumerate()
        {
            checkpoint(cancellation)?;
            expected.push(verify_requirement(
                context,
                planner,
                plan,
                role.reference,
                u16::try_from(offset + 1).map_err(|_| {
                    PlanningFailure::Defect(Arc::from("verifier requirement site overflow"))
                })?,
                category,
                bounds,
            ));
        }
    }
    expected.sort_by_key(|requirement| requirement.reference.identity);
    Ok(expected)
}

fn verify_requirement(
    context: u128,
    owner: PlannerRef,
    plan: DomainPlanRef,
    subject: RoleRef,
    local_site: u16,
    category: RequirementCategory,
    bounds: RequirementBounds,
) -> Requirement {
    let identity =
        verify_requirement_identity(owner.identity, subject.identity, category, local_site);
    let current_meaning = verify_requirement_current_meaning(
        identity,
        owner.current_meaning,
        subject.current_meaning,
        plan.current_meaning,
        category,
        &bounds,
    );
    Requirement {
        context,
        reference: RequirementRef {
            context,
            identity,
            current_meaning,
        },
        owner,
        subject: RequirementOwner::GeneratedRole(subject),
        provenance: RequirementProvenance {
            domain_plan: plan.identity,
            generated_role: subject.identity,
            local_site,
        },
        category,
        bounds,
        facility_contract: None,
    }
}

fn verify_role_graph(
    candidate: &VerifiedPlanningFoundation,
    cancellation: &Cancellation,
) -> Result<(), PlanningFailure> {
    let Some(boot) = candidate
        .generated_roles
        .iter()
        .find(|role| role.kind == GeneratedRoleKind::Boot)
    else {
        return defect("Generated Role closure is missing mandatory boot infrastructure");
    };
    let identities = candidate
        .generated_roles
        .iter()
        .map(|role| role.reference.identity)
        .collect::<BTreeSet<_>>();
    for role in candidate.generated_roles.iter() {
        checkpoint(cancellation)?;
        if role.reference.context != candidate.context
            || role.executable.context != candidate.context
            || role.owner.context != candidate.context
            || role.generator.context != candidate.context
            || role.provenance.context != candidate.context
        {
            return defect("Generated Role uses mixed-context typed references");
        }
        for dependency in role.dependencies.iter() {
            if dependency.context != candidate.context || !identities.contains(&dependency.identity)
            {
                return defect("Generated Role has a dangling or mixed-context dependency");
            }
        }
        let mut seen = BTreeSet::new();
        let mut frontier = role.dependencies.iter().copied().collect::<Vec<_>>();
        while let Some(dependency) = frontier.pop() {
            checkpoint(cancellation)?;
            if dependency.identity == role.reference.identity {
                return defect("Generated Role dependency closure is cyclic");
            }
            if !seen.insert(dependency.identity) {
                continue;
            }
            let Some(record) = candidate
                .generated_roles
                .iter()
                .find(|record| record.reference.identity == dependency.identity)
            else {
                return defect("Generated Role dependency is dangling");
            };
            frontier.extend(record.dependencies.iter().copied());
        }
        let image_owned = matches!(
            role.kind,
            GeneratedRoleKind::Scheduler
                | GeneratedRoleKind::Terminal
                | GeneratedRoleKind::Panic
                | GeneratedRoleKind::Shutdown
                | GeneratedRoleKind::TestRuntime
        );
        if image_owned
            && !seen
                .iter()
                .any(|identity| boot.reference.identity == *identity)
        {
            return defect("Generated Role is unreachable from mandatory boot infrastructure");
        }
    }
    Ok(())
}

fn verify_architecture_evidence(
    candidate: &VerifiedPlanningFoundation,
    cancellation: &Cancellation,
) -> Result<(), PlanningFailure> {
    let architecture = candidate.architecture_contract.for_image_planning();
    let mut selected_shared_roles = BTreeSet::new();
    for requirement in candidate.requirements.iter() {
        checkpoint(cancellation)?;
        match requirement.bounds {
            RequirementBounds::Capability(capability)
                if !architecture.has_capability(capability.contract_kind()) =>
            {
                return defect("mandatory role lacks authenticated capability evidence");
            }
            RequirementBounds::Binding { kind, .. }
                if !architecture.has_binding(kind.contract_kind()) =>
            {
                return defect("mandatory role lacks authenticated binding evidence");
            }
            RequirementBounds::Reservation { kind, multiplicity } => {
                if !architecture.has_reservation(kind.contract_kind()) {
                    return defect("mandatory role lacks authenticated reservation evidence");
                }
                let expected = match kind {
                    PlanningReservation::PanicState => PlanningMultiplicity::PerCore,
                    PlanningReservation::BootState | PlanningReservation::TerminalTransport => {
                        PlanningMultiplicity::Once
                    }
                };
                if multiplicity != expected
                    || multiplicity.contract_kind() != expected.contract_kind()
                {
                    return defect("mandatory role reservation multiplicity is invalid");
                }
            }
            RequirementBounds::FacilitySharing(FacilitySharing::RegisteredDisjoint {
                role,
                maximum_units,
            }) => {
                let Some(registration) = architecture.facility_share(role.architecture_kind())
                else {
                    return defect("Facility sharing role is not registered by the profile");
                };
                if maximum_units > registration.maximum_units {
                    return defect("Facility sharing exceeds its registered semantic capacity");
                }
                if !selected_shared_roles.insert(role) {
                    return defect("Facility sharing roles are not pairwise disjoint");
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn verify_requirement_bounds(
    candidate: &VerifiedPlanningFoundation,
    cancellation: &Cancellation,
) -> Result<(), PlanningFailure> {
    for requirement in candidate.requirements.iter() {
        checkpoint(cancellation)?;
        let owning_plan = candidate.domain_plans.iter().find(|plan| {
            plan.reference.identity == requirement.provenance.domain_plan
                && plan.planner == requirement.owner
        });
        let provenance_subject = match requirement.subject {
            RequirementOwner::GeneratedRole(reference) => reference.identity,
            RequirementOwner::Pool(_) => 0,
            RequirementOwner::Facility(reference) => reference.identity,
        };
        let facility_reference_is_exact = match (requirement.subject, requirement.facility_contract)
        {
            (RequirementOwner::Facility(subject), Some(contract)) => {
                owning_plan.is_some_and(|plan| {
                    plan.facility_instance.as_ref().is_some_and(|instance| {
                        instance.identity == subject.identity
                            && instance.context == subject.context
                            && instance.current_meaning == subject.current_meaning
                            && instance.kind == subject.kind
                    }) && plan.facility_contract == Some(contract)
                        && contract.kind == subject.kind
                })
            }
            (RequirementOwner::Facility(_), None) => false,
            (_, None) => true,
            (_, Some(_)) => false,
        };
        if requirement.context != candidate.context
            || requirement.reference.context != candidate.context
            || requirement.owner.context != candidate.context
            || requirement.subject.context() != candidate.context
            || owning_plan.is_none()
            || requirement.provenance.generated_role != provenance_subject
            || requirement.provenance.local_site == 0
            || !facility_reference_is_exact
        {
            return defect(
                "Planning Requirement has dangling, wrong-owner, wrong-role, mixed-context, or invalid provenance",
            );
        }
        let facility_instance = owning_plan.and_then(|plan| plan.facility_instance.as_ref());
        let valid = match (&requirement.category, &requirement.bounds) {
            (
                RequirementCategory::GeneratedRoleRealization,
                RequirementBounds::RealizeExactlyOnce { executable },
            ) => candidate.generated_roles.iter().any(|role| {
                requirement.subject == RequirementOwner::GeneratedRole(role.reference)
                    && role.executable.identity == *executable
            }),
            (RequirementCategory::Lifetime, RequirementBounds::ImageLifetime) => true,
            (RequirementCategory::ArchitectureCapability, RequirementBounds::Capability(_)) => true,
            (
                RequirementCategory::Cardinality,
                RequirementBounds::Cardinality { minimum, maximum },
            ) => minimum <= maximum,
            (RequirementCategory::Service, RequirementBounds::MaximumServiceUnits(units)) => {
                *units > 0
            }
            (
                RequirementCategory::Binding,
                RequirementBounds::Binding {
                    minimum, maximum, ..
                },
            ) => *minimum > 0 && minimum <= maximum,
            (RequirementCategory::LogicalLayout, RequirementBounds::Reservation { .. }) => true,
            (
                RequirementCategory::CapacityPressure,
                RequirementBounds::PoolCapacity {
                    declared,
                    usable,
                    peak_live,
                    peak_reserved,
                    peak_committed,
                },
            ) => {
                usable <= declared
                    && peak_live.saturating_add(*peak_reserved) >= *peak_committed
                    && *peak_committed <= *usable
                    && matches!(requirement.subject, RequirementOwner::Pool(_))
            }
            (
                RequirementCategory::CapacityPressure,
                RequirementBounds::FacilityCapacity(capacity),
            ) => {
                facility_capacity_tag(*capacity) & u128::from(u64::MAX) > 0
                    && matches!(requirement.subject, RequirementOwner::Facility(_))
            }
            (RequirementCategory::Binding, RequirementBounds::FacilitySharing(sharing)) => {
                matches!(requirement.subject, RequirementOwner::Facility(_))
                    && match sharing {
                        FacilitySharing::Exclusive => true,
                        FacilitySharing::RegisteredDisjoint { maximum_units, .. } => {
                            *maximum_units > 0
                        }
                    }
            }
            (
                RequirementCategory::FacilityOwnership,
                RequirementBounds::FacilityEndpoint {
                    maximum,
                    ownership,
                    input_owner,
                },
            ) => {
                *maximum > 0
                    && matches!(requirement.subject, RequirementOwner::Facility(_))
                    && match ownership {
                        FacilityEndpointOwnership::FacilityInstance => input_owner.is_none(),
                        FacilityEndpointOwnership::BuildWiredActor => {
                            input_owner.is_some_and(|owner| {
                                owner.context == candidate.context
                                    && owner.identity != 0
                                    && owner.current_meaning != 0
                                    && facility_instance
                                        .is_some_and(|instance| instance.input_owner == Some(owner))
                            })
                        }
                    }
            }
            (
                RequirementCategory::Recovery,
                RequirementBounds::FacilityRecovery {
                    supervisor,
                    maximum_attempts,
                    ..
                },
            ) => {
                supervisor.context == candidate.context
                    && supervisor.identity != 0
                    && supervisor.current_meaning != 0
                    && facility_instance.is_some_and(|instance| instance.supervisor == *supervisor)
                    && *maximum_attempts > 0
                    && matches!(requirement.subject, RequirementOwner::Facility(_))
            }
            (RequirementCategory::Shutdown, RequirementBounds::FacilityShutdown(_))
            | (RequirementCategory::Replay, RequirementBounds::FacilityReplay { .. })
            | (RequirementCategory::Flagship, RequirementBounds::FacilityFlagship(_))
            | (
                RequirementCategory::BootAvailability,
                RequirementBounds::FacilityBindingAvailability(_),
            ) => matches!(requirement.subject, RequirementOwner::Facility(_)),
            _ => false,
        };
        if !valid {
            return defect("Planning Requirement category and typed bounds disagree");
        }
    }
    Ok(())
}

fn verify_executable_demand(
    candidate: &VerifiedPlanningFoundation,
    expected_roles: &[GeneratedRole],
) -> Result<(), PlanningFailure> {
    let semantic = candidate.semantic_program.for_image_planning();
    let source = DemandInputRef {
        context: candidate.context,
        identity: semantic.executable_demand_fingerprint(),
        current_meaning: semantic.executable_demand_fingerprint(),
    };
    let mut expected = expected_roles
        .iter()
        .map(|role| role.executable)
        .collect::<Vec<_>>();
    expected.sort_by_key(|executable| executable.identity);
    if candidate.executable_demand.source != source
        || candidate.executable_demand.source_count != semantic.source_executable_count()
        || candidate.executable_demand.additions.as_ref() != expected.as_slice()
    {
        return defect(
            "exact Executable Demand has missing, extra, duplicate, wrong-role, or mixed-context generated executables",
        );
    }
    let unique = candidate
        .executable_demand
        .additions
        .iter()
        .map(|reference| reference.identity)
        .collect::<BTreeSet<_>>();
    if unique.len() != candidate.executable_demand.additions.len() {
        return defect("exact Executable Demand contains a duplicate generated executable");
    }
    let fingerprint =
        verify_demand_fingerprint(source, semantic.source_executable_count(), &expected);
    if candidate.executable_demand.fingerprint != fingerprint {
        return defect("exact Executable Demand fingerprint is false");
    }
    Ok(())
}

fn produce_role_identity(planner: PlannerRef, kind: GeneratedRoleKind, local_key: u16) -> u128 {
    producer_hash(
        b"wrela.generated-role.identity.v1",
        &[
            planner.identity,
            planner.identity,
            kind.tag().into(),
            local_key.into(),
        ],
    )
}

fn verify_role_identity(planner: PlannerRef, kind: GeneratedRoleKind, local_key: u16) -> u128 {
    verifier_hash(
        b"wrela.generated-role.identity.v1",
        &[
            planner.identity,
            planner.identity,
            kind.tag().into(),
            local_key.into(),
        ],
    )
}

fn produce_role_current_meaning(
    identity: u128,
    plan: DomainPlanRef,
    dependencies: &[RoleRef],
    semantic_fingerprint: u128,
    architecture_fingerprint: u128,
) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(b"wrela.generated-role.meaning.v1");
    hash.update(&identity.to_le_bytes());
    hash.update(&plan.current_meaning.to_le_bytes());
    hash.update(&semantic_fingerprint.to_le_bytes());
    hash.update(&architecture_fingerprint.to_le_bytes());
    hash.update(&(dependencies.len() as u64).to_le_bytes());
    for dependency in dependencies {
        hash.update(&dependency.identity.to_le_bytes());
        hash.update(&dependency.current_meaning.to_le_bytes());
    }
    hash.digest128()
}

fn verify_role_current_meaning(
    identity: u128,
    plan: DomainPlanRef,
    dependencies: &[RoleRef],
    semantic_fingerprint: u128,
    architecture_fingerprint: u128,
) -> u128 {
    let mut verifier = Xxh3::new();
    verifier.update(b"wrela.generated-role.meaning.v1");
    verifier.update(&identity.to_le_bytes());
    verifier.update(&plan.current_meaning.to_le_bytes());
    verifier.update(&semantic_fingerprint.to_le_bytes());
    verifier.update(&architecture_fingerprint.to_le_bytes());
    verifier.update(&(dependencies.len() as u64).to_le_bytes());
    for dependency in dependencies {
        verifier.update(&dependency.identity.to_le_bytes());
        verifier.update(&dependency.current_meaning.to_le_bytes());
    }
    verifier.digest128()
}

fn produce_requirement_identity(
    owner: u128,
    subject: u128,
    category: RequirementCategory,
    local_site: u16,
) -> u128 {
    producer_hash(
        b"wrela.requirement.identity.v1",
        &[owner, subject, category.tag().into(), local_site.into()],
    )
}

fn verify_requirement_identity(
    owner: u128,
    subject: u128,
    category: RequirementCategory,
    local_site: u16,
) -> u128 {
    verifier_hash(
        b"wrela.requirement.identity.v1",
        &[owner, subject, category.tag().into(), local_site.into()],
    )
}

fn produce_requirement_current_meaning(
    reference: u128,
    owner_meaning: u128,
    subject_meaning: u128,
    plan_meaning: u128,
    category: RequirementCategory,
    bounds: &RequirementBounds,
) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(b"wrela.requirement.meaning.v1");
    for value in [reference, owner_meaning, subject_meaning, plan_meaning] {
        hash.update(&value.to_le_bytes());
    }
    hash.update(&[category.tag()]);
    produce_bounds_encoding(&mut hash, bounds);
    hash.digest128()
}

fn verify_requirement_current_meaning(
    reference: u128,
    owner_meaning: u128,
    subject_meaning: u128,
    plan_meaning: u128,
    category: RequirementCategory,
    bounds: &RequirementBounds,
) -> u128 {
    let mut verifier = Xxh3::new();
    verifier.update(b"wrela.requirement.meaning.v1");
    for value in [reference, owner_meaning, subject_meaning, plan_meaning] {
        verifier.update(&value.to_le_bytes());
    }
    verifier.update(&[category.tag()]);
    verify_bounds_encoding(&mut verifier, bounds);
    verifier.digest128()
}

fn produce_bounds_encoding(hash: &mut Xxh3, bounds: &RequirementBounds) {
    match bounds {
        RequirementBounds::RealizeExactlyOnce { executable } => {
            hash.update(&[1]);
            hash.update(&executable.to_le_bytes());
        }
        RequirementBounds::ImageLifetime => hash.update(&[2]),
        RequirementBounds::Capability(capability) => hash.update(&[3, capability.tag()]),
        RequirementBounds::Cardinality { minimum, maximum } => {
            hash.update(&[4]);
            hash.update(&minimum.to_le_bytes());
            hash.update(&maximum.to_le_bytes());
        }
        RequirementBounds::MaximumServiceUnits(units) => {
            hash.update(&[5]);
            hash.update(&units.to_le_bytes());
        }
        RequirementBounds::Binding {
            kind,
            minimum,
            maximum,
        } => {
            hash.update(&[6, kind.tag()]);
            hash.update(&minimum.to_le_bytes());
            hash.update(&maximum.to_le_bytes());
        }
        RequirementBounds::Reservation { kind, multiplicity } => {
            hash.update(&[7, kind.tag(), multiplicity.tag()])
        }
        RequirementBounds::PoolCapacity {
            declared,
            usable,
            peak_live,
            peak_reserved,
            peak_committed,
        } => {
            hash.update(&[8]);
            for value in [declared, usable, peak_live, peak_reserved, peak_committed] {
                hash.update(&value.to_le_bytes());
            }
        }
        RequirementBounds::FacilityCapacity(capacity) => {
            hash.update(&[9]);
            hash.update(&facility_capacity_tag(*capacity).to_le_bytes());
        }
        RequirementBounds::FacilitySharing(sharing) => {
            hash.update(&[10]);
            hash.update(&facility_sharing_tag(*sharing).to_le_bytes());
        }
        RequirementBounds::FacilityEndpoint {
            maximum,
            ownership,
            input_owner,
        } => {
            hash.update(&[11, facility_endpoint_ownership_tag(*ownership)]);
            hash.update(&maximum.to_le_bytes());
            hash.update(&[u8::from(input_owner.is_some())]);
            if let Some(owner) = input_owner {
                for value in [owner.context, owner.identity, owner.current_meaning] {
                    hash.update(&value.to_le_bytes());
                }
            }
        }
        RequirementBounds::FacilityRecovery {
            supervisor,
            loss_policy,
            maximum_attempts,
        } => {
            hash.update(&[12]);
            for value in [
                supervisor.context,
                supervisor.identity,
                supervisor.current_meaning,
            ] {
                hash.update(&value.to_le_bytes());
            }
            hash.update(&facility_loss_tag(*loss_policy).to_le_bytes());
            hash.update(&maximum_attempts.to_le_bytes());
        }
        RequirementBounds::FacilityShutdown(shutdown) => {
            hash.update(&[13]);
            hash.update(&facility_shutdown_tag(*shutdown).to_le_bytes());
        }
        RequirementBounds::FacilityReplay { selected, rule } => {
            hash.update(&[14, facility_replay_authority_tag(*selected)]);
            hash.update(&facility_replay_rule_tag(*rule).to_le_bytes());
        }
        RequirementBounds::FacilityFlagship(rule) => {
            hash.update(&[15]);
            hash.update(&facility_flagship_tag(*rule).to_le_bytes());
        }
        RequirementBounds::FacilityBindingAvailability(availability) => {
            hash.update(&[16, facility_binding_availability_tag(*availability)]);
        }
    }
}

fn verify_bounds_encoding(verifier: &mut Xxh3, bounds: &RequirementBounds) {
    match bounds {
        RequirementBounds::RealizeExactlyOnce { executable } => {
            verifier.update(&[1]);
            verifier.update(&executable.to_le_bytes());
        }
        RequirementBounds::ImageLifetime => verifier.update(&[2]),
        RequirementBounds::Capability(capability) => verifier.update(&[3, capability.tag()]),
        RequirementBounds::Cardinality { minimum, maximum } => {
            verifier.update(&[4]);
            verifier.update(&minimum.to_le_bytes());
            verifier.update(&maximum.to_le_bytes());
        }
        RequirementBounds::MaximumServiceUnits(units) => {
            verifier.update(&[5]);
            verifier.update(&units.to_le_bytes());
        }
        RequirementBounds::Binding {
            kind,
            minimum,
            maximum,
        } => {
            verifier.update(&[6, kind.tag()]);
            verifier.update(&minimum.to_le_bytes());
            verifier.update(&maximum.to_le_bytes());
        }
        RequirementBounds::Reservation { kind, multiplicity } => {
            verifier.update(&[7, kind.tag(), multiplicity.tag()])
        }
        RequirementBounds::PoolCapacity {
            declared,
            usable,
            peak_live,
            peak_reserved,
            peak_committed,
        } => {
            verifier.update(&[8]);
            for value in [declared, usable, peak_live, peak_reserved, peak_committed] {
                verifier.update(&value.to_le_bytes());
            }
        }
        RequirementBounds::FacilityCapacity(capacity) => {
            verifier.update(&[9]);
            verifier.update(&facility_capacity_tag(*capacity).to_le_bytes());
        }
        RequirementBounds::FacilitySharing(sharing) => {
            verifier.update(&[10]);
            verifier.update(&facility_sharing_tag(*sharing).to_le_bytes());
        }
        RequirementBounds::FacilityEndpoint {
            maximum,
            ownership,
            input_owner,
        } => {
            verifier.update(&[11, facility_endpoint_ownership_tag(*ownership)]);
            verifier.update(&maximum.to_le_bytes());
            verifier.update(&[u8::from(input_owner.is_some())]);
            if let Some(owner) = input_owner {
                for value in [owner.context, owner.identity, owner.current_meaning] {
                    verifier.update(&value.to_le_bytes());
                }
            }
        }
        RequirementBounds::FacilityRecovery {
            supervisor,
            loss_policy,
            maximum_attempts,
        } => {
            verifier.update(&[12]);
            for value in [
                supervisor.context,
                supervisor.identity,
                supervisor.current_meaning,
            ] {
                verifier.update(&value.to_le_bytes());
            }
            verifier.update(&facility_loss_tag(*loss_policy).to_le_bytes());
            verifier.update(&maximum_attempts.to_le_bytes());
        }
        RequirementBounds::FacilityShutdown(shutdown) => {
            verifier.update(&[13]);
            verifier.update(&facility_shutdown_tag(*shutdown).to_le_bytes());
        }
        RequirementBounds::FacilityReplay { selected, rule } => {
            verifier.update(&[14, facility_replay_authority_tag(*selected)]);
            verifier.update(&facility_replay_rule_tag(*rule).to_le_bytes());
        }
        RequirementBounds::FacilityFlagship(rule) => {
            verifier.update(&[15]);
            verifier.update(&facility_flagship_tag(*rule).to_le_bytes());
        }
        RequirementBounds::FacilityBindingAvailability(availability) => {
            verifier.update(&[16, facility_binding_availability_tag(*availability)]);
        }
    }
}

fn produce_demand_fingerprint(
    source: DemandInputRef,
    source_count: usize,
    additions: &[ExecutableRef],
) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(b"wrela.exact-executable-demand.v1");
    for value in [source.context, source.identity, source.current_meaning] {
        hash.update(&value.to_le_bytes());
    }
    hash.update(&(source_count as u64).to_le_bytes());
    hash.update(&(additions.len() as u64).to_le_bytes());
    for addition in additions {
        for value in [
            addition.context,
            addition.identity,
            addition.current_meaning,
        ] {
            hash.update(&value.to_le_bytes());
        }
    }
    hash.digest128()
}

fn verify_demand_fingerprint(
    source: DemandInputRef,
    source_count: usize,
    additions: &[ExecutableRef],
) -> u128 {
    let mut verifier = Xxh3::new();
    verifier.update(b"wrela.exact-executable-demand.v1");
    verifier.update(&source.context.to_le_bytes());
    verifier.update(&source.identity.to_le_bytes());
    verifier.update(&source.current_meaning.to_le_bytes());
    verifier.update(&(source_count as u64).to_le_bytes());
    verifier.update(&(additions.len() as u64).to_le_bytes());
    for addition in additions {
        verifier.update(&addition.context.to_le_bytes());
        verifier.update(&addition.identity.to_le_bytes());
        verifier.update(&addition.current_meaning.to_le_bytes());
    }
    verifier.digest128()
}

#[allow(clippy::too_many_arguments)]
fn produce_foundation_fingerprint(
    context: u128,
    semantic_fingerprint: u128,
    graph_fingerprint: u128,
    custody_fingerprint: u128,
    architecture_identity: u128,
    architecture_fingerprint: u128,
    architecture_input_receipt: u128,
    planners: &[Planner],
    plans: &[DomainPlan],
    roles: &[GeneratedRole],
    requirements: &[Requirement],
    pools: &[PoolPlan],
    pool_model: &PoolModelObservation,
    demand: &ExactExecutableDemand,
) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(PHASE_SCHEMA.as_bytes());
    hash.update(&SCHEMA_VERSION.to_le_bytes());
    for value in [
        context,
        semantic_fingerprint,
        graph_fingerprint,
        custody_fingerprint,
        architecture_identity,
        architecture_fingerprint,
        architecture_input_receipt,
    ] {
        hash.update(&value.to_le_bytes());
    }
    hash.update(&(planners.len() as u64).to_le_bytes());
    for planner in planners {
        hash.update(&planner.reference.identity.to_le_bytes());
        hash.update(&planner.reference.current_meaning.to_le_bytes());
    }
    hash.update(&(plans.len() as u64).to_le_bytes());
    for plan in plans {
        hash.update(&plan.reference.identity.to_le_bytes());
        hash.update(&plan.reference.current_meaning.to_le_bytes());
        hash.update(&(plan.generated_roles.len() as u64).to_le_bytes());
        for role in plan.generated_roles.iter() {
            hash.update(&role.identity.to_le_bytes());
            hash.update(&role.current_meaning.to_le_bytes());
        }
        hash.update(&(plan.requirements.len() as u64).to_le_bytes());
        for requirement in plan.requirements.iter() {
            hash.update(&requirement.identity.to_le_bytes());
            hash.update(&requirement.current_meaning.to_le_bytes());
        }
    }
    hash.update(&(roles.len() as u64).to_le_bytes());
    for role in roles {
        hash.update(&role.reference.identity.to_le_bytes());
        hash.update(&role.reference.current_meaning.to_le_bytes());
        hash.update(&role.executable.identity.to_le_bytes());
    }
    hash.update(&(requirements.len() as u64).to_le_bytes());
    for requirement in requirements {
        hash.update(&requirement.reference.identity.to_le_bytes());
        hash.update(&requirement.reference.current_meaning.to_le_bytes());
    }
    encode_pool_foundation(&mut hash, pools, pool_model);
    hash.update(&demand.fingerprint.to_le_bytes());
    hash.digest128()
}

#[allow(clippy::too_many_arguments)]
fn verify_foundation_fingerprint(
    context: u128,
    semantic_fingerprint: u128,
    graph_fingerprint: u128,
    custody_fingerprint: u128,
    architecture_identity: u128,
    architecture_fingerprint: u128,
    architecture_input_receipt: u128,
    planners: &[Planner],
    plans: &[DomainPlan],
    roles: &[GeneratedRole],
    requirements: &[Requirement],
    pools: &[PoolPlan],
    pool_model: &PoolModelObservation,
    demand: &ExactExecutableDemand,
) -> u128 {
    let mut verifier = Xxh3::new();
    verifier.update(PHASE_SCHEMA.as_bytes());
    verifier.update(&SCHEMA_VERSION.to_le_bytes());
    for value in [
        context,
        semantic_fingerprint,
        graph_fingerprint,
        custody_fingerprint,
        architecture_identity,
        architecture_fingerprint,
        architecture_input_receipt,
    ] {
        verifier.update(&value.to_le_bytes());
    }
    verifier.update(&(planners.len() as u64).to_le_bytes());
    for planner in planners {
        verifier.update(&planner.reference.identity.to_le_bytes());
        verifier.update(&planner.reference.current_meaning.to_le_bytes());
    }
    verifier.update(&(plans.len() as u64).to_le_bytes());
    for plan in plans {
        verifier.update(&plan.reference.identity.to_le_bytes());
        verifier.update(&plan.reference.current_meaning.to_le_bytes());
        verifier.update(&(plan.generated_roles.len() as u64).to_le_bytes());
        for role in plan.generated_roles.iter() {
            verifier.update(&role.identity.to_le_bytes());
            verifier.update(&role.current_meaning.to_le_bytes());
        }
        verifier.update(&(plan.requirements.len() as u64).to_le_bytes());
        for requirement in plan.requirements.iter() {
            verifier.update(&requirement.identity.to_le_bytes());
            verifier.update(&requirement.current_meaning.to_le_bytes());
        }
    }
    verifier.update(&(roles.len() as u64).to_le_bytes());
    for role in roles {
        verifier.update(&role.reference.identity.to_le_bytes());
        verifier.update(&role.reference.current_meaning.to_le_bytes());
        verifier.update(&role.executable.identity.to_le_bytes());
    }
    verifier.update(&(requirements.len() as u64).to_le_bytes());
    for requirement in requirements {
        verifier.update(&requirement.reference.identity.to_le_bytes());
        verifier.update(&requirement.reference.current_meaning.to_le_bytes());
    }
    encode_pool_foundation(&mut verifier, pools, pool_model);
    verifier.update(&demand.fingerprint.to_le_bytes());
    verifier.digest128()
}

fn encode_pool_foundation(hash: &mut Xxh3, pools: &[PoolPlan], model: &PoolModelObservation) {
    hash.update(&(pools.len() as u64).to_le_bytes());
    for pool in pools {
        hash.update(&pool.reference.identity.to_le_bytes());
        hash.update(&pool.reference.current_meaning.to_le_bytes());
        hash.update(&pool.executable.identity().to_le_bytes());
        for value in [
            pool.declared_capacity,
            pool.usable_slots,
            pool.peak_live,
            pool.peak_reserved,
            pool.peak_committed,
        ] {
            hash.update(&value.to_le_bytes());
        }
        hash.update(&(pool.admission_sites.len() as u64).to_le_bytes());
        for site in pool.admission_sites.iter() {
            hash.update(&[site.operation.canonical_tag()]);
            hash.update(&site.source.start().to_le_bytes());
            hash.update(&site.source.end().to_le_bytes());
            hash.update(&site.source_type_identity.to_le_bytes());
            hash.update(&site.requirement.identity.to_le_bytes());
            hash.update(&site.requirement.current_meaning.to_le_bytes());
        }
    }
    hash.update(&(model.cases as u64).to_le_bytes());
    hash.update(&[
        u8::from(model.agrees),
        u8::from(model.accepted),
        u8::from(model.full),
        u8::from(model.released),
        u8::from(model.reserved),
        u8::from(model.stale),
        u8::from(model.retired),
    ]);
}

fn producer_hash(domain: &[u8], values: &[u128]) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(domain);
    for value in values {
        hash.update(&value.to_le_bytes());
    }
    hash.digest128()
}

fn verifier_hash(domain: &[u8], values: &[u128]) -> u128 {
    let mut verifier = Xxh3::new();
    verifier.update(domain);
    for value in values {
        verifier.update(&value.to_le_bytes());
    }
    verifier.digest128()
}

const fn root_tag(root: Root) -> u8 {
    match root {
        Root::Image => 1,
        Root::Test => 2,
    }
}

fn checked_u32(value: usize) -> Result<u32, PlanningFailure> {
    u32::try_from(value).map_err(|_| {
        PlanningFailure::Defect(Arc::from("planning cardinality exceeds schema width"))
    })
}

fn checkpoint(cancellation: &Cancellation) -> Result<(), PlanningFailure> {
    if cancellation.is_cancelled() {
        Err(PlanningFailure::Cancelled)
    } else {
        Ok(())
    }
}

fn defect<T>(evidence: &'static str) -> Result<T, PlanningFailure> {
    Err(PlanningFailure::Defect(Arc::from(evidence)))
}

const MAX_SOLVER_EXPLORATION_STATES: usize = 1_000_000;
const MAX_CONFLICT_REQUIREMENTS: usize = 24;
type PlacementPairs = Vec<(u128, u16)>;
type BindingPairs = Vec<(u128, u8)>;
type CandidateAssignment = (PlacementPairs, BindingPairs);
type CandidateSearch = Result<Option<CandidateAssignment>, PlanningFailure>;

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
struct CanonicalProblem {
    name: &'static str,
    cores: Vec<CoreResource>,
    executables: Vec<u128>,
    bindings: Vec<BindingResource>,
    binding_subjects: Vec<u128>,
    capabilities: BTreeSet<u8>,
    requirements: Vec<SolverRequirement>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CoreResource {
    identity: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct BindingResource {
    identity: u8,
    kind: u8,
    shareable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SolverRequirement {
    identity: u128,
    source: RequirementSource,
    current_meaning: u128,
    category: SolverRequirementCategory,
    constraint: SolverConstraint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(dead_code)]
enum SolverRequirementCategory {
    RoleRealization,
    RequiredCapability,
    Cardinality,
    Binding,
    Capacity,
    Placement,
    Affinity,
    Separation,
    Exclusivity,
    ActivationLifetime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum SolverConstraint {
    Realize {
        executable: u128,
    },
    Capability {
        capability: u8,
    },
    Cardinality {
        selected: u64,
        minimum: u64,
        maximum: u64,
    },
    Binding {
        subject: u128,
        kind: u8,
        minimum: u16,
        maximum: u16,
        allow_sharing: bool,
    },
    Capacity {
        required: u64,
        available: u64,
    },
    Static {
        satisfied: bool,
    },
    CoreCapacity {
        core: u16,
        maximum: u16,
    },
    AllowedCores {
        executable: u128,
        cores: Arc<[u16]>,
    },
    Affinity {
        left: u128,
        right: u128,
    },
    AffinityGroup {
        executables: Arc<[u128]>,
    },
    Separation {
        left: u128,
        right: u128,
    },
    Exclusive {
        executable: u128,
    },
    Activation {
        executable: u128,
        units: u16,
        start: u16,
        end: u16,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum CanonicalSolveOutcome {
    Assignment {
        placements: Arc<[(u128, u16)]>,
        bindings: Arc<[(u128, u8)]>,
        discharges: Arc<[u128]>,
    },
    Conflict(VerifiedPrivateConflict),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VerifiedPrivateConflict {
    code: ConflictCode,
    requirements: Arc<[u128]>,
}

impl VerifiedPrivateConflict {
    pub(crate) fn requirement_count(&self) -> usize {
        self.requirements.len()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConflictCode {
    RoleClosure,
    MissingCapability,
    Cardinality,
    Binding,
    Capacity,
    Placement,
    Affinity,
    Separation,
    Exclusivity,
    ActivationLifetime,
}

fn solve_canonical_problem(
    puzzle: &CanonicalProblem,
    cancellation: &Cancellation,
) -> Result<CanonicalSolveOutcome, PlanningFailure> {
    let puzzle = canonicalize_problem(puzzle)?;
    let all = (0..puzzle.requirements.len()).collect::<Vec<_>>();
    let mut explored = 0;
    if let Some((placements, bindings)) =
        search_canonical_assignment(&puzzle, &all, cancellation, &mut explored)?
    {
        let mut discharges = puzzle
            .requirements
            .iter()
            .map(|requirement| requirement.identity)
            .collect::<Vec<_>>();
        discharges.sort_unstable();
        return Ok(CanonicalSolveOutcome::Assignment {
            placements: placements.into(),
            bindings: bindings.into(),
            discharges: discharges.into(),
        });
    }
    if puzzle.requirements.len() > MAX_CONFLICT_REQUIREMENTS {
        return defect("solver conflict minimization exhausted its authenticated finite bound");
    }
    let conflict = minimum_conflict(&puzzle, cancellation, &mut explored)?;
    Ok(CanonicalSolveOutcome::Conflict(conflict))
}

fn canonicalize_problem(puzzle: &CanonicalProblem) -> Result<CanonicalProblem, PlanningFailure> {
    let mut puzzle = puzzle.clone();
    puzzle.cores.sort_unstable();
    puzzle.executables.sort_unstable();
    puzzle.bindings.sort_unstable();
    puzzle.binding_subjects.sort_unstable();
    puzzle
        .requirements
        .sort_by_key(|requirement| (requirement.category, requirement.identity));
    let binding_identity_count = puzzle
        .bindings
        .iter()
        .map(|binding| binding.identity)
        .collect::<BTreeSet<_>>()
        .len();
    let requirement_identity_count = puzzle
        .requirements
        .iter()
        .map(|requirement| requirement.identity)
        .collect::<BTreeSet<_>>()
        .len();
    if puzzle.cores.is_empty()
        || puzzle.cores.windows(2).any(|pair| pair[0] == pair[1])
        || puzzle.executables.windows(2).any(|pair| pair[0] == pair[1])
        || puzzle
            .binding_subjects
            .windows(2)
            .any(|pair| pair[0] == pair[1])
        || binding_identity_count != puzzle.bindings.len()
        || requirement_identity_count != puzzle.requirements.len()
    {
        return defect("canonical solver problem is malformed or contains duplicate identities");
    }
    for requirement in &puzzle.requirements {
        let malformed = match &requirement.constraint {
            SolverConstraint::Cardinality {
                minimum, maximum, ..
            } => minimum > maximum,
            SolverConstraint::Binding {
                subject,
                minimum,
                maximum,
                ..
            } => minimum > maximum || !puzzle.binding_subjects.contains(subject),
            SolverConstraint::CoreCapacity { core, .. } => !puzzle
                .cores
                .iter()
                .any(|candidate| candidate.identity == *core),
            SolverConstraint::AllowedCores { executable, cores } => {
                !puzzle.executables.contains(executable)
                    || cores.is_empty()
                    || cores.iter().any(|core| {
                        !puzzle
                            .cores
                            .iter()
                            .any(|candidate| candidate.identity == *core)
                    })
            }
            SolverConstraint::Affinity { left, right }
            | SolverConstraint::Separation { left, right } => {
                !puzzle.executables.contains(left) || !puzzle.executables.contains(right)
            }
            SolverConstraint::AffinityGroup { executables } => {
                executables.is_empty()
                    || executables
                        .iter()
                        .any(|executable| !puzzle.executables.contains(executable))
            }
            SolverConstraint::Exclusive { executable } => !puzzle.executables.contains(executable),
            SolverConstraint::Activation {
                executable,
                units,
                start,
                end,
            } => !puzzle.executables.contains(executable) || *units == 0 || start >= end,
            SolverConstraint::Realize { .. }
            | SolverConstraint::Capability { .. }
            | SolverConstraint::Capacity { .. }
            | SolverConstraint::Static { .. } => false,
        };
        if malformed {
            return defect("canonical solver Requirement has a malformed or dangling subject");
        }
    }
    Ok(puzzle)
}

fn search_canonical_assignment(
    puzzle: &CanonicalProblem,
    active: &[usize],
    cancellation: &Cancellation,
    explored: &mut usize,
) -> CandidateSearch {
    if !static_constraints_hold(puzzle, active) {
        return Ok(None);
    }
    let binding_requirements = active
        .iter()
        .flat_map(|ordinal| match &puzzle.requirements[*ordinal].constraint {
            SolverConstraint::Binding {
                subject,
                kind,
                minimum,
                allow_sharing,
                ..
            } => vec![(*subject, *kind, *allow_sharing); usize::from(*minimum)],
            _ => Vec::new(),
        })
        .collect::<Vec<_>>();
    let binding_choices = binding_requirements
        .iter()
        .map(|(_, kind, _)| {
            puzzle
                .bindings
                .iter()
                .filter(|slot| slot.kind == *kind)
                .copied()
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    if binding_choices.iter().any(Vec::is_empty) {
        return Ok(None);
    }
    let placement_radices = vec![puzzle.cores.len(); puzzle.executables.len()];
    let mut placement_digits = vec![0; puzzle.executables.len()];
    loop {
        checkpoint(cancellation)?;
        *explored = explored.saturating_add(1);
        if *explored > MAX_SOLVER_EXPLORATION_STATES {
            return defect("canonical solver exploration exhausted its bounded state budget");
        }
        let placements = puzzle
            .executables
            .iter()
            .zip(&placement_digits)
            .map(|(executable, digit)| (*executable, puzzle.cores[*digit].identity))
            .collect::<Vec<_>>();
        if placement_constraints_hold(puzzle, active, &placements) {
            if binding_requirements.is_empty() {
                return Ok(Some((placements, Vec::new())));
            }
            let binding_radices = binding_choices.iter().map(Vec::len).collect::<Vec<_>>();
            let mut binding_digits = vec![0; binding_requirements.len()];
            loop {
                checkpoint(cancellation)?;
                *explored = explored.saturating_add(1);
                if *explored > MAX_SOLVER_EXPLORATION_STATES {
                    return defect(
                        "canonical binding exploration exhausted its bounded state budget",
                    );
                }
                let bindings = binding_requirements
                    .iter()
                    .zip(&binding_digits)
                    .enumerate()
                    .map(|(ordinal, ((subject, _, _), digit))| {
                        (*subject, binding_choices[ordinal][*digit].identity)
                    })
                    .collect::<Vec<_>>();
                if binding_candidate_compatible(
                    &binding_requirements,
                    &binding_choices,
                    &binding_digits,
                ) {
                    return Ok(Some((placements, bindings)));
                }
                if !advance_digits(&mut binding_digits, &binding_radices) {
                    break;
                }
            }
        }
        if !advance_digits(&mut placement_digits, &placement_radices) {
            break;
        }
    }
    Ok(None)
}

fn binding_candidate_compatible(
    requirements: &[(u128, u8, bool)],
    choices: &[Vec<BindingResource>],
    digits: &[usize],
) -> bool {
    for left in 0..requirements.len() {
        for right in left + 1..requirements.len() {
            let left_slot = choices[left][digits[left]];
            let right_slot = choices[right][digits[right]];
            if left_slot.identity != right_slot.identity {
                continue;
            }
            let (left_subject, _, left_sharing) = requirements[left];
            let (right_subject, _, right_sharing) = requirements[right];
            if left_subject == right_subject
                || !left_sharing
                || !right_sharing
                || !left_slot.shareable
            {
                return false;
            }
        }
    }
    true
}

fn advance_digits(digits: &mut [usize], radices: &[usize]) -> bool {
    for ordinal in (0..digits.len()).rev() {
        if digits[ordinal] + 1 < radices[ordinal] {
            digits[ordinal] += 1;
            digits[ordinal + 1..].fill(0);
            return true;
        }
    }
    false
}

fn static_constraints_hold(puzzle: &CanonicalProblem, active: &[usize]) -> bool {
    active
        .iter()
        .all(|ordinal| match &puzzle.requirements[*ordinal].constraint {
            SolverConstraint::Realize { executable } => puzzle.executables.contains(executable),
            SolverConstraint::Capability { capability } => puzzle.capabilities.contains(capability),
            SolverConstraint::Cardinality {
                selected,
                minimum,
                maximum,
            } => minimum <= selected && selected <= maximum,
            SolverConstraint::Capacity {
                required,
                available,
            } => required <= available,
            SolverConstraint::Static { satisfied } => *satisfied,
            SolverConstraint::Activation { start, end, .. } => start < end,
            SolverConstraint::Binding { .. }
            | SolverConstraint::CoreCapacity { .. }
            | SolverConstraint::AllowedCores { .. }
            | SolverConstraint::Affinity { .. }
            | SolverConstraint::AffinityGroup { .. }
            | SolverConstraint::Separation { .. }
            | SolverConstraint::Exclusive { .. } => true,
        })
}

fn placement_constraints_hold(
    puzzle: &CanonicalProblem,
    active: &[usize],
    placements: &[(u128, u16)],
) -> bool {
    let placed = placements.iter().copied().collect::<BTreeMap<_, _>>();
    for ordinal in active {
        match &puzzle.requirements[*ordinal].constraint {
            SolverConstraint::AllowedCores { executable, cores } => {
                if !placed
                    .get(executable)
                    .is_some_and(|core| cores.contains(core))
                {
                    return false;
                }
            }
            SolverConstraint::Affinity { left, right } => {
                if placed.get(left) != placed.get(right) {
                    return false;
                }
            }
            SolverConstraint::AffinityGroup { executables } => {
                let mut cores = executables
                    .iter()
                    .filter_map(|executable| placed.get(executable));
                if let Some(first) = cores.next()
                    && cores.any(|core| core != first)
                {
                    return false;
                }
            }
            SolverConstraint::Separation { left, right } => {
                if placed.get(left) == placed.get(right) {
                    return false;
                }
            }
            SolverConstraint::CoreCapacity { core, maximum } => {
                let activations = active_activations(puzzle, active, placements, *core);
                if maximum_simultaneous_units(&activations) > *maximum {
                    return false;
                }
            }
            SolverConstraint::Exclusive { executable } => {
                let Some(core) = placed.get(executable) else {
                    return false;
                };
                let activations = active_activations(puzzle, active, placements, *core);
                let own = activations
                    .iter()
                    .filter(|activation| activation.0 == *executable)
                    .collect::<Vec<_>>();
                if own.iter().any(|activation| {
                    activations.iter().any(|other| {
                        other.0 != *executable
                            && intervals_overlap(activation.2, activation.3, other.2, other.3)
                    })
                }) {
                    return false;
                }
            }
            SolverConstraint::Realize { .. }
            | SolverConstraint::Capability { .. }
            | SolverConstraint::Cardinality { .. }
            | SolverConstraint::Capacity { .. }
            | SolverConstraint::Static { .. }
            | SolverConstraint::Binding { .. }
            | SolverConstraint::Activation { .. } => {}
        }
    }
    true
}

fn active_activations(
    puzzle: &CanonicalProblem,
    active: &[usize],
    placements: &[(u128, u16)],
    core: u16,
) -> Vec<(u128, u16, u16, u16)> {
    let placed = placements.iter().copied().collect::<BTreeMap<_, _>>();
    active
        .iter()
        .filter_map(|ordinal| match puzzle.requirements[*ordinal].constraint {
            SolverConstraint::Activation {
                executable,
                units,
                start,
                end,
            } if placed.get(&executable) == Some(&core) => Some((executable, units, start, end)),
            _ => None,
        })
        .collect()
}

fn maximum_simultaneous_units(activations: &[(u128, u16, u16, u16)]) -> u16 {
    activations
        .iter()
        .flat_map(|activation| [activation.2, activation.3.saturating_sub(1)])
        .map(|point| {
            activations
                .iter()
                .filter(|activation| activation.2 <= point && point < activation.3)
                .fold(0_u16, |sum, activation| sum.saturating_add(activation.1))
        })
        .max()
        .unwrap_or(0)
}

const fn intervals_overlap(
    left_start: u16,
    left_end: u16,
    right_start: u16,
    right_end: u16,
) -> bool {
    left_start < right_end && right_start < left_end
}

fn minimum_conflict(
    puzzle: &CanonicalProblem,
    cancellation: &Cancellation,
    explored: &mut usize,
) -> Result<VerifiedPrivateConflict, PlanningFailure> {
    let conflict = find_minimum_conflict(puzzle, cancellation, explored)?;
    verify_private_conflict(puzzle, &conflict, cancellation)?;
    Ok(conflict)
}

fn find_minimum_conflict(
    puzzle: &CanonicalProblem,
    cancellation: &Cancellation,
    explored: &mut usize,
) -> Result<VerifiedPrivateConflict, PlanningFailure> {
    for size in 1..=puzzle.requirements.len() {
        let mut combination = (0..size).collect::<Vec<_>>();
        loop {
            checkpoint(cancellation)?;
            if search_canonical_assignment(puzzle, &combination, cancellation, explored)?.is_none()
            {
                let requirements = combination
                    .iter()
                    .map(|ordinal| puzzle.requirements[*ordinal].identity)
                    .collect::<Vec<_>>();
                let code = conflict_code(
                    puzzle.requirements[combination[0]].category,
                    combination
                        .iter()
                        .map(|ordinal| puzzle.requirements[*ordinal].category),
                );
                return Ok(VerifiedPrivateConflict {
                    code,
                    requirements: requirements.into(),
                });
            }
            if !advance_combination(&mut combination, puzzle.requirements.len()) {
                break;
            }
        }
    }
    defect("infeasible solver puzzle has no irreducible conflict")
}

fn advance_combination(combination: &mut [usize], length: usize) -> bool {
    for ordinal in (0..combination.len()).rev() {
        let maximum = length - (combination.len() - ordinal);
        if combination[ordinal] < maximum {
            combination[ordinal] += 1;
            for following in ordinal + 1..combination.len() {
                combination[following] = combination[following - 1] + 1;
            }
            return true;
        }
    }
    false
}

fn conflict_code(
    first: SolverRequirementCategory,
    categories: impl Iterator<Item = SolverRequirementCategory>,
) -> ConflictCode {
    let categories = categories.collect::<BTreeSet<_>>();
    let category = if categories.contains(&SolverRequirementCategory::RequiredCapability) {
        SolverRequirementCategory::RequiredCapability
    } else if categories.contains(&SolverRequirementCategory::Cardinality) {
        SolverRequirementCategory::Cardinality
    } else if categories.contains(&SolverRequirementCategory::RoleRealization) {
        SolverRequirementCategory::RoleRealization
    } else if categories.contains(&SolverRequirementCategory::Binding) {
        SolverRequirementCategory::Binding
    } else if categories.contains(&SolverRequirementCategory::Capacity) {
        SolverRequirementCategory::Capacity
    } else {
        first
    };
    match category {
        SolverRequirementCategory::RoleRealization => ConflictCode::RoleClosure,
        SolverRequirementCategory::RequiredCapability => ConflictCode::MissingCapability,
        SolverRequirementCategory::Cardinality => ConflictCode::Cardinality,
        SolverRequirementCategory::Binding => ConflictCode::Binding,
        SolverRequirementCategory::Capacity => ConflictCode::Capacity,
        SolverRequirementCategory::Placement => ConflictCode::Placement,
        SolverRequirementCategory::Affinity => ConflictCode::Affinity,
        SolverRequirementCategory::Separation => ConflictCode::Separation,
        SolverRequirementCategory::Exclusivity => ConflictCode::Exclusivity,
        SolverRequirementCategory::ActivationLifetime => ConflictCode::ActivationLifetime,
    }
}

fn verify_private_conflict(
    puzzle: &CanonicalProblem,
    conflict: &VerifiedPrivateConflict,
    cancellation: &Cancellation,
) -> Result<(), PlanningFailure> {
    let mut canonical_explored = 0;
    let expected = find_minimum_conflict(puzzle, cancellation, &mut canonical_explored)?;
    if conflict != &expected {
        return defect("private conflict is not the canonical minimum witness and code");
    }
    let active = conflict
        .requirements
        .iter()
        .map(|identity| {
            puzzle
                .requirements
                .iter()
                .position(|requirement| requirement.identity == *identity)
                .ok_or_else(|| {
                    PlanningFailure::Defect(Arc::from(
                        "private conflict names a foreign Requirement",
                    ))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut explored = 0;
    if search_canonical_assignment(puzzle, &active, cancellation, &mut explored)?.is_some() {
        return defect("private conflict is feasible");
    }
    for removed in 0..active.len() {
        let subset = active
            .iter()
            .enumerate()
            .filter_map(|(ordinal, requirement)| (ordinal != removed).then_some(*requirement))
            .collect::<Vec<_>>();
        if search_canonical_assignment(puzzle, &subset, cancellation, &mut explored)?.is_none() {
            return defect("private conflict is not irreducible");
        }
    }
    Ok(())
}

#[cfg(test)]
fn independently_enumerate_tiny_puzzle(puzzle: &CanonicalProblem) -> CanonicalSolveOutcome {
    let puzzle = oracle_canonicalize_problem(puzzle);
    let all = (0..puzzle.requirements.len()).collect::<Vec<_>>();
    if let Some((placements, bindings)) = oracle_first_assignment(&puzzle, &all) {
        let mut discharges = puzzle
            .requirements
            .iter()
            .map(|requirement| requirement.identity)
            .collect::<Vec<_>>();
        discharges.sort_unstable();
        return CanonicalSolveOutcome::Assignment {
            placements: placements.into(),
            bindings: bindings.into(),
            discharges: discharges.into(),
        };
    }
    for size in 1..=puzzle.requirements.len() {
        let mut combination = (0..size).collect::<Vec<_>>();
        loop {
            if oracle_first_assignment(&puzzle, &combination).is_none() {
                let first = puzzle.requirements[combination[0]].category;
                return CanonicalSolveOutcome::Conflict(VerifiedPrivateConflict {
                    code: oracle_conflict_code(
                        first,
                        combination
                            .iter()
                            .map(|ordinal| puzzle.requirements[*ordinal].category),
                    ),
                    requirements: combination
                        .iter()
                        .map(|ordinal| puzzle.requirements[*ordinal].identity)
                        .collect::<Vec<_>>()
                        .into(),
                });
            }
            if !oracle_advance_combination(&mut combination, puzzle.requirements.len()) {
                break;
            }
        }
    }
    unreachable!("finite infeasible oracle puzzle has a conflict")
}

#[cfg(test)]
fn oracle_canonicalize_problem(puzzle: &CanonicalProblem) -> CanonicalProblem {
    let mut canonical = puzzle.clone();
    canonical.cores.sort_by_key(|core| core.identity);
    canonical.executables.sort_unstable();
    canonical.binding_subjects.sort_unstable();
    canonical
        .bindings
        .sort_by_key(|binding| (binding.identity, binding.kind, binding.shareable));
    canonical
        .requirements
        .sort_by_key(|requirement| (requirement.category, requirement.identity));
    assert!(!canonical.cores.is_empty(), "oracle puzzle has a core");
    assert_eq!(
        canonical
            .cores
            .iter()
            .map(|core| core.identity)
            .collect::<BTreeSet<_>>()
            .len(),
        canonical.cores.len(),
        "oracle core identities are unique"
    );
    assert_eq!(
        canonical
            .requirements
            .iter()
            .map(|requirement| requirement.identity)
            .collect::<BTreeSet<_>>()
            .len(),
        canonical.requirements.len(),
        "oracle Requirement identities are unique"
    );
    assert_eq!(
        canonical
            .binding_subjects
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len(),
        canonical.binding_subjects.len(),
        "oracle binding subjects are unique"
    );
    canonical
}

#[cfg(test)]
fn oracle_advance_combination(combination: &mut [usize], length: usize) -> bool {
    for position in (0..combination.len()).rev() {
        let last_allowed = length - (combination.len() - position);
        if combination[position] < last_allowed {
            combination[position] += 1;
            for following in position + 1..combination.len() {
                combination[following] = combination[following - 1] + 1;
            }
            return true;
        }
    }
    false
}

#[cfg(test)]
fn oracle_conflict_code(
    first: SolverRequirementCategory,
    categories: impl Iterator<Item = SolverRequirementCategory>,
) -> ConflictCode {
    let present = categories.collect::<BTreeSet<_>>();
    let selected = [
        SolverRequirementCategory::RequiredCapability,
        SolverRequirementCategory::Cardinality,
        SolverRequirementCategory::RoleRealization,
        SolverRequirementCategory::Binding,
        SolverRequirementCategory::Capacity,
    ]
    .into_iter()
    .find(|category| present.contains(category))
    .unwrap_or(first);
    match selected {
        SolverRequirementCategory::RoleRealization => ConflictCode::RoleClosure,
        SolverRequirementCategory::RequiredCapability => ConflictCode::MissingCapability,
        SolverRequirementCategory::Cardinality => ConflictCode::Cardinality,
        SolverRequirementCategory::Binding => ConflictCode::Binding,
        SolverRequirementCategory::Capacity => ConflictCode::Capacity,
        SolverRequirementCategory::Placement => ConflictCode::Placement,
        SolverRequirementCategory::Affinity => ConflictCode::Affinity,
        SolverRequirementCategory::Separation => ConflictCode::Separation,
        SolverRequirementCategory::Exclusivity => ConflictCode::Exclusivity,
        SolverRequirementCategory::ActivationLifetime => ConflictCode::ActivationLifetime,
    }
}

#[cfg(test)]
fn oracle_first_assignment(
    puzzle: &CanonicalProblem,
    active: &[usize],
) -> Option<CandidateAssignment> {
    let placement_radix = puzzle.cores.len();
    let placement_count =
        placement_radix.checked_pow(u32::try_from(puzzle.executables.len()).ok()?)?;
    let binding_requirements = active
        .iter()
        .flat_map(|ordinal| match puzzle.requirements[*ordinal].constraint {
            SolverConstraint::Binding {
                subject,
                kind,
                minimum,
                allow_sharing,
                ..
            } => vec![(subject, kind, allow_sharing); usize::from(minimum)],
            _ => Vec::new(),
        })
        .collect::<Vec<_>>();
    let binding_radix = puzzle.bindings.len().max(1);
    let binding_count =
        binding_radix.checked_pow(u32::try_from(binding_requirements.len()).ok()?)?;
    for placement_number in 0..placement_count {
        let placements = puzzle
            .executables
            .iter()
            .enumerate()
            .map(|(ordinal, executable)| {
                let divisor = placement_radix
                    .pow(u32::try_from(puzzle.executables.len() - ordinal - 1).unwrap_or(u32::MAX));
                let core = puzzle.cores[(placement_number / divisor) % placement_radix].identity;
                (*executable, core)
            })
            .collect::<Vec<_>>();
        for binding_number in 0..binding_count {
            let bindings = binding_requirements
                .iter()
                .enumerate()
                .map(|(ordinal, (subject, _, _))| {
                    let divisor = binding_radix.pow(
                        u32::try_from(binding_requirements.len() - ordinal - 1).unwrap_or(u32::MAX),
                    );
                    let slot = puzzle
                        .bindings
                        .get((binding_number / divisor) % binding_radix)
                        .map(|slot| slot.identity);
                    (*subject, slot.unwrap_or(u8::MAX))
                })
                .collect::<Vec<_>>();
            if oracle_candidate_feasible(puzzle, active, &placements, &bindings) {
                return Some((placements, bindings));
            }
        }
    }
    None
}

#[cfg(test)]
fn oracle_candidate_feasible(
    puzzle: &CanonicalProblem,
    active: &[usize],
    placements: &[(u128, u16)],
    bindings: &[(u128, u8)],
) -> bool {
    let core_of = |executable: u128| {
        placements
            .iter()
            .find(|(candidate, _)| *candidate == executable)
            .map(|(_, core)| *core)
    };
    for ordinal in active {
        match &puzzle.requirements[*ordinal].constraint {
            SolverConstraint::Realize { executable } => {
                if !puzzle.executables.contains(executable) {
                    return false;
                }
            }
            SolverConstraint::Capability { capability } => {
                if !puzzle.capabilities.contains(capability) {
                    return false;
                }
            }
            SolverConstraint::Cardinality {
                selected,
                minimum,
                maximum,
            } => {
                if selected < minimum || selected > maximum {
                    return false;
                }
            }
            SolverConstraint::Binding {
                subject,
                kind,
                minimum,
                maximum,
                allow_sharing,
            } => {
                let selected = bindings
                    .iter()
                    .filter(|(candidate, _)| candidate == subject)
                    .collect::<Vec<_>>();
                if selected.len() < usize::from(*minimum) || selected.len() > usize::from(*maximum)
                {
                    return false;
                }
                for (_, selected_slot) in selected {
                    let Some(slot) = puzzle
                        .bindings
                        .iter()
                        .find(|candidate| candidate.identity == *selected_slot)
                    else {
                        return false;
                    };
                    if slot.kind != *kind {
                        return false;
                    }
                    if bindings
                        .iter()
                        .filter(|(candidate, candidate_slot)| {
                            candidate == subject && candidate_slot == selected_slot
                        })
                        .count()
                        != 1
                    {
                        return false;
                    }
                    for (other, other_slot) in bindings.iter().filter(|(other, _)| other != subject)
                    {
                        if *other_slot != slot.identity {
                            continue;
                        }
                        let other_shared = active.iter().any(|other_ordinal| {
                            matches!(
                                puzzle.requirements[*other_ordinal].constraint,
                                SolverConstraint::Binding {
                                    subject: candidate,
                                    allow_sharing: true,
                                    ..
                                } if candidate == *other
                            )
                        });
                        if !*allow_sharing || !other_shared || !slot.shareable {
                            return false;
                        }
                    }
                }
            }
            SolverConstraint::Capacity {
                required,
                available,
            } => {
                if required > available {
                    return false;
                }
            }
            SolverConstraint::Static { satisfied } => {
                if !satisfied {
                    return false;
                }
            }
            SolverConstraint::CoreCapacity { core, maximum } => {
                let mut points = Vec::new();
                for activation_ordinal in active {
                    if let SolverConstraint::Activation { start, end, .. } =
                        puzzle.requirements[*activation_ordinal].constraint
                    {
                        points.extend([start, end.saturating_sub(1)]);
                    }
                }
                for point in points {
                    let used = active.iter().fold(0_u16, |sum, activation_ordinal| {
                        match puzzle.requirements[*activation_ordinal].constraint {
                            SolverConstraint::Activation {
                                executable,
                                units,
                                start,
                                end,
                            } if core_of(executable) == Some(*core)
                                && start <= point
                                && point < end =>
                            {
                                sum.saturating_add(units)
                            }
                            _ => sum,
                        }
                    });
                    if used > *maximum {
                        return false;
                    }
                }
            }
            SolverConstraint::AllowedCores { executable, cores } => {
                if !core_of(*executable).is_some_and(|core| cores.contains(&core)) {
                    return false;
                }
            }
            SolverConstraint::Affinity { left, right } => {
                if core_of(*left) != core_of(*right) {
                    return false;
                }
            }
            SolverConstraint::AffinityGroup { executables } => {
                let mut cores = executables.iter().map(|executable| core_of(*executable));
                if let Some(first) = cores.next()
                    && cores.any(|core| core != first)
                {
                    return false;
                }
            }
            SolverConstraint::Separation { left, right } => {
                if core_of(*left) == core_of(*right) {
                    return false;
                }
            }
            SolverConstraint::Exclusive { executable } => {
                let Some(core) = core_of(*executable) else {
                    return false;
                };
                let own = active.iter().filter_map(|activation_ordinal| {
                    match puzzle.requirements[*activation_ordinal].constraint {
                        SolverConstraint::Activation {
                            executable: candidate,
                            start,
                            end,
                            ..
                        } if candidate == *executable => Some((start, end)),
                        _ => None,
                    }
                });
                for (start, end) in own {
                    if active.iter().any(|activation_ordinal| {
                        matches!(
                            puzzle.requirements[*activation_ordinal].constraint,
                            SolverConstraint::Activation {
                                executable: other,
                                start: other_start,
                                end: other_end,
                                ..
                            } if other != *executable
                                && core_of(other) == Some(core)
                                && start < other_end && other_start < end
                        )
                    }) {
                        return false;
                    }
                }
            }
            SolverConstraint::Activation { start, end, .. } => {
                if start >= end {
                    return false;
                }
            }
        }
    }
    true
}

#[cfg(test)]
fn solver_requirement(
    identity: u128,
    category: SolverRequirementCategory,
    constraint: SolverConstraint,
) -> SolverRequirement {
    SolverRequirement {
        identity,
        source: RequirementSource::Domain,
        current_meaning: identity,
        category,
        constraint,
    }
}

#[cfg(test)]
fn named_tiny_solver_puzzles() -> Vec<CanonicalProblem> {
    let core = |identity| CoreResource { identity };
    let activation = |identity, executable, start, end| {
        solver_requirement(
            identity,
            SolverRequirementCategory::ActivationLifetime,
            SolverConstraint::Activation {
                executable,
                units: 1,
                start,
                end,
            },
        )
    };
    let capacity = |identity, core, maximum| {
        solver_requirement(
            identity,
            SolverRequirementCategory::Capacity,
            SolverConstraint::CoreCapacity { core, maximum },
        )
    };
    let base = |name, cores, executables, requirements| CanonicalProblem {
        name,
        cores,
        executables,
        bindings: Vec::new(),
        binding_subjects: Vec::new(),
        capabilities: BTreeSet::new(),
        requirements,
    };
    vec![
        base(
            "exact_fit",
            vec![core(0)],
            vec![10, 20],
            vec![
                capacity(1, 0, 2),
                activation(2, 10, 0, 2),
                activation(3, 20, 0, 2),
                solver_requirement(
                    4,
                    SolverRequirementCategory::Placement,
                    SolverConstraint::AllowedCores {
                        executable: 10,
                        cores: Arc::from([0]),
                    },
                ),
            ],
        ),
        base(
            "one_short",
            vec![core(0)],
            vec![10, 20],
            vec![
                capacity(1, 0, 1),
                activation(2, 10, 0, 2),
                activation(3, 20, 0, 2),
            ],
        ),
        CanonicalProblem {
            name: "binding",
            cores: vec![core(0)],
            executables: vec![10],
            bindings: vec![
                BindingResource {
                    identity: 0,
                    kind: 1,
                    shareable: false,
                },
                BindingResource {
                    identity: 1,
                    kind: 1,
                    shareable: false,
                },
            ],
            binding_subjects: vec![100, 200],
            capabilities: BTreeSet::new(),
            requirements: vec![
                solver_requirement(
                    1,
                    SolverRequirementCategory::Binding,
                    SolverConstraint::Binding {
                        subject: 100,
                        kind: 1,
                        minimum: 1,
                        maximum: 1,
                        allow_sharing: false,
                    },
                ),
                solver_requirement(
                    2,
                    SolverRequirementCategory::Binding,
                    SolverConstraint::Binding {
                        subject: 200,
                        kind: 1,
                        minimum: 1,
                        maximum: 1,
                        allow_sharing: false,
                    },
                ),
            ],
        },
        base(
            "cardinality",
            vec![core(0)],
            vec![10],
            vec![solver_requirement(
                1,
                SolverRequirementCategory::Cardinality,
                SolverConstraint::Cardinality {
                    selected: 2,
                    minimum: 0,
                    maximum: 1,
                },
            )],
        ),
        base(
            "affinity",
            vec![core(0), core(1)],
            vec![10, 20],
            vec![
                capacity(1, 0, 1),
                capacity(2, 1, 1),
                activation(3, 10, 0, 2),
                activation(4, 20, 0, 2),
                solver_requirement(
                    5,
                    SolverRequirementCategory::Affinity,
                    SolverConstraint::Affinity {
                        left: 10,
                        right: 20,
                    },
                ),
            ],
        ),
        base(
            "separation",
            vec![core(0)],
            vec![10, 20],
            vec![
                solver_requirement(
                    1,
                    SolverRequirementCategory::Separation,
                    SolverConstraint::Separation {
                        left: 10,
                        right: 20,
                    },
                ),
                solver_requirement(
                    2,
                    SolverRequirementCategory::Exclusivity,
                    SolverConstraint::Exclusive { executable: 10 },
                ),
                activation(3, 10, 0, 1),
                activation(4, 20, 0, 1),
            ],
        ),
        CanonicalProblem {
            name: "sharing",
            cores: vec![core(0)],
            executables: vec![10],
            bindings: vec![BindingResource {
                identity: 0,
                kind: 1,
                shareable: true,
            }],
            binding_subjects: vec![100, 200],
            capabilities: BTreeSet::new(),
            requirements: vec![
                solver_requirement(
                    1,
                    SolverRequirementCategory::Binding,
                    SolverConstraint::Binding {
                        subject: 100,
                        kind: 1,
                        minimum: 1,
                        maximum: 1,
                        allow_sharing: true,
                    },
                ),
                solver_requirement(
                    2,
                    SolverRequirementCategory::Binding,
                    SolverConstraint::Binding {
                        subject: 200,
                        kind: 1,
                        minimum: 1,
                        maximum: 1,
                        allow_sharing: true,
                    },
                ),
            ],
        },
        base(
            "role_closure",
            vec![core(0)],
            vec![10],
            vec![solver_requirement(
                1,
                SolverRequirementCategory::RoleRealization,
                SolverConstraint::Realize { executable: 99 },
            )],
        ),
        base(
            "lifetime",
            vec![core(0)],
            vec![10, 20],
            vec![
                capacity(1, 0, 1),
                activation(2, 10, 0, 1),
                activation(3, 20, 1, 2),
            ],
        ),
        base(
            "required_capability",
            vec![core(0)],
            vec![10],
            vec![solver_requirement(
                1,
                SolverRequirementCategory::RequiredCapability,
                SolverConstraint::Capability { capability: 7 },
            )],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::architecture_planning::{
        ArchitecturePlanningModule, ArchitectureProfile, ContractContext,
    };
    use crate::core::CoreModule;
    use crate::flow::FlowModule;
    use crate::{
        CompilationOutcome, CompilationRequest, Compiler, CompilerInstallation, ProjectFile,
        ProjectSnapshot,
    };

    fn fixture(root: Root) -> VerifiedPlanningFoundation {
        let (path, source): (&str, &[u8]) = match root {
            Root::Image => (
                "src/image.wr",
                b"@image\nfn build() -> Image:\n    return Image.new()\n",
            ),
            Root::Test => (
                "src/test.wr",
                br#"pub suite smoke:
    test passes():
        expect true

@image
fn build() -> Image:
    tests = Test.new(cases=[smoke.passes()])
    return Image.new(tests=tests)
"#,
            ),
        };
        fixture_from_source(path, source, root)
    }

    fn fixture_from_source(path: &str, source: &[u8], root: Root) -> VerifiedPlanningFoundation {
        let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
        let CompilationOutcome::Accepted(accepted) = compiler.compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new(path, source)]),
                root,
            ),
            &Cancellation::new(),
        ) else {
            panic!("semantic fixture accepts");
        };
        let semantic_program = Arc::new(accepted.completed_semantic_program().clone());
        let digest = semantic_program.for_image_planning().distribution_digest();
        let contract =
            ArchitecturePlanningModule::new(ContractContext::new("planning-test", digest))
                .authenticate(ArchitectureProfile::CurrentAarch64, &Cancellation::new())
                .expect("private contract authenticates");
        ImagePlanningModule
            .plan(semantic_program, Arc::new(contract), &Cancellation::new())
            .expect("foundation verifies")
    }

    fn rejects(candidate: &VerifiedPlanningFoundation) {
        assert!(matches!(
            verify(candidate, &Cancellation::new()),
            Err(PlanningFailure::Defect(_))
        ));
    }

    fn assignment_fixture() -> VerifiedWholeImageAssignment {
        let foundation = Arc::new(fixture(Root::Image));
        let core = Arc::new(
            CoreModule
                .derive(foundation.for_core(), &Cancellation::new())
                .expect("Core fixture verifies"),
        );
        let flow = Arc::new(
            FlowModule
                .derive(foundation.for_flow(), core.for_flow(), &Cancellation::new())
                .expect("Flow fixture verifies"),
        );
        let outcome = ImagePlanningModule
            .solve(foundation, core, flow, &Cancellation::new())
            .expect("whole-Image assignment verifies");
        let WholeImageSolveOutcome::Assignment(assignment) = outcome else {
            panic!("fixture Requirement Set is feasible");
        };
        assignment
    }

    fn pool_fixture() -> VerifiedPlanningFoundation {
        fixture_from_source(
            "src/image.wr",
            br#"from core import pool as pools

@image
fn build() -> Image:
    mut value = 0
    with pools.scoped(capacity=1) as scratch:
        allocation = scratch.allocate(value=1)
        value = scratch.reclaim(allocation=take allocation)
    return Image.new(value=value)
"#,
            Root::Image,
        )
    }

    fn facility_fixture() -> VerifiedPlanningFoundation {
        fixture_from_source(
            "src/image.wr",
            br#"from core import facilities

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
    entropy = facilities.Entropy.new(supervisor=coordinator, loss=facilities.SELECTING_IMAGE_POLICY, replay=facilities.NON_REPLAYABLE_FACILITY)
    return Image.new(display=display, entropy=entropy)
"#,
            Root::Image,
        )
    }

    fn input_facility_fixture() -> VerifiedPlanningFoundation {
        fixture_from_source(
            "src/image.wr",
            br#"from core import facilities

@actor
struct Coordinator implements facilities.FacilityActor:
    pure fn facility_identity(read self) -> u64:
        return 1
    pub async fn run(self):
        pass

@image
fn build() -> Image:
    coordinator = Coordinator()
    input = facilities.Input.new(owner=coordinator, supervisor=coordinator, loss=facilities.CONTROLLED_SHUTDOWN, replay=facilities.REPLAYABLE_GAMEPLAY)
    return Image.new(input=input)
"#,
            Root::Image,
        )
    }

    fn resign(candidate: &mut VerifiedPlanningFoundation) {
        let semantic = candidate.semantic_program.for_image_planning();
        let architecture = candidate.architecture_contract.for_image_planning();
        candidate.fingerprint = verify_foundation_fingerprint(
            candidate.context,
            semantic.fingerprint(),
            semantic.construction_graph_fingerprint(),
            semantic.custody_fingerprint(),
            architecture.identity(),
            architecture.fingerprint(),
            architecture.distribution_input_receipt(),
            &candidate.planner_roster,
            &candidate.domain_plans,
            &candidate.generated_roles,
            &candidate.requirements,
            &candidate.pools,
            &candidate.pool_model,
            &candidate.executable_demand,
        );
    }

    fn resign_facility_requirement_set(candidate: &mut VerifiedPlanningFoundation) {
        let mut requirements = candidate.requirements.to_vec();
        for requirement in &mut requirements {
            let subject_meaning = match requirement.subject {
                RequirementOwner::GeneratedRole(reference) => reference.current_meaning,
                RequirementOwner::Pool(reference) => reference.current_meaning,
                RequirementOwner::Facility(reference) => reference.current_meaning,
            };
            let plan = candidate
                .domain_plans
                .iter()
                .find(|plan| plan.reference.identity == requirement.provenance.domain_plan)
                .unwrap();
            requirement.reference.current_meaning = produce_requirement_current_meaning(
                requirement.reference.identity,
                requirement.owner.current_meaning,
                subject_meaning,
                plan.reference.current_meaning,
                requirement.category,
                &requirement.bounds,
            );
        }
        candidate.requirements = requirements.into();
        let mut plans = candidate.domain_plans.to_vec();
        for plan in &mut plans {
            plan.requirements = candidate
                .requirements
                .iter()
                .filter(|requirement| requirement.owner == plan.planner)
                .map(|requirement| requirement.reference)
                .collect::<Vec<_>>()
                .into();
        }
        candidate.domain_plans = plans.into();
        resign(candidate);
    }

    #[test]
    fn verifier_rejects_resigned_single_fault_pool_plan_and_model_corruption() {
        let original = pool_fixture();

        let mut pressure = original.clone();
        let mut pools = pressure.pools.to_vec();
        pools[0].peak_committed = 0;
        pressure.pools = pools.into();
        resign(&mut pressure);
        rejects(&pressure);

        let mut stale_requirement = original.clone();
        let mut requirements = stale_requirement.requirements.to_vec();
        let requirement = requirements
            .iter_mut()
            .find(|requirement| requirement.category == RequirementCategory::CapacityPressure)
            .expect("Pool capacity requirement");
        requirement.reference.current_meaning ^= 1;
        stale_requirement.requirements = requirements.into();
        resign(&mut stale_requirement);
        rejects(&stale_requirement);

        let mut model = original.clone();
        model.pool_model.retired = false;
        resign(&mut model);
        rejects(&model);
    }

    #[test]
    fn verifier_independently_rejects_consistently_low_pool_pressure() {
        let mut candidate = pool_fixture();
        let mut pools = candidate.pools.to_vec();
        let pool = &mut pools[0];
        pool.peak_live = 0;
        pool.peak_reserved = 0;
        pool.peak_committed = 0;
        pool.reference.current_meaning = producer_hash(
            b"wrela.pool-plan.meaning.v1",
            &[
                pool.reference.identity,
                pool.executable.current_meaning(),
                u128::from(pool.declared_capacity),
                0,
                0,
                0,
            ],
        );
        let mut requirements = candidate.requirements.to_vec();
        let requirement = requirements
            .iter_mut()
            .find(|requirement| requirement.category == RequirementCategory::CapacityPressure)
            .expect("Pool pressure requirement");
        requirement.subject = RequirementOwner::Pool(pool.reference);
        requirement.bounds = RequirementBounds::PoolCapacity {
            declared: pool.declared_capacity,
            usable: pool.usable_slots,
            peak_live: 0,
            peak_reserved: 0,
            peak_committed: 0,
        };
        requirement.reference.current_meaning = produce_requirement_current_meaning(
            requirement.reference.identity,
            requirement.owner.current_meaning,
            pool.reference.current_meaning,
            candidate.domain_plans[0].reference.current_meaning,
            requirement.category,
            &requirement.bounds,
        );
        pool.admission_sites = Arc::from([PoolAdmissionSite {
            requirement: requirement.reference,
            ..pool.admission_sites[0].clone()
        }]);
        candidate.pools = pools.into();
        candidate.requirements = requirements.into();
        let mut plan = candidate.domain_plans[0].clone();
        plan.requirements = candidate
            .requirements
            .iter()
            .map(|requirement| requirement.reference)
            .collect::<Vec<_>>()
            .into();
        candidate.domain_plans = Arc::from([plan]);
        resign(&mut candidate);
        rejects(&candidate);
    }

    #[test]
    fn verifier_oracle_rejects_every_exported_transition_fault() {
        let original = pool_fixture();
        let mut mutations = Vec::new();
        for field in 0..8 {
            let mut candidate = original.clone();
            match field {
                0 => candidate.pool_model.cases -= 1,
                1 => candidate.pool_model.agrees = false,
                2 => candidate.pool_model.accepted = false,
                3 => candidate.pool_model.full = false,
                4 => candidate.pool_model.released = false,
                5 => candidate.pool_model.reserved = false,
                6 => candidate.pool_model.stale = false,
                7 => candidate.pool_model.retired = false,
                _ => unreachable!(),
            }
            resign(&mut candidate);
            mutations.push(candidate);
        }
        for mutation in mutations {
            rejects(&mutation);
        }
        let oracle = independently_verify_pool_model();
        assert_eq!(oracle.cases, 10);
        assert!(oracle.agrees);
    }

    #[test]
    fn verifier_rejects_roster_and_domain_plan_corruption() {
        let original = fixture(Root::Image);

        let mut missing = original.clone();
        missing.planner_roster = Arc::from([]);
        rejects(&missing);

        let mut extra = original.clone();
        extra.planner_roster = Arc::from([
            original.planner_roster[0].clone(),
            original.planner_roster[0].clone(),
        ]);
        rejects(&extra);

        let mut wrong_owner = original.clone();
        let mut plan = wrong_owner.domain_plans[0].clone();
        plan.planner.identity ^= 1;
        wrong_owner.domain_plans = Arc::from([plan]);
        rejects(&wrong_owner);

        let mut mixed_context = original.clone();
        let mut planner = mixed_context.planner_roster[0].clone();
        planner.reference.context ^= 1;
        mixed_context.planner_roster = Arc::from([planner]);
        rejects(&mixed_context);

        let mut missing_role_export = original.clone();
        let mut plan = missing_role_export.domain_plans[0].clone();
        plan.generated_roles = original.domain_plans[0].generated_roles[1..].into();
        missing_role_export.domain_plans = Arc::from([plan]);
        rejects(&missing_role_export);

        let mut extra_requirement_export = original.clone();
        let mut plan = extra_requirement_export.domain_plans[0].clone();
        let mut requirements = plan.requirements.to_vec();
        requirements.push(plan.requirements[0]);
        plan.requirements = requirements.into();
        extra_requirement_export.domain_plans = Arc::from([plan]);
        rejects(&extra_requirement_export);
    }

    #[test]
    fn verifier_rejects_malformed_facility_contract_roles_capacities_sharing_cardinality_capabilities_and_contexts()
     {
        let original = facility_fixture();
        let display = original
            .facility_contracts
            .iter()
            .position(|contract| contract.observation.kind == FacilityKind::Display)
            .unwrap();
        let entropy = original
            .facility_contracts
            .iter()
            .position(|contract| contract.observation.kind == FacilityKind::Entropy)
            .unwrap();

        let mut role = original.clone();
        let mut contracts = role.facility_contracts.to_vec();
        contracts[display].observation.generated_roles = Arc::from([GeneratedRoleKind::Shutdown]);
        role.facility_contracts = contracts.into();
        rejects(&role);

        let mut capacity = original.clone();
        let mut contracts = capacity.facility_contracts.to_vec();
        contracts[display].observation.semantic_capacities =
            Arc::from([FacilitySemanticCapacity::FrameBuffers(0)]);
        capacity.facility_contracts = contracts.into();
        rejects(&capacity);

        let mut sharing = original.clone();
        let mut contracts = sharing.facility_contracts.to_vec();
        contracts[display].observation.sharing = FacilitySharing::RegisteredDisjoint {
            role: FacilitySharedRole::EntropyQueue,
            maximum_units: 0,
        };
        sharing.facility_contracts = contracts.into();
        rejects(&sharing);

        let mut cardinality = original.clone();
        let mut contracts = cardinality.facility_contracts.to_vec();
        contracts[display].observation.maximum_instances = 2;
        cardinality.facility_contracts = contracts.into();
        rejects(&cardinality);

        let mut capability = original.clone();
        let mut contracts = capability.facility_contracts.to_vec();
        contracts[display].observation.required_capabilities =
            Arc::from([PlanningCapability::MonotonicCounter]);
        capability.facility_contracts = contracts.into();
        rejects(&capability);

        let mut context = original.clone();
        let mut contracts = context.facility_contracts.to_vec();
        contracts[display].observation.context_receipt ^= 1;
        context.facility_contracts = contracts.into();
        rejects(&context);

        let mut replay_context = original.clone();
        let mut contracts = replay_context.facility_contracts.to_vec();
        contracts[entropy].observation.replay_rule = FacilityReplayRule::ReplayableGameplay;
        replay_context.facility_contracts = contracts.into();
        rejects(&replay_context);

        let mut mixed_plan = original.clone();
        let mut plans = mixed_plan.domain_plans.to_vec();
        let plan = plans
            .iter_mut()
            .find(|plan| plan.kind == DomainPlanKind::Facility(FacilityKind::Display))
            .unwrap();
        plan.facility_instance.as_mut().unwrap().context ^= 1;
        mixed_plan.domain_plans = plans.into();
        rejects(&mixed_plan);
    }

    #[test]
    fn verifier_rejects_wrong_kind_context_and_stale_typed_facility_references() {
        let original = facility_fixture();
        let display_plan =
            |plan: &DomainPlan| plan.kind == DomainPlanKind::Facility(FacilityKind::Display);

        let mut wrong_contract_kind = original.clone();
        let mut plans = wrong_contract_kind.domain_plans.to_vec();
        plans
            .iter_mut()
            .find(|plan| display_plan(plan))
            .unwrap()
            .facility_contract
            .as_mut()
            .unwrap()
            .kind = FacilityKind::Input;
        wrong_contract_kind.domain_plans = plans.into();
        resign(&mut wrong_contract_kind);
        rejects(&wrong_contract_kind);

        let mut wrong_contract_context = original.clone();
        let mut plans = wrong_contract_context.domain_plans.to_vec();
        plans
            .iter_mut()
            .find(|plan| display_plan(plan))
            .unwrap()
            .facility_contract
            .as_mut()
            .unwrap()
            .context ^= 1;
        wrong_contract_context.domain_plans = plans.into();
        resign(&mut wrong_contract_context);
        rejects(&wrong_contract_context);

        let mut stale_contract = original.clone();
        let mut plans = stale_contract.domain_plans.to_vec();
        plans
            .iter_mut()
            .find(|plan| display_plan(plan))
            .unwrap()
            .facility_contract
            .as_mut()
            .unwrap()
            .current_meaning ^= 1;
        stale_contract.domain_plans = plans.into();
        resign(&mut stale_contract);
        rejects(&stale_contract);

        let mut wrong_requirement_contract = original.clone();
        let mut requirements = wrong_requirement_contract.requirements.to_vec();
        requirements
            .iter_mut()
            .find(|requirement| {
                matches!(
                    requirement.subject,
                    RequirementOwner::Facility(FacilitySubjectRef {
                        kind: FacilityKind::Display,
                        ..
                    })
                )
            })
            .unwrap()
            .facility_contract
            .as_mut()
            .unwrap()
            .fingerprint ^= 1;
        wrong_requirement_contract.requirements = requirements.into();
        resign_facility_requirement_set(&mut wrong_requirement_contract);
        rejects(&wrong_requirement_contract);

        let mut wrong_subject_kind = original.clone();
        let mut requirements = wrong_subject_kind.requirements.to_vec();
        let requirement = requirements
            .iter_mut()
            .find(|requirement| {
                matches!(
                    requirement.subject,
                    RequirementOwner::Facility(FacilitySubjectRef {
                        kind: FacilityKind::Display,
                        ..
                    })
                )
            })
            .unwrap();
        let RequirementOwner::Facility(subject) = &mut requirement.subject else {
            unreachable!();
        };
        subject.kind = FacilityKind::Input;
        wrong_subject_kind.requirements = requirements.into();
        resign_facility_requirement_set(&mut wrong_subject_kind);
        rejects(&wrong_subject_kind);
    }

    #[test]
    fn verifier_rejects_wrong_context_and_stale_facility_actor_references() {
        let original = input_facility_fixture();

        let mut wrong_owner_context = original.clone();
        let mut requirements = wrong_owner_context.requirements.to_vec();
        let requirement = requirements
            .iter_mut()
            .find(|requirement| {
                matches!(
                    requirement.bounds,
                    RequirementBounds::FacilityEndpoint {
                        input_owner: Some(_),
                        ..
                    }
                )
            })
            .unwrap();
        let RequirementBounds::FacilityEndpoint {
            input_owner: Some(owner),
            ..
        } = &mut requirement.bounds
        else {
            unreachable!();
        };
        owner.context ^= 1;
        wrong_owner_context.requirements = requirements.into();
        resign_facility_requirement_set(&mut wrong_owner_context);
        rejects(&wrong_owner_context);

        let mut stale_supervisor = original.clone();
        let mut requirements = stale_supervisor.requirements.to_vec();
        let requirement = requirements
            .iter_mut()
            .find(|requirement| {
                matches!(
                    requirement.bounds,
                    RequirementBounds::FacilityRecovery { .. }
                )
            })
            .unwrap();
        let RequirementBounds::FacilityRecovery { supervisor, .. } = &mut requirement.bounds else {
            unreachable!();
        };
        supervisor.current_meaning ^= 1;
        stale_supervisor.requirements = requirements.into();
        resign_facility_requirement_set(&mut stale_supervisor);
        rejects(&stale_supervisor);

        let mut stale_instance_actor = original.clone();
        let mut plans = stale_instance_actor.domain_plans.to_vec();
        let instance = plans
            .iter_mut()
            .find(|plan| plan.kind == DomainPlanKind::Facility(FacilityKind::Input))
            .unwrap()
            .facility_instance
            .as_mut()
            .unwrap();
        instance.supervisor.current_meaning ^= 1;
        stale_instance_actor.domain_plans = plans.into();
        resign(&mut stale_instance_actor);
        rejects(&stale_instance_actor);
    }

    #[test]
    fn selected_facility_profile_incompatibility_is_admission_while_malformed_artifacts_are_defects()
     {
        let original = facility_fixture();

        let mut capability = original.clone();
        Arc::make_mut(&mut capability.architecture_contract)
            .corrupt_remove_capability(VmAbiCapability::PciVirtioModern);
        assert!(matches!(
            ImagePlanningModule.plan(
                Arc::clone(&capability.semantic_program),
                Arc::clone(&capability.architecture_contract),
                &Cancellation::new(),
            ),
            Err(PlanningFailure::FacilityCompatibility {
                kind: FacilityKind::Display,
                missing: FacilityCompatibilityMissing::Capability(
                    PlanningCapability::PciVirtioModern
                ),
                ..
            })
        ));
        rejects(&capability);

        let mut binding = original.clone();
        Arc::make_mut(&mut binding.architecture_contract)
            .corrupt_remove_binding(BindingKind::Display);
        assert!(matches!(
            ImagePlanningModule.plan(
                Arc::clone(&binding.semantic_program),
                Arc::clone(&binding.architecture_contract),
                &Cancellation::new(),
            ),
            Err(PlanningFailure::FacilityCompatibility {
                kind: FacilityKind::Display,
                missing: FacilityCompatibilityMissing::Binding(PlanningBinding::Display),
                ..
            })
        ));
        rejects(&binding);

        let mut sharing = original.clone();
        Arc::make_mut(&mut sharing.architecture_contract)
            .corrupt_facility_share(FacilitySharedRoleKind::EntropyQueue, Some(8));
        assert!(matches!(
            ImagePlanningModule.plan(
                Arc::clone(&sharing.semantic_program),
                Arc::clone(&sharing.architecture_contract),
                &Cancellation::new(),
            ),
            Err(PlanningFailure::FacilityCompatibility {
                kind: FacilityKind::Entropy,
                missing: FacilityCompatibilityMissing::SharedRole {
                    role: FacilitySharedRole::EntropyQueue,
                    required_units: 16,
                    available_units: 8,
                },
                ..
            })
        ));
        rejects(&sharing);
    }

    #[test]
    fn facility_cardinality_and_wide_requirements_follow_the_authenticated_contract() {
        let original = facility_fixture();
        let semantic = original.semantic_program.for_image_planning();
        let architecture = original.architecture_contract.for_image_planning();
        let mut instances = discover_facility_instances(semantic, &Cancellation::new()).unwrap();
        let display = instances
            .iter()
            .find(|instance| instance.kind == FacilityKind::Display)
            .unwrap()
            .clone();
        let mut second = display.clone();
        second.identity ^= 0x4000;
        second.current_meaning ^= 0x8000;
        second.source = SourceRange::new("src/alternate.wr", 10, 20);
        instances.push(second);
        instances.sort_by_key(|instance| instance.identity);

        let producer = producer_facility_contract_with_cardinality(
            FacilityKind::Display,
            semantic.distribution_digest(),
            0,
            2,
        );
        let verifier =
            verifier_facility_contract(FacilityKind::Display, semantic.distribution_digest(), 0, 2);
        assert_eq!(producer, verifier);
        validate_facility_cardinality(
            &instances,
            std::slice::from_ref(&producer),
            &Cancellation::new(),
        )
        .expect("two instances fit a contract-authenticated maximum of two");
        let singleton =
            producer_facility_contract(FacilityKind::Display, semantic.distribution_digest());
        assert!(matches!(
            validate_facility_cardinality(
                &instances,
                std::slice::from_ref(&singleton),
                &Cancellation::new(),
            ),
            Err(PlanningFailure::FacilityCardinality {
                selected: 2,
                minimum: 0,
                maximum: 1,
                ..
            })
        ));

        let planner = produce_facility_planner(
            original.context,
            display.clone(),
            &producer,
            architecture.fingerprint(),
        );
        let plan = produce_facility_domain_plan(
            original.context,
            display.clone(),
            planner.reference,
            &producer,
        );
        let roles = produce_facility_roles(
            original.context,
            planner.reference,
            plan.reference,
            &producer,
            architecture.fingerprint(),
            &Cancellation::new(),
        )
        .unwrap();
        let produced = produce_facility_requirements(
            original.context,
            planner.reference,
            plan.reference,
            &display,
            &roles,
            &producer,
            &Cancellation::new(),
        )
        .unwrap();
        let verified = verify_facility_requirements(
            original.context,
            planner.reference,
            plan.reference,
            &display,
            &roles,
            &verifier,
            &Cancellation::new(),
        )
        .unwrap();
        assert_eq!(produced, verified);
        assert!(produced.iter().any(|requirement| {
            matches!(
                requirement.bounds,
                RequirementBounds::Cardinality {
                    minimum: 0,
                    maximum: 2,
                }
            )
        }));
    }

    #[test]
    fn independent_facility_requirement_verifier_rejects_correlated_producer_drift() {
        let original = facility_fixture();
        let display_requirement = |requirement: &Requirement| {
            matches!(
                requirement.subject,
                RequirementOwner::Facility(FacilitySubjectRef {
                    kind: FacilityKind::Display,
                    ..
                })
            )
        };

        let mut omission = original.clone();
        let omitted = omission
            .requirements
            .iter()
            .position(|requirement| {
                display_requirement(requirement)
                    && requirement.category == RequirementCategory::BootAvailability
            })
            .expect("Display boot-availability requirement");
        let mut requirements = omission.requirements.to_vec();
        requirements.remove(omitted);
        omission.requirements = requirements.into();
        resign_facility_requirement_set(&mut omission);
        rejects(&omission);

        let mut wrong_binding = original.clone();
        let mut requirements = wrong_binding.requirements.to_vec();
        let requirement = requirements
            .iter_mut()
            .find(|requirement| {
                display_requirement(requirement)
                    && requirement.category == RequirementCategory::Binding
            })
            .expect("Display binding requirement");
        requirement.bounds = RequirementBounds::Binding {
            kind: PlanningBinding::Input,
            minimum: 1,
            maximum: 1,
        };
        wrong_binding.requirements = requirements.into();
        resign_facility_requirement_set(&mut wrong_binding);
        rejects(&wrong_binding);

        let mut wrong_cardinality = original.clone();
        let mut requirements = wrong_cardinality.requirements.to_vec();
        let requirement = requirements
            .iter_mut()
            .find(|requirement| {
                display_requirement(requirement)
                    && requirement.category == RequirementCategory::Cardinality
            })
            .expect("Display cardinality requirement");
        requirement.bounds = RequirementBounds::Cardinality {
            minimum: 0,
            maximum: 2,
        };
        wrong_cardinality.requirements = requirements.into();
        resign_facility_requirement_set(&mut wrong_cardinality);
        rejects(&wrong_cardinality);

        let mut wrong_sharing = original.clone();
        let mut requirements = wrong_sharing.requirements.to_vec();
        let requirement = requirements
            .iter_mut()
            .find(|requirement| {
                display_requirement(requirement)
                    && matches!(requirement.bounds, RequirementBounds::FacilitySharing(_))
            })
            .expect("Display sharing requirement");
        requirement.bounds =
            RequirementBounds::FacilitySharing(FacilitySharing::RegisteredDisjoint {
                role: FacilitySharedRole::EntropyQueue,
                maximum_units: 16,
            });
        wrong_sharing.requirements = requirements.into();
        resign_facility_requirement_set(&mut wrong_sharing);
        rejects(&wrong_sharing);

        let mut wrong_capacity = original.clone();
        let mut requirements = wrong_capacity.requirements.to_vec();
        let requirement = requirements
            .iter_mut()
            .find(|requirement| {
                display_requirement(requirement)
                    && requirement.category == RequirementCategory::CapacityPressure
            })
            .expect("Display capacity requirement");
        requirement.bounds =
            RequirementBounds::FacilityCapacity(FacilitySemanticCapacity::FrameBuffers(4));
        wrong_capacity.requirements = requirements.into();
        resign_facility_requirement_set(&mut wrong_capacity);
        rejects(&wrong_capacity);
    }

    #[test]
    fn facility_wide_requirement_identity_is_independent_of_generated_role_order() {
        let original = facility_fixture();
        let semantic = original.semantic_program.for_image_planning();
        let architecture = original.architecture_contract.for_image_planning();
        let instance = discover_facility_instances(semantic, &Cancellation::new())
            .unwrap()
            .into_iter()
            .find(|instance| instance.kind == FacilityKind::Display)
            .unwrap();
        let contract = original
            .facility_contracts
            .iter()
            .find(|contract| contract.observation.kind == FacilityKind::Display)
            .unwrap();
        let planner = produce_facility_planner(
            original.context,
            instance.clone(),
            contract,
            architecture.fingerprint(),
        );
        let plan = produce_facility_domain_plan(
            original.context,
            instance.clone(),
            planner.reference,
            contract,
        );
        let baseline_roles = produce_facility_roles(
            original.context,
            planner.reference,
            plan.reference,
            contract,
            architecture.fingerprint(),
            &Cancellation::new(),
        )
        .unwrap();
        let baseline = produce_facility_requirements(
            original.context,
            planner.reference,
            plan.reference,
            &instance,
            &baseline_roles,
            contract,
            &Cancellation::new(),
        )
        .unwrap();

        let mut reordered_contract = contract.clone();
        reordered_contract.observation.generated_roles = Arc::from([
            GeneratedRoleKind::TelemetryDriver,
            GeneratedRoleKind::DisplayDriver,
        ]);
        let reordered_roles = produce_facility_roles(
            original.context,
            planner.reference,
            plan.reference,
            &reordered_contract,
            architecture.fingerprint(),
            &Cancellation::new(),
        )
        .unwrap();
        let reordered = produce_facility_requirements(
            original.context,
            planner.reference,
            plan.reference,
            &instance,
            &reordered_roles,
            &reordered_contract,
            &Cancellation::new(),
        )
        .unwrap();

        let wide = |requirements: Vec<Requirement>| {
            requirements
                .into_iter()
                .filter(|requirement| matches!(requirement.subject, RequirementOwner::Facility(_)))
                .collect::<Vec<_>>()
        };
        assert_eq!(wide(baseline), wide(reordered));
    }

    #[test]
    fn verifier_rejects_correlated_duplicate_and_over_capacity_shared_roles() {
        let original = facility_fixture();

        let mut duplicate = original.clone();
        let mut requirements = duplicate.requirements.to_vec();
        let display_sharing = requirements
            .iter_mut()
            .find(|requirement| {
                matches!(
                    requirement.subject,
                    RequirementOwner::Facility(FacilitySubjectRef {
                        kind: FacilityKind::Display,
                        ..
                    })
                ) && matches!(requirement.bounds, RequirementBounds::FacilitySharing(_))
            })
            .unwrap();
        display_sharing.bounds =
            RequirementBounds::FacilitySharing(FacilitySharing::RegisteredDisjoint {
                role: FacilitySharedRole::EntropyQueue,
                maximum_units: 16,
            });
        duplicate.requirements = requirements.into();
        resign_facility_requirement_set(&mut duplicate);
        rejects(&duplicate);

        let mut over_capacity = original.clone();
        let mut requirements = over_capacity.requirements.to_vec();
        let entropy_sharing = requirements
            .iter_mut()
            .find(|requirement| {
                matches!(
                    requirement.subject,
                    RequirementOwner::Facility(FacilitySubjectRef {
                        kind: FacilityKind::Entropy,
                        ..
                    })
                ) && matches!(requirement.bounds, RequirementBounds::FacilitySharing(_))
            })
            .unwrap();
        entropy_sharing.bounds =
            RequirementBounds::FacilitySharing(FacilitySharing::RegisteredDisjoint {
                role: FacilitySharedRole::EntropyQueue,
                maximum_units: 17,
            });
        over_capacity.requirements = requirements.into();
        resign_facility_requirement_set(&mut over_capacity);
        rejects(&over_capacity);
    }

    #[test]
    fn verifier_rejects_generated_role_corruption() {
        let original = fixture(Root::Image);

        let mut missing = original.clone();
        missing.generated_roles = original.generated_roles[..4].into();
        rejects(&missing);

        let mut extra = original.clone();
        let mut roles = original.generated_roles.to_vec();
        roles.push(original.generated_roles[4].clone());
        extra.generated_roles = roles.into();
        rejects(&extra);

        let mut dangling = original.clone();
        let mut roles = dangling.generated_roles.to_vec();
        let mut scheduler = roles[1].clone();
        let mut dependency = scheduler.dependencies[0];
        dependency.identity ^= 1;
        scheduler.dependencies = Arc::from([dependency]);
        roles[1] = scheduler;
        dangling.generated_roles = roles.into();
        rejects(&dangling);

        let mut wrong_owner = original.clone();
        let mut roles = wrong_owner.generated_roles.to_vec();
        roles[2].owner.identity ^= 1;
        wrong_owner.generated_roles = roles.into();
        rejects(&wrong_owner);

        let mut wrong_role = original.clone();
        let mut roles = wrong_role.generated_roles.to_vec();
        roles[2].kind = GeneratedRoleKind::Panic;
        wrong_role.generated_roles = roles.into();
        rejects(&wrong_role);

        let mut wrong_generator = original.clone();
        let mut roles = wrong_generator.generated_roles.to_vec();
        roles[3].generator.identity ^= 1;
        wrong_generator.generated_roles = roles.into();
        rejects(&wrong_generator);

        let mut wrong_provenance = original.clone();
        let mut roles = wrong_provenance.generated_roles.to_vec();
        roles[3].provenance.identity ^= 1;
        wrong_provenance.generated_roles = roles.into();
        rejects(&wrong_provenance);

        let mut stale = original.clone();
        let mut roles = stale.generated_roles.to_vec();
        roles[3].reference.current_meaning ^= 1;
        stale.generated_roles = roles.into();
        rejects(&stale);

        let mut cycle = original.clone();
        let mut roles = cycle.generated_roles.to_vec();
        roles[1].dependencies = Arc::from([roles[1].reference]);
        cycle.generated_roles = roles.into();
        rejects(&cycle);

        let mut noncanonical_dependencies = original.clone();
        let mut roles = noncanonical_dependencies.generated_roles.to_vec();
        let position = roles
            .iter()
            .position(|role| role.dependencies.len() > 1)
            .expect("fixture has a multi-dependency role");
        let mut role = roles[position].clone();
        let mut dependencies = role.dependencies.to_vec();
        dependencies.reverse();
        role.dependencies = dependencies.into();
        roles[position] = role;
        noncanonical_dependencies.generated_roles = roles.into();
        rejects(&noncanonical_dependencies);

        let mut mixed_context = original.clone();
        let mut roles = mixed_context.generated_roles.to_vec();
        roles[1].executable.context ^= 1;
        mixed_context.generated_roles = roles.into();
        rejects(&mixed_context);
    }

    #[test]
    fn verifier_rejects_requirement_corruption() {
        let original = fixture(Root::Image);

        let mut missing = original.clone();
        missing.requirements = original.requirements[1..].into();
        rejects(&missing);

        let mut duplicate = original.clone();
        let mut requirements = original.requirements.to_vec();
        requirements.push(original.requirements[0].clone());
        duplicate.requirements = requirements.into();
        rejects(&duplicate);

        let mut dangling = original.clone();
        let mut requirements = dangling.requirements.to_vec();
        if let RequirementOwner::GeneratedRole(reference) = &mut requirements[0].subject {
            reference.identity ^= 1;
        }
        dangling.requirements = requirements.into();
        rejects(&dangling);

        let mut wrong_owner = original.clone();
        let mut requirements = wrong_owner.requirements.to_vec();
        requirements[0].owner.identity ^= 1;
        wrong_owner.requirements = requirements.into();
        rejects(&wrong_owner);

        let mut wrong_provenance = original.clone();
        let mut requirements = wrong_provenance.requirements.to_vec();
        requirements[0].provenance.domain_plan ^= 1;
        wrong_provenance.requirements = requirements.into();
        rejects(&wrong_provenance);

        let mut wrong_category = original.clone();
        let mut requirements = wrong_category.requirements.to_vec();
        requirements[0].category = RequirementCategory::Service;
        wrong_category.requirements = requirements.into();
        rejects(&wrong_category);

        let mut invalid_bounds = original.clone();
        let mut requirements = invalid_bounds.requirements.to_vec();
        requirements[0].bounds = RequirementBounds::Cardinality {
            minimum: 2,
            maximum: 1,
        };
        invalid_bounds.requirements = requirements.into();
        rejects(&invalid_bounds);

        let mut stale = original.clone();
        let mut requirements = stale.requirements.to_vec();
        requirements[0].reference.current_meaning ^= 1;
        stale.requirements = requirements.into();
        rejects(&stale);

        let mut mixed_context = original.clone();
        let mut requirements = mixed_context.requirements.to_vec();
        requirements[0].reference.context ^= 1;
        mixed_context.requirements = requirements.into();
        rejects(&mixed_context);
    }

    #[test]
    fn verifier_rejects_exact_executable_demand_corruption() {
        let original = fixture(Root::Image);

        let mut missing = original.clone();
        missing.executable_demand.additions = original.executable_demand.additions[1..].into();
        rejects(&missing);

        let mut duplicate = original.clone();
        let mut additions = original.executable_demand.additions.to_vec();
        additions.push(original.executable_demand.additions[0]);
        duplicate.executable_demand.additions = additions.into();
        rejects(&duplicate);

        let mut wrong_role = original.clone();
        let mut additions = wrong_role.executable_demand.additions.to_vec();
        additions[0].identity ^= 1;
        wrong_role.executable_demand.additions = additions.into();
        rejects(&wrong_role);

        let mut mixed_context = original.clone();
        mixed_context.executable_demand.source.context ^= 1;
        rejects(&mixed_context);
    }

    #[test]
    fn bounded_cancellation_publishes_no_foundation() {
        let complete = fixture(Root::Image);
        let cancellation = Cancellation::new();
        cancellation.cancel_after_private_polls(3);
        assert!(matches!(
            ImagePlanningModule.plan(
                Arc::clone(&complete.semantic_program),
                Arc::clone(&complete.architecture_contract),
                &cancellation,
            ),
            Err(PlanningFailure::Cancelled)
        ));
    }

    #[test]
    fn large_pool_traversal_cancellation_publishes_no_foundation_or_state() {
        let mut source = String::from(
            "from core import pool as pools\n\n@image\nfn build() -> Image:\n    mut value = 0\n    with pools.scoped(capacity=1) as scratch:\n",
        );
        for ordinal in 0..128 {
            source.push_str(&format!(
                "        allocation_{ordinal} = scratch.allocate(value={ordinal})\n        value = scratch.reclaim(allocation=take allocation_{ordinal})\n"
            ));
        }
        source.push_str("    return Image.new(value=value)\n");

        let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
        let outcome = compiler.compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source.into_bytes())]),
                Root::Image,
            ),
            &Cancellation::new(),
        );
        let CompilationOutcome::Accepted(accepted) = outcome else {
            panic!("large Pool semantic fixture accepts: {outcome:#?}");
        };
        let semantic_program = Arc::new(accepted.completed_semantic_program().clone());
        let digest = semantic_program.for_image_planning().distribution_digest();
        let contract = Arc::new(
            ArchitecturePlanningModule::new(ContractContext::new(
                "large-pool-cancellation-test",
                digest,
            ))
            .authenticate(ArchitectureProfile::CurrentAarch64, &Cancellation::new())
            .expect("private contract authenticates"),
        );

        let cancellation = Cancellation::new();
        cancellation.cancel_after_private_polls(50);
        assert!(matches!(
            ImagePlanningModule.plan(
                Arc::clone(&semantic_program),
                Arc::clone(&contract),
                &cancellation,
            ),
            Err(PlanningFailure::Cancelled)
        ));
        assert!(
            ImagePlanningModule
                .plan(semantic_program, contract, &Cancellation::new())
                .is_ok()
        );
    }

    #[test]
    fn core_input_view_is_backed_by_exact_same_context_direct_references() {
        let foundation = fixture(Root::Test);
        let core = foundation.for_core();

        assert_eq!(core.context_identity(), foundation.context);
        assert_eq!(
            core.completed_semantic_program().context_identity(),
            foundation
                .semantic_program
                .for_image_planning()
                .context_identity()
        );
        assert_eq!(
            core.completed_semantic_program().fingerprint(),
            foundation.semantic_program.fingerprint()
        );
        assert_eq!(
            core.source_executable_demand(),
            foundation.executable_demand.source
        );
        assert_eq!(core.domain_plans(), foundation.domain_plans.as_ref());
        assert_eq!(core.generated_roles(), foundation.generated_roles.as_ref());
        assert_eq!(core.requirements(), foundation.requirements.as_ref());
        assert_eq!(
            core.generated_executable_additions(),
            foundation.executable_demand.additions.as_ref()
        );
        assert!(core.domain_plans().iter().all(|plan| {
            plan.reference.context == core.context_identity()
                && plan
                    .generated_roles
                    .iter()
                    .all(|reference| reference.context == core.context_identity())
                && plan
                    .requirements
                    .iter()
                    .all(|reference| reference.context == core.context_identity())
        }));
        for role in core.generated_roles() {
            assert_eq!(role.reference(), role.reference);
            assert_eq!(role.executable(), role.executable);
            assert_eq!(role.owner(), role.owner);
            assert_eq!(role.generator(), role.generator);
            assert_eq!(role.kind(), role.kind);
            assert_eq!(role.local_key(), role.local_key);
            assert_eq!(role.dependencies(), role.dependencies.as_ref());
            assert_eq!(role.provenance(), role.provenance);
            assert_eq!(role.reference().context(), core.context_identity());
            assert_eq!(role.executable().context(), core.context_identity());
            assert_eq!(role.owner().context(), core.context_identity());
            assert_eq!(role.generator().context(), core.context_identity());
            assert_eq!(role.provenance().context(), core.context_identity());
            assert!(
                role.dependencies()
                    .iter()
                    .all(|dependency| dependency.context() == core.context_identity())
            );
        }
        for requirement in core.requirements() {
            assert_eq!(requirement.reference(), requirement.reference);
            assert_eq!(requirement.owner(), requirement.owner);
            assert_eq!(requirement.subject(), requirement.subject);
            assert_eq!(requirement.provenance(), requirement.provenance);
            assert_eq!(requirement.category(), requirement.category);
            assert_eq!(requirement.bounds(), &requirement.bounds);
            assert_eq!(requirement.reference().context(), core.context_identity());
            assert_eq!(requirement.owner().context(), core.context_identity());
            assert_eq!(requirement.subject().context(), core.context_identity());
        }
    }

    #[test]
    fn core_input_exposes_exact_completed_semantic_source_demand() {
        use crate::completed_semantic::CoreSourceExecutableKind;

        let test_foundation = fixture(Root::Test);
        let test_core = test_foundation.for_core();
        let test_sources = test_core.exact_source_executables().collect::<Vec<_>>();
        assert_eq!(
            test_sources.len(),
            test_foundation.executable_demand.source_count
        );
        assert!(
            test_sources
                .iter()
                .all(|reference| reference.context() == test_core.context_identity())
        );
        assert!(
            test_sources
                .iter()
                .all(|reference| reference.identity() != 0 && reference.current_meaning() != 0)
        );
        assert!(
            test_sources
                .iter()
                .any(|reference| reference.kind() == CoreSourceExecutableKind::Specialization)
        );
        assert!(
            test_sources
                .iter()
                .any(|reference| reference.kind() == CoreSourceExecutableKind::TestBody)
        );

        let closure_foundation = fixture_from_source(
            "src/image.wr",
            br#"@image
fn build() -> Image:
    offset = 2
    callback = |value: i64| value + offset
    return Image.new(callback=callback)
"#,
            Root::Image,
        );
        let closure_core = closure_foundation.for_core();
        let closure_sources = closure_core.exact_source_executables().collect::<Vec<_>>();
        assert_eq!(
            closure_sources.len(),
            closure_foundation.executable_demand.source_count
        );
        assert!(
            closure_sources
                .iter()
                .all(|reference| reference.context() == closure_core.context_identity())
        );
        assert!(
            closure_sources
                .iter()
                .all(|reference| reference.identity() != 0 && reference.current_meaning() != 0)
        );
        assert!(
            closure_sources
                .iter()
                .any(|reference| reference.kind() == CoreSourceExecutableKind::Specialization)
        );
        assert!(
            closure_sources
                .iter()
                .any(|reference| reference.kind() == CoreSourceExecutableKind::ClosureBody)
        );
    }

    #[test]
    fn named_tiny_puzzles_match_the_independent_brute_force_checker() {
        let puzzles = named_tiny_solver_puzzles();
        assert_eq!(
            puzzles
                .iter()
                .map(|puzzle| puzzle.name)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "exact_fit",
                "one_short",
                "binding",
                "cardinality",
                "affinity",
                "separation",
                "sharing",
                "role_closure",
                "lifetime",
                "required_capability",
            ])
        );
        for puzzle in puzzles {
            let production = solve_canonical_problem(&puzzle, &Cancellation::new())
                .expect("bounded production puzzle search completes");
            let independent = independently_enumerate_tiny_puzzle(&puzzle);
            assert_eq!(
                production, independent,
                "production and independent checker disagree for {}",
                puzzle.name
            );

            let mut permuted = puzzle.clone();
            permuted.executables.reverse();
            permuted.cores.reverse();
            permuted.bindings.reverse();
            permuted.requirements.reverse();
            assert_eq!(
                production,
                solve_canonical_problem(&permuted, &Cancellation::new())
                    .expect("permuted bounded production puzzle search completes"),
                "enumeration order changed the canonical result for {}",
                puzzle.name
            );
        }
    }

    #[test]
    fn assignment_verifier_rejects_invariant_disagreement_without_researching() {
        let mut candidate = assignment_fixture();
        let mut placements = candidate.placements.to_vec();
        placements[0].core = 1;
        candidate.placements = placements.into();
        candidate.fingerprint = whole_assignment_fingerprint(
            candidate.requirement_set_fingerprint,
            &candidate.placements,
            &candidate.bindings,
            &candidate.discharges,
        );
        assert!(matches!(
            verify_whole_image_assignment(&candidate, &Cancellation::new()),
            Err(PlanningFailure::Defect(_))
        ));

        let mut candidate = assignment_fixture();
        let mut requirements = candidate.requirements.to_vec();
        let mut discharges = candidate.discharges.to_vec();
        requirements.pop().expect("fixture has a Requirement");
        discharges.pop().expect("fixture has a discharge");
        candidate.requirements = requirements.into();
        candidate.discharges = discharges.into();
        candidate.requirement_set_fingerprint = whole_requirement_set_fingerprint(
            candidate.planning_foundation.fingerprint,
            candidate.core_program.for_image_planning().fingerprint(),
            candidate.flow_program.for_image_planning().fingerprint(),
            &candidate.requirements,
        );
        candidate.fingerprint = whole_assignment_fingerprint(
            candidate.requirement_set_fingerprint,
            &candidate.placements,
            &candidate.bindings,
            &candidate.discharges,
        );
        assert!(matches!(
            verify_whole_image_assignment(&candidate, &Cancellation::new()),
            Err(PlanningFailure::Defect(_))
        ));

        let mut candidate = assignment_fixture();
        let mut discharges = candidate.discharges.to_vec();
        discharges[0].kind = if discharges[0].kind == DischargeKind::Bound {
            DischargeKind::Placed
        } else {
            DischargeKind::Bound
        };
        candidate.discharges = discharges.into();
        candidate.fingerprint = whole_assignment_fingerprint(
            candidate.requirement_set_fingerprint,
            &candidate.placements,
            &candidate.bindings,
            &candidate.discharges,
        );
        assert!(matches!(
            verify_whole_image_assignment(&candidate, &Cancellation::new()),
            Err(PlanningFailure::Defect(_))
        ));

        let mut candidate = assignment_fixture();
        let mut bindings = candidate.bindings.to_vec();
        bindings[0].slot ^= 1;
        candidate.bindings = bindings.into();
        candidate.fingerprint = whole_assignment_fingerprint(
            candidate.requirement_set_fingerprint,
            &candidate.placements,
            &candidate.bindings,
            &candidate.discharges,
        );
        assert!(matches!(
            verify_whole_image_assignment(&candidate, &Cancellation::new()),
            Err(PlanningFailure::Defect(_))
        ));

        let mut candidate = assignment_fixture();
        let mut requirements = candidate.requirements.to_vec();
        let WholeRequirementRef::Domain(reference) = requirements
            .iter_mut()
            .find(|requirement| matches!(requirement, WholeRequirementRef::Domain(_)))
            .expect("fixture has a Domain Requirement")
        else {
            unreachable!()
        };
        reference.current_meaning ^= 1;
        candidate.requirements = requirements.into();
        candidate.requirement_set_fingerprint = whole_requirement_set_fingerprint(
            candidate.planning_foundation.fingerprint,
            candidate.core_program.for_image_planning().fingerprint(),
            candidate.flow_program.for_image_planning().fingerprint(),
            &candidate.requirements,
        );
        candidate.fingerprint = whole_assignment_fingerprint(
            candidate.requirement_set_fingerprint,
            &candidate.placements,
            &candidate.bindings,
            &candidate.discharges,
        );
        assert!(matches!(
            verify_whole_image_assignment(&candidate, &Cancellation::new()),
            Err(PlanningFailure::Defect(_))
        ));

        for false_kind in [DischargeKind::CapacityProved, DischargeKind::LifetimeProved] {
            let mut candidate = assignment_fixture();
            let mut discharges = candidate.discharges.to_vec();
            let discharge = discharges
                .iter_mut()
                .find(|discharge| discharge.kind == false_kind)
                .expect("fixture covers capacity and lifetime evidence");
            discharge.kind = DischargeKind::ContractValidated;
            candidate.discharges = discharges.into();
            candidate.fingerprint = whole_assignment_fingerprint(
                candidate.requirement_set_fingerprint,
                &candidate.placements,
                &candidate.bindings,
                &candidate.discharges,
            );
            assert!(matches!(
                verify_whole_image_assignment(&candidate, &Cancellation::new()),
                Err(PlanningFailure::Defect(_))
            ));
        }

        let mut candidate = assignment_fixture();
        let mut foundation = (*candidate.planning_foundation).clone();
        let mut requirements = foundation.requirements.to_vec();
        let binding = requirements
            .iter_mut()
            .find(|requirement| matches!(requirement.bounds, RequirementBounds::Binding { .. }))
            .unwrap();
        let RequirementBounds::Binding { kind, .. } = binding.bounds else {
            unreachable!()
        };
        binding.bounds = RequirementBounds::Binding {
            kind,
            minimum: 2,
            maximum: 2,
        };
        foundation.requirements = requirements.into();
        candidate.planning_foundation = Arc::new(foundation);
        assert!(matches!(
            verify_whole_image_assignment(&candidate, &Cancellation::new()),
            Err(PlanningFailure::Defect(_))
        ));
    }

    #[test]
    fn solver_cancellation_and_exhaustion_publish_no_candidate_or_conflict() {
        let cancellation = Cancellation::new();
        cancellation.cancel();
        let exact_fit = named_tiny_solver_puzzles()
            .into_iter()
            .find(|puzzle| puzzle.name == "exact_fit")
            .unwrap();
        assert!(matches!(
            solve_canonical_problem(&exact_fit, &cancellation),
            Err(PlanningFailure::Cancelled)
        ));

        let exhausted = CanonicalProblem {
            name: "bounded_exhaustion",
            cores: vec![CoreResource { identity: 0 }],
            executables: vec![1],
            bindings: Vec::new(),
            binding_subjects: Vec::new(),
            capabilities: BTreeSet::new(),
            requirements: (0..=MAX_CONFLICT_REQUIREMENTS)
                .map(|ordinal| {
                    solver_requirement(
                        u128::try_from(ordinal + 1).unwrap(),
                        SolverRequirementCategory::RequiredCapability,
                        SolverConstraint::Capability {
                            capability: u8::try_from(ordinal + 1).unwrap(),
                        },
                    )
                })
                .collect(),
        };
        assert!(matches!(
            solve_canonical_problem(&exhausted, &Cancellation::new()),
            Err(PlanningFailure::Defect(evidence))
                if evidence.contains("exhausted")
        ));
    }

    #[test]
    fn canonical_solver_rejects_duplicate_typed_identities() {
        let duplicate_binding = CanonicalProblem {
            name: "duplicate_binding",
            cores: vec![CoreResource { identity: 0 }],
            executables: vec![1],
            bindings: vec![
                BindingResource {
                    identity: 1,
                    kind: 1,
                    shareable: false,
                },
                BindingResource {
                    identity: 1,
                    kind: 2,
                    shareable: false,
                },
            ],
            binding_subjects: Vec::new(),
            capabilities: BTreeSet::new(),
            requirements: Vec::new(),
        };
        assert!(matches!(
            solve_canonical_problem(&duplicate_binding, &Cancellation::new()),
            Err(PlanningFailure::Defect(_))
        ));

        let duplicate_requirement = CanonicalProblem {
            name: "duplicate_requirement",
            cores: vec![CoreResource { identity: 0 }],
            executables: vec![1],
            bindings: Vec::new(),
            binding_subjects: Vec::new(),
            capabilities: BTreeSet::from([1]),
            requirements: vec![
                solver_requirement(
                    1,
                    SolverRequirementCategory::RoleRealization,
                    SolverConstraint::Realize { executable: 1 },
                ),
                solver_requirement(
                    1,
                    SolverRequirementCategory::RequiredCapability,
                    SolverConstraint::Capability { capability: 1 },
                ),
            ],
        };
        assert!(matches!(
            solve_canonical_problem(&duplicate_requirement, &Cancellation::new()),
            Err(PlanningFailure::Defect(_))
        ));

        let exact_two = CanonicalProblem {
            name: "two_exact_bindings",
            cores: vec![CoreResource { identity: 0 }],
            executables: vec![1],
            bindings: vec![
                BindingResource {
                    identity: 0,
                    kind: 1,
                    shareable: false,
                },
                BindingResource {
                    identity: 1,
                    kind: 1,
                    shareable: false,
                },
            ],
            binding_subjects: vec![10],
            capabilities: BTreeSet::new(),
            requirements: vec![solver_requirement(
                1,
                SolverRequirementCategory::Binding,
                SolverConstraint::Binding {
                    subject: 10,
                    kind: 1,
                    minimum: 2,
                    maximum: 2,
                    allow_sharing: false,
                },
            )],
        };
        let CanonicalSolveOutcome::Assignment { bindings, .. } =
            solve_canonical_problem(&exact_two, &Cancellation::new()).unwrap()
        else {
            panic!("two exact slots are feasible");
        };
        assert_eq!(bindings.as_ref(), [(10, 0), (10, 1)]);
    }

    #[test]
    fn production_translation_uses_the_exact_typed_requirement_set_and_architecture_resources() {
        let candidate = assignment_fixture();
        let problem = translate_whole_requirement_set(
            &candidate.planning_foundation,
            candidate.core_program.for_image_planning(),
            candidate.flow_program.for_image_planning(),
            &Cancellation::new(),
        )
        .expect("verified authorities translate");

        assert_eq!(problem.requirements.len(), candidate.requirements.len());
        assert_eq!(
            problem
                .requirements
                .iter()
                .map(|requirement| requirement.identity)
                .collect::<BTreeSet<_>>(),
            candidate
                .requirements
                .iter()
                .map(|requirement| requirement.identity())
                .collect()
        );
        assert!(!problem.bindings.is_empty());
        assert!(!problem.capabilities.is_empty());
        assert!(
            problem.requirements.iter().any(|requirement| matches!(
                requirement.constraint,
                SolverConstraint::Realize { .. }
            ))
        );
        assert!(
            problem.requirements.iter().any(|requirement| matches!(
                requirement.constraint,
                SolverConstraint::Binding { .. }
            ))
        );
        assert!(problem.requirements.iter().any(|requirement| matches!(
            requirement.constraint,
            SolverConstraint::Cardinality { .. }
        )));
        assert!(problem.requirements.iter().any(|requirement| matches!(
            requirement.constraint,
            SolverConstraint::Activation { .. }
        )));
    }

    #[test]
    fn canonical_solver_rejects_dangling_and_inverted_constraints_as_defects() {
        let malformed = [
            SolverConstraint::AllowedCores {
                executable: 99,
                cores: Arc::from([0]),
            },
            SolverConstraint::CoreCapacity {
                core: 99,
                maximum: 1,
            },
            SolverConstraint::Affinity { left: 1, right: 99 },
            SolverConstraint::Binding {
                subject: 1,
                kind: 1,
                minimum: 2,
                maximum: 1,
                allow_sharing: false,
            },
            SolverConstraint::Binding {
                subject: 99,
                kind: 1,
                minimum: 1,
                maximum: 1,
                allow_sharing: false,
            },
            SolverConstraint::Activation {
                executable: 99,
                units: 1,
                start: 1,
                end: 1,
            },
        ];
        for (ordinal, constraint) in malformed.into_iter().enumerate() {
            let problem = CanonicalProblem {
                name: "malformed_typed_subject",
                cores: vec![CoreResource { identity: 0 }],
                executables: vec![1],
                bindings: vec![BindingResource {
                    identity: 0,
                    kind: 1,
                    shareable: false,
                }],
                binding_subjects: vec![1],
                capabilities: BTreeSet::new(),
                requirements: vec![solver_requirement(
                    u128::try_from(ordinal + 1).unwrap(),
                    SolverRequirementCategory::Placement,
                    constraint,
                )],
            };
            assert!(matches!(
                solve_canonical_problem(&problem, &Cancellation::new()),
                Err(PlanningFailure::Defect(_))
            ));
        }
    }

    #[test]
    fn production_solve_retains_a_verified_binding_conflict_instead_of_a_defect() {
        let candidate = assignment_fixture();
        let mut foundation = (*candidate.planning_foundation).clone();
        let mut requirements = foundation.requirements.to_vec();
        let binding = requirements
            .iter_mut()
            .find(|requirement| {
                matches!(
                    requirement.bounds,
                    RequirementBounds::Binding {
                        kind: PlanningBinding::Terminal,
                        ..
                    }
                )
            })
            .expect("fixture has the Terminal binding Requirement");
        let conflict_requirement = binding.reference.identity;
        binding.bounds = RequirementBounds::Binding {
            kind: PlanningBinding::Terminal,
            minimum: 2,
            maximum: 2,
        };
        foundation.requirements = requirements.into();

        let outcome = ImagePlanningModule
            .solve(
                Arc::new(foundation),
                Arc::clone(&candidate.core_program),
                Arc::clone(&candidate.flow_program),
                &Cancellation::new(),
            )
            .expect("valid infeasibility is a solve outcome");
        let WholeImageSolveOutcome::Conflict(conflict) = outcome else {
            panic!("one Terminal slot cannot satisfy two exact bindings");
        };
        assert_eq!(conflict.code, ConflictCode::Binding);
        assert_eq!(conflict.requirements.as_ref(), [conflict_requirement]);
    }

    #[test]
    fn canonical_solver_handles_an_authenticated_large_closure_without_host_recursion() {
        let problem = CanonicalProblem {
            name: "maximum_generated_closure",
            cores: vec![CoreResource { identity: 0 }],
            executables: (1..=16_384).collect(),
            bindings: Vec::new(),
            binding_subjects: Vec::new(),
            capabilities: BTreeSet::new(),
            requirements: Vec::new(),
        };
        let CanonicalSolveOutcome::Assignment { placements, .. } =
            solve_canonical_problem(&problem, &Cancellation::new()).expect("bounded assignment")
        else {
            panic!("one core admits the unconstrained finite closure");
        };
        assert_eq!(placements.len(), 16_384);

        let cancellation = Cancellation::new();
        cancellation.cancel();
        assert!(matches!(
            solve_canonical_problem(&problem, &cancellation),
            Err(PlanningFailure::Cancelled)
        ));
    }

    #[test]
    fn private_conflict_verifier_rejects_wrong_code_and_noncanonical_irreducible_witness() {
        let problem = CanonicalProblem {
            name: "two_independent_missing_capabilities",
            cores: vec![CoreResource { identity: 0 }],
            executables: vec![1],
            bindings: Vec::new(),
            binding_subjects: Vec::new(),
            capabilities: BTreeSet::new(),
            requirements: vec![
                solver_requirement(
                    1,
                    SolverRequirementCategory::RequiredCapability,
                    SolverConstraint::Capability { capability: 1 },
                ),
                solver_requirement(
                    2,
                    SolverRequirementCategory::RequiredCapability,
                    SolverConstraint::Capability { capability: 2 },
                ),
            ],
        };
        let canonical = canonicalize_problem(&problem).unwrap();
        let wrong_code = VerifiedPrivateConflict {
            code: ConflictCode::Binding,
            requirements: Arc::from([1]),
        };
        assert!(matches!(
            verify_private_conflict(&canonical, &wrong_code, &Cancellation::new()),
            Err(PlanningFailure::Defect(_))
        ));
        let noncanonical = VerifiedPrivateConflict {
            code: ConflictCode::MissingCapability,
            requirements: Arc::from([2]),
        };
        assert!(matches!(
            verify_private_conflict(&canonical, &noncanonical, &Cancellation::new()),
            Err(PlanningFailure::Defect(_))
        ));
    }

    #[test]
    fn translated_production_constraints_change_placement_and_infeasibility() {
        let candidate = assignment_fixture();
        let translated = || {
            translate_whole_requirement_set(
                &candidate.planning_foundation,
                candidate.core_program.for_image_planning(),
                candidate.flow_program.for_image_planning(),
                &Cancellation::new(),
            )
            .unwrap()
        };
        let baseline = solve_canonical_problem(&translated(), &Cancellation::new()).unwrap();
        assert!(matches!(baseline, CanonicalSolveOutcome::Assignment { .. }));

        let mut placement = translated();
        let selected = placement
            .requirements
            .iter()
            .find_map(|requirement| match requirement.constraint {
                SolverConstraint::Activation { executable, .. } => Some(executable),
                _ => None,
            })
            .unwrap();
        let lifetime = placement
            .requirements
            .iter_mut()
            .find(|requirement| {
                matches!(
                    requirement.constraint,
                    SolverConstraint::Activation { executable, .. } if executable == selected
                )
            })
            .unwrap();
        lifetime.constraint = SolverConstraint::AllowedCores {
            executable: selected,
            cores: Arc::from([1]),
        };
        let CanonicalSolveOutcome::Assignment { placements, .. } =
            solve_canonical_problem(&placement, &Cancellation::new()).unwrap()
        else {
            panic!("authenticated secondary core is feasible");
        };
        assert_eq!(
            placements
                .iter()
                .find(|(executable, _)| *executable == selected)
                .unwrap()
                .1,
            1
        );

        let mut separation = translated();
        let left = separation.executables[0];
        let right = separation.executables[1];
        separation
            .requirements
            .iter_mut()
            .find(|requirement| matches!(requirement.constraint, SolverConstraint::Static { .. }))
            .unwrap()
            .constraint = SolverConstraint::Separation { left, right };
        let CanonicalSolveOutcome::Assignment { placements, .. } =
            solve_canonical_problem(&separation, &Cancellation::new()).unwrap()
        else {
            panic!("four cores can separate two executables");
        };
        assert_ne!(
            placements.iter().find(|(id, _)| *id == left).unwrap().1,
            placements.iter().find(|(id, _)| *id == right).unwrap().1
        );

        let mut capability = translated();
        let required_capability = capability
            .requirements
            .iter()
            .find_map(|requirement| match requirement.constraint {
                SolverConstraint::Capability { capability } => Some(capability),
                _ => None,
            })
            .unwrap();
        capability.capabilities.remove(&required_capability);
        assert!(matches!(
            solve_canonical_problem(&capability, &Cancellation::new()).unwrap(),
            CanonicalSolveOutcome::Conflict(VerifiedPrivateConflict {
                code: ConflictCode::MissingCapability,
                ..
            })
        ));

        let mut one_short = translated();
        let capacity = one_short
            .requirements
            .iter_mut()
            .find(|requirement| matches!(requirement.constraint, SolverConstraint::Capacity { required, available } if required == available && required > 0))
            .unwrap();
        let SolverConstraint::Capacity {
            required,
            available,
        } = &mut capacity.constraint
        else {
            unreachable!()
        };
        *available = required.saturating_sub(1);
        assert!(matches!(
            solve_canonical_problem(&one_short, &Cancellation::new()).unwrap(),
            CanonicalSolveOutcome::Conflict(VerifiedPrivateConflict {
                code: ConflictCode::Capacity,
                ..
            })
        ));
    }
}
