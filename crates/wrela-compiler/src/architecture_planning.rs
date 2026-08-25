use std::sync::Arc;

use xxhash_rust::xxh3::Xxh3;

use crate::Cancellation;

const CONTRACT_SCHEMA_VERSION: u16 = 1;
const CURRENT_AARCH64_CONTRACT_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ArchitectureProfile {
    CurrentAarch64,
    X86_64,
}

impl ArchitectureProfile {
    pub(crate) const fn canonical_name(self) -> &'static str {
        match self {
            Self::CurrentAarch64 => "aarch64-current",
            Self::X86_64 => "x86_64",
        }
    }

    const fn tag(self) -> u8 {
        match self {
            Self::CurrentAarch64 => 1,
            Self::X86_64 => 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchitecturePlanningObservation {
    profile: ArchitectureProfile,
    identity: u128,
    contract_schema_version: u16,
    contract_version: u16,
    fingerprint: u128,
    distribution_input_receipt: u128,
    symbolic_core_count: u16,
    page_quantum_bytes: u64,
    minimum_ram_bytes: u64,
    maximum_ram_bytes: u64,
}

impl ArchitecturePlanningObservation {
    #[must_use]
    pub const fn profile(&self) -> ArchitectureProfile {
        self.profile
    }

    #[must_use]
    pub const fn identity(&self) -> u128 {
        self.identity
    }

    #[must_use]
    pub const fn contract_schema_version(&self) -> u16 {
        self.contract_schema_version
    }

    #[must_use]
    pub const fn contract_version(&self) -> u16 {
        self.contract_version
    }

    #[must_use]
    pub const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }

    #[must_use]
    pub const fn distribution_input_receipt(&self) -> u128 {
        self.distribution_input_receipt
    }

    #[must_use]
    pub const fn symbolic_core_count(&self) -> u16 {
        self.symbolic_core_count
    }

    #[must_use]
    pub const fn page_quantum_bytes(&self) -> u64 {
        self.page_quantum_bytes
    }

    #[must_use]
    pub const fn minimum_ram_bytes(&self) -> u64 {
        self.minimum_ram_bytes
    }

    #[must_use]
    pub const fn maximum_ram_bytes(&self) -> u64 {
        self.maximum_ram_bytes
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ContractContext {
    distribution_version: &'static str,
    distribution_digest: u128,
}

impl ContractContext {
    pub(crate) const fn new(distribution_version: &'static str, distribution_digest: u128) -> Self {
        Self {
            distribution_version,
            distribution_digest,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ArchitecturePlanningModule {
    context: ContractContext,
}

impl ArchitecturePlanningModule {
    pub(crate) const fn new(context: ContractContext) -> Self {
        Self { context }
    }

    pub(crate) fn authenticate(
        &self,
        profile: ArchitectureProfile,
        cancellation: &Cancellation,
    ) -> Result<VerifiedArchitecturePlanningContract, ContractFailure> {
        if cancellation.is_cancelled() {
            return Err(ContractFailure::for_selection(
                ContractFailureKind::Cancelled,
                profile,
                self.context,
            ));
        }
        match profile {
            ArchitectureProfile::CurrentAarch64 => verify(
                current_aarch64_candidate(self.context),
                self.context,
                cancellation,
            ),
            ArchitectureProfile::X86_64 => Err(ContractFailure::for_selection(
                ContractFailureKind::UnsupportedProfile,
                profile,
                self.context,
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContractFailureKind {
    UnsupportedProfile,
    UnsupportedVersion,
    Unauthenticated,
    MixedContext,
    IdentityMismatch,
    InputReceiptMismatch,
    FingerprintMismatch,
    AuthenticationMismatch,
    MalformedFacts,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ContractFailure {
    kind: ContractFailureKind,
    reproduction: Box<ContractReproduction>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ContractReproduction {
    profile: ArchitectureProfile,
    expected_schema_version: u16,
    actual_schema_version: u16,
    expected_contract_version: u16,
    actual_contract_version: u16,
    identity: u128,
    fingerprint: u128,
    expected_context_receipt: u128,
    actual_context_receipt: u128,
    expected_input_receipt: u128,
    actual_input_receipt: u128,
    detail: &'static str,
}

impl ContractFailure {
    pub(crate) const fn kind(&self) -> ContractFailureKind {
        self.kind
    }

    #[cfg(test)]
    pub(crate) fn reproduction(&self) -> &ContractReproduction {
        &self.reproduction
    }

    pub(crate) fn bounded_evidence(&self) -> String {
        format!(
            "kind={} profile={} expected_schema={} actual_schema={} expected_contract={} actual_contract={} identity={:032x} fingerprint={:032x} expected_context={:032x} actual_context={:032x} expected_input={:032x} actual_input={:032x} detail={}",
            self.kind.canonical_name(),
            self.reproduction.profile.canonical_name(),
            self.reproduction.expected_schema_version,
            self.reproduction.actual_schema_version,
            self.reproduction.expected_contract_version,
            self.reproduction.actual_contract_version,
            self.reproduction.identity,
            self.reproduction.fingerprint,
            self.reproduction.expected_context_receipt,
            self.reproduction.actual_context_receipt,
            self.reproduction.expected_input_receipt,
            self.reproduction.actual_input_receipt,
            self.reproduction.detail,
        )
    }

    fn for_selection(
        kind: ContractFailureKind,
        profile: ArchitectureProfile,
        context: ContractContext,
    ) -> Self {
        let identity = canonical_identity(profile, CURRENT_AARCH64_CONTRACT_VERSION);
        let context_receipt = canonical_context_receipt(context);
        let input_receipt = canonical_distribution_input_receipt(context, CONTRACT_SCHEMA_VERSION);
        Self {
            kind,
            reproduction: Box::new(ContractReproduction {
                profile,
                expected_schema_version: CONTRACT_SCHEMA_VERSION,
                actual_schema_version: CONTRACT_SCHEMA_VERSION,
                expected_contract_version: CURRENT_AARCH64_CONTRACT_VERSION,
                actual_contract_version: CURRENT_AARCH64_CONTRACT_VERSION,
                identity,
                fingerprint: 0,
                expected_context_receipt: context_receipt,
                actual_context_receipt: context_receipt,
                expected_input_receipt: input_receipt,
                actual_input_receipt: input_receipt,
                detail: kind.default_detail(),
            }),
        }
    }

    fn for_candidate(
        kind: ContractFailureKind,
        detail: &'static str,
        candidate: &RawContract,
        expected_context: ContractContext,
    ) -> Self {
        Self {
            kind,
            reproduction: Box::new(ContractReproduction {
                profile: candidate.profile,
                expected_schema_version: CONTRACT_SCHEMA_VERSION,
                actual_schema_version: candidate.schema_version,
                expected_contract_version: CURRENT_AARCH64_CONTRACT_VERSION,
                actual_contract_version: candidate.version,
                identity: candidate.identity,
                fingerprint: candidate.fingerprint,
                expected_context_receipt: canonical_context_receipt(expected_context),
                actual_context_receipt: canonical_context_receipt(candidate.context),
                expected_input_receipt: canonical_distribution_input_receipt(
                    expected_context,
                    CONTRACT_SCHEMA_VERSION,
                ),
                actual_input_receipt: candidate.distribution_input_receipt,
                detail,
            }),
        }
    }

    fn malformed(detail: &'static str) -> Self {
        Self::unbound(ContractFailureKind::MalformedFacts, detail)
    }

    fn cancelled() -> Self {
        Self::unbound(
            ContractFailureKind::Cancelled,
            ContractFailureKind::Cancelled.default_detail(),
        )
    }

    fn unbound(kind: ContractFailureKind, detail: &'static str) -> Self {
        let context = ContractContext::new("unbound-private-verifier", 0);
        let mut failure = Self::for_selection(kind, ArchitectureProfile::CurrentAarch64, context);
        failure.reproduction.detail = detail;
        failure
    }

    fn bind_candidate(
        mut self,
        candidate: &RawContract,
        expected_context: ContractContext,
    ) -> Self {
        self.reproduction = Self::for_candidate(
            self.kind,
            self.reproduction.detail,
            candidate,
            expected_context,
        )
        .reproduction;
        self
    }
}

impl ContractFailureKind {
    const fn canonical_name(self) -> &'static str {
        match self {
            Self::UnsupportedProfile => "unsupported_profile",
            Self::UnsupportedVersion => "unsupported_version",
            Self::Unauthenticated => "unauthenticated",
            Self::MixedContext => "mixed_context",
            Self::IdentityMismatch => "identity_mismatch",
            Self::InputReceiptMismatch => "input_receipt_mismatch",
            Self::FingerprintMismatch => "fingerprint_mismatch",
            Self::AuthenticationMismatch => "authentication_mismatch",
            Self::MalformedFacts => "malformed_facts",
            Self::Cancelled => "cancelled",
        }
    }

    const fn default_detail(self) -> &'static str {
        match self {
            Self::UnsupportedProfile => "unsupported architecture profile",
            Self::UnsupportedVersion => "unsupported architecture planning contract version",
            Self::Unauthenticated => "architecture planning contract is unauthenticated",
            Self::MixedContext => "architecture planning contract belongs to another context",
            Self::IdentityMismatch => "architecture planning contract identity mismatch",
            Self::InputReceiptMismatch => {
                "architecture planning contract distribution input receipt mismatch"
            }
            Self::FingerprintMismatch => "architecture planning contract fingerprint mismatch",
            Self::AuthenticationMismatch => {
                "architecture planning contract authentication mismatch"
            }
            Self::MalformedFacts => "architecture planning contract facts are malformed",
            Self::Cancelled => "architecture planning contract verification cancelled",
        }
    }
}

#[cfg(test)]
impl ContractReproduction {
    pub(crate) const fn profile(&self) -> ArchitectureProfile {
        self.profile
    }

    pub(crate) const fn expected_schema_version(&self) -> u16 {
        self.expected_schema_version
    }

    pub(crate) const fn actual_schema_version(&self) -> u16 {
        self.actual_schema_version
    }

    pub(crate) const fn expected_contract_version(&self) -> u16 {
        self.expected_contract_version
    }

    pub(crate) const fn actual_contract_version(&self) -> u16 {
        self.actual_contract_version
    }

    pub(crate) const fn expected_context_receipt(&self) -> u128 {
        self.expected_context_receipt
    }

    pub(crate) const fn actual_context_receipt(&self) -> u128 {
        self.actual_context_receipt
    }

    pub(crate) const fn expected_input_receipt(&self) -> u128 {
        self.expected_input_receipt
    }

    pub(crate) const fn actual_input_receipt(&self) -> u128 {
        self.actual_input_receipt
    }

    pub(crate) const fn detail(&self) -> &'static str {
        self.detail
    }
}

#[derive(Clone, Debug)]
pub(crate) struct VerifiedArchitecturePlanningContract {
    profile: ArchitectureProfile,
    identity: u128,
    schema_version: u16,
    version: u16,
    fingerprint: u128,
    distribution_input_receipt: u128,
    facts: ContractFacts,
    _verified: Verified,
}

#[derive(Clone, Debug)]
struct Verified;

#[allow(dead_code)]
impl VerifiedArchitecturePlanningContract {
    pub(crate) fn observation(&self) -> ArchitecturePlanningObservation {
        ArchitecturePlanningObservation {
            profile: self.profile,
            identity: self.identity,
            contract_schema_version: self.schema_version,
            contract_version: self.version,
            fingerprint: self.fingerprint,
            distribution_input_receipt: self.distribution_input_receipt,
            symbolic_core_count: u16::try_from(self.facts.cores.len())
                .expect("verified symbolic core count fits its contract width"),
            page_quantum_bytes: self.facts.page.quantum_bytes,
            minimum_ram_bytes: self.facts.capacity.minimum_ram_bytes,
            maximum_ram_bytes: self.facts.capacity.maximum_ram_bytes,
        }
    }

    pub(crate) const fn for_admission(&self) -> AdmissionArchitecture<'_> {
        AdmissionArchitecture { facts: &self.facts }
    }

    pub(crate) const fn for_service_analysis(&self) -> ServiceArchitecture<'_> {
        ServiceArchitecture { facts: &self.facts }
    }

    pub(crate) const fn for_logical_layout(&self) -> LogicalLayoutArchitecture<'_> {
        LogicalLayoutArchitecture { facts: &self.facts }
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct AdmissionArchitecture<'a> {
    facts: &'a ContractFacts,
}

#[allow(dead_code)]
impl AdmissionArchitecture<'_> {
    pub(crate) fn capabilities(&self) -> &[VmAbiCapability] {
        &self.facts.capabilities
    }

    pub(crate) const fn capacity(&self) -> CapacityRules {
        self.facts.capacity
    }

    pub(crate) fn device_slots(&self) -> &[DeviceSlot] {
        &self.facts.device_slots
    }

    pub(crate) fn binding_slots(&self) -> &[BindingSlot] {
        &self.facts.binding_slots
    }

    pub(crate) const fn interrupts(&self) -> InterruptRules {
        self.facts.interrupts
    }

    pub(crate) const fn dma(&self) -> DmaRules {
        self.facts.dma
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct ServiceArchitecture<'a> {
    facts: &'a ContractFacts,
}

#[allow(dead_code)]
impl ServiceArchitecture<'_> {
    pub(crate) fn cores(&self) -> &[SymbolicCore] {
        &self.facts.cores
    }

    pub(crate) const fn costs(&self) -> ServiceCostBaseline {
        self.facts.service
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct LogicalLayoutArchitecture<'a> {
    facts: &'a ContractFacts,
}

#[allow(dead_code)]
impl LogicalLayoutArchitecture<'_> {
    pub(crate) const fn capacity(&self) -> CapacityRules {
        self.facts.capacity
    }

    pub(crate) const fn alignment(&self) -> AlignmentRules {
        self.facts.alignment
    }

    pub(crate) const fn page(&self) -> PageRules {
        self.facts.page
    }

    pub(crate) const fn guards(&self) -> GuardRules {
        self.facts.guards
    }

    pub(crate) fn reservations(&self) -> &[ReservationRule] {
        &self.facts.reservations
    }

    pub(crate) fn envelopes(&self) -> &[EnvelopeRule] {
        &self.facts.envelopes
    }

    pub(crate) fn regions(&self) -> &[RegionRule] {
        &self.facts.regions
    }

    pub(crate) const fn dma(&self) -> DmaRules {
        self.facts.dma
    }
}

#[derive(Clone, Debug)]
struct RawContract {
    profile: ArchitectureProfile,
    schema_version: u16,
    version: u16,
    identity: u128,
    fingerprint: u128,
    context: ContractContext,
    distribution_input_receipt: u128,
    authentication: Option<u128>,
    facts: ContractFacts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ContractFacts {
    cores: Arc<[SymbolicCore]>,
    capabilities: Arc<[VmAbiCapability]>,
    capacity: CapacityRules,
    alignment: AlignmentRules,
    page: PageRules,
    guards: GuardRules,
    reservations: Arc<[ReservationRule]>,
    envelopes: Arc<[EnvelopeRule]>,
    regions: Arc<[RegionRule]>,
    device_slots: Arc<[DeviceSlot]>,
    binding_slots: Arc<[BindingSlot]>,
    interrupts: InterruptRules,
    dma: DmaRules,
    service: ServiceCostBaseline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SymbolicCore {
    pub(crate) ordinal: u16,
    pub(crate) role: CoreRole,
    pub(crate) maximum_service_units: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CoreRole {
    Primary,
    Secondary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum VmAbiCapability {
    TypedTerminalLifecycle,
    PanicPulse,
    GuestShutdownPulse,
    PciVirtioModern,
    SplitVirtqueue,
    SharedIntx,
    DmaOwnership,
    SecondaryCoreStartup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CapacityRules {
    pub(crate) minimum_ram_bytes: u64,
    pub(crate) maximum_ram_bytes: u64,
    pub(crate) maximum_requirements: u32,
    pub(crate) maximum_allocations: u32,
    pub(crate) maximum_generated_roles: u32,
    pub(crate) maximum_activation_homes: u32,
}

#[allow(dead_code)]
impl CapacityRules {
    pub(crate) const fn minimum_ram_bytes(self) -> u64 {
        self.minimum_ram_bytes
    }

    pub(crate) const fn maximum_requirements(self) -> u32 {
        self.maximum_requirements
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AlignmentRules {
    pub(crate) allocation_bytes: u64,
    pub(crate) region_bytes: u64,
    pub(crate) dma_bytes: u64,
    pub(crate) maximum_envelope_bytes: u64,
}

#[allow(dead_code)]
impl AlignmentRules {
    pub(crate) const fn region_bytes(self) -> u64 {
        self.region_bytes
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PageRules {
    pub(crate) quantum_bytes: u64,
    pub(crate) round_each_region_once: bool,
}

#[allow(dead_code)]
impl PageRules {
    pub(crate) const fn quantum_bytes(self) -> u64 {
        self.quantum_bytes
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GuardRules {
    pub(crate) null_pages: u16,
    pub(crate) normal_stack_before_pages: u16,
    pub(crate) normal_stack_after_pages: u16,
    pub(crate) interrupt_stack_before_pages: u16,
    pub(crate) interrupt_stack_after_pages: u16,
}

#[allow(dead_code)]
impl GuardRules {
    pub(crate) const fn normal_stack_before_pages(self) -> u16 {
        self.normal_stack_before_pages
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ReservationRule {
    pub(crate) kind: ReservationKind,
    pub(crate) region: RegionKind,
    pub(crate) bytes: u64,
    pub(crate) alignment: u64,
    pub(crate) multiplicity: ReservationMultiplicity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ReservationKind {
    DtbExclusion,
    BootState,
    PageTables,
    TerminalTransport,
    PanicState,
    PerCoreBootState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ReservationMultiplicity {
    Once,
    PerCore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct EnvelopeRule {
    pub(crate) kind: EnvelopeKind,
    pub(crate) region: RegionKind,
    pub(crate) maximum_bytes: u64,
    pub(crate) alignment: u64,
    pub(crate) protection: ProtectionClass,
    pub(crate) dma_owned: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum EnvelopeKind {
    Executable,
    ImmutableData,
    PerCoreState,
    SharedState,
    DmaState,
    NormalStack,
    InterruptStack,
    AsyncFrame,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RegionRule {
    pub(crate) kind: RegionKind,
    pub(crate) order: u8,
    pub(crate) protection: ProtectionClass,
    pub(crate) alignment: u64,
    pub(crate) page_rounded: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RegionKind {
    BootReservation,
    Executable,
    ImmutableData,
    PerCoreMutable,
    SharedMutable,
    DmaOwned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ProtectionClass {
    Reserved,
    ReadExecute,
    ReadOnly,
    ReadWrite,
    DeviceOwned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct DeviceSlot {
    pub(crate) ordinal: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BindingSlot {
    pub(crate) ordinal: u8,
    pub(crate) kind: BindingKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum BindingKind {
    Display,
    Input,
    EventStore,
    MonotonicClock,
    Entropy,
    Telemetry,
    Terminal,
    Panic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InterruptRules {
    pub(crate) route_slots: u8,
    pub(crate) maximum_sources_per_route: u8,
    pub(crate) maximum_causes_per_service: u16,
}

#[allow(dead_code)]
impl InterruptRules {
    pub(crate) const fn route_slots(self) -> u8 {
        self.route_slots
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DmaRules {
    pub(crate) maximum_total_bytes: u64,
    pub(crate) maximum_buffer_bytes: u64,
    pub(crate) required_alignment: u64,
    pub(crate) maximum_in_flight: u16,
}

#[allow(dead_code)]
impl DmaRules {
    pub(crate) const fn required_alignment(self) -> u64 {
        self.required_alignment
    }

    pub(crate) const fn maximum_in_flight(self) -> u16 {
        self.maximum_in_flight
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ServiceCostBaseline {
    pub(crate) schema_version: u16,
    pub(crate) ingress_units: u32,
    pub(crate) actor_turn_units: u32,
    pub(crate) group_child_units: u32,
    pub(crate) driver_units: u32,
    pub(crate) cleanup_units: u32,
    pub(crate) cross_core_units: u32,
    pub(crate) cancellation_checkpoint_units: u32,
    pub(crate) maximum_cycle_units: u32,
    pub(crate) maximum_cancellation_delay_units: u32,
}

#[allow(dead_code)]
impl ServiceCostBaseline {
    pub(crate) const fn schema_version(self) -> u16 {
        self.schema_version
    }

    pub(crate) const fn maximum_cycle_units(self) -> u32 {
        self.maximum_cycle_units
    }

    pub(crate) const fn maximum_cancellation_delay_units(self) -> u32 {
        self.maximum_cancellation_delay_units
    }
}

fn current_aarch64_candidate(context: ContractContext) -> RawContract {
    let facts = producer_current_aarch64_facts();
    let identity = canonical_identity(
        ArchitectureProfile::CurrentAarch64,
        CURRENT_AARCH64_CONTRACT_VERSION,
    );
    let fingerprint = canonical_fingerprint(
        ArchitectureProfile::CurrentAarch64,
        CONTRACT_SCHEMA_VERSION,
        CURRENT_AARCH64_CONTRACT_VERSION,
        identity,
        &facts,
    );
    let distribution_input_receipt =
        canonical_distribution_input_receipt(context, CONTRACT_SCHEMA_VERSION);
    RawContract {
        profile: ArchitectureProfile::CurrentAarch64,
        schema_version: CONTRACT_SCHEMA_VERSION,
        version: CURRENT_AARCH64_CONTRACT_VERSION,
        identity,
        fingerprint,
        context,
        distribution_input_receipt,
        authentication: Some(authentication_tag(
            distribution_input_receipt,
            identity,
            fingerprint,
        )),
        facts,
    }
}

fn producer_current_aarch64_facts() -> ContractFacts {
    ContractFacts {
        cores: Arc::from([
            SymbolicCore {
                ordinal: 0,
                role: CoreRole::Primary,
                maximum_service_units: 250_000,
            },
            SymbolicCore {
                ordinal: 1,
                role: CoreRole::Secondary,
                maximum_service_units: 250_000,
            },
            SymbolicCore {
                ordinal: 2,
                role: CoreRole::Secondary,
                maximum_service_units: 250_000,
            },
            SymbolicCore {
                ordinal: 3,
                role: CoreRole::Secondary,
                maximum_service_units: 250_000,
            },
        ]),
        capabilities: Arc::from([
            VmAbiCapability::TypedTerminalLifecycle,
            VmAbiCapability::PanicPulse,
            VmAbiCapability::GuestShutdownPulse,
            VmAbiCapability::PciVirtioModern,
            VmAbiCapability::SplitVirtqueue,
            VmAbiCapability::SharedIntx,
            VmAbiCapability::DmaOwnership,
            VmAbiCapability::SecondaryCoreStartup,
        ]),
        capacity: CapacityRules {
            minimum_ram_bytes: 128 * 1024 * 1024,
            maximum_ram_bytes: 2 * 1024 * 1024 * 1024,
            maximum_requirements: 65_536,
            maximum_allocations: 65_536,
            maximum_generated_roles: 16_384,
            maximum_activation_homes: 16_384,
        },
        alignment: AlignmentRules {
            allocation_bytes: 16,
            region_bytes: 4096,
            dma_bytes: 4096,
            maximum_envelope_bytes: 65_536,
        },
        page: PageRules {
            quantum_bytes: 4096,
            round_each_region_once: true,
        },
        guards: GuardRules {
            null_pages: 1,
            normal_stack_before_pages: 1,
            normal_stack_after_pages: 1,
            interrupt_stack_before_pages: 1,
            interrupt_stack_after_pages: 1,
        },
        reservations: Arc::from([
            ReservationRule {
                kind: ReservationKind::DtbExclusion,
                region: RegionKind::BootReservation,
                bytes: 2 * 1024 * 1024,
                alignment: 4096,
                multiplicity: ReservationMultiplicity::Once,
            },
            ReservationRule {
                kind: ReservationKind::BootState,
                region: RegionKind::BootReservation,
                bytes: 1024 * 1024,
                alignment: 4096,
                multiplicity: ReservationMultiplicity::Once,
            },
            ReservationRule {
                kind: ReservationKind::PageTables,
                region: RegionKind::BootReservation,
                bytes: 2 * 1024 * 1024,
                alignment: 4096,
                multiplicity: ReservationMultiplicity::Once,
            },
            ReservationRule {
                kind: ReservationKind::TerminalTransport,
                region: RegionKind::SharedMutable,
                bytes: 1024 * 1024,
                alignment: 4096,
                multiplicity: ReservationMultiplicity::Once,
            },
            ReservationRule {
                kind: ReservationKind::PanicState,
                region: RegionKind::PerCoreMutable,
                bytes: 4096,
                alignment: 4096,
                multiplicity: ReservationMultiplicity::PerCore,
            },
            ReservationRule {
                kind: ReservationKind::PerCoreBootState,
                region: RegionKind::PerCoreMutable,
                bytes: 64 * 1024,
                alignment: 4096,
                multiplicity: ReservationMultiplicity::PerCore,
            },
        ]),
        envelopes: Arc::from([
            envelope(
                EnvelopeKind::Executable,
                RegionKind::Executable,
                64 * 1024 * 1024,
                4096,
                ProtectionClass::ReadExecute,
                false,
            ),
            envelope(
                EnvelopeKind::ImmutableData,
                RegionKind::ImmutableData,
                64 * 1024 * 1024,
                4096,
                ProtectionClass::ReadOnly,
                false,
            ),
            envelope(
                EnvelopeKind::PerCoreState,
                RegionKind::PerCoreMutable,
                16 * 1024 * 1024,
                64,
                ProtectionClass::ReadWrite,
                false,
            ),
            envelope(
                EnvelopeKind::SharedState,
                RegionKind::SharedMutable,
                256 * 1024 * 1024,
                64,
                ProtectionClass::ReadWrite,
                false,
            ),
            envelope(
                EnvelopeKind::DmaState,
                RegionKind::DmaOwned,
                256 * 1024 * 1024,
                4096,
                ProtectionClass::DeviceOwned,
                true,
            ),
            envelope(
                EnvelopeKind::NormalStack,
                RegionKind::PerCoreMutable,
                8 * 1024 * 1024,
                4096,
                ProtectionClass::ReadWrite,
                false,
            ),
            envelope(
                EnvelopeKind::InterruptStack,
                RegionKind::PerCoreMutable,
                2 * 1024 * 1024,
                4096,
                ProtectionClass::ReadWrite,
                false,
            ),
            envelope(
                EnvelopeKind::AsyncFrame,
                RegionKind::SharedMutable,
                1024 * 1024,
                64,
                ProtectionClass::ReadWrite,
                false,
            ),
        ]),
        regions: canonical_regions(),
        device_slots: Arc::from(
            (0..8)
                .map(|ordinal| DeviceSlot { ordinal })
                .collect::<Vec<_>>(),
        ),
        binding_slots: canonical_bindings(),
        interrupts: InterruptRules {
            route_slots: 4,
            maximum_sources_per_route: 8,
            maximum_causes_per_service: 64,
        },
        dma: DmaRules {
            maximum_total_bytes: 256 * 1024 * 1024,
            maximum_buffer_bytes: 8 * 1024 * 1024,
            required_alignment: 4096,
            maximum_in_flight: 1024,
        },
        service: ServiceCostBaseline {
            schema_version: 1,
            ingress_units: 8,
            actor_turn_units: 16,
            group_child_units: 12,
            driver_units: 20,
            cleanup_units: 10,
            cross_core_units: 6,
            cancellation_checkpoint_units: 4,
            maximum_cycle_units: 1_000_000,
            maximum_cancellation_delay_units: 250_000,
        },
    }
}

fn verifier_current_aarch64_facts() -> ContractFacts {
    // This is deliberately reconstructed independently from the producer above. A field
    // omitted or changed on either side therefore becomes a verifier disagreement.
    ContractFacts {
        cores: Arc::from([
            SymbolicCore {
                ordinal: 0,
                role: CoreRole::Primary,
                maximum_service_units: 250_000,
            },
            SymbolicCore {
                ordinal: 1,
                role: CoreRole::Secondary,
                maximum_service_units: 250_000,
            },
            SymbolicCore {
                ordinal: 2,
                role: CoreRole::Secondary,
                maximum_service_units: 250_000,
            },
            SymbolicCore {
                ordinal: 3,
                role: CoreRole::Secondary,
                maximum_service_units: 250_000,
            },
        ]),
        capabilities: Arc::from([
            VmAbiCapability::TypedTerminalLifecycle,
            VmAbiCapability::PanicPulse,
            VmAbiCapability::GuestShutdownPulse,
            VmAbiCapability::PciVirtioModern,
            VmAbiCapability::SplitVirtqueue,
            VmAbiCapability::SharedIntx,
            VmAbiCapability::DmaOwnership,
            VmAbiCapability::SecondaryCoreStartup,
        ]),
        capacity: CapacityRules {
            minimum_ram_bytes: 134_217_728,
            maximum_ram_bytes: 2_147_483_648,
            maximum_requirements: 65_536,
            maximum_allocations: 65_536,
            maximum_generated_roles: 16_384,
            maximum_activation_homes: 16_384,
        },
        alignment: AlignmentRules {
            allocation_bytes: 16,
            region_bytes: 4096,
            dma_bytes: 4096,
            maximum_envelope_bytes: 65_536,
        },
        page: PageRules {
            quantum_bytes: 4096,
            round_each_region_once: true,
        },
        guards: GuardRules {
            null_pages: 1,
            normal_stack_before_pages: 1,
            normal_stack_after_pages: 1,
            interrupt_stack_before_pages: 1,
            interrupt_stack_after_pages: 1,
        },
        reservations: Arc::from([
            ReservationRule {
                kind: ReservationKind::DtbExclusion,
                region: RegionKind::BootReservation,
                bytes: 2_097_152,
                alignment: 4096,
                multiplicity: ReservationMultiplicity::Once,
            },
            ReservationRule {
                kind: ReservationKind::BootState,
                region: RegionKind::BootReservation,
                bytes: 1_048_576,
                alignment: 4096,
                multiplicity: ReservationMultiplicity::Once,
            },
            ReservationRule {
                kind: ReservationKind::PageTables,
                region: RegionKind::BootReservation,
                bytes: 2_097_152,
                alignment: 4096,
                multiplicity: ReservationMultiplicity::Once,
            },
            ReservationRule {
                kind: ReservationKind::TerminalTransport,
                region: RegionKind::SharedMutable,
                bytes: 1_048_576,
                alignment: 4096,
                multiplicity: ReservationMultiplicity::Once,
            },
            ReservationRule {
                kind: ReservationKind::PanicState,
                region: RegionKind::PerCoreMutable,
                bytes: 4096,
                alignment: 4096,
                multiplicity: ReservationMultiplicity::PerCore,
            },
            ReservationRule {
                kind: ReservationKind::PerCoreBootState,
                region: RegionKind::PerCoreMutable,
                bytes: 65_536,
                alignment: 4096,
                multiplicity: ReservationMultiplicity::PerCore,
            },
        ]),
        envelopes: Arc::from([
            EnvelopeRule {
                kind: EnvelopeKind::Executable,
                region: RegionKind::Executable,
                maximum_bytes: 67_108_864,
                alignment: 4096,
                protection: ProtectionClass::ReadExecute,
                dma_owned: false,
            },
            EnvelopeRule {
                kind: EnvelopeKind::ImmutableData,
                region: RegionKind::ImmutableData,
                maximum_bytes: 67_108_864,
                alignment: 4096,
                protection: ProtectionClass::ReadOnly,
                dma_owned: false,
            },
            EnvelopeRule {
                kind: EnvelopeKind::PerCoreState,
                region: RegionKind::PerCoreMutable,
                maximum_bytes: 16_777_216,
                alignment: 64,
                protection: ProtectionClass::ReadWrite,
                dma_owned: false,
            },
            EnvelopeRule {
                kind: EnvelopeKind::SharedState,
                region: RegionKind::SharedMutable,
                maximum_bytes: 268_435_456,
                alignment: 64,
                protection: ProtectionClass::ReadWrite,
                dma_owned: false,
            },
            EnvelopeRule {
                kind: EnvelopeKind::DmaState,
                region: RegionKind::DmaOwned,
                maximum_bytes: 268_435_456,
                alignment: 4096,
                protection: ProtectionClass::DeviceOwned,
                dma_owned: true,
            },
            EnvelopeRule {
                kind: EnvelopeKind::NormalStack,
                region: RegionKind::PerCoreMutable,
                maximum_bytes: 8_388_608,
                alignment: 4096,
                protection: ProtectionClass::ReadWrite,
                dma_owned: false,
            },
            EnvelopeRule {
                kind: EnvelopeKind::InterruptStack,
                region: RegionKind::PerCoreMutable,
                maximum_bytes: 2_097_152,
                alignment: 4096,
                protection: ProtectionClass::ReadWrite,
                dma_owned: false,
            },
            EnvelopeRule {
                kind: EnvelopeKind::AsyncFrame,
                region: RegionKind::SharedMutable,
                maximum_bytes: 1_048_576,
                alignment: 64,
                protection: ProtectionClass::ReadWrite,
                dma_owned: false,
            },
        ]),
        regions: Arc::from([
            RegionRule {
                kind: RegionKind::BootReservation,
                order: 0,
                protection: ProtectionClass::Reserved,
                alignment: 4096,
                page_rounded: true,
            },
            RegionRule {
                kind: RegionKind::Executable,
                order: 1,
                protection: ProtectionClass::ReadExecute,
                alignment: 4096,
                page_rounded: true,
            },
            RegionRule {
                kind: RegionKind::ImmutableData,
                order: 2,
                protection: ProtectionClass::ReadOnly,
                alignment: 4096,
                page_rounded: true,
            },
            RegionRule {
                kind: RegionKind::PerCoreMutable,
                order: 3,
                protection: ProtectionClass::ReadWrite,
                alignment: 4096,
                page_rounded: true,
            },
            RegionRule {
                kind: RegionKind::SharedMutable,
                order: 4,
                protection: ProtectionClass::ReadWrite,
                alignment: 4096,
                page_rounded: true,
            },
            RegionRule {
                kind: RegionKind::DmaOwned,
                order: 5,
                protection: ProtectionClass::DeviceOwned,
                alignment: 4096,
                page_rounded: true,
            },
        ]),
        device_slots: Arc::from(
            (0..8)
                .map(|ordinal| DeviceSlot { ordinal })
                .collect::<Vec<_>>(),
        ),
        binding_slots: Arc::from([
            BindingSlot {
                ordinal: 0,
                kind: BindingKind::Display,
            },
            BindingSlot {
                ordinal: 1,
                kind: BindingKind::Input,
            },
            BindingSlot {
                ordinal: 2,
                kind: BindingKind::EventStore,
            },
            BindingSlot {
                ordinal: 3,
                kind: BindingKind::MonotonicClock,
            },
            BindingSlot {
                ordinal: 4,
                kind: BindingKind::Entropy,
            },
            BindingSlot {
                ordinal: 5,
                kind: BindingKind::Telemetry,
            },
            BindingSlot {
                ordinal: 6,
                kind: BindingKind::Terminal,
            },
            BindingSlot {
                ordinal: 7,
                kind: BindingKind::Panic,
            },
        ]),
        interrupts: InterruptRules {
            route_slots: 4,
            maximum_sources_per_route: 8,
            maximum_causes_per_service: 64,
        },
        dma: DmaRules {
            maximum_total_bytes: 268_435_456,
            maximum_buffer_bytes: 8_388_608,
            required_alignment: 4096,
            maximum_in_flight: 1024,
        },
        service: ServiceCostBaseline {
            schema_version: 1,
            ingress_units: 8,
            actor_turn_units: 16,
            group_child_units: 12,
            driver_units: 20,
            cleanup_units: 10,
            cross_core_units: 6,
            cancellation_checkpoint_units: 4,
            maximum_cycle_units: 1_000_000,
            maximum_cancellation_delay_units: 250_000,
        },
    }
}

const fn envelope(
    kind: EnvelopeKind,
    region: RegionKind,
    maximum_bytes: u64,
    alignment: u64,
    protection: ProtectionClass,
    dma_owned: bool,
) -> EnvelopeRule {
    EnvelopeRule {
        kind,
        region,
        maximum_bytes,
        alignment,
        protection,
        dma_owned,
    }
}

fn canonical_regions() -> Arc<[RegionRule]> {
    Arc::from([
        RegionRule {
            kind: RegionKind::BootReservation,
            order: 0,
            protection: ProtectionClass::Reserved,
            alignment: 4096,
            page_rounded: true,
        },
        RegionRule {
            kind: RegionKind::Executable,
            order: 1,
            protection: ProtectionClass::ReadExecute,
            alignment: 4096,
            page_rounded: true,
        },
        RegionRule {
            kind: RegionKind::ImmutableData,
            order: 2,
            protection: ProtectionClass::ReadOnly,
            alignment: 4096,
            page_rounded: true,
        },
        RegionRule {
            kind: RegionKind::PerCoreMutable,
            order: 3,
            protection: ProtectionClass::ReadWrite,
            alignment: 4096,
            page_rounded: true,
        },
        RegionRule {
            kind: RegionKind::SharedMutable,
            order: 4,
            protection: ProtectionClass::ReadWrite,
            alignment: 4096,
            page_rounded: true,
        },
        RegionRule {
            kind: RegionKind::DmaOwned,
            order: 5,
            protection: ProtectionClass::DeviceOwned,
            alignment: 4096,
            page_rounded: true,
        },
    ])
}

fn canonical_bindings() -> Arc<[BindingSlot]> {
    Arc::from([
        BindingSlot {
            ordinal: 0,
            kind: BindingKind::Display,
        },
        BindingSlot {
            ordinal: 1,
            kind: BindingKind::Input,
        },
        BindingSlot {
            ordinal: 2,
            kind: BindingKind::EventStore,
        },
        BindingSlot {
            ordinal: 3,
            kind: BindingKind::MonotonicClock,
        },
        BindingSlot {
            ordinal: 4,
            kind: BindingKind::Entropy,
        },
        BindingSlot {
            ordinal: 5,
            kind: BindingKind::Telemetry,
        },
        BindingSlot {
            ordinal: 6,
            kind: BindingKind::Terminal,
        },
        BindingSlot {
            ordinal: 7,
            kind: BindingKind::Panic,
        },
    ])
}

fn verify(
    candidate: RawContract,
    context: ContractContext,
    cancellation: &Cancellation,
) -> Result<VerifiedArchitecturePlanningContract, ContractFailure> {
    if cancellation.is_cancelled() {
        return Err(ContractFailure::for_candidate(
            ContractFailureKind::Cancelled,
            ContractFailureKind::Cancelled.default_detail(),
            &candidate,
            context,
        ));
    }
    let Some(authentication) = candidate.authentication else {
        return Err(ContractFailure::for_candidate(
            ContractFailureKind::Unauthenticated,
            ContractFailureKind::Unauthenticated.default_detail(),
            &candidate,
            context,
        ));
    };
    if candidate.context != context {
        return Err(ContractFailure::for_candidate(
            ContractFailureKind::MixedContext,
            ContractFailureKind::MixedContext.default_detail(),
            &candidate,
            context,
        ));
    }
    if candidate.profile != ArchitectureProfile::CurrentAarch64 {
        return Err(ContractFailure::for_candidate(
            ContractFailureKind::UnsupportedProfile,
            ContractFailureKind::UnsupportedProfile.default_detail(),
            &candidate,
            context,
        ));
    }
    if candidate.schema_version != CONTRACT_SCHEMA_VERSION
        || candidate.version != CURRENT_AARCH64_CONTRACT_VERSION
    {
        return Err(ContractFailure::for_candidate(
            ContractFailureKind::UnsupportedVersion,
            ContractFailureKind::UnsupportedVersion.default_detail(),
            &candidate,
            context,
        ));
    }
    let expected_identity = canonical_identity(candidate.profile, candidate.version);
    if candidate.identity != expected_identity {
        return Err(ContractFailure::for_candidate(
            ContractFailureKind::IdentityMismatch,
            ContractFailureKind::IdentityMismatch.default_detail(),
            &candidate,
            context,
        ));
    }
    let expected_input_receipt =
        canonical_distribution_input_receipt(context, candidate.schema_version);
    if candidate.distribution_input_receipt != expected_input_receipt {
        return Err(ContractFailure::for_candidate(
            ContractFailureKind::InputReceiptMismatch,
            ContractFailureKind::InputReceiptMismatch.default_detail(),
            &candidate,
            context,
        ));
    }
    let facts = reconstruct_and_validate(&candidate.facts, cancellation)
        .map_err(|failure| failure.bind_candidate(&candidate, context))?;
    let expected = verifier_current_aarch64_facts();
    if facts != expected {
        return Err(ContractFailure::for_candidate(
            ContractFailureKind::MalformedFacts,
            "current AArch64 architecture planning facts differ from the authenticated contract",
            &candidate,
            context,
        ));
    }
    let expected_fingerprint = canonical_fingerprint(
        candidate.profile,
        candidate.schema_version,
        candidate.version,
        expected_identity,
        &facts,
    );
    if candidate.fingerprint != expected_fingerprint {
        return Err(ContractFailure::for_candidate(
            ContractFailureKind::FingerprintMismatch,
            ContractFailureKind::FingerprintMismatch.default_detail(),
            &candidate,
            context,
        ));
    }
    if authentication
        != authentication_tag(
            expected_input_receipt,
            expected_identity,
            expected_fingerprint,
        )
    {
        return Err(ContractFailure::for_candidate(
            ContractFailureKind::AuthenticationMismatch,
            ContractFailureKind::AuthenticationMismatch.default_detail(),
            &candidate,
            context,
        ));
    }
    Ok(VerifiedArchitecturePlanningContract {
        profile: candidate.profile,
        identity: expected_identity,
        schema_version: candidate.schema_version,
        version: candidate.version,
        fingerprint: expected_fingerprint,
        distribution_input_receipt: expected_input_receipt,
        facts,
        _verified: Verified,
    })
}

fn reconstruct_and_validate(
    source: &ContractFacts,
    cancellation: &Cancellation,
) -> Result<ContractFacts, ContractFailure> {
    if source.cores.is_empty() || source.cores[0].role != CoreRole::Primary {
        return Err(ContractFailure::malformed(
            "symbolic cores require one primary",
        ));
    }
    for (ordinal, core) in source.cores.iter().enumerate() {
        if cancellation.is_cancelled() {
            return Err(ContractFailure::cancelled());
        }
        if usize::from(core.ordinal) != ordinal || core.maximum_service_units == 0 {
            return Err(ContractFailure::malformed(
                "symbolic core order or capacity is invalid",
            ));
        }
        if ordinal > 0 && core.role != CoreRole::Secondary {
            return Err(ContractFailure::malformed("only core zero may be primary"));
        }
    }
    if source.capabilities.is_empty()
        || source
            .capabilities
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
    {
        return Err(ContractFailure::malformed(
            "VM ABI capabilities are not canonical",
        ));
    }
    if source.capacity.minimum_ram_bytes == 0
        || source.capacity.minimum_ram_bytes > source.capacity.maximum_ram_bytes
        || source.capacity.maximum_requirements == 0
        || source.capacity.maximum_allocations == 0
        || source.capacity.maximum_generated_roles == 0
        || source.capacity.maximum_activation_homes == 0
    {
        return Err(ContractFailure::malformed("capacity bounds are invalid"));
    }
    for alignment in [
        source.alignment.allocation_bytes,
        source.alignment.region_bytes,
        source.alignment.dma_bytes,
        source.alignment.maximum_envelope_bytes,
    ] {
        if !alignment.is_power_of_two() {
            return Err(ContractFailure::malformed(
                "alignment rule is not a power of two",
            ));
        }
    }
    if !source.page.quantum_bytes.is_power_of_two() || !source.page.round_each_region_once {
        return Err(ContractFailure::malformed("page rules are invalid"));
    }
    if source.guards.null_pages == 0
        || source.guards.normal_stack_before_pages == 0
        || source.guards.normal_stack_after_pages == 0
        || source.guards.interrupt_stack_before_pages == 0
        || source.guards.interrupt_stack_after_pages == 0
    {
        return Err(ContractFailure::malformed("guard rules are incomplete"));
    }
    if source.reservations.is_empty()
        || source
            .reservations
            .windows(2)
            .any(|pair| pair[0].kind >= pair[1].kind)
        || source.reservations.iter().any(|rule| {
            rule.bytes == 0
                || !rule.alignment.is_power_of_two()
                || rule.alignment > source.alignment.maximum_envelope_bytes
        })
    {
        return Err(ContractFailure::malformed("reservation rules are invalid"));
    }
    if source.envelopes.is_empty()
        || source
            .envelopes
            .windows(2)
            .any(|pair| pair[0].kind >= pair[1].kind)
        || source.envelopes.iter().any(|rule| {
            rule.maximum_bytes == 0
                || !rule.alignment.is_power_of_two()
                || rule.alignment > source.alignment.maximum_envelope_bytes
                || rule.dma_owned != (rule.region == RegionKind::DmaOwned)
                || rule.dma_owned != (rule.protection == ProtectionClass::DeviceOwned)
        })
    {
        return Err(ContractFailure::malformed(
            "Storage Envelope rules are invalid",
        ));
    }
    if source.regions.is_empty()
        || source.regions.iter().enumerate().any(|(order, region)| {
            usize::from(region.order) != order
                || !region.alignment.is_power_of_two()
                || !region.page_rounded
                || source.regions[..order]
                    .iter()
                    .any(|earlier| earlier.kind == region.kind)
        })
    {
        return Err(ContractFailure::malformed(
            "logical region rules are invalid",
        ));
    }
    if source.device_slots.is_empty()
        || source
            .device_slots
            .iter()
            .enumerate()
            .any(|(ordinal, slot)| usize::from(slot.ordinal) != ordinal)
        || source.binding_slots.is_empty()
        || source
            .binding_slots
            .iter()
            .enumerate()
            .any(|(ordinal, slot)| {
                usize::from(slot.ordinal) != ordinal
                    || source.binding_slots[..ordinal]
                        .iter()
                        .any(|earlier| earlier.kind == slot.kind)
            })
    {
        return Err(ContractFailure::malformed(
            "device or binding slots are invalid",
        ));
    }
    if source.interrupts.route_slots == 0
        || source.interrupts.maximum_sources_per_route == 0
        || source.interrupts.maximum_causes_per_service == 0
    {
        return Err(ContractFailure::malformed("interrupt limits are invalid"));
    }
    if source.dma.maximum_total_bytes == 0
        || source.dma.maximum_buffer_bytes == 0
        || source.dma.maximum_buffer_bytes > source.dma.maximum_total_bytes
        || !source.dma.required_alignment.is_power_of_two()
        || source.dma.required_alignment < source.alignment.dma_bytes
        || source.dma.maximum_in_flight == 0
    {
        return Err(ContractFailure::malformed("DMA limits are invalid"));
    }
    let service = source.service;
    if service.schema_version == 0
        || [
            service.ingress_units,
            service.actor_turn_units,
            service.group_child_units,
            service.driver_units,
            service.cleanup_units,
            service.cross_core_units,
            service.cancellation_checkpoint_units,
        ]
        .contains(&0)
        || service.maximum_cycle_units == 0
        || service.maximum_cancellation_delay_units > service.maximum_cycle_units
        || source
            .cores
            .iter()
            .map(|core| u128::from(core.maximum_service_units))
            .sum::<u128>()
            > u128::from(service.maximum_cycle_units)
    {
        return Err(ContractFailure::malformed(
            "scheduling-cost baseline is invalid",
        ));
    }

    Ok(ContractFacts {
        cores: Arc::from(source.cores.to_vec()),
        capabilities: Arc::from(source.capabilities.to_vec()),
        capacity: source.capacity,
        alignment: source.alignment,
        page: source.page,
        guards: source.guards,
        reservations: Arc::from(source.reservations.to_vec()),
        envelopes: Arc::from(source.envelopes.to_vec()),
        regions: Arc::from(source.regions.to_vec()),
        device_slots: Arc::from(source.device_slots.to_vec()),
        binding_slots: Arc::from(source.binding_slots.to_vec()),
        interrupts: source.interrupts,
        dma: source.dma,
        service: source.service,
    })
}

fn canonical_identity(profile: ArchitectureProfile, version: u16) -> u128 {
    let mut hasher = Xxh3::new();
    hasher.update(b"wrela.architecture-planning-contract.identity\0\x01");
    hasher.update(&[profile.tag()]);
    hash_bytes(&mut hasher, profile.canonical_name().as_bytes());
    hasher.update(&version.to_be_bytes());
    hasher.digest128()
}

fn canonical_fingerprint(
    profile: ArchitectureProfile,
    schema_version: u16,
    version: u16,
    identity: u128,
    facts: &ContractFacts,
) -> u128 {
    let mut hasher = Xxh3::new();
    hasher.update(b"wrela.architecture-planning-contract.fingerprint\0\x01");
    hasher.update(&[profile.tag()]);
    hasher.update(&schema_version.to_be_bytes());
    hasher.update(&version.to_be_bytes());
    hasher.update(&identity.to_be_bytes());
    hash_len(&mut hasher, facts.cores.len());
    for core in &*facts.cores {
        hasher.update(&core.ordinal.to_be_bytes());
        hasher.update(&[match core.role {
            CoreRole::Primary => 1,
            CoreRole::Secondary => 2,
        }]);
        hasher.update(&core.maximum_service_units.to_be_bytes());
    }
    hash_len(&mut hasher, facts.capabilities.len());
    for capability in &*facts.capabilities {
        hasher.update(&[enum_tag(*capability)]);
    }
    for value in [
        facts.capacity.minimum_ram_bytes,
        facts.capacity.maximum_ram_bytes,
    ] {
        hasher.update(&value.to_be_bytes());
    }
    for value in [
        facts.capacity.maximum_requirements,
        facts.capacity.maximum_allocations,
        facts.capacity.maximum_generated_roles,
        facts.capacity.maximum_activation_homes,
    ] {
        hasher.update(&value.to_be_bytes());
    }
    for value in [
        facts.alignment.allocation_bytes,
        facts.alignment.region_bytes,
        facts.alignment.dma_bytes,
        facts.alignment.maximum_envelope_bytes,
        facts.page.quantum_bytes,
    ] {
        hasher.update(&value.to_be_bytes());
    }
    hasher.update(&[u8::from(facts.page.round_each_region_once)]);
    for value in [
        facts.guards.null_pages,
        facts.guards.normal_stack_before_pages,
        facts.guards.normal_stack_after_pages,
        facts.guards.interrupt_stack_before_pages,
        facts.guards.interrupt_stack_after_pages,
    ] {
        hasher.update(&value.to_be_bytes());
    }
    hash_len(&mut hasher, facts.reservations.len());
    for rule in &*facts.reservations {
        hasher.update(&[reservation_tag(rule.kind), region_tag(rule.region)]);
        hasher.update(&rule.bytes.to_be_bytes());
        hasher.update(&rule.alignment.to_be_bytes());
        hasher.update(&[match rule.multiplicity {
            ReservationMultiplicity::Once => 1,
            ReservationMultiplicity::PerCore => 2,
        }]);
    }
    hash_len(&mut hasher, facts.envelopes.len());
    for rule in &*facts.envelopes {
        hasher.update(&[envelope_tag(rule.kind), region_tag(rule.region)]);
        hasher.update(&rule.maximum_bytes.to_be_bytes());
        hasher.update(&rule.alignment.to_be_bytes());
        hasher.update(&[protection_tag(rule.protection), u8::from(rule.dma_owned)]);
    }
    hash_len(&mut hasher, facts.regions.len());
    for region in &*facts.regions {
        hasher.update(&[
            region_tag(region.kind),
            region.order,
            protection_tag(region.protection),
        ]);
        hasher.update(&region.alignment.to_be_bytes());
        hasher.update(&[u8::from(region.page_rounded)]);
    }
    hash_len(&mut hasher, facts.device_slots.len());
    for slot in &*facts.device_slots {
        hasher.update(&[slot.ordinal]);
    }
    hash_len(&mut hasher, facts.binding_slots.len());
    for slot in &*facts.binding_slots {
        hasher.update(&[slot.ordinal, binding_tag(slot.kind)]);
    }
    hasher.update(&[
        facts.interrupts.route_slots,
        facts.interrupts.maximum_sources_per_route,
    ]);
    hasher.update(&facts.interrupts.maximum_causes_per_service.to_be_bytes());
    for value in [
        facts.dma.maximum_total_bytes,
        facts.dma.maximum_buffer_bytes,
        facts.dma.required_alignment,
    ] {
        hasher.update(&value.to_be_bytes());
    }
    hasher.update(&facts.dma.maximum_in_flight.to_be_bytes());
    hasher.update(&facts.service.schema_version.to_be_bytes());
    for value in [
        facts.service.ingress_units,
        facts.service.actor_turn_units,
        facts.service.group_child_units,
        facts.service.driver_units,
        facts.service.cleanup_units,
        facts.service.cross_core_units,
        facts.service.cancellation_checkpoint_units,
        facts.service.maximum_cycle_units,
        facts.service.maximum_cancellation_delay_units,
    ] {
        hasher.update(&value.to_be_bytes());
    }
    hasher.digest128()
}

fn canonical_distribution_input_receipt(
    context: ContractContext,
    contract_schema_version: u16,
) -> u128 {
    let mut hasher = Xxh3::new();
    hasher.update(b"wrela.architecture-planning-contract.distribution-input\0\x01");
    hash_bytes(&mut hasher, context.distribution_version.as_bytes());
    hasher.update(&context.distribution_digest.to_be_bytes());
    hasher.update(&contract_schema_version.to_be_bytes());
    hasher.digest128()
}

fn canonical_context_receipt(context: ContractContext) -> u128 {
    let mut hasher = Xxh3::new();
    hasher.update(b"wrela.architecture-planning-contract.context\0\x01");
    hash_bytes(&mut hasher, context.distribution_version.as_bytes());
    hasher.update(&context.distribution_digest.to_be_bytes());
    hasher.digest128()
}

fn authentication_tag(distribution_input_receipt: u128, identity: u128, fingerprint: u128) -> u128 {
    let mut hasher = Xxh3::new();
    hasher.update(b"wrela.compiler-distribution.architecture-planning-authentication\0\x01");
    hasher.update(&distribution_input_receipt.to_be_bytes());
    hasher.update(&identity.to_be_bytes());
    hasher.update(&fingerprint.to_be_bytes());
    hasher.digest128()
}

fn hash_bytes(hasher: &mut Xxh3, bytes: &[u8]) {
    hasher.update(&u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
    hasher.update(bytes);
}

fn hash_len(hasher: &mut Xxh3, len: usize) {
    hasher.update(&u64::try_from(len).unwrap_or(u64::MAX).to_be_bytes());
}

const fn enum_tag(value: VmAbiCapability) -> u8 {
    match value {
        VmAbiCapability::TypedTerminalLifecycle => 1,
        VmAbiCapability::PanicPulse => 2,
        VmAbiCapability::GuestShutdownPulse => 3,
        VmAbiCapability::PciVirtioModern => 4,
        VmAbiCapability::SplitVirtqueue => 5,
        VmAbiCapability::SharedIntx => 6,
        VmAbiCapability::DmaOwnership => 7,
        VmAbiCapability::SecondaryCoreStartup => 8,
    }
}
const fn reservation_tag(value: ReservationKind) -> u8 {
    match value {
        ReservationKind::DtbExclusion => 1,
        ReservationKind::BootState => 2,
        ReservationKind::PageTables => 3,
        ReservationKind::TerminalTransport => 4,
        ReservationKind::PanicState => 5,
        ReservationKind::PerCoreBootState => 6,
    }
}
const fn envelope_tag(value: EnvelopeKind) -> u8 {
    match value {
        EnvelopeKind::Executable => 1,
        EnvelopeKind::ImmutableData => 2,
        EnvelopeKind::PerCoreState => 3,
        EnvelopeKind::SharedState => 4,
        EnvelopeKind::DmaState => 5,
        EnvelopeKind::NormalStack => 6,
        EnvelopeKind::InterruptStack => 7,
        EnvelopeKind::AsyncFrame => 8,
    }
}
const fn region_tag(value: RegionKind) -> u8 {
    match value {
        RegionKind::BootReservation => 1,
        RegionKind::Executable => 2,
        RegionKind::ImmutableData => 3,
        RegionKind::PerCoreMutable => 4,
        RegionKind::SharedMutable => 5,
        RegionKind::DmaOwned => 6,
    }
}
const fn protection_tag(value: ProtectionClass) -> u8 {
    match value {
        ProtectionClass::Reserved => 1,
        ProtectionClass::ReadExecute => 2,
        ProtectionClass::ReadOnly => 3,
        ProtectionClass::ReadWrite => 4,
        ProtectionClass::DeviceOwned => 5,
    }
}
const fn binding_tag(value: BindingKind) -> u8 {
    match value {
        BindingKind::Display => 1,
        BindingKind::Input => 2,
        BindingKind::EventStore => 3,
        BindingKind::MonotonicClock => 4,
        BindingKind::Entropy => 5,
        BindingKind::Telemetry => 6,
        BindingKind::Terminal => 7,
        BindingKind::Panic => 8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(digest: u128) -> ContractContext {
        ContractContext::new("test-distribution-v1", digest)
    }

    fn candidate() -> RawContract {
        current_aarch64_candidate(context(7))
    }

    #[test]
    fn current_contract_is_complete_and_canonically_verified() {
        let candidate = candidate();
        let expected_receipt = candidate.distribution_input_receipt;
        let verified =
            verify(candidate, context(7), &Cancellation::new()).expect("current contract verifies");
        let _: &Verified = &verified._verified;
        assert_eq!(verified.schema_version, CONTRACT_SCHEMA_VERSION);
        assert_eq!(verified.distribution_input_receipt, expected_receipt);
        assert_eq!(verified.facts.cores.len(), 4);
        assert_eq!(verified.facts.capabilities.len(), 8);
        assert_eq!(verified.facts.reservations.len(), 6);
        assert_eq!(verified.facts.envelopes.len(), 8);
        assert_eq!(verified.facts.regions.len(), 6);
        assert_eq!(verified.facts.device_slots.len(), 8);
        assert_eq!(verified.facts.binding_slots.len(), 8);
        assert_eq!(verified.facts.capacity.minimum_ram_bytes, 134_217_728);
        assert_eq!(verified.facts.capacity.maximum_ram_bytes, 2_147_483_648);
        assert_eq!(verified.facts.alignment.allocation_bytes, 16);
        assert_eq!(verified.facts.alignment.region_bytes, 4096);
        assert_eq!(verified.facts.page.quantum_bytes, 4096);
        assert_eq!(verified.facts.guards.null_pages, 1);
        assert_eq!(verified.facts.interrupts.route_slots, 4);
        assert_eq!(verified.facts.dma.required_alignment, 4096);
        assert_eq!(verified.facts.service.schema_version, 1);
        assert_eq!(verified.facts.service.maximum_cycle_units, 1_000_000);
    }

    #[test]
    fn authentication_and_observation_use_the_same_verified_receipt_in_all_builds() {
        let candidate = candidate();
        let expected_authentication = authentication_tag(
            candidate.distribution_input_receipt,
            candidate.identity,
            candidate.fingerprint,
        );
        assert_eq!(candidate.authentication, Some(expected_authentication));

        let verified = verify(candidate, context(7), &Cancellation::new())
            .expect("the producer and verifier agree on authentication inputs");
        let observation = verified.observation();
        assert_eq!(
            observation.distribution_input_receipt(),
            verified.distribution_input_receipt
        );
        assert_eq!(
            observation.contract_schema_version(),
            verified.schema_version
        );
    }

    #[test]
    fn malformed_contract_is_rejected_deterministically() {
        let mut raw = candidate();
        raw.facts.page.quantum_bytes = 0;
        let failure = verify(raw, context(7), &Cancellation::new());
        let failure = failure.unwrap_err();
        assert_eq!(failure.kind(), ContractFailureKind::MalformedFacts);
        assert_eq!(failure.reproduction().detail(), "page rules are invalid");
    }

    #[test]
    fn unsupported_version_is_rejected_before_fingerprinting() {
        let mut raw = candidate();
        raw.version += 1;
        let failure = verify(raw, context(7), &Cancellation::new());
        assert_eq!(
            failure.unwrap_err().kind(),
            ContractFailureKind::UnsupportedVersion
        );
    }

    #[test]
    fn mixed_context_contract_is_rejected() {
        let failure = verify(candidate(), context(8), &Cancellation::new());
        assert_eq!(
            failure.unwrap_err().kind(),
            ContractFailureKind::MixedContext
        );
    }

    #[test]
    fn unauthenticated_contract_is_rejected() {
        let mut raw = candidate();
        raw.authentication = None;
        let failure = verify(raw, context(7), &Cancellation::new());
        assert_eq!(
            failure.unwrap_err().kind(),
            ContractFailureKind::Unauthenticated
        );
    }

    #[test]
    fn falsely_authenticated_contract_is_rejected() {
        let mut raw = candidate();
        raw.authentication = raw.authentication.map(|authentication| authentication ^ 1);
        let failure = verify(raw, context(7), &Cancellation::new());
        assert_eq!(
            failure.unwrap_err().kind(),
            ContractFailureKind::AuthenticationMismatch
        );
    }

    #[test]
    fn unsupported_profile_has_no_verified_contract() {
        let module = ArchitecturePlanningModule::new(context(7));
        assert_eq!(
            module
                .authenticate(ArchitectureProfile::X86_64, &Cancellation::new())
                .unwrap_err()
                .kind(),
            ContractFailureKind::UnsupportedProfile
        );
    }

    #[test]
    fn corrupted_identity_and_fingerprint_are_rejected() {
        let mut identity = candidate();
        identity.identity ^= 1;
        assert_eq!(
            verify(identity, context(7), &Cancellation::new())
                .unwrap_err()
                .kind(),
            ContractFailureKind::IdentityMismatch
        );

        let mut fingerprint = candidate();
        fingerprint.fingerprint ^= 1;
        assert_eq!(
            verify(fingerprint, context(7), &Cancellation::new())
                .unwrap_err()
                .kind(),
            ContractFailureKind::FingerprintMismatch
        );
    }

    #[test]
    fn cancellation_publishes_no_verified_contract() {
        let cancellation = Cancellation::new();
        cancellation.cancel();
        assert_eq!(
            verify(candidate(), context(7), &cancellation)
                .unwrap_err()
                .kind(),
            ContractFailureKind::Cancelled
        );
    }

    #[test]
    fn synthetic_facts_are_used_only_by_private_profile_neutral_verification() {
        let mut facts = producer_current_aarch64_facts();
        facts.cores = Arc::from([
            SymbolicCore {
                ordinal: 0,
                role: CoreRole::Primary,
                maximum_service_units: 100,
            },
            SymbolicCore {
                ordinal: 1,
                role: CoreRole::Secondary,
                maximum_service_units: 100,
            },
        ]);
        facts.service.maximum_cycle_units = 200;
        facts.service.maximum_cancellation_delay_units = 100;
        let reconstructed = reconstruct_and_validate(&facts, &Cancellation::new())
            .expect("profile-neutral rules accept a private synthetic contract");
        assert_eq!(reconstructed.cores.len(), 2);
    }

    #[test]
    fn profile_neutral_verification_rejects_duplicate_rule_kinds() {
        let mut facts = producer_current_aarch64_facts();
        Arc::make_mut(&mut facts.reservations)[2].kind = ReservationKind::BootState;
        let failure = reconstruct_and_validate(&facts, &Cancellation::new()).unwrap_err();
        assert_eq!(failure.kind(), ContractFailureKind::MalformedFacts);
        assert_eq!(
            failure.reproduction().detail(),
            "reservation rules are invalid"
        );
    }

    #[test]
    fn corrupted_service_capacity_rejects_without_host_overflow() {
        let mut facts = producer_current_aarch64_facts();
        for core in Arc::make_mut(&mut facts.cores) {
            core.maximum_service_units = u32::MAX;
        }
        let failure = reconstruct_and_validate(&facts, &Cancellation::new()).unwrap_err();
        assert_eq!(failure.kind(), ContractFailureKind::MalformedFacts);
        assert_eq!(
            failure.reproduction().detail(),
            "scheduling-cost baseline is invalid"
        );
    }

    #[test]
    fn contract_failures_preserve_bounded_reproduction_metadata() {
        fn assert_common(failure: &ContractFailure, kind: ContractFailureKind) {
            assert_eq!(failure.kind(), kind);
            let reproduction = failure.reproduction();
            assert_eq!(reproduction.profile(), ArchitectureProfile::CurrentAarch64);
            assert_eq!(reproduction.expected_schema_version(), 1);
            assert_eq!(reproduction.actual_schema_version(), 1);
            assert_ne!(reproduction.expected_context_receipt(), 0);
            assert_ne!(reproduction.actual_context_receipt(), 0);
            assert_ne!(reproduction.expected_input_receipt(), 0);
            assert_ne!(reproduction.actual_input_receipt(), 0);
            assert!(failure.bounded_evidence().len() < 768);
        }

        let mixed = verify(candidate(), context(8), &Cancellation::new()).unwrap_err();
        assert_common(&mixed, ContractFailureKind::MixedContext);
        assert_ne!(
            mixed.reproduction().expected_context_receipt(),
            mixed.reproduction().actual_context_receipt()
        );

        let mut identity = candidate();
        identity.identity ^= 1;
        let identity = verify(identity, context(7), &Cancellation::new()).unwrap_err();
        assert_common(&identity, ContractFailureKind::IdentityMismatch);

        let mut version = candidate();
        version.version = 2;
        let version = verify(version, context(7), &Cancellation::new()).unwrap_err();
        assert_common(&version, ContractFailureKind::UnsupportedVersion);
        assert_eq!(version.reproduction().expected_contract_version(), 1);
        assert_eq!(version.reproduction().actual_contract_version(), 2);

        let mut fingerprint = candidate();
        fingerprint.fingerprint ^= 1;
        let fingerprint = verify(fingerprint, context(7), &Cancellation::new()).unwrap_err();
        assert_common(&fingerprint, ContractFailureKind::FingerprintMismatch);

        let mut authentication = candidate();
        authentication.authentication = authentication
            .authentication
            .map(|authentication| authentication ^ 1);
        let authentication = verify(authentication, context(7), &Cancellation::new()).unwrap_err();
        assert_common(&authentication, ContractFailureKind::AuthenticationMismatch);

        let mut malformed = candidate();
        malformed.facts.page.quantum_bytes = 0;
        let malformed = verify(malformed, context(7), &Cancellation::new()).unwrap_err();
        assert_common(&malformed, ContractFailureKind::MalformedFacts);
        assert_eq!(malformed.reproduction().detail(), "page rules are invalid");
    }
}
