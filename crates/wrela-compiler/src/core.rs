#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use xxhash_rust::xxh3::{Xxh3, xxh3_128};

use crate::completed_semantic::{
    CoreSourceExecutableBody, CoreSourceExecutableInput, CoreSourceExecutableKind,
};
use crate::image_planning::{CorePlanningInput, ExecutableRef, GeneratedRole};
use crate::model::{IntegerType, SpecializationId, Type};
use crate::typed_hir::{
    BinaryOperator, CallTarget, Expression, ExpressionKind, HirMatchPattern, Literal, Place,
    PlaceProjection, Statement,
};
use crate::{Cancellation, CanonicalValue, EvaluationOutcome, EvaluationPanicKind, SourceRange};

pub(crate) const PHASE_SCHEMA: &str = "wrela.core.v1";
const SCHEMA_VERSION: u16 = 1;

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
        }
    }
}

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
}

impl FailureLaw {
    const fn tag(self) -> u8 {
        match self {
            Self::None => 0,
            Self::CheckBeforeSuccess => 1,
            Self::PropagateInOrder => 2,
            Self::TerminalPanic => 3,
            Self::RecordTestFailure => 4,
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
    successors: Arc<[RegionId]>,
    details: Arc<[u128]>,
    effect: EffectBoundary,
    access: CoreAccessLaw,
    failure: FailureLaw,
    provenance: SourceRange,
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
    access_laws: Arc<[CoreAccessLaw]>,
    rewrites: Arc<[CoreRewriteKind]>,
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
    pub fn access_laws(&self) -> &[CoreAccessLaw] {
        &self.access_laws
    }

    #[must_use]
    pub fn rewrites(&self) -> &[CoreRewriteKind] {
        &self.rewrites
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
}

impl VerifiedCoreProgram {
    pub(crate) const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }

    pub(crate) fn observation(&self) -> CoreProgramObservation {
        CoreProgramObservation {
            fingerprint: self.fingerprint,
            context_identity: self.context,
            planning_foundation_fingerprint: self.planning_fingerprint,
            executables: self
                .executables
                .iter()
                .map(|executable| CoreExecutableObservation {
                    kind: executable.reference.kind,
                    identity: executable.reference.identity,
                    current_meaning: executable.reference.current_meaning,
                    semantic_owner: executable.semantic_owner,
                    operations: executable
                        .regions
                        .iter()
                        .flat_map(|region| region.operations.iter().map(|operation| operation.kind))
                        .collect::<Vec<_>>()
                        .into(),
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
                    access_laws: executable
                        .regions
                        .iter()
                        .flat_map(|region| {
                            region.operations.iter().filter_map(|operation| {
                                (operation.access != CoreAccessLaw::None)
                                    .then_some(operation.access)
                            })
                        })
                        .collect::<Vec<_>>()
                        .into(),
                    rewrites: executable
                        .rewrites
                        .iter()
                        .map(|witness| witness.kind)
                        .collect::<Vec<_>>()
                        .into(),
                })
                .collect::<Vec<_>>()
                .into(),
            oracle_case_count: self.oracle.cases,
            oracle_agrees: self.oracle.agrees,
        }
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
}

#[allow(dead_code)]
pub(crate) struct FlowCoreView<'a> {
    core: &'a VerifiedCoreProgram,
}

#[allow(dead_code)]
impl FlowCoreView<'_> {
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
            executables.push(produce_source_executable(source, semantic, cancellation)?);
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
    cancellation: &Cancellation,
) -> Result<CoreExecutable, CoreFailure> {
    let reference = ExecutableReference {
        context: input.reference.context(),
        kind: source_kind(input.reference.kind()),
        identity: input.reference.identity(),
        current_meaning: input.reference.current_meaning(),
    };
    let (owner, provenance, signature, source_definition, facts, regions, rewrites) = match input
        .body
    {
        CoreSourceExecutableBody::Specialization(function) => {
            let upstream = semantic
                .specialization_facts(SpecializationId(reference.identity))
                .ok_or_else(|| CoreFailure::Defect(Arc::from("Core body has no solved facts")))?;
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
                        .map(|(local, type_, access)| (local.0, type_, core_access(*access))),
                    &function.return_type,
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
                        .map(|(local, type_, access)| (local.0, type_, core_access(*access))),
                    &Type::Unit,
                ),
                None,
                facts_from_regions(&regions),
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
                        .map(|(local, type_)| (local.0, type_, CoreAccessLaw::CopyValue)),
                    &closure.return_type,
                ),
                None,
                facts_from_regions(&regions),
                regions,
                rewrites,
            )
        }
    };
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
        successors: Arc::from([]),
        details: details.into(),
        effect: EffectBoundary::None,
        failure: FailureLaw::None,
        provenance: provenance.clone(),
        access: CoreAccessLaw::None,
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
        signature: signature(std::iter::empty(), &Type::Unit),
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

fn core_type(type_: &Type) -> CoreType {
    let shape = type_.canonical_key();
    CoreType {
        identity: xxh3_128(&shape),
        shape,
    }
}

fn signature<'a>(
    parameters: impl IntoIterator<Item = (u32, &'a Type, CoreAccessLaw)>,
    return_type: &Type,
) -> CoreSignature {
    CoreSignature {
        parameters: parameters
            .into_iter()
            .map(|(local, type_, access)| CoreParameter {
                local,
                type_: core_type(type_),
                access,
            })
            .collect::<Vec<_>>()
            .into(),
        return_type: core_type(return_type),
    }
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
            successors: successors.into_iter().collect::<Vec<_>>().into(),
            details: details.into_iter().collect::<Vec<_>>().into(),
            effect,
            access,
            failure,
            provenance,
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
                    FailureLaw::CheckBeforeSuccess,
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
                let value = self.lower_expression(value, operations)?;
                let mut successors = Vec::new();
                let mut details = vec![u128::try_from(cases.len()).unwrap_or(u128::MAX)];
                for case in cases.iter() {
                    let case_region = self.reserve_region();
                    let mut case_operations = Vec::new();
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
                    FailureLaw::PropagateInOrder,
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
                    FailureLaw::PropagateInOrder,
                )
            }
            ExpressionKind::RepeatedArray { value, length } => (
                CoreOperationKind::Aggregate,
                vec![self.lower_expression(value, operations)?],
                vec![2, u128::from(*length)],
                EffectBoundary::Ownership,
                FailureLaw::PropagateInOrder,
            ),
            ExpressionKind::Index { value, index } => (
                CoreOperationKind::Index,
                vec![
                    self.lower_expression(value, operations)?,
                    self.lower_expression(index, operations)?,
                ],
                vec![],
                EffectBoundary::None,
                FailureLaw::CheckBeforeSuccess,
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
                FailureLaw::CheckBeforeSuccess,
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
                        FailureLaw::CheckBeforeSuccess
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
        Ok(result)
    }
}

fn facts_from_regions(regions: &[Region]) -> ExecutableFacts {
    let operations = regions.iter().flat_map(|region| region.operations.iter());
    let mut facts = ExecutableFacts {
        pure: true,
        may_panic: false,
        suspends: false,
        ownership_transfer: false,
        evaluator_eligible: true,
    };
    for operation in operations {
        facts.may_panic |= matches!(
            operation.failure,
            FailureLaw::CheckBeforeSuccess
                | FailureLaw::PropagateInOrder
                | FailureLaw::TerminalPanic
        );
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

fn pattern_details(pattern: &HirMatchPattern) -> Vec<u128> {
    let mut output = Vec::new();
    append_pattern(pattern, &mut output);
    output
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
    hash.update(b"wrela.core.executable\0\x01");
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
        VerifierLowerer::verify_source(source, semantic, supplied, cancellation)?;
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
    if candidate.oracle != expected_oracle || !candidate.oracle.agrees {
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
            successors: successors.into_iter().collect::<Vec<_>>().into(),
            details: details.into_iter().collect::<Vec<_>>().into(),
            effect,
            access,
            failure,
            provenance,
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
                    FailureLaw::CheckBeforeSuccess,
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
                let value = self.reconstruct_expression(value, operations)?;
                let mut successors = Vec::new();
                let mut details = vec![u128::try_from(cases.len()).unwrap_or(u128::MAX)];
                for case in cases.iter() {
                    checkpoint(self.cancellation)?;
                    let case_region = self.reserve_region();
                    let mut case_operations = Vec::new();
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
                    FailureLaw::PropagateInOrder,
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
                    FailureLaw::PropagateInOrder,
                )
            }
            ExpressionKind::RepeatedArray { value, length } => (
                CoreOperationKind::Aggregate,
                vec![self.reconstruct_expression(value, operations)?],
                vec![2, u128::from(*length)],
                EffectBoundary::Ownership,
                FailureLaw::PropagateInOrder,
            ),
            ExpressionKind::Index { value, index } => (
                CoreOperationKind::Index,
                vec![
                    self.reconstruct_expression(value, operations)?,
                    self.reconstruct_expression(index, operations)?,
                ],
                vec![],
                EffectBoundary::None,
                FailureLaw::CheckBeforeSuccess,
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
                FailureLaw::CheckBeforeSuccess,
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
                        FailureLaw::CheckBeforeSuccess
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

fn verifier_place_details(place: &Place) -> Vec<u128> {
    let mut details = vec![u128::from(place.local.0)];
    for projection in place.projections.iter() {
        match projection {
            PlaceProjection::Field {
                definition, name, ..
            } => details.extend([1, definition.0, xxh3_128(name.as_bytes())]),
            PlaceProjection::Index { index, .. } => {
                details.extend([2, index.type_id.0]);
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
        || !operation.successors.is_empty()
        || operation.details.as_ref() != details
        || operation.effect != EffectBoundary::None
        || operation.access != CoreAccessLaw::None
        || operation.failure != FailureLaw::None
        || operation.provenance != provenance
        || !supplied.signature.parameters.is_empty()
        || !verifier_type_matches(&supplied.signature.return_type, &Type::Unit)
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
        supplied: &CoreExecutable,
        cancellation: &Cancellation,
    ) -> Result<(), CoreFailure> {
        checkpoint(cancellation)?;
        let (owner, provenance, parameters, return_type, source_definition, facts) = match input
            .body
        {
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
                    &function.return_type,
                    Some(function.id.0),
                    Some(facts),
                )
            }
            CoreSourceExecutableBody::Test(test) => (
                input.reference.identity(),
                &test.source,
                test.parameters.as_slice(),
                &Type::Unit,
                None,
                None,
            ),
            CoreSourceExecutableBody::Closure(closure) => {
                if supplied.signature.parameters.len() != closure.parameters.len()
                    || supplied
                        .signature
                        .parameters
                        .iter()
                        .zip(closure.parameters.iter())
                        .any(|(actual, (local, type_))| {
                            actual.local != local.0
                                || actual.access != CoreAccessLaw::CopyValue
                                || !verifier_type_matches(&actual.type_, type_)
                        })
                {
                    return defect("Core Closure signature is false");
                }
                (
                    closure.id.0,
                    &closure.source,
                    &[][..],
                    &closure.return_type,
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
            || !verifier_type_matches(&supplied.signature.return_type, return_type)
        {
            return defect("Core executable header or return meaning is false");
        }
        if !matches!(input.body, CoreSourceExecutableBody::Closure(_))
            && (supplied.signature.parameters.len() != parameters.len()
                || supplied.signature.parameters.iter().zip(parameters).any(
                    |(actual, (local, type_, access))| {
                        actual.local != local.0
                            || actual.access != verifier_access(*access)
                            || !verifier_type_matches(&actual.type_, type_)
                    },
                ))
        {
            return defect("Core executable parameter signature is false");
        }
        let reconstruction = VerifierLowerer::reconstruct(input.body, cancellation)?;
        if supplied.entry != RegionId(0) || supplied.regions != reconstruction.regions {
            return defect(
                "Core operation graph contradicts exact independent Typed HIR reconstruction",
            );
        }
        if supplied.rewrites != reconstruction.rewrites {
            return defect("Core canonical rewrite witness is false, missing, or misassociated");
        }
        let expected_facts = if let Some(facts) = facts {
            ExecutableFacts {
                pure: facts.pure,
                may_panic: facts.may_panic,
                suspends: facts.suspends,
                ownership_transfer: facts.ownership_transfer,
                evaluator_eligible: facts.evaluator_eligible,
            }
        } else {
            verifier_facts_from_regions(&reconstruction.regions)
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

fn verifier_facts_from_regions(regions: &[Region]) -> ExecutableFacts {
    let mut facts = ExecutableFacts {
        pure: true,
        may_panic: false,
        suspends: false,
        ownership_transfer: false,
        evaluator_eligible: true,
    };
    for operation in regions.iter().flat_map(|region| region.operations.iter()) {
        facts.may_panic |= matches!(
            operation.failure,
            FailureLaw::CheckBeforeSuccess
                | FailureLaw::PropagateInOrder
                | FailureLaw::TerminalPanic
        );
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

fn verifier_type_matches(actual: &CoreType, expected: &Type) -> bool {
    let shape = expected.canonical_key();
    actual.identity == xxh3_128(&shape) && actual.shape == shape
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
    bytes.extend_from_slice(b"wrela.core.executable\0\x01");
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
                                && legal_loop_backs.contains(&(region.identity, *successor))))
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
        let mut work = OracleBudget::new(candidate, cancellation);
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
    Ok(OracleSummary {
        cases,
        agrees,
        fingerprint: hash.digest128(),
    })
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

    fn step(&mut self) -> Result<(), CoreFailure> {
        checkpoint(self.cancellation)?;
        if self.remaining == 0 {
            return defect("Core semantic oracle exceeded its logical work budget");
        }
        self.remaining -= 1;
        Ok(())
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
            CoreOperationKind::CheckedArithmetic | CoreOperationKind::Binary => operation
                .details
                .first()
                .and_then(|tag| operator_from_tag(*tag))
                .is_some_and(|operator| {
                    !matches!(
                        operator,
                        BinaryOperator::Range | BinaryOperator::RangeInclusive
                    )
                }),
            CoreOperationKind::ShortCircuit => operation
                .details
                .first()
                .and_then(|tag| operator_from_tag(*tag))
                .is_some_and(|operator| {
                    matches!(operator, BinaryOperator::And | BinaryOperator::Or)
                }),
            CoreOperationKind::Return
            | CoreOperationKind::TerminalPanic
            | CoreOperationKind::Assert
            | CoreOperationKind::Expect
            | CoreOperationKind::Branch
            | CoreOperationKind::Loop
            | CoreOperationKind::LoopBack
            | CoreOperationKind::Break
            | CoreOperationKind::Continue => true,
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
                oracle_supported(candidate, target, visiting, budget)?
            }
            CoreOperationKind::Pass
            | CoreOperationKind::Constant
            | CoreOperationKind::ClosureValue
            | CoreOperationKind::BuildConstruction
            | CoreOperationKind::Aggregate
            | CoreOperationKind::Index
            | CoreOperationKind::Propagate
            | CoreOperationKind::PatternTest
            | CoreOperationKind::Match
            | CoreOperationKind::Cleanup
            | CoreOperationKind::PoolScope
            | CoreOperationKind::Suspension
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
                let value = oracle_unary(
                    operation.details[0],
                    required_operand(operation, 0, values)?,
                    &operation.provenance,
                )?;
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
) -> Result<CanonicalValue, CoreFailure> {
    match (tag, value) {
        (1, value) => Ok(value),
        (2, CanonicalValue::Integer { type_name, value }) => {
            let value = value
                .checked_neg()
                .filter(|value| integer_kind(&type_name).is_ok_and(|kind| kind.fits(*value)));
            value
                .map(|value| CanonicalValue::Integer { type_name, value })
                .ok_or_else(|| {
                    CoreFailure::Defect(Arc::from(format!(
                        "oracle panic:{}:{}",
                        panic_tag(EvaluationPanicKind::IntegerOverflow),
                        site.start()
                    )))
                })
        }
        (3, CanonicalValue::Integer { type_name, value }) => Ok(CanonicalValue::Integer {
            type_name,
            value: !value,
        }),
        (4, CanonicalValue::Bool(value)) => Ok(CanonicalValue::Bool(!value)),
        _ => defect("Core unary law and operand disagree"),
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
        _ => Err(Signal::Continue),
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
                br#"pure fn answer() -> i64:
    value = 6
    if true:
        return value * 7
    return value + 1

@image
fn build() -> Image:
    return Image.new(value=answer())
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
}
