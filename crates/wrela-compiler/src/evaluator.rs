#![forbid(unsafe_code)]

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use xxhash_rust::xxh3::xxh3_128;

use crate::model::{
    BuildKind, BuiltinVariant, DefinitionId, FloatType, IntegerType, SpecializationId, TestId,
    Type, VariantId,
};
use crate::typed_hir::{
    BinaryOperator, CallTarget, ClosureId, Expression, ExpressionKind, HirClosure, HirFunction,
    HirMatchCase, HirMatchPattern, Literal, LocalId, Place, PlaceProjection, Statement,
    VerifiedProgram,
};
use crate::{
    Cancellation, CanonicalValue, EvaluationContributorObservation, EvaluationFrameObservation,
    EvaluationLimitPolicy as LimitPolicy, EvaluationOutcome, EvaluationPanicKind as PanicKind,
    EvaluationPolicy, EvaluationReceipt, EvaluationRejectionKind as RejectKind, SourceRange,
};

pub(crate) const FUEL_LIMIT: u64 = 100_000;
const MEMORY_LIMIT: u64 = 1_048_576;
const COMPILATION_FUEL_LIMIT: u64 = 10_000_000;
const COMPILATION_MEMORY_LIMIT: u64 = 8_388_608;
const CALL_DEPTH_LIMIT: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
enum EvalFailure {
    Creator(RejectKind),
    Panic(PanicKind, SourceRange),
    Limit {
        policy: LimitPolicy,
        ceiling: u64,
        used: u64,
    },
    Cancelled,
    Defect(Arc<str>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Unavailable,
    Unit,
    Bool(bool),
    Integer {
        kind: IntegerType,
        value: i128,
    },
    Float {
        kind: FloatType,
        bits: u64,
    },
    Text(Arc<str>),
    Scalar(char),
    Bytes(Arc<[u8]>),
    Function(SpecializationId),
    Closure {
        id: ClosureId,
        captures: Arc<[(LocalId, Value)]>,
    },
    Array(Arc<[Value]>),
    Tuple(Arc<[Value]>),
    BuiltinVariant {
        variant: BuiltinVariant,
        payload: Arc<[Value]>,
    },
    UserVariant {
        id: VariantId,
        variant_order: u32,
        type_display: Arc<str>,
        variant_display: Arc<str>,
        payload: Arc<[Value]>,
    },
    Struct {
        definition: DefinitionId,
        type_display: Arc<str>,
        fields: Arc<[(Arc<str>, Value)]>,
    },
    TestApplication {
        id: TestId,
        payload: Arc<[Value]>,
    },
    SymbolicHandle {
        kind: BuildKind,
        identity: u128,
    },
}

#[derive(Clone)]
struct CachedConstant {
    value: Value,
    fuel: u64,
    peak_memory: u64,
    dependencies: Arc<[u128]>,
}

enum RootWork<'hir> {
    Constant(DefinitionId),
    Function(DefinitionId),
    Expression(&'hir Expression),
}

enum FrameKind<'hir> {
    Root,
    Function(&'hir HirFunction),
    Closure(&'hir HirClosure),
    Constant {
        id: DefinitionId,
        type_: &'hir Type,
        fuel_before: u64,
        peak_before: u64,
        dependencies_before: BTreeSet<u128>,
    },
}

struct MachineFrame<'hir> {
    kind: FrameKind<'hir>,
    controls: Vec<Control<'hir>>,
    values: Vec<Value>,
    locals: Vec<Option<Value>>,
    writebacks: Vec<(LocalId, Place)>,
}

enum Control<'hir> {
    Expression(&'hir Expression),
    Constant(DefinitionId),
    FinishRoot,
    FinishConstant,
    FunctionFallthrough,
    FinishClosure {
        return_type: &'hir Type,
    },
    Block {
        function: &'hir HirFunction,
        statements: &'hir [Statement],
        index: usize,
    },
    EndScope {
        deferred: Vec<&'hir Expression>,
    },
    Statement {
        function: &'hir HirFunction,
        statement: &'hir Statement,
    },
    FinishReturn {
        return_type: &'hir Type,
    },
    FinishUnitReturn,
    FinishPanic {
        site: &'hir SourceRange,
    },
    FinishAssert {
        site: &'hir SourceRange,
    },
    Discard,
    Store {
        local: LocalId,
        initialize: bool,
    },
    StorePlace {
        place: &'hir Place,
        index_count: usize,
    },
    FinishReadPlace {
        place: &'hir Place,
        index_count: usize,
        access: crate::typed_hir::AccessMode,
    },
    SelectBranch {
        function: &'hir HirFunction,
        then_branch: &'hir [Statement],
        else_branch: &'hir [Statement],
    },
    SelectPatternBranch {
        function: &'hir HirFunction,
        pattern: &'hir HirMatchPattern,
        then_branch: &'hir [Statement],
        else_branch: &'hir [Statement],
    },
    FinishFor {
        function: &'hir HirFunction,
        pattern: &'hir HirMatchPattern,
        body: &'hir [Statement],
    },
    FinishMatch {
        function: &'hir HirFunction,
        cases: &'hir [HirMatchCase],
    },
    TryMatch {
        function: &'hir HirFunction,
        cases: &'hir [HirMatchCase],
        value: Value,
        index: usize,
    },
    FinishMatchGuard {
        function: &'hir HirFunction,
        cases: &'hir [HirMatchCase],
        value: Value,
        next_index: usize,
        body: &'hir [Statement],
        bindings: Vec<LocalId>,
    },
    ClearLocals(Vec<LocalId>),
    ForNext {
        function: &'hir HirFunction,
        pattern: &'hir HirMatchPattern,
        body: &'hir [Statement],
        values: Arc<[Value]>,
        index: usize,
    },
    WhileNext {
        function: &'hir HirFunction,
        condition: &'hir Expression,
        body: &'hir [Statement],
        remaining: u64,
    },
    FinishWhileCondition {
        function: &'hir HirFunction,
        condition: &'hir Expression,
        body: &'hir [Statement],
        remaining: u64,
    },
    FinishArray {
        count: usize,
    },
    FinishRepeatedArray {
        count: u64,
    },
    FinishTuple {
        count: usize,
    },
    FinishIndex {
        site: &'hir SourceRange,
    },
    FinishNegate {
        site: &'hir SourceRange,
    },
    FinishBitNot,
    FinishNot,
    FinishShortCircuit {
        operator: BinaryOperator,
        right: &'hir Expression,
    },
    FinishPropagate,
    FinishIs {
        pattern: &'hir HirMatchPattern,
    },
    FinishBinary {
        operator: BinaryOperator,
        site: &'hir SourceRange,
    },
    FinishCall {
        target: &'hir CallTarget,
        arguments: &'hir [Expression],
        site: &'hir SourceRange,
    },
}

impl Control<'_> {
    fn is_loop_marker(&self) -> bool {
        matches!(self, Self::ForNext { .. } | Self::WhileNext { .. })
    }
}

fn deferred_expressions<'hir>(controls: &[Control<'hir>], start: usize) -> Vec<&'hir Expression> {
    controls[start..]
        .iter()
        .rev()
        .filter_map(|control| match control {
            Control::EndScope { deferred } => Some(deferred.iter().rev()),
            _ => None,
        })
        .flatten()
        .copied()
        .collect()
}

fn exited_locals(controls: &[Control<'_>], start: usize) -> Vec<LocalId> {
    controls[start..]
        .iter()
        .rev()
        .filter_map(|control| match control {
            Control::ClearLocals(locals) => Some(locals.iter()),
            _ => None,
        })
        .flatten()
        .copied()
        .collect()
}

fn push_deferred_controls<'hir>(
    controls: &mut Vec<Control<'hir>>,
    deferred: Vec<&'hir Expression>,
) {
    for expression in deferred.into_iter().rev() {
        controls.push(Control::Discard);
        controls.push(Control::Expression(expression));
    }
}

pub(crate) struct Engine<'a> {
    program: &'a VerifiedProgram,
    cancellation: &'a Cancellation,
    constant_values: BTreeMap<DefinitionId, CachedConstant>,
    evaluating_constants: Vec<DefinitionId>,
    fuel: u64,
    peak_memory: u64,
    current_memory: u64,
    compilation_fuel: u64,
    compilation_memory: u64,
    constructions: Vec<Construction>,
    construction_keys: BTreeMap<u128, Arc<[u8]>>,
    construction_coordinates: BTreeMap<Arc<[u8]>, u64>,
    test_applications: Vec<AppliedTest>,
    call_stack: Vec<(u128, String, SourceRange)>,
    evaluation_policy: EvaluationPolicy,
    evaluation_root: u128,
    evaluation_provenance: Option<SourceRange>,
    root_dependencies: BTreeSet<u128>,
    fuel_by_site: BTreeMap<SourceRange, u64>,
}

pub(crate) struct Run {
    pub(crate) outcome: EvaluationOutcome,
    pub(crate) receipt: EvaluationReceipt,
    pub(crate) constructions: Vec<Construction>,
    pub(crate) test_applications: Vec<AppliedTest>,
    pub(crate) root_handle: Option<(BuildKind, u128)>,
}

#[derive(Clone, Debug)]
pub(crate) struct Construction {
    pub(crate) identity: u128,
    pub(crate) kind: BuildKind,
    pub(crate) site: SourceRange,
    pub(crate) edges: Vec<u128>,
    pub(crate) operands: Vec<ConstructionOperand>,
}

#[derive(Clone, Debug)]
pub(crate) struct ConstructionOperand {
    pub(crate) label: Arc<str>,
    pub(crate) value: CanonicalValue,
}

#[derive(Clone, Debug)]
pub(crate) struct AppliedTest {
    pub(crate) id: TestId,
    pub(crate) payload: Vec<CanonicalValue>,
}

impl<'a> Engine<'a> {
    pub(crate) fn new(program: &'a VerifiedProgram, cancellation: &'a Cancellation) -> Self {
        Self {
            program,
            cancellation,
            constant_values: BTreeMap::new(),
            evaluating_constants: Vec::new(),
            fuel: 0,
            peak_memory: 0,
            current_memory: 0,
            compilation_fuel: 0,
            compilation_memory: 0,
            constructions: Vec::new(),
            construction_keys: BTreeMap::new(),
            construction_coordinates: BTreeMap::new(),
            test_applications: Vec::new(),
            call_stack: Vec::new(),
            evaluation_policy: EvaluationPolicy::Constant,
            evaluation_root: 0,
            evaluation_provenance: None,
            root_dependencies: BTreeSet::new(),
            fuel_by_site: BTreeMap::new(),
        }
    }

    pub(crate) fn evaluate_constant(&mut self, id: DefinitionId) -> Run {
        self.start_root();
        let result = self.run_machine(RootWork::Constant(id));
        self.finish(result)
    }

    pub(crate) fn evaluate_function(&mut self, id: DefinitionId) -> Run {
        self.start_root();
        let result = self.run_machine(RootWork::Function(id));
        self.finish(result)
    }

    pub(crate) fn evaluate_expression(&mut self, expression: &Expression) -> Run {
        self.start_root();
        let result = self.run_machine(RootWork::Expression(expression));
        self.finish(result)
    }

    fn start_root(&mut self) {
        self.fuel = 0;
        self.peak_memory = 0;
        self.current_memory = 0;
        self.constructions.clear();
        self.construction_keys.clear();
        self.construction_coordinates.clear();
        self.test_applications.clear();
        self.call_stack.clear();
        self.evaluating_constants.clear();
        self.root_dependencies.clear();
        self.evaluation_provenance = None;
        self.fuel_by_site.clear();
    }

    fn finish(&mut self, result: Result<Value, EvalFailure>) -> Run {
        let failed = result.is_err();
        let provenance = match &result {
            Err(EvalFailure::Panic(_, site)) => Some(site.clone()),
            Err(_) => self
                .call_stack
                .last()
                .map(|(_, _, site)| site.clone())
                .or_else(|| self.evaluation_provenance.clone()),
            Ok(_) => None,
        };
        let relevant_identity = failed.then(|| {
            self.call_stack
                .last()
                .map_or(self.evaluation_root, |(identity, _, _)| *identity)
        });
        let call_chain = if failed {
            self.call_stack
                .iter()
                .take(32)
                .map(|(identity, callable, site)| {
                    EvaluationFrameObservation::new(*identity, callable.as_str(), site.clone())
                })
                .collect()
        } else {
            Vec::new()
        };
        let mut contributors = self
            .fuel_by_site
            .iter()
            .map(|(site, fuel)| EvaluationContributorObservation::new(site.clone(), *fuel))
            .collect::<Vec<_>>();
        contributors.sort_by(|left, right| {
            right
                .fuel()
                .cmp(&left.fuel())
                .then(left.site().cmp(right.site()))
        });
        contributors.truncate(8);
        let root_handle = match &result {
            Ok(Value::SymbolicHandle { kind, identity }) => Some((*kind, *identity)),
            _ => None,
        };
        Run {
            outcome: match result {
                Ok(value) => EvaluationOutcome::Completed(canonical(value)),
                Err(EvalFailure::Creator(kind)) => EvaluationOutcome::CreatorRejected { kind },
                Err(EvalFailure::Panic(kind, site)) => EvaluationOutcome::Panicked { kind, site },
                Err(EvalFailure::Limit {
                    policy,
                    ceiling,
                    used,
                }) => EvaluationOutcome::LimitExceeded {
                    policy,
                    ceiling,
                    used,
                },
                Err(EvalFailure::Cancelled) => EvaluationOutcome::Cancelled,
                Err(EvalFailure::Defect(evidence)) => EvaluationOutcome::Defect { evidence },
            },
            receipt: EvaluationReceipt::new(
                self.evaluation_policy,
                self.evaluation_root,
                self.root_dependencies.iter().copied().collect(),
                self.program.fingerprint(),
                self.fuel,
                self.peak_memory,
            )
            .with_failure_evidence(
                provenance,
                relevant_identity,
                call_chain,
                if failed { contributors } else { Vec::new() },
            ),
            constructions: std::mem::take(&mut self.constructions),
            test_applications: std::mem::take(&mut self.test_applications),
            root_handle,
        }
    }

    fn charge(&mut self, amount: u64) -> Result<(), EvalFailure> {
        if self.cancellation.is_cancelled() {
            return Err(EvalFailure::Cancelled);
        }
        self.fuel = self.fuel.saturating_add(amount);
        if let Some(site) = self
            .call_stack
            .last()
            .map(|(_, _, site)| site)
            .or(self.evaluation_provenance.as_ref())
        {
            let fuel = self.fuel_by_site.entry(site.clone()).or_insert(0);
            *fuel = fuel.saturating_add(amount);
        }
        self.compilation_fuel = self.compilation_fuel.saturating_add(amount);
        if self.compilation_fuel > COMPILATION_FUEL_LIMIT {
            return Err(EvalFailure::Limit {
                policy: LimitPolicy::CompilationFuel,
                ceiling: COMPILATION_FUEL_LIMIT,
                used: self.compilation_fuel,
            });
        }
        if self.fuel > FUEL_LIMIT {
            return Err(EvalFailure::Limit {
                policy: LimitPolicy::RootFuel,
                ceiling: FUEL_LIMIT,
                used: self.fuel,
            });
        }
        Ok(())
    }

    fn retain(&mut self, amount: u64) -> Result<(), EvalFailure> {
        self.current_memory = self.current_memory.saturating_add(amount);
        self.peak_memory = self.peak_memory.max(self.current_memory);
        if self.peak_memory > MEMORY_LIMIT {
            return Err(EvalFailure::Limit {
                policy: LimitPolicy::RootMemory,
                ceiling: MEMORY_LIMIT,
                used: self.peak_memory,
            });
        }
        Ok(())
    }

    fn release(&mut self, amount: u64) -> Result<(), EvalFailure> {
        self.current_memory = self.current_memory.checked_sub(amount).ok_or_else(|| {
            EvalFailure::Defect(Arc::from("evaluator retained-memory accounting underflow"))
        })?;
        Ok(())
    }

    fn observe_temporary(&mut self, amount: u64) -> Result<(), EvalFailure> {
        let observed = self.current_memory.saturating_add(amount);
        self.peak_memory = self.peak_memory.max(observed);
        if observed > MEMORY_LIMIT {
            return Err(EvalFailure::Limit {
                policy: LimitPolicy::RootMemory,
                ceiling: MEMORY_LIMIT,
                used: observed,
            });
        }
        Ok(())
    }

    fn run_machine<'hir>(&mut self, work: RootWork<'hir>) -> Result<Value, EvalFailure>
    where
        'a: 'hir,
    {
        let program = self.program;
        let mut frames = Vec::<MachineFrame<'hir>>::new();
        match work {
            RootWork::Constant(id) => {
                self.evaluation_policy = EvaluationPolicy::Constant;
                self.evaluation_root = id.0;
                self.evaluation_provenance = program
                    .constants()
                    .get(&id)
                    .map(|value| value.source.clone());
                self.retain(64)?;
                frames.push(MachineFrame {
                    kind: FrameKind::Root,
                    controls: vec![Control::FinishRoot, Control::Constant(id)],
                    values: Vec::new(),
                    locals: Vec::new(),
                    writebacks: Vec::new(),
                });
            }
            RootWork::Expression(expression) => {
                self.evaluation_policy = EvaluationPolicy::ComptimeAssertion;
                let mut key = b"wrela.comptime-root\0\x01".to_vec();
                key.extend_from_slice(expression.source.path().as_bytes());
                key.extend_from_slice(&expression.source.start().to_be_bytes());
                key.extend_from_slice(&expression.source.end().to_be_bytes());
                self.evaluation_root = xxh3_128(&key);
                self.evaluation_provenance = Some(expression.source.clone());
                self.retain(64)?;
                frames.push(MachineFrame {
                    kind: FrameKind::Root,
                    controls: vec![Control::FinishRoot, Control::Expression(expression)],
                    values: Vec::new(),
                    locals: Vec::new(),
                    writebacks: Vec::new(),
                });
            }
            RootWork::Function(id) => {
                let specialization = program
                    .default_specialization(id)
                    .ok_or(EvalFailure::Creator(RejectKind::UnresolvedCall))?;
                let function = program
                    .specialization_function(specialization)
                    .ok_or(EvalFailure::Creator(RejectKind::UnresolvedCall))?;
                self.evaluation_policy = EvaluationPolicy::ImageConstructor;
                self.evaluation_root = specialization.0;
                let site = program
                    .functions()
                    .get(&id)
                    .map(|function| &function.source)
                    .ok_or(EvalFailure::Creator(RejectKind::UnresolvedCall))?;
                self.evaluation_provenance = Some(site.clone());
                self.push_function_frame(&mut frames, function, Vec::new(), Vec::new(), site)?;
            }
        }

        loop {
            let control = frames
                .last_mut()
                .and_then(|frame| frame.controls.pop())
                .ok_or_else(|| {
                    EvalFailure::Defect(Arc::from("evaluator frame exhausted without a result"))
                })?;
            match control {
                Control::Expression(expression) => {
                    self.charge(1)?;
                    match &expression.kind {
                        ExpressionKind::Literal(literal) => {
                            let value = match literal {
                                Literal::Unit => Value::Unit,
                                Literal::Bool(value) => Value::Bool(*value),
                                Literal::Integer { kind, value } => Value::Integer {
                                    kind: *kind,
                                    value: *value,
                                },
                                Literal::Float { kind, bits } => Value::Float {
                                    kind: *kind,
                                    bits: *bits,
                                },
                                Literal::Text(value) => Value::Text(value.clone()),
                                Literal::Scalar(value) => Value::Scalar(*value),
                                Literal::Bytes(value) => Value::Bytes(value.clone()),
                            };
                            self.push_value(&mut frames, value)?;
                        }
                        ExpressionKind::Read(place) => {
                            if place.projections.is_empty() {
                                let value = if expression.access
                                    == crate::typed_hir::AccessMode::Move
                                {
                                    let root = frames
                                        .last_mut()
                                        .and_then(|frame| {
                                            frame.locals.get_mut(place.local.0 as usize)
                                        })
                                        .and_then(Option::as_mut)
                                        .ok_or(EvalFailure::Creator(RejectKind::MissingLocal))?;
                                    let unavailable = unavailable_shape(root);
                                    std::mem::replace(root, unavailable)
                                } else {
                                    frames
                                        .last()
                                        .and_then(|frame| frame.locals.get(place.local.0 as usize))
                                        .and_then(Option::as_ref)
                                        .cloned()
                                        .ok_or(EvalFailure::Creator(RejectKind::MissingLocal))?
                                };
                                self.push_value(&mut frames, value)?;
                            } else {
                                let indexes = place
                                    .projections
                                    .iter()
                                    .filter_map(|projection| match projection {
                                        PlaceProjection::Index { index, .. } => {
                                            Some(index.as_ref())
                                        }
                                        PlaceProjection::Field { .. } => None,
                                    })
                                    .collect::<Vec<_>>();
                                let controls = &mut frames
                                    .last_mut()
                                    .expect("machine has current frame")
                                    .controls;
                                controls.push(Control::FinishReadPlace {
                                    place,
                                    index_count: indexes.len(),
                                    access: expression.access,
                                });
                                controls.extend(indexes.into_iter().rev().map(Control::Expression));
                            }
                        }
                        ExpressionKind::Constant(id) => frames
                            .last_mut()
                            .expect("machine has current frame")
                            .controls
                            .push(Control::Constant(*id)),
                        ExpressionKind::FunctionValue { specialization, .. } => {
                            let specialization = specialization.ok_or_else(|| {
                                EvalFailure::Defect(Arc::from(
                                    "template function value reached the concrete evaluator",
                                ))
                            })?;
                            self.push_value(&mut frames, Value::Function(specialization))?;
                        }
                        ExpressionKind::Closure(closure) => {
                            let captures = closure
                                .captures
                                .iter()
                                .map(|(local, _)| {
                                    frames
                                        .last()
                                        .and_then(|frame| {
                                            frame
                                                .locals
                                                .get(local.0 as usize)
                                                .and_then(Option::as_ref)
                                        })
                                        .cloned()
                                        .map(|value| (*local, value))
                                        .ok_or(EvalFailure::Creator(RejectKind::MissingLocal))
                                })
                                .collect::<Result<Vec<_>, _>>()?;
                            self.push_value(
                                &mut frames,
                                Value::Closure {
                                    id: closure.id,
                                    captures: captures.into(),
                                },
                            )?;
                        }
                        ExpressionKind::Array(values) => {
                            let controls = &mut frames
                                .last_mut()
                                .expect("machine has current frame")
                                .controls;
                            controls.push(Control::FinishArray {
                                count: values.len(),
                            });
                            controls.extend(values.iter().rev().map(Control::Expression));
                        }
                        ExpressionKind::RepeatedArray { value, length } => {
                            let controls = &mut frames
                                .last_mut()
                                .expect("machine has current frame")
                                .controls;
                            controls.push(Control::FinishRepeatedArray { count: *length });
                            controls.push(Control::Expression(value));
                        }
                        ExpressionKind::Tuple(values) => {
                            let controls = &mut frames
                                .last_mut()
                                .expect("machine has current frame")
                                .controls;
                            controls.push(Control::FinishTuple {
                                count: values.len(),
                            });
                            controls.extend(values.iter().rev().map(Control::Expression));
                        }
                        ExpressionKind::Index { value, index } => {
                            let controls = &mut frames
                                .last_mut()
                                .expect("machine has current frame")
                                .controls;
                            controls.push(Control::FinishIndex {
                                site: &expression.source,
                            });
                            controls.push(Control::Expression(index));
                            controls.push(Control::Expression(value));
                        }
                        ExpressionKind::Positive(value) => frames
                            .last_mut()
                            .expect("machine has current frame")
                            .controls
                            .push(Control::Expression(value)),
                        ExpressionKind::Negate(value) => {
                            let controls = &mut frames
                                .last_mut()
                                .expect("machine has current frame")
                                .controls;
                            controls.push(Control::FinishNegate {
                                site: &expression.source,
                            });
                            controls.push(Control::Expression(value));
                        }
                        ExpressionKind::BitNot(value) => {
                            let controls = &mut frames
                                .last_mut()
                                .expect("machine has current frame")
                                .controls;
                            controls.push(Control::FinishBitNot);
                            controls.push(Control::Expression(value));
                        }
                        ExpressionKind::Not(value) => {
                            let controls = &mut frames
                                .last_mut()
                                .expect("machine has current frame")
                                .controls;
                            controls.push(Control::FinishNot);
                            controls.push(Control::Expression(value));
                        }
                        ExpressionKind::Await(_) => {
                            return Err(EvalFailure::Creator(
                                RejectKind::AwaitNotEvaluatorEligible,
                            ));
                        }
                        ExpressionKind::Propagate(value) => {
                            let controls = &mut frames
                                .last_mut()
                                .expect("machine has current frame")
                                .controls;
                            controls.push(Control::FinishPropagate);
                            controls.push(Control::Expression(value));
                        }
                        ExpressionKind::Is { value, pattern } => {
                            let controls = &mut frames
                                .last_mut()
                                .expect("machine has current frame")
                                .controls;
                            controls.push(Control::FinishIs { pattern });
                            controls.push(Control::Expression(value));
                        }
                        ExpressionKind::Binary {
                            operator,
                            left,
                            right,
                        } => {
                            let controls = &mut frames
                                .last_mut()
                                .expect("machine has current frame")
                                .controls;
                            if matches!(operator, BinaryOperator::And | BinaryOperator::Or) {
                                controls.push(Control::FinishShortCircuit {
                                    operator: *operator,
                                    right,
                                });
                            } else {
                                controls.push(Control::FinishBinary {
                                    operator: *operator,
                                    site: &expression.source,
                                });
                                controls.push(Control::Expression(right));
                            }
                            controls.push(Control::Expression(left));
                        }
                        ExpressionKind::Call { target, arguments } => {
                            let controls = &mut frames
                                .last_mut()
                                .expect("machine has current frame")
                                .controls;
                            controls.push(Control::FinishCall {
                                target,
                                arguments,
                                site: &expression.source,
                            });
                            controls.extend(arguments.iter().rev().map(Control::Expression));
                            if let CallTarget::Callable { value } = target {
                                controls.push(Control::Expression(value));
                            }
                        }
                    }
                }
                Control::Constant(id) => {
                    let dependencies_before = self.root_dependencies.clone();
                    self.root_dependencies.insert(id.0);
                    if let Some(cached) = self.constant_values.get(&id).cloned() {
                        self.root_dependencies
                            .extend(cached.dependencies.iter().copied());
                        self.charge(cached.fuel)?;
                        self.observe_temporary(cached.peak_memory)?;
                        self.push_value(&mut frames, cached.value)?;
                        continue;
                    }
                    if self.evaluating_constants.contains(&id) {
                        return Err(EvalFailure::Creator(RejectKind::ConstantDependencyCycle));
                    }
                    let constant = program
                        .constants()
                        .get(&id)
                        .ok_or(EvalFailure::Creator(RejectKind::UnresolvedConstant))?;
                    self.evaluating_constants.push(id);
                    self.retain(64)?;
                    frames.push(MachineFrame {
                        kind: FrameKind::Constant {
                            id,
                            type_: &constant.type_,
                            fuel_before: self.fuel,
                            peak_before: self.peak_memory,
                            dependencies_before,
                        },
                        controls: vec![
                            Control::FinishConstant,
                            Control::Expression(&constant.expression),
                        ],
                        values: Vec::new(),
                        locals: Vec::new(),
                        writebacks: Vec::new(),
                    });
                }
                Control::FinishRoot | Control::FinishConstant => {
                    let value = self.pop_value(&mut frames)?;
                    if let Some(result) = self.complete_frame(&mut frames, value)? {
                        return Ok(result);
                    }
                }
                Control::FunctionFallthrough => {
                    let function = match &frames.last().expect("machine has frame").kind {
                        FrameKind::Function(function) => *function,
                        _ => {
                            return Err(EvalFailure::Defect(Arc::from(
                                "function fallthrough appeared outside a function frame",
                            )));
                        }
                    };
                    if function.return_type != Type::Unit {
                        return Err(EvalFailure::Defect(Arc::from(
                            "verified non-Unit function fell through",
                        )));
                    }
                    if let Some(result) = self.complete_frame(&mut frames, Value::Unit)? {
                        return Ok(result);
                    }
                }
                Control::FinishClosure { return_type } => {
                    let value = self.pop_value(&mut frames)?;
                    let value = coerce(value, return_type)
                        .ok_or(EvalFailure::Creator(RejectKind::ReturnTypeMismatch))?;
                    if let Some(result) = self.complete_frame(&mut frames, value)? {
                        return Ok(result);
                    }
                }
                Control::Block {
                    function,
                    statements,
                    index,
                } => {
                    if index == 0 {
                        frames
                            .last_mut()
                            .expect("machine has current frame")
                            .controls
                            .push(Control::EndScope {
                                deferred: Vec::new(),
                            });
                    }
                    if let Some(statement) = statements.get(index) {
                        let controls = &mut frames
                            .last_mut()
                            .expect("machine has current frame")
                            .controls;
                        controls.push(Control::Block {
                            function,
                            statements,
                            index: index + 1,
                        });
                        controls.push(Control::Statement {
                            function,
                            statement,
                        });
                    }
                }
                Control::Statement {
                    function,
                    statement,
                } => {
                    self.charge(1)?;
                    let controls = &mut frames
                        .last_mut()
                        .expect("machine has current frame")
                        .controls;
                    match statement {
                        Statement::Return {
                            value: Some(value), ..
                        } => {
                            let deferred = deferred_expressions(controls, 0);
                            controls.clear();
                            controls.push(Control::FinishReturn {
                                return_type: &function.return_type,
                            });
                            push_deferred_controls(controls, deferred);
                            controls.push(Control::Expression(value));
                        }
                        Statement::Return { value: None, .. } => {
                            let deferred = deferred_expressions(controls, 0);
                            controls.clear();
                            controls.push(Control::FinishUnitReturn);
                            push_deferred_controls(controls, deferred);
                        }
                        Statement::Panic { value, source } => {
                            controls.push(Control::FinishPanic { site: source });
                            controls.push(Control::Expression(value));
                        }
                        Statement::Assert { condition, source } => {
                            controls.push(Control::FinishAssert { site: source });
                            controls.push(Control::Expression(condition));
                        }
                        Statement::Expect { condition, .. } => {
                            controls.push(Control::Discard);
                            controls.push(Control::Expression(condition));
                        }
                        Statement::Initialize { place, value, .. } => {
                            controls.push(Control::Store {
                                local: place.local,
                                initialize: true,
                            });
                            controls.push(Control::Expression(value));
                        }
                        Statement::Assign { place, value, .. } => {
                            if place.projections.is_empty() {
                                controls.push(Control::Store {
                                    local: place.local,
                                    initialize: false,
                                });
                                controls.push(Control::Expression(value));
                            } else {
                                let indexes = place
                                    .projections
                                    .iter()
                                    .filter_map(|projection| match projection {
                                        PlaceProjection::Index { index, .. } => {
                                            Some(index.as_ref())
                                        }
                                        PlaceProjection::Field { .. } => None,
                                    })
                                    .collect::<Vec<_>>();
                                controls.push(Control::StorePlace {
                                    place,
                                    index_count: indexes.len(),
                                });
                                controls.push(Control::Expression(value));
                                controls.extend(indexes.into_iter().rev().map(Control::Expression));
                            }
                        }
                        Statement::Evaluate(expression) => {
                            controls.push(Control::Discard);
                            controls.push(Control::Expression(expression));
                        }
                        Statement::If {
                            condition,
                            then_branch,
                            else_branch,
                            ..
                        } => {
                            controls.push(Control::SelectBranch {
                                function,
                                then_branch,
                                else_branch,
                            });
                            controls.push(Control::Expression(condition));
                        }
                        Statement::IfPattern {
                            value,
                            pattern,
                            then_branch,
                            else_branch,
                            ..
                        } => {
                            controls.push(Control::SelectPatternBranch {
                                function,
                                pattern,
                                then_branch,
                                else_branch,
                            });
                            controls.push(Control::Expression(value));
                        }
                        Statement::For {
                            pattern,
                            iterable,
                            body,
                            ..
                        } => {
                            controls.push(Control::FinishFor {
                                function,
                                pattern,
                                body,
                            });
                            controls.push(Control::Expression(iterable));
                        }
                        Statement::While {
                            condition,
                            body,
                            max_iterations,
                            ..
                        } => controls.push(Control::WhileNext {
                            function,
                            condition,
                            body,
                            remaining: *max_iterations,
                        }),
                        Statement::Break(_) => {
                            let Some(index) = controls.iter().rposition(Control::is_loop_marker)
                            else {
                                return Err(EvalFailure::Defect(Arc::from(
                                    "verified break appeared outside a loop",
                                )));
                            };
                            let deferred = deferred_expressions(controls, index + 1);
                            let locals = exited_locals(controls, index + 1);
                            controls.truncate(index);
                            controls.push(Control::ClearLocals(locals));
                            push_deferred_controls(controls, deferred);
                        }
                        Statement::Continue(_) => {
                            let Some(index) = controls.iter().rposition(Control::is_loop_marker)
                            else {
                                return Err(EvalFailure::Defect(Arc::from(
                                    "verified continue appeared outside a loop",
                                )));
                            };
                            let deferred = deferred_expressions(controls, index + 1);
                            let locals = exited_locals(controls, index + 1);
                            controls.truncate(index + 1);
                            controls.push(Control::ClearLocals(locals));
                            push_deferred_controls(controls, deferred);
                        }
                        Statement::Match { value, cases, .. } => {
                            controls.push(Control::FinishMatch { function, cases });
                            controls.push(Control::Expression(value));
                        }
                        Statement::Defer { expression, .. } => {
                            let Some(Control::EndScope { deferred }) = controls
                                .iter_mut()
                                .rev()
                                .find(|control| matches!(control, Control::EndScope { .. }))
                            else {
                                return Err(EvalFailure::Defect(Arc::from(
                                    "verified defer appeared outside a lexical scope",
                                )));
                            };
                            deferred.push(expression);
                        }
                        Statement::WithPool {
                            binding,
                            scope,
                            body,
                            ..
                        } => {
                            controls.push(Control::ClearLocals(vec![binding.local]));
                            controls.push(Control::Block {
                                function,
                                statements: body,
                                index: 0,
                            });
                            controls.push(Control::Store {
                                local: binding.local,
                                initialize: true,
                            });
                            controls.push(Control::Expression(scope));
                        }
                        Statement::Pass(_) => {}
                    }
                }
                Control::EndScope { deferred } => {
                    let controls = &mut frames
                        .last_mut()
                        .expect("machine has current frame")
                        .controls;
                    for expression in deferred {
                        controls.push(Control::Discard);
                        controls.push(Control::Expression(expression));
                    }
                }
                Control::FinishReturn { return_type } => {
                    let value = self.pop_value(&mut frames)?;
                    let value = coerce(value, return_type)
                        .ok_or(EvalFailure::Creator(RejectKind::ReturnTypeMismatch))?;
                    if let Some(result) = self.complete_frame(&mut frames, value)? {
                        return Ok(result);
                    }
                }
                Control::FinishUnitReturn => {
                    if let Some(result) = self.complete_frame(&mut frames, Value::Unit)? {
                        return Ok(result);
                    }
                }
                Control::FinishPanic { site } => {
                    let _ = self.pop_value(&mut frames)?;
                    return Err(EvalFailure::Panic(PanicKind::Explicit, site.clone()));
                }
                Control::FinishAssert { site } => match self.pop_value(&mut frames)? {
                    Value::Bool(true) => {}
                    Value::Bool(false) => {
                        return Err(EvalFailure::Panic(PanicKind::AssertionFailed, site.clone()));
                    }
                    _ => {
                        return Err(EvalFailure::Defect(Arc::from(
                            "verified assertion condition is not Bool",
                        )));
                    }
                },
                Control::Discard => {
                    let _ = self.pop_value(&mut frames)?;
                }
                Control::Store { local, initialize } => {
                    let value = self.pop_value(&mut frames)?;
                    let index = local.0 as usize;
                    let previous = {
                        let frame = frames.last_mut().expect("machine has current frame");
                        if frame.locals.len() <= index {
                            frame.locals.resize(index + 1, None);
                        }
                        frame.locals[index].take()
                    };
                    if initialize && previous.is_some() {
                        return Err(EvalFailure::Defect(Arc::from(
                            "verified initializer targets an initialized local",
                        )));
                    }
                    if !initialize && previous.is_none() {
                        return Err(EvalFailure::Defect(Arc::from(
                            "verified assignment targets no local",
                        )));
                    }
                    if let Some(previous) = &previous {
                        self.release(value_size(previous))?;
                    }
                    self.retain(value_size(&value))?;
                    frames.last_mut().expect("machine has frame").locals[index] = Some(value);
                }
                Control::FinishReadPlace {
                    place,
                    index_count,
                    access,
                } => {
                    let indexes = self.pop_values(&mut frames, index_count)?;
                    let value = if access == crate::typed_hir::AccessMode::Move {
                        let root = frames
                            .last_mut()
                            .and_then(|frame| frame.locals.get_mut(place.local.0 as usize))
                            .and_then(Option::as_mut)
                            .ok_or(EvalFailure::Creator(RejectKind::MissingLocal))?;
                        extract_projected_value(root, &place.projections, &indexes)?
                    } else {
                        let root = frames
                            .last()
                            .and_then(|frame| frame.locals.get(place.local.0 as usize))
                            .and_then(Option::as_ref)
                            .ok_or(EvalFailure::Creator(RejectKind::MissingLocal))?;
                        read_projected_value(root, &place.projections, &indexes)?
                    };
                    self.push_value(&mut frames, value)?;
                }
                Control::StorePlace { place, index_count } => {
                    let value = self.pop_value(&mut frames)?;
                    let indexes = self.pop_values(&mut frames, index_count)?;
                    let slot = place.local.0 as usize;
                    let mut root = frames
                        .last_mut()
                        .and_then(|frame| frame.locals.get_mut(slot))
                        .and_then(Option::take)
                        .ok_or(EvalFailure::Creator(RejectKind::MissingLocal))?;
                    let previous_size = value_size(&root);
                    store_projected_value(&mut root, &place.projections, &indexes, value)?;
                    let next_size = value_size(&root);
                    self.release(previous_size)?;
                    self.retain(next_size)?;
                    frames.last_mut().expect("machine has frame").locals[slot] = Some(root);
                }
                Control::SelectBranch {
                    function,
                    then_branch,
                    else_branch,
                } => {
                    let condition = self.pop_value(&mut frames)?;
                    let branch = match condition {
                        Value::Bool(true) => then_branch,
                        Value::Bool(false) => else_branch,
                        _ => {
                            return Err(EvalFailure::Defect(Arc::from(
                                "verified non-bool condition",
                            )));
                        }
                    };
                    frames
                        .last_mut()
                        .expect("machine has current frame")
                        .controls
                        .push(Control::Block {
                            function,
                            statements: branch,
                            index: 0,
                        });
                }
                Control::SelectPatternBranch {
                    function,
                    pattern,
                    then_branch,
                    else_branch,
                } => {
                    let value = self.pop_value(&mut frames)?;
                    let Some(bindings) = pattern_bindings(&value, pattern) else {
                        frames
                            .last_mut()
                            .expect("machine has current frame")
                            .controls
                            .push(Control::Block {
                                function,
                                statements: else_branch,
                                index: 0,
                            });
                        continue;
                    };
                    let binding_locals =
                        bindings.iter().map(|(local, _)| *local).collect::<Vec<_>>();
                    for (local, binding) in bindings {
                        let slot = local.0 as usize;
                        self.retain(value_size(&binding))?;
                        let frame = frames.last_mut().expect("machine has current frame");
                        if frame.locals.len() <= slot {
                            frame.locals.resize(slot + 1, None);
                        }
                        if frame.locals[slot].replace(binding).is_some() {
                            return Err(EvalFailure::Defect(Arc::from(
                                "verified is pattern reused a live LocalId",
                            )));
                        }
                    }
                    let controls = &mut frames
                        .last_mut()
                        .expect("machine has current frame")
                        .controls;
                    controls.push(Control::ClearLocals(binding_locals));
                    controls.push(Control::Block {
                        function,
                        statements: then_branch,
                        index: 0,
                    });
                }
                Control::FinishFor {
                    function,
                    pattern,
                    body,
                } => {
                    let Value::Array(values) = self.pop_value(&mut frames)? else {
                        return Err(EvalFailure::Defect(Arc::from(
                            "verified for iterable is not an array",
                        )));
                    };
                    frames
                        .last_mut()
                        .expect("machine has current frame")
                        .controls
                        .push(Control::ForNext {
                            function,
                            pattern,
                            body,
                            values,
                            index: 0,
                        });
                }
                Control::ForNext {
                    function,
                    pattern,
                    body,
                    values,
                    index,
                } => {
                    let Some(value) = values.get(index).cloned() else {
                        continue;
                    };
                    let bindings = pattern_bindings(&value, pattern).ok_or_else(|| {
                        EvalFailure::Defect(Arc::from(
                            "verified irrefutable for pattern did not match",
                        ))
                    })?;
                    let binding_locals =
                        bindings.iter().map(|(local, _)| *local).collect::<Vec<_>>();
                    for (local, binding) in bindings {
                        let slot = local.0 as usize;
                        self.retain(value_size(&binding))?;
                        let frame = frames.last_mut().expect("machine has current frame");
                        if frame.locals.len() <= slot {
                            frame.locals.resize(slot + 1, None);
                        }
                        if frame.locals[slot].replace(binding).is_some() {
                            return Err(EvalFailure::Defect(Arc::from(
                                "verified for pattern reused a live LocalId",
                            )));
                        }
                    }
                    let controls = &mut frames
                        .last_mut()
                        .expect("machine has current frame")
                        .controls;
                    controls.push(Control::ForNext {
                        function,
                        pattern,
                        body,
                        values,
                        index: index + 1,
                    });
                    controls.push(Control::ClearLocals(binding_locals));
                    controls.push(Control::Block {
                        function,
                        statements: body,
                        index: 0,
                    });
                }
                Control::WhileNext {
                    function,
                    condition,
                    body,
                    remaining,
                } => {
                    let controls = &mut frames
                        .last_mut()
                        .expect("machine has current frame")
                        .controls;
                    controls.push(Control::FinishWhileCondition {
                        function,
                        condition,
                        body,
                        remaining,
                    });
                    controls.push(Control::Expression(condition));
                }
                Control::FinishWhileCondition {
                    function,
                    condition,
                    body,
                    remaining,
                } => match self.pop_value(&mut frames)? {
                    Value::Bool(false) => {}
                    Value::Bool(true) if remaining == 0 => {
                        return Err(EvalFailure::Defect(Arc::from(
                            "verified while exceeded its compiler-derived bound",
                        )));
                    }
                    Value::Bool(true) => {
                        let controls = &mut frames
                            .last_mut()
                            .expect("machine has current frame")
                            .controls;
                        controls.push(Control::WhileNext {
                            function,
                            condition,
                            body,
                            remaining: remaining - 1,
                        });
                        controls.push(Control::Block {
                            function,
                            statements: body,
                            index: 0,
                        });
                    }
                    _ => {
                        return Err(EvalFailure::Defect(Arc::from(
                            "verified while condition is not Bool",
                        )));
                    }
                },
                Control::FinishMatch { function, cases } => {
                    let value = self.pop_value(&mut frames)?;
                    frames
                        .last_mut()
                        .expect("machine has current frame")
                        .controls
                        .push(Control::TryMatch {
                            function,
                            cases,
                            value,
                            index: 0,
                        });
                }
                Control::TryMatch {
                    function,
                    cases,
                    value,
                    index,
                } => {
                    let Some(case) = cases.get(index) else {
                        return Err(EvalFailure::Defect(Arc::from(
                            "verified exhaustive match selected no case",
                        )));
                    };
                    let Some(bindings) = case.pattern.as_ref().map_or_else(
                        || Some(Vec::new()),
                        |pattern| pattern_bindings(&value, pattern),
                    ) else {
                        frames
                            .last_mut()
                            .expect("machine has current frame")
                            .controls
                            .push(Control::TryMatch {
                                function,
                                cases,
                                value,
                                index: index + 1,
                            });
                        continue;
                    };
                    let binding_locals =
                        bindings.iter().map(|(local, _)| *local).collect::<Vec<_>>();
                    for (local, binding) in bindings {
                        let slot = local.0 as usize;
                        self.retain(value_size(&binding))?;
                        let frame = frames.last_mut().expect("machine has current frame");
                        if frame.locals.len() <= slot {
                            frame.locals.resize(slot + 1, None);
                        }
                        if frame.locals[slot].replace(binding).is_some() {
                            return Err(EvalFailure::Defect(Arc::from(
                                "verified pattern binding reused a live LocalId",
                            )));
                        }
                    }
                    let controls = &mut frames
                        .last_mut()
                        .expect("machine has current frame")
                        .controls;
                    if let Some(guard) = &case.guard {
                        controls.push(Control::FinishMatchGuard {
                            function,
                            cases,
                            value,
                            next_index: index + 1,
                            body: &case.body,
                            bindings: binding_locals,
                        });
                        controls.push(Control::Expression(guard));
                    } else {
                        controls.push(Control::ClearLocals(binding_locals));
                        controls.push(Control::Block {
                            function,
                            statements: &case.body,
                            index: 0,
                        });
                    }
                }
                Control::FinishMatchGuard {
                    function,
                    cases,
                    value,
                    next_index,
                    body,
                    bindings,
                } => match self.pop_value(&mut frames)? {
                    Value::Bool(true) => {
                        let controls = &mut frames
                            .last_mut()
                            .expect("machine has current frame")
                            .controls;
                        controls.push(Control::ClearLocals(bindings));
                        controls.push(Control::Block {
                            function,
                            statements: body,
                            index: 0,
                        });
                    }
                    Value::Bool(false) => {
                        self.clear_locals(&mut frames, &bindings)?;
                        frames
                            .last_mut()
                            .expect("machine has current frame")
                            .controls
                            .push(Control::TryMatch {
                                function,
                                cases,
                                value,
                                index: next_index,
                            });
                    }
                    _ => {
                        return Err(EvalFailure::Defect(Arc::from(
                            "verified match guard is not Bool",
                        )));
                    }
                },
                Control::ClearLocals(locals) => self.clear_locals(&mut frames, &locals)?,
                Control::FinishArray { count } => {
                    let values = self.pop_values(&mut frames, count)?;
                    self.push_value(&mut frames, Value::Array(values.into()))?;
                }
                Control::FinishRepeatedArray { count } => {
                    let value = self.pop_value(&mut frames)?;
                    let retained = value_size(&value)
                        .checked_mul(count)
                        .and_then(|values| values.checked_add(16))
                        .unwrap_or(u64::MAX);
                    self.observe_temporary(retained)?;
                    let count = usize::try_from(count).map_err(|_| EvalFailure::Limit {
                        policy: LimitPolicy::RootMemory,
                        ceiling: MEMORY_LIMIT,
                        used: u64::MAX,
                    })?;
                    self.push_value(&mut frames, Value::Array(vec![value; count].into()))?;
                }
                Control::FinishTuple { count } => {
                    let values = self.pop_values(&mut frames, count)?;
                    self.push_value(&mut frames, Value::Tuple(values.into()))?;
                }
                Control::FinishIndex { site } => {
                    let Value::Integer { value: index, .. } = self.pop_value(&mut frames)? else {
                        return Err(EvalFailure::Defect(Arc::from(
                            "verified array index is not an integer",
                        )));
                    };
                    let Value::Array(values) = self.pop_value(&mut frames)? else {
                        return Err(EvalFailure::Defect(Arc::from(
                            "verified indexed value is not an array",
                        )));
                    };
                    let value = usize::try_from(index)
                        .ok()
                        .and_then(|index| values.get(index))
                        .cloned()
                        .ok_or_else(|| {
                            EvalFailure::Panic(PanicKind::IndexOutOfBounds, site.clone())
                        })?;
                    self.push_value(&mut frames, value)?;
                }
                Control::FinishNegate { site } => {
                    let value = match self.pop_value(&mut frames)? {
                        Value::Integer { kind, value } => {
                            let value = value.checked_neg().ok_or_else(|| {
                                EvalFailure::Panic(PanicKind::IntegerOverflow, site.clone())
                            })?;
                            if !kind.fits(value) {
                                return Err(EvalFailure::Panic(
                                    PanicKind::IntegerOverflow,
                                    site.clone(),
                                ));
                            }
                            Value::Integer { kind, value }
                        }
                        Value::Float { kind, bits } => Value::Float {
                            kind,
                            bits: encode_float(kind, -decode_float(kind, bits)),
                        },
                        _ => {
                            return Err(EvalFailure::Creator(RejectKind::InvalidUnaryOperand));
                        }
                    };
                    self.push_value(&mut frames, value)?;
                }
                Control::FinishBitNot => {
                    let Value::Integer { kind, value } = self.pop_value(&mut frames)? else {
                        return Err(EvalFailure::Defect(Arc::from(
                            "verified bitwise-not operand is not an integer",
                        )));
                    };
                    let value = if kind.is_signed() {
                        !value
                    } else {
                        let mask = (1_i128 << kind.bits()) - 1;
                        mask ^ value
                    };
                    self.push_value(&mut frames, Value::Integer { kind, value })?;
                }
                Control::FinishNot => {
                    let Value::Bool(value) = self.pop_value(&mut frames)? else {
                        return Err(EvalFailure::Defect(Arc::from(
                            "verified not operand is not Bool",
                        )));
                    };
                    self.push_value(&mut frames, Value::Bool(!value))?;
                }
                Control::FinishShortCircuit { operator, right } => {
                    let Value::Bool(left) = self.pop_value(&mut frames)? else {
                        return Err(EvalFailure::Defect(Arc::from(
                            "verified short-circuit operand is not Bool",
                        )));
                    };
                    let result = match (operator, left) {
                        (BinaryOperator::And, false) => Some(false),
                        (BinaryOperator::Or, true) => Some(true),
                        (BinaryOperator::And, true) | (BinaryOperator::Or, false) => None,
                        _ => {
                            return Err(EvalFailure::Defect(Arc::from(
                                "non-boolean operator reached short-circuit control",
                            )));
                        }
                    };
                    if let Some(result) = result {
                        self.push_value(&mut frames, Value::Bool(result))?;
                    } else {
                        frames
                            .last_mut()
                            .expect("machine has current frame")
                            .controls
                            .push(Control::Expression(right));
                    }
                }
                Control::FinishPropagate => {
                    let value = self.pop_value(&mut frames)?;
                    match value {
                        Value::BuiltinVariant {
                            variant: BuiltinVariant::ResultOk | BuiltinVariant::OptionSome,
                            payload,
                        } => {
                            let value = payload
                                .first()
                                .cloned()
                                .ok_or(EvalFailure::Creator(RejectKind::ResultOkMissingPayload))?;
                            self.push_value(&mut frames, value)?;
                        }
                        alternative @ Value::BuiltinVariant {
                            variant: BuiltinVariant::ResultErr | BuiltinVariant::OptionNone,
                            ..
                        } => {
                            let return_type = match frames.last().map(|frame| &frame.kind) {
                                Some(FrameKind::Function(function)) => &function.return_type,
                                Some(FrameKind::Closure(closure)) => &closure.return_type,
                                _ => {
                                    return Err(EvalFailure::Defect(Arc::from(
                                        "verified propagation escaped a callable frame",
                                    )));
                                }
                            };
                            let compatible = matches!(
                                (&alternative, return_type),
                                (
                                    Value::BuiltinVariant {
                                        variant: BuiltinVariant::ResultErr,
                                        ..
                                    },
                                    Type::Result { .. }
                                ) | (
                                    Value::BuiltinVariant {
                                        variant: BuiltinVariant::OptionNone,
                                        ..
                                    },
                                    Type::Option(_)
                                )
                            );
                            if !compatible {
                                return Err(EvalFailure::Defect(Arc::from(
                                    "verified propagation alternative does not match callable return",
                                )));
                            }
                            let return_already_scheduled = frames
                                .last()
                                .expect("machine has current frame")
                                .controls
                                .iter()
                                .any(|control| matches!(control, Control::FinishReturn { .. }));
                            if !return_already_scheduled {
                                let controls = &mut frames
                                    .last_mut()
                                    .expect("machine has current frame")
                                    .controls;
                                let deferred = deferred_expressions(controls, 0);
                                controls.clear();
                                controls.push(Control::FinishReturn { return_type });
                                push_deferred_controls(controls, deferred);
                            }
                            self.push_value(&mut frames, alternative)?;
                        }
                        _ => {
                            return Err(EvalFailure::Creator(
                                RejectKind::PropagationRequiresResult,
                            ));
                        }
                    }
                }
                Control::FinishIs { pattern } => {
                    let value = self.pop_value(&mut frames)?;
                    self.push_value(
                        &mut frames,
                        Value::Bool(pattern_bindings(&value, pattern).is_some()),
                    )?;
                }
                Control::FinishBinary { operator, site } => {
                    let right = self.pop_value(&mut frames)?;
                    let left = self.pop_value(&mut frames)?;
                    let value = apply_binary(operator, left, right, site)?;
                    self.push_value(&mut frames, value)?;
                }
                Control::FinishCall {
                    target,
                    arguments: argument_expressions,
                    site,
                } => {
                    let arguments = self.pop_values(&mut frames, argument_expressions.len())?;
                    match target {
                        CallTarget::Callable { .. } => match self.pop_value(&mut frames)? {
                            Value::Function(specialization) => {
                                let function = program
                                    .specialization_function(specialization)
                                    .ok_or(EvalFailure::Creator(RejectKind::UnresolvedCall))?;
                                let writebacks = function_writebacks(
                                    function,
                                    argument_expressions,
                                    &(0..function.parameters.len())
                                        .map(|index| index as u16)
                                        .collect::<Vec<_>>(),
                                )?;
                                self.push_function_frame(
                                    &mut frames,
                                    function,
                                    arguments,
                                    writebacks,
                                    site,
                                )?;
                            }
                            Value::Closure { id, captures } => {
                                let closure = program
                                    .closure(id)
                                    .ok_or(EvalFailure::Creator(RejectKind::UnresolvedCall))?;
                                self.push_closure_frame(
                                    &mut frames,
                                    closure,
                                    &captures,
                                    arguments,
                                    site,
                                )?;
                            }
                            _ => {
                                return Err(EvalFailure::Defect(Arc::from(
                                    "verified callable target is not a function value",
                                )));
                            }
                        },
                        CallTarget::TemplateFunction { .. } => {
                            return Err(EvalFailure::Defect(Arc::from(
                                "template call reached the concrete evaluator",
                            )));
                        }
                        CallTarget::Function {
                            specialization,
                            argument_order,
                            ..
                        } => {
                            let function = program
                                .specialization_function(*specialization)
                                .ok_or(EvalFailure::Creator(RejectKind::UnresolvedCall))?;
                            self.push_function_frame(
                                &mut frames,
                                function,
                                reorder_values(arguments, argument_order)?,
                                function_writebacks(
                                    function,
                                    argument_expressions,
                                    argument_order,
                                )?,
                                site,
                            )?;
                        }
                        CallTarget::Interface {
                            alternatives,
                            argument_order,
                            ..
                        } => {
                            let receiver = arguments.first().ok_or_else(|| {
                                EvalFailure::Defect(Arc::from(
                                    "verified interface call omitted its receiver",
                                ))
                            })?;
                            let nominal = match receiver {
                                Value::Struct { definition, .. } => *definition,
                                Value::UserVariant { id, .. } => id.owner,
                                _ => {
                                    return Err(EvalFailure::Defect(Arc::from(
                                        "existential receiver has no nominal representation",
                                    )));
                                }
                            };
                            let (_, _, specialization) = alternatives
                                .iter()
                                .find(|(candidate, _, _)| *candidate == nominal)
                                .ok_or(EvalFailure::Creator(RejectKind::UnresolvedCall))?;
                            let function = program
                                .specialization_function(*specialization)
                                .ok_or(EvalFailure::Creator(RejectKind::UnresolvedCall))?;
                            self.push_function_frame(
                                &mut frames,
                                function,
                                reorder_values(arguments, argument_order)?,
                                function_writebacks(
                                    function,
                                    argument_expressions,
                                    argument_order,
                                )?,
                                site,
                            )?;
                        }
                        CallTarget::Build { primitive, labels } => {
                            let value = self.construct(primitive.kind, &arguments, labels, site)?;
                            self.push_value(&mut frames, value)?;
                        }
                        CallTarget::BuiltinVariant(variant) => {
                            self.push_value(
                                &mut frames,
                                Value::BuiltinVariant {
                                    variant: *variant,
                                    payload: arguments.into(),
                                },
                            )?;
                        }
                        CallTarget::UserVariant {
                            id,
                            variant_order,
                            type_display,
                            variant_display,
                            argument_order,
                            ..
                        } => {
                            self.push_value(
                                &mut frames,
                                Value::UserVariant {
                                    id: *id,
                                    variant_order: *variant_order,
                                    type_display: type_display.clone(),
                                    variant_display: variant_display.clone(),
                                    payload: reorder_values(arguments, argument_order)?.into(),
                                },
                            )?;
                        }
                        CallTarget::Struct {
                            definition,
                            type_display,
                            field_order,
                            argument_fields,
                            ..
                        } => {
                            let authored = argument_fields
                                .iter()
                                .cloned()
                                .zip(arguments)
                                .collect::<BTreeMap<_, _>>();
                            let fields = field_order
                                .iter()
                                .map(|name| {
                                    authored
                                        .get(name)
                                        .cloned()
                                        .map(|value| (name.clone(), value))
                                        .ok_or_else(|| {
                                            EvalFailure::Defect(Arc::from(
                                                "verified struct construction omitted a field",
                                            ))
                                        })
                                })
                                .collect::<Result<Vec<_>, _>>()?;
                            self.push_value(
                                &mut frames,
                                Value::Struct {
                                    definition: *definition,
                                    type_display: type_display.clone(),
                                    fields: fields.into(),
                                },
                            )?;
                        }
                        CallTarget::Test {
                            id, argument_order, ..
                        } => {
                            self.push_value(
                                &mut frames,
                                Value::TestApplication {
                                    id: *id,
                                    payload: reorder_values(arguments, argument_order)?.into(),
                                },
                            )?;
                        }
                    }
                }
            }
        }
    }

    fn push_function_frame<'hir>(
        &mut self,
        frames: &mut Vec<MachineFrame<'hir>>,
        function: &'hir HirFunction,
        arguments: Vec<Value>,
        writebacks: Vec<(LocalId, Place)>,
        call_site: &SourceRange,
    ) -> Result<(), EvalFailure> {
        self.charge(5)?;
        if self.call_stack.len() == CALL_DEPTH_LIMIT {
            return Err(EvalFailure::Limit {
                policy: LimitPolicy::CallDepth,
                ceiling: CALL_DEPTH_LIMIT as u64,
                used: (CALL_DEPTH_LIMIT + 1) as u64,
            });
        }
        if function.parameters.len() != arguments.len() {
            return Err(EvalFailure::Creator(RejectKind::ArgumentCount));
        }
        let local_count = function
            .parameters
            .iter()
            .map(|(local, _, _)| local.0 as usize + 1)
            .max()
            .unwrap_or(0);
        let mut locals = vec![None; local_count];
        self.retain(64)?;
        for ((local, type_, _), value) in function.parameters.iter().zip(arguments) {
            let value = coerce(value, type_)
                .ok_or(EvalFailure::Creator(RejectKind::ArgumentTypeMismatch))?;
            self.retain(value_size(&value))?;
            locals[local.0 as usize] = Some(value);
        }
        self.call_stack
            .push((function.id.0, function.name.clone(), call_site.clone()));
        frames.push(MachineFrame {
            kind: FrameKind::Function(function),
            controls: vec![
                Control::FunctionFallthrough,
                Control::Block {
                    function,
                    statements: &function.body,
                    index: 0,
                },
            ],
            values: Vec::new(),
            locals,
            writebacks,
        });
        Ok(())
    }

    fn push_closure_frame<'hir>(
        &mut self,
        frames: &mut Vec<MachineFrame<'hir>>,
        closure: &'hir HirClosure,
        captures: &[(LocalId, Value)],
        arguments: Vec<Value>,
        call_site: &SourceRange,
    ) -> Result<(), EvalFailure> {
        self.charge(5)?;
        if self.call_stack.len() == CALL_DEPTH_LIMIT {
            return Err(EvalFailure::Limit {
                policy: LimitPolicy::CallDepth,
                ceiling: CALL_DEPTH_LIMIT as u64,
                used: (CALL_DEPTH_LIMIT + 1) as u64,
            });
        }
        if closure.parameters.len() != arguments.len() || closure.captures.len() != captures.len() {
            return Err(EvalFailure::Creator(RejectKind::ArgumentCount));
        }
        let local_count = closure
            .parameters
            .iter()
            .chain(closure.captures.iter())
            .map(|(local, _)| local.0 as usize + 1)
            .max()
            .unwrap_or(0);
        let mut locals = vec![None; local_count];
        self.retain(64)?;
        for ((expected_local, type_), (actual_local, value)) in
            closure.captures.iter().zip(captures)
        {
            if expected_local != actual_local {
                return Err(EvalFailure::Defect(Arc::from(
                    "closure capture layout disagrees with its value",
                )));
            }
            let value = coerce(value.clone(), type_)
                .ok_or(EvalFailure::Creator(RejectKind::ArgumentTypeMismatch))?;
            self.retain(value_size(&value))?;
            locals[expected_local.0 as usize] = Some(value);
        }
        for ((local, type_), value) in closure.parameters.iter().zip(arguments) {
            let value = coerce(value, type_)
                .ok_or(EvalFailure::Creator(RejectKind::ArgumentTypeMismatch))?;
            self.retain(value_size(&value))?;
            locals[local.0 as usize] = Some(value);
        }
        self.call_stack
            .push((closure.id.0, "<closure>".to_owned(), call_site.clone()));
        frames.push(MachineFrame {
            kind: FrameKind::Closure(closure),
            controls: vec![
                Control::FinishClosure {
                    return_type: &closure.return_type,
                },
                Control::Expression(&closure.body),
            ],
            values: Vec::new(),
            locals,
            writebacks: Vec::new(),
        });
        Ok(())
    }

    fn push_value<'hir>(
        &mut self,
        frames: &mut [MachineFrame<'hir>],
        value: Value,
    ) -> Result<(), EvalFailure> {
        self.retain(value_size(&value))?;
        frames
            .last_mut()
            .ok_or_else(|| EvalFailure::Defect(Arc::from("value produced without a frame")))?
            .values
            .push(value);
        Ok(())
    }

    fn pop_value<'hir>(&mut self, frames: &mut [MachineFrame<'hir>]) -> Result<Value, EvalFailure> {
        let value = frames
            .last_mut()
            .and_then(|frame| frame.values.pop())
            .ok_or_else(|| EvalFailure::Defect(Arc::from("evaluator value stack underflow")))?;
        self.release(value_size(&value))?;
        Ok(value)
    }

    fn pop_values<'hir>(
        &mut self,
        frames: &mut [MachineFrame<'hir>],
        count: usize,
    ) -> Result<Vec<Value>, EvalFailure> {
        let mut values = (0..count)
            .map(|_| self.pop_value(frames))
            .collect::<Result<Vec<_>, _>>()?;
        values.reverse();
        Ok(values)
    }

    fn clear_locals<'hir>(
        &mut self,
        frames: &mut [MachineFrame<'hir>],
        locals: &[LocalId],
    ) -> Result<(), EvalFailure> {
        let frame = frames
            .last_mut()
            .ok_or_else(|| EvalFailure::Defect(Arc::from("local cleanup without a frame")))?;
        for local in locals {
            if let Some(value) = frame
                .locals
                .get_mut(local.0 as usize)
                .and_then(Option::take)
            {
                self.release(value_size(&value))?;
            }
        }
        Ok(())
    }

    fn complete_frame<'hir>(
        &mut self,
        frames: &mut Vec<MachineFrame<'hir>>,
        value: Value,
    ) -> Result<Option<Value>, EvalFailure> {
        let frame = frames
            .pop()
            .ok_or_else(|| EvalFailure::Defect(Arc::from("completed missing evaluator frame")))?;
        let writebacks = frame
            .writebacks
            .iter()
            .map(|(local, place)| {
                frame
                    .locals
                    .get(local.0 as usize)
                    .and_then(Option::as_ref)
                    .cloned()
                    .map(|value| (place.clone(), value))
                    .ok_or_else(|| {
                        EvalFailure::Defect(Arc::from(
                            "mutable parameter writeback references a missing local",
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let frame_memory = 64_u64
            .saturating_add(frame.locals.iter().flatten().map(value_size).sum::<u64>())
            .saturating_add(frame.values.iter().map(value_size).sum::<u64>());
        self.release(frame_memory)?;
        let value = match frame.kind {
            FrameKind::Root => value,
            FrameKind::Function(_) | FrameKind::Closure(_) => {
                self.call_stack.pop().ok_or_else(|| {
                    EvalFailure::Defect(Arc::from("call frame had no call-stack entry"))
                })?;
                value
            }
            FrameKind::Constant {
                id,
                type_,
                fuel_before,
                peak_before,
                dependencies_before,
            } => {
                if self.evaluating_constants.pop() != Some(id) {
                    return Err(EvalFailure::Defect(Arc::from(
                        "constant evaluation stack disagrees with machine frame",
                    )));
                }
                let value = coerce(value, type_)
                    .ok_or(EvalFailure::Creator(RejectKind::ArgumentTypeMismatch))?;
                self.constant_values.insert(
                    id,
                    CachedConstant {
                        value: value.clone(),
                        fuel: self.fuel.saturating_sub(fuel_before),
                        peak_memory: self.peak_memory.saturating_sub(peak_before),
                        dependencies: self
                            .root_dependencies
                            .difference(&dependencies_before)
                            .copied()
                            .collect(),
                    },
                );
                self.compilation_memory =
                    self.compilation_memory.saturating_add(value_size(&value));
                if self.compilation_memory > COMPILATION_MEMORY_LIMIT {
                    return Err(EvalFailure::Limit {
                        policy: LimitPolicy::CompilationMemory,
                        ceiling: COMPILATION_MEMORY_LIMIT,
                        used: self.compilation_memory,
                    });
                }
                value
            }
        };
        for (place, value) in writebacks {
            let slot = place.local.0 as usize;
            let mut root = frames
                .last_mut()
                .and_then(|frame| frame.locals.get_mut(slot))
                .and_then(Option::take)
                .ok_or(EvalFailure::Creator(RejectKind::MissingLocal))?;
            let previous_size = value_size(&root);
            store_projected_value(&mut root, &place.projections, &[], value)?;
            let next_size = value_size(&root);
            self.release(previous_size)?;
            self.retain(next_size)?;
            frames.last_mut().expect("caller frame").locals[slot] = Some(root);
        }
        if frames.is_empty() {
            Ok(Some(value))
        } else {
            self.push_value(frames, value)?;
            Ok(None)
        }
    }

    fn construct(
        &mut self,
        kind: BuildKind,
        arguments: &[Value],
        labels: &[Arc<str>],
        site: &SourceRange,
    ) -> Result<Value, EvalFailure> {
        self.charge(3)?;
        let mut key = b"wrela.construction\0\x02".to_vec();
        for (id, _, call_site) in &self.call_stack {
            key.extend_from_slice(&id.to_be_bytes());
            key.extend_from_slice(call_site.path().as_bytes());
            key.extend_from_slice(&call_site.start().to_be_bytes());
            key.extend_from_slice(&call_site.end().to_be_bytes());
        }
        key.push(kind.canonical_tag());
        if let BuildKind::Node {
            definition,
            type_identity,
        } = kind
        {
            key.extend_from_slice(&definition.0.to_be_bytes());
            key.extend_from_slice(&type_identity.0.to_be_bytes());
        }
        key.extend_from_slice(site.path().as_bytes());
        key.extend_from_slice(&site.start().to_be_bytes());
        key.extend_from_slice(&site.end().to_be_bytes());
        let coordinate: Arc<[u8]> = Arc::from(key.clone());
        let ordinal = self.construction_coordinates.entry(coordinate).or_insert(0);
        key.extend_from_slice(&ordinal.to_be_bytes());
        *ordinal = ordinal.checked_add(1).ok_or_else(|| {
            EvalFailure::Defect(Arc::from("construction coordinate ordinal overflow"))
        })?;
        let identity = xxh3_128(&key);
        let key: Arc<[u8]> = key.into();
        if let Some(previous) = self.construction_keys.get(&identity) {
            let _ = previous;
            return Err(EvalFailure::Defect(Arc::from(
                "construction identity digest collision",
            )));
        }
        self.construction_keys.insert(identity, key);
        if labels.len() != arguments.len() {
            return Err(EvalFailure::Defect(Arc::from(
                "verified Build call label count disagrees with operands",
            )));
        }
        let mut edges = Vec::new();
        let mut operands = Vec::with_capacity(arguments.len());
        for (label, argument) in labels.iter().zip(arguments) {
            collect_construction_edges(argument, &mut edges);
            if kind == BuildKind::Test {
                collect_test_applications(argument, &mut self.test_applications);
            }
            operands.push(ConstructionOperand {
                label: Arc::clone(label),
                value: canonical(argument.clone()),
            });
        }
        let construction_memory = 64_u64.saturating_add(
            u64::try_from(edges.len())
                .unwrap_or(u64::MAX)
                .saturating_mul(16),
        );
        self.constructions.push(Construction {
            identity,
            kind,
            site: site.clone(),
            edges,
            operands,
        });
        self.retain(construction_memory)?;
        Ok(Value::SymbolicHandle { kind, identity })
    }
}

fn reorder_values(
    values: Vec<Value>,
    source_to_parameter: &[u16],
) -> Result<Vec<Value>, EvalFailure> {
    if values.len() != source_to_parameter.len() {
        return Err(EvalFailure::Defect(Arc::from(
            "verified call argument binding length disagrees with values",
        )));
    }
    let mut ordered = vec![None; values.len()];
    for (value, parameter) in values.into_iter().zip(source_to_parameter) {
        let Some(slot) = ordered.get_mut(usize::from(*parameter)) else {
            return Err(EvalFailure::Defect(Arc::from(
                "verified call argument binding names an invalid parameter",
            )));
        };
        if slot.replace(value).is_some() {
            return Err(EvalFailure::Defect(Arc::from(
                "verified call argument binding initializes a parameter twice",
            )));
        }
    }
    ordered
        .into_iter()
        .map(|value| {
            value.ok_or_else(|| {
                EvalFailure::Defect(Arc::from(
                    "verified call argument binding omits a parameter",
                ))
            })
        })
        .collect()
}

fn function_writebacks(
    function: &HirFunction,
    arguments: &[Expression],
    argument_order: &[u16],
) -> Result<Vec<(LocalId, Place)>, EvalFailure> {
    let mut writebacks = Vec::new();
    for (source_index, argument) in arguments.iter().enumerate() {
        if argument.access != crate::typed_hir::AccessMode::Mut {
            continue;
        }
        let parameter_index = usize::from(*argument_order.get(source_index).ok_or_else(|| {
            EvalFailure::Defect(Arc::from("mutable call argument order is incomplete"))
        })?);
        let local = function
            .parameters
            .get(parameter_index)
            .map(|(local, _, _)| *local)
            .ok_or_else(|| {
                EvalFailure::Defect(Arc::from("mutable call argument targets no parameter"))
            })?;
        let ExpressionKind::Read(place) = &argument.kind else {
            return Err(EvalFailure::Defect(Arc::from(
                "verified mutable call argument is not a typed place",
            )));
        };
        if place
            .projections
            .iter()
            .any(|projection| matches!(projection, PlaceProjection::Index { .. }))
        {
            return Err(EvalFailure::Defect(Arc::from(
                "verified mutable indexed writeback lacks captured indexes",
            )));
        }
        writebacks.push((local, place.clone()));
    }
    Ok(writebacks)
}

fn value_size(value: &Value) -> u64 {
    match value {
        Value::Unavailable | Value::Unit => 0,
        Value::Bool(_) => 1,
        Value::Integer { .. }
        | Value::Float { .. }
        | Value::Function(_)
        | Value::SymbolicHandle { .. } => 16,
        Value::Closure { captures, .. } => captures.iter().fold(16_u64, |size, (_, value)| {
            size.saturating_add(value_size(value))
        }),
        Value::Text(value) => 16_u64.saturating_add(value.len() as u64),
        Value::Scalar(_) => 4,
        Value::Bytes(value) => 16_u64.saturating_add(value.len() as u64),
        Value::Array(values)
        | Value::Tuple(values)
        | Value::BuiltinVariant {
            payload: values, ..
        }
        | Value::UserVariant {
            payload: values, ..
        }
        | Value::TestApplication {
            payload: values, ..
        } => values
            .iter()
            .fold(16_u64, |size, value| size.saturating_add(value_size(value))),
        Value::Struct { fields, .. } => fields.iter().fold(16_u64, |size, (name, value)| {
            size.saturating_add(name.len() as u64)
                .saturating_add(value_size(value))
        }),
    }
}

fn collect_construction_edges(value: &Value, edges: &mut Vec<u128>) {
    match value {
        Value::SymbolicHandle { identity, .. } => edges.push(*identity),
        Value::Array(values)
        | Value::Tuple(values)
        | Value::BuiltinVariant {
            payload: values, ..
        }
        | Value::UserVariant {
            payload: values, ..
        }
        | Value::TestApplication {
            payload: values, ..
        } => {
            for value in &**values {
                collect_construction_edges(value, edges);
            }
        }
        Value::Struct { fields, .. } => {
            for (_, value) in &**fields {
                collect_construction_edges(value, edges);
            }
        }
        Value::Unavailable
        | Value::Unit
        | Value::Bool(_)
        | Value::Integer { .. }
        | Value::Float { .. }
        | Value::Function(_)
        | Value::Text(_)
        | Value::Scalar(_)
        | Value::Bytes(_) => {}
        Value::Closure { captures, .. } => {
            for (_, value) in &**captures {
                collect_construction_edges(value, edges);
            }
        }
    }
}

fn collect_test_applications(value: &Value, applications: &mut Vec<AppliedTest>) {
    match value {
        Value::TestApplication { id, payload } => {
            applications.push(AppliedTest {
                id: *id,
                payload: payload.iter().cloned().map(canonical).collect(),
            });
            for value in &**payload {
                collect_test_applications(value, applications);
            }
        }
        Value::Array(values)
        | Value::Tuple(values)
        | Value::BuiltinVariant {
            payload: values, ..
        }
        | Value::UserVariant {
            payload: values, ..
        } => {
            for value in &**values {
                collect_test_applications(value, applications);
            }
        }
        Value::Struct { fields, .. } => {
            for (_, value) in &**fields {
                collect_test_applications(value, applications);
            }
        }
        Value::Unavailable
        | Value::Unit
        | Value::Bool(_)
        | Value::Integer { .. }
        | Value::Float { .. }
        | Value::Function(_)
        | Value::Text(_)
        | Value::Scalar(_)
        | Value::Bytes(_)
        | Value::SymbolicHandle { .. } => {}
        Value::Closure { captures, .. } => {
            for (_, value) in &**captures {
                collect_test_applications(value, applications);
            }
        }
    }
}

fn apply_binary(
    operator: BinaryOperator,
    left: Value,
    right: Value,
    site: &SourceRange,
) -> Result<Value, EvalFailure> {
    match (left, right) {
        (
            Value::Integer { kind, value: left },
            Value::Integer {
                kind: right_kind,
                value: right,
            },
        ) => {
            if kind != right_kind {
                return Err(EvalFailure::Defect(Arc::from(
                    "verified integer operation mixes formats",
                )));
            }
            match operator {
                BinaryOperator::Range | BinaryOperator::RangeInclusive => {
                    let inclusive = operator == BinaryOperator::RangeInclusive;
                    let count = if right < left {
                        0
                    } else {
                        let distance = right.checked_sub(left).ok_or_else(|| {
                            EvalFailure::Panic(PanicKind::IntegerOverflow, site.clone())
                        })?;
                        distance.checked_add(i128::from(inclusive)).ok_or_else(|| {
                            EvalFailure::Panic(PanicKind::IntegerOverflow, site.clone())
                        })?
                    };
                    let count = u64::try_from(count).unwrap_or(u64::MAX);
                    let maximum = MEMORY_LIMIT / 16;
                    if count > maximum {
                        return Err(EvalFailure::Limit {
                            policy: LimitPolicy::RootMemory,
                            ceiling: MEMORY_LIMIT,
                            used: count.saturating_mul(16),
                        });
                    }
                    let mut values = Vec::with_capacity(count as usize);
                    for offset in 0..count {
                        values.push(Value::Integer {
                            kind,
                            value: left + i128::from(offset),
                        });
                    }
                    Ok(Value::Array(values.into()))
                }
                BinaryOperator::Add => checked_integer(kind, left.checked_add(right), site),
                BinaryOperator::Subtract => checked_integer(kind, left.checked_sub(right), site),
                BinaryOperator::Multiply => checked_integer(kind, left.checked_mul(right), site),
                BinaryOperator::Divide | BinaryOperator::Remainder if right == 0 => {
                    Err(EvalFailure::Panic(PanicKind::DivisionByZero, site.clone()))
                }
                BinaryOperator::Divide => checked_integer(kind, left.checked_div(right), site),
                BinaryOperator::Remainder => checked_integer(kind, left.checked_rem(right), site),
                BinaryOperator::BitAnd => Ok(Value::Integer {
                    kind,
                    value: left & right,
                }),
                BinaryOperator::BitOr => Ok(Value::Integer {
                    kind,
                    value: left | right,
                }),
                BinaryOperator::BitXor => Ok(Value::Integer {
                    kind,
                    value: left ^ right,
                }),
                BinaryOperator::ShiftLeft => {
                    let shift = u32::try_from(right).ok();
                    checked_integer(kind, shift.and_then(|shift| left.checked_shl(shift)), site)
                }
                BinaryOperator::ShiftRight => {
                    let shift = u32::try_from(right).ok();
                    checked_integer(kind, shift.and_then(|shift| left.checked_shr(shift)), site)
                }
                BinaryOperator::Equal => Ok(Value::Bool(left == right)),
                BinaryOperator::NotEqual => Ok(Value::Bool(left != right)),
                BinaryOperator::Less => Ok(Value::Bool(left < right)),
                BinaryOperator::LessEqual => Ok(Value::Bool(left <= right)),
                BinaryOperator::Greater => Ok(Value::Bool(left > right)),
                BinaryOperator::GreaterEqual => Ok(Value::Bool(left >= right)),
                BinaryOperator::And | BinaryOperator::Or => Err(EvalFailure::Defect(Arc::from(
                    "short-circuit boolean operator reached integer evaluation",
                ))),
            }
        }
        (Value::Bool(left), Value::Bool(right)) => match operator {
            BinaryOperator::Equal => Ok(Value::Bool(left == right)),
            BinaryOperator::NotEqual => Ok(Value::Bool(left != right)),
            BinaryOperator::And | BinaryOperator::Or => Err(EvalFailure::Defect(Arc::from(
                "short-circuit boolean operator reached eager evaluation",
            ))),
            _ => Err(EvalFailure::Creator(RejectKind::InvalidBooleanOperator)),
        },
        (Value::Text(left), Value::Text(right)) => apply_ordering(operator, &*left, &*right),
        (Value::Scalar(left), Value::Scalar(right)) => apply_ordering(operator, &left, &right),
        (Value::Bytes(left), Value::Bytes(right)) => apply_ordering(operator, &*left, &*right),
        (Value::Unit, Value::Unit) => apply_equality(operator, true),
        (Value::Array(left), Value::Array(right)) | (Value::Tuple(left), Value::Tuple(right)) => {
            apply_structural_comparison(operator, &left, &right)
        }
        (
            Value::Float { kind, bits: left },
            Value::Float {
                kind: right_kind,
                bits: right,
            },
        ) => {
            if kind != right_kind {
                return Err(EvalFailure::Defect(Arc::from(
                    "verified float operation mixes formats",
                )));
            }
            apply_float(operator, kind, left, right)
        }
        (left @ Value::UserVariant { .. }, right @ Value::UserVariant { .. })
        | (left @ Value::Struct { .. }, right @ Value::Struct { .. })
        | (left @ Value::BuiltinVariant { .. }, right @ Value::BuiltinVariant { .. }) => {
            apply_value_comparison(operator, &left, &right)
        }
        _ => Err(EvalFailure::Creator(RejectKind::BinaryTypeMismatch)),
    }
}

fn apply_structural_comparison(
    operator: BinaryOperator,
    left: &[Value],
    right: &[Value],
) -> Result<Value, EvalFailure> {
    let ordering = compare_sequences(left, right)?;
    apply_comparison_order(operator, ordering)
}

fn apply_value_comparison(
    operator: BinaryOperator,
    left: &Value,
    right: &Value,
) -> Result<Value, EvalFailure> {
    apply_comparison_order(operator, compare_values(left, right)?)
}

fn apply_comparison_order(
    operator: BinaryOperator,
    ordering: Option<Ordering>,
) -> Result<Value, EvalFailure> {
    let value = match operator {
        BinaryOperator::Equal => ordering == Some(Ordering::Equal),
        BinaryOperator::NotEqual => ordering != Some(Ordering::Equal),
        BinaryOperator::Less => ordering == Some(Ordering::Less),
        BinaryOperator::LessEqual => {
            matches!(ordering, Some(Ordering::Less | Ordering::Equal))
        }
        BinaryOperator::Greater => ordering == Some(Ordering::Greater),
        BinaryOperator::GreaterEqual => {
            matches!(ordering, Some(Ordering::Greater | Ordering::Equal))
        }
        _ => return Err(EvalFailure::Creator(RejectKind::BinaryTypeMismatch)),
    };
    Ok(Value::Bool(value))
}

fn compare_sequences(left: &[Value], right: &[Value]) -> Result<Option<Ordering>, EvalFailure> {
    for (left, right) in left.iter().zip(right) {
        let ordering = compare_values(left, right)?;
        if ordering != Some(Ordering::Equal) {
            return Ok(ordering);
        }
    }
    Ok(Some(left.len().cmp(&right.len())))
}

fn compare_values(left: &Value, right: &Value) -> Result<Option<Ordering>, EvalFailure> {
    Ok(Some(match (left, right) {
        (Value::Unit, Value::Unit) => Ordering::Equal,
        (Value::Bool(left), Value::Bool(right)) => left.cmp(right),
        (
            Value::Integer { kind, value: left },
            Value::Integer {
                kind: right_kind,
                value: right,
            },
        ) if kind == right_kind => left.cmp(right),
        (
            Value::Float { kind, bits: left },
            Value::Float {
                kind: right_kind,
                bits: right,
            },
        ) if kind == right_kind => {
            return Ok(decode_float(*kind, *left).partial_cmp(&decode_float(*kind, *right)));
        }
        (Value::Text(left), Value::Text(right)) => left.cmp(right),
        (Value::Scalar(left), Value::Scalar(right)) => left.cmp(right),
        (Value::Bytes(left), Value::Bytes(right)) => left.cmp(right),
        (Value::Array(left), Value::Array(right)) | (Value::Tuple(left), Value::Tuple(right)) => {
            return compare_sequences(left, right);
        }
        (
            Value::UserVariant {
                id: left_id,
                variant_order: left_order,
                payload: left,
                ..
            },
            Value::UserVariant {
                id: right_id,
                variant_order: right_order,
                payload: right,
                ..
            },
        ) if left_id.owner == right_id.owner => {
            let ordering = left_order.cmp(right_order);
            if ordering == Ordering::Equal {
                return compare_sequences(left, right);
            } else {
                ordering
            }
        }
        (
            Value::Struct {
                definition: left_definition,
                fields: left,
                ..
            },
            Value::Struct {
                definition: right_definition,
                fields: right,
                ..
            },
        ) if left_definition == right_definition => {
            let left_values = left
                .iter()
                .map(|(_, value)| value.clone())
                .collect::<Vec<_>>();
            let right_values = right
                .iter()
                .map(|(_, value)| value.clone())
                .collect::<Vec<_>>();
            return compare_sequences(&left_values, &right_values);
        }
        (
            Value::BuiltinVariant {
                variant: left_variant,
                payload: left,
            },
            Value::BuiltinVariant {
                variant: right_variant,
                payload: right,
            },
        ) => {
            let ordering = left_variant
                .canonical_tag()
                .cmp(&right_variant.canonical_tag());
            if ordering == Ordering::Equal {
                return compare_sequences(left, right);
            } else {
                ordering
            }
        }
        _ => return Err(EvalFailure::Creator(RejectKind::BinaryTypeMismatch)),
    }))
}

fn pattern_bindings(value: &Value, pattern: &HirMatchPattern) -> Option<Vec<(LocalId, Value)>> {
    match pattern {
        HirMatchPattern::Variant {
            id: expected,
            payload: expected_payload,
        } => {
            let Value::UserVariant { id, payload, .. } = value else {
                return None;
            };
            if id != expected || payload.len() != expected_payload.len() {
                return None;
            }
            let mut bindings = Vec::new();
            for (value, pattern) in payload.iter().zip(expected_payload.iter()) {
                bindings.extend(pattern_bindings(value, pattern)?);
            }
            Some(bindings)
        }
        HirMatchPattern::Struct {
            definition: expected,
            fields: expected_fields,
        } => {
            let Value::Struct {
                definition, fields, ..
            } = value
            else {
                return None;
            };
            if definition != expected || fields.len() != expected_fields.len() {
                return None;
            }
            let mut bindings = Vec::new();
            for ((_, value), pattern) in fields.iter().zip(expected_fields.iter()) {
                bindings.extend(pattern_bindings(value, pattern)?);
            }
            Some(bindings)
        }
        HirMatchPattern::Tuple(expected) => {
            let Value::Tuple(values) = value else {
                return None;
            };
            sequence_pattern_bindings(values, expected)
        }
        HirMatchPattern::FixedArray(expected) => {
            let Value::Array(values) = value else {
                return None;
            };
            sequence_pattern_bindings(values, expected)
        }
        HirMatchPattern::Or(alternatives) => alternatives
            .iter()
            .find_map(|alternative| pattern_bindings(value, alternative)),
        HirMatchPattern::Literal(literal) => literal_matches(value, literal).then(Vec::new),
        HirMatchPattern::Binding { local, .. } => Some(vec![(*local, value.clone())]),
        HirMatchPattern::Wildcard => Some(Vec::new()),
    }
}

fn unavailable_shape(value: &Value) -> Value {
    match value {
        Value::Struct {
            definition,
            type_display,
            fields,
        } => Value::Struct {
            definition: *definition,
            type_display: type_display.clone(),
            fields: fields
                .iter()
                .map(|(name, value)| (name.clone(), unavailable_shape(value)))
                .collect(),
        },
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(unavailable_shape)
                .collect::<Vec<_>>()
                .into(),
        ),
        Value::Tuple(values) => Value::Tuple(
            values
                .iter()
                .map(unavailable_shape)
                .collect::<Vec<_>>()
                .into(),
        ),
        Value::BuiltinVariant { variant, payload } => Value::BuiltinVariant {
            variant: *variant,
            payload: payload
                .iter()
                .map(unavailable_shape)
                .collect::<Vec<_>>()
                .into(),
        },
        Value::UserVariant {
            id,
            variant_order,
            type_display,
            variant_display,
            payload,
        } => Value::UserVariant {
            id: *id,
            variant_order: *variant_order,
            type_display: type_display.clone(),
            variant_display: variant_display.clone(),
            payload: payload
                .iter()
                .map(unavailable_shape)
                .collect::<Vec<_>>()
                .into(),
        },
        Value::TestApplication { id, payload } => Value::TestApplication {
            id: *id,
            payload: payload
                .iter()
                .map(unavailable_shape)
                .collect::<Vec<_>>()
                .into(),
        },
        Value::Unavailable
        | Value::Unit
        | Value::Bool(_)
        | Value::Integer { .. }
        | Value::Float { .. }
        | Value::Text(_)
        | Value::Scalar(_)
        | Value::Bytes(_)
        | Value::Function(_)
        | Value::Closure { .. }
        | Value::SymbolicHandle { .. } => Value::Unavailable,
    }
}

fn read_projected_value(
    root: &Value,
    projections: &[PlaceProjection],
    indexes: &[Value],
) -> Result<Value, EvalFailure> {
    let mut value = root;
    let mut index_offset = 0;
    for projection in projections {
        if matches!(value, Value::Unavailable) {
            return Err(EvalFailure::Defect(Arc::from(
                "verified place reads unavailable runtime custody",
            )));
        }
        match projection {
            PlaceProjection::Field { name, .. } => {
                let Value::Struct { fields, .. } = value else {
                    return Err(EvalFailure::Defect(Arc::from(
                        "verified place field projects a non-struct value",
                    )));
                };
                value = fields
                    .iter()
                    .find(|(field, _)| field == name)
                    .map(|(_, value)| value)
                    .ok_or_else(|| {
                        EvalFailure::Defect(Arc::from(
                            "verified place field names an absent runtime field",
                        ))
                    })?;
            }
            PlaceProjection::Index { index, .. } => {
                let Some(Value::Integer {
                    value: authored, ..
                }) = indexes.get(index_offset)
                else {
                    return Err(EvalFailure::Defect(Arc::from(
                        "verified place index is not an integer",
                    )));
                };
                index_offset += 1;
                let Value::Array(values) = value else {
                    return Err(EvalFailure::Defect(Arc::from(
                        "verified place index projects a non-array value",
                    )));
                };
                value = usize::try_from(*authored)
                    .ok()
                    .and_then(|offset| values.get(offset))
                    .ok_or_else(|| {
                        EvalFailure::Panic(PanicKind::IndexOutOfBounds, index.source.clone())
                    })?;
            }
        }
    }
    if matches!(value, Value::Unavailable) {
        return Err(EvalFailure::Defect(Arc::from(
            "verified place reads unavailable runtime custody",
        )));
    }
    Ok(value.clone())
}

fn extract_projected_value(
    root: &mut Value,
    projections: &[PlaceProjection],
    indexes: &[Value],
) -> Result<Value, EvalFailure> {
    fn extract(
        current: &mut Value,
        projections: &[PlaceProjection],
        indexes: &[Value],
        index_offset: &mut usize,
    ) -> Result<Value, EvalFailure> {
        let Some((projection, remaining)) = projections.split_first() else {
            if matches!(current, Value::Unavailable) {
                return Err(EvalFailure::Defect(Arc::from(
                    "verified move extracts unavailable runtime custody",
                )));
            }
            let unavailable = unavailable_shape(current);
            return Ok(std::mem::replace(current, unavailable));
        };
        match projection {
            PlaceProjection::Field { name, .. } => {
                let Value::Struct { fields, .. } = current else {
                    return Err(EvalFailure::Defect(Arc::from(
                        "verified place field extracts through a non-struct value",
                    )));
                };
                let field = Arc::make_mut(fields)
                    .iter_mut()
                    .find(|(field, _)| field == name)
                    .map(|(_, value)| value)
                    .ok_or_else(|| {
                        EvalFailure::Defect(Arc::from(
                            "verified place field names an absent runtime field",
                        ))
                    })?;
                extract(field, remaining, indexes, index_offset)
            }
            PlaceProjection::Index { index, .. } => {
                let Some(Value::Integer {
                    value: authored, ..
                }) = indexes.get(*index_offset)
                else {
                    return Err(EvalFailure::Defect(Arc::from(
                        "verified place index is not an integer",
                    )));
                };
                *index_offset += 1;
                let Value::Array(values) = current else {
                    return Err(EvalFailure::Defect(Arc::from(
                        "verified place index extracts through a non-array value",
                    )));
                };
                let element = usize::try_from(*authored)
                    .ok()
                    .and_then(|offset| Arc::make_mut(values).get_mut(offset))
                    .ok_or_else(|| {
                        EvalFailure::Panic(PanicKind::IndexOutOfBounds, index.source.clone())
                    })?;
                extract(element, remaining, indexes, index_offset)
            }
        }
    }

    extract(root, projections, indexes, &mut 0)
}

fn store_projected_value(
    root: &mut Value,
    projections: &[PlaceProjection],
    indexes: &[Value],
    value: Value,
) -> Result<(), EvalFailure> {
    fn store(
        current: &mut Value,
        projections: &[PlaceProjection],
        indexes: &[Value],
        index_offset: &mut usize,
        value: Value,
    ) -> Result<(), EvalFailure> {
        let Some((projection, remaining)) = projections.split_first() else {
            *current = value;
            return Ok(());
        };
        match projection {
            PlaceProjection::Field { name, .. } => {
                let Value::Struct { fields, .. } = current else {
                    return Err(EvalFailure::Defect(Arc::from(
                        "verified place field stores through a non-struct value",
                    )));
                };
                let field = Arc::make_mut(fields)
                    .iter_mut()
                    .find(|(field, _)| field == name)
                    .map(|(_, value)| value)
                    .ok_or_else(|| {
                        EvalFailure::Defect(Arc::from(
                            "verified place field names an absent runtime field",
                        ))
                    })?;
                store(field, remaining, indexes, index_offset, value)
            }
            PlaceProjection::Index { index, .. } => {
                let Some(Value::Integer {
                    value: authored, ..
                }) = indexes.get(*index_offset)
                else {
                    return Err(EvalFailure::Defect(Arc::from(
                        "verified place index is not an integer",
                    )));
                };
                *index_offset += 1;
                let Value::Array(values) = current else {
                    return Err(EvalFailure::Defect(Arc::from(
                        "verified place index stores through a non-array value",
                    )));
                };
                let element = usize::try_from(*authored)
                    .ok()
                    .and_then(|offset| Arc::make_mut(values).get_mut(offset))
                    .ok_or_else(|| {
                        EvalFailure::Panic(PanicKind::IndexOutOfBounds, index.source.clone())
                    })?;
                store(element, remaining, indexes, index_offset, value)
            }
        }
    }

    store(root, projections, indexes, &mut 0, value)
}

fn sequence_pattern_bindings(
    values: &[Value],
    patterns: &[HirMatchPattern],
) -> Option<Vec<(LocalId, Value)>> {
    if values.len() != patterns.len() {
        return None;
    }
    let mut bindings = Vec::new();
    for (value, pattern) in values.iter().zip(patterns) {
        bindings.extend(pattern_bindings(value, pattern)?);
    }
    Some(bindings)
}

fn literal_matches(value: &Value, literal: &Literal) -> bool {
    match (value, literal) {
        (Value::Unit, Literal::Unit) => true,
        (Value::Bool(actual), Literal::Bool(expected)) => actual == expected,
        (
            Value::Integer {
                kind: actual_kind,
                value: actual,
            },
            Literal::Integer {
                kind: expected_kind,
                value: expected,
            },
        ) => actual_kind == expected_kind && actual == expected,
        (
            Value::Float {
                kind: actual_kind,
                bits: actual,
            },
            Literal::Float {
                kind: expected_kind,
                bits: expected,
            },
        ) => actual_kind == expected_kind && actual == expected,
        (Value::Text(actual), Literal::Text(expected)) => actual == expected,
        (Value::Scalar(actual), Literal::Scalar(expected)) => actual == expected,
        (Value::Bytes(actual), Literal::Bytes(expected)) => actual == expected,
        _ => false,
    }
}

fn apply_equality(operator: BinaryOperator, equal: bool) -> Result<Value, EvalFailure> {
    match operator {
        BinaryOperator::Equal => Ok(Value::Bool(equal)),
        BinaryOperator::NotEqual => Ok(Value::Bool(!equal)),
        _ => Err(EvalFailure::Defect(Arc::from(
            "ordered comparison reached an equality-only value",
        ))),
    }
}

fn apply_ordering<T: Ord + ?Sized>(
    operator: BinaryOperator,
    left: &T,
    right: &T,
) -> Result<Value, EvalFailure> {
    Ok(Value::Bool(match operator {
        BinaryOperator::Equal => left == right,
        BinaryOperator::NotEqual => left != right,
        BinaryOperator::Less => left < right,
        BinaryOperator::LessEqual => left <= right,
        BinaryOperator::Greater => left > right,
        BinaryOperator::GreaterEqual => left >= right,
        _ => return Err(EvalFailure::Creator(RejectKind::BinaryTypeMismatch)),
    }))
}

fn checked_integer(
    kind: IntegerType,
    value: Option<i128>,
    site: &SourceRange,
) -> Result<Value, EvalFailure> {
    let value =
        value.ok_or_else(|| EvalFailure::Panic(PanicKind::IntegerOverflow, site.clone()))?;
    if !kind.fits(value) {
        return Err(EvalFailure::Panic(PanicKind::IntegerOverflow, site.clone()));
    }
    Ok(Value::Integer { kind, value })
}

fn apply_float(
    operator: BinaryOperator,
    kind: FloatType,
    left_bits: u64,
    right_bits: u64,
) -> Result<Value, EvalFailure> {
    let left = decode_float(kind, left_bits);
    let right = decode_float(kind, right_bits);
    let value = match operator {
        BinaryOperator::Add => left + right,
        BinaryOperator::Subtract => left - right,
        BinaryOperator::Multiply => left * right,
        BinaryOperator::Divide => left / right,
        BinaryOperator::Remainder => left % right,
        BinaryOperator::Range
        | BinaryOperator::RangeInclusive
        | BinaryOperator::BitAnd
        | BinaryOperator::BitOr
        | BinaryOperator::BitXor
        | BinaryOperator::ShiftLeft
        | BinaryOperator::ShiftRight => {
            return Err(EvalFailure::Defect(Arc::from(
                "integer-only operator reached float evaluation",
            )));
        }
        BinaryOperator::Equal => return Ok(Value::Bool(left == right)),
        BinaryOperator::NotEqual => return Ok(Value::Bool(left != right)),
        BinaryOperator::Less => return Ok(Value::Bool(left < right)),
        BinaryOperator::LessEqual => return Ok(Value::Bool(left <= right)),
        BinaryOperator::Greater => return Ok(Value::Bool(left > right)),
        BinaryOperator::GreaterEqual => return Ok(Value::Bool(left >= right)),
        BinaryOperator::And | BinaryOperator::Or => {
            return Err(EvalFailure::Defect(Arc::from(
                "short-circuit boolean operator reached float evaluation",
            )));
        }
    };
    Ok(Value::Float {
        kind,
        bits: encode_float(kind, value),
    })
}

fn coerce(value: Value, expected: &Type) -> Option<Value> {
    if value_matches(&value, expected) {
        return Some(value);
    }
    match (value, expected) {
        (value, Type::Parameter { .. } | Type::Infer) => Some(value),
        (value, Type::Result { success, .. }) => {
            coerce(value, success).map(|value| Value::BuiltinVariant {
                variant: BuiltinVariant::ResultOk,
                payload: Arc::from([value]),
            })
        }
        _ => None,
    }
}

fn value_matches(value: &Value, expected: &Type) -> bool {
    match (value, expected) {
        (Value::Unit, Type::Unit) | (Value::Bool(_), Type::Bool) | (Value::Text(_), Type::Text) => {
            true
        }
        (Value::Scalar(_), Type::Scalar) | (Value::Bytes(_), Type::Bytes) => true,
        (Value::Integer { kind, .. }, Type::Integer(expected)) => kind == expected,
        (Value::Float { kind, .. }, Type::Float(expected)) => kind == expected,
        (Value::Function(_) | Value::Closure { .. }, Type::Function { .. }) => true,
        (Value::Struct { .. } | Value::UserVariant { .. }, Type::Any { .. }) => true,
        (Value::Array(_), Type::Array(_)) => true,
        (Value::Array(values), Type::FixedArray { length, .. }) => length
            .value()
            .is_some_and(|length| u64::try_from(values.len()) == Ok(length)),
        (Value::Tuple(values), Type::Tuple(expected)) => {
            values.len() == expected.len()
                && values
                    .iter()
                    .zip(expected.iter())
                    .all(|(value, expected)| value_matches(value, expected))
        }
        (
            Value::BuiltinVariant {
                variant: BuiltinVariant::ResultOk | BuiltinVariant::ResultErr,
                ..
            },
            Type::Result { .. },
        ) => true,
        (
            Value::BuiltinVariant {
                variant: BuiltinVariant::OptionSome | BuiltinVariant::OptionNone,
                ..
            },
            Type::Option(_),
        ) => true,
        (
            Value::UserVariant { id, .. },
            Type::Nominal {
                definition: expected,
                ..
            },
        ) => id.owner == *expected,
        (
            Value::Struct { definition, .. },
            Type::Nominal {
                definition: expected,
                ..
            },
        ) => definition == expected,
        (
            Value::SymbolicHandle {
                kind: BuildKind::Image,
                ..
            },
            Type::Builtin(crate::model::BuiltinType::Image),
        ) => true,
        (
            Value::SymbolicHandle {
                kind: BuildKind::Test,
                ..
            },
            Type::Builtin(crate::model::BuiltinType::Test),
        ) => true,
        (
            Value::SymbolicHandle {
                kind: BuildKind::Node {
                    definition: actual, ..
                },
                ..
            },
            Type::Nominal {
                definition: expected,
                ..
            },
        ) => actual == expected,
        (
            Value::TestApplication { .. },
            Type::Builtin(crate::model::BuiltinType::TestApplication),
        ) => true,
        (_, Type::Parameter { .. } | Type::Infer) => true,
        _ => false,
    }
}

fn canonical(value: Value) -> CanonicalValue {
    match value {
        Value::Unavailable => {
            unreachable!("unavailable runtime custody cannot become a canonical result")
        }
        Value::Unit => CanonicalValue::Unit,
        Value::Bool(value) => CanonicalValue::Bool(value),
        Value::Integer { kind, value } => CanonicalValue::Integer {
            type_name: Arc::from(kind.name()),
            value,
        },
        Value::Float { kind, bits } => CanonicalValue::Float {
            type_name: Arc::from(kind.name()),
            bits,
        },
        Value::Text(value) => CanonicalValue::Text(value),
        Value::Scalar(value) => CanonicalValue::Scalar(value),
        Value::Bytes(value) => CanonicalValue::Bytes(value),
        Value::Function(identity) => CanonicalValue::Function {
            identity: identity.0,
        },
        Value::Closure { id, captures } => CanonicalValue::Closure {
            identity: id.0,
            captures: captures
                .iter()
                .map(|(_, value)| canonical(value.clone()))
                .collect(),
        },
        Value::Array(values) => {
            CanonicalValue::Array(values.iter().cloned().map(canonical).collect())
        }
        Value::Tuple(values) => {
            CanonicalValue::Tuple(values.iter().cloned().map(canonical).collect())
        }
        Value::BuiltinVariant { variant, payload } => {
            let (type_name, variant) = match variant {
                BuiltinVariant::ResultOk => ("Result", "Ok"),
                BuiltinVariant::ResultErr => ("Result", "Err"),
                BuiltinVariant::OptionSome => ("Option", "Some"),
                BuiltinVariant::OptionNone => ("Option", "None"),
            };
            CanonicalValue::Variant {
                type_name: Arc::from(type_name),
                variant: Arc::from(variant),
                payload: payload.iter().cloned().map(canonical).collect(),
            }
        }
        Value::UserVariant {
            type_display,
            variant_display,
            payload,
            ..
        } => CanonicalValue::Variant {
            type_name: type_display,
            variant: variant_display,
            payload: payload.iter().cloned().map(canonical).collect(),
        },
        Value::Struct {
            type_display,
            fields,
            ..
        } => CanonicalValue::Struct {
            type_name: type_display,
            fields: fields
                .iter()
                .cloned()
                .map(|(name, value)| (name, canonical(value)))
                .collect(),
        },
        Value::TestApplication { id, payload } => CanonicalValue::Variant {
            type_name: Arc::from("TestApplication"),
            variant: Arc::from(format!("{:032x}", id.identity)),
            payload: payload.iter().cloned().map(canonical).collect(),
        },
        Value::SymbolicHandle { kind, identity } => CanonicalValue::SymbolicHandle {
            kind: match kind {
                BuildKind::Image => crate::ConstructionKind::Image,
                BuildKind::Test => crate::ConstructionKind::Test,
                BuildKind::Node { type_identity, .. } => crate::ConstructionKind::Node {
                    type_identity: type_identity.0,
                },
            },
            identity,
        },
    }
}

fn encode_float(kind: FloatType, value: f64) -> u64 {
    if value.is_nan() {
        return match kind {
            FloatType::F16 => 0x7e00,
            FloatType::F32 => 0x7fc0_0000,
            FloatType::F64 => 0x7ff8_0000_0000_0000,
        };
    }
    match kind {
        FloatType::F16 => u64::from(half::f16::from_f64(value).to_bits()),
        FloatType::F32 => u64::from((value as f32).to_bits()),
        FloatType::F64 => value.to_bits(),
    }
}
fn decode_float(kind: FloatType, bits: u64) -> f64 {
    match kind {
        FloatType::F16 => half::f16::from_bits(bits as u16).to_f64(),
        FloatType::F32 => f32::from_bits(bits as u32).into(),
        FloatType::F64 => f64::from_bits(bits),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typed_hir::{self, BuildAuthority, PoolAuthority, ProgramInput};

    #[test]
    fn projected_resource_moves_extract_instead_of_clone() {
        let projection = PlaceProjection::Field {
            definition: DefinitionId(1),
            name: Arc::from("ticket"),
            type_: Type::Bool,
            mutable: true,
        };
        let mut root = Value::Struct {
            definition: DefinitionId(1),
            type_display: Arc::from("Envelope"),
            fields: Arc::from([
                (Arc::from("ticket"), Value::Bool(true)),
                (Arc::from("other"), Value::Bool(false)),
            ]),
        };

        let extracted = extract_projected_value(&mut root, std::slice::from_ref(&projection), &[])
            .expect("verified projection extracts");
        assert_eq!(extracted, Value::Bool(true));
        assert!(read_projected_value(&root, &[projection], &[]).is_err());
    }

    #[test]
    fn logical_fuel_exhaustion_is_exact_and_host_time_independent() {
        let mut identities = crate::identity::IdentityCatalog::empty();
        let program = typed_hir::verify(
            ProgramInput::default(),
            &BuildAuthority::test_compiler_distribution(),
            &PoolAuthority::from_authenticated_scoped_factory(None),
            &mut identities,
            &Cancellation::new(),
        )
        .expect("empty program verifies");
        let cancellation = Cancellation::new();
        let mut engine = Engine::new(&program, &cancellation);
        engine.charge(FUEL_LIMIT).expect("at ceiling is admitted");
        assert_eq!(
            engine.charge(1),
            Err(EvalFailure::Limit {
                policy: LimitPolicy::RootFuel,
                ceiling: FUEL_LIMIT,
                used: FUEL_LIMIT + 1
            })
        );
    }

    #[test]
    fn cancellation_is_polled_during_evaluation() {
        let mut identities = crate::identity::IdentityCatalog::empty();
        let program = typed_hir::verify(
            ProgramInput::default(),
            &BuildAuthority::test_compiler_distribution(),
            &PoolAuthority::from_authenticated_scoped_factory(None),
            &mut identities,
            &Cancellation::new(),
        )
        .expect("empty program verifies");
        let cancellation = Cancellation::new();
        cancellation.cancel();
        let mut engine = Engine::new(&program, &cancellation);
        assert_eq!(engine.charge(1), Err(EvalFailure::Cancelled));
    }

    #[test]
    fn retained_memory_and_compilation_tariffs_have_exact_boundaries() {
        let mut identities = crate::identity::IdentityCatalog::empty();
        let program = typed_hir::verify(
            ProgramInput::default(),
            &BuildAuthority::test_compiler_distribution(),
            &PoolAuthority::from_authenticated_scoped_factory(None),
            &mut identities,
            &Cancellation::new(),
        )
        .expect("empty program verifies");
        let cancellation = Cancellation::new();
        let mut engine = Engine::new(&program, &cancellation);
        engine
            .retain(MEMORY_LIMIT)
            .expect("root memory ceiling is inclusive");
        assert_eq!(engine.peak_memory, MEMORY_LIMIT);
        assert_eq!(
            engine.retain(1),
            Err(EvalFailure::Limit {
                policy: LimitPolicy::RootMemory,
                ceiling: MEMORY_LIMIT,
                used: MEMORY_LIMIT + 1,
            })
        );

        let mut engine = Engine::new(&program, &cancellation);
        engine.compilation_fuel = COMPILATION_FUEL_LIMIT;
        assert_eq!(
            engine.charge(1),
            Err(EvalFailure::Limit {
                policy: LimitPolicy::CompilationFuel,
                ceiling: COMPILATION_FUEL_LIMIT,
                used: COMPILATION_FUEL_LIMIT + 1,
            })
        );
        assert_eq!(value_size(&Value::Text(Arc::from("abc"))), 19);
        assert_eq!(
            value_size(&Value::Array(Arc::from([
                Value::Integer {
                    kind: IntegerType::I64,
                    value: 1
                },
                Value::Bool(true),
            ]))),
            33
        );
        assert_eq!(
            value_size(&Value::BuiltinVariant {
                variant: BuiltinVariant::OptionSome,
                payload: Arc::from([Value::Bool(true)]),
            }),
            17
        );
    }
}
