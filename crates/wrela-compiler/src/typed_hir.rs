#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use xxhash_rust::xxh3::xxh3_128;

use crate::identity::IdentityCatalog;
use crate::model::{
    BuildKind, BuiltinType, BuiltinVariant, DefinitionId, FloatType, IntegerType, ModuleId,
    SpecializationId, TestId, Type, TypeId, VariantId, resolve_build_kind, resolve_builtin_variant,
};
use crate::syntax::{
    BinaryOperatorSyntax, ExpressionSyntax, ExpressionSyntaxKind, NameSyntax, OwnershipSyntax,
    StatementSyntax,
};
use crate::{Cancellation, SourceRange};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct LocalId(pub(crate) u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Place {
    pub(crate) local: LocalId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AccessMode {
    Copy,
    Move,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedParameter {
    pub(crate) name: String,
    pub(crate) ownership: OwnershipSyntax,
    pub(crate) type_: Type,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedFunction {
    pub(crate) id: DefinitionId,
    pub(crate) module: ModuleId,
    pub(crate) module_display: String,
    pub(crate) name: String,
    pub(crate) modifier: crate::syntax::FunctionModifier,
    pub(crate) type_parameters: Arc<[crate::model::TypeParameterId]>,
    pub(crate) parameters: Vec<ResolvedParameter>,
    pub(crate) return_type: Type,
    pub(crate) body: Vec<StatementSyntax>,
    pub(crate) source: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedConstant {
    pub(crate) id: DefinitionId,
    pub(crate) module: ModuleId,
    pub(crate) name: String,
    pub(crate) type_: Type,
    pub(crate) value: ExpressionSyntax,
    pub(crate) source: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedTest {
    pub(crate) id: TestId,
    pub(crate) suite: String,
    pub(crate) test: String,
    pub(crate) asynchronous: bool,
    pub(crate) parameters: Vec<ResolvedParameter>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct NameKey {
    pub(crate) module: ModuleId,
    pub(crate) segments: Arc<[String]>,
}

impl NameKey {
    pub(crate) fn new(module: ModuleId, segments: impl Into<Arc<[String]>>) -> Self {
        Self {
            module,
            segments: segments.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResolvedName {
    Function(DefinitionId),
    Constant(DefinitionId),
    Nominal(DefinitionId),
    Test(TestId),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ProgramInput {
    pub(crate) functions: BTreeMap<DefinitionId, ResolvedFunction>,
    pub(crate) constants: BTreeMap<DefinitionId, ResolvedConstant>,
    pub(crate) tests: BTreeMap<TestId, ResolvedTest>,
    pub(crate) names: BTreeMap<NameKey, ResolvedName>,
    pub(crate) nominal_displays: BTreeMap<DefinitionId, Arc<str>>,
    pub(crate) comptime_roots: Vec<(ModuleId, ExpressionSyntax)>,
}

#[derive(Clone, Debug)]
pub(crate) struct VerifiedProgram {
    functions: BTreeMap<DefinitionId, HirFunction>,
    specialized_functions: BTreeMap<SpecializationId, HirFunction>,
    default_specializations: BTreeMap<DefinitionId, SpecializationId>,
    constants: BTreeMap<DefinitionId, HirConstant>,
    tests: BTreeMap<TestId, ResolvedTest>,
    specializations: BTreeMap<SpecializationId, SpecializationRecord>,
    comptime_expressions: BTreeMap<SourceRange, Expression>,
    fingerprint: u128,
    _verified: Verified,
}

#[derive(Clone, Debug)]
struct Verified;

#[derive(Clone, Debug)]
pub(crate) struct HirFunction {
    pub(crate) id: DefinitionId,
    pub(crate) name: String,
    pub(crate) module_display: String,
    pub(crate) modifier: crate::syntax::FunctionModifier,
    pub(crate) parameters: Vec<(LocalId, Type, AccessMode)>,
    pub(crate) return_type: Type,
    pub(crate) body: Arc<[Statement]>,
    pub(crate) source: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) struct HirConstant {
    pub(crate) id: DefinitionId,
    pub(crate) type_: Type,
    pub(crate) expression: Expression,
    pub(crate) source: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) enum Statement {
    Return {
        value: Option<Expression>,
        source: SourceRange,
    },
    Panic {
        value: Expression,
        source: SourceRange,
    },
    Initialize {
        place: Place,
        value: Expression,
        source: SourceRange,
    },
    Evaluate(Expression),
    If {
        condition: Expression,
        then_branch: Arc<[Statement]>,
        else_branch: Arc<[Statement]>,
        source: SourceRange,
    },
    Pass(SourceRange),
}

#[derive(Clone, Debug)]
pub(crate) struct Expression {
    pub(crate) kind: ExpressionKind,
    pub(crate) type_id: TypeId,
    pub(crate) type_: Type,
    pub(crate) access: AccessMode,
    pub(crate) source: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) enum ExpressionKind {
    Literal(Literal),
    Read(Place),
    Constant(DefinitionId),
    Call {
        target: CallTarget,
        arguments: Arc<[Expression]>,
    },
    Array(Arc<[Expression]>),
    Negate(Box<Expression>),
    Await(Box<Expression>),
    Propagate(Box<Expression>),
    Binary {
        operator: BinaryOperator,
        left: Box<Expression>,
        right: Box<Expression>,
    },
}

#[derive(Clone, Debug)]
pub(crate) enum Literal {
    Unit,
    Bool(bool),
    Integer { kind: IntegerType, value: i128 },
    Float { kind: FloatType, bits: u64 },
    Text(Arc<str>),
}

#[derive(Clone, Debug)]
pub(crate) enum CallTarget {
    TemplateFunction(DefinitionId),
    Function {
        definition: DefinitionId,
        specialization: SpecializationId,
    },
    Build(BuildKind),
    BuiltinVariant(BuiltinVariant),
    UserVariant {
        id: VariantId,
        type_display: Arc<str>,
        variant_display: Arc<str>,
    },
    Test(TestId),
}

#[derive(Clone, Debug)]
pub(crate) struct SpecializationRecord {
    pub(crate) id: SpecializationId,
    pub(crate) definition: DefinitionId,
    pub(crate) type_arguments: Arc<[Type]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CreatorFailureKind {
    EmptyName,
    DuplicateParameter,
    ConstantTypeMismatch,
    ReturnTypeMismatch,
    IfConditionRequiresBool,
    UnresolvedName,
    UnresolvedCall,
    UnresolvedNominalType,
    ArgumentCount,
    ArgumentTypeMismatch,
    ArgumentLabelMismatch,
    GenericArgumentConflict,
    PropagationRequiresResult,
    BinaryTypeMismatch,
    ArrayElementTypeMismatch,
    InvalidUnaryOperand,
    InvalidIntegerLiteral,
    InvalidFloatLiteral,
}

impl CreatorFailureKind {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::EmptyName => "empty_name",
            Self::DuplicateParameter => "duplicate_parameter",
            Self::ConstantTypeMismatch => "constant_type_mismatch",
            Self::ReturnTypeMismatch => "return_type_mismatch",
            Self::IfConditionRequiresBool => "if_condition_requires_bool",
            Self::UnresolvedName => "unresolved_name",
            Self::UnresolvedCall => "unresolved_call",
            Self::UnresolvedNominalType => "unresolved_nominal_type",
            Self::ArgumentCount => "argument_count",
            Self::ArgumentTypeMismatch => "argument_type_mismatch",
            Self::ArgumentLabelMismatch => "argument_label_mismatch",
            Self::GenericArgumentConflict => "generic_argument_conflict",
            Self::PropagationRequiresResult => "propagation_requires_result",
            Self::BinaryTypeMismatch => "binary_type_mismatch",
            Self::ArrayElementTypeMismatch => "array_element_type_mismatch",
            Self::InvalidUnaryOperand => "invalid_unary_operand",
            Self::InvalidIntegerLiteral => "invalid_integer_literal",
            Self::InvalidFloatLiteral => "invalid_float_literal",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum VerificationFailure {
    Creator {
        kind: CreatorFailureKind,
        site: SourceRange,
    },
    Defect {
        evidence: Arc<str>,
    },
    Cancelled,
}

#[derive(Clone, Debug)]
pub(crate) struct BuildAuthority {
    _sealed: SealedAuthority,
}

#[derive(Clone, Debug)]
struct SealedAuthority;

impl BuildAuthority {
    pub(crate) fn compiler_distribution() -> Self {
        Self {
            _sealed: SealedAuthority,
        }
    }

    fn permits(&self, _kind: BuildKind) -> bool {
        true
    }
}

impl VerifiedProgram {
    pub(crate) fn functions(&self) -> &BTreeMap<DefinitionId, HirFunction> {
        &self.functions
    }
    pub(crate) fn constants(&self) -> &BTreeMap<DefinitionId, HirConstant> {
        &self.constants
    }
    pub(crate) fn specialization_function(&self, id: SpecializationId) -> Option<&HirFunction> {
        self.specialized_functions.get(&id)
    }
    pub(crate) fn specialized_functions(&self) -> &BTreeMap<SpecializationId, HirFunction> {
        &self.specialized_functions
    }
    pub(crate) fn default_specialization(&self, id: DefinitionId) -> Option<SpecializationId> {
        self.default_specializations.get(&id).copied()
    }
    pub(crate) fn test(&self, id: TestId) -> Option<&ResolvedTest> {
        let test = self.tests.get(&id)?;
        debug_assert_eq!(test.id, id);
        Some(test)
    }
    pub(crate) fn specializations(&self) -> &BTreeMap<SpecializationId, SpecializationRecord> {
        &self.specializations
    }
    pub(crate) const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }

    pub(crate) fn verify_expression(
        &self,
        syntax: &ExpressionSyntax,
    ) -> Result<Expression, VerificationFailure> {
        self.comptime_expressions
            .get(&syntax.range)
            .cloned()
            .ok_or_else(|| VerificationFailure::Defect {
                evidence: Arc::from("comptime expression was not included in concrete demand"),
            })
    }
}

pub(crate) fn verify(
    input: ProgramInput,
    build_authority: &BuildAuthority,
    identity_catalog: &mut IdentityCatalog,
    cancellation: &Cancellation,
) -> Result<VerifiedProgram, VerificationFailure> {
    validate_input(&input)?;
    intern_input_types(&input, identity_catalog)?;
    let mut specializations = BTreeMap::new();
    let mut comptime_expressions = BTreeMap::new();
    let mut constants = BTreeMap::new();
    for constant in input.constants.values() {
        if cancellation.is_cancelled() {
            return Err(VerificationFailure::Cancelled);
        }
        let mut lowerer = Lowerer::new(
            constant.module,
            &input,
            build_authority,
            identity_catalog,
            cancellation,
            &mut specializations,
            true,
        );
        let expression = lowerer.expression(&constant.value)?;
        if !expression.type_.compatible_with(&constant.type_) {
            return creator(CreatorFailureKind::ConstantTypeMismatch, &constant.source);
        }
        constants.insert(
            constant.id,
            HirConstant {
                id: constant.id,
                type_: constant.type_.clone(),
                expression,
                source: constant.source.clone(),
            },
        );
    }
    for (module, root) in &input.comptime_roots {
        if cancellation.is_cancelled() {
            return Err(VerificationFailure::Cancelled);
        }
        let expression = Lowerer::new(
            *module,
            &input,
            build_authority,
            identity_catalog,
            cancellation,
            &mut specializations,
            true,
        )
        .expression(root)?;
        comptime_expressions.insert(root.range.clone(), expression);
    }

    let mut functions = BTreeMap::new();
    for function in input.functions.values() {
        if cancellation.is_cancelled() {
            return Err(VerificationFailure::Cancelled);
        }
        let mut lowerer = Lowerer::new(
            function.module,
            &input,
            build_authority,
            identity_catalog,
            cancellation,
            &mut specializations,
            false,
        );
        let mut parameters = Vec::new();
        for parameter in &function.parameters {
            let local = lowerer.bind_local(&parameter.name, parameter.type_.clone())?;
            parameters.push((
                local,
                parameter.type_.clone(),
                match parameter.ownership {
                    OwnershipSyntax::Value => AccessMode::Copy,
                    OwnershipSyntax::Take => AccessMode::Move,
                },
            ));
        }
        let body = lowerer.statements(&function.body, &function.return_type)?;
        functions.insert(
            function.id,
            HirFunction {
                id: function.id,
                name: function.name.clone(),
                module_display: function.module_display.clone(),
                modifier: function.modifier,
                parameters,
                return_type: function.return_type.clone(),
                body: body.into(),
                source: function.source.clone(),
            },
        );
    }

    let mut default_specializations = BTreeMap::new();
    for function in input
        .functions
        .values()
        .filter(|function| function.type_parameters.is_empty())
    {
        let id = identity_catalog
            .specialization(function.id, &[])
            .map_err(|collision| VerificationFailure::Defect {
                evidence: Arc::from(format!(
                    "specialization identity collision {:032x}",
                    collision.digest
                )),
            })?;
        default_specializations.insert(function.id, id);
        specializations.entry(id).or_insert(SpecializationRecord {
            id,
            definition: function.id,
            type_arguments: Arc::from([]),
        });
    }

    let mut specialized_functions = BTreeMap::new();
    loop {
        let next = specializations
            .values()
            .find(|record| !specialized_functions.contains_key(&record.id))
            .cloned();
        let Some(record) = next else {
            break;
        };
        if cancellation.is_cancelled() {
            return Err(VerificationFailure::Cancelled);
        }
        let function =
            input
                .functions
                .get(&record.definition)
                .ok_or_else(|| VerificationFailure::Defect {
                    evidence: Arc::from("SpecializationId references an unknown function"),
                })?;
        if function.type_parameters.len() != record.type_arguments.len()
            || record.type_arguments.iter().any(type_has_placeholder)
        {
            return defect("SpecializationId does not describe a fully concrete application");
        }
        let substitutions = function
            .type_parameters
            .iter()
            .copied()
            .zip(record.type_arguments.iter().cloned())
            .collect::<BTreeMap<_, _>>();
        let mut lowerer = Lowerer::new(
            function.module,
            &input,
            build_authority,
            identity_catalog,
            cancellation,
            &mut specializations,
            true,
        );
        let mut parameters = Vec::new();
        for parameter in &function.parameters {
            let type_ = substitute(&parameter.type_, &substitutions);
            let local = lowerer.bind_local(&parameter.name, type_.clone())?;
            parameters.push((
                local,
                type_,
                match parameter.ownership {
                    OwnershipSyntax::Value => AccessMode::Copy,
                    OwnershipSyntax::Take => AccessMode::Move,
                },
            ));
        }
        let return_type = substitute(&function.return_type, &substitutions);
        let body = lowerer.statements(&function.body, &return_type)?;
        specialized_functions.insert(
            record.id,
            HirFunction {
                id: function.id,
                name: function.name.clone(),
                module_display: function.module_display.clone(),
                modifier: function.modifier,
                parameters,
                return_type,
                body: body.into(),
                source: function.source.clone(),
            },
        );
    }

    let mut canonical = b"wrela.typed-hir\0\x03".to_vec();
    canonical.push(0);
    canonical.extend_from_slice(&identity_catalog.revision_fingerprint().to_be_bytes());
    append_collection_header(&mut canonical, 1, functions.len());
    for (id, function) in &functions {
        append_part(&mut canonical, &id.0.to_be_bytes());
        append_function(&mut canonical, function);
    }
    append_collection_header(&mut canonical, 2, constants.len());
    for (id, constant) in &constants {
        append_part(&mut canonical, &id.0.to_be_bytes());
        append_part(&mut canonical, &constant.type_.canonical_key());
        append_expression(&mut canonical, &constant.expression);
    }
    append_collection_header(&mut canonical, 3, specialized_functions.len());
    for (id, function) in &specialized_functions {
        append_part(&mut canonical, &id.0.to_be_bytes());
        append_function(&mut canonical, function);
    }
    append_collection_header(&mut canonical, 4, default_specializations.len());
    for (definition, specialization) in &default_specializations {
        canonical.extend_from_slice(&definition.0.to_be_bytes());
        canonical.extend_from_slice(&specialization.0.to_be_bytes());
    }
    append_collection_header(&mut canonical, 5, specializations.len());
    for (id, specialization) in &specializations {
        canonical.extend_from_slice(&id.0.to_be_bytes());
        canonical.extend_from_slice(&specialization.definition.0.to_be_bytes());
        canonical.extend_from_slice(
            &u64::try_from(specialization.type_arguments.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        for argument in &*specialization.type_arguments {
            append_part(&mut canonical, &argument.canonical_key());
        }
    }
    append_collection_header(&mut canonical, 6, input.tests.len());
    for (id, test) in &input.tests {
        canonical.extend_from_slice(&id.suite.0.to_be_bytes());
        canonical.extend_from_slice(&id.test.0.to_be_bytes());
        canonical.extend_from_slice(&id.identity.to_be_bytes());
        append_part(&mut canonical, test.suite.as_bytes());
        append_part(&mut canonical, test.test.as_bytes());
        canonical.push(u8::from(test.asynchronous));
        canonical.extend_from_slice(
            &u64::try_from(test.parameters.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        for parameter in &test.parameters {
            append_part(&mut canonical, parameter.name.as_bytes());
            canonical.push(match parameter.ownership {
                OwnershipSyntax::Value => 0,
                OwnershipSyntax::Take => 1,
            });
            append_part(&mut canonical, &parameter.type_.canonical_key());
        }
    }
    append_collection_header(&mut canonical, 7, comptime_expressions.len());
    for (source, expression) in &comptime_expressions {
        append_range(&mut canonical, source);
        append_expression(&mut canonical, expression);
    }
    verify_lowered_artifact(
        &functions,
        &specialized_functions,
        &constants,
        &specializations,
        identity_catalog,
    )?;
    Ok(VerifiedProgram {
        functions,
        specialized_functions,
        default_specializations,
        constants,
        tests: input.tests,
        specializations,
        comptime_expressions,
        fingerprint: xxh3_128(&canonical),
        _verified: Verified,
    })
}

fn append_collection_header(bytes: &mut Vec<u8>, tag: u8, length: usize) {
    bytes.push(tag);
    bytes.extend_from_slice(&u64::try_from(length).unwrap_or(u64::MAX).to_be_bytes());
}

fn intern_input_types(
    input: &ProgramInput,
    identities: &mut IdentityCatalog,
) -> Result<(), VerificationFailure> {
    let mut intern = |type_: &Type| {
        identities
            .intern_type(type_)
            .map(|_| ())
            .map_err(|collision| VerificationFailure::Defect {
                evidence: Arc::from(format!("type identity collision {:032x}", collision.digest)),
            })
    };
    for function in input.functions.values() {
        for parameter in &function.parameters {
            intern(&parameter.type_)?;
        }
        intern(&function.return_type)?;
    }
    for constant in input.constants.values() {
        intern(&constant.type_)?;
    }
    for test in input.tests.values() {
        for parameter in &test.parameters {
            intern(&parameter.type_)?;
        }
    }
    Ok(())
}

struct ArtifactCatalog<'a> {
    templates: &'a BTreeMap<DefinitionId, HirFunction>,
    specialized: &'a BTreeMap<SpecializationId, HirFunction>,
    constants: &'a BTreeMap<DefinitionId, HirConstant>,
    specializations: &'a BTreeMap<SpecializationId, SpecializationRecord>,
    identities: &'a IdentityCatalog,
}

fn type_has_placeholder(type_: &Type) -> bool {
    match type_ {
        Type::Parameter { .. } | Type::Infer => true,
        Type::Array(element) | Type::Option(element) => type_has_placeholder(element),
        Type::Tuple(members) => members.iter().any(type_has_placeholder),
        Type::Result { success, error } => {
            type_has_placeholder(success)
                || error
                    .as_ref()
                    .is_some_and(|error| type_has_placeholder(error))
        }
        Type::Unit
        | Type::Bool
        | Type::Integer(_)
        | Type::Float(_)
        | Type::Text
        | Type::Bytes
        | Type::Builtin(_)
        | Type::Nominal { .. } => false,
    }
}

fn verify_specialized_artifact(
    specialized: &BTreeMap<SpecializationId, HirFunction>,
    catalog: &ArtifactCatalog<'_>,
) -> Result<(), VerificationFailure> {
    for (id, function) in specialized {
        let record =
            catalog
                .specializations
                .get(id)
                .ok_or_else(|| VerificationFailure::Defect {
                    evidence: Arc::from("concrete body has no Specialization record"),
                })?;
        if record.definition != function.id
            || function
                .parameters
                .iter()
                .any(|(_, type_, _)| type_has_placeholder(type_))
            || type_has_placeholder(&function.return_type)
        {
            return defect("concrete Specialization body contains template facts");
        }
        let mut locals = function
            .parameters
            .iter()
            .map(|(local, type_, _)| (*local, type_.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut previous_source_start = function.source.start();
        verify_statements(
            &function.body,
            &function.return_type,
            &mut locals,
            &mut BTreeSet::new(),
            catalog,
            &function.source,
            &mut previous_source_start,
        )?;
        if statements_have_placeholder(&function.body) {
            return defect("concrete Specialization operation contains a placeholder type");
        }
    }
    Ok(())
}

fn statements_have_placeholder(statements: &[Statement]) -> bool {
    fn expression_has_placeholder(expression: &Expression) -> bool {
        type_has_placeholder(&expression.type_)
            || match &expression.kind {
                ExpressionKind::Call { arguments, .. } | ExpressionKind::Array(arguments) => {
                    arguments.iter().any(expression_has_placeholder)
                }
                ExpressionKind::Negate(value)
                | ExpressionKind::Await(value)
                | ExpressionKind::Propagate(value) => expression_has_placeholder(value),
                ExpressionKind::Binary { left, right, .. } => {
                    expression_has_placeholder(left) || expression_has_placeholder(right)
                }
                ExpressionKind::Literal(_)
                | ExpressionKind::Read(_)
                | ExpressionKind::Constant(_) => false,
            }
    }
    statements.iter().any(|statement| match statement {
        Statement::Return { value, .. } => value.as_ref().is_some_and(expression_has_placeholder),
        Statement::Panic { value, .. }
        | Statement::Initialize { value, .. }
        | Statement::Evaluate(value) => expression_has_placeholder(value),
        Statement::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            expression_has_placeholder(condition)
                || statements_have_placeholder(then_branch)
                || statements_have_placeholder(else_branch)
        }
        Statement::Pass(_) => false,
    })
}

fn verify_lowered_artifact(
    functions: &BTreeMap<DefinitionId, HirFunction>,
    specialized: &BTreeMap<SpecializationId, HirFunction>,
    constants: &BTreeMap<DefinitionId, HirConstant>,
    specializations: &BTreeMap<SpecializationId, SpecializationRecord>,
    identities: &IdentityCatalog,
) -> Result<(), VerificationFailure> {
    let catalog = ArtifactCatalog {
        templates: functions,
        specialized,
        constants,
        specializations,
        identities,
    };
    for (key, function) in functions {
        if key != &function.id {
            return defect("lowered function key disagrees with its DefinitionId");
        }
        let mut locals = BTreeMap::new();
        for (local, type_, _) in &function.parameters {
            if locals.insert(*local, type_.clone()).is_some() {
                return defect("lowered function repeats a parameter LocalId");
            }
        }
        let mut moved = BTreeSet::new();
        let mut previous_source_start = function.source.start();
        verify_statements(
            &function.body,
            &function.return_type,
            &mut locals,
            &mut moved,
            &catalog,
            &function.source,
            &mut previous_source_start,
        )?;
    }
    for (key, constant) in constants {
        if key != &constant.id {
            return defect("lowered constant key disagrees with its DefinitionId");
        }
        let actual = verify_expression_artifact(
            &constant.expression,
            &BTreeMap::new(),
            &mut BTreeSet::new(),
            &catalog,
            &constant.source,
        )?;
        if !actual.compatible_with(&constant.type_) {
            return defect("lowered constant expression disagrees with its resolved type");
        }
    }
    verify_specialized_artifact(specialized, &catalog)
}

fn verify_statements(
    statements: &[Statement],
    return_type: &Type,
    locals: &mut BTreeMap<LocalId, Type>,
    moved: &mut BTreeSet<LocalId>,
    catalog: &ArtifactCatalog<'_>,
    provenance_owner: &SourceRange,
    previous_source_start: &mut u64,
) -> Result<(), VerificationFailure> {
    for statement in statements {
        let source = statement_source(statement);
        if source.path() != provenance_owner.path()
            || source.start() > source.end()
            || source.start() < provenance_owner.start()
            || source.end() > provenance_owner.end()
            || source.start() < *previous_source_start
        {
            return defect("lowered statement provenance is outside source order or ownership");
        }
        *previous_source_start = source.start();
        match statement {
            Statement::Return { value, source: _ } => {
                let actual = if let Some(value) = value {
                    verify_expression_artifact(value, locals, moved, catalog, source)?
                } else {
                    Type::Unit
                };
                if !actual.compatible_with(return_type) {
                    return defect("lowered return expression disagrees with function type");
                }
            }
            Statement::Panic { value, .. } | Statement::Evaluate(value) => {
                verify_expression_artifact(value, locals, moved, catalog, source)?;
            }
            Statement::Initialize { place, value, .. } => {
                let type_ = verify_expression_artifact(value, locals, moved, catalog, source)?;
                if locals.insert(place.local, type_).is_some() {
                    return defect("lowered initialization repeats a LocalId");
                }
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                if verify_expression_artifact(condition, locals, moved, catalog, source)?
                    != Type::Bool
                {
                    return defect("lowered if condition is not Bool");
                }
                let mut then_locals = locals.clone();
                let mut then_moved = moved.clone();
                verify_statements(
                    then_branch,
                    return_type,
                    &mut then_locals,
                    &mut then_moved,
                    catalog,
                    provenance_owner,
                    previous_source_start,
                )?;
                let mut else_locals = locals.clone();
                let mut else_moved = moved.clone();
                verify_statements(
                    else_branch,
                    return_type,
                    &mut else_locals,
                    &mut else_moved,
                    catalog,
                    provenance_owner,
                    previous_source_start,
                )?;
                moved.extend(then_moved.union(&else_moved).copied());
            }
            Statement::Pass(_) => {}
        }
    }
    Ok(())
}

fn statement_source(statement: &Statement) -> &SourceRange {
    match statement {
        Statement::Return { source, .. }
        | Statement::Panic { source, .. }
        | Statement::Initialize { source, .. }
        | Statement::If { source, .. }
        | Statement::Pass(source) => source,
        Statement::Evaluate(expression) => &expression.source,
    }
}

fn verify_expression_artifact(
    expression: &Expression,
    locals: &BTreeMap<LocalId, Type>,
    moved: &mut BTreeSet<LocalId>,
    catalog: &ArtifactCatalog<'_>,
    provenance_owner: &SourceRange,
) -> Result<Type, VerificationFailure> {
    if expression.source.start() > expression.source.end()
        || expression.source.path() != provenance_owner.path()
        || expression.source.start() < provenance_owner.start()
        || expression.source.end() > provenance_owner.end()
    {
        return defect("lowered expression has reversed or foreign provenance");
    }
    if !catalog
        .identities
        .type_matches(expression.type_id, &expression.type_)
    {
        return defect("lowered expression TypeId disagrees with its canonical type");
    }
    let children: Vec<&Expression> = match &expression.kind {
        ExpressionKind::Call { arguments, .. } | ExpressionKind::Array(arguments) => {
            arguments.iter().collect()
        }
        ExpressionKind::Negate(value)
        | ExpressionKind::Await(value)
        | ExpressionKind::Propagate(value) => vec![value],
        ExpressionKind::Binary { left, right, .. } => vec![left, right],
        ExpressionKind::Literal(_) | ExpressionKind::Read(_) | ExpressionKind::Constant(_) => {
            Vec::new()
        }
    };
    let mut previous_child_start = expression.source.start();
    for child in children {
        if child.source.path() != provenance_owner.path()
            || child.source.start() < expression.source.start()
            || child.source.end() > expression.source.end()
            || child.source.start() < previous_child_start
        {
            return defect("lowered child expression provenance escapes its owner or source order");
        }
        previous_child_start = child.source.start();
    }
    let actual = match &expression.kind {
        ExpressionKind::Literal(literal) => match literal {
            Literal::Unit => Type::Unit,
            Literal::Bool(_) => Type::Bool,
            Literal::Integer { kind, .. } => Type::Integer(*kind),
            Literal::Float { kind, .. } => Type::Float(*kind),
            Literal::Text(_) => Type::Text,
        },
        ExpressionKind::Read(place) => {
            let type_ =
                locals
                    .get(&place.local)
                    .cloned()
                    .ok_or_else(|| VerificationFailure::Defect {
                        evidence: Arc::from("lowered read references an unknown LocalId"),
                    })?;
            if moved.contains(&place.local) {
                return defect("lowered read uses a LocalId after it was moved");
            }
            if expression.access == AccessMode::Move {
                moved.insert(place.local);
            }
            type_
        }
        ExpressionKind::Constant(id) => catalog
            .constants
            .get(id)
            .map(|constant| constant.type_.clone())
            .ok_or_else(|| VerificationFailure::Defect {
                evidence: Arc::from("lowered expression references an unknown constant"),
            })?,
        ExpressionKind::Call { target, arguments } => {
            let argument_types = arguments
                .iter()
                .map(|argument| {
                    verify_expression_artifact(argument, locals, moved, catalog, &expression.source)
                })
                .collect::<Result<Vec<_>, _>>()?;
            match target {
                CallTarget::TemplateFunction(definition) => catalog
                    .templates
                    .get(definition)
                    .map(|function| {
                        if !arguments_match(&argument_types, &function.parameters) {
                            return defect("template call operands disagree with parameters");
                        }
                        Ok(function.return_type.clone())
                    })
                    .transpose()?
                    .ok_or_else(|| VerificationFailure::Defect {
                        evidence: Arc::from("template call references an unknown function"),
                    })?,
                CallTarget::Function {
                    definition,
                    specialization,
                } => {
                    let record = catalog.specializations.get(specialization).ok_or_else(|| {
                        VerificationFailure::Defect {
                            evidence: Arc::from(
                                "lowered call references an unknown SpecializationId",
                            ),
                        }
                    })?;
                    if record.definition != *definition {
                        return defect("lowered call mixes DefinitionId and SpecializationId");
                    }
                    let function = catalog.specialized.get(specialization).ok_or_else(|| {
                        VerificationFailure::Defect {
                            evidence: Arc::from("lowered call has no concrete specialization body"),
                        }
                    })?;
                    if function.id != *definition
                        || !arguments_match(&argument_types, &function.parameters)
                    {
                        return defect("concrete call operands disagree with specialization");
                    }
                    function.return_type.clone()
                }
                CallTarget::Build(BuildKind::Image) => Type::Builtin(BuiltinType::Image),
                CallTarget::Build(BuildKind::Test) => Type::Builtin(BuiltinType::Test),
                CallTarget::BuiltinVariant(variant) => {
                    let inferred = builtin_variant_type(*variant, arguments, &expression.source)
                        .map_err(|_| VerificationFailure::Defect {
                            evidence: Arc::from("built-in variant operands are malformed"),
                        })?;
                    if !inferred.compatible_with(&expression.type_) {
                        return defect("built-in variant result annotation is inconsistent");
                    }
                    expression.type_.clone()
                }
                CallTarget::UserVariant { id, .. } => {
                    let Type::Nominal { definition, .. } = &expression.type_ else {
                        return defect("user variant result is not its nominal type");
                    };
                    if *definition != id.owner {
                        return defect("user variant owner disagrees with result type");
                    }
                    expression.type_.clone()
                }
                CallTarget::Test(_) => Type::Builtin(BuiltinType::TestApplication),
            }
        }
        ExpressionKind::Array(values) => {
            let Type::Array(element) = &expression.type_ else {
                return defect("array operation result is not an Array type");
            };
            for value in &**values {
                let actual =
                    verify_expression_artifact(value, locals, moved, catalog, &expression.source)?;
                if actual != **element {
                    return defect("array element disagrees with aggregate type");
                }
            }
            expression.type_.clone()
        }
        ExpressionKind::Negate(value) => {
            let type_ =
                verify_expression_artifact(value, locals, moved, catalog, &expression.source)?;
            if !type_.is_numeric() {
                return defect("negate operand is not numeric");
            }
            type_
        }
        ExpressionKind::Await(value) => {
            verify_expression_artifact(value, locals, moved, catalog, &expression.source)?
        }
        ExpressionKind::Propagate(value) => {
            let Type::Result { success, .. } =
                verify_expression_artifact(value, locals, moved, catalog, &expression.source)?
            else {
                return defect("lowered propagation operand is not Result");
            };
            (*success).clone()
        }
        ExpressionKind::Binary {
            operator,
            left,
            right,
        } => {
            let left =
                verify_expression_artifact(left, locals, moved, catalog, &expression.source)?;
            let right =
                verify_expression_artifact(right, locals, moved, catalog, &expression.source)?;
            binary_type(*operator, &left, &right).ok_or_else(|| VerificationFailure::Defect {
                evidence: Arc::from("binary operation operands are not well typed"),
            })?
        }
    };
    if actual != expression.type_ {
        return defect("lowered expression type annotation is inconsistent");
    }
    Ok(actual)
}

fn arguments_match(argument_types: &[Type], parameters: &[(LocalId, Type, AccessMode)]) -> bool {
    argument_types.len() == parameters.len()
        && argument_types
            .iter()
            .zip(parameters)
            .all(|(argument, (_, parameter, _))| argument.compatible_with(parameter))
}

fn validate_input(input: &ProgramInput) -> Result<(), VerificationFailure> {
    for (lookup, function) in &input.functions {
        if lookup != &function.id {
            return defect("function catalog key disagrees with typed identity");
        }
        if function.name.is_empty() {
            return creator(CreatorFailureKind::EmptyName, &function.source);
        }
        let mut names = BTreeSet::new();
        if function
            .parameters
            .iter()
            .any(|parameter| !names.insert(&parameter.name))
        {
            return creator(CreatorFailureKind::DuplicateParameter, &function.source);
        }
        if function.source.start() > function.source.end() {
            return defect("reversed function provenance");
        }
    }
    Ok(())
}

struct Lowerer<'a> {
    module: ModuleId,
    functions: &'a BTreeMap<DefinitionId, ResolvedFunction>,
    constants: &'a BTreeMap<DefinitionId, ResolvedConstant>,
    tests: &'a BTreeMap<TestId, ResolvedTest>,
    names: &'a BTreeMap<NameKey, ResolvedName>,
    nominal_displays: &'a BTreeMap<DefinitionId, Arc<str>>,
    locals: BTreeMap<String, (LocalId, Type)>,
    next_local: u32,
    build_authority: &'a BuildAuthority,
    identity_catalog: &'a mut IdentityCatalog,
    cancellation: &'a Cancellation,
    specializations: &'a mut BTreeMap<SpecializationId, SpecializationRecord>,
    concrete_context: bool,
}

impl<'a> Lowerer<'a> {
    fn new(
        module: ModuleId,
        input: &'a ProgramInput,
        build_authority: &'a BuildAuthority,
        identity_catalog: &'a mut IdentityCatalog,
        cancellation: &'a Cancellation,
        specializations: &'a mut BTreeMap<SpecializationId, SpecializationRecord>,
        concrete_context: bool,
    ) -> Self {
        Self {
            module,
            functions: &input.functions,
            constants: &input.constants,
            tests: &input.tests,
            names: &input.names,
            nominal_displays: &input.nominal_displays,
            locals: BTreeMap::new(),
            next_local: 0,
            build_authority,
            identity_catalog,
            cancellation,
            specializations,
            concrete_context,
        }
    }

    fn bind_local(&mut self, name: &str, type_: Type) -> Result<LocalId, VerificationFailure> {
        let id = LocalId(self.next_local);
        self.next_local =
            self.next_local
                .checked_add(1)
                .ok_or_else(|| VerificationFailure::Defect {
                    evidence: Arc::from("local identity overflow"),
                })?;
        self.locals.insert(name.to_owned(), (id, type_));
        Ok(id)
    }

    fn statements(
        &mut self,
        syntax: &[StatementSyntax],
        return_type: &Type,
    ) -> Result<Vec<Statement>, VerificationFailure> {
        let mut statements = Vec::new();
        for statement in syntax {
            if self.cancellation.is_cancelled() {
                return Err(VerificationFailure::Cancelled);
            }
            statements.push(match statement {
                StatementSyntax::Return { value, range } => {
                    let mut value = value
                        .as_ref()
                        .map(|value| self.expression(value))
                        .transpose()?;
                    let actual = value.as_ref().map_or(&Type::Unit, |value| &value.type_);
                    if !actual.compatible_with(return_type) {
                        return creator(CreatorFailureKind::ReturnTypeMismatch, range);
                    }
                    if let Some(value) = &mut value
                        && matches!(
                            value.kind,
                            ExpressionKind::Call {
                                target: CallTarget::BuiltinVariant(
                                    BuiltinVariant::ResultOk | BuiltinVariant::ResultErr
                                ),
                                ..
                            }
                        )
                    {
                        value.type_ = return_type.clone();
                        value.type_id = self.identity_catalog.intern_type(return_type).map_err(
                            |collision| VerificationFailure::Defect {
                                evidence: Arc::from(format!(
                                    "type identity collision {:032x}",
                                    collision.digest
                                )),
                            },
                        )?;
                    }
                    Statement::Return {
                        value,
                        source: range.clone(),
                    }
                }
                StatementSyntax::Panic { value, range } => Statement::Panic {
                    value: self.expression(value)?,
                    source: range.clone(),
                },
                StatementSyntax::Assign { name, value, range } => {
                    let value = self.expression(value)?;
                    let place = self.bind_local(name, value.type_.clone())?;
                    Statement::Initialize {
                        place: Place { local: place },
                        value,
                        source: range.clone(),
                    }
                }
                StatementSyntax::Evaluate(expression) => {
                    Statement::Evaluate(self.expression(expression)?)
                }
                StatementSyntax::If {
                    condition,
                    then_branch,
                    else_branch,
                    range,
                } => {
                    let condition = self.expression(condition)?;
                    if condition.type_ != Type::Bool {
                        return creator(CreatorFailureKind::IfConditionRequiresBool, range);
                    }
                    let before = self.locals.clone();
                    let then_branch = self.statements(then_branch, return_type)?;
                    self.locals.clone_from(&before);
                    let else_branch = self.statements(else_branch, return_type)?;
                    self.locals = before;
                    Statement::If {
                        condition,
                        then_branch: then_branch.into(),
                        else_branch: else_branch.into(),
                        source: range.clone(),
                    }
                }
                StatementSyntax::Pass(range) => Statement::Pass(range.clone()),
            });
        }
        Ok(statements)
    }

    fn expression(&mut self, syntax: &ExpressionSyntax) -> Result<Expression, VerificationFailure> {
        if self.cancellation.is_cancelled() {
            return Err(VerificationFailure::Cancelled);
        }
        let (kind, type_) = match &syntax.kind {
            ExpressionSyntaxKind::Integer(authored) => {
                let (value, integer) = parse_integer_literal(authored).map_err(|()| {
                    creator_value(CreatorFailureKind::InvalidIntegerLiteral, &syntax.range)
                })?;
                (
                    ExpressionKind::Literal(Literal::Integer {
                        kind: integer,
                        value,
                    }),
                    Type::Integer(integer),
                )
            }
            ExpressionSyntaxKind::Float(authored) => {
                let (value, float) = parse_float_literal(authored).map_err(|()| {
                    creator_value(CreatorFailureKind::InvalidFloatLiteral, &syntax.range)
                })?;
                (
                    ExpressionKind::Literal(Literal::Float {
                        kind: float,
                        bits: encode_float(float, value),
                    }),
                    Type::Float(float),
                )
            }
            ExpressionSyntaxKind::Text(value) => (
                ExpressionKind::Literal(Literal::Text(Arc::from(value.as_str()))),
                Type::Text,
            ),
            ExpressionSyntaxKind::Bool(value) => {
                (ExpressionKind::Literal(Literal::Bool(*value)), Type::Bool)
            }
            ExpressionSyntaxKind::Unit => (ExpressionKind::Literal(Literal::Unit), Type::Unit),
            ExpressionSyntaxKind::Name(name) => return self.value(name, syntax),
            ExpressionSyntaxKind::Call { callee, arguments } => {
                let mut lowered = arguments
                    .iter()
                    .map(|argument| self.expression(&argument.value))
                    .collect::<Result<Vec<_>, _>>()?;
                let (target, type_) = self.call(callee, &lowered, &syntax.range)?;
                if let CallTarget::Function { definition, .. } = &target {
                    let function = &self.functions[definition];
                    for (value, parameter) in lowered.iter_mut().zip(&function.parameters) {
                        value.access = match parameter.ownership {
                            OwnershipSyntax::Value => AccessMode::Copy,
                            OwnershipSyntax::Take => AccessMode::Move,
                        };
                    }
                }
                if let CallTarget::Test(id) = &target {
                    let test = &self.tests[id];
                    for ((syntax_argument, value), parameter) in
                        arguments.iter().zip(&mut lowered).zip(&test.parameters)
                    {
                        if syntax_argument
                            .label
                            .as_ref()
                            .is_some_and(|label| label != &parameter.name)
                        {
                            return creator(
                                CreatorFailureKind::ArgumentLabelMismatch,
                                &syntax_argument.value.range,
                            );
                        }
                        value.access = match parameter.ownership {
                            OwnershipSyntax::Value => AccessMode::Copy,
                            OwnershipSyntax::Take => AccessMode::Move,
                        };
                    }
                }
                (
                    ExpressionKind::Call {
                        target,
                        arguments: lowered.into(),
                    },
                    type_,
                )
            }
            ExpressionSyntaxKind::Array(values) => {
                let values = values
                    .iter()
                    .map(|value| self.expression(value))
                    .collect::<Result<Vec<_>, _>>()?;
                let element = values
                    .first()
                    .map_or(Type::Infer, |value| value.type_.clone());
                if values
                    .iter()
                    .any(|value| !value.type_.compatible_with(&element))
                {
                    return creator(CreatorFailureKind::ArrayElementTypeMismatch, &syntax.range);
                }
                (
                    ExpressionKind::Array(values.into()),
                    Type::Array(Arc::new(element)),
                )
            }
            ExpressionSyntaxKind::Negate(value) => {
                let value = self.expression(value)?;
                if !value.type_.is_numeric() {
                    return creator(CreatorFailureKind::InvalidUnaryOperand, &syntax.range);
                }
                let type_ = value.type_.clone();
                (ExpressionKind::Negate(Box::new(value)), type_)
            }
            ExpressionSyntaxKind::Await(value) => {
                let value = self.expression(value)?;
                let type_ = value.type_.clone();
                (ExpressionKind::Await(Box::new(value)), type_)
            }
            ExpressionSyntaxKind::Propagate(value) => {
                let value = self.expression(value)?;
                let Type::Result { success, .. } = &value.type_ else {
                    return creator(CreatorFailureKind::PropagationRequiresResult, &syntax.range);
                };
                let type_ = (**success).clone();
                (ExpressionKind::Propagate(Box::new(value)), type_)
            }
            ExpressionSyntaxKind::Binary {
                operator,
                left,
                right,
            } => {
                let left = self.expression(left)?;
                let right = self.expression(right)?;
                let operator = BinaryOperator::from(*operator);
                let type_ = binary_type(operator, &left.type_, &right.type_).ok_or_else(|| {
                    creator_value(CreatorFailureKind::BinaryTypeMismatch, &syntax.range)
                })?;
                (
                    ExpressionKind::Binary {
                        operator,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    type_,
                )
            }
        };
        self.finish_expression(kind, type_, syntax.range.clone())
    }

    fn finish_expression(
        &mut self,
        kind: ExpressionKind,
        type_: Type,
        source: SourceRange,
    ) -> Result<Expression, VerificationFailure> {
        let type_id = self
            .identity_catalog
            .intern_type(&type_)
            .map_err(|collision| VerificationFailure::Defect {
                evidence: Arc::from(format!("type identity collision {:032x}", collision.digest)),
            })?;
        Ok(Expression {
            kind,
            type_id,
            type_,
            access: AccessMode::Copy,
            source,
        })
    }

    fn value(
        &mut self,
        name: &NameSyntax,
        syntax: &ExpressionSyntax,
    ) -> Result<Expression, VerificationFailure> {
        if let [local] = name.segments.as_slice()
            && let Some((id, type_)) = self.locals.get(local)
        {
            return self.finish_expression(
                ExpressionKind::Read(Place { local: *id }),
                type_.clone(),
                syntax.range.clone(),
            );
        }
        let key = NameKey::new(self.module, Arc::from(name.segments.clone()));
        if let Some(ResolvedName::Constant(id)) = self.names.get(&key) {
            let constant = self
                .constants
                .get(id)
                .ok_or_else(|| VerificationFailure::Defect {
                    evidence: Arc::from("resolved constant absent from catalog"),
                })?;
            return self.finish_expression(
                ExpressionKind::Constant(*id),
                constant.type_.clone(),
                syntax.range.clone(),
            );
        }
        if resolve_builtin_variant(name).is_some() || name.segments.len() == 2 {
            let (target, type_) = self.call(name, &[], &syntax.range)?;
            return self.finish_expression(
                ExpressionKind::Call {
                    target,
                    arguments: Arc::from([]),
                },
                type_,
                syntax.range.clone(),
            );
        }
        creator(CreatorFailureKind::UnresolvedName, &syntax.range)
    }

    fn call(
        &mut self,
        name: &NameSyntax,
        arguments: &[Expression],
        site: &SourceRange,
    ) -> Result<(CallTarget, Type), VerificationFailure> {
        let key = NameKey::new(self.module, Arc::from(name.segments.clone()));
        if let Some(ResolvedName::Function(id)) = self.names.get(&key) {
            let function = self
                .functions
                .get(id)
                .ok_or_else(|| VerificationFailure::Defect {
                    evidence: Arc::from("resolved function absent from catalog"),
                })?;
            if function.parameters.len() != arguments.len() {
                return creator(CreatorFailureKind::ArgumentCount, site);
            }
            let mut substitutions = BTreeMap::new();
            for (parameter, argument) in function.parameters.iter().zip(arguments) {
                bind_type(&parameter.type_, &argument.type_, &mut substitutions, site)?;
            }
            let return_type = substitute(&function.return_type, &substitutions);
            if !self.concrete_context {
                return Ok((CallTarget::TemplateFunction(*id), return_type));
            }
            let type_arguments = function
                .type_parameters
                .iter()
                .map(|parameter| {
                    substitutions.get(parameter).cloned().ok_or_else(|| {
                        VerificationFailure::Creator {
                            kind: CreatorFailureKind::GenericArgumentConflict,
                            site: site.clone(),
                        }
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let specialization_id = self
                .identity_catalog
                .specialization(*id, &type_arguments)
                .map_err(|collision| VerificationFailure::Defect {
                    evidence: Arc::from(format!(
                        "specialization identity collision {:032x}",
                        collision.digest
                    )),
                })?;
            let specialization = SpecializationRecord {
                id: specialization_id,
                definition: *id,
                type_arguments: type_arguments.into(),
            };
            self.specializations
                .entry(specialization.id)
                .or_insert_with(|| specialization.clone());
            return Ok((
                CallTarget::Function {
                    definition: *id,
                    specialization: specialization.id,
                },
                return_type,
            ));
        }
        if let Some(kind) = resolve_build_kind(name)
            && self.build_authority.permits(kind)
        {
            return Ok((
                CallTarget::Build(kind),
                Type::Builtin(match kind {
                    BuildKind::Image => BuiltinType::Image,
                    BuildKind::Test => BuiltinType::Test,
                }),
            ));
        }
        if let Some(ResolvedName::Test(id)) = self.names.get(&key) {
            let test = self
                .tests
                .get(id)
                .ok_or_else(|| VerificationFailure::Defect {
                    evidence: Arc::from("resolved test absent from catalog"),
                })?;
            if test.parameters.len() != arguments.len() {
                return creator(CreatorFailureKind::ArgumentCount, site);
            }
            for (parameter, argument) in test.parameters.iter().zip(arguments) {
                if !argument.type_.compatible_with(&parameter.type_) {
                    return creator(CreatorFailureKind::ArgumentTypeMismatch, site);
                }
            }
            return Ok((
                CallTarget::Test(*id),
                Type::Builtin(BuiltinType::TestApplication),
            ));
        }
        if let Some(variant) = resolve_builtin_variant(name) {
            return Ok((
                CallTarget::BuiltinVariant(variant),
                builtin_variant_type(variant, arguments, site)?,
            ));
        }
        if name.segments.len() == 2 {
            let owner_key = NameKey::new(self.module, Arc::from([name.segments[0].clone()]));
            let Some(ResolvedName::Nominal(owner)) = self.names.get(&owner_key) else {
                return creator(CreatorFailureKind::UnresolvedNominalType, site);
            };
            let display = self.nominal_displays.get(owner).cloned().ok_or_else(|| {
                VerificationFailure::Defect {
                    evidence: Arc::from("nominal display absent from catalog"),
                }
            })?;
            let Some(variant_id) = self.identity_catalog.variant(*owner, &name.segments[1]) else {
                return creator(CreatorFailureKind::UnresolvedCall, site);
            };
            return Ok((
                CallTarget::UserVariant {
                    id: variant_id,
                    type_display: display.clone(),
                    variant_display: Arc::from(name.segments[1].as_str()),
                },
                Type::Nominal {
                    definition: *owner,
                    display,
                },
            ));
        }
        creator(CreatorFailureKind::UnresolvedCall, site)
    }
}

fn bind_type(
    expected: &Type,
    actual: &Type,
    substitutions: &mut BTreeMap<crate::model::TypeParameterId, Type>,
    site: &SourceRange,
) -> Result<(), VerificationFailure> {
    if let Type::Parameter { id, .. } = expected {
        if let Some(previous) = substitutions.insert(*id, actual.clone())
            && previous != *actual
        {
            return creator(CreatorFailureKind::GenericArgumentConflict, site);
        }
    } else if !actual.compatible_with(expected) {
        return creator(CreatorFailureKind::ArgumentTypeMismatch, site);
    }
    Ok(())
}

fn substitute(type_: &Type, substitutions: &BTreeMap<crate::model::TypeParameterId, Type>) -> Type {
    match type_ {
        Type::Parameter { id, .. } => substitutions
            .get(id)
            .cloned()
            .unwrap_or_else(|| type_.clone()),
        Type::Array(element) => Type::Array(Arc::new(substitute(element, substitutions))),
        Type::Tuple(members) => Type::Tuple(
            members
                .iter()
                .map(|member| substitute(member, substitutions))
                .collect(),
        ),
        Type::Result { success, error } => Type::Result {
            success: Arc::new(substitute(success, substitutions)),
            error: error
                .as_ref()
                .map(|error| Arc::new(substitute(error, substitutions))),
        },
        Type::Option(value) => Type::Option(Arc::new(substitute(value, substitutions))),
        _ => type_.clone(),
    }
}

fn builtin_variant_type(
    variant: BuiltinVariant,
    arguments: &[Expression],
    site: &SourceRange,
) -> Result<Type, VerificationFailure> {
    Ok(match variant {
        BuiltinVariant::ResultOk => Type::Result {
            success: Arc::new(
                arguments
                    .first()
                    .ok_or_else(|| creator_value(CreatorFailureKind::ArgumentCount, site))?
                    .type_
                    .clone(),
            ),
            error: Some(Arc::new(Type::Infer)),
        },
        BuiltinVariant::ResultErr => Type::Result {
            success: Arc::new(Type::Infer),
            error: Some(Arc::new(
                arguments
                    .first()
                    .ok_or_else(|| creator_value(CreatorFailureKind::ArgumentCount, site))?
                    .type_
                    .clone(),
            )),
        },
        BuiltinVariant::OptionSome => Type::Option(Arc::new(
            arguments
                .first()
                .ok_or_else(|| creator_value(CreatorFailureKind::ArgumentCount, site))?
                .type_
                .clone(),
        )),
        BuiltinVariant::OptionNone => Type::Option(Arc::new(Type::Infer)),
    })
}

fn binary_type(operator: BinaryOperator, left: &Type, right: &Type) -> Option<Type> {
    if !left.compatible_with(right) {
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
        return Some(Type::Bool);
    }
    left.is_numeric().then(|| left.clone())
}

impl From<BinaryOperatorSyntax> for BinaryOperator {
    fn from(value: BinaryOperatorSyntax) -> Self {
        match value {
            BinaryOperatorSyntax::Add => Self::Add,
            BinaryOperatorSyntax::Subtract => Self::Subtract,
            BinaryOperatorSyntax::Multiply => Self::Multiply,
            BinaryOperatorSyntax::Divide => Self::Divide,
            BinaryOperatorSyntax::Remainder => Self::Remainder,
            BinaryOperatorSyntax::Equal => Self::Equal,
            BinaryOperatorSyntax::NotEqual => Self::NotEqual,
            BinaryOperatorSyntax::Less => Self::Less,
            BinaryOperatorSyntax::LessEqual => Self::LessEqual,
            BinaryOperatorSyntax::Greater => Self::Greater,
            BinaryOperatorSyntax::GreaterEqual => Self::GreaterEqual,
        }
    }
}

fn parse_integer_literal(source: &str) -> Result<(i128, IntegerType), ()> {
    let kinds = [
        ("u8", IntegerType::U8),
        ("u16", IntegerType::U16),
        ("u32", IntegerType::U32),
        ("u64", IntegerType::U64),
        ("i8", IntegerType::I8),
        ("i16", IntegerType::I16),
        ("i32", IntegerType::I32),
        ("i64", IntegerType::I64),
    ];
    let (digits, kind) = kinds
        .iter()
        .find_map(|(suffix, kind)| source.strip_suffix(suffix).map(|digits| (digits, *kind)))
        .unwrap_or((source, IntegerType::I64));
    let digits = digits.replace('_', "");
    let (radix, digits) = if let Some(digits) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        (16, digits)
    } else if let Some(digits) = digits
        .strip_prefix("0b")
        .or_else(|| digits.strip_prefix("0B"))
    {
        (2, digits)
    } else if let Some(digits) = digits
        .strip_prefix("0o")
        .or_else(|| digits.strip_prefix("0O"))
    {
        (8, digits)
    } else {
        (10, digits.as_str())
    };
    let value = i128::from_str_radix(digits, radix).map_err(|_| ())?;
    kind.fits(value).then_some((value, kind)).ok_or(())
}

fn parse_float_literal(source: &str) -> Result<(f64, FloatType), ()> {
    let kinds = [
        ("f16", FloatType::F16),
        ("f32", FloatType::F32),
        ("f64", FloatType::F64),
    ];
    let (digits, kind) = kinds
        .iter()
        .find_map(|(suffix, kind)| source.strip_suffix(suffix).map(|digits| (digits, *kind)))
        .unwrap_or((source, FloatType::F64));
    Ok((
        digits.replace('_', "").parse::<f64>().map_err(|_| ())?,
        kind,
    ))
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

fn creator<T>(kind: CreatorFailureKind, site: &SourceRange) -> Result<T, VerificationFailure> {
    Err(creator_value(kind, site))
}
fn creator_value(kind: CreatorFailureKind, site: &SourceRange) -> VerificationFailure {
    VerificationFailure::Creator {
        kind,
        site: site.clone(),
    }
}
fn defect<T>(evidence: &'static str) -> Result<T, VerificationFailure> {
    Err(VerificationFailure::Defect {
        evidence: Arc::from(evidence),
    })
}

fn append_part(bytes: &mut Vec<u8>, part: &[u8]) {
    bytes.extend_from_slice(&u64::try_from(part.len()).unwrap_or(u64::MAX).to_be_bytes());
    bytes.extend_from_slice(part);
}

fn append_function(bytes: &mut Vec<u8>, function: &HirFunction) {
    bytes.push(function.modifier as u8);
    bytes.extend_from_slice(
        &u64::try_from(function.parameters.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for (local, type_, access) in &function.parameters {
        bytes.extend_from_slice(&local.0.to_be_bytes());
        append_part(bytes, &type_.canonical_key());
        bytes.push(*access as u8);
    }
    append_part(bytes, &function.return_type.canonical_key());
    append_statements(bytes, &function.body);
}

fn append_statements(bytes: &mut Vec<u8>, statements: &[Statement]) {
    bytes.extend_from_slice(
        &u64::try_from(statements.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for statement in statements {
        match statement {
            Statement::Return { value, source } => {
                bytes.push(1);
                append_range(bytes, source);
                if let Some(value) = value {
                    bytes.push(1);
                    append_expression(bytes, value);
                } else {
                    bytes.push(0);
                }
            }
            Statement::Panic { value, source } => {
                bytes.push(2);
                append_range(bytes, source);
                append_expression(bytes, value);
            }
            Statement::Initialize {
                place,
                value,
                source,
            } => {
                bytes.push(3);
                bytes.extend_from_slice(&place.local.0.to_be_bytes());
                append_range(bytes, source);
                append_expression(bytes, value);
            }
            Statement::Evaluate(value) => {
                bytes.push(4);
                append_expression(bytes, value);
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
                source,
            } => {
                bytes.push(5);
                append_range(bytes, source);
                append_expression(bytes, condition);
                append_statements(bytes, then_branch);
                append_statements(bytes, else_branch);
            }
            Statement::Pass(source) => {
                bytes.push(6);
                append_range(bytes, source);
            }
        }
    }
}

fn append_expression(bytes: &mut Vec<u8>, expression: &Expression) {
    append_range(bytes, &expression.source);
    bytes.extend_from_slice(&expression.type_id.0.to_be_bytes());
    bytes.push(match expression.access {
        AccessMode::Copy => 0,
        AccessMode::Move => 1,
    });
    append_part(bytes, &expression.type_.canonical_key());
    match &expression.kind {
        ExpressionKind::Literal(literal) => {
            bytes.push(0);
            match literal {
                Literal::Unit => bytes.push(0),
                Literal::Bool(value) => {
                    bytes.push(1);
                    bytes.push(u8::from(*value));
                }
                Literal::Integer { kind, value } => {
                    bytes.push(2);
                    append_part(bytes, kind.name().as_bytes());
                    bytes.extend_from_slice(&value.to_be_bytes());
                }
                Literal::Float { kind, bits } => {
                    bytes.push(3);
                    append_part(bytes, kind.name().as_bytes());
                    bytes.extend_from_slice(&bits.to_be_bytes());
                }
                Literal::Text(value) => {
                    bytes.push(4);
                    append_part(bytes, value.as_bytes());
                }
            }
        }
        ExpressionKind::Read(place) => {
            bytes.push(1);
            bytes.extend_from_slice(&place.local.0.to_be_bytes());
        }
        ExpressionKind::Constant(id) => {
            bytes.push(2);
            bytes.extend_from_slice(&id.0.to_be_bytes());
        }
        ExpressionKind::Call { target, arguments } => {
            bytes.push(3);
            match target {
                CallTarget::TemplateFunction(definition) => {
                    bytes.push(0);
                    bytes.extend_from_slice(&definition.0.to_be_bytes());
                }
                CallTarget::Function {
                    definition,
                    specialization,
                } => {
                    bytes.push(1);
                    bytes.extend_from_slice(&definition.0.to_be_bytes());
                    bytes.extend_from_slice(&specialization.0.to_be_bytes());
                }
                CallTarget::Build(kind) => {
                    bytes.push(2);
                    append_part(bytes, kind.name().as_bytes());
                }
                CallTarget::BuiltinVariant(variant) => {
                    bytes.push(3);
                    bytes.push(*variant as u8);
                }
                CallTarget::UserVariant { id, .. } => {
                    bytes.push(4);
                    bytes.extend_from_slice(&id.owner.0.to_be_bytes());
                    bytes.extend_from_slice(&id.variant.to_be_bytes());
                }
                CallTarget::Test(id) => {
                    bytes.push(5);
                    bytes.extend_from_slice(&id.suite.0.to_be_bytes());
                    bytes.extend_from_slice(&id.test.0.to_be_bytes());
                    bytes.extend_from_slice(&id.identity.to_be_bytes());
                }
            }
            bytes.extend_from_slice(
                &u64::try_from(arguments.len())
                    .unwrap_or(u64::MAX)
                    .to_be_bytes(),
            );
            for argument in &**arguments {
                append_expression(bytes, argument);
            }
        }
        ExpressionKind::Array(values) => {
            bytes.push(4);
            bytes.extend_from_slice(
                &u64::try_from(values.len())
                    .unwrap_or(u64::MAX)
                    .to_be_bytes(),
            );
            for value in &**values {
                append_expression(bytes, value);
            }
        }
        ExpressionKind::Negate(value) => {
            bytes.push(5);
            append_expression(bytes, value);
        }
        ExpressionKind::Await(value) => {
            bytes.push(6);
            append_expression(bytes, value);
        }
        ExpressionKind::Propagate(value) => {
            bytes.push(7);
            append_expression(bytes, value);
        }
        ExpressionKind::Binary {
            operator,
            left,
            right,
        } => {
            bytes.push(8);
            bytes.push(*operator as u8);
            append_expression(bytes, left);
            append_expression(bytes, right);
        }
    }
}

fn append_range(bytes: &mut Vec<u8>, range: &SourceRange) {
    append_part(bytes, range.path().as_bytes());
    bytes.extend_from_slice(&range.start().to_be_bytes());
    bytes.extend_from_slice(&range.end().to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_compiler_artifact_is_contained_as_a_verification_defect() {
        let mut identities = crate::identity::IdentityCatalog::empty();
        let id = DefinitionId(1);
        let mut input = ProgramInput::default();
        input.functions.insert(
            DefinitionId(2),
            ResolvedFunction {
                id,
                module: ModuleId(1),
                module_display: "image".to_owned(),
                name: "broken".to_owned(),
                modifier: crate::syntax::FunctionModifier::Ordinary,
                type_parameters: Arc::from([]),
                parameters: Vec::new(),
                return_type: Type::Unit,
                body: Vec::new(),
                source: SourceRange::from_u64("src/image.wr", 1, 2),
            },
        );
        assert!(matches!(
            verify(
                input,
                &BuildAuthority::compiler_distribution(),
                &mut identities,
                &Cancellation::new()
            ),
            Err(VerificationFailure::Defect { .. })
        ));
    }

    #[test]
    fn post_lowering_verifier_recomputes_operation_types() {
        let range = SourceRange::from_u64("src/image.wr", 0, 1);
        let mut identities = crate::identity::IdentityCatalog::empty();
        let bool_id = identities.intern_type(&Type::Bool).expect("type interns");
        let boolean = || Expression {
            kind: ExpressionKind::Literal(Literal::Bool(true)),
            type_id: bool_id,
            type_: Type::Bool,
            access: AccessMode::Copy,
            source: range.clone(),
        };
        let malformed = Expression {
            kind: ExpressionKind::Binary {
                operator: BinaryOperator::Add,
                left: Box::new(boolean()),
                right: Box::new(boolean()),
            },
            type_id: bool_id,
            type_: Type::Bool,
            access: AccessMode::Copy,
            source: range,
        };
        let templates = BTreeMap::new();
        let specialized = BTreeMap::new();
        let constants = BTreeMap::new();
        let specializations = BTreeMap::new();
        let catalog = ArtifactCatalog {
            templates: &templates,
            specialized: &specialized,
            constants: &constants,
            specializations: &specializations,
            identities: &identities,
        };
        assert!(matches!(
            verify_expression_artifact(
                &malformed,
                &BTreeMap::new(),
                &mut BTreeSet::new(),
                &catalog,
                &SourceRange::from_u64("src/image.wr", 0, 1),
            ),
            Err(VerificationFailure::Defect { .. })
        ));

        let owner = SourceRange::from_u64("src/image.wr", 0, 1);
        let mut previous = owner.start();
        assert!(matches!(
            verify_statements(
                &[Statement::Pass(
                    SourceRange::from_u64("src/image.wr", 2, 3,)
                )],
                &Type::Unit,
                &mut BTreeMap::new(),
                &mut BTreeSet::new(),
                &catalog,
                &owner,
                &mut previous,
            ),
            Err(VerificationFailure::Defect { .. })
        ));
    }
}
