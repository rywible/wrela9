#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use xxhash_rust::xxh3::Xxh3;

use crate::compiler::{
    Cancellation, CompletedSemanticProgramObservation, CompletedSemanticProgramValues, Root,
};
use crate::evaluator::{Construction, ConstructionOperand, Engine, SemanticEvaluation, Value};
use crate::identity::IdentityCatalog;
use crate::image_evaluation::SealedImage;
use crate::model::{BuildKind, DefinitionId, SpecializationId};
use crate::semantic_facts::{FunctionFacts, SolvedSemanticFacts};
use crate::typed_hir::{AccessMode, VerifiedProgram};

pub(crate) const PHASE_SCHEMA: &str = "wrela.completed-semantic-program.v3";
const COMPILER_SCHEMA: &str = "wrela.compiler.semantic-context.v1";
const SEMANTIC_SCHEMA: &str = "wrela.semantic-authority.v1";

#[derive(Clone, Copy)]
pub(crate) struct ContextInput {
    pub(crate) distribution_digest: u128,
    pub(crate) semantic_closure_digest: u128,
    pub(crate) root: Root,
}

pub(crate) struct CompletionInput {
    pub(crate) context: ContextInput,
    pub(crate) identity_catalog: Arc<IdentityCatalog>,
    pub(crate) program: Arc<VerifiedProgram>,
    pub(crate) facts: Arc<SolvedSemanticFacts>,
    pub(crate) evaluations: Vec<SemanticEvaluation>,
    pub(crate) image: SealedImage,
    pub(crate) image_specialization: SpecializationId,
    pub(crate) actors: Vec<ActorInput>,
}

#[derive(Clone)]
pub(crate) struct ActorInput {
    pub(crate) definition: DefinitionId,
    pub(crate) source: crate::SourceRange,
    pub(crate) handlers: Arc<[DefinitionId]>,
}

pub(crate) enum CompletionFailure {
    Cancelled,
    Defect(Arc<str>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ArtifactKind {
    TypedProgram,
    SolvedFacts,
    EvaluationTable,
    ConstructionGraph,
    ExecutableDemand,
    ConstructionNode,
    Specialization,
    TestBody,
    ClosureBody,
    GeneratedExecutable,
    ConstantEvaluationRoot,
    ConditionEvaluationRoot,
    ImageEvaluationRoot,
}

impl ArtifactKind {
    const fn tag(self) -> u8 {
        match self {
            Self::TypedProgram => 1,
            Self::SolvedFacts => 2,
            Self::EvaluationTable => 3,
            Self::ConstructionGraph => 4,
            Self::ExecutableDemand => 5,
            Self::ConstructionNode => 6,
            Self::Specialization => 7,
            Self::TestBody => 8,
            Self::ClosureBody => 9,
            Self::GeneratedExecutable => 10,
            Self::ConstantEvaluationRoot => 11,
            Self::ConditionEvaluationRoot => 12,
            Self::ImageEvaluationRoot => 13,
        }
    }
}

#[derive(Clone, Debug)]
struct CompilationContext {
    identity: u128,
    distribution_digest: u128,
    semantic_closure_digest: u128,
    identity_catalog_revision: u128,
    root: Root,
    identity_catalog: Arc<IdentityCatalog>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RawReference {
    context: u128,
    kind: ArtifactKind,
    identity: u128,
    current_meaning: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TypedReference<K> {
    raw: RawReference,
    _kind: PhantomData<fn() -> K>,
}

impl<K> TypedReference<K> {
    fn new(context: u128, kind: ArtifactKind, identity: u128, current_meaning: u128) -> Self {
        Self {
            raw: RawReference {
                context,
                kind,
                identity,
                current_meaning,
            },
            _kind: PhantomData,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProgramAuthority;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FactsAuthority;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EvaluationAuthority;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GraphAuthority;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DemandAuthority;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ConstructionNodeAuthority;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SpecializationAuthority;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TestBodyAuthority;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ClosureBodyAuthority;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GeneratedExecutableAuthority;

#[derive(Clone, Debug, PartialEq, Eq)]
struct DirectReceipts {
    program: TypedReference<ProgramAuthority>,
    facts: TypedReference<FactsAuthority>,
    evaluations: TypedReference<EvaluationAuthority>,
    graph: TypedReference<GraphAuthority>,
    demand: TypedReference<DemandAuthority>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GraphEdge {
    label: Arc<str>,
    ordinal: u32,
    ownership: AccessMode,
    target_kind: BuildKind,
    target: TypedReference<ConstructionNodeAuthority>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GraphNode {
    identity: u128,
    kind: BuildKind,
    owner: TypedReference<SpecializationAuthority>,
    site: crate::SourceRange,
    operands: Arc<[ConstructionOperand]>,
    local_fingerprint: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SealedConstructionGraph {
    root: TypedReference<ConstructionNodeAuthority>,
    nodes: Arc<[GraphNode]>,
    test_applications: Arc<[crate::evaluator::AppliedTest]>,
    fingerprint: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ExecutableDemand {
    root: TypedReference<SpecializationAuthority>,
    executables: Arc<[ExecutableReference]>,
    fingerprint: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExecutableReference {
    Specialization(TypedReference<SpecializationAuthority>),
    TestBody(TypedReference<TestBodyAuthority>),
    ClosureBody(TypedReference<ClosureBodyAuthority>),
    #[allow(dead_code)]
    // The closed family authenticates generated roles even when this phase emits none.
    Generated(TypedReference<GeneratedExecutableAuthority>),
}

impl ExecutableReference {
    const fn raw(self) -> RawReference {
        match self {
            Self::Specialization(reference) => reference.raw,
            Self::TestBody(reference) => reference.raw,
            Self::ClosureBody(reference) => reference.raw,
            Self::Generated(reference) => reference.raw,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CoreSourceExecutableKind {
    Specialization,
    TestBody,
    ClosureBody,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CoreSourceExecutableRef {
    context: u128,
    kind: CoreSourceExecutableKind,
    identity: u128,
    current_meaning: u128,
}

#[allow(
    dead_code,
    reason = "crate-private handoff reserved for the Core planner"
)]
impl CoreSourceExecutableRef {
    pub(crate) const fn context(self) -> u128 {
        self.context
    }

    pub(crate) const fn kind(self) -> CoreSourceExecutableKind {
        self.kind
    }

    pub(crate) const fn identity(self) -> u128 {
        self.identity
    }

    pub(crate) const fn current_meaning(self) -> u128 {
        self.current_meaning
    }
}

#[derive(Clone, Debug)]
struct VerifiedMarker;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EvaluationReference {
    root: crate::typed_hir::EvaluationRoot,
    raw: RawReference,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CompletedEvaluation {
    authority: SemanticEvaluation,
    root: EvaluationReference,
    dependencies: Arc<[EvaluationReference]>,
}

#[derive(Clone)]
pub(crate) struct CompletedSemanticProgram {
    context: Arc<CompilationContext>,
    program: Arc<VerifiedProgram>,
    facts: Arc<SolvedSemanticFacts>,
    evaluations: Arc<[CompletedEvaluation]>,
    graph: SealedConstructionGraph,
    demand: ExecutableDemand,
    actors: Arc<[CompletedActor]>,
    receipts: DirectReceipts,
    custody_fingerprint: u128,
    fingerprint: u128,
    _verified: VerifiedMarker,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CompletedActor {
    identity: u128,
    source: crate::SourceRange,
    handlers: Arc<[SpecializationId]>,
}

impl fmt::Debug for CompletedSemanticProgram {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CompletedSemanticProgram")
            .field("context", &self.context.identity)
            .field("fingerprint", &format_args!("{:032x}", self.fingerprint))
            .finish_non_exhaustive()
    }
}

impl PartialEq for CompletedSemanticProgram {
    fn eq(&self, other: &Self) -> bool {
        self.context.identity == other.context.identity && self.fingerprint == other.fingerprint
    }
}

impl Eq for CompletedSemanticProgram {}

impl CompletedSemanticProgram {
    pub(crate) const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }

    pub(crate) fn observation(&self) -> CompletedSemanticProgramObservation {
        CompletedSemanticProgramObservation::new(CompletedSemanticProgramValues {
            fingerprint: self.fingerprint,
            context_identity: self.context.identity,
            typed_program_fingerprint: self.program.fingerprint(),
            identity_catalog_revision: self.context.identity_catalog_revision,
            custody_fingerprint: self.custody_fingerprint,
            construction_graph_fingerprint: self.graph.fingerprint,
            executable_demand_fingerprint: self.demand.fingerprint,
            solved_specialization_count: self.facts.specializations.len(),
            evaluation_count: self.evaluations.len(),
            construction_count: self.graph.nodes.len(),
            executable_count: self.demand.executables.len(),
        })
    }

    #[allow(dead_code)]
    pub(crate) fn typed_program(&self) -> &VerifiedProgram {
        &self.program
    }

    #[allow(dead_code)]
    pub(crate) fn solved_facts(&self) -> &SolvedSemanticFacts {
        &self.facts
    }

    #[allow(dead_code)]
    pub(crate) fn semantic_evaluations(
        &self,
    ) -> impl ExactSizeIterator<Item = &SemanticEvaluation> {
        self.evaluations
            .iter()
            .map(|evaluation| &evaluation.authority)
    }

    #[allow(dead_code)]
    pub(crate) fn sealed_constructions(
        &self,
    ) -> impl ExactSizeIterator<
        Item = (
            u128,
            BuildKind,
            SpecializationId,
            &crate::SourceRange,
            &[ConstructionOperand],
        ),
    > {
        self.graph.nodes.iter().map(|node| {
            (
                node.identity,
                node.kind,
                SpecializationId(node.owner.raw.identity),
                &node.site,
                node.operands.as_ref(),
            )
        })
    }

    #[allow(dead_code)]
    pub(crate) fn applied_tests(&self) -> &[crate::evaluator::AppliedTest] {
        &self.graph.test_applications
    }

    #[allow(dead_code)]
    pub(crate) fn executable_specializations(&self) -> impl Iterator<Item = SpecializationId> + '_ {
        self.demand
            .executables
            .iter()
            .filter_map(|reference| match reference {
                ExecutableReference::Specialization(reference) => {
                    Some(SpecializationId(reference.raw.identity))
                }
                _ => None,
            })
    }

    pub(crate) fn for_image_planning(&self) -> ImagePlanningSemanticProgram<'_> {
        ImagePlanningSemanticProgram { program: self }
    }

    pub(crate) fn for_core_planning(&self) -> CorePlanningSemanticProgram<'_> {
        CorePlanningSemanticProgram { program: self }
    }

    pub(crate) fn for_flow_planning(&self) -> FlowPlanningSemanticProgram<'_> {
        FlowPlanningSemanticProgram { program: self }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct FlowPlanningSemanticProgram<'a> {
    program: &'a CompletedSemanticProgram,
}

impl<'a> FlowPlanningSemanticProgram<'a> {
    pub(crate) fn actors(self) -> impl ExactSizeIterator<Item = FlowActorInput<'a>> {
        self.program
            .actors
            .iter()
            .map(|actor| FlowActorInput { actor })
    }

    pub(crate) fn context_identity(self) -> u128 {
        self.program.context.identity
    }
}

#[derive(Clone, Copy)]
pub(crate) struct FlowActorInput<'a> {
    actor: &'a CompletedActor,
}

impl<'a> FlowActorInput<'a> {
    pub(crate) const fn identity(self) -> u128 {
        self.actor.identity
    }

    pub(crate) fn source(self) -> &'a crate::SourceRange {
        &self.actor.source
    }

    pub(crate) fn handlers(self) -> impl ExactSizeIterator<Item = u128> + 'a {
        self.actor.handlers.iter().map(|handler| handler.0)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct CorePlanningSemanticProgram<'a> {
    program: &'a CompletedSemanticProgram,
}

#[allow(dead_code)]
impl<'a> CorePlanningSemanticProgram<'a> {
    pub(crate) fn context_identity(self) -> u128 {
        self.program.context.identity
    }

    pub(crate) const fn fingerprint(self) -> u128 {
        self.program.fingerprint
    }

    pub(crate) fn exact_source_executables(
        self,
    ) -> impl ExactSizeIterator<Item = CoreSourceExecutableRef> + 'a {
        self.program
            .demand
            .executables
            .iter()
            .copied()
            .map(core_source_executable_reference)
    }

    pub(crate) fn executable_input(
        self,
        reference: CoreSourceExecutableRef,
    ) -> Option<CoreSourceExecutableInput<'a>> {
        if reference.context != self.program.context.identity {
            return None;
        }
        let body = match reference.kind {
            CoreSourceExecutableKind::Specialization => {
                let identity = SpecializationId(reference.identity);
                CoreSourceExecutableBody::Specialization(
                    self.program.program.specialization_function(identity)?,
                )
            }
            CoreSourceExecutableKind::TestBody => {
                let id = self
                    .program
                    .graph
                    .test_applications
                    .iter()
                    .find(|test| test.id.identity == reference.identity)?
                    .id;
                CoreSourceExecutableBody::Test(self.program.program.test_body_with_signature(id)?)
            }
            CoreSourceExecutableKind::ClosureBody => CoreSourceExecutableBody::Closure(
                self.program
                    .program
                    .closure(crate::typed_hir::ClosureId(reference.identity))?,
            ),
        };
        Some(CoreSourceExecutableInput { reference, body })
    }

    pub(crate) fn specialization_facts(
        self,
        identity: SpecializationId,
    ) -> Option<&'a FunctionFacts> {
        self.program.facts.specializations.get(&identity)
    }

    pub(crate) fn verified_program(self) -> &'a VerifiedProgram {
        &self.program.program
    }
}

#[derive(Clone, Copy)]
pub(crate) struct CoreSourceExecutableInput<'a> {
    pub(crate) reference: CoreSourceExecutableRef,
    pub(crate) body: CoreSourceExecutableBody<'a>,
}

#[derive(Clone, Copy)]
pub(crate) enum CoreSourceExecutableBody<'a> {
    Specialization(&'a crate::typed_hir::HirFunction),
    Test(&'a crate::typed_hir::HirTest),
    Closure(&'a crate::typed_hir::HirClosure),
}

fn core_source_executable_reference(reference: ExecutableReference) -> CoreSourceExecutableRef {
    let (kind, raw) = match reference {
        ExecutableReference::Specialization(reference) => {
            (CoreSourceExecutableKind::Specialization, reference.raw)
        }
        ExecutableReference::TestBody(reference) => {
            (CoreSourceExecutableKind::TestBody, reference.raw)
        }
        ExecutableReference::ClosureBody(reference) => {
            (CoreSourceExecutableKind::ClosureBody, reference.raw)
        }
        ExecutableReference::Generated(_) => {
            unreachable!("verified Completed Semantic Program source demand cannot be generated")
        }
    };
    CoreSourceExecutableRef {
        context: raw.context,
        kind,
        identity: raw.identity,
        current_meaning: raw.current_meaning,
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ImagePlanningSemanticProgram<'a> {
    program: &'a CompletedSemanticProgram,
}

impl ImagePlanningSemanticProgram<'_> {
    pub(crate) fn context_identity(self) -> u128 {
        self.program.context.identity
    }

    pub(crate) fn distribution_digest(self) -> u128 {
        self.program.context.distribution_digest
    }

    pub(crate) fn root(self) -> Root {
        self.program.context.root
    }

    pub(crate) const fn fingerprint(self) -> u128 {
        self.program.fingerprint
    }

    pub(crate) const fn construction_graph_fingerprint(self) -> u128 {
        self.program.graph.fingerprint
    }

    pub(crate) const fn executable_demand_fingerprint(self) -> u128 {
        self.program.demand.fingerprint
    }

    pub(crate) const fn custody_fingerprint(self) -> u128 {
        self.program.custody_fingerprint
    }

    pub(crate) fn source_executable_count(self) -> usize {
        self.program.demand.executables.len()
    }

    pub(crate) fn test_application_count(self) -> usize {
        self.program.graph.test_applications.len()
    }
}

pub(crate) fn complete(
    input: CompletionInput,
    cancellation: &Cancellation,
) -> Result<CompletedSemanticProgram, CompletionFailure> {
    if cancellation.is_cancelled() {
        return Err(CompletionFailure::Cancelled);
    }
    let identity_catalog_revision = input.identity_catalog.revision_fingerprint();
    let context_identity = produce_context_identity(input.context, identity_catalog_revision);
    checkpoint(cancellation)?;
    let context = Arc::new(CompilationContext {
        identity: context_identity,
        distribution_digest: input.context.distribution_digest,
        semantic_closure_digest: input.context.semantic_closure_digest,
        identity_catalog_revision,
        root: input.context.root,
        identity_catalog: input.identity_catalog,
    });

    let program_fingerprint = input.program.fingerprint();
    let facts_fingerprint = produce_facts_fingerprint(&input.facts, cancellation)?;
    checkpoint(cancellation)?;
    let mut evaluations = Vec::with_capacity(input.evaluations.len());
    for authority in input.evaluations {
        checkpoint(cancellation)?;
        let root = evaluation_reference(context_identity, program_fingerprint, authority.root);
        let mut dependencies = Vec::with_capacity(authority.dependencies.len());
        for dependency in authority.dependencies.iter().copied() {
            checkpoint(cancellation)?;
            dependencies.push(evaluation_reference(
                context_identity,
                program_fingerprint,
                dependency,
            ));
        }
        evaluations.push(CompletedEvaluation {
            authority,
            root,
            dependencies: dependencies.into(),
        });
    }
    evaluations.sort_by_key(|evaluation| evaluation.authority.root);
    let evaluations_fingerprint = produce_evaluations_fingerprint(&evaluations, cancellation)?;
    checkpoint(cancellation)?;
    let actors = complete_actors(&input.actors, &input.program, cancellation)?;
    let demand = produce_demand(
        DemandInput {
            context: context_identity,
            root: input.image_specialization,
            program: &input.program,
            facts: &input.facts,
            image: &input.image,
            test_applications: &input.image.test_applications,
            actors: &actors,
        },
        cancellation,
    )?;
    let graph = produce_graph(
        context_identity,
        &input.image,
        &demand,
        program_fingerprint,
        cancellation,
    )?;
    checkpoint(cancellation)?;
    let receipts = DirectReceipts {
        program: TypedReference::new(
            context_identity,
            ArtifactKind::TypedProgram,
            program_fingerprint,
            program_fingerprint,
        ),
        facts: TypedReference::new(
            context_identity,
            ArtifactKind::SolvedFacts,
            facts_fingerprint,
            facts_fingerprint,
        ),
        evaluations: TypedReference::new(
            context_identity,
            ArtifactKind::EvaluationTable,
            evaluations_fingerprint,
            evaluations_fingerprint,
        ),
        graph: TypedReference::new(
            context_identity,
            ArtifactKind::ConstructionGraph,
            graph.fingerprint,
            graph.fingerprint,
        ),
        demand: TypedReference::new(
            context_identity,
            ArtifactKind::ExecutableDemand,
            demand.fingerprint,
            demand.fingerprint,
        ),
    };
    let custody_fingerprint = input.program.custody_fingerprint();
    let fingerprint = produce_completed_fingerprint(
        context_identity,
        &receipts,
        custody_fingerprint,
        cancellation,
    )?;
    checkpoint(cancellation)?;
    let candidate = CompletedSemanticProgram {
        context,
        program: input.program,
        facts: input.facts,
        evaluations: evaluations.into(),
        graph,
        demand,
        actors: actors.into(),
        receipts,
        custody_fingerprint,
        fingerprint,
        _verified: VerifiedMarker,
    };
    verify(&candidate, cancellation)?;
    Ok(candidate)
}

fn verify(
    candidate: &CompletedSemanticProgram,
    cancellation: &Cancellation,
) -> Result<(), CompletionFailure> {
    if cancellation.is_cancelled() {
        return Err(CompletionFailure::Cancelled);
    }
    let expected_context = verify_context_identity(
        ContextInput {
            distribution_digest: candidate.context.distribution_digest,
            semantic_closure_digest: candidate.context.semantic_closure_digest,
            root: candidate.context.root,
        },
        candidate.context.identity_catalog.revision_fingerprint(),
    );
    if candidate.context.identity != expected_context
        || candidate.context.identity_catalog_revision
            != candidate.context.identity_catalog.revision_fingerprint()
    {
        return defect("false compilation context or Identity Catalog revision");
    }
    if candidate.program.identity_catalog_revision() != candidate.context.identity_catalog_revision
    {
        return defect("verified Typed Program belongs to a different Identity Catalog revision");
    }

    let facts_fingerprint = verify_facts_fingerprint(&candidate.facts, cancellation)?;
    checkpoint(cancellation)?;
    let evaluations_fingerprint =
        verify_evaluations_fingerprint(&candidate.evaluations, cancellation)?;
    checkpoint(cancellation)?;
    let registry = BTreeMap::from([
        (ArtifactKind::TypedProgram, candidate.program.fingerprint()),
        (ArtifactKind::SolvedFacts, facts_fingerprint),
        (ArtifactKind::EvaluationTable, evaluations_fingerprint),
        (ArtifactKind::ConstructionGraph, candidate.graph.fingerprint),
        (ArtifactKind::ExecutableDemand, candidate.demand.fingerprint),
    ]);
    for (reference, kind) in [
        (candidate.receipts.program.raw, ArtifactKind::TypedProgram),
        (candidate.receipts.facts.raw, ArtifactKind::SolvedFacts),
        (
            candidate.receipts.evaluations.raw,
            ArtifactKind::EvaluationTable,
        ),
        (
            candidate.receipts.graph.raw,
            ArtifactKind::ConstructionGraph,
        ),
        (
            candidate.receipts.demand.raw,
            ArtifactKind::ExecutableDemand,
        ),
    ] {
        verify_reference(reference, candidate.context.identity, kind, &registry)?;
    }
    if candidate.custody_fingerprint != candidate.program.custody_fingerprint() {
        return defect("Resource custody receipt disagrees with verified Typed Program");
    }
    verify_fact_coverage(&candidate.program, &candidate.facts, cancellation)?;
    verify_actors(candidate, cancellation)?;
    verify_evaluation_coverage(
        &candidate.program,
        SpecializationId(candidate.demand.root.raw.identity),
        &candidate.evaluations,
        candidate.context.identity,
        cancellation,
    )?;
    verify_demand(candidate, cancellation)?;
    verify_graph(candidate, cancellation)?;

    let reconstructed = verify_completed_fingerprint(
        candidate.context.identity,
        &candidate.receipts,
        candidate.custody_fingerprint,
        cancellation,
    )?;
    checkpoint(cancellation)?;
    if reconstructed != candidate.fingerprint {
        return defect("Completed Semantic Program fingerprint is false");
    }
    Ok(())
}

fn checkpoint(cancellation: &Cancellation) -> Result<(), CompletionFailure> {
    if cancellation.is_cancelled() {
        Err(CompletionFailure::Cancelled)
    } else {
        Ok(())
    }
}

fn verify_reference(
    reference: RawReference,
    context: u128,
    expected_kind: ArtifactKind,
    registry: &BTreeMap<ArtifactKind, u128>,
) -> Result<(), CompletionFailure> {
    if reference.context != context {
        return defect("typed artifact reference crosses compilation contexts");
    }
    if reference.kind != expected_kind {
        return defect("typed artifact reference has the wrong kind");
    }
    let Some(current) = registry.get(&reference.kind) else {
        return defect("typed artifact reference names a missing authority");
    };
    if reference.identity != *current || reference.current_meaning != *current {
        return defect("typed artifact reference is stale");
    }
    Ok(())
}

fn complete_actors(
    inputs: &[ActorInput],
    program: &VerifiedProgram,
    cancellation: &Cancellation,
) -> Result<Vec<CompletedActor>, CompletionFailure> {
    let mut actors = Vec::with_capacity(inputs.len());
    let mut identities = BTreeSet::new();
    for input in inputs {
        checkpoint(cancellation)?;
        if !identities.insert(input.definition.0) {
            return defect("Actor identity is duplicated");
        }
        let mut handlers = Vec::with_capacity(input.handlers.len());
        for definition in input.handlers.iter().copied() {
            checkpoint(cancellation)?;
            let Some(specialization) = program.default_specialization(definition) else {
                return defect("Actor handler has no concrete Specialization");
            };
            let Some(function) = program.specialization_function(specialization) else {
                return defect("Actor handler Specialization is missing");
            };
            if function.modifier != crate::syntax::FunctionModifier::Async {
                return defect("Actor message handler is not async");
            }
            handlers.push(specialization);
        }
        handlers.sort();
        handlers.dedup();
        actors.push(CompletedActor {
            identity: input.definition.0,
            source: input.source.clone(),
            handlers: handlers.into(),
        });
    }
    actors.sort_by_key(|actor| actor.identity);
    Ok(actors)
}

struct DemandInput<'a> {
    context: u128,
    root: SpecializationId,
    program: &'a VerifiedProgram,
    facts: &'a SolvedSemanticFacts,
    image: &'a SealedImage,
    test_applications: &'a [crate::evaluator::AppliedTest],
    actors: &'a [CompletedActor],
}

fn produce_demand(
    input: DemandInput<'_>,
    cancellation: &Cancellation,
) -> Result<ExecutableDemand, CompletionFailure> {
    let DemandInput {
        context,
        root,
        program,
        facts,
        image,
        test_applications,
        actors,
    } = input;
    if !program.specializations().contains_key(&root) {
        return defect("Image Constructor has no concrete Specialization");
    }
    let mut roots = BTreeSet::new();
    for construction in image.constructions() {
        checkpoint(cancellation)?;
        roots.insert(SpecializationId(construction.owner));
    }
    let mut retained_closures = BTreeSet::new();
    for construction in image.constructions() {
        for operand in &construction.operands {
            if !crate::evaluator::visit_retained_executables(
                &operand.value,
                cancellation,
                &mut |executable| match executable {
                    crate::evaluator::RetainedExecutable::Function(identity) => {
                        roots.insert(identity);
                    }
                    crate::evaluator::RetainedExecutable::Closure(identity) => {
                        retained_closures.insert(identity);
                    }
                },
            ) {
                return Err(CompletionFailure::Cancelled);
            }
        }
    }
    roots.insert(root);
    for actor in actors {
        checkpoint(cancellation)?;
        roots.extend(actor.handlers.iter().copied());
    }
    for application in test_applications {
        if cancellation.is_cancelled() {
            return Err(CompletionFailure::Cancelled);
        }
        let Some(test_roots) =
            crate::semantic_facts::test_specialization_demands(program, application.id)
        else {
            return defect("Test Application names a missing verified Test body");
        };
        roots.extend(test_roots);
    }
    let identities = reachable_specializations(&roots, facts, cancellation)?;
    let mut executables = Vec::new();
    for identity in &identities {
        checkpoint(cancellation)?;
        executables.push(ExecutableReference::Specialization(TypedReference::new(
            context,
            ArtifactKind::Specialization,
            identity.0,
            specialization_current_meaning(program.fingerprint(), *identity),
        )));
    }
    let mut applied_tests = BTreeSet::new();
    for application in test_applications {
        checkpoint(cancellation)?;
        applied_tests.insert(application.id);
    }
    for test in &applied_tests {
        checkpoint(cancellation)?;
        executables.push(ExecutableReference::TestBody(TypedReference::new(
            context,
            ArtifactKind::TestBody,
            test.identity,
            executable_current_meaning(
                program.fingerprint(),
                ArtifactKind::TestBody,
                test.identity,
            ),
        )));
    }
    let mut closures = retained_closures;
    for specialization in &identities {
        checkpoint(cancellation)?;
        closures.extend(program.specialization_closures(*specialization));
    }
    for test in &applied_tests {
        checkpoint(cancellation)?;
        closures.extend(program.test_closures(*test));
    }
    for closure in &closures {
        checkpoint(cancellation)?;
        executables.push(ExecutableReference::ClosureBody(TypedReference::new(
            context,
            ArtifactKind::ClosureBody,
            closure.0,
            executable_current_meaning(program.fingerprint(), ArtifactKind::ClosureBody, closure.0),
        )));
    }
    executables.sort_by_key(|reference| {
        let raw = reference.raw();
        (raw.kind, raw.identity)
    });
    let root_reference = executables
        .iter()
        .find_map(|reference| match reference {
            ExecutableReference::Specialization(reference) if reference.raw.identity == root.0 => {
                Some(*reference)
            }
            _ => None,
        })
        .ok_or_else(|| CompletionFailure::Defect(Arc::from("demand omitted its root")))?;
    let fingerprint = produce_demand_fingerprint(root_reference, &executables, cancellation)?;
    Ok(ExecutableDemand {
        root: root_reference,
        executables: executables.into(),
        fingerprint,
    })
}

fn reachable_specializations(
    roots: &BTreeSet<SpecializationId>,
    facts: &SolvedSemanticFacts,
    cancellation: &Cancellation,
) -> Result<BTreeSet<SpecializationId>, CompletionFailure> {
    let mut reachable = BTreeSet::new();
    let mut pending = roots.iter().rev().copied().collect::<Vec<_>>();
    while let Some(identity) = pending.pop() {
        checkpoint(cancellation)?;
        if !reachable.insert(identity) {
            continue;
        }
        let Some(function) = facts.specializations.get(&identity) else {
            return defect("Executable Demand names a Specialization without solved facts");
        };
        pending.extend(function.specialization_calls.keys().rev().copied());
    }
    Ok(reachable)
}

fn produce_graph(
    context: u128,
    image: &SealedImage,
    demand: &ExecutableDemand,
    program_fingerprint: u128,
    cancellation: &Cancellation,
) -> Result<SealedConstructionGraph, CompletionFailure> {
    let mut demand_set = BTreeSet::new();
    for reference in demand.executables.iter() {
        checkpoint(cancellation)?;
        if let ExecutableReference::Specialization(reference) = reference {
            demand_set.insert(reference.raw.identity);
        }
    }
    let mut local_fingerprints = BTreeMap::new();
    for construction in image.constructions() {
        if cancellation.is_cancelled() {
            return Err(CompletionFailure::Cancelled);
        }
        let local = produce_node_local_fingerprint(construction, cancellation)?;
        if local_fingerprints
            .insert(construction.identity, local)
            .is_some()
        {
            return defect("construction graph repeats an identity");
        }
    }
    let mut nodes = Vec::new();
    for construction in image.constructions() {
        if cancellation.is_cancelled() {
            return Err(CompletionFailure::Cancelled);
        }
        if !demand_set.contains(&construction.owner) {
            return defect("construction owner is outside Executable Demand");
        }
        let owner_meaning = specialization_current_meaning(
            program_fingerprint,
            SpecializationId(construction.owner),
        );
        nodes.push(GraphNode {
            identity: construction.identity,
            kind: construction.kind,
            owner: TypedReference::new(
                context,
                ArtifactKind::Specialization,
                construction.owner,
                owner_meaning,
            ),
            site: construction.site.clone(),
            operands: construction.operands.clone().into(),
            local_fingerprint: local_fingerprints[&construction.identity],
        });
    }
    nodes.sort_by_key(|node| node.identity);
    let mut derived_tests = Vec::new();
    for node in &nodes {
        for operand in node.operands.iter() {
            if !crate::evaluator::visit_test_applications_cancellable(
                &operand.value,
                cancellation,
                &mut |id, payload| {
                    derived_tests.push(crate::evaluator::AppliedTest {
                        id,
                        payload: payload.to_vec(),
                    });
                },
            ) {
                return Err(CompletionFailure::Cancelled);
            }
        }
    }
    if derived_tests != image.test_applications {
        return defect("Test Application bookkeeping disagrees with typed construction operands");
    }
    for node in &nodes {
        let _ = reconstruct_edges(context, &node.operands, &local_fingerprints, cancellation)?;
    }
    let root_fingerprint = local_fingerprints
        .get(&image.root())
        .copied()
        .ok_or_else(|| CompletionFailure::Defect(Arc::from("graph root is missing")))?;
    let root = TypedReference::new(
        context,
        ArtifactKind::ConstructionNode,
        image.root(),
        root_fingerprint,
    );
    let fingerprint = produce_graph_fingerprint(root, &nodes, &derived_tests, cancellation)?;
    Ok(SealedConstructionGraph {
        root,
        nodes: nodes.into(),
        test_applications: derived_tests.into(),
        fingerprint,
    })
}

fn collect_operand_edges(
    context: u128,
    operand: &ConstructionOperand,
    local_fingerprints: &BTreeMap<u128, u128>,
    edges: &mut Vec<GraphEdge>,
    cancellation: &Cancellation,
) -> Result<(), CompletionFailure> {
    let mut handles = Vec::new();
    if !crate::evaluator::visit_construction_handles_cancellable(
        &operand.value,
        cancellation,
        &mut |kind, identity| handles.push((kind, identity)),
    ) {
        return Err(CompletionFailure::Cancelled);
    }
    for (kind, identity) in handles {
        let current = local_fingerprints.get(&identity).copied().ok_or_else(|| {
            CompletionFailure::Defect(Arc::from("graph handle names a missing node"))
        })?;
        edges.push(GraphEdge {
            label: Arc::clone(&operand.label),
            ordinal: u32::try_from(edges.len()).unwrap_or(u32::MAX),
            ownership: operand.ownership,
            target_kind: kind,
            target: TypedReference::new(context, ArtifactKind::ConstructionNode, identity, current),
        });
    }
    Ok(())
}

fn verify_fact_coverage(
    program: &VerifiedProgram,
    facts: &SolvedSemanticFacts,
    cancellation: &Cancellation,
) -> Result<(), CompletionFailure> {
    if !facts.recursion.unproven.is_empty() || !facts.diagnostics.is_empty() {
        return defect("completed solved facts retain unresolved semantic evidence");
    }
    if program.functions().keys().copied().collect::<BTreeSet<_>>()
        != facts.definitions.keys().copied().collect()
        || program
            .specializations()
            .keys()
            .copied()
            .collect::<BTreeSet<_>>()
            != facts.specializations.keys().copied().collect()
    {
        return defect("solved fact coverage disagrees with verified Typed Program");
    }
    if !crate::semantic_facts::independently_verify(program, facts, cancellation) {
        if cancellation.is_cancelled() {
            return Err(CompletionFailure::Cancelled);
        }
        return defect("solved facts disagree with verified Typed Program");
    }
    for (identity, function) in &facts.specializations {
        if function
            .specialization_calls
            .keys()
            .any(|target| !facts.specializations.contains_key(target))
            || program.specialization_function(*identity).is_none()
        {
            return defect("solved facts contain a missing or stale Specialization");
        }
    }
    Ok(())
}

fn verify_evaluation_coverage(
    program: &VerifiedProgram,
    image: SpecializationId,
    evaluations: &[CompletedEvaluation],
    context: u128,
    cancellation: &Cancellation,
) -> Result<(), CompletionFailure> {
    let expected = program.expected_evaluation_roots(image);
    for pair in evaluations.windows(2) {
        checkpoint(cancellation)?;
        if pair[0].authority.root >= pair[1].authority.root {
            return defect("evaluation table is not in canonical typed-root order");
        }
    }
    let mut supplied = BTreeSet::new();
    for evaluation in evaluations {
        checkpoint(cancellation)?;
        if !supplied.insert(evaluation.authority.root) {
            return defect("evaluation table repeats a root");
        }
        if evaluation.authority.typed_program_fingerprint != program.fingerprint() {
            return defect("evaluation receipt names a different Typed Program");
        }
        verify_evaluation_reference(
            evaluation.root,
            evaluation.authority.root,
            program.fingerprint(),
            context,
        )?;
        let mut dependencies = BTreeSet::new();
        for pair in evaluation.dependencies.windows(2) {
            checkpoint(cancellation)?;
            if pair[0].root >= pair[1].root {
                return defect("evaluation dependencies are not in canonical typed-root order");
            }
        }
        for dependency in evaluation.dependencies.iter() {
            checkpoint(cancellation)?;
            if !dependencies.insert(dependency.root) {
                return defect("evaluation dependency table repeats a root");
            }
            verify_evaluation_reference(
                *dependency,
                dependency.root,
                program.fingerprint(),
                context,
            )?;
            if !expected.contains(&dependency.root) {
                return defect("evaluation dependency names a missing root authority");
            }
        }
        if dependencies != evaluation.authority.dependencies.iter().copied().collect() {
            return defect("evaluation dependency references disagree with semantic authority");
        }
    }
    if supplied != expected {
        return defect("evaluation coverage disagrees with verified Typed Program");
    }
    for evaluation in evaluations {
        if cancellation.is_cancelled() {
            return Err(CompletionFailure::Cancelled);
        }
        let mut engine = Engine::new(program, cancellation);
        let run = match evaluation.authority.root {
            crate::typed_hir::EvaluationRoot::Constant(identity) => {
                engine.evaluate_constant(identity)
            }
            crate::typed_hir::EvaluationRoot::Condition(identity) => {
                let Some(expression) = program.comptime_expression(identity) else {
                    return defect(
                        "evaluation condition root is missing from verified Typed Program",
                    );
                };
                engine.evaluate_expression(expression)
            }
            crate::typed_hir::EvaluationRoot::Image(identity) => {
                let Some(record) = program.specializations().get(&identity) else {
                    return defect("evaluation Image root is missing from verified Typed Program");
                };
                engine.evaluate_function(record.definition)
            }
        };
        if run.semantic.as_ref() != Some(&evaluation.authority) {
            return defect(
                "evaluation authority disagrees with independently replayed Typed Program",
            );
        }
    }
    let mut graph = BTreeMap::new();
    for evaluation in evaluations {
        checkpoint(cancellation)?;
        let mut dependencies = BTreeSet::new();
        for dependency in evaluation.dependencies.iter() {
            checkpoint(cancellation)?;
            if dependency.root != evaluation.authority.root {
                dependencies.insert(dependency.root);
            }
        }
        graph.insert(evaluation.authority.root, dependencies);
    }
    if crate::graph::recursive_nodes(&graph)
        .iter()
        .next()
        .is_some()
    {
        return defect("evaluation dependency authority contains a cycle");
    }
    Ok(())
}

fn verify_demand(
    candidate: &CompletedSemanticProgram,
    cancellation: &Cancellation,
) -> Result<(), CompletionFailure> {
    let root = SpecializationId(candidate.demand.root.raw.identity);
    let Some(root_executable) =
        candidate
            .demand
            .executables
            .iter()
            .find_map(|reference| match reference {
                ExecutableReference::Specialization(reference)
                    if reference.raw.identity == root.0 =>
                {
                    Some(*reference)
                }
                _ => None,
            })
    else {
        return defect("Executable Demand root is missing from its executable family");
    };
    if root_executable != candidate.demand.root {
        return defect("Executable Demand root reference is cross-context, wrong-kind, or stale");
    }
    let mut roots = BTreeSet::new();
    for node in candidate.graph.nodes.iter() {
        checkpoint(cancellation)?;
        roots.insert(SpecializationId(node.owner.raw.identity));
    }
    let mut retained_closures = BTreeSet::new();
    for node in candidate.graph.nodes.iter() {
        for operand in node.operands.iter() {
            let mut retained = IndependentValueFacts::default();
            independently_traverse_value(&operand.value, cancellation, &mut retained)?;
            roots.extend(retained.functions);
            retained_closures.extend(retained.closures);
        }
    }
    roots.insert(root);
    for actor in candidate.actors.iter() {
        checkpoint(cancellation)?;
        roots.extend(actor.handlers.iter().copied());
    }
    for application in candidate.graph.test_applications.iter() {
        if cancellation.is_cancelled() {
            return Err(CompletionFailure::Cancelled);
        }
        let Some(test_roots) =
            crate::semantic_facts::independently_verify_test_specialization_demands(
                &candidate.program,
                application.id,
            )
        else {
            return defect("Test Application names a missing verified Test body");
        };
        roots.extend(test_roots);
    }
    let specializations = verify_reachable_specializations(&roots, &candidate.facts, cancellation)?;
    let mut tests = BTreeSet::new();
    for application in candidate.graph.test_applications.iter() {
        checkpoint(cancellation)?;
        tests.insert(application.id);
    }
    let mut closures = retained_closures;
    for specialization in &specializations {
        checkpoint(cancellation)?;
        closures.extend(candidate.program.specialization_closures(*specialization));
    }
    for test in &tests {
        checkpoint(cancellation)?;
        closures.extend(candidate.program.test_closures(*test));
    }
    let mut exact = BTreeSet::new();
    for identity in &specializations {
        checkpoint(cancellation)?;
        exact.insert((ArtifactKind::Specialization, identity.0));
    }
    for test in &tests {
        checkpoint(cancellation)?;
        exact.insert((ArtifactKind::TestBody, test.identity));
    }
    for closure in &closures {
        checkpoint(cancellation)?;
        exact.insert((ArtifactKind::ClosureBody, closure.0));
    }
    let mut supplied = BTreeSet::new();
    for reference in candidate.demand.executables.iter() {
        checkpoint(cancellation)?;
        let raw = reference.raw();
        let expected_kind = match reference {
            ExecutableReference::Specialization(_) => ArtifactKind::Specialization,
            ExecutableReference::TestBody(_) => ArtifactKind::TestBody,
            ExecutableReference::ClosureBody(_) => ArtifactKind::ClosureBody,
            ExecutableReference::Generated(_) => ArtifactKind::GeneratedExecutable,
        };
        if raw.kind != expected_kind {
            return defect("typed executable reference has the wrong kind");
        }
        supplied.insert((raw.kind, raw.identity));
    }
    if exact != supplied {
        return defect("Executable Demand is not the exact reachable executable family");
    }
    for reference in candidate.demand.executables.iter() {
        let raw = reference.raw();
        if raw.context != candidate.context.identity {
            return defect("typed executable reference crosses compilation contexts");
        }
        let current = match reference {
            ExecutableReference::Specialization(reference) => {
                verify_specialization_current_meaning(
                    candidate.program.fingerprint(),
                    SpecializationId(reference.raw.identity),
                )
            }
            ExecutableReference::TestBody(reference) => verify_executable_current_meaning(
                candidate.program.fingerprint(),
                ArtifactKind::TestBody,
                reference.raw.identity,
            ),
            ExecutableReference::ClosureBody(reference) => verify_executable_current_meaning(
                candidate.program.fingerprint(),
                ArtifactKind::ClosureBody,
                reference.raw.identity,
            ),
            ExecutableReference::Generated(reference) => verify_executable_current_meaning(
                candidate.program.fingerprint(),
                ArtifactKind::GeneratedExecutable,
                reference.raw.identity,
            ),
        };
        if raw.current_meaning != current {
            return defect("typed executable reference is stale");
        }
    }
    if verify_demand_fingerprint(
        candidate.demand.root,
        &candidate.demand.executables,
        cancellation,
    )? != candidate.demand.fingerprint
    {
        return defect("Executable Demand fingerprint is false");
    }
    Ok(())
}

fn verify_actors(
    candidate: &CompletedSemanticProgram,
    cancellation: &Cancellation,
) -> Result<(), CompletionFailure> {
    let mut previous = None;
    for actor in candidate.actors.iter() {
        checkpoint(cancellation)?;
        if previous.is_some_and(|identity| identity >= actor.identity) {
            return defect("Actor identities are not unique canonical order");
        }
        previous = Some(actor.identity);
        if actor.source.path().is_empty() {
            return defect("Actor provenance is missing");
        }
        let mut previous_handler = None;
        for handler in actor.handlers.iter().copied() {
            checkpoint(cancellation)?;
            if previous_handler.is_some_and(|identity| identity >= handler) {
                return defect("Actor handlers are not unique canonical order");
            }
            previous_handler = Some(handler);
            let Some(function) = candidate.program.specialization_function(handler) else {
                return defect("Actor handler references a missing Specialization");
            };
            if function.modifier != crate::syntax::FunctionModifier::Async {
                return defect("Actor handler references a non-async function");
            }
        }
    }
    Ok(())
}

fn verify_graph(
    candidate: &CompletedSemanticProgram,
    cancellation: &Cancellation,
) -> Result<(), CompletionFailure> {
    let mut node_registry = BTreeMap::new();
    let mut node_kinds = BTreeMap::new();
    for node in candidate.graph.nodes.iter() {
        checkpoint(cancellation)?;
        if node_registry
            .insert(node.identity, node.local_fingerprint)
            .is_some()
        {
            return defect("construction graph repeats a node identity");
        }
        node_kinds.insert(node.identity, node.kind);
    }
    verify_semantic_reference(
        candidate.graph.root.raw,
        candidate.context.identity,
        ArtifactKind::ConstructionNode,
        &node_registry,
    )?;
    let root = candidate.graph.root.raw.identity;
    let Some(root_kind) = node_kinds.get(&root).copied() else {
        return defect("construction graph root is missing");
    };
    if root_kind != BuildKind::Image {
        return defect("construction graph root has the wrong kind");
    }
    let mut image_nodes = 0_usize;
    for node in candidate.graph.nodes.iter() {
        checkpoint(cancellation)?;
        image_nodes = image_nodes.saturating_add(usize::from(node.kind == BuildKind::Image));
    }
    if image_nodes != 1 {
        return defect("construction graph has a false Image root identity");
    }
    let mut demand_registry = BTreeMap::new();
    for reference in candidate.demand.executables.iter() {
        checkpoint(cancellation)?;
        if let ExecutableReference::Specialization(reference) = reference {
            demand_registry.insert(reference.raw.identity, reference.raw.current_meaning);
        }
    }
    let mut reconstructed_tests = Vec::new();
    for node in candidate.graph.nodes.iter() {
        for operand in node.operands.iter() {
            let mut facts = IndependentValueFacts::default();
            independently_traverse_value(&operand.value, cancellation, &mut facts)?;
            reconstructed_tests.extend(facts.tests);
        }
    }
    if reconstructed_tests.as_slice() != candidate.graph.test_applications.as_ref() {
        return defect("sealed graph Test Applications disagree with typed operands");
    }
    let mut applied_tests = BTreeSet::new();
    for application in candidate.graph.test_applications.iter() {
        if cancellation.is_cancelled() {
            return Err(CompletionFailure::Cancelled);
        }
        if candidate.program.test(application.id).is_none() || !applied_tests.insert(application.id)
        {
            return defect("sealed graph has a missing or duplicate Test Application");
        }
    }
    let mut incoming_moves = BTreeMap::<u128, usize>::new();
    let mut adjacency = BTreeMap::<u128, BTreeSet<u128>>::new();
    for node in candidate.graph.nodes.iter() {
        if cancellation.is_cancelled() {
            return Err(CompletionFailure::Cancelled);
        }
        if verify_node_local_fingerprint(node, cancellation)? != node.local_fingerprint {
            return defect("construction node current meaning is stale");
        }
        verify_semantic_reference(
            node.owner.raw,
            candidate.context.identity,
            ArtifactKind::Specialization,
            &demand_registry,
        )?;
        let mut reconstructed = Vec::new();
        for operand in node.operands.iter() {
            let mut facts = IndependentValueFacts::default();
            independently_traverse_value(&operand.value, cancellation, &mut facts)?;
            for (kind, identity) in facts.handles {
                let Some(current_meaning) = node_registry.get(&identity).copied() else {
                    return defect("graph handle names a missing node");
                };
                reconstructed.push(GraphEdge {
                    label: Arc::clone(&operand.label),
                    ordinal: u32::try_from(reconstructed.len()).unwrap_or(u32::MAX),
                    ownership: operand.ownership,
                    target_kind: kind,
                    target: TypedReference::new(
                        candidate.context.identity,
                        ArtifactKind::ConstructionNode,
                        identity,
                        current_meaning,
                    ),
                });
            }
        }
        for edge in &reconstructed {
            checkpoint(cancellation)?;
            verify_semantic_reference(
                edge.target.raw,
                candidate.context.identity,
                ArtifactKind::ConstructionNode,
                &node_registry,
            )?;
            let target_kind = node_kinds
                .get(&edge.target.raw.identity)
                .copied()
                .ok_or_else(|| CompletionFailure::Defect(Arc::from("missing graph target")))?;
            if target_kind != edge.target_kind {
                return defect("construction handle refers to a node of the wrong kind");
            }
            if edge.ownership == AccessMode::Move {
                *incoming_moves.entry(edge.target.raw.identity).or_default() += 1;
            }
            adjacency
                .entry(node.identity)
                .or_default()
                .insert(edge.target.raw.identity);
        }
    }
    for count in incoming_moves.values() {
        checkpoint(cancellation)?;
        if *count > 1 {
            return defect("construction graph duplicates Resource custody");
        }
    }
    let mut reachable = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(identity) = pending.pop() {
        checkpoint(cancellation)?;
        if reachable.insert(identity) {
            pending.extend(
                adjacency
                    .get(&identity)
                    .into_iter()
                    .flatten()
                    .rev()
                    .copied(),
            );
        }
    }
    let mut all_nodes = BTreeSet::new();
    for identity in node_registry.keys() {
        checkpoint(cancellation)?;
        all_nodes.insert(*identity);
    }
    if reachable != all_nodes {
        return defect("construction graph contains unreachable nodes");
    }
    if verify_graph_fingerprint(
        candidate.graph.root,
        &candidate.graph.nodes,
        &candidate.graph.test_applications,
        cancellation,
    )? != candidate.graph.fingerprint
    {
        return defect("construction graph fingerprint is false");
    }
    Ok(())
}

#[derive(Default)]
struct IndependentValueFacts {
    handles: Vec<(BuildKind, u128)>,
    functions: BTreeSet<SpecializationId>,
    closures: BTreeSet<crate::typed_hir::ClosureId>,
    tests: Vec<crate::evaluator::AppliedTest>,
}

fn independently_traverse_value(
    value: &Value,
    cancellation: &Cancellation,
    facts: &mut IndependentValueFacts,
) -> Result<(), CompletionFailure> {
    checkpoint(cancellation)?;
    match value {
        Value::TestApplication { id, payload } => {
            facts.tests.push(crate::evaluator::AppliedTest {
                id: *id,
                payload: payload.to_vec(),
            });
            for nested in payload.iter() {
                independently_traverse_value(nested, cancellation, facts)?;
            }
        }
        Value::Array(values)
        | Value::Tuple(values)
        | Value::BuiltinVariant {
            payload: values, ..
        }
        | Value::UserVariant {
            payload: values, ..
        } => {
            for nested in values.iter() {
                independently_traverse_value(nested, cancellation, facts)?;
            }
        }
        Value::Struct { fields, .. } => {
            for (_, nested) in fields.iter() {
                independently_traverse_value(nested, cancellation, facts)?;
            }
        }
        Value::Closure { id, captures } => {
            facts.closures.insert(*id);
            for (_, nested) in captures.iter() {
                independently_traverse_value(nested, cancellation, facts)?;
            }
        }
        Value::Function(identity) => {
            facts.functions.insert(*identity);
        }
        Value::SymbolicHandle { kind, identity } => facts.handles.push((*kind, *identity)),
        Value::Unavailable
        | Value::Unit
        | Value::Bool(_)
        | Value::Integer { .. }
        | Value::Float { .. }
        | Value::Text(_)
        | Value::Scalar(_)
        | Value::Bytes(_) => {}
    }
    Ok(())
}

fn verify_reachable_specializations(
    roots: &BTreeSet<SpecializationId>,
    facts: &SolvedSemanticFacts,
    cancellation: &Cancellation,
) -> Result<BTreeSet<SpecializationId>, CompletionFailure> {
    let mut verified = BTreeSet::new();
    let mut work = roots.iter().copied().collect::<Vec<_>>();
    while let Some(identity) = work.pop() {
        checkpoint(cancellation)?;
        if !verified.insert(identity) {
            continue;
        }
        let Some(function) = facts.specializations.get(&identity) else {
            return defect("Executable Demand names a Specialization without solved facts");
        };
        for target in function.specialization_calls.keys().copied() {
            if !verified.contains(&target) {
                work.push(target);
            }
        }
    }
    Ok(verified)
}

fn reconstruct_edges(
    context: u128,
    operands: &[ConstructionOperand],
    registry: &BTreeMap<u128, u128>,
    cancellation: &Cancellation,
) -> Result<Vec<GraphEdge>, CompletionFailure> {
    let mut edges = Vec::new();
    for operand in operands {
        checkpoint(cancellation)?;
        collect_operand_edges(context, operand, registry, &mut edges, cancellation)?;
    }
    Ok(edges)
}

fn verify_semantic_reference(
    reference: RawReference,
    context: u128,
    expected_kind: ArtifactKind,
    registry: &BTreeMap<u128, u128>,
) -> Result<(), CompletionFailure> {
    if reference.context != context {
        return defect("typed semantic reference crosses compilation contexts");
    }
    if reference.kind != expected_kind {
        return defect("typed semantic reference has the wrong kind");
    }
    let Some(current) = registry.get(&reference.identity) else {
        return defect("typed semantic reference names a missing artifact");
    };
    if reference.current_meaning != *current {
        return defect("typed semantic reference is stale");
    }
    Ok(())
}

fn specialization_current_meaning(program_fingerprint: u128, identity: SpecializationId) -> u128 {
    let mut hash = Xxh3::new();
    append_part(&mut hash, b"wrela.specialization-reference\0\x02");
    hash.update(&program_fingerprint.to_be_bytes());
    hash.update(&identity.0.to_be_bytes());
    hash.digest128()
}

fn executable_current_meaning(
    program_fingerprint: u128,
    kind: ArtifactKind,
    identity: u128,
) -> u128 {
    let mut hash = Xxh3::new();
    append_part(&mut hash, b"wrela.executable-reference\0\x02");
    hash.update(&[kind.tag()]);
    hash.update(&program_fingerprint.to_be_bytes());
    hash.update(&identity.to_be_bytes());
    hash.digest128()
}

fn verify_specialization_current_meaning(
    program_fingerprint: u128,
    identity: SpecializationId,
) -> u128 {
    let mut encoding = VerificationEncoding::new();
    encoding.part(b"wrela.specialization-reference\0\x02");
    encoding.u128(program_fingerprint);
    encoding.u128(identity.0);
    encoding.finish()
}

fn verify_executable_current_meaning(
    program_fingerprint: u128,
    kind: ArtifactKind,
    identity: u128,
) -> u128 {
    let mut encoding = VerificationEncoding::new();
    encoding.part(b"wrela.executable-reference\0\x02");
    encoding.byte(kind.tag());
    encoding.u128(program_fingerprint);
    encoding.u128(identity);
    encoding.finish()
}

fn evaluation_kind(root: crate::typed_hir::EvaluationRoot) -> ArtifactKind {
    match root {
        crate::typed_hir::EvaluationRoot::Constant(_) => ArtifactKind::ConstantEvaluationRoot,
        crate::typed_hir::EvaluationRoot::Condition(_) => ArtifactKind::ConditionEvaluationRoot,
        crate::typed_hir::EvaluationRoot::Image(_) => ArtifactKind::ImageEvaluationRoot,
    }
}

fn evaluation_reference(
    context: u128,
    program_fingerprint: u128,
    root: crate::typed_hir::EvaluationRoot,
) -> EvaluationReference {
    let kind = evaluation_kind(root);
    let current = executable_current_meaning(program_fingerprint, kind, root.identity());
    EvaluationReference {
        root,
        raw: RawReference {
            context,
            kind,
            identity: root.identity(),
            current_meaning: current,
        },
    }
}

fn verify_evaluation_reference(
    reference: EvaluationReference,
    expected_root: crate::typed_hir::EvaluationRoot,
    program_fingerprint: u128,
    context: u128,
) -> Result<(), CompletionFailure> {
    let expected_kind = evaluation_kind(expected_root);
    if reference.root != expected_root
        || reference.raw.context != context
        || reference.raw.kind != expected_kind
        || reference.raw.identity != expected_root.identity()
    {
        return defect("evaluation reference has a missing, cross-context, or wrong-kind root");
    }
    if reference.raw.current_meaning
        != verify_executable_current_meaning(
            program_fingerprint,
            expected_kind,
            expected_root.identity(),
        )
    {
        return defect("evaluation reference is stale");
    }
    Ok(())
}

fn produce_context_identity(input: ContextInput, catalog: u128) -> u128 {
    let mut hash = Xxh3::new();
    append_part(&mut hash, b"wrela.compilation-context\0\x02");
    append_part(&mut hash, COMPILER_SCHEMA.as_bytes());
    append_part(&mut hash, SEMANTIC_SCHEMA.as_bytes());
    hash.update(&input.distribution_digest.to_be_bytes());
    hash.update(&input.semantic_closure_digest.to_be_bytes());
    hash.update(&catalog.to_be_bytes());
    hash.update(&[root_tag(input.root)]);
    hash.digest128()
}

fn verify_context_identity(input: ContextInput, catalog: u128) -> u128 {
    let mut encoding = VerificationEncoding::new();
    encoding.part(b"wrela.compilation-context\0\x02");
    encoding.part(COMPILER_SCHEMA.as_bytes());
    encoding.part(SEMANTIC_SCHEMA.as_bytes());
    encoding.u128(input.distribution_digest);
    encoding.u128(input.semantic_closure_digest);
    encoding.u128(catalog);
    encoding.byte(root_tag(input.root));
    encoding.finish()
}

const fn root_tag(root: Root) -> u8 {
    match root {
        Root::Image => 1,
        Root::Test => 2,
    }
}

fn produce_facts_fingerprint(
    facts: &SolvedSemanticFacts,
    cancellation: &Cancellation,
) -> Result<u128, CompletionFailure> {
    hash_facts(b"wrela.solved-semantic-facts\0\x02", facts, cancellation)
}

fn verify_facts_fingerprint(
    facts: &SolvedSemanticFacts,
    cancellation: &Cancellation,
) -> Result<u128, CompletionFailure> {
    let mut encoding = VerificationEncoding::new();
    encoding.part(b"wrela.solved-semantic-facts\0\x02");
    encoding.u64(facts.definitions.len());
    for (identity, function) in &facts.definitions {
        checkpoint(cancellation)?;
        encoding.byte(1);
        encoding.u128(identity.0);
        encoding.function_facts(function, cancellation)?;
    }
    encoding.u64(facts.specializations.len());
    for (identity, function) in &facts.specializations {
        checkpoint(cancellation)?;
        encoding.byte(2);
        encoding.u128(identity.0);
        encoding.function_facts(function, cancellation)?;
    }
    encoding.u64(facts.recursion.proven.len());
    for (identity, maximum) in &facts.recursion.proven {
        checkpoint(cancellation)?;
        encoding.byte(3);
        encoding.u128(identity.0);
        encoding.u64(*maximum);
    }
    encoding.u64(facts.recursion.unproven.len());
    for source in &facts.recursion.unproven {
        checkpoint(cancellation)?;
        encoding.part(source.path().as_bytes());
        encoding.u64(source.start());
        encoding.u64(source.end());
    }
    encoding.u64(facts.inferred_errors.len());
    for inferred in &facts.inferred_errors {
        checkpoint(cancellation)?;
        encoding.u128(inferred.specialization_identity());
        encoding.part(inferred.function().as_bytes());
        encoding.part(inferred.error_type().as_bytes());
        encoding.part(inferred.provenance().path().as_bytes());
        encoding.u64(inferred.provenance().start());
        encoding.u64(inferred.provenance().end());
    }
    encoding.u64(facts.diagnostics.len());
    encoding.finish_cancellable(cancellation)
}

fn hash_facts(
    domain: &[u8],
    facts: &SolvedSemanticFacts,
    cancellation: &Cancellation,
) -> Result<u128, CompletionFailure> {
    let mut hash = Xxh3::new();
    append_part(&mut hash, domain);
    hash.update(
        &u64::try_from(facts.definitions.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for (identity, function) in &facts.definitions {
        checkpoint(cancellation)?;
        hash.update(&[1]);
        hash.update(&identity.0.to_be_bytes());
        append_function_facts(&mut hash, function, cancellation)?;
    }
    hash.update(
        &u64::try_from(facts.specializations.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for (identity, function) in &facts.specializations {
        checkpoint(cancellation)?;
        hash.update(&[2]);
        hash.update(&identity.0.to_be_bytes());
        append_function_facts(&mut hash, function, cancellation)?;
    }
    hash.update(
        &u64::try_from(facts.recursion.proven.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for (identity, maximum) in &facts.recursion.proven {
        checkpoint(cancellation)?;
        hash.update(&[3]);
        hash.update(&identity.0.to_be_bytes());
        hash.update(&maximum.to_be_bytes());
    }
    hash.update(
        &u64::try_from(facts.recursion.unproven.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for source in &facts.recursion.unproven {
        checkpoint(cancellation)?;
        append_part(&mut hash, source.path().as_bytes());
        hash.update(&source.start().to_be_bytes());
        hash.update(&source.end().to_be_bytes());
    }
    hash.update(
        &u64::try_from(facts.inferred_errors.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for inferred in &facts.inferred_errors {
        checkpoint(cancellation)?;
        hash.update(&inferred.specialization_identity().to_be_bytes());
        append_part(&mut hash, inferred.function().as_bytes());
        append_part(&mut hash, inferred.error_type().as_bytes());
        append_part(&mut hash, inferred.provenance().path().as_bytes());
        hash.update(&inferred.provenance().start().to_be_bytes());
        hash.update(&inferred.provenance().end().to_be_bytes());
    }
    hash.update(
        &u64::try_from(facts.diagnostics.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    checkpoint(cancellation)?;
    Ok(hash.digest128())
}

fn append_function_facts(
    hash: &mut Xxh3,
    facts: &FunctionFacts,
    cancellation: &Cancellation,
) -> Result<(), CompletionFailure> {
    checkpoint(cancellation)?;
    hash.update(&[
        u8::from(facts.pure),
        u8::from(facts.may_panic),
        u8::from(facts.suspends),
        u8::from(facts.evaluator_eligible),
        u8::from(facts.ownership_transfer),
        u8::from(facts.bounded),
    ]);
    hash.update(&facts.logical_cost.to_be_bytes());
    hash.update(
        &u64::try_from(facts.constructs.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for kind in &facts.constructs {
        checkpoint(cancellation)?;
        append_build_kind(hash, *kind);
    }
    hash.update(
        &u64::try_from(facts.calls.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for (identity, multiplicity) in &facts.calls {
        checkpoint(cancellation)?;
        hash.update(&identity.0.to_be_bytes());
        hash.update(&multiplicity.to_be_bytes());
    }
    hash.update(
        &u64::try_from(facts.specialization_calls.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for (identity, multiplicity) in &facts.specialization_calls {
        checkpoint(cancellation)?;
        hash.update(&identity.0.to_be_bytes());
        hash.update(&multiplicity.to_be_bytes());
    }
    Ok(())
}

fn produce_evaluations_fingerprint(
    evaluations: &[CompletedEvaluation],
    cancellation: &Cancellation,
) -> Result<u128, CompletionFailure> {
    hash_evaluations(
        b"wrela.semantic-evaluation-table\0\x01",
        evaluations,
        cancellation,
    )
}

fn verify_evaluations_fingerprint(
    evaluations: &[CompletedEvaluation],
    cancellation: &Cancellation,
) -> Result<u128, CompletionFailure> {
    let mut encoding = VerificationEncoding::new();
    encoding.part(b"wrela.semantic-evaluation-table\0\x01");
    encoding.u64(evaluations.len());
    for evaluation in evaluations {
        checkpoint(cancellation)?;
        let authority = &evaluation.authority;
        encoding.typed_value(&authority.value, cancellation)?;
        encoding.byte(authority.policy as u8);
        encoding.byte(evaluation.root.raw.kind.tag());
        encoding.u128(evaluation.root.raw.identity);
        encoding.u128(evaluation.root.raw.current_meaning);
        encoding.u128(authority.argument_fingerprint);
        encoding.byte(u8::from(authority.evaluator_eligible));
        encoding.u128(authority.typed_program_fingerprint);
        encoding.u64(evaluation.dependencies.len());
        for dependency in evaluation.dependencies.iter() {
            checkpoint(cancellation)?;
            encoding.byte(dependency.raw.kind.tag());
            encoding.u128(dependency.raw.identity);
            encoding.u128(dependency.raw.current_meaning);
        }
    }
    encoding.finish_cancellable(cancellation)
}

fn hash_evaluations(
    domain: &[u8],
    evaluations: &[CompletedEvaluation],
    cancellation: &Cancellation,
) -> Result<u128, CompletionFailure> {
    let mut hash = Xxh3::new();
    append_part(&mut hash, domain);
    hash.update(
        &u64::try_from(evaluations.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for evaluation in evaluations {
        checkpoint(cancellation)?;
        let authority = &evaluation.authority;
        append_typed_value(&mut hash, &authority.value, cancellation)?;
        hash.update(&[authority.policy as u8]);
        hash.update(&[evaluation.root.raw.kind.tag()]);
        hash.update(&evaluation.root.raw.identity.to_be_bytes());
        hash.update(&evaluation.root.raw.current_meaning.to_be_bytes());
        hash.update(&authority.argument_fingerprint.to_be_bytes());
        hash.update(&[u8::from(authority.evaluator_eligible)]);
        hash.update(&authority.typed_program_fingerprint.to_be_bytes());
        hash.update(
            &u64::try_from(evaluation.dependencies.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        for dependency in evaluation.dependencies.iter() {
            checkpoint(cancellation)?;
            hash.update(&[dependency.raw.kind.tag()]);
            hash.update(&dependency.raw.identity.to_be_bytes());
            hash.update(&dependency.raw.current_meaning.to_be_bytes());
        }
    }
    checkpoint(cancellation)?;
    Ok(hash.digest128())
}

fn produce_node_local_fingerprint(
    construction: &Construction,
    cancellation: &Cancellation,
) -> Result<u128, CompletionFailure> {
    hash_construction(b"wrela.construction-node\0\x01", construction, cancellation)
}

fn verify_node_local_fingerprint(
    node: &GraphNode,
    cancellation: &Cancellation,
) -> Result<u128, CompletionFailure> {
    let mut encoding = VerificationEncoding::new();
    encoding.part(b"wrela.construction-node\0\x01");
    encoding.u128(node.identity);
    encoding.build_kind(node.kind);
    encoding.u128(node.owner.raw.identity);
    encoding.part(node.site.path().as_bytes());
    encoding.u64(node.site.start());
    encoding.u64(node.site.end());
    encoding.u64(node.operands.len());
    for operand in node.operands.iter() {
        checkpoint(cancellation)?;
        encoding.part(operand.label.as_bytes());
        encoding.byte(access_tag(operand.ownership));
        encoding.typed_value(&operand.value, cancellation)?;
    }
    encoding.finish_cancellable(cancellation)
}

fn hash_construction(
    domain: &[u8],
    construction: &Construction,
    cancellation: &Cancellation,
) -> Result<u128, CompletionFailure> {
    let mut hash = Xxh3::new();
    append_part(&mut hash, domain);
    hash.update(&construction.identity.to_be_bytes());
    append_build_kind(&mut hash, construction.kind);
    hash.update(&construction.owner.to_be_bytes());
    append_part(&mut hash, construction.site.path().as_bytes());
    hash.update(&construction.site.start().to_be_bytes());
    hash.update(&construction.site.end().to_be_bytes());
    hash.update(
        &u64::try_from(construction.operands.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for operand in &construction.operands {
        checkpoint(cancellation)?;
        append_part(&mut hash, operand.label.as_bytes());
        hash.update(&[access_tag(operand.ownership)]);
        append_typed_value(&mut hash, &operand.value, cancellation)?;
    }
    checkpoint(cancellation)?;
    Ok(hash.digest128())
}

fn produce_graph_fingerprint(
    root: TypedReference<ConstructionNodeAuthority>,
    nodes: &[GraphNode],
    test_applications: &[crate::evaluator::AppliedTest],
    cancellation: &Cancellation,
) -> Result<u128, CompletionFailure> {
    hash_graph(
        b"wrela.sealed-construction-graph\0\x01",
        root,
        nodes,
        test_applications,
        cancellation,
    )
}

fn verify_graph_fingerprint(
    root: TypedReference<ConstructionNodeAuthority>,
    nodes: &[GraphNode],
    test_applications: &[crate::evaluator::AppliedTest],
    cancellation: &Cancellation,
) -> Result<u128, CompletionFailure> {
    let mut encoding = VerificationEncoding::new();
    encoding.part(b"wrela.sealed-construction-graph\0\x01");
    encoding.u128(root.raw.identity);
    encoding.u64(nodes.len());
    let mut registry = BTreeMap::new();
    for node in nodes {
        checkpoint(cancellation)?;
        registry.insert(node.identity, node.local_fingerprint);
    }
    for node in nodes {
        checkpoint(cancellation)?;
        encoding.u128(node.identity);
        encoding.u128(node.local_fingerprint);
        let mut edges = Vec::new();
        for operand in node.operands.iter() {
            let mut facts = IndependentValueFacts::default();
            independently_traverse_value(&operand.value, cancellation, &mut facts)?;
            edges.extend(facts.handles.into_iter().map(|(kind, identity)| {
                (
                    Arc::clone(&operand.label),
                    operand.ownership,
                    kind,
                    identity,
                )
            }));
        }
        encoding.u64(edges.len());
        for (ordinal, (label, ownership, kind, identity)) in edges.iter().enumerate() {
            checkpoint(cancellation)?;
            encoding.part(label.as_bytes());
            encoding.u32(u32::try_from(ordinal).unwrap_or(u32::MAX));
            encoding.byte(access_tag(*ownership));
            encoding.build_kind(*kind);
            encoding.u128(*identity);
            encoding.u128(registry.get(identity).copied().unwrap_or_default());
        }
    }
    encoding.u64(test_applications.len());
    for application in test_applications {
        checkpoint(cancellation)?;
        encoding.u128(application.id.suite.0);
        encoding.u128(application.id.test.0);
        encoding.u128(application.id.identity);
        encoding.u64(application.payload.len());
        for value in &application.payload {
            encoding.typed_value(value, cancellation)?;
        }
    }
    encoding.finish_cancellable(cancellation)
}

fn hash_graph(
    domain: &[u8],
    root: TypedReference<ConstructionNodeAuthority>,
    nodes: &[GraphNode],
    test_applications: &[crate::evaluator::AppliedTest],
    cancellation: &Cancellation,
) -> Result<u128, CompletionFailure> {
    let mut hash = Xxh3::new();
    append_part(&mut hash, domain);
    hash.update(&root.raw.identity.to_be_bytes());
    hash.update(&u64::try_from(nodes.len()).unwrap_or(u64::MAX).to_be_bytes());
    let mut registry = BTreeMap::new();
    for node in nodes {
        checkpoint(cancellation)?;
        registry.insert(node.identity, node.local_fingerprint);
    }
    for node in nodes {
        checkpoint(cancellation)?;
        hash.update(&node.identity.to_be_bytes());
        hash.update(&node.local_fingerprint.to_be_bytes());
        let edges = reconstruct_edges(root.raw.context, &node.operands, &registry, cancellation)?;
        hash.update(&u64::try_from(edges.len()).unwrap_or(u64::MAX).to_be_bytes());
        for edge in &edges {
            checkpoint(cancellation)?;
            append_part(&mut hash, edge.label.as_bytes());
            hash.update(&edge.ordinal.to_be_bytes());
            hash.update(&[access_tag(edge.ownership)]);
            append_build_kind(&mut hash, edge.target_kind);
            hash.update(&edge.target.raw.identity.to_be_bytes());
            hash.update(&edge.target.raw.current_meaning.to_be_bytes());
        }
    }
    hash.update(
        &u64::try_from(test_applications.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for application in test_applications {
        checkpoint(cancellation)?;
        hash.update(&application.id.suite.0.to_be_bytes());
        hash.update(&application.id.test.0.to_be_bytes());
        hash.update(&application.id.identity.to_be_bytes());
        hash.update(
            &u64::try_from(application.payload.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        for value in &application.payload {
            append_typed_value(&mut hash, value, cancellation)?;
        }
    }
    checkpoint(cancellation)?;
    Ok(hash.digest128())
}

fn produce_demand_fingerprint(
    root: TypedReference<SpecializationAuthority>,
    executables: &[ExecutableReference],
    cancellation: &Cancellation,
) -> Result<u128, CompletionFailure> {
    hash_demand(
        b"wrela.executable-demand\0\x02",
        root,
        executables,
        cancellation,
    )
}

fn verify_demand_fingerprint(
    root: TypedReference<SpecializationAuthority>,
    executables: &[ExecutableReference],
    cancellation: &Cancellation,
) -> Result<u128, CompletionFailure> {
    let mut encoding = VerificationEncoding::new();
    encoding.part(b"wrela.executable-demand\0\x02");
    encoding.u128(root.raw.identity);
    encoding.u64(executables.len());
    for executable in executables {
        checkpoint(cancellation)?;
        let reference = executable.raw();
        encoding.byte(reference.kind.tag());
        encoding.u128(reference.identity);
        encoding.u128(reference.current_meaning);
    }
    encoding.finish_cancellable(cancellation)
}

fn hash_demand(
    domain: &[u8],
    root: TypedReference<SpecializationAuthority>,
    executables: &[ExecutableReference],
    cancellation: &Cancellation,
) -> Result<u128, CompletionFailure> {
    let mut hash = Xxh3::new();
    append_part(&mut hash, domain);
    hash.update(&root.raw.identity.to_be_bytes());
    hash.update(
        &u64::try_from(executables.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for executable in executables {
        checkpoint(cancellation)?;
        let reference = executable.raw();
        hash.update(&[reference.kind.tag()]);
        hash.update(&reference.identity.to_be_bytes());
        hash.update(&reference.current_meaning.to_be_bytes());
    }
    checkpoint(cancellation)?;
    Ok(hash.digest128())
}

fn produce_completed_fingerprint(
    context: u128,
    receipts: &DirectReceipts,
    custody: u128,
    cancellation: &Cancellation,
) -> Result<u128, CompletionFailure> {
    hash_completed(
        b"wrela.completed-semantic-program\0\x01",
        context,
        receipts,
        custody,
        cancellation,
    )
}

fn verify_completed_fingerprint(
    context: u128,
    receipts: &DirectReceipts,
    custody: u128,
    cancellation: &Cancellation,
) -> Result<u128, CompletionFailure> {
    let mut encoding = VerificationEncoding::new();
    encoding.part(b"wrela.completed-semantic-program\0\x01");
    encoding.part(PHASE_SCHEMA.as_bytes());
    encoding.u128(context);
    for reference in [
        receipts.program.raw,
        receipts.facts.raw,
        receipts.evaluations.raw,
        receipts.graph.raw,
        receipts.demand.raw,
    ] {
        checkpoint(cancellation)?;
        encoding.byte(reference.kind.tag());
        encoding.u128(reference.identity);
        encoding.u128(reference.current_meaning);
    }
    encoding.u128(custody);
    encoding.finish_cancellable(cancellation)
}

fn hash_completed(
    domain: &[u8],
    context: u128,
    receipts: &DirectReceipts,
    custody: u128,
    cancellation: &Cancellation,
) -> Result<u128, CompletionFailure> {
    let mut hash = Xxh3::new();
    append_part(&mut hash, domain);
    append_part(&mut hash, PHASE_SCHEMA.as_bytes());
    hash.update(&context.to_be_bytes());
    for reference in [
        receipts.program.raw,
        receipts.facts.raw,
        receipts.evaluations.raw,
        receipts.graph.raw,
        receipts.demand.raw,
    ] {
        checkpoint(cancellation)?;
        hash.update(&[reference.kind.tag()]);
        hash.update(&reference.identity.to_be_bytes());
        hash.update(&reference.current_meaning.to_be_bytes());
    }
    hash.update(&custody.to_be_bytes());
    checkpoint(cancellation)?;
    Ok(hash.digest128())
}

fn append_part(hash: &mut Xxh3, bytes: &[u8]) {
    hash.update(&u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
    hash.update(bytes);
}

fn append_typed_value(
    hash: &mut Xxh3,
    value: &Value,
    cancellation: &Cancellation,
) -> Result<(), CompletionFailure> {
    checkpoint(cancellation)?;
    match value {
        Value::Unavailable => hash.update(&[0]),
        Value::Unit => hash.update(&[1]),
        Value::Bool(value) => hash.update(&[2, u8::from(*value)]),
        Value::Integer { kind, value } => {
            hash.update(&[3]);
            append_part(hash, kind.name().as_bytes());
            hash.update(&value.to_be_bytes());
        }
        Value::Float { kind, bits } => {
            hash.update(&[4]);
            append_part(hash, kind.name().as_bytes());
            hash.update(&bits.to_be_bytes());
        }
        Value::Text(value) => {
            hash.update(&[5]);
            append_part(hash, value.as_bytes());
        }
        Value::Scalar(value) => {
            hash.update(&[6]);
            hash.update(&u32::from(*value).to_be_bytes());
        }
        Value::Bytes(value) => {
            hash.update(&[7]);
            append_part(hash, value);
        }
        Value::Function(identity) => {
            hash.update(&[8]);
            hash.update(&identity.0.to_be_bytes());
        }
        Value::Closure { id, captures } => {
            hash.update(&[9]);
            hash.update(&id.0.to_be_bytes());
            hash.update(
                &u64::try_from(captures.len())
                    .unwrap_or(u64::MAX)
                    .to_be_bytes(),
            );
            for (local, value) in captures.iter() {
                hash.update(&local.0.to_be_bytes());
                append_typed_value(hash, value, cancellation)?;
            }
        }
        Value::Tuple(values) => {
            hash.update(&[10]);
            append_typed_values(hash, values, cancellation)?;
        }
        Value::Array(values) => {
            hash.update(&[11]);
            append_typed_values(hash, values, cancellation)?;
        }
        Value::BuiltinVariant { variant, payload } => {
            hash.update(&[12, variant.canonical_tag()]);
            append_typed_values(hash, payload, cancellation)?;
        }
        Value::UserVariant {
            id,
            variant_order,
            type_display,
            variant_display,
            payload,
        } => {
            hash.update(&[13]);
            hash.update(&id.owner.0.to_be_bytes());
            hash.update(&id.definition.0.to_be_bytes());
            hash.update(&id.variant.to_be_bytes());
            hash.update(&variant_order.to_be_bytes());
            append_part(hash, type_display.as_bytes());
            append_part(hash, variant_display.as_bytes());
            append_typed_values(hash, payload, cancellation)?;
        }
        Value::Struct {
            definition,
            type_display,
            fields,
        } => {
            hash.update(&[14]);
            hash.update(&definition.0.to_be_bytes());
            append_part(hash, type_display.as_bytes());
            hash.update(
                &u64::try_from(fields.len())
                    .unwrap_or(u64::MAX)
                    .to_be_bytes(),
            );
            for (name, value) in fields.iter() {
                append_part(hash, name.as_bytes());
                append_typed_value(hash, value, cancellation)?;
            }
        }
        Value::TestApplication { id, payload } => {
            hash.update(&[15]);
            hash.update(&id.suite.0.to_be_bytes());
            hash.update(&id.test.0.to_be_bytes());
            hash.update(&id.identity.to_be_bytes());
            append_typed_values(hash, payload, cancellation)?;
        }
        Value::SymbolicHandle { kind, identity } => {
            hash.update(&[16]);
            append_build_kind(hash, *kind);
            hash.update(&identity.to_be_bytes());
        }
    }
    Ok(())
}

fn append_typed_values(
    hash: &mut Xxh3,
    values: &[Value],
    cancellation: &Cancellation,
) -> Result<(), CompletionFailure> {
    hash.update(
        &u64::try_from(values.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for value in values {
        append_typed_value(hash, value, cancellation)?;
    }
    Ok(())
}

fn append_build_kind(hash: &mut Xxh3, kind: BuildKind) {
    hash.update(&[kind.canonical_tag()]);
    if let BuildKind::Node {
        definition,
        type_identity,
    } = kind
    {
        hash.update(&definition.0.to_be_bytes());
        hash.update(&type_identity.0.to_be_bytes());
    }
}

const fn access_tag(access: AccessMode) -> u8 {
    match access {
        AccessMode::Copy => 1,
        AccessMode::Read => 2,
        AccessMode::Mut => 3,
        AccessMode::Move => 4,
    }
}

/// The verifier deliberately owns a second canonical encoder. It never calls the
/// producer's streaming hash helpers, so a producer bookkeeping defect cannot
/// authenticate itself merely by being repeated during verification.
struct VerificationEncoding {
    bytes: Vec<u8>,
}

impl VerificationEncoding {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn finish(self) -> u128 {
        xxhash_rust::xxh3::xxh3_128(&self.bytes)
    }

    fn finish_cancellable(self, cancellation: &Cancellation) -> Result<u128, CompletionFailure> {
        let mut hash = Xxh3::new();
        for chunk in self.bytes.chunks(4_096) {
            checkpoint(cancellation)?;
            hash.update(chunk);
        }
        Ok(hash.digest128())
    }

    fn byte(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn u64(&mut self, value: impl TryInto<u64>) {
        self.bytes
            .extend_from_slice(&value.try_into().ok().unwrap_or(u64::MAX).to_be_bytes());
    }

    fn u128(&mut self, value: u128) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn i128(&mut self, value: i128) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn part(&mut self, value: &[u8]) {
        self.u64(value.len());
        self.bytes.extend_from_slice(value);
    }

    fn build_kind(&mut self, kind: BuildKind) {
        self.byte(kind.canonical_tag());
        if let BuildKind::Node {
            definition,
            type_identity,
        } = kind
        {
            self.u128(definition.0);
            self.u128(type_identity.0);
        }
    }

    fn values(
        &mut self,
        values: &[Value],
        cancellation: &Cancellation,
    ) -> Result<(), CompletionFailure> {
        self.u64(values.len());
        for value in values {
            self.typed_value(value, cancellation)?;
        }
        Ok(())
    }

    fn typed_value(
        &mut self,
        value: &Value,
        cancellation: &Cancellation,
    ) -> Result<(), CompletionFailure> {
        checkpoint(cancellation)?;
        match value {
            Value::Unavailable => self.byte(0),
            Value::Unit => self.byte(1),
            Value::Bool(value) => {
                self.byte(2);
                self.byte(u8::from(*value));
            }
            Value::Integer { kind, value } => {
                self.byte(3);
                self.part(kind.name().as_bytes());
                self.i128(*value);
            }
            Value::Float { kind, bits } => {
                self.byte(4);
                self.part(kind.name().as_bytes());
                self.u64(*bits);
            }
            Value::Text(value) => {
                self.byte(5);
                self.part(value.as_bytes());
            }
            Value::Scalar(value) => {
                self.byte(6);
                self.u32(u32::from(*value));
            }
            Value::Bytes(value) => {
                self.byte(7);
                self.part(value);
            }
            Value::Function(identity) => {
                self.byte(8);
                self.u128(identity.0);
            }
            Value::Closure { id, captures } => {
                self.byte(9);
                self.u128(id.0);
                self.u64(captures.len());
                for (local, value) in captures.iter() {
                    self.u32(local.0);
                    self.typed_value(value, cancellation)?;
                }
            }
            Value::Tuple(values) => {
                self.byte(10);
                self.values(values, cancellation)?;
            }
            Value::Array(values) => {
                self.byte(11);
                self.values(values, cancellation)?;
            }
            Value::BuiltinVariant { variant, payload } => {
                self.byte(12);
                self.byte(variant.canonical_tag());
                self.values(payload, cancellation)?;
            }
            Value::UserVariant {
                id,
                variant_order,
                type_display,
                variant_display,
                payload,
            } => {
                self.byte(13);
                self.u128(id.owner.0);
                self.u128(id.definition.0);
                self.u128(id.variant);
                self.u32(*variant_order);
                self.part(type_display.as_bytes());
                self.part(variant_display.as_bytes());
                self.values(payload, cancellation)?;
            }
            Value::Struct {
                definition,
                type_display,
                fields,
            } => {
                self.byte(14);
                self.u128(definition.0);
                self.part(type_display.as_bytes());
                self.u64(fields.len());
                for (name, value) in fields.iter() {
                    self.part(name.as_bytes());
                    self.typed_value(value, cancellation)?;
                }
            }
            Value::TestApplication { id, payload } => {
                self.byte(15);
                self.u128(id.suite.0);
                self.u128(id.test.0);
                self.u128(id.identity);
                self.values(payload, cancellation)?;
            }
            Value::SymbolicHandle { kind, identity } => {
                self.byte(16);
                self.build_kind(*kind);
                self.u128(*identity);
            }
        }
        Ok(())
    }

    fn function_facts(
        &mut self,
        facts: &FunctionFacts,
        cancellation: &Cancellation,
    ) -> Result<(), CompletionFailure> {
        checkpoint(cancellation)?;
        for value in [
            facts.pure,
            facts.may_panic,
            facts.suspends,
            facts.evaluator_eligible,
            facts.ownership_transfer,
            facts.bounded,
        ] {
            self.byte(u8::from(value));
        }
        self.u64(facts.logical_cost);
        self.u64(facts.constructs.len());
        for kind in &facts.constructs {
            checkpoint(cancellation)?;
            self.build_kind(*kind);
        }
        self.u64(facts.calls.len());
        for (identity, multiplicity) in &facts.calls {
            checkpoint(cancellation)?;
            self.u128(identity.0);
            self.u64(*multiplicity);
        }
        self.u64(facts.specialization_calls.len());
        for (identity, multiplicity) in &facts.specialization_calls {
            checkpoint(cancellation)?;
            self.u128(identity.0);
            self.u64(*multiplicity);
        }
        Ok(())
    }
}

fn defect<T>(evidence: &'static str) -> Result<T, CompletionFailure> {
    Err(CompletionFailure::Defect(Arc::from(evidence)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::{
        CompilationOutcome, CompilationRequest, Compiler, CompilerInstallation, InspectSelection,
        ProjectFile, ProjectSnapshot,
    };

    fn completed_fixture() -> CompletedSemanticProgram {
        completed_fixture_from_source(
            br#"from runtime import topology

pure fn leaf() -> topology.Node:
    return topology.Node.new(children=[])

@image
fn build() -> Image:
    node = leaf()
    return Image.new(node=node)
"#,
        )
    }

    fn completed_fixture_with_dependencies() -> CompletedSemanticProgram {
        completed_fixture_from_source(
            br#"from runtime import topology

const BASE: i64 = 40
const OTHER: i64 = 2
const ANSWER: i64 = BASE + OTHER

pure fn leaf() -> topology.Node:
    return topology.Node.new(children=[])

@image
fn build() -> Image:
    node = leaf()
    return Image.new(node=node)
"#,
        )
    }

    fn completed_test_fixture() -> CompletedSemanticProgram {
        completed_fixture_from_source_with_root(
            br#"pub suite behavior:
    test succeeds():
        expect true

@image
fn build() -> Image:
    tests = Test.new(cases=[behavior.succeeds()])
    return Image.new(tests=tests)
"#,
            Root::Test,
        )
    }

    fn completed_inferred_error_fixture() -> CompletedSemanticProgram {
        completed_fixture_from_source(
            br#"enum ReadError:
    Missing

fn inferred() -> Result[i64]:
    return Result.Err(ReadError.Missing)

@image
fn build() -> Image:
    return Image.new()
"#,
        )
    }

    fn completed_retained_function_fixture() -> CompletedSemanticProgram {
        completed_fixture_from_source(
            br#"pure fn callback(value: i64) -> i64:
    return value + 1

@image
fn build() -> Image:
    return Image.new(callback=callback)
"#,
        )
    }

    fn completed_fixture_from_source(source: &'static [u8]) -> CompletedSemanticProgram {
        completed_fixture_from_source_with_root(source, Root::Image)
    }

    fn completed_fixture_from_source_with_root(
        source: &'static [u8],
        root: Root,
    ) -> CompletedSemanticProgram {
        let compiler = Compiler::open(CompilerInstallation::with_authenticated_modules(vec![
            ProjectFile::new(
                "src/runtime/topology.wr",
                br#"pub struct Node:
    pub pure fn new(children: [Node]) -> Node:
        panic "sealed Node constructor"
"#,
            ),
        ]))
        .expect("fixture distribution seals");
        let outcome = compiler.compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new(
                    match root {
                        Root::Image => "src/image.wr",
                        Root::Test => "src/test.wr",
                    },
                    source,
                )]),
                root,
            )
            .with_inspection(InspectSelection::all()),
            &Cancellation::new(),
        );
        let CompilationOutcome::Accepted(accepted) = outcome else {
            panic!("private completion fixture accepts: {outcome:#?}");
        };
        accepted.completed_semantic_program().clone()
    }

    fn evidence(result: Result<(), CompletionFailure>) -> Arc<str> {
        match result {
            Err(CompletionFailure::Defect(evidence)) => evidence,
            Err(CompletionFailure::Cancelled) => panic!("corruption must be a Defect"),
            Ok(()) => panic!("single-fault corruption was accepted"),
        }
    }

    fn produced<T>(result: Result<T, CompletionFailure>) -> T {
        match result {
            Ok(value) => value,
            Err(CompletionFailure::Cancelled) => panic!("test production unexpectedly cancelled"),
            Err(CompletionFailure::Defect(evidence)) => {
                panic!("test production defect: {evidence}")
            }
        }
    }

    fn resign_facts(candidate: &mut CompletedSemanticProgram) {
        let fingerprint = produced(produce_facts_fingerprint(
            &candidate.facts,
            &Cancellation::new(),
        ));
        candidate.receipts.facts = TypedReference::new(
            candidate.context.identity,
            ArtifactKind::SolvedFacts,
            fingerprint,
            fingerprint,
        );
        resign_completed(candidate);
    }

    fn resign_evaluations(candidate: &mut CompletedSemanticProgram) {
        let fingerprint = produced(produce_evaluations_fingerprint(
            &candidate.evaluations,
            &Cancellation::new(),
        ));
        candidate.receipts.evaluations = TypedReference::new(
            candidate.context.identity,
            ArtifactKind::EvaluationTable,
            fingerprint,
            fingerprint,
        );
        resign_completed(candidate);
    }

    fn resign_demand(candidate: &mut CompletedSemanticProgram) {
        candidate.demand.fingerprint = produced(produce_demand_fingerprint(
            candidate.demand.root,
            &candidate.demand.executables,
            &Cancellation::new(),
        ));
        candidate.receipts.demand = TypedReference::new(
            candidate.context.identity,
            ArtifactKind::ExecutableDemand,
            candidate.demand.fingerprint,
            candidate.demand.fingerprint,
        );
        resign_completed(candidate);
    }

    fn resign_graph(candidate: &mut CompletedSemanticProgram) {
        candidate.graph.fingerprint = produced(produce_graph_fingerprint(
            candidate.graph.root,
            &candidate.graph.nodes,
            &candidate.graph.test_applications,
            &Cancellation::new(),
        ));
        candidate.receipts.graph = TypedReference::new(
            candidate.context.identity,
            ArtifactKind::ConstructionGraph,
            candidate.graph.fingerprint,
            candidate.graph.fingerprint,
        );
        resign_completed(candidate);
    }

    fn resign_completed(candidate: &mut CompletedSemanticProgram) {
        candidate.fingerprint = produced(produce_completed_fingerprint(
            candidate.context.identity,
            &candidate.receipts,
            candidate.custody_fingerprint,
            &Cancellation::new(),
        ));
    }

    #[test]
    fn malformed_graph_is_rejected_by_the_completion_verifier() {
        let mut candidate = completed_fixture();
        let nodes = Arc::make_mut(&mut candidate.graph.nodes);
        nodes[0].kind = BuildKind::Test;

        assert!(evidence(verify(&candidate, &Cancellation::new())).contains("construction node"));
    }

    #[test]
    fn mismatched_solved_facts_are_rejected_by_the_completion_verifier() {
        let mut candidate = completed_fixture();
        let facts = Arc::make_mut(&mut candidate.facts);
        facts
            .specializations
            .values_mut()
            .next()
            .expect("fixture facts")
            .pure ^= true;

        assert!(evidence(verify(&candidate, &Cancellation::new())).contains("stale"));
    }

    #[test]
    fn omitted_solved_fact_fields_are_bound_by_the_completion_fingerprint() {
        let mut candidate = completed_fixture();
        let facts = Arc::make_mut(&mut candidate.facts);
        facts
            .specializations
            .values_mut()
            .next()
            .expect("fixture facts")
            .constructs
            .insert(BuildKind::Test);

        assert!(evidence(verify(&candidate, &Cancellation::new())).contains("stale"));
    }

    #[test]
    fn false_solved_facts_cannot_be_blessed_before_receipt_creation() {
        let mut candidate = completed_fixture();
        Arc::make_mut(&mut candidate.facts)
            .specializations
            .values_mut()
            .next()
            .expect("fixture facts")
            .pure ^= true;
        resign_facts(&mut candidate);

        assert!(
            evidence(verify(&candidate, &Cancellation::new()))
                .contains("solved facts disagree with verified Typed Program")
        );
    }

    #[test]
    fn false_panic_cost_and_call_facts_cannot_be_blessed_before_receipt_creation() {
        for corruption in 0..3 {
            let mut candidate = completed_fixture();
            let definition = *candidate
                .program
                .functions()
                .keys()
                .next()
                .expect("fixture function");
            let facts = Arc::make_mut(&mut candidate.facts)
                .specializations
                .values_mut()
                .next()
                .expect("fixture facts");
            match corruption {
                0 => facts.may_panic ^= true,
                1 => facts.logical_cost = facts.logical_cost.saturating_add(1),
                2 => {
                    facts.calls.insert(definition, 91);
                }
                _ => unreachable!(),
            }
            resign_facts(&mut candidate);

            assert!(
                evidence(verify(&candidate, &Cancellation::new()))
                    .contains("solved facts disagree with verified Typed Program"),
                "pre-receipt fact corruption {corruption} must be independently rejected"
            );
        }
    }

    #[test]
    fn false_inferred_error_records_and_provenance_cannot_be_blessed() {
        for corruption in 0..6 {
            let mut candidate = completed_inferred_error_fixture();
            let original = candidate
                .facts
                .inferred_errors
                .first()
                .expect("fixture inferred error")
                .clone();
            let errors = &mut Arc::make_mut(&mut candidate.facts).inferred_errors;
            match corruption {
                0 => errors.clear(),
                1 => errors.push(original),
                2 => {
                    errors[0] = crate::InferredErrorObservation::new(
                        original.specialization_identity(),
                        "other",
                        original.error_type(),
                        original.provenance().clone(),
                    );
                }
                3 => {
                    errors[0] = crate::InferredErrorObservation::new(
                        original.specialization_identity() ^ 1,
                        original.function(),
                        original.error_type(),
                        original.provenance().clone(),
                    );
                }
                4 => {
                    errors[0] = crate::InferredErrorObservation::new(
                        original.specialization_identity(),
                        original.function(),
                        "OtherError",
                        original.provenance().clone(),
                    );
                }
                5 => {
                    errors[0] = crate::InferredErrorObservation::new(
                        original.specialization_identity(),
                        original.function(),
                        original.error_type(),
                        crate::SourceRange::new("src/other.wr", 1, 2),
                    );
                }
                _ => unreachable!(),
            }
            resign_facts(&mut candidate);

            assert!(
                evidence(verify(&candidate, &Cancellation::new()))
                    .contains("solved facts disagree with verified Typed Program"),
                "inferred-error corruption {corruption} must be independently rejected"
            );
        }
    }

    #[test]
    fn evaluator_telemetry_does_not_change_completed_semantic_identity() {
        let candidate = completed_fixture();
        let original = candidate.fingerprint;
        let evaluation = candidate.evaluations.first().expect("fixture evaluation");
        let low = crate::EvaluationReceipt::new(
            evaluation.authority.policy,
            evaluation.authority.root.identity(),
            evaluation
                .authority
                .dependencies
                .iter()
                .map(|root| root.identity())
                .collect(),
            evaluation.authority.typed_program_fingerprint,
            1,
            2,
        );
        let high = crate::EvaluationReceipt::new(
            evaluation.authority.policy,
            evaluation.authority.root.identity(),
            evaluation
                .authority
                .dependencies
                .iter()
                .map(|root| root.identity())
                .collect(),
            evaluation.authority.typed_program_fingerprint,
            10_001,
            20_002,
        )
        .with_test_tariff_schema("wrela.evaluator.tariff.test-only-v999");

        assert_ne!(low, high, "inspection telemetry differs");
        assert_ne!(low.tariff_schema(), high.tariff_schema());
        assert_ne!(low.fuel_used(), high.fuel_used());
        assert_ne!(low.peak_memory(), high.peak_memory());
        assert_eq!(candidate.fingerprint, original);
    }

    #[test]
    fn false_evaluation_result_or_dependencies_cannot_be_blessed() {
        let mut false_result = completed_fixture();
        let authority = &mut Arc::make_mut(&mut false_result.evaluations)[0].authority;
        authority.value = match &authority.value {
            Value::Unavailable => Value::Unit,
            _ => Value::Unavailable,
        };
        resign_evaluations(&mut false_result);
        assert!(
            evidence(verify(&false_result, &Cancellation::new()))
                .contains("independently replayed Typed Program")
        );

        let mut false_dependencies = completed_fixture_with_dependencies();
        let evaluation = Arc::make_mut(&mut false_dependencies.evaluations)
            .iter_mut()
            .find(|evaluation| !evaluation.dependencies.is_empty())
            .expect("fixture evaluation dependency");
        evaluation.authority.dependencies = Arc::from([]);
        evaluation.dependencies = Arc::from([]);
        resign_evaluations(&mut false_dependencies);
        assert!(
            evidence(verify(&false_dependencies, &Cancellation::new()))
                .contains("independently replayed Typed Program")
        );
    }

    #[test]
    fn evaluation_references_reject_wrong_kind_cross_context_stale_missing_and_duplicates() {
        for corruption in 0..5 {
            let mut candidate = completed_fixture_with_dependencies();
            let evaluation = Arc::make_mut(&mut candidate.evaluations)
                .iter_mut()
                .find(|evaluation| !evaluation.dependencies.is_empty())
                .expect("fixture dependency");
            match corruption {
                0 => {
                    Arc::make_mut(&mut evaluation.dependencies)[0].raw.kind = ArtifactKind::TestBody
                }
                1 => Arc::make_mut(&mut evaluation.dependencies)[0].raw.context ^= 1,
                2 => {
                    Arc::make_mut(&mut evaluation.dependencies)[0]
                        .raw
                        .current_meaning ^= 1
                }
                3 => Arc::make_mut(&mut evaluation.dependencies)[0].raw.identity ^= 1,
                4 => {
                    let duplicate = evaluation.dependencies[0];
                    evaluation.dependencies = Arc::from([duplicate, duplicate]);
                }
                _ => unreachable!(),
            }
            resign_evaluations(&mut candidate);

            let rejection = evidence(verify(&candidate, &Cancellation::new()));
            assert!(
                rejection.contains("evaluation") || rejection.contains("typed-root"),
                "typed dependency corruption {corruption} rejected as {rejection}"
            );
        }
    }

    #[test]
    fn evaluation_dependencies_reject_noncanonical_order_and_cycles() {
        let mut noncanonical = completed_fixture_with_dependencies();
        let evaluation = Arc::make_mut(&mut noncanonical.evaluations)
            .iter_mut()
            .find(|evaluation| evaluation.dependencies.len() >= 2)
            .expect("fixture multi-dependency evaluation");
        Arc::make_mut(&mut evaluation.dependencies).swap(0, 1);
        resign_evaluations(&mut noncanonical);
        assert!(
            evidence(verify(&noncanonical, &Cancellation::new())).contains("canonical typed-root")
        );

        let mut cyclic = completed_fixture_with_dependencies();
        let evaluations = Arc::make_mut(&mut cyclic.evaluations);
        let first_root = evaluations[0].root;
        let second_root = evaluations[1].root;
        evaluations[0].authority.dependencies = Arc::from([second_root.root]);
        evaluations[0].dependencies = Arc::from([second_root]);
        evaluations[1].authority.dependencies = Arc::from([first_root.root]);
        evaluations[1].dependencies = Arc::from([first_root]);
        resign_evaluations(&mut cyclic);
        assert!(
            evidence(verify(&cyclic, &Cancellation::new())).contains("evaluation"),
            "cyclic semantic dependency authority must not be publishable"
        );
    }

    #[test]
    fn invalid_executable_demand_is_rejected_by_the_completion_verifier() {
        let mut candidate = completed_fixture();
        let root = candidate.demand.root.raw.identity;
        candidate.demand.executables = candidate
            .demand
            .executables
            .iter()
            .copied()
            .filter(|reference| {
                matches!(reference, ExecutableReference::Specialization(reference) if reference.raw.identity == root)
            })
            .collect::<Vec<_>>()
            .into();

        assert!(
            evidence(verify(&candidate, &Cancellation::new()))
                .contains("exact reachable executable")
        );
    }

    #[test]
    fn executable_references_reject_extra_wrong_kind_cross_context_and_stale_authorities() {
        for corruption in 0..6 {
            let mut candidate = completed_fixture();
            match corruption {
                0 => {
                    let raw = RawReference {
                        context: candidate.context.identity,
                        kind: ArtifactKind::GeneratedExecutable,
                        identity: 99,
                        current_meaning: executable_current_meaning(
                            candidate.program.fingerprint(),
                            ArtifactKind::GeneratedExecutable,
                            99,
                        ),
                    };
                    let mut executables = candidate.demand.executables.to_vec();
                    executables.push(ExecutableReference::Generated(TypedReference {
                        raw,
                        _kind: PhantomData,
                    }));
                    candidate.demand.executables = executables.into();
                }
                1 => {
                    let reference = &mut Arc::make_mut(&mut candidate.demand.executables)[0];
                    match reference {
                        ExecutableReference::Specialization(reference) => {
                            reference.raw.kind = ArtifactKind::TestBody;
                        }
                        _ => panic!("fixture first executable is a Specialization"),
                    }
                }
                2 => {
                    let reference = &mut Arc::make_mut(&mut candidate.demand.executables)[0];
                    match reference {
                        ExecutableReference::Specialization(reference) => {
                            reference.raw.context ^= 1;
                        }
                        _ => panic!("fixture first executable is a Specialization"),
                    }
                }
                3 => {
                    let reference = &mut Arc::make_mut(&mut candidate.demand.executables)[0];
                    match reference {
                        ExecutableReference::Specialization(reference) => {
                            reference.raw.current_meaning ^= 1;
                        }
                        _ => panic!("fixture first executable is a Specialization"),
                    }
                }
                4 => candidate.demand.root.raw.context ^= 1,
                5 => candidate.demand.root.raw.current_meaning ^= 1,
                _ => unreachable!(),
            }
            resign_demand(&mut candidate);

            let rejection = evidence(verify(&candidate, &Cancellation::new()));
            assert!(
                rejection.contains("Executable Demand")
                    || rejection.contains("executable reference"),
                "executable corruption {corruption} rejected as {rejection}"
            );
        }
    }

    #[test]
    fn graph_retained_executable_references_reject_missing_wrong_kind_cross_context_and_stale() {
        for corruption in 0..4 {
            let mut candidate = completed_retained_function_fixture();
            let root = candidate.demand.root.raw.identity;
            let retained_index = candidate
                .demand
                .executables
                .iter()
                .position(|reference| {
                    matches!(reference, ExecutableReference::Specialization(reference) if reference.raw.identity != root)
                })
                .expect("fixture retained callback executable");
            match corruption {
                0 => {
                    let mut executables = candidate.demand.executables.to_vec();
                    executables.remove(retained_index);
                    candidate.demand.executables = executables.into();
                }
                1 => match &mut Arc::make_mut(&mut candidate.demand.executables)[retained_index] {
                    ExecutableReference::Specialization(reference) => {
                        reference.raw.kind = ArtifactKind::TestBody;
                    }
                    _ => unreachable!(),
                },
                2 => match &mut Arc::make_mut(&mut candidate.demand.executables)[retained_index] {
                    ExecutableReference::Specialization(reference) => reference.raw.context ^= 1,
                    _ => unreachable!(),
                },
                3 => match &mut Arc::make_mut(&mut candidate.demand.executables)[retained_index] {
                    ExecutableReference::Specialization(reference) => {
                        reference.raw.current_meaning ^= 1;
                    }
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            }
            resign_demand(&mut candidate);

            let rejection = evidence(verify(&candidate, &Cancellation::new()));
            assert!(
                rejection.contains("exact reachable executable")
                    || rejection.contains("wrong kind")
                    || rejection.contains("crosses compilation contexts")
                    || rejection.contains("stale"),
                "graph-retained executable corruption {corruption} rejected as {rejection}"
            );
        }
    }

    #[test]
    fn producer_and_independent_value_traversals_cover_every_value_variant() {
        use crate::model::{BuiltinVariant, DefinitionId, TestId, VariantId};
        use crate::typed_hir::{ClosureId, LocalId};

        let test = TestId {
            suite: DefinitionId(71),
            test: DefinitionId(72),
            identity: 73,
        };
        let handle = |kind, identity| Value::SymbolicHandle { kind, identity };
        let value = Value::Tuple(Arc::from([
            Value::Array(Arc::from([handle(BuildKind::Image, 11)])),
            Value::BuiltinVariant {
                variant: BuiltinVariant::OptionSome,
                payload: Arc::from([handle(BuildKind::Test, 12)]),
            },
            Value::UserVariant {
                id: VariantId {
                    owner: DefinitionId(81),
                    definition: DefinitionId(82),
                    variant: 83,
                },
                variant_order: 1,
                type_display: Arc::from("Choice"),
                variant_display: Arc::from("Some"),
                payload: Arc::from([handle(BuildKind::Image, 13)]),
            },
            Value::Struct {
                definition: DefinitionId(91),
                type_display: Arc::from("Container"),
                fields: Arc::from([(Arc::from("value"), handle(BuildKind::Test, 14))]),
            },
            Value::TestApplication {
                id: test,
                payload: Arc::from([
                    handle(BuildKind::Image, 15),
                    Value::Function(SpecializationId(101)),
                ]),
            },
            Value::Closure {
                id: ClosureId(201),
                captures: Arc::from([(
                    LocalId(1),
                    Value::Tuple(Arc::from([handle(BuildKind::Test, 16)])),
                )]),
            },
            Value::Function(SpecializationId(102)),
            Value::Unavailable,
            Value::Unit,
            Value::Bool(true),
            Value::Integer {
                kind: crate::model::IntegerType::I64,
                value: 1,
            },
            Value::Float {
                kind: crate::model::FloatType::F64,
                bits: 1,
            },
            Value::Text(Arc::from("text")),
            Value::Scalar('x'),
            Value::Bytes(Arc::from([1_u8])),
        ]));
        let expected_handles = vec![
            (BuildKind::Image, 11),
            (BuildKind::Test, 12),
            (BuildKind::Image, 13),
            (BuildKind::Test, 14),
            (BuildKind::Image, 15),
            (BuildKind::Test, 16),
        ];

        let mut independent = IndependentValueFacts::default();
        produced(independently_traverse_value(
            &value,
            &Cancellation::new(),
            &mut independent,
        ));
        assert_eq!(independent.handles, expected_handles);
        assert_eq!(
            independent.functions,
            BTreeSet::from([SpecializationId(101), SpecializationId(102)])
        );
        assert_eq!(independent.closures, BTreeSet::from([ClosureId(201)]));
        assert_eq!(independent.tests.len(), 1);
        assert_eq!(independent.tests[0].id, test);

        let mut producer_handles = Vec::new();
        assert!(crate::evaluator::visit_construction_handles_cancellable(
            &value,
            &Cancellation::new(),
            &mut |kind, identity| producer_handles.push((kind, identity)),
        ));
        assert_eq!(producer_handles, expected_handles);
        let mut producer_executables = Vec::new();
        assert!(crate::evaluator::visit_retained_executables(
            &value,
            &Cancellation::new(),
            &mut |executable| {
                producer_executables.push(executable);
            },
        ));
        assert_eq!(producer_executables.len(), 3);
        assert_eq!(
            producer_executables
                .iter()
                .filter(|executable| matches!(
                    executable,
                    crate::evaluator::RetainedExecutable::Function(_)
                ))
                .count(),
            2
        );
        assert!(producer_executables.iter().any(|executable| matches!(
            executable,
            crate::evaluator::RetainedExecutable::Closure(ClosureId(201))
        )));
        let mut producer_tests = Vec::new();
        assert!(crate::evaluator::visit_test_applications_cancellable(
            &value,
            &Cancellation::new(),
            &mut |id, payload| producer_tests.push((id, payload.to_vec())),
        ));
        assert_eq!(producer_tests.len(), 1);
        assert_eq!(producer_tests[0].0, test);
    }

    #[test]
    fn current_fixture_has_no_generated_executable_roles() {
        let candidate = completed_fixture();
        assert!(
            candidate
                .demand
                .executables
                .iter()
                .all(|reference| !matches!(reference, ExecutableReference::Generated(_)))
        );
    }

    #[test]
    fn false_context_identity_is_rejected_by_the_completion_verifier() {
        let mut candidate = completed_fixture();
        Arc::make_mut(&mut candidate.context).identity ^= 1;

        assert!(evidence(verify(&candidate, &Cancellation::new())).contains("false compilation"));
    }

    #[test]
    fn false_completed_identity_is_rejected_by_the_completion_verifier() {
        let mut candidate = completed_fixture();
        candidate.fingerprint ^= 1;

        assert!(
            evidence(verify(&candidate, &Cancellation::new())).contains("fingerprint is false")
        );
    }

    #[test]
    fn mismatched_evaluation_table_is_rejected_by_the_completion_verifier() {
        let mut candidate = completed_fixture();
        candidate.evaluations = Arc::from([]);

        assert!(evidence(verify(&candidate, &Cancellation::new())).contains("stale"));
    }

    #[test]
    fn false_resource_custody_receipt_is_rejected_by_the_completion_verifier() {
        let mut candidate = completed_fixture();
        candidate.custody_fingerprint ^= 1;

        assert!(evidence(verify(&candidate, &Cancellation::new())).contains("Resource custody"));
    }

    #[test]
    fn graph_wiring_kind_corruption_is_rejected() {
        fn corrupt(value: &mut Value) -> bool {
            match value {
                Value::SymbolicHandle { kind, .. } => {
                    *kind = BuildKind::Image;
                    true
                }
                Value::Array(values)
                | Value::Tuple(values)
                | Value::BuiltinVariant {
                    payload: values, ..
                }
                | Value::UserVariant {
                    payload: values, ..
                }
                | Value::TestApplication {
                    payload: values, ..
                } => Arc::make_mut(values).iter_mut().any(corrupt),
                Value::Struct { fields, .. } => Arc::make_mut(fields)
                    .iter_mut()
                    .any(|(_, value)| corrupt(value)),
                Value::Closure { captures, .. } => Arc::make_mut(captures)
                    .iter_mut()
                    .any(|(_, value)| corrupt(value)),
                _ => false,
            }
        }
        let mut candidate = completed_fixture();
        let node = Arc::make_mut(&mut candidate.graph.nodes)
            .iter_mut()
            .find(|node| {
                node.operands.iter().any(|operand| {
                    let mut found = false;
                    crate::evaluator::visit_construction_handles(&operand.value, &mut |_, _| {
                        found = true
                    });
                    found
                })
            })
            .expect("fixture graph edge");
        assert!(
            Arc::make_mut(&mut node.operands)
                .iter_mut()
                .any(|operand| corrupt(&mut operand.value))
        );

        assert!(evidence(verify(&candidate, &Cancellation::new())).contains("stale"));
    }

    #[test]
    fn duplicated_test_application_bookkeeping_is_not_graph_authority() {
        let mut candidate = completed_test_fixture();
        Arc::make_mut(&mut candidate.graph.test_applications)[0]
            .payload
            .push(Value::Unit);
        resign_graph(&mut candidate);

        assert!(
            evidence(verify(&candidate, &Cancellation::new()))
                .contains("disagree with typed operands")
        );
    }

    #[test]
    fn typed_references_reject_cross_context_wrong_kind_missing_and_stale_authorities() {
        let context = 7;
        let registry = BTreeMap::from([(ArtifactKind::TypedProgram, 11)]);
        let valid = RawReference {
            context,
            kind: ArtifactKind::TypedProgram,
            identity: 11,
            current_meaning: 11,
        };
        let mut cross_context = valid;
        cross_context.context = 8;
        assert!(
            evidence(verify_reference(
                cross_context,
                context,
                ArtifactKind::TypedProgram,
                &registry,
            ))
            .contains("crosses")
        );
        let mut wrong_kind = valid;
        wrong_kind.kind = ArtifactKind::SolvedFacts;
        assert!(
            evidence(verify_reference(
                wrong_kind,
                context,
                ArtifactKind::TypedProgram,
                &registry,
            ))
            .contains("wrong kind")
        );
        assert!(
            evidence(verify_reference(
                valid,
                context,
                ArtifactKind::TypedProgram,
                &BTreeMap::new(),
            ))
            .contains("missing")
        );
        let mut stale = valid;
        stale.current_meaning = 12;
        assert!(
            evidence(verify_reference(
                stale,
                context,
                ArtifactKind::TypedProgram,
                &registry,
            ))
            .contains("stale")
        );
    }

    #[test]
    fn cancellation_at_entry_and_mid_verification_publishes_no_verified_handoff() {
        for polls in [1, 5] {
            let candidate = completed_fixture();
            let cancellation = Cancellation::new();
            cancellation.cancel_after_private_polls(polls);

            assert!(matches!(
                verify(&candidate, &cancellation),
                Err(CompletionFailure::Cancelled)
            ));
        }
    }

    #[test]
    fn cancellation_interrupts_each_artifact_sized_completion_traversal() {
        let candidate = completed_fixture_with_dependencies();

        for encode in [
            produce_facts_fingerprint
                as fn(&SolvedSemanticFacts, &Cancellation) -> Result<u128, CompletionFailure>,
            verify_facts_fingerprint,
        ] {
            let cancellation = Cancellation::new();
            cancellation.cancel_after_private_polls(3);
            assert!(matches!(
                encode(candidate.facts.as_ref(), &cancellation),
                Err(CompletionFailure::Cancelled)
            ));
        }

        for encode in [
            produce_evaluations_fingerprint
                as fn(&[CompletedEvaluation], &Cancellation) -> Result<u128, CompletionFailure>,
            verify_evaluations_fingerprint,
        ] {
            let cancellation = Cancellation::new();
            cancellation.cancel_after_private_polls(3);
            assert!(matches!(
                encode(candidate.evaluations.as_ref(), &cancellation),
                Err(CompletionFailure::Cancelled)
            ));
        }

        let mut deep_graph = completed_fixture();
        let operand = &mut Arc::make_mut(&mut deep_graph.graph.nodes)
            .first_mut()
            .expect("fixture node")
            .operands;
        let mut value = operand.first().expect("fixture operand").value.clone();
        for _ in 0..64 {
            value = Value::Array(Arc::from([value]));
        }
        Arc::make_mut(operand)
            .first_mut()
            .expect("fixture operand")
            .value = value;
        for verify_encoding in [false, true] {
            let cancellation = Cancellation::new();
            cancellation.cancel_after_private_polls(12);
            let result = if verify_encoding {
                verify_graph_fingerprint(
                    deep_graph.graph.root,
                    &deep_graph.graph.nodes,
                    &deep_graph.graph.test_applications,
                    &cancellation,
                )
            } else {
                produce_graph_fingerprint(
                    deep_graph.graph.root,
                    &deep_graph.graph.nodes,
                    &deep_graph.graph.test_applications,
                    &cancellation,
                )
            };
            assert!(matches!(result, Err(CompletionFailure::Cancelled)));
        }

        let roots = candidate
            .demand
            .executables
            .iter()
            .filter_map(|reference| match reference {
                ExecutableReference::Specialization(reference) => {
                    Some(SpecializationId(reference.raw.identity))
                }
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        for reachability in [
            reachable_specializations
                as fn(
                    &BTreeSet<SpecializationId>,
                    &SolvedSemanticFacts,
                    &Cancellation,
                ) -> Result<BTreeSet<SpecializationId>, CompletionFailure>,
            verify_reachable_specializations,
        ] {
            let cancellation = Cancellation::new();
            cancellation.cancel_after_private_polls(2);
            assert!(matches!(
                reachability(&roots, candidate.facts.as_ref(), &cancellation),
                Err(CompletionFailure::Cancelled)
            ));
        }
    }

    #[test]
    fn canonical_parts_and_graph_edge_counts_are_boundary_safe() {
        let encode = |parts: &[&[u8]]| {
            let mut hash = Xxh3::new();
            for part in parts {
                append_part(&mut hash, part);
            }
            hash.digest128()
        };
        assert_ne!(
            encode(&[b"ab", b"c"]),
            encode(&[b"a", b"bc"]),
            "component boundaries are part of canonical identity"
        );

        let candidate = completed_fixture();
        assert_eq!(
            produced(produce_graph_fingerprint(
                candidate.graph.root,
                &candidate.graph.nodes,
                &candidate.graph.test_applications,
                &Cancellation::new(),
            )),
            produced(verify_graph_fingerprint(
                candidate.graph.root,
                &candidate.graph.nodes,
                &candidate.graph.test_applications,
                &Cancellation::new(),
            )),
            "producer and independent verifier agree on per-node edge framing"
        );
    }
}
