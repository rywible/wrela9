#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use crate::compiler::{
    Cancellation, Diagnostic, DiagnosticLabelRole, IdentityDomain, InferredErrorObservation,
    RecoveryAction,
};
use crate::model::{BuiltinVariant, DefinitionId, SpecializationId, Type};
use crate::typed_hir::{
    CallTarget, Expression, ExpressionKind, HirFunction, LocalId, Statement, VerifiedProgram,
};

#[derive(Clone, Debug, Default)]
struct FlowFacts {
    direct_errors: BTreeMap<Type, BTreeSet<crate::compiler::SourceRange>>,
    dependencies: BTreeSet<SpecializationId>,
    propagated: BTreeSet<SpecializationId>,
    propagated_errors: BTreeMap<crate::compiler::SourceRange, Type>,
    propagated_options: BTreeSet<crate::compiler::SourceRange>,
}

#[derive(Clone, Debug, Default)]
struct DefinitionFlowFacts {
    direct_errors: BTreeSet<Type>,
    dependencies: BTreeSet<DefinitionId>,
    propagated_errors: BTreeSet<Type>,
}

pub(crate) fn infer_signatures(
    functions: &BTreeMap<DefinitionId, std::sync::Arc<HirFunction>>,
    cancellation: &Cancellation,
) -> BTreeMap<DefinitionId, Type> {
    let facts = functions
        .iter()
        .map(|(id, function)| (*id, scan_definition(&function.body)))
        .collect::<BTreeMap<_, _>>();
    let candidates = functions
        .iter()
        .filter(|(_, function)| matches!(function.return_type, Type::Result { error: None, .. }))
        .map(|(id, _)| *id)
        .collect::<BTreeSet<_>>();
    let mut errors = candidates
        .iter()
        .map(|id| {
            let mut errors = facts[id].direct_errors.clone();
            errors.extend(facts[id].propagated_errors.iter().cloned());
            (*id, errors)
        })
        .collect::<BTreeMap<_, _>>();
    let mut dependents = BTreeMap::<DefinitionId, BTreeSet<DefinitionId>>::new();
    for caller in &candidates {
        for callee in &facts[caller].dependencies {
            match functions.get(callee).map(|function| &function.return_type) {
                Some(Type::Result {
                    error: Some(error), ..
                }) => {
                    errors
                        .get_mut(caller)
                        .expect("candidate has an error set")
                        .insert((**error).clone());
                }
                Some(Type::Result { error: None, .. }) if candidates.contains(callee) => {
                    dependents.entry(*callee).or_default().insert(*caller);
                }
                _ => {}
            }
        }
    }
    let mut pending = candidates;
    while let Some(callee) = pending.pop_first() {
        if cancellation.is_cancelled() {
            return BTreeMap::new();
        }
        let additions = errors[&callee].clone();
        for caller in dependents.get(&callee).into_iter().flatten() {
            let caller_errors = errors
                .get_mut(caller)
                .expect("dependent candidate has an error set");
            let previous_len = caller_errors.len();
            caller_errors.extend(additions.iter().cloned());
            if caller_errors.len() != previous_len {
                pending.insert(*caller);
            }
        }
    }
    errors
        .into_iter()
        .filter_map(|(id, errors)| {
            (errors.len() == 1).then(|| (id, errors.into_iter().next().expect("one error")))
        })
        .collect()
}

fn scan_definition(statements: &[Statement]) -> DefinitionFlowFacts {
    type LocalOrigins = BTreeMap<LocalId, BTreeSet<DefinitionId>>;

    fn result_origins(expression: &Expression, locals: &LocalOrigins) -> BTreeSet<DefinitionId> {
        match &expression.kind {
            ExpressionKind::Call {
                target:
                    CallTarget::Function { definition, .. }
                    | CallTarget::TemplateFunction { definition, .. },
                ..
            } => BTreeSet::from([*definition]),
            ExpressionKind::Read(place) => locals.get(&place.local).cloned().unwrap_or_default(),
            ExpressionKind::Await(value) => result_origins(value, locals),
            _ => BTreeSet::new(),
        }
    }

    fn join_origins(branches: impl IntoIterator<Item = LocalOrigins>) -> LocalOrigins {
        let mut joined = LocalOrigins::new();
        for branch in branches {
            for (local, origins) in branch {
                joined.entry(local).or_default().extend(origins);
            }
        }
        joined
    }

    fn visit(statements: &[Statement], facts: &mut DefinitionFlowFacts, locals: &mut LocalOrigins) {
        for statement in statements {
            match statement {
                Statement::Return { value, .. } => {
                    if let Some(value) = value {
                        facts.dependencies.extend(result_origins(value, locals));
                        if let Type::Result {
                            error: Some(error), ..
                        } = &value.type_
                        {
                            facts.direct_errors.insert((**error).clone());
                        }
                        visit_expression(value, facts, locals);
                    }
                }
                Statement::Panic { value, .. }
                | Statement::Assert {
                    condition: value, ..
                }
                | Statement::Expect {
                    condition: value, ..
                }
                | Statement::Evaluate(value) => visit_expression(value, facts, locals),
                Statement::Defer { action, .. } => {
                    visit_expression(action.expression(), facts, locals)
                }
                Statement::Initialize { place, value, .. }
                | Statement::Assign { place, value, .. } => {
                    visit_expression(value, facts, locals);
                    locals.insert(place.local, result_origins(value, locals));
                }
                Statement::If {
                    condition: value,
                    then_branch,
                    else_branch,
                    ..
                }
                | Statement::IfPattern {
                    value,
                    then_branch,
                    else_branch,
                    ..
                } => {
                    visit_expression(value, facts, locals);
                    let mut then_origins = locals.clone();
                    visit(then_branch, facts, &mut then_origins);
                    let mut else_origins = locals.clone();
                    visit(else_branch, facts, &mut else_origins);
                    *locals = join_origins([then_origins, else_origins]);
                }
                Statement::For { iterable, body, .. } => {
                    visit_expression(iterable, facts, locals);
                    let before = locals.clone();
                    let mut after = before.clone();
                    visit(body, facts, &mut after);
                    *locals = join_origins([before, after]);
                }
                Statement::While {
                    condition, body, ..
                } => {
                    visit_expression(condition, facts, locals);
                    let before = locals.clone();
                    let mut after = before.clone();
                    visit(body, facts, &mut after);
                    *locals = join_origins([before, after]);
                }
                Statement::Break(_) | Statement::Continue(_) => {}
                Statement::Match { value, cases, .. } => {
                    visit_expression(value, facts, locals);
                    let mut branches = Vec::new();
                    for case in cases.iter() {
                        let mut branch = locals.clone();
                        visit(&case.body, facts, &mut branch);
                        branches.push(branch);
                    }
                    *locals = join_origins(branches);
                }
                Statement::WithPool {
                    binding,
                    scope,
                    body,
                    ..
                } => {
                    visit_expression(scope, facts, locals);
                    locals.insert(binding.local, result_origins(scope, locals));
                    visit(body, facts, locals);
                    locals.remove(&binding.local);
                }
                Statement::Pass(_) => {}
            }
        }
    }

    fn visit_expression(
        expression: &Expression,
        facts: &mut DefinitionFlowFacts,
        locals: &LocalOrigins,
    ) {
        match &expression.kind {
            ExpressionKind::Call { target, arguments } => {
                if matches!(
                    target,
                    CallTarget::BuiltinVariant(BuiltinVariant::ResultErr)
                ) && let Some(error) = arguments.first()
                {
                    facts.direct_errors.insert(error.type_.clone());
                }
            }
            ExpressionKind::Propagate(value) => {
                facts.dependencies.extend(result_origins(value, locals));
                if let Type::Result {
                    error: Some(error), ..
                } = &value.type_
                {
                    facts.propagated_errors.insert((**error).clone());
                }
            }
            _ => {}
        }
        expression.visit_children(&mut |child| visit_expression(child, facts, locals));
    }

    let mut facts = DefinitionFlowFacts::default();
    visit(statements, &mut facts, &mut LocalOrigins::new());
    facts
}

pub(crate) fn analyze(
    program: &VerifiedProgram,
    cancellation: &Cancellation,
) -> (Vec<InferredErrorObservation>, Vec<Diagnostic>) {
    let facts = program
        .specialized_functions()
        .iter()
        .map(|(id, function)| (*id, scan(&function.body)))
        .collect::<BTreeMap<_, _>>();
    let candidates = program
        .specialized_functions()
        .iter()
        .filter(|(_, function)| matches!(function.return_type, Type::Result { error: None, .. }))
        .map(|(id, _)| *id)
        .collect::<BTreeSet<_>>();

    let mut errors = candidates
        .iter()
        .map(|id| {
            let mut errors = facts[id]
                .direct_errors
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>();
            errors.extend(facts[id].propagated_errors.values().cloned());
            (*id, errors)
        })
        .collect::<BTreeMap<_, _>>();
    let mut dependents = BTreeMap::<SpecializationId, BTreeSet<SpecializationId>>::new();
    for caller in &candidates {
        for callee in &facts[caller].dependencies {
            match &program.specialized_functions()[callee].return_type {
                Type::Result {
                    error: Some(error), ..
                } => {
                    errors
                        .get_mut(caller)
                        .expect("candidate has an error set")
                        .insert((**error).clone());
                }
                Type::Result { error: None, .. } if candidates.contains(callee) => {
                    dependents.entry(*callee).or_default().insert(*caller);
                }
                _ => {}
            }
        }
    }

    let mut pending = candidates.clone();
    while let Some(callee) = pending.pop_first() {
        if cancellation.is_cancelled() {
            return (Vec::new(), Vec::new());
        }
        let additions = errors[&callee].clone();
        for caller in dependents.get(&callee).into_iter().flatten() {
            let caller_errors = errors
                .get_mut(caller)
                .expect("dependent candidate has an error set");
            let previous_len = caller_errors.len();
            caller_errors.extend(additions.iter().cloned());
            if caller_errors.len() != previous_len {
                pending.insert(*caller);
            }
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
                set.first().expect("one inferred error").display(),
            ));
        } else {
            let mut diagnostic = Diagnostic::new(
                if set.is_empty() {
                    "semantic.unconstrained_inferred_error"
                } else {
                    "semantic.conflicting_inferred_errors"
                },
                function.source.clone(),
                RecoveryAction::None,
            )
            .with_parameter("repair", "explicit_error_annotation")
            .with_parameter("conversion", "map_error");
            for type_ in set {
                for site in facts[id]
                    .direct_errors
                    .get(type_)
                    .into_iter()
                    .flatten()
                    .chain(
                        facts[id]
                            .propagated_errors
                            .iter()
                            .filter_map(|(site, error)| (error == type_).then_some(site)),
                    )
                    .take(8)
                {
                    diagnostic =
                        diagnostic.with_label(site.clone(), DiagnosticLabelRole::PropagationSource);
                }
            }
            diagnostics.push(diagnostic);
        }
    }

    for (id, function) in program.specialized_functions() {
        if !matches!(function.return_type, Type::Option(_)) {
            diagnostics.extend(facts[id].propagated_options.iter().cloned().map(|site| {
                Diagnostic::new(
                    "semantic.propagation_return_mismatch",
                    site,
                    RecoveryAction::None,
                )
            }));
        }
        if !matches!(function.return_type, Type::Result { .. }) {
            diagnostics.extend(facts[id].propagated_errors.keys().cloned().map(|site| {
                Diagnostic::new(
                    "semantic.propagation_return_mismatch",
                    site,
                    RecoveryAction::None,
                )
            }));
        }
        let Type::Result {
            error: Some(caller_error),
            ..
        } = &function.return_type
        else {
            continue;
        };
        for (site, propagated_error) in &facts[id].propagated_errors {
            if propagated_error != caller_error.as_ref() {
                let mut diagnostic = Diagnostic::new(
                    "semantic.propagation_error_mismatch",
                    site.clone(),
                    RecoveryAction::None,
                );
                if let Some(callee) = facts[id].propagated.iter().find(|callee| {
                    matches!(
                        &program.specialized_functions()[callee].return_type,
                        Type::Result { error: Some(error), .. } if error.as_ref() == propagated_error
                    )
                }) {
                    diagnostic = diagnostic
                        .with_identity_parameter(
                            "callee",
                            IdentityDomain::Specialization,
                            callee.0,
                        )
                        .with_label(
                            program.specialized_functions()[callee].source.clone(),
                            DiagnosticLabelRole::PropagationSource,
                        );
                }
                diagnostics.push(diagnostic);
            }
        }
    }

    (observations, diagnostics)
}

fn scan(statements: &[Statement]) -> FlowFacts {
    let mut facts = FlowFacts::default();
    walk_statements(statements, &mut facts);
    facts
}

fn walk_statements(statements: &[Statement], facts: &mut FlowFacts) {
    for statement in statements {
        match statement {
            Statement::Return { value, .. } => {
                if let Some(value) = value {
                    if let Some(callee) = directly_returned_specialization(value) {
                        facts.dependencies.insert(callee);
                    }
                    walk_expression(value, facts);
                }
            }
            Statement::Panic { value, .. }
            | Statement::Assert {
                condition: value, ..
            }
            | Statement::Expect {
                condition: value, ..
            }
            | Statement::Initialize { value, .. }
            | Statement::Assign { value, .. }
            | Statement::Evaluate(value) => walk_expression(value, facts),
            Statement::Defer { action, .. } => walk_expression(action.expression(), facts),
            Statement::If {
                condition: value,
                then_branch,
                else_branch,
                ..
            }
            | Statement::IfPattern {
                value,
                then_branch,
                else_branch,
                ..
            } => {
                walk_expression(value, facts);
                walk_statements(then_branch, facts);
                walk_statements(else_branch, facts);
            }
            Statement::For { iterable, body, .. } => {
                walk_expression(iterable, facts);
                walk_statements(body, facts);
            }
            Statement::While {
                condition, body, ..
            } => {
                walk_expression(condition, facts);
                walk_statements(body, facts);
            }
            Statement::Break(_) | Statement::Continue(_) => {}
            Statement::Match { value, cases, .. } => {
                walk_expression(value, facts);
                for case in cases.iter() {
                    walk_statements(&case.body, facts);
                }
            }
            Statement::WithPool { scope, body, .. } => {
                walk_expression(scope, facts);
                walk_statements(body, facts);
            }
            Statement::Pass(_) => {}
        }
    }
}

fn directly_returned_specialization(expression: &Expression) -> Option<SpecializationId> {
    match &expression.kind {
        ExpressionKind::Call {
            target: CallTarget::Function { specialization, .. },
            ..
        } => Some(*specialization),
        ExpressionKind::Await(value) => directly_returned_specialization(value),
        _ => None,
    }
}

fn walk_expression(expression: &Expression, facts: &mut FlowFacts) {
    match &expression.kind {
        ExpressionKind::Call { target, arguments } => {
            if matches!(
                target,
                CallTarget::BuiltinVariant(BuiltinVariant::ResultErr)
            ) && let Some(error) = arguments.first()
            {
                facts
                    .direct_errors
                    .entry(error.type_.clone())
                    .or_default()
                    .insert(expression.source.clone());
            }
        }
        ExpressionKind::Propagate(value) => {
            if matches!(value.type_, Type::Option(_)) {
                facts.propagated_options.insert(expression.source.clone());
            }
            if let Type::Result {
                error: Some(error), ..
            } = &value.type_
            {
                facts
                    .propagated_errors
                    .insert(expression.source.clone(), (**error).clone());
            }
            if let ExpressionKind::Call {
                target: CallTarget::Function { specialization, .. },
                ..
            } = &value.kind
            {
                facts.dependencies.insert(*specialization);
                facts.propagated.insert(*specialization);
            }
        }
        _ => {}
    }
    expression.visit_children(&mut |child| walk_expression(child, facts));
}
