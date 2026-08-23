#![forbid(unsafe_code)]

mod compiler;
mod evaluator;
mod identity;
mod model;
mod semantic;
mod syntax;
mod typed_hir;

pub use compiler::{
    AcceptedCompilation, Cancellation, CanonicalValue, CompilationOutcome, CompilationRequest,
    Compiler, CompilerInstallation, ConstructionObservation, Defect, Diagnostic, DiagnosticLabel,
    DiagnosticLabelRole, DiagnosticValue, EvaluationObservation, EvaluationOutcome,
    EvaluationReceipt, FunctionFactsObservation, HostFailure, IdentityDomain, IdentityObservation,
    IdentityOrigin, InferredErrorObservation, InspectSelection, Inspection, OpenError,
    OwnershipMode, OwnershipObservation, ProjectFile, ProjectSnapshot, RecoveryAction,
    RejectedCompilation, Root, SourceRange, SpecializationObservation, SyntaxElement,
    SyntaxElementKind, SyntaxNodeObservation, SyntaxObservation, TestApplicationObservation,
    TestBindingObservation, TypeObservation, TypeRole,
};
