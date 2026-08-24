#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use xxhash_rust::xxh3::{Xxh3, xxh3_128};

use crate::identity::IdentityCatalog;
use crate::model::{
    BuildKind, BuiltinType, BuiltinVariant, DefinitionId, FloatType, IntegerType, ModuleId,
    SpecializationId, TestId, Type, TypeId, VariantId, resolve_builtin_variant,
};
use crate::syntax::{
    BinaryOperatorSyntax, ExpressionSyntax, ExpressionSyntaxKind, NameSyntax, OwnershipSyntax,
    StatementSyntax,
};
use crate::type_semantics::{
    CallError, CallableParameter, CallableSignature, LabelMode, can_initialize, can_pass,
    can_return, can_unify,
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
    Read,
    Mut,
    Move,
}

impl AccessMode {
    const fn canonical_tag(self) -> u8 {
        match self {
            Self::Copy => 0x01,
            Self::Read => 0x02,
            Self::Mut => 0x03,
            Self::Move => 0x04,
        }
    }
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
    pub(crate) module: ModuleId,
    pub(crate) body: Vec<StatementSyntax>,
    pub(crate) source: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedVariant {
    pub(crate) parameters: Vec<ResolvedParameter>,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedStruct {
    pub(crate) definition: DefinitionId,
    pub(crate) module: ModuleId,
    pub(crate) display: Arc<str>,
    pub(crate) fields: Vec<ResolvedField>,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedField {
    pub(crate) name: String,
    pub(crate) public: bool,
    pub(crate) type_: Type,
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
    Alias(DefinitionId),
    Test(TestId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NamespaceEntry {
    name: ResolvedName,
    public: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct NamespaceCatalog {
    declarations: BTreeMap<NameKey, NamespaceEntry>,
    bindings: BTreeMap<(ModuleId, String), ModuleId>,
    members: BTreeMap<(DefinitionId, String), (ModuleId, NamespaceEntry)>,
}

impl NamespaceCatalog {
    pub(crate) fn declare(
        &mut self,
        module: ModuleId,
        segments: impl Into<Arc<[String]>>,
        name: ResolvedName,
        public: bool,
    ) {
        self.declarations.insert(
            NameKey::new(module, segments),
            NamespaceEntry { name, public },
        );
    }

    pub(crate) fn bind(&mut self, module: ModuleId, alias: String, target: ModuleId) {
        self.bindings.insert((module, alias), target);
    }

    pub(crate) fn declare_member(
        &mut self,
        owner: DefinitionId,
        module: ModuleId,
        name: String,
        resolved: ResolvedName,
        public: bool,
    ) {
        self.members.insert(
            (owner, name),
            (
                module,
                NamespaceEntry {
                    name: resolved,
                    public,
                },
            ),
        );
    }

    pub(crate) fn resolve_member(
        &self,
        requester: ModuleId,
        owner: DefinitionId,
        name: &str,
    ) -> Option<ResolvedName> {
        let (defining_module, entry) = self.members.get(&(owner, name.to_owned()))?;
        (*defining_module == requester || entry.public).then_some(entry.name)
    }

    pub(crate) fn resolve(&self, module: ModuleId, segments: &[String]) -> Option<ResolvedName> {
        let (target, member_segments, imported) = match segments {
            [alias, rest @ ..] if !rest.is_empty() => self
                .bindings
                .get(&(module, alias.clone()))
                .map_or((module, segments, false), |target| (*target, rest, true)),
            _ => (module, segments, false),
        };
        let entry = self
            .declarations
            .get(&NameKey::new(target, Arc::from(member_segments.to_vec())))?;
        (!imported || entry.public).then_some(entry.name)
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ProgramInput {
    pub(crate) functions: BTreeMap<DefinitionId, ResolvedFunction>,
    pub(crate) constants: BTreeMap<DefinitionId, ResolvedConstant>,
    pub(crate) tests: BTreeMap<TestId, ResolvedTest>,
    pub(crate) variants: BTreeMap<VariantId, ResolvedVariant>,
    pub(crate) structs: BTreeMap<DefinitionId, ResolvedStruct>,
    pub(crate) namespace: NamespaceCatalog,
    pub(crate) nominal_displays: BTreeMap<DefinitionId, Arc<str>>,
    pub(crate) comptime_roots: Vec<(ModuleId, ExpressionSyntax)>,
}

#[derive(Clone, Debug)]
pub(crate) struct VerifiedProgram {
    functions: BTreeMap<DefinitionId, Arc<HirFunction>>,
    specialized_functions: BTreeMap<SpecializationId, Arc<HirFunction>>,
    default_specializations: BTreeMap<DefinitionId, SpecializationId>,
    constants: BTreeMap<DefinitionId, HirConstant>,
    tests: BTreeMap<TestId, ResolvedTest>,
    _test_bodies: BTreeMap<TestId, HirTest>,
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
struct HirTest {
    parameters: Vec<(LocalId, Type, AccessMode)>,
    body: Arc<[Statement]>,
    source: SourceRange,
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
    Expect {
        condition: Expression,
        source: SourceRange,
    },
    Initialize {
        place: Place,
        value: Expression,
        source: SourceRange,
    },
    Assign {
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
    TemplateFunction {
        definition: DefinitionId,
        argument_order: Arc<[u16]>,
    },
    Function {
        definition: DefinitionId,
        specialization: SpecializationId,
        argument_order: Arc<[u16]>,
    },
    Build(BuildPrimitive),
    BuiltinVariant(BuiltinVariant),
    UserVariant {
        id: VariantId,
        type_display: Arc<str>,
        variant_display: Arc<str>,
        argument_order: Arc<[u16]>,
    },
    Struct {
        definition: DefinitionId,
        type_display: Arc<str>,
        field_order: Arc<[Arc<str>]>,
        argument_fields: Arc<[Arc<str>]>,
    },
    Test {
        id: TestId,
        argument_order: Arc<[u16]>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BuildPrimitive {
    pub(crate) identity: u128,
    pub(crate) kind: BuildKind,
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

impl BinaryOperator {
    const fn canonical_tag(self) -> u8 {
        match self {
            Self::Add => 0x01,
            Self::Subtract => 0x02,
            Self::Multiply => 0x03,
            Self::Divide => 0x04,
            Self::Remainder => 0x05,
            Self::Equal => 0x11,
            Self::NotEqual => 0x12,
            Self::Less => 0x13,
            Self::LessEqual => 0x14,
            Self::Greater => 0x15,
            Self::GreaterEqual => 0x16,
        }
    }
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
    ReadAfterMove,
    ImmutableReassignment,
    DuplicateLocal,
    AwaitRequiresAsync,
    ExpectRequiresBool,
    TestApplicationOutsideCases,
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
            Self::ReadAfterMove => "read_after_move",
            Self::ImmutableReassignment => "immutable_reassignment",
            Self::DuplicateLocal => "duplicate_local",
            Self::AwaitRequiresAsync => "await_requires_async",
            Self::ExpectRequiresBool => "expect_requires_bool",
            Self::TestApplicationOutsideCases => "test_application_outside_cases",
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
    grants: BTreeMap<Arc<[String]>, BuildPrimitive>,
    _sealed: SealedAuthority,
}

#[derive(Clone, Debug)]
struct SealedAuthority;

impl BuildAuthority {
    pub(crate) fn compiler_distribution() -> Self {
        let grants = [
            (
                Arc::<[String]>::from(["Image".to_owned(), "new".to_owned()]),
                BuildKind::Image,
            ),
            (
                Arc::<[String]>::from(["Test".to_owned(), "new".to_owned()]),
                BuildKind::Test,
            ),
        ]
        .into_iter()
        .map(|(name, kind)| {
            let mut key = b"wrela.authenticated-build-primitive\0\x01".to_vec();
            for segment in &*name {
                append_part(&mut key, segment.as_bytes());
            }
            (
                name,
                BuildPrimitive {
                    identity: xxh3_128(&key),
                    kind,
                },
            )
        })
        .collect();
        Self {
            grants,
            _sealed: SealedAuthority,
        }
    }

    fn resolve(&self, name: &NameSyntax) -> Option<BuildPrimitive> {
        self.grants.get(name.segments.as_slice()).copied()
    }
}

impl VerifiedProgram {
    pub(crate) fn functions(&self) -> &BTreeMap<DefinitionId, Arc<HirFunction>> {
        &self.functions
    }
    pub(crate) fn constants(&self) -> &BTreeMap<DefinitionId, HirConstant> {
        &self.constants
    }
    pub(crate) fn specialization_function(&self, id: SpecializationId) -> Option<&HirFunction> {
        self.specialized_functions.get(&id).map(AsRef::as_ref)
    }
    pub(crate) fn specialized_functions(&self) -> &BTreeMap<SpecializationId, Arc<HirFunction>> {
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
        if !can_initialize(&expression.type_, &constant.type_) {
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
            function.type_parameters.is_empty(),
        );
        let mut parameters = Vec::new();
        for parameter in &function.parameters {
            let local = lowerer.bind_local(&parameter.name, parameter.type_.clone())?;
            parameters.push((
                local,
                parameter.type_.clone(),
                match parameter.ownership {
                    OwnershipSyntax::Value => AccessMode::Copy,
                    OwnershipSyntax::Read => AccessMode::Read,
                    OwnershipSyntax::Mut => AccessMode::Mut,
                    OwnershipSyntax::Take => AccessMode::Move,
                },
            ));
        }
        let body = lowerer.statements(&function.body, &function.return_type)?;
        functions.insert(
            function.id,
            Arc::new(HirFunction {
                id: function.id,
                name: function.name.clone(),
                module_display: function.module_display.clone(),
                modifier: function.modifier,
                parameters,
                return_type: function.return_type.clone(),
                body: body.into(),
                source: function.source.clone(),
            }),
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
    materialize_missing_specializations(
        &input,
        build_authority,
        identity_catalog,
        cancellation,
        &functions,
        &mut specializations,
        &mut specialized_functions,
    )?;

    let mut test_bodies = BTreeMap::new();
    for test in input.tests.values() {
        let mut lowerer = Lowerer::new(
            test.module,
            &input,
            build_authority,
            identity_catalog,
            cancellation,
            &mut specializations,
            true,
        );
        let mut parameters = Vec::new();
        for parameter in &test.parameters {
            let local = lowerer.bind_local(&parameter.name, parameter.type_.clone())?;
            parameters.push((
                local,
                parameter.type_.clone(),
                match parameter.ownership {
                    OwnershipSyntax::Value => AccessMode::Copy,
                    OwnershipSyntax::Read => AccessMode::Read,
                    OwnershipSyntax::Mut => AccessMode::Mut,
                    OwnershipSyntax::Take => AccessMode::Move,
                },
            ));
        }
        let body: Arc<[Statement]> = lowerer.statements(&test.body, &Type::Unit)?.into();
        if !test.asynchronous && statements_suspend(&body) {
            return creator(CreatorFailureKind::AwaitRequiresAsync, &test.source);
        }
        test_bodies.insert(
            test.id,
            HirTest {
                parameters,
                body,
                source: test.source.clone(),
            },
        );
    }

    materialize_missing_specializations(
        &input,
        build_authority,
        identity_catalog,
        cancellation,
        &functions,
        &mut specializations,
        &mut specialized_functions,
    )?;

    for (id, function) in &specialized_functions {
        let mut implementation = Xxh3::new();
        implementation.extend_from_slice(b"wrela.specialization-implementation\0\x01");
        append_function(&mut implementation, function);
        if !identity_catalog.set_specialization_fingerprint(*id, implementation.digest128()) {
            return defect("specialized body has no identity-catalog observation");
        }
    }
    identity_catalog.finalize();

    let mut canonical = Xxh3::new();
    canonical.extend_from_slice(b"wrela.typed-hir\0\x03");
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
                OwnershipSyntax::Read => 1,
                OwnershipSyntax::Mut => 2,
                OwnershipSyntax::Take => 3,
            });
            append_part(&mut canonical, &parameter.type_.canonical_key());
        }
        let body = &test_bodies[id];
        append_statements(&mut canonical, &body.body);
    }
    append_collection_header(&mut canonical, 7, comptime_expressions.len());
    for (source, expression) in &comptime_expressions {
        append_range(&mut canonical, source);
        append_expression(&mut canonical, expression);
    }
    append_collection_header(&mut canonical, 8, input.variants.len());
    for (id, variant) in &input.variants {
        canonical.extend_from_slice(&id.owner.0.to_be_bytes());
        canonical.extend_from_slice(&id.definition.0.to_be_bytes());
        canonical.extend_from_slice(&id.variant.to_be_bytes());
        for parameter in &variant.parameters {
            append_part(&mut canonical, parameter.name.as_bytes());
            append_part(&mut canonical, &parameter.type_.canonical_key());
        }
    }
    let artifact_catalog = ArtifactCatalog {
        templates: &functions,
        specialized: &specialized_functions,
        constants: &constants,
        specializations: &specializations,
        identities: identity_catalog,
        variants: &input.variants,
        structs: &input.structs,
    };
    verify_lowered_artifact(&artifact_catalog, &test_bodies)?;
    Ok(VerifiedProgram {
        functions,
        specialized_functions,
        default_specializations,
        constants,
        tests: input.tests,
        _test_bodies: test_bodies,
        specializations,
        comptime_expressions,
        fingerprint: canonical.digest128(),
        _verified: Verified,
    })
}

fn materialize_missing_specializations(
    input: &ProgramInput,
    build_authority: &BuildAuthority,
    identity_catalog: &mut IdentityCatalog,
    cancellation: &Cancellation,
    functions: &BTreeMap<DefinitionId, Arc<HirFunction>>,
    specializations: &mut BTreeMap<SpecializationId, SpecializationRecord>,
    specialized_functions: &mut BTreeMap<SpecializationId, Arc<HirFunction>>,
) -> Result<(), VerificationFailure> {
    loop {
        let next = specializations
            .values()
            .find(|record| !specialized_functions.contains_key(&record.id))
            .cloned();
        let Some(record) = next else {
            return Ok(());
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
        if function.type_parameters.is_empty() {
            let concrete = functions.get(&record.definition).cloned().ok_or_else(|| {
                VerificationFailure::Defect {
                    evidence: Arc::from("non-generic specialization has no template body"),
                }
            })?;
            specialized_functions.insert(record.id, concrete);
            continue;
        }
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
            input,
            build_authority,
            identity_catalog,
            cancellation,
            specializations,
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
                    OwnershipSyntax::Read => AccessMode::Read,
                    OwnershipSyntax::Mut => AccessMode::Mut,
                    OwnershipSyntax::Take => AccessMode::Move,
                },
            ));
        }
        let return_type = substitute(&function.return_type, &substitutions);
        let body = lowerer.statements(&function.body, &return_type)?;
        specialized_functions.insert(
            record.id,
            Arc::new(HirFunction {
                id: function.id,
                name: function.name.clone(),
                module_display: function.module_display.clone(),
                modifier: function.modifier,
                parameters,
                return_type,
                body: body.into(),
                source: function.source.clone(),
            }),
        );
    }
}

fn append_collection_header(bytes: &mut impl ByteSink, tag: u8, length: usize) {
    bytes.push(tag);
    bytes.extend_from_slice(&u64::try_from(length).unwrap_or(u64::MAX).to_be_bytes());
}

fn append_argument_order(bytes: &mut impl ByteSink, order: &[u16]) {
    bytes.extend_from_slice(&u64::try_from(order.len()).unwrap_or(u64::MAX).to_be_bytes());
    for parameter in order {
        bytes.extend_from_slice(&parameter.to_be_bytes());
    }
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
    templates: &'a BTreeMap<DefinitionId, Arc<HirFunction>>,
    specialized: &'a BTreeMap<SpecializationId, Arc<HirFunction>>,
    constants: &'a BTreeMap<DefinitionId, HirConstant>,
    specializations: &'a BTreeMap<SpecializationId, SpecializationRecord>,
    identities: &'a IdentityCatalog,
    variants: &'a BTreeMap<VariantId, ResolvedVariant>,
    structs: &'a BTreeMap<DefinitionId, ResolvedStruct>,
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
    specialized: &BTreeMap<SpecializationId, Arc<HirFunction>>,
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
        | Statement::Expect {
            condition: value, ..
        }
        | Statement::Initialize { value, .. }
        | Statement::Assign { value, .. }
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

fn statements_suspend(statements: &[Statement]) -> bool {
    fn expression_suspends(expression: &Expression) -> bool {
        match &expression.kind {
            ExpressionKind::Await(_) => true,
            ExpressionKind::Call { arguments, .. } | ExpressionKind::Array(arguments) => {
                arguments.iter().any(expression_suspends)
            }
            ExpressionKind::Negate(value) | ExpressionKind::Propagate(value) => {
                expression_suspends(value)
            }
            ExpressionKind::Binary { left, right, .. } => {
                expression_suspends(left) || expression_suspends(right)
            }
            ExpressionKind::Literal(_) | ExpressionKind::Read(_) | ExpressionKind::Constant(_) => {
                false
            }
        }
    }
    statements.iter().any(|statement| match statement {
        Statement::Return { value, .. } => value.as_ref().is_some_and(expression_suspends),
        Statement::Panic { value, .. }
        | Statement::Expect {
            condition: value, ..
        }
        | Statement::Initialize { value, .. }
        | Statement::Assign { value, .. }
        | Statement::Evaluate(value) => expression_suspends(value),
        Statement::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            expression_suspends(condition)
                || statements_suspend(then_branch)
                || statements_suspend(else_branch)
        }
        Statement::Pass(_) => false,
    })
}

fn verify_lowered_artifact(
    catalog: &ArtifactCatalog<'_>,
    tests: &BTreeMap<TestId, HirTest>,
) -> Result<(), VerificationFailure> {
    for (key, function) in catalog.templates {
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
            catalog,
            &function.source,
            &mut previous_source_start,
        )?;
    }
    for (key, constant) in catalog.constants {
        if key != &constant.id {
            return defect("lowered constant key disagrees with its DefinitionId");
        }
        let actual = verify_expression_artifact(
            &constant.expression,
            &BTreeMap::new(),
            &mut BTreeSet::new(),
            catalog,
            &constant.source,
        )?;
        if !can_initialize(&actual, &constant.type_) {
            return defect("lowered constant expression disagrees with its resolved type");
        }
    }
    for test in tests.values() {
        let mut locals = test
            .parameters
            .iter()
            .map(|(local, type_, _)| (*local, type_.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut previous_source_start = test.source.start();
        verify_statements(
            &test.body,
            &Type::Unit,
            &mut locals,
            &mut BTreeSet::new(),
            catalog,
            &test.source,
            &mut previous_source_start,
        )?;
        if statements_have_placeholder(&test.body) {
            return defect("verified Test body contains a placeholder type");
        }
    }
    verify_specialized_artifact(catalog.specialized, catalog)
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
                if !can_return(&actual, return_type) {
                    return defect("lowered return expression disagrees with function type");
                }
            }
            Statement::Panic { value, .. } | Statement::Evaluate(value) => {
                verify_expression_artifact(value, locals, moved, catalog, source)?;
            }
            Statement::Expect { condition, .. } => {
                if verify_expression_artifact(condition, locals, moved, catalog, source)?
                    != Type::Bool
                {
                    return defect("lowered expectation condition is not Bool");
                }
            }
            Statement::Initialize { place, value, .. } => {
                let type_ = verify_expression_artifact(value, locals, moved, catalog, source)?;
                if locals.insert(place.local, type_).is_some() {
                    return defect("lowered initialization repeats a LocalId");
                }
            }
            Statement::Assign { place, value, .. } => {
                let type_ = verify_expression_artifact(value, locals, moved, catalog, source)?;
                let Some(expected) = locals.get(&place.local) else {
                    return defect("lowered assignment references an unknown LocalId");
                };
                if !can_initialize(&type_, expected) {
                    return defect("lowered assignment changes the local type");
                }
                moved.remove(&place.local);
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
                if !hir_statements_terminate(then_branch) {
                    moved.extend(then_moved);
                }
                if !hir_statements_terminate(else_branch) {
                    moved.extend(else_moved);
                }
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
        | Statement::Expect { source, .. }
        | Statement::Initialize { source, .. }
        | Statement::Assign { source, .. }
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
                CallTarget::TemplateFunction {
                    definition,
                    argument_order,
                } => catalog
                    .templates
                    .get(definition)
                    .map(|function| {
                        let ordered = reorder_types(&argument_types, argument_order)?;
                        if !arguments_match(&ordered, &function.parameters) {
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
                    argument_order,
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
                    let ordered = reorder_types(&argument_types, argument_order)?;
                    if function.id != *definition
                        || !arguments_match(&ordered, &function.parameters)
                    {
                        return defect("concrete call operands disagree with specialization");
                    }
                    function.return_type.clone()
                }
                CallTarget::Build(primitive) => Type::Builtin(match primitive.kind {
                    BuildKind::Image => BuiltinType::Image,
                    BuildKind::Test => BuiltinType::Test,
                }),
                CallTarget::BuiltinVariant(variant) => {
                    let inferred = builtin_variant_type(*variant, arguments, &expression.source)
                        .map_err(|_| VerificationFailure::Defect {
                            evidence: Arc::from("built-in variant operands are malformed"),
                        })?;
                    if !can_unify(&inferred, &expression.type_) {
                        return defect("built-in variant result annotation is inconsistent");
                    }
                    expression.type_.clone()
                }
                CallTarget::UserVariant {
                    id, argument_order, ..
                } => {
                    let Type::Nominal { definition, .. } = &expression.type_ else {
                        return defect("user variant result is not its nominal type");
                    };
                    if *definition != id.owner {
                        return defect("user variant owner disagrees with result type");
                    }
                    let signature =
                        catalog
                            .variants
                            .get(id)
                            .ok_or_else(|| VerificationFailure::Defect {
                                evidence: Arc::from("user variant has no semantic signature"),
                            })?;
                    let ordered = reorder_types(&argument_types, argument_order)?;
                    if ordered.len() != signature.parameters.len()
                        || ordered
                            .iter()
                            .zip(&signature.parameters)
                            .any(|(argument, parameter)| !can_pass(argument, &parameter.type_))
                    {
                        return defect("user variant operands disagree with its payload");
                    }
                    expression.type_.clone()
                }
                CallTarget::Struct {
                    definition,
                    field_order,
                    argument_fields,
                    ..
                } => {
                    let Type::Nominal {
                        definition: result_definition,
                        ..
                    } = &expression.type_
                    else {
                        return defect("struct construction result is not its nominal type");
                    };
                    if result_definition != definition {
                        return defect("struct construction identity disagrees with result type");
                    }
                    let signature = catalog.structs.get(definition).ok_or_else(|| {
                        VerificationFailure::Defect {
                            evidence: Arc::from("struct construction has no semantic signature"),
                        }
                    })?;
                    let declared_order = signature
                        .fields
                        .iter()
                        .map(|field| field.name.as_str())
                        .collect::<Vec<_>>();
                    if field_order.iter().map(AsRef::as_ref).collect::<Vec<_>>() != declared_order {
                        return defect(
                            "struct construction field order disagrees with declaration",
                        );
                    }
                    if argument_fields.len() != argument_types.len()
                        || argument_fields.len() != signature.fields.len()
                    {
                        return defect("struct construction does not initialize every field");
                    }
                    let mut seen = BTreeSet::new();
                    for (field_name, argument_type) in argument_fields.iter().zip(&argument_types) {
                        if !seen.insert(field_name.as_ref()) {
                            return defect("struct construction initializes a field twice");
                        }
                        let Some(field) = signature
                            .fields
                            .iter()
                            .find(|field| field.name == field_name.as_ref())
                        else {
                            return defect("struct construction initializes an unknown field");
                        };
                        if !can_pass(argument_type, &field.type_) {
                            return defect("struct construction operand disagrees with field type");
                        }
                    }
                    expression.type_.clone()
                }
                CallTarget::Test { argument_order, .. } => {
                    let _ = reorder_types(&argument_types, argument_order)?;
                    Type::Builtin(BuiltinType::TestApplication)
                }
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
            .all(|(argument, (_, parameter, _))| can_pass(argument, parameter))
}

fn reorder_types(
    source: &[Type],
    source_to_parameter: &[u16],
) -> Result<Vec<Type>, VerificationFailure> {
    if source.len() != source_to_parameter.len() {
        return defect("call argument binding length disagrees with operands");
    }
    let mut ordered = vec![None; source.len()];
    for (value, parameter) in source.iter().zip(source_to_parameter) {
        let slot = ordered.get_mut(usize::from(*parameter)).ok_or_else(|| {
            VerificationFailure::Defect {
                evidence: Arc::from("call argument binding names an invalid parameter"),
            }
        })?;
        if slot.replace(value.clone()).is_some() {
            return defect("call argument binding initializes a parameter twice");
        }
    }
    ordered
        .into_iter()
        .map(|value| {
            value.ok_or_else(|| VerificationFailure::Defect {
                evidence: Arc::from("call argument binding omits a parameter"),
            })
        })
        .collect()
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
    variants: &'a BTreeMap<VariantId, ResolvedVariant>,
    structs: &'a BTreeMap<DefinitionId, ResolvedStruct>,
    namespace: &'a NamespaceCatalog,
    nominal_displays: &'a BTreeMap<DefinitionId, Arc<str>>,
    locals: BTreeMap<String, (LocalId, Type, bool)>,
    moved: BTreeSet<LocalId>,
    next_local: u32,
    build_authority: &'a BuildAuthority,
    identity_catalog: &'a mut IdentityCatalog,
    cancellation: &'a Cancellation,
    specializations: &'a mut BTreeMap<SpecializationId, SpecializationRecord>,
    concrete_context: bool,
    test_application_context: u16,
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
            variants: &input.variants,
            structs: &input.structs,
            namespace: &input.namespace,
            nominal_displays: &input.nominal_displays,
            locals: BTreeMap::new(),
            moved: BTreeSet::new(),
            next_local: 0,
            build_authority,
            identity_catalog,
            cancellation,
            specializations,
            concrete_context,
            test_application_context: 0,
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
        self.locals.insert(name.to_owned(), (id, type_, false));
        Ok(id)
    }

    fn bind_source_local(
        &mut self,
        name: &str,
        type_: Type,
        mutable: bool,
        site: &SourceRange,
    ) -> Result<LocalId, VerificationFailure> {
        if self.locals.contains_key(name) {
            return creator(CreatorFailureKind::DuplicateLocal, site);
        }
        let id = self.bind_local(name, type_)?;
        self.locals.get_mut(name).expect("new local").2 = mutable;
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
                    if !can_return(actual, return_type) {
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
                StatementSyntax::Expect { condition, range } => {
                    let condition = self.expression(condition)?;
                    if condition.type_ != Type::Bool {
                        return creator(CreatorFailureKind::ExpectRequiresBool, range);
                    }
                    Statement::Expect {
                        condition,
                        source: range.clone(),
                    }
                }
                StatementSyntax::Assign {
                    name,
                    mutable_binding,
                    value,
                    range,
                } => {
                    let value = self.expression(value)?;
                    if let Some((place, type_, mutable)) = self.locals.get(name).cloned() {
                        if *mutable_binding {
                            return creator(CreatorFailureKind::DuplicateLocal, range);
                        }
                        if !can_initialize(&value.type_, &type_) {
                            return creator(CreatorFailureKind::ArgumentTypeMismatch, range);
                        }
                        if !mutable && !self.moved.remove(&place) {
                            return creator(CreatorFailureKind::ImmutableReassignment, range);
                        }
                        Statement::Assign {
                            place: Place { local: place },
                            value,
                            source: range.clone(),
                        }
                    } else {
                        let place = self.bind_source_local(
                            name,
                            value.type_.clone(),
                            *mutable_binding,
                            range,
                        )?;
                        Statement::Initialize {
                            place: Place { local: place },
                            value,
                            source: range.clone(),
                        }
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
                    let moved_before = self.moved.clone();
                    let then_terminates = syntax_statements_terminate(then_branch);
                    let else_terminates = syntax_statements_terminate(else_branch);
                    let then_branch = self.statements(then_branch, return_type)?;
                    let then_moved = self.moved.clone();
                    self.locals.clone_from(&before);
                    self.moved.clone_from(&moved_before);
                    let else_branch = self.statements(else_branch, return_type)?;
                    let else_moved = self.moved.clone();
                    self.locals = before;
                    self.moved = moved_before;
                    if !then_terminates {
                        self.moved.extend(then_moved);
                    }
                    if !else_terminates {
                        self.moved.extend(else_moved);
                    }
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
                let receiver = match callee.segments.as_slice() {
                    [receiver, member] if self.locals.contains_key(receiver) => {
                        let receiver_expression = self.value(
                            &NameSyntax {
                                segments: vec![receiver.clone()],
                            },
                            syntax,
                        )?;
                        let Type::Nominal { definition, .. } = &receiver_expression.type_ else {
                            return creator(CreatorFailureKind::UnresolvedCall, &syntax.range);
                        };
                        let Some(ResolvedName::Function(id)) =
                            self.namespace
                                .resolve_member(self.module, *definition, member)
                        else {
                            return creator(CreatorFailureKind::UnresolvedCall, &syntax.range);
                        };
                        Some((receiver_expression, id))
                    }
                    _ => None,
                };
                let mut lowered = Vec::new();
                let mut labels = Vec::new();
                if let Some((receiver, _)) = &receiver {
                    lowered.push(receiver.clone());
                    labels.push(None);
                }
                let inside_test_cases = self
                    .build_authority
                    .resolve(callee)
                    .is_some_and(|primitive| primitive.kind == BuildKind::Test);
                for argument in arguments {
                    let establishes_context =
                        inside_test_cases && argument.label.as_deref() == Some("cases");
                    if establishes_context {
                        self.test_application_context =
                            self.test_application_context.saturating_add(1);
                    }
                    let value = self.expression(&argument.value);
                    if establishes_context {
                        self.test_application_context -= 1;
                    }
                    lowered.push(value?);
                }
                labels.extend(
                    arguments
                        .iter()
                        .map(|argument| argument.label.clone())
                        .collect::<Vec<_>>(),
                );
                let (target, type_) = if let Some((_, id)) = receiver {
                    self.function_call(id, &lowered, &labels, &syntax.range)?
                } else {
                    self.call(callee, &lowered, &labels, &syntax.range)?
                };
                if let CallTarget::Function {
                    definition,
                    argument_order,
                    ..
                } = &target
                {
                    let function = &self.functions[definition];
                    for (source_index, value) in lowered.iter_mut().enumerate() {
                        let parameter =
                            &function.parameters[usize::from(argument_order[source_index])];
                        value.access = match parameter.ownership {
                            OwnershipSyntax::Value => AccessMode::Copy,
                            OwnershipSyntax::Read => AccessMode::Read,
                            OwnershipSyntax::Mut => AccessMode::Mut,
                            OwnershipSyntax::Take => AccessMode::Move,
                        };
                        self.record_move(value)?;
                    }
                }
                if let CallTarget::Test { id, argument_order } = &target {
                    if self.test_application_context == 0 {
                        return creator(
                            CreatorFailureKind::TestApplicationOutsideCases,
                            &syntax.range,
                        );
                    }
                    let test = &self.tests[id];
                    for (source_index, value) in lowered.iter_mut().enumerate() {
                        let parameter = &test.parameters[usize::from(argument_order[source_index])];
                        value.access = match parameter.ownership {
                            OwnershipSyntax::Value => AccessMode::Copy,
                            OwnershipSyntax::Read => AccessMode::Read,
                            OwnershipSyntax::Mut => AccessMode::Mut,
                            OwnershipSyntax::Take => AccessMode::Move,
                        };
                        self.record_move(value)?;
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
                    .any(|value| !can_unify(&value.type_, &element))
                {
                    return creator(CreatorFailureKind::ArrayElementTypeMismatch, &syntax.range);
                }
                (
                    ExpressionKind::Array(values.into()),
                    Type::Array(Arc::new(element)),
                )
            }
            ExpressionSyntaxKind::Negate(value) => {
                if let ExpressionSyntaxKind::Integer(authored) = &value.kind {
                    let (_, kind) = parse_integer_parts(authored).map_err(|()| {
                        creator_value(CreatorFailureKind::InvalidIntegerLiteral, &syntax.range)
                    })?;
                    if !kind.is_signed() {
                        return creator(CreatorFailureKind::InvalidUnaryOperand, &syntax.range);
                    }
                    let (value, kind) = parse_negated_integer_literal(authored).map_err(|()| {
                        creator_value(CreatorFailureKind::InvalidIntegerLiteral, &syntax.range)
                    })?;
                    (
                        ExpressionKind::Literal(Literal::Integer { kind, value }),
                        Type::Integer(kind),
                    )
                } else {
                    let value = self.expression(value)?;
                    let can_negate = match value.type_ {
                        Type::Float(_) => true,
                        Type::Integer(kind) => kind.is_signed(),
                        _ => false,
                    };
                    if !can_negate {
                        return creator(CreatorFailureKind::InvalidUnaryOperand, &syntax.range);
                    }
                    let type_ = value.type_.clone();
                    (ExpressionKind::Negate(Box::new(value)), type_)
                }
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

    fn record_move(&mut self, expression: &Expression) -> Result<(), VerificationFailure> {
        if expression.access != AccessMode::Move {
            return Ok(());
        }
        if let ExpressionKind::Read(place) = expression.kind
            && !self.moved.insert(place.local)
        {
            return creator(CreatorFailureKind::ReadAfterMove, &expression.source);
        }
        Ok(())
    }

    fn value(
        &mut self,
        name: &NameSyntax,
        syntax: &ExpressionSyntax,
    ) -> Result<Expression, VerificationFailure> {
        if let [local] = name.segments.as_slice()
            && let Some((id, type_, _)) = self.locals.get(local)
        {
            if self.moved.contains(id) {
                return creator(CreatorFailureKind::ReadAfterMove, &syntax.range);
            }
            return self.finish_expression(
                ExpressionKind::Read(Place { local: *id }),
                type_.clone(),
                syntax.range.clone(),
            );
        }
        if let Some(ResolvedName::Constant(id)) =
            self.namespace.resolve(self.module, &name.segments)
        {
            let constant = self
                .constants
                .get(&id)
                .ok_or_else(|| VerificationFailure::Defect {
                    evidence: Arc::from("resolved constant absent from catalog"),
                })?;
            return self.finish_expression(
                ExpressionKind::Constant(id),
                constant.type_.clone(),
                syntax.range.clone(),
            );
        }
        if resolve_builtin_variant(name).is_some() || name.segments.len() == 2 {
            let (target, type_) = self.call(name, &[], &[], &syntax.range)?;
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
        labels: &[Option<String>],
        site: &SourceRange,
    ) -> Result<(CallTarget, Type), VerificationFailure> {
        if let Some(ResolvedName::Function(id)) =
            self.namespace.resolve(self.module, &name.segments)
        {
            return self.function_call(id, arguments, labels, site);
        }
        if let Some(ResolvedName::Nominal(id)) = self.namespace.resolve(self.module, &name.segments)
            && let Some(struct_) = self.structs.get(&id)
        {
            if struct_.definition != id {
                return defect("struct catalog key disagrees with DefinitionId");
            }
            if struct_.module != self.module && struct_.fields.iter().any(|field| !field.public) {
                return creator(CreatorFailureKind::UnresolvedCall, site);
            }
            let signature = CallableSignature {
                parameters: struct_
                    .fields
                    .iter()
                    .map(|field| CallableParameter {
                        name: &field.name,
                        type_: &field.type_,
                    })
                    .collect(),
                label_mode: LabelMode::Required,
            };
            check_callable_signature(&signature, arguments, labels, site)?;
            let argument_fields = labels
                .iter()
                .map(|label| {
                    label.as_deref().map(Arc::from).ok_or_else(|| {
                        creator_value(CreatorFailureKind::ArgumentLabelMismatch, site)
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            return Ok((
                CallTarget::Struct {
                    definition: id,
                    type_display: struct_.display.clone(),
                    field_order: struct_
                        .fields
                        .iter()
                        .map(|field| Arc::from(field.name.as_str()))
                        .collect(),
                    argument_fields: argument_fields.into(),
                },
                Type::Nominal {
                    definition: id,
                    display: struct_.display.clone(),
                },
            ));
        }
        if let Some(primitive) = self.build_authority.resolve(name) {
            match primitive.kind {
                BuildKind::Image => {
                    let mut seen = BTreeSet::new();
                    if labels.iter().any(|label| {
                        label
                            .as_ref()
                            .is_none_or(|label| !seen.insert(label.clone()))
                    }) {
                        return creator(CreatorFailureKind::ArgumentLabelMismatch, site);
                    }
                }
                BuildKind::Test => {
                    let cases_type =
                        Type::Array(Arc::new(Type::Builtin(BuiltinType::TestApplication)));
                    let signature = CallableSignature {
                        parameters: vec![CallableParameter {
                            name: "cases",
                            type_: &cases_type,
                        }],
                        label_mode: LabelMode::Required,
                    };
                    check_callable_signature(&signature, arguments, labels, site)?;
                }
            }
            return Ok((
                CallTarget::Build(primitive),
                Type::Builtin(match primitive.kind {
                    BuildKind::Image => BuiltinType::Image,
                    BuildKind::Test => BuiltinType::Test,
                }),
            ));
        }
        if let Some(ResolvedName::Test(id)) = self.namespace.resolve(self.module, &name.segments) {
            let test = self
                .tests
                .get(&id)
                .ok_or_else(|| VerificationFailure::Defect {
                    evidence: Arc::from("resolved test absent from catalog"),
                })?;
            let argument_order = check_callable(
                &test.parameters,
                arguments,
                labels,
                LabelMode::Optional,
                site,
            )?;
            return Ok((
                CallTarget::Test { id, argument_order },
                Type::Builtin(BuiltinType::TestApplication),
            ));
        }
        if let Some(variant) = resolve_builtin_variant(name) {
            let inferred = Type::Infer;
            let parameters = match variant {
                BuiltinVariant::OptionNone => Vec::new(),
                BuiltinVariant::ResultOk | BuiltinVariant::OptionSome => {
                    vec![CallableParameter {
                        name: "value",
                        type_: &inferred,
                    }]
                }
                BuiltinVariant::ResultErr => vec![CallableParameter {
                    name: "error",
                    type_: &inferred,
                }],
            };
            check_callable_signature(
                &CallableSignature {
                    parameters,
                    label_mode: LabelMode::Optional,
                },
                arguments,
                labels,
                site,
            )?;
            return Ok((
                CallTarget::BuiltinVariant(variant),
                builtin_variant_type(variant, arguments, site)?,
            ));
        }
        if name.segments.len() == 2 {
            let Some(ResolvedName::Nominal(owner)) =
                self.namespace.resolve(self.module, &name.segments[..1])
            else {
                return creator(CreatorFailureKind::UnresolvedNominalType, site);
            };
            let display = self.nominal_displays.get(&owner).cloned().ok_or_else(|| {
                VerificationFailure::Defect {
                    evidence: Arc::from("nominal display absent from catalog"),
                }
            })?;
            let Some(variant_id) = self.identity_catalog.variant(owner, &name.segments[1]) else {
                return creator(CreatorFailureKind::UnresolvedCall, site);
            };
            let variant =
                self.variants
                    .get(&variant_id)
                    .ok_or_else(|| VerificationFailure::Defect {
                        evidence: Arc::from("resolved variant absent from semantic catalog"),
                    })?;
            let argument_order = check_callable(
                &variant.parameters,
                arguments,
                labels,
                LabelMode::Optional,
                site,
            )?;
            return Ok((
                CallTarget::UserVariant {
                    id: variant_id,
                    type_display: display.clone(),
                    variant_display: Arc::from(name.segments[1].as_str()),
                    argument_order,
                },
                Type::Nominal {
                    definition: owner,
                    display,
                },
            ));
        }
        creator(CreatorFailureKind::UnresolvedCall, site)
    }

    fn function_call(
        &mut self,
        id: DefinitionId,
        arguments: &[Expression],
        labels: &[Option<String>],
        site: &SourceRange,
    ) -> Result<(CallTarget, Type), VerificationFailure> {
        let function = self
            .functions
            .get(&id)
            .ok_or_else(|| VerificationFailure::Defect {
                evidence: Arc::from("resolved function absent from catalog"),
            })?;
        let argument_order = check_callable(
            &function.parameters,
            arguments,
            labels,
            LabelMode::Optional,
            site,
        )?;
        let mut substitutions = BTreeMap::new();
        for (source_index, parameter_index) in argument_order.iter().enumerate() {
            bind_type(
                &function.parameters[usize::from(*parameter_index)].type_,
                &arguments[source_index].type_,
                &mut substitutions,
                site,
            )?;
        }
        let return_type = substitute(&function.return_type, &substitutions);
        if !self.concrete_context {
            return Ok((
                CallTarget::TemplateFunction {
                    definition: id,
                    argument_order,
                },
                return_type,
            ));
        }
        let type_arguments = function
            .type_parameters
            .iter()
            .map(|parameter| {
                substitutions
                    .get(parameter)
                    .cloned()
                    .ok_or_else(|| VerificationFailure::Creator {
                        kind: CreatorFailureKind::GenericArgumentConflict,
                        site: site.clone(),
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let specialization_id = self
            .identity_catalog
            .specialization(id, &type_arguments)
            .map_err(|collision| VerificationFailure::Defect {
                evidence: Arc::from(format!(
                    "specialization identity collision {:032x}",
                    collision.digest
                )),
            })?;
        let specialization = SpecializationRecord {
            id: specialization_id,
            definition: id,
            type_arguments: type_arguments.into(),
        };
        self.specializations
            .entry(specialization.id)
            .or_insert_with(|| specialization.clone());
        Ok((
            CallTarget::Function {
                definition: id,
                specialization: specialization.id,
                argument_order,
            },
            return_type,
        ))
    }
}

fn bind_type(
    expected: &Type,
    actual: &Type,
    substitutions: &mut BTreeMap<crate::model::TypeParameterId, Type>,
    site: &SourceRange,
) -> Result<(), VerificationFailure> {
    match (expected, actual) {
        (Type::Parameter { id, .. }, actual) => {
            if let Some(previous) = substitutions.insert(*id, actual.clone())
                && previous != *actual
            {
                return creator(CreatorFailureKind::GenericArgumentConflict, site);
            }
            Ok(())
        }
        (Type::Array(expected), Type::Array(actual))
        | (Type::Option(expected), Type::Option(actual)) => {
            bind_type(expected, actual, substitutions, site)
        }
        (Type::Tuple(expected), Type::Tuple(actual)) if expected.len() == actual.len() => {
            for (expected, actual) in expected.iter().zip(actual.iter()) {
                bind_type(expected, actual, substitutions, site)?;
            }
            Ok(())
        }
        (
            Type::Result {
                success: expected_success,
                error: expected_error,
            },
            Type::Result {
                success: actual_success,
                error: actual_error,
            },
        ) => {
            bind_type(expected_success, actual_success, substitutions, site)?;
            match (expected_error, actual_error) {
                (Some(expected), Some(actual)) => bind_type(expected, actual, substitutions, site),
                (None, _) => Ok(()),
                (Some(_), None) => creator(CreatorFailureKind::ArgumentTypeMismatch, site),
            }
        }
        _ if can_pass(actual, expected) => Ok(()),
        _ => creator(CreatorFailureKind::ArgumentTypeMismatch, site),
    }
}

fn syntax_statements_terminate(statements: &[StatementSyntax]) -> bool {
    statements.iter().any(|statement| match statement {
        StatementSyntax::Return { .. } | StatementSyntax::Panic { .. } => true,
        StatementSyntax::If {
            then_branch,
            else_branch,
            ..
        } => {
            !else_branch.is_empty()
                && syntax_statements_terminate(then_branch)
                && syntax_statements_terminate(else_branch)
        }
        StatementSyntax::Expect { .. }
        | StatementSyntax::Assign { .. }
        | StatementSyntax::Evaluate(_)
        | StatementSyntax::Pass(_) => false,
    })
}

fn hir_statements_terminate(statements: &[Statement]) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Return { .. } | Statement::Panic { .. } => true,
        Statement::If {
            then_branch,
            else_branch,
            ..
        } => {
            !else_branch.is_empty()
                && hir_statements_terminate(then_branch)
                && hir_statements_terminate(else_branch)
        }
        Statement::Expect { .. }
        | Statement::Initialize { .. }
        | Statement::Assign { .. }
        | Statement::Evaluate(_)
        | Statement::Pass(_) => false,
    })
}

fn check_callable(
    parameters: &[ResolvedParameter],
    arguments: &[Expression],
    labels: &[Option<String>],
    label_mode: LabelMode,
    site: &SourceRange,
) -> Result<Arc<[u16]>, VerificationFailure> {
    let signature = CallableSignature {
        parameters: parameters
            .iter()
            .map(|parameter| CallableParameter {
                name: &parameter.name,
                type_: &parameter.type_,
            })
            .collect(),
        label_mode,
    };
    check_callable_signature(&signature, arguments, labels, site)
}

fn check_callable_signature(
    signature: &CallableSignature<'_>,
    arguments: &[Expression],
    labels: &[Option<String>],
    site: &SourceRange,
) -> Result<Arc<[u16]>, VerificationFailure> {
    signature
        .check(
            &arguments
                .iter()
                .map(|argument| argument.type_.clone())
                .collect::<Vec<_>>(),
            labels,
        )
        .map_err(|error| creator_value(call_error_kind(error), site))?
        .source_to_parameter
        .into_iter()
        .map(|index| {
            u16::try_from(index).map_err(|_| VerificationFailure::Defect {
                evidence: Arc::from("callable parameter index exceeds the HIR representation"),
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Into::into)
}

const fn call_error_kind(error: CallError) -> CreatorFailureKind {
    match error {
        CallError::Count => CreatorFailureKind::ArgumentCount,
        CallError::Label => CreatorFailureKind::ArgumentLabelMismatch,
        CallError::Type => CreatorFailureKind::ArgumentTypeMismatch,
    }
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
    if !can_unify(left, right) {
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
    let (magnitude, kind) = parse_integer_parts(source)?;
    let value = i128::try_from(magnitude).map_err(|_| ())?;
    kind.fits(value).then_some((value, kind)).ok_or(())
}

fn parse_negated_integer_literal(source: &str) -> Result<(i128, IntegerType), ()> {
    let (magnitude, kind) = parse_integer_parts(source)?;
    if !kind.is_signed() {
        return Err(());
    }
    let magnitude = i128::try_from(magnitude).map_err(|_| ())?;
    let value = magnitude.checked_neg().ok_or(())?;
    kind.fits(value).then_some((value, kind)).ok_or(())
}

fn parse_integer_parts(source: &str) -> Result<(u128, IntegerType), ()> {
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
    let value = u128::from_str_radix(digits, radix).map_err(|_| ())?;
    Ok((value, kind))
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

trait ByteSink {
    fn push(&mut self, byte: u8);
    fn extend_from_slice(&mut self, bytes: &[u8]);
}

impl ByteSink for Vec<u8> {
    fn push(&mut self, byte: u8) {
        Vec::push(self, byte);
    }

    fn extend_from_slice(&mut self, bytes: &[u8]) {
        Vec::extend_from_slice(self, bytes);
    }
}

impl ByteSink for Xxh3 {
    fn push(&mut self, byte: u8) {
        self.update(&[byte]);
    }

    fn extend_from_slice(&mut self, bytes: &[u8]) {
        self.update(bytes);
    }
}

fn append_part(bytes: &mut impl ByteSink, part: &[u8]) {
    bytes.extend_from_slice(&u64::try_from(part.len()).unwrap_or(u64::MAX).to_be_bytes());
    bytes.extend_from_slice(part);
}

fn append_function(bytes: &mut impl ByteSink, function: &HirFunction) {
    bytes.push(function.modifier.canonical_tag());
    bytes.extend_from_slice(
        &u64::try_from(function.parameters.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for (local, type_, access) in &function.parameters {
        bytes.extend_from_slice(&local.0.to_be_bytes());
        append_part(bytes, &type_.canonical_key());
        bytes.push(access.canonical_tag());
    }
    append_part(bytes, &function.return_type.canonical_key());
    append_statements(bytes, &function.body);
}

fn append_statements(bytes: &mut impl ByteSink, statements: &[Statement]) {
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
            Statement::Expect { condition, source } => {
                bytes.push(8);
                append_range(bytes, source);
                append_expression(bytes, condition);
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
            Statement::Assign {
                place,
                value,
                source,
            } => {
                bytes.push(7);
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

fn append_expression(bytes: &mut impl ByteSink, expression: &Expression) {
    append_range(bytes, &expression.source);
    bytes.extend_from_slice(&expression.type_id.0.to_be_bytes());
    bytes.push(match expression.access {
        AccessMode::Copy => 0,
        AccessMode::Read => 1,
        AccessMode::Mut => 2,
        AccessMode::Move => 3,
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
                CallTarget::TemplateFunction {
                    definition,
                    argument_order,
                } => {
                    bytes.push(0);
                    bytes.extend_from_slice(&definition.0.to_be_bytes());
                    append_argument_order(bytes, argument_order);
                }
                CallTarget::Function {
                    definition,
                    specialization,
                    argument_order,
                } => {
                    bytes.push(1);
                    bytes.extend_from_slice(&definition.0.to_be_bytes());
                    bytes.extend_from_slice(&specialization.0.to_be_bytes());
                    append_argument_order(bytes, argument_order);
                }
                CallTarget::Build(primitive) => {
                    bytes.push(2);
                    bytes.extend_from_slice(&primitive.identity.to_be_bytes());
                    append_part(bytes, primitive.kind.name().as_bytes());
                }
                CallTarget::BuiltinVariant(variant) => {
                    bytes.push(3);
                    bytes.push(variant.canonical_tag());
                }
                CallTarget::UserVariant {
                    id, argument_order, ..
                } => {
                    bytes.push(4);
                    bytes.extend_from_slice(&id.owner.0.to_be_bytes());
                    bytes.extend_from_slice(&id.variant.to_be_bytes());
                    append_argument_order(bytes, argument_order);
                }
                CallTarget::Test { id, argument_order } => {
                    bytes.push(5);
                    bytes.extend_from_slice(&id.suite.0.to_be_bytes());
                    bytes.extend_from_slice(&id.test.0.to_be_bytes());
                    bytes.extend_from_slice(&id.identity.to_be_bytes());
                    append_argument_order(bytes, argument_order);
                }
                CallTarget::Struct {
                    definition,
                    field_order,
                    argument_fields,
                    ..
                } => {
                    bytes.push(6);
                    bytes.extend_from_slice(&definition.0.to_be_bytes());
                    for field in &**field_order {
                        append_part(bytes, field.as_bytes());
                    }
                    bytes.push(0xff);
                    for field in &**argument_fields {
                        append_part(bytes, field.as_bytes());
                    }
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
            bytes.push(operator.canonical_tag());
            append_expression(bytes, left);
            append_expression(bytes, right);
        }
    }
}

fn append_range(bytes: &mut impl ByteSink, range: &SourceRange) {
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
        let variants = BTreeMap::new();
        let structs = BTreeMap::new();
        let catalog = ArtifactCatalog {
            templates: &templates,
            specialized: &specialized,
            constants: &constants,
            specializations: &specializations,
            identities: &identities,
            variants: &variants,
            structs: &structs,
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
