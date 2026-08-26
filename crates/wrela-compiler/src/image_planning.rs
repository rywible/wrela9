#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use xxhash_rust::xxh3::Xxh3;

use crate::architecture_planning::{
    BindingKind, ReservationKind, ReservationMultiplicity, VerifiedArchitecturePlanningContract,
    VmAbiCapability,
};
use crate::completed_semantic::{
    CompletedSemanticProgram, CorePlanningSemanticProgram, CoreSourceExecutableBody,
    CoreSourceExecutableRef,
};
use crate::typed_hir::{
    CallTarget, Expression, ExpressionKind, HirMatchCase, Literal, LocalId, PoolOperation,
    Statement, VerifiedProgram,
};
use crate::{Cancellation, Root, SourceRange};

pub(crate) const PHASE_SCHEMA: &str = "wrela.image-planning-foundation.v1";
const SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlannerKind {
    ImageKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DomainPlanKind {
    MandatoryImage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GeneratedRoleKind {
    Boot,
    Scheduler,
    Terminal,
    Panic,
    Shutdown,
    TestRuntime,
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
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlanningCapability {
    TypedTerminalLifecycle,
    PanicPulse,
    GuestShutdownPulse,
    SecondaryCoreStartup,
}

impl PlanningCapability {
    const fn tag(self) -> u8 {
        match self {
            Self::TypedTerminalLifecycle => 1,
            Self::PanicPulse => 2,
            Self::GuestShutdownPulse => 3,
            Self::SecondaryCoreStartup => 4,
        }
    }

    const fn contract_kind(self) -> VmAbiCapability {
        match self {
            Self::TypedTerminalLifecycle => VmAbiCapability::TypedTerminalLifecycle,
            Self::PanicPulse => VmAbiCapability::PanicPulse,
            Self::GuestShutdownPulse => VmAbiCapability::GuestShutdownPulse,
            Self::SecondaryCoreStartup => VmAbiCapability::SecondaryCoreStartup,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlanningBinding {
    Terminal,
    Panic,
}

impl PlanningBinding {
    const fn tag(self) -> u8 {
        match self {
            Self::Terminal => 1,
            Self::Panic => 2,
        }
    }

    const fn contract_kind(self) -> BindingKind {
        match self {
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequirementSubject {
    GeneratedRole(u128),
    Pool(u128),
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RequirementOwner {
    GeneratedRole(RoleRef),
    Pool(PoolRef),
}

impl RequirementOwner {
    const fn context(self) -> u128 {
        match self {
            Self::GeneratedRole(reference) => reference.context,
            Self::Pool(reference) => reference.context,
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
        }
    }

    #[allow(
        dead_code,
        reason = "crate-private handoff reserved for the Core planner"
    )]
    pub(crate) const fn for_core(&self) -> CorePlanningInput<'_> {
        CorePlanningInput { foundation: self }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct CorePlanningInput<'a> {
    foundation: &'a VerifiedPlanningFoundation,
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
        operation: PoolOperation,
        source: &SourceRange,
    ) -> Option<(RequirementRef, u128)> {
        self.foundation
            .pools
            .iter()
            .flat_map(|pool| pool.admission_sites.iter())
            .find(|site| site.operation == operation && site.source == *source)
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
    Admission {
        source: SourceRange,
        declared: u64,
        required: u64,
    },
    Defect(Arc<str>),
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
        let generated_roles = produce_roles(
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
        let (pools, mut pool_requirements) = produce_pool_plans(
            context,
            semantic_program.for_core_planning(),
            planner.reference,
            plan.reference,
            cancellation,
        )?;
        requirements.append(&mut pool_requirements);
        requirements.sort_by_key(|requirement| requirement.reference.identity);
        if let Some(pool) = pools
            .iter()
            .find(|pool| pool.peak_committed > pool.usable_slots)
        {
            return Err(PlanningFailure::Admission {
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
            .map(|role| role.reference)
            .collect::<Vec<_>>()
            .into();
        plan.requirements = requirements
            .iter()
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
        let planner_roster: Arc<[Planner]> = Arc::from([planner]);
        let domain_plans: Arc<[DomainPlan]> = Arc::from([plan]);
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
            fingerprint,
            _verified: Verified,
        };
        verify(&candidate, cancellation)?;
        Ok(candidate)
    }
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
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PoolFlowState {
    live: u64,
    reserved: u64,
}

#[derive(Default)]
struct PoolFlowSummary {
    peak_live: u64,
    peak_reserved: u64,
    peak_committed: u64,
    admission_sites: Vec<(PoolOperation, SourceRange, u128)>,
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
                let mut summary = PoolFlowSummary::default();
                let states = BTreeSet::from([PoolFlowState {
                    live: 0,
                    reserved: 0,
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
                let identity =
                    planning_source_identity(b"wrela.pool-plan.v1", executable.identity(), source);
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
                for (ordinal, (operation, source, source_type_identity)) in
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
                    });
                    admission_sites.push(PoolAdmissionSite {
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
                Some(value) => {
                    pool_flow_expression(value, states, binding, capacity, program, summary)?
                }
                None => states,
            },
            Statement::Panic { value, .. }
            | Statement::Assert {
                condition: value, ..
            }
            | Statement::Expect {
                condition: value, ..
            }
            | Statement::Initialize { value, .. }
            | Statement::Assign { value, .. }
            | Statement::Evaluate(value) => {
                pool_flow_expression(value, states, binding, capacity, program, summary)?
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
                let entered =
                    pool_flow_expression(condition, states, binding, capacity, program, summary)?;
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
                let entered =
                    pool_flow_expression(value, states, binding, capacity, program, summary)?;
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
                        condition, frontier, binding, capacity, program, summary,
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
                    joined.extend(next.iter().copied());
                    if joined.len() == before {
                        break;
                    }
                    frontier = next;
                }
                joined
            }
            Statement::For { iterable, body, .. } => {
                let entered =
                    pool_flow_expression(iterable, states, binding, capacity, program, summary)?;
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
                let entered =
                    pool_flow_expression(scope, states, binding, capacity, program, summary)?;
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
            )?,
            Statement::Break(_) | Statement::Continue(_) | Statement::Pass(_) => states,
        };
    }
    Ok(states)
}

fn pool_flow_expression(
    expression: &Expression,
    mut states: BTreeSet<PoolFlowState>,
    binding: LocalId,
    capacity: u64,
    program: &VerifiedProgram,
    summary: &mut PoolFlowSummary,
) -> Result<BTreeSet<PoolFlowState>, PlanningFailure> {
    let mut children = Vec::new();
    expression.visit_children(&mut |child| children.push(child.clone()));
    for child in children {
        states = pool_flow_expression(&child, states, binding, capacity, program, summary)?;
    }
    let ExpressionKind::Call { target, arguments } = &expression.kind else {
        return Ok(states);
    };
    let CallTarget::Function { specialization, .. } = target else {
        return Ok(states);
    };
    let Some(operation) = program
        .specialization_function(*specialization)
        .and_then(|function| function.pool_operation)
    else {
        return Ok(states);
    };
    let receiver_matches = arguments.first().is_some_and(
        |receiver| matches!(&receiver.kind, ExpressionKind::Read(place) if place.local == binding),
    );
    if !receiver_matches {
        return Ok(states);
    }
    if matches!(operation, PoolOperation::Allocate | PoolOperation::Reserve) {
        summary
            .admission_sites
            .push((operation, expression.source.clone(), expression.type_id.0));
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
                ..state
            },
            PoolOperation::Consume if state.reserved > 0 => PoolFlowState {
                live: state.live.saturating_add(1).min(limit),
                reserved: state.reserved - 1,
            },
            PoolOperation::Consume => state,
            PoolOperation::Reclaim if state.live > 0 => PoolFlowState {
                live: state.live - 1,
                ..state
            },
            PoolOperation::Reclaim => state,
            PoolOperation::Release if state.reserved > 0 => PoolFlowState {
                reserved: state.reserved - 1,
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

fn planning_source_identity(domain: &[u8], executable: u128, source: &SourceRange) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(domain);
    hash.update(&executable.to_be_bytes());
    hash.update(&(source.path().len() as u64).to_be_bytes());
    hash.update(source.path().as_bytes());
    hash.update(&source.start().to_be_bytes());
    hash.update(&source.end().to_be_bytes());
    hash.digest128()
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
    PoolModelObservation {
        cases: 6,
        agrees: accepted && full && reserved && released && stale && retired,
        accepted,
        full,
        released,
        reserved,
        stale,
        retired,
    }
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
    let expected_planner = verify_planner(
        candidate.context,
        semantic.root(),
        semantic.fingerprint(),
        architecture.fingerprint(),
    );
    if candidate.planner_roster.as_ref() != [expected_planner.clone()] {
        return defect("planner roster is missing, extra, duplicated, or stale");
    }
    let mut expected_plan = verify_domain_plan(
        candidate.context,
        expected_planner.reference,
        semantic.fingerprint(),
        architecture.fingerprint(),
    );
    if candidate.domain_plans.len() != 1
        || candidate.domain_plans[0].reference != expected_plan.reference
        || candidate.domain_plans[0].planner != expected_plan.planner
        || candidate.domain_plans[0].kind != expected_plan.kind
    {
        return defect("Domain Plans are missing, extra, wrong-owner, or stale");
    }
    let expected_roles = verify_roles(
        candidate.context,
        semantic.root(),
        expected_planner.reference,
        expected_plan.reference,
        semantic.fingerprint(),
        architecture.fingerprint(),
        cancellation,
    )?;
    if candidate.generated_roles.as_ref() != expected_roles.as_slice() {
        return defect(
            "Generated Roles are missing, extra, dangling, wrong-owner, wrong-role, wrong-generator, or stale",
        );
    }
    verify_role_graph(candidate, cancellation)?;
    verify_architecture_evidence(candidate, cancellation)?;
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
    let (expected_pools, mut expected_pool_requirements) = produce_pool_plans(
        candidate.context,
        candidate.semantic_program.for_core_planning(),
        expected_planner.reference,
        expected_plan.reference,
        cancellation,
    )?;
    expected_requirements.append(&mut expected_pool_requirements);
    expected_requirements.sort_by_key(|requirement| requirement.reference.identity);
    if candidate.pools.as_ref() != expected_pools.as_slice() {
        return defect("Pool Plans are missing, extra, stale, or semantically false");
    }
    let expected_pool_model = run_pool_model();
    if candidate.pool_model != expected_pool_model || !candidate.pool_model.agrees {
        return defect("bounded Pool authority model disagrees");
    }
    if candidate.requirements.as_ref() != expected_requirements.as_slice() {
        return defect(
            "Requirement Set is missing, extra, duplicate, dangling, wrong-owner, wrong-role, wrong-provenance, or stale",
        );
    }
    expected_plan.generated_roles = expected_roles
        .iter()
        .map(|role| role.reference)
        .collect::<Vec<_>>()
        .into();
    expected_plan.requirements = expected_requirements
        .iter()
        .map(|requirement| requirement.reference)
        .collect::<Vec<_>>()
        .into();
    if candidate.domain_plans.as_ref() != [expected_plan] {
        return defect(
            "Domain Plan exports have missing, extra, duplicate, wrong-owner, stale, or mixed-context references",
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
        if role.kind != GeneratedRoleKind::Boot
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
        let provenance_subject = match requirement.subject {
            RequirementOwner::GeneratedRole(reference) => reference.identity,
            RequirementOwner::Pool(_) => 0,
        };
        if requirement.context != candidate.context
            || requirement.reference.context != candidate.context
            || requirement.owner.context != candidate.context
            || requirement.subject.context() != candidate.context
            || requirement.provenance.domain_plan != candidate.domain_plans[0].reference.identity
            || requirement.provenance.generated_role != provenance_subject
            || requirement.provenance.local_site == 0
        {
            return defect(
                "Planning Requirement has dangling, wrong-owner, wrong-role, mixed-context, or invalid provenance",
            );
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::architecture_planning::{
        ArchitecturePlanningModule, ArchitectureProfile, ContractContext,
    };
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
}
