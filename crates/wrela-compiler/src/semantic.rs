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
    BuildKind, BuiltinType, BuiltinVariant, DefinitionId, ModuleId, SpecializationId, TestId, Type,
    TypeParameterId, resolve_builtin_type,
};
use crate::syntax::{
    AttributeSyntax, Declaration, DeclarationKind, DeclarationSyntax, OwnershipSyntax,
    ParameterSyntax, ParsedSource, StatementSyntax, TypeSyntax,
};
use crate::type_semantics::can_unify;
use crate::typed_hir::{
    self, BuildAuthority, CallTarget, Expression, ExpressionKind, NamespaceCatalog, ProgramInput,
    ResolvedConstant, ResolvedField, ResolvedFunction, ResolvedName, ResolvedParameter,
    ResolvedStruct, ResolvedTest, ResolvedVariant, Statement, VerifiedProgram,
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

#[derive(Clone)]
struct InterfaceRequirement {
    name: String,
    modifier: crate::syntax::FunctionModifier,
    parameters: Vec<ResolvedParameter>,
    return_type: Type,
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
    calls: BTreeMap<DefinitionId, u64>,
    specialization_calls: BTreeMap<SpecializationId, u64>,
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

    let mut namespace = NamespaceCatalog::default();
    for definition in definitions.values() {
        if let Some(name) = resolved_name(definition) {
            namespace.declare(
                definition.module,
                Arc::from([definition.name.clone()]),
                name,
                definition.public,
            );
        }
    }
    for definition in definitions.values() {
        let functions = match definition.declaration.syntax.as_ref() {
            Some(
                DeclarationSyntax::Struct(struct_) | DeclarationSyntax::ResourceStruct(struct_),
            ) => &struct_.functions,
            Some(DeclarationSyntax::Enum(enum_)) => &enum_.functions,
            _ => continue,
        };
        for member in functions {
            let id = identity_catalog
                .associated_function(definition.id, &member.name)
                .expect("structured associated function was catalogued");
            namespace.declare(
                definition.module,
                Arc::from([definition.name.clone(), member.name.clone()]),
                ResolvedName::Function(id),
                definition.public && member.public,
            );
            namespace.declare_member(
                definition.id,
                definition.module,
                member.name.clone(),
                ResolvedName::Function(id),
                definition.public && member.public,
            );
        }
    }
    let mut module_bindings = BTreeMap::new();
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
            let binding = (importer, import.alias.clone());
            if module_bindings.contains_key(&binding) {
                diagnostics.push(
                    Diagnostic::new(
                        "semantic.duplicate_import_alias",
                        import.range.clone(),
                        RecoveryAction::None,
                    )
                    .with_parameter("alias", import.alias.clone()),
                );
                continue;
            }
            module_bindings.insert(binding, target);
            namespace.bind(importer, import.alias.clone(), target);
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

    let alias_declarations = definitions
        .values()
        .filter_map(|definition| {
            let DeclarationSyntax::TypeAlias(target) = definition.declaration.syntax.as_ref()?
            else {
                return None;
            };
            Some((
                definition.id,
                (
                    definition.module,
                    target.clone(),
                    definition.declaration.range.clone(),
                ),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let mut alias_types = BTreeMap::new();
    let mut unresolved_aliases = alias_declarations.keys().copied().collect::<BTreeSet<_>>();
    loop {
        let before = unresolved_aliases.len();
        for id in unresolved_aliases.iter().copied().collect::<Vec<_>>() {
            let (module, target, _) = &alias_declarations[&id];
            if let Some(type_) = resolve_type(
                target,
                *module,
                &namespace,
                &nominal_displays,
                &alias_types,
                &BTreeMap::new(),
            ) {
                alias_types.insert(id, type_);
                unresolved_aliases.remove(&id);
            }
        }
        if unresolved_aliases.is_empty() || unresolved_aliases.len() == before {
            break;
        }
    }
    let mut alias_dependencies = BTreeMap::new();
    let mut missing_aliases = BTreeSet::new();
    for id in &unresolved_aliases {
        let (module, syntax, _) = &alias_declarations[id];
        let (dependencies, missing) = unresolved_alias_dependencies(syntax, *module, &namespace);
        alias_dependencies.insert(*id, dependencies);
        if missing {
            missing_aliases.insert(*id);
        }
    }
    loop {
        let before = missing_aliases.len();
        for (id, dependencies) in &alias_dependencies {
            if dependencies
                .iter()
                .any(|dependency| missing_aliases.contains(dependency))
            {
                missing_aliases.insert(*id);
            }
        }
        if missing_aliases.len() == before {
            break;
        }
    }
    for id in unresolved_aliases {
        diagnostics.push(Diagnostic::new(
            if missing_aliases.contains(&id) {
                "semantic.unresolved_type"
            } else {
                "semantic.recursive_type_alias"
            },
            alias_declarations[&id].2.clone(),
            RecoveryAction::None,
        ));
    }

    let mut input = ProgramInput {
        namespace,
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
    let mut interfaces = BTreeMap::<DefinitionId, Vec<InterfaceRequirement>>::new();
    for definition in definitions
        .values()
        .filter(|definition| definition.kind == DeclarationKind::Interface)
    {
        let Some(DeclarationSyntax::Interface(interface)) = definition.declaration.syntax.as_ref()
        else {
            continue;
        };
        let self_parameters = BTreeMap::from([("Self".to_owned(), Type::Infer)]);
        let mut names = BTreeSet::new();
        let mut requirements = Vec::new();
        for requirement in &interface.requirements {
            if !names.insert(&requirement.name) {
                diagnostics.push(
                    Diagnostic::new(
                        "semantic.duplicate_interface_requirement",
                        requirement.range.clone(),
                        RecoveryAction::None,
                    )
                    .with_parameter("name", requirement.name.clone()),
                );
                continue;
            }
            let Some(parameters) = resolve_parameters(
                &requirement.parameters,
                definition.module,
                &input.namespace,
                &input.nominal_displays,
                &alias_types,
                &self_parameters,
                &mut diagnostics,
            ) else {
                continue;
            };
            let Some(return_type) = resolve_type(
                &requirement.return_type,
                definition.module,
                &input.namespace,
                &input.nominal_displays,
                &alias_types,
                &self_parameters,
            ) else {
                diagnostics.push(Diagnostic::new(
                    "semantic.unresolved_type",
                    requirement.range.clone(),
                    RecoveryAction::None,
                ));
                continue;
            };
            requirements.push(InterfaceRequirement {
                name: requirement.name.clone(),
                modifier: requirement.modifier,
                parameters,
                return_type,
                range: requirement.range.clone(),
            });
        }
        interfaces.insert(definition.id, requirements);
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
                    &function.parameters,
                    definition.module,
                    &input.namespace,
                    &input.nominal_displays,
                    &alias_types,
                    &type_parameters,
                    &mut diagnostics,
                ) {
                    Some(parameters) => parameters,
                    None => continue,
                };
                validate_parameter_modes(
                    &parameters,
                    &function.parameters,
                    &definitions,
                    &mut diagnostics,
                );
                let Some(return_type) = resolve_type(
                    &function.return_type,
                    definition.module,
                    &input.namespace,
                    &input.nominal_displays,
                    &alias_types,
                    &type_parameters,
                ) else {
                    diagnostics.push(Diagnostic::new(
                        "semantic.unresolved_type",
                        definition.declaration.range.clone(),
                        RecoveryAction::None,
                    ));
                    continue;
                };
                if return_type != Type::Unit && !statements_definitely_terminate(&function.body) {
                    diagnostics.push(Diagnostic::new(
                        "semantic.missing_return",
                        definition.declaration.range.clone(),
                        RecoveryAction::None,
                    ));
                    continue;
                }
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
                    if !parameters.is_empty()
                        || !function.type_parameters.is_empty()
                        || function.modifier == crate::syntax::FunctionModifier::Async
                        || return_type != Type::Builtin(BuiltinType::Image)
                    {
                        diagnostics.push(Diagnostic::new(
                            "semantic.invalid_image_constructor_signature",
                            definition.declaration.range.clone(),
                            RecoveryAction::None,
                        ));
                    }
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
                    &input.namespace,
                    &input.nominal_displays,
                    &alias_types,
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
                    for parameter in &test.parameters {
                        if parameter.ownership != OwnershipSyntax::Take {
                            diagnostics.push(Diagnostic::new(
                                "test.parameter_requires_take",
                                parameter.range.clone(),
                                RecoveryAction::None,
                            ));
                        }
                    }
                    let Some(parameters) = resolve_parameters(
                        &test.parameters,
                        definition.module,
                        &input.namespace,
                        &input.nominal_displays,
                        &alias_types,
                        &BTreeMap::new(),
                        &mut diagnostics,
                    ) else {
                        continue;
                    };
                    let resolved = ResolvedTest {
                        id,
                        suite: definition.name.clone(),
                        test: test.name.clone(),
                        asynchronous: test.asynchronous,
                        parameters,
                        module: definition.module,
                        body: test.body.clone(),
                        source: test.range.clone(),
                    };
                    input.tests.insert(id, resolved);
                    input.namespace.declare(
                        definition.module,
                        Arc::from([definition.name.clone(), test.name.clone()]),
                        ResolvedName::Test(id),
                        definition.public,
                    );
                    tests_in_source_order.push(TestRecord {
                        id,
                        range: test.range.clone(),
                    });
                }
            }
            DeclarationSyntax::Struct(struct_) | DeclarationSyntax::ResourceStruct(struct_) => {
                let resource = definition.kind == DeclarationKind::ResourceStruct;
                let mut field_names = BTreeSet::new();
                let mut resolved_fields = Vec::new();
                for field in &struct_.fields {
                    if !field_names.insert(&field.name) {
                        diagnostics.push(
                            Diagnostic::new(
                                "semantic.duplicate_field",
                                field.range.clone(),
                                RecoveryAction::None,
                            )
                            .with_parameter("name", field.name.clone()),
                        );
                        continue;
                    }
                    let Some(type_) = resolve_type(
                        &field.type_syntax,
                        definition.module,
                        &input.namespace,
                        &input.nominal_displays,
                        &alias_types,
                        &BTreeMap::new(),
                    ) else {
                        diagnostics.push(Diagnostic::new(
                            "semantic.unresolved_type",
                            field.range.clone(),
                            RecoveryAction::None,
                        ));
                        continue;
                    };
                    if !resource && is_resource_type(&type_, &definitions) {
                        diagnostics.push(Diagnostic::new(
                            "semantic.resource_field_requires_resource_struct",
                            field.range.clone(),
                            RecoveryAction::None,
                        ));
                    }
                    if definition.public
                        && field.public
                        && exposes_private_type(&type_, &definitions)
                    {
                        diagnostics.push(Diagnostic::new(
                            "semantic.private_type_in_public_signature",
                            field.range.clone(),
                            RecoveryAction::None,
                        ));
                    }
                    resolved_fields.push(ResolvedField {
                        name: field.name.clone(),
                        public: field.public,
                        type_,
                    });
                    let _ = field.mutable;
                }
                input.structs.insert(
                    definition.id,
                    ResolvedStruct {
                        definition: definition.id,
                        module: definition.module,
                        display: Arc::from(definition.name.as_str()),
                        fields: resolved_fields,
                    },
                );
                for member in &struct_.functions {
                    if field_names.contains(&member.name) {
                        diagnostics.push(
                            Diagnostic::new(
                                "semantic.duplicate_member",
                                member.range.clone(),
                                RecoveryAction::None,
                            )
                            .with_parameter("name", member.name.clone()),
                        );
                    }
                }
                let member_functions = resolve_associated_functions(
                    definition,
                    &struct_.functions,
                    &input.namespace,
                    &input.nominal_displays,
                    &alias_types,
                    &definitions,
                    identity_catalog,
                    &mut diagnostics,
                );
                for function in &member_functions {
                    input.functions.insert(function.id, function.clone());
                }
                for interface in &struct_.implements {
                    let Some(ResolvedName::Nominal(interface_id)) = input
                        .namespace
                        .resolve(definition.module, &interface.segments)
                    else {
                        diagnostics.push(Diagnostic::new(
                            "semantic.unresolved_interface",
                            definition.declaration.range.clone(),
                            RecoveryAction::None,
                        ));
                        continue;
                    };
                    if definitions
                        .get(&interface_id)
                        .is_none_or(|record| record.kind != DeclarationKind::Interface)
                    {
                        diagnostics.push(Diagnostic::new(
                            "semantic.implements_requires_interface",
                            definition.declaration.range.clone(),
                            RecoveryAction::None,
                        ));
                        continue;
                    }
                    for requirement in interfaces.get(&interface_id).into_iter().flatten() {
                        let implementation = member_functions.iter().find(|function| {
                            function
                                .name
                                .rsplit_once('.')
                                .is_some_and(|(_, name)| name == requirement.name)
                        });
                        let Some(implementation) = implementation else {
                            diagnostics.push(
                                Diagnostic::new(
                                    "semantic.missing_interface_requirement",
                                    definition.declaration.range.clone(),
                                    RecoveryAction::None,
                                )
                                .with_parameter("member", requirement.name.clone()),
                            );
                            continue;
                        };
                        let signature_matches = implementation.modifier == requirement.modifier
                            && implementation.parameters.len() == requirement.parameters.len()
                            && implementation
                                .parameters
                                .iter()
                                .zip(&requirement.parameters)
                                .all(|(actual, expected)| {
                                    actual.ownership == expected.ownership
                                        && can_unify(&actual.type_, &expected.type_)
                                })
                            && can_unify(&implementation.return_type, &requirement.return_type);
                        if !signature_matches {
                            diagnostics.push(
                                Diagnostic::new(
                                    "semantic.interface_signature_mismatch",
                                    implementation.source.clone(),
                                    RecoveryAction::None,
                                )
                                .with_label(requirement.range.clone(), DiagnosticLabelRole::Related)
                                .with_parameter("member", requirement.name.clone()),
                            );
                        }
                    }
                }
            }
            DeclarationSyntax::Interface(_) => {}
            DeclarationSyntax::Enum(enum_) => {
                let type_parameters = enum_
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
                for variant in &enum_.variants {
                    let Some(parameters) = resolve_parameters(
                        &variant.parameters,
                        definition.module,
                        &input.namespace,
                        &input.nominal_displays,
                        &alias_types,
                        &type_parameters,
                        &mut diagnostics,
                    ) else {
                        continue;
                    };
                    if variant
                        .parameters
                        .iter()
                        .any(|parameter| parameter.ownership != OwnershipSyntax::Value)
                    {
                        diagnostics.push(Diagnostic::new(
                            "semantic.enum_payload_requires_value_mode",
                            variant.range.clone(),
                            RecoveryAction::None,
                        ));
                    }
                    let id = identity_catalog
                        .variant(definition.id, &variant.name)
                        .expect("structured enum variant was catalogued");
                    input.variants.insert(id, ResolvedVariant { parameters });
                }
                for function in resolve_associated_functions(
                    definition,
                    &enum_.functions,
                    &input.namespace,
                    &input.nominal_displays,
                    &alias_types,
                    &definitions,
                    identity_catalog,
                    &mut diagnostics,
                ) {
                    input.functions.insert(function.id, function);
                }
            }
            DeclarationSyntax::TypeAlias(_) | DeclarationSyntax::Pool => {}
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
                    OwnershipSyntax::Read => OwnershipMode::Read,
                    OwnershipSyntax::Mut => OwnershipMode::Mut,
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
                    OwnershipSyntax::Read => OwnershipMode::Read,
                    OwnershipSyntax::Mut => OwnershipMode::Mut,
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
            if kind == typed_hir::CreatorFailureKind::ReadAfterMove {
                diagnostics.push(Diagnostic::new(
                    "semantic.read_after_move",
                    site,
                    RecoveryAction::None,
                ));
            } else if kind == typed_hir::CreatorFailureKind::ImmutableReassignment {
                diagnostics.push(Diagnostic::new(
                    "semantic.immutable_reassignment",
                    site,
                    RecoveryAction::None,
                ));
            } else if kind == typed_hir::CreatorFailureKind::DuplicateLocal {
                diagnostics.push(Diagnostic::new(
                    "semantic.duplicate_local",
                    site,
                    RecoveryAction::None,
                ));
            } else if kind == typed_hir::CreatorFailureKind::ExpectRequiresBool {
                diagnostics.push(Diagnostic::new(
                    "semantic.expect_requires_bool",
                    site,
                    RecoveryAction::None,
                ));
            } else if kind == typed_hir::CreatorFailureKind::AwaitRequiresAsync {
                diagnostics.push(Diagnostic::new(
                    "semantic.await_requires_async",
                    site,
                    RecoveryAction::None,
                ));
            } else {
                diagnostics.push(
                    Diagnostic::new("semantic.invalid_typed_hir", site, RecoveryAction::None)
                        .with_parameter("kind", kind.code()),
                );
            }
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
        let concrete_facts = solve_specialization_facts(program, cancellation);
        if cancellation.is_cancelled() {
            return cancelled();
        }
        for (id, facts) in &facts {
            let function = &program.functions()[id];
            if facts.suspends && function.modifier != crate::syntax::FunctionModifier::Async {
                diagnostics.push(Diagnostic::new(
                    "semantic.await_requires_async",
                    function.source.clone(),
                    RecoveryAction::None,
                ));
            }
            if function.modifier == crate::syntax::FunctionModifier::Pure && !facts.pure {
                diagnostics.push(Diagnostic::new(
                    "semantic.pure_effect_violation",
                    function.source.clone(),
                    RecoveryAction::None,
                ));
            }
        }
        for (id, facts) in &concrete_facts {
            let specialization = &program.specializations()[id];
            let function = &program.functions()[&specialization.definition];
            function_facts.push(FunctionFactsObservation::new(
                id.0,
                function.name.clone(),
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
            .map(|(id, facts)| (*id, facts.calls.keys().copied().collect()))
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
            let mut engine = Engine::new(program, cancellation);
            for constant in program.constants().values() {
                let run = engine.evaluate_constant(constant.id);
                if run.outcome == EvaluationOutcome::Cancelled {
                    return cancelled();
                }
                if let Err(evidence) =
                    map_evaluation_failure(&run.outcome, &constant.source, &mut diagnostics)
                {
                    return defect("constant evaluation", evidence);
                }
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
                            let run = engine.evaluate_expression(&expression);
                            if run.outcome == EvaluationOutcome::Cancelled {
                                return cancelled();
                            }
                            match &run.outcome {
                                EvaluationOutcome::Completed(crate::CanonicalValue::Bool(true)) => {
                                }
                                EvaluationOutcome::Completed(_) => {
                                    diagnostics.push(Diagnostic::new(
                                        "evaluation.assertion_failed",
                                        assertion.range.clone(),
                                        RecoveryAction::None,
                                    ))
                                }
                                EvaluationOutcome::Defect { evidence } => {
                                    return defect(
                                        "compile-time assertion evaluation",
                                        evidence.clone(),
                                    );
                                }
                                _ => {
                                    if let Err(evidence) = map_evaluation_failure(
                                        &run.outcome,
                                        &assertion.range,
                                        &mut diagnostics,
                                    ) {
                                        return defect(
                                            "compile-time assertion evaluation",
                                            evidence,
                                        );
                                    }
                                }
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
                let run = engine.evaluate_function(*image);
                if run.outcome == EvaluationOutcome::Cancelled {
                    return cancelled();
                }
                if let Err(evidence) =
                    map_evaluation_failure(&run.outcome, image_range, &mut diagnostics)
                {
                    return defect("Image Constructor evaluation", evidence);
                }
                if root == Root::Test {
                    test_plan = plan_tests(
                        program,
                        &tests_in_source_order,
                        &run.test_applications,
                        image_range,
                        &mut diagnostics,
                    );
                }
                if let Err(failure) = seal_construction_graph(run.root_handle, &run.constructions) {
                    match failure {
                        GraphSealFailure::Creator(kind) => diagnostics.push(
                            Diagnostic::new(
                                "construction.invalid_graph",
                                image_range.clone(),
                                RecoveryAction::None,
                            )
                            .with_parameter("kind", kind),
                        ),
                        GraphSealFailure::Defect(evidence) => {
                            return defect("construction graph verification", Arc::from(evidence));
                        }
                    }
                }
                constructions.extend(run.constructions.into_iter().map(|construction| {
                    ConstructionObservation::new(
                        construction.identity,
                        match construction.kind {
                            BuildKind::Image => crate::ConstructionKind::Image,
                            BuildKind::Test => crate::ConstructionKind::Test,
                        },
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

fn resolved_name(definition: &DefinitionRecord) -> Option<ResolvedName> {
    Some(match definition.kind {
        DeclarationKind::Function => ResolvedName::Function(definition.id),
        DeclarationKind::Constant => ResolvedName::Constant(definition.id),
        DeclarationKind::TypeAlias => ResolvedName::Alias(definition.id),
        DeclarationKind::Struct
        | DeclarationKind::ResourceStruct
        | DeclarationKind::Enum
        | DeclarationKind::Interface => ResolvedName::Nominal(definition.id),
        DeclarationKind::Pool | DeclarationKind::Suite => return None,
    })
}

fn is_nominal(kind: DeclarationKind) -> bool {
    matches!(
        kind,
        DeclarationKind::Struct
            | DeclarationKind::ResourceStruct
            | DeclarationKind::Enum
            | DeclarationKind::Interface
    )
}

fn statements_definitely_terminate(statements: &[StatementSyntax]) -> bool {
    statements.iter().any(|statement| match statement {
        StatementSyntax::Return { .. } | StatementSyntax::Panic { .. } => true,
        StatementSyntax::If {
            then_branch,
            else_branch,
            ..
        } => {
            !else_branch.is_empty()
                && statements_definitely_terminate(then_branch)
                && statements_definitely_terminate(else_branch)
        }
        StatementSyntax::Assign { .. }
        | StatementSyntax::Expect { .. }
        | StatementSyntax::Evaluate(_)
        | StatementSyntax::Pass(_) => false,
    })
}

#[allow(clippy::too_many_arguments)]
fn resolve_associated_functions(
    definition: &DefinitionRecord,
    functions: &[crate::syntax::MemberFunctionSyntax],
    namespace: &NamespaceCatalog,
    displays: &BTreeMap<DefinitionId, Arc<str>>,
    aliases: &BTreeMap<DefinitionId, Type>,
    definitions: &BTreeMap<DefinitionId, DefinitionRecord>,
    identity_catalog: &IdentityCatalog,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<ResolvedFunction> {
    let self_type = Type::Nominal {
        definition: definition.id,
        display: displays[&definition.id].clone(),
    };
    let mut member_names = BTreeSet::new();
    let mut resolved = Vec::new();
    for member in functions {
        if !member_names.insert(&member.name) {
            diagnostics.push(
                Diagnostic::new(
                    "semantic.duplicate_member",
                    member.range.clone(),
                    RecoveryAction::None,
                )
                .with_parameter("name", member.name.clone()),
            );
            continue;
        }
        let id = identity_catalog
            .associated_function(definition.id, &member.name)
            .expect("associated function identity exists");
        let mut type_parameters = BTreeMap::from([("Self".to_owned(), self_type.clone())]);
        for (index, name) in member.function.type_parameters.iter().enumerate() {
            if type_parameters
                .insert(
                    name.clone(),
                    Type::Parameter {
                        owner: id,
                        id: TypeParameterId(u16::try_from(index).unwrap_or(u16::MAX)),
                        display: Arc::from(name.as_str()),
                    },
                )
                .is_some()
            {
                diagnostics.push(
                    Diagnostic::new(
                        "semantic.duplicate_type_parameter",
                        member.range.clone(),
                        RecoveryAction::None,
                    )
                    .with_parameter("name", name.clone()),
                );
            }
        }
        let Some(parameters) = resolve_parameters(
            &member.function.parameters,
            definition.module,
            namespace,
            displays,
            aliases,
            &type_parameters,
            diagnostics,
        ) else {
            continue;
        };
        for parameter in &parameters {
            let resource = is_resource_type(&parameter.type_, definitions);
            match parameter.ownership {
                OwnershipSyntax::Value if resource => diagnostics.push(Diagnostic::new(
                    "semantic.resource_parameter_requires_mode",
                    member.range.clone(),
                    RecoveryAction::None,
                )),
                OwnershipSyntax::Read | OwnershipSyntax::Mut | OwnershipSyntax::Take
                    if parameter.name != "self" && !resource =>
                {
                    diagnostics.push(Diagnostic::new(
                        "semantic.ownership_mode_requires_resource",
                        member.range.clone(),
                        RecoveryAction::None,
                    ));
                }
                _ => {}
            }
        }
        let Some(return_type) = resolve_type(
            &member.function.return_type,
            definition.module,
            namespace,
            displays,
            aliases,
            &type_parameters,
        ) else {
            diagnostics.push(Diagnostic::new(
                "semantic.unresolved_type",
                member.range.clone(),
                RecoveryAction::None,
            ));
            continue;
        };
        if return_type != Type::Unit && !statements_definitely_terminate(&member.function.body) {
            diagnostics.push(Diagnostic::new(
                "semantic.missing_return",
                member.range.clone(),
                RecoveryAction::None,
            ));
            continue;
        }
        if definition.public
            && member.public
            && (exposes_private_type(&return_type, definitions)
                || parameters
                    .iter()
                    .any(|parameter| exposes_private_type(&parameter.type_, definitions)))
        {
            diagnostics.push(Diagnostic::new(
                "semantic.private_type_in_public_signature",
                member.range.clone(),
                RecoveryAction::None,
            ));
        }
        resolved.push(ResolvedFunction {
            id,
            module: definition.module,
            module_display: definition.module_display.clone(),
            name: format!("{}.{}", definition.name, member.name),
            modifier: member.function.modifier,
            type_parameters: (0..member.function.type_parameters.len())
                .map(|index| TypeParameterId(u16::try_from(index).unwrap_or(u16::MAX)))
                .collect(),
            parameters,
            return_type,
            body: member.function.body.clone(),
            source: member.range.clone(),
        });
    }
    resolved
}

fn resolve_parameters(
    parameters: &[ParameterSyntax],
    module: ModuleId,
    namespace: &NamespaceCatalog,
    displays: &BTreeMap<DefinitionId, Arc<str>>,
    aliases: &BTreeMap<DefinitionId, Type>,
    type_parameters: &BTreeMap<String, Type>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Vec<ResolvedParameter>> {
    parameters
        .iter()
        .map(|parameter| {
            let type_ = resolve_type(
                &parameter.type_syntax,
                module,
                namespace,
                displays,
                aliases,
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

fn validate_parameter_modes(
    parameters: &[ResolvedParameter],
    syntax: &[ParameterSyntax],
    definitions: &BTreeMap<DefinitionId, DefinitionRecord>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (parameter, syntax) in parameters.iter().zip(syntax) {
        let resource = is_resource_type(&parameter.type_, definitions);
        let invalid = match parameter.ownership {
            OwnershipSyntax::Value => resource,
            OwnershipSyntax::Read | OwnershipSyntax::Mut | OwnershipSyntax::Take => !resource,
        };
        if invalid {
            diagnostics.push(Diagnostic::new(
                if resource {
                    "semantic.resource_parameter_requires_mode"
                } else {
                    "semantic.ownership_mode_requires_resource"
                },
                syntax.range.clone(),
                RecoveryAction::None,
            ));
        }
    }
}

fn resolve_type(
    syntax: &TypeSyntax,
    module: ModuleId,
    namespace: &NamespaceCatalog,
    displays: &BTreeMap<DefinitionId, Arc<str>>,
    aliases: &BTreeMap<DefinitionId, Type>,
    type_parameters: &BTreeMap<String, Type>,
) -> Option<Type> {
    match syntax {
        TypeSyntax::Unit => Some(Type::Unit),
        TypeSyntax::Infer => Some(Type::Infer),
        TypeSyntax::Array(element) => Some(Type::Array(Arc::new(resolve_type(
            element,
            module,
            namespace,
            displays,
            aliases,
            type_parameters,
        )?))),
        TypeSyntax::Tuple(members) => Some(Type::Tuple(
            members
                .iter()
                .map(|member| {
                    resolve_type(
                        member,
                        module,
                        namespace,
                        displays,
                        aliases,
                        type_parameters,
                    )
                })
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
            match namespace.resolve(module, &name.segments)? {
                ResolvedName::Nominal(id) => Some(Type::Nominal {
                    definition: id,
                    display: displays.get(&id)?.clone(),
                }),
                ResolvedName::Alias(id) => aliases.get(&id).cloned(),
                _ => None,
            }
        }
        TypeSyntax::Apply { base, arguments } => {
            let values = arguments
                .iter()
                .map(|argument| {
                    resolve_type(
                        argument,
                        module,
                        namespace,
                        displays,
                        aliases,
                        type_parameters,
                    )
                })
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

fn unresolved_alias_dependencies(
    syntax: &TypeSyntax,
    module: ModuleId,
    namespace: &NamespaceCatalog,
) -> (BTreeSet<DefinitionId>, bool) {
    let mut dependencies = BTreeSet::new();
    let mut missing = false;
    fn visit(
        syntax: &TypeSyntax,
        module: ModuleId,
        namespace: &NamespaceCatalog,
        dependencies: &mut BTreeSet<DefinitionId>,
        missing: &mut bool,
    ) {
        match syntax {
            TypeSyntax::Unit | TypeSyntax::Infer => {}
            TypeSyntax::Array(value) => visit(value, module, namespace, dependencies, missing),
            TypeSyntax::Tuple(values) => {
                for value in values {
                    visit(value, module, namespace, dependencies, missing);
                }
            }
            TypeSyntax::Named(name) => {
                if resolve_builtin_type(name).is_some() {
                    return;
                }
                match namespace.resolve(module, &name.segments) {
                    Some(ResolvedName::Alias(id)) => {
                        dependencies.insert(id);
                    }
                    Some(ResolvedName::Nominal(_)) => {}
                    _ => *missing = true,
                }
            }
            TypeSyntax::Apply { base, arguments } => {
                let builtin = matches!(
                    base.segments
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .as_slice(),
                    ["Result"] | ["Option"]
                );
                if !builtin && namespace.resolve(module, &base.segments).is_none() {
                    *missing = true;
                }
                for argument in arguments {
                    visit(argument, module, namespace, dependencies, missing);
                }
            }
        }
    }
    visit(syntax, module, namespace, &mut dependencies, &mut missing);
    (dependencies, missing)
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

fn is_resource_type(type_: &Type, definitions: &BTreeMap<DefinitionId, DefinitionRecord>) -> bool {
    match type_ {
        Type::Nominal { definition, .. } => definitions
            .get(definition)
            .is_some_and(|definition| definition.kind == DeclarationKind::ResourceStruct),
        Type::Array(value) | Type::Option(value) => is_resource_type(value, definitions),
        Type::Tuple(values) => values
            .iter()
            .any(|value| is_resource_type(value, definitions)),
        Type::Result { success, error } => {
            is_resource_type(success, definitions)
                || error
                    .as_ref()
                    .is_some_and(|error| is_resource_type(error, definitions))
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
    loop {
        if cancellation.is_cancelled() {
            return BTreeMap::new();
        }
        let previous = facts.clone();
        let mut next = base.clone();
        for current in next.values_mut() {
            for call in current.calls.keys() {
                if let Some(callee) = previous.get(call) {
                    current.pure &= callee.pure;
                    current.may_panic |= callee.may_panic;
                    current.suspends |= callee.suspends;
                    current.evaluator_eligible &= callee.evaluator_eligible;
                    current.constructs.extend(callee.constructs.iter().copied());
                    current.ownership_transfer |= callee.ownership_transfer;
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
        .map(|(id, facts)| (*id, facts.calls.keys().copied().collect()))
        .collect::<BTreeMap<_, _>>();
    let recursive = crate::graph::recursive_nodes(&graph);
    let costs = solve_weighted_costs(&base, &recursive, |fact| &fact.calls);
    for (id, cost) in costs {
        facts.get_mut(&id).expect("fact exists").logical_cost = cost;
    }
    for (id, current) in &mut facts {
        current.bounded = current.logical_cost != u64::MAX && !recursive.contains(id);
    }
    facts
}

fn solve_specialization_facts(
    program: &VerifiedProgram,
    cancellation: &Cancellation,
) -> BTreeMap<SpecializationId, FunctionFacts> {
    let base = program
        .specialized_functions()
        .iter()
        .map(|(id, function)| (*id, local_facts(function)))
        .collect::<BTreeMap<_, _>>();
    let mut facts = base.clone();
    loop {
        if cancellation.is_cancelled() {
            return BTreeMap::new();
        }
        let previous = facts.clone();
        let mut next = base.clone();
        for current in next.values_mut() {
            for call in current.specialization_calls.keys() {
                if let Some(callee) = previous.get(call) {
                    current.pure &= callee.pure;
                    current.may_panic |= callee.may_panic;
                    current.suspends |= callee.suspends;
                    current.evaluator_eligible &= callee.evaluator_eligible;
                    current.constructs.extend(callee.constructs.iter().copied());
                    current.ownership_transfer |= callee.ownership_transfer;
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
        .map(|(id, facts)| (*id, facts.specialization_calls.keys().copied().collect()))
        .collect::<BTreeMap<_, BTreeSet<_>>>();
    let recursive = crate::graph::recursive_nodes(&graph);
    let costs = solve_weighted_costs(&base, &recursive, |fact| &fact.specialization_calls);
    for (id, cost) in costs {
        let fact = facts.get_mut(&id).expect("specialization fact exists");
        fact.logical_cost = cost;
        fact.bounded = cost != u64::MAX && !recursive.contains(&id);
    }
    facts
}

fn solve_weighted_costs<N>(
    base: &BTreeMap<N, FunctionFacts>,
    recursive: &BTreeSet<N>,
    edges: impl Fn(&FunctionFacts) -> &BTreeMap<N, u64>,
) -> BTreeMap<N, u64>
where
    N: Copy + Ord,
{
    let mut remaining = BTreeMap::new();
    let mut callers = BTreeMap::<N, Vec<(N, u64)>>::new();
    let mut costs = base
        .iter()
        .map(|(id, facts)| {
            (
                *id,
                if recursive.contains(id) {
                    u64::MAX
                } else {
                    facts.logical_cost
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    for (caller, facts) in base {
        if recursive.contains(caller) {
            continue;
        }
        let dependencies = edges(facts)
            .iter()
            .filter(|(callee, _)| base.contains_key(callee))
            .map(|(callee, multiplicity)| (*callee, *multiplicity))
            .collect::<Vec<_>>();
        remaining.insert(*caller, dependencies.len());
        for (callee, multiplicity) in dependencies {
            callers
                .entry(callee)
                .or_default()
                .push((*caller, multiplicity));
        }
    }
    let mut ready = remaining
        .iter()
        .filter_map(|(id, count)| (*count == 0).then_some(*id))
        .collect::<BTreeSet<_>>();
    ready.extend(recursive.iter().copied());
    while let Some(id) = ready.pop_first() {
        let callee_cost = costs[&id];
        for (caller, multiplicity) in callers.get(&id).into_iter().flatten() {
            costs.entry(*caller).and_modify(|cost| {
                *cost = cost.saturating_add(callee_cost.saturating_mul(*multiplicity));
            });
            let count = remaining
                .get_mut(caller)
                .expect("caller has dependency count");
            *count -= 1;
            if *count == 0 {
                ready.insert(*caller);
            }
        }
    }
    costs
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
        calls: BTreeMap::new(),
        specialization_calls: BTreeMap::new(),
    };
    visit_statements(&function.body, &mut facts);
    facts.pure = !facts.suspends;
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
            Statement::Expect { condition, .. } => visit_expression(condition, facts),
            Statement::Initialize { value, .. }
            | Statement::Assign { value, .. }
            | Statement::Evaluate(value) => visit_expression(value, facts),
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
                CallTarget::TemplateFunction { definition, .. } => {
                    *facts.calls.entry(*definition).or_default() += 1;
                }
                CallTarget::Function {
                    definition,
                    specialization,
                    ..
                } => {
                    *facts.calls.entry(*definition).or_default() += 1;
                    *facts
                        .specialization_calls
                        .entry(*specialization)
                        .or_default() += 1;
                }
                CallTarget::Build(primitive) => {
                    facts.constructs.insert(primitive.kind);
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
        ExpressionKind::Negate(value) => {
            facts.may_panic |= matches!(value.type_, Type::Integer(_));
            visit_expression(value, facts);
        }
        ExpressionKind::Propagate(value) => visit_expression(value, facts),
        ExpressionKind::Await(value) => {
            facts.suspends = true;
            visit_expression(value, facts);
        }
        ExpressionKind::Binary {
            operator,
            left,
            right,
        } => {
            if matches!(left.type_, Type::Integer(_))
                && matches!(
                    operator,
                    typed_hir::BinaryOperator::Add
                        | typed_hir::BinaryOperator::Subtract
                        | typed_hir::BinaryOperator::Multiply
                        | typed_hir::BinaryOperator::Divide
                        | typed_hir::BinaryOperator::Remainder
                )
            {
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
    enum Work<'a> {
        Statement(&'a Statement),
        Expression(&'a Expression),
    }
    let mut pending = statements
        .iter()
        .rev()
        .map(Work::Statement)
        .collect::<Vec<_>>();
    while let Some(work) = pending.pop() {
        match work {
            Work::Expression(value) => {
                visitor(value);
                match &value.kind {
                    ExpressionKind::Call { arguments, .. } | ExpressionKind::Array(arguments) => {
                        pending.extend(arguments.iter().rev().map(Work::Expression));
                    }
                    ExpressionKind::Negate(value)
                    | ExpressionKind::Await(value)
                    | ExpressionKind::Propagate(value) => {
                        pending.push(Work::Expression(value));
                    }
                    ExpressionKind::Binary { left, right, .. } => {
                        pending.push(Work::Expression(right));
                        pending.push(Work::Expression(left));
                    }
                    ExpressionKind::Literal(_)
                    | ExpressionKind::Read(_)
                    | ExpressionKind::Constant(_) => {}
                }
            }
            Work::Statement(statement) => match statement {
                Statement::Return { value, .. } => {
                    if let Some(value) = value {
                        pending.push(Work::Expression(value));
                    }
                }
                Statement::Panic { value, .. }
                | Statement::Expect {
                    condition: value, ..
                }
                | Statement::Initialize { value, .. }
                | Statement::Assign { value, .. }
                | Statement::Evaluate(value) => pending.push(Work::Expression(value)),
                Statement::If {
                    condition,
                    then_branch,
                    else_branch,
                    ..
                } => {
                    pending.extend(else_branch.iter().rev().map(Work::Statement));
                    pending.extend(then_branch.iter().rev().map(Work::Statement));
                    pending.push(Work::Expression(condition));
                }
                Statement::Pass(_) => {}
            },
        }
    }
}

fn recursive_functions(program: &VerifiedProgram) -> Vec<SourceRange> {
    let graph = program
        .functions()
        .iter()
        .map(|(id, function)| (*id, local_facts(function).calls.keys().copied().collect()))
        .collect::<BTreeMap<_, _>>();
    crate::graph::recursive_nodes(&graph)
        .iter()
        .map(|id| program.functions()[id].source.clone())
        .collect()
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
                            OwnershipSyntax::Read => OwnershipMode::Read,
                            OwnershipSyntax::Mut => OwnershipMode::Mut,
                            OwnershipSyntax::Take => OwnershipMode::Take,
                        },
                    )
                })
                .collect(),
        ));
    }
    plan
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GraphSealFailure {
    Creator(&'static str),
    Defect(&'static str),
}

fn seal_construction_graph(
    root_handle: Option<(BuildKind, u128)>,
    constructions: &[Construction],
) -> Result<(), GraphSealFailure> {
    let Some((kind, root)) = root_handle else {
        return Err(GraphSealFailure::Defect(
            "Image evaluation produced no root",
        ));
    };
    if kind != BuildKind::Image {
        return Err(GraphSealFailure::Creator("returned_root_is_not_image"));
    }
    if constructions
        .iter()
        .filter(|construction| construction.kind == BuildKind::Image)
        .count()
        != 1
    {
        return Err(GraphSealFailure::Creator("multiple_image_roots"));
    }
    let mut graph = BTreeMap::new();
    for construction in constructions {
        if graph
            .insert(construction.identity, construction.edges.as_slice())
            .is_some()
        {
            return Err(GraphSealFailure::Defect(
                "duplicate Construction identity escaped the evaluator catalog",
            ));
        }
    }
    if !graph.contains_key(&root) {
        return Err(GraphSealFailure::Defect(
            "returned Image root does not name a construction",
        ));
    }
    if graph
        .values()
        .flat_map(|edges| edges.iter())
        .any(|edge| !graph.contains_key(edge))
    {
        return Err(GraphSealFailure::Defect(
            "construction edge names a node outside its evaluation root",
        ));
    }
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
        return Err(GraphSealFailure::Creator("unreachable_construction"));
    }
    Ok(())
}

fn map_evaluation_failure(
    outcome: &EvaluationOutcome,
    range: &SourceRange,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), Arc<str>> {
    match outcome {
        EvaluationOutcome::CreatorRejected { kind } => diagnostics.push(
            Diagnostic::new("evaluation.rejected", range.clone(), RecoveryAction::None)
                .with_parameter("kind", kind.code()),
        ),
        EvaluationOutcome::Panicked { kind, site } => diagnostics.push(
            Diagnostic::new("evaluation.panicked", site.clone(), RecoveryAction::None)
                .with_parameter("kind", kind.code()),
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
            .with_parameter("policy", policy.code())
            .with_unsigned_parameter("ceiling", u128::from(*ceiling))
            .with_unsigned_parameter("used", u128::from(*used)),
        ),
        EvaluationOutcome::Defect { evidence } => return Err(evidence.clone()),
        EvaluationOutcome::Completed(_) | EvaluationOutcome::Cancelled => {}
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cost_fact(cost: u64, calls: BTreeMap<DefinitionId, u64>) -> FunctionFacts {
        FunctionFacts {
            pure: true,
            may_panic: false,
            suspends: false,
            evaluator_eligible: true,
            ownership_transfer: false,
            bounded: true,
            logical_cost: cost,
            constructs: BTreeSet::new(),
            calls,
            specialization_calls: BTreeMap::new(),
        }
    }

    #[test]
    fn recursive_logical_cost_is_stable_when_unrelated_functions_are_added() {
        let recursive = DefinitionId(1);
        let base = BTreeMap::from([(recursive, cost_fact(3, BTreeMap::from([(recursive, 1)])))]);
        let recursive_nodes = crate::graph::recursive_nodes(&BTreeMap::from([(
            recursive,
            BTreeSet::from([recursive]),
        )]));
        let before = solve_weighted_costs(&base, &recursive_nodes, |facts| &facts.calls);
        let mut with_unrelated = base;
        with_unrelated.insert(DefinitionId(2), cost_fact(99, BTreeMap::new()));
        let after = solve_weighted_costs(&with_unrelated, &recursive_nodes, |facts| &facts.calls);
        assert_eq!(before[&recursive], u64::MAX);
        assert_eq!(after[&recursive], before[&recursive]);
        assert_eq!(after[&DefinitionId(2)], 99);
    }

    #[test]
    fn graph_sealer_treats_missing_compiler_produced_root_as_defect() {
        assert_eq!(
            seal_construction_graph(None, &[]),
            Err(GraphSealFailure::Defect(
                "Image evaluation produced no root"
            ))
        );
    }

    #[test]
    fn graph_sealer_contains_unknown_duplicate_and_cross_root_identities() {
        let site = SourceRange::new("src/image.wr", 0, 1);
        let node = |identity, edges| Construction {
            identity,
            kind: BuildKind::Image,
            site: site.clone(),
            edges,
        };
        assert!(matches!(
            seal_construction_graph(Some((BuildKind::Image, 2)), &[node(1, vec![])]),
            Err(GraphSealFailure::Defect(_))
        ));
        assert!(matches!(
            seal_construction_graph(
                Some((BuildKind::Image, 1)),
                &[node(1, vec![]), node(1, vec![])]
            ),
            Err(GraphSealFailure::Creator("multiple_image_roots"))
                | Err(GraphSealFailure::Defect(_))
        ));
        assert!(matches!(
            seal_construction_graph(Some((BuildKind::Image, 1)), &[node(1, vec![9])]),
            Err(GraphSealFailure::Defect(_))
        ));
    }

    #[test]
    fn graph_sealer_is_cycle_safe_and_rejects_only_unreachable_nodes() {
        let site = SourceRange::new("src/image.wr", 0, 1);
        let cycle = [
            Construction {
                identity: 1,
                kind: BuildKind::Image,
                site: site.clone(),
                edges: vec![2],
            },
            Construction {
                identity: 2,
                kind: BuildKind::Test,
                site: site.clone(),
                edges: vec![1],
            },
        ];
        assert_eq!(
            seal_construction_graph(Some((BuildKind::Image, 1)), &cycle),
            Ok(())
        );
        let mut unreachable = cycle.to_vec();
        unreachable.push(Construction {
            identity: 3,
            kind: BuildKind::Test,
            site,
            edges: Vec::new(),
        });
        assert_eq!(
            seal_construction_graph(Some((BuildKind::Image, 1)), &unreachable),
            Err(GraphSealFailure::Creator("unreachable_construction"))
        );
    }

    #[test]
    fn evaluator_defect_cannot_be_downgraded_to_a_creator_diagnostic() {
        let mut diagnostics = Vec::new();
        let evidence = Arc::<str>::from("malformed verified operation");
        assert_eq!(
            map_evaluation_failure(
                &EvaluationOutcome::Defect {
                    evidence: evidence.clone(),
                },
                &SourceRange::new("src/image.wr", 0, 0),
                &mut diagnostics,
            ),
            Err(evidence)
        );
        assert!(diagnostics.is_empty());
    }
}
