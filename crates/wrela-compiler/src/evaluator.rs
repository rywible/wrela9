use std::collections::BTreeMap;
use std::sync::Arc;

use xxhash_rust::xxh3::xxh3_128;

use crate::typed_hir::VerifiedProgram;
use crate::{CanonicalValue, EvaluationOutcome, EvaluationReceipt, SourceRange};

pub(crate) const FUEL_LIMIT: u64 = 100_000;
const MEMORY_LIMIT: u64 = 1_048_576;
const CALL_DEPTH_LIMIT: usize = 32;

#[derive(Clone, Debug)]
pub(crate) struct Function {
    pub(crate) name: String,
    pub(crate) public: bool,
    pub(crate) parameters: Vec<(String, String)>,
    pub(crate) return_type: String,
    pub(crate) body: Vec<(usize, String)>,
    pub(crate) source: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) struct Constant {
    pub(crate) name: String,
    pub(crate) type_name: String,
    pub(crate) expression: String,
    pub(crate) source: SourceRange,
}

pub(crate) struct Engine<'a> {
    program: &'a VerifiedProgram,
    constants: &'a BTreeMap<String, Constant>,
    constant_values: BTreeMap<String, CanonicalValue>,
    evaluating_constants: Vec<String>,
    fuel: u64,
    peak_memory: u64,
    constructions: Vec<(u128, String, SourceRange)>,
    call_stack: Vec<(String, SourceRange)>,
    test_applications: std::collections::BTreeSet<String>,
}

pub(crate) struct Run {
    pub(crate) outcome: EvaluationOutcome,
    pub(crate) receipt: EvaluationReceipt,
    pub(crate) constructions: Vec<(u128, String, SourceRange)>,
}

impl<'a> Engine<'a> {
    pub(crate) fn new(
        program: &'a VerifiedProgram,
        constants: &'a BTreeMap<String, Constant>,
    ) -> Self {
        Self {
            program,
            constants,
            constant_values: BTreeMap::new(),
            evaluating_constants: Vec::new(),
            fuel: 0,
            peak_memory: 0,
            constructions: Vec::new(),
            call_stack: Vec::new(),
            test_applications: std::collections::BTreeSet::new(),
        }
    }

    pub(crate) fn with_test_applications(
        mut self,
        applications: impl IntoIterator<Item = String>,
    ) -> Self {
        self.test_applications.extend(applications);
        self
    }

    pub(crate) fn evaluate_constant(&mut self, name: &str) -> Run {
        let outcome = match self.constant(name) {
            Ok(value) => EvaluationOutcome::Completed(value),
            Err(outcome) => outcome,
        };
        self.finish(outcome)
    }

    pub(crate) fn evaluate_function(&mut self, name: &str) -> Run {
        let outcome = match self.call(name, Vec::new()) {
            Ok(value) => EvaluationOutcome::Completed(value),
            Err(outcome) => outcome,
        };
        self.finish(outcome)
    }

    pub(crate) fn evaluate_expression(&mut self, expression: &str, site: &SourceRange) -> Run {
        let outcome = match self.expression(expression, &BTreeMap::new(), site) {
            Ok(value) => EvaluationOutcome::Completed(value),
            Err(outcome) => outcome,
        };
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
        }
    }

    fn charge(&mut self, amount: u64) -> Result<(), EvaluationOutcome> {
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
            return Err(EvaluationOutcome::CreatorRejected {
                kind: Arc::from("constant_dependency_cycle"),
            });
        }
        let Some(constant) = self.constants.get(name) else {
            return Err(EvaluationOutcome::CreatorRejected {
                kind: Arc::from("unresolved_constant"),
            });
        };
        self.evaluating_constants.push(name.to_owned());
        let result = self.expression(&constant.expression, &BTreeMap::new(), &constant.source);
        self.evaluating_constants.pop();
        let value = coerce(result?, &constant.type_name).ok_or_else(|| {
            EvaluationOutcome::CreatorRejected {
                kind: Arc::from("constant_type_mismatch"),
            }
        })?;
        if !type_matches(&value, &constant.type_name) {
            return Err(EvaluationOutcome::CreatorRejected {
                kind: Arc::from("constant_type_mismatch"),
            });
        }
        self.constant_values.insert(name.to_owned(), value.clone());
        Ok(value)
    }

    fn call(
        &mut self,
        name: &str,
        arguments: Vec<CanonicalValue>,
    ) -> Result<CanonicalValue, EvaluationOutcome> {
        self.charge(5)?;
        if name == "Image.new" || name == "Test.new" {
            let kind = name.strip_suffix(".new").expect("known constructor");
            let coordinate = self.constructions.len();
            let call_path = self
                .call_stack
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join("/");
            let site = self.call_stack.last().map_or_else(
                || SourceRange::new("src/image.wr", 0, 0),
                |(_, site)| site.clone(),
            );
            let key = format!(
                "wrela.construction.v1|{call_path}|{name}|{}:{}|{coordinate}",
                site.start(),
                site.end()
            );
            let identity = xxh3_128(key.as_bytes());
            self.constructions
                .push((identity, kind.to_owned(), site.clone()));
            return Ok(CanonicalValue::SymbolicHandle {
                kind: Arc::from(kind),
                identity,
            });
        }
        if let Some((type_name, variant)) = name.split_once('.')
            && !self.program.functions().contains_key(name)
            && type_name
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_uppercase)
        {
            return Ok(CanonicalValue::Variant {
                type_name: Arc::from(type_name),
                variant: Arc::from(variant),
                payload: arguments.into(),
            });
        }
        if self.test_applications.contains(name) {
            return Ok(CanonicalValue::Variant {
                type_name: Arc::from("TestApplication"),
                variant: Arc::from(name),
                payload: arguments.into(),
            });
        }
        let Some(function) = self.program.functions().get(name).cloned() else {
            return Err(EvaluationOutcome::CreatorRejected {
                kind: Arc::from("unresolved_call"),
            });
        };
        if self.call_stack.len() == CALL_DEPTH_LIMIT {
            return Err(EvaluationOutcome::LimitExceeded {
                policy: Arc::from("call_depth"),
                ceiling: u64::try_from(CALL_DEPTH_LIMIT).expect("small limit"),
                used: u64::try_from(CALL_DEPTH_LIMIT + 1).expect("small limit"),
            });
        }
        if function.parameters.len() != arguments.len() {
            return Err(EvaluationOutcome::CreatorRejected {
                kind: Arc::from("argument_count"),
            });
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
        let locals = converted.into_iter().collect::<BTreeMap<_, _>>();
        let mut function = function;
        if let Some(concrete) = substitutions.get(&function.return_type) {
            function.return_type.clone_from(concrete);
        }
        self.call_stack
            .push((function.name.clone(), function.source.clone()));
        let result = self.execute(&function, locals);
        self.call_stack.pop();
        result
    }

    fn execute(
        &mut self,
        function: &Function,
        mut locals: BTreeMap<String, CanonicalValue>,
    ) -> Result<CanonicalValue, EvaluationOutcome> {
        self.retain(u64::try_from(locals.len()).unwrap_or(u64::MAX) * 32)?;
        for (_, statement) in &function.body {
            self.charge(1)?;
            let statement = statement.trim();
            if let Some(expression) = statement.strip_prefix("return ") {
                let value = coerce(
                    self.expression(expression, &locals, &function.source)?,
                    &function.return_type,
                )
                .ok_or_else(|| rejected("return_type_mismatch"))?;
                if !type_matches(&value, &function.return_type) {
                    return Err(EvaluationOutcome::CreatorRejected {
                        kind: Arc::from("return_type_mismatch"),
                    });
                }
                return Ok(value);
            }
            if statement == "return" {
                return Ok(CanonicalValue::Unit);
            }
            if let Some(expression) = statement.strip_prefix("panic ") {
                let _ = self.expression(expression, &locals, &function.source)?;
                return Err(EvaluationOutcome::Panicked {
                    kind: Arc::from("explicit"),
                    site: function.source.clone(),
                });
            }
            if let Some((name, expression)) = statement.split_once(" = ")
                && valid_name(name)
            {
                let value = self.expression(expression, &locals, &function.source)?;
                locals.insert(name.to_owned(), value);
            }
        }
        Ok(CanonicalValue::Unit)
    }

    fn expression(
        &mut self,
        source: &str,
        locals: &BTreeMap<String, CanonicalValue>,
        site: &SourceRange,
    ) -> Result<CanonicalValue, EvaluationOutcome> {
        let tokens = tokenize(source).map_err(|_| EvaluationOutcome::CreatorRejected {
            kind: Arc::from("invalid_expression"),
        })?;
        let mut parser = Parser {
            tokens: &tokens,
            index: 0,
            engine: self,
            locals,
            site,
        };
        let value = parser.parse_expression(0)?;
        if parser.index != tokens.len() {
            return Err(EvaluationOutcome::CreatorRejected {
                kind: Arc::from("trailing_expression_tokens"),
            });
        }
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Name(String),
    Integer(i128, Option<String>),
    Float(f64, Option<String>),
    Text(String),
    Symbol(&'static str),
}

fn tokenize(source: &str) -> Result<Vec<Token>, ()> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            byte if byte.is_ascii_whitespace() => index += 1,
            b'0'..=b'9' => {
                let start = index;
                index += 1;
                while bytes
                    .get(index)
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                {
                    index += 1;
                }
                if !["0x", "0X", "0b", "0B", "0o", "0O"]
                    .iter()
                    .any(|prefix| source[start..index].starts_with(prefix))
                    && bytes.get(index) == Some(&b'.')
                    && bytes.get(index + 1).is_some_and(u8::is_ascii_digit)
                {
                    index += 1;
                    while bytes
                        .get(index)
                        .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'_')
                    {
                        index += 1;
                    }
                    if matches!(bytes.get(index), Some(b'e' | b'E')) {
                        index += 1;
                        if matches!(bytes.get(index), Some(b'+' | b'-')) {
                            index += 1;
                        }
                        while bytes
                            .get(index)
                            .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'_')
                        {
                            index += 1;
                        }
                    }
                    while bytes.get(index).is_some_and(u8::is_ascii_alphanumeric) {
                        index += 1;
                    }
                    let (value, suffix) = parse_float_literal(&source[start..index])?;
                    tokens.push(Token::Float(value, suffix));
                } else {
                    let (value, suffix) = parse_integer_literal(&source[start..index])?;
                    tokens.push(Token::Integer(value, suffix));
                }
            }
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => {
                let start = index;
                index += 1;
                while bytes
                    .get(index)
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.'))
                {
                    index += 1;
                }
                tokens.push(Token::Name(source[start..index].to_owned()));
            }
            b'"' => {
                let start = index + 1;
                index += 1;
                while bytes.get(index).is_some_and(|byte| *byte != b'"') {
                    index += 1;
                }
                if index == bytes.len() {
                    return Err(());
                }
                tokens.push(Token::Text(source[start..index].to_owned()));
                index += 1;
            }
            _ => {
                let remaining = &source[index..];
                let symbol = [
                    "==", "!=", "<=", ">=", "<<", ">>", "+", "-", "*", "/", "%", "<", ">", "(",
                    ")", "[", "]", ",", "=",
                ]
                .into_iter()
                .find(|symbol| remaining.starts_with(symbol))
                .ok_or(())?;
                tokens.push(Token::Symbol(symbol));
                index += symbol.len();
            }
        }
    }
    Ok(tokens)
}

struct Parser<'a, 'b> {
    tokens: &'a [Token],
    index: usize,
    engine: &'a mut Engine<'b>,
    locals: &'a BTreeMap<String, CanonicalValue>,
    site: &'a SourceRange,
}

impl Parser<'_, '_> {
    fn parse_expression(
        &mut self,
        minimum_precedence: u8,
    ) -> Result<CanonicalValue, EvaluationOutcome> {
        self.engine.charge(1)?;
        let mut left = self.parse_primary()?;
        while let Some((operator, precedence)) = self.binary_operator() {
            if precedence < minimum_precedence {
                break;
            }
            self.index += 1;
            let right = self.parse_expression(precedence + 1)?;
            left = apply_binary(operator, left, right, self.site)?;
        }
        Ok(left)
    }

    fn parse_primary(&mut self) -> Result<CanonicalValue, EvaluationOutcome> {
        let Some(token) = self.tokens.get(self.index).cloned() else {
            return Err(rejected("missing_expression"));
        };
        self.index += 1;
        match token {
            Token::Integer(value, suffix) => Ok(CanonicalValue::Integer {
                type_name: suffix.unwrap_or_else(|| "i64".to_owned()).into(),
                value,
            }),
            Token::Float(value, suffix) => Ok(CanonicalValue::Float {
                type_name: suffix.clone().unwrap_or_else(|| "f64".to_owned()).into(),
                bits: encode_float(&suffix.unwrap_or_else(|| "f64".to_owned()), value),
            }),
            Token::Text(value) => Ok(CanonicalValue::Text(value.into())),
            Token::Name(name) if name == "true" => Ok(CanonicalValue::Bool(true)),
            Token::Name(name) if name == "false" => Ok(CanonicalValue::Bool(false)),
            Token::Name(name) if self.tokens.get(self.index) == Some(&Token::Symbol("(")) => {
                self.index += 1;
                let mut arguments = Vec::new();
                while self.tokens.get(self.index) != Some(&Token::Symbol(")")) {
                    if let (Some(Token::Name(_)), Some(Token::Symbol("="))) =
                        (self.tokens.get(self.index), self.tokens.get(self.index + 1))
                    {
                        self.index += 2;
                    }
                    arguments.push(self.parse_expression(0)?);
                    if self.tokens.get(self.index) == Some(&Token::Symbol(",")) {
                        self.index += 1;
                    } else {
                        break;
                    }
                }
                if self.tokens.get(self.index) != Some(&Token::Symbol(")")) {
                    return Err(rejected("missing_call_closer"));
                }
                self.index += 1;
                self.engine.call(&name, arguments)
            }
            Token::Name(name) => self
                .locals
                .get(&name)
                .cloned()
                .map(Ok)
                .unwrap_or_else(|| self.engine.constant(&name)),
            Token::Symbol("(") => {
                if self.tokens.get(self.index) == Some(&Token::Symbol(")")) {
                    self.index += 1;
                    return Ok(CanonicalValue::Unit);
                }
                let value = self.parse_expression(0)?;
                if self.tokens.get(self.index) != Some(&Token::Symbol(")")) {
                    return Err(rejected("missing_group_closer"));
                }
                self.index += 1;
                Ok(value)
            }
            Token::Symbol("[") => {
                let mut values = Vec::new();
                while self.tokens.get(self.index) != Some(&Token::Symbol("]")) {
                    values.push(self.parse_expression(0)?);
                    if self.tokens.get(self.index) == Some(&Token::Symbol(",")) {
                        self.index += 1;
                    } else {
                        break;
                    }
                }
                if self.tokens.get(self.index) != Some(&Token::Symbol("]")) {
                    return Err(rejected("missing_array_closer"));
                }
                self.index += 1;
                Ok(CanonicalValue::Array(values.into()))
            }
            Token::Symbol("-") => match self.parse_expression(12)? {
                CanonicalValue::Integer { type_name, value } => Ok(CanonicalValue::Integer {
                    type_name,
                    value: value
                        .checked_neg()
                        .ok_or_else(|| panic_at("integer_overflow", self.site))?,
                }),
                CanonicalValue::Float { type_name, bits } => Ok(CanonicalValue::Float {
                    bits: encode_float(&type_name, -decode_float(&type_name, bits)),
                    type_name,
                }),
                _ => Err(rejected("invalid_unary_operand")),
            },
            _ => Err(rejected("invalid_primary")),
        }
    }

    fn binary_operator(&self) -> Option<(&'static str, u8)> {
        let Token::Symbol(operator) = self.tokens.get(self.index)? else {
            return None;
        };
        let precedence = match *operator {
            "==" | "!=" | "<" | "<=" | ">" | ">=" => 5,
            "+" | "-" => 10,
            "*" | "/" | "%" => 11,
            _ => return None,
        };
        Some((operator, precedence))
    }
}

fn apply_binary(
    operator: &str,
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
            "+" => checked_integer(type_name, left.checked_add(right), site),
            "-" => checked_integer(type_name, left.checked_sub(right), site),
            "*" => checked_integer(type_name, left.checked_mul(right), site),
            "/" if right == 0 => Err(panic_at("division_by_zero", site)),
            "/" => checked_integer(type_name, left.checked_div(right), site),
            "%" if right == 0 => Err(panic_at("division_by_zero", site)),
            "%" => checked_integer(type_name, left.checked_rem(right), site),
            "==" => Ok(CanonicalValue::Bool(left == right)),
            "!=" => Ok(CanonicalValue::Bool(left != right)),
            "<" => Ok(CanonicalValue::Bool(left < right)),
            "<=" => Ok(CanonicalValue::Bool(left <= right)),
            ">" => Ok(CanonicalValue::Bool(left > right)),
            ">=" => Ok(CanonicalValue::Bool(left >= right)),
            _ => Err(rejected("invalid_binary_operator")),
        },
        (CanonicalValue::Bool(left), CanonicalValue::Bool(right)) => match operator {
            "==" => Ok(CanonicalValue::Bool(left == right)),
            "!=" => Ok(CanonicalValue::Bool(left != right)),
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

fn apply_float(
    operator: &str,
    type_name: Arc<str>,
    left_bits: u64,
    right_bits: u64,
) -> Result<CanonicalValue, EvaluationOutcome> {
    let left = decode_float(&type_name, left_bits);
    let right = decode_float(&type_name, right_bits);
    let value = match operator {
        "+" => left + right,
        "-" => left - right,
        "*" => left * right,
        "/" => left / right,
        "%" => left % right,
        "==" => return Ok(CanonicalValue::Bool(left == right)),
        "!=" => return Ok(CanonicalValue::Bool(left != right)),
        "<" => return Ok(CanonicalValue::Bool(left < right)),
        "<=" => return Ok(CanonicalValue::Bool(left <= right)),
        ">" => return Ok(CanonicalValue::Bool(left > right)),
        ">=" => return Ok(CanonicalValue::Bool(left >= right)),
        _ => return Err(rejected("invalid_float_operator")),
    };
    Ok(CanonicalValue::Float {
        bits: encode_float(&type_name, value),
        type_name,
    })
}

fn encode_float(type_name: &str, value: f64) -> u64 {
    let bits = match type_name {
        "f16" => u64::from(half::f16::from_f64(value).to_bits()),
        "f32" => u64::from((value as f32).to_bits()),
        _ => value.to_bits(),
    };
    if value.is_nan() {
        match type_name {
            "f16" => 0x7e00,
            "f32" => 0x7fc0_0000,
            _ => 0x7ff8_0000_0000_0000,
        }
    } else {
        bits
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

fn parse_integer_literal(source: &str) -> Result<(i128, Option<String>), ()> {
    const SUFFIXES: [&str; 8] = ["u16", "u32", "u64", "i16", "i32", "i64", "u8", "i8"];
    let suffix = SUFFIXES.into_iter().find(|suffix| source.ends_with(suffix));
    let digits = suffix.map_or(source, |suffix| &source[..source.len() - suffix.len()]);
    let digits = digits.replace('_', "");
    let (radix, digits) = if let Some(digits) = digits.strip_prefix("0x") {
        (16, digits)
    } else if let Some(digits) = digits.strip_prefix("0b") {
        (2, digits)
    } else if let Some(digits) = digits.strip_prefix("0o") {
        (8, digits)
    } else {
        (10, digits.as_str())
    };
    let value = i128::from_str_radix(digits, radix).map_err(|_| ())?;
    Ok((value, suffix.map(str::to_owned)))
}

fn parse_float_literal(source: &str) -> Result<(f64, Option<String>), ()> {
    let suffix = ["f16", "f32", "f64"]
        .into_iter()
        .find(|suffix| source.ends_with(suffix));
    let number = suffix.map_or(source, |suffix| &source[..source.len() - suffix.len()]);
    let value = number.replace('_', "").parse().map_err(|_| ())?;
    Ok((value, suffix.map(str::to_owned)))
}

fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typed_hir;

    #[test]
    fn logical_fuel_exhaustion_is_exact_and_host_time_independent() {
        let program = typed_hir::verify(BTreeMap::new()).expect("empty program verifies");
        let constants = BTreeMap::new();
        let mut engine = Engine::new(&program, &constants);
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
}
