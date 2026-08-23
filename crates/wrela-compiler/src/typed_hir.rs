use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use xxhash_rust::xxh3::xxh3_128;

use crate::evaluator::{Constant, Function};
use crate::{Cancellation, CanonicalValue, SourceRange};

#[derive(Clone, Debug)]
pub(crate) struct VerifiedProgram {
    functions: BTreeMap<String, HirFunction>,
    constants: BTreeMap<String, HirConstant>,
    fingerprint: u128,
    _verified: Verified,
}

#[derive(Clone, Debug)]
struct Verified;

#[derive(Clone, Debug)]
pub(crate) struct HirFunction {
    pub(crate) name: String,
    pub(crate) module: String,
    pub(crate) parameters: Vec<(String, String)>,
    pub(crate) return_type: String,
    pub(crate) body: Arc<[Statement]>,
    pub(crate) source: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) struct HirConstant {
    pub(crate) type_name: String,
    pub(crate) expression: Expression,
    pub(crate) source: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) enum Statement {
    Return(Option<Expression>),
    Panic(Expression),
    Assign {
        name: String,
        value: Expression,
    },
    Evaluate(Expression),
    If {
        condition: Expression,
        then_branch: Arc<[Statement]>,
        else_branch: Arc<[Statement]>,
    },
    Pass,
}

#[derive(Clone, Debug)]
pub(crate) struct Expression {
    pub(crate) kind: ExpressionKind,
    pub(crate) type_name: String,
}

#[derive(Clone, Debug)]
pub(crate) enum ExpressionKind {
    Literal(CanonicalValue),
    Local(String),
    Constant(String),
    Call {
        target: CallTarget,
        arguments: Arc<[Expression]>,
    },
    Array(Arc<[Expression]>),
    Negate(Box<Expression>),
    Propagate(Box<Expression>),
    Binary {
        operator: BinaryOperator,
        left: Box<Expression>,
        right: Box<Expression>,
    },
}

#[derive(Clone, Debug)]
pub(crate) enum CallTarget {
    Function(String),
    Build(BuildConstructor),
    Variant { type_name: String, variant: String },
    TestApplication(String),
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum BuildConstructor {
    Image,
    Test,
}

#[derive(Clone, Debug)]
pub(crate) struct BuildAuthority {
    constructors: BTreeMap<&'static str, BuildConstructor>,
    _sealed: SealedAuthority,
}

#[derive(Clone, Debug)]
struct SealedAuthority;

impl BuildAuthority {
    pub(crate) fn compiler_distribution() -> Self {
        Self {
            constructors: BTreeMap::from([
                ("Image.new", BuildConstructor::Image),
                ("Test.new", BuildConstructor::Test),
            ]),
            _sealed: SealedAuthority,
        }
    }

    fn resolve(&self, name: &str) -> Option<BuildConstructor> {
        self.constructors.get(name).copied()
    }
}

impl BuildConstructor {
    pub(crate) const fn kind(self) -> &'static str {
        match self {
            Self::Image => "Image",
            Self::Test => "Test",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum VerificationFailure {
    Creator { kind: Arc<str>, site: SourceRange },
    Defect { evidence: Arc<str> },
    Cancelled,
}

impl VerifiedProgram {
    pub(crate) fn functions(&self) -> &BTreeMap<String, HirFunction> {
        &self.functions
    }

    pub(crate) fn constants(&self) -> &BTreeMap<String, HirConstant> {
        &self.constants
    }

    pub(crate) const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn verify_expression(
        &self,
        module: &str,
        source: &str,
        site: &SourceRange,
        allowed_tests: &BTreeSet<String>,
        build_authority: &BuildAuthority,
        nominal_types: &BTreeSet<String>,
        cancellation: &Cancellation,
    ) -> Result<Expression, VerificationFailure> {
        let signatures = self
            .functions
            .iter()
            .map(|(name, function)| {
                (
                    name.clone(),
                    (
                        function
                            .parameters
                            .iter()
                            .map(|(_, type_name)| type_name.clone())
                            .collect(),
                        function.return_type.clone(),
                    ),
                )
            })
            .collect();
        parse_expression(
            source,
            site,
            &Resolution {
                module,
                signatures: &signatures,
                constants: &self
                    .constants
                    .iter()
                    .map(|(name, constant)| (name.clone(), constant.type_name.clone()))
                    .collect(),
                locals: &BTreeMap::new(),
                allowed_tests,
                build_authority,
                nominal_types,
            },
            cancellation,
        )
    }
}

pub(crate) fn verify(
    functions: BTreeMap<String, Function>,
    constants: BTreeMap<String, Constant>,
    allowed_tests: &BTreeSet<String>,
    build_authority: &BuildAuthority,
    nominal_types: &BTreeSet<String>,
    cancellation: &Cancellation,
) -> Result<VerifiedProgram, VerificationFailure> {
    validate_artifact(&functions)?;
    let signatures = functions
        .iter()
        .map(|(name, function)| {
            (
                name.clone(),
                (
                    function
                        .parameters
                        .iter()
                        .map(|(_, type_name)| type_name.clone())
                        .collect(),
                    function.return_type.clone(),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let constant_types = constants
        .iter()
        .map(|(name, constant)| (name.clone(), constant.type_name.clone()))
        .collect::<BTreeMap<_, _>>();

    let mut hir_constants = BTreeMap::new();
    for (lookup_name, constant) in &constants {
        if cancellation.is_cancelled() {
            return Err(VerificationFailure::Cancelled);
        }
        let expression = parse_expression(
            &constant.expression,
            &constant.source,
            &Resolution {
                module: &constant.module,
                signatures: &signatures,
                constants: &constant_types,
                locals: &BTreeMap::new(),
                allowed_tests,
                build_authority,
                nominal_types,
            },
            cancellation,
        )?;
        if !compatible(&expression.type_name, &constant.type_name) {
            return creator("constant_type_mismatch", &constant.source);
        }
        hir_constants.insert(
            lookup_name.clone(),
            HirConstant {
                type_name: constant.type_name.clone(),
                expression,
                source: constant.source.clone(),
            },
        );
    }

    let mut hir_functions = BTreeMap::new();
    for (lookup_name, function) in &functions {
        if cancellation.is_cancelled() {
            return Err(VerificationFailure::Cancelled);
        }
        let mut locals = function
            .parameters
            .iter()
            .cloned()
            .collect::<BTreeMap<_, _>>();
        let mut index = 0;
        let body = lower_statements(
            function,
            &function.body,
            &mut index,
            4,
            &mut locals,
            &signatures,
            &constant_types,
            allowed_tests,
            build_authority,
            nominal_types,
            cancellation,
        )?;
        hir_functions.insert(
            lookup_name.clone(),
            HirFunction {
                name: function.name.clone(),
                module: function.module.clone(),
                parameters: function.parameters.clone(),
                return_type: function.return_type.clone(),
                body: body.into(),
                source: function.source.clone(),
            },
        );
    }

    let mut canonical = b"wrela.typed-hir\0\x01".to_vec();
    for (name, function) in &hir_functions {
        append_part(&mut canonical, name.as_bytes());
        append_part(&mut canonical, function.return_type.as_bytes());
        append_part(
            &mut canonical,
            &u64::try_from(function.body.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
    }
    for (name, constant) in &hir_constants {
        append_part(&mut canonical, name.as_bytes());
        append_part(&mut canonical, constant.type_name.as_bytes());
    }
    Ok(VerifiedProgram {
        functions: hir_functions,
        constants: hir_constants,
        fingerprint: xxh3_128(&canonical),
        _verified: Verified,
    })
}

#[allow(clippy::too_many_arguments)]
fn lower_statements(
    function: &Function,
    lines: &[(u64, String)],
    index: &mut usize,
    indent: usize,
    locals: &mut BTreeMap<String, String>,
    signatures: &BTreeMap<String, (Vec<String>, String)>,
    constants: &BTreeMap<String, String>,
    allowed_tests: &BTreeSet<String>,
    build_authority: &BuildAuthority,
    nominal_types: &BTreeSet<String>,
    cancellation: &Cancellation,
) -> Result<Vec<Statement>, VerificationFailure> {
    let mut statements = Vec::new();
    while let Some((_, raw)) = lines.get(*index) {
        if cancellation.is_cancelled() {
            return Err(VerificationFailure::Cancelled);
        }
        let source = raw.trim();
        if source.is_empty() || source.starts_with('#') || source.starts_with('@') {
            *index += 1;
            continue;
        }
        let actual_indent = raw.bytes().take_while(|byte| *byte == b' ').count();
        if actual_indent < indent {
            break;
        }
        if actual_indent > indent {
            return creator("unexpected_statement_indentation", &function.source);
        }
        if source == "else:" {
            break;
        }
        *index += 1;
        let resolution = Resolution {
            module: &function.module,
            signatures,
            constants,
            locals,
            allowed_tests,
            build_authority,
            nominal_types,
        };
        if let Some(condition) = source
            .strip_prefix("if ")
            .and_then(|value| value.strip_suffix(':'))
        {
            let condition =
                parse_expression(condition, &function.source, &resolution, cancellation)?;
            if condition.type_name != "bool" {
                return creator("if_condition_requires_bool", &function.source);
            }
            let mut then_locals = locals.clone();
            let then_branch = lower_statements(
                function,
                lines,
                index,
                indent + 4,
                &mut then_locals,
                signatures,
                constants,
                allowed_tests,
                build_authority,
                nominal_types,
                cancellation,
            )?;
            let mut else_branch = Vec::new();
            if let Some((_, next)) = lines.get(*index)
                && next.bytes().take_while(|byte| *byte == b' ').count() == indent
                && next.trim() == "else:"
            {
                *index += 1;
                let mut else_locals = locals.clone();
                else_branch = lower_statements(
                    function,
                    lines,
                    index,
                    indent + 4,
                    &mut else_locals,
                    signatures,
                    constants,
                    allowed_tests,
                    build_authority,
                    nominal_types,
                    cancellation,
                )?;
            }
            statements.push(Statement::If {
                condition,
                then_branch: then_branch.into(),
                else_branch: else_branch.into(),
            });
        } else if source == "pass" {
            statements.push(Statement::Pass);
        } else if source == "return" {
            if !compatible("()", &function.return_type) {
                return creator("return_type_mismatch", &function.source);
            }
            statements.push(Statement::Return(None));
        } else if let Some(source) = source.strip_prefix("return ") {
            let expression = parse_expression(source, &function.source, &resolution, cancellation)?;
            if !compatible(&expression.type_name, &function.return_type) {
                return creator("return_type_mismatch", &function.source);
            }
            statements.push(Statement::Return(Some(expression)));
        } else if let Some(source) = source.strip_prefix("panic ") {
            statements.push(Statement::Panic(parse_expression(
                source,
                &function.source,
                &resolution,
                cancellation,
            )?));
        } else if let Some((name, source)) = source.split_once(" = ") {
            if !valid_name(name) {
                return creator("invalid_assignment_target", &function.source);
            }
            let value = parse_expression(source, &function.source, &resolution, cancellation)?;
            locals.insert(name.to_owned(), value.type_name.clone());
            statements.push(Statement::Assign {
                name: name.to_owned(),
                value,
            });
        } else {
            statements.push(Statement::Evaluate(parse_expression(
                source,
                &function.source,
                &resolution,
                cancellation,
            )?));
        }
    }
    Ok(statements)
}

fn validate_artifact(functions: &BTreeMap<String, Function>) -> Result<(), VerificationFailure> {
    for (lookup_name, function) in functions {
        if lookup_name.is_empty() || function.name.is_empty() {
            return defect("empty resolved function name");
        }
        if function.source.start() > function.source.end() {
            return defect("reversed function provenance");
        }
        let mut parameters = BTreeSet::new();
        for (name, type_name) in &function.parameters {
            if name.is_empty() || type_name.is_empty() {
                return defect("unresolved parameter in concrete function");
            }
            if !parameters.insert(name) {
                return defect("duplicate parameter in concrete function");
            }
        }
        if function
            .body
            .iter()
            .any(|(offset, _)| *offset < function.source.start())
        {
            return defect("statement provenance escapes its declaration");
        }
    }
    Ok(())
}

struct Resolution<'a> {
    module: &'a str,
    signatures: &'a BTreeMap<String, (Vec<String>, String)>,
    constants: &'a BTreeMap<String, String>,
    locals: &'a BTreeMap<String, String>,
    allowed_tests: &'a BTreeSet<String>,
    build_authority: &'a BuildAuthority,
    nominal_types: &'a BTreeSet<String>,
}

fn parse_expression(
    source: &str,
    site: &SourceRange,
    resolution: &Resolution<'_>,
    cancellation: &Cancellation,
) -> Result<Expression, VerificationFailure> {
    let tokens = tokenize(source, cancellation).map_err(|failure| match failure {
        TokenizeFailure::Invalid => VerificationFailure::Creator {
            kind: Arc::from("invalid_expression"),
            site: site.clone(),
        },
        TokenizeFailure::Cancelled => VerificationFailure::Cancelled,
    })?;
    let mut parser = Parser {
        tokens: &tokens,
        index: 0,
        site,
        resolution,
    };
    let expression = parser.expression(0)?;
    if parser.index != tokens.len() {
        return creator("trailing_expression_tokens", site);
    }
    Ok(expression)
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Name(String),
    Integer(i128, Option<String>),
    Float(f64, Option<String>),
    Text(String),
    Symbol(&'static str),
}

enum TokenizeFailure {
    Invalid,
    Cancelled,
}

fn tokenize(source: &str, cancellation: &Cancellation) -> Result<Vec<Token>, TokenizeFailure> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if index % 256 == 0 && cancellation.is_cancelled() {
            return Err(TokenizeFailure::Cancelled);
        }
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
                    let (value, suffix) = parse_float_literal(&source[start..index])
                        .map_err(|()| TokenizeFailure::Invalid)?;
                    tokens.push(Token::Float(value, suffix));
                } else {
                    let (value, suffix) = parse_integer_literal(&source[start..index])
                        .map_err(|()| TokenizeFailure::Invalid)?;
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
                    return Err(TokenizeFailure::Invalid);
                }
                tokens.push(Token::Text(source[start..index].to_owned()));
                index += 1;
            }
            _ => {
                let remaining = &source[index..];
                let symbol = [
                    "==", "!=", "<=", ">=", "+", "-", "*", "/", "%", "<", ">", "(", ")", "[", "]",
                    ",", "=", "?",
                ]
                .into_iter()
                .find(|symbol| remaining.starts_with(symbol))
                .ok_or(TokenizeFailure::Invalid)?;
                tokens.push(Token::Symbol(symbol));
                index += symbol.len();
            }
        }
    }
    Ok(tokens)
}

struct Parser<'a> {
    tokens: &'a [Token],
    index: usize,
    site: &'a SourceRange,
    resolution: &'a Resolution<'a>,
}

impl Parser<'_> {
    fn expression(&mut self, minimum_precedence: u8) -> Result<Expression, VerificationFailure> {
        let mut left = self.primary()?;
        if self.tokens.get(self.index) == Some(&Token::Symbol("?")) {
            self.index += 1;
            let type_name = left
                .type_name
                .strip_prefix("Result[")
                .and_then(|result| result.split([',', ']']).next())
                .map(str::trim)
                .filter(|result| !result.is_empty())
                .ok_or_else(|| VerificationFailure::Creator {
                    kind: Arc::from("propagation_requires_result"),
                    site: self.site.clone(),
                })?
                .to_owned();
            left = Expression {
                kind: ExpressionKind::Propagate(Box::new(left)),
                type_name,
            };
        }
        while let Some((operator, precedence)) = self.binary_operator() {
            if precedence < minimum_precedence {
                break;
            }
            self.index += 1;
            let right = self.expression(precedence + 1)?;
            let type_name =
                binary_type(operator, &left.type_name, &right.type_name).ok_or_else(|| {
                    VerificationFailure::Creator {
                        kind: Arc::from("binary_type_mismatch"),
                        site: self.site.clone(),
                    }
                })?;
            left = Expression {
                kind: ExpressionKind::Binary {
                    operator,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                type_name,
            };
        }
        Ok(left)
    }

    fn primary(&mut self) -> Result<Expression, VerificationFailure> {
        let token =
            self.tokens
                .get(self.index)
                .cloned()
                .ok_or_else(|| VerificationFailure::Creator {
                    kind: Arc::from("missing_expression"),
                    site: self.site.clone(),
                })?;
        self.index += 1;
        match token {
            Token::Integer(value, suffix) => {
                let type_name = suffix.unwrap_or_else(|| "i64".to_owned());
                Ok(Expression {
                    kind: ExpressionKind::Literal(CanonicalValue::Integer {
                        type_name: Arc::from(type_name.as_str()),
                        value,
                    }),
                    type_name,
                })
            }
            Token::Float(value, suffix) => {
                let type_name = suffix.unwrap_or_else(|| "f64".to_owned());
                Ok(Expression {
                    kind: ExpressionKind::Literal(CanonicalValue::Float {
                        type_name: Arc::from(type_name.as_str()),
                        bits: encode_float(&type_name, value),
                    }),
                    type_name,
                })
            }
            Token::Text(value) => Ok(Expression {
                kind: ExpressionKind::Literal(CanonicalValue::Text(value.into())),
                type_name: "Text".to_owned(),
            }),
            Token::Name(name) if matches!(name.as_str(), "true" | "false") => Ok(Expression {
                kind: ExpressionKind::Literal(CanonicalValue::Bool(name == "true")),
                type_name: "bool".to_owned(),
            }),
            Token::Name(name) if self.tokens.get(self.index) == Some(&Token::Symbol("(")) => {
                self.index += 1;
                let mut arguments = Vec::new();
                while self.tokens.get(self.index) != Some(&Token::Symbol(")")) {
                    if let (Some(Token::Name(_)), Some(Token::Symbol("="))) =
                        (self.tokens.get(self.index), self.tokens.get(self.index + 1))
                    {
                        self.index += 2;
                    }
                    arguments.push(self.expression(0)?);
                    if self.tokens.get(self.index) == Some(&Token::Symbol(",")) {
                        self.index += 1;
                    } else {
                        break;
                    }
                }
                if self.tokens.get(self.index) != Some(&Token::Symbol(")")) {
                    return creator("missing_call_closer", self.site);
                }
                self.index += 1;
                let (target, type_name) = self.resolve_call(&name, &arguments)?;
                Ok(Expression {
                    kind: ExpressionKind::Call {
                        target,
                        arguments: arguments.into(),
                    },
                    type_name,
                })
            }
            Token::Name(name) => self.resolve_value(&name),
            Token::Symbol("(") => {
                if self.tokens.get(self.index) == Some(&Token::Symbol(")")) {
                    self.index += 1;
                    return Ok(Expression {
                        kind: ExpressionKind::Literal(CanonicalValue::Unit),
                        type_name: "()".to_owned(),
                    });
                }
                let expression = self.expression(0)?;
                if self.tokens.get(self.index) != Some(&Token::Symbol(")")) {
                    return creator("missing_group_closer", self.site);
                }
                self.index += 1;
                Ok(expression)
            }
            Token::Symbol("[") => {
                let mut values = Vec::new();
                while self.tokens.get(self.index) != Some(&Token::Symbol("]")) {
                    values.push(self.expression(0)?);
                    if self.tokens.get(self.index) == Some(&Token::Symbol(",")) {
                        self.index += 1;
                    } else {
                        break;
                    }
                }
                if self.tokens.get(self.index) != Some(&Token::Symbol("]")) {
                    return creator("missing_array_closer", self.site);
                }
                self.index += 1;
                let element = values
                    .first()
                    .map_or_else(|| "_".to_owned(), |expression| expression.type_name.clone());
                if values
                    .iter()
                    .any(|expression| !compatible(&expression.type_name, &element))
                {
                    return creator("array_element_type_mismatch", self.site);
                }
                Ok(Expression {
                    kind: ExpressionKind::Array(values.into()),
                    type_name: format!("[{element}]"),
                })
            }
            Token::Symbol("-") => {
                let expression = self.expression(12)?;
                if !numeric(&expression.type_name) {
                    return creator("invalid_unary_operand", self.site);
                }
                let type_name = expression.type_name.clone();
                Ok(Expression {
                    kind: ExpressionKind::Negate(Box::new(expression)),
                    type_name,
                })
            }
            _ => creator("invalid_primary", self.site),
        }
    }

    fn resolve_value(&self, name: &str) -> Result<Expression, VerificationFailure> {
        if let Some(type_name) = self.resolution.locals.get(name) {
            return Ok(Expression {
                kind: ExpressionKind::Local(name.to_owned()),
                type_name: type_name.clone(),
            });
        }
        let qualified = qualify_lookup(self.resolution.module, name, self.resolution.constants);
        if let Some(type_name) = self.resolution.constants.get(&qualified) {
            return Ok(Expression {
                kind: ExpressionKind::Constant(qualified),
                type_name: type_name.clone(),
            });
        }
        if let Some((type_name, variant)) = name.split_once('.')
            && type_name
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_uppercase)
        {
            if !self.resolution.nominal_types.contains(type_name)
                && !matches!(type_name, "Result" | "Option")
            {
                return creator("unresolved_nominal_type", self.site);
            }
            return Ok(Expression {
                kind: ExpressionKind::Call {
                    target: CallTarget::Variant {
                        type_name: type_name.to_owned(),
                        variant: variant.to_owned(),
                    },
                    arguments: Arc::from([]),
                },
                type_name: type_name.to_owned(),
            });
        }
        creator("unresolved_name", self.site)
    }

    fn resolve_call(
        &self,
        name: &str,
        arguments: &[Expression],
    ) -> Result<(CallTarget, String), VerificationFailure> {
        let qualified = qualify_lookup(self.resolution.module, name, self.resolution.signatures);
        if let Some((parameters, return_type)) = self.resolution.signatures.get(&qualified) {
            if parameters.len() != arguments.len() {
                return creator("argument_count", self.site);
            }
            let mut substitutions = BTreeMap::new();
            for (parameter, argument) in parameters.iter().zip(arguments) {
                if generic_type_parameter(parameter) {
                    if let Some(previous) =
                        substitutions.insert(parameter, argument.type_name.as_str())
                        && previous != argument.type_name
                    {
                        return creator("generic_argument_conflict", self.site);
                    }
                } else if !compatible(&argument.type_name, parameter) {
                    return creator("argument_type_mismatch", self.site);
                }
            }
            let return_type = substitutions
                .get(&return_type)
                .map_or_else(|| return_type.clone(), |concrete| (*concrete).to_owned());
            return Ok((CallTarget::Function(qualified), return_type));
        }
        if let Some(constructor) = self.resolution.build_authority.resolve(name) {
            return Ok((
                CallTarget::Build(constructor),
                constructor.kind().to_owned(),
            ));
        }
        if self.resolution.allowed_tests.contains(name) {
            return Ok((
                CallTarget::TestApplication(name.to_owned()),
                "TestApplication".into(),
            ));
        }
        if let Some((type_name, variant)) = name.split_once('.')
            && type_name
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_uppercase)
        {
            if !self.resolution.nominal_types.contains(type_name)
                && !matches!(type_name, "Result" | "Option")
            {
                return creator("unresolved_nominal_type", self.site);
            }
            let resolved_type = match (type_name, variant, arguments.first()) {
                ("Result", "Ok", Some(value)) => format!("Result[{}, _]", value.type_name),
                ("Result", "Err", Some(error)) => format!("Result[_, {}]", error.type_name),
                _ => type_name.to_owned(),
            };
            return Ok((
                CallTarget::Variant {
                    type_name: type_name.to_owned(),
                    variant: variant.to_owned(),
                },
                resolved_type,
            ));
        }
        creator("unresolved_call", self.site)
    }

    fn binary_operator(&self) -> Option<(BinaryOperator, u8)> {
        let Token::Symbol(operator) = self.tokens.get(self.index)? else {
            return None;
        };
        Some(match *operator {
            "==" => (BinaryOperator::Equal, 5),
            "!=" => (BinaryOperator::NotEqual, 5),
            "<" => (BinaryOperator::Less, 5),
            "<=" => (BinaryOperator::LessEqual, 5),
            ">" => (BinaryOperator::Greater, 5),
            ">=" => (BinaryOperator::GreaterEqual, 5),
            "+" => (BinaryOperator::Add, 10),
            "-" => (BinaryOperator::Subtract, 10),
            "*" => (BinaryOperator::Multiply, 11),
            "/" => (BinaryOperator::Divide, 11),
            "%" => (BinaryOperator::Remainder, 11),
            _ => return None,
        })
    }
}

fn qualify_lookup<T>(module: &str, name: &str, map: &BTreeMap<String, T>) -> String {
    if map.contains_key(name) {
        name.to_owned()
    } else {
        let imported = format!("{module}|{name}");
        if map.contains_key(&imported) {
            imported
        } else {
            format!("{module}.{name}")
        }
    }
}

fn binary_type(operator: BinaryOperator, left: &str, right: &str) -> Option<String> {
    if !compatible(left, right) {
        return None;
    }
    if matches!(
        operator,
        BinaryOperator::Equal
            | BinaryOperator::NotEqual
            | BinaryOperator::Less
            | BinaryOperator::LessEqual
            | BinaryOperator::Greater
            | BinaryOperator::GreaterEqual
    ) {
        return Some("bool".to_owned());
    }
    numeric(left).then(|| left.to_owned())
}

fn compatible(actual: &str, expected: &str) -> bool {
    actual == expected
        || (numeric(actual) && numeric(expected))
        || generic_type_parameter(actual)
        || generic_type_parameter(expected)
        || compatible_result(actual, expected)
        || (actual == "()" && expected.is_empty())
}

fn compatible_result(actual: &str, expected: &str) -> bool {
    let Some(expected) = expected
        .strip_prefix("Result[")
        .and_then(|value| value.strip_suffix(']'))
    else {
        return false;
    };
    let expected = expected.split(',').map(str::trim).collect::<Vec<_>>();
    let Some(actual_result) = actual
        .strip_prefix("Result[")
        .and_then(|value| value.strip_suffix(']'))
    else {
        return expected
            .first()
            .is_some_and(|success| compatible(actual, success));
    };
    let actual = actual_result.split(',').map(str::trim).collect::<Vec<_>>();
    let success_matches = actual.first().is_some_and(|actual| {
        *actual == "_"
            || expected
                .first()
                .is_some_and(|expected| compatible(actual, expected))
    });
    let error_matches = expected.len() == 1
        || actual.get(1).is_some_and(|actual| {
            *actual == "_"
                || expected
                    .get(1)
                    .is_some_and(|expected| compatible(actual, expected))
        });
    success_matches && error_matches
}

fn numeric(type_name: &str) -> bool {
    matches!(
        type_name,
        "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64" | "f16" | "f32" | "f64"
    )
}

fn generic_type_parameter(type_name: &str) -> bool {
    let mut characters = type_name.chars();
    matches!(characters.next(), Some('A'..='Z'))
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
        && !matches!(
            type_name,
            "Text" | "Bytes" | "Image" | "Result" | "Option" | "Test"
        )
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
    Ok((
        i128::from_str_radix(digits, radix).map_err(|_| ())?,
        suffix.map(str::to_owned),
    ))
}

fn parse_float_literal(source: &str) -> Result<(f64, Option<String>), ()> {
    let suffix = ["f16", "f32", "f64"]
        .into_iter()
        .find(|suffix| source.ends_with(suffix));
    let number = suffix.map_or(source, |suffix| &source[..source.len() - suffix.len()]);
    Ok((
        number.replace('_', "").parse().map_err(|_| ())?,
        suffix.map(str::to_owned),
    ))
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

fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn append_part(target: &mut Vec<u8>, part: &[u8]) {
    target.extend_from_slice(&u64::try_from(part.len()).unwrap_or(u64::MAX).to_be_bytes());
    target.extend_from_slice(part);
}

fn creator<T>(kind: &'static str, site: &SourceRange) -> Result<T, VerificationFailure> {
    Err(VerificationFailure::Creator {
        kind: Arc::from(kind),
        site: site.clone(),
    })
}

fn defect<T>(evidence: &'static str) -> Result<T, VerificationFailure> {
    Err(VerificationFailure::Defect {
        evidence: Arc::from(evidence),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_compiler_artifact_is_contained_as_a_verification_defect() {
        let malformed = Function {
            name: String::new(),
            module: "image".to_owned(),
            public: false,
            parameters: Vec::new(),
            return_type: "i64".to_owned(),
            body: vec![(0, "return 1".to_owned())],
            source: SourceRange::new("src/image.wr", 0, 1),
        };
        let failure = verify(
            BTreeMap::from([(String::new(), malformed)]),
            BTreeMap::new(),
            &BTreeSet::new(),
            &BuildAuthority::compiler_distribution(),
            &BTreeSet::new(),
            &Cancellation::new(),
        )
        .expect_err("malformed artifact must not receive verified marker");
        assert_eq!(
            failure,
            VerificationFailure::Defect {
                evidence: Arc::from("empty resolved function name")
            }
        );
    }

    #[test]
    fn unused_unresolved_calls_are_creator_errors() {
        let function = Function {
            name: "unused".to_owned(),
            module: "image".to_owned(),
            public: false,
            parameters: Vec::new(),
            return_type: "i64".to_owned(),
            body: vec![(1, "    return missing()".to_owned())],
            source: SourceRange::new("src/image.wr", 0, 1),
        };
        assert!(matches!(
            verify(
                BTreeMap::from([("image.unused".to_owned(), function)]),
                BTreeMap::new(),
                &BTreeSet::new(),
                &BuildAuthority::compiler_distribution(),
                &BTreeSet::new(),
                &Cancellation::new()
            ),
            Err(VerificationFailure::Creator { kind, .. }) if kind.as_ref() == "unresolved_call"
        ));
    }

    #[test]
    fn invented_nominal_variants_cannot_enter_verified_hir() {
        let function = Function {
            name: "unused".to_owned(),
            module: "image".to_owned(),
            public: false,
            parameters: Vec::new(),
            return_type: "Missing".to_owned(),
            body: vec![(1, "    return Missing.Invented".to_owned())],
            source: SourceRange::new("src/image.wr", 0, 1),
        };
        assert!(matches!(
            verify(
                BTreeMap::from([("image.unused".to_owned(), function)]),
                BTreeMap::new(),
                &BTreeSet::new(),
                &BuildAuthority::compiler_distribution(),
                &BTreeSet::new(),
                &Cancellation::new()
            ),
            Err(VerificationFailure::Creator { kind, .. })
                if kind.as_ref() == "unresolved_nominal_type"
        ));
    }
}
