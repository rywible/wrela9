#![forbid(unsafe_code)]

mod compiler;
mod evaluator;
mod graph;
mod identity;
mod model;
mod semantic;
mod syntax;
mod type_semantics;
mod typed_hir;

pub use compiler::{
    AcceptedCompilation, Cancellation, CanonicalValue, CompilationOutcome, CompilationRequest,
    Compiler, CompilerInstallation, ConstructionKind, ConstructionObservation, Defect, Diagnostic,
    DiagnosticLabel, DiagnosticLabelRole, DiagnosticValue, EvaluationLimitPolicy,
    EvaluationObservation, EvaluationOutcome, EvaluationPanicKind, EvaluationPolicy,
    EvaluationReceipt, EvaluationRejectionKind, FunctionFactsObservation, HostFailure,
    IdentityDomain, IdentityObservation, IdentityOrigin, InferredErrorObservation,
    InspectSelection, Inspection, OpenError, OwnershipMode, OwnershipObservation, ProjectFile,
    ProjectSnapshot, RecoveryAction, RejectedCompilation, Root, SourceRange,
    SpecializationObservation, SyntaxElement, SyntaxElementKind, SyntaxErrorKind,
    SyntaxInvalidKind, SyntaxLayoutKind, SyntaxMissingKind, SyntaxNodeKind, SyntaxNodeObservation,
    SyntaxObservation, SyntaxTokenKind, SyntaxTriviaKind, TestApplicationObservation,
    TestBindingObservation, TypeObservation, TypeRole,
};
