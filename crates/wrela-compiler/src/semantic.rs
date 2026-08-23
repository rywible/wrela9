#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::compiler::{
    Cancellation, ConstructionObservation, Defect, Diagnostic, DiagnosticLabelRole,
    EvaluationObservation, EvaluationOutcome, FunctionFactsObservation, FunctionFactsValues,
    IdentityDomain, InferredErrorObservation, OwnershipMode, OwnershipObservation, ProjectFile,
    RecoveryAction, Root, SourceRange, SpecializationObservation, TestApplicationObservation,
    TestBindingObservation, TypeObservation, TypeRole,
};
use crate::evaluator::{Construction, Engine};
use crate::identity::IdentityCatalog;
use crate::model::{
    BuildKind, BuiltinVariant, DefinitionId, ModuleId, SpecializationId, TestId, Type,
    TypeParameterId, resolve_builtin_type,
};
use crate::syntax::{
    AttributeSyntax, Declaration, DeclarationKind, DeclarationSyntax, FunctionSyntax,
    OwnershipSyntax, ParsedSource, TypeSyntax,
};
use crate::typed_hir::{
    self, BuildAuthority, CallTarget, Expression, ExpressionKind, NameKey, ProgramInput,
    ResolvedConstant, ResolvedFunction, ResolvedName, ResolvedParameter, ResolvedTest, Statement,
    VerifiedProgram,
};

pub(crate) struct Analysis {
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) function_facts: Vec<FunctionFactsObservation>,
    pub(crate) types: Vec<TypeObservation>,
    pub(crate) ownership: Vec<OwnershipObservation>,
    pub(crate) specializations: Vec<SpecializationObservation>,
    pub(crate) inferred_errors: Vec<InferredErrorObservation>,
    pub(crate) evaluations: Vec<EvaluationObservation>,
    pub(crate) constructions: Vec<ConstructionObservation>,
    pub(crate) test_plan: Vec<TestApplicationObservation>,
    pub(crate) defect: Option<Defect>,
    pub(crate) cancelled: bool,
}

#[derive(Clone)]
struct DefinitionRecord {
    id: DefinitionId,
    module: ModuleId,
    module_display: String,
    path: String,
    name: String,
    kind: DeclarationKind,
    public: bool,
    declaration: Declaration,
}

#[derive(Clone)]
struct TestRecord {
    id: TestId,
    range: SourceRange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FunctionFacts {
    pure: bool,
    may_panic: bool,
    suspends: bool,
    evaluator_eligible: bool,
    ownership_transfer: bool,
    bounded: bool,
    logical_cost: u64,
    constructs: BTreeSet<BuildKind>,
    calls: BTreeSet<DefinitionId>,
}

pub(crate) fn analyze<'a>(
    parsed_sources: &BTreeMap<String, ParsedSource>,
    _files: &BTreeMap<&'a str, &'a ProjectFile>,
    identity_catalog: &mut IdentityCatalog,
    root: Root,
    cancellation: &Cancellation,
    executable_allowed: bool,
    build_authority: &BuildAuthority,
) -> Analysis {
    let mut diagnostics = Vec::new();
    let modules = parsed_sources
        .keys()
        .map(|path| {
            (
                path.clone(),
                identity_catalog
                    .module(path)
                    .expect("reachable Module was catalogued"),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let module_displays = parsed_sources
        .keys()
        .map(|path| (modules[path], module_name(path)))
        .collect::<BTreeMap<_, _>>();
    let mut definitions: BTreeMap<DefinitionId, DefinitionRecord> = BTreeMap::new();
    let mut local_names: BTreeMap<(ModuleId, String), DefinitionId> = BTreeMap::new();

    for (path, parsed) in parsed_sources {
        if cancellation.is_cancelled() {
            return cancelled();
        }
        let module = modules[path];
        for declaration in &parsed.declarations {
            if !declaration.structurally_valid || declaration.syntax.is_none() {
                continue;
            }
            for attribute in &declaration.attributes {
                if *attribute == AttributeSyntax::Unknown {
                    diagnostics.push(Diagnostic::new(
                        "semantic.unknown_attribute",
                        declaration.range.clone(),
                        RecoveryAction::None,
                    ));
                }
            }
            let id = identity_catalog
                .definition(path, declaration.kind, &declaration.name)
                .expect("structurally valid declaration was catalogued");
            let key = (module, declaration.name.clone());
            if let Some(previous) = local_names.insert(key, id) {
                let mut diagnostic = Diagnostic::new(
                    "semantic.duplicate_declaration",
                    declaration.range.clone(),
                    RecoveryAction::None,
                )
                .with_parameter("name", declaration.name.clone())
                .with_identity_parameter(
                    "definition",
                    IdentityDomain::Definition,
                    id.0,
                );
                if let Some(previous) = definitions.get(&previous) {
                    diagnostic = diagnostic.with_label(
                        previous.declaration.range.clone(),
                        DiagnosticLabelRole::PreviousDeclaration,
                    );
                }
                diagnostics.push(diagnostic);
                continue;
            }
            definitions.insert(
                id,
                DefinitionRecord {
                    id,
                    module,
                    module_display: module_displays[&module].clone(),
                    path: path.clone(),
                    name: declaration.name.clone(),
                    kind: declaration.kind,
                    public: declaration.public,
                    declaration: declaration.clone(),
                },
            );
        }
    }

    let mut names = BTreeMap::new();
    for definition in definitions.values() {
        names.insert(
            NameKey::new(definition.module, Arc::from([definition.name.clone()])),
            resolved_name(definition),
        );
    }
    for (path, parsed) in parsed_sources {
        let importer = modules[path];
        for import in &parsed.imports {
            let Some(target) = modules.get(&import.target_path).copied() else {
                continue;
            };
            if local_names.contains_key(&(importer, import.alias.clone())) {
                diagnostics.push(
                    Diagnostic::new(
                        "semantic.import_alias_conflict",
                        import.range.clone(),
                        RecoveryAction::None,
                    )
                    .with_parameter("alias", import.alias.clone()),
                );
                continue;
            }
            for definition in definitions
                .values()
                .filter(|definition| definition.module == target && definition.public)
            {
                names.insert(
                    NameKey::new(
                        importer,
                        Arc::from([import.alias.clone(), definition.name.clone()]),
                    ),
                    resolved_name(definition),
                );
            }
        }
    }

    let mut nominal_displays = BTreeMap::new();
    for definition in definitions
        .values()
        .filter(|definition| is_nominal(definition.kind))
    {
        nominal_displays.insert(definition.id, Arc::from(definition.name.as_str()));
        identity_catalog
            .type_for_definition(definition.id)
            .expect("nominal declaration has a catalogued TypeId");
    }

    let mut input = ProgramInput {
        names,
        nominal_displays,
        ..ProgramInput::default()
    };
    for (path, parsed) in parsed_sources {
        let module = modules[path];
        input.comptime_roots.extend(
            parsed
                .comptime_assertions
                .iter()
                .cloned()
                .map(|assertion| (module, assertion)),
        );
    }
    let mut tests_in_source_order = Vec::new();
    let mut image_functions = Vec::new();
    for definition in definitions.values() {
        if cancellation.is_cancelled() {
            return cancelled();
        }
        match definition
            .declaration
            .syntax
            .as_ref()
            .expect("catalogued syntax")
        {
            DeclarationSyntax::Function(function) => {
                let type_parameters = function
                    .type_parameters
                    .iter()
                    .enumerate()
                    .map(|(index, name)| {
                        (
                            name.clone(),
                            Type::Parameter {
                                owner: definition.id,
                                id: TypeParameterId(u16::try_from(index).unwrap_or(u16::MAX)),
                                display: Arc::from(name.as_str()),
                            },
                        )
                    })
                    .collect::<BTreeMap<_, _>>();
                let parameters = match resolve_parameters(
                    function,
                    definition.module,
                    &input.names,
                    &input.nominal_displays,
                    &type_parameters,
                    &mut diagnostics,
                ) {
                    Some(parameters) => parameters,
                    None => continue,
                };
                let Some(return_type) = resolve_type(
                    &function.return_type,
                    definition.module,
                    &input.names,
                    &input.nominal_displays,
                    &type_parameters,
                ) else {
                    diagnostics.push(Diagnostic::new(
                        "semantic.unresolved_type",
                        definition.declaration.range.clone(),
                        RecoveryAction::None,
                    ));
                    continue;
                };
                if definition.public
                    && (exposes_private_type(&return_type, &definitions)
                        || parameters
                            .iter()
                            .any(|parameter| exposes_private_type(&parameter.type_, &definitions)))
                {
                    diagnostics.push(Diagnostic::new(
                        "semantic.private_type_in_public_signature",
                        definition.declaration.range.clone(),
                        RecoveryAction::None,
                    ));
                }
                if definition.public && matches!(return_type, Type::Result { error: None, .. }) {
                    diagnostics.push(Diagnostic::new(
                        "semantic.public_result_requires_error_type",
                        definition.declaration.range.clone(),
                        RecoveryAction::None,
                    ));
                }
                if definition
                    .declaration
                    .attributes
                    .contains(&AttributeSyntax::Image)
                {
                    image_functions.push((definition.id, definition.declaration.range.clone()));
                }
                input.functions.insert(
                    definition.id,
                    ResolvedFunction {
                        id: definition.id,
                        module: definition.module,
                        module_display: definition.module_display.clone(),
                        name: definition.name.clone(),
                        modifier: function.modifier,
                        type_parameters: (0..function.type_parameters.len())
                            .map(|index| TypeParameterId(u16::try_from(index).unwrap_or(u16::MAX)))
                            .collect(),
                        parameters,
                        return_type,
                        body: function.body.clone(),
                        source: SourceRange::from_u64(
                            &definition.path,
                            definition.declaration.start,
                            definition.declaration.end,
                        ),
                    },
                );
            }
            DeclarationSyntax::Constant(constant) => {
                let Some(type_) = resolve_type(
                    &constant.type_syntax,
                    definition.module,
                    &input.names,
                    &input.nominal_displays,
                    &BTreeMap::new(),
                ) else {
                    diagnostics.push(Diagnostic::new(
                        "semantic.unresolved_type",
                        definition.declaration.range.clone(),
                        RecoveryAction::None,
                    ));
                    continue;
                };
                input.constants.insert(
                    definition.id,
                    ResolvedConstant {
                        id: definition.id,
                        module: definition.module,
                        name: definition.name.clone(),
                        type_,
                        value: constant.value.clone(),
                        source: SourceRange::from_u64(
                            &definition.path,
                            definition.declaration.start,
                            definition.declaration.end,
                        ),
                    },
                );
            }
            DeclarationSyntax::Suite(suite) => {
                if !definition.public {
                    diagnostics.push(Diagnostic::new(
                        "test.suite_must_be_public",
                        definition.declaration.range.clone(),
                        RecoveryAction::None,
                    ));
                }
                for test in &suite.tests {
                    let id = identity_catalog
                        .test(&definition.path, &definition.name, &test.name)
                        .expect("structured Test was catalogued");
                    let parameters = test
                        .parameters
                        .iter()
                        .filter_map(|parameter| {
                            if parameter.ownership != OwnershipSyntax::Take {
                                diagnostics.push(Diagnostic::new(
                                    "test.parameter_requires_take",
                                    parameter.range.clone(),
                                    RecoveryAction::None,
                                ));
                            }
                            resolve_type(
                                &parameter.type_syntax,
                                definition.module,
                                &input.names,
                                &input.nominal_displays,
                                &BTreeMap::new(),
                            )
                            .map(|type_| ResolvedParameter {
                                name: parameter.name.clone(),
                                ownership: parameter.ownership,
                                type_,
                            })
                        })
                        .collect::<Vec<_>>();
                    let resolved = ResolvedTest {
                        id,
                        suite: definition.name.clone(),
                        test: test.name.clone(),
                        asynchronous: test.asynchronous,
                        parameters,
                    };
                    input.tests.insert(id, resolved);
                    input.names.insert(
                        NameKey::new(
                            definition.module,
                            Arc::from([definition.name.clone(), test.name.clone()]),
                        ),
                        ResolvedName::Test(id),
                    );
                    tests_in_source_order.push(TestRecord {
                        id,
                        range: test.range.clone(),
                    });
                }
            }
            DeclarationSyntax::Enum(_) | DeclarationSyntax::Named => {}
        }
    }

    let expected_root_path = match root {
        Root::Image => "src/image.wr",
        Root::Test => "src/test.wr",
    };
    if executable_allowed && image_functions.is_empty() {
        diagnostics.push(Diagnostic::new(
            "semantic.missing_image_constructor",
            SourceRange::new(expected_root_path, 0, 0),
            RecoveryAction::None,
        ));
    } else if executable_allowed && image_functions.len() > 1 {
        for (_, range) in image_functions.iter().skip(1) {
            diagnostics.push(Diagnostic::new(
                "semantic.multiple_image_constructors",
                range.clone(),
                RecoveryAction::None,
            ));
        }
    } else if executable_allowed
        && image_functions
            .first()
            .is_some_and(|(id, _)| definitions[id].module != modules[expected_root_path])
    {
        diagnostics.push(Diagnostic::new(
            "semantic.image_constructor_outside_root",
            image_functions[0].1.clone(),
            RecoveryAction::None,
        ));
    }

    let mut type_observations = Vec::new();
    let mut ownership_observations = Vec::new();
    for function in input.functions.values() {
        for parameter in &function.parameters {
            type_observations.push(TypeObservation::new(
                function.id.0,
                parameter.name.clone(),
                TypeRole::Parameter,
                parameter.type_.display(),
            ));
            ownership_observations.push(OwnershipObservation::new(
                function.id.0,
                parameter.name.clone(),
                match parameter.ownership {
                    OwnershipSyntax::Value => OwnershipMode::Value,
                    OwnershipSyntax::Take => OwnershipMode::Take,
                },
            ));
        }
        type_observations.push(TypeObservation::new(
            function.id.0,
            function.name.clone(),
            TypeRole::Return,
            function.return_type.display(),
        ));
    }
    for constant in input.constants.values() {
        type_observations.push(TypeObservation::new(
            constant.id.0,
            constant.name.clone(),
            TypeRole::Constant,
            constant.type_.display(),
        ));
    }
    for test in input.tests.values() {
        for parameter in &test.parameters {
            type_observations.push(TypeObservation::new(
                test.id.test.0,
                parameter.name.clone(),
                TypeRole::Parameter,
                parameter.type_.display(),
            ));
            ownership_observations.push(OwnershipObservation::new(
                test.id.test.0,
                parameter.name.clone(),
                match parameter.ownership {
                    OwnershipSyntax::Value => OwnershipMode::Value,
                    OwnershipSyntax::Take => OwnershipMode::Take,
                },
            ));
        }
    }
    let program = match typed_hir::verify(input, build_authority, identity_catalog, cancellation) {
        Ok(program) => Some(program),
        Err(typed_hir::VerificationFailure::Defect { evidence }) => {
            return defect("typed HIR verification", evidence);
        }
        Err(typed_hir::VerificationFailure::Creator { kind, site }) => {
            diagnostics.push(
                Diagnostic::new("semantic.invalid_typed_hir", site, RecoveryAction::None)
                    .with_parameter("kind", kind.code()),
            );
            None
        }
        Err(typed_hir::VerificationFailure::Cancelled) => return cancelled(),
    };

    let mut function_facts = Vec::new();
    let mut specialization_observations = Vec::new();
    let mut inferred_errors = Vec::new();
    let mut evaluations = Vec::new();
    let mut constructions = Vec::new();
    let mut test_plan = Vec::new();
    if let Some(program) = &program {
        let facts = solve_function_facts(program, cancellation);
        if cancellation.is_cancelled() {
            return cancelled();
        }
        for (id, facts) in &facts {
            function_facts.push(FunctionFactsObservation::new(
                id.0,
                program.functions()[id].name.clone(),
                FunctionFactsValues {
                    pure: facts.pure,
                    may_panic: facts.may_panic,
                    suspends: facts.suspends,
                    evaluator_eligible: facts.evaluator_eligible,
                    ownership_transfer: facts.ownership_transfer,
                    bounded: facts.bounded,
                    logical_cost: facts.logical_cost,
                },
            ));
        }
        for specialization in program.specializations().values() {
            let function = &program.functions()[&specialization.definition];
            specialization_observations.push(SpecializationObservation::new(
                specialization.id.0,
                specialization.definition.0,
                function.name.clone(),
                specialization
                    .type_arguments
                    .iter()
                    .map(|type_| Arc::<str>::from(type_.display()))
                    .collect(),
            ));
        }
        let call_graph = facts
            .iter()
            .map(|(id, facts)| (*id, facts.calls.clone()))
            .collect::<BTreeMap<_, _>>();
        for constant in program.constants().values() {
            if expression_constructs(&constant.expression) {
                diagnostics.push(Diagnostic::new(
                    "semantic.build_constructor_outside_image",
                    constant.source.clone(),
                    RecoveryAction::None,
                ));
            }
        }
        for (id, facts) in &facts {
            if !facts.constructs.is_empty()
                && !image_functions
                    .first()
                    .is_some_and(|(root, _)| reachable(*root, &call_graph, *id))
            {
                diagnostics.push(Diagnostic::new(
                    "semantic.build_constructor_outside_image",
                    program.functions()[id].source.clone(),
                    RecoveryAction::None,
                ));
            }
        }
        for range in recursive_functions(program) {
            diagnostics.push(Diagnostic::new(
                "semantic.unproven_recursive_bound",
                range,
                RecoveryAction::None,
            ));
        }
        let (errors, error_diagnostics) = infer_errors(program, cancellation);
        inferred_errors = errors;
        diagnostics.extend(error_diagnostics);

        if executable_allowed && diagnostics.is_empty() {
            for constant in program.constants().values() {
                let mut engine = Engine::new(program, cancellation);
                let run = engine.evaluate_constant(constant.id);
                if run.outcome == EvaluationOutcome::Cancelled {
                    return cancelled();
                }
                map_evaluation_failure(&run.outcome, &constant.source, &mut diagnostics);
                let record = definitions.get(&constant.id).expect("constant definition");
                evaluations.push(EvaluationObservation::new(
                    format!("{}.{}", record.module_display, record.name),
                    run.outcome,
                    run.receipt,
                ));
            }
            for (path, parsed) in parsed_sources {
                let module = modules[path];
                for assertion in &parsed.comptime_assertions {
                    match program.verify_expression(assertion) {
                        Ok(expression) => {
                            let mut engine = Engine::new(program, cancellation);
                            let run = engine.evaluate_expression(&expression);
                            if run.outcome == EvaluationOutcome::Cancelled {
                                return cancelled();
                            }
                            if run.outcome
                                != EvaluationOutcome::Completed(crate::CanonicalValue::Bool(true))
                            {
                                diagnostics.push(Diagnostic::new(
                                    "evaluation.assertion_failed",
                                    assertion.range.clone(),
                                    RecoveryAction::None,
                                ));
                            }
                            evaluations.push(EvaluationObservation::new(
                                format!("{}.comptime_assert", module_displays[&module]),
                                run.outcome,
                                run.receipt,
                            ));
                        }
                        Err(typed_hir::VerificationFailure::Cancelled) => return cancelled(),
                        Err(_) => diagnostics.push(Diagnostic::new(
                            "semantic.invalid_comptime_expression",
                            assertion.range.clone(),
                            RecoveryAction::None,
                        )),
                    }
                }
            }
            if diagnostics.is_empty()
                && let Some((image, image_range)) = image_functions.first()
            {
                let mut engine = Engine::new(program, cancellation);
                let run = engine.evaluate_function(*image);
                if run.outcome == EvaluationOutcome::Cancelled {
                    return cancelled();
                }
                map_evaluation_failure(&run.outcome, image_range, &mut diagnostics);
                if root == Root::Test {
                    test_plan = plan_tests(
                        program,
                        &tests_in_source_order,
                        &run.test_applications,
                        image_range,
                        &mut diagnostics,
                    );
                }
                if let Err(kind) = seal_construction_graph(run.root_handle, &run.constructions) {
                    diagnostics.push(
                        Diagnostic::new(
                            "construction.invalid_graph",
                            image_range.clone(),
                            RecoveryAction::None,
                        )
                        .with_parameter("kind", kind),
                    );
                }
                constructions.extend(run.constructions.into_iter().map(|construction| {
                    ConstructionObservation::new(
                        construction.identity,
                        construction.kind.name(),
                        construction.site,
                    )
                }));
                let function = &program.functions()[image];
                evaluations.push(EvaluationObservation::new(
                    format!("{}.{}", function.module_display, function.name),
                    run.outcome,
                    run.receipt,
                ));
            }
        }
    }

    function_facts.sort_by(|left, right| left.name().cmp(right.name()));
    inferred_errors.sort_by(|left, right| left.function().cmp(right.function()));
    evaluations.sort_by(|left, right| left.root().cmp(right.root()));
    constructions.sort_by_key(ConstructionObservation::identity);
    type_observations.sort_by(|left, right| {
        left.owner_identity()
            .cmp(&right.owner_identity())
            .then(left.name().cmp(right.name()))
    });
    ownership_observations.sort_by(|left, right| {
        left.owner_identity()
            .cmp(&right.owner_identity())
            .then(left.name().cmp(right.name()))
    });
    specialization_observations.sort_by_key(SpecializationObservation::identity);
    Analysis {
        diagnostics,
        function_facts,
        types: type_observations,
        ownership: ownership_observations,
        specializations: specialization_observations,
        inferred_errors,
        evaluations,
        constructions,
        test_plan,
        defect: None,
        cancelled: false,
    }
}

fn resolved_name(definition: &DefinitionRecord) -> ResolvedName {
    match definition.kind {
        DeclarationKind::Function => ResolvedName::Function(definition.id),
        DeclarationKind::Constant => ResolvedName::Constant(definition.id),
        kind if is_nominal(kind) => ResolvedName::Nominal(definition.id),
        _ => ResolvedName::Nominal(definition.id),
    }
}

fn is_nominal(kind: DeclarationKind) -> bool {
    matches!(
        kind,
        DeclarationKind::Struct
            | DeclarationKind::ResourceStruct
            | DeclarationKind::Enum
            | DeclarationKind::Interface
            | DeclarationKind::TypeAlias
    )
}

fn resolve_parameters(
    function: &FunctionSyntax,
    module: ModuleId,
    names: &BTreeMap<NameKey, ResolvedName>,
    displays: &BTreeMap<DefinitionId, Arc<str>>,
    type_parameters: &BTreeMap<String, Type>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Vec<ResolvedParameter>> {
    function
        .parameters
        .iter()
        .map(|parameter| {
            let type_ = resolve_type(
                &parameter.type_syntax,
                module,
                names,
                displays,
                type_parameters,
            );
            if type_.is_none() {
                diagnostics.push(Diagnostic::new(
                    "semantic.unresolved_type",
                    parameter.range.clone(),
                    RecoveryAction::None,
                ));
            }
            type_.map(|type_| ResolvedParameter {
                name: parameter.name.clone(),
                ownership: parameter.ownership,
                type_,
            })
        })
        .collect()
}

fn resolve_type(
    syntax: &TypeSyntax,
    module: ModuleId,
    names: &BTreeMap<NameKey, ResolvedName>,
    displays: &BTreeMap<DefinitionId, Arc<str>>,
    type_parameters: &BTreeMap<String, Type>,
) -> Option<Type> {
    match syntax {
        TypeSyntax::Unit => Some(Type::Unit),
        TypeSyntax::Infer => Some(Type::Infer),
        TypeSyntax::Array(element) => Some(Type::Array(Arc::new(resolve_type(
            element,
            module,
            names,
            displays,
            type_parameters,
        )?))),
        TypeSyntax::Tuple(members) => Some(Type::Tuple(
            members
                .iter()
                .map(|member| resolve_type(member, module, names, displays, type_parameters))
                .collect::<Option<Vec<_>>>()?
                .into(),
        )),
        TypeSyntax::Named(name) => {
            if let [name] = name.segments.as_slice()
                && let Some(parameter) = type_parameters.get(name)
            {
                return Some(parameter.clone());
            }
            if let Some(builtin) = resolve_builtin_type(name) {
                return Some(builtin);
            }
            let ResolvedName::Nominal(id) =
                names.get(&NameKey::new(module, Arc::from(name.segments.clone())))?
            else {
                return None;
            };
            Some(Type::Nominal {
                definition: *id,
                display: displays.get(id)?.clone(),
            })
        }
        TypeSyntax::Apply { base, arguments } => {
            let values = arguments
                .iter()
                .map(|argument| resolve_type(argument, module, names, displays, type_parameters))
                .collect::<Option<Vec<_>>>()?;
            match base
                .segments
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice()
            {
                ["Result"] if matches!(values.as_slice(), [_] | [_, _]) => Some(Type::Result {
                    success: Arc::new(values[0].clone()),
                    error: values.get(1).cloned().map(Arc::new),
                }),
                ["Option"] if values.len() == 1 => Some(Type::Option(Arc::new(values[0].clone()))),
                _ => None,
            }
        }
    }
}

fn exposes_private_type(
    type_: &Type,
    definitions: &BTreeMap<DefinitionId, DefinitionRecord>,
) -> bool {
    match type_ {
        Type::Nominal { definition, .. } => definitions
            .get(definition)
            .is_some_and(|definition| !definition.public),
        Type::Array(value) | Type::Option(value) => exposes_private_type(value, definitions),
        Type::Tuple(values) => values
            .iter()
            .any(|value| exposes_private_type(value, definitions)),
        Type::Result { success, error } => {
            exposes_private_type(success, definitions)
                || error
                    .as_ref()
                    .is_some_and(|error| exposes_private_type(error, definitions))
        }
        _ => false,
    }
}

fn solve_function_facts(
    program: &VerifiedProgram,
    cancellation: &Cancellation,
) -> BTreeMap<DefinitionId, FunctionFacts> {
    let base = program
        .functions()
        .iter()
        .map(|(id, function)| (*id, local_facts(function)))
        .collect::<BTreeMap<_, _>>();
    let mut facts = base.clone();
    for _ in 0..=facts.len() {
        if cancellation.is_cancelled() {
            return BTreeMap::new();
        }
        let previous = facts;
        let mut next = base.clone();
        for current in next.values_mut() {
            for call in current.calls.clone() {
                if let Some(callee) = previous.get(&call) {
                    current.pure &= callee.pure;
                    current.may_panic |= callee.may_panic;
                    current.suspends |= callee.suspends;
                    current.evaluator_eligible &= callee.evaluator_eligible;
                    current.constructs.extend(callee.constructs.iter().copied());
                    current.ownership_transfer |= callee.ownership_transfer;
                    current.logical_cost = current.logical_cost.saturating_add(callee.logical_cost);
                }
            }
        }
        if next == previous {
            facts = next;
            break;
        }
        facts = next;
    }
    let graph = facts
        .iter()
        .map(|(id, facts)| (*id, facts.calls.clone()))
        .collect::<BTreeMap<_, _>>();
    for (id, current) in &mut facts {
        current.bounded = !reaches_definition(*id, *id, &graph, &mut BTreeSet::new());
    }
    facts
}

fn local_facts(function: &typed_hir::HirFunction) -> FunctionFacts {
    let mut facts = FunctionFacts {
        pure: true,
        may_panic: false,
        suspends: function.modifier == crate::syntax::FunctionModifier::Async,
        evaluator_eligible: true,
        ownership_transfer: function
            .parameters
            .iter()
            .any(|(_, _, access)| *access == typed_hir::AccessMode::Move),
        bounded: true,
        logical_cost: 1,
        constructs: BTreeSet::new(),
        calls: BTreeSet::new(),
    };
    visit_statements(&function.body, &mut facts);
    facts.pure = facts.constructs.is_empty() && !facts.suspends;
    facts.evaluator_eligible = !facts.suspends;
    facts
}

fn visit_statements(statements: &[Statement], facts: &mut FunctionFacts) {
    for statement in statements {
        facts.logical_cost = facts.logical_cost.saturating_add(1);
        match statement {
            Statement::Return { value, .. } => {
                if let Some(value) = value {
                    visit_expression(value, facts);
                }
            }
            Statement::Panic { value, .. } => {
                facts.may_panic = true;
                visit_expression(value, facts);
            }
            Statement::Initialize { value, .. } | Statement::Evaluate(value) => {
                visit_expression(value, facts)
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                visit_expression(condition, facts);
                visit_statements(then_branch, facts);
                visit_statements(else_branch, facts);
            }
            Statement::Pass(_) => {}
        }
    }
}

fn visit_expression(expression: &Expression, facts: &mut FunctionFacts) {
    facts.logical_cost = facts.logical_cost.saturating_add(1);
    match &expression.kind {
        ExpressionKind::Call { target, arguments } => {
            match target {
                CallTarget::TemplateFunction(definition)
                | CallTarget::Function { definition, .. } => {
                    facts.calls.insert(*definition);
                }
                CallTarget::Build(kind) => {
                    facts.constructs.insert(*kind);
                }
                _ => {}
            }
            for argument in &**arguments {
                visit_expression(argument, facts);
            }
        }
        ExpressionKind::Array(values) => {
            for value in &**values {
                visit_expression(value, facts);
            }
        }
        ExpressionKind::Negate(value) | ExpressionKind::Propagate(value) => {
            visit_expression(value, facts)
        }
        ExpressionKind::Await(value) => {
            facts.suspends = true;
            visit_expression(value, facts);
        }
        ExpressionKind::Binary {
            operator,
            left,
            right,
        } => {
            if matches!(
                operator,
                typed_hir::BinaryOperator::Divide | typed_hir::BinaryOperator::Remainder
            ) {
                facts.may_panic = true;
            }
            visit_expression(left, facts);
            visit_expression(right, facts);
        }
        ExpressionKind::Literal(_) | ExpressionKind::Read(_) | ExpressionKind::Constant(_) => {}
    }
}

fn expression_constructs(expression: &Expression) -> bool {
    match &expression.kind {
        ExpressionKind::Call { target, arguments } => {
            matches!(target, CallTarget::Build(_)) || arguments.iter().any(expression_constructs)
        }
        ExpressionKind::Array(values) => values.iter().any(expression_constructs),
        ExpressionKind::Negate(value)
        | ExpressionKind::Await(value)
        | ExpressionKind::Propagate(value) => expression_constructs(value),
        ExpressionKind::Binary { left, right, .. } => {
            expression_constructs(left) || expression_constructs(right)
        }
        ExpressionKind::Literal(_) | ExpressionKind::Read(_) | ExpressionKind::Constant(_) => false,
    }
}

fn infer_errors(
    program: &VerifiedProgram,
    cancellation: &Cancellation,
) -> (Vec<InferredErrorObservation>, Vec<Diagnostic>) {
    let candidates = program
        .specialized_functions()
        .iter()
        .filter(|(_, function)| matches!(function.return_type, Type::Result { error: None, .. }))
        .map(|(id, _)| *id)
        .collect::<BTreeSet<_>>();
    let mut errors = candidates
        .iter()
        .map(|id| {
            (
                *id,
                direct_error_types(&program.specialized_functions()[id].body),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for _ in 0..=errors.len() {
        if cancellation.is_cancelled() {
            return (Vec::new(), Vec::new());
        }
        let previous = errors.clone();
        for (id, set) in &mut errors {
            for callee in propagated_specializations(&program.specialized_functions()[id].body) {
                match &program.specialized_functions()[&callee].return_type {
                    Type::Result {
                        error: Some(error), ..
                    } => {
                        set.insert((**error).clone());
                    }
                    Type::Result { error: None, .. } => {
                        if let Some(inferred) = previous.get(&callee) {
                            set.extend(inferred.iter().cloned());
                        }
                    }
                    _ => {}
                }
            }
        }
        if errors == previous {
            break;
        }
    }
    let mut observations = Vec::new();
    let mut diagnostics = Vec::new();
    for (id, set) in &errors {
        let function = &program.specialized_functions()[id];
        if set.len() == 1 {
            observations.push(InferredErrorObservation::new(
                id.0,
                function.name.clone(),
                set.first().expect("one").display(),
            ));
        } else {
            diagnostics.push(Diagnostic::new(
                if set.is_empty() {
                    "semantic.unconstrained_inferred_error"
                } else {
                    "semantic.conflicting_inferred_errors"
                },
                function.source.clone(),
                RecoveryAction::None,
            ));
        }
    }
    for function in program.specialized_functions().values() {
        let Type::Result {
            error: Some(caller_error),
            ..
        } = &function.return_type
        else {
            continue;
        };
        for callee in propagated_specializations(&function.body) {
            let callee_error = match &program.specialized_functions()[&callee].return_type {
                Type::Result {
                    error: Some(error), ..
                } => Some((**error).clone()),
                Type::Result { error: None, .. } => errors
                    .get(&callee)
                    .and_then(|set| (set.len() == 1).then(|| set.first().expect("one").clone())),
                _ => None,
            };
            if callee_error.is_some_and(|error| error != **caller_error) {
                diagnostics.push(
                    Diagnostic::new(
                        "semantic.propagation_error_mismatch",
                        function.source.clone(),
                        RecoveryAction::None,
                    )
                    .with_identity_parameter("callee", IdentityDomain::Specialization, callee.0)
                    .with_label(
                        program.specialized_functions()[&callee].source.clone(),
                        DiagnosticLabelRole::PropagationSource,
                    ),
                );
            }
        }
    }
    (observations, diagnostics)
}

fn propagated_specializations(statements: &[Statement]) -> BTreeSet<SpecializationId> {
    let mut result = BTreeSet::new();
    walk_expressions(statements, &mut |expression| {
        if let ExpressionKind::Propagate(value) = &expression.kind
            && let ExpressionKind::Call {
                target: CallTarget::Function { specialization, .. },
                ..
            } = &value.kind
        {
            result.insert(*specialization);
        }
    });
    result
}

fn direct_error_types(statements: &[Statement]) -> BTreeSet<Type> {
    let mut result = BTreeSet::new();
    walk_expressions(statements, &mut |expression| {
        if let ExpressionKind::Call {
            target: CallTarget::BuiltinVariant(BuiltinVariant::ResultErr),
            arguments,
        } = &expression.kind
            && let Some(error) = arguments.first()
        {
            result.insert(error.type_.clone());
        }
    });
    result
}

fn walk_expressions(statements: &[Statement], visitor: &mut impl FnMut(&Expression)) {
    fn expression(value: &Expression, visitor: &mut impl FnMut(&Expression)) {
        visitor(value);
        match &value.kind {
            ExpressionKind::Call { arguments, .. } | ExpressionKind::Array(arguments) => {
                for argument in &**arguments {
                    expression(argument, visitor);
                }
            }
            ExpressionKind::Negate(value)
            | ExpressionKind::Await(value)
            | ExpressionKind::Propagate(value) => expression(value, visitor),
            ExpressionKind::Binary { left, right, .. } => {
                expression(left, visitor);
                expression(right, visitor);
            }
            _ => {}
        }
    }
    for statement in statements {
        match statement {
            Statement::Return { value, .. } => {
                if let Some(value) = value {
                    expression(value, visitor);
                }
            }
            Statement::Panic { value, .. }
            | Statement::Initialize { value, .. }
            | Statement::Evaluate(value) => expression(value, visitor),
            Statement::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                expression(condition, visitor);
                walk_expressions(then_branch, visitor);
                walk_expressions(else_branch, visitor);
            }
            Statement::Pass(_) => {}
        }
    }
}

fn recursive_functions(program: &VerifiedProgram) -> Vec<SourceRange> {
    let graph = program
        .functions()
        .iter()
        .map(|(id, function)| (*id, local_facts(function).calls))
        .collect::<BTreeMap<_, _>>();
    graph
        .keys()
        .filter(|root| reaches_definition(**root, **root, &graph, &mut BTreeSet::new()))
        .map(|id| program.functions()[id].source.clone())
        .collect()
}

fn reaches_definition(
    root: DefinitionId,
    current: DefinitionId,
    graph: &BTreeMap<DefinitionId, BTreeSet<DefinitionId>>,
    visited: &mut BTreeSet<DefinitionId>,
) -> bool {
    if !visited.insert(current) {
        return false;
    }
    graph.get(&current).is_some_and(|dependencies| {
        dependencies.contains(&root)
            || dependencies
                .iter()
                .any(|dependency| reaches_definition(root, *dependency, graph, visited))
    })
}

fn reachable(
    root: DefinitionId,
    graph: &BTreeMap<DefinitionId, BTreeSet<DefinitionId>>,
    target: DefinitionId,
) -> bool {
    if root == target {
        return true;
    }
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(current) = pending.pop() {
        if !visited.insert(current) {
            continue;
        }
        if graph
            .get(&current)
            .is_some_and(|calls| calls.contains(&target))
        {
            return true;
        }
        if let Some(calls) = graph.get(&current) {
            pending.extend(calls.iter().copied());
        }
    }
    false
}

fn plan_tests(
    program: &VerifiedProgram,
    tests: &[TestRecord],
    applications: &[TestId],
    range: &SourceRange,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<TestApplicationObservation> {
    let mut counts = BTreeMap::new();
    for id in applications {
        *counts.entry(*id).or_insert(0_u32) += 1;
    }
    let known = tests.iter().map(|test| test.id).collect::<BTreeSet<_>>();
    for id in applications {
        if !known.contains(id) {
            diagnostics.push(Diagnostic::new(
                "test.unknown_application",
                range.clone(),
                RecoveryAction::None,
            ));
        }
    }
    for test in tests {
        let resolved = program
            .test(test.id)
            .expect("planned Test identity resolves");
        match counts.get(&test.id).copied().unwrap_or(0) {
            0 => diagnostics.push(
                Diagnostic::new(
                    "test.missing_application",
                    test.range.clone(),
                    RecoveryAction::None,
                )
                .with_parameter("suite", resolved.suite.clone())
                .with_parameter("test", resolved.test.clone()),
            ),
            1 => {}
            _ => diagnostics.push(
                Diagnostic::new(
                    "test.duplicate_application",
                    test.range.clone(),
                    RecoveryAction::None,
                )
                .with_parameter("suite", resolved.suite.clone())
                .with_parameter("test", resolved.test.clone()),
            ),
        }
    }
    let mut plan = Vec::new();
    for (order, id) in applications.iter().enumerate() {
        if counts.get(id) != Some(&1) || !known.contains(id) {
            continue;
        }
        let resolved = program.test(*id).expect("applied Test identity resolves");
        plan.push(TestApplicationObservation::new(
            resolved.suite.clone(),
            resolved.test.clone(),
            u32::try_from(order).unwrap_or(u32::MAX),
            resolved.asynchronous,
            resolved
                .parameters
                .iter()
                .map(|parameter| {
                    TestBindingObservation::new(
                        parameter.name.clone(),
                        parameter.type_.display(),
                        match parameter.ownership {
                            OwnershipSyntax::Value => OwnershipMode::Value,
                            OwnershipSyntax::Take => OwnershipMode::Take,
                        },
                    )
                })
                .collect(),
        ));
    }
    plan
}

fn seal_construction_graph(
    root_handle: Option<(BuildKind, u128)>,
    constructions: &[Construction],
) -> Result<(), &'static str> {
    let Some((kind, root)) = root_handle else {
        return Ok(());
    };
    if kind != BuildKind::Image {
        return Err("returned_root_is_not_image");
    }
    if constructions
        .iter()
        .filter(|construction| construction.kind == BuildKind::Image)
        .count()
        != 1
    {
        return Err("multiple_image_roots");
    }
    let graph = constructions
        .iter()
        .map(|construction| (construction.identity, construction.edges.as_slice()))
        .collect::<BTreeMap<_, _>>();
    let mut reachable = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(identity) = pending.pop() {
        if reachable.insert(identity)
            && let Some(edges) = graph.get(&identity)
        {
            pending.extend(edges.iter().copied());
        }
    }
    if reachable.len() != constructions.len() {
        return Err("unreachable_construction");
    }
    Ok(())
}

fn map_evaluation_failure(
    outcome: &EvaluationOutcome,
    range: &SourceRange,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match outcome {
        EvaluationOutcome::CreatorRejected { kind } => diagnostics.push(
            Diagnostic::new("evaluation.rejected", range.clone(), RecoveryAction::None)
                .with_parameter("kind", kind.clone()),
        ),
        EvaluationOutcome::Panicked { kind, site } => diagnostics.push(
            Diagnostic::new("evaluation.panicked", site.clone(), RecoveryAction::None)
                .with_parameter("kind", kind.clone()),
        ),
        EvaluationOutcome::LimitExceeded {
            policy,
            ceiling,
            used,
        } => diagnostics.push(
            Diagnostic::new(
                "evaluation.limit_exceeded",
                range.clone(),
                RecoveryAction::None,
            )
            .with_parameter("policy", policy.clone())
            .with_unsigned_parameter("ceiling", u128::from(*ceiling))
            .with_unsigned_parameter("used", u128::from(*used)),
        ),
        EvaluationOutcome::Defect { evidence } => diagnostics.push(
            Diagnostic::new("evaluation.defect", range.clone(), RecoveryAction::None)
                .with_parameter("evidence", evidence.clone()),
        ),
        EvaluationOutcome::Completed(_) | EvaluationOutcome::Cancelled => {}
    }
}

fn module_name(path: &str) -> String {
    path.strip_prefix("src/")
        .and_then(|path| path.strip_suffix(".wr"))
        .unwrap_or(path)
        .replace('/', ".")
}

fn cancelled() -> Analysis {
    Analysis {
        diagnostics: Vec::new(),
        function_facts: Vec::new(),
        types: Vec::new(),
        ownership: Vec::new(),
        specializations: Vec::new(),
        inferred_errors: Vec::new(),
        evaluations: Vec::new(),
        constructions: Vec::new(),
        test_plan: Vec::new(),
        defect: None,
        cancelled: true,
    }
}

fn defect(phase: &'static str, evidence: Arc<str>) -> Analysis {
    Analysis {
        diagnostics: Vec::new(),
        function_facts: Vec::new(),
        types: Vec::new(),
        ownership: Vec::new(),
        specializations: Vec::new(),
        inferred_errors: Vec::new(),
        evaluations: Vec::new(),
        constructions: Vec::new(),
        test_plan: Vec::new(),
        defect: Some(Defect::new(phase, evidence)),
        cancelled: false,
    }
}
