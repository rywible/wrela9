#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::compiler::{
    ConstructionKind, ConstructionObservation, EvaluationOutcome, EvaluationReceipt,
};
use crate::evaluator::{AppliedTest, Construction, Run};
use crate::model::BuildKind;

pub(crate) struct FinishedImageEvaluation {
    pub(crate) outcome: EvaluationOutcome,
    pub(crate) receipt: EvaluationReceipt,
    pub(crate) status: ImageEvaluationStatus,
}

pub(crate) enum ImageEvaluationStatus {
    NotCompleted,
    Sealed(SealedImage),
    Invalid(GraphSealFailure),
}

pub(crate) struct SealedImage {
    root: u128,
    constructions: Vec<Construction>,
    pub(crate) test_applications: Vec<AppliedTest>,
}

impl SealedImage {
    pub(crate) const fn root(&self) -> u128 {
        self.root
    }

    pub(crate) fn constructions(&self) -> &[Construction] {
        &self.constructions
    }

    pub(crate) fn observations(&self) -> Vec<ConstructionObservation> {
        self.constructions
            .iter()
            .map(|construction| {
                ConstructionObservation::new(
                    construction.identity,
                    match construction.kind {
                        BuildKind::Image => ConstructionKind::Image,
                        BuildKind::Test => ConstructionKind::Test,
                        BuildKind::Node { type_identity, .. } => ConstructionKind::Node {
                            type_identity: type_identity.0,
                        },
                    },
                    construction.site.clone(),
                    construction
                        .operands
                        .iter()
                        .flat_map(|operand| operand.handles.iter().map(|handle| handle.identity))
                        .collect(),
                    construction
                        .operands
                        .iter()
                        .map(|operand| {
                            crate::ConstructionOperandObservation::new(
                                Arc::clone(&operand.label),
                                operand.value.clone(),
                            )
                        })
                        .collect(),
                )
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GraphSealFailure {
    Creator(&'static str),
    Defect(&'static str),
}

pub(crate) fn finish(run: Run) -> FinishedImageEvaluation {
    let status = if matches!(run.outcome, EvaluationOutcome::Completed(_)) {
        match seal_construction_graph(run.root_handle, &run.constructions) {
            Ok(()) => ImageEvaluationStatus::Sealed(SealedImage {
                root: run.root_handle.expect("sealed graph has a root").1,
                constructions: run.constructions,
                test_applications: run.test_applications,
            }),
            Err(failure) => ImageEvaluationStatus::Invalid(failure),
        }
    } else {
        ImageEvaluationStatus::NotCompleted
    };
    FinishedImageEvaluation {
        outcome: run.outcome,
        receipt: run.receipt,
        status,
    }
}

fn seal_construction_graph(
    root_handle: Option<(BuildKind, u128)>,
    constructions: &[Construction],
) -> Result<(), GraphSealFailure> {
    let Some((kind, root)) = root_handle else {
        return Err(GraphSealFailure::Defect(
            "Image evaluation produced no root",
        ));
    };
    if kind != BuildKind::Image {
        return Err(GraphSealFailure::Creator("returned_root_is_not_image"));
    }
    if constructions
        .iter()
        .filter(|construction| construction.kind == BuildKind::Image)
        .count()
        != 1
    {
        return Err(GraphSealFailure::Creator("multiple_image_roots"));
    }
    let mut graph = BTreeMap::new();
    for construction in constructions {
        if graph
            .insert(
                construction.identity,
                construction
                    .operands
                    .iter()
                    .flat_map(|operand| operand.handles.iter().map(|handle| handle.identity))
                    .collect::<Vec<_>>(),
            )
            .is_some()
        {
            return Err(GraphSealFailure::Defect(
                "duplicate Construction identity escaped the evaluator catalog",
            ));
        }
    }
    if !graph.contains_key(&root) {
        return Err(GraphSealFailure::Defect(
            "returned Image root does not name a construction",
        ));
    }
    if graph
        .values()
        .flat_map(|edges| edges.iter())
        .any(|edge| !graph.contains_key(edge))
    {
        return Err(GraphSealFailure::Defect(
            "construction edge names a node outside its evaluation root",
        ));
    }
    let mut reachable = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(identity) = pending.pop() {
        if reachable.insert(identity)
            && let Some(edges) = graph.get(&identity)
        {
            pending.extend(edges.iter().copied());
        }
    }
    if reachable.len() != constructions.len() {
        return Err(GraphSealFailure::Creator("unreachable_construction"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::{EvaluationPolicy, EvaluationReceipt, SourceRange};
    use crate::evaluator::{ConstructionHandle, ConstructionOperand};
    use crate::typed_hir::AccessMode;

    fn operand(edges: Vec<u128>) -> ConstructionOperand {
        ConstructionOperand {
            label: Arc::from("edge"),
            ownership: AccessMode::Copy,
            value: crate::CanonicalValue::Unit,
            handles: edges
                .into_iter()
                .map(|identity| ConstructionHandle {
                    kind: BuildKind::Test,
                    identity,
                })
                .collect(),
        }
    }

    fn completed_run(
        root_handle: Option<(BuildKind, u128)>,
        constructions: Vec<Construction>,
    ) -> Run {
        Run {
            outcome: EvaluationOutcome::Completed(crate::CanonicalValue::Unit),
            receipt: EvaluationReceipt::new(
                EvaluationPolicy::ImageConstructor,
                0,
                Vec::new(),
                0,
                0,
                0,
            ),
            constructions,
            test_applications: Vec::new(),
            root_handle,
        }
    }

    #[test]
    fn noncompleted_runs_cannot_reach_graph_sealing() {
        let run = Run {
            outcome: EvaluationOutcome::CreatorRejected {
                kind: crate::EvaluationRejectionKind::UnresolvedCall,
            },
            receipt: EvaluationReceipt::new(
                EvaluationPolicy::ImageConstructor,
                0,
                Vec::new(),
                0,
                0,
                0,
            ),
            constructions: Vec::new(),
            test_applications: Vec::new(),
            root_handle: None,
        };
        assert!(matches!(
            finish(run).status,
            ImageEvaluationStatus::NotCompleted
        ));
    }

    #[test]
    fn missing_compiler_produced_root_is_a_defect() {
        assert!(matches!(
            finish(completed_run(None, Vec::new())).status,
            ImageEvaluationStatus::Invalid(GraphSealFailure::Defect(
                "Image evaluation produced no root"
            ))
        ));
    }

    #[test]
    fn unknown_duplicate_and_cross_root_identities_are_contained() {
        let site = SourceRange::new("src/image.wr", 0, 1);
        let node = |identity, edges| Construction {
            identity,
            kind: BuildKind::Image,
            owner: 1,
            site: site.clone(),
            operands: vec![operand(edges)],
        };
        assert!(matches!(
            finish(completed_run(
                Some((BuildKind::Image, 2)),
                vec![node(1, vec![])]
            ))
            .status,
            ImageEvaluationStatus::Invalid(GraphSealFailure::Defect(_))
        ));
        assert!(matches!(
            finish(completed_run(
                Some((BuildKind::Image, 1)),
                vec![node(1, vec![]), node(1, vec![])]
            ))
            .status,
            ImageEvaluationStatus::Invalid(GraphSealFailure::Creator("multiple_image_roots"))
                | ImageEvaluationStatus::Invalid(GraphSealFailure::Defect(_))
        ));
        assert!(matches!(
            finish(completed_run(
                Some((BuildKind::Image, 1)),
                vec![node(1, vec![9])]
            ))
            .status,
            ImageEvaluationStatus::Invalid(GraphSealFailure::Defect(_))
        ));
    }

    #[test]
    fn graph_sealing_is_cycle_safe_and_rejects_only_unreachable_nodes() {
        let site = SourceRange::new("src/image.wr", 0, 1);
        let cycle = vec![
            Construction {
                identity: 1,
                kind: BuildKind::Image,
                owner: 1,
                site: site.clone(),
                operands: vec![operand(vec![2])],
            },
            Construction {
                identity: 2,
                kind: BuildKind::Test,
                owner: 1,
                site: site.clone(),
                operands: vec![operand(vec![1])],
            },
        ];
        assert!(matches!(
            finish(completed_run(Some((BuildKind::Image, 1)), cycle.clone())).status,
            ImageEvaluationStatus::Sealed(_)
        ));
        let mut unreachable = cycle;
        unreachable.push(Construction {
            identity: 3,
            kind: BuildKind::Test,
            owner: 1,
            site,
            operands: Vec::new(),
        });
        assert!(matches!(
            finish(completed_run(Some((BuildKind::Image, 1)), unreachable)).status,
            ImageEvaluationStatus::Invalid(GraphSealFailure::Creator("unreachable_construction"))
        ));
    }
}
