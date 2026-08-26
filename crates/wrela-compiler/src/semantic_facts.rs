//! Monotone semantic fact solving and propagation provenance.
//!
//! This module consumes verified HIR and publishes one solved revision. Local
//! facts, call-graph propagation, recursion proofs, logical costs, and inferred
//! error provenance therefore advance together.

mod error_provenance;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::compiler::{Cancellation, Diagnostic, InferredErrorObservation, SourceRange};
use crate::model::{BuildKind, DefinitionId, SpecializationId, Type};
use crate::typed_hir::{self, CallTarget, Expression, ExpressionKind, Statement, VerifiedProgram};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FunctionFacts {
    pub(crate) pure: bool,
    pub(crate) may_panic: bool,
    pub(crate) suspends: bool,
    pub(crate) evaluator_eligible: bool,
    pub(crate) ownership_transfer: bool,
    pub(crate) bounded: bool,
    pub(crate) logical_cost: u64,
    pub(crate) constructs: BTreeSet<BuildKind>,
    pub(crate) calls: BTreeMap<DefinitionId, u64>,
    pub(crate) specialization_calls: BTreeMap<SpecializationId, u64>,
}

#[derive(Clone, Debug)]
pub(crate) struct RecursionFacts {
    pub(crate) proven: BTreeMap<DefinitionId, u64>,
    pub(crate) unproven: Vec<SourceRange>,
}

#[derive(Clone, Debug)]
pub(crate) struct SolvedSemanticFacts {
    pub(crate) definitions: BTreeMap<DefinitionId, FunctionFacts>,
    pub(crate) specializations: BTreeMap<SpecializationId, FunctionFacts>,
    pub(crate) recursion: RecursionFacts,
    pub(crate) inferred_errors: Vec<InferredErrorObservation>,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

pub(crate) fn solve(program: &VerifiedProgram, cancellation: &Cancellation) -> SolvedSemanticFacts {
    let local_definitions = program
        .functions()
        .iter()
        .map(|(id, function)| (*id, local_facts(function)))
        .collect::<BTreeMap<_, _>>();
    let mut definitions = solve_function_facts(local_definitions.clone(), cancellation);
    let mut specializations = solve_specialization_facts(program, &local_definitions, cancellation);
    let recursion = analyze_recursion(program);
    for (definition, maximum_calls) in &recursion.proven {
        if let Some(facts) = definitions.get_mut(definition) {
            facts.bounded = true;
            facts.logical_cost = local_definitions[definition]
                .logical_cost
                .saturating_mul(*maximum_calls);
        }
    }
    for (specialization, facts) in &mut specializations {
        let definition = program.specializations()[specialization].definition;
        if let Some(maximum_calls) = recursion.proven.get(&definition) {
            facts.bounded = true;
            facts.logical_cost = local_definitions[&definition]
                .logical_cost
                .saturating_mul(*maximum_calls);
        }
    }
    let (inferred_errors, diagnostics) = error_provenance::analyze(program, cancellation);
    SolvedSemanticFacts {
        definitions,
        specializations,
        recursion,
        inferred_errors,
        diagnostics,
    }
}

pub(crate) fn test_specialization_demands(
    program: &VerifiedProgram,
    test: crate::model::TestId,
) -> Option<BTreeSet<SpecializationId>> {
    let mut facts = FunctionFacts {
        pure: true,
        may_panic: false,
        suspends: false,
        evaluator_eligible: true,
        ownership_transfer: false,
        bounded: true,
        logical_cost: 0,
        constructs: BTreeSet::new(),
        calls: BTreeMap::new(),
        specialization_calls: BTreeMap::new(),
    };
    visit_statements(program.test_body(test)?, &mut facts);
    Some(facts.specialization_calls.into_keys().collect())
}

pub(crate) fn independently_verify_test_specialization_demands(
    program: &VerifiedProgram,
    test: crate::model::TestId,
) -> Option<BTreeSet<SpecializationId>> {
    let mut facts = FunctionFacts {
        pure: true,
        may_panic: false,
        suspends: false,
        evaluator_eligible: true,
        ownership_transfer: false,
        bounded: true,
        logical_cost: 0,
        constructs: BTreeSet::new(),
        calls: BTreeMap::new(),
        specialization_calls: BTreeMap::new(),
    };
    verifier_visit_statements(program.test_body(test)?, &mut facts);
    Some(facts.specialization_calls.into_keys().collect())
}

/// Re-derives fact-table truth from verified Typed HIR without consulting the
/// producer's local facts or propagation bookkeeping.
pub(crate) fn independently_verify(
    program: &VerifiedProgram,
    claimed: &SolvedSemanticFacts,
    cancellation: &Cancellation,
) -> bool {
    let Some(recursion) = verifier_recursion(program, cancellation) else {
        return false;
    };
    if recursion.proven != claimed.recursion.proven
        || recursion.unproven != claimed.recursion.unproven
    {
        return false;
    }
    let local_definitions = program
        .functions()
        .iter()
        .map(|(id, function)| (*id, verifier_local_facts(function)))
        .collect::<BTreeMap<_, _>>();
    let mut definitions = verifier_propagate(
        local_definitions.clone(),
        |facts| &facts.calls,
        cancellation,
    );
    let specialization_base = program
        .specialized_functions()
        .iter()
        .map(|(id, function)| {
            let specialization = &program.specializations()[id];
            let facts = if specialization.type_arguments.is_empty() {
                local_definitions[&specialization.definition].clone()
            } else {
                verifier_local_facts(function)
            };
            (*id, facts)
        })
        .collect::<BTreeMap<_, _>>();
    let mut specializations = verifier_propagate(
        specialization_base,
        |facts| &facts.specialization_calls,
        cancellation,
    );
    if cancellation.is_cancelled() {
        return false;
    }
    for (definition, maximum_calls) in &recursion.proven {
        if let Some(facts) = definitions.get_mut(definition) {
            facts.bounded = true;
            facts.logical_cost = local_definitions[definition]
                .logical_cost
                .saturating_mul(*maximum_calls);
        }
    }
    for (specialization, facts) in &mut specializations {
        let definition = program.specializations()[specialization].definition;
        if let Some(maximum_calls) = recursion.proven.get(&definition) {
            facts.bounded = true;
            facts.logical_cost = local_definitions[&definition]
                .logical_cost
                .saturating_mul(*maximum_calls);
        }
    }
    definitions == claimed.definitions && specializations == claimed.specializations
}

fn verifier_recursion(
    program: &VerifiedProgram,
    cancellation: &Cancellation,
) -> Option<RecursionFacts> {
    let graph = program
        .functions()
        .iter()
        .map(|(identity, function)| {
            (
                *identity,
                verifier_local_facts(function).calls.into_keys().collect(),
            )
        })
        .collect::<BTreeMap<_, BTreeSet<_>>>();
    let mut proven = BTreeMap::new();
    let mut unproven = Vec::new();
    for component in crate::graph::strongly_connected_components(&graph) {
        if cancellation.is_cancelled() {
            return None;
        }
        let recursive = component.len() > 1
            || component
                .first()
                .is_some_and(|identity| graph[identity].contains(identity));
        if !recursive {
            continue;
        }
        let members = component.iter().copied().collect::<BTreeSet<_>>();
        let measures = component
            .iter()
            .filter_map(|identity| {
                program.functions()[identity]
                    .parameters
                    .iter()
                    .enumerate()
                    .find_map(|(index, (local, type_, _))| match type_ {
                        Type::Integer(kind) => Some((
                            *identity,
                            (
                                index,
                                *local,
                                1_u64.checked_shl(kind.bits()).unwrap_or(u64::MAX),
                            ),
                        )),
                        _ => None,
                    })
            })
            .collect::<BTreeMap<_, _>>();
        let decreases = measures.len() == component.len()
            && component.iter().all(|identity| {
                verifier_recursive_calls_decrease(
                    &program.functions()[identity].body,
                    measures[identity].1,
                    &members,
                    &measures,
                )
            });
        if decreases {
            for identity in component {
                proven.insert(identity, measures[&identity].2);
            }
        } else {
            unproven.extend(
                component
                    .iter()
                    .map(|identity| program.functions()[identity].source.clone()),
            );
        }
    }
    Some(RecursionFacts { proven, unproven })
}

fn verifier_recursive_calls_decrease(
    statements: &[Statement],
    caller_measure: typed_hir::LocalId,
    members: &BTreeSet<DefinitionId>,
    measures: &BTreeMap<DefinitionId, (usize, typed_hir::LocalId, u64)>,
) -> bool {
    fn expression_decreases(
        expression: &Expression,
        caller_measure: typed_hir::LocalId,
        members: &BTreeSet<DefinitionId>,
        measures: &BTreeMap<DefinitionId, (usize, typed_hir::LocalId, u64)>,
    ) -> bool {
        if let ExpressionKind::Call { target, arguments } = &expression.kind {
            let recursive_call = match target {
                CallTarget::Function {
                    definition,
                    argument_order,
                    ..
                }
                | CallTarget::TemplateFunction {
                    definition,
                    argument_order,
                    ..
                } if members.contains(definition) => Some((*definition, argument_order)),
                _ => None,
            };
            if let Some((callee, argument_order)) = recursive_call {
                let parameter = measures[&callee].0;
                let Some(argument_index) = argument_order
                    .iter()
                    .position(|bound| usize::from(*bound) == parameter)
                else {
                    return false;
                };
                let Some(argument) = arguments.get(argument_index) else {
                    return false;
                };
                if !matches!(
                    &argument.kind,
                    ExpressionKind::Binary {
                        operator: typed_hir::BinaryOperator::Subtract,
                        left,
                        right,
                    } if matches!(&left.kind, ExpressionKind::Read(place) if place.local == caller_measure)
                        && matches!(right.kind, ExpressionKind::Literal(typed_hir::Literal::Integer { value, .. }) if value > 0)
                ) {
                    return false;
                }
            }
        }
        let mut valid = true;
        expression.visit_children(&mut |child| {
            valid &= expression_decreases(child, caller_measure, members, measures);
        });
        valid
    }

    statements.iter().all(|statement| match statement {
        Statement::Return { value, .. } => value
            .as_ref()
            .is_none_or(|value| expression_decreases(value, caller_measure, members, measures)),
        Statement::Panic { value, .. }
        | Statement::Assert {
            condition: value, ..
        }
        | Statement::Expect {
            condition: value, ..
        }
        | Statement::Initialize { value, .. }
        | Statement::Assign { value, .. }
        | Statement::Evaluate(value) => {
            expression_decreases(value, caller_measure, members, measures)
        }
        Statement::Defer { action, .. } => {
            let mut valid = true;
            action.visit_expressions(&mut |expression| {
                valid &= expression_decreases(expression, caller_measure, members, measures);
            });
            valid
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
            expression_decreases(value, caller_measure, members, measures)
                && verifier_recursive_calls_decrease(then_branch, caller_measure, members, measures)
                && verifier_recursive_calls_decrease(else_branch, caller_measure, members, measures)
        }
        Statement::For { iterable, body, .. } => {
            expression_decreases(iterable, caller_measure, members, measures)
                && verifier_recursive_calls_decrease(body, caller_measure, members, measures)
        }
        Statement::While {
            condition, body, ..
        } => {
            expression_decreases(condition, caller_measure, members, measures)
                && verifier_recursive_calls_decrease(body, caller_measure, members, measures)
        }
        Statement::Match { value, cases, .. } => {
            expression_decreases(value, caller_measure, members, measures)
                && cases.iter().all(|case| {
                    verifier_recursive_calls_decrease(&case.body, caller_measure, members, measures)
                })
        }
        Statement::WithPool { scope, body, .. } => {
            expression_decreases(scope, caller_measure, members, measures)
                && verifier_recursive_calls_decrease(body, caller_measure, members, measures)
        }
        Statement::Break(_) | Statement::Continue(_) | Statement::Pass(_) => true,
    })
}

fn verifier_propagate<N>(
    base: BTreeMap<N, FunctionFacts>,
    edges: impl Fn(&FunctionFacts) -> &BTreeMap<N, u64> + Copy,
    cancellation: &Cancellation,
) -> BTreeMap<N, FunctionFacts>
where
    N: Copy + Ord,
{
    let graph = base
        .iter()
        .map(|(id, facts)| (*id, edges(facts).keys().copied().collect()))
        .collect::<BTreeMap<N, BTreeSet<N>>>();
    let Some(mut facts) = crate::graph::propagate_monotone(&graph, &base, verifier_merge, || {
        cancellation.is_cancelled()
    }) else {
        return BTreeMap::new();
    };
    let recursive = crate::graph::recursive_nodes(&graph);
    let costs = verifier_costs(&base, &recursive, edges);
    for (identity, cost) in costs {
        let fact = facts.get_mut(&identity).expect("verifier fact exists");
        fact.logical_cost = cost;
        fact.bounded = cost != u64::MAX && !recursive.contains(&identity);
    }
    facts
}

fn verifier_merge(caller: &mut FunctionFacts, callee: &FunctionFacts) -> bool {
    let before = caller.clone();
    caller.pure &= callee.pure;
    caller.may_panic |= callee.may_panic;
    caller.suspends |= callee.suspends;
    caller.evaluator_eligible &= callee.evaluator_eligible;
    caller.ownership_transfer |= callee.ownership_transfer;
    caller.constructs.extend(callee.constructs.iter().copied());
    *caller != before
}

fn verifier_costs<N>(
    base: &BTreeMap<N, FunctionFacts>,
    recursive: &BTreeSet<N>,
    edges: impl Fn(&FunctionFacts) -> &BTreeMap<N, u64>,
) -> BTreeMap<N, u64>
where
    N: Copy + Ord,
{
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
    let mut pending = base.keys().copied().collect::<BTreeSet<_>>();
    while !pending.is_empty() {
        let mut progressed = false;
        for identity in pending.clone() {
            if recursive.contains(&identity) {
                pending.remove(&identity);
                progressed = true;
                continue;
            }
            let dependencies = edges(&base[&identity]);
            if dependencies.keys().any(|callee| pending.contains(callee)) {
                continue;
            }
            let mut cost = base[&identity].logical_cost;
            for (callee, multiplicity) in dependencies {
                if let Some(callee_cost) = costs.get(callee) {
                    cost = cost.saturating_add(callee_cost.saturating_mul(*multiplicity));
                }
            }
            costs.insert(identity, cost);
            pending.remove(&identity);
            progressed = true;
        }
        if !progressed {
            break;
        }
    }
    costs
}

fn verifier_local_facts(function: &typed_hir::HirFunction) -> FunctionFacts {
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
    verifier_visit_statements(&function.body, &mut facts);
    facts.pure = !facts.suspends;
    facts.evaluator_eligible = !facts.suspends;
    facts
}

fn verifier_visit_statements(statements: &[Statement], facts: &mut FunctionFacts) {
    for statement in statements {
        facts.logical_cost = facts.logical_cost.saturating_add(1);
        match statement {
            Statement::Return { value, .. } => {
                value
                    .as_ref()
                    .iter()
                    .for_each(|value| verifier_visit_expression(value, facts));
            }
            Statement::Panic { value, .. }
            | Statement::Assert {
                condition: value, ..
            } => {
                facts.may_panic = true;
                verifier_visit_expression(value, facts);
            }
            Statement::Expect { condition, .. } => verifier_visit_expression(condition, facts),
            Statement::Initialize { value, .. }
            | Statement::Assign { value, .. }
            | Statement::Evaluate(value) => verifier_visit_expression(value, facts),
            Statement::Defer { action, .. } => action
                .visit_expressions(&mut |expression| verifier_visit_expression(expression, facts)),
            Statement::If {
                condition,
                then_branch,
                else_branch,
                ..
            }
            | Statement::IfPattern {
                value: condition,
                then_branch,
                else_branch,
                ..
            } => {
                verifier_visit_expression(condition, facts);
                verifier_visit_statements(then_branch, facts);
                verifier_visit_statements(else_branch, facts);
            }
            Statement::For { iterable, body, .. } => {
                verifier_visit_expression(iterable, facts);
                verifier_visit_statements(body, facts);
            }
            Statement::While {
                condition, body, ..
            } => {
                verifier_visit_expression(condition, facts);
                verifier_visit_statements(body, facts);
            }
            Statement::Match { value, cases, .. } => {
                verifier_visit_expression(value, facts);
                for case in cases.iter() {
                    verifier_visit_statements(&case.body, facts);
                }
            }
            Statement::WithPool { scope, body, .. } => {
                verifier_visit_expression(scope, facts);
                verifier_visit_statements(body, facts);
            }
            Statement::Break(_) | Statement::Continue(_) | Statement::Pass(_) => {}
        }
    }
}

fn verifier_visit_expression(expression: &Expression, facts: &mut FunctionFacts) {
    facts.logical_cost = facts.logical_cost.saturating_add(1);
    if let ExpressionKind::Call { target, arguments } = &expression.kind {
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
            CallTarget::Build { primitive, .. } => {
                facts.constructs.insert(primitive.kind);
            }
            _ => {}
        }
        if let CallTarget::Callable { value } = target {
            verifier_visit_expression(value, facts);
        }
        for argument in arguments.iter() {
            verifier_visit_expression(argument, facts);
        }
        return;
    }
    match &expression.kind {
        ExpressionKind::RepeatedArray { value, length } => {
            facts.logical_cost = facts.logical_cost.saturating_add(*length);
            verifier_visit_expression(value, facts);
        }
        ExpressionKind::Negate(value) => {
            facts.may_panic |= matches!(value.type_, Type::Integer(_));
            verifier_visit_expression(value, facts);
        }
        ExpressionKind::Index { .. } => {
            facts.may_panic = true;
            expression.visit_children(&mut |child| verifier_visit_expression(child, facts));
        }
        ExpressionKind::Await(value) => {
            facts.suspends = true;
            verifier_visit_expression(value, facts);
        }
        ExpressionKind::Binary { operator, left, .. }
            if matches!(left.type_, Type::Integer(_))
                && matches!(
                    operator,
                    typed_hir::BinaryOperator::Add
                        | typed_hir::BinaryOperator::Subtract
                        | typed_hir::BinaryOperator::Multiply
                        | typed_hir::BinaryOperator::Divide
                        | typed_hir::BinaryOperator::Remainder
                ) =>
        {
            facts.may_panic = true;
            expression.visit_children(&mut |child| verifier_visit_expression(child, facts));
        }
        _ => expression.visit_children(&mut |child| verifier_visit_expression(child, facts)),
    }
}

pub(crate) fn infer_error_signatures(
    functions: &BTreeMap<DefinitionId, Arc<typed_hir::HirFunction>>,
    cancellation: &Cancellation,
) -> BTreeMap<DefinitionId, Type> {
    error_provenance::infer_signatures(functions, cancellation)
}

fn solve_function_facts(
    base: BTreeMap<DefinitionId, FunctionFacts>,
    cancellation: &Cancellation,
) -> BTreeMap<DefinitionId, FunctionFacts> {
    let graph = base
        .iter()
        .map(|(id, facts)| (*id, facts.calls.keys().copied().collect()))
        .collect::<BTreeMap<_, _>>();
    solve_fact_graph(base, graph, |fact| &fact.calls, cancellation)
}

fn solve_specialization_facts(
    program: &VerifiedProgram,
    local_definitions: &BTreeMap<DefinitionId, FunctionFacts>,
    cancellation: &Cancellation,
) -> BTreeMap<SpecializationId, FunctionFacts> {
    let base = program
        .specialized_functions()
        .iter()
        .map(|(id, function)| {
            let specialization = &program.specializations()[id];
            let facts = if specialization.type_arguments.is_empty() {
                local_definitions[&specialization.definition].clone()
            } else {
                local_facts(function)
            };
            (*id, facts)
        })
        .collect::<BTreeMap<_, _>>();
    let graph = base
        .iter()
        .map(|(id, facts)| (*id, facts.specialization_calls.keys().copied().collect()))
        .collect::<BTreeMap<_, BTreeSet<_>>>();
    solve_fact_graph(base, graph, |fact| &fact.specialization_calls, cancellation)
}

fn solve_fact_graph<N>(
    base: BTreeMap<N, FunctionFacts>,
    graph: BTreeMap<N, BTreeSet<N>>,
    weighted_edges: impl Fn(&FunctionFacts) -> &BTreeMap<N, u64>,
    cancellation: &Cancellation,
) -> BTreeMap<N, FunctionFacts>
where
    N: Copy + Ord,
{
    let Some(mut facts) =
        crate::graph::propagate_monotone(&graph, &base, merge_function_facts, || {
            cancellation.is_cancelled()
        })
    else {
        return BTreeMap::new();
    };
    let recursive = crate::graph::recursive_nodes(&graph);
    let costs = solve_weighted_costs(&base, &recursive, weighted_edges);
    for (id, cost) in costs {
        let fact = facts.get_mut(&id).expect("fact exists");
        fact.logical_cost = cost;
        fact.bounded = cost != u64::MAX && !recursive.contains(&id);
    }
    facts
}

fn merge_function_facts(caller: &mut FunctionFacts, callee: &FunctionFacts) -> bool {
    let previous = (
        caller.pure,
        caller.may_panic,
        caller.suspends,
        caller.evaluator_eligible,
        caller.ownership_transfer,
        caller.constructs.len(),
    );
    caller.pure &= callee.pure;
    caller.may_panic |= callee.may_panic;
    caller.suspends |= callee.suspends;
    caller.evaluator_eligible &= callee.evaluator_eligible;
    caller.constructs.extend(callee.constructs.iter().copied());
    caller.ownership_transfer |= callee.ownership_transfer;
    previous
        != (
            caller.pure,
            caller.may_panic,
            caller.suspends,
            caller.evaluator_eligible,
            caller.ownership_transfer,
            caller.constructs.len(),
        )
}

pub(crate) fn solve_weighted_costs<N>(
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
            Statement::Assert { condition, .. } => {
                facts.may_panic = true;
                visit_expression(condition, facts);
            }
            Statement::Expect { condition, .. } => visit_expression(condition, facts),
            Statement::Initialize { value, .. }
            | Statement::Assign { value, .. }
            | Statement::Evaluate(value) => visit_expression(value, facts),
            Statement::Defer { action, .. } => {
                action.visit_expressions(&mut |expression| visit_expression(expression, facts));
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
                visit_expression(value, facts);
                visit_statements(then_branch, facts);
                visit_statements(else_branch, facts);
            }
            Statement::For { iterable, body, .. } => {
                visit_expression(iterable, facts);
                visit_statements(body, facts);
            }
            Statement::While {
                condition, body, ..
            } => {
                visit_expression(condition, facts);
                visit_statements(body, facts);
            }
            Statement::Match { value, cases, .. } => {
                visit_expression(value, facts);
                cases
                    .iter()
                    .for_each(|case| visit_statements(&case.body, facts));
            }
            Statement::WithPool { scope, body, .. } => {
                visit_expression(scope, facts);
                visit_statements(body, facts);
            }
            Statement::Break(_) | Statement::Continue(_) | Statement::Pass(_) => {}
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
                CallTarget::Build { primitive, .. } => {
                    facts.constructs.insert(primitive.kind);
                }
                _ => {}
            }
            if let CallTarget::Callable { value } = target {
                visit_expression(value, facts);
            }
            arguments
                .iter()
                .for_each(|argument| visit_expression(argument, facts));
        }
        ExpressionKind::RepeatedArray { value, length } => {
            facts.logical_cost = facts.logical_cost.saturating_add(*length);
            visit_expression(value, facts);
        }
        ExpressionKind::Negate(value) => {
            facts.may_panic |= matches!(value.type_, Type::Integer(_));
            visit_expression(value, facts);
        }
        ExpressionKind::Index { .. } => {
            facts.may_panic = true;
            expression.visit_children(&mut |child| visit_expression(child, facts));
        }
        ExpressionKind::Await(value) => {
            facts.suspends = true;
            visit_expression(value, facts);
        }
        ExpressionKind::Binary { operator, left, .. }
            if matches!(left.type_, Type::Integer(_))
                && matches!(
                    operator,
                    typed_hir::BinaryOperator::Add
                        | typed_hir::BinaryOperator::Subtract
                        | typed_hir::BinaryOperator::Multiply
                        | typed_hir::BinaryOperator::Divide
                        | typed_hir::BinaryOperator::Remainder
                ) =>
        {
            facts.may_panic = true;
            expression.visit_children(&mut |child| visit_expression(child, facts));
        }
        _ => expression.visit_children(&mut |child| visit_expression(child, facts)),
    }
}

pub(crate) fn expression_constructs(expression: &Expression) -> bool {
    if matches!(
        expression.kind,
        ExpressionKind::Call {
            target: CallTarget::Build { .. },
            ..
        }
    ) {
        return true;
    }
    let mut constructs = false;
    expression.visit_children(&mut |child| constructs |= expression_constructs(child));
    constructs
}

fn analyze_recursion(program: &VerifiedProgram) -> RecursionFacts {
    let graph = program
        .functions()
        .iter()
        .map(|(id, function)| (*id, local_facts(function).calls.keys().copied().collect()))
        .collect::<BTreeMap<_, _>>();
    let mut proven = BTreeMap::new();
    let mut unproven = Vec::new();
    for component in crate::graph::strongly_connected_components(&graph) {
        let recursive = component.len() > 1
            || component
                .first()
                .is_some_and(|id| graph.get(id).is_some_and(|callees| callees.contains(id)));
        if !recursive {
            continue;
        }
        let members = component.iter().copied().collect::<BTreeSet<_>>();
        let measures = component
            .iter()
            .filter_map(|id| {
                let function = &program.functions()[id];
                function
                    .parameters
                    .iter()
                    .enumerate()
                    .find_map(|(index, (local, type_, _))| match type_ {
                        Type::Integer(kind) => Some((
                            *id,
                            (
                                index,
                                *local,
                                1_u64.checked_shl(kind.bits()).unwrap_or(u64::MAX),
                            ),
                        )),
                        _ => None,
                    })
            })
            .collect::<BTreeMap<_, _>>();
        let decreases = measures.len() == component.len()
            && component.iter().all(|id| {
                recursive_calls_decrease(
                    &program.functions()[id].body,
                    measures[id].1,
                    &members,
                    &measures,
                )
            });
        if decreases {
            component.iter().for_each(|id| {
                proven.insert(*id, measures[id].2);
            });
        } else {
            unproven.extend(
                component
                    .iter()
                    .map(|id| program.functions()[id].source.clone()),
            );
        }
    }
    RecursionFacts { proven, unproven }
}

fn recursive_calls_decrease(
    statements: &[Statement],
    caller_measure: typed_hir::LocalId,
    members: &BTreeSet<DefinitionId>,
    measures: &BTreeMap<DefinitionId, (usize, typed_hir::LocalId, u64)>,
) -> bool {
    fn expression_decreases(
        expression: &Expression,
        caller_measure: typed_hir::LocalId,
        members: &BTreeSet<DefinitionId>,
        measures: &BTreeMap<DefinitionId, (usize, typed_hir::LocalId, u64)>,
    ) -> bool {
        if let ExpressionKind::Call { target, arguments } = &expression.kind {
            let call = match target {
                CallTarget::Function {
                    definition,
                    argument_order,
                    ..
                }
                | CallTarget::TemplateFunction {
                    definition,
                    argument_order,
                    ..
                } if members.contains(definition) => Some((*definition, argument_order)),
                _ => None,
            };
            if let Some((callee, argument_order)) = call {
                let parameter_index = measures[&callee].0;
                let Some(source_index) = argument_order
                    .iter()
                    .position(|bound| usize::from(*bound) == parameter_index)
                else {
                    return false;
                };
                let Some(argument) = arguments.get(source_index) else {
                    return false;
                };
                if !matches!(
                    &argument.kind,
                    ExpressionKind::Binary {
                        operator: typed_hir::BinaryOperator::Subtract,
                        left,
                        right,
                    } if matches!(&left.kind, ExpressionKind::Read(place) if place.local == caller_measure)
                        && matches!(right.kind, ExpressionKind::Literal(typed_hir::Literal::Integer { value, .. }) if value > 0)
                ) {
                    return false;
                }
            }
        }
        let mut valid = true;
        expression.visit_children(&mut |child| {
            valid &= expression_decreases(child, caller_measure, members, measures);
        });
        valid
    }

    statements.iter().all(|statement| match statement {
        Statement::Return { value, .. } => value
            .as_ref()
            .is_none_or(|value| expression_decreases(value, caller_measure, members, measures)),
        Statement::Panic { value, .. }
        | Statement::Assert {
            condition: value, ..
        }
        | Statement::Expect {
            condition: value, ..
        }
        | Statement::Initialize { value, .. }
        | Statement::Assign { value, .. }
        | Statement::Evaluate(value) => {
            expression_decreases(value, caller_measure, members, measures)
        }
        Statement::Defer { action, .. } => {
            let mut decreases = true;
            action.visit_expressions(&mut |expression| {
                decreases &= expression_decreases(expression, caller_measure, members, measures);
            });
            decreases
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
            expression_decreases(value, caller_measure, members, measures)
                && recursive_calls_decrease(then_branch, caller_measure, members, measures)
                && recursive_calls_decrease(else_branch, caller_measure, members, measures)
        }
        Statement::For { iterable, body, .. } => {
            expression_decreases(iterable, caller_measure, members, measures)
                && recursive_calls_decrease(body, caller_measure, members, measures)
        }
        Statement::While {
            condition, body, ..
        } => {
            expression_decreases(condition, caller_measure, members, measures)
                && recursive_calls_decrease(body, caller_measure, members, measures)
        }
        Statement::Match { value, cases, .. } => {
            expression_decreases(value, caller_measure, members, measures)
                && cases.iter().all(|case| {
                    recursive_calls_decrease(&case.body, caller_measure, members, measures)
                })
        }
        Statement::WithPool { scope, body, .. } => {
            expression_decreases(scope, caller_measure, members, measures)
                && recursive_calls_decrease(body, caller_measure, members, measures)
        }
        Statement::Break(_) | Statement::Continue(_) | Statement::Pass(_) => true,
    })
}
