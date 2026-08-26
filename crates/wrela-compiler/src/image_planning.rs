#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use xxhash_rust::xxh3::Xxh3;

use crate::architecture_planning::{
    BindingKind, ReservationKind, ReservationMultiplicity, VerifiedArchitecturePlanningContract,
    VmAbiCapability,
};
use crate::completed_semantic::CompletedSemanticProgram;
use crate::{Cancellation, Root};

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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequirementSubject {
    GeneratedRole(u128),
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
    pub const fn executable_demand(&self) -> &ExecutableDemandObservation {
        &self.executable_demand
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PlannerRef {
    context: u128,
    identity: u128,
    current_meaning: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DomainPlanRef {
    context: u128,
    identity: u128,
    current_meaning: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct RoleRef {
    context: u128,
    identity: u128,
    current_meaning: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ExecutableRef {
    context: u128,
    identity: u128,
    current_meaning: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DemandInputRef {
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
struct DomainPlan {
    reference: DomainPlanRef,
    planner: PlannerRef,
    kind: DomainPlanKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GeneratedRole {
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
struct Requirement {
    context: u128,
    reference: u128,
    owner: PlannerRef,
    subject: RoleRef,
    provenance: RequirementProvenance,
    category: RequirementCategory,
    bounds: RequirementBounds,
    current_meaning: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ExactExecutableDemand {
    source: DemandInputRef,
    source_count: usize,
    additions: Arc<[ExecutableRef]>,
    fingerprint: u128,
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
                    reference: requirement.reference,
                    owner: requirement.owner.identity,
                    subject: RequirementSubject::GeneratedRole(requirement.subject.identity),
                    provenance: requirement.provenance,
                    category: requirement.category,
                    bounds: requirement.bounds.clone(),
                    current_meaning: requirement.current_meaning,
                })
                .collect::<Vec<_>>()
                .into(),
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
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ImagePlanningModule;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PlanningFailure {
    Cancelled,
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
        let plan = produce_domain_plan(
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
        let requirements = produce_requirements(
            context,
            &generated_roles,
            planner.reference,
            plan.reference,
            semantic.test_application_count(),
            architecture.core_count(),
            architecture.service().maximum_cycle_units,
            cancellation,
        )?;
        let source = DemandInputRef {
            context,
            identity: semantic.executable_demand_fingerprint(),
            current_meaning: semantic.executable_demand_fingerprint(),
        };
        let additions = generated_roles
            .iter()
            .map(|role| role.executable)
            .collect::<Vec<_>>();
        let executable_demand = ExactExecutableDemand {
            source,
            source_count: semantic.source_executable_count(),
            fingerprint: demand_fingerprint(source, semantic.source_executable_count(), &additions),
            additions: additions.into(),
        };
        let planner_roster: Arc<[Planner]> = Arc::from([planner]);
        let domain_plans: Arc<[DomainPlan]> = Arc::from([plan]);
        let fingerprint = foundation_fingerprint(
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
    let identity = identity_hash(b"wrela.planner.image-kind.v1", &[context, 1]);
    Planner {
        reference: PlannerRef {
            context,
            identity,
            current_meaning: identity_hash(
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
    let identity = identity_hash(
        b"wrela.domain-plan.mandatory-image.v1",
        &[context, planner.identity, 1],
    );
    DomainPlan {
        reference: DomainPlanRef {
            context,
            identity,
            current_meaning: identity_hash(
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
        let dependencies = spec
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
        let local_key = u16::try_from(ordinal + 1)
            .map_err(|_| PlanningFailure::Defect(Arc::from("generated role local key overflow")))?;
        let identity = role_identity(context, planner, spec.kind, local_key);
        let current_meaning = role_current_meaning(
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
                identity: identity_hash(b"wrela.generated-executable.v1", &[context, identity]),
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
            requirements.push(make_requirement(
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
    requirements.sort_by_key(|requirement| requirement.reference);
    Ok(requirements)
}

fn make_requirement(
    context: u128,
    owner: PlannerRef,
    plan: DomainPlanRef,
    subject: RoleRef,
    local_site: u16,
    spec: RequirementSpec,
) -> Requirement {
    let reference = requirement_identity(
        context,
        owner.identity,
        subject.identity,
        spec.category,
        local_site,
    );
    let provenance = RequirementProvenance {
        domain_plan: plan.identity,
        generated_role: subject.identity,
        local_site,
    };
    let current_meaning = requirement_current_meaning(
        reference,
        owner.current_meaning,
        subject.current_meaning,
        plan.current_meaning,
        spec.category,
        &spec.bounds,
    );
    Requirement {
        context,
        reference,
        owner,
        subject,
        provenance,
        category: spec.category,
        bounds: spec.bounds,
        current_meaning,
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
    let expected_plan = verify_domain_plan(
        candidate.context,
        expected_planner.reference,
        semantic.fingerprint(),
        architecture.fingerprint(),
    );
    if candidate.domain_plans.as_ref() != [expected_plan.clone()] {
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
    let expected_requirements = verify_requirements(
        candidate.context,
        &expected_roles,
        expected_planner.reference,
        expected_plan.reference,
        semantic.test_application_count(),
        architecture.core_count(),
        architecture.service().maximum_cycle_units,
        cancellation,
    )?;
    if candidate.requirements.as_ref() != expected_requirements.as_slice() {
        return defect(
            "Requirement Set is missing, extra, duplicate, dangling, wrong-owner, wrong-role, wrong-provenance, or stale",
        );
    }
    verify_requirement_bounds(candidate, cancellation)?;
    let mut references = BTreeSet::new();
    for requirement in candidate.requirements.iter() {
        checkpoint(cancellation)?;
        if !references.insert(requirement.reference) {
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
    let expected_fingerprint = foundation_fingerprint(
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
    let identity = identity_hash(b"wrela.planner.image-kind.v1", &[context, 1]);
    let current_meaning = identity_hash(
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
    let identity = identity_hash(
        b"wrela.domain-plan.mandatory-image.v1",
        &[context, planner.identity, 1],
    );
    let current_meaning = identity_hash(
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
        let dependencies = dependency_kinds
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
        let local_key = u16::try_from(ordinal + 1)
            .map_err(|_| PlanningFailure::Defect(Arc::from("verifier role key overflow")))?;
        let identity = role_identity(context, planner, kind, local_key);
        let current_meaning = role_current_meaning(
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
                identity: identity_hash(b"wrela.generated-executable.v1", &[context, identity]),
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
            expected.push(make_requirement(
                context,
                planner,
                plan,
                role.reference,
                u16::try_from(offset + 1).map_err(|_| {
                    PlanningFailure::Defect(Arc::from("verifier requirement site overflow"))
                })?,
                RequirementSpec { category, bounds },
            ));
        }
    }
    expected.sort_by_key(|requirement| requirement.reference);
    Ok(expected)
}

fn verify_role_graph(
    candidate: &VerifiedPlanningFoundation,
    cancellation: &Cancellation,
) -> Result<(), PlanningFailure> {
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
                .any(|identity| candidate.generated_roles[0].reference.identity == *identity)
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
        if requirement.context != candidate.context
            || requirement.owner.context != candidate.context
            || requirement.subject.context != candidate.context
            || requirement.provenance.domain_plan != candidate.domain_plans[0].reference.identity
            || requirement.provenance.generated_role != requirement.subject.identity
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
                role.reference == requirement.subject && role.executable.identity == *executable
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
    let expected = expected_roles
        .iter()
        .map(|role| role.executable)
        .collect::<Vec<_>>();
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
    let fingerprint = demand_fingerprint(source, semantic.source_executable_count(), &expected);
    if candidate.executable_demand.fingerprint != fingerprint {
        return defect("exact Executable Demand fingerprint is false");
    }
    Ok(())
}

fn role_identity(
    context: u128,
    planner: PlannerRef,
    kind: GeneratedRoleKind,
    local_key: u16,
) -> u128 {
    identity_hash(
        b"wrela.generated-role.identity.v1",
        &[
            context,
            planner.identity,
            planner.identity,
            kind.tag().into(),
            local_key.into(),
        ],
    )
}

fn role_current_meaning(
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

fn requirement_identity(
    context: u128,
    owner: u128,
    subject: u128,
    category: RequirementCategory,
    local_site: u16,
) -> u128 {
    identity_hash(
        b"wrela.requirement.identity.v1",
        &[
            context,
            owner,
            subject,
            category.tag().into(),
            local_site.into(),
        ],
    )
}

fn requirement_current_meaning(
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
    hash_bounds(&mut hash, bounds);
    hash.digest128()
}

fn hash_bounds(hash: &mut Xxh3, bounds: &RequirementBounds) {
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
    }
}

fn demand_fingerprint(
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

#[allow(clippy::too_many_arguments)]
fn foundation_fingerprint(
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
    }
    hash.update(&(roles.len() as u64).to_le_bytes());
    for role in roles {
        hash.update(&role.reference.identity.to_le_bytes());
        hash.update(&role.reference.current_meaning.to_le_bytes());
        hash.update(&role.executable.identity.to_le_bytes());
    }
    hash.update(&(requirements.len() as u64).to_le_bytes());
    for requirement in requirements {
        hash.update(&requirement.reference.to_le_bytes());
        hash.update(&requirement.current_meaning.to_le_bytes());
    }
    hash.update(&demand.fingerprint.to_le_bytes());
    hash.digest128()
}

fn identity_hash(domain: &[u8], values: &[u128]) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(domain);
    for value in values {
        hash.update(&value.to_le_bytes());
    }
    hash.digest128()
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
        let path = match root {
            Root::Image => "src/image.wr",
            Root::Test => "src/test.wr",
        };
        let source: &[u8] = match root {
            Root::Image => b"@image\nfn build() -> Image:\n    return Image.new()\n",
            Root::Test => {
                br#"pub suite smoke:
    test passes():
        expect true

@image
fn build() -> Image:
    tests = Test.new(cases=[smoke.passes()])
    return Image.new(tests=tests)
"#
            }
        };
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
        requirements[0].subject.identity ^= 1;
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
        requirements[0].current_meaning ^= 1;
        stale.requirements = requirements.into();
        rejects(&stale);

        let mut mixed_context = original.clone();
        let mut requirements = mixed_context.requirements.to_vec();
        requirements[0].context ^= 1;
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
}
