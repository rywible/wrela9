#![forbid(unsafe_code)]

mod architecture_planning;
#[cfg(test)]
mod architecture_planning_consumer_tests;
mod compiler;
mod completed_semantic;
mod control_flow;
mod core;
mod distribution;
mod evaluator;
mod flow;
mod graph;
mod identity;
mod image_evaluation;
mod image_planning;
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
pub use core::{
    CoreAccessLaw, CoreCustodianEffect, CoreCustodyEffectObservation, CoreCustodyOperation,
    CoreExecutableKind, CoreExecutableObservation, CoreInitializationEffect, CoreLoanEffect,
    CoreObligationEffect, CoreOperationKind, CoreParameterObservation, CoreProgramObservation,
    CoreRewriteKind,
};
pub use flow::{
    FlowActorObservation, FlowAdmissionKind, FlowCustodian, FlowDeadlineClass,
    FlowDeadlineLawObservation, FlowEventKind, FlowGroupObservation, FlowGroupPolicy,
    FlowGroupPolicyLawObservation, FlowProgramObservation, FlowProposalKey,
    FlowProposalObservation, FlowReplyObligationObservation, FlowRequirementKind,
    FlowRequirementObservation, FlowSendOutcome, FlowStructuredEventObservation,
    FlowStructuredOutcome, FlowStructuredScenarioKind, FlowStructuredScenarioObservation,
    FlowSuspensionHomeObservation, FlowTraceRecord,
};
pub use image_planning::{
    BindingAssignmentObservation, DischargeKind, DomainPlanKind, DomainPlanObservation,
    ExecutableDemandObservation, ExecutablePlacementObservation, FacilityActorRef,
    FacilityBindingAvailability, FacilityContractObservation, FacilityDomainPlanObservation,
    FacilityEndpointOwnership, FacilityFlagshipRule, FacilityKind, FacilityLossPolicy,
    FacilityReplayAuthority, FacilityReplayRule, FacilitySemanticCapacity, FacilitySharedRole,
    FacilitySharing, FacilityShutdown, GeneratedRoleKind, GeneratedRoleObservation, LayoutCostKind,
    LayoutCostObservation, LayoutLocalKey, LogicalAllocationObservation,
    LogicalImageLayoutObservation, LogicalLifetime, LogicalProtection, LogicalRegionKind,
    LogicalRegionObservation, LogicalReservationKind, LogicalReservationObservation, PlannerKind,
    PlannerObservation, PlanningBinding, PlanningCapability, PlanningFoundationObservation,
    PlanningMultiplicity, PlanningReservation, PoolAdmissionEvidenceObservation,
    PoolModelObservation, PoolObservation, RequirementBounds, RequirementCategory,
    RequirementDischargeObservation, RequirementObservation, RequirementProvenance,
    RequirementSource, RequirementSubject, ServiceClassKind, ServiceClassObservation,
    ServiceCoreObservation, ServicePlanObservation, StorageEnvelopeKind,
    WholeImageAssignmentObservation,
};
pub use typed_hir::PoolOperation;
