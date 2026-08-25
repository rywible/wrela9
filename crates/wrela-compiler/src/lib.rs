#![forbid(unsafe_code)]

mod architecture_planning;
#[cfg(test)]
mod architecture_planning_consumer_tests;
mod compiler;
mod completed_semantic;
mod control_flow;
mod distribution;
mod evaluator;
mod graph;
mod identity;
mod image_evaluation;
mod model;
mod project_closure;
mod semantic;
mod semantic_facts;
mod syntax;
mod type_semantics;
mod typed_hir;

pub use architecture_planning::{ArchitecturePlanningObservation, ArchitectureProfile};
pub use compiler::{
    AcceptedCompilation, Cancellation, CanonicalValue, CompilationOutcome, CompilationRequest,
    Compiler, CompilerInstallation, CompletedSemanticProgramObservation, ConstructionKind,
    ConstructionObservation, ConstructionOperandObservation, Defect, Diagnostic, DiagnosticLabel,
    DiagnosticLabelRole, DiagnosticValue, EvaluationContributorObservation,
    EvaluationFrameObservation, EvaluationLimitPolicy, EvaluationObservation, EvaluationOutcome,
    EvaluationPanicKind, EvaluationPolicy, EvaluationReceipt, EvaluationRejectionKind,
    FunctionFactsObservation, HostFailure, IdentityDomain, IdentityObservation, IdentityOrigin,
    InferredErrorObservation, InspectSelection, Inspection, OpenError, OwnershipMode,
    OwnershipObservation, ProjectFile, ProjectSnapshot, RecoveryAction, RejectedCompilation,
    ResolutionKind, ResolutionObservation, Root, SnapshotDigestMismatch, SourceRange,
    SpecializationObservation, SyntaxElement, SyntaxElementKind, SyntaxErrorKind,
    SyntaxInvalidKind, SyntaxLayoutKind, SyntaxMissingKind, SyntaxNodeKind, SyntaxNodeObservation,
    SyntaxObservation, SyntaxTokenKind, SyntaxTriviaKind, TestApplicationObservation,
    TestBindingObservation, TypeObservation, TypeRole,
};
