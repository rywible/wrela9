#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::sync::Arc;

use xxhash_rust::xxh3::xxh3_128;

use crate::model::{
    BuildKind, BuiltinVariant, DefinitionId, FloatType, IntegerType, TestId, Type, VariantId,
};
use crate::typed_hir::{
    BinaryOperator, CallTarget, Expression, ExpressionKind, HirFunction, Literal, LocalId,
    Statement, VerifiedProgram,
};
use crate::{Cancellation, CanonicalValue, EvaluationOutcome, EvaluationReceipt, SourceRange};

pub(crate) const FUEL_LIMIT: u64 = 100_000;
const MEMORY_LIMIT: u64 = 1_048_576;
const CALL_DEPTH_LIMIT: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RejectKind {
    ConstantDependencyCycle,
    UnresolvedConstant,
    UnresolvedCall,
    ArgumentCount,
    ArgumentTypeMismatch,
    ReturnTypeMismatch,
    MissingLocal,
    InvalidUnaryOperand,
    PropagationRequiresResult,
    PropagatedError,
    ResultOkMissingPayload,
    InvalidBooleanOperator,
    BinaryTypeMismatch,
    AwaitNotEvaluatorEligible,
}

impl RejectKind {
    const fn code(self) -> &'static str {
        match self {
            Self::ConstantDependencyCycle => "constant_dependency_cycle",
            Self::UnresolvedConstant => "unresolved_constant",
            Self::UnresolvedCall => "unresolved_call",
            Self::ArgumentCount => "argument_count",
            Self::ArgumentTypeMismatch => "argument_type_mismatch",
            Self::ReturnTypeMismatch => "return_type_mismatch",
            Self::MissingLocal => "missing_local",
            Self::InvalidUnaryOperand => "invalid_unary_operand",
            Self::PropagationRequiresResult => "propagation_requires_result",
            Self::PropagatedError => "propagated_error",
            Self::ResultOkMissingPayload => "result_ok_missing_payload",
            Self::InvalidBooleanOperator => "invalid_boolean_operator",
            Self::BinaryTypeMismatch => "binary_type_mismatch",
            Self::AwaitNotEvaluatorEligible => "await_not_evaluator_eligible",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PanicKind {
    Explicit,
    IntegerOverflow,
    DivisionByZero,
}

impl PanicKind {
    const fn code(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::IntegerOverflow => "integer_overflow",
            Self::DivisionByZero => "division_by_zero",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LimitPolicy {
    RootFuel,
    RootMemory,
    CallDepth,
}

impl LimitPolicy {
    const fn code(self) -> &'static str {
        match self {
            Self::RootFuel => "root_fuel",
            Self::RootMemory => "root_memory",
            Self::CallDepth => "call_depth",
        }
    }
}

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

#[derive(Clone, Debug)]
enum Value {
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
    Array(Arc<[Value]>),
    BuiltinVariant {
        variant: BuiltinVariant,
        payload: Arc<[Value]>,
    },
    UserVariant {
        id: VariantId,
        type_display: Arc<str>,
        variant_display: Arc<str>,
        payload: Arc<[Value]>,
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

pub(crate) struct Engine<'a> {
    program: &'a VerifiedProgram,
    cancellation: &'a Cancellation,
    constant_values: BTreeMap<DefinitionId, Value>,
    evaluating_constants: Vec<DefinitionId>,
    fuel: u64,
    peak_memory: u64,
    constructions: Vec<Construction>,
    test_applications: Vec<TestId>,
    call_stack: Vec<(DefinitionId, String, SourceRange)>,
}

pub(crate) struct Run {
    pub(crate) outcome: EvaluationOutcome,
    pub(crate) receipt: EvaluationReceipt,
    pub(crate) constructions: Vec<Construction>,
    pub(crate) test_applications: Vec<TestId>,
    pub(crate) root_handle: Option<(BuildKind, u128)>,
}

#[derive(Clone, Debug)]
pub(crate) struct Construction {
    pub(crate) identity: u128,
    pub(crate) kind: BuildKind,
    pub(crate) site: SourceRange,
    pub(crate) edges: Vec<u128>,
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
            constructions: Vec::new(),
            test_applications: Vec::new(),
            call_stack: Vec::new(),
        }
    }

    pub(crate) fn evaluate_constant(&mut self, id: DefinitionId) -> Run {
        let result = self.constant(id);
        self.finish(result)
    }

    pub(crate) fn evaluate_function(&mut self, id: DefinitionId) -> Run {
        let result = self.call_function(id, Vec::new());
        self.finish(result)
    }

    pub(crate) fn evaluate_expression(&mut self, expression: &Expression) -> Run {
        let result = self.expression(expression, &BTreeMap::new());
        self.finish(result)
    }

    fn finish(&mut self, result: Result<Value, EvalFailure>) -> Run {
        let root_handle = match &result {
            Ok(Value::SymbolicHandle { kind, identity }) => Some((*kind, *identity)),
            _ => None,
        };
        Run {
            outcome: match result {
                Ok(value) => EvaluationOutcome::Completed(canonical(value)),
                Err(EvalFailure::Creator(kind)) => EvaluationOutcome::CreatorRejected {
                    kind: Arc::from(kind.code()),
                },
                Err(EvalFailure::Panic(kind, site)) => EvaluationOutcome::Panicked {
                    kind: Arc::from(kind.code()),
                    site,
                },
                Err(EvalFailure::Limit {
                    policy,
                    ceiling,
                    used,
                }) => EvaluationOutcome::LimitExceeded {
                    policy: Arc::from(policy.code()),
                    ceiling,
                    used,
                },
                Err(EvalFailure::Cancelled) => EvaluationOutcome::Cancelled,
                Err(EvalFailure::Defect(evidence)) => EvaluationOutcome::Defect { evidence },
            },
            receipt: EvaluationReceipt::new(
                self.program.fingerprint(),
                self.fuel,
                self.peak_memory,
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
        self.peak_memory = self.peak_memory.max(amount);
        if self.peak_memory > MEMORY_LIMIT {
            return Err(EvalFailure::Limit {
                policy: LimitPolicy::RootMemory,
                ceiling: MEMORY_LIMIT,
                used: self.peak_memory,
            });
        }
        Ok(())
    }

    fn constant(&mut self, id: DefinitionId) -> Result<Value, EvalFailure> {
        if let Some(value) = self.constant_values.get(&id) {
            return Ok(value.clone());
        }
        if self.evaluating_constants.contains(&id) {
            return Err(EvalFailure::Creator(RejectKind::ConstantDependencyCycle));
        }
        let constant = self
            .program
            .constants()
            .get(&id)
            .cloned()
            .ok_or(EvalFailure::Creator(RejectKind::UnresolvedConstant))?;
        self.evaluating_constants.push(id);
        let result = self.expression(&constant.expression, &BTreeMap::new());
        self.evaluating_constants.pop();
        let value = coerce(result?, &constant.type_)
            .ok_or(EvalFailure::Creator(RejectKind::ArgumentTypeMismatch))?;
        self.constant_values.insert(id, value.clone());
        Ok(value)
    }

    fn call_function(
        &mut self,
        id: DefinitionId,
        arguments: Vec<Value>,
    ) -> Result<Value, EvalFailure> {
        let specialization = self
            .program
            .default_specialization(id)
            .ok_or(EvalFailure::Creator(RejectKind::UnresolvedCall))?;
        self.call_specialization(specialization, arguments)
    }

    fn call_specialization(
        &mut self,
        id: crate::model::SpecializationId,
        arguments: Vec<Value>,
    ) -> Result<Value, EvalFailure> {
        self.charge(5)?;
        let function = self
            .program
            .specialization_function(id)
            .cloned()
            .ok_or(EvalFailure::Creator(RejectKind::UnresolvedCall))?;
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
        let locals = function
            .parameters
            .iter()
            .zip(arguments)
            .map(|((local, type_, _access), value)| {
                coerce(value, type_)
                    .map(|value| (*local, value))
                    .ok_or(EvalFailure::Creator(RejectKind::ArgumentTypeMismatch))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        self.call_stack
            .push((function.id, function.name.clone(), function.source.clone()));
        let result = self.execute(&function, locals);
        self.call_stack.pop();
        result
    }

    fn execute(
        &mut self,
        function: &HirFunction,
        mut locals: BTreeMap<LocalId, Value>,
    ) -> Result<Value, EvalFailure> {
        self.retain(u64::try_from(locals.len()).unwrap_or(u64::MAX) * 32)?;
        self.execute_block(function, &function.body, &mut locals)?
            .map_or(Ok(Value::Unit), Ok)
    }

    fn execute_block(
        &mut self,
        function: &HirFunction,
        statements: &[Statement],
        locals: &mut BTreeMap<LocalId, Value>,
    ) -> Result<Option<Value>, EvalFailure> {
        for statement in statements {
            self.charge(1)?;
            match statement {
                Statement::Return {
                    value: Some(expression),
                    ..
                } => {
                    return coerce(self.expression(expression, locals)?, &function.return_type)
                        .map(Some)
                        .ok_or(EvalFailure::Creator(RejectKind::ReturnTypeMismatch));
                }
                Statement::Return { value: None, .. } => return Ok(Some(Value::Unit)),
                Statement::Panic { value, source } => {
                    let _ = self.expression(value, locals)?;
                    return Err(EvalFailure::Panic(PanicKind::Explicit, source.clone()));
                }
                Statement::Initialize { place, value, .. } => {
                    let value = self.expression(value, locals)?;
                    locals.insert(place.local, value);
                }
                Statement::Evaluate(expression) => {
                    let _ = self.expression(expression, locals)?;
                }
                Statement::If {
                    condition,
                    then_branch,
                    else_branch,
                    ..
                } => {
                    let branch = match self.expression(condition, locals)? {
                        Value::Bool(true) => then_branch,
                        Value::Bool(false) => else_branch,
                        _ => {
                            return Err(EvalFailure::Defect(Arc::from(
                                "verified non-bool condition",
                            )));
                        }
                    };
                    if let Some(value) = self.execute_block(function, branch, locals)? {
                        return Ok(Some(value));
                    }
                }
                Statement::Pass(_) => {}
            }
        }
        Ok(None)
    }

    fn expression(
        &mut self,
        expression: &Expression,
        locals: &BTreeMap<LocalId, Value>,
    ) -> Result<Value, EvalFailure> {
        self.charge(1)?;
        match &expression.kind {
            ExpressionKind::Literal(value) => Ok(match value {
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
            }),
            ExpressionKind::Read(place) => locals
                .get(&place.local)
                .cloned()
                .ok_or(EvalFailure::Creator(RejectKind::MissingLocal)),
            ExpressionKind::Constant(id) => self.constant(*id),
            ExpressionKind::Array(values) => values
                .iter()
                .map(|value| self.expression(value, locals))
                .collect::<Result<Vec<_>, _>>()
                .map(|values| Value::Array(values.into())),
            ExpressionKind::Negate(value) => match self.expression(value, locals)? {
                Value::Integer { kind, value } => Ok(Value::Integer {
                    kind,
                    value: value.checked_neg().ok_or_else(|| {
                        EvalFailure::Panic(PanicKind::IntegerOverflow, expression.source.clone())
                    })?,
                }),
                Value::Float { kind, bits } => Ok(Value::Float {
                    kind,
                    bits: encode_float(kind, -decode_float(kind, bits)),
                }),
                _ => Err(EvalFailure::Creator(RejectKind::InvalidUnaryOperand)),
            },
            ExpressionKind::Await(_) => {
                Err(EvalFailure::Creator(RejectKind::AwaitNotEvaluatorEligible))
            }
            ExpressionKind::Propagate(value) => match self.expression(value, locals)? {
                Value::BuiltinVariant {
                    variant: BuiltinVariant::ResultOk,
                    payload,
                } => payload
                    .first()
                    .cloned()
                    .ok_or(EvalFailure::Creator(RejectKind::ResultOkMissingPayload)),
                Value::BuiltinVariant {
                    variant: BuiltinVariant::ResultErr,
                    ..
                } => Err(EvalFailure::Creator(RejectKind::PropagatedError)),
                _ => Err(EvalFailure::Creator(RejectKind::PropagationRequiresResult)),
            },
            ExpressionKind::Binary {
                operator,
                left,
                right,
            } => apply_binary(
                *operator,
                self.expression(left, locals)?,
                self.expression(right, locals)?,
                &expression.source,
            ),
            ExpressionKind::Call { target, arguments } => {
                let arguments = arguments
                    .iter()
                    .map(|argument| self.expression(argument, locals))
                    .collect::<Result<Vec<_>, _>>()?;
                self.call_target(target, arguments, &expression.source)
            }
        }
    }

    fn call_target(
        &mut self,
        target: &CallTarget,
        arguments: Vec<Value>,
        site: &SourceRange,
    ) -> Result<Value, EvalFailure> {
        match target {
            CallTarget::TemplateFunction(_) => Err(EvalFailure::Defect(Arc::from(
                "template call reached the concrete evaluator",
            ))),
            CallTarget::Function { specialization, .. } => {
                self.call_specialization(*specialization, arguments)
            }
            CallTarget::Build(kind) => self.construct(*kind, &arguments, site),
            CallTarget::BuiltinVariant(variant) => Ok(Value::BuiltinVariant {
                variant: *variant,
                payload: arguments.into(),
            }),
            CallTarget::UserVariant {
                id,
                type_display,
                variant_display,
            } => Ok(Value::UserVariant {
                id: *id,
                type_display: type_display.clone(),
                variant_display: variant_display.clone(),
                payload: arguments.into(),
            }),
            CallTarget::Test(id) => Ok(Value::TestApplication {
                id: *id,
                payload: arguments.into(),
            }),
        }
    }

    fn construct(
        &mut self,
        kind: BuildKind,
        arguments: &[Value],
        site: &SourceRange,
    ) -> Result<Value, EvalFailure> {
        self.charge(3)?;
        let coordinate = self.constructions.len();
        let mut key = b"wrela.construction\0\x02".to_vec();
        for (id, _, _) in &self.call_stack {
            key.extend_from_slice(&id.0.to_be_bytes());
        }
        key.push(match kind {
            BuildKind::Image => 1,
            BuildKind::Test => 2,
        });
        key.extend_from_slice(site.path().as_bytes());
        key.extend_from_slice(&site.start().to_be_bytes());
        key.extend_from_slice(&site.end().to_be_bytes());
        key.extend_from_slice(&u64::try_from(coordinate).unwrap_or(u64::MAX).to_be_bytes());
        let identity = xxh3_128(&key);
        let mut edges = Vec::new();
        for argument in arguments {
            collect_construction_edges(argument, &mut edges);
            if kind == BuildKind::Test {
                collect_test_applications(argument, &mut self.test_applications);
            }
        }
        self.constructions.push(Construction {
            identity,
            kind,
            site: site.clone(),
            edges,
        });
        Ok(Value::SymbolicHandle { kind, identity })
    }
}

fn collect_construction_edges(value: &Value, edges: &mut Vec<u128>) {
    match value {
        Value::SymbolicHandle { identity, .. } => edges.push(*identity),
        Value::Array(values)
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
        Value::Unit
        | Value::Bool(_)
        | Value::Integer { .. }
        | Value::Float { .. }
        | Value::Text(_) => {}
    }
}

fn collect_test_applications(value: &Value, applications: &mut Vec<TestId>) {
    match value {
        Value::TestApplication { id, payload } => {
            applications.push(*id);
            for value in &**payload {
                collect_test_applications(value, applications);
            }
        }
        Value::Array(values)
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
        Value::Unit
        | Value::Bool(_)
        | Value::Integer { .. }
        | Value::Float { .. }
        | Value::Text(_)
        | Value::SymbolicHandle { .. } => {}
    }
}

fn apply_binary(
    operator: BinaryOperator,
    left: Value,
    right: Value,
    site: &SourceRange,
) -> Result<Value, EvalFailure> {
    match (left, right) {
        (Value::Integer { kind, value: left }, Value::Integer { value: right, .. }) => {
            match operator {
                BinaryOperator::Add => checked_integer(kind, left.checked_add(right), site),
                BinaryOperator::Subtract => checked_integer(kind, left.checked_sub(right), site),
                BinaryOperator::Multiply => checked_integer(kind, left.checked_mul(right), site),
                BinaryOperator::Divide | BinaryOperator::Remainder if right == 0 => {
                    Err(EvalFailure::Panic(PanicKind::DivisionByZero, site.clone()))
                }
                BinaryOperator::Divide => checked_integer(kind, left.checked_div(right), site),
                BinaryOperator::Remainder => checked_integer(kind, left.checked_rem(right), site),
                BinaryOperator::Equal => Ok(Value::Bool(left == right)),
                BinaryOperator::NotEqual => Ok(Value::Bool(left != right)),
                BinaryOperator::Less => Ok(Value::Bool(left < right)),
                BinaryOperator::LessEqual => Ok(Value::Bool(left <= right)),
                BinaryOperator::Greater => Ok(Value::Bool(left > right)),
                BinaryOperator::GreaterEqual => Ok(Value::Bool(left >= right)),
            }
        }
        (Value::Bool(left), Value::Bool(right)) => match operator {
            BinaryOperator::Equal => Ok(Value::Bool(left == right)),
            BinaryOperator::NotEqual => Ok(Value::Bool(left != right)),
            _ => Err(EvalFailure::Creator(RejectKind::InvalidBooleanOperator)),
        },
        (Value::Float { kind, bits: left }, Value::Float { bits: right, .. }) => {
            apply_float(operator, kind, left, right)
        }
        _ => Err(EvalFailure::Creator(RejectKind::BinaryTypeMismatch)),
    }
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
        BinaryOperator::Equal => return Ok(Value::Bool(left == right)),
        BinaryOperator::NotEqual => return Ok(Value::Bool(left != right)),
        BinaryOperator::Less => return Ok(Value::Bool(left < right)),
        BinaryOperator::LessEqual => return Ok(Value::Bool(left <= right)),
        BinaryOperator::Greater => return Ok(Value::Bool(left > right)),
        BinaryOperator::GreaterEqual => return Ok(Value::Bool(left >= right)),
    };
    Ok(Value::Float {
        kind,
        bits: encode_float(kind, value),
    })
}

fn coerce(value: Value, expected: &Type) -> Option<Value> {
    match (value, expected) {
        (Value::Integer { value, .. }, Type::Integer(kind)) if kind.fits(value) => {
            Some(Value::Integer { kind: *kind, value })
        }
        (Value::Float { kind, bits }, Type::Float(expected)) => Some(Value::Float {
            kind: *expected,
            bits: encode_float(*expected, decode_float(kind, bits)),
        }),
        (value, Type::Parameter { .. } | Type::Infer) => Some(value),
        (value, expected) if value_matches(&value, expected) => Some(value),
        _ => None,
    }
}

fn value_matches(value: &Value, expected: &Type) -> bool {
    match (value, expected) {
        (Value::Unit, Type::Unit) | (Value::Bool(_), Type::Bool) | (Value::Text(_), Type::Text) => {
            true
        }
        (Value::Integer { kind, .. }, Type::Integer(expected)) => kind == expected,
        (Value::Float { kind, .. }, Type::Float(expected)) => kind == expected,
        (Value::Array(_), Type::Array(_)) => true,
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
            Value::TestApplication { .. },
            Type::Builtin(crate::model::BuiltinType::TestApplication),
        ) => true,
        (_, Type::Parameter { .. } | Type::Infer) => true,
        _ => false,
    }
}

fn canonical(value: Value) -> CanonicalValue {
    match value {
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
        Value::Array(values) => {
            CanonicalValue::Array(values.iter().cloned().map(canonical).collect())
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
        Value::TestApplication { id, payload } => CanonicalValue::Variant {
            type_name: Arc::from("TestApplication"),
            variant: Arc::from(format!("{:032x}", id.identity)),
            payload: payload.iter().cloned().map(canonical).collect(),
        },
        Value::SymbolicHandle { kind, identity } => CanonicalValue::SymbolicHandle {
            kind: Arc::from(kind.name()),
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
    use crate::typed_hir::{self, BuildAuthority, ProgramInput};

    #[test]
    fn logical_fuel_exhaustion_is_exact_and_host_time_independent() {
        let mut identities = crate::identity::IdentityCatalog::empty();
        let program = typed_hir::verify(
            ProgramInput::default(),
            &BuildAuthority::compiler_distribution(),
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
            &BuildAuthority::compiler_distribution(),
            &mut identities,
            &Cancellation::new(),
        )
        .expect("empty program verifies");
        let cancellation = Cancellation::new();
        cancellation.cancel();
        let mut engine = Engine::new(&program, &cancellation);
        assert_eq!(engine.charge(1), Err(EvalFailure::Cancelled));
    }
}
