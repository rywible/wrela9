#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use xxhash_rust::xxh3::Xxh3;

use crate::compiler::{
    Cancellation, CanonicalValue, CompletedSemanticProgramObservation,
    CompletedSemanticProgramValues, EvaluationObservation, EvaluationPolicy, Root,
};
use crate::evaluator::{Construction, ConstructionOperand};
use crate::identity::IdentityCatalog;
use crate::image_evaluation::SealedImage;
use crate::model::{BuildKind, SpecializationId};
use crate::semantic_facts::{FunctionFacts, SolvedSemanticFacts};
use crate::typed_hir::{AccessMode, EvaluationRoot, VerifiedProgram};

pub(crate) const PHASE_SCHEMA: &str = "wrela.completed-semantic-program.v1";
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
    pub(crate) evaluations: Vec<EvaluationObservation>,
    pub(crate) image: SealedImage,
    pub(crate) image_specialization: SpecializationId,
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
    edges: Arc<[GraphEdge]>,
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
    specializations: Arc<[TypedReference<SpecializationAuthority>]>,
    fingerprint: u128,
}

#[derive(Clone, Debug)]
struct VerifiedMarker;

#[derive(Clone, Debug, PartialEq, Eq)]
struct CompletedEvaluation {
    value: CanonicalValue,
    policy: EvaluationPolicy,
    root: EvaluationRoot,
    argument_fingerprint: u128,
    evaluator_eligible: bool,
    dependency_roots: Arc<[u128]>,
    typed_program_fingerprint: u128,
    tariff_schema: Arc<str>,
    fuel_used: u64,
    peak_memory: u64,
}

#[derive(Clone)]
pub(crate) struct CompletedSemanticProgram {
    context: Arc<CompilationContext>,
    program: Arc<VerifiedProgram>,
    facts: Arc<SolvedSemanticFacts>,
    evaluations: Arc<[CompletedEvaluation]>,
    graph: SealedConstructionGraph,
    demand: ExecutableDemand,
    receipts: DirectReceipts,
    custody_fingerprint: u128,
    fingerprint: u128,
    _verified: VerifiedMarker,
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
            executable_count: self.demand.specializations.len(),
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
    pub(crate) fn executable_specializations(&self) -> impl Iterator<Item = SpecializationId> + '_ {
        self.demand
            .specializations
            .iter()
            .map(|reference| SpecializationId(reference.raw.identity))
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
    let context = Arc::new(CompilationContext {
        identity: context_identity,
        distribution_digest: input.context.distribution_digest,
        semantic_closure_digest: input.context.semantic_closure_digest,
        identity_catalog_revision,
        root: input.context.root,
        identity_catalog: input.identity_catalog,
    });

    let program_fingerprint = input.program.fingerprint();
    let facts_fingerprint = produce_facts_fingerprint(&input.facts);
    let evaluations = retain_completed_evaluations(
        &input.evaluations,
        &input.program,
        input.image_specialization,
    )?;
    let evaluations_fingerprint = produce_evaluations_fingerprint(&evaluations);
    let demand = produce_demand(
        context_identity,
        input.image_specialization,
        &input.program,
        &input.facts,
        &input.image,
        &input.image.test_applications,
    )?;
    let graph = produce_graph(context_identity, &input.image, &demand, program_fingerprint)?;
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
    let fingerprint =
        produce_completed_fingerprint(context_identity, &receipts, custody_fingerprint);
    let candidate = CompletedSemanticProgram {
        context,
        program: input.program,
        facts: input.facts,
        evaluations: evaluations.into(),
        graph,
        demand,
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

    let facts_fingerprint = verify_facts_fingerprint(&candidate.facts);
    let evaluations_fingerprint = verify_evaluations_fingerprint(&candidate.evaluations);
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
    verify_fact_coverage(&candidate.program, &candidate.facts)?;
    verify_evaluation_coverage(
        &candidate.program,
        SpecializationId(candidate.demand.root.raw.identity),
        &candidate.evaluations,
    )?;
    verify_demand(candidate)?;
    verify_graph(candidate)?;

    let reconstructed = verify_completed_fingerprint(
        candidate.context.identity,
        &candidate.receipts,
        candidate.custody_fingerprint,
    );
    if reconstructed != candidate.fingerprint {
        return defect("Completed Semantic Program fingerprint is false");
    }
    Ok(())
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

fn produce_demand(
    context: u128,
    root: SpecializationId,
    program: &VerifiedProgram,
    facts: &SolvedSemanticFacts,
    image: &SealedImage,
    test_applications: &[crate::evaluator::AppliedTest],
) -> Result<ExecutableDemand, CompletionFailure> {
    if !program.specializations().contains_key(&root) {
        return defect("Image Constructor has no concrete Specialization");
    }
    let mut roots = image
        .constructions()
        .iter()
        .map(|construction| SpecializationId(construction.owner))
        .collect::<BTreeSet<_>>();
    roots.insert(root);
    for application in test_applications {
        let Some(test_roots) =
            crate::semantic_facts::test_specialization_demands(program, application.id)
        else {
            return defect("Test Application names a missing verified Test body");
        };
        roots.extend(test_roots);
    }
    let identities = reachable_specializations(&roots, facts)?;
    let references = identities
        .iter()
        .map(|identity| {
            TypedReference::new(
                context,
                ArtifactKind::Specialization,
                identity.0,
                specialization_current_meaning(program.fingerprint(), *identity),
            )
        })
        .collect::<Vec<_>>();
    let fingerprint = produce_demand_fingerprint(root, &references);
    Ok(ExecutableDemand {
        root: references
            .iter()
            .find(|reference| reference.raw.identity == root.0)
            .copied()
            .ok_or_else(|| CompletionFailure::Defect(Arc::from("demand omitted its root")))?,
        specializations: references.into(),
        fingerprint,
    })
}

fn reachable_specializations(
    roots: &BTreeSet<SpecializationId>,
    facts: &SolvedSemanticFacts,
) -> Result<BTreeSet<SpecializationId>, CompletionFailure> {
    let mut reachable = BTreeSet::new();
    let mut pending = roots.iter().rev().copied().collect::<Vec<_>>();
    while let Some(identity) = pending.pop() {
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
) -> Result<SealedConstructionGraph, CompletionFailure> {
    let demand_set = demand
        .specializations
        .iter()
        .map(|reference| reference.raw.identity)
        .collect::<BTreeSet<_>>();
    let mut local_fingerprints = BTreeMap::new();
    for construction in image.constructions() {
        let local = produce_node_local_fingerprint(construction);
        if local_fingerprints
            .insert(construction.identity, local)
            .is_some()
        {
            return defect("construction graph repeats an identity");
        }
    }
    let mut nodes = Vec::new();
    for construction in image.constructions() {
        if !demand_set.contains(&construction.owner) {
            return defect("construction owner is outside Executable Demand");
        }
        let owner_meaning = specialization_current_meaning(
            program_fingerprint,
            SpecializationId(construction.owner),
        );
        let mut edges = Vec::new();
        for operand in &construction.operands {
            collect_operand_edges(context, operand, &local_fingerprints, &mut edges)?;
        }
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
            edges: edges.into(),
            local_fingerprint: local_fingerprints[&construction.identity],
        });
    }
    nodes.sort_by_key(|node| node.identity);
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
    let fingerprint = produce_graph_fingerprint(root, &nodes, &image.test_applications);
    Ok(SealedConstructionGraph {
        root,
        nodes: nodes.into(),
        test_applications: image.test_applications.clone().into(),
        fingerprint,
    })
}

fn collect_operand_edges(
    context: u128,
    operand: &ConstructionOperand,
    local_fingerprints: &BTreeMap<u128, u128>,
    edges: &mut Vec<GraphEdge>,
) -> Result<(), CompletionFailure> {
    for handle in &operand.handles {
        let current = local_fingerprints
            .get(&handle.identity)
            .copied()
            .ok_or_else(|| {
                CompletionFailure::Defect(Arc::from("graph handle names a missing node"))
            })?;
        edges.push(GraphEdge {
            label: Arc::clone(&operand.label),
            ordinal: u32::try_from(edges.len()).unwrap_or(u32::MAX),
            ownership: operand.ownership,
            target_kind: handle.kind,
            target: TypedReference::new(
                context,
                ArtifactKind::ConstructionNode,
                handle.identity,
                current,
            ),
        });
    }
    Ok(())
}

fn verify_fact_coverage(
    program: &VerifiedProgram,
    facts: &SolvedSemanticFacts,
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
) -> Result<(), CompletionFailure> {
    let expected = program.expected_evaluation_roots(image);
    let mut supplied = BTreeSet::new();
    for evaluation in evaluations {
        if !supplied.insert(evaluation.root) {
            return defect("evaluation table repeats a root");
        }
        if evaluation.typed_program_fingerprint != program.fingerprint() {
            return defect("evaluation receipt names a different Typed Program");
        }
    }
    if supplied != expected {
        return defect("evaluation coverage disagrees with verified Typed Program");
    }
    Ok(())
}

fn retain_completed_evaluations(
    observations: &[EvaluationObservation],
    program: &VerifiedProgram,
    image: SpecializationId,
) -> Result<Vec<CompletedEvaluation>, CompletionFailure> {
    observations
        .iter()
        .map(|observation| {
            let crate::EvaluationOutcome::Completed(value) = observation.outcome() else {
                return defect("completed handoff retains an incomplete evaluation outcome");
            };
            let receipt = observation.receipt();
            let Some(root) =
                program.evaluation_root(receipt.policy(), receipt.root_identity(), image)
            else {
                return defect("evaluation receipt names a missing or wrong-kind typed root");
            };
            Ok(CompletedEvaluation {
                value: value.clone(),
                policy: receipt.policy(),
                root,
                argument_fingerprint: receipt.argument_fingerprint(),
                evaluator_eligible: receipt.evaluator_eligible(),
                dependency_roots: Arc::from(receipt.dependency_roots()),
                typed_program_fingerprint: receipt.typed_hir_fingerprint(),
                tariff_schema: Arc::from(receipt.tariff_schema()),
                fuel_used: receipt.fuel_used(),
                peak_memory: receipt.peak_memory(),
            })
        })
        .collect()
}

fn verify_demand(candidate: &CompletedSemanticProgram) -> Result<(), CompletionFailure> {
    let root = SpecializationId(candidate.demand.root.raw.identity);
    let mut roots = candidate
        .graph
        .nodes
        .iter()
        .map(|node| SpecializationId(node.owner.raw.identity))
        .collect::<BTreeSet<_>>();
    roots.insert(root);
    for application in candidate.graph.test_applications.iter() {
        let Some(test_roots) =
            crate::semantic_facts::test_specialization_demands(&candidate.program, application.id)
        else {
            return defect("Test Application names a missing verified Test body");
        };
        roots.extend(test_roots);
    }
    let exact = reachable_specializations(&roots, &candidate.facts)?;
    let supplied = candidate
        .demand
        .specializations
        .iter()
        .map(|reference| SpecializationId(reference.raw.identity))
        .collect::<BTreeSet<_>>();
    if exact != supplied {
        return defect("Executable Demand is not the exact reachable Specialization family");
    }
    let registry = candidate
        .program
        .specializations()
        .keys()
        .map(|identity| {
            (
                identity.0,
                specialization_current_meaning(candidate.program.fingerprint(), *identity),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for reference in candidate.demand.specializations.iter() {
        verify_semantic_reference(
            reference.raw,
            candidate.context.identity,
            ArtifactKind::Specialization,
            &registry,
        )?;
    }
    if verify_demand_fingerprint(root, &candidate.demand.specializations)
        != candidate.demand.fingerprint
    {
        return defect("Executable Demand fingerprint is false");
    }
    Ok(())
}

fn verify_graph(candidate: &CompletedSemanticProgram) -> Result<(), CompletionFailure> {
    let node_registry = candidate
        .graph
        .nodes
        .iter()
        .map(|node| (node.identity, node.local_fingerprint))
        .collect::<BTreeMap<_, _>>();
    verify_semantic_reference(
        candidate.graph.root.raw,
        candidate.context.identity,
        ArtifactKind::ConstructionNode,
        &node_registry,
    )?;
    let root = candidate.graph.root.raw.identity;
    let Some(root_node) = candidate
        .graph
        .nodes
        .iter()
        .find(|node| node.identity == root)
    else {
        return defect("construction graph root is missing");
    };
    if root_node.kind != BuildKind::Image {
        return defect("construction graph root has the wrong kind");
    }
    if candidate
        .graph
        .nodes
        .iter()
        .filter(|node| node.kind == BuildKind::Image)
        .count()
        != 1
    {
        return defect("construction graph has a false Image root identity");
    }
    let demand_registry = candidate
        .demand
        .specializations
        .iter()
        .map(|reference| (reference.raw.identity, reference.raw.current_meaning))
        .collect::<BTreeMap<_, _>>();
    let mut applied_tests = BTreeSet::new();
    for application in candidate.graph.test_applications.iter() {
        if candidate.program.test(application.id).is_none() || !applied_tests.insert(application.id)
        {
            return defect("sealed graph has a missing or duplicate Test Application");
        }
    }
    let mut incoming_moves = BTreeMap::<u128, usize>::new();
    let mut adjacency = BTreeMap::<u128, BTreeSet<u128>>::new();
    for node in candidate.graph.nodes.iter() {
        if verify_node_local_fingerprint(node) != node.local_fingerprint {
            return defect("construction node current meaning is stale");
        }
        verify_semantic_reference(
            node.owner.raw,
            candidate.context.identity,
            ArtifactKind::Specialization,
            &demand_registry,
        )?;
        let reconstructed =
            reconstruct_edges(candidate.context.identity, &node.operands, &node_registry)?;
        if reconstructed != node.edges.as_ref() {
            return defect("construction graph wiring disagrees with typed operands");
        }
        for edge in node.edges.iter() {
            verify_semantic_reference(
                edge.target.raw,
                candidate.context.identity,
                ArtifactKind::ConstructionNode,
                &node_registry,
            )?;
            let target = candidate
                .graph
                .nodes
                .iter()
                .find(|target| target.identity == edge.target.raw.identity)
                .ok_or_else(|| CompletionFailure::Defect(Arc::from("missing graph target")))?;
            if target.kind != edge.target_kind {
                return defect("construction handle refers to a node of the wrong kind");
            }
            if edge.ownership == AccessMode::Move {
                *incoming_moves.entry(target.identity).or_default() += 1;
            }
            adjacency
                .entry(node.identity)
                .or_default()
                .insert(target.identity);
        }
    }
    if incoming_moves.values().any(|count| *count > 1) {
        return defect("construction graph duplicates Resource custody");
    }
    let reachable = crate::graph::reachable_from(root, &adjacency);
    if reachable != node_registry.keys().copied().collect() {
        return defect("construction graph contains unreachable nodes");
    }
    if verify_graph_fingerprint(
        candidate.graph.root,
        &candidate.graph.nodes,
        &candidate.graph.test_applications,
    ) != candidate.graph.fingerprint
    {
        return defect("construction graph fingerprint is false");
    }
    Ok(())
}

fn reconstruct_edges(
    context: u128,
    operands: &[ConstructionOperand],
    registry: &BTreeMap<u128, u128>,
) -> Result<Vec<GraphEdge>, CompletionFailure> {
    let mut edges = Vec::new();
    for operand in operands {
        collect_operand_edges(context, operand, registry, &mut edges)?;
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
    hash.update(b"wrela.specialization-reference\0\x01");
    hash.update(&program_fingerprint.to_be_bytes());
    hash.update(&identity.0.to_be_bytes());
    hash.digest128()
}

fn produce_context_identity(input: ContextInput, catalog: u128) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(b"wrela.compilation-context\0\x01");
    hash.update(COMPILER_SCHEMA.as_bytes());
    hash.update(SEMANTIC_SCHEMA.as_bytes());
    hash.update(&input.distribution_digest.to_be_bytes());
    hash.update(&input.semantic_closure_digest.to_be_bytes());
    hash.update(&catalog.to_be_bytes());
    hash.update(&[root_tag(input.root)]);
    hash.digest128()
}

fn verify_context_identity(input: ContextInput, catalog: u128) -> u128 {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"wrela.compilation-context\0\x01");
    bytes.extend_from_slice(COMPILER_SCHEMA.as_bytes());
    bytes.extend_from_slice(SEMANTIC_SCHEMA.as_bytes());
    bytes.extend_from_slice(&input.distribution_digest.to_be_bytes());
    bytes.extend_from_slice(&input.semantic_closure_digest.to_be_bytes());
    bytes.extend_from_slice(&catalog.to_be_bytes());
    bytes.push(root_tag(input.root));
    xxhash_rust::xxh3::xxh3_128(&bytes)
}

const fn root_tag(root: Root) -> u8 {
    match root {
        Root::Image => 1,
        Root::Test => 2,
    }
}

fn produce_facts_fingerprint(facts: &SolvedSemanticFacts) -> u128 {
    hash_facts(b"wrela.solved-semantic-facts\0\x01", facts)
}

fn verify_facts_fingerprint(facts: &SolvedSemanticFacts) -> u128 {
    hash_facts(b"wrela.solved-semantic-facts\0\x01", facts)
}

fn hash_facts(domain: &[u8], facts: &SolvedSemanticFacts) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(domain);
    hash.update(
        &u64::try_from(facts.definitions.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for (identity, function) in &facts.definitions {
        hash.update(&[1]);
        hash.update(&identity.0.to_be_bytes());
        append_function_facts(&mut hash, function);
    }
    hash.update(
        &u64::try_from(facts.specializations.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for (identity, function) in &facts.specializations {
        hash.update(&[2]);
        hash.update(&identity.0.to_be_bytes());
        append_function_facts(&mut hash, function);
    }
    hash.update(
        &u64::try_from(facts.recursion.proven.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for (identity, maximum) in &facts.recursion.proven {
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
        hash.update(&inferred.specialization_identity().to_be_bytes());
        append_part(&mut hash, inferred.function().as_bytes());
        append_part(&mut hash, inferred.error_type().as_bytes());
    }
    hash.update(
        &u64::try_from(facts.diagnostics.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    hash.digest128()
}

fn append_function_facts(hash: &mut Xxh3, facts: &FunctionFacts) {
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
        append_build_kind(hash, *kind);
    }
    hash.update(
        &u64::try_from(facts.calls.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for (identity, multiplicity) in &facts.calls {
        hash.update(&identity.0.to_be_bytes());
        hash.update(&multiplicity.to_be_bytes());
    }
    hash.update(
        &u64::try_from(facts.specialization_calls.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for (identity, multiplicity) in &facts.specialization_calls {
        hash.update(&identity.0.to_be_bytes());
        hash.update(&multiplicity.to_be_bytes());
    }
}

fn produce_evaluations_fingerprint(evaluations: &[CompletedEvaluation]) -> u128 {
    hash_evaluations(b"wrela.semantic-evaluation-table\0\x01", evaluations)
}

fn verify_evaluations_fingerprint(evaluations: &[CompletedEvaluation]) -> u128 {
    hash_evaluations(b"wrela.semantic-evaluation-table\0\x01", evaluations)
}

fn hash_evaluations(domain: &[u8], evaluations: &[CompletedEvaluation]) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(domain);
    hash.update(
        &u64::try_from(evaluations.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for evaluation in evaluations {
        append_canonical_value(&mut hash, &evaluation.value);
        hash.update(&[evaluation.policy as u8]);
        hash.update(&[evaluation.root.tag()]);
        hash.update(&evaluation.root.identity().to_be_bytes());
        hash.update(&evaluation.argument_fingerprint.to_be_bytes());
        hash.update(&[u8::from(evaluation.evaluator_eligible)]);
        hash.update(&evaluation.typed_program_fingerprint.to_be_bytes());
        append_part(&mut hash, evaluation.tariff_schema.as_bytes());
        hash.update(&evaluation.fuel_used.to_be_bytes());
        hash.update(&evaluation.peak_memory.to_be_bytes());
        hash.update(
            &u64::try_from(evaluation.dependency_roots.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        for dependency in evaluation.dependency_roots.iter() {
            hash.update(&dependency.to_be_bytes());
        }
    }
    hash.digest128()
}

fn produce_node_local_fingerprint(construction: &Construction) -> u128 {
    hash_construction(b"wrela.construction-node\0\x01", construction)
}

fn verify_node_local_fingerprint(node: &GraphNode) -> u128 {
    let construction = Construction {
        identity: node.identity,
        kind: node.kind,
        owner: node.owner.raw.identity,
        site: node.site.clone(),
        operands: node.operands.to_vec(),
    };
    hash_construction(b"wrela.construction-node\0\x01", &construction)
}

fn hash_construction(domain: &[u8], construction: &Construction) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(domain);
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
        append_part(&mut hash, operand.label.as_bytes());
        hash.update(&[access_tag(operand.ownership)]);
        append_canonical_value(&mut hash, &operand.value);
        hash.update(
            &u64::try_from(operand.handles.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        for handle in &operand.handles {
            append_build_kind(&mut hash, handle.kind);
            hash.update(&handle.identity.to_be_bytes());
        }
    }
    hash.digest128()
}

fn produce_graph_fingerprint(
    root: TypedReference<ConstructionNodeAuthority>,
    nodes: &[GraphNode],
    test_applications: &[crate::evaluator::AppliedTest],
) -> u128 {
    hash_graph(
        b"wrela.sealed-construction-graph\0\x01",
        root,
        nodes,
        test_applications,
    )
}

fn verify_graph_fingerprint(
    root: TypedReference<ConstructionNodeAuthority>,
    nodes: &[GraphNode],
    test_applications: &[crate::evaluator::AppliedTest],
) -> u128 {
    hash_graph(
        b"wrela.sealed-construction-graph\0\x01",
        root,
        nodes,
        test_applications,
    )
}

fn hash_graph(
    domain: &[u8],
    root: TypedReference<ConstructionNodeAuthority>,
    nodes: &[GraphNode],
    test_applications: &[crate::evaluator::AppliedTest],
) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(domain);
    hash.update(&root.raw.identity.to_be_bytes());
    hash.update(&u64::try_from(nodes.len()).unwrap_or(u64::MAX).to_be_bytes());
    for node in nodes {
        hash.update(&node.identity.to_be_bytes());
        hash.update(&node.local_fingerprint.to_be_bytes());
        for edge in node.edges.iter() {
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
        hash.update(&application.id.identity.to_be_bytes());
        hash.update(
            &u64::try_from(application.payload.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        for value in &application.payload {
            append_canonical_value(&mut hash, value);
        }
    }
    hash.digest128()
}

fn produce_demand_fingerprint(
    root: SpecializationId,
    specializations: &[TypedReference<SpecializationAuthority>],
) -> u128 {
    hash_demand(b"wrela.executable-demand\0\x01", root, specializations)
}

fn verify_demand_fingerprint(
    root: SpecializationId,
    specializations: &[TypedReference<SpecializationAuthority>],
) -> u128 {
    hash_demand(b"wrela.executable-demand\0\x01", root, specializations)
}

fn hash_demand(
    domain: &[u8],
    root: SpecializationId,
    specializations: &[TypedReference<SpecializationAuthority>],
) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(domain);
    hash.update(&root.0.to_be_bytes());
    hash.update(
        &u64::try_from(specializations.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for specialization in specializations {
        hash.update(&specialization.raw.identity.to_be_bytes());
        hash.update(&specialization.raw.current_meaning.to_be_bytes());
    }
    hash.digest128()
}

fn produce_completed_fingerprint(context: u128, receipts: &DirectReceipts, custody: u128) -> u128 {
    hash_completed(
        b"wrela.completed-semantic-program\0\x01",
        context,
        receipts,
        custody,
    )
}

fn verify_completed_fingerprint(context: u128, receipts: &DirectReceipts, custody: u128) -> u128 {
    hash_completed(
        b"wrela.completed-semantic-program\0\x01",
        context,
        receipts,
        custody,
    )
}

fn hash_completed(domain: &[u8], context: u128, receipts: &DirectReceipts, custody: u128) -> u128 {
    let mut hash = Xxh3::new();
    hash.update(domain);
    hash.update(PHASE_SCHEMA.as_bytes());
    hash.update(&context.to_be_bytes());
    for reference in [
        receipts.program.raw,
        receipts.facts.raw,
        receipts.evaluations.raw,
        receipts.graph.raw,
        receipts.demand.raw,
    ] {
        hash.update(&[reference.kind.tag()]);
        hash.update(&reference.identity.to_be_bytes());
        hash.update(&reference.current_meaning.to_be_bytes());
    }
    hash.update(&custody.to_be_bytes());
    hash.digest128()
}

fn append_part(hash: &mut Xxh3, bytes: &[u8]) {
    hash.update(&u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
    hash.update(bytes);
}

fn append_canonical_value(hash: &mut Xxh3, value: &CanonicalValue) {
    match value {
        CanonicalValue::Unit => hash.update(&[0]),
        CanonicalValue::Bool(value) => hash.update(&[1, u8::from(*value)]),
        CanonicalValue::Integer { type_name, value } => {
            hash.update(&[2]);
            append_part(hash, type_name.as_bytes());
            hash.update(&value.to_be_bytes());
        }
        CanonicalValue::Float { type_name, bits } => {
            hash.update(&[3]);
            append_part(hash, type_name.as_bytes());
            hash.update(&bits.to_be_bytes());
        }
        CanonicalValue::Text(value) => {
            hash.update(&[4]);
            append_part(hash, value.as_bytes());
        }
        CanonicalValue::Scalar(value) => {
            hash.update(&[5]);
            hash.update(&u32::from(*value).to_be_bytes());
        }
        CanonicalValue::Bytes(value) => {
            hash.update(&[6]);
            append_part(hash, value);
        }
        CanonicalValue::Function { identity } => {
            hash.update(&[7]);
            hash.update(&identity.to_be_bytes());
        }
        CanonicalValue::Closure { identity, captures } => {
            hash.update(&[8]);
            hash.update(&identity.to_be_bytes());
            append_values(hash, captures);
        }
        CanonicalValue::Tuple(values) => {
            hash.update(&[9]);
            append_values(hash, values);
        }
        CanonicalValue::Array(values) => {
            hash.update(&[10]);
            append_values(hash, values);
        }
        CanonicalValue::Variant {
            type_name,
            variant,
            payload,
        } => {
            hash.update(&[11]);
            append_part(hash, type_name.as_bytes());
            append_part(hash, variant.as_bytes());
            append_values(hash, payload);
        }
        CanonicalValue::Struct { type_name, fields } => {
            hash.update(&[12]);
            append_part(hash, type_name.as_bytes());
            hash.update(
                &u64::try_from(fields.len())
                    .unwrap_or(u64::MAX)
                    .to_be_bytes(),
            );
            for (name, value) in fields.iter() {
                append_part(hash, name.as_bytes());
                append_canonical_value(hash, value);
            }
        }
        CanonicalValue::SymbolicHandle { kind, identity } => {
            hash.update(&[13]);
            match kind {
                crate::ConstructionKind::Image => hash.update(&[1]),
                crate::ConstructionKind::Test => hash.update(&[2]),
                crate::ConstructionKind::Node { type_identity } => {
                    hash.update(&[3]);
                    hash.update(&type_identity.to_be_bytes());
                }
            }
            hash.update(&identity.to_be_bytes());
        }
    }
}

fn append_values(hash: &mut Xxh3, values: &[CanonicalValue]) {
    hash.update(
        &u64::try_from(values.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for value in values {
        append_canonical_value(hash, value);
    }
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
        let source = br#"from runtime import topology

pure fn leaf() -> topology.Node:
    return topology.Node.new(children=[])

@image
fn build() -> Image:
    node = leaf()
    return Image.new(node=node)
"#;
        let outcome = compiler.compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
                Root::Image,
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
    fn invalid_executable_demand_is_rejected_by_the_completion_verifier() {
        let mut candidate = completed_fixture();
        let root = candidate.demand.root.raw.identity;
        candidate.demand.specializations = candidate
            .demand
            .specializations
            .iter()
            .copied()
            .filter(|reference| reference.raw.identity == root)
            .collect::<Vec<_>>()
            .into();

        assert!(
            evidence(verify(&candidate, &Cancellation::new()))
                .contains("exact reachable Specialization")
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
        let mut candidate = completed_fixture();
        let node = Arc::make_mut(&mut candidate.graph.nodes)
            .iter_mut()
            .find(|node| !node.edges.is_empty())
            .expect("fixture graph edge");
        Arc::make_mut(&mut node.edges)[0].target_kind = BuildKind::Image;

        assert!(evidence(verify(&candidate, &Cancellation::new())).contains("wiring"));
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
}
