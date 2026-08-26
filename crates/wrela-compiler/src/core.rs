#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use xxhash_rust::xxh3::{Xxh3, xxh3_128};

use crate::completed_semantic::{
    CoreSourceExecutableBody, CoreSourceExecutableInput, CoreSourceExecutableKind,
};
use crate::image_planning::{CorePlanningInput, ExecutableRef, GeneratedRole};
use crate::model::{BuiltinVariant, IntegerType, SpecializationId, Type, TypeId};
use crate::typed_hir::{
    BinaryOperator, CallTarget, Expression, ExpressionKind, HirMatchPattern, Literal, Place,
    PlaceProjection, PoolOperation, Statement, root_place,
};
use crate::{Cancellation, CanonicalValue, EvaluationOutcome, EvaluationPanicKind, SourceRange};

pub(crate) const PHASE_SCHEMA: &str = "wrela.core.v3";
const SCHEMA_VERSION: u16 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CoreExecutableKind {
    SourceSpecialization,
    SourceTestBody,
    SourceClosureBody,
    Generated,
}

impl CoreExecutableKind {
    const fn tag(self) -> u8 {
        match self {
            Self::SourceSpecialization => 1,
            Self::SourceTestBody => 2,
            Self::SourceClosureBody => 3,
            Self::Generated => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CoreOperationKind {
    Literal,
    Read,
    Constant,
    FunctionValue,
    ClosureValue,
    Call,
    BuildConstruction,
    Aggregate,
    Index,
    Unary,
    CheckedArithmetic,
    Binary,
    ShortCircuit,
    Propagate,
    PatternTest,
    Store,
    Return,
    TerminalPanic,
    Assert,
    Expect,
    Branch,
    Loop,
    LoopBack,
    Match,
    Cleanup,
    PoolScope,
    Break,
    Continue,
    Pass,
    Suspension,
    GeneratedRole,
    RecoverableExit,
    CleanupRun,
    MessageProposal,
}

impl CoreOperationKind {
    const fn tag(self) -> u8 {
        match self {
            Self::Literal => 1,
            Self::Read => 2,
            Self::Constant => 3,
            Self::FunctionValue => 4,
            Self::ClosureValue => 5,
            Self::Call => 6,
            Self::BuildConstruction => 7,
            Self::Aggregate => 8,
            Self::Index => 9,
            Self::Unary => 10,
            Self::CheckedArithmetic => 11,
            Self::Binary => 12,
            Self::ShortCircuit => 13,
            Self::Propagate => 14,
            Self::PatternTest => 15,
            Self::Store => 16,
            Self::Return => 17,
            Self::TerminalPanic => 18,
            Self::Assert => 19,
            Self::Expect => 20,
            Self::Branch => 21,
            Self::Loop => 22,
            Self::LoopBack => 23,
            Self::Match => 24,
            Self::Cleanup => 25,
            Self::PoolScope => 26,
            Self::Break => 27,
            Self::Continue => 28,
            Self::Pass => 29,
            Self::Suspension => 30,
            Self::GeneratedRole => 31,
            Self::RecoverableExit => 32,
            Self::CleanupRun => 33,
            Self::MessageProposal => 34,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CoreCustodyOperation {
    Construct,
    Move,
    SharedLoan,
    ExclusiveLoan,
    Reinitialize,
    Replace,
    TransferCommit,
    Discharge,
    CleanupRegister,
    CleanupRun,
    Join,
    LoopFixpoint,
    ProofCondition,
    Panic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreInitializationEffect {
    None,
    Initialize,
    Uninitialize,
    Reinitialize,
    Replace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreCustodianEffect {
    None,
    Establish,
    Transfer,
    Discharge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreLoanEffect {
    None,
    Shared,
    Exclusive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreObligationEffect {
    None,
    Establish,
    Transfer,
    Discharge,
}

impl CoreCustodyOperation {
    const fn tag(self) -> u8 {
        match self {
            Self::Construct => 1,
            Self::Move => 2,
            Self::SharedLoan => 3,
            Self::ExclusiveLoan => 4,
            Self::Reinitialize => 5,
            Self::Replace => 6,
            Self::TransferCommit => 7,
            Self::Discharge => 8,
            Self::CleanupRegister => 9,
            Self::CleanupRun => 10,
            Self::Join => 11,
            Self::LoopFixpoint => 12,
            Self::ProofCondition => 13,
            Self::Panic => 14,
        }
    }
}

impl CoreInitializationEffect {
    const fn tag(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Initialize => 1,
            Self::Uninitialize => 2,
            Self::Reinitialize => 3,
            Self::Replace => 4,
        }
    }
}

impl CoreCustodianEffect {
    const fn tag(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Establish => 1,
            Self::Transfer => 2,
            Self::Discharge => 3,
        }
    }
}

impl CoreLoanEffect {
    const fn tag(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Shared => 1,
            Self::Exclusive => 2,
        }
    }
}

impl CoreObligationEffect {
    const fn tag(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Establish => 1,
            Self::Transfer => 2,
            Self::Discharge => 3,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProofCondition {
    requirement_identity: u128,
    requirement_current_meaning: u128,
    source_type_identity: u128,
    retains_source_return_type: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CustodyEffect {
    operation: CoreCustodyOperation,
    initialization: CoreInitializationEffect,
    custodian: CoreCustodianEffect,
    loan: CoreLoanEffect,
    obligation: CoreObligationEffect,
    place: Arc<[u128]>,
    type_identity: Option<u128>,
    source_home: Option<u128>,
    destination_home: Option<u128>,
    cleanup_ordinal: Option<u32>,
    proof: Option<ProofCondition>,
}

type LivePlaces = BTreeMap<Arc<[u128]>, u128>;
type ResourceComponents = BTreeMap<u128, Vec<(Arc<[u128]>, u128)>>;
type SourceRootDischarges = BTreeMap<(Arc<[u128]>, u128), usize>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CoreAccessLaw {
    None,
    CopyValue,
    SharedLoan,
    ExclusiveLoan,
    Move,
    CleanupCapture,
}

impl CoreAccessLaw {
    const fn tag(self) -> u8 {
        match self {
            Self::None => 0,
            Self::CopyValue => 1,
            Self::SharedLoan => 2,
            Self::ExclusiveLoan => 3,
            Self::Move => 4,
            Self::CleanupCapture => 5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CoreRewriteKind {
    EliminatedPass,
}

impl CoreRewriteKind {
    const fn tag(self) -> u8 {
        match self {
            Self::EliminatedPass => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EffectBoundary {
    None,
    LocalMutation,
    Ownership,
    BuildConstruction,
    Suspension,
    Call,
}

impl EffectBoundary {
    const fn tag(self) -> u8 {
        match self {
            Self::None => 0,
            Self::LocalMutation => 1,
            Self::Ownership => 2,
            Self::BuildConstruction => 3,
            Self::Suspension => 4,
            Self::Call => 5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FailureLaw {
    None,
    CheckBeforeSuccess,
    PropagateInOrder,
    TerminalPanic,
    RecordTestFailure,
    ArithmeticPanic,
    BoundsPanic,
    CallPanicPropagation,
}

impl FailureLaw {
    const fn tag(self) -> u8 {
        match self {
            Self::None => 0,
            Self::CheckBeforeSuccess => 1,
            Self::PropagateInOrder => 2,
            Self::TerminalPanic => 3,
            Self::RecordTestFailure => 4,
            Self::ArithmeticPanic => 5,
            Self::BoundsPanic => 6,
            Self::CallPanicPropagation => 7,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct RegionId(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ValueId(u32);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Operation {
    identity: u32,
    kind: CoreOperationKind,
    result: Option<ValueId>,
    type_identity: Option<u128>,
    operands: Arc<[ValueId]>,
    call_binding: Arc<[u16]>,
    successors: Arc<[RegionId]>,
    details: Arc<[u128]>,
    effect: EffectBoundary,
    access: CoreAccessLaw,
    failure: FailureLaw,
    provenance: SourceRange,
    custody: Arc<[CustodyEffect]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CoreType {
    identity: u128,
    shape: Arc<[u8]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CoreParameter {
    local: u32,
    type_: CoreType,
    access: CoreAccessLaw,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CoreSignature {
    parameters: Arc<[CoreParameter]>,
    return_type: CoreType,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RewriteWitness {
    kind: CoreRewriteKind,
    provenance: SourceRange,
    source_order: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Region {
    identity: RegionId,
    operations: Arc<[Operation]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ExecutableReference {
    context: u128,
    kind: CoreExecutableKind,
    identity: u128,
    current_meaning: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ExecutableFacts {
    pure: bool,
    may_panic: bool,
    suspends: bool,
    ownership_transfer: bool,
    evaluator_eligible: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CoreExecutable {
    reference: ExecutableReference,
    semantic_owner: u128,
    provenance: SourceRange,
    entry: RegionId,
    regions: Arc<[Region]>,
    facts: ExecutableFacts,
    signature: CoreSignature,
    rewrites: Arc<[RewriteWitness]>,
    source_definition: Option<u128>,
    fingerprint: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OracleSummary {
    cases: usize,
    agrees: bool,
    custody_cases: usize,
    custody_agrees: bool,
    fingerprint: u128,
}

#[derive(Clone)]
pub(crate) struct VerifiedCoreProgram {
    context: u128,
    planning_fingerprint: u128,
    source_demand_identity: u128,
    executables: Arc<[CoreExecutable]>,
    oracle: OracleSummary,
    fingerprint: u128,
    _verified: Verified,
}

#[derive(Clone, Debug)]
struct Verified;

impl fmt::Debug for VerifiedCoreProgram {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedCoreProgram")
            .field("context", &format_args!("{:032x}", self.context))
            .field("fingerprint", &format_args!("{:032x}", self.fingerprint))
            .finish_non_exhaustive()
    }
}

impl PartialEq for VerifiedCoreProgram {
    fn eq(&self, other: &Self) -> bool {
        self.context == other.context && self.fingerprint == other.fingerprint
    }
}

impl Eq for VerifiedCoreProgram {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreExecutableObservation {
    kind: CoreExecutableKind,
    identity: u128,
    current_meaning: u128,
    semantic_owner: u128,
    operations: Arc<[CoreOperationKind]>,
    region_count: usize,
    may_panic: bool,
    effectful: bool,
    parameters: Arc<[CoreParameterObservation]>,
    return_type_identity: u128,
    operation_type_identities: Arc<[u128]>,
    access_laws: Arc<[CoreAccessLaw]>,
    rewrites: Arc<[CoreRewriteKind]>,
    custody_effects: Arc<[CoreCustodyEffectObservation]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreCustodyEffectObservation {
    semantic_identity: u128,
    provenance: SourceRange,
    operation: CoreCustodyOperation,
    initialization: CoreInitializationEffect,
    custodian: CoreCustodianEffect,
    loan: CoreLoanEffect,
    obligation: CoreObligationEffect,
    place_projection_count: usize,
    type_identity: Option<u128>,
    custody_continuous: bool,
    cleanup_identity: Option<u128>,
    cleanup_has_success_continuation: bool,
    cleanup_action_may_panic: bool,
    requirement_identity: Option<u128>,
    requirement_current_meaning: Option<u128>,
    source_type_identity: Option<u128>,
    retains_source_return_type: bool,
}

impl CoreCustodyEffectObservation {
    #[must_use]
    pub const fn semantic_identity(&self) -> u128 {
        self.semantic_identity
    }

    #[must_use]
    pub const fn provenance(&self) -> &SourceRange {
        &self.provenance
    }

    #[must_use]
    pub const fn operation(&self) -> CoreCustodyOperation {
        self.operation
    }

    #[must_use]
    pub const fn initialization(&self) -> CoreInitializationEffect {
        self.initialization
    }

    #[must_use]
    pub const fn custodian(&self) -> CoreCustodianEffect {
        self.custodian
    }

    #[must_use]
    pub const fn loan(&self) -> CoreLoanEffect {
        self.loan
    }

    #[must_use]
    pub const fn obligation(&self) -> CoreObligationEffect {
        self.obligation
    }

    #[must_use]
    pub const fn place_projection_count(&self) -> usize {
        self.place_projection_count
    }

    #[must_use]
    pub const fn type_identity(&self) -> Option<u128> {
        self.type_identity
    }

    #[must_use]
    pub const fn custody_continuous(&self) -> bool {
        self.custody_continuous
    }

    #[must_use]
    pub const fn cleanup_identity(&self) -> Option<u128> {
        self.cleanup_identity
    }

    #[must_use]
    pub const fn cleanup_has_success_continuation(&self) -> bool {
        self.cleanup_has_success_continuation
    }

    #[must_use]
    pub const fn cleanup_action_may_panic(&self) -> bool {
        self.cleanup_action_may_panic
    }

    #[must_use]
    pub const fn requirement_identity(&self) -> Option<u128> {
        self.requirement_identity
    }

    #[must_use]
    pub const fn requirement_current_meaning(&self) -> Option<u128> {
        self.requirement_current_meaning
    }

    #[must_use]
    pub const fn source_type_identity(&self) -> Option<u128> {
        self.source_type_identity
    }

    #[must_use]
    pub const fn retains_source_return_type(&self) -> bool {
        self.retains_source_return_type
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreParameterObservation {
    local_identity: u32,
    type_identity: u128,
    access: CoreAccessLaw,
}

impl CoreParameterObservation {
    #[must_use]
    pub const fn local_identity(&self) -> u32 {
        self.local_identity
    }

    #[must_use]
    pub const fn type_identity(&self) -> u128 {
        self.type_identity
    }

    #[must_use]
    pub const fn access(&self) -> CoreAccessLaw {
        self.access
    }
}

impl CoreExecutableObservation {
    #[must_use]
    pub const fn kind(&self) -> CoreExecutableKind {
        self.kind
    }

    #[must_use]
    pub const fn identity(&self) -> u128 {
        self.identity
    }

    #[must_use]
    pub const fn current_meaning(&self) -> u128 {
        self.current_meaning
    }

    #[must_use]
    pub const fn semantic_owner(&self) -> u128 {
        self.semantic_owner
    }

    #[must_use]
    pub fn operations(&self) -> &[CoreOperationKind] {
        &self.operations
    }

    #[must_use]
    pub const fn region_count(&self) -> usize {
        self.region_count
    }

    #[must_use]
    pub const fn may_panic(&self) -> bool {
        self.may_panic
    }

    #[must_use]
    pub const fn effectful(&self) -> bool {
        self.effectful
    }

    #[must_use]
    pub fn parameters(&self) -> &[CoreParameterObservation] {
        &self.parameters
    }

    #[must_use]
    pub const fn return_type_identity(&self) -> u128 {
        self.return_type_identity
    }

    #[must_use]
    pub fn operation_type_identities(&self) -> &[u128] {
        &self.operation_type_identities
    }

    #[must_use]
    pub fn access_laws(&self) -> &[CoreAccessLaw] {
        &self.access_laws
    }

    #[must_use]
    pub fn rewrites(&self) -> &[CoreRewriteKind] {
        &self.rewrites
    }

    #[must_use]
    pub fn custody_effects(&self) -> &[CoreCustodyEffectObservation] {
        &self.custody_effects
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreProgramObservation {
    fingerprint: u128,
    context_identity: u128,
    planning_foundation_fingerprint: u128,
    executables: Arc<[CoreExecutableObservation]>,
    oracle_case_count: usize,
    oracle_agrees: bool,
    custody_oracle_case_count: usize,
    custody_oracle_agrees: bool,
}

impl CoreProgramObservation {
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
    pub fn executables(&self) -> &[CoreExecutableObservation] {
        &self.executables
    }

    #[must_use]
    pub const fn oracle_case_count(&self) -> usize {
        self.oracle_case_count
    }

    #[must_use]
    pub const fn oracle_agrees(&self) -> bool {
        self.oracle_agrees
    }

    #[must_use]
    pub const fn custody_oracle_case_count(&self) -> usize {
        self.custody_oracle_case_count
    }

    #[must_use]
    pub const fn custody_oracle_agrees(&self) -> bool {
        self.custody_oracle_agrees
    }
}

impl VerifiedCoreProgram {
    pub(crate) const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }

    pub(crate) fn observation(
        &self,
        cancellation: &Cancellation,
    ) -> Result<CoreProgramObservation, CoreFailure> {
        let mut executables = Vec::with_capacity(self.executables.len());
        for executable in self.executables.iter() {
            checkpoint(cancellation)?;
            let mut operations = Vec::new();
            let mut operation_type_identities = Vec::new();
            let mut access_laws = Vec::new();
            for region in executable.regions.iter() {
                checkpoint(cancellation)?;
                for operation in region.operations.iter() {
                    checkpoint(cancellation)?;
                    operations.push(operation.kind);
                    if let Some(type_identity) = operation.type_identity {
                        operation_type_identities.push(type_identity);
                    }
                    if operation.access != CoreAccessLaw::None {
                        access_laws.push(operation.access);
                    }
                }
            }
            executables.push(CoreExecutableObservation {
                kind: executable.reference.kind,
                identity: executable.reference.identity,
                current_meaning: executable.reference.current_meaning,
                semantic_owner: executable.semantic_owner,
                operations: operations.into(),
                region_count: executable.regions.len(),
                may_panic: executable.facts.may_panic,
                effectful: !executable.facts.pure,
                parameters: executable
                    .signature
                    .parameters
                    .iter()
                    .map(|parameter| CoreParameterObservation {
                        local_identity: parameter.local,
                        type_identity: parameter.type_.identity,
                        access: parameter.access,
                    })
                    .collect::<Vec<_>>()
                    .into(),
                return_type_identity: executable.signature.return_type.identity,
                operation_type_identities: operation_type_identities.into(),
                access_laws: access_laws.into(),
                rewrites: executable
                    .rewrites
                    .iter()
                    .map(|witness| witness.kind)
                    .collect::<Vec<_>>()
                    .into(),
                custody_effects: observe_executable_custody(
                    executable,
                    &self.executables,
                    cancellation,
                )?,
            });
        }
        Ok(CoreProgramObservation {
            fingerprint: self.fingerprint,
            context_identity: self.context,
            planning_foundation_fingerprint: self.planning_fingerprint,
            executables: executables.into(),
            oracle_case_count: self.oracle.cases,
            oracle_agrees: self.oracle.agrees,
            custody_oracle_case_count: self.oracle.custody_cases,
            custody_oracle_agrees: self.oracle.custody_agrees,
        })
    }

    #[allow(dead_code)]
    pub(crate) fn for_custody(&self) -> CustodyCoreView<'_> {
        CustodyCoreView { core: self }
    }

    #[allow(dead_code)]
    pub(crate) fn for_flow(&self) -> FlowCoreView<'_> {
        FlowCoreView { core: self }
    }

    #[allow(dead_code)]
    pub(crate) fn for_backend(&self) -> BackendCoreView<'_> {
        BackendCoreView { core: self }
    }
}

fn observation_value_subject(
    value: ValueId,
    producers: &BTreeMap<ValueId, &Operation>,
    memo: &mut BTreeMap<ValueId, u128>,
    visiting: &mut BTreeSet<ValueId>,
    cancellation: &Cancellation,
) -> Result<u128, CoreFailure> {
    checkpoint(cancellation)?;
    if let Some(subject) = memo.get(&value) {
        return Ok(*subject);
    }
    if !visiting.insert(value) {
        return Ok(0);
    }
    let mut hash = Xxh3::new();
    hash.update(b"wrela.core.value-observation-subject\0\x01");
    if let Some(operation) = producers.get(&value) {
        hash.update(&[
            operation.kind.tag(),
            operation.access.tag(),
            operation.effect.tag(),
        ]);
        hash.update(&operation.type_identity.unwrap_or(0).to_be_bytes());
        let skip = usize::from(operation.kind == CoreOperationKind::Read);
        for detail in operation.details.iter().skip(skip) {
            hash.update(&detail.to_be_bytes());
        }
        for operand in operation.operands.iter().copied() {
            hash.update(
                &observation_value_subject(operand, producers, memo, visiting, cancellation)?
                    .to_be_bytes(),
            );
        }
    }
    visiting.remove(&value);
    let subject = hash.digest128();
    memo.insert(value, subject);
    Ok(subject)
}

fn observation_control_paths(
    executable: &CoreExecutable,
    cancellation: &Cancellation,
) -> Result<BTreeMap<u32, Arc<[u32]>>, CoreFailure> {
    fn visit(
        executable: &CoreExecutable,
        region: RegionId,
        path: Vec<u32>,
        region_paths: &mut BTreeMap<RegionId, Arc<[u32]>>,
        operation_paths: &mut BTreeMap<u32, Arc<[u32]>>,
        cancellation: &Cancellation,
    ) -> Result<(), CoreFailure> {
        checkpoint(cancellation)?;
        if region_paths.contains_key(&region) {
            return Ok(());
        }
        region_paths.insert(region, Arc::clone(&Arc::from(path.clone())));
        let Some(region) = executable.regions.get(region.0 as usize) else {
            return Ok(());
        };
        for operation in region.operations.iter() {
            operation_paths.insert(operation.identity, Arc::from(path.clone()));
            for (ordinal, successor) in operation.successors.iter().copied().enumerate() {
                let mut successor_path = path.clone();
                successor_path.extend([
                    u32::from(operation.kind.tag()),
                    u32::try_from(ordinal).unwrap_or(u32::MAX),
                ]);
                visit(
                    executable,
                    successor,
                    successor_path,
                    region_paths,
                    operation_paths,
                    cancellation,
                )?;
            }
        }
        Ok(())
    }
    let mut regions = BTreeMap::new();
    let mut operations = BTreeMap::new();
    visit(
        executable,
        RegionId(0),
        Vec::new(),
        &mut regions,
        &mut operations,
        cancellation,
    )?;
    for region in executable.regions.iter() {
        if !regions.contains_key(&region.identity) {
            visit(
                executable,
                region.identity,
                vec![u32::MAX],
                &mut regions,
                &mut operations,
                cancellation,
            )?;
        }
    }
    Ok(operations)
}

fn observe_executable_custody(
    executable: &CoreExecutable,
    executables: &[CoreExecutable],
    cancellation: &Cancellation,
) -> Result<Arc<[CoreCustodyEffectObservation]>, CoreFailure> {
    let control_paths = observation_control_paths(executable, cancellation)?;
    let producers = executable
        .regions
        .iter()
        .flat_map(|region| region.operations.iter())
        .filter_map(|operation| Some((operation.result?, operation)))
        .collect::<BTreeMap<_, _>>();
    let mut value_subjects = BTreeMap::new();
    for value in producers.keys().copied().collect::<Vec<_>>() {
        observation_value_subject(
            value,
            &producers,
            &mut value_subjects,
            &mut BTreeSet::new(),
            cancellation,
        )?;
    }
    let mut value_uses = BTreeMap::<ValueId, Vec<u128>>::new();
    for consumer in executable
        .regions
        .iter()
        .flat_map(|region| region.operations.iter())
    {
        checkpoint(cancellation)?;
        for (ordinal, operand) in consumer.operands.iter().copied().enumerate() {
            let mut hash = Xxh3::new();
            hash.update(b"wrela.core.value-use-subject\0\x01");
            hash.update(&[consumer.kind.tag(), consumer.access.tag()]);
            hash.update(&(ordinal as u64).to_be_bytes());
            let skip = usize::from(matches!(
                consumer.kind,
                CoreOperationKind::Read | CoreOperationKind::Store
            ));
            for detail in consumer.details.iter().skip(skip) {
                hash.update(&detail.to_be_bytes());
            }
            value_uses
                .entry(operand)
                .or_default()
                .push(hash.digest128());
        }
    }
    let mut local_subjects = executable
        .signature
        .parameters
        .iter()
        .enumerate()
        .map(|(ordinal, parameter)| {
            let mut hash = Xxh3::new();
            hash.update(b"wrela.core.parameter-custody-subject\0\x01");
            hash.update(&executable.reference.identity.to_be_bytes());
            hash.update(&(ordinal as u64).to_be_bytes());
            hash.update(&parameter.type_.identity.to_be_bytes());
            (u128::from(parameter.local), hash.digest128())
        })
        .collect::<BTreeMap<_, _>>();
    let cleanup_sources = executable
        .regions
        .iter()
        .flat_map(|region| region.operations.iter())
        .filter(|operation| operation.kind == CoreOperationKind::Cleanup)
        .map(|operation| {
            let mut hash = Xxh3::new();
            hash.update(b"wrela.core.cleanup-observation\0\x02");
            hash.update(&executable.reference.identity.to_be_bytes());
            hash.update(operation.provenance.path().as_bytes());
            hash.update(
                &operation
                    .provenance
                    .end()
                    .saturating_sub(operation.provenance.start())
                    .to_be_bytes(),
            );
            hash.update(&(operation.operands.len() as u64).to_be_bytes());
            hash.update(&(operation.successors.len() as u64).to_be_bytes());
            if let Some(action) = operation.successors.first()
                && let Some(action) = executable.regions.get(action.0 as usize)
            {
                for action_operation in action.operations.iter() {
                    hash.update(&[action_operation.kind.tag(), action_operation.access.tag()]);
                    hash.update(&action_operation.type_identity.unwrap_or(0).to_be_bytes());
                    let skip = usize::from(matches!(
                        action_operation.kind,
                        CoreOperationKind::Read | CoreOperationKind::Store
                    ));
                    for detail in action_operation.details.iter().skip(skip) {
                        hash.update(&detail.to_be_bytes());
                    }
                    for operand in action_operation.operands.iter() {
                        hash.update(
                            &value_subjects
                                .get(operand)
                                .copied()
                                .unwrap_or(0)
                                .to_be_bytes(),
                        );
                    }
                }
            }
            (operation.identity, hash.digest128())
        })
        .collect::<BTreeMap<_, _>>();
    let mut owned_places = executable
        .signature
        .parameters
        .iter()
        .filter(|parameter| parameter.access == CoreAccessLaw::Move)
        .map(|parameter| Arc::<[u128]>::from([u128::from(parameter.local)]))
        .collect::<BTreeSet<_>>();
    let mut moved_components = BTreeSet::<Arc<[u128]>>::new();
    let mut known_homes = owned_places
        .iter()
        .map(|place| place_home(place))
        .collect::<BTreeSet<_>>();
    let mut observations = Vec::new();
    let mut semantic_identities = BTreeSet::new();
    for operation in executable
        .regions
        .iter()
        .flat_map(|region| region.operations.iter())
    {
        checkpoint(cancellation)?;
        for (effect_role, effect) in operation.custody.iter().enumerate() {
            checkpoint(cancellation)?;
            let cleanup_identity = effect
                .cleanup_ordinal
                .and_then(|ordinal| cleanup_sources.get(&ordinal).copied());
            let derived_component = !effect.place.is_empty()
                && effect.source_home == Some(place_home(&effect.place))
                && owned_places
                    .iter()
                    .any(|root| place_is_prefix(root, &effect.place))
                && !moved_components
                    .iter()
                    .any(|moved| places_overlap(moved, &effect.place));
            let custody_continuous = effect.source_home.is_none_or(|source| {
                known_homes.contains(&source)
                    || derived_component
                    || effect.custodian == CoreCustodianEffect::Establish
                    || effect.operation == CoreCustodyOperation::CleanupRun
            });
            if let Some(source) = effect.source_home
                && matches!(
                    effect.custodian,
                    CoreCustodianEffect::Transfer | CoreCustodianEffect::Discharge
                )
            {
                known_homes.remove(&source);
            }
            if matches!(
                effect.operation,
                CoreCustodyOperation::Move | CoreCustodyOperation::Discharge
            ) && !effect.place.is_empty()
            {
                if owned_places.remove(&effect.place) {
                    moved_components.retain(|place| !place_is_prefix(&effect.place, place));
                } else if owned_places
                    .iter()
                    .any(|root| place_is_prefix(root, &effect.place))
                {
                    moved_components.insert(Arc::clone(&effect.place));
                }
            }
            if let Some(destination) = effect.destination_home
                && effect.custodian != CoreCustodianEffect::Discharge
            {
                known_homes.insert(destination);
            }
            if matches!(
                effect.operation,
                CoreCustodyOperation::TransferCommit | CoreCustodyOperation::Reinitialize
            ) && !effect.place.is_empty()
            {
                if !owned_places
                    .iter()
                    .any(|root| place_is_prefix(root, &effect.place))
                {
                    owned_places.insert(Arc::clone(&effect.place));
                }
                moved_components.retain(|moved| !places_overlap(moved, &effect.place));
            }
            let mut hash = Xxh3::new();
            hash.update(b"wrela.core.custody-observation\0\x02");
            hash.update(&executable.reference.identity.to_be_bytes());
            hash.update(&[
                operation.kind.tag(),
                operation.access.tag(),
                effect.operation.tag(),
                effect.initialization.tag(),
                effect.custodian.tag(),
                effect.loan.tag(),
                effect.obligation.tag(),
            ]);
            hash.update(&effect.type_identity.unwrap_or(0).to_be_bytes());
            hash.update(&(effect_role as u64).to_be_bytes());
            if let Some(path) = control_paths.get(&operation.identity) {
                hash.update(&(path.len() as u64).to_be_bytes());
                for part in path.iter() {
                    hash.update(&part.to_be_bytes());
                }
            }
            hash.update(operation.provenance.path().as_bytes());
            hash.update(
                &operation
                    .provenance
                    .end()
                    .saturating_sub(operation.provenance.start())
                    .to_be_bytes(),
            );
            hash.update(&(effect.place.len().saturating_sub(1) as u64).to_be_bytes());
            for projection in effect.place.iter().skip(1) {
                hash.update(&projection.to_be_bytes());
            }
            if let Some(local) = effect.place.first()
                && let Some(subject) = local_subjects.get(local)
            {
                hash.update(&subject.to_be_bytes());
            }
            for operand in operation.operands.iter() {
                hash.update(
                    &value_subjects
                        .get(operand)
                        .copied()
                        .unwrap_or(0)
                        .to_be_bytes(),
                );
            }
            for successor in operation.successors.iter() {
                if let Some(successor) = executable.regions.get(successor.0 as usize) {
                    for successor_operation in successor.operations.iter() {
                        hash.update(&[
                            successor_operation.kind.tag(),
                            successor_operation.access.tag(),
                        ]);
                        hash.update(&successor_operation.type_identity.unwrap_or(0).to_be_bytes());
                        let skip = usize::from(matches!(
                            successor_operation.kind,
                            CoreOperationKind::Read | CoreOperationKind::Store
                        ));
                        for detail in successor_operation.details.iter().skip(skip) {
                            hash.update(&detail.to_be_bytes());
                        }
                    }
                }
            }
            if let Some(result) = operation.result
                && let Some(uses) = value_uses.get(&result)
            {
                for use_subject in uses {
                    hash.update(&use_subject.to_be_bytes());
                }
            }
            hash.update(&cleanup_identity.unwrap_or(0).to_be_bytes());
            let action_may_panic = (operation.kind == CoreOperationKind::CleanupRun)
                .then(|| operation.successors.first())
                .flatten()
                .and_then(|action| executable.regions.get(action.0 as usize))
                .is_some_and(|region| {
                    region.operations.iter().any(|action| match action.failure {
                        FailureLaw::TerminalPanic
                        | FailureLaw::ArithmeticPanic
                        | FailureLaw::BoundsPanic => true,
                        FailureLaw::CallPanicPropagation => direct_call_target(action)
                            .and_then(|target| {
                                executables.iter().find(|candidate| {
                                    candidate.reference.kind
                                        == CoreExecutableKind::SourceSpecialization
                                        && candidate.reference.identity == target
                                })
                            })
                            .is_none_or(|target| target.facts.may_panic),
                        FailureLaw::None
                        | FailureLaw::CheckBeforeSuccess
                        | FailureLaw::PropagateInOrder
                        | FailureLaw::RecordTestFailure => false,
                    })
                });
            let semantic_identity = hash.digest128();
            if !semantic_identities.insert(semantic_identity) {
                return defect("Core custody observation identity collision");
            }
            observations.push(CoreCustodyEffectObservation {
                semantic_identity,
                provenance: operation.provenance.clone(),
                operation: effect.operation,
                initialization: effect.initialization,
                custodian: effect.custodian,
                loan: effect.loan,
                obligation: effect.obligation,
                place_projection_count: effect.place.len().saturating_sub(1),
                type_identity: effect.type_identity,
                custody_continuous,
                cleanup_identity,
                cleanup_has_success_continuation: operation.kind == CoreOperationKind::CleanupRun
                    && operation.successors.len() == 2,
                cleanup_action_may_panic: action_may_panic,
                requirement_identity: effect
                    .proof
                    .as_ref()
                    .map(|proof| proof.requirement_identity),
                requirement_current_meaning: effect
                    .proof
                    .as_ref()
                    .map(|proof| proof.requirement_current_meaning),
                source_type_identity: effect
                    .proof
                    .as_ref()
                    .map(|proof| proof.source_type_identity),
                retains_source_return_type: effect
                    .proof
                    .as_ref()
                    .is_some_and(|proof| proof.retains_source_return_type),
            });
        }
        if operation.kind == CoreOperationKind::Store
            && let Some(local) = operation.details.first().copied()
            && let Some(subject) = operation
                .operands
                .first()
                .and_then(|operand| value_subjects.get(operand))
        {
            local_subjects.insert(local, *subject);
        }
    }
    Ok(observations.into())
}

#[allow(dead_code)]
pub(crate) struct CustodyCoreView<'a> {
    core: &'a VerifiedCoreProgram,
}

#[allow(dead_code)]
impl CustodyCoreView<'_> {
    pub(crate) fn executables(&self) -> impl ExactSizeIterator<Item = CoreExecutableIndex<'_>> {
        self.core.executables.iter().map(CoreExecutableIndex)
    }

    pub(crate) fn ownership_boundaries(&self) -> impl Iterator<Item = (u128, usize)> + '_ {
        self.core.executables.iter().map(|executable| {
            let count = executable
                .regions
                .iter()
                .flat_map(|region| &*region.operations)
                .filter(|operation| operation.effect == EffectBoundary::Ownership)
                .count();
            (executable.reference.identity, count)
        })
    }

    pub(crate) fn proof_requirements(&self) -> impl Iterator<Item = (u128, u128, u128, u128)> + '_ {
        self.core.executables.iter().flat_map(|executable| {
            executable
                .regions
                .iter()
                .flat_map(|region| region.operations.iter())
                .flat_map(|operation| operation.custody.iter())
                .filter_map(|effect| {
                    effect.proof.as_ref().map(|proof| {
                        (
                            executable.reference.identity,
                            proof.requirement_identity,
                            proof.requirement_current_meaning,
                            proof.source_type_identity,
                        )
                    })
                })
        })
    }
}

#[allow(dead_code)]
pub(crate) struct FlowCoreView<'a> {
    core: &'a VerifiedCoreProgram,
}

#[allow(dead_code)]
impl FlowCoreView<'_> {
    pub(crate) const fn context_identity(&self) -> u128 {
        self.core.context
    }

    pub(crate) const fn fingerprint(&self) -> u128 {
        self.core.fingerprint
    }

    pub(crate) fn executables(&self) -> impl ExactSizeIterator<Item = CoreExecutableIndex<'_>> {
        self.core.executables.iter().map(CoreExecutableIndex)
    }

    pub(crate) fn suspending_executables(&self) -> impl Iterator<Item = u128> + '_ {
        self.core
            .executables
            .iter()
            .filter(|executable| executable.facts.suspends)
            .map(|executable| executable.reference.identity)
    }

    pub(crate) fn message_proposals(&self) -> Vec<FlowCoreMessageProposal> {
        let mut proposals = Vec::new();
        for executable in self.core.executables.iter() {
            let mut ordinal = 0_u32;
            for operation in executable
                .regions
                .iter()
                .flat_map(|region| region.operations.iter())
                .filter(|operation| operation.kind == CoreOperationKind::MessageProposal)
            {
                proposals.push(FlowCoreMessageProposal {
                    sender_handler: executable.reference.identity,
                    destination_handler: operation.details.first().copied().unwrap_or(0),
                    send_ordinal: ordinal,
                    moved_resource_count: operation
                        .details
                        .get(1)
                        .and_then(|count| usize::try_from(*count).ok())
                        .unwrap_or(usize::MAX),
                    source: operation.provenance.clone(),
                });
                ordinal = ordinal.saturating_add(1);
            }
        }
        proposals
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FlowCoreMessageProposal {
    pub(crate) sender_handler: u128,
    pub(crate) destination_handler: u128,
    pub(crate) send_ordinal: u32,
    pub(crate) moved_resource_count: usize,
    pub(crate) source: SourceRange,
}

#[allow(dead_code)]
pub(crate) struct BackendCoreView<'a> {
    core: &'a VerifiedCoreProgram,
}

#[allow(dead_code)]
impl BackendCoreView<'_> {
    pub(crate) fn executables(&self) -> impl ExactSizeIterator<Item = CoreExecutableIndex<'_>> {
        self.core.executables.iter().map(CoreExecutableIndex)
    }

    pub(crate) fn executable_identities(&self) -> impl ExactSizeIterator<Item = u128> + '_ {
        self.core
            .executables
            .iter()
            .map(|executable| executable.reference.identity)
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
pub(crate) struct CoreExecutableIndex<'a>(&'a CoreExecutable);

#[allow(dead_code)]
impl<'a> CoreExecutableIndex<'a> {
    pub(crate) fn identity(self) -> u128 {
        self.0.reference.identity
    }

    pub(crate) fn context(self) -> u128 {
        self.0.reference.context
    }

    pub(crate) fn parameters(self) -> impl ExactSizeIterator<Item = CoreParameterIndex<'a>> {
        self.0.signature.parameters.iter().map(CoreParameterIndex)
    }

    pub(crate) fn return_type_identity(self) -> u128 {
        self.0.signature.return_type.identity
    }

    pub(crate) fn regions(self) -> impl ExactSizeIterator<Item = CoreRegionIndex<'a>> {
        self.0.regions.iter().map(CoreRegionIndex)
    }

    pub(crate) fn rewrites(self) -> impl ExactSizeIterator<Item = CoreRewriteIndex<'a>> {
        self.0.rewrites.iter().map(CoreRewriteIndex)
    }

    pub(crate) fn facts(self) -> (bool, bool, bool, bool) {
        (
            self.0.facts.pure,
            self.0.facts.may_panic,
            self.0.facts.suspends,
            self.0.facts.ownership_transfer,
        )
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
pub(crate) struct CoreParameterIndex<'a>(&'a CoreParameter);

#[allow(dead_code)]
impl CoreParameterIndex<'_> {
    pub(crate) fn local(self) -> u32 {
        self.0.local
    }

    pub(crate) fn type_identity(self) -> u128 {
        self.0.type_.identity
    }

    pub(crate) fn access(self) -> CoreAccessLaw {
        self.0.access
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
pub(crate) struct CoreRegionIndex<'a>(&'a Region);

#[allow(dead_code)]
impl<'a> CoreRegionIndex<'a> {
    pub(crate) fn identity(self) -> u32 {
        self.0.identity.0
    }

    pub(crate) fn operations(self) -> impl ExactSizeIterator<Item = CoreOperationIndex<'a>> {
        self.0.operations.iter().map(CoreOperationIndex)
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
pub(crate) struct CoreOperationIndex<'a>(&'a Operation);

#[allow(dead_code)]
impl<'a> CoreOperationIndex<'a> {
    pub(crate) fn identity(self) -> u32 {
        self.0.identity
    }

    pub(crate) fn kind(self) -> CoreOperationKind {
        self.0.kind
    }

    pub(crate) fn result(self) -> Option<u32> {
        self.0.result.map(|value| value.0)
    }

    pub(crate) fn type_identity(self) -> Option<u128> {
        self.0.type_identity
    }

    pub(crate) fn operands(self) -> impl ExactSizeIterator<Item = u32> + 'a {
        self.0.operands.iter().map(|value| value.0)
    }

    pub(crate) fn call_binding(self) -> &'a [u16] {
        &self.0.call_binding
    }

    pub(crate) fn successors(self) -> impl ExactSizeIterator<Item = u32> + 'a {
        self.0.successors.iter().map(|region| region.0)
    }

    pub(crate) fn details(self) -> &'a [u128] {
        &self.0.details
    }

    pub(crate) fn laws(self) -> (CoreAccessLaw, u8, u8) {
        (self.0.access, self.0.effect.tag(), self.0.failure.tag())
    }

    pub(crate) fn provenance(self) -> &'a SourceRange {
        &self.0.provenance
    }

    pub(crate) fn custody(self) -> impl ExactSizeIterator<Item = CoreCustodyIndex<'a>> {
        self.0.custody.iter().map(CoreCustodyIndex)
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
pub(crate) struct CoreCustodyIndex<'a>(&'a CustodyEffect);

#[allow(dead_code)]
impl<'a> CoreCustodyIndex<'a> {
    pub(crate) fn operation(self) -> CoreCustodyOperation {
        self.0.operation
    }

    pub(crate) fn dimensions(
        self,
    ) -> (
        CoreInitializationEffect,
        CoreCustodianEffect,
        CoreLoanEffect,
        CoreObligationEffect,
    ) {
        (
            self.0.initialization,
            self.0.custodian,
            self.0.loan,
            self.0.obligation,
        )
    }

    pub(crate) fn place(self) -> &'a [u128] {
        &self.0.place
    }

    pub(crate) fn proof_requirement(self) -> Option<(u128, u128, u128)> {
        self.0.proof.as_ref().map(|proof| {
            (
                proof.requirement_identity,
                proof.requirement_current_meaning,
                proof.source_type_identity,
            )
        })
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
pub(crate) struct CoreRewriteIndex<'a>(&'a RewriteWitness);

#[allow(dead_code)]
impl<'a> CoreRewriteIndex<'a> {
    pub(crate) fn kind(self) -> CoreRewriteKind {
        self.0.kind
    }

    pub(crate) fn provenance(self) -> &'a SourceRange {
        &self.0.provenance
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CoreModule;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CoreFailure {
    Cancelled,
    Defect(Arc<str>),
}

impl CoreModule {
    pub(crate) fn derive(
        &self,
        input: CorePlanningInput<'_>,
        cancellation: &Cancellation,
    ) -> Result<VerifiedCoreProgram, CoreFailure> {
        checkpoint(cancellation)?;
        let context = input.context_identity();
        let semantic = input.completed_semantic_program();
        let mut executables = Vec::new();
        for reference in input.exact_source_executables() {
            checkpoint(cancellation)?;
            let source = semantic.executable_input(reference).ok_or_else(|| {
                CoreFailure::Defect(Arc::from("Core demand names a missing typed source body"))
            })?;
            executables.push(produce_source_executable(
                source,
                semantic,
                input,
                cancellation,
            )?);
        }
        for addition in input.generated_executable_additions() {
            checkpoint(cancellation)?;
            let role = input
                .generated_roles()
                .iter()
                .find(|role| role.executable().identity() == addition.identity())
                .ok_or_else(|| {
                    CoreFailure::Defect(Arc::from(
                        "Core generated demand has no authenticated role",
                    ))
                })?;
            executables.push(produce_generated_executable(*addition, role, cancellation)?);
        }
        executables
            .sort_by_key(|executable| (executable.reference.kind, executable.reference.identity));
        let source_demand_identity = input.source_executable_demand().identity();
        let planning_fingerprint = planning_input_fingerprint(input);
        let mut candidate = VerifiedCoreProgram {
            context,
            planning_fingerprint,
            source_demand_identity,
            executables: executables.into(),
            oracle: OracleSummary {
                cases: 0,
                agrees: true,
                custody_cases: 0,
                custody_agrees: true,
                fingerprint: 0,
            },
            fingerprint: 0,
            _verified: Verified,
        };
        candidate.oracle = run_oracle(&candidate, input, cancellation)?;
        candidate.fingerprint = producer_fingerprint(&candidate, cancellation)?;
        verify(&candidate, input, cancellation)?;
        Ok(candidate)
    }
}

fn planning_input_fingerprint(input: CorePlanningInput<'_>) -> u128 {
    input.fingerprint()
}

fn produce_source_executable(
    input: CoreSourceExecutableInput<'_>,
    semantic: crate::completed_semantic::CorePlanningSemanticProgram<'_>,
    planning: CorePlanningInput<'_>,
    cancellation: &Cancellation,
) -> Result<CoreExecutable, CoreFailure> {
    let reference = ExecutableReference {
        context: input.reference.context(),
        kind: source_kind(input.reference.kind()),
        identity: input.reference.identity(),
        current_meaning: input.reference.current_meaning(),
    };
    let (owner, provenance, signature, source_definition, facts, mut regions, rewrites) =
        match input.body {
            CoreSourceExecutableBody::Specialization(function) => {
                let upstream = semantic
                    .specialization_facts(SpecializationId(reference.identity))
                    .ok_or_else(|| {
                        CoreFailure::Defect(Arc::from("Core body has no solved facts"))
                    })?;
                let mut lowerer = ProducerLowerer::new(cancellation);
                let entry = lowerer.lower_block(&function.body)?;
                debug_assert_eq!(entry, RegionId(0));
                let (regions, rewrites) = lowerer.finish();
                (
                    function.id.0,
                    function.source.clone(),
                    signature(
                        function
                            .parameters
                            .iter()
                            .zip(function.parameter_type_ids.iter())
                            .map(|((local, type_, access), type_id)| {
                                (local.0, type_, *type_id, core_access(*access))
                            }),
                        &function.return_type,
                        function.return_type_id,
                    ),
                    Some(function.id.0),
                    ExecutableFacts {
                        pure: upstream.pure,
                        may_panic: upstream.may_panic,
                        suspends: upstream.suspends,
                        ownership_transfer: upstream.ownership_transfer,
                        evaluator_eligible: upstream.evaluator_eligible,
                    },
                    regions,
                    rewrites,
                )
            }
            CoreSourceExecutableBody::Test(test) => {
                let mut lowerer = ProducerLowerer::new(cancellation);
                let entry = lowerer.lower_block(&test.body)?;
                debug_assert_eq!(entry, RegionId(0));
                let (regions, rewrites) = lowerer.finish();
                (
                    reference.identity,
                    test.source.clone(),
                    signature(
                        test.parameters
                            .iter()
                            .zip(test.parameter_type_ids.iter())
                            .map(|((local, type_, access), type_id)| {
                                (local.0, type_, *type_id, core_access(*access))
                            }),
                        &Type::Unit,
                        test.return_type_id,
                    ),
                    None,
                    facts_from_regions(&regions, semantic),
                    regions,
                    rewrites,
                )
            }
            CoreSourceExecutableBody::Closure(closure) => {
                let mut lowerer = ProducerLowerer::new(cancellation);
                let entry = lowerer.lower_expression_body(&closure.body)?;
                debug_assert_eq!(entry, RegionId(0));
                let (regions, rewrites) = lowerer.finish();
                (
                    closure.id.0,
                    closure.source.clone(),
                    signature(
                        closure
                            .parameters
                            .iter()
                            .zip(closure.parameter_type_ids.iter())
                            .map(|((local, type_), type_id)| {
                                (local.0, type_, *type_id, CoreAccessLaw::CopyValue)
                            }),
                        &closure.return_type,
                        closure.return_type_id,
                    ),
                    None,
                    facts_from_regions(&regions, semantic),
                    regions,
                    rewrites,
                )
            }
        };
    producer_link_cleanup_control(&mut regions, cancellation)?;
    producer_attach_custody(
        &mut regions,
        &signature,
        semantic.verified_program(),
        cancellation,
    )?;
    attach_pool_proof_conditions(
        &mut regions,
        reference.identity,
        semantic,
        planning,
        cancellation,
    )?;
    let mut executable = CoreExecutable {
        reference,
        semantic_owner: owner,
        provenance,
        entry: RegionId(0),
        regions,
        facts,
        signature,
        rewrites,
        source_definition,
        fingerprint: 0,
    };
    executable.fingerprint = producer_executable_fingerprint(&executable, cancellation)?;
    Ok(executable)
}

fn produce_generated_executable(
    executable: ExecutableRef,
    role: &GeneratedRole,
    cancellation: &Cancellation,
) -> Result<CoreExecutable, CoreFailure> {
    let provenance = SourceRange::new("<generated:mandatory-image>", 0, 0);
    let mut details = vec![
        role.reference().identity(),
        role.reference().current_meaning(),
        role.owner().identity(),
        role.generator().identity(),
        u128::from(role.local_key()),
        role.provenance().identity(),
        u128::from(generated_role_tag(role.kind())),
    ];
    details.extend(
        role.dependencies()
            .iter()
            .flat_map(|dependency| [dependency.identity(), dependency.current_meaning()]),
    );
    let operation = Operation {
        identity: 0,
        kind: CoreOperationKind::GeneratedRole,
        result: None,
        type_identity: None,
        operands: Arc::from([]),
        call_binding: Arc::from([]),
        successors: Arc::from([]),
        details: details.into(),
        effect: EffectBoundary::None,
        failure: FailureLaw::None,
        provenance: provenance.clone(),
        access: CoreAccessLaw::None,
        custody: Arc::from([]),
    };
    let mut core = CoreExecutable {
        reference: ExecutableReference {
            context: executable.context(),
            kind: CoreExecutableKind::Generated,
            identity: executable.identity(),
            current_meaning: executable.current_meaning(),
        },
        semantic_owner: role.owner().identity(),
        provenance,
        entry: RegionId(0),
        regions: Arc::from([Region {
            identity: RegionId(0),
            operations: Arc::from([operation]),
        }]),
        facts: ExecutableFacts {
            pure: false,
            may_panic: role.kind() == crate::image_planning::GeneratedRoleKind::Panic,
            suspends: false,
            ownership_transfer: false,
            evaluator_eligible: false,
        },
        signature: generated_signature(),
        rewrites: Arc::from([]),
        source_definition: None,
        fingerprint: 0,
    };
    core.fingerprint = producer_executable_fingerprint(&core, cancellation)?;
    Ok(core)
}

const fn generated_role_tag(kind: crate::image_planning::GeneratedRoleKind) -> u8 {
    match kind {
        crate::image_planning::GeneratedRoleKind::Boot => 1,
        crate::image_planning::GeneratedRoleKind::Scheduler => 2,
        crate::image_planning::GeneratedRoleKind::Terminal => 3,
        crate::image_planning::GeneratedRoleKind::Panic => 4,
        crate::image_planning::GeneratedRoleKind::Shutdown => 5,
        crate::image_planning::GeneratedRoleKind::TestRuntime => 6,
    }
}

const fn source_kind(kind: CoreSourceExecutableKind) -> CoreExecutableKind {
    match kind {
        CoreSourceExecutableKind::Specialization => CoreExecutableKind::SourceSpecialization,
        CoreSourceExecutableKind::TestBody => CoreExecutableKind::SourceTestBody,
        CoreSourceExecutableKind::ClosureBody => CoreExecutableKind::SourceClosureBody,
    }
}

fn core_type(type_: &Type, type_id: TypeId) -> CoreType {
    CoreType {
        identity: type_id.0,
        shape: type_.canonical_key(),
    }
}

fn signature<'a>(
    parameters: impl IntoIterator<Item = (u32, &'a Type, TypeId, CoreAccessLaw)>,
    return_type: &Type,
    return_type_id: TypeId,
) -> CoreSignature {
    CoreSignature {
        parameters: parameters
            .into_iter()
            .map(|(local, type_, type_id, access)| CoreParameter {
                local,
                type_: core_type(type_, type_id),
                access,
            })
            .collect::<Vec<_>>()
            .into(),
        return_type: core_type(return_type, return_type_id),
    }
}

fn generated_signature() -> CoreSignature {
    CoreSignature {
        parameters: Arc::from([]),
        return_type: CoreType {
            identity: 0,
            shape: Type::Unit.canonical_key(),
        },
    }
}

fn custody_home(domain: &[u8], identity: u128) -> u128 {
    let mut bytes = Vec::with_capacity(domain.len() + 16);
    bytes.extend_from_slice(domain);
    bytes.extend_from_slice(&identity.to_be_bytes());
    xxh3_128(&bytes)
}

fn place_home(place: &[u128]) -> u128 {
    let mut bytes = Vec::with_capacity(16 + place.len() * 16);
    bytes.extend_from_slice(b"wrela.core.place-home\0\x01");
    for part in place {
        bytes.extend_from_slice(&part.to_be_bytes());
    }
    xxh3_128(&bytes)
}

fn cleanup_capture_home(registration: u128, ordinal: u128) -> u128 {
    custody_home(b"cleanup-capture", registration.rotate_left(37) ^ ordinal)
}

fn custody_effect(
    operation: CoreCustodyOperation,
    dimensions: (
        CoreInitializationEffect,
        CoreCustodianEffect,
        CoreLoanEffect,
        CoreObligationEffect,
    ),
    place: Arc<[u128]>,
    type_identity: Option<u128>,
    source_home: Option<u128>,
    destination_home: Option<u128>,
) -> CustodyEffect {
    let (initialization, custodian, loan, obligation) = dimensions;
    CustodyEffect {
        operation,
        initialization,
        custodian,
        loan,
        obligation,
        place,
        type_identity,
        source_home,
        destination_home,
        cleanup_ordinal: None,
        proof: None,
    }
}

fn producer_link_cleanup_control(
    regions: &mut Arc<[Region]>,
    cancellation: &Cancellation,
) -> Result<(), CoreFailure> {
    let original = regions.to_vec();
    let mut inherited = BTreeMap::from([(RegionId(0), Vec::<u32>::new())]);
    let mut pending = vec![RegionId(0)];
    let mut exits = Vec::<(RegionId, usize, Vec<u32>, Arc<[RegionId]>)>::new();
    let registrations = original
        .iter()
        .flat_map(|region| region.operations.iter())
        .filter(|operation| operation.kind == CoreOperationKind::Cleanup)
        .filter_map(|operation| Some((operation.identity, *operation.successors.first()?)))
        .collect::<BTreeMap<_, _>>();
    while let Some(region_id) = pending.pop() {
        checkpoint(cancellation)?;
        let Some(region) = original.get(region_id.0 as usize) else {
            continue;
        };
        let mut active = inherited.get(&region_id).cloned().unwrap_or_default();
        let inherited_count = active.len();
        let mut reachable = true;
        for (operation_index, operation) in region.operations.iter().enumerate() {
            checkpoint(cancellation)?;
            if !reachable {
                continue;
            }
            if operation.kind == CoreOperationKind::Cleanup {
                active.push(operation.identity);
            }
            let exiting = match operation.kind {
                CoreOperationKind::Return | CoreOperationKind::Propagate => active.as_slice(),
                CoreOperationKind::Break
                | CoreOperationKind::Continue
                | CoreOperationKind::LoopBack
                | CoreOperationKind::RecoverableExit => &active[inherited_count..],
                _ => &[],
            };
            if !exiting.is_empty() {
                exits.push((
                    region_id,
                    operation_index,
                    exiting.iter().rev().copied().collect(),
                    Arc::clone(&operation.successors),
                ));
            }
            for successor in operation.successors.iter().copied() {
                if operation.kind == CoreOperationKind::LoopBack {
                    continue;
                }
                let next = if operation.kind == CoreOperationKind::Cleanup {
                    Vec::new()
                } else {
                    active.clone()
                };
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    inherited.entry(successor)
                {
                    entry.insert(next);
                    pending.push(successor);
                }
            }
            reachable &= !matches!(
                operation.kind,
                CoreOperationKind::Return
                    | CoreOperationKind::Propagate
                    | CoreOperationKind::TerminalPanic
                    | CoreOperationKind::Break
                    | CoreOperationKind::Continue
                    | CoreOperationKind::LoopBack
            );
        }
    }
    let mut next = original;
    let mut next_operation = next
        .iter()
        .flat_map(|region| region.operations.iter())
        .map(|operation| operation.identity)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    for (exit_region, exit_index, cleanup_order, original_successors) in exits {
        checkpoint(cancellation)?;
        let mut continuation = original_successors.to_vec();
        for cleanup in cleanup_order.iter().rev().copied() {
            checkpoint(cancellation)?;
            let action = registrations.get(&cleanup).copied().ok_or_else(|| {
                CoreFailure::Defect(Arc::from("Core cleanup registration has no action region"))
            })?;
            let registration = next
                .iter()
                .flat_map(|region| region.operations.iter())
                .find(|operation| operation.identity == cleanup)
                .ok_or_else(|| {
                    CoreFailure::Defect(Arc::from("Core cleanup registration is missing"))
                })?;
            let run_region = RegionId(u32::try_from(next.len()).unwrap_or(u32::MAX));
            let mut successors = vec![action];
            successors.extend(continuation.iter().copied());
            next.push(Region {
                identity: run_region,
                operations: Arc::from([Operation {
                    identity: next_operation,
                    kind: CoreOperationKind::CleanupRun,
                    result: None,
                    type_identity: None,
                    operands: Arc::from([]),
                    call_binding: Arc::from([]),
                    successors: successors.into(),
                    details: Arc::from([u128::from(cleanup)]),
                    effect: EffectBoundary::Ownership,
                    access: CoreAccessLaw::None,
                    failure: FailureLaw::CallPanicPropagation,
                    provenance: registration.provenance.clone(),
                    custody: Arc::from([]),
                }]),
            });
            next_operation = next_operation.saturating_add(1);
            continuation = vec![run_region];
        }
        let region = &mut next[exit_region.0 as usize];
        let mut operations = region.operations.to_vec();
        operations[exit_index].successors = continuation.into();
        region.operations = operations.into();
    }
    *regions = next.into();
    Ok(())
}

fn producer_attach_custody(
    regions: &mut Arc<[Region]>,
    signature: &CoreSignature,
    program: &crate::typed_hir::VerifiedProgram,
    cancellation: &Cancellation,
) -> Result<(), CoreFailure> {
    let components = producer_resource_components(regions, program, cancellation)?;
    let mut value_types = BTreeMap::new();
    let mut value_access = BTreeMap::new();
    for operation in regions.iter().flat_map(|region| region.operations.iter()) {
        if let (Some(value), Some(type_identity)) = (operation.result, operation.type_identity) {
            value_types.insert(value, type_identity);
            value_access.insert(value, operation.access);
        }
    }
    let entry = signature
        .parameters
        .iter()
        .filter(|parameter| {
            parameter.access == CoreAccessLaw::Move
                && program.owns_resource_type(TypeId(parameter.type_.identity))
        })
        .flat_map(|parameter| {
            let root = u128::from(parameter.local);
            components
                .get(&root)
                .cloned()
                .unwrap_or_else(|| vec![(Arc::<[u128]>::from([root]), parameter.type_.identity)])
        })
        .collect::<LivePlaces>();
    let mut flow = ProducerCustodyFlow {
        regions,
        value_types: &value_types,
        value_access: &value_access,
        program,
        cancellation,
        components: &components,
        effects: BTreeMap::new(),
        completed: BTreeMap::new(),
        visiting: BTreeSet::new(),
    };
    flow.region(RegionId(0), entry, BTreeMap::new())?;
    for region in regions.iter() {
        if !flow.completed.contains_key(&region.identity) {
            flow.region(region.identity, BTreeMap::new(), BTreeMap::new())?;
        }
    }
    let mut next = regions.to_vec();
    for region in &mut next {
        let mut operations = region.operations.to_vec();
        for operation in &mut operations {
            checkpoint(cancellation)?;
            operation.custody = flow
                .effects
                .remove(&operation.identity)
                .unwrap_or_default()
                .into();
        }
        region.operations = operations.into();
    }
    *regions = next.into();
    Ok(())
}

fn pool_operation_for_core_call(
    operation: &Operation,
    semantic: crate::completed_semantic::CorePlanningSemanticProgram<'_>,
) -> Option<PoolOperation> {
    if operation.kind != CoreOperationKind::Call || operation.details.first().copied() != Some(3) {
        return None;
    }
    let specialization = operation.details.get(2).copied()?;
    semantic
        .verified_program()
        .specialization_function(SpecializationId(specialization))?
        .pool_operation
}

fn pool_proof_condition(
    operation: &Operation,
    executable_identity: u128,
    semantic: crate::completed_semantic::CorePlanningSemanticProgram<'_>,
    planning: CorePlanningInput<'_>,
) -> Option<ProofCondition> {
    let pool_operation = pool_operation_for_core_call(operation, semantic)?;
    if !matches!(
        pool_operation,
        PoolOperation::Allocate | PoolOperation::Reserve
    ) {
        return None;
    }
    let (requirement, source_type_identity) =
        planning.pool_admission_site(executable_identity, pool_operation, &operation.provenance)?;
    Some(ProofCondition {
        requirement_identity: requirement.identity(),
        requirement_current_meaning: requirement.current_meaning(),
        source_type_identity,
        retains_source_return_type: true,
    })
}

fn attach_pool_proof_conditions(
    regions: &mut Arc<[Region]>,
    executable_identity: u128,
    semantic: crate::completed_semantic::CorePlanningSemanticProgram<'_>,
    planning: CorePlanningInput<'_>,
    cancellation: &Cancellation,
) -> Result<(), CoreFailure> {
    let mut next = regions.to_vec();
    for region in &mut next {
        let mut operations = region.operations.to_vec();
        for operation in &mut operations {
            checkpoint(cancellation)?;
            if let Some(proof) =
                pool_proof_condition(operation, executable_identity, semantic, planning)
            {
                let mut effect = custody_effect(
                    CoreCustodyOperation::ProofCondition,
                    (
                        CoreInitializationEffect::None,
                        CoreCustodianEffect::None,
                        CoreLoanEffect::None,
                        CoreObligationEffect::None,
                    ),
                    Arc::from([]),
                    None,
                    None,
                    None,
                );
                effect.proof = Some(proof);
                let mut effects = operation.custody.to_vec();
                effects.push(effect);
                operation.custody = effects.into();
            }
        }
        region.operations = operations.into();
    }
    *regions = next.into();
    Ok(())
}

fn verify_pool_proof_conditions(
    executable: &CoreExecutable,
    semantic: crate::completed_semantic::CorePlanningSemanticProgram<'_>,
    planning: CorePlanningInput<'_>,
    cancellation: &Cancellation,
) -> Result<(), CoreFailure> {
    for operation in executable
        .regions
        .iter()
        .flat_map(|region| region.operations.iter())
    {
        checkpoint(cancellation)?;
        let expected =
            pool_proof_condition(operation, executable.reference.identity, semantic, planning);
        let supplied = operation
            .custody
            .iter()
            .filter_map(|effect| effect.proof.as_ref())
            .collect::<Vec<_>>();
        match expected {
            Some(expected) if supplied.as_slice() == [&expected] => {}
            None if supplied.is_empty() => {}
            _ => {
                return defect(
                    "Core Pool proof is missing, extra, stale, wrong-site, or changes the source type",
                );
            }
        }
    }
    Ok(())
}

struct ProducerCustodyFlow<'a> {
    regions: &'a [Region],
    value_types: &'a BTreeMap<ValueId, u128>,
    value_access: &'a BTreeMap<ValueId, CoreAccessLaw>,
    program: &'a crate::typed_hir::VerifiedProgram,
    cancellation: &'a Cancellation,
    components: &'a ResourceComponents,
    effects: BTreeMap<u32, Vec<CustodyEffect>>,
    completed: BTreeMap<RegionId, Option<LivePlaces>>,
    visiting: BTreeSet<RegionId>,
}

impl ProducerCustodyFlow<'_> {
    fn region(
        &mut self,
        identity: RegionId,
        mut live: LivePlaces,
        inherited: LivePlaces,
    ) -> Result<Option<LivePlaces>, CoreFailure> {
        checkpoint(self.cancellation)?;
        if let Some(output) = self.completed.get(&identity) {
            return Ok(output.clone());
        }
        if !self.visiting.insert(identity) {
            return Ok(Some(live));
        }
        let operations = self
            .regions
            .get(identity.0 as usize)
            .ok_or_else(|| {
                CoreFailure::Defect(Arc::from("Core custody flow has a dangling region"))
            })?
            .operations
            .to_vec();
        let mut known = live.clone();
        let mut terminated = false;
        for operation in &operations {
            checkpoint(self.cancellation)?;
            let mut effects = producer_operation_custody(
                operation,
                self.value_types,
                self.value_access,
                self.program,
                self.cancellation,
            )?;
            producer_update_live_places(
                operation,
                self.value_types,
                self.program,
                self.components,
                &mut live,
            );
            if operation.kind == CoreOperationKind::Store {
                producer_update_live_places(
                    operation,
                    self.value_types,
                    self.program,
                    self.components,
                    &mut known,
                );
            }
            if matches!(
                operation.kind,
                CoreOperationKind::Branch | CoreOperationKind::Match
            ) {
                let mut outputs = Vec::new();
                for successor in operation.successors.iter().copied() {
                    if let Some(output) = self.region(successor, live.clone(), known.clone())? {
                        outputs.push(output);
                    }
                }
                if let Some(joined) = producer_join_live(outputs)? {
                    live = joined;
                } else {
                    live.clear();
                    terminated = true;
                }
            } else if operation.kind == CoreOperationKind::Loop {
                let mut outputs = vec![live.clone()];
                for successor in operation.successors.iter().copied() {
                    if let Some(output) = self.region(successor, live.clone(), live.clone())? {
                        outputs.push(output);
                    }
                }
                live = producer_join_live(outputs)?.unwrap_or_default();
            } else if operation.kind == CoreOperationKind::PoolScope
                && let Some(successor) = operation.successors.first().copied()
            {
                let mut scoped = live.clone();
                if let Some(type_identity) = operation
                    .operands
                    .iter()
                    .filter_map(|operand| self.value_types.get(operand).copied())
                    .find(|type_identity| self.program.owns_resource_type(TypeId(*type_identity)))
                {
                    scoped.insert(Arc::clone(&operation.details), type_identity);
                }
                if let Some(output) = self.region(successor, scoped, live.clone())? {
                    live = output;
                } else {
                    live.clear();
                    terminated = true;
                }
            }
            let leaving = matches!(
                operation.kind,
                CoreOperationKind::Return
                    | CoreOperationKind::Propagate
                    | CoreOperationKind::Break
                    | CoreOperationKind::Continue
                    | CoreOperationKind::LoopBack
                    | CoreOperationKind::RecoverableExit
            );
            if leaving {
                let discharge_all = matches!(
                    operation.kind,
                    CoreOperationKind::Return | CoreOperationKind::Propagate
                ) || identity == RegionId(0);
                let discharges = live
                    .iter()
                    .filter(|(place, type_identity)| {
                        discharge_all || inherited.get(*place) != Some(*type_identity)
                    })
                    .map(|(place, type_identity)| (Arc::clone(place), *type_identity))
                    .collect::<Vec<_>>();
                effects.extend(discharges.iter().map(|(place, type_identity)| {
                    producer_discharge_effect(place, *type_identity, self.program)
                }));
                for (place, _) in discharges {
                    live.remove(&place);
                }
                terminated |= matches!(
                    operation.kind,
                    CoreOperationKind::Return | CoreOperationKind::Propagate
                );
            }
            if operation.kind == CoreOperationKind::TerminalPanic {
                live.clear();
                terminated = true;
            }
            self.effects.insert(operation.identity, effects);
        }
        self.visiting.remove(&identity);
        let output = (!terminated).then_some(live);
        self.completed.insert(identity, output.clone());
        Ok(output)
    }
}

fn producer_join_live(states: Vec<LivePlaces>) -> Result<Option<LivePlaces>, CoreFailure> {
    let mut states = states.into_iter();
    let Some(first) = states.next() else {
        return Ok(None);
    };
    if states.all(|state| state == first) {
        Ok(Some(first))
    } else {
        defect("Core producer found path-dependent custody at a control-flow join")
    }
}

fn producer_resource_components(
    regions: &[Region],
    program: &crate::typed_hir::VerifiedProgram,
    cancellation: &Cancellation,
) -> Result<ResourceComponents, CoreFailure> {
    let mut components = BTreeMap::<u128, BTreeMap<Arc<[u128]>, u128>>::new();
    for operation in regions.iter().flat_map(|region| region.operations.iter()) {
        checkpoint(cancellation)?;
        if operation.kind == CoreOperationKind::Read
            && operation.details.len() > 1
            && operation
                .type_identity
                .is_some_and(|identity| program.owns_resource_type(TypeId(identity)))
        {
            components.entry(operation.details[0]).or_default().insert(
                Arc::clone(&operation.details),
                operation.type_identity.unwrap_or(0),
            );
        }
    }
    Ok(components
        .into_iter()
        .map(|(root, components)| (root, components.into_iter().collect()))
        .collect())
}

fn producer_update_live_places(
    operation: &Operation,
    value_types: &BTreeMap<ValueId, u128>,
    program: &crate::typed_hir::VerifiedProgram,
    components: &ResourceComponents,
    live_places: &mut BTreeMap<Arc<[u128]>, u128>,
) {
    if operation.kind == CoreOperationKind::Read
        && matches!(
            operation.access,
            CoreAccessLaw::Move | CoreAccessLaw::CleanupCapture
        )
        && operation
            .type_identity
            .is_some_and(|identity| program.owns_resource_type(TypeId(identity)))
    {
        live_places.retain(|place, _| !places_overlap(place, &operation.details));
    }
    if operation.kind == CoreOperationKind::Store {
        let place: Arc<[u128]> =
            operation.details[..operation.details.len().saturating_sub(1)].into();
        if let Some(type_identity) = operation
            .operands
            .iter()
            .filter_map(|operand| value_types.get(operand).copied())
            .find(|identity| program.owns_resource_type(TypeId(*identity)))
        {
            if place.len() == 1 {
                if let Some(children) = components.get(&place[0]) {
                    live_places.retain(|candidate, _| !place_is_prefix(&place, candidate));
                    live_places.extend(children.iter().cloned());
                } else {
                    live_places.insert(place, type_identity);
                }
            } else {
                live_places.insert(place, type_identity);
            }
        }
    }
}

fn producer_discharge_effect(
    place: &Arc<[u128]>,
    type_identity: u128,
    program: &crate::typed_hir::VerifiedProgram,
) -> CustodyEffect {
    let destination_domain = if program.requires_explicit_discharge(TypeId(type_identity)) {
        b"explicit-discharge" as &[u8]
    } else {
        b"compiler-reclaim"
    };
    custody_effect(
        CoreCustodyOperation::Discharge,
        (
            CoreInitializationEffect::Uninitialize,
            CoreCustodianEffect::Discharge,
            CoreLoanEffect::None,
            CoreObligationEffect::Discharge,
        ),
        Arc::clone(place),
        Some(type_identity),
        Some(place_home(place)),
        Some(custody_home(destination_domain, place_home(place))),
    )
}

fn producer_operation_custody(
    operation: &Operation,
    value_types: &BTreeMap<ValueId, u128>,
    value_access: &BTreeMap<ValueId, CoreAccessLaw>,
    program: &crate::typed_hir::VerifiedProgram,
    cancellation: &Cancellation,
) -> Result<Vec<CustodyEffect>, CoreFailure> {
    let mut effects = Vec::new();
    let resource_result = operation
        .type_identity
        .filter(|identity| program.owns_resource_type(TypeId(*identity)));
    if let (Some(result), Some(type_identity)) = (operation.result, resource_result) {
        let destination = custody_home(b"value", u128::from(result.0));
        match (operation.kind, operation.access) {
            (CoreOperationKind::Read, CoreAccessLaw::Move | CoreAccessLaw::CleanupCapture) => {
                let source = if operation.access == CoreAccessLaw::CleanupCapture {
                    cleanup_capture_home(
                        operation.details.first().copied().unwrap_or(u128::MAX),
                        operation.details.get(1).copied().unwrap_or(u128::MAX),
                    )
                } else {
                    place_home(&operation.details)
                };
                effects.push(custody_effect(
                    CoreCustodyOperation::Move,
                    (
                        CoreInitializationEffect::Uninitialize,
                        CoreCustodianEffect::Transfer,
                        CoreLoanEffect::None,
                        CoreObligationEffect::Transfer,
                    ),
                    Arc::clone(&operation.details),
                    Some(type_identity),
                    Some(source),
                    Some(destination),
                ));
            }
            (CoreOperationKind::Read, CoreAccessLaw::SharedLoan) => effects.push(custody_effect(
                CoreCustodyOperation::SharedLoan,
                (
                    CoreInitializationEffect::None,
                    CoreCustodianEffect::None,
                    CoreLoanEffect::Shared,
                    CoreObligationEffect::None,
                ),
                Arc::clone(&operation.details),
                Some(type_identity),
                None,
                None,
            )),
            (CoreOperationKind::Read, CoreAccessLaw::ExclusiveLoan) => {
                effects.push(custody_effect(
                    CoreCustodyOperation::ExclusiveLoan,
                    (
                        CoreInitializationEffect::None,
                        CoreCustodianEffect::None,
                        CoreLoanEffect::Exclusive,
                        CoreObligationEffect::None,
                    ),
                    Arc::clone(&operation.details),
                    Some(type_identity),
                    None,
                    None,
                ));
            }
            (CoreOperationKind::Aggregate | CoreOperationKind::BuildConstruction, _) => {
                effects.push(custody_effect(
                    CoreCustodyOperation::Construct,
                    (
                        CoreInitializationEffect::Initialize,
                        CoreCustodianEffect::Establish,
                        CoreLoanEffect::None,
                        CoreObligationEffect::Establish,
                    ),
                    Arc::from([]),
                    Some(type_identity),
                    None,
                    Some(destination),
                ));
            }
            (CoreOperationKind::Call, _) if operation.details.first() == Some(&8) => {
                effects.push(custody_effect(
                    CoreCustodyOperation::Construct,
                    (
                        CoreInitializationEffect::Initialize,
                        CoreCustodianEffect::Establish,
                        CoreLoanEffect::None,
                        CoreObligationEffect::Establish,
                    ),
                    Arc::from([]),
                    Some(type_identity),
                    None,
                    Some(destination),
                ));
            }
            (CoreOperationKind::Call | CoreOperationKind::Propagate, _) => {
                effects.push(custody_effect(
                    CoreCustodyOperation::TransferCommit,
                    (
                        CoreInitializationEffect::Initialize,
                        CoreCustodianEffect::Establish,
                        CoreLoanEffect::None,
                        CoreObligationEffect::Establish,
                    ),
                    Arc::from([]),
                    Some(type_identity),
                    Some(custody_home(b"operation", u128::from(operation.identity))),
                    Some(destination),
                ));
            }
            _ => {}
        }
    }
    let moved_resources = operation.operands.iter().filter_map(|operand| {
        let type_identity = *value_types.get(operand)?;
        (program.owns_resource_type(TypeId(type_identity))
            && !matches!(
                value_access.get(operand),
                Some(CoreAccessLaw::SharedLoan | CoreAccessLaw::ExclusiveLoan)
            ))
        .then_some((*operand, type_identity))
    });
    for (ordinal, (operand, type_identity)) in moved_resources.enumerate() {
        checkpoint(cancellation)?;
        let source = custody_home(b"value", u128::from(operand.0));
        let destination = if operation.kind == CoreOperationKind::PoolScope {
            place_home(&operation.details)
        } else if operation.kind == CoreOperationKind::Cleanup {
            cleanup_capture_home(u128::from(operation.identity), ordinal as u128)
        } else {
            custody_home(
                match operation.kind {
                    CoreOperationKind::Return => b"return" as &[u8],
                    CoreOperationKind::Store => b"place",
                    _ => b"operation",
                },
                u128::from(operation.identity) << 32 | ordinal as u128,
            )
        };
        if operation.kind == CoreOperationKind::Store {
            let initialize = operation.details.last().copied() == Some(1);
            effects.push(custody_effect(
                if initialize {
                    CoreCustodyOperation::TransferCommit
                } else {
                    CoreCustodyOperation::Reinitialize
                },
                (
                    if initialize {
                        CoreInitializationEffect::Initialize
                    } else {
                        CoreInitializationEffect::Reinitialize
                    },
                    CoreCustodianEffect::Transfer,
                    CoreLoanEffect::None,
                    CoreObligationEffect::Transfer,
                ),
                operation.details[..operation.details.len().saturating_sub(1)].into(),
                Some(type_identity),
                Some(source),
                Some(place_home(
                    &operation.details[..operation.details.len().saturating_sub(1)],
                )),
            ));
        } else if operation.kind == CoreOperationKind::PoolScope {
            effects.push(custody_effect(
                CoreCustodyOperation::TransferCommit,
                (
                    CoreInitializationEffect::Initialize,
                    CoreCustodianEffect::Transfer,
                    CoreLoanEffect::None,
                    CoreObligationEffect::Transfer,
                ),
                Arc::clone(&operation.details),
                Some(type_identity),
                Some(source),
                Some(destination),
            ));
        } else if matches!(
            operation.kind,
            CoreOperationKind::Call
                | CoreOperationKind::BuildConstruction
                | CoreOperationKind::Aggregate
                | CoreOperationKind::Return
                | CoreOperationKind::Cleanup
                | CoreOperationKind::Propagate
        ) {
            effects.push(custody_effect(
                CoreCustodyOperation::TransferCommit,
                (
                    CoreInitializationEffect::Uninitialize,
                    CoreCustodianEffect::Transfer,
                    CoreLoanEffect::None,
                    CoreObligationEffect::Transfer,
                ),
                Arc::from([]),
                Some(type_identity),
                Some(source),
                Some(destination),
            ));
        }
    }
    match operation.kind {
        CoreOperationKind::Cleanup => {
            let mut effect = custody_effect(
                CoreCustodyOperation::CleanupRegister,
                (
                    CoreInitializationEffect::None,
                    CoreCustodianEffect::None,
                    CoreLoanEffect::None,
                    CoreObligationEffect::Establish,
                ),
                Arc::from([]),
                None,
                None,
                Some(custody_home(b"cleanup", u128::from(operation.identity))),
            );
            effect.cleanup_ordinal = Some(operation.identity);
            effects.push(effect);
        }
        CoreOperationKind::CleanupRun => {
            let ordinal = operation
                .details
                .first()
                .copied()
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(u32::MAX);
            let mut effect = custody_effect(
                CoreCustodyOperation::CleanupRun,
                (
                    CoreInitializationEffect::None,
                    CoreCustodianEffect::Discharge,
                    CoreLoanEffect::None,
                    CoreObligationEffect::Discharge,
                ),
                Arc::from([]),
                None,
                Some(custody_home(b"cleanup", u128::from(ordinal))),
                Some(custody_home(b"discharged", u128::from(ordinal))),
            );
            effect.cleanup_ordinal = Some(ordinal);
            effects.push(effect);
        }
        CoreOperationKind::Branch | CoreOperationKind::Match => effects.push(custody_effect(
            CoreCustodyOperation::Join,
            (
                CoreInitializationEffect::None,
                CoreCustodianEffect::None,
                CoreLoanEffect::None,
                CoreObligationEffect::None,
            ),
            Arc::clone(&operation.details),
            None,
            None,
            None,
        )),
        CoreOperationKind::Loop | CoreOperationKind::LoopBack => effects.push(custody_effect(
            CoreCustodyOperation::LoopFixpoint,
            (
                CoreInitializationEffect::None,
                CoreCustodianEffect::None,
                CoreLoanEffect::None,
                CoreObligationEffect::None,
            ),
            Arc::clone(&operation.details),
            None,
            None,
            None,
        )),
        CoreOperationKind::TerminalPanic => effects.push(custody_effect(
            CoreCustodyOperation::Panic,
            (
                CoreInitializationEffect::None,
                CoreCustodianEffect::None,
                CoreLoanEffect::None,
                CoreObligationEffect::None,
            ),
            Arc::from([]),
            None,
            None,
            None,
        )),
        _ => {}
    }
    Ok(effects)
}

const fn core_access(access: crate::typed_hir::AccessMode) -> CoreAccessLaw {
    match access {
        crate::typed_hir::AccessMode::Copy => CoreAccessLaw::CopyValue,
        crate::typed_hir::AccessMode::Read => CoreAccessLaw::SharedLoan,
        crate::typed_hir::AccessMode::Mut => CoreAccessLaw::ExclusiveLoan,
        crate::typed_hir::AccessMode::Move => CoreAccessLaw::Move,
    }
}

struct ProducerLowerer<'a> {
    cancellation: &'a Cancellation,
    regions: Vec<Region>,
    next_value: u32,
    next_operation: u32,
    rewrites: Vec<RewriteWitness>,
}

impl<'a> ProducerLowerer<'a> {
    fn new(cancellation: &'a Cancellation) -> Self {
        Self {
            cancellation,
            regions: Vec::new(),
            next_value: 0,
            next_operation: 0,
            rewrites: Vec::new(),
        }
    }

    fn finish(self) -> (Arc<[Region]>, Arc<[RewriteWitness]>) {
        (self.regions.into(), self.rewrites.into())
    }

    fn lower_expression_body(&mut self, expression: &Expression) -> Result<RegionId, CoreFailure> {
        let region = self.reserve_region();
        let mut operations = Vec::new();
        let value = self.lower_expression(expression, &mut operations)?;
        self.push_operation(
            &mut operations,
            CoreOperationKind::Return,
            None,
            None,
            [value],
            [],
            [],
            EffectBoundary::None,
            FailureLaw::None,
            expression.source.clone(),
            CoreAccessLaw::None,
        );
        self.install_region(region, operations);
        Ok(region)
    }

    fn lower_block(&mut self, statements: &[Statement]) -> Result<RegionId, CoreFailure> {
        checkpoint(self.cancellation)?;
        let region = self.reserve_region();
        let mut operations = Vec::new();
        for statement in statements {
            checkpoint(self.cancellation)?;
            self.lower_statement(statement, &mut operations)?;
        }
        let exit_source = statements
            .last()
            .map(statement_source)
            .cloned()
            .unwrap_or_else(|| SourceRange::new("<core:empty-scope>", 0, 0));
        self.push_operation(
            &mut operations,
            CoreOperationKind::RecoverableExit,
            None,
            None,
            [],
            [],
            [],
            EffectBoundary::Ownership,
            FailureLaw::None,
            exit_source,
            CoreAccessLaw::None,
        );
        self.install_region(region, operations);
        Ok(region)
    }

    fn reserve_region(&mut self) -> RegionId {
        let identity = RegionId(u32::try_from(self.regions.len()).unwrap_or(u32::MAX));
        self.regions.push(Region {
            identity,
            operations: Arc::from([]),
        });
        identity
    }

    fn install_region(&mut self, region: RegionId, operations: Vec<Operation>) {
        self.regions[region.0 as usize].operations = operations.into();
    }

    #[allow(clippy::too_many_arguments)]
    fn push_operation(
        &mut self,
        operations: &mut Vec<Operation>,
        kind: CoreOperationKind,
        result: Option<ValueId>,
        type_identity: Option<u128>,
        operands: impl IntoIterator<Item = ValueId>,
        successors: impl IntoIterator<Item = RegionId>,
        details: impl IntoIterator<Item = u128>,
        effect: EffectBoundary,
        failure: FailureLaw,
        provenance: SourceRange,
        access: CoreAccessLaw,
    ) {
        let identity = self.next_operation;
        self.next_operation = self.next_operation.saturating_add(1);
        operations.push(Operation {
            identity,
            kind,
            result,
            type_identity,
            operands: operands.into_iter().collect::<Vec<_>>().into(),
            call_binding: Arc::from([]),
            successors: successors.into_iter().collect::<Vec<_>>().into(),
            details: details.into_iter().collect::<Vec<_>>().into(),
            effect,
            access,
            failure,
            provenance,
            custody: Arc::from([]),
        });
    }

    fn value(&mut self) -> ValueId {
        let value = ValueId(self.next_value);
        self.next_value = self.next_value.saturating_add(1);
        value
    }

    fn lower_statement(
        &mut self,
        statement: &Statement,
        operations: &mut Vec<Operation>,
    ) -> Result<(), CoreFailure> {
        match statement {
            Statement::Return { value, source } => {
                let value = value
                    .as_ref()
                    .map(|value| self.lower_expression(value, operations))
                    .transpose()?;
                self.push_operation(
                    operations,
                    CoreOperationKind::Return,
                    None,
                    None,
                    value,
                    [],
                    [],
                    EffectBoundary::None,
                    FailureLaw::None,
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::Panic { value, source } => {
                let value = self.lower_expression(value, operations)?;
                self.push_operation(
                    operations,
                    CoreOperationKind::TerminalPanic,
                    None,
                    None,
                    [value],
                    [],
                    [],
                    EffectBoundary::None,
                    FailureLaw::TerminalPanic,
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::Assert { condition, source } | Statement::Expect { condition, source } => {
                let value = self.lower_expression(condition, operations)?;
                let kind = if matches!(statement, Statement::Assert { .. }) {
                    CoreOperationKind::Assert
                } else {
                    CoreOperationKind::Expect
                };
                self.push_operation(
                    operations,
                    kind,
                    None,
                    None,
                    [value],
                    [],
                    [],
                    EffectBoundary::None,
                    if kind == CoreOperationKind::Assert {
                        FailureLaw::TerminalPanic
                    } else {
                        FailureLaw::RecordTestFailure
                    },
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::Initialize {
                place,
                value,
                source,
            }
            | Statement::Assign {
                place,
                value,
                source,
            } => {
                let mut operands = place_index_values(self, place, operations)?;
                let value = self.lower_expression(value, operations)?;
                operands.push(value);
                let mut details = place_details(place);
                details.push(u128::from(matches!(
                    statement,
                    Statement::Initialize { .. }
                )));
                self.push_operation(
                    operations,
                    CoreOperationKind::Store,
                    None,
                    None,
                    operands,
                    [],
                    details,
                    EffectBoundary::LocalMutation,
                    if place
                        .projections
                        .iter()
                        .any(|projection| matches!(projection, PlaceProjection::Index { .. }))
                    {
                        FailureLaw::BoundsPanic
                    } else {
                        FailureLaw::CheckBeforeSuccess
                    },
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::Evaluate(expression) => {
                self.lower_expression(expression, operations)?;
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
                source,
            } => {
                let condition = self.lower_expression(condition, operations)?;
                let then_region = self.lower_block(then_branch)?;
                let else_region = self.lower_block(else_branch)?;
                self.push_operation(
                    operations,
                    CoreOperationKind::Branch,
                    None,
                    None,
                    [condition],
                    [then_region, else_region],
                    [],
                    EffectBoundary::None,
                    FailureLaw::None,
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::IfPattern {
                value,
                pattern,
                then_branch,
                else_branch,
                source,
            } => {
                let value = self.lower_expression(value, operations)?;
                let then_region = self.lower_block(then_branch)?;
                let else_region = self.lower_block(else_branch)?;
                self.push_operation(
                    operations,
                    CoreOperationKind::Branch,
                    None,
                    None,
                    [value],
                    [then_region, else_region],
                    pattern_details(pattern),
                    EffectBoundary::Ownership,
                    FailureLaw::None,
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::For {
                pattern,
                iterable,
                body,
                source,
            } => {
                let iterable = self.lower_expression(iterable, operations)?;
                let body = self.lower_block(body)?;
                self.push_operation(
                    operations,
                    CoreOperationKind::Loop,
                    None,
                    None,
                    [iterable],
                    [body],
                    pattern_details(pattern),
                    EffectBoundary::Ownership,
                    FailureLaw::None,
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::While {
                condition,
                body,
                max_iterations,
                source,
            } => {
                let condition_region = self.reserve_region();
                let body_region = self.reserve_region();
                let exit_region = self.reserve_region();
                let mut condition_operations = Vec::new();
                let condition_value =
                    self.lower_expression(condition, &mut condition_operations)?;
                self.install_region(condition_region, condition_operations);
                let mut body_operations = Vec::new();
                for statement in body.iter() {
                    self.lower_statement(statement, &mut body_operations)?;
                }
                self.push_operation(
                    &mut body_operations,
                    CoreOperationKind::LoopBack,
                    None,
                    None,
                    [],
                    [condition_region],
                    [],
                    EffectBoundary::None,
                    FailureLaw::None,
                    source.clone(),
                    CoreAccessLaw::None,
                );
                self.install_region(body_region, body_operations);
                self.push_operation(
                    operations,
                    CoreOperationKind::Loop,
                    None,
                    None,
                    [condition_value],
                    [condition_region, body_region, exit_region],
                    [u128::from(*max_iterations)],
                    EffectBoundary::None,
                    FailureLaw::None,
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::Break(source) | Statement::Continue(source) => {
                let kind = if matches!(statement, Statement::Break(_)) {
                    CoreOperationKind::Break
                } else {
                    CoreOperationKind::Continue
                };
                self.push_operation(
                    operations,
                    kind,
                    None,
                    None,
                    [],
                    [],
                    [],
                    EffectBoundary::None,
                    FailureLaw::None,
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::Match {
                value,
                cases,
                source,
            } => {
                let matched_place = root_place(value).map(|place| place_details(&place));
                let value = self.lower_expression(value, operations)?;
                let mut successors = Vec::new();
                let mut details = vec![u128::try_from(cases.len()).unwrap_or(u128::MAX)];
                for case in cases.iter() {
                    let case_region = self.reserve_region();
                    let mut case_operations = Vec::new();
                    if let (Some(pattern), Some(place)) = (&case.pattern, &matched_place)
                        && let Some((binding, type_identity)) =
                            producer_result_payload_move_binding(pattern)
                    {
                        let payload = self.value();
                        self.push_operation(
                            &mut case_operations,
                            CoreOperationKind::Read,
                            Some(payload),
                            Some(type_identity),
                            [],
                            [],
                            place.iter().copied(),
                            EffectBoundary::Ownership,
                            FailureLaw::None,
                            case.source.clone(),
                            CoreAccessLaw::Move,
                        );
                        self.push_operation(
                            &mut case_operations,
                            CoreOperationKind::Store,
                            None,
                            None,
                            [payload],
                            [],
                            [binding, 1],
                            EffectBoundary::Ownership,
                            FailureLaw::None,
                            case.source.clone(),
                            CoreAccessLaw::None,
                        );
                    }
                    if let Some(guard) = &case.guard {
                        let guard_value = self.lower_expression(guard, &mut case_operations)?;
                        details.push(u128::from(guard_value.0));
                    } else {
                        details.push(u128::MAX);
                    }
                    if let Some(pattern) = &case.pattern {
                        details.extend(pattern_details(pattern));
                    }
                    for statement in case.body.iter() {
                        self.lower_statement(statement, &mut case_operations)?;
                    }
                    self.push_operation(
                        &mut case_operations,
                        CoreOperationKind::RecoverableExit,
                        None,
                        None,
                        [],
                        [],
                        [],
                        EffectBoundary::Ownership,
                        FailureLaw::None,
                        source.clone(),
                        CoreAccessLaw::None,
                    );
                    self.install_region(case_region, case_operations);
                    successors.push(case_region);
                }
                self.push_operation(
                    operations,
                    CoreOperationKind::Match,
                    None,
                    None,
                    [value],
                    successors,
                    details,
                    EffectBoundary::Ownership,
                    FailureLaw::None,
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::Defer { action, source } => {
                let mut values = Vec::new();
                for capture in action.captures.iter() {
                    values.push(self.lower_expression(&capture.expression, operations)?);
                }
                let action_region = self.reserve_region();
                let mut action_operations = Vec::new();
                self.lower_expression(&action.expression, &mut action_operations)?;
                let cleanup_identity = self.next_operation;
                for operation in &mut action_operations {
                    if operation.access == CoreAccessLaw::CleanupCapture {
                        let ordinal = operation.details.first().copied().unwrap_or(u128::MAX);
                        operation.details =
                            Arc::from([u128::from(cleanup_identity), ordinal, u128::MAX]);
                    }
                }
                self.install_region(action_region, action_operations);
                self.push_operation(
                    operations,
                    CoreOperationKind::Cleanup,
                    None,
                    None,
                    values,
                    [action_region],
                    [],
                    EffectBoundary::Ownership,
                    FailureLaw::CheckBeforeSuccess,
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::WithPool {
                binding,
                scope,
                body,
                source,
                ..
            } => {
                let scope = self.lower_expression(scope, operations)?;
                let body = self.lower_block(body)?;
                self.push_operation(
                    operations,
                    CoreOperationKind::PoolScope,
                    None,
                    None,
                    [scope],
                    [body],
                    place_details(binding),
                    EffectBoundary::Ownership,
                    FailureLaw::CheckBeforeSuccess,
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::Pass(source) => self.rewrites.push(RewriteWitness {
                kind: CoreRewriteKind::EliminatedPass,
                provenance: source.clone(),
                source_order: u32::try_from(self.rewrites.len()).unwrap_or(u32::MAX),
            }),
        }
        Ok(())
    }

    fn lower_expression(
        &mut self,
        expression: &Expression,
        operations: &mut Vec<Operation>,
    ) -> Result<ValueId, CoreFailure> {
        checkpoint(self.cancellation)?;
        let result = self.value();
        let type_identity = Some(expression.type_id.0);
        let source = expression.source.clone();
        let operation_access = if matches!(expression.kind, ExpressionKind::CleanupCapture(_)) {
            CoreAccessLaw::CleanupCapture
        } else {
            core_access(expression.access)
        };
        let (kind, operands, details, effect, failure) = match &expression.kind {
            ExpressionKind::Literal(literal) => (
                CoreOperationKind::Literal,
                vec![],
                literal_details(literal),
                EffectBoundary::None,
                FailureLaw::None,
            ),
            ExpressionKind::Read(place) => (
                CoreOperationKind::Read,
                place_index_values(self, place, operations)?,
                place_details(place),
                if expression.access == crate::typed_hir::AccessMode::Move {
                    EffectBoundary::Ownership
                } else {
                    EffectBoundary::None
                },
                FailureLaw::CheckBeforeSuccess,
            ),
            ExpressionKind::Constant(identity) => (
                CoreOperationKind::Constant,
                vec![],
                vec![identity.0],
                EffectBoundary::None,
                FailureLaw::PropagateInOrder,
            ),
            ExpressionKind::FunctionValue {
                definition,
                specialization,
            } => (
                CoreOperationKind::FunctionValue,
                vec![],
                vec![definition.0, specialization.map_or(0, |value| value.0)],
                EffectBoundary::None,
                FailureLaw::None,
            ),
            ExpressionKind::Closure(closure) => (
                CoreOperationKind::ClosureValue,
                vec![],
                vec![closure.id.0],
                EffectBoundary::None,
                FailureLaw::None,
            ),
            ExpressionKind::CleanupCapture(capture) => (
                CoreOperationKind::Read,
                vec![],
                vec![u128::from(capture.0), u128::MAX],
                EffectBoundary::Ownership,
                FailureLaw::None,
            ),
            ExpressionKind::Call { target, arguments } => {
                let mut operands = Vec::new();
                if let CallTarget::Callable { value } = target {
                    operands.push(self.lower_expression(value, operations)?);
                }
                for argument in arguments.iter() {
                    operands.push(self.lower_expression(argument, operations)?);
                }
                let (kind, effect) = if matches!(target, CallTarget::Build { .. }) {
                    (
                        CoreOperationKind::BuildConstruction,
                        EffectBoundary::BuildConstruction,
                    )
                } else {
                    (CoreOperationKind::Call, EffectBoundary::Call)
                };
                (
                    kind,
                    operands,
                    call_target_details(target),
                    effect,
                    if kind == CoreOperationKind::Call {
                        FailureLaw::CallPanicPropagation
                    } else {
                        FailureLaw::PropagateInOrder
                    },
                )
            }
            ExpressionKind::Array(values) | ExpressionKind::Tuple(values) => {
                let mut operands = Vec::new();
                for value in values.iter() {
                    operands.push(self.lower_expression(value, operations)?);
                }
                (
                    CoreOperationKind::Aggregate,
                    operands,
                    vec![u128::from(matches!(
                        expression.kind,
                        ExpressionKind::Tuple(_)
                    ))],
                    EffectBoundary::Ownership,
                    FailureLaw::None,
                )
            }
            ExpressionKind::RepeatedArray { value, length } => (
                CoreOperationKind::Aggregate,
                vec![self.lower_expression(value, operations)?],
                vec![2, u128::from(*length)],
                EffectBoundary::Ownership,
                FailureLaw::None,
            ),
            ExpressionKind::Index { value, index } => (
                CoreOperationKind::Index,
                vec![
                    self.lower_expression(value, operations)?,
                    self.lower_expression(index, operations)?,
                ],
                vec![],
                EffectBoundary::None,
                FailureLaw::BoundsPanic,
            ),
            ExpressionKind::Positive(value) => (
                CoreOperationKind::Unary,
                vec![self.lower_expression(value, operations)?],
                vec![1],
                EffectBoundary::None,
                FailureLaw::None,
            ),
            ExpressionKind::Negate(value) => (
                CoreOperationKind::Unary,
                vec![self.lower_expression(value, operations)?],
                vec![2],
                EffectBoundary::None,
                if matches!(expression.type_, Type::Integer(_)) {
                    FailureLaw::ArithmeticPanic
                } else {
                    FailureLaw::None
                },
            ),
            ExpressionKind::BitNot(value) => (
                CoreOperationKind::Unary,
                vec![self.lower_expression(value, operations)?],
                vec![3],
                EffectBoundary::None,
                FailureLaw::None,
            ),
            ExpressionKind::Not(value) => (
                CoreOperationKind::Unary,
                vec![self.lower_expression(value, operations)?],
                vec![4],
                EffectBoundary::None,
                FailureLaw::None,
            ),
            ExpressionKind::Await(value) => (
                CoreOperationKind::Suspension,
                vec![self.lower_expression(value, operations)?],
                vec![],
                EffectBoundary::Suspension,
                FailureLaw::PropagateInOrder,
            ),
            ExpressionKind::TrySend(value) => (
                CoreOperationKind::MessageProposal,
                vec![self.lower_expression(value, operations)?],
                try_send_details(value),
                EffectBoundary::Suspension,
                FailureLaw::CheckBeforeSuccess,
            ),
            ExpressionKind::Propagate(value) => (
                CoreOperationKind::Propagate,
                vec![self.lower_expression(value, operations)?],
                vec![],
                EffectBoundary::Ownership,
                FailureLaw::PropagateInOrder,
            ),
            ExpressionKind::Is { value, pattern } => (
                CoreOperationKind::PatternTest,
                vec![self.lower_expression(value, operations)?],
                pattern_details(pattern),
                EffectBoundary::Ownership,
                FailureLaw::None,
            ),
            ExpressionKind::Binary {
                operator,
                left,
                right,
            } => {
                let kind = binary_kind(*operator);
                if kind == CoreOperationKind::ShortCircuit {
                    let left = self.lower_expression(left, operations)?;
                    let right_region = self.reserve_region();
                    let mut right_operations = Vec::new();
                    let right = self.lower_expression(right, &mut right_operations)?;
                    self.install_region(right_region, right_operations);
                    self.push_operation(
                        operations,
                        kind,
                        Some(result),
                        type_identity,
                        [left, right],
                        [right_region],
                        [u128::from(operator_tag(*operator))],
                        EffectBoundary::None,
                        FailureLaw::None,
                        source,
                        operation_access,
                    );
                    return Ok(result);
                }
                let operands = vec![
                    self.lower_expression(left, operations)?,
                    self.lower_expression(right, operations)?,
                ];
                (
                    kind,
                    operands,
                    vec![u128::from(operator_tag(*operator))],
                    EffectBoundary::None,
                    if kind == CoreOperationKind::CheckedArithmetic {
                        FailureLaw::ArithmeticPanic
                    } else {
                        FailureLaw::None
                    },
                )
            }
        };
        self.push_operation(
            operations,
            kind,
            Some(result),
            type_identity,
            operands,
            [],
            details,
            effect,
            failure,
            source,
            operation_access,
        );
        if let ExpressionKind::Call { target, arguments } = &expression.kind {
            operations
                .last_mut()
                .expect("call operation was just emitted")
                .call_binding = producer_call_binding(target, arguments.len());
        }
        Ok(result)
    }
}

fn facts_from_regions(
    regions: &[Region],
    semantic: crate::completed_semantic::CorePlanningSemanticProgram<'_>,
) -> ExecutableFacts {
    let operations = regions.iter().flat_map(|region| region.operations.iter());
    let mut facts = ExecutableFacts {
        pure: true,
        may_panic: false,
        suspends: false,
        ownership_transfer: false,
        evaluator_eligible: true,
    };
    for operation in operations {
        facts.may_panic |= operation_may_panic(operation, semantic);
        facts.suspends |= operation.effect == EffectBoundary::Suspension;
        facts.ownership_transfer |= operation.effect == EffectBoundary::Ownership;
        facts.pure &= !matches!(
            operation.effect,
            EffectBoundary::BuildConstruction | EffectBoundary::Suspension
        );
    }
    facts.evaluator_eligible = facts.pure && !facts.suspends;
    facts
}

fn operation_may_panic(
    operation: &Operation,
    semantic: crate::completed_semantic::CorePlanningSemanticProgram<'_>,
) -> bool {
    match operation.failure {
        FailureLaw::TerminalPanic | FailureLaw::ArithmeticPanic | FailureLaw::BoundsPanic => true,
        FailureLaw::CallPanicPropagation => direct_call_target(operation)
            .and_then(|identity| semantic.specialization_facts(SpecializationId(identity)))
            .is_none_or(|facts| facts.may_panic),
        FailureLaw::None
        | FailureLaw::CheckBeforeSuccess
        | FailureLaw::PropagateInOrder
        | FailureLaw::RecordTestFailure => false,
    }
}

fn place_index_values(
    lowerer: &mut ProducerLowerer<'_>,
    place: &Place,
    operations: &mut Vec<Operation>,
) -> Result<Vec<ValueId>, CoreFailure> {
    let mut values = Vec::new();
    for projection in place.projections.iter() {
        if let PlaceProjection::Index { index, .. } = projection {
            values.push(lowerer.lower_expression(index, operations)?);
        }
    }
    Ok(values)
}

fn statement_source(statement: &Statement) -> &SourceRange {
    match statement {
        Statement::Return { source, .. }
        | Statement::Panic { source, .. }
        | Statement::Assert { source, .. }
        | Statement::Expect { source, .. }
        | Statement::Initialize { source, .. }
        | Statement::Assign { source, .. }
        | Statement::If { source, .. }
        | Statement::IfPattern { source, .. }
        | Statement::For { source, .. }
        | Statement::While { source, .. }
        | Statement::Match { source, .. }
        | Statement::Defer { source, .. }
        | Statement::WithPool { source, .. } => source,
        Statement::Evaluate(expression) => &expression.source,
        Statement::Break(source) | Statement::Continue(source) | Statement::Pass(source) => source,
    }
}

fn place_details(place: &Place) -> Vec<u128> {
    let mut details = vec![u128::from(place.local.0)];
    for projection in place.projections.iter() {
        match projection {
            PlaceProjection::Field {
                definition, name, ..
            } => {
                details.push(1);
                details.push(definition.0);
                details.push(xxh3_128(name.as_bytes()));
            }
            PlaceProjection::Index { index, .. } => {
                details.push(2);
                details.push(index.type_id.0);
                match &index.kind {
                    ExpressionKind::Literal(literal) => {
                        details.push(1);
                        details.extend(literal_details(literal));
                    }
                    ExpressionKind::Constant(identity) => details.extend([2, identity.0]),
                    _ => details.extend([
                        3,
                        xxh3_128(index.source.path().as_bytes()),
                        u128::from(index.source.start()),
                        u128::from(index.source.end()),
                    ]),
                }
            }
        }
    }
    details
}

fn literal_details(literal: &Literal) -> Vec<u128> {
    match literal {
        Literal::Unit => vec![1],
        Literal::Bool(value) => vec![2, u128::from(*value)],
        Literal::Integer { kind, value } => {
            vec![3, u128::from(kind.canonical_tag()), *value as u128]
        }
        Literal::Float { kind, bits } => {
            vec![4, u128::from(kind.canonical_tag()), u128::from(*bits)]
        }
        Literal::Text(value) => exact_bytes_details(5, value.as_bytes()),
        Literal::Scalar(value) => vec![6, u128::from(u32::from(*value))],
        Literal::Bytes(value) => exact_bytes_details(7, value),
    }
}

fn exact_bytes_details(tag: u128, bytes: &[u8]) -> Vec<u128> {
    let mut details = vec![tag, bytes.len() as u128];
    for chunk in bytes.chunks(16) {
        let mut packed = [0_u8; 16];
        packed[..chunk.len()].copy_from_slice(chunk);
        details.push(u128::from_be_bytes(packed));
    }
    details
}

fn call_target_details(target: &CallTarget) -> Vec<u128> {
    match target {
        CallTarget::Callable { .. } => vec![1],
        CallTarget::TemplateFunction { definition, .. } => vec![2, definition.0],
        CallTarget::Function {
            definition,
            specialization,
            argument_order,
            argument_parameters,
        } => {
            let mut values = vec![3, definition.0, specialization.0];
            values.extend(argument_order.iter().map(|value| u128::from(*value)));
            values.extend(argument_parameters.iter().map(|value| value.0));
            values
        }
        CallTarget::Build { primitive, labels } => {
            let mut values = vec![4, primitive.identity, primitive.definition.0];
            values.extend(labels.iter().map(|label| xxh3_128(label.as_bytes())));
            values
        }
        CallTarget::BuiltinVariant(variant) => {
            vec![5, u128::from(variant.canonical_tag())]
        }
        CallTarget::UserVariant {
            id,
            variant_order,
            argument_order,
            argument_parameters,
            ..
        } => {
            let mut values = vec![6, id.variant, u128::from(*variant_order)];
            values.extend(argument_order.iter().map(|value| u128::from(*value)));
            values.extend(argument_parameters.iter().map(|value| value.0));
            values
        }
        CallTarget::Interface {
            interface,
            alternatives,
            argument_order,
            argument_parameters,
        } => {
            let mut values = vec![7, interface.0];
            for (witness, definition, specialization) in alternatives.iter() {
                values.extend([witness.0, definition.0, specialization.0]);
            }
            values.extend(argument_order.iter().map(|value| u128::from(*value)));
            values.extend(argument_parameters.iter().map(|value| value.0));
            values
        }
        CallTarget::Struct {
            definition,
            argument_field_definitions,
            ..
        } => {
            let mut values = vec![8, definition.0];
            values.extend(argument_field_definitions.iter().map(|field| field.0));
            values
        }
        CallTarget::Test {
            id,
            argument_order,
            argument_parameters,
        } => {
            let mut values = vec![9, id.identity];
            values.extend(argument_order.iter().map(|value| u128::from(*value)));
            values.extend(argument_parameters.iter().map(|value| value.0));
            values
        }
    }
}

fn try_send_details(expression: &Expression) -> Vec<u128> {
    let ExpressionKind::Call { target, arguments } = &expression.kind else {
        return vec![0, 0];
    };
    let destination_handler = match target {
        CallTarget::Function { specialization, .. } => specialization.0,
        _ => 0,
    };
    let moved_resources = arguments
        .iter()
        .filter(|argument| argument.access == crate::typed_hir::AccessMode::Move)
        .count();
    vec![
        destination_handler,
        u128::try_from(moved_resources).unwrap_or(u128::MAX),
    ]
}

fn producer_call_binding(target: &CallTarget, argument_count: usize) -> Arc<[u16]> {
    match target {
        CallTarget::Function { argument_order, .. }
        | CallTarget::UserVariant { argument_order, .. }
        | CallTarget::Interface { argument_order, .. }
        | CallTarget::Test { argument_order, .. } => Arc::clone(argument_order),
        CallTarget::Callable { .. }
        | CallTarget::TemplateFunction { .. }
        | CallTarget::Build { .. }
        | CallTarget::BuiltinVariant(_)
        | CallTarget::Struct { .. } => (0..argument_count)
            .map(|index| u16::try_from(index).unwrap_or(u16::MAX))
            .collect::<Vec<_>>()
            .into(),
    }
}

fn pattern_details(pattern: &HirMatchPattern) -> Vec<u128> {
    let mut output = Vec::new();
    append_pattern(pattern, &mut output);
    output
}

fn producer_result_payload_move_binding(pattern: &HirMatchPattern) -> Option<(u128, u128)> {
    match pattern {
        HirMatchPattern::BuiltinVariant {
            variant: BuiltinVariant::ResultOk | BuiltinVariant::ResultErr,
            payload,
        } => match payload.as_ref() {
            [
                HirMatchPattern::Binding {
                    local,
                    type_id,
                    access: crate::typed_hir::AccessMode::Move,
                    ..
                },
            ] => Some((u128::from(local.0), type_id.0)),
            _ => None,
        },
        _ => None,
    }
}

fn append_pattern(pattern: &HirMatchPattern, output: &mut Vec<u128>) {
    match pattern {
        HirMatchPattern::Wildcard => output.push(1),
        HirMatchPattern::Literal(literal) => {
            output.push(2);
            output.extend(literal_details(literal));
        }
        HirMatchPattern::Variant { id, payload } => {
            output.extend([3, id.variant, payload.len() as u128]);
            payload
                .iter()
                .for_each(|pattern| append_pattern(pattern, output));
        }
        HirMatchPattern::BuiltinVariant { variant, payload } => {
            output.extend([
                9,
                u128::from(variant.canonical_tag()),
                payload.len() as u128,
            ]);
            payload
                .iter()
                .for_each(|pattern| append_pattern(pattern, output));
        }
        HirMatchPattern::Struct { definition, fields } => {
            output.extend([4, definition.0, fields.len() as u128]);
            fields
                .iter()
                .for_each(|pattern| append_pattern(pattern, output));
        }
        HirMatchPattern::Tuple(values) => {
            output.extend([5, values.len() as u128]);
            values
                .iter()
                .for_each(|pattern| append_pattern(pattern, output));
        }
        HirMatchPattern::FixedArray(values) => {
            output.extend([6, values.len() as u128]);
            values
                .iter()
                .for_each(|pattern| append_pattern(pattern, output));
        }
        HirMatchPattern::Or(values) => {
            output.extend([7, values.len() as u128]);
            values
                .iter()
                .for_each(|pattern| append_pattern(pattern, output));
        }
        HirMatchPattern::Binding {
            local,
            type_id,
            access,
            ..
        } => output.extend([
            8,
            u128::from(local.0),
            type_id.0,
            u128::from(access_tag(*access)),
        ]),
    }
}

const fn access_tag(access: crate::typed_hir::AccessMode) -> u8 {
    match access {
        crate::typed_hir::AccessMode::Copy => 1,
        crate::typed_hir::AccessMode::Read => 2,
        crate::typed_hir::AccessMode::Mut => 3,
        crate::typed_hir::AccessMode::Move => 4,
    }
}

const fn binary_kind(operator: BinaryOperator) -> CoreOperationKind {
    match operator {
        BinaryOperator::Add
        | BinaryOperator::Subtract
        | BinaryOperator::Multiply
        | BinaryOperator::Divide
        | BinaryOperator::Remainder => CoreOperationKind::CheckedArithmetic,
        BinaryOperator::And | BinaryOperator::Or => CoreOperationKind::ShortCircuit,
        _ => CoreOperationKind::Binary,
    }
}

const fn operator_tag(operator: BinaryOperator) -> u8 {
    match operator {
        BinaryOperator::Range => 1,
        BinaryOperator::RangeInclusive => 2,
        BinaryOperator::Add => 3,
        BinaryOperator::Subtract => 4,
        BinaryOperator::Multiply => 5,
        BinaryOperator::Divide => 6,
        BinaryOperator::Remainder => 7,
        BinaryOperator::BitAnd => 8,
        BinaryOperator::BitOr => 9,
        BinaryOperator::BitXor => 10,
        BinaryOperator::ShiftLeft => 11,
        BinaryOperator::ShiftRight => 12,
        BinaryOperator::And => 13,
        BinaryOperator::Or => 14,
        BinaryOperator::Equal => 15,
        BinaryOperator::NotEqual => 16,
        BinaryOperator::Less => 17,
        BinaryOperator::LessEqual => 18,
        BinaryOperator::Greater => 19,
        BinaryOperator::GreaterEqual => 20,
    }
}

fn producer_executable_fingerprint(
    executable: &CoreExecutable,
    cancellation: &Cancellation,
) -> Result<u128, CoreFailure> {
    let mut hash = Xxh3::new();
    hash.update(b"wrela.core.executable\0\x02");
    encode_executable(&mut hash, executable, cancellation)?;
    Ok(hash.digest128())
}

fn producer_fingerprint(
    candidate: &VerifiedCoreProgram,
    cancellation: &Cancellation,
) -> Result<u128, CoreFailure> {
    let mut hash = Xxh3::new();
    hash.update(PHASE_SCHEMA.as_bytes());
    hash.update(&SCHEMA_VERSION.to_be_bytes());
    hash.update(&candidate.context.to_be_bytes());
    hash.update(&candidate.planning_fingerprint.to_be_bytes());
    hash.update(&candidate.source_demand_identity.to_be_bytes());
    for executable in candidate.executables.iter() {
        checkpoint(cancellation)?;
        hash.update(&executable.fingerprint.to_be_bytes());
    }
    hash.update(&(candidate.oracle.cases as u64).to_be_bytes());
    hash.update(&[u8::from(candidate.oracle.agrees)]);
    hash.update(&(candidate.oracle.custody_cases as u64).to_be_bytes());
    hash.update(&[u8::from(candidate.oracle.custody_agrees)]);
    hash.update(&candidate.oracle.fingerprint.to_be_bytes());
    Ok(hash.digest128())
}

fn encode_executable(
    hash: &mut Xxh3,
    executable: &CoreExecutable,
    cancellation: &Cancellation,
) -> Result<(), CoreFailure> {
    checkpoint(cancellation)?;
    hash.update(&[executable.reference.kind.tag()]);
    for value in [
        executable.reference.context,
        executable.reference.identity,
        executable.reference.current_meaning,
        executable.semantic_owner,
    ] {
        hash.update(&value.to_be_bytes());
    }
    encode_source(hash, &executable.provenance);
    hash.update(&executable.entry.0.to_be_bytes());
    hash.update(&(executable.signature.parameters.len() as u64).to_be_bytes());
    for parameter in executable.signature.parameters.iter() {
        checkpoint(cancellation)?;
        hash.update(&parameter.local.to_be_bytes());
        hash.update(&parameter.type_.identity.to_be_bytes());
        encode_bytes(&mut *hash, &parameter.type_.shape);
        hash.update(&[parameter.access.tag()]);
    }
    hash.update(&executable.signature.return_type.identity.to_be_bytes());
    encode_bytes(&mut *hash, &executable.signature.return_type.shape);
    hash.update(&executable.source_definition.unwrap_or(0).to_be_bytes());
    hash.update(&[
        u8::from(executable.facts.pure),
        u8::from(executable.facts.may_panic),
        u8::from(executable.facts.suspends),
        u8::from(executable.facts.ownership_transfer),
        u8::from(executable.facts.evaluator_eligible),
    ]);
    for region in executable.regions.iter() {
        checkpoint(cancellation)?;
        hash.update(&region.identity.0.to_be_bytes());
        for operation in region.operations.iter() {
            checkpoint(cancellation)?;
            hash.update(&operation.identity.to_be_bytes());
            hash.update(&[
                operation.kind.tag(),
                operation.effect.tag(),
                operation.access.tag(),
                operation.failure.tag(),
            ]);
            hash.update(
                &operation
                    .result
                    .map_or(u32::MAX, |value| value.0)
                    .to_be_bytes(),
            );
            hash.update(&operation.type_identity.unwrap_or(0).to_be_bytes());
            hash.update(&(operation.call_binding.len() as u64).to_be_bytes());
            for parameter in operation.call_binding.iter() {
                checkpoint(cancellation)?;
                hash.update(&parameter.to_be_bytes());
            }
            for operand in operation.operands.iter() {
                checkpoint(cancellation)?;
                hash.update(&operand.0.to_be_bytes());
            }
            hash.update(b"|");
            for successor in operation.successors.iter() {
                checkpoint(cancellation)?;
                hash.update(&successor.0.to_be_bytes());
            }
            hash.update(b"|");
            for detail in operation.details.iter() {
                checkpoint(cancellation)?;
                hash.update(&detail.to_be_bytes());
            }
            encode_source(hash, &operation.provenance);
            encode_custody(hash, &operation.custody, cancellation)?;
        }
    }
    for rewrite in executable.rewrites.iter() {
        checkpoint(cancellation)?;
        hash.update(&[rewrite.kind.tag()]);
        hash.update(&rewrite.source_order.to_be_bytes());
        encode_source(hash, &rewrite.provenance);
    }
    Ok(())
}

fn encode_custody(
    hash: &mut Xxh3,
    effects: &[CustodyEffect],
    cancellation: &Cancellation,
) -> Result<(), CoreFailure> {
    hash.update(&(effects.len() as u64).to_be_bytes());
    for effect in effects {
        checkpoint(cancellation)?;
        hash.update(&[
            effect.operation.tag(),
            effect.initialization.tag(),
            effect.custodian.tag(),
            effect.loan.tag(),
            effect.obligation.tag(),
        ]);
        hash.update(&(effect.place.len() as u64).to_be_bytes());
        for part in effect.place.iter() {
            hash.update(&part.to_be_bytes());
        }
        for value in [
            effect.type_identity,
            effect.source_home,
            effect.destination_home,
        ] {
            hash.update(&value.unwrap_or(0).to_be_bytes());
        }
        hash.update(&effect.cleanup_ordinal.unwrap_or(u32::MAX).to_be_bytes());
        if let Some(proof) = &effect.proof {
            hash.update(&[1]);
            hash.update(&proof.requirement_identity.to_be_bytes());
            hash.update(&proof.requirement_current_meaning.to_be_bytes());
            hash.update(&proof.source_type_identity.to_be_bytes());
            hash.update(&[u8::from(proof.retains_source_return_type)]);
        } else {
            hash.update(&[0]);
        }
    }
    Ok(())
}

fn encode_source(hash: &mut Xxh3, source: &SourceRange) {
    hash.update(&(source.path().len() as u64).to_be_bytes());
    hash.update(source.path().as_bytes());
    hash.update(&source.start().to_be_bytes());
    hash.update(&source.end().to_be_bytes());
}

fn verify(
    candidate: &VerifiedCoreProgram,
    input: CorePlanningInput<'_>,
    cancellation: &Cancellation,
) -> Result<(), CoreFailure> {
    checkpoint(cancellation)?;
    if candidate.context != input.context_identity()
        || candidate.planning_fingerprint != planning_input_fingerprint(input)
        || candidate.source_demand_identity != input.source_executable_demand().identity()
    {
        return defect("Core program mixes compilation or planning contexts");
    }
    let semantic = input.completed_semantic_program();
    let mut expected_references = Vec::new();
    for reference in input.exact_source_executables() {
        checkpoint(cancellation)?;
        let source = semantic
            .executable_input(reference)
            .ok_or_else(|| CoreFailure::Defect(Arc::from("Core verifier found a dangling body")))?;
        let supplied = candidate
            .executables
            .iter()
            .find(|executable| {
                executable.reference.kind == source_kind(reference.kind())
                    && executable.reference.identity == reference.identity()
            })
            .ok_or_else(|| {
                CoreFailure::Defect(Arc::from("Core verifier found a missing realization"))
            })?;
        VerifierLowerer::verify_source(source, semantic, input, supplied, cancellation)?;
        expected_references.push((source_kind(reference.kind()), reference.identity()));
    }
    for addition in input.generated_executable_additions() {
        checkpoint(cancellation)?;
        let role = input
            .generated_roles()
            .iter()
            .find(|role| role.executable().identity() == addition.identity())
            .ok_or_else(|| CoreFailure::Defect(Arc::from("Core verifier found a dangling role")))?;
        verify_generated_executable(*addition, role, candidate, cancellation)?;
        expected_references.push((CoreExecutableKind::Generated, addition.identity()));
    }
    expected_references.sort_unstable();
    let supplied_references = candidate
        .executables
        .iter()
        .map(|executable| (executable.reference.kind, executable.reference.identity))
        .collect::<Vec<_>>();
    if supplied_references != expected_references {
        return defect(
            "Core executable realization is missing, extra, duplicate, wrong-kind, stale, or semantically false",
        );
    }
    validate_graphs(candidate, cancellation)?;
    validate_custody(candidate, semantic, input, cancellation)?;
    let mut references = BTreeSet::new();
    for executable in candidate.executables.iter() {
        checkpoint(cancellation)?;
        if executable.reference.context != candidate.context
            || !references.insert((executable.reference.kind, executable.reference.identity))
        {
            return defect("Core realization contains mixed-context or duplicate references");
        }
    }
    let expected_oracle = run_oracle(candidate, input, cancellation)?;
    if candidate.oracle != expected_oracle
        || !candidate.oracle.agrees
        || !candidate.oracle.custody_agrees
    {
        return defect("Core differential semantic oracle disagrees with evaluator meaning");
    }
    let fingerprint = verifier_fingerprint(candidate, cancellation)?;
    if candidate.fingerprint != fingerprint {
        return defect("Core program fingerprint is false");
    }
    Ok(())
}

struct VerifierLowerer<'a> {
    cancellation: &'a Cancellation,
    regions: Vec<Region>,
    next_value: u32,
    next_operation: u32,
    rewrites: Vec<RewriteWitness>,
}

struct VerifierReconstruction {
    regions: Arc<[Region]>,
    rewrites: Arc<[RewriteWitness]>,
}

impl<'a> VerifierLowerer<'a> {
    fn reconstruct(
        body: CoreSourceExecutableBody<'_>,
        cancellation: &'a Cancellation,
    ) -> Result<VerifierReconstruction, CoreFailure> {
        let mut lowerer = Self {
            cancellation,
            regions: Vec::new(),
            next_value: 0,
            next_operation: 0,
            rewrites: Vec::new(),
        };
        let entry = match body {
            CoreSourceExecutableBody::Specialization(function) => {
                lowerer.reconstruct_block(&function.body)?
            }
            CoreSourceExecutableBody::Test(test) => lowerer.reconstruct_block(&test.body)?,
            CoreSourceExecutableBody::Closure(closure) => {
                lowerer.reconstruct_expression_body(&closure.body)?
            }
        };
        if entry != RegionId(0) {
            return defect("Core verifier reconstructed a noncanonical entry");
        }
        verifier_link_cleanup_control(
            &mut lowerer.regions,
            &mut lowerer.next_operation,
            cancellation,
        )?;
        Ok(VerifierReconstruction {
            regions: lowerer.regions.into(),
            rewrites: lowerer.rewrites.into(),
        })
    }

    fn reserve_region(&mut self) -> RegionId {
        let identity = RegionId(u32::try_from(self.regions.len()).unwrap_or(u32::MAX));
        self.regions.push(Region {
            identity,
            operations: Arc::from([]),
        });
        identity
    }

    fn install_region(&mut self, identity: RegionId, operations: Vec<Operation>) {
        self.regions[identity.0 as usize].operations = operations.into();
    }

    fn value(&mut self) -> ValueId {
        let identity = ValueId(self.next_value);
        self.next_value = self.next_value.saturating_add(1);
        identity
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        operations: &mut Vec<Operation>,
        kind: CoreOperationKind,
        result: Option<ValueId>,
        type_identity: Option<u128>,
        operands: impl IntoIterator<Item = ValueId>,
        successors: impl IntoIterator<Item = RegionId>,
        details: impl IntoIterator<Item = u128>,
        effect: EffectBoundary,
        failure: FailureLaw,
        provenance: SourceRange,
        access: CoreAccessLaw,
    ) {
        let identity = self.next_operation;
        self.next_operation = self.next_operation.saturating_add(1);
        operations.push(Operation {
            identity,
            kind,
            result,
            type_identity,
            operands: operands.into_iter().collect::<Vec<_>>().into(),
            call_binding: Arc::from([]),
            successors: successors.into_iter().collect::<Vec<_>>().into(),
            details: details.into_iter().collect::<Vec<_>>().into(),
            effect,
            access,
            failure,
            provenance,
            custody: Arc::from([]),
        });
    }

    fn reconstruct_expression_body(
        &mut self,
        expression: &Expression,
    ) -> Result<RegionId, CoreFailure> {
        let region = self.reserve_region();
        let mut operations = Vec::new();
        let value = self.reconstruct_expression(expression, &mut operations)?;
        self.emit(
            &mut operations,
            CoreOperationKind::Return,
            None,
            None,
            [value],
            [],
            [],
            EffectBoundary::None,
            FailureLaw::None,
            expression.source.clone(),
            CoreAccessLaw::None,
        );
        self.install_region(region, operations);
        Ok(region)
    }

    fn reconstruct_block(&mut self, statements: &[Statement]) -> Result<RegionId, CoreFailure> {
        checkpoint(self.cancellation)?;
        let region = self.reserve_region();
        let mut operations = Vec::new();
        for statement in statements {
            checkpoint(self.cancellation)?;
            self.reconstruct_statement(statement, &mut operations)?;
        }
        let exit_source = statements
            .last()
            .map(statement_source)
            .cloned()
            .unwrap_or_else(|| SourceRange::new("<core:empty-scope>", 0, 0));
        self.emit(
            &mut operations,
            CoreOperationKind::RecoverableExit,
            None,
            None,
            [],
            [],
            [],
            EffectBoundary::Ownership,
            FailureLaw::None,
            exit_source,
            CoreAccessLaw::None,
        );
        self.install_region(region, operations);
        Ok(region)
    }

    fn reconstruct_statement(
        &mut self,
        statement: &Statement,
        operations: &mut Vec<Operation>,
    ) -> Result<(), CoreFailure> {
        match statement {
            Statement::Return { value, source } => {
                let value = value
                    .as_ref()
                    .map(|value| self.reconstruct_expression(value, operations))
                    .transpose()?;
                self.emit(
                    operations,
                    CoreOperationKind::Return,
                    None,
                    None,
                    value,
                    [],
                    [],
                    EffectBoundary::None,
                    FailureLaw::None,
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::Panic { value, source } => {
                let value = self.reconstruct_expression(value, operations)?;
                self.emit(
                    operations,
                    CoreOperationKind::TerminalPanic,
                    None,
                    None,
                    [value],
                    [],
                    [],
                    EffectBoundary::None,
                    FailureLaw::TerminalPanic,
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::Assert { condition, source } | Statement::Expect { condition, source } => {
                let value = self.reconstruct_expression(condition, operations)?;
                let (kind, failure) = if matches!(statement, Statement::Assert { .. }) {
                    (CoreOperationKind::Assert, FailureLaw::TerminalPanic)
                } else {
                    (CoreOperationKind::Expect, FailureLaw::RecordTestFailure)
                };
                self.emit(
                    operations,
                    kind,
                    None,
                    None,
                    [value],
                    [],
                    [],
                    EffectBoundary::None,
                    failure,
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::Initialize {
                place,
                value,
                source,
            }
            | Statement::Assign {
                place,
                value,
                source,
            } => {
                let mut operands = self.reconstruct_place_indexes(place, operations)?;
                operands.push(self.reconstruct_expression(value, operations)?);
                let mut details = verifier_place_details(place);
                details.push(u128::from(matches!(
                    statement,
                    Statement::Initialize { .. }
                )));
                self.emit(
                    operations,
                    CoreOperationKind::Store,
                    None,
                    None,
                    operands,
                    [],
                    details,
                    EffectBoundary::LocalMutation,
                    if place
                        .projections
                        .iter()
                        .any(|projection| matches!(projection, PlaceProjection::Index { .. }))
                    {
                        FailureLaw::BoundsPanic
                    } else {
                        FailureLaw::CheckBeforeSuccess
                    },
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::Evaluate(expression) => {
                self.reconstruct_expression(expression, operations)?;
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
                source,
            } => {
                let condition = self.reconstruct_expression(condition, operations)?;
                let then_region = self.reconstruct_block(then_branch)?;
                let else_region = self.reconstruct_block(else_branch)?;
                self.emit(
                    operations,
                    CoreOperationKind::Branch,
                    None,
                    None,
                    [condition],
                    [then_region, else_region],
                    [],
                    EffectBoundary::None,
                    FailureLaw::None,
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::IfPattern {
                value,
                pattern,
                then_branch,
                else_branch,
                source,
            } => {
                let value = self.reconstruct_expression(value, operations)?;
                let then_region = self.reconstruct_block(then_branch)?;
                let else_region = self.reconstruct_block(else_branch)?;
                self.emit(
                    operations,
                    CoreOperationKind::Branch,
                    None,
                    None,
                    [value],
                    [then_region, else_region],
                    verifier_pattern_details(pattern),
                    EffectBoundary::Ownership,
                    FailureLaw::None,
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::For {
                pattern,
                iterable,
                body,
                source,
            } => {
                let iterable = self.reconstruct_expression(iterable, operations)?;
                let body = self.reconstruct_block(body)?;
                self.emit(
                    operations,
                    CoreOperationKind::Loop,
                    None,
                    None,
                    [iterable],
                    [body],
                    verifier_pattern_details(pattern),
                    EffectBoundary::Ownership,
                    FailureLaw::None,
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::While {
                condition,
                body,
                max_iterations,
                source,
            } => {
                let condition_region = self.reserve_region();
                let body_region = self.reserve_region();
                let exit_region = self.reserve_region();
                let mut condition_operations = Vec::new();
                let condition_value =
                    self.reconstruct_expression(condition, &mut condition_operations)?;
                self.install_region(condition_region, condition_operations);
                let mut body_operations = Vec::new();
                for statement in body.iter() {
                    checkpoint(self.cancellation)?;
                    self.reconstruct_statement(statement, &mut body_operations)?;
                }
                self.emit(
                    &mut body_operations,
                    CoreOperationKind::LoopBack,
                    None,
                    None,
                    [],
                    [condition_region],
                    [],
                    EffectBoundary::None,
                    FailureLaw::None,
                    source.clone(),
                    CoreAccessLaw::None,
                );
                self.install_region(body_region, body_operations);
                self.emit(
                    operations,
                    CoreOperationKind::Loop,
                    None,
                    None,
                    [condition_value],
                    [condition_region, body_region, exit_region],
                    [u128::from(*max_iterations)],
                    EffectBoundary::None,
                    FailureLaw::None,
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::Break(source) | Statement::Continue(source) => {
                let kind = if matches!(statement, Statement::Break(_)) {
                    CoreOperationKind::Break
                } else {
                    CoreOperationKind::Continue
                };
                self.emit(
                    operations,
                    kind,
                    None,
                    None,
                    [],
                    [],
                    [],
                    EffectBoundary::None,
                    FailureLaw::None,
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::Match {
                value,
                cases,
                source,
            } => {
                let matched_place = root_place(value).map(|place| place_details(&place));
                let value = self.reconstruct_expression(value, operations)?;
                let mut successors = Vec::new();
                let mut details = vec![u128::try_from(cases.len()).unwrap_or(u128::MAX)];
                for case in cases.iter() {
                    checkpoint(self.cancellation)?;
                    let case_region = self.reserve_region();
                    let mut case_operations = Vec::new();
                    if let (Some(pattern), Some(place)) = (&case.pattern, &matched_place)
                        && let Some((binding, type_identity)) =
                            verifier_result_payload_move_binding(pattern)
                    {
                        let payload = self.value();
                        self.emit(
                            &mut case_operations,
                            CoreOperationKind::Read,
                            Some(payload),
                            Some(type_identity),
                            [],
                            [],
                            place.iter().copied(),
                            EffectBoundary::Ownership,
                            FailureLaw::None,
                            case.source.clone(),
                            CoreAccessLaw::Move,
                        );
                        self.emit(
                            &mut case_operations,
                            CoreOperationKind::Store,
                            None,
                            None,
                            [payload],
                            [],
                            [binding, 1],
                            EffectBoundary::Ownership,
                            FailureLaw::None,
                            case.source.clone(),
                            CoreAccessLaw::None,
                        );
                    }
                    if let Some(guard) = &case.guard {
                        let guard = self.reconstruct_expression(guard, &mut case_operations)?;
                        details.push(u128::from(guard.0));
                    } else {
                        details.push(u128::MAX);
                    }
                    if let Some(pattern) = &case.pattern {
                        details.extend(verifier_pattern_details(pattern));
                    }
                    for statement in case.body.iter() {
                        self.reconstruct_statement(statement, &mut case_operations)?;
                    }
                    self.emit(
                        &mut case_operations,
                        CoreOperationKind::RecoverableExit,
                        None,
                        None,
                        [],
                        [],
                        [],
                        EffectBoundary::Ownership,
                        FailureLaw::None,
                        source.clone(),
                        CoreAccessLaw::None,
                    );
                    self.install_region(case_region, case_operations);
                    successors.push(case_region);
                }
                self.emit(
                    operations,
                    CoreOperationKind::Match,
                    None,
                    None,
                    [value],
                    successors,
                    details,
                    EffectBoundary::Ownership,
                    FailureLaw::None,
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::Defer { action, source } => {
                let mut captures = Vec::new();
                for capture in action.captures.iter() {
                    checkpoint(self.cancellation)?;
                    captures.push(self.reconstruct_expression(&capture.expression, operations)?);
                }
                let action_region = self.reserve_region();
                let mut action_operations = Vec::new();
                self.reconstruct_expression(&action.expression, &mut action_operations)?;
                let cleanup_identity = self.next_operation;
                for operation in &mut action_operations {
                    if operation.access == CoreAccessLaw::CleanupCapture {
                        let ordinal = operation.details.first().copied().unwrap_or(u128::MAX);
                        operation.details =
                            Arc::from([u128::from(cleanup_identity), ordinal, u128::MAX]);
                    }
                }
                self.install_region(action_region, action_operations);
                self.emit(
                    operations,
                    CoreOperationKind::Cleanup,
                    None,
                    None,
                    captures,
                    [action_region],
                    [],
                    EffectBoundary::Ownership,
                    FailureLaw::CheckBeforeSuccess,
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::WithPool {
                binding,
                scope,
                body,
                source,
                ..
            } => {
                let scope = self.reconstruct_expression(scope, operations)?;
                let body = self.reconstruct_block(body)?;
                self.emit(
                    operations,
                    CoreOperationKind::PoolScope,
                    None,
                    None,
                    [scope],
                    [body],
                    verifier_place_details(binding),
                    EffectBoundary::Ownership,
                    FailureLaw::CheckBeforeSuccess,
                    source.clone(),
                    CoreAccessLaw::None,
                );
            }
            Statement::Pass(source) => self.rewrites.push(RewriteWitness {
                kind: CoreRewriteKind::EliminatedPass,
                provenance: source.clone(),
                source_order: u32::try_from(self.rewrites.len()).unwrap_or(u32::MAX),
            }),
        }
        Ok(())
    }

    fn reconstruct_expression(
        &mut self,
        expression: &Expression,
        operations: &mut Vec<Operation>,
    ) -> Result<ValueId, CoreFailure> {
        checkpoint(self.cancellation)?;
        let result = self.value();
        let type_identity = Some(expression.type_id.0);
        let provenance = expression.source.clone();
        let access = if matches!(expression.kind, ExpressionKind::CleanupCapture(_)) {
            CoreAccessLaw::CleanupCapture
        } else {
            verifier_access(expression.access)
        };
        let (kind, operands, details, effect, failure) = match &expression.kind {
            ExpressionKind::Literal(literal) => (
                CoreOperationKind::Literal,
                vec![],
                verifier_literal_details(literal),
                EffectBoundary::None,
                FailureLaw::None,
            ),
            ExpressionKind::Read(place) => (
                CoreOperationKind::Read,
                self.reconstruct_place_indexes(place, operations)?,
                verifier_place_details(place),
                if expression.access == crate::typed_hir::AccessMode::Move {
                    EffectBoundary::Ownership
                } else {
                    EffectBoundary::None
                },
                FailureLaw::CheckBeforeSuccess,
            ),
            ExpressionKind::Constant(identity) => (
                CoreOperationKind::Constant,
                vec![],
                vec![identity.0],
                EffectBoundary::None,
                FailureLaw::PropagateInOrder,
            ),
            ExpressionKind::FunctionValue {
                definition,
                specialization,
            } => (
                CoreOperationKind::FunctionValue,
                vec![],
                vec![definition.0, specialization.map_or(0, |value| value.0)],
                EffectBoundary::None,
                FailureLaw::None,
            ),
            ExpressionKind::Closure(closure) => (
                CoreOperationKind::ClosureValue,
                vec![],
                vec![closure.id.0],
                EffectBoundary::None,
                FailureLaw::None,
            ),
            ExpressionKind::CleanupCapture(capture) => (
                CoreOperationKind::Read,
                vec![],
                vec![u128::from(capture.0), u128::MAX],
                EffectBoundary::Ownership,
                FailureLaw::None,
            ),
            ExpressionKind::Call { target, arguments } => {
                let mut operands = Vec::new();
                if let CallTarget::Callable { value } = target {
                    operands.push(self.reconstruct_expression(value, operations)?);
                }
                for argument in arguments.iter() {
                    checkpoint(self.cancellation)?;
                    operands.push(self.reconstruct_expression(argument, operations)?);
                }
                let (kind, effect) = if matches!(target, CallTarget::Build { .. }) {
                    (
                        CoreOperationKind::BuildConstruction,
                        EffectBoundary::BuildConstruction,
                    )
                } else {
                    (CoreOperationKind::Call, EffectBoundary::Call)
                };
                (
                    kind,
                    operands,
                    verifier_call_target_details(target),
                    effect,
                    if kind == CoreOperationKind::Call {
                        FailureLaw::CallPanicPropagation
                    } else {
                        FailureLaw::PropagateInOrder
                    },
                )
            }
            ExpressionKind::Array(values) | ExpressionKind::Tuple(values) => {
                let mut operands = Vec::new();
                for value in values.iter() {
                    checkpoint(self.cancellation)?;
                    operands.push(self.reconstruct_expression(value, operations)?);
                }
                (
                    CoreOperationKind::Aggregate,
                    operands,
                    vec![u128::from(matches!(
                        expression.kind,
                        ExpressionKind::Tuple(_)
                    ))],
                    EffectBoundary::Ownership,
                    FailureLaw::None,
                )
            }
            ExpressionKind::RepeatedArray { value, length } => (
                CoreOperationKind::Aggregate,
                vec![self.reconstruct_expression(value, operations)?],
                vec![2, u128::from(*length)],
                EffectBoundary::Ownership,
                FailureLaw::None,
            ),
            ExpressionKind::Index { value, index } => (
                CoreOperationKind::Index,
                vec![
                    self.reconstruct_expression(value, operations)?,
                    self.reconstruct_expression(index, operations)?,
                ],
                vec![],
                EffectBoundary::None,
                FailureLaw::BoundsPanic,
            ),
            ExpressionKind::Positive(value) => (
                CoreOperationKind::Unary,
                vec![self.reconstruct_expression(value, operations)?],
                vec![1],
                EffectBoundary::None,
                FailureLaw::None,
            ),
            ExpressionKind::Negate(value) => (
                CoreOperationKind::Unary,
                vec![self.reconstruct_expression(value, operations)?],
                vec![2],
                EffectBoundary::None,
                if matches!(expression.type_, Type::Integer(_)) {
                    FailureLaw::ArithmeticPanic
                } else {
                    FailureLaw::None
                },
            ),
            ExpressionKind::BitNot(value) => (
                CoreOperationKind::Unary,
                vec![self.reconstruct_expression(value, operations)?],
                vec![3],
                EffectBoundary::None,
                FailureLaw::None,
            ),
            ExpressionKind::Not(value) => (
                CoreOperationKind::Unary,
                vec![self.reconstruct_expression(value, operations)?],
                vec![4],
                EffectBoundary::None,
                FailureLaw::None,
            ),
            ExpressionKind::Await(value) => (
                CoreOperationKind::Suspension,
                vec![self.reconstruct_expression(value, operations)?],
                vec![],
                EffectBoundary::Suspension,
                FailureLaw::PropagateInOrder,
            ),
            ExpressionKind::TrySend(value) => (
                CoreOperationKind::MessageProposal,
                vec![self.reconstruct_expression(value, operations)?],
                try_send_details(value),
                EffectBoundary::Suspension,
                FailureLaw::CheckBeforeSuccess,
            ),
            ExpressionKind::Propagate(value) => (
                CoreOperationKind::Propagate,
                vec![self.reconstruct_expression(value, operations)?],
                vec![],
                EffectBoundary::Ownership,
                FailureLaw::PropagateInOrder,
            ),
            ExpressionKind::Is { value, pattern } => (
                CoreOperationKind::PatternTest,
                vec![self.reconstruct_expression(value, operations)?],
                verifier_pattern_details(pattern),
                EffectBoundary::Ownership,
                FailureLaw::None,
            ),
            ExpressionKind::Binary {
                operator,
                left,
                right,
            } => {
                let kind = binary_kind(*operator);
                if kind == CoreOperationKind::ShortCircuit {
                    let left = self.reconstruct_expression(left, operations)?;
                    let right_region = self.reserve_region();
                    let mut right_operations = Vec::new();
                    let right = self.reconstruct_expression(right, &mut right_operations)?;
                    self.install_region(right_region, right_operations);
                    self.emit(
                        operations,
                        kind,
                        Some(result),
                        type_identity,
                        [left, right],
                        [right_region],
                        [u128::from(operator_tag(*operator))],
                        EffectBoundary::None,
                        FailureLaw::None,
                        provenance,
                        access,
                    );
                    return Ok(result);
                }
                (
                    kind,
                    vec![
                        self.reconstruct_expression(left, operations)?,
                        self.reconstruct_expression(right, operations)?,
                    ],
                    vec![u128::from(operator_tag(*operator))],
                    EffectBoundary::None,
                    if kind == CoreOperationKind::CheckedArithmetic {
                        FailureLaw::ArithmeticPanic
                    } else {
                        FailureLaw::None
                    },
                )
            }
        };
        self.emit(
            operations,
            kind,
            Some(result),
            type_identity,
            operands,
            [],
            details,
            effect,
            failure,
            provenance,
            access,
        );
        if let ExpressionKind::Call { target, arguments } = &expression.kind {
            operations
                .last_mut()
                .expect("call operation was just reconstructed")
                .call_binding = verifier_call_binding(target, arguments.len());
        }
        Ok(result)
    }

    fn reconstruct_place_indexes(
        &mut self,
        place: &Place,
        operations: &mut Vec<Operation>,
    ) -> Result<Vec<ValueId>, CoreFailure> {
        let mut values = Vec::new();
        for projection in place.projections.iter() {
            checkpoint(self.cancellation)?;
            if let PlaceProjection::Index { index, .. } = projection {
                values.push(self.reconstruct_expression(index, operations)?);
            }
        }
        Ok(values)
    }
}

fn verifier_link_cleanup_control(
    regions: &mut Vec<Region>,
    next_operation: &mut u32,
    cancellation: &Cancellation,
) -> Result<(), CoreFailure> {
    let source_regions = regions.clone();
    let action_by_registration = source_regions
        .iter()
        .flat_map(|region| region.operations.iter())
        .filter(|operation| operation.kind == CoreOperationKind::Cleanup)
        .filter_map(|operation| Some((operation.identity, *operation.successors.first()?)))
        .collect::<BTreeMap<_, _>>();
    let registration_by_identity = source_regions
        .iter()
        .flat_map(|region| region.operations.iter())
        .filter(|operation| operation.kind == CoreOperationKind::Cleanup)
        .map(|operation| (operation.identity, operation.provenance.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut inherited = BTreeMap::from([(RegionId(0), Vec::<u32>::new())]);
    let mut work = vec![RegionId(0)];
    let mut exits = Vec::<(RegionId, usize, Vec<u32>, Vec<RegionId>)>::new();
    while let Some(region_id) = work.pop() {
        checkpoint(cancellation)?;
        let region = source_regions.get(region_id.0 as usize).ok_or_else(|| {
            CoreFailure::Defect(Arc::from("Core verifier cleanup traversal is malformed"))
        })?;
        let mut active = inherited.get(&region_id).cloned().unwrap_or_default();
        let outer_count = active.len();
        let mut reachable = true;
        for (index, operation) in region.operations.iter().enumerate() {
            checkpoint(cancellation)?;
            if !reachable {
                continue;
            }
            if operation.kind == CoreOperationKind::Cleanup {
                active.push(operation.identity);
            }
            let leaving = match operation.kind {
                CoreOperationKind::Return | CoreOperationKind::Propagate => active.as_slice(),
                CoreOperationKind::Break
                | CoreOperationKind::Continue
                | CoreOperationKind::LoopBack
                | CoreOperationKind::RecoverableExit => &active[outer_count..],
                _ => &[],
            };
            if !leaving.is_empty() {
                exits.push((
                    region_id,
                    index,
                    leaving.iter().rev().copied().collect(),
                    operation.successors.to_vec(),
                ));
            }
            if operation.kind != CoreOperationKind::LoopBack {
                for successor in operation.successors.iter().copied() {
                    let successor_state = if operation.kind == CoreOperationKind::Cleanup {
                        Vec::new()
                    } else {
                        active.clone()
                    };
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        inherited.entry(successor)
                    {
                        entry.insert(successor_state);
                        work.push(successor);
                    }
                }
            }
            reachable &= !matches!(
                operation.kind,
                CoreOperationKind::Return
                    | CoreOperationKind::Propagate
                    | CoreOperationKind::TerminalPanic
                    | CoreOperationKind::Break
                    | CoreOperationKind::Continue
                    | CoreOperationKind::LoopBack
            );
        }
    }
    for (region_id, exit_index, order, original_successors) in exits {
        checkpoint(cancellation)?;
        let mut continuation = original_successors;
        for registration in order.iter().rev().copied() {
            checkpoint(cancellation)?;
            let action = action_by_registration
                .get(&registration)
                .copied()
                .ok_or_else(|| {
                    CoreFailure::Defect(Arc::from(
                        "Core verifier found cleanup without action authority",
                    ))
                })?;
            let provenance = registration_by_identity
                .get(&registration)
                .cloned()
                .ok_or_else(|| {
                    CoreFailure::Defect(Arc::from("Core verifier found cleanup without provenance"))
                })?;
            let run_region = RegionId(u32::try_from(regions.len()).unwrap_or(u32::MAX));
            let mut successors = vec![action];
            successors.extend(continuation.iter().copied());
            regions.push(Region {
                identity: run_region,
                operations: Arc::from([Operation {
                    identity: *next_operation,
                    kind: CoreOperationKind::CleanupRun,
                    result: None,
                    type_identity: None,
                    operands: Arc::from([]),
                    call_binding: Arc::from([]),
                    successors: successors.into(),
                    details: Arc::from([u128::from(registration)]),
                    effect: EffectBoundary::Ownership,
                    access: CoreAccessLaw::None,
                    failure: FailureLaw::CallPanicPropagation,
                    provenance,
                    custody: Arc::from([]),
                }]),
            });
            *next_operation = next_operation.saturating_add(1);
            continuation = vec![run_region];
        }
        let region = &mut regions[region_id.0 as usize];
        let mut operations = region.operations.to_vec();
        operations[exit_index].successors = continuation.into();
        region.operations = operations.into();
    }
    Ok(())
}

fn verifier_place_details(place: &Place) -> Vec<u128> {
    let mut details = vec![u128::from(place.local.0)];
    for projection in place.projections.iter() {
        match projection {
            PlaceProjection::Field {
                definition, name, ..
            } => details.extend([1, definition.0, xxh3_128(name.as_bytes())]),
            PlaceProjection::Index { index, .. } => {
                details.extend([2, index.type_id.0]);
                match &index.kind {
                    ExpressionKind::Literal(literal) => {
                        details.push(1);
                        details.extend(verifier_literal_details(literal));
                    }
                    ExpressionKind::Constant(identity) => details.extend([2, identity.0]),
                    _ => details.extend([
                        3,
                        xxh3_128(index.source.path().as_bytes()),
                        u128::from(index.source.start()),
                        u128::from(index.source.end()),
                    ]),
                }
            }
        }
    }
    details
}

fn verifier_literal_details(literal: &Literal) -> Vec<u128> {
    match literal {
        Literal::Unit => vec![1],
        Literal::Bool(value) => vec![2, u128::from(*value)],
        Literal::Integer { kind, value } => {
            vec![3, u128::from(kind.canonical_tag()), *value as u128]
        }
        Literal::Float { kind, bits } => {
            vec![4, u128::from(kind.canonical_tag()), u128::from(*bits)]
        }
        Literal::Text(value) => verifier_exact_bytes_details(5, value.as_bytes()),
        Literal::Scalar(value) => vec![6, u128::from(u32::from(*value))],
        Literal::Bytes(value) => verifier_exact_bytes_details(7, value),
    }
}

fn verifier_exact_bytes_details(tag: u128, bytes: &[u8]) -> Vec<u128> {
    let mut details = vec![tag, bytes.len() as u128];
    for chunk in bytes.chunks(16) {
        let mut packed = [0_u8; 16];
        packed[..chunk.len()].copy_from_slice(chunk);
        details.push(u128::from_be_bytes(packed));
    }
    details
}

fn verifier_call_target_details(target: &CallTarget) -> Vec<u128> {
    match target {
        CallTarget::Callable { .. } => vec![1],
        CallTarget::TemplateFunction { definition, .. } => vec![2, definition.0],
        CallTarget::Function {
            definition,
            specialization,
            argument_order,
            argument_parameters,
        } => {
            let mut values = vec![3, definition.0, specialization.0];
            values.extend(argument_order.iter().map(|value| u128::from(*value)));
            values.extend(argument_parameters.iter().map(|value| value.0));
            values
        }
        CallTarget::Build { primitive, labels } => {
            let mut values = vec![4, primitive.identity, primitive.definition.0];
            values.extend(labels.iter().map(|label| xxh3_128(label.as_bytes())));
            values
        }
        CallTarget::BuiltinVariant(variant) => vec![5, u128::from(variant.canonical_tag())],
        CallTarget::UserVariant {
            id,
            variant_order,
            argument_order,
            argument_parameters,
            ..
        } => {
            let mut values = vec![6, id.variant, u128::from(*variant_order)];
            values.extend(argument_order.iter().map(|value| u128::from(*value)));
            values.extend(argument_parameters.iter().map(|value| value.0));
            values
        }
        CallTarget::Interface {
            interface,
            alternatives,
            argument_order,
            argument_parameters,
        } => {
            let mut values = vec![7, interface.0];
            for (witness, definition, specialization) in alternatives.iter() {
                values.extend([witness.0, definition.0, specialization.0]);
            }
            values.extend(argument_order.iter().map(|value| u128::from(*value)));
            values.extend(argument_parameters.iter().map(|value| value.0));
            values
        }
        CallTarget::Struct {
            definition,
            argument_field_definitions,
            ..
        } => {
            let mut values = vec![8, definition.0];
            values.extend(argument_field_definitions.iter().map(|field| field.0));
            values
        }
        CallTarget::Test {
            id,
            argument_order,
            argument_parameters,
        } => {
            let mut values = vec![9, id.identity];
            values.extend(argument_order.iter().map(|value| u128::from(*value)));
            values.extend(argument_parameters.iter().map(|value| value.0));
            values
        }
    }
}

fn verifier_call_binding(target: &CallTarget, argument_count: usize) -> Arc<[u16]> {
    match target {
        CallTarget::Function { argument_order, .. }
        | CallTarget::UserVariant { argument_order, .. }
        | CallTarget::Interface { argument_order, .. }
        | CallTarget::Test { argument_order, .. } => Arc::clone(argument_order),
        CallTarget::Callable { .. }
        | CallTarget::TemplateFunction { .. }
        | CallTarget::Build { .. }
        | CallTarget::BuiltinVariant(_)
        | CallTarget::Struct { .. } => (0..argument_count)
            .map(|index| u16::try_from(index).unwrap_or(u16::MAX))
            .collect::<Vec<_>>()
            .into(),
    }
}

fn verifier_pattern_details(pattern: &HirMatchPattern) -> Vec<u128> {
    fn encode(pattern: &HirMatchPattern, output: &mut Vec<u128>) {
        match pattern {
            HirMatchPattern::Wildcard => output.push(1),
            HirMatchPattern::Literal(literal) => {
                output.push(2);
                output.extend(verifier_literal_details(literal));
            }
            HirMatchPattern::Variant { id, payload } => {
                output.extend([3, id.variant, payload.len() as u128]);
                payload.iter().for_each(|pattern| encode(pattern, output));
            }
            HirMatchPattern::BuiltinVariant { variant, payload } => {
                output.extend([
                    9,
                    u128::from(variant.canonical_tag()),
                    payload.len() as u128,
                ]);
                payload.iter().for_each(|pattern| encode(pattern, output));
            }
            HirMatchPattern::Struct { definition, fields } => {
                output.extend([4, definition.0, fields.len() as u128]);
                fields.iter().for_each(|pattern| encode(pattern, output));
            }
            HirMatchPattern::Tuple(values) => {
                output.extend([5, values.len() as u128]);
                values.iter().for_each(|pattern| encode(pattern, output));
            }
            HirMatchPattern::FixedArray(values) => {
                output.extend([6, values.len() as u128]);
                values.iter().for_each(|pattern| encode(pattern, output));
            }
            HirMatchPattern::Or(values) => {
                output.extend([7, values.len() as u128]);
                values.iter().for_each(|pattern| encode(pattern, output));
            }
            HirMatchPattern::Binding {
                local,
                type_id,
                access,
                ..
            } => output.extend([
                8,
                u128::from(local.0),
                type_id.0,
                u128::from(verifier_access_tag(*access)),
            ]),
        }
    }

    let mut output = Vec::new();
    encode(pattern, &mut output);
    output
}

fn verifier_result_payload_move_binding(pattern: &HirMatchPattern) -> Option<(u128, u128)> {
    match pattern {
        HirMatchPattern::BuiltinVariant {
            variant: BuiltinVariant::ResultOk | BuiltinVariant::ResultErr,
            payload,
        } => match payload.as_ref() {
            [
                HirMatchPattern::Binding {
                    local,
                    type_id,
                    access: crate::typed_hir::AccessMode::Move,
                    ..
                },
            ] => Some((u128::from(local.0), type_id.0)),
            _ => None,
        },
        _ => None,
    }
}

const fn verifier_access_tag(access: crate::typed_hir::AccessMode) -> u8 {
    match access {
        crate::typed_hir::AccessMode::Copy => 1,
        crate::typed_hir::AccessMode::Read => 2,
        crate::typed_hir::AccessMode::Mut => 3,
        crate::typed_hir::AccessMode::Move => 4,
    }
}

fn verify_generated_executable(
    reference: ExecutableRef,
    role: &GeneratedRole,
    candidate: &VerifiedCoreProgram,
    cancellation: &Cancellation,
) -> Result<(), CoreFailure> {
    checkpoint(cancellation)?;
    let supplied = candidate
        .executables
        .iter()
        .find(|executable| {
            executable.reference.kind == CoreExecutableKind::Generated
                && executable.reference.identity == reference.identity()
        })
        .ok_or_else(|| {
            CoreFailure::Defect(Arc::from(
                "Core verifier found a missing generated realization",
            ))
        })?;
    let mut details = vec![
        role.reference().identity(),
        role.reference().current_meaning(),
        role.owner().identity(),
        role.generator().identity(),
        u128::from(role.local_key()),
        role.provenance().identity(),
        u128::from(verifier_generated_role_tag(role.kind())),
    ];
    for dependency in role.dependencies() {
        checkpoint(cancellation)?;
        details.extend([dependency.identity(), dependency.current_meaning()]);
    }
    let provenance = SourceRange::new("<generated:mandatory-image>", 0, 0);
    let Some(operation) = supplied
        .regions
        .first()
        .and_then(|region| region.operations.first())
    else {
        return defect("Core generated executable has no role operation");
    };
    if supplied.reference.context != reference.context()
        || supplied.reference.current_meaning != reference.current_meaning()
        || supplied.semantic_owner != role.owner().identity()
        || supplied.provenance != provenance
        || supplied.entry != RegionId(0)
        || supplied.regions.len() != 1
        || supplied.regions[0].identity != RegionId(0)
        || supplied.regions[0].operations.len() != 1
        || operation.identity != 0
        || operation.kind != CoreOperationKind::GeneratedRole
        || operation.result.is_some()
        || operation.type_identity.is_some()
        || !operation.operands.is_empty()
        || !operation.custody.is_empty()
        || !operation.call_binding.is_empty()
        || !operation.successors.is_empty()
        || operation.details.as_ref() != details
        || operation.effect != EffectBoundary::None
        || operation.access != CoreAccessLaw::None
        || operation.failure != FailureLaw::None
        || operation.provenance != provenance
        || !supplied.signature.parameters.is_empty()
        || supplied.signature.return_type.identity != 0
        || supplied.signature.return_type.shape != Type::Unit.canonical_key()
        || !supplied.rewrites.is_empty()
        || supplied.source_definition.is_some()
        || supplied.facts
            != (ExecutableFacts {
                pure: false,
                may_panic: role.kind() == crate::image_planning::GeneratedRoleKind::Panic,
                suspends: false,
                ownership_transfer: false,
                evaluator_eligible: false,
            })
    {
        return defect("Core generated executable contradicts its authenticated role");
    }
    if supplied.fingerprint != verifier_executable_fingerprint(supplied, cancellation)? {
        return defect("Core generated executable fingerprint is false");
    }
    Ok(())
}

const fn verifier_generated_role_tag(kind: crate::image_planning::GeneratedRoleKind) -> u8 {
    match kind {
        crate::image_planning::GeneratedRoleKind::Boot => 1,
        crate::image_planning::GeneratedRoleKind::Scheduler => 2,
        crate::image_planning::GeneratedRoleKind::Terminal => 3,
        crate::image_planning::GeneratedRoleKind::Panic => 4,
        crate::image_planning::GeneratedRoleKind::Shutdown => 5,
        crate::image_planning::GeneratedRoleKind::TestRuntime => 6,
    }
}

impl VerifierLowerer<'_> {
    fn verify_source(
        input: CoreSourceExecutableInput<'_>,
        semantic: crate::completed_semantic::CorePlanningSemanticProgram<'_>,
        planning: CorePlanningInput<'_>,
        supplied: &CoreExecutable,
        cancellation: &Cancellation,
    ) -> Result<(), CoreFailure> {
        checkpoint(cancellation)?;
        let (
            owner,
            provenance,
            parameters,
            parameter_type_ids,
            return_type,
            return_type_id,
            source_definition,
            facts,
        ) = match input.body {
            CoreSourceExecutableBody::Specialization(function) => {
                let facts = semantic
                    .specialization_facts(SpecializationId(input.reference.identity()))
                    .ok_or_else(|| {
                        CoreFailure::Defect(Arc::from("Core verifier found missing solved facts"))
                    })?;
                (
                    function.id.0,
                    &function.source,
                    function.parameters.as_slice(),
                    function.parameter_type_ids.as_ref(),
                    &function.return_type,
                    function.return_type_id,
                    Some(function.id.0),
                    Some(facts),
                )
            }
            CoreSourceExecutableBody::Test(test) => (
                input.reference.identity(),
                &test.source,
                test.parameters.as_slice(),
                test.parameter_type_ids.as_ref(),
                &Type::Unit,
                test.return_type_id,
                None,
                None,
            ),
            CoreSourceExecutableBody::Closure(closure) => {
                if supplied.signature.parameters.len() != closure.parameters.len()
                    || supplied
                        .signature
                        .parameters
                        .iter()
                        .zip(
                            closure
                                .parameters
                                .iter()
                                .zip(closure.parameter_type_ids.iter()),
                        )
                        .any(|(actual, ((local, type_), type_id))| {
                            actual.local != local.0
                                || actual.access != CoreAccessLaw::CopyValue
                                || !verifier_type_matches(&actual.type_, type_, *type_id)
                        })
                {
                    return defect("Core Closure signature is false");
                }
                (
                    closure.id.0,
                    &closure.source,
                    &[][..],
                    &[][..],
                    &closure.return_type,
                    closure.return_type_id,
                    None,
                    None,
                )
            }
        };
        if supplied.reference.context != input.reference.context()
            || supplied.reference.kind != source_kind(input.reference.kind())
            || supplied.reference.identity != input.reference.identity()
            || supplied.reference.current_meaning != input.reference.current_meaning()
            || supplied.semantic_owner != owner
            || &supplied.provenance != provenance
            || supplied.source_definition != source_definition
            || !verifier_type_matches(&supplied.signature.return_type, return_type, return_type_id)
        {
            return defect("Core executable header or return meaning is false");
        }
        if !matches!(input.body, CoreSourceExecutableBody::Closure(_))
            && (supplied.signature.parameters.len() != parameters.len()
                || parameters.len() != parameter_type_ids.len()
                || supplied
                    .signature
                    .parameters
                    .iter()
                    .zip(parameters.iter().zip(parameter_type_ids.iter()))
                    .any(|(actual, ((local, type_, access), type_id))| {
                        actual.local != local.0
                            || actual.access != verifier_access(*access)
                            || !verifier_type_matches(&actual.type_, type_, *type_id)
                    }))
        {
            return defect("Core executable parameter signature is false");
        }
        let reconstruction = VerifierLowerer::reconstruct(input.body, cancellation)?;
        let mut supplied_without_custody = supplied.regions.to_vec();
        for region in &mut supplied_without_custody {
            let mut operations = region.operations.to_vec();
            for operation in &mut operations {
                operation.custody = Arc::from([]);
            }
            region.operations = operations.into();
        }
        if supplied.entry != RegionId(0)
            || supplied_without_custody.as_slice() != reconstruction.regions.as_ref()
        {
            return defect(
                "Core operation graph contradicts exact independent Typed HIR reconstruction",
            );
        }
        if supplied.rewrites != reconstruction.rewrites {
            return defect("Core canonical rewrite witness is false, missing, or misassociated");
        }
        verify_pool_proof_conditions(supplied, semantic, planning, cancellation)?;
        let expected_facts = if let Some(facts) = facts {
            ExecutableFacts {
                pure: facts.pure,
                may_panic: facts.may_panic,
                suspends: facts.suspends,
                ownership_transfer: facts.ownership_transfer,
                evaluator_eligible: facts.evaluator_eligible,
            }
        } else {
            verifier_facts_from_regions(&reconstruction.regions, semantic)
        };
        if supplied.facts != expected_facts {
            return defect("Core solved executable facts are false");
        }
        let expected_fingerprint = verifier_executable_fingerprint(supplied, cancellation)?;
        if supplied.fingerprint != expected_fingerprint {
            return defect("Core executable fingerprint is false");
        }
        Ok(())
    }
}

fn verifier_facts_from_regions(
    regions: &[Region],
    semantic: crate::completed_semantic::CorePlanningSemanticProgram<'_>,
) -> ExecutableFacts {
    let mut facts = ExecutableFacts {
        pure: true,
        may_panic: false,
        suspends: false,
        ownership_transfer: false,
        evaluator_eligible: true,
    };
    for operation in regions.iter().flat_map(|region| region.operations.iter()) {
        facts.may_panic |= match operation.failure {
            FailureLaw::TerminalPanic | FailureLaw::ArithmeticPanic | FailureLaw::BoundsPanic => {
                true
            }
            FailureLaw::CallPanicPropagation => verifier_direct_call_target(operation)
                .and_then(|identity| semantic.specialization_facts(SpecializationId(identity)))
                .is_none_or(|facts| facts.may_panic),
            FailureLaw::None
            | FailureLaw::CheckBeforeSuccess
            | FailureLaw::PropagateInOrder
            | FailureLaw::RecordTestFailure => false,
        };
        facts.suspends |= operation.effect == EffectBoundary::Suspension;
        facts.ownership_transfer |= operation.effect == EffectBoundary::Ownership;
        facts.pure &= !matches!(
            operation.effect,
            EffectBoundary::BuildConstruction | EffectBoundary::Suspension
        );
    }
    facts.evaluator_eligible = facts.pure && !facts.suspends;
    facts
}

fn verifier_direct_call_target(operation: &Operation) -> Option<u128> {
    matches!(operation.details.as_ref(), [3, ..])
        .then(|| operation.details.get(2).copied())
        .flatten()
}

fn verifier_type_matches(actual: &CoreType, expected: &Type, expected_id: TypeId) -> bool {
    actual.identity == expected_id.0 && actual.shape == expected.canonical_key()
}

const fn verifier_access(access: crate::typed_hir::AccessMode) -> CoreAccessLaw {
    match access {
        crate::typed_hir::AccessMode::Copy => CoreAccessLaw::CopyValue,
        crate::typed_hir::AccessMode::Read => CoreAccessLaw::SharedLoan,
        crate::typed_hir::AccessMode::Mut => CoreAccessLaw::ExclusiveLoan,
        crate::typed_hir::AccessMode::Move => CoreAccessLaw::Move,
    }
}

fn verifier_executable_fingerprint(
    executable: &CoreExecutable,
    cancellation: &Cancellation,
) -> Result<u128, CoreFailure> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"wrela.core.executable\0\x02");
    verifier_encode_executable(&mut bytes, executable, cancellation)?;
    Ok(xxh3_128(&bytes))
}

fn verifier_fingerprint(
    candidate: &VerifiedCoreProgram,
    cancellation: &Cancellation,
) -> Result<u128, CoreFailure> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(PHASE_SCHEMA.as_bytes());
    bytes.extend_from_slice(&SCHEMA_VERSION.to_be_bytes());
    bytes.extend_from_slice(&candidate.context.to_be_bytes());
    bytes.extend_from_slice(&candidate.planning_fingerprint.to_be_bytes());
    bytes.extend_from_slice(&candidate.source_demand_identity.to_be_bytes());
    for executable in candidate.executables.iter() {
        checkpoint(cancellation)?;
        bytes.extend_from_slice(
            &verifier_executable_fingerprint(executable, cancellation)?.to_be_bytes(),
        );
    }
    bytes.extend_from_slice(&(candidate.oracle.cases as u64).to_be_bytes());
    bytes.push(u8::from(candidate.oracle.agrees));
    bytes.extend_from_slice(&(candidate.oracle.custody_cases as u64).to_be_bytes());
    bytes.push(u8::from(candidate.oracle.custody_agrees));
    bytes.extend_from_slice(&candidate.oracle.fingerprint.to_be_bytes());
    Ok(xxh3_128(&bytes))
}

fn verifier_encode_executable(
    bytes: &mut Vec<u8>,
    executable: &CoreExecutable,
    cancellation: &Cancellation,
) -> Result<(), CoreFailure> {
    checkpoint(cancellation)?;
    bytes.push(executable.reference.kind.tag());
    bytes.extend_from_slice(&executable.reference.context.to_be_bytes());
    bytes.extend_from_slice(&executable.reference.identity.to_be_bytes());
    bytes.extend_from_slice(&executable.reference.current_meaning.to_be_bytes());
    bytes.extend_from_slice(&executable.semantic_owner.to_be_bytes());
    verifier_encode_source(bytes, &executable.provenance);
    bytes.extend_from_slice(&executable.entry.0.to_be_bytes());
    bytes.extend_from_slice(&(executable.signature.parameters.len() as u64).to_be_bytes());
    for parameter in executable.signature.parameters.iter() {
        checkpoint(cancellation)?;
        bytes.extend_from_slice(&parameter.local.to_be_bytes());
        bytes.extend_from_slice(&parameter.type_.identity.to_be_bytes());
        bytes.extend_from_slice(&(parameter.type_.shape.len() as u64).to_be_bytes());
        bytes.extend_from_slice(&parameter.type_.shape);
        bytes.push(parameter.access.tag());
    }
    bytes.extend_from_slice(&executable.signature.return_type.identity.to_be_bytes());
    bytes.extend_from_slice(&(executable.signature.return_type.shape.len() as u64).to_be_bytes());
    bytes.extend_from_slice(&executable.signature.return_type.shape);
    bytes.extend_from_slice(&executable.source_definition.unwrap_or(0).to_be_bytes());
    bytes.extend([
        u8::from(executable.facts.pure),
        u8::from(executable.facts.may_panic),
        u8::from(executable.facts.suspends),
        u8::from(executable.facts.ownership_transfer),
        u8::from(executable.facts.evaluator_eligible),
    ]);
    for region in executable.regions.iter() {
        checkpoint(cancellation)?;
        bytes.extend_from_slice(&region.identity.0.to_be_bytes());
        for operation in region.operations.iter() {
            checkpoint(cancellation)?;
            bytes.extend_from_slice(&operation.identity.to_be_bytes());
            bytes.extend([
                operation.kind.tag(),
                operation.effect.tag(),
                operation.access.tag(),
                operation.failure.tag(),
            ]);
            bytes.extend_from_slice(
                &operation
                    .result
                    .map_or(u32::MAX, |value| value.0)
                    .to_be_bytes(),
            );
            bytes.extend_from_slice(&operation.type_identity.unwrap_or(0).to_be_bytes());
            bytes.extend_from_slice(&(operation.call_binding.len() as u64).to_be_bytes());
            for parameter in operation.call_binding.iter() {
                checkpoint(cancellation)?;
                bytes.extend_from_slice(&parameter.to_be_bytes());
            }
            operation.operands.iter().try_for_each(|value| {
                checkpoint(cancellation)?;
                bytes.extend_from_slice(&value.0.to_be_bytes());
                Ok::<_, CoreFailure>(())
            })?;
            bytes.push(b'|');
            operation.successors.iter().try_for_each(|region| {
                checkpoint(cancellation)?;
                bytes.extend_from_slice(&region.0.to_be_bytes());
                Ok::<_, CoreFailure>(())
            })?;
            bytes.push(b'|');
            operation.details.iter().try_for_each(|value| {
                checkpoint(cancellation)?;
                bytes.extend_from_slice(&value.to_be_bytes());
                Ok::<_, CoreFailure>(())
            })?;
            verifier_encode_source(bytes, &operation.provenance);
            verifier_encode_custody(bytes, &operation.custody, cancellation)?;
        }
    }
    for rewrite in executable.rewrites.iter() {
        checkpoint(cancellation)?;
        bytes.push(rewrite.kind.tag());
        bytes.extend_from_slice(&rewrite.source_order.to_be_bytes());
        verifier_encode_source(bytes, &rewrite.provenance);
    }
    Ok(())
}

fn verifier_encode_custody(
    bytes: &mut Vec<u8>,
    effects: &[CustodyEffect],
    cancellation: &Cancellation,
) -> Result<(), CoreFailure> {
    bytes.extend_from_slice(&(effects.len() as u64).to_be_bytes());
    for effect in effects {
        checkpoint(cancellation)?;
        bytes.extend([
            effect.operation.tag(),
            effect.initialization.tag(),
            effect.custodian.tag(),
            effect.loan.tag(),
            effect.obligation.tag(),
        ]);
        bytes.extend_from_slice(&(effect.place.len() as u64).to_be_bytes());
        for part in effect.place.iter() {
            bytes.extend_from_slice(&part.to_be_bytes());
        }
        for value in [
            effect.type_identity,
            effect.source_home,
            effect.destination_home,
        ] {
            bytes.extend_from_slice(&value.unwrap_or(0).to_be_bytes());
        }
        bytes.extend_from_slice(&effect.cleanup_ordinal.unwrap_or(u32::MAX).to_be_bytes());
        if let Some(proof) = &effect.proof {
            bytes.push(1);
            bytes.extend_from_slice(&proof.requirement_identity.to_be_bytes());
            bytes.extend_from_slice(&proof.requirement_current_meaning.to_be_bytes());
            bytes.extend_from_slice(&proof.source_type_identity.to_be_bytes());
            bytes.push(u8::from(proof.retains_source_return_type));
        } else {
            bytes.push(0);
        }
    }
    Ok(())
}

fn verifier_encode_source(bytes: &mut Vec<u8>, source: &SourceRange) {
    bytes.extend_from_slice(&(source.path().len() as u64).to_be_bytes());
    bytes.extend_from_slice(source.path().as_bytes());
    bytes.extend_from_slice(&source.start().to_be_bytes());
    bytes.extend_from_slice(&source.end().to_be_bytes());
}

fn validate_graphs(
    candidate: &VerifiedCoreProgram,
    cancellation: &Cancellation,
) -> Result<(), CoreFailure> {
    for executable in candidate.executables.iter() {
        checkpoint(cancellation)?;
        if executable.entry != RegionId(0) || executable.regions.is_empty() {
            return defect("Core control flow has no canonical entry region");
        }
        let region_ids = executable
            .regions
            .iter()
            .map(|region| region.identity)
            .collect::<BTreeSet<_>>();
        if region_ids.len() != executable.regions.len()
            || executable
                .regions
                .iter()
                .enumerate()
                .any(|(index, region)| region.identity.0 as usize != index)
        {
            return defect("Core control flow has duplicate or noncanonical regions");
        }
        let mut operations = BTreeSet::new();
        let mut values = BTreeSet::new();
        let legal_loop_backs = executable
            .regions
            .iter()
            .flat_map(|region| {
                region.operations.iter().filter_map(|operation| {
                    if operation.kind == CoreOperationKind::Loop && operation.successors.len() == 3
                    {
                        Some((operation.successors[1], operation.successors[0]))
                    } else {
                        None
                    }
                })
            })
            .collect::<BTreeSet<_>>();
        for region in executable.regions.iter() {
            for operation in region.operations.iter() {
                checkpoint(cancellation)?;
                if !operations.insert(operation.identity)
                    || operation
                        .result
                        .is_some_and(|result| !values.insert(result))
                {
                    return defect("Core operation graph has duplicate identities");
                }
                if operation.successors.iter().any(|successor| {
                    !region_ids.contains(successor)
                        || (successor.0 <= region.identity.0
                            && !(operation.kind == CoreOperationKind::LoopBack
                                && legal_loop_backs.contains(&(region.identity, *successor)))
                            && operation.kind != CoreOperationKind::CleanupRun)
                }) {
                    return defect("Core operation has a dangling or malformed control target");
                }
                if operation.kind == CoreOperationKind::LoopBack
                    && (operation.successors.len() != 1
                        || !operation.operands.is_empty()
                        || operation.result.is_some())
                {
                    return defect("Core loop backedge has a malformed recurrence law");
                }
                if operation.kind == CoreOperationKind::CleanupRun
                    && (!(1..=2).contains(&operation.successors.len())
                        || operation.details.len() != 1
                        || !operation.operands.is_empty()
                        || operation.result.is_some())
                {
                    return defect("Core cleanup run has malformed success-only sequencing");
                }
            }
        }
        for operation in executable
            .regions
            .iter()
            .flat_map(|region| region.operations.iter())
        {
            if operation
                .operands
                .iter()
                .any(|value| !values.contains(value))
            {
                return defect("Core operation has an invalid value reference");
            }
        }
    }
    Ok(())
}

fn validate_custody(
    candidate: &VerifiedCoreProgram,
    semantic: crate::completed_semantic::CorePlanningSemanticProgram<'_>,
    planning: CorePlanningInput<'_>,
    cancellation: &Cancellation,
) -> Result<(), CoreFailure> {
    let program = semantic.verified_program();
    for executable in candidate.executables.iter() {
        let value_types = executable
            .regions
            .iter()
            .flat_map(|region| region.operations.iter())
            .filter_map(|operation| Some((operation.result?, operation.type_identity?)))
            .collect::<BTreeMap<_, _>>();
        let value_access = executable
            .regions
            .iter()
            .flat_map(|region| region.operations.iter())
            .filter_map(|operation| Some((operation.result?, operation.access)))
            .collect::<BTreeMap<_, _>>();
        let context = CustodyVerifierContext {
            signature: &executable.signature,
            value_types: &value_types,
            value_access: &value_access,
            program,
        };
        let expected_trace =
            verifier_expected_custody_trace(executable, semantic, planning, cancellation)?;
        let mut expected_trace = expected_trace.iter();
        for region in executable.regions.iter() {
            let mut active_loans = Vec::<(CoreLoanEffect, Arc<[u128]>)>::new();
            for operation in region.operations.iter() {
                checkpoint(cancellation)?;
                let exact_expected = expected_trace.next().ok_or_else(|| {
                    CoreFailure::Defect(Arc::from("Core custody trace is shorter than its graph"))
                })?;
                if operation.custody.as_ref() != exact_expected.as_ref() {
                    return defect(
                        "Core custody transition fields do not match their exact source authority",
                    );
                }
                let moved_resource_count = operation
                    .operands
                    .iter()
                    .filter(|operand| {
                        value_types
                            .get(operand)
                            .is_some_and(|identity| program.owns_resource_type(TypeId(*identity)))
                            && !matches!(
                                value_access.get(operand),
                                Some(CoreAccessLaw::SharedLoan | CoreAccessLaw::ExclusiveLoan)
                            )
                    })
                    .count();
                let expected_store_operation = (operation.kind == CoreOperationKind::Store
                    && moved_resource_count > 0)
                    .then(|| {
                        if operation.details.last().copied() == Some(1) {
                            (1, moved_resource_count - 1)
                        } else {
                            (0, moved_resource_count)
                        }
                    });
                let expected_discharges = exact_expected
                    .iter()
                    .filter(|effect| effect.operation == CoreCustodyOperation::Discharge)
                    .filter_map(|effect| Some((Arc::clone(&effect.place), effect.type_identity?)))
                    .collect::<Vec<_>>();
                validate_operation_custody(
                    operation,
                    region.identity == RegionId(0),
                    &context,
                    &[],
                    expected_store_operation,
                    &expected_discharges,
                    exact_expected
                        .iter()
                        .filter(|effect| effect.operation == CoreCustodyOperation::ProofCondition)
                        .count(),
                )?;
                let mut cleanup_runs = Vec::new();
                let mut commits = BTreeSet::new();
                for effect in operation.custody.iter() {
                    checkpoint(cancellation)?;
                    if !valid_custody_law(effect) {
                        return defect("Core custody effect contradicts the closed law catalog");
                    }
                    if effect.operation == CoreCustodyOperation::CleanupRun {
                        let ordinal = effect.cleanup_ordinal.ok_or_else(|| {
                            CoreFailure::Defect(Arc::from("Core cleanup run has no registration"))
                        })?;
                        if cleanup_runs
                            .last()
                            .is_some_and(|previous| *previous <= ordinal)
                        {
                            return defect(
                                "Core cleanup order is not deterministic reverse registration",
                            );
                        }
                        cleanup_runs.push(ordinal);
                    }
                    if matches!(
                        effect.operation,
                        CoreCustodyOperation::Move
                            | CoreCustodyOperation::Reinitialize
                            | CoreCustodyOperation::Replace
                            | CoreCustodyOperation::TransferCommit
                    ) {
                        let commit = (effect.source_home, effect.destination_home);
                        if commit.0.is_none()
                            || commit.1.is_none()
                            || commit.0 == commit.1
                            || !commits.insert(commit)
                        {
                            return defect(
                                "Core Transfer Commit duplicates or loses Resource custody",
                            );
                        }
                    }
                    if matches!(
                        effect.loan,
                        CoreLoanEffect::Shared | CoreLoanEffect::Exclusive
                    ) {
                        if active_loans.iter().any(|(loan, place)| {
                            places_overlap(place, &effect.place)
                                && (*loan == CoreLoanEffect::Exclusive
                                    || effect.loan == CoreLoanEffect::Exclusive)
                        }) {
                            return defect("Core Resource loans alias illegally");
                        }
                        active_loans.push((effect.loan, Arc::clone(&effect.place)));
                    }
                }
                if operation.kind == CoreOperationKind::TerminalPanic
                    && operation
                        .custody
                        .iter()
                        .any(|effect| effect.operation == CoreCustodyOperation::CleanupRun)
                {
                    return defect("Core Panic incorrectly runs source cleanup");
                }
                if operation.kind == CoreOperationKind::Suspension && !active_loans.is_empty() {
                    return defect("Core Resource loan crosses a suspension-capable operation");
                }
                if matches!(
                    operation.kind,
                    CoreOperationKind::Call
                        | CoreOperationKind::BuildConstruction
                        | CoreOperationKind::Store
                        | CoreOperationKind::Return
                        | CoreOperationKind::RecoverableExit
                ) {
                    active_loans.clear();
                }
            }
            if !active_loans.is_empty() {
                return defect("Core Resource loan escapes its operation scope");
            }
        }
    }
    Ok(())
}

fn places_overlap(left: &[u128], right: &[u128]) -> bool {
    !left.is_empty()
        && !right.is_empty()
        && left
            .iter()
            .zip(right.iter())
            .all(|(left, right)| left == right)
}

fn place_is_prefix(prefix: &[u128], place: &[u128]) -> bool {
    !prefix.is_empty()
        && prefix.len() <= place.len()
        && prefix
            .iter()
            .zip(place.iter())
            .all(|(left, right)| left == right)
}

struct CustodyVerifierContext<'a> {
    signature: &'a CoreSignature,
    value_types: &'a BTreeMap<ValueId, u128>,
    value_access: &'a BTreeMap<ValueId, CoreAccessLaw>,
    program: &'a crate::typed_hir::VerifiedProgram,
}

fn validate_operation_custody(
    operation: &Operation,
    entry_region: bool,
    context: &CustodyVerifierContext<'_>,
    expected_cleanup_runs: &[u32],
    expected_store_operation: Option<(usize, usize)>,
    expected_discharges: &[(Arc<[u128]>, u128)],
    expected_proofs: usize,
) -> Result<(), CoreFailure> {
    let CustodyVerifierContext {
        signature,
        value_types,
        value_access,
        program,
    } = context;
    let mut expected = BTreeMap::<CoreCustodyOperation, usize>::new();
    let mut expect = |kind, count| {
        if count > 0 {
            *expected.entry(kind).or_insert(0) += count;
        }
    };
    let resource_result = operation
        .type_identity
        .is_some_and(|identity| program.owns_resource_type(TypeId(identity)));
    let expected_primary = match (operation.kind, operation.access, resource_result) {
        (CoreOperationKind::Read, CoreAccessLaw::Move | CoreAccessLaw::CleanupCapture, true) => {
            Some(CoreCustodyOperation::Move)
        }
        (CoreOperationKind::Read, CoreAccessLaw::SharedLoan, true) => {
            Some(CoreCustodyOperation::SharedLoan)
        }
        (CoreOperationKind::Read, CoreAccessLaw::ExclusiveLoan, true) => {
            Some(CoreCustodyOperation::ExclusiveLoan)
        }
        (CoreOperationKind::Aggregate | CoreOperationKind::BuildConstruction, _, true) => {
            Some(CoreCustodyOperation::Construct)
        }
        (CoreOperationKind::Call, _, true) if operation.details.first() == Some(&8) => {
            Some(CoreCustodyOperation::Construct)
        }
        (CoreOperationKind::Call | CoreOperationKind::Propagate, _, true) => {
            Some(CoreCustodyOperation::TransferCommit)
        }
        _ => None,
    };
    if let Some(primary) = expected_primary {
        expect(primary, 1);
    }
    let moved_operand_count = operation
        .operands
        .iter()
        .filter(|operand| {
            value_types
                .get(operand)
                .is_some_and(|identity| program.owns_resource_type(TypeId(*identity)))
                && !matches!(
                    value_access.get(operand),
                    Some(CoreAccessLaw::SharedLoan | CoreAccessLaw::ExclusiveLoan)
                )
        })
        .count();
    if let Some((transfers, reinitializations)) = expected_store_operation {
        expect(CoreCustodyOperation::TransferCommit, transfers);
        expect(CoreCustodyOperation::Reinitialize, reinitializations);
    } else if matches!(
        operation.kind,
        CoreOperationKind::Call
            | CoreOperationKind::BuildConstruction
            | CoreOperationKind::Aggregate
            | CoreOperationKind::Return
            | CoreOperationKind::Cleanup
            | CoreOperationKind::Propagate
            | CoreOperationKind::PoolScope
    ) {
        expect(CoreCustodyOperation::TransferCommit, moved_operand_count);
    }
    let structural = match operation.kind {
        CoreOperationKind::Cleanup => Some(CoreCustodyOperation::CleanupRegister),
        CoreOperationKind::CleanupRun => Some(CoreCustodyOperation::CleanupRun),
        CoreOperationKind::Branch | CoreOperationKind::Match => Some(CoreCustodyOperation::Join),
        CoreOperationKind::Loop | CoreOperationKind::LoopBack => {
            Some(CoreCustodyOperation::LoopFixpoint)
        }
        CoreOperationKind::TerminalPanic => Some(CoreCustodyOperation::Panic),
        _ => None,
    };
    if let Some(structural) = structural {
        expect(structural, 1);
    }
    expect(
        CoreCustodyOperation::CleanupRun,
        expected_cleanup_runs.len(),
    );
    let _ = (entry_region, signature);
    expect(CoreCustodyOperation::Discharge, expected_discharges.len());
    expect(CoreCustodyOperation::ProofCondition, expected_proofs);
    let mut actual = BTreeMap::<CoreCustodyOperation, usize>::new();
    for effect in operation.custody.iter() {
        *actual.entry(effect.operation).or_insert(0) += 1;
    }
    if actual != expected {
        return defect("Core operation changes, loses, or duplicates Resource custody");
    }
    Ok(())
}

fn verifier_effect(
    operation: CoreCustodyOperation,
    dimensions: (
        CoreInitializationEffect,
        CoreCustodianEffect,
        CoreLoanEffect,
        CoreObligationEffect,
    ),
    place: Arc<[u128]>,
    type_identity: Option<u128>,
    source_home: Option<u128>,
    destination_home: Option<u128>,
) -> CustodyEffect {
    let (initialization, custodian, loan, obligation) = dimensions;
    CustodyEffect {
        operation,
        initialization,
        custodian,
        loan,
        obligation,
        place,
        type_identity,
        source_home,
        destination_home,
        cleanup_ordinal: None,
        proof: None,
    }
}

fn verifier_operation_custody(
    operation: &Operation,
    entry_region: bool,
    context: &CustodyVerifierContext<'_>,
    cleanup_runs: &[u32],
    discharges: &[(Arc<[u128]>, u128)],
) -> Vec<CustodyEffect> {
    let mut effects = Vec::new();
    let resource_result = operation
        .type_identity
        .filter(|identity| context.program.owns_resource_type(TypeId(*identity)));
    if let (Some(result), Some(type_identity)) = (operation.result, resource_result) {
        let destination = custody_home(b"value", u128::from(result.0));
        let effect = match (operation.kind, operation.access) {
            (CoreOperationKind::Read, CoreAccessLaw::Move | CoreAccessLaw::CleanupCapture) => {
                let source = if operation.access == CoreAccessLaw::CleanupCapture {
                    cleanup_capture_home(
                        operation.details.first().copied().unwrap_or(u128::MAX),
                        operation.details.get(1).copied().unwrap_or(u128::MAX),
                    )
                } else {
                    place_home(&operation.details)
                };
                Some(verifier_effect(
                    CoreCustodyOperation::Move,
                    (
                        CoreInitializationEffect::Uninitialize,
                        CoreCustodianEffect::Transfer,
                        CoreLoanEffect::None,
                        CoreObligationEffect::Transfer,
                    ),
                    Arc::clone(&operation.details),
                    Some(type_identity),
                    Some(source),
                    Some(destination),
                ))
            }
            (CoreOperationKind::Read, CoreAccessLaw::SharedLoan) => Some(verifier_effect(
                CoreCustodyOperation::SharedLoan,
                (
                    CoreInitializationEffect::None,
                    CoreCustodianEffect::None,
                    CoreLoanEffect::Shared,
                    CoreObligationEffect::None,
                ),
                Arc::clone(&operation.details),
                Some(type_identity),
                None,
                None,
            )),
            (CoreOperationKind::Read, CoreAccessLaw::ExclusiveLoan) => Some(verifier_effect(
                CoreCustodyOperation::ExclusiveLoan,
                (
                    CoreInitializationEffect::None,
                    CoreCustodianEffect::None,
                    CoreLoanEffect::Exclusive,
                    CoreObligationEffect::None,
                ),
                Arc::clone(&operation.details),
                Some(type_identity),
                None,
                None,
            )),
            (CoreOperationKind::Aggregate | CoreOperationKind::BuildConstruction, _) => {
                Some(verifier_effect(
                    CoreCustodyOperation::Construct,
                    (
                        CoreInitializationEffect::Initialize,
                        CoreCustodianEffect::Establish,
                        CoreLoanEffect::None,
                        CoreObligationEffect::Establish,
                    ),
                    Arc::from([]),
                    Some(type_identity),
                    None,
                    Some(destination),
                ))
            }
            (CoreOperationKind::Call, _) if operation.details.first() == Some(&8) => {
                Some(verifier_effect(
                    CoreCustodyOperation::Construct,
                    (
                        CoreInitializationEffect::Initialize,
                        CoreCustodianEffect::Establish,
                        CoreLoanEffect::None,
                        CoreObligationEffect::Establish,
                    ),
                    Arc::from([]),
                    Some(type_identity),
                    None,
                    Some(destination),
                ))
            }
            (CoreOperationKind::Call | CoreOperationKind::Propagate, _) => Some(verifier_effect(
                CoreCustodyOperation::TransferCommit,
                (
                    CoreInitializationEffect::Initialize,
                    CoreCustodianEffect::Establish,
                    CoreLoanEffect::None,
                    CoreObligationEffect::Establish,
                ),
                Arc::from([]),
                Some(type_identity),
                Some(custody_home(b"operation", u128::from(operation.identity))),
                Some(destination),
            )),
            _ => None,
        };
        effects.extend(effect);
    }
    for (ordinal, operand) in operation
        .operands
        .iter()
        .filter(|operand| {
            context
                .value_types
                .get(operand)
                .is_some_and(|identity| context.program.owns_resource_type(TypeId(*identity)))
                && !matches!(
                    context.value_access.get(operand),
                    Some(CoreAccessLaw::SharedLoan | CoreAccessLaw::ExclusiveLoan)
                )
        })
        .enumerate()
    {
        let type_identity = context.value_types[operand];
        let source = custody_home(b"value", u128::from(operand.0));
        if operation.kind == CoreOperationKind::Store {
            let place: Arc<[u128]> =
                operation.details[..operation.details.len().saturating_sub(1)].into();
            let initialize = operation.details.last().copied() == Some(1);
            effects.push(verifier_effect(
                if initialize {
                    CoreCustodyOperation::TransferCommit
                } else {
                    CoreCustodyOperation::Reinitialize
                },
                (
                    if initialize {
                        CoreInitializationEffect::Initialize
                    } else {
                        CoreInitializationEffect::Reinitialize
                    },
                    CoreCustodianEffect::Transfer,
                    CoreLoanEffect::None,
                    CoreObligationEffect::Transfer,
                ),
                Arc::clone(&place),
                Some(type_identity),
                Some(source),
                Some(place_home(&place)),
            ));
        } else if operation.kind == CoreOperationKind::PoolScope {
            effects.push(verifier_effect(
                CoreCustodyOperation::TransferCommit,
                (
                    CoreInitializationEffect::Initialize,
                    CoreCustodianEffect::Transfer,
                    CoreLoanEffect::None,
                    CoreObligationEffect::Transfer,
                ),
                Arc::clone(&operation.details),
                Some(type_identity),
                Some(source),
                Some(place_home(&operation.details)),
            ));
        } else if matches!(
            operation.kind,
            CoreOperationKind::Call
                | CoreOperationKind::BuildConstruction
                | CoreOperationKind::Aggregate
                | CoreOperationKind::Return
                | CoreOperationKind::Cleanup
                | CoreOperationKind::Propagate
        ) {
            let destination = if operation.kind == CoreOperationKind::Cleanup {
                cleanup_capture_home(u128::from(operation.identity), ordinal as u128)
            } else {
                custody_home(
                    if operation.kind == CoreOperationKind::Return {
                        b"return" as &[u8]
                    } else {
                        b"operation"
                    },
                    u128::from(operation.identity) << 32 | ordinal as u128,
                )
            };
            effects.push(verifier_effect(
                CoreCustodyOperation::TransferCommit,
                (
                    CoreInitializationEffect::Uninitialize,
                    CoreCustodianEffect::Transfer,
                    CoreLoanEffect::None,
                    CoreObligationEffect::Transfer,
                ),
                Arc::from([]),
                Some(type_identity),
                Some(source),
                Some(destination),
            ));
        }
    }
    let structural = match operation.kind {
        CoreOperationKind::Cleanup => Some((
            CoreCustodyOperation::CleanupRegister,
            CoreObligationEffect::Establish,
        )),
        CoreOperationKind::CleanupRun => Some((
            CoreCustodyOperation::CleanupRun,
            CoreObligationEffect::Discharge,
        )),
        CoreOperationKind::Branch | CoreOperationKind::Match => {
            Some((CoreCustodyOperation::Join, CoreObligationEffect::None))
        }
        CoreOperationKind::Loop | CoreOperationKind::LoopBack => Some((
            CoreCustodyOperation::LoopFixpoint,
            CoreObligationEffect::None,
        )),
        CoreOperationKind::TerminalPanic => {
            Some((CoreCustodyOperation::Panic, CoreObligationEffect::None))
        }
        _ => None,
    };
    if let Some((kind, obligation)) = structural {
        let cleanup_run = kind == CoreCustodyOperation::CleanupRun;
        let cleanup_ordinal = operation
            .details
            .first()
            .copied()
            .and_then(|value| u32::try_from(value).ok());
        let mut effect = verifier_effect(
            kind,
            (
                CoreInitializationEffect::None,
                if cleanup_run {
                    CoreCustodianEffect::Discharge
                } else {
                    CoreCustodianEffect::None
                },
                CoreLoanEffect::None,
                obligation,
            ),
            if matches!(
                kind,
                CoreCustodyOperation::Join | CoreCustodyOperation::LoopFixpoint
            ) {
                Arc::clone(&operation.details)
            } else {
                Arc::from([])
            },
            None,
            cleanup_run
                .then(|| custody_home(b"cleanup", u128::from(cleanup_ordinal.unwrap_or(u32::MAX)))),
            (kind == CoreCustodyOperation::CleanupRegister)
                .then(|| custody_home(b"cleanup", u128::from(operation.identity)))
                .or_else(|| {
                    cleanup_run.then(|| {
                        custody_home(
                            b"discharged",
                            u128::from(cleanup_ordinal.unwrap_or(u32::MAX)),
                        )
                    })
                }),
        );
        if kind == CoreCustodyOperation::CleanupRegister {
            effect.cleanup_ordinal = Some(operation.identity);
        } else if cleanup_run {
            effect.cleanup_ordinal = cleanup_ordinal;
        }
        effects.push(effect);
    }
    effects.extend(cleanup_runs.iter().map(|ordinal| {
        let mut effect = verifier_effect(
            CoreCustodyOperation::CleanupRun,
            (
                CoreInitializationEffect::None,
                CoreCustodianEffect::Discharge,
                CoreLoanEffect::None,
                CoreObligationEffect::Discharge,
            ),
            Arc::from([]),
            None,
            Some(custody_home(b"cleanup", u128::from(*ordinal))),
            Some(custody_home(b"discharged", u128::from(*ordinal))),
        );
        effect.cleanup_ordinal = Some(*ordinal);
        effect
    }));
    let _ = entry_region;
    effects.extend(discharges.iter().map(|(place, type_identity)| {
        let destination_domain = if context
            .program
            .requires_explicit_discharge(TypeId(*type_identity))
        {
            b"explicit-discharge" as &[u8]
        } else {
            b"compiler-reclaim"
        };
        verifier_effect(
            CoreCustodyOperation::Discharge,
            (
                CoreInitializationEffect::Uninitialize,
                CoreCustodianEffect::Discharge,
                CoreLoanEffect::None,
                CoreObligationEffect::Discharge,
            ),
            Arc::clone(place),
            Some(*type_identity),
            Some(place_home(place)),
            Some(custody_home(destination_domain, place_home(place))),
        )
    }));
    effects
}

fn valid_custody_law(effect: &CustodyEffect) -> bool {
    let dimensions = (
        effect.initialization,
        effect.custodian,
        effect.loan,
        effect.obligation,
    );
    match effect.operation {
        CoreCustodyOperation::Construct => {
            dimensions
                == (
                    CoreInitializationEffect::Initialize,
                    CoreCustodianEffect::Establish,
                    CoreLoanEffect::None,
                    CoreObligationEffect::Establish,
                )
                && effect.type_identity.is_some()
                && effect.source_home.is_none()
                && effect.destination_home.is_some()
        }
        CoreCustodyOperation::Move => {
            dimensions
                == (
                    CoreInitializationEffect::Uninitialize,
                    CoreCustodianEffect::Transfer,
                    CoreLoanEffect::None,
                    CoreObligationEffect::Transfer,
                )
                && !effect.place.is_empty()
                && effect.type_identity.is_some()
        }
        CoreCustodyOperation::Reinitialize => {
            dimensions
                == (
                    CoreInitializationEffect::Reinitialize,
                    CoreCustodianEffect::Transfer,
                    CoreLoanEffect::None,
                    CoreObligationEffect::Transfer,
                )
                && !effect.place.is_empty()
                && effect.type_identity.is_some()
        }
        CoreCustodyOperation::SharedLoan | CoreCustodyOperation::ExclusiveLoan => {
            dimensions
                == (
                    CoreInitializationEffect::None,
                    CoreCustodianEffect::None,
                    if effect.operation == CoreCustodyOperation::SharedLoan {
                        CoreLoanEffect::Shared
                    } else {
                        CoreLoanEffect::Exclusive
                    },
                    CoreObligationEffect::None,
                )
                && !effect.place.is_empty()
                && effect.type_identity.is_some()
                && effect.source_home.is_none()
                && effect.destination_home.is_none()
        }
        CoreCustodyOperation::Replace => {
            dimensions
                == (
                    CoreInitializationEffect::Replace,
                    CoreCustodianEffect::Transfer,
                    CoreLoanEffect::None,
                    CoreObligationEffect::Transfer,
                )
                && !effect.place.is_empty()
                && effect.type_identity.is_some()
        }
        CoreCustodyOperation::TransferCommit => {
            matches!(
                dimensions,
                (
                    CoreInitializationEffect::Initialize | CoreInitializationEffect::Uninitialize,
                    CoreCustodianEffect::Transfer | CoreCustodianEffect::Establish,
                    CoreLoanEffect::None,
                    CoreObligationEffect::Transfer | CoreObligationEffect::Establish,
                )
            ) && effect.type_identity.is_some()
        }
        CoreCustodyOperation::Discharge => {
            dimensions
                == (
                    CoreInitializationEffect::Uninitialize,
                    CoreCustodianEffect::Discharge,
                    CoreLoanEffect::None,
                    CoreObligationEffect::Discharge,
                )
                && effect.type_identity.is_some()
        }
        CoreCustodyOperation::CleanupRegister => {
            dimensions
                == (
                    CoreInitializationEffect::None,
                    CoreCustodianEffect::None,
                    CoreLoanEffect::None,
                    CoreObligationEffect::Establish,
                )
                && effect.cleanup_ordinal.is_some()
                && effect.destination_home.is_some()
        }
        CoreCustodyOperation::CleanupRun => {
            dimensions
                == (
                    CoreInitializationEffect::None,
                    CoreCustodianEffect::Discharge,
                    CoreLoanEffect::None,
                    CoreObligationEffect::Discharge,
                )
                && effect.cleanup_ordinal.is_some()
        }
        CoreCustodyOperation::Join | CoreCustodyOperation::LoopFixpoint => {
            dimensions
                == (
                    CoreInitializationEffect::None,
                    CoreCustodianEffect::None,
                    CoreLoanEffect::None,
                    CoreObligationEffect::None,
                )
                && effect.proof.is_none()
        }
        CoreCustodyOperation::ProofCondition => {
            dimensions
                == (
                    CoreInitializationEffect::None,
                    CoreCustodianEffect::None,
                    CoreLoanEffect::None,
                    CoreObligationEffect::None,
                )
                && effect.proof.as_ref().is_some_and(|proof| {
                    proof.requirement_identity != 0
                        && proof.requirement_current_meaning != 0
                        && proof.source_type_identity != 0
                        && proof.retains_source_return_type
                })
        }
        CoreCustodyOperation::Panic => {
            dimensions
                == (
                    CoreInitializationEffect::None,
                    CoreCustodianEffect::None,
                    CoreLoanEffect::None,
                    CoreObligationEffect::None,
                )
                && effect.proof.is_none()
        }
    }
}

fn run_oracle(
    candidate: &VerifiedCoreProgram,
    input: CorePlanningInput<'_>,
    cancellation: &Cancellation,
) -> Result<OracleSummary, CoreFailure> {
    let semantic = input.completed_semantic_program();
    let mut cases = 0usize;
    let mut agrees = true;
    let mut hash = Xxh3::new();
    hash.update(b"wrela.core.semantic-oracle\0\x01");
    for executable in candidate.executables.iter() {
        checkpoint(cancellation)?;
        if executable.reference.kind != CoreExecutableKind::SourceSpecialization
            || !executable.signature.parameters.is_empty()
            || !executable.facts.pure
            || !executable.facts.evaluator_eligible
        {
            continue;
        }
        let mut support_budget = OracleBudget::new(candidate, cancellation);
        if !oracle_supported(
            candidate,
            executable,
            &mut BTreeSet::new(),
            &mut support_budget,
        )? {
            continue;
        }
        let source_reference = input
            .exact_source_executables()
            .find(|reference| {
                reference.kind() == CoreSourceExecutableKind::Specialization
                    && reference.identity() == executable.reference.identity
                    && reference.current_meaning() == executable.reference.current_meaning
            })
            .ok_or_else(|| CoreFailure::Defect(Arc::from("oracle source reference is missing")))?;
        let source = semantic
            .executable_input(source_reference)
            .ok_or_else(|| CoreFailure::Defect(Arc::from("oracle source body is missing")))?;
        let CoreSourceExecutableBody::Specialization(function) = source.body else {
            return defect("oracle specialization has the wrong source kind");
        };
        let mut work = OracleBudget::for_execution(candidate, executable, cancellation)?;
        let core_outcome = oracle_execute(candidate, executable, &[], &mut work)?;
        let evaluator = crate::evaluator::Engine::new(semantic.verified_program(), cancellation)
            .evaluate_function(function.id)
            .outcome;
        match &evaluator {
            EvaluationOutcome::Cancelled => return Err(CoreFailure::Cancelled),
            EvaluationOutcome::Defect { evidence } => {
                return Err(CoreFailure::Defect(Arc::clone(evidence)));
            }
            _ => {}
        }
        let same = oracle_matches_evaluator(&core_outcome, &evaluator);
        cases += 1;
        agrees &= same;
        hash.update(&executable.reference.identity.to_be_bytes());
        hash.update(&[u8::from(same)]);
        encode_oracle_outcome(&mut hash, &core_outcome);
    }
    let (custody_cases, custody_agrees, custody_fingerprint) =
        run_custody_oracle(candidate, input, cancellation)?;
    hash.update(&custody_fingerprint.to_be_bytes());
    Ok(OracleSummary {
        cases,
        agrees,
        custody_cases,
        custody_agrees,
        fingerprint: hash.digest128(),
    })
}

fn run_custody_oracle(
    candidate: &VerifiedCoreProgram,
    input: CorePlanningInput<'_>,
    cancellation: &Cancellation,
) -> Result<(usize, bool, u128), CoreFailure> {
    let semantic = input.completed_semantic_program();
    let program = semantic.verified_program();
    let mut cases = 0;
    let mut agrees = true;
    let mut hash = Xxh3::new();
    hash.update(b"wrela.core.custody-oracle\0\x02");
    for reference in input.exact_source_executables() {
        checkpoint(cancellation)?;
        let source = semantic.executable_input(reference).ok_or_else(|| {
            CoreFailure::Defect(Arc::from("custody oracle source body is missing"))
        })?;
        let executable = candidate
            .executables
            .iter()
            .find(|executable| {
                executable.reference.kind == source_kind(reference.kind())
                    && executable.reference.identity == reference.identity()
            })
            .ok_or_else(|| CoreFailure::Defect(Arc::from("custody oracle Core body is missing")))?;
        let expected = source_custody_contract(source.body, program, cancellation)?;
        let actual = core_custody_contract(executable, cancellation)?;
        let same = expected == actual;
        cases += 1;
        agrees &= same;
        hash.update(&reference.identity().to_be_bytes());
        hash.update(&[u8::from(same)]);
        encode_source_custody_contract(&mut hash, &expected, cancellation)?;
        encode_source_custody_contract(&mut hash, &actual, cancellation)?;
    }
    Ok((cases, agrees, hash.digest128()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceCustodyContract {
    events: Vec<SourceCustodyEvent>,
    cleanup_runs: usize,
    root_discharges: SourceRootDischarges,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SourceCustodyEvent {
    operation: CoreCustodyOperation,
    type_identity: Option<u128>,
    place: Arc<[u128]>,
    source_path: Arc<str>,
    source_start: u64,
    source_end: u64,
    control_path: Arc<[u32]>,
}

fn source_custody_event(
    events: &mut Vec<SourceCustodyEvent>,
    operation: CoreCustodyOperation,
    type_identity: Option<u128>,
    place: impl Into<Arc<[u128]>>,
    source: &SourceRange,
    control_path: &[u32],
) {
    events.push(SourceCustodyEvent {
        operation,
        type_identity,
        place: place.into(),
        source_path: Arc::from(source.path()),
        source_start: source.start(),
        source_end: source.end(),
        control_path: Arc::from(control_path),
    });
}

fn source_custody_contract(
    body: CoreSourceExecutableBody<'_>,
    program: &crate::typed_hir::VerifiedProgram,
    cancellation: &Cancellation,
) -> Result<SourceCustodyContract, CoreFailure> {
    let mut events = Vec::new();
    match body {
        CoreSourceExecutableBody::Specialization(function) => {
            source_custody_statements(&function.body, &[], program, cancellation, &mut events)?
        }
        CoreSourceExecutableBody::Test(test) => {
            source_custody_statements(&test.body, &[], program, cancellation, &mut events)?
        }
        CoreSourceExecutableBody::Closure(closure) => {
            source_custody_expression(&closure.body, &[], program, cancellation, &mut events)?
        }
    }
    events.sort();
    let statements = match body {
        CoreSourceExecutableBody::Specialization(function) => Some(&*function.body),
        CoreSourceExecutableBody::Test(test) => Some(&*test.body),
        CoreSourceExecutableBody::Closure(_) => None,
    };
    let cleanup_runs = match statements {
        Some(statements) => source_cleanup_run_count(statements, 0, cancellation)?,
        None => 0,
    };
    let root_discharges = source_root_discharge_contract(body, program, cancellation)?;
    Ok(SourceCustodyContract {
        events,
        cleanup_runs,
        root_discharges,
    })
}

type SourceLiveRoots = BTreeMap<Arc<[u128]>, u128>;

fn source_root_discharge_contract(
    body: CoreSourceExecutableBody<'_>,
    program: &crate::typed_hir::VerifiedProgram,
    cancellation: &Cancellation,
) -> Result<SourceRootDischarges, CoreFailure> {
    let (parameters, type_ids, statements) = match body {
        CoreSourceExecutableBody::Specialization(function) => (
            function.parameters.as_slice(),
            &*function.parameter_type_ids,
            Some(&*function.body),
        ),
        CoreSourceExecutableBody::Test(test) => (
            test.parameters.as_slice(),
            &*test.parameter_type_ids,
            Some(&*test.body),
        ),
        CoreSourceExecutableBody::Closure(closure) => {
            let parameters = closure
                .parameters
                .iter()
                .zip(closure.parameter_type_ids.iter())
                .filter(|(_, type_id)| program.owns_resource_type(**type_id))
                .map(|((local, _), type_id)| {
                    (Arc::<[u128]>::from([u128::from(local.0)]), type_id.0)
                })
                .collect::<SourceLiveRoots>();
            let mut discharges = BTreeMap::new();
            let mut live = parameters;
            source_flow_expression(&closure.body, program, &mut live, cancellation)?;
            for entry in live {
                *discharges.entry(entry).or_insert(0) += 1;
            }
            return Ok(discharges);
        }
    };
    let live = parameters
        .iter()
        .zip(type_ids.iter())
        .filter(|((_, _, access), type_id)| {
            *access == crate::typed_hir::AccessMode::Move && program.owns_resource_type(**type_id)
        })
        .map(|((local, _, _), type_id)| (Arc::<[u128]>::from([u128::from(local.0)]), type_id.0))
        .collect::<SourceLiveRoots>();
    let mut discharges = BTreeMap::new();
    if let Some(statements) = statements {
        source_flow_statements(
            statements,
            live,
            &BTreeMap::new(),
            true,
            program,
            cancellation,
            &mut discharges,
        )?;
    }
    Ok(discharges)
}

fn source_record_root_discharges(
    live: &SourceLiveRoots,
    inherited: &SourceLiveRoots,
    all: bool,
    discharges: &mut BTreeMap<(Arc<[u128]>, u128), usize>,
) {
    for (place, type_identity) in live {
        if place.len() == 1 && (all || inherited.get(place) != Some(type_identity)) {
            *discharges
                .entry((Arc::clone(place), *type_identity))
                .or_insert(0) += 1;
        }
    }
}

fn source_flow_expression(
    expression: &Expression,
    program: &crate::typed_hir::VerifiedProgram,
    live: &mut SourceLiveRoots,
    cancellation: &Cancellation,
) -> Result<(), CoreFailure> {
    checkpoint(cancellation)?;
    match &expression.kind {
        ExpressionKind::Read(place) => {
            for projection in place.projections.iter() {
                if let PlaceProjection::Index { index, .. } = projection {
                    source_flow_expression(index, program, live, cancellation)?;
                }
            }
            if expression.access == crate::typed_hir::AccessMode::Move
                && program.owns_resource_type(expression.type_id)
            {
                let details = place_details(place);
                live.retain(|candidate, _| !places_overlap(candidate, &details));
            }
        }
        _ => {
            let mut result = Ok(());
            expression.visit_children(&mut |child| {
                if result.is_ok() {
                    result = source_flow_expression(child, program, live, cancellation);
                }
            });
            result?;
        }
    }
    Ok(())
}

fn source_flow_statements(
    statements: &[Statement],
    mut live: SourceLiveRoots,
    inherited: &SourceLiveRoots,
    root: bool,
    program: &crate::typed_hir::VerifiedProgram,
    cancellation: &Cancellation,
    discharges: &mut BTreeMap<(Arc<[u128]>, u128), usize>,
) -> Result<Option<SourceLiveRoots>, CoreFailure> {
    let mut known = live.clone();
    for statement in statements {
        checkpoint(cancellation)?;
        match statement {
            Statement::Return { value, .. } => {
                if let Some(value) = value {
                    source_flow_expression(value, program, &mut live, cancellation)?;
                }
                source_record_root_discharges(&live, inherited, true, discharges);
                return Ok(None);
            }
            Statement::Panic { value, .. } => {
                source_flow_expression(value, program, &mut live, cancellation)?;
                return Ok(None);
            }
            Statement::Assert { condition, .. } | Statement::Expect { condition, .. } => {
                source_flow_expression(condition, program, &mut live, cancellation)?;
            }
            Statement::Initialize { place, value, .. } | Statement::Assign { place, value, .. } => {
                source_flow_expression(value, program, &mut live, cancellation)?;
                if program.owns_resource_type(value.type_id) {
                    let place = Arc::from(place_details(place));
                    live.insert(Arc::clone(&place), value.type_id.0);
                    known.insert(place, value.type_id.0);
                }
            }
            Statement::Evaluate(expression) => {
                source_flow_expression(expression, program, &mut live, cancellation)?;
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
                source_flow_expression(condition, program, &mut live, cancellation)?;
                let mut outputs = Vec::new();
                for branch in [then_branch, else_branch] {
                    if let Some(output) = source_flow_statements(
                        branch,
                        live.clone(),
                        &known,
                        false,
                        program,
                        cancellation,
                        discharges,
                    )? {
                        outputs.push(output);
                    }
                }
                let Some(first) = outputs.first().cloned() else {
                    return Ok(None);
                };
                if outputs.iter().any(|output| output != &first) {
                    return defect("source custody oracle found a path-dependent branch output");
                }
                live = first;
            }
            Statement::For { iterable, body, .. } => {
                source_flow_expression(iterable, program, &mut live, cancellation)?;
                if let Some(output) = source_flow_statements(
                    body,
                    live.clone(),
                    &live,
                    false,
                    program,
                    cancellation,
                    discharges,
                )? && output != live
                {
                    return defect("source custody oracle found a non-fixpoint for-loop body");
                }
            }
            Statement::While {
                condition, body, ..
            } => {
                source_flow_expression(condition, program, &mut live, cancellation)?;
                if let Some(output) = source_flow_statements(
                    body,
                    live.clone(),
                    &live,
                    false,
                    program,
                    cancellation,
                    discharges,
                )? && output != live
                {
                    return defect("source custody oracle found a non-fixpoint while body");
                }
            }
            Statement::Break(_) | Statement::Continue(_) => {
                source_record_root_discharges(&live, inherited, false, discharges);
                return Ok(None);
            }
            Statement::Match { value, cases, .. } => {
                source_flow_expression(value, program, &mut live, cancellation)?;
                let mut outputs = Vec::new();
                for case in cases.iter() {
                    let mut case_live = live.clone();
                    if let (Some(pattern), Some(place)) = (
                        &case.pattern,
                        root_place(value).map(|place| place_details(&place)),
                    ) && let Some((binding, type_identity)) =
                        source_result_payload_move_binding(pattern)
                    {
                        case_live.retain(|candidate, _| !places_overlap(candidate, &place));
                        case_live.insert(Arc::from([binding]), type_identity);
                    }
                    if let Some(guard) = &case.guard {
                        source_flow_expression(guard, program, &mut case_live, cancellation)?;
                    }
                    if let Some(output) = source_flow_statements(
                        &case.body,
                        case_live,
                        &known,
                        false,
                        program,
                        cancellation,
                        discharges,
                    )? {
                        outputs.push(output);
                    }
                }
                if let Some(first) = outputs.first().cloned() {
                    if outputs.iter().any(|output| output != &first) {
                        return defect("source custody oracle found a path-dependent match output");
                    }
                    live = first;
                } else {
                    return Ok(None);
                }
            }
            Statement::Defer { action, .. } => {
                for capture in action.captures.iter() {
                    source_flow_expression(&capture.expression, program, &mut live, cancellation)?;
                }
            }
            Statement::WithPool {
                binding,
                scope,
                body,
                ..
            } => {
                source_flow_expression(scope, program, &mut live, cancellation)?;
                let outer = live.clone();
                if program.owns_resource_type(scope.type_id) {
                    live.insert(Arc::from(place_details(binding)), scope.type_id.0);
                }
                if let Some(output) = source_flow_statements(
                    body,
                    live,
                    &outer,
                    false,
                    program,
                    cancellation,
                    discharges,
                )? {
                    live = output;
                } else {
                    return Ok(None);
                }
            }
            Statement::Pass(_) => {}
        }
    }
    source_record_root_discharges(&live, inherited, root, discharges);
    if root {
        Ok(Some(BTreeMap::new()))
    } else {
        live.retain(|place, type_identity| inherited.get(place) == Some(type_identity));
        Ok(Some(live))
    }
}

fn source_expression_propagates(
    expression: &Expression,
    cancellation: &Cancellation,
) -> Result<usize, CoreFailure> {
    checkpoint(cancellation)?;
    let mut count = usize::from(matches!(expression.kind, ExpressionKind::Propagate(_)));
    let mut result = Ok(());
    expression.visit_children(&mut |child| {
        if result.is_ok() {
            match source_expression_propagates(child, cancellation) {
                Ok(child_count) => count = count.saturating_add(child_count),
                Err(error) => result = Err(error),
            }
        }
    });
    result?;
    Ok(count)
}

fn source_cleanup_run_count(
    statements: &[Statement],
    inherited: usize,
    cancellation: &Cancellation,
) -> Result<usize, CoreFailure> {
    let mut local = 0usize;
    let mut runs = 0usize;
    let mut reachable = true;
    for statement in statements {
        checkpoint(cancellation)?;
        if !reachable {
            break;
        }
        let active = inherited.saturating_add(local);
        let expressions = match statement {
            Statement::Return { value, .. } => value.iter().collect::<Vec<_>>(),
            Statement::Panic { value, .. } => vec![value],
            Statement::Assert { condition, .. } | Statement::Expect { condition, .. } => {
                vec![condition]
            }
            Statement::Initialize { value, .. } | Statement::Assign { value, .. } => vec![value],
            Statement::Evaluate(expression) => vec![expression],
            Statement::If { condition, .. } => vec![condition],
            Statement::IfPattern { value, .. } | Statement::Match { value, .. } => vec![value],
            Statement::For { iterable, .. } => vec![iterable],
            Statement::While { condition, .. } => vec![condition],
            Statement::Defer { action, .. } => action
                .captures
                .iter()
                .map(|capture| &capture.expression)
                .collect(),
            Statement::WithPool { scope, .. } => vec![scope],
            Statement::Break(_) | Statement::Continue(_) | Statement::Pass(_) => Vec::new(),
        };
        let propagated = expressions
            .iter()
            .map(|expression| source_expression_propagates(expression, cancellation))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .sum::<usize>();
        runs = runs.saturating_add(active.saturating_mul(propagated));
        if propagated > 0 {
            reachable = false;
            continue;
        }
        match statement {
            Statement::Return { .. } => {
                runs = runs.saturating_add(active);
                reachable = false;
            }
            Statement::Panic { .. } => reachable = false,
            Statement::Break(_) | Statement::Continue(_) => {
                runs = runs.saturating_add(local);
                reachable = false;
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
                runs = runs.saturating_add(source_cleanup_run_count(
                    then_branch,
                    active,
                    cancellation,
                )?);
                runs = runs.saturating_add(source_cleanup_run_count(
                    else_branch,
                    active,
                    cancellation,
                )?);
            }
            Statement::For { body, .. } | Statement::While { body, .. } => {
                runs = runs.saturating_add(source_cleanup_run_count(body, active, cancellation)?);
            }
            Statement::Match { cases, .. } => {
                for case in cases.iter() {
                    runs = runs.saturating_add(source_cleanup_run_count(
                        &case.body,
                        active,
                        cancellation,
                    )?);
                }
            }
            Statement::Defer { .. } => local = local.saturating_add(1),
            Statement::WithPool { body, .. } => {
                runs = runs.saturating_add(source_cleanup_run_count(body, active, cancellation)?);
            }
            Statement::Assert { .. }
            | Statement::Expect { .. }
            | Statement::Initialize { .. }
            | Statement::Assign { .. }
            | Statement::Evaluate(_)
            | Statement::Pass(_) => {}
        }
    }
    if reachable {
        runs = runs.saturating_add(local);
    }
    Ok(runs)
}

fn source_child_transfer(
    expression: &Expression,
    source: &SourceRange,
    path: &[u32],
    program: &crate::typed_hir::VerifiedProgram,
    events: &mut Vec<SourceCustodyEvent>,
) {
    if program.owns_resource_type(expression.type_id)
        && !matches!(
            expression.access,
            crate::typed_hir::AccessMode::Read | crate::typed_hir::AccessMode::Mut
        )
    {
        source_custody_event(
            events,
            CoreCustodyOperation::TransferCommit,
            Some(expression.type_id.0),
            Arc::<[u128]>::from([]),
            source,
            path,
        );
    }
}

fn source_custody_expression(
    expression: &Expression,
    path: &[u32],
    program: &crate::typed_hir::VerifiedProgram,
    cancellation: &Cancellation,
    events: &mut Vec<SourceCustodyEvent>,
) -> Result<(), CoreFailure> {
    checkpoint(cancellation)?;
    match &expression.kind {
        ExpressionKind::Read(place) => {
            for projection in place.projections.iter() {
                if let PlaceProjection::Index { index, .. } = projection {
                    source_custody_expression(index, path, program, cancellation, events)?;
                }
            }
        }
        ExpressionKind::Call { target, arguments } => {
            if let CallTarget::Callable { value } = target {
                source_custody_expression(value, path, program, cancellation, events)?;
            }
            for argument in arguments.iter() {
                source_custody_expression(argument, path, program, cancellation, events)?;
            }
        }
        ExpressionKind::Array(values) | ExpressionKind::Tuple(values) => {
            for value in values.iter() {
                source_custody_expression(value, path, program, cancellation, events)?;
            }
        }
        ExpressionKind::RepeatedArray { value, .. }
        | ExpressionKind::Positive(value)
        | ExpressionKind::Negate(value)
        | ExpressionKind::BitNot(value)
        | ExpressionKind::Not(value)
        | ExpressionKind::Await(value)
        | ExpressionKind::TrySend(value)
        | ExpressionKind::Propagate(value)
        | ExpressionKind::Is { value, .. } => {
            source_custody_expression(value, path, program, cancellation, events)?;
        }
        ExpressionKind::Index { value, index } => {
            source_custody_expression(value, path, program, cancellation, events)?;
            source_custody_expression(index, path, program, cancellation, events)?;
        }
        ExpressionKind::Binary {
            operator,
            left,
            right,
        } => {
            source_custody_expression(left, path, program, cancellation, events)?;
            let mut right_path = path.to_vec();
            if binary_kind(*operator) == CoreOperationKind::ShortCircuit {
                right_path.extend([u32::from(CoreOperationKind::ShortCircuit.tag()), 0]);
            }
            source_custody_expression(right, &right_path, program, cancellation, events)?;
        }
        ExpressionKind::Closure(closure) => {
            source_custody_expression(&closure.body, path, program, cancellation, events)?;
        }
        ExpressionKind::Literal(_)
        | ExpressionKind::Constant(_)
        | ExpressionKind::FunctionValue { .. }
        | ExpressionKind::CleanupCapture(_) => {}
    }

    let resource = program.owns_resource_type(expression.type_id);
    if resource {
        let operation = match &expression.kind {
            ExpressionKind::Read(_) | ExpressionKind::CleanupCapture(_)
                if expression.access == crate::typed_hir::AccessMode::Move
                    || matches!(expression.kind, ExpressionKind::CleanupCapture(_)) =>
            {
                Some(CoreCustodyOperation::Move)
            }
            ExpressionKind::Read(_) if expression.access == crate::typed_hir::AccessMode::Read => {
                Some(CoreCustodyOperation::SharedLoan)
            }
            ExpressionKind::Read(_) if expression.access == crate::typed_hir::AccessMode::Mut => {
                Some(CoreCustodyOperation::ExclusiveLoan)
            }
            ExpressionKind::Array(_)
            | ExpressionKind::Tuple(_)
            | ExpressionKind::RepeatedArray { .. } => Some(CoreCustodyOperation::Construct),
            ExpressionKind::Call {
                target: CallTarget::Build { .. } | CallTarget::Struct { .. },
                ..
            } => Some(CoreCustodyOperation::Construct),
            ExpressionKind::Call { .. } | ExpressionKind::Propagate(_) => {
                Some(CoreCustodyOperation::TransferCommit)
            }
            _ => None,
        };
        if let Some(operation) = operation {
            let place = match &expression.kind {
                ExpressionKind::Read(place) => Arc::from(place_details(place)),
                ExpressionKind::CleanupCapture(ordinal) => {
                    Arc::from([u128::from(ordinal.0), u128::MAX])
                }
                _ => Arc::from([]),
            };
            source_custody_event(
                events,
                operation,
                Some(expression.type_id.0),
                place,
                &expression.source,
                path,
            );
        }
    }

    let transfers_children = matches!(
        expression.kind,
        ExpressionKind::Call { .. }
            | ExpressionKind::Array(_)
            | ExpressionKind::Tuple(_)
            | ExpressionKind::RepeatedArray { .. }
            | ExpressionKind::Propagate(_)
    );
    if transfers_children {
        match &expression.kind {
            ExpressionKind::Call { target, arguments } => {
                if let CallTarget::Callable { value } = target {
                    source_child_transfer(value, &expression.source, path, program, events);
                }
                for child in arguments.iter() {
                    source_child_transfer(child, &expression.source, path, program, events);
                }
            }
            ExpressionKind::Array(children) | ExpressionKind::Tuple(children) => {
                for child in children.iter() {
                    source_child_transfer(child, &expression.source, path, program, events);
                }
            }
            ExpressionKind::RepeatedArray { value, .. } | ExpressionKind::Propagate(value) => {
                source_child_transfer(value, &expression.source, path, program, events);
            }
            _ => {}
        }
    }
    Ok(())
}

fn source_custody_statements(
    statements: &[Statement],
    path: &[u32],
    program: &crate::typed_hir::VerifiedProgram,
    cancellation: &Cancellation,
    events: &mut Vec<SourceCustodyEvent>,
) -> Result<(), CoreFailure> {
    for statement in statements {
        checkpoint(cancellation)?;
        match statement {
            Statement::Return { value, source } => {
                if let Some(value) = value {
                    source_custody_expression(value, path, program, cancellation, events)?;
                    source_child_transfer(value, source, path, program, events);
                }
            }
            Statement::Panic { value, source } => {
                source_custody_expression(value, path, program, cancellation, events)?;
                source_custody_event(
                    events,
                    CoreCustodyOperation::Panic,
                    None,
                    Arc::<[u128]>::from([]),
                    source,
                    path,
                );
            }
            Statement::Assert { condition, .. } => {
                source_custody_expression(condition, path, program, cancellation, events)?;
            }
            Statement::Expect { condition, .. } | Statement::Evaluate(condition) => {
                source_custody_expression(condition, path, program, cancellation, events)?;
            }
            Statement::Initialize {
                place,
                value,
                source,
            }
            | Statement::Assign {
                place,
                value,
                source,
            } => {
                for projection in place.projections.iter() {
                    if let PlaceProjection::Index { index, .. } = projection {
                        source_custody_expression(index, path, program, cancellation, events)?;
                    }
                }
                source_custody_expression(value, path, program, cancellation, events)?;
                if program.owns_resource_type(value.type_id) {
                    source_custody_event(
                        events,
                        if matches!(statement, Statement::Initialize { .. }) {
                            CoreCustodyOperation::TransferCommit
                        } else {
                            CoreCustodyOperation::Reinitialize
                        },
                        Some(value.type_id.0),
                        Arc::from(place_details(place)),
                        source,
                        path,
                    );
                }
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
                source,
            }
            | Statement::IfPattern {
                value: condition,
                then_branch,
                else_branch,
                source,
                ..
            } => {
                source_custody_expression(condition, path, program, cancellation, events)?;
                source_custody_event(
                    events,
                    CoreCustodyOperation::Join,
                    None,
                    Arc::<[u128]>::from([]),
                    source,
                    path,
                );
                for (ordinal, branch) in [then_branch, else_branch].into_iter().enumerate() {
                    let mut child = path.to_vec();
                    child.extend([
                        u32::from(CoreOperationKind::Branch.tag()),
                        u32::try_from(ordinal).unwrap_or(u32::MAX),
                    ]);
                    source_custody_statements(branch, &child, program, cancellation, events)?;
                }
            }
            Statement::For {
                pattern,
                iterable,
                body,
                source,
                ..
            } => {
                source_custody_expression(iterable, path, program, cancellation, events)?;
                source_custody_event(
                    events,
                    CoreCustodyOperation::LoopFixpoint,
                    None,
                    Arc::from(pattern_details(pattern)),
                    source,
                    path,
                );
                let mut child = path.to_vec();
                child.extend([u32::from(CoreOperationKind::Loop.tag()), 0]);
                source_custody_statements(body, &child, program, cancellation, events)?;
            }
            Statement::While {
                condition,
                body,
                max_iterations,
                source,
                ..
            } => {
                let mut condition_path = path.to_vec();
                condition_path.extend([u32::from(CoreOperationKind::Loop.tag()), 0]);
                source_custody_expression(
                    condition,
                    &condition_path,
                    program,
                    cancellation,
                    events,
                )?;
                source_custody_event(
                    events,
                    CoreCustodyOperation::LoopFixpoint,
                    None,
                    Arc::from([u128::from(*max_iterations)]),
                    source,
                    path,
                );
                let mut body_path = path.to_vec();
                body_path.extend([u32::from(CoreOperationKind::Loop.tag()), 1]);
                source_custody_statements(body, &body_path, program, cancellation, events)?;
                source_custody_event(
                    events,
                    CoreCustodyOperation::LoopFixpoint,
                    None,
                    Arc::<[u128]>::from([]),
                    source,
                    &body_path,
                );
            }
            Statement::Match {
                value,
                cases,
                source,
            } => {
                source_custody_expression(value, path, program, cancellation, events)?;
                source_custody_event(
                    events,
                    CoreCustodyOperation::Join,
                    None,
                    Arc::<[u128]>::from([]),
                    source,
                    path,
                );
                for (ordinal, case) in cases.iter().enumerate() {
                    let mut child = path.to_vec();
                    child.extend([
                        u32::from(CoreOperationKind::Match.tag()),
                        u32::try_from(ordinal).unwrap_or(u32::MAX),
                    ]);
                    if let (Some(pattern), Some(place)) = (
                        &case.pattern,
                        root_place(value).map(|place| place_details(&place)),
                    ) && let Some((binding, type_identity)) =
                        source_result_payload_move_binding(pattern)
                    {
                        source_custody_event(
                            events,
                            CoreCustodyOperation::Move,
                            Some(type_identity),
                            place,
                            &case.source,
                            &child,
                        );
                        source_custody_event(
                            events,
                            CoreCustodyOperation::TransferCommit,
                            Some(type_identity),
                            Arc::from([binding]),
                            &case.source,
                            &child,
                        );
                    }
                    if let Some(guard) = &case.guard {
                        source_custody_expression(guard, &child, program, cancellation, events)?;
                    }
                    source_custody_statements(&case.body, &child, program, cancellation, events)?;
                }
            }
            Statement::Defer { action, source } => {
                for capture in action.captures.iter() {
                    source_custody_expression(
                        &capture.expression,
                        path,
                        program,
                        cancellation,
                        events,
                    )?;
                    source_child_transfer(&capture.expression, source, path, program, events);
                }
                source_custody_event(
                    events,
                    CoreCustodyOperation::CleanupRegister,
                    None,
                    Arc::<[u128]>::from([]),
                    source,
                    path,
                );
                let mut action_path = path.to_vec();
                action_path.extend([u32::from(CoreOperationKind::Cleanup.tag()), 0]);
                source_custody_expression(
                    &action.expression,
                    &action_path,
                    program,
                    cancellation,
                    events,
                )?;
            }
            Statement::WithPool {
                binding,
                scope,
                body,
                source,
                ..
            } => {
                source_custody_expression(scope, path, program, cancellation, events)?;
                if program.owns_resource_type(scope.type_id) {
                    source_custody_event(
                        events,
                        CoreCustodyOperation::TransferCommit,
                        Some(scope.type_id.0),
                        Arc::from(place_details(binding)),
                        source,
                        path,
                    );
                }
                let mut child = path.to_vec();
                child.extend([u32::from(CoreOperationKind::PoolScope.tag()), 0]);
                source_custody_statements(body, &child, program, cancellation, events)?;
            }
            Statement::Break(_) | Statement::Continue(_) | Statement::Pass(_) => {}
        }
    }
    Ok(())
}

fn source_result_payload_move_binding(pattern: &HirMatchPattern) -> Option<(u128, u128)> {
    match pattern {
        HirMatchPattern::BuiltinVariant {
            variant: BuiltinVariant::ResultOk | BuiltinVariant::ResultErr,
            payload,
        } => match payload.as_ref() {
            [
                HirMatchPattern::Binding {
                    local,
                    type_id,
                    access: crate::typed_hir::AccessMode::Move,
                    ..
                },
            ] => Some((u128::from(local.0), type_id.0)),
            _ => None,
        },
        _ => None,
    }
}

fn core_custody_contract(
    executable: &CoreExecutable,
    cancellation: &Cancellation,
) -> Result<SourceCustodyContract, CoreFailure> {
    let paths = observation_control_paths(executable, cancellation)?;
    let mut events = Vec::new();
    let mut cleanup_runs = 0usize;
    let mut root_discharges = BTreeMap::new();
    for region in executable.regions.iter() {
        checkpoint(cancellation)?;
        for operation in region.operations.iter() {
            checkpoint(cancellation)?;
            for effect in operation.custody.iter() {
                checkpoint(cancellation)?;
                if matches!(
                    effect.operation,
                    CoreCustodyOperation::Discharge
                        | CoreCustodyOperation::CleanupRun
                        | CoreCustodyOperation::ProofCondition
                ) {
                    cleanup_runs = cleanup_runs.saturating_add(usize::from(
                        effect.operation == CoreCustodyOperation::CleanupRun,
                    ));
                    continue;
                }
                let place = if effect.operation == CoreCustodyOperation::Join {
                    Arc::from([])
                } else if operation.access == CoreAccessLaw::CleanupCapture
                    && effect.place.len() == 3
                {
                    Arc::from(&effect.place[1..])
                } else {
                    Arc::clone(&effect.place)
                };
                events.push(SourceCustodyEvent {
                    operation: effect.operation,
                    type_identity: effect.type_identity,
                    place,
                    source_path: Arc::from(operation.provenance.path()),
                    source_start: operation.provenance.start(),
                    source_end: operation.provenance.end(),
                    control_path: paths
                        .get(&operation.identity)
                        .cloned()
                        .unwrap_or_else(|| Arc::from([])),
                });
            }
        }
    }
    for effect in executable
        .regions
        .iter()
        .flat_map(|region| region.operations.iter())
        .flat_map(|operation| operation.custody.iter())
        .filter(|effect| {
            effect.operation == CoreCustodyOperation::Discharge && effect.place.len() == 1
        })
    {
        if let Some(type_identity) = effect.type_identity {
            *root_discharges
                .entry((Arc::clone(&effect.place), type_identity))
                .or_insert(0) += 1;
        }
    }
    events.sort();
    Ok(SourceCustodyContract {
        events,
        cleanup_runs,
        root_discharges,
    })
}

fn encode_source_custody_contract(
    hash: &mut Xxh3,
    contract: &SourceCustodyContract,
    cancellation: &Cancellation,
) -> Result<(), CoreFailure> {
    hash.update(&(contract.events.len() as u64).to_be_bytes());
    hash.update(&(contract.cleanup_runs as u64).to_be_bytes());
    hash.update(&(contract.root_discharges.len() as u64).to_be_bytes());
    for ((place, type_identity), count) in &contract.root_discharges {
        for part in place.iter() {
            hash.update(&part.to_be_bytes());
        }
        hash.update(&type_identity.to_be_bytes());
        hash.update(&(*count as u64).to_be_bytes());
    }
    for event in &contract.events {
        checkpoint(cancellation)?;
        hash.update(&[event.operation.tag()]);
        hash.update(&event.type_identity.unwrap_or(0).to_be_bytes());
        hash.update(event.source_path.as_bytes());
        hash.update(&event.source_start.to_be_bytes());
        hash.update(&event.source_end.to_be_bytes());
        for part in event.place.iter() {
            hash.update(&part.to_be_bytes());
        }
        for part in event.control_path.iter() {
            hash.update(&part.to_be_bytes());
        }
    }
    Ok(())
}

fn verifier_expected_custody_trace(
    executable: &CoreExecutable,
    semantic: crate::completed_semantic::CorePlanningSemanticProgram<'_>,
    planning: CorePlanningInput<'_>,
    cancellation: &Cancellation,
) -> Result<Vec<Arc<[CustodyEffect]>>, CoreFailure> {
    let program = semantic.verified_program();
    let components = verifier_resource_components(executable, program, cancellation)?;
    let value_types = executable
        .regions
        .iter()
        .flat_map(|region| region.operations.iter())
        .filter_map(|operation| Some((operation.result?, operation.type_identity?)))
        .collect::<BTreeMap<_, _>>();
    let value_access = executable
        .regions
        .iter()
        .flat_map(|region| region.operations.iter())
        .filter_map(|operation| Some((operation.result?, operation.access)))
        .collect::<BTreeMap<_, _>>();
    let context = CustodyVerifierContext {
        signature: &executable.signature,
        value_types: &value_types,
        value_access: &value_access,
        program,
    };
    let entry = executable
        .signature
        .parameters
        .iter()
        .filter(|parameter| {
            parameter.access == CoreAccessLaw::Move
                && program.owns_resource_type(TypeId(parameter.type_.identity))
        })
        .flat_map(|parameter| {
            let root = u128::from(parameter.local);
            components
                .get(&root)
                .cloned()
                .unwrap_or_else(|| vec![(Arc::<[u128]>::from([root]), parameter.type_.identity)])
        })
        .collect::<LivePlaces>();
    let mut flow = VerifierCustodyFlow {
        executable,
        context: &context,
        cancellation,
        components: &components,
        expected: BTreeMap::new(),
        outputs: BTreeMap::new(),
        active: BTreeSet::new(),
    };
    flow.inspect_region(RegionId(0), entry, BTreeMap::new())?;
    for region in executable.regions.iter() {
        if !flow.outputs.contains_key(&region.identity) {
            flow.inspect_region(region.identity, BTreeMap::new(), BTreeMap::new())?;
        }
    }
    Ok(executable
        .regions
        .iter()
        .flat_map(|region| region.operations.iter())
        .map(|operation| {
            let mut effects = flow
                .expected
                .remove(&operation.identity)
                .unwrap_or_default();
            if let Some(proof) =
                pool_proof_condition(operation, executable.reference.identity, semantic, planning)
            {
                let mut effect = verifier_effect(
                    CoreCustodyOperation::ProofCondition,
                    (
                        CoreInitializationEffect::None,
                        CoreCustodianEffect::None,
                        CoreLoanEffect::None,
                        CoreObligationEffect::None,
                    ),
                    Arc::from([]),
                    None,
                    None,
                    None,
                );
                effect.proof = Some(proof);
                effects.push(effect);
            }
            effects.into()
        })
        .collect())
}

struct VerifierCustodyFlow<'a> {
    executable: &'a CoreExecutable,
    context: &'a CustodyVerifierContext<'a>,
    cancellation: &'a Cancellation,
    components: &'a ResourceComponents,
    expected: BTreeMap<u32, Vec<CustodyEffect>>,
    outputs: BTreeMap<RegionId, Option<LivePlaces>>,
    active: BTreeSet<RegionId>,
}

impl VerifierCustodyFlow<'_> {
    fn inspect_region(
        &mut self,
        region_id: RegionId,
        mut state: LivePlaces,
        inherited: LivePlaces,
    ) -> Result<Option<LivePlaces>, CoreFailure> {
        checkpoint(self.cancellation)?;
        if let Some(output) = self.outputs.get(&region_id) {
            return Ok(output.clone());
        }
        if !self.active.insert(region_id) {
            return Ok(Some(state));
        }
        let operations = self
            .executable
            .regions
            .get(region_id.0 as usize)
            .ok_or_else(|| {
                CoreFailure::Defect(Arc::from("Core verifier custody region is absent"))
            })?
            .operations
            .to_vec();
        let mut known = state.clone();
        let mut terminal = false;
        for operation in &operations {
            checkpoint(self.cancellation)?;
            if operation.kind == CoreOperationKind::Read
                && matches!(
                    operation.access,
                    CoreAccessLaw::Move | CoreAccessLaw::CleanupCapture
                )
                && operation.type_identity.is_some_and(|identity| {
                    self.context.program.owns_resource_type(TypeId(identity))
                })
            {
                state.retain(|place, _| !places_overlap(place, &operation.details));
            }
            if operation.kind == CoreOperationKind::Store {
                let place: Arc<[u128]> =
                    operation.details[..operation.details.len().saturating_sub(1)].into();
                if let Some(type_identity) = operation
                    .operands
                    .iter()
                    .filter_map(|operand| self.context.value_types.get(operand).copied())
                    .find(|identity| self.context.program.owns_resource_type(TypeId(*identity)))
                {
                    if place.len() == 1 {
                        if let Some(children) = self.components.get(&place[0]) {
                            state.retain(|candidate, _| !place_is_prefix(&place, candidate));
                            state.extend(children.iter().cloned());
                            known.retain(|candidate, _| !place_is_prefix(&place, candidate));
                            known.extend(children.iter().cloned());
                        } else {
                            state.insert(Arc::clone(&place), type_identity);
                            known.insert(place, type_identity);
                        }
                    } else {
                        state.insert(Arc::clone(&place), type_identity);
                        known.insert(place, type_identity);
                    }
                }
            }
            if matches!(
                operation.kind,
                CoreOperationKind::Branch | CoreOperationKind::Match
            ) {
                let mut continuations = Vec::new();
                for successor in operation.successors.iter().copied() {
                    if let Some(output) =
                        self.inspect_region(successor, state.clone(), known.clone())?
                    {
                        continuations.push(output);
                    }
                }
                state = verifier_join_live(continuations)?.unwrap_or_default();
                terminal = state.is_empty() && !operation.successors.is_empty();
            } else if operation.kind == CoreOperationKind::Loop {
                let mut fixpoint = vec![state.clone()];
                for successor in operation.successors.iter().copied() {
                    if let Some(output) =
                        self.inspect_region(successor, state.clone(), state.clone())?
                    {
                        fixpoint.push(output);
                    }
                }
                state = verifier_join_live(fixpoint)?.unwrap_or_default();
            } else if operation.kind == CoreOperationKind::PoolScope
                && let Some(body) = operation.successors.first().copied()
            {
                let mut scoped = state.clone();
                if let Some(type_identity) = operation
                    .operands
                    .iter()
                    .filter_map(|operand| self.context.value_types.get(operand).copied())
                    .find(|identity| self.context.program.owns_resource_type(TypeId(*identity)))
                {
                    scoped.insert(Arc::clone(&operation.details), type_identity);
                }
                match self.inspect_region(body, scoped, state.clone())? {
                    Some(output) => state = output,
                    None => {
                        state.clear();
                        terminal = true;
                    }
                }
            }
            let leaving = matches!(
                operation.kind,
                CoreOperationKind::Return
                    | CoreOperationKind::Propagate
                    | CoreOperationKind::Break
                    | CoreOperationKind::Continue
                    | CoreOperationKind::LoopBack
                    | CoreOperationKind::RecoverableExit
            );
            let discharge_all = matches!(
                operation.kind,
                CoreOperationKind::Return | CoreOperationKind::Propagate
            ) || region_id == RegionId(0);
            let discharges = if leaving {
                state
                    .iter()
                    .filter(|(place, type_identity)| {
                        discharge_all || inherited.get(*place) != Some(*type_identity)
                    })
                    .map(|(place, type_identity)| (Arc::clone(place), *type_identity))
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            self.expected.insert(
                operation.identity,
                verifier_operation_custody(
                    operation,
                    region_id == RegionId(0),
                    self.context,
                    &[],
                    &discharges,
                ),
            );
            for (place, _) in discharges {
                state.remove(&place);
            }
            if matches!(
                operation.kind,
                CoreOperationKind::Return | CoreOperationKind::Propagate
            ) || operation.kind == CoreOperationKind::TerminalPanic
            {
                state.clear();
                terminal = true;
            }
        }
        self.active.remove(&region_id);
        let output = (!terminal).then_some(state);
        self.outputs.insert(region_id, output.clone());
        Ok(output)
    }
}

fn verifier_resource_components(
    executable: &CoreExecutable,
    program: &crate::typed_hir::VerifiedProgram,
    cancellation: &Cancellation,
) -> Result<ResourceComponents, CoreFailure> {
    let mut roots = BTreeMap::<u128, BTreeMap<Arc<[u128]>, u128>>::new();
    for region in executable.regions.iter() {
        checkpoint(cancellation)?;
        for operation in region.operations.iter() {
            checkpoint(cancellation)?;
            if operation.kind != CoreOperationKind::Read || operation.details.len() < 2 {
                continue;
            }
            let Some(type_identity) = operation.type_identity else {
                continue;
            };
            if !program.owns_resource_type(TypeId(type_identity)) {
                continue;
            }
            roots
                .entry(operation.details[0])
                .or_default()
                .insert(Arc::clone(&operation.details), type_identity);
        }
    }
    Ok(roots
        .into_iter()
        .map(|(root, children)| (root, children.into_iter().collect()))
        .collect())
}

fn verifier_join_live(states: Vec<LivePlaces>) -> Result<Option<LivePlaces>, CoreFailure> {
    let mut paths = states.into_iter();
    let Some(expected) = paths.next() else {
        return Ok(None);
    };
    if paths.all(|path| path == expected) {
        Ok(Some(expected))
    } else {
        defect("Core verifier found mismatched custody at a join or loop fixpoint")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum CompactOutcome {
    Completed(CanonicalValue),
    Panicked(EvaluationPanicKind, SourceRange),
}

struct OracleBudget<'a> {
    remaining: u64,
    cancellation: &'a Cancellation,
}

impl<'a> OracleBudget<'a> {
    fn new(candidate: &VerifiedCoreProgram, cancellation: &'a Cancellation) -> Self {
        let graph_work = candidate
            .executables
            .iter()
            .flat_map(|executable| executable.regions.iter())
            .map(|region| region.operations.len() as u64)
            .sum::<u64>();
        Self {
            remaining: graph_work.saturating_mul(128).saturating_add(1_024),
            cancellation,
        }
    }

    fn for_execution(
        candidate: &VerifiedCoreProgram,
        executable: &CoreExecutable,
        cancellation: &'a Cancellation,
    ) -> Result<Self, CoreFailure> {
        let remaining = oracle_execution_bound(candidate, executable, &mut BTreeSet::new())
            .ok_or_else(|| {
                CoreFailure::Defect(Arc::from(
                    "Core oracle admitted an execution without a bounded work proof",
                ))
            })?;
        Ok(Self {
            remaining,
            cancellation,
        })
    }

    fn step(&mut self) -> Result<(), CoreFailure> {
        checkpoint(self.cancellation)?;
        if self.remaining == 0 {
            return defect("Core semantic oracle exceeded its logical work budget");
        }
        self.remaining -= 1;
        Ok(())
    }
}

fn oracle_execution_bound(
    candidate: &VerifiedCoreProgram,
    executable: &CoreExecutable,
    visiting: &mut BTreeSet<u128>,
) -> Option<u64> {
    const ORACLE_WORK_LIMIT: u64 = crate::evaluator::FUEL_LIMIT;
    if !visiting.insert(executable.reference.identity) {
        return None;
    }
    let mut base = executable
        .regions
        .iter()
        .map(|region| region.operations.len() as u64 + 1)
        .sum::<u64>()
        .saturating_add(16);
    let mut recurrence = 1_u64;
    for operation in executable
        .regions
        .iter()
        .flat_map(|region| region.operations.iter())
    {
        if operation.kind == CoreOperationKind::Loop {
            let iterations = if operation.successors.len() == 3 {
                u64::try_from(*operation.details.first()?)
                    .ok()?
                    .saturating_add(1)
            } else {
                u64::try_from(oracle_for_length(executable, operation)?).ok()?
            };
            recurrence = recurrence.saturating_mul(iterations.max(1));
        }
        if operation.kind == CoreOperationKind::Call {
            let target_identity = direct_call_target(operation)?;
            let target = candidate.executables.iter().find(|candidate| {
                candidate.reference.kind == CoreExecutableKind::SourceSpecialization
                    && candidate.reference.identity == target_identity
            })?;
            base = base.saturating_add(oracle_execution_bound(candidate, target, visiting)?);
        }
    }
    visiting.remove(&executable.reference.identity);
    let bound = base.saturating_mul(recurrence).saturating_add(1_024);
    (bound <= ORACLE_WORK_LIMIT).then_some(bound)
}

fn oracle_for_length(executable: &CoreExecutable, operation: &Operation) -> Option<usize> {
    let iterable = operation.operands.first()?;
    let producer = executable
        .regions
        .iter()
        .flat_map(|region| region.operations.iter())
        .find(|candidate| candidate.result == Some(*iterable))?;
    if producer.kind != CoreOperationKind::Aggregate {
        return None;
    }
    match producer.details.as_ref() {
        [0] => Some(producer.operands.len()),
        [2, length] => usize::try_from(*length).ok(),
        _ => None,
    }
}

fn oracle_supported(
    candidate: &VerifiedCoreProgram,
    executable: &CoreExecutable,
    visiting: &mut BTreeSet<u128>,
    budget: &mut OracleBudget<'_>,
) -> Result<bool, CoreFailure> {
    budget.step()?;
    if !visiting.insert(executable.reference.identity) {
        return Ok(false);
    }
    for operation in executable
        .regions
        .iter()
        .flat_map(|region| region.operations.iter())
    {
        budget.step()?;
        let supported = match operation.kind {
            CoreOperationKind::Literal => {
                canonical_literal_from_details(&operation.details).is_some()
            }
            CoreOperationKind::Read => operation.details.len() == 1,
            CoreOperationKind::Store => operation.details.len() == 2,
            CoreOperationKind::Unary => matches!(operation.details.as_ref(), [1..=4]),
            CoreOperationKind::CheckedArithmetic => operation
                .details
                .first()
                .and_then(|tag| operator_from_tag(*tag))
                .is_some_and(|operator| {
                    matches!(
                        operator,
                        BinaryOperator::Add
                            | BinaryOperator::Subtract
                            | BinaryOperator::Multiply
                            | BinaryOperator::Divide
                            | BinaryOperator::Remainder
                    )
                }),
            CoreOperationKind::Binary => operation
                .details
                .first()
                .and_then(|tag| operator_from_tag(*tag))
                .is_some_and(|operator| {
                    matches!(
                        operator,
                        BinaryOperator::BitAnd
                            | BinaryOperator::BitOr
                            | BinaryOperator::BitXor
                            | BinaryOperator::ShiftLeft
                            | BinaryOperator::ShiftRight
                            | BinaryOperator::Equal
                            | BinaryOperator::NotEqual
                            | BinaryOperator::Less
                            | BinaryOperator::LessEqual
                            | BinaryOperator::Greater
                            | BinaryOperator::GreaterEqual
                    )
                }),
            CoreOperationKind::ShortCircuit => operation
                .details
                .first()
                .and_then(|tag| operator_from_tag(*tag))
                .is_some_and(|operator| {
                    matches!(operator, BinaryOperator::And | BinaryOperator::Or)
                }),
            CoreOperationKind::Aggregate => matches!(operation.details.as_ref(), [0 | 1] | [2, _]),
            CoreOperationKind::Return
            | CoreOperationKind::TerminalPanic
            | CoreOperationKind::Assert
            | CoreOperationKind::Expect
            | CoreOperationKind::Branch
            | CoreOperationKind::LoopBack
            | CoreOperationKind::Break
            | CoreOperationKind::Continue
            | CoreOperationKind::RecoverableExit => true,
            CoreOperationKind::FunctionValue => operation.details.len() == 2,
            CoreOperationKind::Call => {
                let Some(target) = direct_call_target(operation) else {
                    visiting.remove(&executable.reference.identity);
                    return Ok(false);
                };
                let Some(target) = candidate.executables.iter().find(|candidate| {
                    candidate.reference.kind == CoreExecutableKind::SourceSpecialization
                        && candidate.reference.identity == target
                }) else {
                    visiting.remove(&executable.reference.identity);
                    return Ok(false);
                };
                if !valid_call_binding(operation, target)
                    || target.signature.parameters.iter().any(|parameter| {
                        matches!(
                            parameter.access,
                            CoreAccessLaw::ExclusiveLoan | CoreAccessLaw::Move
                        )
                    })
                {
                    visiting.remove(&executable.reference.identity);
                    return Ok(false);
                }
                oracle_supported(candidate, target, visiting, budget)?
            }
            CoreOperationKind::Loop => {
                match (operation.successors.len(), operation.details.as_ref()) {
                    (3, [_]) => true,
                    (1, [8, ..]) => oracle_for_length(executable, operation).is_some(),
                    _ => false,
                }
            }
            CoreOperationKind::Pass
            | CoreOperationKind::Constant
            | CoreOperationKind::ClosureValue
            | CoreOperationKind::BuildConstruction
            | CoreOperationKind::Index
            | CoreOperationKind::Propagate
            | CoreOperationKind::PatternTest
            | CoreOperationKind::Match
            | CoreOperationKind::Cleanup
            | CoreOperationKind::CleanupRun
            | CoreOperationKind::PoolScope
            | CoreOperationKind::Suspension
            | CoreOperationKind::MessageProposal
            | CoreOperationKind::GeneratedRole => false,
        };
        if !supported {
            visiting.remove(&executable.reference.identity);
            return Ok(false);
        }
    }
    visiting.remove(&executable.reference.identity);
    Ok(true)
}

enum Signal {
    Continue,
    Return(CanonicalValue),
    Panic(EvaluationPanicKind, SourceRange),
    Break,
    ContinueLoop,
    LoopBack,
}

fn oracle_execute(
    candidate: &VerifiedCoreProgram,
    executable: &CoreExecutable,
    arguments: &[CanonicalValue],
    budget: &mut OracleBudget<'_>,
) -> Result<CompactOutcome, CoreFailure> {
    let mut values = BTreeMap::new();
    let mut locals = executable
        .signature
        .parameters
        .iter()
        .zip(arguments)
        .map(|(parameter, value)| (parameter.local, value.clone()))
        .collect::<BTreeMap<_, _>>();
    let signal = oracle_region(
        candidate,
        executable,
        executable.entry,
        &mut values,
        &mut locals,
        budget,
    )?;
    Ok(match signal {
        Signal::Return(value) => CompactOutcome::Completed(value),
        Signal::Panic(kind, site) => CompactOutcome::Panicked(kind, site),
        Signal::Continue => CompactOutcome::Completed(CanonicalValue::Unit),
        Signal::Break | Signal::ContinueLoop | Signal::LoopBack => {
            return defect("Core semantic oracle observed loop control outside a loop");
        }
    })
}

fn oracle_region(
    candidate: &VerifiedCoreProgram,
    executable: &CoreExecutable,
    region: RegionId,
    values: &mut BTreeMap<ValueId, CanonicalValue>,
    locals: &mut BTreeMap<u32, CanonicalValue>,
    budget: &mut OracleBudget<'_>,
) -> Result<Signal, CoreFailure> {
    budget.step()?;
    let region = executable
        .regions
        .get(region.0 as usize)
        .ok_or_else(|| CoreFailure::Defect(Arc::from("Core oracle region is dangling")))?;
    for operation in region.operations.iter() {
        budget.step()?;
        match operation.kind {
            CoreOperationKind::Literal => {
                let value =
                    canonical_literal_from_details(&operation.details).ok_or_else(|| {
                        CoreFailure::Defect(Arc::from("unsupported literal entered Core oracle"))
                    })?;
                values.insert(required_result(operation)?, value);
            }
            CoreOperationKind::Read => {
                let local = u32::try_from(operation.details[0])
                    .map_err(|_| CoreFailure::Defect(Arc::from("Core local identity overflows")))?;
                let value = locals.get(&local).cloned().ok_or_else(|| {
                    CoreFailure::Defect(Arc::from("Core oracle reads an unavailable local"))
                })?;
                values.insert(required_result(operation)?, value);
            }
            CoreOperationKind::Store => {
                let local = u32::try_from(operation.details[0])
                    .map_err(|_| CoreFailure::Defect(Arc::from("Core local identity overflows")))?;
                let value = required_operand(
                    operation,
                    operation.operands.len().saturating_sub(1),
                    values,
                )?;
                locals.insert(local, value);
            }
            CoreOperationKind::CheckedArithmetic | CoreOperationKind::Binary => {
                let operator = operator_from_tag(operation.details[0])
                    .ok_or_else(|| CoreFailure::Defect(Arc::from("Core binary law is invalid")))?;
                match oracle_binary(
                    operator,
                    &required_operand(operation, 0, values)?,
                    &required_operand(operation, 1, values)?,
                    &operation.provenance,
                ) {
                    Ok(value) => {
                        values.insert(required_result(operation)?, value);
                    }
                    Err(signal) => return Ok(signal),
                }
            }
            CoreOperationKind::ShortCircuit => {
                let operator = operator_from_tag(operation.details[0]).ok_or_else(|| {
                    CoreFailure::Defect(Arc::from("Core short-circuit law is invalid"))
                })?;
                let CanonicalValue::Bool(left_value) = required_operand(operation, 0, values)?
                else {
                    return defect("Core short-circuit operand is not bool");
                };
                let skip = (operator == BinaryOperator::And && !left_value)
                    || (operator == BinaryOperator::Or && left_value);
                if skip {
                    values.insert(
                        required_result(operation)?,
                        CanonicalValue::Bool(left_value),
                    );
                } else {
                    match oracle_region(
                        candidate,
                        executable,
                        operation.successors[0],
                        values,
                        locals,
                        budget,
                    )? {
                        Signal::Continue => {}
                        signal => return Ok(signal),
                    }
                    let right = required_operand(operation, 1, values)?;
                    values.insert(required_result(operation)?, right);
                }
            }
            CoreOperationKind::Unary => {
                let value = match oracle_unary(
                    operation.details[0],
                    required_operand(operation, 0, values)?,
                    &operation.provenance,
                ) {
                    Ok(value) => value,
                    Err(signal) => return Ok(signal),
                };
                values.insert(required_result(operation)?, value);
            }
            CoreOperationKind::Aggregate => {
                let operands = operation
                    .operands
                    .iter()
                    .map(|operand| {
                        values.get(operand).cloned().ok_or_else(|| {
                            CoreFailure::Defect(Arc::from("Core aggregate operand is unavailable"))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let value = match operation.details.as_ref() {
                    [0] => CanonicalValue::Array(operands.into()),
                    [1] => CanonicalValue::Tuple(operands.into()),
                    [2, length] => {
                        let Some(value) = operands.first() else {
                            return defect("Core repeated aggregate has no element");
                        };
                        let length = usize::try_from(*length).map_err(|_| {
                            CoreFailure::Defect(Arc::from("Core aggregate length overflows"))
                        })?;
                        CanonicalValue::Array(vec![value.clone(); length].into())
                    }
                    _ => return defect("unsupported aggregate entered Core oracle"),
                };
                values.insert(required_result(operation)?, value);
            }
            CoreOperationKind::Return => {
                let value = if operation.operands.is_empty() {
                    CanonicalValue::Unit
                } else {
                    required_operand(operation, 0, values)?
                };
                return Ok(Signal::Return(value));
            }
            CoreOperationKind::TerminalPanic => {
                return Ok(Signal::Panic(
                    EvaluationPanicKind::Explicit,
                    operation.provenance.clone(),
                ));
            }
            CoreOperationKind::Assert => {
                if required_operand(operation, 0, values)? != CanonicalValue::Bool(true) {
                    return Ok(Signal::Panic(
                        EvaluationPanicKind::AssertionFailed,
                        operation.provenance.clone(),
                    ));
                }
            }
            CoreOperationKind::Expect => {}
            CoreOperationKind::Branch => {
                let CanonicalValue::Bool(condition) = required_operand(operation, 0, values)?
                else {
                    return defect("Core branch operand is not bool");
                };
                let successor = operation.successors[usize::from(!condition)];
                match oracle_region(candidate, executable, successor, values, locals, budget)? {
                    Signal::Continue => {}
                    signal => return Ok(signal),
                }
            }
            CoreOperationKind::Loop => {
                if operation.successors.len() == 1 {
                    let iterable = required_operand(operation, 0, values)?;
                    let CanonicalValue::Array(elements) = iterable else {
                        return defect("Core bounded for iterable is not an array");
                    };
                    let [8, local, ..] = operation.details.as_ref() else {
                        return defect("Core bounded for pattern is unsupported");
                    };
                    let local = u32::try_from(*local).map_err(|_| {
                        CoreFailure::Defect(Arc::from("Core for binding identity overflows"))
                    })?;
                    for element in elements.iter() {
                        budget.step()?;
                        locals.insert(local, element.clone());
                        match oracle_region(
                            candidate,
                            executable,
                            operation.successors[0],
                            values,
                            locals,
                            budget,
                        )? {
                            Signal::Continue | Signal::ContinueLoop => {}
                            Signal::Break => break,
                            signal => return Ok(signal),
                        }
                    }
                    continue;
                }
                let max = u64::try_from(operation.details[0]).unwrap_or(u64::MAX);
                let [condition_region, body_region, _exit_region] = operation.successors.as_ref()
                else {
                    return defect("Core while law has malformed regions");
                };
                let condition_value = *operation.operands.first().ok_or_else(|| {
                    CoreFailure::Defect(Arc::from("Core while has no condition value"))
                })?;
                for iteration in 0..=max {
                    budget.step()?;
                    match oracle_region(
                        candidate,
                        executable,
                        *condition_region,
                        values,
                        locals,
                        budget,
                    )? {
                        Signal::Continue => {}
                        signal => return Ok(signal),
                    }
                    let CanonicalValue::Bool(condition) =
                        values.get(&condition_value).cloned().ok_or_else(|| {
                            CoreFailure::Defect(Arc::from("Core while condition is unavailable"))
                        })?
                    else {
                        return defect("Core while condition is not bool");
                    };
                    if !condition {
                        break;
                    }
                    if iteration == max {
                        return defect("Core while exceeded its verified maximum iterations");
                    }
                    match oracle_region(
                        candidate,
                        executable,
                        *body_region,
                        values,
                        locals,
                        budget,
                    )? {
                        Signal::Continue | Signal::ContinueLoop | Signal::LoopBack => {}
                        Signal::Break => {
                            break;
                        }
                        signal => return Ok(signal),
                    }
                }
            }
            CoreOperationKind::LoopBack => return Ok(Signal::LoopBack),
            CoreOperationKind::Break => return Ok(Signal::Break),
            CoreOperationKind::Continue => return Ok(Signal::ContinueLoop),
            CoreOperationKind::RecoverableExit => {}
            CoreOperationKind::FunctionValue => {
                let identity = *operation.details.get(1).ok_or_else(|| {
                    CoreFailure::Defect(Arc::from("Core function value is malformed"))
                })?;
                values.insert(
                    required_result(operation)?,
                    CanonicalValue::Function { identity },
                );
            }
            CoreOperationKind::Call => {
                let target = direct_call_target(operation).ok_or_else(|| {
                    CoreFailure::Defect(Arc::from("unsupported call entered Core oracle"))
                })?;
                let target = candidate
                    .executables
                    .iter()
                    .find(|candidate| {
                        candidate.reference.kind == CoreExecutableKind::SourceSpecialization
                            && candidate.reference.identity == target
                    })
                    .ok_or_else(|| CoreFailure::Defect(Arc::from("Core call target is missing")))?;
                let arguments = operation
                    .operands
                    .iter()
                    .map(|value| {
                        values.get(value).cloned().ok_or_else(|| {
                            CoreFailure::Defect(Arc::from("Core call argument is unavailable"))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let arguments = bind_call_arguments(operation, arguments, target)?;
                match oracle_execute(candidate, target, &arguments, budget)? {
                    CompactOutcome::Completed(value) => {
                        values.insert(required_result(operation)?, value);
                    }
                    CompactOutcome::Panicked(kind, site) => return Ok(Signal::Panic(kind, site)),
                }
            }
            _ => return defect("unsupported operation entered Core semantic oracle"),
        }
    }
    Ok(Signal::Continue)
}

fn required_result(operation: &Operation) -> Result<ValueId, CoreFailure> {
    operation
        .result
        .ok_or_else(|| CoreFailure::Defect(Arc::from("Core value operation has no result")))
}

fn required_operand(
    operation: &Operation,
    index: usize,
    values: &BTreeMap<ValueId, CanonicalValue>,
) -> Result<CanonicalValue, CoreFailure> {
    operation
        .operands
        .get(index)
        .and_then(|value| values.get(value))
        .cloned()
        .ok_or_else(|| CoreFailure::Defect(Arc::from("Core operand is unavailable")))
}

fn direct_call_target(operation: &Operation) -> Option<u128> {
    matches!(operation.details.as_ref(), [3, ..])
        .then(|| operation.details.get(2).copied())
        .flatten()
}

fn valid_call_binding(operation: &Operation, target: &CoreExecutable) -> bool {
    if operation.call_binding.len() != operation.operands.len()
        || operation.call_binding.len() != target.signature.parameters.len()
    {
        return false;
    }
    let mut seen = vec![false; operation.call_binding.len()];
    for parameter in operation.call_binding.iter().copied() {
        let parameter = usize::from(parameter);
        if parameter >= seen.len() || seen[parameter] {
            return false;
        }
        seen[parameter] = true;
    }
    seen.into_iter().all(std::convert::identity)
}

fn bind_call_arguments(
    operation: &Operation,
    source_arguments: Vec<CanonicalValue>,
    target: &CoreExecutable,
) -> Result<Vec<CanonicalValue>, CoreFailure> {
    if !valid_call_binding(operation, target) {
        return defect("Core call binding is not a complete parameter permutation");
    }
    let mut parameters = vec![None; source_arguments.len()];
    for (value, parameter) in source_arguments
        .into_iter()
        .zip(operation.call_binding.iter().copied())
    {
        parameters[usize::from(parameter)] = Some(value);
    }
    parameters
        .into_iter()
        .map(|value| {
            value.ok_or_else(|| CoreFailure::Defect(Arc::from("Core call parameter is unbound")))
        })
        .collect()
}

fn canonical_literal_from_details(details: &[u128]) -> Option<CanonicalValue> {
    match details {
        [1] => Some(CanonicalValue::Unit),
        [2, value] => Some(CanonicalValue::Bool(*value != 0)),
        [3, kind, value] => Some(CanonicalValue::Integer {
            type_name: Arc::from(integer_from_tag(u8::try_from(*kind).ok()?)?.name()),
            value: *value as i128,
        }),
        [4, kind, bits] => Some(CanonicalValue::Float {
            type_name: Arc::from(float_name(u8::try_from(*kind).ok()?)?),
            bits: u64::try_from(*bits).ok()?,
        }),
        [5, length, chunks @ ..] => {
            let bytes = unpack_exact_bytes(*length, chunks)?;
            Some(CanonicalValue::Text(Arc::from(
                String::from_utf8(bytes).ok()?,
            )))
        }
        [6, value] => Some(CanonicalValue::Scalar(char::from_u32(
            u32::try_from(*value).ok()?,
        )?)),
        [7, length, chunks @ ..] => Some(CanonicalValue::Bytes(
            unpack_exact_bytes(*length, chunks)?.into(),
        )),
        _ => None,
    }
}

fn unpack_exact_bytes(length: u128, chunks: &[u128]) -> Option<Vec<u8>> {
    let length = usize::try_from(length).ok()?;
    if chunks.len() != length.div_ceil(16) {
        return None;
    }
    let mut bytes = Vec::with_capacity(chunks.len() * 16);
    for chunk in chunks {
        bytes.extend_from_slice(&chunk.to_be_bytes());
    }
    bytes.truncate(length);
    Some(bytes)
}

fn oracle_unary(
    tag: u128,
    value: CanonicalValue,
    site: &SourceRange,
) -> Result<CanonicalValue, Signal> {
    match (tag, value) {
        (1, value) => Ok(value),
        (2, CanonicalValue::Integer { type_name, value }) => {
            let value = value
                .checked_neg()
                .filter(|value| integer_kind(&type_name).is_ok_and(|kind| kind.fits(*value)));
            value
                .map(|value| CanonicalValue::Integer { type_name, value })
                .ok_or_else(|| Signal::Panic(EvaluationPanicKind::IntegerOverflow, site.clone()))
        }
        (2, CanonicalValue::Float { type_name, bits }) => {
            let bits = match type_name.as_ref() {
                "f16" => u64::from((-half::f16::from_bits(bits as u16)).to_bits()),
                "f32" => u64::from((-f32::from_bits(bits as u32)).to_bits()),
                "f64" => (-f64::from_bits(bits)).to_bits(),
                _ => return Err(Signal::Continue),
            };
            Ok(CanonicalValue::Float { type_name, bits })
        }
        (3, CanonicalValue::Integer { type_name, value }) => {
            let kind = integer_kind(&type_name)?;
            let value = if kind.is_signed() {
                !value
            } else {
                let mask = (1_i128 << kind.bits()) - 1;
                (!value) & mask
            };
            Ok(CanonicalValue::Integer { type_name, value })
        }
        (4, CanonicalValue::Bool(value)) => Ok(CanonicalValue::Bool(!value)),
        _ => Err(Signal::Continue),
    }
}

fn oracle_binary(
    operator: BinaryOperator,
    left: &CanonicalValue,
    right: &CanonicalValue,
    site: &SourceRange,
) -> Result<CanonicalValue, Signal> {
    match (left, right) {
        (
            CanonicalValue::Integer {
                type_name,
                value: left,
            },
            CanonicalValue::Integer { value: right, .. },
        ) => {
            let value = match operator {
                BinaryOperator::Add => left.checked_add(*right),
                BinaryOperator::Subtract => left.checked_sub(*right),
                BinaryOperator::Multiply => left.checked_mul(*right),
                BinaryOperator::Divide if *right == 0 => {
                    return Err(Signal::Panic(
                        EvaluationPanicKind::DivisionByZero,
                        site.clone(),
                    ));
                }
                BinaryOperator::Divide => left.checked_div(*right),
                BinaryOperator::Remainder if *right == 0 => {
                    return Err(Signal::Panic(
                        EvaluationPanicKind::DivisionByZero,
                        site.clone(),
                    ));
                }
                BinaryOperator::Remainder => left.checked_rem(*right),
                BinaryOperator::BitAnd => {
                    return Ok(CanonicalValue::Integer {
                        type_name: type_name.clone(),
                        value: *left & *right,
                    });
                }
                BinaryOperator::BitOr => {
                    return Ok(CanonicalValue::Integer {
                        type_name: type_name.clone(),
                        value: *left | *right,
                    });
                }
                BinaryOperator::BitXor => {
                    return Ok(CanonicalValue::Integer {
                        type_name: type_name.clone(),
                        value: *left ^ *right,
                    });
                }
                BinaryOperator::ShiftLeft => u32::try_from(*right)
                    .ok()
                    .and_then(|shift| left.checked_shl(shift)),
                BinaryOperator::ShiftRight => u32::try_from(*right)
                    .ok()
                    .and_then(|shift| left.checked_shr(shift)),
                BinaryOperator::Equal => return Ok(CanonicalValue::Bool(left == right)),
                BinaryOperator::NotEqual => return Ok(CanonicalValue::Bool(left != right)),
                BinaryOperator::Less => return Ok(CanonicalValue::Bool(left < right)),
                BinaryOperator::LessEqual => return Ok(CanonicalValue::Bool(left <= right)),
                BinaryOperator::Greater => return Ok(CanonicalValue::Bool(left > right)),
                BinaryOperator::GreaterEqual => return Ok(CanonicalValue::Bool(left >= right)),
                _ => return Err(Signal::Continue),
            }
            .ok_or_else(|| Signal::Panic(EvaluationPanicKind::IntegerOverflow, site.clone()))?;
            let kind = integer_kind(type_name)?;
            if !kind.fits(value) {
                return Err(Signal::Panic(
                    EvaluationPanicKind::IntegerOverflow,
                    site.clone(),
                ));
            }
            Ok(CanonicalValue::Integer {
                type_name: type_name.clone(),
                value,
            })
        }
        (CanonicalValue::Bool(left), CanonicalValue::Bool(right)) => match operator {
            BinaryOperator::And => Ok(CanonicalValue::Bool(*left && *right)),
            BinaryOperator::Or => Ok(CanonicalValue::Bool(*left || *right)),
            BinaryOperator::Equal => Ok(CanonicalValue::Bool(left == right)),
            BinaryOperator::NotEqual => Ok(CanonicalValue::Bool(left != right)),
            _ => Err(Signal::Continue),
        },
        (
            CanonicalValue::Float {
                type_name,
                bits: left,
            },
            CanonicalValue::Float { bits: right, .. },
        ) => {
            let (left, right, encode): (f64, f64, fn(f64) -> u64) = match type_name.as_ref() {
                "f16" => (
                    half::f16::from_bits(*left as u16).to_f64(),
                    half::f16::from_bits(*right as u16).to_f64(),
                    |value| u64::from(half::f16::from_f64(value).to_bits()),
                ),
                "f32" => (
                    f64::from(f32::from_bits(*left as u32)),
                    f64::from(f32::from_bits(*right as u32)),
                    |value| u64::from((value as f32).to_bits()),
                ),
                "f64" => (f64::from_bits(*left), f64::from_bits(*right), f64::to_bits),
                _ => return Err(Signal::Continue),
            };
            let value = match operator {
                BinaryOperator::Add => left + right,
                BinaryOperator::Subtract => left - right,
                BinaryOperator::Multiply => left * right,
                BinaryOperator::Divide => left / right,
                BinaryOperator::Remainder => left % right,
                BinaryOperator::Equal => return Ok(CanonicalValue::Bool(left == right)),
                BinaryOperator::NotEqual => return Ok(CanonicalValue::Bool(left != right)),
                BinaryOperator::Less => return Ok(CanonicalValue::Bool(left < right)),
                BinaryOperator::LessEqual => return Ok(CanonicalValue::Bool(left <= right)),
                BinaryOperator::Greater => return Ok(CanonicalValue::Bool(left > right)),
                BinaryOperator::GreaterEqual => return Ok(CanonicalValue::Bool(left >= right)),
                _ => return Err(Signal::Continue),
            };
            Ok(CanonicalValue::Float {
                type_name: type_name.clone(),
                bits: if value.is_nan() {
                    match type_name.as_ref() {
                        "f16" => 0x7e00,
                        "f32" => 0x7fc0_0000,
                        _ => 0x7ff8_0000_0000_0000,
                    }
                } else {
                    encode(value)
                },
            })
        }
        (left, right) if operator == BinaryOperator::Equal => {
            Ok(CanonicalValue::Bool(left == right))
        }
        (left, right) if operator == BinaryOperator::NotEqual => {
            Ok(CanonicalValue::Bool(left != right))
        }
        (left, right) => {
            let Some(ordering) = oracle_data_order(left, right) else {
                return Err(Signal::Continue);
            };
            Ok(CanonicalValue::Bool(match operator {
                BinaryOperator::Less => ordering.is_lt(),
                BinaryOperator::LessEqual => ordering.is_le(),
                BinaryOperator::Greater => ordering.is_gt(),
                BinaryOperator::GreaterEqual => ordering.is_ge(),
                _ => return Err(Signal::Continue),
            }))
        }
    }
}

fn oracle_data_order(left: &CanonicalValue, right: &CanonicalValue) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (CanonicalValue::Text(left), CanonicalValue::Text(right)) => Some(left.cmp(right)),
        (CanonicalValue::Scalar(left), CanonicalValue::Scalar(right)) => Some(left.cmp(right)),
        (CanonicalValue::Bytes(left), CanonicalValue::Bytes(right)) => Some(left.cmp(right)),
        (CanonicalValue::Tuple(left), CanonicalValue::Tuple(right))
        | (CanonicalValue::Array(left), CanonicalValue::Array(right)) => {
            for (left, right) in left.iter().zip(right.iter()) {
                let ordering = oracle_data_order(left, right)?;
                if !ordering.is_eq() {
                    return Some(ordering);
                }
            }
            Some(left.len().cmp(&right.len()))
        }
        _ => None,
    }
}

const fn integer_from_tag(tag: u8) -> Option<IntegerType> {
    match tag {
        0x01 => Some(IntegerType::U8),
        0x02 => Some(IntegerType::U16),
        0x03 => Some(IntegerType::U32),
        0x04 => Some(IntegerType::U64),
        0x11 => Some(IntegerType::I8),
        0x12 => Some(IntegerType::I16),
        0x13 => Some(IntegerType::I32),
        0x14 => Some(IntegerType::I64),
        _ => None,
    }
}

const fn float_name(tag: u8) -> Option<&'static str> {
    match tag {
        0x21 => Some("f16"),
        0x22 => Some("f32"),
        0x23 => Some("f64"),
        _ => None,
    }
}

const fn operator_from_tag(tag: u128) -> Option<BinaryOperator> {
    match tag {
        1 => Some(BinaryOperator::Range),
        2 => Some(BinaryOperator::RangeInclusive),
        3 => Some(BinaryOperator::Add),
        4 => Some(BinaryOperator::Subtract),
        5 => Some(BinaryOperator::Multiply),
        6 => Some(BinaryOperator::Divide),
        7 => Some(BinaryOperator::Remainder),
        8 => Some(BinaryOperator::BitAnd),
        9 => Some(BinaryOperator::BitOr),
        10 => Some(BinaryOperator::BitXor),
        11 => Some(BinaryOperator::ShiftLeft),
        12 => Some(BinaryOperator::ShiftRight),
        13 => Some(BinaryOperator::And),
        14 => Some(BinaryOperator::Or),
        15 => Some(BinaryOperator::Equal),
        16 => Some(BinaryOperator::NotEqual),
        17 => Some(BinaryOperator::Less),
        18 => Some(BinaryOperator::LessEqual),
        19 => Some(BinaryOperator::Greater),
        20 => Some(BinaryOperator::GreaterEqual),
        _ => None,
    }
}

fn integer_kind(name: &str) -> Result<IntegerType, Signal> {
    match name {
        "u8" => Ok(IntegerType::U8),
        "u16" => Ok(IntegerType::U16),
        "u32" => Ok(IntegerType::U32),
        "u64" => Ok(IntegerType::U64),
        "i8" => Ok(IntegerType::I8),
        "i16" => Ok(IntegerType::I16),
        "i32" => Ok(IntegerType::I32),
        "i64" => Ok(IntegerType::I64),
        _ => Err(Signal::Continue),
    }
}

fn oracle_matches_evaluator(core: &CompactOutcome, evaluator: &EvaluationOutcome) -> bool {
    match (core, evaluator) {
        (CompactOutcome::Completed(left), EvaluationOutcome::Completed(right)) => left == right,
        (
            CompactOutcome::Panicked(left_kind, left_site),
            EvaluationOutcome::Panicked {
                kind: right_kind,
                site: right_site,
            },
        ) => left_kind == right_kind && left_site == right_site,
        _ => false,
    }
}

fn encode_oracle_outcome(hash: &mut Xxh3, outcome: &CompactOutcome) {
    match outcome {
        CompactOutcome::Completed(value) => {
            hash.update(&[1]);
            encode_canonical_value(hash, value);
        }
        CompactOutcome::Panicked(kind, site) => {
            hash.update(&[2, panic_tag(*kind)]);
            encode_source(hash, site);
        }
    }
}

fn encode_canonical_value(hash: &mut Xxh3, value: &CanonicalValue) {
    match value {
        CanonicalValue::Unit => hash.update(&[1]),
        CanonicalValue::Bool(value) => hash.update(&[2, u8::from(*value)]),
        CanonicalValue::Integer { type_name, value } => {
            hash.update(&[3]);
            encode_bytes(hash, type_name.as_bytes());
            hash.update(&value.to_be_bytes());
        }
        CanonicalValue::Float { type_name, bits } => {
            hash.update(&[4]);
            encode_bytes(hash, type_name.as_bytes());
            hash.update(&bits.to_be_bytes());
        }
        CanonicalValue::Text(value) => {
            hash.update(&[5]);
            encode_bytes(hash, value.as_bytes());
        }
        CanonicalValue::Scalar(value) => {
            hash.update(&[6]);
            hash.update(&u32::from(*value).to_be_bytes());
        }
        CanonicalValue::Bytes(value) => {
            hash.update(&[7]);
            encode_bytes(hash, value);
        }
        CanonicalValue::Function { identity } => {
            hash.update(&[8]);
            hash.update(&identity.to_be_bytes());
        }
        CanonicalValue::Closure { identity, captures } => {
            hash.update(&[9]);
            hash.update(&identity.to_be_bytes());
            hash.update(&(captures.len() as u64).to_be_bytes());
            captures
                .iter()
                .for_each(|value| encode_canonical_value(hash, value));
        }
        CanonicalValue::Tuple(values) => {
            hash.update(&[10]);
            hash.update(&(values.len() as u64).to_be_bytes());
            values
                .iter()
                .for_each(|value| encode_canonical_value(hash, value));
        }
        CanonicalValue::Array(values) => {
            hash.update(&[11]);
            hash.update(&(values.len() as u64).to_be_bytes());
            values
                .iter()
                .for_each(|value| encode_canonical_value(hash, value));
        }
        CanonicalValue::Variant {
            type_name,
            variant,
            payload,
        } => {
            hash.update(&[12]);
            encode_bytes(hash, type_name.as_bytes());
            encode_bytes(hash, variant.as_bytes());
            hash.update(&(payload.len() as u64).to_be_bytes());
            payload
                .iter()
                .for_each(|value| encode_canonical_value(hash, value));
        }
        CanonicalValue::Struct { type_name, fields } => {
            hash.update(&[13]);
            encode_bytes(hash, type_name.as_bytes());
            hash.update(&(fields.len() as u64).to_be_bytes());
            for (name, value) in fields.iter() {
                encode_bytes(hash, name.as_bytes());
                encode_canonical_value(hash, value);
            }
        }
        CanonicalValue::SymbolicHandle { kind, identity } => {
            hash.update(&[14]);
            match kind {
                crate::ConstructionKind::Image => hash.update(&[1]),
                crate::ConstructionKind::Test => hash.update(&[2]),
                crate::ConstructionKind::Node { type_identity } => {
                    hash.update(&[3]);
                    hash.update(&type_identity.to_be_bytes());
                }
            }
            hash.update(&identity.to_be_bytes());
        }
    }
}

fn encode_bytes(hash: &mut Xxh3, bytes: &[u8]) {
    hash.update(&(bytes.len() as u64).to_be_bytes());
    hash.update(bytes);
}

const fn panic_tag(kind: EvaluationPanicKind) -> u8 {
    match kind {
        EvaluationPanicKind::Explicit => 1,
        EvaluationPanicKind::AssertionFailed => 2,
        EvaluationPanicKind::IntegerOverflow => 3,
        EvaluationPanicKind::DivisionByZero => 4,
        EvaluationPanicKind::IndexOutOfBounds => 5,
    }
}

fn checkpoint(cancellation: &Cancellation) -> Result<(), CoreFailure> {
    if cancellation.is_cancelled() {
        Err(CoreFailure::Cancelled)
    } else {
        Ok(())
    }
}

fn defect<T>(evidence: impl Into<Arc<str>>) -> Result<T, CoreFailure> {
    Err(CoreFailure::Defect(evidence.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ArchitectureProfile, CompilationOutcome, CompilationRequest, Compiler,
        CompilerInstallation, InspectSelection, ProjectFile, ProjectSnapshot, Root,
    };

    fn fixture() -> (
        VerifiedCoreProgram,
        Arc<crate::image_planning::VerifiedPlanningFoundation>,
    ) {
        let request = CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new(
                "src/image.wr",
                br#"pure fn answer(value: i64) -> i64:
    if true:
        return value * 7
    return value + 1

pure fn wrapper() -> i64:
    return answer(6)

@image
fn build() -> Image:
    return Image.new(value=wrapper())
"#,
            )]),
            Root::Image,
        )
        .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::all());
        let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
        let CompilationOutcome::Accepted(accepted) =
            compiler.compile(request, &Cancellation::new())
        else {
            panic!("fixture accepts");
        };
        (
            accepted
                .verified_core_program()
                .expect("Core derived")
                .clone(),
            Arc::new(
                accepted
                    .verified_planning_foundation()
                    .expect("planning derived")
                    .clone(),
            ),
        )
    }

    fn pool_fixture() -> (
        VerifiedCoreProgram,
        Arc<crate::image_planning::VerifiedPlanningFoundation>,
    ) {
        let request = CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new(
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
            )]),
            Root::Image,
        )
        .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::all());
        let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
        let outcome = compiler.compile(request, &Cancellation::new());
        let CompilationOutcome::Accepted(accepted) = outcome else {
            panic!("Pool Core fixture accepts: {outcome:#?}");
        };
        (
            accepted
                .verified_core_program()
                .expect("Core derived")
                .clone(),
            Arc::new(
                accepted
                    .verified_planning_foundation()
                    .expect("planning derived")
                    .clone(),
            ),
        )
    }

    fn custody_fixture() -> (
        VerifiedCoreProgram,
        Arc<crate::image_planning::VerifiedPlanningFoundation>,
    ) {
        let request = CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new(
                "src/image.wr",
                br#"enum Failure:
    Failed

resource struct Ticket:
    mut id: i64

fn fail() -> Result[i64, Failure]:
    return Result.Err(Failure.Failed)

fn inspect(read left: Ticket, read right: Ticket) -> i64:
    return left.id + right.id

fn edit(mut ticket: Ticket):
    ticket.id = 7

fn consume(take ticket: Ticket):
    pass

fn oldest():
    pass

fn newest():
    pass

fn run() -> Result[i64, Failure]:
    mut ticket = Ticket(id=1)
    total = inspect(ticket, ticket)
    edit(mut ticket)
    defer oldest()
    defer newest()
    consume(take ticket)
    if total > 0:
        return fail()?
    return Result.Ok(total)

fn terminal():
    defer oldest()
    panic "terminal"

@image
fn build() -> Image:
    if false:
        terminal()
    return Image.new(value=run())
"#,
            )]),
            Root::Image,
        )
        .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::all());
        let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
        let outcome = compiler.compile(request, &Cancellation::new());
        let CompilationOutcome::Accepted(accepted) = outcome else {
            panic!("custody fixture accepts: {outcome:#?}");
        };
        (
            accepted
                .verified_core_program()
                .expect("Core derived")
                .clone(),
            Arc::new(
                accepted
                    .verified_planning_foundation()
                    .expect("planning derived")
                    .clone(),
            ),
        )
    }

    fn branch_custody_fixture() -> (
        VerifiedCoreProgram,
        Arc<crate::image_planning::VerifiedPlanningFoundation>,
    ) {
        let request = CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new(
                "src/image.wr",
                br#"resource struct Ticket:
    id: i64

fn consume(take ticket: Ticket):
    pass

fn finish(flag: bool, take ticket: Ticket):
    if flag:
        consume(take ticket)
    else:
        consume(take ticket)

@image
fn build() -> Image:
    ticket = Ticket(id=1)
    finish(true, take ticket)
    return Image.new()
"#,
            )]),
            Root::Image,
        )
        .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::all());
        let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
        let outcome = compiler.compile(request, &Cancellation::new());
        let CompilationOutcome::Accepted(accepted) = outcome else {
            panic!("branch custody fixture accepts: {outcome:#?}");
        };
        (
            accepted
                .verified_core_program()
                .expect("Core derived")
                .clone(),
            Arc::new(
                accepted
                    .verified_planning_foundation()
                    .expect("planning derived")
                    .clone(),
            ),
        )
    }

    fn suspension_fixture() -> (
        VerifiedCoreProgram,
        Arc<crate::image_planning::VerifiedPlanningFoundation>,
    ) {
        let request = CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new(
                "src/test.wr",
                br#"resource struct Ticket:
    id: i64

async fn answer() -> i64:
    return 1

fn consume(take ticket: Ticket):
    pass

pub suite behavior:
    async test after_resume(take ticket: Ticket):
        value = await answer()
        expect ticket.id + value == 2
        consume(take ticket)

@image
fn build() -> Image:
    tests = Test.new(cases=[behavior.after_resume(Ticket(id=1))])
    return Image.new(tests=tests)
"#,
            )]),
            Root::Test,
        )
        .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
        .with_inspection(InspectSelection::all());
        let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
        let outcome = compiler.compile(request, &Cancellation::new());
        let CompilationOutcome::Accepted(accepted) = outcome else {
            panic!("suspension fixture accepts: {outcome:#?}");
        };
        (
            accepted
                .verified_core_program()
                .expect("Core derived")
                .clone(),
            Arc::new(
                accepted
                    .verified_planning_foundation()
                    .expect("planning derived")
                    .clone(),
            ),
        )
    }

    fn rejected(
        candidate: &VerifiedCoreProgram,
        planning: &crate::image_planning::VerifiedPlanningFoundation,
    ) -> bool {
        matches!(
            verify(candidate, planning.for_core(), &Cancellation::new()),
            Err(CoreFailure::Defect(_))
        )
    }

    fn resign(
        candidate: &mut VerifiedCoreProgram,
        planning: &crate::image_planning::VerifiedPlanningFoundation,
    ) {
        let cancellation = Cancellation::new();
        let mut executables = candidate.executables.to_vec();
        for executable in &mut executables {
            executable.fingerprint =
                verifier_executable_fingerprint(executable, &cancellation).expect("fingerprint");
        }
        candidate.executables = executables.into();
        candidate.oracle = run_oracle(candidate, planning.for_core(), &cancellation)
            .expect("corruption remains oracle-executable");
        candidate.fingerprint =
            verifier_fingerprint(candidate, &cancellation).expect("program fingerprint");
    }

    fn resign_fingerprints_only(candidate: &mut VerifiedCoreProgram) {
        let cancellation = Cancellation::new();
        let mut executables = candidate.executables.to_vec();
        for executable in &mut executables {
            executable.fingerprint =
                verifier_executable_fingerprint(executable, &cancellation).expect("fingerprint");
        }
        candidate.executables = executables.into();
        candidate.fingerprint =
            verifier_fingerprint(candidate, &cancellation).expect("program fingerprint");
    }

    fn corrupt_source_operation(
        core: &VerifiedCoreProgram,
        kind: CoreOperationKind,
        mutator: impl FnOnce(&mut Operation),
    ) -> VerifiedCoreProgram {
        let mut candidate = core.clone();
        let mut executables = candidate.executables.to_vec();
        let source = executables
            .iter_mut()
            .find(|executable| {
                executable.reference.kind == CoreExecutableKind::SourceSpecialization
                    && executable.regions.iter().any(|region| {
                        region
                            .operations
                            .iter()
                            .any(|operation| operation.kind == kind)
                    })
            })
            .expect("source operation executable");
        let mut regions = source.regions.to_vec();
        let region = regions
            .iter_mut()
            .find(|region| {
                region
                    .operations
                    .iter()
                    .any(|operation| operation.kind == kind)
            })
            .expect("source operation region");
        let mut operations = region.operations.to_vec();
        let operation = operations
            .iter_mut()
            .find(|operation| operation.kind == kind)
            .expect("source operation");
        mutator(operation);
        region.operations = operations.into();
        source.regions = regions.into();
        candidate.executables = executables.into();
        resign_fingerprints_only(&mut candidate);
        candidate
    }

    fn corrupt_custody_effect(
        core: &VerifiedCoreProgram,
        kind: CoreCustodyOperation,
        mutator: impl FnOnce(&mut CustodyEffect),
    ) -> VerifiedCoreProgram {
        let mut candidate = core.clone();
        let mut executables = candidate.executables.to_vec();
        let executable = executables
            .iter_mut()
            .find(|executable| {
                executable.regions.iter().any(|region| {
                    region.operations.iter().any(|operation| {
                        operation
                            .custody
                            .iter()
                            .any(|effect| effect.operation == kind)
                    })
                })
            })
            .expect("custody operation executable");
        let mut regions = executable.regions.to_vec();
        let region = regions
            .iter_mut()
            .find(|region| {
                region.operations.iter().any(|operation| {
                    operation
                        .custody
                        .iter()
                        .any(|effect| effect.operation == kind)
                })
            })
            .expect("custody operation region");
        let mut operations = region.operations.to_vec();
        let operation = operations
            .iter_mut()
            .find(|operation| {
                operation
                    .custody
                    .iter()
                    .any(|effect| effect.operation == kind)
            })
            .expect("custody operation");
        let mut effects = operation.custody.to_vec();
        let effect = effects
            .iter_mut()
            .find(|effect| effect.operation == kind)
            .expect("custody effect");
        mutator(effect);
        operation.custody = effects.into();
        region.operations = operations.into();
        executable.regions = regions.into();
        candidate.executables = executables.into();
        candidate
    }

    #[test]
    fn verifier_rejects_missing_extra_duplicate_and_mixed_context_realization() {
        let (core, planning) = fixture();

        let mut missing = core.clone();
        missing.executables = missing.executables[..missing.executables.len() - 1].into();
        assert!(rejected(&missing, &planning));

        let mut duplicate = core.clone();
        let mut executables = duplicate.executables.to_vec();
        executables.push(executables[0].clone());
        duplicate.executables = executables.into();
        assert!(rejected(&duplicate, &planning));

        let mut mixed = core.clone();
        let mut executables = mixed.executables.to_vec();
        executables[0].reference.context ^= 1;
        mixed.executables = executables.into();
        assert!(rejected(&mixed, &planning));
    }

    #[test]
    fn verifier_rejects_resigned_single_fault_pool_proof_corruption() {
        let (core, planning) = pool_fixture();
        let mut stale =
            corrupt_custody_effect(&core, CoreCustodyOperation::ProofCondition, |effect| {
                effect
                    .proof
                    .as_mut()
                    .expect("Pool proof")
                    .requirement_current_meaning ^= 1;
            });
        resign(&mut stale, &planning);
        assert!(rejected(&stale, &planning));
    }

    #[test]
    fn verifier_rejects_malformed_control_invalid_refs_and_order_changing_rewrite() {
        let (core, planning) = fixture();
        let source_index = core
            .executables
            .iter()
            .position(|executable| {
                executable.reference.kind == CoreExecutableKind::SourceSpecialization
                    && executable.regions.iter().any(|region| {
                        region
                            .operations
                            .iter()
                            .any(|operation| operation.kind == CoreOperationKind::Branch)
                    })
            })
            .expect("branching source executable");

        let mutate_operation =
            |core: &VerifiedCoreProgram, mutator: &mut dyn FnMut(&mut Operation)| {
                let mut candidate = core.clone();
                let mut executables = candidate.executables.to_vec();
                let mut regions = executables[source_index].regions.to_vec();
                let region_index = regions
                    .iter()
                    .position(|region| !region.operations.is_empty())
                    .expect("nonempty region");
                let mut operations = regions[region_index].operations.to_vec();
                mutator(&mut operations[0]);
                regions[region_index].operations = operations.into();
                executables[source_index].regions = regions.into();
                candidate.executables = executables.into();
                candidate
            };

        let malformed = mutate_operation(&core, &mut |operation| {
            operation.successors = Arc::from([RegionId(u32::MAX)]);
        });
        assert!(rejected(&malformed, &planning));

        let invalid_value = mutate_operation(&core, &mut |operation| {
            operation.operands = Arc::from([ValueId(u32::MAX)]);
        });
        assert!(rejected(&invalid_value, &planning));

        let mut reordered = core.clone();
        let mut executables = reordered.executables.to_vec();
        let mut regions = executables[source_index].regions.to_vec();
        let index = regions
            .iter()
            .position(|region| region.operations.len() >= 2)
            .expect("region with ordered operations");
        let mut operations = regions[index].operations.to_vec();
        operations.swap(0, 1);
        regions[index].operations = operations.into();
        executables[source_index].regions = regions.into();
        reordered.executables = executables.into();
        assert!(rejected(&reordered, &planning));
    }

    #[test]
    fn verifier_rejects_wrong_kind_false_facts_oracle_and_fingerprint() {
        let (core, planning) = fixture();

        let mut wrong_kind = core.clone();
        let mut executables = wrong_kind.executables.to_vec();
        executables[0].reference.kind = CoreExecutableKind::Generated;
        wrong_kind.executables = executables.into();
        assert!(rejected(&wrong_kind, &planning));

        let mut stale_meaning = core.clone();
        let mut executables = stale_meaning.executables.to_vec();
        executables[0].reference.current_meaning ^= 1;
        stale_meaning.executables = executables.into();
        assert!(rejected(&stale_meaning, &planning));

        let mut false_facts = core.clone();
        let mut executables = false_facts.executables.to_vec();
        executables[0].facts.may_panic = !executables[0].facts.may_panic;
        false_facts.executables = executables.into();
        assert!(rejected(&false_facts, &planning));

        let mut false_oracle = core.clone();
        false_oracle.oracle.agrees = false;
        assert!(rejected(&false_oracle, &planning));

        let mut false_fingerprint = core.clone();
        false_fingerprint.fingerprint ^= 1;
        assert!(rejected(&false_fingerprint, &planning));
    }

    #[test]
    fn verifier_independently_rejects_resigned_access_type_and_rewrite_corruptions() {
        let (core, planning) = fixture();

        let mut false_access = core.clone();
        let mut executables = false_access.executables.to_vec();
        let source = executables
            .iter_mut()
            .find(|executable| {
                executable.reference.kind == CoreExecutableKind::SourceSpecialization
            })
            .expect("source executable");
        let mut regions = source.regions.to_vec();
        let region = regions
            .iter_mut()
            .find(|region| !region.operations.is_empty())
            .expect("operation region");
        let mut operations = region.operations.to_vec();
        operations[0].access = CoreAccessLaw::Move;
        region.operations = operations.into();
        source.regions = regions.into();
        false_access.executables = executables.into();
        resign(&mut false_access, &planning);
        assert!(rejected(&false_access, &planning));

        let mut false_type = core.clone();
        let mut executables = false_type.executables.to_vec();
        let source = executables
            .iter_mut()
            .find(|executable| {
                executable.reference.kind == CoreExecutableKind::SourceSpecialization
            })
            .expect("source executable");
        source.signature.return_type.identity ^= 1;
        false_type.executables = executables.into();
        resign(&mut false_type, &planning);
        assert!(rejected(&false_type, &planning));

        let mut mixed_parameter_type = core.clone();
        let mut executables = mixed_parameter_type.executables.to_vec();
        let source = executables
            .iter_mut()
            .find(|executable| !executable.signature.parameters.is_empty())
            .expect("parameterized source executable");
        let mut parameters = source.signature.parameters.to_vec();
        parameters[0].type_.identity ^= 1;
        source.signature.parameters = parameters.into();
        mixed_parameter_type.executables = executables.into();
        resign(&mut mixed_parameter_type, &planning);
        assert!(rejected(&mixed_parameter_type, &planning));

        let mut false_rewrite = core.clone();
        let mut executables = false_rewrite.executables.to_vec();
        let source = executables
            .iter_mut()
            .find(|executable| {
                executable.reference.kind == CoreExecutableKind::SourceSpecialization
            })
            .expect("source executable");
        source.rewrites = Arc::from([RewriteWitness {
            kind: CoreRewriteKind::EliminatedPass,
            provenance: source.provenance.clone(),
            source_order: 0,
        }]);
        false_rewrite.executables = executables.into();
        resign(&mut false_rewrite, &planning);
        assert!(rejected(&false_rewrite, &planning));
    }

    #[test]
    fn verifier_independently_rejects_every_resigned_source_operation_field() {
        let (core, planning) = fixture();
        let mut corruptions = Vec::new();

        let candidate =
            corrupt_source_operation(&core, CoreOperationKind::CheckedArithmetic, |operation| {
                let mut operands = operation.operands.to_vec();
                operands.swap(0, 1);
                operation.operands = operands.into();
            });
        corruptions.push(("operand identity/order", rejected(&candidate, &planning)));

        let candidate = corrupt_source_operation(&core, CoreOperationKind::Branch, |operation| {
            let mut successors = operation.successors.to_vec();
            successors.swap(0, 1);
            operation.successors = successors.into();
        });
        corruptions.push(("successor identity/order", rejected(&candidate, &planning)));

        let candidate = corrupt_source_operation(&core, CoreOperationKind::Branch, |operation| {
            operation.details = Arc::from([99]);
        });
        corruptions.push(("details", rejected(&candidate, &planning)));

        let candidate = corrupt_source_operation(&core, CoreOperationKind::Call, |operation| {
            operation.call_binding = Arc::from([u16::MAX]);
        });
        corruptions.push(("call binding", rejected(&candidate, &planning)));

        let candidate = corrupt_source_operation(&core, CoreOperationKind::Branch, |operation| {
            operation.effect = EffectBoundary::Call;
        });
        corruptions.push(("effect", rejected(&candidate, &planning)));

        let candidate = corrupt_source_operation(&core, CoreOperationKind::Branch, |operation| {
            operation.failure = FailureLaw::RecordTestFailure;
        });
        corruptions.push(("failure", rejected(&candidate, &planning)));

        let candidate = corrupt_source_operation(&core, CoreOperationKind::Branch, |operation| {
            operation.result = Some(ValueId(u32::MAX - 1));
        });
        corruptions.push(("result identity", rejected(&candidate, &planning)));

        let candidate = corrupt_source_operation(&core, CoreOperationKind::Literal, |operation| {
            operation.type_identity = operation.type_identity.map(|identity| identity ^ 1);
        });
        corruptions.push(("value type identity", rejected(&candidate, &planning)));

        let candidate = corrupt_source_operation(&core, CoreOperationKind::Branch, |operation| {
            operation.identity = operation.identity.saturating_add(100);
        });
        corruptions.push(("operation identity", rejected(&candidate, &planning)));

        let candidate = corrupt_source_operation(&core, CoreOperationKind::Branch, |operation| {
            operation.provenance = SourceRange::new("src/false.wr", 0, 1);
        });
        corruptions.push(("provenance", rejected(&candidate, &planning)));

        assert!(
            corruptions.iter().all(|(_, rejected)| *rejected),
            "unrejected corruptions: {corruptions:?}"
        );
    }

    #[test]
    fn custody_verifier_rejects_duplicate_lost_and_invalid_transfer_commit() {
        let (core, planning) = custody_fixture();

        let mut lost = corrupt_custody_effect(&core, CoreCustodyOperation::Move, |effect| {
            effect.destination_home = None;
        });
        resign(&mut lost, &planning);
        assert!(rejected(&lost, &planning));

        let mut invalid =
            corrupt_custody_effect(&core, CoreCustodyOperation::TransferCommit, |effect| {
                effect.destination_home = effect.source_home;
            });
        resign(&mut invalid, &planning);
        assert!(rejected(&invalid, &planning));

        let mut duplicate = core.clone();
        let mut executables = duplicate.executables.to_vec();
        let executable = executables
            .iter_mut()
            .find(|executable| {
                executable.regions.iter().any(|region| {
                    region.operations.iter().any(|operation| {
                        operation
                            .custody
                            .iter()
                            .any(|effect| effect.operation == CoreCustodyOperation::TransferCommit)
                    })
                })
            })
            .expect("transfer executable");
        let mut regions = executable.regions.to_vec();
        let region = regions
            .iter_mut()
            .find(|region| {
                region.operations.iter().any(|operation| {
                    operation
                        .custody
                        .iter()
                        .any(|effect| effect.operation == CoreCustodyOperation::TransferCommit)
                })
            })
            .expect("transfer region");
        let mut operations = region.operations.to_vec();
        let operation = operations
            .iter_mut()
            .find(|operation| {
                operation
                    .custody
                    .iter()
                    .any(|effect| effect.operation == CoreCustodyOperation::TransferCommit)
            })
            .expect("transfer operation");
        let mut effects = operation.custody.to_vec();
        let repeated = effects
            .iter()
            .find(|effect| effect.operation == CoreCustodyOperation::TransferCommit)
            .expect("transfer effect")
            .clone();
        effects.push(repeated);
        operation.custody = effects.into();
        region.operations = operations.into();
        executable.regions = regions.into();
        duplicate.executables = executables.into();
        resign(&mut duplicate, &planning);
        assert!(rejected(&duplicate, &planning));

        for mut corrupted in [
            corrupt_custody_effect(&core, CoreCustodyOperation::Move, |effect| {
                let mut place = effect.place.to_vec();
                place[0] ^= 1;
                effect.place = place.into();
            }),
            corrupt_custody_effect(&core, CoreCustodyOperation::Move, |effect| {
                effect.type_identity = effect.type_identity.map(|identity| identity ^ 1);
            }),
            corrupt_custody_effect(&core, CoreCustodyOperation::Move, |effect| {
                effect.source_home = effect.source_home.map(|home| home ^ 1);
            }),
            corrupt_custody_effect(&core, CoreCustodyOperation::Move, |effect| {
                effect.destination_home = effect.destination_home.map(|home| home ^ 1);
            }),
            corrupt_custody_effect(&core, CoreCustodyOperation::TransferCommit, |effect| {
                effect.place = Arc::from([1]);
            }),
            corrupt_custody_effect(&core, CoreCustodyOperation::TransferCommit, |effect| {
                effect.type_identity = effect.type_identity.map(|identity| identity ^ 1);
            }),
            corrupt_custody_effect(&core, CoreCustodyOperation::TransferCommit, |effect| {
                effect.source_home = effect.source_home.map(|home| home ^ 1);
            }),
            corrupt_custody_effect(&core, CoreCustodyOperation::TransferCommit, |effect| {
                effect.destination_home = effect.destination_home.map(|home| home ^ 1);
            }),
        ] {
            resign(&mut corrupted, &planning);
            assert!(rejected(&corrupted, &planning));
        }
    }

    #[test]
    fn custody_verifier_rejects_cleanup_reorder_and_custody_changing_rewrite() {
        let (core, planning) = custody_fixture();
        let mut reordered = core.clone();
        let mut executables = reordered.executables.to_vec();
        let executable = executables
            .iter_mut()
            .find(|executable| {
                executable.regions.iter().any(|region| {
                    region.operations.iter().any(|operation| {
                        operation.kind == CoreOperationKind::CleanupRun
                            && operation.successors.len() == 2
                    })
                })
            })
            .expect("cleanup executable");
        let mut regions = executable.regions.to_vec();
        let head = regions
            .iter()
            .find_map(|region| {
                region.operations.iter().find_map(|operation| {
                    if operation.kind == CoreOperationKind::CleanupRun
                        && operation.successors.len() == 2
                    {
                        Some((region.identity, operation.successors[1]))
                    } else {
                        None
                    }
                })
            })
            .expect("cleanup chain head");
        let region = regions
            .iter_mut()
            .find(|region| {
                region
                    .operations
                    .iter()
                    .any(|operation| operation.successors.contains(&head.0))
            })
            .expect("cleanup chain exit");
        let mut operations = region.operations.to_vec();
        let operation = operations
            .iter_mut()
            .find(|operation| operation.successors.contains(&head.0))
            .expect("cleanup chain predecessor");
        operation.successors = Arc::from([head.1]);
        region.operations = operations.into();
        executable.regions = regions.into();
        reordered.executables = executables.into();
        resign(&mut reordered, &planning);
        assert!(rejected(&reordered, &planning));

        let mut rewritten = core.clone();
        let mut executables = rewritten.executables.to_vec();
        let executable = executables
            .iter_mut()
            .find(|executable| {
                executable.regions.iter().any(|region| {
                    region.operations.iter().any(|operation| {
                        operation
                            .custody
                            .iter()
                            .any(|effect| effect.operation == CoreCustodyOperation::Move)
                    })
                })
            })
            .expect("moved custody executable");
        let move_site = executable
            .regions
            .iter()
            .flat_map(|region| region.operations.iter())
            .find(|operation| {
                operation
                    .custody
                    .iter()
                    .any(|effect| effect.operation == CoreCustodyOperation::Move)
            })
            .expect("move operation")
            .provenance
            .clone();
        executable.rewrites = Arc::from([RewriteWitness {
            kind: CoreRewriteKind::EliminatedPass,
            provenance: move_site,
            source_order: 0,
        }]);
        rewritten.executables = executables.into();
        resign(&mut rewritten, &planning);
        assert!(rejected(&rewritten, &planning));
    }

    #[test]
    fn custody_verifier_rejects_alias_escape_suspension_and_false_proof_type() {
        let (core, planning) = custody_fixture();

        let mut alias = corrupt_custody_effect(&core, CoreCustodyOperation::SharedLoan, |effect| {
            effect.operation = CoreCustodyOperation::ExclusiveLoan;
            effect.loan = CoreLoanEffect::Exclusive;
        });
        resign(&mut alias, &planning);
        assert!(rejected(&alias, &planning));

        let mut false_proof =
            corrupt_source_operation(&core, CoreOperationKind::Propagate, |operation| {
                let mut effects = operation.custody.to_vec();
                let mut proof = custody_effect(
                    CoreCustodyOperation::ProofCondition,
                    (
                        CoreInitializationEffect::None,
                        CoreCustodianEffect::None,
                        CoreLoanEffect::None,
                        CoreObligationEffect::None,
                    ),
                    Arc::from([]),
                    None,
                    None,
                    None,
                );
                proof.proof = Some(ProofCondition {
                    requirement_identity: 1,
                    requirement_current_meaning: 2,
                    source_type_identity: 3,
                    retains_source_return_type: true,
                });
                effects.push(proof);
                operation.custody = effects.into();
            });
        resign(&mut false_proof, &planning);
        assert!(rejected(&false_proof, &planning));

        let (suspension_core, suspension_planning) = suspension_fixture();
        let mut suspended = suspension_core.clone();
        let mut executables = suspended.executables.to_vec();
        let executable = executables
            .iter_mut()
            .find(|executable| {
                executable.regions.iter().any(|region| {
                    region
                        .operations
                        .iter()
                        .any(|operation| operation.kind == CoreOperationKind::Suspension)
                })
            })
            .expect("suspending executable");
        let resource_parameter = executable
            .signature
            .parameters
            .iter()
            .find(|parameter| parameter.access == CoreAccessLaw::Move)
            .expect("Resource parameter")
            .clone();
        let mut regions = executable.regions.to_vec();
        let region = regions
            .iter_mut()
            .find(|region| {
                region
                    .operations
                    .iter()
                    .any(|operation| operation.kind == CoreOperationKind::Suspension)
            })
            .expect("suspension region");
        let mut operations = region.operations.to_vec();
        let operation = operations
            .iter_mut()
            .find(|operation| operation.kind == CoreOperationKind::Suspension)
            .expect("suspension operation");
        let mut effects = operation.custody.to_vec();
        effects.push(custody_effect(
            CoreCustodyOperation::SharedLoan,
            (
                CoreInitializationEffect::None,
                CoreCustodianEffect::None,
                CoreLoanEffect::Shared,
                CoreObligationEffect::None,
            ),
            Arc::from([u128::from(resource_parameter.local)]),
            Some(resource_parameter.type_.identity),
            None,
            None,
        ));
        operation.custody = effects.into();
        region.operations = operations.into();
        executable.regions = regions.into();
        suspended.executables = executables.into();
        resign(&mut suspended, &suspension_planning);
        assert!(rejected(&suspended, &suspension_planning));
    }

    #[test]
    fn custody_verifier_rejects_cleanup_on_panic_and_cancels_without_publication() {
        let (core, planning) = custody_fixture();
        let mut panic_cleanup = core.clone();
        let mut executables = panic_cleanup.executables.to_vec();
        let executable = executables
            .iter_mut()
            .find(|executable| {
                executable.regions.iter().any(|region| {
                    region
                        .operations
                        .iter()
                        .any(|operation| operation.kind == CoreOperationKind::TerminalPanic)
                })
            })
            .expect("Panic executable");
        let mut regions = executable.regions.to_vec();
        let region = regions
            .iter_mut()
            .find(|region| {
                region
                    .operations
                    .iter()
                    .any(|operation| operation.kind == CoreOperationKind::TerminalPanic)
            })
            .expect("Panic region");
        let mut operations = region.operations.to_vec();
        let operation = operations
            .iter_mut()
            .find(|operation| operation.kind == CoreOperationKind::TerminalPanic)
            .expect("Panic operation");
        let mut effects = operation.custody.to_vec();
        let mut cleanup = custody_effect(
            CoreCustodyOperation::CleanupRun,
            (
                CoreInitializationEffect::None,
                CoreCustodianEffect::Discharge,
                CoreLoanEffect::None,
                CoreObligationEffect::Discharge,
            ),
            Arc::from([]),
            None,
            Some(custody_home(b"cleanup", 1)),
            Some(custody_home(b"discharged", 1)),
        );
        cleanup.cleanup_ordinal = Some(1);
        effects.push(cleanup);
        operation.custody = effects.into();
        region.operations = operations.into();
        executable.regions = regions.into();
        panic_cleanup.executables = executables.into();
        resign(&mut panic_cleanup, &planning);
        assert!(rejected(&panic_cleanup, &planning));

        let cancellation = Cancellation::new();
        cancellation.cancel_after_private_polls(1);
        assert_eq!(
            validate_custody(
                &core,
                planning.for_core().completed_semantic_program(),
                planning.for_core(),
                &cancellation,
            ),
            Err(CoreFailure::Cancelled)
        );
    }

    #[test]
    fn source_oracle_and_verifier_reject_stale_join_discharge_after_both_arms_move() {
        let (core, planning) = branch_custody_fixture();
        let mut candidate = core.clone();
        let mut executables = candidate.executables.to_vec();
        let executable = executables
            .iter_mut()
            .find(|executable| {
                executable
                    .regions
                    .iter()
                    .flat_map(|region| region.operations.iter())
                    .any(|operation| operation.kind == CoreOperationKind::Branch)
            })
            .expect("branching custody executable");
        let parameter = executable
            .signature
            .parameters
            .iter()
            .find(|parameter| parameter.access == CoreAccessLaw::Move)
            .expect("Resource parameter")
            .clone();
        let mut regions = executable.regions.to_vec();
        let operation = regions
            .iter_mut()
            .flat_map(|region| Arc::make_mut(&mut region.operations).iter_mut())
            .find(|operation| operation.kind == CoreOperationKind::RecoverableExit)
            .expect("function fallthrough exit");
        let place = Arc::<[u128]>::from([u128::from(parameter.local)]);
        let mut effects = operation.custody.to_vec();
        effects.push(producer_discharge_effect(
            &place,
            parameter.type_.identity,
            planning
                .for_core()
                .completed_semantic_program()
                .verified_program(),
        ));
        operation.custody = effects.into();
        executable.regions = regions.into();
        candidate.executables = executables.into();
        resign_fingerprints_only(&mut candidate);

        let (_, agrees, _) =
            run_custody_oracle(&candidate, planning.for_core(), &Cancellation::new())
                .expect("source oracle remains executable");
        assert!(!agrees, "direct source dataflow must catch the stale join");
        assert!(rejected(&candidate, &planning));
    }

    #[test]
    fn custody_observation_cancels_during_large_publication_without_partial_artifact() {
        let (core, _) = custody_fixture();
        let cancellation = Cancellation::new();
        cancellation.cancel_after_private_polls(20);
        assert_eq!(core.observation(&cancellation), Err(CoreFailure::Cancelled));
    }

    #[test]
    fn verification_cancels_mid_artifact_and_purpose_views_index_one_authoritative_graph() {
        let (core, planning) = fixture();
        let cancellation = Cancellation::new();
        cancellation.cancel_after_private_polls(7);
        assert_eq!(
            verify(&core, planning.for_core(), &cancellation),
            Err(CoreFailure::Cancelled)
        );

        assert_eq!(
            core.for_custody().executables().len(),
            core.executables.len()
        );
        assert_eq!(core.for_flow().executables().len(), core.executables.len());
        let backend = core.for_backend();
        let indexed = backend.executables().collect::<Vec<_>>();
        assert_eq!(indexed.len(), core.executables.len());
        assert!(
            indexed
                .iter()
                .all(|executable| executable.context() == core.context)
        );
        assert!(indexed.iter().any(|executable| {
            executable.regions().any(|region| {
                region.operations().any(|operation| {
                    operation.identity() < u32::MAX
                        && operation.provenance().path() == "src/image.wr"
                })
            })
        }));
    }

    #[test]
    fn oracle_budget_boundary_and_call_cycle_are_contained_deterministically() {
        let cancellation = Cancellation::new();
        let mut boundary = OracleBudget {
            remaining: 1,
            cancellation: &cancellation,
        };
        assert_eq!(boundary.step(), Ok(()));
        assert!(matches!(boundary.step(), Err(CoreFailure::Defect(_))));

        let (core, _) = fixture();
        let mut cyclic = core.clone();
        let mut executables = cyclic.executables.to_vec();
        let caller = executables
            .iter_mut()
            .find(|executable| {
                executable.reference.kind == CoreExecutableKind::SourceSpecialization
                    && executable.regions.iter().any(|region| {
                        region
                            .operations
                            .iter()
                            .any(|operation| operation.kind == CoreOperationKind::Call)
                    })
            })
            .expect("direct caller");
        let caller_identity = caller.reference.identity;
        let mut regions = caller.regions.to_vec();
        let region = regions
            .iter_mut()
            .find(|region| {
                region
                    .operations
                    .iter()
                    .any(|operation| operation.kind == CoreOperationKind::Call)
            })
            .expect("call region");
        let mut operations = region.operations.to_vec();
        let call = operations
            .iter_mut()
            .find(|operation| operation.kind == CoreOperationKind::Call)
            .expect("call operation");
        let mut details = call.details.to_vec();
        details[2] = caller_identity;
        call.details = details.into();
        region.operations = operations.into();
        caller.regions = regions.into();
        cyclic.executables = executables.into();
        let caller = cyclic
            .executables
            .iter()
            .find(|executable| executable.reference.identity == caller_identity)
            .expect("cyclic caller");
        let mut support_budget = OracleBudget::new(&cyclic, &cancellation);
        assert_eq!(
            oracle_supported(&cyclic, caller, &mut BTreeSet::new(), &mut support_budget,),
            Ok(false)
        );
        assert_eq!(
            oracle_execution_bound(&cyclic, caller, &mut BTreeSet::new()),
            None
        );
    }
}
