//! Shared termination and match-coverage proofs.
//!
//! Syntax and verified HIR intentionally remain separate representations. This
//! module is the single owner of the proof rules and exposes one adapter for
//! each representation so callers cannot silently drift apart.

use crate::syntax::{
    ExpressionSyntax, ExpressionSyntaxKind, MatchCaseSyntax, PatternSyntaxKind, StatementSyntax,
};
use crate::typed_hir::{HirMatchCase, HirMatchPattern, Literal, Statement};

#[derive(Clone, Copy)]
struct FlowSummary {
    falls_through: bool,
    terminates_function: bool,
}

impl FlowSummary {
    const FALLTHROUGH: Self = Self {
        falls_through: true,
        terminates_function: false,
    };
    const FUNCTION_EXIT: Self = Self {
        falls_through: false,
        terminates_function: true,
    };
    const LOOP_EXIT: Self = Self {
        falls_through: false,
        terminates_function: false,
    };

    fn alternatives(summaries: impl IntoIterator<Item = Self>, exhaustive: bool) -> Self {
        if !exhaustive {
            return Self::FALLTHROUGH;
        }
        let summaries = summaries.into_iter().collect::<Vec<_>>();
        Self {
            falls_through: summaries.iter().any(|summary| summary.falls_through),
            terminates_function: summaries.iter().all(|summary| summary.terminates_function),
        }
    }
}

fn syntax_flow(statements: &[StatementSyntax]) -> FlowSummary {
    for statement in statements {
        let summary = match statement {
            StatementSyntax::Return { .. } | StatementSyntax::Panic { .. } => {
                FlowSummary::FUNCTION_EXIT
            }
            StatementSyntax::Break(_) | StatementSyntax::Continue(_) => FlowSummary::LOOP_EXIT,
            StatementSyntax::If {
                then_branch,
                else_branch,
                ..
            } => FlowSummary::alternatives(
                [syntax_flow(then_branch), syntax_flow(else_branch)],
                !else_branch.is_empty(),
            ),
            StatementSyntax::Comptime { branches, .. } => {
                let exhaustive = branches
                    .last()
                    .is_some_and(|branch| branch.condition.is_none());
                FlowSummary::alternatives(
                    branches
                        .iter()
                        .map(|branch| syntax_flow(&branch.statements)),
                    exhaustive,
                )
            }
            StatementSyntax::Match { cases, .. } => FlowSummary::alternatives(
                cases.iter().map(|case| syntax_flow(&case.body)),
                syntax_match_exhaustive(cases),
            ),
            StatementSyntax::With { body, .. } => syntax_flow(body),
            StatementSyntax::Assert { .. }
            | StatementSyntax::Assign { .. }
            | StatementSyntax::Expect { .. }
            | StatementSyntax::Evaluate(_)
            | StatementSyntax::For { .. }
            | StatementSyntax::While { .. }
            | StatementSyntax::Defer { .. }
            | StatementSyntax::Unsupported { .. }
            | StatementSyntax::Pass(_) => FlowSummary::FALLTHROUGH,
        };
        if !summary.falls_through {
            return summary;
        }
    }
    FlowSummary::FALLTHROUGH
}

pub(crate) fn syntax_statements_terminate(statements: &[StatementSyntax]) -> bool {
    syntax_flow(statements).terminates_function
}

pub(crate) fn syntax_statements_fall_through(statements: &[StatementSyntax]) -> bool {
    syntax_flow(statements).falls_through
}

fn syntax_match_exhaustive(cases: &[MatchCaseSyntax]) -> bool {
    if cases.last().is_some_and(|case| {
        matches!(
            case.pattern.kind,
            PatternSyntaxKind::Wildcard
                | PatternSyntaxKind::Binding(_)
                | PatternSyntaxKind::Take(_)
        ) && case.guard.is_none()
    }) {
        return true;
    }
    let mut saw_false = false;
    let mut saw_true = false;
    let mut only_bools = true;
    for case in cases {
        match &case.pattern.kind {
            PatternSyntaxKind::Literal(ExpressionSyntax {
                kind: ExpressionSyntaxKind::Bool(false),
                ..
            }) if case.guard.is_none() => saw_false = true,
            PatternSyntaxKind::Literal(ExpressionSyntax {
                kind: ExpressionSyntaxKind::Bool(true),
                ..
            }) if case.guard.is_none() => saw_true = true,
            _ => only_bools = false,
        }
    }
    (only_bools && saw_false && saw_true)
        || (!cases.is_empty()
            && cases.iter().all(|case| {
                case.guard.is_none() && syntax_pattern_can_close_match(&case.pattern.kind)
            }))
}

fn syntax_pattern_can_close_match(kind: &PatternSyntaxKind) -> bool {
    match kind {
        PatternSyntaxKind::Constructor { .. }
        | PatternSyntaxKind::Tuple(_)
        | PatternSyntaxKind::FixedArray(_)
        | PatternSyntaxKind::Take(_) => true,
        PatternSyntaxKind::Or(alternatives) => alternatives
            .iter()
            .all(|alternative| syntax_pattern_can_close_match(&alternative.kind)),
        _ => false,
    }
}

fn verified_flow(statements: &[Statement]) -> FlowSummary {
    for statement in statements {
        let summary = match statement {
            Statement::Return { .. } | Statement::Panic { .. } => FlowSummary::FUNCTION_EXIT,
            Statement::Break(_) | Statement::Continue(_) => FlowSummary::LOOP_EXIT,
            Statement::If {
                then_branch,
                else_branch,
                ..
            }
            | Statement::IfPattern {
                then_branch,
                else_branch,
                ..
            } => FlowSummary::alternatives(
                [verified_flow(then_branch), verified_flow(else_branch)],
                !else_branch.is_empty(),
            ),
            Statement::Match { cases, .. } => FlowSummary::alternatives(
                cases.iter().map(|case| verified_flow(&case.body)),
                verified_match_exhaustive(cases),
            ),
            Statement::WithPool { body, .. } => verified_flow(body),
            Statement::Assert { .. }
            | Statement::Expect { .. }
            | Statement::Initialize { .. }
            | Statement::Assign { .. }
            | Statement::Evaluate(_)
            | Statement::For { .. }
            | Statement::While { .. }
            | Statement::Defer { .. }
            | Statement::Pass(_) => FlowSummary::FALLTHROUGH,
        };
        if !summary.falls_through {
            return summary;
        }
    }
    FlowSummary::FALLTHROUGH
}

pub(crate) fn verified_statements_fall_through(statements: &[Statement]) -> bool {
    verified_flow(statements).falls_through
}

pub(crate) fn verified_match_exhaustive(cases: &[HirMatchCase]) -> bool {
    if cases.last().is_some_and(|case| {
        case.guard.is_none()
            && (case.pattern.is_none()
                || matches!(
                    case.pattern,
                    Some(HirMatchPattern::Binding { .. } | HirMatchPattern::Wildcard)
                ))
    }) {
        return true;
    }
    let mut saw_false = false;
    let mut saw_true = false;
    for case in cases {
        match (&case.pattern, &case.guard) {
            (Some(HirMatchPattern::Literal(Literal::Bool(false))), None) => saw_false = true,
            (Some(HirMatchPattern::Literal(Literal::Bool(true))), None) => saw_true = true,
            _ => return false,
        }
    }
    saw_false && saw_true
}
