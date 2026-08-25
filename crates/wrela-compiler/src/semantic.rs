#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::compiler::{
    Cancellation, ConstructionObservation, Defect, Diagnostic, DiagnosticLabelRole,
    EvaluationObservation, EvaluationOutcome, FunctionFactsObservation, FunctionFactsValues,
    IdentityDomain, InferredErrorObservation, OwnershipMode, OwnershipObservation, ProjectFile,
    RecoveryAction, ResolutionKind, ResolutionObservation, Root, SourceRange,
    SpecializationObservation, TestApplicationObservation, TestBindingObservation, TypeObservation,
    TypeRole,
};
use crate::evaluator::{AppliedTest, Engine};
use crate::identity::{IdentityCatalog, IdentityFailure};
use crate::image_evaluation::{self, GraphSealFailure, ImageEvaluationStatus};
use crate::model::{
    ArrayLength, BuiltinType, DefinitionId, ModuleId, PoolTerm, TestId, Type, TypeParameterId,
    resolve_builtin_type,
};
use crate::syntax::{
    AttributeSyntax, ComptimeMemberBranch, ComptimeStatementBranch, Declaration, DeclarationKind,
    DeclarationSyntax, ExpressionSyntax, ExpressionSyntaxKind, OwnershipSyntax, ParameterSyntax,
    ParsedSource, StatementSyntax, TypeSyntax,
};
use crate::type_semantics::{can_unify, contains_resource};
use crate::typed_hir::{
    self, AuthorityContext, BuildAuthority, CallTarget, Expression, ExpressionKind,
    NamespaceCatalog, PoolAuthority, ProgramInput, ResolvedConstant, ResolvedField,
    ResolvedFieldBranch, ResolvedFieldSelection, ResolvedFunction, ResolvedInterface,
    ResolvedInterfaceRequirement, ResolvedName, ResolvedParameter, ResolvedStruct, ResolvedTest,
    ResolvedVariant, Statement, VerifiedProgram,
};

pub(crate) struct SemanticRevision {
    diagnostics: Vec<Diagnostic>,
    observations: SemanticObservations,
    defect: Option<Defect>,
    cancelled: bool,
    selection_value: Option<bool>,
}

#[derive(Default)]
struct SemanticObservations {
    resolutions: Vec<ResolutionObservation>,
    function_facts: Vec<FunctionFactsObservation>,
    types: Vec<TypeObservation>,
    ownership: Vec<OwnershipObservation>,
    specializations: Vec<SpecializationObservation>,
    inferred_errors: Vec<InferredErrorObservation>,
    evaluations: Vec<EvaluationObservation>,
    constructions: Vec<ConstructionObservation>,
    test_plan: Vec<TestApplicationObservation>,
}

pub(crate) struct SemanticProjection {
    pub(crate) resolutions: Arc<[ResolutionObservation]>,
    pub(crate) function_facts: Arc<[FunctionFactsObservation]>,
    pub(crate) types: Arc<[TypeObservation]>,
    pub(crate) ownership: Arc<[OwnershipObservation]>,
    pub(crate) specializations: Arc<[SpecializationObservation]>,
    pub(crate) inferred_errors: Arc<[InferredErrorObservation]>,
    pub(crate) evaluations: Arc<[EvaluationObservation]>,
    pub(crate) constructions: Arc<[ConstructionObservation]>,
    pub(crate) test_plan: Arc<[TestApplicationObservation]>,
}

pub(crate) enum SemanticFailure {
    Cancelled,
    Defect(Defect),
}

impl SemanticRevision {
    pub(crate) fn finalize(
        self,
        semantics: bool,
        evaluation: bool,
        construction: bool,
        tests: bool,
    ) -> Result<(Vec<Diagnostic>, SemanticProjection), SemanticFailure> {
        if self.cancelled {
            return Err(SemanticFailure::Cancelled);
        }
        if let Some(defect) = self.defect {
            return Err(SemanticFailure::Defect(defect));
        }
        let observations = self.observations;
        Ok((
            self.diagnostics,
            SemanticProjection {
                resolutions: project_observations(semantics, observations.resolutions),
                function_facts: project_observations(semantics, observations.function_facts),
                types: project_observations(semantics, observations.types),
                ownership: project_observations(semantics, observations.ownership),
                specializations: project_observations(semantics, observations.specializations),
                inferred_errors: project_observations(semantics, observations.inferred_errors),
                evaluations: project_observations(evaluation, observations.evaluations),
                constructions: project_observations(construction, observations.constructions),
                test_plan: project_observations(tests, observations.test_plan),
            },
        ))
    }
}

fn project_observations<T>(selected: bool, observations: Vec<T>) -> Arc<[T]> {
    if selected {
        observations.into()
    } else {
        Arc::from([])
    }
}

pub(crate) enum SelectionFailure {
    Diagnostic(Diagnostic),
    Defect(Defect),
    Cancelled,
}

pub(crate) fn select_comptime_declarations<'a>(
    parsed_sources: &mut BTreeMap<String, ParsedSource>,
    files: &BTreeMap<&'a str, &'a ProjectFile>,
    authenticated_paths: &BTreeSet<&str>,
    root: Root,
    cancellation: &Cancellation,
    build_authority: &BuildAuthority,
    pool_authority: &PoolAuthority,
) -> Result<(), SelectionFailure> {
    let selections = parsed_sources
        .iter()
        .flat_map(|(path, parsed)| {
            parsed
                .comptime_selections
                .iter()
                .cloned()
                .map(|selection| (path.clone(), selection))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for (path, selection) in selections {
        if cancellation.is_cancelled() {
            return Err(SelectionFailure::Cancelled);
        }
        let mut selected = None;
        for branch in &selection.branches {
            let matches = if let Some(condition) = &branch.condition {
                let mut identities = match crate::identity::catalog(
                    parsed_sources,
                    files,
                    authenticated_paths,
                    cancellation,
                ) {
                    Ok(identities) => identities,
                    Err(IdentityFailure::Cancelled) => {
                        return Err(SelectionFailure::Cancelled);
                    }
                    Err(IdentityFailure::Collision(collision)) => {
                        return Err(SelectionFailure::Defect(Defect::new(
                            "compile-time selection identity catalog",
                            Arc::from(format!(
                                "identity collision {:032x} while selecting {}",
                                collision.digest,
                                selection.range.path()
                            )),
                        )));
                    }
                };
                let analysis = analyze_with_probe(
                    parsed_sources,
                    &mut identities,
                    root,
                    cancellation,
                    false,
                    AuthorityContext::new(build_authority, pool_authority),
                    Some((&path, condition)),
                );
                if analysis.cancelled {
                    return Err(SelectionFailure::Cancelled);
                }
                if let Some(defect) = analysis.defect {
                    return Err(SelectionFailure::Defect(defect));
                }
                if let Some(diagnostic) = analysis.diagnostics.into_iter().next() {
                    return Err(SelectionFailure::Diagnostic(diagnostic));
                }
                analysis.selection_value.ok_or_else(|| {
                    SelectionFailure::Diagnostic(Diagnostic::new(
                        "semantic.invalid_comptime_expression",
                        condition.range.clone(),
                        RecoveryAction::None,
                    ))
                })?
            } else {
                true
            };
            if matches {
                selected = Some(branch.declarations.clone());
                break;
            }
        }
        if let Some(declarations) = selected {
            parsed_sources
                .get_mut(&path)
                .expect("selection source remains reachable")
                .declarations
                .extend(declarations);
        }
    }
    for parsed in parsed_sources.values_mut() {
        parsed
            .declarations
            .sort_by_key(|declaration| declaration.start);
    }
    while let Some((path, range, branches)) = first_nested_member_selection(parsed_sources) {
        let mut selected = None;
        for (index, branch) in branches.iter().enumerate() {
            let matches = if let Some(condition) = &branch.condition {
                evaluate_nested_condition(
                    parsed_sources,
                    files,
                    authenticated_paths,
                    root,
                    cancellation,
                    build_authority,
                    pool_authority,
                    &path,
                    condition,
                )?
            } else {
                true
            };
            if matches {
                selected = Some(index);
                break;
            }
        }
        let selected = selected.map(|index| branches[index].clone());
        if !replace_member_selection(
            &mut parsed_sources
                .get_mut(&path)
                .expect("member selection source remains reachable")
                .declarations,
            &range,
            selected,
        ) {
            return Err(SelectionFailure::Defect(Defect::new(
                "compile-time member selection",
                Arc::from("selected nested member site disappeared"),
            )));
        }
    }
    while let Some((path, range, branches)) = first_nested_statement_selection(parsed_sources) {
        let selected = select_nested_statement_branch(
            parsed_sources,
            files,
            authenticated_paths,
            root,
            cancellation,
            build_authority,
            pool_authority,
            &path,
            &branches,
        )?;
        let replacement =
            selected.map_or_else(Vec::new, |index| branches[index].statements.clone());
        let replaced = replace_statement_selection(
            &mut parsed_sources
                .get_mut(&path)
                .expect("nested selection source remains reachable")
                .declarations,
            &range,
            replacement,
        );
        if !replaced {
            return Err(SelectionFailure::Defect(Defect::new(
                "compile-time statement selection",
                Arc::from("selected nested statement site disappeared"),
            )));
        }
    }
    Ok(())
}

fn first_nested_member_selection(
    parsed_sources: &BTreeMap<String, ParsedSource>,
) -> Option<(String, SourceRange, Vec<ComptimeMemberBranch>)> {
    for (path, parsed) in parsed_sources {
        for declaration in &parsed.declarations {
            let selection = match declaration.syntax.as_ref() {
                Some(DeclarationSyntax::Struct(struct_))
                | Some(DeclarationSyntax::ResourceStruct(struct_)) => struct_
                    .generic_parameters
                    .is_empty()
                    .then(|| struct_.comptime_selections.first())
                    .flatten(),
                Some(DeclarationSyntax::Enum(enum_)) => enum_
                    .generic_parameters
                    .is_empty()
                    .then(|| enum_.comptime_selections.first())
                    .flatten(),
                _ => None,
            };
            if let Some(selection) = selection {
                return Some((
                    path.clone(),
                    selection.range.clone(),
                    selection.branches.clone(),
                ));
            }
        }
    }
    None
}

fn replace_member_selection(
    declarations: &mut [Declaration],
    range: &SourceRange,
    selected: Option<ComptimeMemberBranch>,
) -> bool {
    for declaration in declarations {
        match declaration.syntax.as_mut() {
            Some(DeclarationSyntax::Struct(struct_))
            | Some(DeclarationSyntax::ResourceStruct(struct_)) => {
                let Some(index) = struct_
                    .comptime_selections
                    .iter()
                    .position(|selection| selection.range == *range)
                else {
                    continue;
                };
                struct_.comptime_selections.remove(index);
                if let Some(selected) = selected {
                    struct_.fields.extend(selected.fields);
                    struct_.functions.extend(selected.functions);
                    struct_.constants.extend(selected.constants);
                    struct_.fields.sort_by_key(|field| field.range.start());
                    struct_
                        .functions
                        .sort_by_key(|function| function.range.start());
                    struct_
                        .constants
                        .sort_by_key(|constant| constant.range.start());
                }
                return true;
            }
            Some(DeclarationSyntax::Enum(enum_)) => {
                let Some(index) = enum_
                    .comptime_selections
                    .iter()
                    .position(|selection| selection.range == *range)
                else {
                    continue;
                };
                enum_.comptime_selections.remove(index);
                if let Some(selected) = selected {
                    enum_.variants.extend(selected.variants);
                    enum_.functions.extend(selected.functions);
                    enum_.constants.extend(selected.constants);
                    enum_.variants.sort_by_key(|variant| variant.range.start());
                    enum_
                        .functions
                        .sort_by_key(|function| function.range.start());
                    enum_
                        .constants
                        .sort_by_key(|constant| constant.range.start());
                }
                return true;
            }
            _ => {}
        }
    }
    false
}

fn first_nested_statement_selection(
    parsed_sources: &BTreeMap<String, ParsedSource>,
) -> Option<(String, SourceRange, Vec<ComptimeStatementBranch>)> {
    for (path, parsed) in parsed_sources {
        for declaration in &parsed.declarations {
            let bodies = match declaration.syntax.as_ref() {
                Some(DeclarationSyntax::Function(function))
                    if function.generic_parameters.is_empty() =>
                {
                    vec![&function.body]
                }
                Some(DeclarationSyntax::Struct(struct_))
                | Some(DeclarationSyntax::ResourceStruct(struct_)) => struct_
                    .functions
                    .iter()
                    .filter(|member| {
                        struct_.generic_parameters.is_empty()
                            && member.function.generic_parameters.is_empty()
                    })
                    .map(|member| &member.function.body)
                    .collect(),
                Some(DeclarationSyntax::Enum(enum_)) => enum_
                    .functions
                    .iter()
                    .filter(|member| {
                        enum_.generic_parameters.is_empty()
                            && member.function.generic_parameters.is_empty()
                    })
                    .map(|member| &member.function.body)
                    .collect(),
                Some(DeclarationSyntax::Suite(suite)) => {
                    suite.tests.iter().map(|test| &test.body).collect()
                }
                _ => Vec::new(),
            };
            for body in bodies {
                if let Some((range, branches)) = find_statement_selection(body) {
                    return Some((path.clone(), range, branches));
                }
            }
        }
    }
    None
}

fn find_statement_selection(
    statements: &[StatementSyntax],
) -> Option<(SourceRange, Vec<ComptimeStatementBranch>)> {
    for statement in statements {
        match statement {
            StatementSyntax::Comptime { branches, range } => {
                return Some((range.clone(), branches.clone()));
            }
            StatementSyntax::If {
                then_branch,
                else_branch,
                ..
            } => {
                if let Some(selection) = find_statement_selection(then_branch)
                    .or_else(|| find_statement_selection(else_branch))
                {
                    return Some(selection);
                }
            }
            StatementSyntax::For { body, .. } | StatementSyntax::While { body, .. } => {
                if let Some(selection) = find_statement_selection(body) {
                    return Some(selection);
                }
            }
            StatementSyntax::Match { cases, .. } => {
                for case in cases {
                    if let Some(selection) = find_statement_selection(&case.body) {
                        return Some(selection);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn replace_statement_selection(
    declarations: &mut [Declaration],
    range: &SourceRange,
    replacement: Vec<StatementSyntax>,
) -> bool {
    for declaration in declarations {
        let bodies = match declaration.syntax.as_mut() {
            Some(DeclarationSyntax::Function(function)) => vec![&mut function.body],
            Some(DeclarationSyntax::Struct(struct_))
            | Some(DeclarationSyntax::ResourceStruct(struct_)) => struct_
                .functions
                .iter_mut()
                .map(|member| &mut member.function.body)
                .collect(),
            Some(DeclarationSyntax::Enum(enum_)) => enum_
                .functions
                .iter_mut()
                .map(|member| &mut member.function.body)
                .collect(),
            Some(DeclarationSyntax::Suite(suite)) => {
                suite.tests.iter_mut().map(|test| &mut test.body).collect()
            }
            _ => Vec::new(),
        };
        for body in bodies {
            if replace_statement_in_block(body, range, replacement.clone()) {
                return true;
            }
        }
    }
    false
}

fn replace_statement_in_block(
    statements: &mut Vec<StatementSyntax>,
    range: &SourceRange,
    replacement: Vec<StatementSyntax>,
) -> bool {
    let mut index = 0;
    while index < statements.len() {
        if matches!(&statements[index], StatementSyntax::Comptime { range: site, .. } if site == range)
        {
            statements.splice(index..=index, replacement);
            return true;
        }
        let found = match &mut statements[index] {
            StatementSyntax::If {
                then_branch,
                else_branch,
                ..
            } => {
                replace_statement_in_block(then_branch, range, replacement.clone())
                    || replace_statement_in_block(else_branch, range, replacement.clone())
            }
            StatementSyntax::For { body, .. } | StatementSyntax::While { body, .. } => {
                replace_statement_in_block(body, range, replacement.clone())
            }
            StatementSyntax::Match { cases, .. } => cases
                .iter_mut()
                .any(|case| replace_statement_in_block(&mut case.body, range, replacement.clone())),
            _ => false,
        };
        if found {
            return true;
        }
        index += 1;
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn select_nested_statement_branch<'a>(
    parsed_sources: &BTreeMap<String, ParsedSource>,
    files: &BTreeMap<&'a str, &'a ProjectFile>,
    authenticated_paths: &BTreeSet<&str>,
    root: Root,
    cancellation: &Cancellation,
    build_authority: &BuildAuthority,
    pool_authority: &PoolAuthority,
    path: &str,
    branches: &[ComptimeStatementBranch],
) -> Result<Option<usize>, SelectionFailure> {
    for (index, branch) in branches.iter().enumerate() {
        let matches = if let Some(condition) = &branch.condition {
            evaluate_nested_condition(
                parsed_sources,
                files,
                authenticated_paths,
                root,
                cancellation,
                build_authority,
                pool_authority,
                path,
                condition,
            )?
        } else {
            true
        };
        if matches {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

#[allow(clippy::too_many_arguments)]
fn evaluate_nested_condition<'a>(
    parsed_sources: &BTreeMap<String, ParsedSource>,
    files: &BTreeMap<&'a str, &'a ProjectFile>,
    authenticated_paths: &BTreeSet<&str>,
    root: Root,
    cancellation: &Cancellation,
    build_authority: &BuildAuthority,
    pool_authority: &PoolAuthority,
    path: &str,
    condition: &ExpressionSyntax,
) -> Result<bool, SelectionFailure> {
    let mut probe_sources = parsed_sources.clone();
    for parsed in probe_sources.values_mut() {
        erase_nested_statement_selections(&mut parsed.declarations);
    }
    let mut identities =
        match crate::identity::catalog(&probe_sources, files, authenticated_paths, cancellation) {
            Ok(identities) => identities,
            Err(IdentityFailure::Cancelled) => return Err(SelectionFailure::Cancelled),
            Err(IdentityFailure::Collision(collision)) => {
                return Err(SelectionFailure::Defect(Defect::new(
                    "nested compile-time selection identity catalog",
                    Arc::from(format!(
                        "identity collision {:032x} while selecting {}",
                        collision.digest,
                        condition.range.path()
                    )),
                )));
            }
        };
    let analysis = analyze_with_probe(
        &probe_sources,
        &mut identities,
        root,
        cancellation,
        false,
        AuthorityContext::new(build_authority, pool_authority),
        Some((path, condition)),
    );
    if analysis.cancelled {
        return Err(SelectionFailure::Cancelled);
    }
    if let Some(defect) = analysis.defect {
        return Err(SelectionFailure::Defect(defect));
    }
    if let Some(diagnostic) = analysis.diagnostics.into_iter().next() {
        return Err(SelectionFailure::Diagnostic(diagnostic));
    }
    analysis.selection_value.ok_or_else(|| {
        SelectionFailure::Diagnostic(Diagnostic::new(
            "semantic.invalid_comptime_expression",
            condition.range.clone(),
            RecoveryAction::None,
        ))
    })
}

fn erase_nested_statement_selections(declarations: &mut [Declaration]) {
    for declaration in declarations {
        let bodies = match declaration.syntax.as_mut() {
            Some(DeclarationSyntax::Function(function)) => vec![&mut function.body],
            Some(DeclarationSyntax::Struct(struct_))
            | Some(DeclarationSyntax::ResourceStruct(struct_)) => struct_
                .functions
                .iter_mut()
                .map(|member| &mut member.function.body)
                .collect(),
            Some(DeclarationSyntax::Enum(enum_)) => enum_
                .functions
                .iter_mut()
                .map(|member| &mut member.function.body)
                .collect(),
            Some(DeclarationSyntax::Suite(suite)) => {
                suite.tests.iter_mut().map(|test| &mut test.body).collect()
            }
            _ => Vec::new(),
        };
        for body in bodies {
            erase_statement_block(body);
        }
    }
}

fn erase_statement_block(statements: &mut Vec<StatementSyntax>) {
    for statement in statements {
        match statement {
            StatementSyntax::Comptime { range, .. } => {
                *statement = StatementSyntax::Panic {
                    value: ExpressionSyntax {
                        kind: ExpressionSyntaxKind::Text(
                            "compile-time selection placeholder".to_owned(),
                        ),
                        range: range.clone(),
                    },
                    range: range.clone(),
                };
            }
            StatementSyntax::If {
                then_branch,
                else_branch,
                ..
            } => {
                erase_statement_block(then_branch);
                erase_statement_block(else_branch);
            }
            StatementSyntax::For { body, .. } | StatementSyntax::While { body, .. } => {
                erase_statement_block(body);
            }
            StatementSyntax::Match { cases, .. } => {
                for case in cases {
                    erase_statement_block(&mut case.body);
                }
            }
            _ => {}
        }
    }
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
    pool: Option<crate::model::PoolId>,
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

pub(crate) fn analyze<'a>(
    parsed_sources: &BTreeMap<String, ParsedSource>,
    _files: &BTreeMap<&'a str, &'a ProjectFile>,
    identity_catalog: &mut IdentityCatalog,
    root: Root,
    cancellation: &Cancellation,
    executable_allowed: bool,
    authorities: AuthorityContext<'_>,
) -> SemanticRevision {
    analyze_with_probe(
        parsed_sources,
        identity_catalog,
        root,
        cancellation,
        executable_allowed,
        authorities,
        None,
    )
}

fn analyze_with_probe(
    parsed_sources: &BTreeMap<String, ParsedSource>,
    identity_catalog: &mut IdentityCatalog,
    root: Root,
    cancellation: &Cancellation,
    executable_allowed: bool,
    authorities: AuthorityContext<'_>,
    selection_probe: Option<(&str, &crate::syntax::ExpressionSyntax)>,
) -> SemanticRevision {
    let build_authority = authorities.build();
    let pool_authority = authorities.pool();
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
                    pool: identity_catalog.pool_for_definition(id),
                    declaration: declaration.clone(),
                },
            );
        }
    }

    let mut namespace = NamespaceCatalog::default();
    for definition in definitions.values() {
        let name = if definition.kind == DeclarationKind::Pool {
            identity_catalog
                .pool_for_definition(definition.id)
                .map(ResolvedName::Pool)
        } else {
            resolved_name(definition)
        };
        if let Some(name) = name {
            namespace.declare(
                definition.module,
                Arc::from([definition.name.clone()]),
                name,
                definition.public,
            );
        }
        if is_nominal(definition.kind) {
            let arity = match definition.declaration.syntax.as_ref() {
                Some(DeclarationSyntax::Struct(struct_))
                | Some(DeclarationSyntax::ResourceStruct(struct_)) => struct_.type_parameters.len(),
                Some(DeclarationSyntax::Enum(enum_)) => enum_.type_parameters.len(),
                _ => 0,
            };
            namespace.set_nominal_arity(definition.id, arity);
        }
    }
    for definition in definitions.values() {
        let (functions, constants): (&[_], &[_]) = match definition.declaration.syntax.as_ref() {
            Some(
                DeclarationSyntax::Struct(struct_) | DeclarationSyntax::ResourceStruct(struct_),
            ) => (&struct_.functions, &struct_.constants),
            Some(DeclarationSyntax::Enum(enum_)) => (&enum_.functions, &enum_.constants),
            Some(DeclarationSyntax::Interface(interface)) => (&[], &interface.constants),
            _ => (&[], &[]),
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
        for member in constants {
            let id = identity_catalog
                .associated_constant(definition.id, &member.name)
                .expect("structured associated constant was catalogued");
            namespace.declare(
                definition.module,
                Arc::from([definition.name.clone(), member.name.clone()]),
                ResolvedName::Constant(id),
                definition.public && member.public,
            );
            namespace.declare_member(
                definition.id,
                definition.module,
                member.name.clone(),
                ResolvedName::Constant(id),
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
        aliases: alias_types.clone(),
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
    let mut interface_constants = BTreeMap::<DefinitionId, Vec<(String, Type, SourceRange)>>::new();
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
            let requirement_id = identity_catalog
                .nested_definition(definition.id, "interface_requirement", &requirement.name)
                .expect("interface requirement identity exists");
            let Some(parameters) = resolve_parameters(
                &requirement.parameters,
                requirement_id,
                definition.module,
                &input.namespace,
                &input.nominal_displays,
                &alias_types,
                &self_parameters,
                identity_catalog,
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
            if definition.public
                && (exposes_private_type(&return_type, &definitions)
                    || parameters
                        .iter()
                        .any(|parameter| exposes_private_type(&parameter.type_, &definitions)))
            {
                diagnostics.push(Diagnostic::new(
                    "semantic.private_type_in_public_signature",
                    requirement.range.clone(),
                    RecoveryAction::None,
                ));
            }
            if !result_type_well_formed(&return_type, false, &definitions)
                || parameters.iter().any(|parameter| {
                    !result_type_well_formed(&parameter.type_, false, &definitions)
                })
            {
                diagnostics.push(Diagnostic::new(
                    "semantic.invalid_result_error_type",
                    requirement.range.clone(),
                    RecoveryAction::None,
                ));
            }
            requirements.push(InterfaceRequirement {
                name: requirement.name.clone(),
                modifier: requirement.modifier,
                parameters,
                return_type,
                range: requirement.range.clone(),
            });
        }
        interfaces.insert(definition.id, requirements);
        let constants = interface
            .constants
            .iter()
            .filter_map(|constant| {
                let type_ = resolve_type(
                    &constant.type_syntax,
                    definition.module,
                    &input.namespace,
                    &input.nominal_displays,
                    &alias_types,
                    &BTreeMap::new(),
                );
                if type_.is_none() {
                    diagnostics.push(Diagnostic::new(
                        "semantic.unresolved_type",
                        constant.range.clone(),
                        RecoveryAction::None,
                    ));
                }
                type_.map(|type_| (constant.name.clone(), type_, constant.range.clone()))
            })
            .collect();
        interface_constants.insert(definition.id, constants);
    }
    input.interfaces = interfaces
        .iter()
        .map(|(id, requirements)| {
            (
                *id,
                ResolvedInterface {
                    requirements: requirements
                        .iter()
                        .map(|requirement| {
                            (
                                requirement.name.clone(),
                                ResolvedInterfaceRequirement {
                                    parameters: requirement.parameters.clone(),
                                    return_type: requirement.return_type.clone(),
                                },
                            )
                        })
                        .collect(),
                    implementations: BTreeMap::new(),
                },
            )
        })
        .collect();
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
                    definition.id,
                    definition.module,
                    &input.namespace,
                    &input.nominal_displays,
                    &alias_types,
                    &type_parameters,
                    identity_catalog,
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
                if return_type != Type::Unit
                    && !crate::control_flow::syntax_statements_terminate(&function.body)
                {
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
                if !result_type_well_formed(&return_type, !definition.public, &definitions)
                    || parameters.iter().any(|parameter| {
                        !result_type_well_formed(&parameter.type_, false, &definitions)
                    })
                {
                    diagnostics.push(Diagnostic::new(
                        "semantic.invalid_result_error_type",
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
                        generic_parameter_names: function.type_parameters.clone().into(),
                        generic_constraints: resolve_generic_constraints(
                            &function.generic_parameters,
                            definition.module,
                            &input.namespace,
                            &input.nominal_displays,
                            &alias_types,
                            &type_parameters,
                            &definitions,
                            &definition.declaration.range,
                            &mut diagnostics,
                        ),
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
                if definition.public && exposes_private_type(&type_, &definitions) {
                    diagnostics.push(Diagnostic::new(
                        "semantic.private_type_in_public_signature",
                        definition.declaration.range.clone(),
                        RecoveryAction::None,
                    ));
                }
                if !result_type_well_formed(&type_, false, &definitions) {
                    diagnostics.push(Diagnostic::new(
                        "semantic.invalid_result_error_type",
                        definition.declaration.range.clone(),
                        RecoveryAction::None,
                    ));
                }
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
                        id.test,
                        definition.module,
                        &input.namespace,
                        &input.nominal_displays,
                        &alias_types,
                        &BTreeMap::new(),
                        identity_catalog,
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
                let type_parameters = struct_
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
                let mut field_names = BTreeSet::new();
                for constant in &struct_.constants {
                    if !field_names.insert(&constant.name) {
                        diagnostics.push(
                            Diagnostic::new(
                                "semantic.duplicate_member",
                                constant.range.clone(),
                                RecoveryAction::None,
                            )
                            .with_parameter("name", constant.name.clone()),
                        );
                        continue;
                    }
                    let Some(type_) = resolve_type(
                        &constant.type_syntax,
                        definition.module,
                        &input.namespace,
                        &input.nominal_displays,
                        &alias_types,
                        &type_parameters,
                    ) else {
                        diagnostics.push(Diagnostic::new(
                            "semantic.unresolved_type",
                            constant.range.clone(),
                            RecoveryAction::None,
                        ));
                        continue;
                    };
                    if definition.public
                        && constant.public
                        && exposes_private_type(&type_, &definitions)
                    {
                        diagnostics.push(Diagnostic::new(
                            "semantic.private_type_in_public_signature",
                            constant.range.clone(),
                            RecoveryAction::None,
                        ));
                    }
                    let Some(value) = constant.value.clone() else {
                        return defect(
                            "associated constant resolution",
                            Arc::from("concrete associated constant has no value"),
                        );
                    };
                    let id = identity_catalog
                        .associated_constant(definition.id, &constant.name)
                        .expect("associated constant identity exists");
                    input.constants.insert(
                        id,
                        ResolvedConstant {
                            id,
                            module: definition.module,
                            name: format!("{}.{}", definition.name, constant.name),
                            type_,
                            value,
                            source: constant.range.clone(),
                        },
                    );
                }
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
                        &type_parameters,
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
                    if !result_type_well_formed(&type_, false, &definitions) {
                        diagnostics.push(Diagnostic::new(
                            "semantic.invalid_result_error_type",
                            field.range.clone(),
                            RecoveryAction::None,
                        ));
                    }
                    resolved_fields.push(ResolvedField {
                        definition: identity_catalog
                            .nested_definition(definition.id, "field", &field.name)
                            .expect("field identity exists"),
                        name: field.name.clone(),
                        public: field.public,
                        mutable: field.mutable,
                        type_,
                    });
                }
                let field_selections = struct_
                    .comptime_selections
                    .iter()
                    .map(|selection| ResolvedFieldSelection {
                        branches: selection
                            .branches
                            .iter()
                            .map(|branch| {
                                let mut branch_names = field_names.clone();
                                let fields = branch
                                    .fields
                                    .iter()
                                    .filter_map(|field| {
                                        if !branch_names.insert(&field.name) {
                                            diagnostics.push(
                                                Diagnostic::new(
                                                    "semantic.duplicate_field",
                                                    field.range.clone(),
                                                    RecoveryAction::None,
                                                )
                                                .with_parameter("name", field.name.clone()),
                                            );
                                            return None;
                                        }
                                        let type_ = resolve_type(
                                            &field.type_syntax,
                                            definition.module,
                                            &input.namespace,
                                            &input.nominal_displays,
                                            &alias_types,
                                            &type_parameters,
                                        )?;
                                        Some(ResolvedField {
                                            definition: identity_catalog
                                                .nested_definition(
                                                    definition.id,
                                                    "field",
                                                    &field.name,
                                                )
                                                .expect("conditional field identity exists"),
                                            name: field.name.clone(),
                                            public: field.public,
                                            mutable: field.mutable,
                                            type_,
                                        })
                                    })
                                    .collect::<Vec<_>>();
                                ResolvedFieldBranch {
                                    condition: branch.condition.clone(),
                                    fields: fields.into(),
                                }
                            })
                            .collect::<Vec<_>>()
                            .into(),
                    })
                    .collect::<Vec<_>>();
                input.structs.insert(
                    definition.id,
                    ResolvedStruct {
                        definition: definition.id,
                        module: definition.module,
                        display: Arc::from(definition.name.as_str()),
                        resource,
                        type_parameters: (0..struct_.type_parameters.len())
                            .map(|index| TypeParameterId(u16::try_from(index).unwrap_or(u16::MAX)))
                            .collect(),
                        generic_parameter_names: struct_.type_parameters.clone().into(),
                        generic_constraints: resolve_generic_constraints(
                            &struct_.generic_parameters,
                            definition.module,
                            &input.namespace,
                            &input.nominal_displays,
                            &alias_types,
                            &type_parameters,
                            &definitions,
                            &definition.declaration.range,
                            &mut diagnostics,
                        ),
                        fields: resolved_fields,
                        field_selections: field_selections.into(),
                        applied_fields: RefCell::new(BTreeMap::new()),
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
                        } else if let Some(interface) = input.interfaces.get_mut(&interface_id) {
                            interface
                                .implementations
                                .entry(definition.id)
                                .or_default()
                                .insert(requirement.name.clone(), implementation.id);
                        }
                    }
                    for (name, expected, requirement_range) in
                        interface_constants.get(&interface_id).into_iter().flatten()
                    {
                        let implementation =
                            struct_.constants.iter().find(|value| value.name == *name);
                        let actual = implementation.and_then(|constant| {
                            resolve_type(
                                &constant.type_syntax,
                                definition.module,
                                &input.namespace,
                                &input.nominal_displays,
                                &alias_types,
                                &type_parameters,
                            )
                        });
                        if actual
                            .as_ref()
                            .is_none_or(|actual| !can_unify(actual, expected))
                        {
                            diagnostics.push(
                                Diagnostic::new(
                                    "semantic.interface_constant_mismatch",
                                    implementation.map_or_else(
                                        || definition.declaration.range.clone(),
                                        |constant| constant.range.clone(),
                                    ),
                                    RecoveryAction::None,
                                )
                                .with_label(requirement_range.clone(), DiagnosticLabelRole::Related)
                                .with_parameter("member", name.clone()),
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
                for constant in &enum_.constants {
                    let Some(type_) = resolve_type(
                        &constant.type_syntax,
                        definition.module,
                        &input.namespace,
                        &input.nominal_displays,
                        &alias_types,
                        &type_parameters,
                    ) else {
                        diagnostics.push(Diagnostic::new(
                            "semantic.unresolved_type",
                            constant.range.clone(),
                            RecoveryAction::None,
                        ));
                        continue;
                    };
                    if definition.public
                        && constant.public
                        && exposes_private_type(&type_, &definitions)
                    {
                        diagnostics.push(Diagnostic::new(
                            "semantic.private_type_in_public_signature",
                            constant.range.clone(),
                            RecoveryAction::None,
                        ));
                    }
                    let Some(value) = constant.value.clone() else {
                        return defect(
                            "associated constant resolution",
                            Arc::from("concrete associated constant has no value"),
                        );
                    };
                    let id = identity_catalog
                        .associated_constant(definition.id, &constant.name)
                        .expect("associated constant identity exists");
                    input.constants.insert(
                        id,
                        ResolvedConstant {
                            id,
                            module: definition.module,
                            name: format!("{}.{}", definition.name, constant.name),
                            type_,
                            value,
                            source: constant.range.clone(),
                        },
                    );
                }
                for (variant_order, variant) in enum_.variants.iter().enumerate() {
                    let variant_id = identity_catalog
                        .variant(definition.id, &variant.name)
                        .expect("structured enum variant was catalogued");
                    let Some(parameters) = resolve_parameters(
                        &variant.parameters,
                        variant_id.definition,
                        definition.module,
                        &input.namespace,
                        &input.nominal_displays,
                        &alias_types,
                        &type_parameters,
                        identity_catalog,
                        &mut diagnostics,
                    ) else {
                        continue;
                    };
                    if definition.public
                        && parameters
                            .iter()
                            .any(|parameter| exposes_private_type(&parameter.type_, &definitions))
                    {
                        diagnostics.push(Diagnostic::new(
                            "semantic.private_type_in_public_signature",
                            variant.range.clone(),
                            RecoveryAction::None,
                        ));
                    }
                    if parameters.iter().any(|parameter| {
                        !result_type_well_formed(&parameter.type_, false, &definitions)
                    }) {
                        diagnostics.push(Diagnostic::new(
                            "semantic.invalid_result_error_type",
                            variant.range.clone(),
                            RecoveryAction::None,
                        ));
                    }
                    if parameters
                        .iter()
                        .any(|parameter| is_resource_type(&parameter.type_, &definitions))
                    {
                        diagnostics.push(Diagnostic::new(
                            "semantic.resource_payload_requires_resource_type",
                            variant.range.clone(),
                            RecoveryAction::None,
                        ));
                    }
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
                    let id = variant_id;
                    input.variants.insert(
                        id,
                        ResolvedVariant {
                            order: u32::try_from(variant_order).unwrap_or(u32::MAX),
                            type_parameters: (0..enum_.type_parameters.len())
                                .map(|index| {
                                    TypeParameterId(u16::try_from(index).unwrap_or(u16::MAX))
                                })
                                .collect(),
                            generic_constraints: resolve_generic_constraints(
                                &enum_.generic_parameters,
                                definition.module,
                                &input.namespace,
                                &input.nominal_displays,
                                &alias_types,
                                &type_parameters,
                                &definitions,
                                &definition.declaration.range,
                                &mut diagnostics,
                            ),
                            parameters,
                        },
                    );
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

    for (id, type_) in &alias_types {
        if definitions[id].public && exposes_private_type(type_, &definitions) {
            diagnostics.push(Diagnostic::new(
                "semantic.private_type_in_public_signature",
                definitions[id].declaration.range.clone(),
                RecoveryAction::None,
            ));
        }
        if !result_type_well_formed(type_, false, &definitions) {
            diagnostics.push(Diagnostic::new(
                "semantic.invalid_result_error_type",
                definitions[id].declaration.range.clone(),
                RecoveryAction::None,
            ));
        }
    }

    let needs_error_inference = input
        .functions
        .values()
        .any(|function| matches!(function.return_type, Type::Result { error: None, .. }));
    let inference_functions = if needs_error_inference {
        match typed_hir::lower_functions_for_error_inference(
            &input,
            build_authority,
            pool_authority,
            identity_catalog,
            cancellation,
        ) {
            Ok(functions) => Some(functions),
            Err(typed_hir::VerificationFailure::Defect { evidence }) => {
                return defect("error-signature inference", evidence);
            }
            Err(
                typed_hir::VerificationFailure::Creator { .. }
                | typed_hir::VerificationFailure::Custody { .. },
            ) => None,
            Err(typed_hir::VerificationFailure::Cancelled) => return cancelled(),
        }
    } else {
        Some(BTreeMap::new())
    };
    let inferred_signature_types = inference_functions
        .as_ref()
        .map_or_else(BTreeMap::new, |functions| {
            crate::semantic_facts::infer_error_signatures(functions, cancellation)
        });
    if cancellation.is_cancelled() {
        return cancelled();
    }
    for (definition, error) in &inferred_signature_types {
        let Some(function) = input.functions.get_mut(definition) else {
            return defect(
                "error-signature inference",
                Arc::from("inferred function is absent from the semantic catalog"),
            );
        };
        if !matches!(error, Type::Nominal { .. }) {
            diagnostics.push(Diagnostic::new(
                "semantic.invalid_result_error_type",
                function.source.clone(),
                RecoveryAction::None,
            ));
            continue;
        }
        let Type::Result {
            success,
            error: inferred_error,
        } = &mut function.return_type
        else {
            return defect(
                "error-signature inference",
                Arc::from("inferred function no longer has a Result signature"),
            );
        };
        debug_assert!(inferred_error.is_none());
        let _ = success;
        *inferred_error = Some(Arc::new(error.clone()));
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

    for function in input.functions.values() {
        if invalid_resource_argument_in_data_nominal(&function.return_type, &definitions)
            || function.parameters.iter().any(|parameter| {
                invalid_resource_argument_in_data_nominal(&parameter.type_, &definitions)
            })
        {
            diagnostics.push(Diagnostic::new(
                "semantic.resource_argument_requires_resource_struct",
                function.source.clone(),
                RecoveryAction::None,
            ));
        }
    }
    for constant in input.constants.values() {
        if invalid_resource_argument_in_data_nominal(&constant.type_, &definitions) {
            diagnostics.push(Diagnostic::new(
                "semantic.resource_argument_requires_resource_struct",
                constant.source.clone(),
                RecoveryAction::None,
            ));
        }
    }
    for test in input.tests.values() {
        if test.parameters.iter().any(|parameter| {
            invalid_resource_argument_in_data_nominal(&parameter.type_, &definitions)
        }) {
            diagnostics.push(Diagnostic::new(
                "semantic.resource_argument_requires_resource_struct",
                test.source.clone(),
                RecoveryAction::None,
            ));
        }
    }
    for struct_ in input.structs.values() {
        if struct_
            .fields
            .iter()
            .any(|field| invalid_resource_argument_in_data_nominal(&field.type_, &definitions))
        {
            diagnostics.push(Diagnostic::new(
                "semantic.resource_argument_requires_resource_struct",
                definitions[&struct_.definition].declaration.range.clone(),
                RecoveryAction::None,
            ));
        }
    }
    for (variant, resolved) in &input.variants {
        if resolved.parameters.iter().any(|parameter| {
            invalid_resource_argument_in_data_nominal(&parameter.type_, &definitions)
        }) {
            diagnostics.push(Diagnostic::new(
                "semantic.resource_argument_requires_resource_struct",
                definitions[&variant.owner].declaration.range.clone(),
                RecoveryAction::None,
            ));
        }
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
    if let Some((path, condition)) = selection_probe {
        if !diagnostics.is_empty() {
            return selection_analysis(diagnostics, None);
        }
        let module = modules[path];
        let (program, expression) = match typed_hir::verify_comptime_condition(
            &input,
            module,
            condition,
            build_authority,
            pool_authority,
            identity_catalog,
            cancellation,
        ) {
            Ok(probe) => probe,
            Err(typed_hir::VerificationFailure::Cancelled) => return cancelled(),
            Err(typed_hir::VerificationFailure::Defect { evidence }) => {
                return defect("compile-time selection verification", evidence);
            }
            Err(
                typed_hir::VerificationFailure::Creator { .. }
                | typed_hir::VerificationFailure::Custody { .. },
            ) => {
                return selection_analysis(
                    vec![Diagnostic::new(
                        "semantic.invalid_comptime_expression",
                        condition.range.clone(),
                        RecoveryAction::None,
                    )],
                    None,
                );
            }
        };
        let run = Engine::new(&program, cancellation).evaluate_expression(&expression);
        return match run.outcome {
            EvaluationOutcome::Completed(crate::CanonicalValue::Bool(value)) => {
                selection_analysis(Vec::new(), Some(value))
            }
            EvaluationOutcome::Cancelled => cancelled(),
            EvaluationOutcome::Defect { evidence } => {
                defect("compile-time selection evaluation", evidence)
            }
            outcome => {
                let mut diagnostics = Vec::new();
                if let Err(evidence) =
                    map_evaluation_failure(&outcome, &condition.range, &mut diagnostics)
                {
                    defect("compile-time selection evaluation", evidence)
                } else {
                    if diagnostics.is_empty() {
                        diagnostics.push(Diagnostic::new(
                            "semantic.invalid_comptime_expression",
                            condition.range.clone(),
                            RecoveryAction::None,
                        ));
                    }
                    selection_analysis(diagnostics, None)
                }
            }
        };
    }
    let program = match typed_hir::verify(
        input,
        build_authority,
        pool_authority,
        identity_catalog,
        cancellation,
    ) {
        Ok(program) => Some(program),
        Err(typed_hir::VerificationFailure::Defect { evidence }) => {
            return defect("typed HIR verification", evidence);
        }
        Err(typed_hir::VerificationFailure::Creator { kind, site }) => {
            let is_comptime_expression = parsed_sources.values().any(|parsed| {
                parsed
                    .comptime_assertions
                    .iter()
                    .any(|assertion| assertion.range == site)
            });
            if is_comptime_expression {
                diagnostics.push(Diagnostic::new(
                    "semantic.invalid_comptime_expression",
                    site,
                    RecoveryAction::None,
                ));
            } else {
                diagnostics.push(Diagnostic::new(
                    kind.diagnostic_code(),
                    site,
                    RecoveryAction::None,
                ));
            }
            None
        }
        Err(typed_hir::VerificationFailure::Custody {
            kind,
            site,
            subject,
            state,
            identities,
            related,
        }) => {
            let state = match state {
                typed_hir::CustodyDiagnosticState::Moved => "moved",
                typed_hir::CustodyDiagnosticState::Initialized => "initialized",
                typed_hir::CustodyDiagnosticState::Loaned => "loaned",
                typed_hir::CustodyDiagnosticState::ConflictingLoan => "conflicting_loan",
                typed_hir::CustodyDiagnosticState::PathDependent => "path_dependent",
                typed_hir::CustodyDiagnosticState::LiveUndischarged => "live_undischarged",
            };
            let mut diagnostic =
                Diagnostic::new(kind.diagnostic_code(), site, RecoveryAction::None)
                    .with_parameter("subject", subject)
                    .with_parameter("state", state);
            if let Some(identity) = identities.subject {
                diagnostic = diagnostic.with_identity_parameter(
                    "subject_identity",
                    IdentityDomain::Definition,
                    identity.0,
                );
            }
            if let Some(identity) = identities.owner {
                diagnostic = diagnostic.with_identity_parameter(
                    "owner_identity",
                    IdentityDomain::Definition,
                    identity.0,
                );
            }
            if let Some(related) = related {
                diagnostic = diagnostic.with_label(related, DiagnosticLabelRole::Related);
            }
            diagnostics.push(diagnostic);
            None
        }
        Err(typed_hir::VerificationFailure::Cancelled) => return cancelled(),
    };

    let mut resolutions = Vec::new();
    let mut function_facts = Vec::new();
    let mut specialization_observations = Vec::new();
    let mut inferred_errors = Vec::new();
    let mut evaluations = Vec::new();
    let mut constructions = Vec::new();
    let mut test_plan = Vec::new();
    if let Some(program) = &program {
        resolutions = resolution_observations(program);
        let solved_facts = crate::semantic_facts::solve(program, cancellation);
        let facts = solved_facts.definitions;
        let concrete_facts = solved_facts.specializations;
        let recursion = solved_facts.recursion;
        inferred_errors = solved_facts.inferred_errors;
        diagnostics.extend(solved_facts.diagnostics);
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
        let image_reachable = image_functions
            .first()
            .map_or_else(BTreeSet::new, |(root, _)| {
                crate::graph::reachable_from(*root, &call_graph)
            });
        if executable_allowed {
            for constant in program.constants().values() {
                if crate::semantic_facts::expression_constructs(&constant.expression) {
                    diagnostics.push(Diagnostic::new(
                        "semantic.build_constructor_outside_image",
                        constant.source.clone(),
                        RecoveryAction::None,
                    ));
                }
            }
            for (id, facts) in &facts {
                if !facts.constructs.is_empty() && !image_reachable.contains(id) {
                    diagnostics.push(Diagnostic::new(
                        "semantic.build_constructor_outside_image",
                        program.functions()[id].source.clone(),
                        RecoveryAction::None,
                    ));
                }
            }
        }
        for range in recursion.unproven {
            diagnostics.push(Diagnostic::new(
                "semantic.unproven_recursive_bound",
                range,
                RecoveryAction::None,
            ));
        }
        for specialization in program.specializations().values() {
            if !inferred_signature_types.contains_key(&specialization.definition) {
                continue;
            }
            let function = &program.specialized_functions()[&specialization.id];
            let Type::Result {
                error: Some(error), ..
            } = &function.return_type
            else {
                return defect(
                    "error-signature inference",
                    Arc::from("inferred specialization does not have a concrete Result error"),
                );
            };
            inferred_errors.push(InferredErrorObservation::new(
                specialization.id.0,
                function.name.clone(),
                error.display(),
            ));
        }
        inferred_errors.sort_by_key(InferredErrorObservation::specialization_identity);
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
                evaluations.push(EvaluationObservation::new(
                    format!("{}.{}", module_displays[&constant.module], constant.name),
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
                let finished = image_evaluation::finish(engine.evaluate_function(*image));
                if finished.outcome == EvaluationOutcome::Cancelled {
                    return cancelled();
                }
                if let Err(evidence) =
                    map_evaluation_failure(&finished.outcome, image_range, &mut diagnostics)
                {
                    return defect("Image Constructor evaluation", evidence);
                }
                match finished.status {
                    ImageEvaluationStatus::NotCompleted => {}
                    ImageEvaluationStatus::Sealed(sealed) => {
                        if root == Root::Test {
                            test_plan = plan_tests(
                                program,
                                &tests_in_source_order,
                                &sealed.test_applications,
                                image_range,
                                &mut diagnostics,
                            );
                        }
                        constructions.extend(sealed.constructions);
                    }
                    ImageEvaluationStatus::Invalid(GraphSealFailure::Creator(kind)) => diagnostics
                        .push(
                            Diagnostic::new(
                                "construction.invalid_graph",
                                image_range.clone(),
                                RecoveryAction::None,
                            )
                            .with_parameter("kind", kind),
                        ),
                    ImageEvaluationStatus::Invalid(GraphSealFailure::Defect(evidence)) => {
                        return defect("construction graph verification", Arc::from(evidence));
                    }
                }
                let function = &program.functions()[image];
                evaluations.push(EvaluationObservation::new(
                    format!("{}.{}", function.module_display, function.name),
                    finished.outcome,
                    finished.receipt,
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
    SemanticRevision {
        diagnostics,
        observations: SemanticObservations {
            resolutions,
            function_facts,
            types: type_observations,
            ownership: ownership_observations,
            specializations: specialization_observations,
            inferred_errors,
            evaluations,
            constructions,
            test_plan,
        },
        defect: None,
        cancelled: false,
        selection_value: None,
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
    let enclosing_parameter_names: &[String] = match definition.declaration.syntax.as_ref() {
        Some(DeclarationSyntax::Struct(struct_))
        | Some(DeclarationSyntax::ResourceStruct(struct_)) => &struct_.type_parameters,
        Some(DeclarationSyntax::Enum(enum_)) => &enum_.type_parameters,
        _ => &[],
    };
    let enclosing_generic_parameters: &[crate::syntax::GenericParameterSyntax] =
        match definition.declaration.syntax.as_ref() {
            Some(DeclarationSyntax::Struct(struct_))
            | Some(DeclarationSyntax::ResourceStruct(struct_)) => &struct_.generic_parameters,
            Some(DeclarationSyntax::Enum(enum_)) => &enum_.generic_parameters,
            _ => &[],
        };
    let enclosing_parameters = enclosing_parameter_names
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
    let self_type = Type::Nominal {
        definition: definition.id,
        display: displays[&definition.id].clone(),
        arguments: enclosing_parameter_names
            .iter()
            .map(|name| enclosing_parameters[name].clone())
            .collect(),
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
        let mut type_parameters = enclosing_parameters.clone();
        type_parameters.insert("Self".to_owned(), self_type.clone());
        for (index, name) in member.function.type_parameters.iter().enumerate() {
            let parameter_index = enclosing_parameter_names.len().saturating_add(index);
            if type_parameters
                .insert(
                    name.clone(),
                    Type::Parameter {
                        owner: id,
                        id: TypeParameterId(u16::try_from(parameter_index).unwrap_or(u16::MAX)),
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
            id,
            definition.module,
            namespace,
            displays,
            aliases,
            &type_parameters,
            identity_catalog,
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
        if return_type != Type::Unit
            && !crate::control_flow::syntax_statements_terminate(&member.function.body)
        {
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
        if definition.public
            && member.public
            && matches!(return_type, Type::Result { error: None, .. })
        {
            diagnostics.push(Diagnostic::new(
                "semantic.public_result_requires_error_type",
                member.range.clone(),
                RecoveryAction::None,
            ));
        }
        if !result_type_well_formed(
            &return_type,
            !(definition.public && member.public),
            definitions,
        ) || parameters
            .iter()
            .any(|parameter| !result_type_well_formed(&parameter.type_, false, definitions))
        {
            diagnostics.push(Diagnostic::new(
                "semantic.invalid_result_error_type",
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
            type_parameters: (0..enclosing_parameter_names
                .len()
                .saturating_add(member.function.type_parameters.len()))
                .map(|index| TypeParameterId(u16::try_from(index).unwrap_or(u16::MAX)))
                .collect(),
            generic_parameter_names: enclosing_parameter_names
                .iter()
                .cloned()
                .chain(member.function.type_parameters.iter().cloned())
                .collect(),
            generic_constraints: {
                let mut constraints = enclosing_generic_parameters.to_vec();
                constraints.extend(member.function.generic_parameters.clone());
                resolve_generic_constraints(
                    &constraints,
                    definition.module,
                    namespace,
                    displays,
                    aliases,
                    &type_parameters,
                    definitions,
                    &member.range,
                    diagnostics,
                )
            },
            parameters,
            return_type,
            body: member.function.body.clone(),
            source: member.range.clone(),
        });
    }
    resolved
}

#[allow(clippy::too_many_arguments)]
fn resolve_parameters(
    parameters: &[ParameterSyntax],
    owner: DefinitionId,
    module: ModuleId,
    namespace: &NamespaceCatalog,
    displays: &BTreeMap<DefinitionId, Arc<str>>,
    aliases: &BTreeMap<DefinitionId, Type>,
    type_parameters: &BTreeMap<String, Type>,
    identity_catalog: &IdentityCatalog,
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
                definition: identity_catalog
                    .nested_definition(owner, "parameter", &parameter.name)
                    .expect("parameter identity exists"),
                name: parameter.name.clone(),
                ownership: parameter.ownership,
                type_,
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn resolve_generic_constraints(
    parameters: &[crate::syntax::GenericParameterSyntax],
    module: ModuleId,
    namespace: &NamespaceCatalog,
    displays: &BTreeMap<DefinitionId, Arc<str>>,
    aliases: &BTreeMap<DefinitionId, Type>,
    type_parameters: &BTreeMap<String, Type>,
    definitions: &BTreeMap<DefinitionId, DefinitionRecord>,
    range: &SourceRange,
    diagnostics: &mut Vec<Diagnostic>,
) -> Arc<[typed_hir::GenericConstraint]> {
    parameters
        .iter()
        .map(|parameter| match &parameter.kind {
            crate::syntax::GenericParameterKindSyntax::Type { interface_bound } => {
                let interface = interface_bound.as_ref().and_then(|bound| {
                    let Some(ResolvedName::Nominal(id)) =
                        namespace.resolve(module, &bound.segments)
                    else {
                        diagnostics.push(Diagnostic::new(
                            "semantic.unresolved_interface",
                            range.clone(),
                            RecoveryAction::None,
                        ));
                        return None;
                    };
                    if definitions
                        .get(&id)
                        .is_none_or(|definition| definition.kind != DeclarationKind::Interface)
                    {
                        diagnostics.push(Diagnostic::new(
                            "semantic.generic_bound_requires_interface",
                            range.clone(),
                            RecoveryAction::None,
                        ));
                        None
                    } else {
                        Some(id)
                    }
                });
                typed_hir::GenericConstraint::Type { interface }
            }
            crate::syntax::GenericParameterKindSyntax::Const { type_syntax } => {
                let type_ = resolve_type(
                    type_syntax,
                    module,
                    namespace,
                    displays,
                    aliases,
                    type_parameters,
                )
                .unwrap_or_else(|| {
                    diagnostics.push(Diagnostic::new(
                        "semantic.unresolved_type",
                        range.clone(),
                        RecoveryAction::None,
                    ));
                    Type::Infer
                });
                if is_resource_type(&type_, definitions)
                    || !matches!(
                        type_,
                        Type::Bool
                            | Type::Integer(_)
                            | Type::Text
                            | Type::Scalar
                            | Type::Bytes
                            | Type::Nominal { .. }
                            | Type::Tuple(_)
                            | Type::FixedArray { .. }
                    )
                {
                    diagnostics.push(Diagnostic::new(
                        "semantic.const_generic_requires_canonical_data",
                        range.clone(),
                        RecoveryAction::None,
                    ));
                }
                typed_hir::GenericConstraint::Const { type_ }
            }
            crate::syntax::GenericParameterKindSyntax::Pool => typed_hir::GenericConstraint::Pool,
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
        TypeSyntax::ConstU64(value) => Some(Type::ConstU64(*value)),
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
        TypeSyntax::FixedArray { element, length } => Some(Type::FixedArray {
            element: Arc::new(resolve_type(
                element,
                module,
                namespace,
                displays,
                aliases,
                type_parameters,
            )?),
            length: match length {
                crate::syntax::FixedArrayLengthSyntax::Literal(value) => ArrayLength::Value(*value),
                crate::syntax::FixedArrayLengthSyntax::Parameter(name) => {
                    let Type::Parameter { owner, id, display } = type_parameters.get(name)? else {
                        return None;
                    };
                    ArrayLength::Parameter {
                        owner: *owner,
                        id: *id,
                        display: Arc::clone(display),
                    }
                }
            },
        }),
        TypeSyntax::Function {
            parameters,
            return_type,
        } => Some(Type::Function {
            parameters: parameters
                .iter()
                .map(|parameter| {
                    resolve_type(
                        parameter,
                        module,
                        namespace,
                        displays,
                        aliases,
                        type_parameters,
                    )
                })
                .collect::<Option<Vec<_>>>()?
                .into(),
            return_type: Arc::new(resolve_type(
                return_type,
                module,
                namespace,
                displays,
                aliases,
                type_parameters,
            )?),
        }),
        TypeSyntax::Own { pool, value } => {
            let pool = match namespace.resolve(module, &pool.segments) {
                Some(ResolvedName::Pool(pool)) => PoolTerm::Concrete(pool),
                None if pool.segments.len() == 1 => {
                    let Type::Parameter { owner, id, display } =
                        type_parameters.get(&pool.segments[0])?
                    else {
                        return None;
                    };
                    PoolTerm::Parameter {
                        owner: *owner,
                        id: *id,
                        display: Arc::clone(display),
                    }
                }
                _ => return None,
            };
            Some(Type::Own {
                pool,
                value: Arc::new(resolve_type(
                    value,
                    module,
                    namespace,
                    displays,
                    aliases,
                    type_parameters,
                )?),
            })
        }
        TypeSyntax::Any(interface) => {
            let ResolvedName::Nominal(interface) =
                namespace.resolve(module, &interface.segments)?
            else {
                return None;
            };
            Some(Type::Any {
                interface,
                display: displays.get(&interface)?.clone(),
            })
        }
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
                ResolvedName::Pool(pool) => Some(Type::PoolArgument(pool)),
                ResolvedName::Nominal(id) if namespace.nominal_arity(id) == Some(0) => {
                    Some(Type::Nominal {
                        definition: id,
                        display: displays.get(&id)?.clone(),
                        arguments: Arc::from([]),
                    })
                }
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
                _ => match namespace.resolve(module, &base.segments)? {
                    ResolvedName::Nominal(id)
                        if namespace.nominal_arity(id) == Some(values.len()) =>
                    {
                        Some(Type::Nominal {
                            definition: id,
                            display: displays.get(&id)?.clone(),
                            arguments: values.into(),
                        })
                    }
                    _ => None,
                },
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
            TypeSyntax::Unit | TypeSyntax::ConstU64(_) | TypeSyntax::Infer => {}
            TypeSyntax::Array(value) => visit(value, module, namespace, dependencies, missing),
            TypeSyntax::FixedArray { element, .. } => {
                visit(element, module, namespace, dependencies, missing);
            }
            TypeSyntax::Function {
                parameters,
                return_type,
            } => {
                for parameter in parameters {
                    visit(parameter, module, namespace, dependencies, missing);
                }
                visit(return_type, module, namespace, dependencies, missing);
            }
            TypeSyntax::Own { pool, value } => {
                if namespace.resolve(module, &pool.segments).is_none() {
                    *missing = true;
                }
                visit(value, module, namespace, dependencies, missing);
            }
            TypeSyntax::Any(interface) => {
                if namespace.resolve(module, &interface.segments).is_none() {
                    *missing = true;
                }
            }
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
        Type::Nominal {
            definition,
            arguments,
            ..
        } => {
            definitions
                .get(definition)
                .is_some_and(|definition| !definition.public)
                || arguments
                    .iter()
                    .any(|argument| exposes_private_type(argument, definitions))
        }
        Type::Array(value) | Type::FixedArray { element: value, .. } | Type::Option(value) => {
            exposes_private_type(value, definitions)
        }
        Type::Own { pool, value } => {
            matches!(
                pool,
                PoolTerm::Concrete(pool) if definitions.values().any(|definition| {
                    definition.pool == Some(*pool) && !definition.public
                })
            ) || exposes_private_type(value, definitions)
        }
        Type::PoolArgument(pool) => definitions
            .values()
            .any(|definition| definition.pool == Some(*pool) && !definition.public),
        Type::Function {
            parameters,
            return_type,
        } => {
            parameters
                .iter()
                .any(|value| exposes_private_type(value, definitions))
                || exposes_private_type(return_type, definitions)
        }
        Type::Any { interface, .. } => definitions
            .get(interface)
            .is_some_and(|definition| !definition.public),
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

fn result_type_well_formed(
    type_: &Type,
    allow_omitted_root_error: bool,
    definitions: &BTreeMap<DefinitionId, DefinitionRecord>,
) -> bool {
    fn check(
        type_: &Type,
        allow_omitted_error: bool,
        definitions: &BTreeMap<DefinitionId, DefinitionRecord>,
    ) -> bool {
        match type_ {
            Type::Result { success, error } => {
                check(success, false, definitions)
                    && match error.as_deref() {
                        Some(error @ Type::Nominal { definition, .. })
                            if definitions.get(definition).is_some_and(|definition| {
                                matches!(
                                    definition.kind,
                                    DeclarationKind::Struct
                                        | DeclarationKind::ResourceStruct
                                        | DeclarationKind::Enum
                                )
                            }) =>
                        {
                            check(error, false, definitions)
                        }
                        Some(Type::Parameter { .. }) => true,
                        Some(_) => false,
                        None => allow_omitted_error,
                    }
            }
            Type::Array(value)
            | Type::FixedArray { element: value, .. }
            | Type::Own { value, .. }
            | Type::Option(value) => check(value, false, definitions),
            Type::Tuple(values) => values.iter().all(|value| check(value, false, definitions)),
            Type::Function {
                parameters,
                return_type,
            } => {
                parameters
                    .iter()
                    .all(|value| check(value, false, definitions))
                    && check(return_type, false, definitions)
            }
            Type::Nominal { arguments, .. } => arguments
                .iter()
                .all(|argument| check(argument, false, definitions)),
            _ => true,
        }
    }
    check(type_, allow_omitted_root_error, definitions)
}

fn is_resource_type(type_: &Type, definitions: &BTreeMap<DefinitionId, DefinitionRecord>) -> bool {
    contains_resource(type_, &|definition| {
        definitions
            .get(&definition)
            .is_some_and(|definition| definition.kind == DeclarationKind::ResourceStruct)
    })
}

fn invalid_resource_argument_in_data_nominal(
    type_: &Type,
    definitions: &BTreeMap<DefinitionId, DefinitionRecord>,
) -> bool {
    match type_ {
        Type::Nominal {
            definition,
            arguments,
            ..
        } => {
            let resource_nominal = definitions
                .get(definition)
                .is_some_and(|definition| definition.kind == DeclarationKind::ResourceStruct);
            (!resource_nominal
                && arguments
                    .iter()
                    .any(|argument| is_resource_type(argument, definitions)))
                || arguments.iter().any(|argument| {
                    invalid_resource_argument_in_data_nominal(argument, definitions)
                })
        }
        Type::Array(value) | Type::FixedArray { element: value, .. } | Type::Option(value) => {
            invalid_resource_argument_in_data_nominal(value, definitions)
        }
        Type::Tuple(values) => values
            .iter()
            .any(|value| invalid_resource_argument_in_data_nominal(value, definitions)),
        Type::Function {
            parameters,
            return_type,
        } => {
            parameters
                .iter()
                .any(|value| invalid_resource_argument_in_data_nominal(value, definitions))
                || invalid_resource_argument_in_data_nominal(return_type, definitions)
        }
        Type::Own { value, .. } => invalid_resource_argument_in_data_nominal(value, definitions),
        Type::Result { success, error } => {
            invalid_resource_argument_in_data_nominal(success, definitions)
                || error.as_ref().is_some_and(|error| {
                    invalid_resource_argument_in_data_nominal(error, definitions)
                })
        }
        Type::Unit
        | Type::Bool
        | Type::Integer(_)
        | Type::Float(_)
        | Type::Text
        | Type::Scalar
        | Type::Bytes
        | Type::ConstU64(_)
        | Type::PoolArgument(_)
        | Type::Any { .. }
        | Type::Builtin(_)
        | Type::Parameter { .. }
        | Type::Infer => false,
    }
}

fn resolution_observations(program: &VerifiedProgram) -> Vec<ResolutionObservation> {
    fn expression(value: &Expression, observations: &mut BTreeSet<ResolutionObservation>) {
        {
            let mut observe = |kind, domain, identity| {
                observations.insert(ResolutionObservation::new(
                    kind,
                    value.source.clone(),
                    domain,
                    identity,
                ));
            };
            match &value.kind {
                ExpressionKind::Constant(definition) => {
                    observe(
                        ResolutionKind::Reference,
                        IdentityDomain::Definition,
                        definition.0,
                    );
                }
                ExpressionKind::FunctionValue {
                    definition,
                    specialization,
                } => {
                    observe(
                        ResolutionKind::Reference,
                        IdentityDomain::Definition,
                        definition.0,
                    );
                    if let Some(specialization) = specialization {
                        observe(
                            ResolutionKind::Reference,
                            IdentityDomain::Specialization,
                            specialization.0,
                        );
                    }
                }
                ExpressionKind::Read(place) => {
                    for projection in place.projections.iter() {
                        if let typed_hir::PlaceProjection::Field { definition, .. } = projection {
                            observe(
                                ResolutionKind::Reference,
                                IdentityDomain::Definition,
                                definition.0,
                            );
                        }
                    }
                }
                ExpressionKind::Call { target, .. } => match target {
                    CallTarget::TemplateFunction { definition, .. } => {
                        observe(
                            ResolutionKind::Call,
                            IdentityDomain::Definition,
                            definition.0,
                        );
                    }
                    CallTarget::Function {
                        definition,
                        specialization,
                        ..
                    } => {
                        observe(
                            ResolutionKind::Call,
                            IdentityDomain::Definition,
                            definition.0,
                        );
                        observe(
                            ResolutionKind::Call,
                            IdentityDomain::Specialization,
                            specialization.0,
                        );
                    }
                    CallTarget::Build { primitive, .. } => {
                        observe(
                            ResolutionKind::Call,
                            IdentityDomain::Definition,
                            primitive.definition.0,
                        );
                    }
                    CallTarget::UserVariant { id, .. } => {
                        observe(
                            ResolutionKind::Call,
                            IdentityDomain::Definition,
                            id.definition.0,
                        );
                    }
                    CallTarget::Interface { interface, .. } => {
                        observe(
                            ResolutionKind::Call,
                            IdentityDomain::Definition,
                            interface.0,
                        );
                    }
                    CallTarget::Struct { definition, .. } => {
                        observe(
                            ResolutionKind::Call,
                            IdentityDomain::Definition,
                            definition.0,
                        );
                    }
                    CallTarget::Test { id, .. } => {
                        observe(ResolutionKind::Call, IdentityDomain::Test, id.identity);
                    }
                    CallTarget::Callable { .. } | CallTarget::BuiltinVariant(_) => {}
                },
                _ => {}
            }
        }
        value.visit_children(&mut |child| expression(child, observations));
    }

    fn statements(values: &[Statement], observations: &mut BTreeSet<ResolutionObservation>) {
        for statement in values {
            match statement {
                Statement::Return { value, .. } => {
                    if let Some(value) = value {
                        expression(value, observations);
                    }
                }
                Statement::Panic { value, .. }
                | Statement::Initialize { value, .. }
                | Statement::Assign { value, .. }
                | Statement::Evaluate(value) => expression(value, observations),
                Statement::Defer { action, .. } => expression(action.expression(), observations),
                Statement::Assert { condition, .. } | Statement::Expect { condition, .. } => {
                    expression(condition, observations);
                }
                Statement::If {
                    condition,
                    then_branch,
                    else_branch,
                    ..
                } => {
                    expression(condition, observations);
                    statements(then_branch, observations);
                    statements(else_branch, observations);
                }
                Statement::IfPattern {
                    value,
                    then_branch,
                    else_branch,
                    ..
                } => {
                    expression(value, observations);
                    statements(then_branch, observations);
                    statements(else_branch, observations);
                }
                Statement::For { iterable, body, .. } => {
                    expression(iterable, observations);
                    statements(body, observations);
                }
                Statement::While {
                    condition, body, ..
                } => {
                    expression(condition, observations);
                    statements(body, observations);
                }
                Statement::Match { value, cases, .. } => {
                    expression(value, observations);
                    for case in cases.iter() {
                        if let Some(guard) = &case.guard {
                            expression(guard, observations);
                        }
                        statements(&case.body, observations);
                    }
                }
                Statement::WithPool { scope, body, .. } => {
                    expression(scope, observations);
                    statements(body, observations);
                }
                Statement::Break(_) | Statement::Continue(_) | Statement::Pass(_) => {}
            }
        }
    }

    let mut observations = BTreeSet::new();
    for constant in program.constants().values() {
        expression(&constant.expression, &mut observations);
    }
    for function in program
        .functions()
        .values()
        .chain(program.specialized_functions().values())
    {
        statements(&function.body, &mut observations);
    }
    observations.into_iter().collect()
}

fn plan_tests(
    program: &VerifiedProgram,
    tests: &[TestRecord],
    applications: &[AppliedTest],
    range: &SourceRange,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<TestApplicationObservation> {
    let mut counts = BTreeMap::new();
    for application in applications {
        *counts.entry(application.id).or_insert(0_u32) += 1;
    }
    let known = tests.iter().map(|test| test.id).collect::<BTreeSet<_>>();
    diagnose_unknown_test_applications(&known, applications, range, diagnostics);
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
    for (order, application) in applications.iter().enumerate() {
        let id = &application.id;
        if counts.get(id) != Some(&1) || !known.contains(id) {
            continue;
        }
        let resolved = program.test(*id).expect("applied Test identity resolves");
        if resolved.parameters.len() != application.payload.len() {
            diagnostics.push(Diagnostic::new(
                "test.binding_arity_mismatch",
                range.clone(),
                RecoveryAction::None,
            ));
            continue;
        }
        plan.push(TestApplicationObservation::new(
            resolved.suite.clone(),
            resolved.test.clone(),
            u32::try_from(order).unwrap_or(u32::MAX),
            resolved.asynchronous,
            resolved
                .parameters
                .iter()
                .zip(&application.payload)
                .map(|(parameter, value)| {
                    TestBindingObservation::new(
                        parameter.name.clone(),
                        parameter.type_.display(),
                        match parameter.ownership {
                            OwnershipSyntax::Value => OwnershipMode::Value,
                            OwnershipSyntax::Read => OwnershipMode::Read,
                            OwnershipSyntax::Mut => OwnershipMode::Mut,
                            OwnershipSyntax::Take => OwnershipMode::Take,
                        },
                        value.clone(),
                    )
                })
                .collect(),
        ));
    }
    plan
}

fn diagnose_unknown_test_applications(
    known: &BTreeSet<TestId>,
    applications: &[AppliedTest],
    range: &SourceRange,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for application in applications {
        if !known.contains(&application.id) {
            diagnostics.push(Diagnostic::new(
                "test.unknown_application",
                range.clone(),
                RecoveryAction::None,
            ));
        }
    }
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

fn cancelled() -> SemanticRevision {
    SemanticRevision {
        diagnostics: Vec::new(),
        observations: SemanticObservations::default(),
        defect: None,
        cancelled: true,
        selection_value: None,
    }
}

fn defect(phase: &'static str, evidence: Arc<str>) -> SemanticRevision {
    SemanticRevision {
        diagnostics: Vec::new(),
        observations: SemanticObservations::default(),
        defect: Some(Defect::new(phase, evidence)),
        cancelled: false,
        selection_value: None,
    }
}

fn selection_analysis(
    diagnostics: Vec<Diagnostic>,
    selection_value: Option<bool>,
) -> SemanticRevision {
    SemanticRevision {
        diagnostics,
        observations: SemanticObservations::default(),
        defect: None,
        cancelled: false,
        selection_value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_facts::{FunctionFacts, solve_weighted_costs};

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
    fn malformed_compiler_test_applications_receive_a_structured_diagnostic() {
        let known = TestId {
            suite: DefinitionId(1),
            test: DefinitionId(2),
            identity: 3,
        };
        let unknown = TestId {
            suite: DefinitionId(4),
            test: DefinitionId(5),
            identity: 6,
        };
        let mut diagnostics = Vec::new();
        diagnose_unknown_test_applications(
            &BTreeSet::from([known]),
            &[AppliedTest {
                id: unknown,
                payload: Vec::new(),
            }],
            &SourceRange::new("src/test.wr", 0, 1),
            &mut diagnostics,
        );
        assert_eq!(diagnostics[0].code(), "test.unknown_application");
    }

    #[test]
    fn evaluation_limit_outcomes_receive_a_structured_diagnostic() {
        let mut diagnostics = Vec::new();
        map_evaluation_failure(
            &EvaluationOutcome::LimitExceeded {
                policy: crate::compiler::EvaluationLimitPolicy::RootFuel,
                ceiling: 10,
                used: 11,
            },
            &SourceRange::new("src/image.wr", 0, 1),
            &mut diagnostics,
        )
        .expect("a resource limit is Creator-facing, not a compiler defect");
        assert_eq!(diagnostics[0].code(), "evaluation.limit_exceeded");
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
