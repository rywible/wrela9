use std::collections::BTreeMap;
use std::sync::Arc;

use xxhash_rust::xxh3::xxh3_128;

use crate::typed_hir::{
    BinaryOperator, BuildConstructor, CallTarget, Expression, ExpressionKind, HirFunction,
    Statement, VerifiedProgram,
};
use crate::{Cancellation, CanonicalValue, EvaluationOutcome, EvaluationReceipt, SourceRange};

pub(crate) const FUEL_LIMIT: u64 = 100_000;
const MEMORY_LIMIT: u64 = 1_048_576;
const CALL_DEPTH_LIMIT: usize = 32;

#[derive(Clone, Debug)]
pub(crate) struct Function {
    pub(crate) name: String,
    pub(crate) module: String,
    pub(crate) public: bool,
    pub(crate) parameters: Vec<(String, String)>,
    pub(crate) return_type: String,
    pub(crate) body: Vec<(u64, String)>,
    pub(crate) source: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) struct Constant {
    pub(crate) name: String,
    pub(crate) module: String,
    pub(crate) type_name: String,
    pub(crate) expression: String,
    pub(crate) source: SourceRange,
}

pub(crate) struct Engine<'a> {
    program: &'a VerifiedProgram,
    cancellation: &'a Cancellation,
    constant_values: BTreeMap<String, CanonicalValue>,
    evaluating_constants: Vec<String>,
    fuel: u64,
    peak_memory: u64,
    constructions: Vec<Construction>,
    test_applications: Vec<String>,
    call_stack: Vec<(String, String, SourceRange)>,
}

pub(crate) struct Run {
    pub(crate) outcome: EvaluationOutcome,
    pub(crate) receipt: EvaluationReceipt,
    pub(crate) constructions: Vec<Construction>,
    pub(crate) test_applications: Vec<String>,
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

    pub(crate) fn evaluate_constant(&mut self, name: &str) -> Run {
        let outcome = self
            .constant(name)
            .map_or_else(|outcome| outcome, EvaluationOutcome::Completed);
        self.finish(outcome)
    }

    pub(crate) fn evaluate_function(&mut self, name: &str) -> Run {
        let outcome = self
            .call_function(name, Vec::new())
            .map_or_else(|outcome| outcome, EvaluationOutcome::Completed);
        self.finish(outcome)
    }

    pub(crate) fn evaluate_expression(
        &mut self,
        expression: &Expression,
        site: &SourceRange,
    ) -> Run {
        let outcome = self
            .expression(expression, &BTreeMap::new(), site)
            .map_or_else(|outcome| outcome, EvaluationOutcome::Completed);
        self.finish(outcome)
    }

    fn finish(&mut self, outcome: EvaluationOutcome) -> Run {
        Run {
            outcome,
            receipt: EvaluationReceipt::new(
                self.program.fingerprint(),
                self.fuel,
                self.peak_memory,
            ),
            constructions: std::mem::take(&mut self.constructions),
            test_applications: std::mem::take(&mut self.test_applications),
        }
    }

    fn charge(&mut self, amount: u64) -> Result<(), EvaluationOutcome> {
        if self.cancellation.is_cancelled() {
            return Err(EvaluationOutcome::Cancelled);
        }
        self.fuel = self.fuel.saturating_add(amount);
        if self.fuel > FUEL_LIMIT {
            return Err(EvaluationOutcome::LimitExceeded {
                policy: Arc::from("root_fuel"),
                ceiling: FUEL_LIMIT,
                used: self.fuel,
            });
        }
        Ok(())
    }

    fn retain(&mut self, amount: u64) -> Result<(), EvaluationOutcome> {
        self.peak_memory = self.peak_memory.max(amount);
        if self.peak_memory > MEMORY_LIMIT {
            return Err(EvaluationOutcome::LimitExceeded {
                policy: Arc::from("root_memory"),
                ceiling: MEMORY_LIMIT,
                used: self.peak_memory,
            });
        }
        Ok(())
    }

    fn constant(&mut self, name: &str) -> Result<CanonicalValue, EvaluationOutcome> {
        if let Some(value) = self.constant_values.get(name) {
            return Ok(value.clone());
        }
        if self
            .evaluating_constants
            .iter()
            .any(|active| active == name)
        {
            return Err(rejected("constant_dependency_cycle"));
        }
        let constant = self
            .program
            .constants()
            .get(name)
            .cloned()
            .ok_or_else(|| rejected("unresolved_constant"))?;
        self.evaluating_constants.push(name.to_owned());
        self.call_stack.push((
            module_from_lookup(name),
            name.to_owned(),
            constant.source.clone(),
        ));
        let result = self.expression(&constant.expression, &BTreeMap::new(), &constant.source);
        self.call_stack.pop();
        self.evaluating_constants.pop();
        let value = coerce(result?, &constant.type_name)
            .ok_or_else(|| rejected("constant_type_mismatch"))?;
        self.constant_values.insert(name.to_owned(), value.clone());
        Ok(value)
    }

    fn call_function(
        &mut self,
        name: &str,
        arguments: Vec<CanonicalValue>,
    ) -> Result<CanonicalValue, EvaluationOutcome> {
        self.charge(5)?;
        let function = self
            .program
            .functions()
            .get(name)
            .cloned()
            .ok_or_else(|| rejected("unresolved_call"))?;
        if self.call_stack.len() == CALL_DEPTH_LIMIT {
            return Err(EvaluationOutcome::LimitExceeded {
                policy: Arc::from("call_depth"),
                ceiling: u64::try_from(CALL_DEPTH_LIMIT).expect("small limit"),
                used: u64::try_from(CALL_DEPTH_LIMIT + 1).expect("small limit"),
            });
        }
        if function.parameters.len() != arguments.len() {
            return Err(rejected("argument_count"));
        }
        let mut substitutions = BTreeMap::new();
        let converted = function
            .parameters
            .iter()
            .zip(arguments)
            .map(|((parameter, type_name), value)| {
                if generic_type_parameter(type_name) {
                    let concrete = value_type(&value);
                    if let Some(previous) =
                        substitutions.insert(type_name.clone(), concrete.clone())
                        && previous != concrete
                    {
                        return Err(rejected("generic_argument_conflict"));
                    }
                    Ok((parameter.clone(), value))
                } else {
                    coerce(value, type_name)
                        .map(|value| (parameter.clone(), value))
                        .ok_or_else(|| rejected("argument_type_mismatch"))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let locals = converted.into_iter().collect();
        let mut function = function;
        if let Some(concrete) = substitutions.get(&function.return_type) {
            function.return_type.clone_from(concrete);
        }
        self.call_stack.push((
            function.module.clone(),
            function.name.clone(),
            function.source.clone(),
        ));
        let result = self.execute(&function, locals);
        self.call_stack.pop();
        result
    }

    fn execute(
        &mut self,
        function: &HirFunction,
        mut locals: BTreeMap<String, CanonicalValue>,
    ) -> Result<CanonicalValue, EvaluationOutcome> {
        self.retain(u64::try_from(locals.len()).unwrap_or(u64::MAX) * 32)?;
        for statement in &*function.body {
            self.charge(1)?;
            match statement {
                Statement::Return(Some(expression)) => {
                    return coerce(
                        self.expression(expression, &locals, &function.source)?,
                        &function.return_type,
                    )
                    .ok_or_else(|| rejected("return_type_mismatch"));
                }
                Statement::Return(None) => return Ok(CanonicalValue::Unit),
                Statement::Panic(expression) => {
                    let _ = self.expression(expression, &locals, &function.source)?;
                    return Err(EvaluationOutcome::Panicked {
                        kind: Arc::from("explicit"),
                        site: function.source.clone(),
                    });
                }
                Statement::Assign { name, value } => {
                    let value = self.expression(value, &locals, &function.source)?;
                    locals.insert(name.clone(), value);
                }
                Statement::Evaluate(expression) => {
                    let _ = self.expression(expression, &locals, &function.source)?;
                }
                Statement::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    let condition = self.expression(condition, &locals, &function.source)?;
                    let branch = match condition {
                        CanonicalValue::Bool(true) => then_branch,
                        CanonicalValue::Bool(false) => else_branch,
                        _ => return Err(rejected("if_condition_requires_bool")),
                    };
                    if let Some(value) = self.execute_block(function, branch, &mut locals)? {
                        return Ok(value);
                    }
                }
                Statement::Pass => {}
            }
        }
        Ok(CanonicalValue::Unit)
    }

    fn execute_block(
        &mut self,
        function: &HirFunction,
        statements: &[Statement],
        locals: &mut BTreeMap<String, CanonicalValue>,
    ) -> Result<Option<CanonicalValue>, EvaluationOutcome> {
        for statement in statements {
            self.charge(1)?;
            match statement {
                Statement::Return(Some(expression)) => {
                    return coerce(
                        self.expression(expression, locals, &function.source)?,
                        &function.return_type,
                    )
                    .map(Some)
                    .ok_or_else(|| rejected("return_type_mismatch"));
                }
                Statement::Return(None) => return Ok(Some(CanonicalValue::Unit)),
                Statement::Panic(expression) => {
                    let _ = self.expression(expression, locals, &function.source)?;
                    return Err(EvaluationOutcome::Panicked {
                        kind: Arc::from("explicit"),
                        site: function.source.clone(),
                    });
                }
                Statement::Assign { name, value } => {
                    let value = self.expression(value, locals, &function.source)?;
                    locals.insert(name.clone(), value);
                }
                Statement::Evaluate(expression) => {
                    let _ = self.expression(expression, locals, &function.source)?;
                }
                Statement::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    let branch = match self.expression(condition, locals, &function.source)? {
                        CanonicalValue::Bool(true) => then_branch,
                        CanonicalValue::Bool(false) => else_branch,
                        _ => return Err(rejected("if_condition_requires_bool")),
                    };
                    if let Some(value) = self.execute_block(function, branch, locals)? {
                        return Ok(Some(value));
                    }
                }
                Statement::Pass => {}
            }
        }
        Ok(None)
    }

    fn expression(
        &mut self,
        expression: &Expression,
        locals: &BTreeMap<String, CanonicalValue>,
        site: &SourceRange,
    ) -> Result<CanonicalValue, EvaluationOutcome> {
        self.charge(1)?;
        match &expression.kind {
            ExpressionKind::Literal(value) => Ok(value.clone()),
            ExpressionKind::Local(name) => locals
                .get(name)
                .cloned()
                .ok_or_else(|| rejected("missing_local")),
            ExpressionKind::Constant(name) => self.constant(name),
            ExpressionKind::Array(values) => values
                .iter()
                .map(|value| self.expression(value, locals, site))
                .collect::<Result<Vec<_>, _>>()
                .map(|values| CanonicalValue::Array(values.into())),
            ExpressionKind::Negate(value) => match self.expression(value, locals, site)? {
                CanonicalValue::Integer { type_name, value } => Ok(CanonicalValue::Integer {
                    type_name,
                    value: value
                        .checked_neg()
                        .ok_or_else(|| panic_at("integer_overflow", site))?,
                }),
                CanonicalValue::Float { type_name, bits } => Ok(CanonicalValue::Float {
                    bits: encode_float(&type_name, -decode_float(&type_name, bits)),
                    type_name,
                }),
                _ => Err(rejected("invalid_unary_operand")),
            },
            ExpressionKind::Propagate(value) => match self.expression(value, locals, site)? {
                CanonicalValue::Variant {
                    type_name,
                    variant,
                    payload,
                } if type_name.as_ref() == "Result" && variant.as_ref() == "Ok" => payload
                    .first()
                    .cloned()
                    .ok_or_else(|| rejected("result_ok_missing_payload")),
                CanonicalValue::Variant {
                    type_name, variant, ..
                } if type_name.as_ref() == "Result" && variant.as_ref() == "Err" => {
                    Err(rejected("propagated_error"))
                }
                _ => Err(rejected("propagation_requires_result")),
            },
            ExpressionKind::Binary {
                operator,
                left,
                right,
            } => apply_binary(
                *operator,
                self.expression(left, locals, site)?,
                self.expression(right, locals, site)?,
                site,
            ),
            ExpressionKind::Call { target, arguments } => {
                let arguments = arguments
                    .iter()
                    .map(|argument| self.expression(argument, locals, site))
                    .collect::<Result<Vec<_>, _>>()?;
                self.call_target(target, arguments, site)
            }
        }
    }

    fn call_target(
        &mut self,
        target: &CallTarget,
        arguments: Vec<CanonicalValue>,
        site: &SourceRange,
    ) -> Result<CanonicalValue, EvaluationOutcome> {
        match target {
            CallTarget::Function(name) => self.call_function(name, arguments),
            CallTarget::Build(constructor) => self.construct(*constructor, &arguments, site),
            CallTarget::Variant { type_name, variant } => Ok(CanonicalValue::Variant {
                type_name: Arc::from(type_name.as_str()),
                variant: Arc::from(variant.as_str()),
                payload: arguments.into(),
            }),
            CallTarget::TestApplication(name) => {
                self.test_applications.push(name.clone());
                Ok(CanonicalValue::Variant {
                    type_name: Arc::from("TestApplication"),
                    variant: Arc::from(name.as_str()),
                    payload: arguments.into(),
                })
            }
        }
    }

    fn construct(
        &mut self,
        constructor: BuildConstructor,
        arguments: &[CanonicalValue],
        site: &SourceRange,
    ) -> Result<CanonicalValue, EvaluationOutcome> {
        self.charge(3)?;
        let kind = constructor.kind();
        let coordinate = self.constructions.len();
        let call_path = self
            .call_stack
            .iter()
            .map(|(_, name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join("/");
        let mut key = b"wrela.construction\0\x01".to_vec();
        for part in [
            call_path.as_bytes(),
            kind.as_bytes(),
            site.path().as_bytes(),
        ] {
            key.extend_from_slice(&u64::try_from(part.len()).unwrap_or(u64::MAX).to_be_bytes());
            key.extend_from_slice(part);
        }
        key.extend_from_slice(&site.start().to_be_bytes());
        key.extend_from_slice(&site.end().to_be_bytes());
        key.extend_from_slice(&u64::try_from(coordinate).unwrap_or(u64::MAX).to_be_bytes());
        let identity = xxh3_128(&key);
        let mut edges = Vec::new();
        for argument in arguments {
            collect_construction_edges(argument, &mut edges);
        }
        self.constructions.push(Construction {
            identity,
            kind: kind.to_owned(),
            site: site.clone(),
            edges,
        });
        Ok(CanonicalValue::SymbolicHandle {
            kind: Arc::from(kind),
            identity,
        })
    }
}

fn collect_construction_edges(value: &CanonicalValue, edges: &mut Vec<u128>) {
    match value {
        CanonicalValue::SymbolicHandle { identity, .. } => edges.push(*identity),
        CanonicalValue::Array(values)
        | CanonicalValue::Tuple(values)
        | CanonicalValue::Variant {
            payload: values, ..
        } => {
            for value in &**values {
                collect_construction_edges(value, edges);
            }
        }
        CanonicalValue::Unit
        | CanonicalValue::Bool(_)
        | CanonicalValue::Integer { .. }
        | CanonicalValue::Float { .. }
        | CanonicalValue::Text(_) => {}
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Construction {
    pub(crate) identity: u128,
    pub(crate) kind: String,
    pub(crate) site: SourceRange,
    pub(crate) edges: Vec<u128>,
}

fn apply_binary(
    operator: BinaryOperator,
    left: CanonicalValue,
    right: CanonicalValue,
    site: &SourceRange,
) -> Result<CanonicalValue, EvaluationOutcome> {
    match (left, right) {
        (
            CanonicalValue::Integer {
                type_name,
                value: left,
            },
            CanonicalValue::Integer { value: right, .. },
        ) => match operator {
            BinaryOperator::Add => checked_integer(type_name, left.checked_add(right), site),
            BinaryOperator::Subtract => checked_integer(type_name, left.checked_sub(right), site),
            BinaryOperator::Multiply => checked_integer(type_name, left.checked_mul(right), site),
            BinaryOperator::Divide | BinaryOperator::Remainder if right == 0 => {
                Err(panic_at("division_by_zero", site))
            }
            BinaryOperator::Divide => checked_integer(type_name, left.checked_div(right), site),
            BinaryOperator::Remainder => checked_integer(type_name, left.checked_rem(right), site),
            BinaryOperator::Equal => Ok(CanonicalValue::Bool(left == right)),
            BinaryOperator::NotEqual => Ok(CanonicalValue::Bool(left != right)),
            BinaryOperator::Less => Ok(CanonicalValue::Bool(left < right)),
            BinaryOperator::LessEqual => Ok(CanonicalValue::Bool(left <= right)),
            BinaryOperator::Greater => Ok(CanonicalValue::Bool(left > right)),
            BinaryOperator::GreaterEqual => Ok(CanonicalValue::Bool(left >= right)),
        },
        (CanonicalValue::Bool(left), CanonicalValue::Bool(right)) => match operator {
            BinaryOperator::Equal => Ok(CanonicalValue::Bool(left == right)),
            BinaryOperator::NotEqual => Ok(CanonicalValue::Bool(left != right)),
            _ => Err(rejected("invalid_boolean_operator")),
        },
        (
            CanonicalValue::Float {
                type_name,
                bits: left,
            },
            CanonicalValue::Float { bits: right, .. },
        ) => apply_float(operator, type_name, left, right),
        _ => Err(rejected("binary_type_mismatch")),
    }
}

fn checked_integer(
    type_name: Arc<str>,
    value: Option<i128>,
    site: &SourceRange,
) -> Result<CanonicalValue, EvaluationOutcome> {
    let value = value.ok_or_else(|| panic_at("integer_overflow", site))?;
    if !integer_fits(&type_name, value) {
        return Err(panic_at("integer_overflow", site));
    }
    Ok(CanonicalValue::Integer { type_name, value })
}

fn apply_float(
    operator: BinaryOperator,
    type_name: Arc<str>,
    left_bits: u64,
    right_bits: u64,
) -> Result<CanonicalValue, EvaluationOutcome> {
    let left = decode_float(&type_name, left_bits);
    let right = decode_float(&type_name, right_bits);
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
    };
    Ok(CanonicalValue::Float {
        bits: encode_float(&type_name, value),
        type_name,
    })
}

fn panic_at(kind: &'static str, site: &SourceRange) -> EvaluationOutcome {
    EvaluationOutcome::Panicked {
        kind: Arc::from(kind),
        site: site.clone(),
    }
}

fn rejected(kind: &'static str) -> EvaluationOutcome {
    EvaluationOutcome::CreatorRejected {
        kind: Arc::from(kind),
    }
}

fn type_matches(value: &CanonicalValue, expected: &str) -> bool {
    let expected = expected.trim();
    match value {
        CanonicalValue::Unit => expected.is_empty() || expected == "()",
        CanonicalValue::Bool(_) => expected == "bool",
        CanonicalValue::Integer { type_name, .. }
        | CanonicalValue::Float { type_name, .. }
        | CanonicalValue::Variant { type_name, .. }
        | CanonicalValue::SymbolicHandle {
            kind: type_name, ..
        } => expected == type_name.as_ref() || expected.starts_with("Result["),
        CanonicalValue::Text(_) => expected == "Text",
        CanonicalValue::Tuple(_) => expected.starts_with('('),
        CanonicalValue::Array(_) => expected.starts_with('['),
    }
}

fn coerce(value: CanonicalValue, expected: &str) -> Option<CanonicalValue> {
    match value {
        CanonicalValue::Integer { value, .. } if integer_fits(expected, value) => {
            Some(CanonicalValue::Integer {
                type_name: Arc::from(expected),
                value,
            })
        }
        CanonicalValue::Float { type_name, bits } if matches!(expected, "f16" | "f32" | "f64") => {
            Some(CanonicalValue::Float {
                type_name: Arc::from(expected),
                bits: encode_float(expected, decode_float(&type_name, bits)),
            })
        }
        value if type_matches(&value, expected) => Some(value),
        _ => None,
    }
}

fn encode_float(type_name: &str, value: f64) -> u64 {
    if value.is_nan() {
        return match type_name {
            "f16" => 0x7e00,
            "f32" => 0x7fc0_0000,
            _ => 0x7ff8_0000_0000_0000,
        };
    }
    match type_name {
        "f16" => u64::from(half::f16::from_f64(value).to_bits()),
        "f32" => u64::from((value as f32).to_bits()),
        _ => value.to_bits(),
    }
}

fn decode_float(type_name: &str, bits: u64) -> f64 {
    match type_name {
        "f16" => half::f16::from_bits(bits as u16).to_f64(),
        "f32" => f32::from_bits(bits as u32).into(),
        _ => f64::from_bits(bits),
    }
}

fn integer_fits(type_name: &str, value: i128) -> bool {
    match type_name {
        "u8" => u8::try_from(value).is_ok(),
        "u16" => u16::try_from(value).is_ok(),
        "u32" => u32::try_from(value).is_ok(),
        "u64" => u64::try_from(value).is_ok(),
        "i8" => i8::try_from(value).is_ok(),
        "i16" => i16::try_from(value).is_ok(),
        "i32" => i32::try_from(value).is_ok(),
        "i64" => i64::try_from(value).is_ok(),
        _ => false,
    }
}

fn generic_type_parameter(type_name: &str) -> bool {
    let mut characters = type_name.chars();
    matches!(characters.next(), Some('A'..='Z'))
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
        && !matches!(type_name, "Text" | "Bytes" | "Image" | "Result" | "Option")
}

fn value_type(value: &CanonicalValue) -> String {
    match value {
        CanonicalValue::Unit => "()".to_owned(),
        CanonicalValue::Bool(_) => "bool".to_owned(),
        CanonicalValue::Integer { type_name, .. }
        | CanonicalValue::Float { type_name, .. }
        | CanonicalValue::Variant { type_name, .. }
        | CanonicalValue::SymbolicHandle {
            kind: type_name, ..
        } => type_name.to_string(),
        CanonicalValue::Text(_) => "Text".to_owned(),
        CanonicalValue::Tuple(_) => "tuple".to_owned(),
        CanonicalValue::Array(_) => "array".to_owned(),
    }
}

fn module_from_lookup(lookup: &str) -> String {
    lookup.split(['.', '|']).next().unwrap_or(lookup).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typed_hir;
    use std::collections::BTreeSet;

    #[test]
    fn logical_fuel_exhaustion_is_exact_and_host_time_independent() {
        let program = typed_hir::verify(
            BTreeMap::new(),
            BTreeMap::new(),
            &BTreeSet::new(),
            &typed_hir::BuildAuthority::compiler_distribution(),
            &BTreeSet::new(),
            &Cancellation::new(),
        )
        .expect("empty program verifies");
        let cancellation = Cancellation::new();
        let mut engine = Engine::new(&program, &cancellation);
        engine.charge(FUEL_LIMIT).expect("at ceiling is admitted");
        assert_eq!(
            engine.charge(1),
            Err(EvaluationOutcome::LimitExceeded {
                policy: Arc::from("root_fuel"),
                ceiling: FUEL_LIMIT,
                used: FUEL_LIMIT + 1,
            })
        );
    }

    #[test]
    fn cancellation_is_polled_during_evaluation() {
        let program = typed_hir::verify(
            BTreeMap::new(),
            BTreeMap::new(),
            &BTreeSet::new(),
            &typed_hir::BuildAuthority::compiler_distribution(),
            &BTreeSet::new(),
            &Cancellation::new(),
        )
        .expect("empty program verifies");
        let cancellation = Cancellation::new();
        let mut engine = Engine::new(&program, &cancellation);
        cancellation.cancel();
        assert_eq!(engine.charge(1), Err(EvaluationOutcome::Cancelled));
    }
}
