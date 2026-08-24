//! Shared termination and match-coverage proofs.
//!
//! Syntax and verified HIR intentionally remain separate representations. This
//! module is the single owner of the proof rules and exposes one adapter for
//! each representation so callers cannot silently drift apart.

use crate::syntax::{
    ExpressionSyntax, ExpressionSyntaxKind, MatchCaseSyntax, PatternSyntaxKind, StatementSyntax,
};
use crate::typed_hir::{HirMatchCase, HirMatchPattern, Literal, Statement};

pub(crate) fn syntax_statements_terminate(statements: &[StatementSyntax]) -> bool {
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
        StatementSyntax::Comptime { branches, .. } => {
            branches
                .last()
                .is_some_and(|branch| branch.condition.is_none())
                && branches
                    .iter()
                    .all(|branch| syntax_statements_terminate(&branch.statements))
        }
        StatementSyntax::Match { cases, .. } => {
            syntax_match_exhaustive(cases)
                && cases
                    .iter()
                    .all(|case| syntax_statements_terminate(&case.body))
        }
        StatementSyntax::With { body, .. } => syntax_statements_terminate(body),
        StatementSyntax::Assert { .. }
        | StatementSyntax::Assign { .. }
        | StatementSyntax::Expect { .. }
        | StatementSyntax::Evaluate(_)
        | StatementSyntax::For { .. }
        | StatementSyntax::While { .. }
        | StatementSyntax::Break(_)
        | StatementSyntax::Continue(_)
        | StatementSyntax::Defer { .. }
        | StatementSyntax::Unsupported { .. }
        | StatementSyntax::Pass(_) => false,
    })
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

pub(crate) fn verified_statements_terminate(statements: &[Statement]) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Return { .. } | Statement::Panic { .. } => true,
        Statement::If {
            then_branch,
            else_branch,
            ..
        }
        | Statement::IfPattern {
            then_branch,
            else_branch,
            ..
        } => {
            !else_branch.is_empty()
                && verified_statements_terminate(then_branch)
                && verified_statements_terminate(else_branch)
        }
        Statement::Match { cases, .. } => {
            verified_match_exhaustive(cases)
                && cases
                    .iter()
                    .all(|case| verified_statements_terminate(&case.body))
        }
        Statement::WithPool { body, .. } => verified_statements_terminate(body),
        Statement::Assert { .. }
        | Statement::Expect { .. }
        | Statement::Initialize { .. }
        | Statement::Assign { .. }
        | Statement::Evaluate(_)
        | Statement::For { .. }
        | Statement::While { .. }
        | Statement::Break(_)
        | Statement::Continue(_)
        | Statement::Defer { .. }
        | Statement::Pass(_) => false,
    })
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
