#![forbid(unsafe_code)]

mod compiler;
mod evaluator;
mod identity;
mod semantic;
mod syntax;
mod typed_hir;

pub use compiler::{
    AcceptedCompilation, Cancellation, CanonicalValue, CompilationOutcome, CompilationRequest,
    Compiler, CompilerInstallation, ConstructionObservation, Defect, Diagnostic, DiagnosticLabel,
    EvaluationObservation, EvaluationOutcome, EvaluationReceipt, FunctionFactsObservation,
    HostFailure, IdentityDomain, IdentityObservation, InferredErrorObservation, InspectSelection,
    Inspection, OpenError, ProjectFile, ProjectSnapshot, RecoveryAction, RejectedCompilation, Root,
    SourceRange, SyntaxElement, SyntaxElementKind, SyntaxObservation, TestApplicationObservation,
};
