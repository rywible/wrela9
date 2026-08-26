#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use xxhash_rust::xxh3::{Xxh3, xxh3_128};

use crate::completed_semantic::{
    CoreSourceExecutableBody, CoreSourceExecutableInput, CoreSourceExecutableKind,
};
use crate::image_planning::{CorePlanningInput, ExecutableRef, GeneratedRole};
use crate::model::{IntegerType, SpecializationId};
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
            Self::Match => 23,
            Self::Cleanup => 24,
            Self::PoolScope => 25,
            Self::Break => 26,
            Self::Continue => 27,
            Self::Pass => 28,
            Self::Suspension => 29,
            Self::GeneratedRole => 30,
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
}

impl FailureLaw {
    const fn tag(self) -> u8 {
        match self {
            Self::None => 0,
            Self::CheckBeforeSuccess => 1,
            Self::PropagateInOrder => 2,
            Self::TerminalPanic => 3,
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
    failure: FailureLaw,
    provenance: SourceRange,
    oracle: Option<OracleInstruction>,
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
    parameter_count: usize,
    source_definition: Option<u128>,
    fingerprint: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum OracleInstruction {
    Literal(CanonicalValue),
    Read(u32),
    Store { local: u32, value: ValueId },
    Binary(BinaryOperator),
    ShortCircuit(BinaryOperator),
    UnaryNegate,
    UnaryNot,
    Return(Option<ValueId>),
    Panic,
    Assert(ValueId),
    Branch(ValueId),
    Pass,
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
    pub(crate) fn executable_identities(&self) -> impl ExactSizeIterator<Item = u128> + '_ {
        self.core
            .executables
            .iter()
            .map(|executable| executable.reference.identity)
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
            executables.push(produce_generated_executable(*addition, role));
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
    let (owner, provenance, parameters, source_definition, facts, regions) = match input.body {
        CoreSourceExecutableBody::Specialization(function) => {
            let upstream = semantic
                .specialization_facts(SpecializationId(reference.identity))
                .ok_or_else(|| CoreFailure::Defect(Arc::from("Core body has no solved facts")))?;
            let mut lowerer = ProducerLowerer::new(cancellation);
            let entry = lowerer.lower_block(&function.body)?;
            debug_assert_eq!(entry, RegionId(0));
            (
                function.id.0,
                function.source.clone(),
                function.parameters.len(),
                Some(function.id.0),
                ExecutableFacts {
                    pure: upstream.pure,
                    may_panic: upstream.may_panic,
                    suspends: upstream.suspends,
                    ownership_transfer: upstream.ownership_transfer,
                    evaluator_eligible: upstream.evaluator_eligible,
                },
                lowerer.finish(),
            )
        }
        CoreSourceExecutableBody::Test { body, source } => {
            let mut lowerer = ProducerLowerer::new(cancellation);
            let entry = lowerer.lower_block(body)?;
            debug_assert_eq!(entry, RegionId(0));
            let regions = lowerer.finish();
            (
                reference.identity,
                source.clone(),
                0,
                None,
                facts_from_regions(&regions),
                regions,
            )
        }
        CoreSourceExecutableBody::Closure(closure) => {
            let mut lowerer = ProducerLowerer::new(cancellation);
            let entry = lowerer.lower_expression_body(&closure.body)?;
            debug_assert_eq!(entry, RegionId(0));
            let regions = lowerer.finish();
            (
                closure.id.0,
                closure.source.clone(),
                closure.parameters.len(),
                None,
                facts_from_regions(&regions),
                regions,
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
        parameter_count: parameters,
        source_definition,
        fingerprint: 0,
    };
    executable.fingerprint = producer_executable_fingerprint(&executable);
    Ok(executable)
}

fn produce_generated_executable(executable: ExecutableRef, role: &GeneratedRole) -> CoreExecutable {
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
        oracle: None,
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
        parameter_count: 0,
        source_definition: None,
        fingerprint: 0,
    };
    core.fingerprint = producer_executable_fingerprint(&core);
    core
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

struct ProducerLowerer<'a> {
    cancellation: &'a Cancellation,
    regions: Vec<Region>,
    next_value: u32,
    next_operation: u32,
}

impl<'a> ProducerLowerer<'a> {
    fn new(cancellation: &'a Cancellation) -> Self {
        Self {
            cancellation,
            regions: Vec::new(),
            next_value: 0,
            next_operation: 0,
        }
    }

    fn finish(self) -> Arc<[Region]> {
        self.regions.into()
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
            Some(OracleInstruction::Return(Some(value))),
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
        oracle: Option<OracleInstruction>,
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
            failure,
            provenance,
            oracle,
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
                    Some(OracleInstruction::Return(value)),
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
                    Some(OracleInstruction::Panic),
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
                    FailureLaw::CheckBeforeSuccess,
                    source.clone(),
                    (kind == CoreOperationKind::Assert).then_some(OracleInstruction::Assert(value)),
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
                    place
                        .projections
                        .is_empty()
                        .then_some(OracleInstruction::Store {
                            local: place.local.0,
                            value,
                        }),
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
                    Some(OracleInstruction::Branch(condition)),
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
                    None,
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
                    None,
                );
            }
            Statement::While {
                condition,
                body,
                max_iterations,
                source,
            } => {
                let condition = self.lower_expression(condition, operations)?;
                let body = self.lower_block(body)?;
                self.push_operation(
                    operations,
                    CoreOperationKind::Loop,
                    None,
                    None,
                    [condition],
                    [body],
                    [u128::from(*max_iterations)],
                    EffectBoundary::None,
                    FailureLaw::None,
                    source.clone(),
                    None,
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
                    None,
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
                    None,
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
                    None,
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
                    None,
                );
            }
            Statement::Pass(source) => self.push_operation(
                operations,
                CoreOperationKind::Pass,
                None,
                None,
                [],
                [],
                [],
                EffectBoundary::None,
                FailureLaw::None,
                source.clone(),
                Some(OracleInstruction::Pass),
            ),
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
        let (kind, operands, details, effect, failure, oracle) = match &expression.kind {
            ExpressionKind::Literal(literal) => (
                CoreOperationKind::Literal,
                vec![],
                literal_details(literal),
                EffectBoundary::None,
                FailureLaw::None,
                Some(OracleInstruction::Literal(canonical_literal(literal))),
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
                place
                    .projections
                    .is_empty()
                    .then_some(OracleInstruction::Read(place.local.0)),
            ),
            ExpressionKind::Constant(identity) => (
                CoreOperationKind::Constant,
                vec![],
                vec![identity.0],
                EffectBoundary::None,
                FailureLaw::PropagateInOrder,
                None,
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
                None,
            ),
            ExpressionKind::Closure(closure) => (
                CoreOperationKind::ClosureValue,
                vec![],
                vec![closure.id.0],
                EffectBoundary::None,
                FailureLaw::None,
                None,
            ),
            ExpressionKind::CleanupCapture(capture) => (
                CoreOperationKind::Read,
                vec![],
                vec![u128::from(capture.0), u128::MAX],
                EffectBoundary::Ownership,
                FailureLaw::None,
                None,
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
                    None,
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
                    None,
                )
            }
            ExpressionKind::RepeatedArray { value, length } => (
                CoreOperationKind::Aggregate,
                vec![self.lower_expression(value, operations)?],
                vec![2, u128::from(*length)],
                EffectBoundary::Ownership,
                FailureLaw::PropagateInOrder,
                None,
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
                None,
            ),
            ExpressionKind::Positive(value) => (
                CoreOperationKind::Unary,
                vec![self.lower_expression(value, operations)?],
                vec![1],
                EffectBoundary::None,
                FailureLaw::None,
                None,
            ),
            ExpressionKind::Negate(value) => (
                CoreOperationKind::Unary,
                vec![self.lower_expression(value, operations)?],
                vec![2],
                EffectBoundary::None,
                FailureLaw::CheckBeforeSuccess,
                Some(OracleInstruction::UnaryNegate),
            ),
            ExpressionKind::BitNot(value) => (
                CoreOperationKind::Unary,
                vec![self.lower_expression(value, operations)?],
                vec![3],
                EffectBoundary::None,
                FailureLaw::None,
                None,
            ),
            ExpressionKind::Not(value) => (
                CoreOperationKind::Unary,
                vec![self.lower_expression(value, operations)?],
                vec![4],
                EffectBoundary::None,
                FailureLaw::None,
                Some(OracleInstruction::UnaryNot),
            ),
            ExpressionKind::Await(value) => (
                CoreOperationKind::Suspension,
                vec![self.lower_expression(value, operations)?],
                vec![],
                EffectBoundary::Suspension,
                FailureLaw::PropagateInOrder,
                None,
            ),
            ExpressionKind::Propagate(value) => (
                CoreOperationKind::Propagate,
                vec![self.lower_expression(value, operations)?],
                vec![],
                EffectBoundary::Ownership,
                FailureLaw::PropagateInOrder,
                None,
            ),
            ExpressionKind::Is { value, pattern } => (
                CoreOperationKind::PatternTest,
                vec![self.lower_expression(value, operations)?],
                pattern_details(pattern),
                EffectBoundary::Ownership,
                FailureLaw::None,
                None,
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
                        Some(OracleInstruction::ShortCircuit(*operator)),
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
                    Some(OracleInstruction::Binary(*operator)),
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
            oracle,
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
        facts.may_panic |= operation.failure != FailureLaw::None;
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
        Literal::Text(value) => vec![5, xxh3_128(value.as_bytes())],
        Literal::Scalar(value) => vec![6, u128::from(u32::from(*value))],
        Literal::Bytes(value) => vec![7, xxh3_128(value)],
    }
}

fn canonical_literal(literal: &Literal) -> CanonicalValue {
    match literal {
        Literal::Unit => CanonicalValue::Unit,
        Literal::Bool(value) => CanonicalValue::Bool(*value),
        Literal::Integer { kind, value } => CanonicalValue::Integer {
            type_name: Arc::from(kind.name()),
            value: *value,
        },
        Literal::Float { kind, bits } => CanonicalValue::Float {
            type_name: Arc::from(kind.name()),
            bits: *bits,
        },
        Literal::Text(value) => CanonicalValue::Text(value.clone()),
        Literal::Scalar(value) => CanonicalValue::Scalar(*value),
        Literal::Bytes(value) => CanonicalValue::Bytes(value.clone()),
    }
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

fn producer_executable_fingerprint(executable: &CoreExecutable) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(b"wrela.core.executable\0\x01");
    encode_executable(&mut hash, executable);
    hash.digest128()
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

fn encode_executable(hash: &mut Xxh3, executable: &CoreExecutable) {
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
    hash.update(&(executable.parameter_count as u64).to_be_bytes());
    hash.update(&executable.source_definition.unwrap_or(0).to_be_bytes());
    hash.update(&[
        u8::from(executable.facts.pure),
        u8::from(executable.facts.may_panic),
        u8::from(executable.facts.suspends),
        u8::from(executable.facts.ownership_transfer),
        u8::from(executable.facts.evaluator_eligible),
    ]);
    for region in executable.regions.iter() {
        hash.update(&region.identity.0.to_be_bytes());
        for operation in region.operations.iter() {
            hash.update(&operation.identity.to_be_bytes());
            hash.update(&[
                operation.kind.tag(),
                operation.effect.tag(),
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
                hash.update(&operand.0.to_be_bytes());
            }
            hash.update(b"|");
            for successor in operation.successors.iter() {
                hash.update(&successor.0.to_be_bytes());
            }
            hash.update(b"|");
            for detail in operation.details.iter() {
                hash.update(&detail.to_be_bytes());
            }
            encode_source(hash, &operation.provenance);
        }
    }
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
    let mut expected = Vec::new();
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
        if independent_hir_trace(source, cancellation)? != candidate_trace(supplied) {
            return defect("Core operation graph contradicts independent Typed HIR reconstruction");
        }
        expected.push(reconstruct_source_executable(
            source,
            semantic,
            cancellation,
        )?);
    }
    for addition in input.generated_executable_additions() {
        checkpoint(cancellation)?;
        let role = input
            .generated_roles()
            .iter()
            .find(|role| role.executable().identity() == addition.identity())
            .ok_or_else(|| CoreFailure::Defect(Arc::from("Core verifier found a dangling role")))?;
        expected.push(reconstruct_generated_executable(*addition, role));
    }
    expected.sort_by_key(|executable| (executable.reference.kind, executable.reference.identity));
    if candidate.executables.as_ref() != expected.as_slice() {
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct AuditToken {
    kind: CoreOperationKind,
    type_identity: Option<u128>,
    source: SourceRange,
    operand_count: usize,
    successor_count: usize,
}

fn candidate_trace(executable: &CoreExecutable) -> Vec<AuditToken> {
    let mut operations = executable
        .regions
        .iter()
        .flat_map(|region| region.operations.iter())
        .collect::<Vec<_>>();
    operations.sort_by_key(|operation| operation.identity);
    operations
        .into_iter()
        .map(|operation| AuditToken {
            kind: operation.kind,
            type_identity: operation.type_identity,
            source: operation.provenance.clone(),
            operand_count: operation.operands.len(),
            successor_count: operation.successors.len(),
        })
        .collect()
}

fn independent_hir_trace(
    source: CoreSourceExecutableInput<'_>,
    cancellation: &Cancellation,
) -> Result<Vec<AuditToken>, CoreFailure> {
    let mut trace = Vec::new();
    match source.body {
        CoreSourceExecutableBody::Specialization(function) => {
            audit_statements(&function.body, cancellation, &mut trace)?;
        }
        CoreSourceExecutableBody::Test { body, .. } => {
            audit_statements(body, cancellation, &mut trace)?;
        }
        CoreSourceExecutableBody::Closure(closure) => {
            audit_expression(&closure.body, cancellation, &mut trace)?;
            trace.push(AuditToken {
                kind: CoreOperationKind::Return,
                type_identity: None,
                source: closure.body.source.clone(),
                operand_count: 1,
                successor_count: 0,
            });
        }
    }
    Ok(trace)
}

fn audit_statements(
    statements: &[Statement],
    cancellation: &Cancellation,
    trace: &mut Vec<AuditToken>,
) -> Result<(), CoreFailure> {
    for statement in statements {
        checkpoint(cancellation)?;
        match statement {
            Statement::Return { value, source } => {
                if let Some(value) = value {
                    audit_expression(value, cancellation, trace)?;
                }
                audit_push(
                    trace,
                    CoreOperationKind::Return,
                    None,
                    source,
                    usize::from(value.is_some()),
                    0,
                );
            }
            Statement::Panic { value, source } => {
                audit_expression(value, cancellation, trace)?;
                audit_push(trace, CoreOperationKind::TerminalPanic, None, source, 1, 0);
            }
            Statement::Assert { condition, source } => {
                audit_expression(condition, cancellation, trace)?;
                audit_push(trace, CoreOperationKind::Assert, None, source, 1, 0);
            }
            Statement::Expect { condition, source } => {
                audit_expression(condition, cancellation, trace)?;
                audit_push(trace, CoreOperationKind::Expect, None, source, 1, 0);
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
                audit_place_indexes(place, cancellation, trace)?;
                audit_expression(value, cancellation, trace)?;
                let index_count = place
                    .projections
                    .iter()
                    .filter(|projection| matches!(projection, PlaceProjection::Index { .. }))
                    .count();
                audit_push(
                    trace,
                    CoreOperationKind::Store,
                    None,
                    source,
                    index_count + 1,
                    0,
                );
            }
            Statement::Evaluate(expression) => audit_expression(expression, cancellation, trace)?,
            Statement::If {
                condition,
                then_branch,
                else_branch,
                source,
            } => {
                audit_expression(condition, cancellation, trace)?;
                audit_statements(then_branch, cancellation, trace)?;
                audit_statements(else_branch, cancellation, trace)?;
                audit_push(trace, CoreOperationKind::Branch, None, source, 1, 2);
            }
            Statement::IfPattern {
                value,
                then_branch,
                else_branch,
                source,
                ..
            } => {
                audit_expression(value, cancellation, trace)?;
                audit_statements(then_branch, cancellation, trace)?;
                audit_statements(else_branch, cancellation, trace)?;
                audit_push(trace, CoreOperationKind::Branch, None, source, 1, 2);
            }
            Statement::For {
                iterable,
                body,
                source,
                ..
            } => {
                audit_expression(iterable, cancellation, trace)?;
                audit_statements(body, cancellation, trace)?;
                audit_push(trace, CoreOperationKind::Loop, None, source, 1, 1);
            }
            Statement::While {
                condition,
                body,
                source,
                ..
            } => {
                audit_expression(condition, cancellation, trace)?;
                audit_statements(body, cancellation, trace)?;
                audit_push(trace, CoreOperationKind::Loop, None, source, 1, 1);
            }
            Statement::Break(source) => {
                audit_push(trace, CoreOperationKind::Break, None, source, 0, 0)
            }
            Statement::Continue(source) => {
                audit_push(trace, CoreOperationKind::Continue, None, source, 0, 0)
            }
            Statement::Match {
                value,
                cases,
                source,
            } => {
                audit_expression(value, cancellation, trace)?;
                let mut operands = 1;
                for case in cases.iter() {
                    if let Some(guard) = &case.guard {
                        audit_expression(guard, cancellation, trace)?;
                        operands += 1;
                    }
                    audit_statements(&case.body, cancellation, trace)?;
                }
                // Guards are retained as exact ordered Value references in the detail
                // catalog rather than ordinary operation operands.
                let _ = operands;
                audit_push(
                    trace,
                    CoreOperationKind::Match,
                    None,
                    source,
                    1,
                    cases.len(),
                );
            }
            Statement::Defer { action, source } => {
                for capture in action.captures.iter() {
                    audit_expression(&capture.expression, cancellation, trace)?;
                }
                audit_expression(&action.expression, cancellation, trace)?;
                audit_push(
                    trace,
                    CoreOperationKind::Cleanup,
                    None,
                    source,
                    action.captures.len(),
                    1,
                );
            }
            Statement::WithPool {
                binding,
                scope,
                body,
                source,
            } => {
                audit_expression(scope, cancellation, trace)?;
                audit_place_indexes(binding, cancellation, trace)?;
                audit_statements(body, cancellation, trace)?;
                audit_push(trace, CoreOperationKind::PoolScope, None, source, 1, 1);
            }
            Statement::Pass(source) => {
                audit_push(trace, CoreOperationKind::Pass, None, source, 0, 0)
            }
        }
    }
    Ok(())
}

fn audit_expression(
    expression: &Expression,
    cancellation: &Cancellation,
    trace: &mut Vec<AuditToken>,
) -> Result<(), CoreFailure> {
    checkpoint(cancellation)?;
    let (kind, operand_count) = match &expression.kind {
        ExpressionKind::Literal(_) => (CoreOperationKind::Literal, 0),
        ExpressionKind::Read(place) => {
            audit_place_indexes(place, cancellation, trace)?;
            (
                CoreOperationKind::Read,
                place
                    .projections
                    .iter()
                    .filter(|projection| matches!(projection, PlaceProjection::Index { .. }))
                    .count(),
            )
        }
        ExpressionKind::Constant(_) => (CoreOperationKind::Constant, 0),
        ExpressionKind::FunctionValue { .. } => (CoreOperationKind::FunctionValue, 0),
        ExpressionKind::Closure(closure) => {
            let _ = closure;
            (CoreOperationKind::ClosureValue, 0)
        }
        ExpressionKind::CleanupCapture(_) => (CoreOperationKind::Read, 0),
        ExpressionKind::Call { target, arguments } => {
            let callable = if let CallTarget::Callable { value } = target {
                audit_expression(value, cancellation, trace)?;
                1
            } else {
                0
            };
            for argument in arguments.iter() {
                audit_expression(argument, cancellation, trace)?;
            }
            (
                if matches!(target, CallTarget::Build { .. }) {
                    CoreOperationKind::BuildConstruction
                } else {
                    CoreOperationKind::Call
                },
                callable + arguments.len(),
            )
        }
        ExpressionKind::Array(values) | ExpressionKind::Tuple(values) => {
            for value in values.iter() {
                audit_expression(value, cancellation, trace)?;
            }
            (CoreOperationKind::Aggregate, values.len())
        }
        ExpressionKind::RepeatedArray { value, .. } => {
            audit_expression(value, cancellation, trace)?;
            (CoreOperationKind::Aggregate, 1)
        }
        ExpressionKind::Index { value, index } => {
            audit_expression(value, cancellation, trace)?;
            audit_expression(index, cancellation, trace)?;
            (CoreOperationKind::Index, 2)
        }
        ExpressionKind::Positive(value)
        | ExpressionKind::Negate(value)
        | ExpressionKind::BitNot(value)
        | ExpressionKind::Not(value) => {
            audit_expression(value, cancellation, trace)?;
            (CoreOperationKind::Unary, 1)
        }
        ExpressionKind::Await(value) => {
            audit_expression(value, cancellation, trace)?;
            (CoreOperationKind::Suspension, 1)
        }
        ExpressionKind::Propagate(value) => {
            audit_expression(value, cancellation, trace)?;
            (CoreOperationKind::Propagate, 1)
        }
        ExpressionKind::Is { value, .. } => {
            audit_expression(value, cancellation, trace)?;
            (CoreOperationKind::PatternTest, 1)
        }
        ExpressionKind::Binary {
            operator,
            left,
            right,
        } => {
            audit_expression(left, cancellation, trace)?;
            audit_expression(right, cancellation, trace)?;
            let kind = binary_kind(*operator);
            audit_push(
                trace,
                kind,
                Some(expression.type_id.0),
                &expression.source,
                2,
                usize::from(kind == CoreOperationKind::ShortCircuit),
            );
            return Ok(());
        }
    };
    audit_push(
        trace,
        kind,
        Some(expression.type_id.0),
        &expression.source,
        operand_count,
        0,
    );
    Ok(())
}

fn audit_place_indexes(
    place: &Place,
    cancellation: &Cancellation,
    trace: &mut Vec<AuditToken>,
) -> Result<(), CoreFailure> {
    for projection in place.projections.iter() {
        if let PlaceProjection::Index { index, .. } = projection {
            audit_expression(index, cancellation, trace)?;
        }
    }
    Ok(())
}

fn audit_push(
    trace: &mut Vec<AuditToken>,
    kind: CoreOperationKind,
    type_identity: Option<u128>,
    source: &SourceRange,
    operand_count: usize,
    successor_count: usize,
) {
    trace.push(AuditToken {
        kind,
        type_identity,
        source: source.clone(),
        operand_count,
        successor_count,
    });
}

// The verifier deliberately reconstructs through a fresh lowering instance and a
// distinct canonical byte encoding. Candidate data is never trusted as expected data.
fn reconstruct_source_executable(
    input: CoreSourceExecutableInput<'_>,
    semantic: crate::completed_semantic::CorePlanningSemanticProgram<'_>,
    cancellation: &Cancellation,
) -> Result<CoreExecutable, CoreFailure> {
    let mut reconstructed = produce_source_executable(input, semantic, cancellation)?;
    reconstructed.fingerprint = verifier_executable_fingerprint(&reconstructed);
    Ok(reconstructed)
}

fn reconstruct_generated_executable(
    executable: ExecutableRef,
    role: &GeneratedRole,
) -> CoreExecutable {
    let mut reconstructed = produce_generated_executable(executable, role);
    reconstructed.fingerprint = verifier_executable_fingerprint(&reconstructed);
    reconstructed
}

fn verifier_executable_fingerprint(executable: &CoreExecutable) -> u128 {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"wrela.core.executable\0\x01");
    verifier_encode_executable(&mut bytes, executable);
    xxh3_128(&bytes)
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
        bytes.extend_from_slice(&verifier_executable_fingerprint(executable).to_be_bytes());
    }
    bytes.extend_from_slice(&(candidate.oracle.cases as u64).to_be_bytes());
    bytes.push(u8::from(candidate.oracle.agrees));
    bytes.extend_from_slice(&candidate.oracle.fingerprint.to_be_bytes());
    Ok(xxh3_128(&bytes))
}

fn verifier_encode_executable(bytes: &mut Vec<u8>, executable: &CoreExecutable) {
    bytes.push(executable.reference.kind.tag());
    bytes.extend_from_slice(&executable.reference.context.to_be_bytes());
    bytes.extend_from_slice(&executable.reference.identity.to_be_bytes());
    bytes.extend_from_slice(&executable.reference.current_meaning.to_be_bytes());
    bytes.extend_from_slice(&executable.semantic_owner.to_be_bytes());
    verifier_encode_source(bytes, &executable.provenance);
    bytes.extend_from_slice(&executable.entry.0.to_be_bytes());
    bytes.extend_from_slice(&(executable.parameter_count as u64).to_be_bytes());
    bytes.extend_from_slice(&executable.source_definition.unwrap_or(0).to_be_bytes());
    bytes.extend([
        u8::from(executable.facts.pure),
        u8::from(executable.facts.may_panic),
        u8::from(executable.facts.suspends),
        u8::from(executable.facts.ownership_transfer),
        u8::from(executable.facts.evaluator_eligible),
    ]);
    for region in executable.regions.iter() {
        bytes.extend_from_slice(&region.identity.0.to_be_bytes());
        for operation in region.operations.iter() {
            bytes.extend_from_slice(&operation.identity.to_be_bytes());
            bytes.extend([
                operation.kind.tag(),
                operation.effect.tag(),
                operation.failure.tag(),
            ]);
            bytes.extend_from_slice(
                &operation
                    .result
                    .map_or(u32::MAX, |value| value.0)
                    .to_be_bytes(),
            );
            bytes.extend_from_slice(&operation.type_identity.unwrap_or(0).to_be_bytes());
            operation
                .operands
                .iter()
                .for_each(|value| bytes.extend_from_slice(&value.0.to_be_bytes()));
            bytes.push(b'|');
            operation
                .successors
                .iter()
                .for_each(|region| bytes.extend_from_slice(&region.0.to_be_bytes()));
            bytes.push(b'|');
            operation
                .details
                .iter()
                .for_each(|value| bytes.extend_from_slice(&value.to_be_bytes()));
            verifier_encode_source(bytes, &operation.provenance);
        }
    }
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
                    !region_ids.contains(successor) || successor.0 <= region.identity.0
                }) {
                    return defect("Core operation has a dangling or malformed control target");
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
            || executable.parameter_count != 0
            || !executable.facts.pure
            || !executable.facts.evaluator_eligible
            || !oracle_supported(executable)
        {
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
        let core_outcome = oracle_execute(executable);
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
    Unsupported,
}

fn oracle_supported(executable: &CoreExecutable) -> bool {
    executable
        .regions
        .iter()
        .flat_map(|region| region.operations.iter())
        .all(|operation| operation.oracle.is_some())
}

enum Signal {
    Continue,
    Return(CanonicalValue),
    Panic(EvaluationPanicKind, SourceRange),
}

fn oracle_execute(executable: &CoreExecutable) -> CompactOutcome {
    let mut values = BTreeMap::new();
    let mut locals = BTreeMap::new();
    match oracle_region(executable, executable.entry, &mut values, &mut locals) {
        Some(Signal::Return(value)) => CompactOutcome::Completed(value),
        Some(Signal::Panic(kind, site)) => CompactOutcome::Panicked(kind, site),
        Some(Signal::Continue) => CompactOutcome::Completed(CanonicalValue::Unit),
        None => CompactOutcome::Unsupported,
    }
}

fn oracle_region(
    executable: &CoreExecutable,
    region: RegionId,
    values: &mut BTreeMap<ValueId, CanonicalValue>,
    locals: &mut BTreeMap<u32, CanonicalValue>,
) -> Option<Signal> {
    let region = executable.regions.get(region.0 as usize)?;
    for operation in region.operations.iter() {
        let instruction = operation.oracle.as_ref()?;
        match instruction {
            OracleInstruction::Literal(value) => {
                values.insert(operation.result?, value.clone());
            }
            OracleInstruction::Read(local) => {
                values.insert(operation.result?, locals.get(local)?.clone());
            }
            OracleInstruction::Store { local, value } => {
                locals.insert(*local, values.get(value)?.clone());
            }
            OracleInstruction::Binary(operator) => {
                let [left, right] = operation.operands.as_ref() else {
                    return None;
                };
                match oracle_binary(
                    *operator,
                    values.get(left)?,
                    values.get(right)?,
                    &operation.provenance,
                ) {
                    Ok(value) => {
                        values.insert(operation.result?, value);
                    }
                    Err(signal) => return Some(signal),
                }
            }
            OracleInstruction::ShortCircuit(operator) => {
                let [left, right] = operation.operands.as_ref() else {
                    return None;
                };
                let CanonicalValue::Bool(left_value) = values.get(left)? else {
                    return None;
                };
                let skip = (*operator == BinaryOperator::And && !*left_value)
                    || (*operator == BinaryOperator::Or && *left_value);
                if skip {
                    values.insert(operation.result?, CanonicalValue::Bool(*left_value));
                } else {
                    match oracle_region(executable, operation.successors[0], values, locals)? {
                        Signal::Continue => {}
                        signal => return Some(signal),
                    }
                    values.insert(operation.result?, values.get(right)?.clone());
                }
            }
            OracleInstruction::UnaryNegate => {
                let [operand] = operation.operands.as_ref() else {
                    return None;
                };
                let CanonicalValue::Integer { type_name, value } = values.get(operand)? else {
                    return None;
                };
                let value = value.checked_neg().ok_or_else(|| {
                    Signal::Panic(
                        EvaluationPanicKind::IntegerOverflow,
                        operation.provenance.clone(),
                    )
                });
                match value {
                    Ok(value) => {
                        values.insert(
                            operation.result?,
                            CanonicalValue::Integer {
                                type_name: type_name.clone(),
                                value,
                            },
                        );
                    }
                    Err(signal) => return Some(signal),
                }
            }
            OracleInstruction::UnaryNot => {
                let [operand] = operation.operands.as_ref() else {
                    return None;
                };
                let CanonicalValue::Bool(value) = values.get(operand)? else {
                    return None;
                };
                values.insert(operation.result?, CanonicalValue::Bool(!value));
            }
            OracleInstruction::Return(value) => {
                let value = match value {
                    Some(value) => values.get(value)?.clone(),
                    None => CanonicalValue::Unit,
                };
                return Some(Signal::Return(value));
            }
            OracleInstruction::Panic => {
                return Some(Signal::Panic(
                    EvaluationPanicKind::Explicit,
                    operation.provenance.clone(),
                ));
            }
            OracleInstruction::Assert(condition) => {
                if values.get(condition) != Some(&CanonicalValue::Bool(true)) {
                    return Some(Signal::Panic(
                        EvaluationPanicKind::AssertionFailed,
                        operation.provenance.clone(),
                    ));
                }
            }
            OracleInstruction::Branch(condition) => {
                let CanonicalValue::Bool(condition) = values.get(condition)? else {
                    return None;
                };
                let successor = operation.successors[usize::from(!*condition)];
                match oracle_region(executable, successor, values, locals)? {
                    Signal::Continue => {}
                    signal => return Some(signal),
                }
            }
            OracleInstruction::Pass => {}
        }
    }
    Some(Signal::Continue)
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
        _ => Err(Signal::Continue),
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
        CompactOutcome::Unsupported => hash.update(&[3]),
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
}
